# 014 — Consuming: the NFSv2 and dbserver clients

`crates/prolink/src/consume/{nfs,dbclient}.rs`. The two halves of seeing and
playing what is on somebody else's CDJ, and one string that joins them.

## What the two are for, and why they are separate

NFS makes a medium's **files** readable; dbserver makes the medium
**browsable**. They are independent — a peer whose every byte we can read still
shows nothing without dbserver, and a peer we can browse perfectly still plays
nothing without NFS — and only one of them is passive.

| | `consume::nfs` | `consume::dbclient` |
|---|---|---|
| Transport | UDP, three programs | TCP, one connection |
| Needs an announcement | no (F11, F12) | **yes**, device 1–4 (F45) |
| Gets you | `export.pdb`, audio bytes | menus, metadata, artwork, analysis |
| Failure of the other half looks like | a deck that lists tracks and will not play them | a deck whose files are all there and whose screen is empty |

A track load uses both, in this order:

```
DbClient::track_info(slot, id)  ─►  "/Contents/…/track.mp3", 7 633 531 bytes
NfsClient::mount_slot(slot)     ─►  the root filehandle
NfsClient::open(&mount, path)   ─►  a handle, and the size again from LOOKUP
NfsClient::read_range(…)        ─►  audio
```

## Public API

```
consume::nfs
  ReadSize            CDJ = 8192, UNFRAGMENTED = 1280, new(1..=8192)
  NfsConfig           read_size, timeout, attempts, portmap_port
  RpcPorts            mount, nfs — discovered, not assumed
  NfsClient           connect / connect_with, peer, ports, config, stats,
                      set_read_size, forget_directories, refresh,
                      dump, exports, mount, mount_slot, unmount,
                      attributes, lookup, walk, open,
                      read_at, read_range, read_file, read_file_with
  Mount               export, slot, root            (proof that MNT succeeded)
  RemoteFile          path, handle, size            (proof of a regular file)
  Progress            read, total, fraction
  NfsStats            datagrams, retries, timeouts, lookups, reads, bytes
  EXPORT_PDB          "/PIONEER/rekordbox/export.pdb"

consume::dbclient
  DbConfig            connect_timeout, request_timeout, batch
  Analysis            WaveformPreview | WaveformDetail | BeatGrid
                      | CuePoints | ExtendedCuePoints | VbrIndex
  DbClient            query_port / query_port_at, connect / connect_at,
                      peer, port, device, server, is_desynchronised,
                      descriptor, close,
                      root_menu, sort_menu, category, tracks, playlist,
                      drill, folder, search,
                      metadata, track_info, artwork, analysis,
                      menu, request
  TrackMetadata       the named items, plus the raw rows
  TrackInfo           path, size, container, duration, tempo, comment
  NOT_FOUND           0xFFFFFFFF — "no such thing", not a refusal
  ROOT_MENU_MASK      0x00FFFFFF — argument 2 of every MENU_ROOT
```

Two new `Error` variants, added additively to `error.rs`: `Error::Nfs`
(operation, path, `ErrorStatus`) and `Error::Refused` (what, detail). The first
exists because the *status* is what a caller acts on — `ACCES` means announce,
`NOENT` means try another spelling, `STALE` means re-mount. The second is "the
peer is speaking the protocol and saying no", which is a different thing from
`Error::Protocol`, i.e. "these bytes did not decode".

## Where types replaced comments

- **`ReadSize`** proves `1..=8192` once, at construction. A read size wider than
  the count field is unwritable, and the two named constants carry the
  fragmentation trade-off in their doc comments rather than in a wiki.
- **`Mount`** carries the root filehandle, so walking a path on an export that
  was never mounted cannot be written.
- **`RemoteFile`** exists only for something a `LOOKUP` reported as
  `NFREG`, and it carries the size. That is load-bearing: a `READ` reply from a
  real CDJ has an all-zero `fattr` (7884 of 7884), so the size *has* to come
  from elsewhere and this type is the elsewhere.
- **`DbClient::connect`** takes a `BrowsableDeviceNumber`. Announcing is a
  precondition of dbserver and the observer number a passive listener takes
  cannot be borrowed for a session — that is now a compile error rather than a
  refusal at run time (F45).
- **`TrackInfo::from_items`** returns `Option`: no path item means the track
  cannot be loaded, and a struct with an empty path would push that check onto
  everyone downstream.
