// SPDX-License-Identifier: GPL-3.0-only

//! Every ONC RPC call in the capture corpus, replayed through our servers.
//!
//! The unit tests in `serve/nfs` pin eight captured datagrams as hex literals.
//! This one takes the other 45 000: it walks all 37 captures, reconstructs the
//! medium each session was reading, stands our three servers up on ephemeral
//! ports, and sends every call a device actually sent — the deck's own
//! credentials, its own UTF-16LE names, its own rewritten filehandles — over a
//! real socket.
//!
//! # Reconstructing the medium from the traffic
//!
//! A capture contains no filesystem, but it contains enough to rebuild one. The
//! server in each session minted a handle per path, and every `LOOKUP` names a
//! parent handle and a component, so the call and reply streams together are a
//! walk of the tree: `MNT` gives the root handle for an export, and each
//! `LOOKUP` reply gives a child's handle, type and size. That yields a set of
//! paths with sizes, which is written out as **sparse files** — a 75 MB track
//! costs no disk — and mounted as a [`Vfs`].
//!
//! The point of doing it that way rather than inventing a tree is that the
//! replay then has to answer the same questions the real server answered, and
//! the answers can be compared: the same status, the same file size, the same
//! number of payload bytes per `READ`.
//!
//! # Filehandles, at scale
//!
//! A captured handle is meaningless to us — it is a hash of a path in someone
//! else's tree, or a CDJ's own three-word reference. Each is re-aimed at ours by
//! replacing the twelve bytes a deck preserves and **keeping its own twenty
//! rewritten bytes verbatim**, so every call in the corpus exercises F28 rather
//! than the one fixture that does in the unit tests.
//!
//! # The floor
//!
//! The corpus is 272 MB and lives in `captures/`. A clone without it — the
//! crate published to crates.io excludes it — makes the sweep skip, so this
//! file also carries tests that need no capture at all, including the one about
//! read sizes that the corpus prompted.

// An assertion *is* the failure mode of a test; propagating errors carefully
// would report them as passes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::as_conversions,
    clippy::cast_possible_truncation
)]

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use prolink::serve::nfs::{NfsConfig, NfsServer, Ports};
use prolink::serve::{ServedSlot, Vfs};
use prolink_capture::{Capture, Corpus};
use prolink_proto::rpc::nfs2::{FType, FileHandle, FileHandleKey, ReadArgs, Status};
use prolink_proto::rpc::{Call, Message, Program, Reply, mount, nfs2, portmap};
use tokio::net::UdpSocket;

/// How long to wait for a reply on loopback before calling the server broken.
const PATIENCE: Duration = Duration::from_secs(5);

// -- the floor -------------------------------------------------------------

/// Ephemeral everything: no privileges, no collisions.
fn ephemeral() -> NfsConfig {
    NfsConfig {
        interface: None,
        portmap_port: 0,
        mount_port: 0,
        nfs_port: 0,
    }
}

async fn round_trip(client: &UdpSocket, port: u16, call: &[u8]) -> Vec<u8> {
    client
        .send_to(call, SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port))
        .await
        .expect("a call goes out");
    let mut buffer = vec![0u8; 65535];
    let (len, _) = tokio::time::timeout(PATIENCE, client.recv_from(&mut buffer))
        .await
        .expect("a reply arrives")
        .expect("a reply is received");
    buffer.truncate(len);
    buffer
}

fn results(datagram: &[u8]) -> Vec<u8> {
    Reply::parse(datagram)
        .expect("a reply must decode")
        .results()
        .expect("a successful reply")
        .to_vec()
}

fn call_bytes(program: Program, version: u32, procedure: u32, arguments: &[u8]) -> Vec<u8> {
    let credential =
        prolink_proto::rpc::AuthUnix::cdj(prolink_proto::rpc::STAMP_FIRST_CALL).encode();
    Call::new(
        prolink_proto::rpc::Xid(1),
        program,
        version,
        procedure,
        prolink_proto::rpc::Auth::unix(&credential),
        arguments,
    )
    .encode()
}

