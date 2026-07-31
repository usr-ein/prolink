// SPDX-License-Identifier: GPL-3.0-only

//! Every packet in every capture, replayed through the codecs.
//!
//! The codecs in this crate were written from a specification that was itself
//! derived from these captures. Replaying the captures is what turns "I
//! implemented the document" into "I decode what the hardware actually sent",
//! and the two are not the same thing: the reference implementation this
//! project replaces had an encoder and a decoder that agreed perfectly on a
//! bug, because nothing but a capture can tell an internally consistent
//! mistake from a correct one.
//!
//! Four wire formats, four claims:
//!
//! - **UDP 50000** — every datagram decodes and *re-encodes byte for byte*,
//!   including the bytes whose meaning is unknown. Anything less and a virtual
//!   CDJ is merely plausible rather than indistinguishable.
//! - **UDP 50002** — every datagram decodes, and the fields cross-check
//!   against the rest of the packet and against the IP header that carried it.
//! - **TCP dbserver** — every reassembled stream frames *end to end*, with the
//!   consumed bytes accounting for the stream exactly. Nothing here has a
//!   length prefix, so this is the only check that can catch the
//!   desynchronisation the omitted-blob rule causes.
//! - **ONC RPC** — every call parses and its arguments are the shape its
//!   `(program, procedure)` implies, name encoding included.
//!
//! # Filter on the destination port
//!
//! The type byte at `0x0a` is shared across UDP ports and the layouts behind
//! it are not: `0x06` is a keep-alive on 50000 and a media response on 50002.
//! An "either endpoint" filter therefore feeds tool traffic into the wrong
//! decoder, which does not fail — it produces the right number of bytes and
//! plausible values. [`prolink_capture::Capture::udp_to`] is destination-only
//! for that reason and is the only filter used here. TCP is the opposite: a
//! connection has two ends and both directions belong to the server, so
//! streams are matched by content rather than by port.
//!
//! # Running it
//!
//! The corpus is ~272 MB and is committed, in `captures/`, so a plain
//! `cargo test` replays it. `PROLINK_CAPTURES` points somewhere else instead;
//! without either, the corpus tests skip — but skipping is not passing, so the
//! committed fixtures in
//! `testdata/corpus-fixtures.hex` run the same checks over real packets with
//! no corpus at all.
//!
//! `cargo test -p prolink-proto -- --nocapture` prints the per-capture and
//! total counts, which is how a coverage change is meant to be noticed.

// An integration test is its own crate, so the allowances in `src/lib.rs` do
// not reach here: an assertion *is* this file's failure mode, and a test that
// carefully propagated its errors would report them as passes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::as_conversions
)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use prolink_capture::{Capture, Corpus, Flow, Packet, tcp};
use prolink_proto::rpc::{self, Program};
use prolink_proto::{MAGIC, dbserver, djl, status};

/// Where discovery, numbering and keep-alive are broadcast.
const DISCOVERY_PORT: u16 = 50000;
/// Where beat packets go.
///
/// Counted here and decoded elsewhere: `beat.rs` replays this port against the
/// same corpus from its own unit tests, and duplicating that would be two
/// places to change rather than one.
const BEAT_PORT: u16 = 50001;
/// Where status, media queries and settings queries are unicast.
const STATUS_PORT: u16 = 50002;

/// The ports RFC 1057 and Pioneer between them make well known.
///
/// Necessary but nowhere near sufficient: a portmapper exists precisely so
/// that NFS and MOUNT need not be anywhere in particular, and in this corpus
/// they mostly are not. See [`every_rpc_call_in_the_corpus_parses`].
const WELL_KNOWN_RPC_PORTS: [u16; 3] = [
    rpc::portmap::PORT,
    rpc::nfs2::PORT,
    rpc::mount::PIONEER_PORT,
];

/// The five bytes every dbserver message starts with: a tagged `UInt32`.
const DBSERVER_MAGIC: [u8; 5] = [0x11, 0x87, 0x23, 0x49, 0xae];

// -- the committed fixture floor ------------------------------------------

/// Real packets, extracted from the corpus and committed.
///
/// Read at compile time so a coverage regression cannot hide behind a missing
/// file any more than behind a missing corpus.
const FIXTURES: &str = include_str!("../../../testdata/corpus-fixtures.hex");

/// Every fixture, in file order, as `(label, bytes)`.
fn fixtures() -> Vec<(String, Vec<u8>)> {
    let mut records: Vec<(String, String)> = Vec::new();
    for line in FIXTURES.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some((label, head)) = line.split_once(" = ") {
            records.push((label.trim().to_owned(), head.trim().to_owned()));
        } else if let Some((_, tail)) = records.last_mut() {
            tail.push_str(line.trim());
        } else {
            panic!("continuation line before any record: {line:?}");
        }
    }
    records
        .into_iter()
        .map(|(label, hex)| {
            assert!(
                hex.len() % 2 == 0,
                "{label} has an odd number of hex digits"
            );
            let bytes = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                        .unwrap_or_else(|_| panic!("{label} is not hex: {pair:?}"))
                })
                .collect();
            (label, bytes)
        })
        .collect()
}

/// The one fixture with this label.
fn fixture(label: &str) -> Vec<u8> {
    fixtures()
        .into_iter()
        .find(|(found, _)| found == label)
        .unwrap_or_else(|| panic!("no fixture {label:?} in testdata/corpus-fixtures.hex"))
        .1
}

/// Every fixture whose label starts with `prefix`.
fn fixtures_under(prefix: &str) -> Vec<(String, Vec<u8>)> {
    let found: Vec<(String, Vec<u8>)> = fixtures()
        .into_iter()
        .filter(|(label, _)| label.starts_with(prefix))
        .collect();
    assert!(!found.is_empty(), "no fixtures under {prefix:?}");
    found
}

// -- what "decodes correctly" means, in one place -------------------------
//
// The corpus scan and the fixture floor run the *same* checks, so the floor is
// a floor and not a second, weaker opinion.

/// Everything wrong with one UDP-50000 datagram, or nothing.
///
/// `source` is the IP header's source address where one is known; a body that
/// carries an address must agree with it, which is a cross-check the packet
/// alone cannot provide.
fn discovery_problems(raw: &[u8], source: Option<Ipv4Addr>) -> Vec<String> {
    let mut problems = Vec::new();
    let packet = match djl::Packet::decode(raw) {
        Ok(packet) => packet,
        Err(error) => {
            problems.push(format!("{error} ({} bytes)", raw.len()));
            return problems;
        }
    };
    let kind = packet.kind();
    if packet.encode() != raw {
        problems.push(format!("{kind:?} does not re-encode byte for byte"));
    }
    // C2: `stype` equals the total datagram length on every kind observed.
    if usize::from(packet.stype) != raw.len() {
        problems.push(format!(
            "{kind:?}: stype {:#04x} is not the datagram length {}",
            packet.stype,
            raw.len()
        ));
    }
    if let Some(expected) = kind.wire_length() {
        if usize::from(expected) != raw.len() {
            problems.push(format!(
                "{kind:?}: {} bytes on the wire, wire_length says {expected:#04x}",
                raw.len()
            ));
        }
    }
    if packet.subtype != 0x00 {
        problems.push(format!("{kind:?}: subtype {:#04x}", packet.subtype));
    }
    if packet.const_one != 0x01 {
        problems.push(format!("{kind:?}: byte 0x20 is {:#04x}", packet.const_one));
    }
    if packet.pad_22 != 0x00 {
        problems.push(format!("{kind:?}: byte 0x22 is {:#04x}", packet.pad_22));
    }
    if !packet.trailing.is_empty() {
        problems.push(format!(
            "{kind:?}: {} bytes past the fields this kind declares",
            packet.trailing.len()
        ));
    }
    if let (Some(claimed), Some(actual)) = (packet.body.ip(), source) {
        if claimed != actual {
            problems.push(format!("{kind:?}: body says {claimed}, sent from {actual}"));
        }
    }
    problems
}