- **`NfsConfig::attempts`** is a `NonZeroU8`. Zero attempts is not a policy.

## Findings each tricky part answers

| Piece | Finding |
|---|---|
| `GETPORT` for mountd then nfsd, discovered not assumed | F6, F10, F46 |
| A failed `GETPORT` falls back to `DUMP` to say *which* failure it was | portmap module doc |
| `MNT` via `EXPORT` first, documented path second | C6 (`/C/EXPORT`), F37 |
| `ACCES` on `MNT` surfaces as a status, not as fatal | F12 |
| `UMNT` on release | C9, F37 |
| Filehandles echoed back byte for byte | F28, from the client side |
| Directory-handle cache; one `LOOKUP` per file, not four | the C++ port's 495/576 `STALE` failures |
| Read size 8192 by default, 1280 available | F19 |
| A `READ` reply's `fattr` is ignored entirely | `nfs2::FileData::attr`, 7884 of 7884 |
| A short read is re-requested, never taken for EOF | RFC 1094; the C++ port counted and did not re-request |
| `NOENT` retried through case and NFC/NFD spellings | O6 |
| Credential stamp walks the boot sequence | `rpc::STAMP_SEQUENCE`, C8's correction corrected |
| dbserver: preamble both ways before any message | §5, `skip_preamble` |
| `INTRODUCE` reply's argument 1 is the *server's* number | F7 |
| Transaction ids from `0x03800001` | C10 |
| Render echoes the whole-result-set count, not the page | F27, F41 |
| `MENU_CLOSE` reuses the render's id and draws no reply | F16 |
| Count `0xFFFFFFFF` is "not found", and empty ≠ error | F40's user-visible surface |
| Drill grid `0x1000 \| depth << 8 \| category` | F42, F44 |
| Search: argument 2 is a byte length, argument 3 is the text | F44 |
| Metadata read by item type, masked | F32, F35, CDJ-3000 high half |
| Track info: file size in argument 0 of the path item | F31 |
| `0x04` is the title in metadata and the container in track info | F35 |
| Artwork: an omitted blob is a success | the omitted-blob rule |
| Waveform preview: track id at argument 2, trailing blob omitted | §5.11 |
| We never send `0x3e03` | F25, from the other side |

## Decisions worth arguing with

**One call in flight, not four.** Both reference clients pipeline a window of
four reads. This one is strictly sequential, and the arithmetic is the
justification: their measured 1459 KiB/s came from four **1280-byte** reads in
flight, and one **8192-byte** read moves 6.4× the payload per round trip. So
sequential-at-8192 beats windowed-at-1280 on the same link while keeping "a
reply with another xid answers a call we abandoned" small enough to be obviously
right. Windowing on top of 8192 would multiply again and is the first thing to
reach for if a 75 MB track ever feels slow. Not measured against hardware.

**8192 is the default, not 1280.** The reference defaults to 1280 to stay inside
a 1500-byte MTU. But a real deck asks for 8192 and relies on IP fragmentation in
*both* directions, so a link that cannot carry it cannot carry a real track load
either — and 6.4× the round trips is a real cost. `ReadSize::UNFRAGMENTED` is
one line away for a network that is dropping fragments.

**Timeouts split the difference.** Python retries 6 times at 2 s (≈12 s to
notice a dead peer); the C++ port retries 8 times at 250 ms (≈2 s, and it
resends *every* pending call on every tick). Neither measured it. This uses
500 ms × 4 attempts, per-call, retransmitting only the call that is overdue. The
dbserver side uses 10 s per request, which is the C++ number and is deliberate:
a player answers these off the same processor that is decoding audio.

**`MENU_CLOSE` is sent once per menu, after the last page.** A real deck sends
it 23 times in one browse and not after every render; sending one per menu is a
superset of that and cannot destroy a result set we have finished paging. It is
sent with `write_all` and never awaited.

**Stale recovery is explicit, not automatic.** The directory-handle cache is
what *prevents* the `STALE` storm; for the residual case — a swapped medium —
`NfsClient::refresh` re-mounts and forgets every handle, and the caller decides.
Automating it would mean `walk` taking `&mut Mount`, which spreads mutability
through an otherwise read-only API for a case the cache already removes.