/// A single sparse file of `size` bytes, mounted at `/C/big.wav`.
fn sparse_medium(size: u64, name: &str) -> (Arc<RwLock<Vfs>>, PathBuf) {
    let root = std::env::temp_dir().join(format!("prolink-nfs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a temporary directory");
    let file = std::fs::File::create(root.join("big.wav")).expect("a file");
    file.set_len(size).expect("a sparse file");
    drop(file);
    let mut vfs = Vfs::new();
    vfs.mount("C", &root).expect("it mounts");
    (Arc::new(RwLock::new(vfs)), root)
}

/// The question the corpus prompted, and its answer is not the obvious one.
///
/// Deck to deck the modal `READ` is **9408** bytes and a file's first read can
/// be **28584**, answered as one 28 656-byte datagram in about twenty IP
/// fragments. Answering like that is not portable: **macOS refuses to send a UDP
/// datagram larger than 9216 bytes** (`net.inet.udp.maxdgram`), so the reply
/// would fail with `EMSGSIZE` and never leave the host, and the caller would
/// retransmit for ever. A short read is ordinary — the client asks for the rest —
/// and a reply that cannot be sent is a stall, so short is not merely allowed
/// here, it is the only thing that works.
///
/// This test asserts both halves: everything up to 8192 comes back in full, and
/// anything larger comes back short but **sendable**, which is checked by the
/// datagram actually arriving.
#[tokio::test]
async fn a_read_is_answered_in_full_up_to_a_reply_the_host_can_send() {
    let (vfs, root) = sparse_medium(1 << 20, "bigread");
    let server = NfsServer::start(vfs, ephemeral()).await.expect("start");
    let client = UdpSocket::bind("127.0.0.1:0").await.expect("a client");
    let handle = Vfs::handle_for("/C/big.wav");

    // 160, 2048 and 8192 are the sizes decks have actually asked us for; 9408
    // and 28584 are what they ask each other; 100000 is past what NFSv2 itself
    // can express in one reply.
    for count in [160u32, 2048, 8192, 9408, 28_584, 100_000] {
        let call = call_bytes(
            Program::NFS,
            nfs2::VERSION,
            nfs2::Proc::READ.0,
            &nfs2::Request::Read(ReadArgs::at(handle, 0, count).unwrap()).encode_arguments(),
        );
        let reply = round_trip(&client, server.ports().nfs, &call).await;
        let payload = results(&reply);
        let nfs2::Response::Read(Ok(read)) =
            nfs2::Response::parse(nfs2::Proc::READ, &payload).unwrap()
        else {
            panic!("a read of {count} bytes must succeed");
        };
        assert_eq!(
            read.data.len(),
            count.min(8192) as usize,
            "a {count}-byte read is answered in full up to 8192 and short past it",
        );
        // The reply arrived, which is the assertion that matters: a larger one
        // would not have been sent at all on this host.
        assert!(reply.len() <= 9216, "{} bytes", reply.len());
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Two captured calls, committed as literals, replayed over real sockets.
///
/// The floor under the sweep below: it proves the replay machinery itself on a
/// machine with no corpus.
#[tokio::test]
async fn captured_calls_replay_over_sockets() {
    // `S10j-serve-to-cdj` frames 1 and 3: the deck's first `GETPORT`, and the
    // `MNT` of `/C/` that followed it.
    let getport = hex(
        "00000fe90000000000000002000186a000000002000000030000000100000014\
         14b7e60a000000000000000000000000000000000000000000000000000186a5\
         000000010000001100000000",
    );
    let mnt = hex(
        "00000feb0000000000000002000186a500000001000000010000000100000014\
         05de9b1c00000000000000000000000000000000000000000000000000000006\
         2f0043002f000011",
    );

    let (vfs, root) = sparse_medium(4096, "floor");
    let server = NfsServer::start(vfs, ephemeral()).await.expect("start");
    let client = UdpSocket::bind("127.0.0.1:0").await.expect("a client");

    let reply = round_trip(&client, server.ports().portmap, &getport).await;
    assert_eq!(
        portmap::Response::parse(portmap::Proc::GETPORT, &results(&reply)).unwrap(),
        portmap::Response::GetPort(Some(server.ports().mount)),
    );
    let reply = round_trip(&client, server.ports().mount, &mnt).await;
    assert_eq!(
        mount::Response::parse(mount::Proc::MNT, &results(&reply)).unwrap(),
        mount::Response::Mnt(Ok(Vfs::handle_for("/C"))),
    );
    let _ = std::fs::remove_dir_all(&root);
}

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<char> = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        digits.len().is_multiple_of(2),
        "an even number of hex digits"
    );
    digits
        .chunks_exact(2)
        .map(|pair| {
            let byte: String = pair.iter().collect();
            u8::from_str_radix(&byte, 16).expect("a hex literal must be hex")
        })
        .collect()
}

// -- the sweep -------------------------------------------------------------

/// One call, and what the server in the capture answered it with.
struct Replayed {
    program: Program,
    version: u32,
    procedure: u32,
    datagram: Vec<u8>,
    /// The status the captured reply carried, when the reply was captured too.
    status: Option<Status>,
    /// For a `READ`, how many payload bytes came back.
    payload: Option<usize>,
    /// For a `LOOKUP` or `GETATTR`, the type and size reported.
    attributes: Option<(FType, u32)>,
}

/// One session: the tree it was reading, and the calls that read it.
#[derive(Default)]
struct Session {
    /// A filehandle the captured server issued → the path we will serve it at.
    paths: BTreeMap<FileHandleKey, String>,
    /// Path → size, or `None` for a directory.
    nodes: BTreeMap<String, Option<u64>>,
    calls: Vec<Replayed>,
    /// (client, server, xid) → which call, so a reply can find its call.
    outstanding: BTreeMap<(SocketAddrV4, SocketAddrV4, u32), usize>,
    /// A handle whose last reply came up short of the request, and the offset a
    /// client re-requesting the shortfall would ask for next.
    shortfall: BTreeMap<FileHandleKey, u32>,
}

/// What the whole sweep measured.
#[derive(Default)]
struct Census {
    captures: usize,
    sessions: usize,
    calls: BTreeMap<String, u64>,
    replayed: u64,
    compared: u64,
    unmapped: u64,
    errors_agreed: u64,
    shortened: u64,
    largest_shortened: usize,
    short_mid_file: u64,
    resumed_after_short: u64,
    errors_differed: BTreeMap<(u32, u32), u64>,
    names: BTreeSet<String>,
    read_sizes: BTreeMap<u32, u64>,
    largest_read: u32,
    tree_entries: usize,
    unplaced: u64,
    rpc_failures: u64,
}

fn procedure_name(program: Program, procedure: u32) -> String {
    let name = match program {
        Program::PORTMAP => portmap::Proc(procedure).name(),
        Program::MOUNT => mount::Proc(procedure).name(),
        _ => nfs2::Proc(procedure).name(),
    };
    format!(
        "{}:{}",
        program.name().unwrap_or("?"),
        name.map_or_else(|| procedure.to_string(), str::to_owned),
    )
}

fn is_ours(program: Program) -> bool {
    matches!(program, Program::PORTMAP | Program::MOUNT | Program::NFS)
}

/// The subtree an export path maps into, `/B` or `/C`.
fn subtree_for(export: &str) -> Option<String> {
    let slot = ServedSlot::new(mount::slot_for_export(export)?)?;
    Some(format!("/{}", slot.vfs_prefix()))
}

/// A name that can be a directory entry on this filesystem.
fn usable(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\0')
}

fn join(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    }
}