/// Everything wrong with one UDP-50002 datagram, or nothing.
fn status_problems(raw: &[u8], source: Option<Ipv4Addr>) -> Vec<String> {
    let mut problems = Vec::new();
    let packet = match status::decode(raw) {
        Ok(packet) => packet,
        Err(error) => {
            problems.push(format!("{error} ({} bytes)", raw.len()));
            return problems;
        }
    };
    let kind = packet.kind();
    if let status::Packet::Other { .. } = packet {
        problems.push(format!(
            "{kind:?}: no layout for this kind, or too short for the one it has ({} bytes)",
            raw.len()
        ));
        return problems;
    }
    // C14: byte 0x1f is structural on this port, where the discovery header
    // has the last byte of its device name.
    if raw.get(0x1f) != Some(&0x01) {
        problems.push(format!("{kind:?}: byte 0x1f is {:?}", raw.get(0x1f)));
    }
    let declared = raw
        .get(0x22..0x24)
        .map(|field| usize::from(u16::from_be_bytes([field[0], field[1]])));
    if declared != Some(raw.len().saturating_sub(0x24)) {
        problems.push(format!(
            "{kind:?}: declares {declared:?} body bytes, carries {}",
            raw.len().saturating_sub(0x24)
        ));
    }
    match &packet {
        status::Packet::CdjStatus(cdj) => {
            // The number appears at 0x21 and again at 0x24; a virtual CDJ that
            // set only one of them would be visibly not a CDJ.
            if raw.get(0x21) != raw.get(0x24) {
                problems.push(format!(
                    "cdj_status: device {:?} at 0x21 but {:?} at 0x24",
                    raw.get(0x21),
                    raw.get(0x24)
                ));
            }
            if cdj.sender().is_none() {
                problems.push("cdj_status: sent by device 0".to_owned());
            }
        }
        status::Packet::MediaQuery(query) => {
            // The requester names itself by address as well as by number, and
            // the two must be the address the datagram came from.
            if let Some(actual) = source {
                if query.requester_ip != actual {
                    problems.push(format!(
                        "media_query: names {} but was sent from {actual}",
                        query.requester_ip
                    ));
                }
            }
            if Some(query.requester.get()) != raw.get(0x21).copied() {
                problems.push("media_query: requester disagrees with byte 0x21".to_owned());
            }
        }
        status::Packet::MediaResponse(response) => {
            if response.device() != response.sender() {
                problems.push(format!(
                    "media_response: describes device {:?} but was sent by {:?}",
                    response.device(),
                    response.sender()
                ));
            }
        }
        status::Packet::SettingsQuery(_)
        | status::Packet::SettingsResponse(_)
        | status::Packet::Other { .. } => {}
    }
    problems
}

/// What framing one dbserver byte stream found.
#[derive(Default)]
struct Framing {
    messages: usize,
    kinds: Counts,
    problems: Vec<String>,
}

/// Frame a whole dbserver stream, both the messages and the bookkeeping.
///
/// The bookkeeping is the point. A dbserver message carries no length prefix,
/// so the parser's final position *is* the frame boundary, and a reader that
/// mishandles the omitted-blob rule consumes the next message's magic as a
/// field and stays one position out for the rest of the connection with
/// nothing to show for it. Only "the consumed bytes account for the stream
/// exactly" catches that.
fn frame_dbserver_stream(bytes: &[u8]) -> Framing {
    let mut framing = Framing::default();
    let body = dbserver::skip_preamble(bytes);
    let mut offset = bytes.len() - body.len();
    while offset < bytes.len() {
        match dbserver::Message::decode(&bytes[offset..]) {
            Ok((message, consumed)) => {
                let kind = message.kind;
                framing.kinds.bump(name_or_debug(kind.name(), kind));
                if message.encode() != bytes[offset..offset + consumed] {
                    framing.problems.push(format!(
                        "message {} at offset {offset} ({kind:?}) does not re-encode byte for byte",
                        framing.messages
                    ));
                }
                offset += consumed;
                framing.messages += 1;
            }
            Err(error) => {
                framing.problems.push(format!(
                    "framing stopped after {} messages, {offset} of {} bytes consumed: {error}",
                    framing.messages,
                    bytes.len()
                ));
                break;
            }
        }
    }
    framing
}

/// What one RPC call turned out to be.
struct RpcCall {
    /// `program procedure`, for the coverage histogram.
    label: String,
    /// The port it was addressed to.
    port: u16,
    /// The name a `LOOKUP` asked for, with the byte count the wire declared.
    lookup: Option<(String, usize)>,
    /// Whether the argument block differed from a re-encode only in the XDR
    /// pad, which a CDJ does not clear.
    stale_pad: bool,
    /// Whether the stamp matched the position-indexed table for its xid.
    stamp_agreed: bool,
    problems: Vec<String>,
}

/// Decode one datagram as an RPC call, or answer `None` if it is not one.
///
/// Not one means: not an RPC v2 message at all, or a reply. Both are ordinary
/// on these ports and neither is this function's business.
fn rpc_call_problems(raw: &[u8], port: u16) -> Option<RpcCall> {
    let call = match rpc::Message::parse(raw) {
        Ok(rpc::Message::Call(call)) => call,
        Ok(rpc::Message::Reply(_)) | Err(_) => return None,
    };
    let mut found = RpcCall {
        label: String::new(),
        port,
        lookup: None,
        stale_pad: false,
        stamp_agreed: false,
        problems: Vec::new(),
    };
    credential_problems(&call, &mut found);
    let encoded = argument_problems(&call, &mut found);
    if let Some(encoded) = encoded {
        pad_problems(call.arguments, &encoded, &mut found);
    }
    Some(found)
}

/// The credential, which the module claims is the same on every call ever
/// seen: `AUTH_UNIX`, uid 0, gid 0, no machine name, no supplementary gids.
fn credential_problems(call: &rpc::Call<'_>, found: &mut RpcCall) {
    if call.credential.flavor != rpc::AuthFlavor::UNIX {
        found
            .problems
            .push(format!("credential flavor {:?}", call.credential.flavor));
        return;
    }
    let auth = match rpc::AuthUnix::parse(call.credential.body) {
        Ok(auth) => auth,
        Err(error) => {
            found.problems.push(format!("credential: {error}"));
            return;
        }
    };
    if auth.uid != 0 || auth.gid != 0 || !auth.machine_name.is_empty() {
        found.problems.push(format!(
            "credential is uid {} gid {} machine {:?}",
            auth.uid, auth.gid, auth.machine_name
        ));
    }
    if !auth.gids.is_empty() {
        found
            .problems
            .push(format!("credential carries {} gids", auth.gids.len()));
    }
    // The stamp is a fixed sequence indexed by how many calls the device has
    // made since power-on, not a nonce and not a magic constant. Where the
    // table knows the xid it must agree.
    match rpc::stamp_for_xid(call.xid) {
        Some(expected) if expected == auth.stamp => found.stamp_agreed = true,
        Some(expected) => found.problems.push(format!(
            "{:?} carries stamp {:#010x}, the sequence says {expected:#010x}",
            call.xid, auth.stamp
        )),
        None => {}
    }
}

