# prolink

A Pioneer **Pro DJ Link** implementation in Rust, both directions:

1. **Consume** — see the media in CDJs on the network, browse their libraries the
   way the LINK button does, and stream their tracks.
2. **Serve** — appear as another CDJ with a USB and an SD slot, and let real
   players browse and play a local rekordbox medium.

It is written from captures of real hardware: two CDJ-2000NXS on firmware 1.44,
33 pcapng sessions, and a working Python proof of concept and C++ port that
preceded it. Where the published reverse-engineering literature disagrees with
what the hardware does, the hardware wins and the doc comment says so.

> **Status: in development.** Not yet usable end to end. See
> [`work_log/`](work_log/README.md) for what is built and what is next.

## Why this exists

The reference projects in this space are all *consumers*: they watch a Pro DJ
Link network and read from it. Serving — being the device a CDJ browses and
loads from — needs a different set of things to be right, and most of them fail
**silently**:

- A device number outside 1–4 is accepted in full and then never browsed.
- A slot not advertised in a status packet is a slot no player will ask about.
- Answering an unknown dbserver request with an error makes a deck fetch the
  root menu and disconnect without opening anything.
- Without a portmapper on UDP/111 a deck retries `GETPORT` once a second
  forever and never falls back to the well-known ports.
- Hand a player the analysis bytes rekordbox wrote and the waveform does not
  draw; hand it the wrong container byte and it fetches the whole file and then
  refuses to decode it.

None of those produce an error message anywhere. This library encodes each of
them — several of them in the *type system*, so the mistake cannot be written.

## Layout

| Crate | What |
|---|---|
| [`prolink-proto`](crates/prolink-proto) | The wire codecs. No I/O, no clock. UDP 50000 discovery, UDP 50002 status, TCP 1051 dbserver, ONC RPC v2 / NFSv2, and the ANLZ→wire analysis transforms. |
| [`prolink-rekordbox`](crates/prolink-rekordbox) | The files on a rekordbox medium: `export.pdb`, the `ANLZ` analysis files, device settings, and the joined library model. |
| [`prolink-capture`](crates/prolink-capture) | Reading Pro DJ Link traffic out of pcap/pcapng, with TCP reassembly. What lets the codecs be tested against real hardware traffic. |
| [`prolink`](crates/prolink) | Sockets, timers and state machines: discovery, the virtual CDJ, the NFS and dbserver clients and servers. |
| [`prolink-cli`](crates/prolink-cli) | The `prolink` binary. |

## Design

Read [`CONVENTIONS.md`](CONVENTIONS.md) before contributing. The short version:

- **Parse, don't validate.** Checks happen once, at the boundary, and the result
  is a value whose type records that the check passed. `DeviceNumber` is
  non-zero. `BrowsableDeviceNumber` is 1–4. `PrefixWord` cannot be zero, because
  zero stops the waveform drawing. A `CdjStatus` is proof that the buffer is
  long enough for every field its accessors read.
- **Byte-exactness is a feature.** The goal is to be indistinguishable from a
  real CDJ, so packets we do not fully understand are built from captured
  skeletons with only understood fields substituted, and decoding preserves
  every byte so a captured packet re-encodes exactly.
- **Provenance in the doc comment.** Every non-obvious constant carries the
  finding that establishes it, and where a value is reproduced without being
  understood, the comment says so in those words.

## On rekordbox parsing

`export.pdb` and the `ANLZ` files are parsed here rather than by an existing
crate, which deserves an explanation, since reinventing them would otherwise be
the wrong call:

- [`rekordcrate`](https://crates.io/crates/rekordcrate) is the canonical crate
  and its schema is excellent, but almost every field of `pdb::Track` is
  private — a Pro DJ Link server cannot read the tempo, duration, bitrate, key,
  album, analysis path, comment, file size or container it has to serve. Its
  `anlz::VBR` payload is private too, and that payload is what gates MP3
  playback. It is wired in here behind an optional, default-off feature for the
  one thing it does that nothing else does: naming the device-settings values.
- [`rekordbox-pdb`](https://crates.io/crates/rekordbox-pdb) exposes its fields
  but omits the container byte at row offset `0x5a`, which is the field a deck
  uses to decide how to decode a track.
- [`rbox`](https://crates.io/crates/rbox) targets rekordbox desktop's SQLite
  databases rather than the USB export.

## Licence

GPL-3.0-only. See [`LICENSE`](LICENSE).

The protocol knowledge behind this comes from the author's own captures of the
author's own hardware. Reference projects in this space were read for context;
no code was copied from any of them.