/// Walk one capture: pair calls with replies, and learn the tree from both.
fn read_session(path: &Path, census: &mut Census) -> Session {
    let mut session = Session::default();
    let Ok(capture) = Capture::open(path) else {
        return session;
    };
    for packet in capture {
        // A file cut short costs the rest of that file and not the run.
        let Ok(packet) = packet else { break };
        if !packet.transport.is_udp() {
            continue;
        }
        match Message::parse(&packet.payload) {
            Ok(Message::Call(call)) if is_ours(call.program) => {
                session.outstanding.insert(
                    (packet.source, packet.destination, call.xid.0),
                    session.calls.len(),
                );
                *census
                    .calls
                    .entry(procedure_name(call.program, call.procedure))
                    .or_default() += 1;
                if call.program == Program::NFS
                    && let Ok(nfs2::Request::Read(args)) =
                        nfs2::Request::parse(nfs2::Proc(call.procedure), call.arguments)
                {
                    *census.read_sizes.entry(args.count).or_default() += 1;
                    census.largest_read = census.largest_read.max(args.count);
                    // Whether a short answer is safe is the question our own
                    // 8192-byte cap turns on, and only hardware can settle it:
                    // did the reader re-request the shortfall, or take it for
                    // the end of the file?
                    if session.shortfall.remove(&args.handle.key()) == Some(args.offset) {
                        census.resumed_after_short += 1;
                    }
                }
                if call.program == Program::NFS
                    && let Ok(nfs2::Request::Lookup { name, .. }) =
                        nfs2::Request::parse(nfs2::Proc(call.procedure), call.arguments)
                {
                    census.names.insert(name.to_string_lossy());
                }
                session.calls.push(Replayed {
                    program: call.program,
                    version: call.version,
                    procedure: call.procedure,
                    datagram: packet.payload.clone(),
                    status: None,
                    payload: None,
                    attributes: None,
                });
            }
            Ok(Message::Reply(reply)) => {
                let key = (packet.destination, packet.source, reply.xid().0);
                let Some(&at) = session.outstanding.get(&key) else {
                    continue;
                };
                let Some(results) = reply.results() else {
                    census.rpc_failures += 1;
                    continue;
                };
                let results = results.to_vec();
                let datagram = session.calls[at].datagram.clone();
                learn(&mut session, at, &datagram, &results, census);
            }
            _ => {}
        }
    }
    session
}