/// The argument block must be the shape its `(program, procedure)` implies.
///
/// Answers the bytes re-encoding that shape produces, for [`pad_problems`] to
/// compare against the wire.
fn argument_problems(call: &rpc::Call<'_>, found: &mut RpcCall) -> Option<Vec<u8>> {
    let arguments = call.arguments;
    let (label, encoded) = match call.program {
        Program::PORTMAP => {
            let procedure = rpc::portmap::Proc(call.procedure);
            let label = format!("portmap {}", name_or_debug(procedure.name(), procedure));
            match rpc::portmap::Request::parse(procedure, arguments) {
                Ok(request) => {
                    if matches!(request, rpc::portmap::Request::Unknown { .. }) {
                        found.problems.push(format!("{label}: no shape for it"));
                    }
                    (label, Some(request.encode_arguments()))
                }
                Err(error) => {
                    found.problems.push(format!("{label}: {error}"));
                    (label, None)
                }
            }
        }
        Program::MOUNT => {
            let procedure = rpc::mount::Proc(call.procedure);
            let label = format!("mount {}", name_or_debug(procedure.name(), procedure));
            match rpc::mount::Request::parse(procedure, arguments) {
                Ok(request) => {
                    if matches!(request, rpc::mount::Request::Unknown { .. }) {
                        found.problems.push(format!("{label}: no shape for it"));
                    }
                    if let Some(path) = request.path() {
                        found.problems.extend(utf16le_problems(&label, path));
                    }
                    (label, Some(request.encode_arguments()))
                }
                Err(error) => {
                    found.problems.push(format!("{label}: {error}"));
                    (label, None)
                }
            }
        }
        Program::NFS => {
            let procedure = rpc::nfs2::Proc(call.procedure);
            let label = format!("nfs {}", name_or_debug(procedure.name(), procedure));
            match rpc::nfs2::Request::parse(procedure, arguments) {
                Ok(request) => {
                    if matches!(request, rpc::nfs2::Request::Unknown { .. }) {
                        found.problems.push(format!("{label}: no shape for it"));
                    }
                    if let rpc::nfs2::Request::Lookup { name, .. } = &request {
                        found.problems.extend(utf16le_problems(&label, name));
                        found.lookup = Some((name.to_string_lossy(), name.len_bytes()));
                    }
                    (label, Some(request.encode_arguments()))
                }
                Err(error) => {
                    found.problems.push(format!("{label}: {error}"));
                    (label, None)
                }
            }
        }
        other => {
            // Not an error in the codec: a `DUMP` may list anything. It is a
            // finding about the corpus, reported rather than filtered away.
            let label = format!("unmodelled program {other:?}");
            found.problems.push(label.clone());
            (label, None)
        }
    };
    found.label = label;
    encoded
}

/// Compare an argument block against a re-encode of what it parsed to.
///
/// XDR pads a variable-length field out to four bytes and RFC 4506 asks for
/// zeros there. A CDJ sends whatever was in the buffer, so the pad is the one
/// place an argument block may legitimately differ from what we would write.
/// Everything before it may not — a length prefix read in the wrong units, or
/// a field read in the wrong order, moves bytes that are not the pad.
fn pad_problems(arguments: &[u8], encoded: &[u8], found: &mut RpcCall) {
    if encoded.len() == arguments.len() {
        let differing: Vec<usize> = arguments
            .iter()
            .zip(encoded.iter())
            .enumerate()
            .filter(|(_, (theirs, ours))| theirs != ours)
            .map(|(index, _)| index)
            .collect();
        let only_pad = differing
            .iter()
            .all(|&index| index + 4 > arguments.len() && encoded[index] == 0);
        if !differing.is_empty() && !only_pad {
            found.problems.push(format!(
                "{}: arguments differ from a re-encode at {differing:?}",
                found.label
            ));
        }
        found.stale_pad = !differing.is_empty() && only_pad;
    } else {
        found.problems.push(format!(
            "{}: arguments are {} bytes, re-encode to {}",
            found.label,
            arguments.len(),
            encoded.len()
        ));
    }
}

/// Everything wrong with a Pioneer UTF-16LE name, or nothing.
///
/// Three separate things get this wrong, and each is the difference between a
/// track that loads and `NFSERR_NOENT`: the length prefix counts **bytes**
/// where dbserver's counts characters, the units are **little**-endian where
/// dbserver's are big, and the bytes are the field while the string is only a
/// reading of them.
fn utf16le_problems(label: &str, name: &rpc::xdr::Utf16LeString) -> Vec<String> {
    let mut problems = Vec::new();
    let bytes = name.as_bytes();
    if bytes.len() % 2 != 0 {
        problems.push(format!(
            "{label}: {} bytes of UTF-16, which is an odd number",
            bytes.len()
        ));
    }
    let text = name.to_string_lossy();
    if rpc::xdr::Utf16LeString::new(&text).as_bytes() != bytes {
        problems.push(format!("{label}: {text:?} does not re-encode to its bytes"));
    }
    // The prefix counts bytes, so it is twice the UTF-16 unit count and not
    // the unit count itself.
    if name.len_bytes() != 2 * text.encode_utf16().count() {
        problems.push(format!(
            "{label}: {} bytes for {} UTF-16 units",
            name.len_bytes(),
            text.encode_utf16().count()
        ));
    }
    // Little-endian, so an ASCII name puts the character first and the zero
    // second. Big-endian would be the other way round and would still be a
    // valid parse of an even number of bytes, which is why this is checked
    // against the corpus rather than assumed.
    if text.is_ascii() && bytes.len() >= 2 && (bytes[0] == 0 || bytes[1] != 0) {
        problems.push(format!(
            "{label}: {text:?} begins {:02x?}, which is not UTF-16 little-endian",
            &bytes[..2]
        ));
    }
    problems
}

// -- counting -------------------------------------------------------------

/// A histogram with a stable order, so two runs are diffable.
#[derive(Default, Debug)]
struct Counts(BTreeMap<String, usize>);

impl Counts {
    fn bump(&mut self, key: impl Into<String>) {
        self.add(key, 1);
    }

    fn add(&mut self, key: impl Into<String>, by: usize) {
        *self.0.entry(key.into()).or_insert(0) += by;
    }

    fn get(&self, key: &str) -> usize {
        self.0.get(key).copied().unwrap_or(0)
    }

    fn total(&self) -> usize {
        self.0.values().sum()
    }

    fn merge(&mut self, other: &Self) {
        for (key, count) in &other.0 {
            self.add(key.clone(), *count);
        }
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(String::as_str).collect()
    }
}

/// A newtype's own name where it has one, its `Debug` where it does not.
fn name_or_debug(name: Option<&'static str>, value: impl std::fmt::Debug) -> String {
    name.map_or_else(|| format!("{value:?}"), str::to_owned)
}

// -- the scan -------------------------------------------------------------

/// Everything one pass over the corpus found.
///
/// One pass, not four: the corpus is 240 MB and reading it once per format
/// would be four times the I/O for the same answers. The tests below each
/// assert over their own slice of this.
#[derive(Default)]
struct Scan {
    /// Per capture directory, in name order.
    per_capture: BTreeMap<String, Tally>,
    /// A file that could not be opened, or that stopped part-way through.
    capture_problems: Vec<String>,

    djl_kinds: Counts,
    djl_problems: Vec<String>,

    status_kinds: Counts,
    status_problems: Vec<String>,

    beat_datagrams: usize,
    magic_ports: Counts,

    tcp_streams: usize,
    tcp_problems: Vec<String>,
    dbserver_bytes: u64,
    dbserver_kinds: Counts,
    dbserver_problems: Vec<String>,
    /// Server-side port of each dbserver conversation, by stream.
    dbserver_server_ports: Counts,
    /// Streams whose SYN was not captured, so offset zero is the earliest byte
    /// seen and not the connection's first.
    dbserver_mid_connection: usize,
    /// Ports the TCP-12523 query advertised.
    advertised_ports: Counts,
    port_queries: usize,

