# 018 — the NFS server: portmapper, mountd, nfsd

The file-serving half of objective 2. `serve/nfs.rs` (sockets, ports,
lifecycle) plus `serve/nfs/answer.rs` (one call in, one reply out). Nothing else
in the crate was touched but one additive variant on `Error`, noted at the
bottom.

## Shape

Two layers, split on whether a socket is involved.

`answer.rs` is synchronous and has no I/O: a `Dispatcher` holds the shared
`Arc<RwLock<Vfs>>`, the three ports and the mount registry, and
`Dispatcher::answer(service, datagram, peer) -> Option<Vec<u8>>` turns one
datagram into one reply. `None` means "say nothing", which is the answer for a
stray that is not an RPC call. That makes the entire serving surface — every
status a player can be told, every handle it can send back — testable from a
byte literal, which is where all but five of the tests are.

`nfs.rs` binds the sockets and does nothing else: one task per socket receiving,
one task per datagram answering. The answer runs on `spawn_blocking`, because a
`READ` off a USB stick is a disk seek and the same runtime is emitting the
200 ms status packets that keep us on the network; parking a worker mid-`recv`
would be an audio dropout on someone's deck. In-flight work is capped at 64 and
excess datagrams are dropped rather than queued — on UDP that is a retransmit,
and the alternative is unbounded memory for whoever is on the link.

`Service` (Portmap / Mount / Nfs) is fixed when a socket is bound, so a call for
the wrong program is `PROG_UNAVAIL` and the right program at the wrong version
is `PROG_MISMATCH` carrying the range. Both are real answers.

## The public API

```rust
let vfs = Arc::new(RwLock::new(Vfs::new()));
vfs.write()?.mount("C", Path::new("/Volumes/REKORDBOX"))?;   // a USB stick
let server = NfsServer::start(Arc::clone(&vfs), NfsConfig {
    interface: Some(interface), ..NfsConfig::default()
}).await?;                       // 111, 48276, 2049 by default
assert!(server.ports().is_discoverable());
```

`NfsServer::ports() -> Ports { portmap, mount, nfs }` says where the three
ended up, `Ports::is_discoverable()` whether real hardware can find them, and
`NfsServer::mounts() -> Vec<Mount>` who currently holds what. Dropping the
server stops all three.

**There is no export table.** An export is served exactly when its subtree is in
the `Vfs`, so inserting a stick is a `Vfs::mount` and nothing else, and a `MNT`
of an empty slot answers `NFSERR_NOENT` by construction rather than by
remembering to keep a list in step. `ServedSlot` supplies both halves — the
export path a player names and the subtree it maps to — so the two cannot
disagree. The tree is behind an `RwLock` for exactly this: media come and go
while the server runs.

## Findings each piece answers

| Piece | Finding |
|---|---|
| Portmapper mandatory, bound last, hard failure with a remedy | F46 |
| Ports 111 / 48276 / 2049, and the `DUMP` table | F6, F10 |
| `/B/` SD, `/C/` USB, matched on the **prefix** | F37, F12, C6 |
| `MNT` returns the medium's subtree, not the tree root | F28 |
| Handles resolved on their leading twelve bytes | F28 |
| `fileid` is the handle's leading word | 8285 of 8285 replies |
| `UMNT` per slot after an eject; `EXPORT` never called | C9, F37 |
| UTF-16LE path beside ASCII groups in one `EXPORT` reply | C7 |
| Exported to `169.254.0.0/255.255.0.0` | F11, F12 |
| Credentials parsed and not enforced | F11, F12 |
| 8192-byte reads, random access, low latency | F18, F19, F39 |
| Case- and normalisation-insensitive matching | O6 |
| 32-bit ceiling refused rather than clamped | RFC 1094 |

## Three decisions worth recording

### Duplicate requests: no cache, and the corpus says why

A deck that gets no answer resends the identical datagram — same `xid`, same
`AUTH_UNIX` stamp, once a second. `S24c-e9-noportmap` is 31 calls of which 29
are retransmissions of two that nothing answered. Across the eight sessions
where a server *did* answer, **45 763 calls carry 45 763 distinct `xid`s: not one
retransmission in the corpus.**