/// Fold one captured reply into the reconstructed tree.
fn learn(session: &mut Session, at: usize, datagram: &[u8], results: &[u8], census: &mut Census) {
    let Ok(call) = Call::parse(datagram) else {
        return;
    };
    match call.program {
        Program::MOUNT if call.procedure == mount::Proc::MNT.0 => {
            learn_mnt(session, at, &call, results);
        }
        Program::NFS if call.procedure == nfs2::Proc::LOOKUP.0 => {
            learn_lookup(session, at, &call, results, census);
        }
        Program::NFS if call.procedure == nfs2::Proc::GETATTR.0 => {
            learn_getattr(session, at, &call, results);
        }
        Program::NFS if call.procedure == nfs2::Proc::READ.0 => {
            learn_read(session, at, &call, results, census);
        }
        _ => {}
    }
}

/// `MNT`: the root handle for an export, and the subtree we will serve it at.
fn learn_mnt(session: &mut Session, at: usize, call: &Call<'_>, results: &[u8]) {
    {
        let (Ok(mount::Request::Mnt(export)), Ok(mount::Response::Mnt(answer))) = (
            mount::Request::parse(mount::Proc::MNT, call.arguments),
            mount::Response::parse(mount::Proc::MNT, results),
        ) else {
            return;
        };
        session.calls[at].status = Some(match answer {
            Ok(_) => Status::OK,
            Err(status) => status.status(),
        });
        let (Ok(handle), Some(subtree)) = (answer, subtree_for(&export.to_string_lossy())) else {
            return;
        };
        session.paths.insert(handle.key(), subtree.clone());
        session.nodes.entry(subtree).or_insert(None);
    }
}

/// `LOOKUP`: one more component of the tree, with its type and size.
fn learn_lookup(
    session: &mut Session,
    at: usize,
    call: &Call<'_>,
    results: &[u8],
    census: &mut Census,
) {
    {
        let (Ok(nfs2::Request::Lookup { dir, name }), Ok(nfs2::Response::Lookup(answer))) = (
            nfs2::Request::parse(nfs2::Proc::LOOKUP, call.arguments),
            nfs2::Response::parse(nfs2::Proc::LOOKUP, results),
        ) else {
            return;
        };
        session.calls[at].status = Some(match &answer {
            Ok(_) => Status::OK,
            Err(status) => status.status(),
        });
        let Ok(found) = answer else { return };
        session.calls[at].attributes = Some((found.attr.ftype, found.attr.size));
        let name = name.to_string_lossy();
        let (Some(parent), true) = (session.paths.get(&dir.key()).cloned(), usable(&name)) else {
            census.unplaced += 1;
            return;
        };
        let child = join(&parent, &name);
        session.paths.insert(found.handle.key(), child.clone());
        let size = if found.attr.is_directory() {
            None
        } else {
            Some(u64::from(found.attr.size))
        };
        merge(&mut session.nodes, child, size);
    }
}