    rpc_procedures: Counts,
    rpc_ports: Counts,
    rpc_replies: usize,
    rpc_problems: Vec<String>,
    rpc_stale_pads: usize,
    rpc_stamp_agreements: usize,
    lookup_names: Counts,
    /// Names carrying a character outside ASCII, which is where an encoder and
    /// a decoder that agree with each other stop agreeing with a CDJ (O6).
    lookup_non_ascii: usize,
    /// The longest name looked up, in bytes.
    lookup_longest: usize,
}

/// The headline counts for one capture file.
#[derive(Default, Clone, Copy)]
struct Tally {
    djl: usize,
    status: usize,
    dbserver_streams: usize,
    dbserver_messages: usize,
    /// Calls addressed to 111, 2049 or 48276.
    rpc_well_known: usize,
    /// Calls anywhere.
    rpc_calls: usize,
}

impl Scan {
    fn merge(&mut self, other: Self) {
        for (name, tally) in other.per_capture {
            let slot = self.per_capture.entry(name).or_default();
            slot.djl += tally.djl;
            slot.status += tally.status;
            slot.dbserver_streams += tally.dbserver_streams;
            slot.dbserver_messages += tally.dbserver_messages;
            slot.rpc_well_known += tally.rpc_well_known;
            slot.rpc_calls += tally.rpc_calls;
        }
        self.capture_problems.extend(other.capture_problems);
        self.djl_kinds.merge(&other.djl_kinds);
        self.djl_problems.extend(other.djl_problems);
        self.status_kinds.merge(&other.status_kinds);
        self.status_problems.extend(other.status_problems);
        self.beat_datagrams += other.beat_datagrams;
        self.magic_ports.merge(&other.magic_ports);
        self.tcp_streams += other.tcp_streams;
        self.tcp_problems.extend(other.tcp_problems);
        self.dbserver_bytes += other.dbserver_bytes;
        self.dbserver_kinds.merge(&other.dbserver_kinds);
        self.dbserver_problems.extend(other.dbserver_problems);
        self.dbserver_server_ports
            .merge(&other.dbserver_server_ports);
        self.dbserver_mid_connection += other.dbserver_mid_connection;
        self.advertised_ports.merge(&other.advertised_ports);
        self.port_queries += other.port_queries;
        self.rpc_procedures.merge(&other.rpc_procedures);
        self.rpc_ports.merge(&other.rpc_ports);
        self.rpc_replies += other.rpc_replies;
        self.rpc_problems.extend(other.rpc_problems);
        self.rpc_stale_pads += other.rpc_stale_pads;
        self.rpc_stamp_agreements += other.rpc_stamp_agreements;
        self.lookup_names.merge(&other.lookup_names);
        self.lookup_non_ascii += other.lookup_non_ascii;
        self.lookup_longest = self.lookup_longest.max(other.lookup_longest);
    }

    fn dbserver_messages(&self) -> usize {
        self.dbserver_kinds.total()
    }

    fn rpc_calls(&self) -> usize {
        self.rpc_procedures.total()
    }

    /// Calls addressed to a well-known port, which is the narrow reading of
    /// "the RPC traffic" and a small fraction of it.
    fn rpc_calls_well_known(&self) -> usize {
        WELL_KNOWN_RPC_PORTS
            .iter()
            .map(|port| self.rpc_ports.get(&port.to_string()))
            .sum()
    }
}

/// Read one capture and replay everything in it.
fn scan_capture(path: &Path) -> Scan {
    let name = path
        .parent()
        .and_then(Path::file_name)
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("?")
        .to_owned();
    let mut scan = Scan::default();
    let mut tally = Tally::default();

    let capture = match Capture::open(path) {
        Ok(capture) => capture,
        Err(error) => {
            scan.capture_problems
                .push(format!("{name}: cannot open: {error}"));
            scan.per_capture.insert(name, tally);
            return scan;
        }
    };

    let mut reassembler = tcp::Reassembler::new();
    for packet in capture {
        let packet = match packet {
            Ok(packet) => packet,
            Err(error) => {
                // A `tcpdump` that was killed mid-record costs the tail of one
                // file and everything before it still stands. Reported all the
                // same: a file that stops early is a file whose counts are not
                // the counts of the session it recorded.
                scan.capture_problems.push(format!(
                    "{name}: stopped reading after {} datagrams on 50000, \
                     {} on 50002: {error}",
                    tally.djl, tally.status
                ));
                break;
            }
        };
        if packet.transport.is_tcp() {
            reassembler.push(&packet);
            continue;
        }
        scan_datagram(&mut scan, &mut tally, &name, &packet);
    }

    scan_streams(&mut scan, &mut tally, &name, &reassembler.finish());
    scan.per_capture.insert(name, tally);
    scan
}

/// Everything one UDP datagram contributes.
fn scan_datagram(scan: &mut Scan, tally: &mut Tally, name: &str, packet: &Packet) {
    let port = packet.destination.port();
    let raw = packet.payload.as_slice();
    let source = Some(*packet.source.ip());

    if raw.starts_with(&MAGIC) {
        scan.magic_ports.bump(port.to_string());
    }

    if port == DISCOVERY_PORT {
        tally.djl += 1;
        match djl::Packet::decode(raw) {
            Ok(decoded) => {
                let kind = decoded.kind();
                scan.djl_kinds.bump(name_or_debug(kind.name(), kind));
            }
            Err(_) => scan.djl_kinds.bump("undecodable"),
        }
        for problem in discovery_problems(raw, source) {
            scan.djl_problems
                .push(format!("{name}#{}: {problem}", packet.index));
        }
    } else if port == STATUS_PORT {
        tally.status += 1;
        match status::decode(raw) {
            Ok(decoded) => {
                let kind = decoded.kind();
                scan.status_kinds.bump(name_or_debug(kind.name(), kind));
            }
            Err(_) => scan.status_kinds.bump("undecodable"),
        }
        for problem in status_problems(raw, source) {
            scan.status_problems
                .push(format!("{name}#{}: {problem}", packet.index));
        }
    } else if port == BEAT_PORT {
        scan.beat_datagrams += 1;
    }

    // RPC is looked for on every port, not only the well-known three. A
    // portmapper exists precisely so that NFS need not be at 2049, and in this
    // corpus it usually is not.
    let Some(found) = rpc_call_problems(raw, port) else {
        if raw.len() >= 8 && matches!(rpc::Message::parse(raw), Ok(rpc::Message::Reply(_))) {
            scan.rpc_replies += 1;
        }
        return;
    };
    tally.rpc_calls += 1;
    if WELL_KNOWN_RPC_PORTS.contains(&port) {
        tally.rpc_well_known += 1;
    }
    scan.rpc_procedures.bump(found.label);
    scan.rpc_ports.bump(found.port.to_string());
    scan.rpc_stale_pads += usize::from(found.stale_pad);
    scan.rpc_stamp_agreements += usize::from(found.stamp_agreed);
    if let Some((text, bytes)) = found.lookup {
        scan.lookup_non_ascii += usize::from(!text.is_ascii());
        scan.lookup_longest = scan.lookup_longest.max(bytes);
        scan.lookup_names.bump(format!("{text} ({bytes} bytes)"));
    }
    for problem in found.problems {
        scan.rpc_problems
            .push(format!("{name}#{}: {problem}", packet.index));
    }
}

