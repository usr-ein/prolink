// SPDX-License-Identifier: GPL-3.0-only

//! A transcript of an emu-capture: what each player asked another, and what
//! it was answered, frame by frame.
//!
//! ```sh
//! cargo run --release -- ../../E11-link-browse/run.pcap > transcript.txt
//! ```
//!
//! One line per event: `frame  seconds  from>to  what`. A player is `P<n>`
//! once one of its keep-alives has named it, its address before that.
//!
//! - **dbserver** (TCP 1051): every message, through `prolink-proto`'s own
//!   codec. A menu item is printed by field name; a descriptor (argument 0 of
//!   a request) as `D/M/S/T`; a blob by length, its first 32 bytes and its
//!   last 16.
//! - **NFS, mount and portmap** (UDP): each call with its reply, on the reply's
//!   line, with paths resolved from the `MNT` and `LOOKUP` replies before it.
//! - **UDP 50002**: media queries, responses and settings in full; a player's
//!   status only when a field it follows changes (media, loaded track, play
//!   state, flags, tempo, browse list size).
//! - **Keep-alives and beats** are counted, at the end.
//!
//! It reads the capture and nothing else: it decodes with `prolink-proto` and
//! changes nothing in it.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::process::ExitCode;
use std::time::Duration;

use prolink_capture::{Capture, Packet, Transport};
use prolink_proto::dbserver::{self, Descriptor, Field, MenuItem, Message, MessageKind};
use prolink_proto::djl;
use prolink_proto::rpc::{self, mount, nfs2, portmap};
use prolink_proto::status;