/// `GETATTR`: the size a file really was, when a `LOOKUP` did not say.
fn learn_getattr(session: &mut Session, at: usize, call: &Call<'_>, results: &[u8]) {
    {
        let (Ok(nfs2::Request::GetAttr(handle)), Ok(nfs2::Response::Attr(answer))) = (
            nfs2::Request::parse(nfs2::Proc::GETATTR, call.arguments),
            nfs2::Response::parse(nfs2::Proc::GETATTR, results),
        ) else {
            return;
        };
        session.calls[at].status = Some(match &answer {
            Ok(_) => Status::OK,
            Err(status) => status.status(),
        });
        let Ok(attr) = answer else { return };
        session.calls[at].attributes = Some((attr.ftype, attr.size));
        if let Some(path) = session.paths.get(&handle.key()).cloned() {
            let size = if attr.is_directory() {
                None
            } else {
                Some(u64::from(attr.size))
            };
            merge(&mut session.nodes, path, size);
        }
    }
}

/// `READ`: how much came back, and whether the answer was short of the ask.
fn learn_read(
    session: &mut Session,
    at: usize,
    call: &Call<'_>,
    results: &[u8],
    census: &mut Census,
) {
    {
        let (Ok(nfs2::Request::Read(args)), Ok(nfs2::Response::Read(answer))) = (
            nfs2::Request::parse(nfs2::Proc::READ, call.arguments),
            nfs2::Response::parse(nfs2::Proc::READ, results),
        ) else {
            return;
        };
        session.calls[at].status = Some(match &answer {
            Ok(_) => Status::OK,
            Err(status) => status.status(),
        });
        let Ok(read) = answer else { return };
        session.calls[at].payload = Some(read.data.len());
        let reach = u64::from(args.offset).saturating_add(read.data.len() as u64);
        let known = session
            .paths
            .get(&args.handle.key())
            .and_then(|path| session.nodes.get(path).copied().flatten());
        if read.data.len() < args.count as usize && known.is_some_and(|size| reach < size) {
            census.short_mid_file += 1;
            session
                .shortfall
                .insert(args.handle.key(), args.offset + read.data.len() as u32);
        }
        // A read proves the file is at least this long, which matters when
        // a capture starts after the LOOKUP that would have said so.
        if let Some(path) = session.paths.get(&args.handle.key()).cloned() {
            let reach = u64::from(args.offset).saturating_add(read.data.len() as u64);
            merge(&mut session.nodes, path, Some(reach));
        }
    }
}

/// Record a node, keeping the largest size seen and never turning a directory
/// into a file.
fn merge(nodes: &mut BTreeMap<String, Option<u64>>, path: String, size: Option<u64>) {
    match nodes.entry(path) {
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(size);
        }
        std::collections::btree_map::Entry::Occupied(mut slot) => {
            let merged = match (*slot.get(), size) {
                (None, _) | (_, None) => None,
                (Some(a), Some(b)) => Some(a.max(b)),
            };
            slot.insert(merged);
        }
    }
}

/// Write the reconstructed tree out as sparse files and mount it.
///
/// Sparse because a session may reference 75 MB of audio and the sweep cares
/// about lengths, not samples: what the bytes are is pinned byte-for-byte by the
/// streaming test in `serve::nfs`.
fn build_tree(session: &Session, root: &Path) -> Vfs {
    let _ = std::fs::remove_dir_all(root);
    for (path, size) in &session.nodes {
        let on_disk = root.join(path.trim_start_matches('/'));
        match size {
            None => {
                let _ = std::fs::create_dir_all(&on_disk);
            }
            Some(size) => {
                if let Some(parent) = on_disk.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Ok(file) = std::fs::File::create(&on_disk) {
                    let _ = file.set_len(*size);
                }
            }
        }
    }
    let mut vfs = Vfs::new();
    for prefix in ["B", "C"] {
        let subtree = root.join(prefix);
        if subtree.is_dir() {
            let _ = vfs.mount(prefix, &subtree);
        }
    }
    vfs
}

/// Our handle for a reconstructed path, found the way a client finds one.
///
/// Walks the tree component by component rather than hashing the path, because
/// a case-insensitive filesystem may have folded two spellings into one and the
/// handle has to be the one for the name **as stored** (O6).
fn walk(vfs: &Vfs, path: &str) -> Option<FileHandle> {
    let mut handle = Vfs::handle_for("/");
    for component in path.split('/').filter(|part| !part.is_empty()) {
        handle = vfs.lookup(handle, component)?.0;
    }
    Some(handle)
}