A duplicate-request cache exists in general NFS servers to stop a retried
`WRITE` or `REMOVE` being applied twice. Every procedure here is idempotent,
because a rekordbox export is read-only, so re-running a retransmitted call
returns the same bytes; the cost of doing so is a `BTreeMap` lookup and at worst
an 8 KiB read. A cache would add memory, a lifetime policy, and a way to answer
from a medium that has since been ejected. So: none, and
`a_retransmitted_call_is_answered_identically` pins the property that makes that
safe.

### Privileged ports: degrade loudly

Order is nfsd, then mountd, then the portmapper — the two that cannot fail for
want of privilege first, so a host that has neither free is discovered cheaply.
mountd and nfsd fall back to an ephemeral port if their usual number is taken (a
real `rpcbind` or `nfsd` may hold it) and the portmapper publishes whatever they
got, which is what a portmapper is for.

UDP/111 gets no fallback. A portmapper anywhere else is one no player will ever
ask, so moving it quietly would turn a loud failure into a silent one; it is
`Error::PrivilegedPort`, whose message carries the platform's remedy —
`net.ipv4.ip_unprivileged_port_start=111` on Linux, "run as root" on macOS,
which has no equivalent setting. A caller that deliberately asks for another
port (tests, and experiment E9) gets a `warn!` at startup and a
`Ports::is_discoverable()` of false.

One caveat, inherited from `crate::socket::bind`: `SO_REUSEADDR`/`SO_REUSEPORT`
are set on every socket, so binding 2049 or 111 can *succeed* beside a real
`nfsd` or `rpcbind` and then the two split the datagrams between them. Nothing
here can detect that; on a host running either, pass explicit ports.

### Attributes: the reference server's, byte for byte, except `fileid`

`S10j-serve-to-cdj` is a whole load and thirty seconds of playback with zero
errors, and the server in it answered `LOOKUP`, `GETATTR` **and `READ`** with a
complete `fattr`: mode `0o40755`/`0o100644`, `fsid` 1, `rdev` 0, and 2020-09-13
in all three timestamps. That is exactly what `Vfs::attributes` synthesises, so
that is what we send, and `our_attributes_are_the_ones_a_deck_played_from_word_for_word`
asserts our seventeen words against the two captured `fattr`s from that session.

It is deliberately *not* what a real deck sends. A deck fills a `READ` reply's
`fattr` with zeroes but for the `fileid` (7884 of 7884), and elsewhere uses mode
`0o100000`, `rdev` 1, `fsid` 2 and 2023-01-01. Reproducing that would mean
removing correct information from a reply already known to work in this exact
role, and the codec's own documentation tells clients not to read a `READ`
reply's attributes anyway. The one field where hardware and the reference
disagreed is `fileid` — the reference used a counter, a deck uses the handle's
leading word in 8285 of 8285 replies — and there we follow the hardware, which
`Vfs::attributes` already does.

The other departure from `Vfs::attributes` is the 4 GiB ceiling. `fattr.size` is
32 bits and the helper clamps; a file reported as 4 GiB minus one byte would be
read to that point and no further, which presents as a truncated track rather
than as an error. So a node larger than `MAX_FILE_SIZE` is `NFSERR_FBIG` from
`GETATTR`, `LOOKUP` and `READ` alike.

## Sizes, and the one number that is derived rather than observed

A `READ` is answered up to `MAX_READ_PAYLOAD - 100` bytes — what one UDP
datagram can carry, less the RPC header, status word, `fattr` and length prefix.
RFC 1094's ceiling is 8192 and every read a deck has ever sent *us* is 8192 or
less (160, 2048 and 8192 in `S10j`), but deck-to-deck the modal request is 9408
and a file's first read can be 28584, answered in full. So the bound is what the
wire can carry rather than what the specification says, and a server answering
short is normal in either direction.