/// Everything the reassembled TCP streams of one capture contribute.
fn scan_streams(scan: &mut Scan, tally: &mut Tally, name: &str, streams: &[tcp::Stream]) {
    // Whichever direction of a connection was seen first is the client's, so
    // its destination is the server's port. Corroborated below against what
    // the TCP-12523 query advertised, which is arrived at independently.
    let mut seen: Vec<Flow> = Vec::new();
    for (index, stream) in streams.iter().enumerate() {
        scan.tcp_streams += 1;
        let flow = stream.flow();
        let answering = seen.contains(&flow.reversed());
        seen.push(flow);

        let Some(bytes) = stream.contiguous() else {
            // A hole in a protocol with no framing is not something to
            // concatenate over; it is something to report.
            scan.tcp_problems.push(format!(
                "{name}: {flow:?} has {} holes in {} bytes, so it cannot be framed",
                stream.gaps().len(),
                stream.len()
            ));
            continue;
        };

        if flow.involves(dbserver::PORT_QUERY_PORT) {
            scan_port_query(scan, name, flow, answering, bytes);
            continue;
        }

        if !dbserver::skip_preamble(bytes).starts_with(&DBSERVER_MAGIC) {
            if !bytes.is_empty() {
                scan.tcp_problems.push(format!(
                    "{name}: {flow:?} carries {} bytes that are neither a port query nor dbserver",
                    bytes.len()
                ));
            }
            continue;
        }

        tally.dbserver_streams += 1;
        scan.dbserver_bytes += bytes.len() as u64;
        if !answering {
            scan.dbserver_server_ports
                .bump(flow.destination.port().to_string());
        }
        if !stream.from_connection_start() {
            // Not a fault: `tcpdump` started after the connection did. Worth
            // knowing, because offset zero is then the earliest byte seen and
            // not the connection's first — so framing it end to end proves
            // only that the bytes captured are whole messages.
            scan.dbserver_mid_connection += 1;
        }
        let framing = frame_dbserver_stream(bytes);
        tally.dbserver_messages += framing.messages;
        scan.dbserver_kinds.merge(&framing.kinds);
        for problem in framing.problems {
            scan.dbserver_problems
                .push(format!("{name}: stream {index} {flow:?}: {problem}"));
        }
    }
}

/// The fixed 19-byte question and the two-byte answer on TCP 12523.
fn scan_port_query(scan: &mut Scan, name: &str, flow: Flow, answering: bool, bytes: &[u8]) {
    if answering {
        match dbserver::decode_port_reply(bytes) {
            Ok(port) if bytes.len() == 2 => scan.advertised_ports.bump(port.to_string()),
            Ok(_) => scan.tcp_problems.push(format!(
                "{name}: {flow:?} answers the port query with {} bytes, not 2",
                bytes.len()
            )),
            Err(error) => scan
                .tcp_problems
                .push(format!("{name}: {flow:?} port reply: {error}")),
        }
        return;
    }
    if bytes == dbserver::PORT_QUERY {
        scan.port_queries += 1;
    } else {
        scan.tcp_problems.push(format!(
            "{name}: {flow:?} asks the port query differently: {} bytes",
            bytes.len()
        ));
    }
}

/// The whole corpus, scanned once, or `None` on a machine with no captures.
fn corpus() -> Option<&'static Scan> {
    static SCANNED: OnceLock<Option<Scan>> = OnceLock::new();
    SCANNED
        .get_or_init(|| {
            let corpus = Corpus::locate()?;
            let captures = corpus.captures();
            if captures.is_empty() {
                return None;
            }
            let next = AtomicUsize::new(0);
            let total = Mutex::new(Scan::default());
            // 240 MB of pcap, so the files are read in parallel: one thread per
            // core, each taking the next unclaimed file. Reading them one at a
            // time is about four times slower and buys nothing.
            let threads = std::thread::available_parallelism()
                .map_or(4, std::num::NonZero::get)
                .min(captures.len());
            std::thread::scope(|scope| {
                for _ in 0..threads {
                    scope.spawn(|| {
                        loop {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            let Some(path) = captures.get(index) else {
                                break;
                            };
                            let scan = scan_capture(path);
                            if let Ok(mut total) = total.lock() {
                                total.merge(scan);
                            }
                        }
                    });
                }
            });
            total.into_inner().ok()
        })
        .as_ref()
}

/// Say why a test did nothing, so a skip is never mistaken for a pass.
fn no_corpus(what: &str) {
    println!(
        "skipping {what}: no capture corpus. Set {} to a directory of pcap or \
         pcapng files; the committed fixtures ran regardless.",
        prolink_capture::CORPUS_ENV
    );
}

/// The first few of a list of problems, for an assertion message.
fn first_few(problems: &[String]) -> String {
    let mut out = String::new();
    for problem in problems.iter().take(8) {
        let _ = writeln!(out, "\n    {problem}");
    }
    if problems.len() > 8 {
        let _ = write!(out, "\n    … and {} more", problems.len() - 8);
    }
    out
}

// -- the fixture floor ----------------------------------------------------

#[test]
fn every_committed_discovery_packet_decodes_and_re_encodes_byte_for_byte() {
    let mut kinds = Vec::new();
    for (label, raw) in fixtures_under("djl/") {
        let problems = discovery_problems(&raw, None);
        assert!(problems.is_empty(), "{label}: {}", first_few(&problems));
        let packet = djl::Packet::decode(&raw).unwrap();
        kinds.push(packet.kind());
        assert_eq!(packet.encode(), raw, "{label} must round-trip");
    }
    // Six of the nine kinds `PacketKind` names. The other three are absent
    // from the corpus, not from this file, and the reason differs: the two
    // mixer kinds need a mixer, and `number_conflict` has never been seen at
    // all. `no_capture_has_ever_contested_a_device_number` is the tripwire.
    for expected in [
        djl::PacketKind::HELLO,
        djl::PacketKind::CLAIM_MAC,
        djl::PacketKind::CLAIM_IP,
        djl::PacketKind::CLAIM_NUMBER,
        djl::PacketKind::NUMBER_IN_USE,
        djl::PacketKind::KEEP_ALIVE,
    ] {
        assert!(kinds.contains(&expected), "no committed {expected:?}");
    }
}

#[test]
fn the_committed_keep_alive_is_the_one_a_nexus_sends() {
    let raw = fixture("djl/keep_alive");
    let packet = djl::Packet::decode(&raw).unwrap();
    assert_eq!(packet.name.as_str(), "CDJ-2000nexus");
    assert_eq!(packet.device_kind, prolink_proto::DeviceKind::CDJ);
    let djl::Body::KeepAlive {
        device_number,
        ip,
        peer_count,
        trailing,
        ..
    } = packet.body
    else {
        panic!("expected a keep-alive body");
    };
    assert_eq!(device_number, 1);
    assert_eq!(ip, Ipv4Addr::new(169, 254, 103, 172));
    assert_eq!(peer_count, 1, "alone on the network");
    // C3: 0x00 on nexus hardware, where the pre-hardware literature says 0x01.
    assert_eq!(trailing, 0x00);
}

#[test]
fn every_committed_status_packet_decodes_as_the_kind_it_claims() {
    let mut kinds = Vec::new();
    for (label, raw) in fixtures_under("status/") {
        let problems = status_problems(&raw, None);
        assert!(problems.is_empty(), "{label}: {}", first_few(&problems));
        let packet = status::decode(&raw).unwrap();
        assert_eq!(
            Some(packet.kind().0),
            raw.get(MAGIC.len()).copied(),
            "{label}: decoded as a kind the bytes do not carry"
        );
        kinds.push(packet.kind());
    }
    for expected in [
        status::StatusKind::CDJ_STATUS,
        status::StatusKind::MEDIA_QUERY,
        status::StatusKind::MEDIA_RESPONSE,
        status::StatusKind::SETTINGS_QUERY,
        status::StatusKind::SETTINGS_RESPONSE,
    ] {
        assert!(kinds.contains(&expected), "no committed {expected:?}");
    }
}

#[test]
fn the_committed_media_response_carries_the_counts_a_deck_needs() {
    let raw = fixture("status/media_response");
    let status::Packet::MediaResponse(response) = status::decode(&raw).unwrap() else {
        panic!("expected a media response");
    };
    assert_eq!(response.volume_name(), "SAM2");
    assert_eq!(response.created(), "2025-06-24");
    assert_eq!(response.slot(), prolink_proto::Slot::USB);
    // A deck told a medium holds nothing has no reason to offer it (F24), so
    // the true counts are load-bearing rather than cosmetic.
    assert_eq!(response.track_count(), 692);
    assert_eq!(response.playlist_count(), 35);
}

