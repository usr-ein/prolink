# 024 — Beats, phase and the tempo master

The question this answers: *what is playing, at what tempo, at what phase of the
beat and the bar, and who is master?* Two ports answer it and neither answers
all of it, which is the whole shape of the work.

| Port | Transport | Carries | Needs an announcement |
|---|---|---|---|
| 50001 | broadcast | tempo, pitch, beat-in-bar, the six grid distances | no |
| 50002 | unicast to announced peers | loaded track, play state, **tempo master** | yes (F21) |

## Layout

| File | What is in it |
|---|---|
| `prolink-proto/src/beat.rs` | the 50001 codec: `Beat`, `Pitch`, `BeatInBar`, `Timings`, `ChannelsOnAir`, `FaderStart`, `decode` |
| `prolink/src/monitor.rs` | `Monitor`, the player table, phase interpolation, `PlayState`, `TrackKind`, `LoadedTrack` |
| `prolink-cli/src/main.rs` | `prolink status [--watch] [--announce]` |

## Public API

```rust
// prolink_proto::beat
let beat = Beat::parse(datagram)?;         // or beat::decode(datagram)? -> Packet
beat.device;                               // DeviceNumber, non-zero by construction
beat.beat_in_bar;                          // Option<BeatInBar>, 1-4; None means no bar
beat.timings.next_beat;                    // Option<Duration>, at 0% pitch
beat.bpm();                                // the track's tempo
beat.effective_bpm();                      // ...with the pitch fader applied
beat.beat_interval();                      // Option<Duration>, from the effective tempo
beat.encode();                             // [u8; 96], byte-exact

// prolink::monitor
let monitor = Monitor::start(interface).await?;               // 50001 only, transmits nothing
let monitor = Monitor::with_status(interface, &cdj).await?;   // + 50002, needs a VirtualCdj
monitor.players();                          // Vec<PlayerState>, ordered by device number
monitor.tempo_master();                     // Option<DeviceNumber>
monitor.watches_status();                   // false => master and track are *unknown*, not absent
monitor.subscribe();                        // broadcast::Receiver<MonitorEvent>

state.effective_bpm();                      // Option<f64>
state.beat_phase();                         // Option<f64>, 0.0 on the beat, clamped at 1.0
state.bar_phase();                          // Option<f64>, 0.0 on the downbeat
state.is_tempo_master();                    // Option<bool> — None is "cannot know"
state.track();                              // Option<LoadedTrack>
```

`Monitor::with_status` takes `&VirtualCdj` rather than a boolean because the
requirement is a fact about the wire, not a preference: without an announcement
50002 stays silent forever (F21). Putting it in the signature means the mistake
cannot be made.

`is_tempo_master()` returns `Option<bool>` for the same reason. A passive
listener cannot tell "not master" from "cannot know", and reporting the first
when it means the second is the one lie this API must not tell. The CLI prints
`?` in that column, not a blank.

## Where each part comes from

- The 96-byte layout is `research/03` §3.2, **corrected** by `docs/PROTOCOL.md`
  §3 and `status.rs` on the header: the name is 20 bytes at `0x0b`–`0x1e` and
  byte `0x1f` is a structural `0x01`, not a 21st name byte (C14). The corpus
  confirms C14 on this port too — `0x1f` is `0x01` in all 1110 packets.
- Phase interpolation, the 3-second staleness rule and the clamp-don't-wrap
  decision are the C++ port in
  `mixxx/src/network/prolink/prolinkbeatlistener.{h,cpp}`.
- The master byte `0x9e` and the "status is unicast to announced peers" rule are
  `docs/PROTOCOL.md` §3.1 and `status.rs` (F20, F21, F45).
- The play-state and track-type tables are `research/03` §1.2, filtered to what
  the corpus actually shows (below).

## The corpus

33 captures. **1110 datagrams addressed to UDP 50001**, and every single one is
a type-`0x28` beat packet: 96 bytes, subtype `0x00`, `len_r = 0x003c`, from a
`CDJ-2000nexus`, broadcast to `169.254.255.255` from an ephemeral, incrementing
source port. Eleven of the 33 capture directories contain them:

| Directory | Beat packets |
|---|---|
| S13-format-ground-truth | 278 |
| S10i-serve-to-cdj | 223 |
| S06-load-and-play | 186 |
| S10j-serve-to-cdj | 107 |
| S11-format-matrix | 71 |
| S15a-sd-alone | 68 |
| S22-sorting | 46 |
| S17-serve-formats | 42 |
| S15b-sd-and-usb | 40 |
| S24b-e9-control | 29 |
| S18-two-slots | 20 |

