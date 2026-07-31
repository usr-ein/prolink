# 005 — XDR, ONC RPC v2, portmap, MOUNT, NFSv2

The file-access layer, both directions, every procedure. `rpc.rs` plus
`rpc/{xdr,portmap,mount,nfs2}.rs`.

Raw evidence for everything below is in
[`005-proto-rpc-evidence.txt`](005-proto-rpc-evidence.txt): real captured
datagrams with field-by-field decodes, extracted from 33 `S*/run.pcap`, five
dysentery `.pcapng` and 26 journals — 113,900 RPC datagrams, 56,966 calls and
56,934 replies.

## Shape

Four halves per procedure, because we are both ends. A program module exposes a
`Request` enum (`parse` a call's arguments, `encode_arguments` to build one) and
a `Response` enum (`parse` a reply's results, `encode` to build one). `rpc.rs`
owns only the framing: `Call`, `Reply`, `Auth`, `AuthUnix`, `Program`.

`Reply` is two variants rather than a struct with optionals, because a denied
reply carries **no verifier** — the shapes genuinely differ. Inside an accepted
one, `Accepted` has exactly the three arms the wire union has: `Success` carries
results, `ProgMismatch` carries a version range, everything else carries
nothing. A caller cannot read results out of a failure.

Parsing borrows throughout. `Call::arguments` and a `READ` payload are slices
into the datagram, so decoding a call allocates nothing and an 8 KiB read is not
copied. `Request` owns its names (a few hundred bytes at most); `Response<'a>`
borrows, because the payload is the one field where it matters.

## `Utf16LeString`: the bytes are the field

The prefix counts **bytes**, so `/C/` is three characters and announces six.
Rather than pass `&str` around and hope, the type holds the wire bytes and
derives the `String`. Three consequences fall out for free: the
byte-versus-character mistake is unwritable; a name that is not valid UTF-16
round-trips instead of being normalised (O6's third bug); and what the hardware
*said* stays available beside our reading of it, which is what F12 needed.

`ascii_string` and `utf16le_string` are separate functions, never one with a
flag, because a single MOUNT `EXPORT` reply uses both (C7).

## Findings each piece answers

| Piece | Finding |
|---|---|
| UTF-16LE, byte-counted | F12 (`'/C/' raw=2f0043002f00`) |
| ASCII groups beside a UTF-16LE path | C7 |
| mountd 48276, nfsd 2049, portmapper 111 | F6, F10 |
| Portmapper mandatory; NFS precedes dbserver | F46 |
| `/B/` SD, `/C/` USB, prefix matching | F37, F12, C6 |
| A deck never calls `EXPORT`, but does call `UMNT` | F37, C9 |
| `FileHandle` / `FileHandleKey` split | F28 |
| 32-bit ceiling asserted, not wrapped | ksy, RFC 1094 |
| Streaming reads, latency not throughput | F18, F19, F39 |
| Name matching is the caller's, and is not byte equality | O6 |

## Four corrections the corpus forced

Each was checked against the captures directly before being written down.

### C8 is wrong: the stamp is not a nonce

C8 corrected the literature's "magic constant `0x967b8703`" to "a per-call
nonce, its value arbitrary". Neither survives the whole corpus. **The stamp is a
fixed sequence indexed by the number of RPC calls since power-on.**

9947 xids recur across two or more *separate captures*; every one carries the
same stamp both times, **zero disagreements**. Two physically different
CDJ-2000NXS units agree with each other, and both agree with dysentery's 2016
capture of other hardware on xids 1–9. A deck's xid is a boot-relative counter
starting at 1, so xid and call-index are the same thing. `0x967b8703` really is
a constant — it is entry one, which is why a client hard-coding it works.

C8's *practical* advice stands and is better grounded: a server must never
validate the stamp; a client may send anything. What changes is that a test may
now assert an exact value. `STAMP_SEQUENCE` holds the first forty entries, each
witnessed in two to four captures on two or three devices; `stamp_for_xid`
returns `None` past the table rather than extrapolating.

I re-derived this myself rather than taking it on report: the discriminating
test is agreement *across capture files*, since repeats within one file are just
retransmissions reusing the whole datagram.

### F19 understates read sizes; 8192 is not a ceiling

F19 records real CDJs using 8192-byte reads. Deck to deck across the corpus the
modal request is **9408** (5097 of 7043), then 8192 (1283), then 2048 (264) —
and a file's first read goes as high as **28584**, answered *in full* as one
28,656-byte datagram in ~20 IP fragments.

This was a live bug: capping the decoder at `MAX_DATA` would have rejected real
deck-to-deck replies. `MAX_READ_PAYLOAD` is now what a UDP datagram can
physically carry; `MAX_DATA` stays as the documented figure with a doc comment
saying which is which.

### A CDJ's `READ` reply carries an empty `fattr`

7884 of 7884 `READ` replies from a real deck have `type`, `mode`, `nlink`,
`uid`, `gid`, `size`, `blocksize`, `rdev`, `blocks`, `fsid` and all three
timestamps **zero**, with only `fileid` filled in. `LOOKUP` and `GETATTR`
replies carry a complete, correct one.

So a client must not read anything out of a `READ` reply's attributes, and in
particular **must not take `size == 0` for end of file**. Documented on
`FileData::attr`, where someone about to do it will see it.

### A real CDJ does not zero its XDR padding

`MNT('/C/')` ends `…2f00 0011`; `UMNT('/C/')` ends `…2f00 3cd2`; a `LOOKUP`
ends `…3300 8930`. Uninitialised bytes, not the zeroes RFC 4506 asks for. A
decoder must therefore skip padding rather than check it.

It also puts a seam in byte-exactness worth knowing about: a parsed `Call`
carries its argument block opaquely, so a captured datagram re-encodes
*exactly*, padding included — but re-encoding from a decoded `mount::Request`
writes the standard zeroes and differs in those bytes. Both are correct; a
round-trip test has to know which it is asserting. The test asserts both.

## `fattr` constants: reproduced, not invented

The reference implementation synthesised plausible POSIX values. A real deck
sends different ones, and CONVENTIONS §3 says to build from the captured
skeleton, so `Fattr::directory` and `Fattr::regular_file` now emit what a deck
emits, each constant carrying its sample count:

| Field | CDJ | Reference impl |
|---|---|---|
| file `mode` | `0o100000` — **no permission bits at all** | `0o100644` |
| dir `mode` | `0o040666` | `0o040755` |
| `nlink` | 1 for both | 2 / 1 |
| `rdev` | **1** | 0 |
| `fsid` | 2 | 1 |
| dir `size` / `blocks` | 0 / **1** | 0 / 0 |
| times | `1672531200`, hard-coded | real mtime |

A client that checks permission bits before reading refuses every track. Decks
plainly do not check, since they read from each other.

The reference values also worked against hardware (F39), so this field is not
load-bearing — but the observed value costs nothing, and the test
`a_real_getattr_reply_is_a_status_and_seventeen_words` now asserts that
`Fattr::regular_file` reproduces a captured CDJ `fattr` **field for field**,
which is a much stronger check than "it reads back".

`fileid` is the leading four bytes of the handle in 8285 of 8285 replies, so
`FileHandle::fileid()` exists and a server gets consistency for free.

## The filehandle

`FileHandle` is 32 bytes; `FileHandleKey` is the 12 a server may rely on. Two
types, so "I keyed the table correctly" is a property of the type rather than of
remembering to slice. `Debug` prints `<12 kept>|<20 rewritten>` so a log shows
the truncation.

The evidence is stronger than F28 records: the rewrite happens **deck to deck**
too (372 calls, no code of ours involved), and the surviving twelve bytes are
three 32-bit ids reading as `[self, parent, mount-root]` — the mount root's
leading byte was `01` for USB and `02` for SD on the same deck. Twelve bytes
survive because twelve bytes is a deck's entire idea of what a handle is.
Across the corpus, 3066 + 372 calls kept exactly the first twelve; **zero** kept
fewer.

## Tests

190 tests in the crate, of which ~120 are this layer. The `captured` module in
`rpc.rs` is the fixture floor: 22 whole datagrams off real hardware, decoded end
to end and re-encoded, covering portmap `NULL`/`GETPORT`/`DUMP`, `MNT` of `/C/`,
`/B/` and `/C/EXPORT`, `UMNT`, both flavours of `EXPORT` reply, `LOOKUP`
(ASCII, Japanese, and a failing one), `GETATTR`, and `READ` calls at 8192 and
28556. Where a datagram is marked *deck to deck*, both directions are genuine
Pioneer bytes.

No test invents bytes and calls them captured. Each hex literal is traceable to
a frame in the evidence file; the two I mis-transcribed were caught by the
tests, and I re-extracted both from the pcap myself.

## Not settled

- **`READDIR` and `STATFS` are unexercised.** Zero calls in 56,966 — a deck
  never issues either, and no capture shows one answered. Both are implemented
  from RFC 1094 as written and neither has ever met hardware. If a deck does
  call `READDIR`, the cookie and `eof` semantics are the most likely thing to be
  wrong.
- **MOUNT `DUMP`** likewise. Its hostname is ASCII and its directory UTF-16LE by
  inference from `EXPORT`, not from a capture. Marked as inference in the type's
  doc comment.
- **The stamp generator.** The mapping is measured; that it is a PRNG seeded at
  boot is a guess. Past the fortieth call we have no table.
- **`MSG_DENIED` and every non-`SUCCESS` `accept_stat`** are implemented from
  RFC 1057. The corpus contains not one of either, so the encoders are
  unvalidated against hardware — including `PROC_UNAVAIL`, which is what we
  would answer a write with.
- **Whether serving `mode 0o100000` is safe.** A deck serves it and other decks
  read it, but nothing has tested *us* serving it. If a track fails to load
  after this change and nothing else explains it, `Fattr::FILE_MODE` is the
  first thing to try at `0o100644`.
- **NUL-terminated names.** No captured name carries one, so the decoder does
  not strip a trailing NUL. The Mixxx C++ port truncates at the first NUL; if
  some firmware pads its names, that difference will show up here first.

## Outside my files

Nothing. No change to `error.rs` — `Truncated`, `Malformed` and
`ImplausibleLength` covered every case, and the distinction that mattered
(`is_truncated()` true for a short datagram, false for a hostile length prefix)
already existed.