#[test]
fn a_committed_dbserver_exchange_frames_end_to_end() {
    for label in ["dbserver/request", "dbserver/reply"] {
        let bytes = fixture(label);
        assert!(
            bytes.starts_with(&dbserver::PREAMBLE),
            "{label} must begin with the five-byte preamble both peers send"
        );
        let framing = frame_dbserver_stream(&bytes);
        assert!(
            framing.problems.is_empty(),
            "{label}: {}",
            first_few(&framing.problems)
        );
        assert!(framing.messages >= 2, "{label} carries only one message");
    }
    let request = fixture("dbserver/request");
    let (introduce, _) = dbserver::Message::decode(dbserver::skip_preamble(&request)).unwrap();
    assert_eq!(introduce.kind, dbserver::MessageKind::INTRODUCE);
    assert_eq!(introduce.transaction_id, dbserver::SETUP_TRANSACTION_ID);
}

#[test]
fn a_zero_length_blob_is_absent_from_the_wire_and_stays_absent() {
    // The rule that desynchronises a naive reader, pinned against a message a
    // real player sent: argument 1 is a `UInt32` zero and argument 2 is the
    // blob it declares, which is simply not there.
    let raw = fixture("dbserver/omitted_blob");
    let (message, consumed) = dbserver::Message::decode(&raw).unwrap();
    assert_eq!(
        consumed,
        raw.len(),
        "the message must account for the whole fixture"
    );
    assert_eq!(message.number(1), Some(0), "the declared length is zero");
    assert_eq!(
        message.blob(2),
        Some(&[][..]),
        "and the blob it declares is empty"
    );
    assert_eq!(
        message.encode(),
        raw,
        "encoding must omit exactly what decoding inferred was absent"
    );
    // Had the blob been sent as an empty field it would have cost five bytes.
    assert!(
        raw.len() < 5 + 5 + 3 + 2 + 5 + 12 + 5 * 3,
        "an omitted blob is shorter than a sent one"
    );
}

#[test]
fn the_checks_notice_a_packet_that_has_been_tampered_with() {
    // Every assertion above is of the form "collect the problems, assert the
    // list is empty", which passes just as happily when the checker is broken
    // as when the packets are good. So: break each kind of packet in the one
    // way its check exists to catch, and require the list to fill up.
    let mut keep_alive = fixture("djl/keep_alive");
    keep_alive[0x23] = 0x37; // stype, which must equal the datagram length (C2)
    assert!(
        !discovery_problems(&keep_alive, None).is_empty(),
        "a stype that disagrees with the length must be caught"
    );
    let real = fixture("djl/claim_ip");
    assert!(
        !discovery_problems(&real, Some(Ipv4Addr::LOCALHOST)).is_empty(),
        "a claim_ip whose body disagrees with the IP header must be caught"
    );

    let mut status_packet = fixture("status/cdj_status");
    status_packet[0x1f] = 0x00; // the structural byte the discovery header lacks
    assert!(
        !status_problems(&status_packet, None).is_empty(),
        "a status packet without its structural 0x01 must be caught"
    );

    // Send the zero-length blob the wire omits, as an empty blob field: five
    // bytes the framer will not consume, which is precisely the offset a
    // desynchronised reader picks up. "The consumed bytes account for the
    // stream exactly" is the only check that sees it.
    let mut sent_anyway = dbserver::PREAMBLE.to_vec();
    sent_anyway.extend_from_slice(&fixture("dbserver/omitted_blob"));
    sent_anyway.extend_from_slice(&[0x14, 0x00, 0x00, 0x00, 0x00]);
    let framing = frame_dbserver_stream(&sent_anyway);
    assert!(
        !framing.problems.is_empty(),
        "an argument that should have been omitted leaves five bytes over"
    );

    // And a stream cut short mid-message, which is what a hole would look like.
    let mut cut = fixture("dbserver/reply");
    cut.truncate(cut.len() - 1);
    assert!(
        !frame_dbserver_stream(&cut).problems.is_empty(),
        "a stream ending inside a message must not frame cleanly"
    );

    // A LOOKUP name counted in characters rather than bytes, which is the
    // dbserver convention and the mistake that makes a track not load. The
    // prefix is the last word before the 14 name bytes and their two of pad.
    let mut lookup = fixture("rpc/nfs_lookup");
    let prefix = lookup.len() - 16 - 1;
    assert_eq!(lookup[prefix], 14, "the fixture's name length moved");
    lookup[prefix] = 7;
    let found = rpc_call_problems(&lookup, 2049).expect("still a call");
    assert!(
        !found.problems.is_empty(),
        "a name whose length prefix counts the wrong thing must be caught"
    );
}

#[test]
fn every_committed_rpc_call_parses_into_the_shape_its_procedure_implies() {
    let mut procedures = Vec::new();
    for (label, raw) in fixtures_under("rpc/") {
        let found = rpc_call_problems(&raw, 0).unwrap_or_else(|| panic!("{label} is not a call"));
        assert!(
            found.problems.is_empty(),
            "{label}: {}",
            first_few(&found.problems)
        );
        procedures.push(found.label);
    }
    for expected in [
        "portmap NULL",
        "portmap GETPORT",
        "portmap DUMP",
        "mount MNT",
        "mount UMNT",
        "mount EXPORT",
        "nfs GETATTR",
        "nfs LOOKUP",
        "nfs READ",
    ] {
        assert!(
            procedures.iter().any(|found| found == expected),
            "no committed {expected} call; have {procedures:?}"
        );
    }
}

#[test]
fn a_lookup_name_is_utf16_little_endian_counted_in_bytes() {
    let raw = fixture("rpc/nfs_lookup");
    let rpc::Message::Call(call) = rpc::Message::parse(&raw).unwrap() else {
        panic!("expected a call");
    };
    let rpc::nfs2::Request::Lookup { dir, name } =
        rpc::nfs2::Request::parse(rpc::nfs2::Proc::LOOKUP, call.arguments).unwrap()
    else {
        panic!("expected a LOOKUP");
    };
    assert_eq!(dir.as_bytes().len(), rpc::nfs2::FHANDLE_LEN);
    assert_eq!(name.to_string_lossy(), "PIONEER");
    // Seven characters, fourteen bytes. dbserver would have said 8, counting
    // characters including a terminating NUL, and would have sent them the
    // other way round.
    assert_eq!(name.len_bytes(), 14);
    assert_eq!(
        name.as_bytes(),
        b"P\0I\0O\0N\0E\0E\0R\0",
        "little-endian: the character first and the zero second"
    );
    assert_ne!(
        name.as_bytes(),
        b"\0P\0I\0O\0N\0E\0E\0R",
        "big-endian would be a valid parse of the same length and the wrong name"
    );
}

#[test]
fn a_deck_leaves_stale_bytes_in_the_xdr_pad() {
    // RFC 4506 asks for zeros in the bytes that round a variable-length field
    // up to four; a CDJ sends whatever the buffer held. Our encoder writes
    // zeros, which is correct and which means an argument block from hardware
    // does not always re-encode byte for byte. Everything before the pad does.
    let raw = fixture("rpc/nfs_lookup_stale_pad");
    let rpc::Message::Call(call) = rpc::Message::parse(&raw).unwrap() else {
        panic!("expected a call");
    };
    let request = rpc::nfs2::Request::parse(rpc::nfs2::Proc::LOOKUP, call.arguments).unwrap();
    let ours = request.encode_arguments();
    assert_eq!(ours.len(), call.arguments.len(), "same length");
    assert_ne!(ours, call.arguments, "this fixture exists for its pad");
    let differing: Vec<usize> = call
        .arguments
        .iter()
        .zip(ours.iter())
        .enumerate()
        .filter(|(_, (theirs, ours))| theirs != ours)
        .map(|(index, _)| index)
        .collect();
    assert!(
        differing
            .iter()
            .all(|&index| index + 4 > call.arguments.len() && ours[index] == 0),
        "differences outside the trailing pad would be a mis-parse: {differing:?}"
    );
    let rpc::nfs2::Request::Lookup { name, .. } = request else {
        panic!("expected a LOOKUP");
    };
    assert_eq!(name.to_string_lossy(), "6 SENSE");
}