`READDIR` spends its `count` as a byte budget and mints its cookies as indices,
which is what they are for; it always returns at least one entry, so a client
with a small budget cannot loop for ever. `STATFS` is RFC 1094 as written with
the reference implementation's numbers: no capture in the corpus contains one in
either direction.

`LOOKUP` answers `.` and `..`, which no deck has ever asked for — it walks paths
it read out of `export.pdb` — because a client with no database cannot navigate
without them.

## How it was tested

45 tests, no hardware, no capture corpus at run time.

**Real datagrams, committed as literals.** Eight calls a CDJ-2000NXS actually
sent, lifted out of `S10j-serve-to-cdj` (the zero-error serve session),
`S18-two-slots` and `S24c-e9-noportmap`: two `GETPORT`s, `MNT` of `/C/` and of
`/B/`, a `LOOKUP`, a `GETATTR`, the `READ` of the last 160 bytes of a 6.9 MB MP3
that F18 describes, and one of the 30 retransmissions. They are hex literals
rather than a corpus walk, because this crate has no dev-dependency on
`prolink-capture` and adding one would mean editing a manifest three other
agents are working around; the fixtures are therefore always present, which is
the "committed fixture floor" the conventions ask for.

Calls carrying a filehandle are *re-aimed* at our tree: the twelve bytes the
deck preserved are replaced with ours and **its own twenty rewritten bytes are
kept verbatim**, so the F28 path is exercised against the bytes a real deck
produced rather than against a stand-in.

**Over sockets**, on ephemeral ports so no test needs privileges or can collide:
`GETPORT` → `MNT` → three `LOOKUP`s → thirteen `READ`s reassembling a 100 KB
file byte for byte, with every handle's tail rewritten the way a CDJ rewrites
it. Plus: a medium grafted in after startup becomes mountable, a reply comes
back from the port the call went to, and a dropped server stops answering.

**Everything else** is at the `answer` layer: a read past the end (short, not an
error), a read of a directory (`ISDIR`), an unknown handle (`STALE`) against a
missing name (`NOENT`) against a lookup inside a file (`NOTDIR`), a name whose
case differs and one whose Unicode normalisation differs (both resolving to the
handle for the name *as stored*), a 5 GiB sparse file (`FBIG`), every write
procedure (`PROC_UNAVAIL`), a call on the wrong port (`PROG_UNAVAIL`), NFS v3
(`PROG_MISMATCH {2,2}`), undecodable arguments (`GARBAGE_ARGS`), a reply that
wandered in (no answer at all), and 6000 mutated and truncated datagrams that
must return rather than panic.

## What is untested against hardware

- **Everything.** No CDJ has seen this server; the whole module is validated
  against captures of the Python reference server answering a deck, and against
  the deck's own calls.
- **Binding 111 for real.** Every test binds ephemeral ports. The privileged
  path is exercised only by the error branch, and the `Error::PrivilegedPort`
  message has never been read by a user.
- **`READDIR`, `STATFS`, `UMNT`, `UMNTALL`, `DUMP` and `EXPORT`.** No deck calls
  any of them, so their shapes are RFC 1094 plus C7 and nothing has ever
  answered or consumed ours.
- **Two media at once over the wire.** `S18-two-slots` shows a deck mounting
  both, and the two subtrees are tested at the `answer` layer, but the socket
  test serves one.
- **Sustained playback.** The 75 MB streaming figure (F39) is the C++ port's;
  this one has streamed 100 KB in a test. Each `READ` reopens the file, as the
  reference did — a per-file descriptor cache is the obvious next optimisation
  if a real deck ever stalls.
- **The link-local source address.** Replies go out of a socket bound to
  `0.0.0.0` with the interface pinned; that is what `crate::socket` does for
  every other port and it works there, but it has not been watched on a
  multi-homed host here.

## Outside my files

One additive variant on `crates/prolink/src/error.rs`:

```rust
Error::PrivilegedPort { port: u16, source: std::io::Error, remedy: &'static str }
```

Nothing else in the crate was changed.