**We do not send `0x3e03`.** A player fires it after `INTRODUCE` only when the
thing it is browsing is a *foreign* device; deck-to-deck it never appears. As a
client against a real player we are the deck, so sending it would be a message
no CDJ ever sends. `serve::dbserver` still has to answer it (F25).

## How this was tested

No hardware, so two loopback servers built out of the codec crate's own
**reply builders**, which is how the reference project tested its NFS client and
how it caught real bugs.

- **A loopback deck** on three ephemeral UDP sockets — one per RPC program — so
  port discovery is genuinely exercised: a client that assumed 48276 and 2049
  would be talking to nothing. It rejects any filehandle it did not itself
  serve, which is how "we echo handles verbatim" is pinned. Configurable
  misbehaviour: drop every *n*th datagram, refuse `MNT` with a status, cap a
  `READ` short, or say nothing at all.
- **A loopback player** on TCP, which logs every request it receives so a test
  can assert the *sequence* a browse produces and not merely its result.

27 tests on the NFS side and 36 on the dbserver side. The ones that would have
caught a real bug:

- a 1 077 760-byte pull comes back byte for byte in exactly **842** reads at
  1280 and 132 at 8192, with monotonic progress;
- a 37-byte cap on every `READ` still assembles the whole file;
- a `READ` reply whose `fattr` is all zeros does not end the transfer;
- one datagram in three dropped, and the pull still completes with
  `retries > 0` and `timeouts == 0`;
- a deaf peer times out after exactly `attempts` datagrams;
- a stale reply under an old xid is discarded and the retransmission is what
  gets answered;
- twelve files in one folder cost **15** lookups, not 48;
- `Gesaffelstein` finds `GESAFFELSTEIN`, and NFC finds NFD;
- 150 rows page in three renders, every render carrying `150` as the total;
- one `MENU_CLOSE` per menu, under the last render's id, and the session is
  still in step afterwards;
- `0xFFFFFFFF` yields an empty list and issues no render at all;
- a track with no art yields an empty `Vec` and no error.

**Committed fixture floor.** Ten tests assert our encoder against the exact
bytes two CDJ-2000NXS put on the wire, extracted from `S06-load-and-play` and
`S20-browse-ground-truth` by reassembling the dbserver streams: the root menu,
a render, `MENU_CLOSE`, `GET_METADATA`, `GET_TRACK_INFO`, `GET_ARTWORK`, the
waveform preview with its omitted blob, the drill grid at depths 1/2/3, the sort
menu, the search, and the flat track list. On the RPC side: a `GETPORT` is 76
bytes, `MNT '/C/'` is `00000006 2f0043002f00 0000`, and a `READ` call is always
104. These are literals rather than corpus replay because `prolink` has no
`prolink-capture` dev-dependency and adding one was out of scope for this agent.

## Untested against hardware

Everything. There are no CDJs on this machine, and nothing below has met one.

- **Throughput.** The 1459 KiB/s figure is the reference's, on a window of four
  1280-byte reads. Sequential-at-8192 is an argument, not a measurement.
- **IP fragmentation.** The default read size relies on the host stack
  reassembling ~8.3 KB replies. Real decks do it; this code has only ever seen
  loopback, where nothing fragments.
- **`NFSERR_ACCES` on `MNT`.** The announce-then-retry path is documented and
  surfaced as a distinct status; it has never been exercised, because the device
  that would refuse us is in somebody else's capture.
- **The handle table.** The directory cache is modelled on the C++ port's
  measurements (2300 handles minted, 495/576 failures, a real deck's four
  handles across forty-eight lookups). Our lookup count is asserted; the
  player's tolerance for it is not.
- **`GET_CUE_POINTS_EXT` and `MENU_FOLDER`** are built from the documented
  shapes; no capture in the corpus shows a client sending either.
- **`sort_menu`'s menu-target byte.** Real decks used an undocumented `0x05`;
  we send `MAIN`, on the reading that the target says where an answer is
  *displayed*. If a player ever refuses a sort menu, that is the first thing to
  change.
- **Analysis blobs come back undecoded.** `prolink_proto::analysis` implements
  file → wire for the serving side; the inverse does not exist, so a consumer
  gets the player's bytes and has to know what they mean.
- **The `0x3d03` and `0x3100` requests** are never sent by this client, and it
  has no handling for a player that sends *us* something unsolicited on a
  session we opened — it would be logged and ignored inside a render, and would
  desynchronise us anywhere else.