/// Re-aim a captured call at our tree: our twelve bytes, the deck's twenty.
fn re_aim(datagram: &[u8], ours: &BTreeMap<FileHandleKey, FileHandle>) -> Option<Vec<u8>> {
    let call = Call::parse(datagram).ok()?;
    if call.program != Program::NFS {
        return Some(datagram.to_vec());
    }
    let request = nfs2::Request::parse(nfs2::Proc(call.procedure), call.arguments).ok()?;
    let graft = |theirs: FileHandle| -> Option<FileHandle> {
        let mut bytes = ours.get(&theirs.key())?.0;
        bytes[FileHandle::KEY_LEN..].copy_from_slice(&theirs.0[FileHandle::KEY_LEN..]);
        Some(FileHandle(bytes))
    };
    let request = match request {
        nfs2::Request::Lookup { dir, name } => nfs2::Request::Lookup {
            dir: graft(dir)?,
            name,
        },
        nfs2::Request::GetAttr(handle) => nfs2::Request::GetAttr(graft(handle)?),
        nfs2::Request::StatFs(handle) => nfs2::Request::StatFs(graft(handle)?),
        nfs2::Request::Read(args) => nfs2::Request::Read(ReadArgs {
            handle: graft(args.handle)?,
            ..args
        }),
        nfs2::Request::ReadDir(args) => nfs2::Request::ReadDir(nfs2::ReadDirArgs {
            handle: graft(args.handle)?,
            ..args
        }),
        other => other,
    };
    let arguments = request.encode_arguments();
    Some(
        Call::new(
            call.xid,
            call.program,
            call.version,
            call.procedure,
            call.credential,
            &arguments,
        )
        .encode(),
    )
}

fn port_for(ports: Ports, program: Program) -> u16 {
    match program {
        Program::PORTMAP => ports.portmap,
        Program::MOUNT => ports.mount,
        _ => ports.nfs,
    }
}

/// Replay one session and check every answer against the captured one.
async fn replay(
    session: &Session,
    vfs: &Arc<RwLock<Vfs>>,
    server: &NfsServer,
    census: &mut Census,
) {
    let client = UdpSocket::bind("127.0.0.1:0").await.expect("a client");
    // Resolved before the first await: the tree's guard is not held across one.
    let ours: BTreeMap<FileHandleKey, FileHandle> = {
        let tree = vfs.read().expect("the tree");
        session
            .paths
            .iter()
            .filter_map(|(key, path)| walk(&tree, path).map(|handle| (*key, handle)))
            .collect()
    };
    census.tree_entries += ours.len();

    for replayed in &session.calls {
        // A version we do not speak is answered `PROG_MISMATCH`, which is
        // correct but not what this sweep is measuring.
        let expected_version = match replayed.program {
            Program::PORTMAP => portmap::VERSION,
            Program::MOUNT => mount::VERSION,
            _ => nfs2::VERSION,
        };
        if replayed.version != expected_version {
            continue;
        }
        let Some(datagram) = re_aim(&replayed.datagram, &ours) else {
            // A handle from a tree we could not reconstruct — a capture that
            // began mid-session, or a reply that never arrived.
            census.unmapped += 1;
            continue;
        };

        let port = port_for(server.ports(), replayed.program);
        let reply = round_trip(&client, port, &datagram).await;
        census.replayed += 1;
        let parsed = Reply::parse(&reply).expect("every call gets a well-formed reply");
        let results = parsed
            .results()
            .unwrap_or_else(|| panic!("{} was not accepted", describe(replayed)))
            .to_vec();

        match replayed.program {
            Program::PORTMAP => check_portmap(replayed, &results, server.ports()),
            Program::MOUNT => check_mount(replayed, &results, census),
            _ => check_nfs(replayed, &results, census),
        }
    }
}

fn describe(replayed: &Replayed) -> String {
    // The name a LOOKUP asked for, where there is one. Without it a failure
    // says only that some lookup disagreed, which is not enough to tell a
    // fixture problem from a server one — and the difference showed up the
    // first time this ran on a case-sensitive filesystem.
    let named = nfs2::Request::parse(nfs2::Proc(replayed.procedure), datagram_body(replayed))
        .ok()
        .and_then(|request| match request {
            nfs2::Request::Lookup { name, .. } => Some(name),
            _ => None,
        });
    match named {
        Some(name) => format!(
            "{} {name:?}",
            procedure_name(replayed.program, replayed.procedure)
        ),
        None => procedure_name(replayed.program, replayed.procedure),
    }
}

