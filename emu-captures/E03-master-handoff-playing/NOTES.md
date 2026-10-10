# E03-master-handoff-playing

- started: 2026-10-10T18:09:07Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: both decks playing, both in sync, a master. MASTER pressed on
  b, then a, b, a: four hand-overs. Then SYNC off on both, and one more
  hand-over (to b) with sync off.

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded and then b's. Each played as soon as
it loaded, so a, first, became master. At the first frame both are playing at
pitch 0: a at 126.00 (master), b at 132.00 (not in sync).

The Mac's load averages were 9.8 at the start and 8.5 at the end. Keep-alives
came every 2.0002 s (median; 46 intervals each, 1.9998-2.026 s).

## Timeline

Times are seconds from the first frame. Each press lands between the step's
start and the end of its `pi-qemu` command, about 0.13 s later.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00 BPM, pitch 0. b: 0xc4 (playing), 132.00, pitch 0. |
| 4.385-4.508 | b: SYNC | 4.497: b's status 0xd4 (playing, sync), pitch still 0. 4.561 (+64 ms): pitch -4.55% (0x000f45d1), so b plays at 132.00 x 0.95455 = 126.000, a's tempo. Its first beat after (4.772) falls 4.6 ms after a's, where the one before fell 13.5 ms after; its beat packets carry -4.49% for a few beats, a nudge the status packets never show. |
| 9.383-9.513 | a: SYNC | 9.530: a's status 0xf4 (playing, master, sync). A master may be in sync too: nothing else changes. |
| 19.383-19.503 | b: MASTER | 19.490: **0x26** (40 B) b to a, on 50001. 19.491 (+0.8 ms): **0x27** (44 B) a to b, granted (1). 19.491 (+1.2 ms): a's status `0x9e` = 1, **`0x9f` = 2**. 19.562 (+72 ms): b's status 0xf4, `0x9e` = 1. 19.618 (+128 ms): a's status 0xd4, `0x9e` = 0, `0x9f` = 0xff. |
| | | b keeps playing at 126.00 (pitch -4.55%), the tempo it was following: its own fader (at 0, 132.00) is not taken up. a, now a follower in sync, reads pitch 0x000fffff (-0.00%): 126.00 x 0.999999. |
| 29.382-29.508 | a: MASTER | 29.499: 0x26 a to b. +7.7 ms: 0x27, granted. +8.0 ms: b's `0x9f` = 1. +79.8 ms: a `0x9e` = 1. +135.7 ms: b `0x9e` = 0. |
| 39.382-39.499 | b: MASTER | 39.491: 0x26. +1.9 ms: 0x27. +2.0 ms: a's `0x9f` = 2. +15.9 ms: b `0x9e` = 1. +66.1 ms: a `0x9e` = 0. |
| 49.386-49.517 | a: MASTER | 49.502: 0x26. +8.1 ms: 0x27. +23.9 ms: b's `0x9f` = 1. +88.3 ms: a `0x9e` = 1. +152.2 ms: b `0x9e` = 0. |
| 59.382-59.513 | a (master): SYNC off | 59.640: a's status 0xe4 (playing, master); its pitch goes from 0x000ffffe to 0x00100000, where its fader is. 126.00 either way. |
| 64.384-64.517 | b: SYNC off | 64.600: b's status 0xc4 (playing). b stays at pitch -4.55% (126.00), not at its fader (0, 132.00). Pitch copies 2 and 4 read 0x000f4622 in that packet, then follow pitch 1 again. |
| 74.382-74.510 | b: MASTER, neither deck in sync | 74.491: 0x26. +7.9 ms: 0x27, granted. +8.0 ms: a's `0x9f` = 2. +72.1 ms: b `0x9e` = 1. +88.0 ms: a `0x9e` = 0. Both stay at 126.00. |
| 94.5 | (capture ends) | |

The MASTER lamps followed the hand-overs within the 0.1 s poll: b's lit at
19.59 and a's went dark at 19.67; a's lit at 29.61, b's dark at 29.65; and so
on. The SYNC lamps lit at 4.51 (b) and 9.55 (a), and went dark at 59.65 (a)
and 64.69 (b).

## What came of it

- **Each MASTER press is one exchange on 50001:** the 40-byte 0x26 from the
  deck that wants master, unicast to the master, and the master's 44-byte
  0x27 back, granted (`1`) every time. Then three status packets: the old
  master names its successor in `0x9f` (still `0x9e` = 1), the new master
  sets `0x9e` = 1, and the old master clears both.

  | hand-over | 0x27 | old master `0x9f` | new `0x9e` = 1 | old `0x9e` = 0 |
  | --- | ---: | ---: | ---: | ---: |
  | 19.490, a to b | +0.8 ms | +1.2 ms | +72.0 ms | +128.4 ms |
  | 29.499, b to a | +7.7 ms | +8.0 ms | +79.8 ms | +135.7 ms |
  | 39.491, a to b | +1.9 ms | +2.0 ms | +15.9 ms | +66.1 ms |
  | 49.502, b to a | +8.1 ms | +23.9 ms | +88.3 ms | +152.2 ms |
  | 74.491, a to b, no sync | +7.9 ms | +8.0 ms | +72.1 ms | +88.0 ms |

  For 16-64 ms of each, both decks say `0x9e` = 1; only `0x9f` says which one
  is leaving (S28: 14-60 ms).
- **The new master keeps the tempo that was playing,** not its own fader's.
- **Beat phase:** while synced, b's beats fell within 0.01 beat (5 ms) of a's
  from the first beat after SYNC on, through every hand-over (10-s windows:
  mean offset -0.004 to +0.005 beat, sd 0.002-0.003). They stayed there after
  SYNC was off, both decks being left at the same tempo. The tracks' bars
  lined up too: both decks' beat 1 of 4 came together.
- **Beats:** a's every 0.4762 s (sd 1.4 ms; 60/126.00 = 0.47619).

## Against S28

- **The hand-over sequence is the same,** packet for packet: 0x26 to the
  master, 0x27 back, `0x9f` = successor, the successor's `0x9e` = 1, the old
  master's `0x9e` = 0. S28 has five, all granted, with no packet on 50001
  other than these and beats:

  | | 0x27 | old `0x9f` | new `0x9e` = 1 | old `0x9e` = 0 |
  | --- | ---: | ---: | ---: | ---: |
  | S28, five hand-overs | +2.4 to +5.0 ms | +3.3 to +5.2 ms | +16 to +73 ms | +67 to +133 ms |
  | E03, five hand-overs | +0.8 to +8.1 ms | +1.2 to +23.9 ms | +16 to +88 ms | +66 to +152 ms |

  The emulated master answered in 0.8-8.1 ms where the real ones took
  2.4-5.0 ms, and once (49.502) named its successor 16 ms after its 0x27,
  where the real ones did it with the 0x27 every time. The rest is within the
  real decks' range, or close to it: the status packets go out on ~64 ms
  ticks on both, so the steps land a tick early or late.
- **The new master keeps the tempo,** as in S28 (109.296: deck 1 took master
  at pitch +1.40%, its follower's pitch, not its fader's). The old master,
  now following, reads 0x000fffff there too (S28 109.310).
- **SYNC on:** S28's follower also matched the master's tempo one packet
  after the sync flag, 65 ms (102.111, 102.176); here 64 ms.
- **The bytes:** the 0x26 and 0x27 packets carry nothing the real ones did
  not, byte for byte, but names and numbers (`../README.md`, "Against S28").
