# E06-paused-master-yields

- started: 2026-10-10T19:13:41Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: a paused master, nobody playing; then the other deck starts.
  Does the master hand over? Every combination of the master's sync and the
  starting deck's sync, one each:

  | case | paused master | deck that starts |
  | --- | --- | --- |
  | D | in sync | in sync (S28 208.3's case) |
  | A | not in sync | in sync |
  | B | in sync | not in sync |
  | C | not in sync | not in sync |

  And one more (E): B's paused master turns SYNC off while the other deck
  plays.

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded, then b's; each played as it loaded,
and a became master.

The Mac's load averages were 4.6 at the start and 4.0 at the end. Keep-alives
came every 2.0000 s (median; 49 intervals each, 1.9997-2.0101 s).

This is the session's third take. The first took cases A and B only, so it
could not tell which deck's sync decides; the second lost case C to a hand-over
it did not expect (E, kept here as a step of its own). Every step below is
from this take.

## Timeline

Times are seconds from the first frame; each press lands between the step's
start and the end of its `pi-qemu` command, about 0.13 s later. PLAY toggles
play and pause. "Hands over" means the three status packets of E05: the
master names the other in `0x9f`, the other sets `0x9e` = 1, the master
clears its own. Nothing is sent on 50001 in any of them.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00. b: 0xc4 (playing), 132.00. |
| 3.366 | b: SYNC | 3.563: b's status 0xd4 (playing, sync), pitch -4.55%: 126.00. Its beat packets say -2.72% for five beats (3.928-5.797) while it bends into a's phase (`../README.md`), then -4.55%. |
| 7.369 | a: SYNC | 7.496: a's status 0xf4 (playing, master, sync). |
| 11.366 | b: PLAY, so b stops | 11.507: b's status 0x94 (sync, not playing). |
| 17.370 | a (master, in sync): PLAY, so a stops | 17.496: a's status 0xb4 (master, sync, not playing), `0x9f` = 0xff. Nobody is playing: a keeps master. |
| 25.365 | **D:** b (in sync) plays | 25.515: b's status 0xd4 (playing, sync). **a hands over:** 25.520 (+5 ms after b's first "playing" packet) a's `0x9f` = 2; 25.579 (+64 ms) b's `0x9e` = 1; 25.648 (+133 ms) a's `0x9e` = 0. |
| 33.367 | b (master, playing): SYNC off | 33.587: b's status 0xe4 (playing, master). |
| 37.366 | b: PLAY, so b stops | 37.491: b's status 0xa4 (master, not playing). a is stopped too: b keeps master. |
| 45.367 | **A:** a (in sync) plays | 45.480: a's status 0xd4 (playing, sync). **b hands over:** 45.483 (+3 ms) b's `0x9f` = 1; 45.544 (+64 ms) a's `0x9e` = 1; 45.611 (+131 ms) b's `0x9e` = 0. |
| 53.370 | a (master, in sync): PLAY, so a stops | 53.480: a's status 0xb4 (master, sync, not playing). b is stopped: a keeps master. |
| 61.366 | **B:** b (not in sync) plays | 61.491: b's status 0xc4 (playing). **a keeps master,** paused: `0x9e` = 1 and `0x9f` = 0xff in every packet up to 71.608, ten seconds of b playing alone. |
| 71.370 | **E:** a (the paused master, in sync): SYNC off | 71.608: a's status 0xa4 (master, not playing, sync now off), `0x9e` = 1, **`0x9f` = 2**: in the packet that says its sync went off, a hands master to b, which is playing. 71.619 (+11 ms) b's `0x9e` = 1; 71.672 (+64 ms) a's `0x9e` = 0. |
| 79.366 | b (master, not in sync): PLAY, so b stops | 79.483: b's status 0xa4 (master, not playing). a is stopped: b keeps master. |
| 87.372 | **C:** a (not in sync) plays | 87.480: a's status 0xc4 (playing). **b hands over:** 87.539 (+59 ms) b's `0x9f` = 1; 87.552 (+72 ms) a's `0x9e` = 1; 87.603 (+123 ms) b's `0x9e` = 0. |
| 100.0 | (capture ends) | |

The MASTER lamps followed each hand-over within the 0.1 s poll: b lit at
25.58, a dark at 25.67; a lit at 45.49, b dark at 45.59; b lit at 71.70, a
dark at 71.68; a lit at 87.62, b dark at 87.65.

## What came of it

| case | paused master | deck that plays | hands over? |
| --- | --- | --- | --- |
| D | in sync | in sync | yes |
| A | not in sync | in sync | yes |
| B | in sync | not in sync | **no** |
| C | not in sync | not in sync | yes |
| E | (B, then its own sync off) | not in sync | yes, at once |

- **A master that is not playing hands master to a deck that plays, unless
  the master is in sync and the playing deck is not.** One case of each
  here. Every other hand-over to a playing deck in these sessions (E01, E05,
  E09) and in S28 (193.7, 208.3) fits it, but none of them has a master in
  sync beside a deck out of sync, so only B and E test the exception. It is
  the emulator's word for A, B, C and E: S28 never had them.
- **The rule is held, not only looked at when a deck starts:** in E the
  paused master gave master away the moment its own sync went off, with
  nobody pressing PLAY.
- **The hand-over is the status packets alone,** the paused master naming
  its successor 3-59 ms after the other deck's first "playing" packet.

## Against S28

- **S28 208.276 is case D,** like for like: deck 2 the paused master in sync
  (0xb4), deck 1 starting in sync. Deck 2 named deck 1 4 ms after deck 1's
  first "playing" packet, deck 1 set `0x9e` 63 ms after, deck 2 cleared its
  own 76 ms after. Here: 5, 64 and 133 ms.
- **B, C and E never happened in S28,** nor A: what this session shows for
  them rests on the emulator.
- **Stopped,** the emulated decks keep pitch copies 2 and 4 non-zero; S28's
  read 0 there while paused (`../README.md`).