// -- the corpus -----------------------------------------------------------

#[test]
fn every_capture_in_the_corpus_reads_from_end_to_end() {
    let Some(scan) = corpus() else {
        return no_corpus("the capture-integrity check");
    };
    // A `tcpdump` killed mid-record is a normal thing to find in a corpus and
    // an abnormal thing to ignore: everything after the cut is missing, so the
    // counts below would be the counts of a session that was not recorded.
    assert!(
        scan.capture_problems.is_empty(),
        "{} captures did not read to the end:{}",
        scan.capture_problems.len(),
        first_few(&scan.capture_problems)
    );
    assert!(
        scan.per_capture.len() >= 10,
        "only {} captures found under the corpus root",
        scan.per_capture.len()
    );
}

#[test]
fn every_discovery_datagram_in_the_corpus_decodes_and_re_encodes_byte_for_byte() {
    let Some(scan) = corpus() else {
        return no_corpus("the UDP-50000 replay");
    };
    assert!(
        scan.djl_problems.is_empty(),
        "{} of {} datagrams addressed to 50000 did not survive the round trip:{}",
        scan.djl_problems.len(),
        scan.djl_kinds.total(),
        first_few(&scan.djl_problems)
    );
    assert_eq!(scan.djl_kinds.get("undecodable"), 0);
    assert!(
        scan.djl_kinds.total() >= 5_000,
        "only {} datagrams on 50000 — is the corpus complete?",
        scan.djl_kinds.total()
    );
}

#[test]
fn the_corpus_covers_every_discovery_packet_kind_two_players_can_produce() {
    let Some(scan) = corpus() else {
        return no_corpus("the UDP-50000 coverage floor");
    };
    // What 33 captures of two CDJ-2000NXS contain. The two mixer-side kinds,
    // `mixer_assign_intent` (0x01) and `mixer_assign` (0x03), need a mixer and
    // this rig has none — so they are named here as a gap rather than left to
    // be noticed. Add a DJM to the rig and this list grows.
    for kind in [
        "hello",
        "claim_mac",
        "claim_ip",
        "claim_number",
        "number_in_use",
        "keep_alive",
    ] {
        assert!(
            scan.djl_kinds.get(kind) > 0,
            "stopped decoding {kind}; the corpus holds {:?}",
            scan.djl_kinds.keys()
        );
    }
}

#[test]
fn no_capture_has_ever_contested_a_device_number() {
    let Some(scan) = corpus() else {
        return no_corpus("the number-conflict tripwire");
    };
    // Not a coverage hole to paper over: our conflict back-off has never been
    // tested against hardware, and this is what will say so on the day a
    // capture finally contains a type-0x08 packet.
    assert_eq!(
        scan.djl_kinds.get("number_conflict"),
        0,
        "a capture now contains a device-number conflict — promote one to a \
         committed fixture and test the announcer's back-off against it"
    );
}

#[test]
fn every_status_datagram_in_the_corpus_decodes_and_agrees_with_its_own_header() {
    let Some(scan) = corpus() else {
        return no_corpus("the UDP-50002 replay");
    };
    assert!(
        scan.status_problems.is_empty(),
        "{} of {} datagrams addressed to 50002 disagreed with themselves:{}",
        scan.status_problems.len(),
        scan.status_kinds.total(),
        first_few(&scan.status_problems)
    );
    assert!(
        scan.status_kinds.total() >= 20_000,
        "only {} datagrams on 50002 — is the corpus complete?",
        scan.status_kinds.total()
    );
    for kind in [
        "cdj_status",
        "media_query",
        "media_response",
        "settings_query",
        "settings_response",
    ] {
        assert!(
            scan.status_kinds.get(kind) > 0,
            "stopped decoding {kind}; the corpus holds {:?}",
            scan.status_kinds.keys()
        );
    }
}

#[test]
fn the_pro_dj_link_magic_appears_on_three_ports_and_no_others() {
    let Some(scan) = corpus() else {
        return no_corpus("the magic-by-port census");
    };
    // The reason a destination-port filter is enough: nothing else on the
    // network speaks this magic, so a datagram carrying it and addressed
    // somewhere else would mean a port this test does not read.
    let mut unexpected = scan.magic_ports.keys();
    unexpected.retain(|port| {
        !matches!(
            port.parse::<u16>(),
            Ok(DISCOVERY_PORT | BEAT_PORT | STATUS_PORT)
        )
    });
    assert!(
        unexpected.is_empty(),
        "Pro DJ Link magic addressed to {unexpected:?}, which nothing decodes"
    );
}

#[test]
fn every_dbserver_stream_in_the_corpus_frames_end_to_end() {
    let Some(scan) = corpus() else {
        return no_corpus("the dbserver replay");
    };
    assert!(
        scan.dbserver_problems.is_empty(),
        "{} dbserver streams did not frame:{}",
        scan.dbserver_problems.len(),
        first_few(&scan.dbserver_problems)
    );
    assert!(
        scan.tcp_problems.is_empty(),
        "{} TCP streams could not be replayed:{}",
        scan.tcp_problems.len(),
        first_few(&scan.tcp_problems)
    );
    assert!(
        scan.dbserver_messages() >= 30_000,
        "only {} dbserver messages — is the corpus complete?",
        scan.dbserver_messages()
    );
    let streams: usize = scan.per_capture.values().map(|t| t.dbserver_streams).sum();
    assert!(
        streams >= 40,
        "only {streams} dbserver streams of {} TCP streams; a classifier that \
         looks only for the preamble finds twelve fewer than one that looks for \
         the message magic, and those twelve carry the largest browses",
        scan.tcp_streams
    );
    for kind in ["introduce", "menu_item", "get_metadata", "success"] {
        assert!(scan.dbserver_kinds.get(kind) > 0, "stopped decoding {kind}");
    }
}

#[test]
fn the_dbserver_port_a_deck_advertises_is_the_one_it_then_serves_on() {
    let Some(scan) = corpus() else {
        return no_corpus("the port-query cross-check");
    };
    assert!(
        scan.port_queries > 0,
        "no TCP-12523 port queries found at all"
    );
    // Two independent readings of the same fact: what the 19-byte query was
    // answered with, and which port the conversation that followed went to.
    // They agree, which is what makes the documented 1051 an observation
    // rather than a rule — most of these conversations are not on it.
    let advertised = scan.advertised_ports.keys();
    let served = scan.dbserver_server_ports.keys();
    let unserved: Vec<&&str> = advertised
        .iter()
        .filter(|port| !served.contains(port))
        .collect();
    assert!(
        unserved.is_empty(),
        "ports advertised on 12523 that no dbserver conversation used: {unserved:?}"
    );
    assert!(
        scan.dbserver_server_ports.get(&dbserver::PORT.to_string()) > 0,
        "no dbserver conversation on the documented port {}; served ports were {:?}",
        dbserver::PORT,
        served
    );
}

