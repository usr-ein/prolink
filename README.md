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

> **Status: complete but untested against hardware.** Both directions are
> implemented and 596 tests pass, including a corpus replay of 33 captures of
> real CDJ traffic with zero failures and an end-to-end test of this library's
> servers against its own clients. Nothing here has yet met a real deck.
> [`work_log/`](work_log/README.md) records what was built, in what order, and
> what is still unproven.

## Using it

```sh
cargo build --release          # the binary lands in target/release/prolink
prolink interfaces             # which NIC faces the CDJs
```

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

| Crate | What |
|---|---|
| [`prolink-proto`](crates/prolink-proto) | The wire codecs. No I/O, no clock. UDP 50000 discovery, UDP 50001 beats, UDP 50002 status, TCP 1051 dbserver, ONC RPC v2 / NFSv2, and the ANLZ→wire analysis transforms. |
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

## Testing

```sh
cargo test --workspace                                   # 596 tests, no hardware needed
PROLINK_CAPTURES=/path/to/captures cargo test --workspace # ...plus the corpus replay
```

The corpus replay is the test that matters. It reads 33 pcap and pcapng captures
of two CDJ-2000NXS and pushes every datagram through the codecs: 7,519 discovery
packets that must re-encode byte for byte, 35,103 status packets, 1,110 beat
packets, 59,205 dbserver messages whose framing must account for the stream
exactly, and 56,957 ONC RPC calls that must parse into the shape their procedure
implies. Zero failures. Without a corpus those tests skip, and a committed
fixture floor keeps them meaningful anyway.

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