/// The call arguments inside a replayed datagram.
fn datagram_body(replayed: &Replayed) -> &[u8] {
    // The RPC header is fixed-width for a call with AUTH_NULL, which is what a
    // deck sends; anything else simply fails to parse and the name is omitted.
    replayed.datagram.get(40..).unwrap_or_default()
}

fn check_portmap(replayed: &Replayed, results: &[u8], ports: Ports) {
    let procedure = portmap::Proc(replayed.procedure);
    let Ok(answer) = portmap::Response::parse(procedure, results) else {
        panic!("our portmap reply must decode: {}", describe(replayed));
    };
    // The captured answer named the reference server's ports; ours must name
    // ours, and must never be zero for a program we run.
    if let (portmap::Response::GetPort(port), Ok(portmap::Request::GetPort(asked))) = (
        &answer,
        portmap::Request::parse(
            procedure,
            Call::parse(&replayed.datagram).unwrap().arguments,
        ),
    ) {
        let expected = match (asked.program, asked.version) {
            (Program::MOUNT, 1) => Some(ports.mount),
            (Program::NFS, 2) => Some(ports.nfs),
            (Program::PORTMAP, 2) => Some(ports.portmap),
            _ => None,
        }
        .filter(|_| asked.protocol == prolink_proto::rpc::IpProtocol::UDP);
        assert_eq!(*port, expected, "GETPORT for {:?}", asked.program);
    }
}

fn check_mount(replayed: &Replayed, results: &[u8], census: &mut Census) {
    let procedure = mount::Proc(replayed.procedure);
    let Ok(answer) = mount::Response::parse(procedure, results) else {
        panic!("our MOUNT reply must decode: {}", describe(replayed));
    };
    let (mount::Response::Mnt(ours), Some(theirs)) = (&answer, replayed.status) else {
        return;
    };
    let ours = match ours {
        Ok(_) => Status::OK,
        Err(status) => status.status(),
    };
    if agrees(ours, theirs, census) {
        assert_eq!(
            ours, theirs,
            "MNT: the captured server said {theirs}, we said {ours}",
        );
    }
}

/// Whether the captured answer is one we are entitled to be held to.
///
/// A success is: every call the real medium satisfied, ours must satisfy, and
/// that is the whole point of the sweep. A *failure* is not, because the tree
/// here is rebuilt out of the replies that succeeded — so a handle the captured
/// server had forgotten (it was restarted mid-session, and one of these
/// sessions restarts it) is one we still know, and a name it never resolved is
/// one our tree never had. Those disagreements are artefacts of reconstruction
/// rather than of the protocol, so they are counted and printed instead.
fn agrees(ours: Status, theirs: Status, census: &mut Census) -> bool {
    if theirs == Status::OK {
        census.compared += 1;
        return true;
    }
    if ours == theirs {
        census.errors_agreed += 1;
    } else {
        *census
            .errors_differed
            .entry((theirs.0, ours.0))
            .or_default() += 1;
    }
    false
}

fn check_nfs(replayed: &Replayed, results: &[u8], census: &mut Census) {
    let procedure = nfs2::Proc(replayed.procedure);
    let Ok(answer) = nfs2::Response::parse(procedure, results) else {
        panic!("our NFS reply must decode: {}", describe(replayed));
    };
    let Some(theirs) = replayed.status else {
        return;
    };
    if !agrees(answer.status(), theirs, census) {
        return;
    }
    assert_eq!(
        answer.status(),
        theirs,
        "{}: the captured server said {theirs}, we said {}",
        describe(replayed),
        answer.status(),
    );

    match &answer {
        // The two fields that are load-bearing: `ftype` decides whether a deck
        // treats it as a folder, and `size` decides how many reads it issues
        // and when it stops. The rest of the `fattr` legitimately differs
        // between a deck and the reference server, and is pinned word for word
        // against the reference in `serve::nfs`.
        nfs2::Response::Attr(Ok(attr)) => {
            if let Some((ftype, size)) = replayed.attributes {
                assert_eq!((attr.ftype, attr.size), (ftype, size), "GETATTR attributes");
            }
        }
        nfs2::Response::Lookup(Ok(found)) => {
            if let Some((ftype, size)) = replayed.attributes {
                assert_eq!(
                    (found.attr.ftype, found.attr.size),
                    (ftype, size),
                    "LOOKUP attributes",
                );
            }
        }
        nfs2::Response::Read(Ok(read)) => {
            if let Some(payload) = replayed.payload {
                // Deck to deck a serving player answers a 28584-byte request in
                // full. We cap at 8192, because macOS will not send a datagram
                // past 9216 bytes and an unsendable reply is a stall where a
                // short read is an ordinary "ask for the rest". Every read that
                // costs is counted and printed, so the cap cannot hide.
                let expected = payload.min(8192);
                if payload > expected {
                    census.shortened += 1;
                    census.largest_shortened = census.largest_shortened.max(payload);
                }
                assert_eq!(
                    read.data.len(),
                    expected,
                    "READ: the captured server returned {payload} bytes and we returned {}",
                    read.data.len(),
                );
            }
        }
        _ => {}
    }
}

