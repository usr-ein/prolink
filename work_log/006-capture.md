# 006 — prolink-capture

Reading Pro DJ Link traffic out of pcap and pcapng. Link and transport only:
nothing in this crate knows what a Pro DJ Link message is, and it does not
depend on `prolink-proto`.

## Public API

```
Capture<R>              iterator of Result<Packet>; ::open(path) / ::new(reader)
    ::format()          Format::Pcap | Format::PcapNg, decided by the magic
    ::udp_to(port)      -> UdpTo<R>: every datagram ADDRESSED TO that port
Packet                  index, timestamp, interface, source, destination,
                        transport, payload (IP-defragmented)
    ::udp_payload_to(port) -> Option<&[u8]>
Transport               Udp | Tcp { sequence, syn, fin, reset }
Flow                    one direction of one connection; ::of, ::reversed,
                        ::involves(port)
tcp::Reassembler        ::new() / ::on_ports(ports); ::push(&Packet); ::finish()
tcp::Stream             ::flow, ::first_seen, ::from_connection_start,
                        ::runs, ::contiguous -> Option<&[u8]>, ::gaps, ::len
Run { offset, data }    a stretch that arrived contiguously
Gap { offset, len }     a stretch that never arrived
Corpus                  ::locate() -> Option<Corpus>, ::at, ::root, ::captures
Error                   NotACapture | Truncated | Malformed | Io |
                        UnsupportedLinkType; ::is_truncated()
DISCOVERY_PORT / BEAT_PORT / STATUS_PORT
```

Files added, all under `crates/prolink-capture/`: `src/{lib,error,packet,
capture,sparse,tcp,corpus}.rs`, `tests/capture.rs`, `README.md`. **Nothing
outside the crate was touched** — not the workspace manifest, not `clippy.toml`,
not another crate.

## Design decisions worth defending

**The destination-port filter is the only filter offered.** `Capture::udp_to`
is destination-only and there is deliberately no source-port equivalent;
`Packet::source`'s doc comment says why. The measurement that makes the case is
in the docs and in a test: of the **35 103** datagrams in the corpus addressed
to 50002, only **24** were sent *from* 50002, because a CDJ sends each status
packet from a different, incrementing source port (S20 shows 4763, 4764,
4765 …). A source-port filter finds essentially nothing; an either-endpoint
filter is right until a second tool binds 50002.

**TCP is the opposite, and says so.** A connection has two ends and both
directions belong to the server's port, so `Reassembler::on_ports` matches
either endpoint. `Flow::involves`'s doc comment states the asymmetry rather
than leaving a reader to notice the inconsistency.

**One sparse-buffer primitive serves IP defragmentation and TCP reassembly.**
`sparse::Sparse` holds bytes at offsets and never closes a hole up. Both jobs
are the same job — out-of-order pieces, duplicate pieces, missing pieces — so
there is one implementation with twelve unit tests rather than two.

**A hole is in the type, not in a flag.** `Stream::contiguous()` returns
`Option<&[u8]>`: `Some` only when there is exactly one run and it starts at
offset zero. A caller cannot get the bytes without having been told whether
they are whole, which is the point — dbserver has no length framing, so
concatenating across a gap desynchronises every message after it silently.
`Stream::from_connection_start()` records whether the SYN was captured, so
`Some` from a stream that saw its SYN means the whole conversation and `Some`
from one that did not means "whole from the earliest byte we saw".

**Overlaps are first-writer-wins**, which is what a receiving TCP stack does,
and which also collapses the duplicate copies a bridge capture produces for
free.

**An unsupported link type is an error, not an empty result.** A `pktap` or
`usbmon` capture would otherwise be indistinguishable from a capture of a quiet
network. Classic pcap declares its link type once, so `Capture::new` fails
immediately; pcapng declares one per interface, so the error is raised once per
offending interface and the other interfaces keep working.

**Truncation is a distinct error.** `Error::is_truncated()` separates "the
`tcpdump` writing this was killed" — normal in a corpus, everything before the
cut is good — from "this file is not a capture". Note that `pcap-file` reports
a cut-short record as `IoError(UnexpectedEof)` rather than `IncompleteBuffer`;
both are mapped to `Truncated`.

## Two things the formats made easy to get wrong

1. **pcapng timestamp resolution.** `pcap-file` 2.0 returns the raw 64-bit
   counter as `Duration::from_nanos(counter)` regardless of the interface's
   `if_tsresol` option, which defaults to microseconds and is what `tcpdump`
   writes. Every pcapng timestamp it produces is therefore 1000× too small —
   1970-01-21 instead of 2026-07-30. The option is re-read and the scaling
   redone; `a_pcapng_timestamp_is_scaled_by_the_interface_resolution` pins it.
2. **IP fragmentation.** A CDJ's NFS reads come back as several fragments of
   which only the first has a UDP header. Without reassembly the reader
   under-reports every transfer *silently*. The defragmenter is keyed on the
   RFC 791 tuple **plus the capture interface** — without the interface, a
   bridge capture's two copies of one datagram would be folded into one, so
   fragmented traffic would be silently de-duplicated while unfragmented
   traffic stayed doubled.

## Measured against the corpus

33 files, ~240 MB, `captures/S*/run.pcap`. **25 are classic pcap and 8 are
pcapng** despite all being named `run.pcap`, which is why the format is
decided by the magic. Every interface in every file is link type 1 (Ethernet);
no Linux cooked-capture or raw-IP frames exist in the corpus, so no speculative
support for them was written. The eight pcapng files were captured on a bridge
(`en12` + `en9`) and therefore contain **every datagram twice**.

