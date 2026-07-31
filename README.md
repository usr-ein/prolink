# prolink

[![CI](https://github.com/usr-ein/prolink/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/usr-ein/prolink/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/prolink.svg?logo=rust)](https://crates.io/crates/prolink)
[![docs.rs](https://img.shields.io/docsrs/prolink?logo=docsdotrs&label=docs.rs)](https://docs.rs/prolink)
[![MSRV 1.88](https://img.shields.io/badge/MSRV-1.88-blue.svg?logo=rust)](Cargo.toml)
[![licence GPL-3.0-only](https://img.shields.io/crates/l/prolink.svg)](LICENSE)

A Pioneer **Pro DJ Link** implementation in Rust, both directions:

1. **Consume** — see the media in CDJs on the network, browse their libraries the
   way the LINK button does, and stream their tracks.
2. **Serve** — appear as another CDJ with a USB and an SD slot, and let real
   players browse and play a local rekordbox medium.

It is written from captures of real hardware: two CDJ-2000NXS on firmware 1.44,
37 pcapng sessions, and a working Python proof of concept and C++ port that
preceded it. Where the published reverse-engineering literature disagrees with
what the hardware does, the hardware wins and the doc comment says so.

> **Status: working against real hardware.** Two CDJ-2000NXS browse this
> server's media, load and play from it, search it, sort it, tag from it and
> read its analysis, and 647 tests pass — including a corpus replay of 37
> captures of real CDJ traffic and an end-to-end test of this library's servers
> against its own clients. Several bugs that only hardware could find have been
> fixed, each recorded in [`work_log/`](work_log/README.md) with the capture
> that proved it.
>
> One known gap: the **key-matching indicator** does not light on a deck
> browsing this server, and every field we send has been byte-compared against
> a real CDJ serving the same medium without finding the difference. See
> [`work_log/028`](work_log/028-the-key-matching-indicator.md).

## Using it

```sh
cargo install prolink-cli      # the binary is called `prolink`
prolink interfaces             # which NIC faces the CDJs
```

Or as a library, `cargo add prolink` — see [docs.rs/prolink](https://docs.rs/prolink).
From a clone, `cargo build --release` puts the same binary in
`target/release/prolink`.

Everything except `announce`, `serve` and `status --announce` transmits nothing
on any Pro DJ Link port, so it is safe beside a live rig.

### Seeing what is on the network

```sh
prolink devices --watch                     # who is here, as it changes
prolink status --watch                      # tempo and beat/bar phase, passively
prolink status --watch --announce           # ...plus the loaded track and who is master
```

Tempo and phase come from beat packets, which are broadcast. The loaded track,
the play state and the **tempo master** are published only in status packets,
which a player unicasts to peers that have announced themselves — so those need
`--announce`, which takes a device number outside the 1–6 player range and
cannot collide with hardware.

### What media the players have

```sh
prolink media                               # every player's slots, named and counted
```

```text
device 1 — CDJ-2000nexus at 169.254.103.172
  USB  SAM2 — 692 tracks, 35 playlists — 14.9 GiB free of 29.8 GiB
  SD   empty
```

This transmits, and for two reasons. Whether a slot holds anything is published
only in status packets, which a player unicasts to peers that have announced
themselves; the label and the counts are a separate question-and-answer on
UDP 50002, and a deck replies to it in about a millisecond.

### Reading another player's media

```sh
prolink rpcinfo 169.254.244.181                       # does it serve NFS?
prolink pull-db 169.254.244.181 --slot usb            # fetch its export.pdb
prolink tracks export.pdb                             # ...and read it
prolink browse 169.254.244.181 --slot usb             # the root menu, as LINK shows it
prolink browse 169.254.244.181 --slot sd --tracks
prolink browse 169.254.244.181 --slot usb --search "gesaffelstein"
prolink browse 169.254.244.181 --slot usb --track 366 # metadata and the path a load reads
```

`pull-db` is passive. `browse` is not: dbserver needs a device number in 1–4, so
it announces and contends for one first.

### Serving your own media to real CDJs

```sh
sudo prolink serve --usb /Volumes/MY_STICK
sudo prolink serve --usb /Volumes/ONE --sd /Volumes/TWO
```

**`sudo` is not optional.** The portmapper needs UDP/111, which is privileged,
and a deck with nothing there retries `GETPORT` once a second for ever rather
than falling back to the well-known ports. On Linux you can instead set
`net.ipv4.ip_unprivileged_port_start=111`; macOS has no equivalent. If the port
cannot be taken, `serve` says so rather than starting something unreachable.
[`docs/TESTING.md`](docs/TESTING.md) has an `install.sh` and a scoped sudoers
rule so it stops asking for a password.

A second USB stick presented with `--sd` appears to a CDJ as an SD card, which is
exactly what it expects to see. Both media are served over one dbserver
connection and told apart by the slot byte in each request.

Ctrl-C **ejects** before it stops, the way pulling the stick out of a real deck
does: the slot goes unmounting, the players reading from it send `UMNT` and let
go, and only then do the servers close. Two to three seconds when a deck is
actually reading, about half of one when none is. A second Ctrl-C skips the wait
and leaves whoever was reading holding a stale mount.

### Offline

```sh
prolink pcap run.pcap                       # what Pro DJ Link traffic is in a capture
```

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

| Crate | | What |
|---|---|---|
| [`prolink-proto`](crates/prolink-proto) | [![crates.io](https://img.shields.io/crates/v/prolink-proto.svg)](https://crates.io/crates/prolink-proto) | The wire codecs. No I/O, no clock. UDP 50000 discovery, UDP 50001 beats, UDP 50002 status, TCP 1051 dbserver, ONC RPC v2 / NFSv2, and the ANLZ→wire analysis transforms. |
| [`prolink-rekordbox`](crates/prolink-rekordbox) | [![crates.io](https://img.shields.io/crates/v/prolink-rekordbox.svg)](https://crates.io/crates/prolink-rekordbox) | The files on a rekordbox medium: `export.pdb`, the `ANLZ` analysis files, device settings, and the joined library model. |
| [`prolink-capture`](crates/prolink-capture) | [![crates.io](https://img.shields.io/crates/v/prolink-capture.svg)](https://crates.io/crates/prolink-capture) | Reading Pro DJ Link traffic out of pcap/pcapng, with TCP reassembly. What lets the codecs be tested against real hardware traffic. |
| [`prolink`](crates/prolink) | [![crates.io](https://img.shields.io/crates/v/prolink.svg)](https://crates.io/crates/prolink) | Sockets, timers and state machines: discovery, the virtual CDJ, the NFS and dbserver clients and servers. |
| [`prolink-cli`](crates/prolink-cli) | [![crates.io](https://img.shields.io/crates/v/prolink-cli.svg)](https://crates.io/crates/prolink-cli) | The `prolink` binary. |
| [`prolink-cxx`](crates/prolink-cxx) | [![crates.io](https://img.shields.io/crates/v/prolink-cxx.svg)](https://crates.io/crates/prolink-cxx) | A C++ binding over `prolink`, generated by [`cxx`](https://cxx.rs). The one crate that does not inherit the workspace's `unsafe_code = "forbid"`. |

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

## Testing

```sh
cargo test --workspace                                   # 647 tests, no hardware needed
PROLINK_CAPTURES=/other/captures cargo test --workspace   # ...against a different corpus
```

The corpus replay is the test that matters, and it needs nothing set up: the
272 MB of traffic it replays is committed, in [`captures/`](captures/). It reads
37 pcap and pcapng captures of two CDJ-2000NXS and pushes every datagram through
the codecs: 9,308 discovery packets that must re-encode byte for byte, 46,103
status packets, 5,594 beat packets, 65,705 dbserver messages whose framing must
account for the stream exactly, and 60,138 ONC RPC calls that must parse into the
shape their procedure implies. Zero failures. A checkout without the captures —
the published crates carry none — makes those tests skip, and a committed fixture
floor keeps them meaningful anyway.

`crates/prolink/tests/loopback.rs` runs this library's servers against its own
clients over loopback: a real 651-track `export.pdb` served, browsed, drilled,
sorted, searched, and pulled back byte for byte. Two implementations agreeing is
weaker evidence than one agreeing with a CDJ — that is what the corpus is for —
but it is what proves the wiring.

## Licence

GPL-3.0-only. See [`LICENSE`](LICENSE).

The protocol knowledge behind this comes from the author's own captures of the
author's own hardware. Reference projects in this space were read for context;
no code was copied from any of them.