/// Every call in the corpus, through our servers, compared with the answers the
/// devices actually got.
#[tokio::test(flavor = "multi_thread")]
async fn every_captured_call_is_answered_as_the_real_server_answered_it() {
    let Some(corpus) = Corpus::locate() else {
        eprintln!(
            "skipping: no capture corpus. Set {} to a directory of pcap files.",
            prolink_capture::CORPUS_ENV
        );
        return;
    };
    let mut census = Census::default();
    let scratch = std::env::temp_dir().join(format!("prolink-nfs-corpus-{}", std::process::id()));

    for (index, path) in corpus.captures().into_iter().enumerate() {
        census.captures += 1;
        let session = read_session(&path, &mut census);
        if session.calls.is_empty() {
            continue;
        }
        census.sessions += 1;

        let root = scratch.join(index.to_string());
        let vfs = Arc::new(RwLock::new(build_tree(&session, &root)));
        let server = NfsServer::start(Arc::clone(&vfs), ephemeral())
            .await
            .expect("ephemeral ports need no privileges");
        replay(&session, &vfs, &server, &mut census).await;
        drop(server);
        let _ = std::fs::remove_dir_all(&root);
    }
    let _ = std::fs::remove_dir_all(&scratch);

    eprintln!("corpus: {}", corpus.root().display());
    eprintln!(
        "{} captures, {} with RPC traffic, {} tree entries reconstructed",
        census.captures, census.sessions, census.tree_entries,
    );
    for (procedure, count) in &census.calls {
        eprintln!("  {procedure:>16}: {count}");
    }
    eprintln!(
        "  replayed {} calls, {} answers compared with the captured one",
        census.replayed, census.compared,
    );
    eprintln!(
        "  {} calls skipped for a handle from a tree we could not rebuild, {} names or parents we could not place",
        census.unmapped, census.unplaced,
    );
    eprintln!("  {} distinct LOOKUP names", census.names.len());
    eprintln!(
        "  {} reads a real server answered in full that we answer short, largest {} bytes",
        census.shortened, census.largest_shortened,
    );
    eprintln!(
        "  {} captured failures reproduced exactly; {} differ (rebuilt tree, not protocol):",
        census.errors_agreed,
        census.errors_differed.values().sum::<u64>(),
    );
    for ((theirs, ours), count) in &census.errors_differed {
        eprintln!(
            "    they said {:?}, we said {:?}: {count}",
            Status(*theirs),
            Status(*ours),
        );
    }
    eprintln!(
        "  {} RPC-level failures in the captures",
        census.rpc_failures
    );
    eprintln!(
        "  {} distinct read sizes, largest {}; those asked for 50 times or more:",
        census.read_sizes.len(),
        census.largest_read,
    );
    for (size, count) in &census.read_sizes {
        if *count >= 50 {
            eprintln!("    {size:>6}: {count}");
        }
    }
    eprintln!(
        "  a real server answered short of the request {} times mid-file, and the next read of \
         that file resumed at exactly the shortfall {} times",
        census.short_mid_file, census.resumed_after_short,
    );

    assert!(census.sessions >= 8, "the corpus has serve sessions in it");
    assert!(
        census.replayed >= 1000,
        "only {} calls replayed; the corpus should hold tens of thousands",
        census.replayed,
    );
    assert!(
        census.compared * 20 >= census.replayed,
        "only {} of {} replayed calls could be compared with a captured answer",
        census.compared,
        census.replayed,
    );
}