const DISCOVERY: u16 = 50000;
const BEAT: u16 = 50001;
const STATUS: u16 = 50002;
const PROGRAM_PORTMAP: u32 = 100_000;
const PROGRAM_NFS: u32 = 100_003;
const PROGRAM_MOUNT: u32 = 100_005;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let raw_status = args.iter().any(|a| a == "--raw-status");
    args.retain(|a| a != "--raw-status");
    let [path] = args.as_slice() else {
        eprintln!("usage: emudump [--raw-status] CAPTURE.pcap");
        return ExitCode::from(2);
    };
    let path = path.clone();
    let capture = match Capture::open(&path) {
        Ok(capture) => capture,
        Err(err) => {
            eprintln!("{path}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut dump = Dump {
        raw_status,
        ..Dump::default()
    };
    for packet in capture {
        match packet {
            Ok(packet) => dump.packet(&packet),
            Err(err) => {
                eprintln!("{path}: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    dump.finish();
    ExitCode::SUCCESS
}

/// One direction of one TCP connection, reassembled in order.
#[derive(Default)]
struct Stream {
    next_seq: Option<u32>,
    bytes: Vec<u8>,
    /// Where decoding has got to.
    pos: usize,
    /// (offset in `bytes`, frame, time) of each segment's first byte.
    starts: Vec<(usize, u64, Duration)>,
    dead: bool,
}

impl Stream {
    fn origin(&self, offset: usize) -> (u64, Duration) {
        let i = self.starts.partition_point(|s| s.0 <= offset);
        self.starts
            .get(i.saturating_sub(1))
            .map_or((0, Duration::ZERO), |s| (s.1, s.2))
    }
}

struct PendingCall {
    frame: u64,
    program: u32,
    procedure: u32,
    what: String,
    /// For a LOOKUP: the path its reply's handle names. For a MNT: the export.
    names: Option<String>,
    read: Option<(String, u32, u32)>,
}

#[derive(Default)]
struct Dump {
    /// Also print, under each status line, the whole packet in hex.
    raw_status: bool,
    t0: Option<Duration>,
    players: HashMap<Ipv4Addr, u8>,
    keepalives: BTreeMap<String, usize>,
    beats: BTreeMap<String, usize>,
    status_last: HashMap<Ipv4Addr, String>,
    status_count: BTreeMap<String, usize>,
    streams: HashMap<(SocketAddrV4, SocketAddrV4), Stream>,
    dbserver_ports: Vec<u16>,
    calls: HashMap<(Ipv4Addr, u32), PendingCall>,
    handles: HashMap<[u8; 32], String>,
    reads: BTreeMap<String, Vec<(u32, u32)>>,
}

impl Dump {
    fn who(&self, ip: Ipv4Addr) -> String {
        self.players
            .get(&ip)
            .map_or_else(|| ip.to_string(), |n| format!("P{n}"))
    }

    fn line(&mut self, frame: u64, at: Duration, from: Ipv4Addr, to: Ipv4Addr, what: &str) {
        let t0 = *self.t0.get_or_insert(at);
        let t = at.saturating_sub(t0).as_secs_f64();
        println!(
            "{frame:>7} {t:>10.3}  {:>5}>{:<5} {what}",
            self.who(from),
            self.who(to)
        );
    }

    fn packet(&mut self, p: &Packet) {
        self.t0.get_or_insert(p.timestamp);
        let (from, to) = (*p.source.ip(), *p.destination.ip());
        match p.transport {
            Transport::Tcp { sequence, syn, .. } => self.tcp(p, sequence, syn),
            Transport::Udp => match p.destination.port() {
                DISCOVERY => self.discovery(p),
                BEAT => *self.beats.entry(self.who(from)).or_default() += 1,
                STATUS => self.status(p),
                _ => self.rpc(p, from, to),
            },
        }
    }

    // ---- UDP 50000 ----------------------------------------------------------

    fn discovery(&mut self, p: &Packet) {
        let Ok(decoded) = djl::Packet::decode(&p.payload) else {
            return;
        };
        let kind = decoded.kind();
        let from = *p.source.ip();
        if let Some(n) = decoded.body.device_number().filter(|n| *n != 0)
            && kind == djl::PacketKind::KEEP_ALIVE
        {
            if let Some(ip) = decoded.body.ip()
                && self.players.insert(ip, n).is_none()
            {
                let what = format!("keep-alive: player {n} is {ip}, {:?}", decoded.name);
                self.line(p.index, p.timestamp, from, *p.destination.ip(), &what);
            }
            *self.keepalives.entry(format!("P{n}")).or_default() += 1;
            return;
        }
        let name = kind
            .name()
            .map_or_else(|| format!("{kind:?}"), str::to_owned);
        let mut what = name.to_string();
        if let Some(n) = decoded.body.device_number() {
            let _ = write!(what, " number={n}");
        }
        if let Some(ip) = decoded.body.ip() {
            let _ = write!(what, " ip={ip}");
        }
        self.line(p.index, p.timestamp, from, *p.destination.ip(), &what);
    }

    // ---- UDP 50002 ----------------------------------------------------------

    fn status(&mut self, p: &Packet) {
        let (from, to) = (*p.source.ip(), *p.destination.ip());
        let Ok(decoded) = status::decode(&p.payload) else {
            self.line(p.index, p.timestamp, from, to, "50002: undecodable");
            return;
        };
        let what = match decoded {
            status::Packet::CdjStatus(s) => {
                *self.status_count.entry(self.who(from)).or_default() += 1;
                let flags = s.flags().map(|f| f.0);
                let loaded = match s.source_player() {
                    Some(player) => format!(
                        "track P{} {:?} type={} id={}",
                        player.get(),
                        s.source_slot(),
                        s.track_type(),
                        s.track_id()
                    ),
                    None => "no track".to_owned(),
                };
                let summary = format!(
                    "usb={:?} sd={:?} link={} {loaded} play={:?} flags={} bpm={} browse={:?}",
                    s.usb_state(),
                    s.sd_state(),
                    s.link_available(),
                    s.play_state(),
                    flags.map_or_else(|| "-".to_owned(), |f| format!("{f:#04x}")),
                    s.bpm_centi().map_or_else(
                        || "-".to_owned(),
                        |b| format!("{:.2}", f64::from(b) / 100.0)
                    ),
                    s.browse_list_size(),
                );
                if self.status_last.get(&from) == Some(&summary) {
                    return;
                }
                self.status_last.insert(from, summary.clone());
                if self.raw_status {
                    format!(
                        "status {summary}\n{}",
                        hexdump(&p.payload, "                         ")
                    )
                } else {
                    format!("status {summary}")
                }
            }
            status::Packet::MediaQuery(q) => format!(
                "media_query: P{} ({}) asks about P{} {:?}",
                q.requester.get(),
                q.requester_ip,
                q.target.get(),
                q.slot
            ),
            status::Packet::MediaResponse(r) => format!(
                "media_response: P{} {:?} name={:?} created={:?} tracks={} playlists={} \
                 total={:?} free={:?}\n{}",
                r.device().map_or(0, |d| d.get()),
                r.slot(),
                r.volume_name(),
                r.created(),
                r.track_count(),
                r.playlist_count(),
                r.total_bytes(),
                r.free_bytes(),
                hexdump(&p.payload, "                         ")
            ),
            status::Packet::SettingsQuery(q) => format!("settings_query: {q:?}"),
            status::Packet::SettingsResponse(r) => format!("settings_response: {r:?}"),
            status::Packet::Other { kind, raw } => format!(
                "50002 kind {kind:?}, {} bytes\n{}",
                raw.len(),
                hexdump(&raw, "                         ")
            ),
        };
        self.line(p.index, p.timestamp, from, to, &what);
    }

    // ---- ONC RPC: portmap, mount, NFS ---------------------------------------

    fn rpc(&mut self, p: &Packet, from: Ipv4Addr, to: Ipv4Addr) {
        let Ok(message) = rpc::Message::parse(&p.payload) else {
            let what = format!(
                "UDP {}>{} {} bytes, not RPC",
                p.source.port(),
                p.destination.port(),
                p.payload.len()
            );
            self.line(p.index, p.timestamp, from, to, &what);
            return;
        };
        match message {
            rpc::Message::Call(call) => {
                let pending = self.describe_call(p.index, &call);
                self.calls.insert((from, call.xid.0), pending);
            }
            rpc::Message::Reply(reply) => {
                let Some(call) = self.calls.remove(&(to, reply.xid().0)) else {
                    self.line(p.index, p.timestamp, from, to, "RPC reply to no call seen");
                    return;
                };
                let answer = match reply.results() {
                    Some(results) => self.describe_reply(&call, results),
                    None => format!("rejected or failed: {reply:?}"),
                };
                let what = format!("{} = {answer}   [call {}]", call.what, call.frame);
                self.line(p.index, p.timestamp, to, from, &what);
            }
        }
    }

    fn path_of(&self, handle: &nfs2::FileHandle) -> String {
        self.handles
            .get(handle.as_bytes())
            .cloned()
            .unwrap_or_else(|| {
                let key = &handle.as_bytes()[..nfs2::FileHandle::KEY_LEN];
                self.handles
                    .iter()
                    .find(|(h, _)| &h[..nfs2::FileHandle::KEY_LEN] == key)
                    .map_or_else(
                        || format!("fh:{}", hex(handle.as_bytes())),
                        |(_, p)| p.clone(),
                    )
            })
    }

    fn describe_call(&self, frame: u64, call: &rpc::Call<'_>) -> PendingCall {
        let (program, procedure) = (call.program.0, call.procedure);
        let mut names = None;
        let mut read = None;
        let what = match program {
            PROGRAM_PORTMAP => {
                match portmap::Request::parse(portmap::Proc(procedure), call.arguments) {
                    Ok(portmap::Request::GetPort(m)) => format!("portmap GETPORT {m:?}"),
                    Ok(other) => format!("portmap {other:?}"),
                    Err(err) => format!("portmap proc {procedure}: {err}"),
                }
            }
            PROGRAM_MOUNT => match mount::Request::parse(mount::Proc(procedure), call.arguments) {
                Ok(mount::Request::Mnt(path)) => {
                    names = Some(path.to_string_lossy());
                    format!("mount MNT {:?}", path.to_string_lossy())
                }
                Ok(mount::Request::Umnt(path)) => {
                    format!("mount UMNT {:?}", path.to_string_lossy())
                }
                Ok(other) => format!("mount {other:?}"),
                Err(err) => format!("mount proc {procedure}: {err}"),
            },
            PROGRAM_NFS => match nfs2::Request::parse(nfs2::Proc(procedure), call.arguments) {
                Ok(nfs2::Request::Lookup { dir, name }) => {
                    let tail = hex(&dir.as_bytes()[nfs2::FileHandle::KEY_LEN..]);
                    let dir = self.path_of(&dir);
                    let shown = lookup_name(name.as_bytes());
                    let full = format!("{}/{shown}", dir.trim_end_matches('/'));
                    names = Some(full.clone());
                    format!(
                        "nfs LOOKUP \"{full}\" (name bytes {}; dir fh tail {tail})",
                        hex(name.as_bytes())
                    )
                }
                Ok(nfs2::Request::Read(args)) => {
                    let path = self.path_of(&args.handle);
                    read = Some((path.clone(), args.offset, args.count));
                    format!("nfs READ \"{path}\" @{} +{}", args.offset, args.count)
                }
                Ok(nfs2::Request::GetAttr(h)) => format!("nfs GETATTR \"{}\"", self.path_of(&h)),
                Ok(nfs2::Request::ReadDir(args)) => {
                    format!(
                        "nfs READDIR \"{}\" count={}",
                        self.path_of(&args.handle),
                        args.count
                    )
                }
                Ok(nfs2::Request::StatFs(h)) => format!("nfs STATFS \"{}\"", self.path_of(&h)),
                Ok(nfs2::Request::Null) => "nfs NULL".to_owned(),
                Ok(nfs2::Request::Unknown {
                    procedure,
                    arguments,
                }) => {
                    format!(
                        "nfs proc {} ({} bytes of arguments)",
                        procedure.0,
                        arguments.len()
                    )
                }
                Err(err) => format!("nfs proc {procedure}: {err}"),
            },
            other => format!("RPC program {other} proc {procedure}"),
        };
        PendingCall {
            frame,
            program,
            procedure,
            what,
            names,
            read,
        }
    }

    fn describe_reply(&mut self, call: &PendingCall, results: &[u8]) -> String {
        match call.program {
            PROGRAM_PORTMAP => {
                match portmap::Response::parse(portmap::Proc(call.procedure), results) {
                    Ok(r) => format!("{r:?}"),
                    Err(err) => format!("undecodable: {err}"),
                }
            }
            PROGRAM_MOUNT => match mount::Response::parse(mount::Proc(call.procedure), results) {
                Ok(mount::Response::Mnt(Ok(handle))) => {
                    if let Some(path) = &call.names {
                        self.handles.insert(*handle.as_bytes(), path.clone());
                    }
                    format!("root fh:{}", hex(handle.as_bytes()))
                }
                Ok(r) => format!("{r:?}"),
                Err(err) => format!("undecodable: {err}"),
            },
            PROGRAM_NFS => match nfs2::Response::parse(nfs2::Proc(call.procedure), results) {
                Ok(nfs2::Response::Lookup(Ok(file))) => {
                    if let Some(path) = &call.names {
                        self.handles.insert(*file.handle.as_bytes(), path.clone());
                    }
                    format!(
                        "{:?} size={} fileid={} fh:{}",
                        file.attr.ftype,
                        file.attr.size,
                        file.attr.fileid,
                        hex(file.handle.as_bytes())
                    )
                }
                Ok(nfs2::Response::Read(Ok(data))) => {
                    if let Some((path, offset, _)) = &call.read {
                        let len = u32::try_from(data.data.len()).unwrap_or(u32::MAX);
                        self.reads
                            .entry(path.clone())
                            .or_default()
                            .push((*offset, len));
                    }
                    format!("{} bytes", data.data.len())
                }
                Ok(nfs2::Response::Attr(Ok(attr))) => {
                    format!("{:?} size={} fileid={}", attr.ftype, attr.size, attr.fileid)
                }
                Ok(r) => match r.status() {
                    nfs2::Status::OK => format!("{r:?}"),
                    status => status
                        .name()
                        .map_or_else(|| format!("{status:?}"), str::to_owned),
                },
                Err(err) => format!("undecodable: {err}"),
            },
            _ => format!("{} bytes", results.len()),
        }
    }

    // ---- TCP: the dbserver and its port query --------------------------------

    fn tcp(&mut self, p: &Packet, sequence: u32, syn: bool) {
        let key = (p.source, p.destination);
        let ports = (p.source.port(), p.destination.port());
        let stream = self.streams.entry(key).or_default();
        if syn {
            stream.next_seq = Some(sequence.wrapping_add(1));
        }
        if !p.payload.is_empty() && !stream.dead {
            let expected = *stream.next_seq.get_or_insert(sequence);
            let skip = expected.wrapping_sub(sequence);
            let len = u32::try_from(p.payload.len()).unwrap_or(u32::MAX);
            if sequence == expected || (skip > 0 && skip < len) {
                // In order, or a retransmission that runs past what we have.
                let fresh = &p.payload
                    [usize::try_from(if sequence == expected { 0 } else { skip }).unwrap_or(0)..];
                stream
                    .starts
                    .push((stream.bytes.len(), p.index, p.timestamp));
                stream.bytes.extend_from_slice(fresh);
                stream.next_seq = Some(sequence.wrapping_add(len));
            } else if expected.wrapping_sub(sequence) < 0x8000_0000 {
                // Entirely a retransmission: nothing new.
            } else {
                stream.dead = true;
                let what = format!(
                    "TCP {}>{}: a gap in the stream, stopped decoding it",
                    ports.0, ports.1
                );
                self.line(
                    p.index,
                    p.timestamp,
                    *p.source.ip(),
                    *p.destination.ip(),
                    &what,
                );
                return;
            }
        }
        if ports.0 == dbserver::PORT_QUERY_PORT || ports.1 == dbserver::PORT_QUERY_PORT {
            self.port_query(key);
        } else if self.is_dbserver(ports) {
            self.dbserver(key);
        } else if !p.payload.is_empty() {
            let what = format!("TCP {}>{} {} bytes", ports.0, ports.1, p.payload.len());
            self.line(
                p.index,
                p.timestamp,
                *p.source.ip(),
                *p.destination.ip(),
                &what,
            );
        }
    }

    fn is_dbserver(&self, ports: (u16, u16)) -> bool {
        [ports.0, ports.1]
            .iter()
            .any(|port| *port == dbserver::PORT || self.dbserver_ports.contains(port))
    }

    fn port_query(&mut self, key: (SocketAddrV4, SocketAddrV4)) {
        let Some(stream) = self.streams.get_mut(&key) else {
            return;
        };
        let data = stream.bytes.get(stream.pos..).unwrap_or(&[]).to_vec();
        if data.is_empty() {
            return;
        }
        let (frame, at) = stream.origin(stream.pos);
        let what = if key.1.port() == dbserver::PORT_QUERY_PORT {
            if data.len() < dbserver::PORT_QUERY.len() {
                return;
            }
            stream.pos += dbserver::PORT_QUERY.len();
            format!(
                "port query{} ({})",
                if data[..dbserver::PORT_QUERY.len()] == dbserver::PORT_QUERY {
                    ""
                } else {
                    ", not the usual bytes"
                },
                hex(&data[..dbserver::PORT_QUERY.len()])
            )
        } else {
            if data.len() < 2 {
                return;
            }
            stream.pos += 2;
            match dbserver::decode_port_reply(&data[..2]) {
                Ok(port) => {
                    self.dbserver_ports.push(port);
                    format!("port reply: dbserver on {port}")
                }
                Err(err) => format!("port reply undecodable: {err}"),
            }
        };
        self.line(frame, at, *key.0.ip(), *key.1.ip(), &what);
    }

    fn dbserver(&mut self, key: (SocketAddrV4, SocketAddrV4)) {
        loop {
            let Some(stream) = self.streams.get_mut(&key) else {
                return;
            };
            if stream.dead {
                return;
            }
            let rest = stream.bytes.get(stream.pos..).unwrap_or(&[]);
            if rest.is_empty() {
                return;
            }
            if stream.pos == 0 {
                if rest.len() < dbserver::PREAMBLE.len() {
                    return;
                }
                let skipped = rest.len() - dbserver::skip_preamble(rest).len();
                if skipped > 0 {
                    let (frame, at) = stream.origin(0);
                    stream.pos = skipped;
                    self.line(
                        frame,
                        at,
                        *key.0.ip(),
                        *key.1.ip(),
                        "dbserver preamble 11 00 00 00 01",
                    );
                    continue;
                }
            }
            let (frame, at) = stream.origin(stream.pos);
            match Message::decode(rest) {
                Ok((message, used)) => {
                    stream.pos += used;
                    let what = format!("db {}", describe_message(&message));
                    self.line(frame, at, *key.0.ip(), *key.1.ip(), &what);
                }
                Err(err) if err.is_truncated() => return,
                Err(err) => {
                    stream.dead = true;
                    let what = format!("db undecodable, stopped decoding this direction: {err}");
                    self.line(frame, at, *key.0.ip(), *key.1.ip(), &what);
                    return;
                }
            }
        }
    }

    fn finish(&mut self) {
        let mut unanswered: Vec<_> = self
            .calls
            .values()
            .map(|c| (c.frame, c.what.clone()))
            .collect();
        unanswered.sort();
        println!();
        for (frame, what) in unanswered {
            println!("unanswered call at frame {frame}: {what}");
        }
        for (title, counts) in [
            ("keep-alives", &self.keepalives),
            ("beat packets", &self.beats),
            ("status packets", &self.status_count),
        ] {
            let list: Vec<String> = counts.iter().map(|(who, n)| format!("{who} {n}")).collect();
            println!(
                "{title}: {}",
                if list.is_empty() {
                    "none".to_owned()
                } else {
                    list.join(", ")
                }
            );
        }
        for (path, reads) in &self.reads {
            let total: u64 = reads.iter().map(|r| u64::from(r.1)).sum();
            let first = reads.iter().map(|r| r.0).min().unwrap_or(0);
            let last = reads.iter().map(|r| r.0 + r.1).max().unwrap_or(0);
            println!(
                "NFS reads of \"{path}\": {} reads, {total} bytes, offsets {first}..{last}",
                reads.len()
            );
        }
    }
}

fn describe_message(m: &Message) -> String {
    let name = m
        .kind
        .name()
        .map_or_else(|| format!("{:#06x}", m.kind.0), str::to_owned);
    if let Some(item) = MenuItem::from_message(m) {
        return format!(
            "{name} id={} type={:#06x}{} label1={} label2={} arg0={:#x} flags={:#x} artwork={} \
             pos={} key={}",
            item.id,
            item.item_type.0,
            item_type_name(item.item_type.0).map_or_else(String::new, |n| format!(" ({n})")),
            text(&item.label1),
            text(&item.label2),
            item.argument0,
            item.flags,
            item.artwork_id,
            item.playlist_position,
            item.key_index,
        );
    }
    let is_request = m.kind.0 < 0x4000 && m.kind != MessageKind::INTRODUCE;
    let args: Vec<String> = m
        .args
        .as_slice()
        .iter()
        .enumerate()
        .map(|(i, f)| match f {
            Field::U32(v) if i == 0 && is_request => match Descriptor::parse(*v) {
                Some(d) => format!(
                    "{v:#010x}=D{}/M{}/S{}/T{}",
                    d.device.get(),
                    d.menu.0,
                    d.slot.0,
                    d.track_type.0
                ),
                None => format!("{v:#x}"),
            },
            Field::U8(v) => format!("{v:#x}"),
            Field::U16(v) => format!("{v:#x}"),
            Field::U32(v) => format!("{v:#x}"),
            Field::Text { text: t, .. } => text(t),
            Field::Blob(b) if b.is_empty() => "blob[0]".to_owned(),
            Field::Blob(b) if b.len() <= 64 => format!("blob[{}] {}", b.len(), hex(b)),
            Field::Blob(b) => format!(
                "blob[{}] {}...{} ({} zero bytes)",
                b.len(),
                hex(&b[..32]),
                hex(&b[b.len() - 16..]),
                b.iter().filter(|x| **x == 0).count()
            ),
        })
        .collect();
    format!("{name} txn={:#x} [{}]", m.transaction_id, args.join(", "))
}

/// An NFS name as text. Names are UTF-16LE, but a CDJ asking for an
/// unanalysed file sends `3f 00` (a UTF-16 `?`) and then plain ASCII, which
/// is shown here as `?\0` and that ASCII.
fn lookup_name(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0x3f, 0x00]) {
        let rest = rest.strip_suffix(&[0]).unwrap_or(rest);
        if rest.iter().all(|b| (0x20..=0x7e).contains(b)) {
            return format!("?\\0{}", String::from_utf8_lossy(rest));
        }
    }
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// A label, with the U+FFFA..U+FFFB category wrapper shown as `⟦..⟧`.
fn text(t: &str) -> String {
    let shown = t.replace('\u{fffa}', "⟦").replace('\u{fffb}', "⟧");
    format!("{shown:?}")
}

fn item_type_name(t: u32) -> Option<&'static str> {
    Some(match t & 0xffff {
        0x0000 => "path",
        0x0001 => "folder",
        0x0002 => "album",
        0x0003 => "disc",
        0x0004 => "title",
        0x0006 => "genre",
        0x0007 => "artist",
        0x0008 => "playlist",
        0x000a => "rating",
        0x000b => "duration",
        0x000d => "tempo",
        0x000e => "label",
        0x000f => "key",
        0x0010 => "bitrate",
        0x0011 => "year",
        0x0013 => "colour",
        0x0023 => "comment",
        0x0024 => "history playlist",
        0x0028 => "original artist",
        0x0029 => "remixer",
        0x002a => "play count",
        0x002e => "date added",
        0x002f => "track info 6",
        0x00a0 => "ALL",
        0x00a1 => "sort DEFAULT",
        0x00a2 => "sort ALPHABET",
        0x0080 => "GENRE menu",
        0x0081 => "ARTIST menu",
        0x0082 => "ALBUM menu",
        0x0083 => "TRACK menu",
        0x0084 => "PLAYLIST menu",
        0x0085 => "BPM menu",
        0x0086 => "RATING menu",
        0x0089 => "LABEL menu",
        0x008b => "KEY menu",
        0x008c => "DATE ADDED menu",
        0x0090 => "FOLDER menu",
        0x0091 => "SEARCH menu",
        0x0093 => "BITRATE menu",
        0x0095 => "HISTORY menu",
        0x0097 => "PLAY COUNT menu",
        t if t & 0xff == 0x04 && t > 0x04 => return item_type_name(t >> 8).map(|_| "track row"),
        _ => return None,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hexdump(bytes: &[u8], indent: &str) -> String {
    let mut out = String::new();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        if !out.is_empty() {
            out.push('\n');
        }
        let hexes: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let _ = write!(out, "{indent}{:04x}  {}", row * 16, hexes.join(" "));
    }
    out
}