#[test]
fn every_rpc_call_in_the_corpus_parses() {
    let Some(scan) = corpus() else {
        return no_corpus("the RPC replay");
    };
    assert!(
        scan.rpc_problems.is_empty(),
        "{} of {} RPC calls did not parse into the shape their procedure implies:{}",
        scan.rpc_problems.len(),
        scan.rpc_calls(),
        first_few(&scan.rpc_problems)
    );
    // The narrow reading of "the RPC traffic in the corpus" — the three ports
    // RFC 1057 and Pioneer make well known — and the whole of it. They differ
    // by a factor of seven, because a portmapper exists exactly so that NFS
    // and MOUNT need not be anywhere in particular.
    assert!(
        scan.rpc_calls_well_known() >= 5_000,
        "only {} calls to {WELL_KNOWN_RPC_PORTS:?}",
        scan.rpc_calls_well_known()
    );
    assert!(
        scan.rpc_calls() > scan.rpc_calls_well_known(),
        "every RPC call in the corpus was on a well-known port, which would mean \
         the portmapper was never used"
    );
    assert!(
        scan.rpc_calls() >= 30_000,
        "only {} RPC calls — is the corpus complete?",
        scan.rpc_calls()
    );
    for procedure in [
        "portmap GETPORT",
        "mount MNT",
        "nfs LOOKUP",
        "nfs READ",
        "nfs GETATTR",
    ] {
        assert!(
            scan.rpc_procedures.get(procedure) > 0,
            "no {procedure} calls; the corpus holds {:?}",
            scan.rpc_procedures.keys()
        );
    }
}

#[test]
fn every_lookup_name_in_the_corpus_is_utf16_little_endian_counted_in_bytes() {
    let Some(scan) = corpus() else {
        return no_corpus("the LOOKUP name check");
    };
    // `rpc_call_problems` has already rejected any name that is not
    // well-formed UTF-16LE or whose byte count is not twice its unit count;
    // this is the coverage floor for that check. A mangled name is the
    // difference between a track that loads and NFSERR_NOENT.
    assert!(
        scan.rpc_procedures.get("nfs LOOKUP") >= 100,
        "only {} LOOKUPs",
        scan.rpc_procedures.get("nfs LOOKUP")
    );
    assert!(
        scan.lookup_names.0.len() >= 20,
        "only {} distinct names looked up: {:?}",
        scan.lookup_names.0.len(),
        scan.lookup_names.keys()
    );
    // The two a deck must resolve before it can read anything at all.
    for name in ["PIONEER (14 bytes)", "export.pdb (20 bytes)"] {
        assert!(
            scan.lookup_names.get(name) > 0,
            "no LOOKUP of {name}; the corpus holds {:?}",
            scan.lookup_names.keys()
        );
    }
    // An ASCII-only sample would prove nothing about the hard case. The bug
    // this replaces — an encoder and a decoder agreeing perfectly on a wrong
    // answer — only showed on non-ASCII input (O6), so the corpus has to
    // contain some and this is the assertion that says it did.
    assert!(
        scan.lookup_non_ascii > 0,
        "every name looked up was ASCII, so the UTF-16 path is untested where \
         it has actually broken before"
    );
}

#[test]
fn every_rpc_credential_in_the_corpus_is_the_same_auth_unix() {
    let Some(scan) = corpus() else {
        return no_corpus("the credential census");
    };
    // Asserted by absence: `rpc_call_problems` records anything that is not
    // AUTH_UNIX with uid 0, gid 0, no machine name and no supplementary gids,
    // and `every_rpc_call_in_the_corpus_parses` fails on it. What is worth
    // asserting here is that the stamp table was exercised at all — it is
    // indexed by how many calls a device has made since power-on, which is a
    // claim only a corpus can support.
    assert!(
        scan.rpc_stamp_agreements > 0,
        "no call carried an xid the stamp sequence covers, so the table is untested"
    );
}

#[test]
fn report_the_numbers() {
    println!("\n== committed fixture floor ==");
    let mut by_prefix = Counts::default();
    let mut bytes = 0usize;
    for (label, raw) in fixtures() {
        by_prefix.bump(label.split('/').next().unwrap_or(&label).to_owned());
        bytes += raw.len();
    }
    for (prefix, count) in &by_prefix.0 {
        println!("  {prefix:<10} {count:>3} fixtures");
    }
    println!("  {bytes} bytes of real packets, no corpus needed");

    let Some(scan) = corpus() else {
        return no_corpus("the corpus summary");
    };

    println!("\n== per capture ==");
    println!(
        "  {:<28} {:>7} {:>7} {:>7} {:>9} {:>7} {:>7}",
        "capture", "50000", "50002", "streams", "messages", "rpc/wk", "rpc"
    );
    for (name, tally) in &scan.per_capture {
        println!(
            "  {name:<28} {:>7} {:>7} {:>7} {:>9} {:>7} {:>7}",
            tally.djl,
            tally.status,
            tally.dbserver_streams,
            tally.dbserver_messages,
            tally.rpc_well_known,
            tally.rpc_calls
        );
    }

    println!("\n== totals ==");
    println!("  captures read              {}", scan.per_capture.len());
    println!("  UDP -> 50000               {}", scan.djl_kinds.total());
    println!("  UDP -> 50001 (not decoded) {}", scan.beat_datagrams);
    println!("  UDP -> 50002               {}", scan.status_kinds.total());
    println!("  TCP streams                {}", scan.tcp_streams);
    println!("  dbserver bytes framed      {}", scan.dbserver_bytes);
    println!("  dbserver messages          {}", scan.dbserver_messages());
    println!(
        "  dbserver streams opened mid-connection {}",
        scan.dbserver_mid_connection
    );
    println!("  TCP-12523 port queries     {}", scan.port_queries);
    println!(
        "  RPC calls, well-known port {}",
        scan.rpc_calls_well_known()
    );
    println!("  RPC calls, any port        {}", scan.rpc_calls());
    println!(
        "  datagrams parsing as an RPC reply (not decoded further) {}",
        scan.rpc_replies
    );
    println!(
        "  argument blocks with a stale XDR pad {}",
        scan.rpc_stale_pads
    );
    println!(
        "  distinct LOOKUP names      {} ({} not ASCII, longest {} bytes)",
        scan.lookup_names.0.len(),
        scan.lookup_non_ascii,
        scan.lookup_longest
    );
    println!(
        "  stamps matching the xid sequence     {}",
        scan.rpc_stamp_agreements
    );

    for (title, counts) in [
        ("UDP 50000 by kind", &scan.djl_kinds),
        ("UDP 50002 by kind", &scan.status_kinds),
        ("Pro DJ Link magic by destination port", &scan.magic_ports),
        ("dbserver messages by kind", &scan.dbserver_kinds),
        (
            "dbserver conversations by server port",
            &scan.dbserver_server_ports,
        ),
        ("ports advertised on TCP 12523", &scan.advertised_ports),
        ("RPC calls by procedure", &scan.rpc_procedures),
        ("RPC calls by destination port", &scan.rpc_ports),
    ] {
        println!("\n== {title} ==");
        for (key, count) in &counts.0 {
            println!("  {count:>7}  {key}");
        }
    }

    // The names themselves are a property of whatever media were in the decks
    // and there are over a thousand of them, so the interesting ones are the
    // most-asked-for and the ones that are not ASCII.
    println!("\n== NFS LOOKUP, the twenty most asked for ==");
    let mut names: Vec<(&String, &usize)> = scan.lookup_names.0.iter().collect();
    names.sort_by(|left, right| right.1.cmp(left.1).then(left.0.cmp(right.0)));
    for (name, count) in names.iter().take(20) {
        println!("  {count:>7}  {name}");
    }
    println!("\n== NFS LOOKUP, the first ten that are not ASCII ==");
    for (name, count) in names.iter().filter(|(name, _)| !name.is_ascii()).take(10) {
        println!("  {count:>7}  {name}");
    }
}