`cargo test -p prolink-capture` runs the whole corpus in **5.4 s** (debug).

| Quantity | This crate | Python reference (`prolinks_poc.capture.pcap`) |
|---|---|---|
| Files read, no errors | 33 | 33 |
| UDP + TCP packets | 244 501 | 244 501 |
| UDP payload bytes | 205 641 007 | 205 641 007 |
| Largest UDP payload | 28 684 | 28 684 (S13, NFS 2049 → 1302) |
| UDP → 50000 | 7 519 | 7 519 |
| UDP → 50001 | 1 110 | 1 110 |
| UDP → 50002 | 35 103 | 35 103 |
| UDP → 2049 (NFS) | 8 286 | 8 286 |
| UDP *from* 50002 | 24 | 24 |
| TCP streams | 104 | 104 |
| TCP bytes reassembled | 18 726 781 | 18 726 781 |
| Streams opening with the dbserver preamble | 46 | 46 |

**Every figure agrees exactly**, including the byte totals, which is the check
that would catch a defragmentation difference. (The reference's raw
segment-payload sum is 18 729 204: 2 423 bytes more, because it concatenates
overlapping retransmissions and this crate merges them. Its *reassembled* total
is 18 726 781, the same as here.)

The brief's expected numbers — 7833 on 50000, 38371 on 50002 — are higher than
these because they include the JSONL journals under `captures/journals/`, which
are not capture files and are not this crate's business.

**All 104 TCP streams are contiguous**: the corpus has no holes, so the
hole-reporting path is proved by unit tests rather than by the corpus. That is
worth knowing — it means the corpus cannot regress-test that path.

## Findings

- **There is no fixed dbserver port.** The literature gives 1051 and the corpus
  does contain streams on it, but it also carries dbserver conversations on
  1054, 1056, 1058, 1060, 1062, 1064, 1066, 1068, 1070, 1072, 1074, 1076 and
  1078 — the port a CDJ publishes through the TCP-12523 query is whatever it
  bound, and it drifts upward across sessions. **A reassembler filtered to 1051
  finds a fraction of the traffic and looks like it worked.** Hence
  `Reassembler::new()` (every flow) is the default and `on_ports` the
  exception. This is also why the brief's "TCP streams on 1051" framing needs
  care: filtering to 1051 alone gives **20 of the corpus's 104 streams**. The
  server ports observed across the corpus are 1051 and the even numbers 1054
  through 1078; the odd neighbours in the same range are the *client* side of
  the TCP-12523 port queries.
- The "either endpoint is 50002" trap does not actually bite on
  `captures/S*/run.pcap` alone: all 24 datagrams sent from 50002 there also
  went *to* 50002. It bites on the journals and on any capture with a second
  tool present. The API is destination-only regardless, because the corpus
  happening not to contain the failure is not a reason to make it writable.

## Committed fixture floor

`tests/capture.rs` carries two **byte-for-byte extracts of real `tcpdump`
output**, not synthesised files:

- `PCAP` (403 bytes) — the global header of `S24b-e9-control/run.pcap` plus
  four of its records: a UDP-50000 keep-alive, a UDP-50002 media query, and
  both directions of the TCP-12523 dbserver port query.
- `PCAPNG` (548 bytes) — the section header of `S20-browse-ground-truth`, both
  interface descriptions (`en12`, `en9`, neither declaring `if_tsresol`), and
  the same keep-alive as recorded by each of them.

Fifteen tests run off these with no corpus present, including the trap itself:
`a_keep_alive_sent_from_the_status_port_is_still_not_a_status_packet` takes the
real keep-alive record, rewrites *only* its UDP source port to 50002, and
asserts it still does not appear in `udp_to(50002)`. The two corpus tests skip
with a message when `Corpus::locate()` returns `None`.

`.gitignore` ignores `*.pcap` and `*.pcapng` workspace-wide, so the fixtures are
Rust byte literals rather than files — which also makes them diffable.

## Not settled

- **Sequence-number wrap is handled but untested against hardware.** Offsets are
  computed with wrapping arithmetic against a base that moves down when an
  earlier segment arrives late, so a stream that crosses 2³² reassembles
  correctly and a late retransmission does not open a four-gigabyte phantom
  hole. Both are unit-tested; no capture exercises either, because no dbserver
  stream here approaches 4 GiB.
- **`Corpus::captures()` selects by extension** (`pcap`/`pcapng`) while
  `Capture` dispatches on the magic. A corpus file with a different extension is
  missed. Recursing and sniffing every file instead would be slower and would
  read the journals; the extension filter seemed the better trade, but it is a
  trade.
- **The obsolete pcapng packet block (type 2) is not read.** Nothing since
  libpcap 0.x writes one and none is in the corpus, so it is skipped rather
  than handled speculatively — but it is skipped *silently*, unlike an
  unsupported link type.
- **Duplicate packets from a bridged capture are not de-duplicated.** They
  cannot be: nothing on the wire distinguishes a duplicate from a genuine
  repeat. `Packet::interface` is carried so a caller can, and TCP reassembly
  folds them together for free, but UDP counts from the eight pcapng files are
  inflated roughly twofold. The measured figures above therefore describe the
  corpus as recorded, not the traffic as sent.
- **`prolink-proto` cannot use this crate for its corpus tests without a
  dev-dependency** on it, which only its owner may add. Nothing here depends on
  `prolink-proto`, so there is no cycle; `Corpus` is public precisely so that
  other crates' tests can locate the captures the same way.