**286 of the 1110 are the same IP datagram recorded twice** — identical source
port, identical IP id, identical MACs, 20 µs apart. It happens in exactly four
directories (S06, S13, S15a, S15b) and there it is exactly half the packets, so
those four runs were captured on an interface that saw every frame twice. That
is a capture artefact, **not** a double transmission: 824 distinct beat packets.
It is worth knowing because a naive "beat" event stream would double-fire on
that data.

Devices 1 (186 packets) and 2 (924). Beat-in-bar 1: 289, 2: 294, 3: 275, 4: 252.

Tempos, as BPM × 100: `13200` ×69, `13201` ×186, `13500` ×4, `14000` ×9,
`14200` ×2, `14500` ×796, `15200` ×38, `15400` ×3, `16686` ×3. That last one is
a real 166.86 BPM grid, not a decoding error — its next-beat field is 360 ms,
which is 60000/166.86.

Pitch, the four values that dominate: `0x00100000` (0%) ×79, `0x000f8312`
(−3.05%) ×186, `0x000fbc6a` (−1.65%) ×300, `0x000ffdf3` (−0.05%) ×401,
`0x0010020c` (+0.05%) ×120. The extremes are jog-wheel nudges in S10j:
`0x000ab4a2` is −33.09% and `0x001435a8` is +26.31%.

### What the corpus settled

**The timing fields are quoted at 0% pitch — confirmed on hardware, not
inherited.** Two independent measurements:

1. Across all 1110 packets, the next-beat field equals `60000 / bpm` to within
   −0.79 … +0.56 ms.
2. In S10j, one deck held next-beat at 414 ms across 19 distinct pitch values
   spanning 0.669 to 1.094 while the DJ worked the jog wheel. The field did not
   move.

**...and the interval between packets follows the effective tempo.** Over the
762 consecutive-beat pairs where the bar position advanced by one:

| Prediction | median error | p05 … p95 |
|---|---|---|
| `60000 / (bpm × pitch)` | **−0.11 ms** | −2.43 … +2.81 |
| `60000 / bpm` | +2.12 ms | −2.28 … +14.19 |

Restricted to the 375 pairs where the fader was more than 1.6% off centre, the
gap widens to −0.19 ms against +6.83 ms. So `effective_bpm` is the one to
interpolate with, and a consumer reporting the raw BPM is wrong by exactly the
fader.

**The grid fields are consistent with each other.** `next_bar == next_beat +
(4 − beat_in_bar) × step` holds exactly in 752 of 1110 and within ±2 ms in all
1110, the residue being the deck's own per-beat rounding. `second_beat`,
`fourth_beat` and `eighth_beat` are 2, 4 and 8 beats out; `second_bar` is four
beats past `next_bar`. The unit test asserts all of this over the whole corpus
with a ±7 ms tolerance.

**Constant bytes.** In all 1110: the 24 filler bytes are `0xff`, `0x1f` is
`0x01`, both scratch words are `0000`, and the device number at `0x5f` equals
the one at `0x21`. That is what lets `Beat` be a struct with an exact encoder
rather than a captured skeleton — and all 1110 re-encode byte for byte.

### The status side, for the monitor

35 016 CDJ status packets, all 284 bytes.

- **Play state `0x7b`.** Ten of the twelve documented values occur: `0x00`
  ×26745, `0x02` ×1083, `0x03` ×2559, `0x04` ×47, `0x05` ×3341, `0x06` ×412,
  `0x07` ×12, `0x09` ×71, `0x0e` ×557, `0x12` ×189. `0x08` (cue scratch) and
  `0x11` (end of track) never appear.
- **`0x12` is the emergency loop, confirmed.** All 189 came from a deck in
  S11-format-matrix whose slots had both just gone empty while a rekordbox
  track (id 182) was loaded, and all 189 set byte `0xba`, which the literature
  calls the emergency flag. The medium was pulled mid-play.
- **Beat packets track playback.** For each status packet, whether the same deck
  sent a beat in the preceding second:

  | Play state | with a recent beat | without |
  |---|---|---|
  | `0x00` no track | 0 | 26745 |
  | `0x03` playing | 2299 | 260 |
  | `0x04` looping | 33 | 14 |
  | `0x05` paused | 241 | 3100 |
  | `0x0e` spun down | 0 | 557 |
  | `0x12` emergency loop | 0 | 189 |

  The 241 "paused with a beat" are the tail of the one-second window after the
  platter stopped, and the 260 "playing without" are its head. `0x12` is the
  interesting row: the deck is audibly looping and sends **no** beats, because
  the medium carrying the grid is gone. So "playing" and "sending beats" are two
  facts, and `PlayState::is_playing` says so in its doc comment.
- **Mastership.** Byte `0x9e`: `0` ×28087, `1` ×6620, `2` ×309. It agreed with
  flag bit 5 of byte `0x89` in 35 015 of 35 016 packets — one disagreed, caught
  mid-update. `0x9e` is used because it also distinguishes a master on a
  rekordbox track from one with no usable tempo.
- **Byte `0x9f` was `0xff` in all 35 016**, so no master handoff was ever
  captured.
- **The downbeat comes from the beat packet, not from status.** Status byte
  `0xa6` matched the last beat packet's `0x5c` in only 2482 of 2645 comparisons
  — 6% disagreement — which is the concrete form of `research/03` §5.4's warning
  that status beat-in-bar is not beat-aligned.

## Two design notes

**Sharing the header rather than copying it.** The 50001 and 50002 headers are
the same layout with a different byte at `0x0a`. `beat.rs` imports
`status.rs`'s offsets and readers instead of keeping a second copy that would
have to be corrected twice. That needed five small additive changes in
`status.rs`: five offset constants and three readers promoted to `pub(crate)`,
and two new `pub(crate)` functions, `write_shared_header` and `check_header`,
taking the kind as a plain `u8` — the existing `write_header` and `check` are
now one-line wrappers that keep their `StatusKind` signatures and their error
messages. The honest home for all of it is a `crate::header` module; that is a
larger edit to `status.rs` than was safe while four agents were in the tree.

**Two sockets cannot share port 50002.** `Monitor::with_status` binds it, and a
`VirtualCdj` with `emit_status` set binds it too, to answer media queries. Both
set `SO_REUSEPORT`, so both binds succeed — but a *unicast* datagram goes to
only one of them. Measured on this machine: with two `SO_REUSEPORT` sockets on
one port, macOS delivered all 20 test datagrams to the first-bound socket.
Linux hashes instead. Either way, a monitor and a serving virtual CDJ cannot
both read status.

The resolution is that they never need to. What makes a peer unicast its status
to us is the **keep-alive**, not our own status stream, so a virtual CDJ built
with `emit_status: false` announces, is told everything, and leaves 50002 to the
monitor. `prolink status --announce` does exactly that. Emitting status is
needed only to be *browsed*.

## Verified end to end

The 40 beat packets of S06-load-and-play were replayed at a running
`prolink status --watch` over a real socket, at their captured spacing. The
table showed 127.98 BPM — 132.01 at −3.05% — with the bar marker walking
1 → 2 → 3 and the phase advancing 0.427 per 200 ms tick against a 468.8 ms
beat. When the replay stopped, the phase went stale within three seconds and
the tempo stayed on screen, which is the intended behaviour.

## Still untested against hardware

- **Scratching.** Both flag words are `0000` in all 1110 packets. The `ffff`
  form is literature only, and a packet whose two words disagreed would not
  re-encode byte for byte. None has been seen.
- **`0xffff_ffff` in a timing field** ("the track ends first"). Never occurs in
  the corpus — every packet carries all six distances.
- **Beat-in-bar `0`** (a player with no rekordbox track). Never occurs; only
  1–4 appear. The decode path and the "no bar phase" behaviour are unit-tested
  from a mutated packet, not from a captured one.
- **`ChannelsOnAir` (`0x03`) and `FaderStart` (`0x02`) entirely.** No mixer took
  part in any capture — all 1110 datagrams on 50001 are beat packets — so both
  layouts are the pre-hardware literature's. They are decode-only for that
  reason, and the six-channel form doubly so.
- **`0x0b` absolute position, and `0x2a` / `0x26` / `0x27` sync and master
  handoff.** Named in `BeatKind` so a log can identify them; never observed, not
  modelled. `0x9f` never left `0xff`, so the handoff dance is unwitnessed.
- **A mixer as a device** (number `0x21`), and its continuous metronome beats.
  Every claim about a mixer in these two modules is inherited.
- **Play states `0x08` and `0x11`**, and track types other than `0x00` and
  `0x01`.
- **`Monitor::with_status` against a live rig.** The decoding is exercised
  against 35 016 real status packets, but the socket path is not; and the
  `SO_REUSEPORT` behaviour is measured on macOS only.
- **A tempo master moving between decks** while the monitor watches. The table
  logic is unit-tested, but no capture shows a handoff.

## Files touched outside this work's own

- `prolink-proto/src/lib.rs` — `pub mod beat;`
- `prolink-proto/src/status.rs` — the five additive changes described above
- `prolink/src/lib.rs` — `pub mod monitor;` and two `pub use` lines
- `prolink-cli/src/main.rs` — the `status` subcommand, and one word in the
  module doc, which claimed every command but `announce` and `serve` is passive
