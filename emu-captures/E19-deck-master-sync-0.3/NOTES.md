# E19-deck-master-sync-0.3

- started: 2026-10-10T20:40:19Z (the first frame)
- link: `prolink-learnings-net`, recorded with `pi-qemu link capture` (every frame once)
- description: tempo master and sync between a TriMixxx deck on prolink 0.3.0
  and two NXSs: the first play, MASTER both ways, the deck stopping as master
  beside a deck out of sync and in sync, SYNC on the deck, its master
  stopping. E21 runs the same kinds of steps on 0.4.0.

## Devices

| | player | address | MAC | media | track |
| --- | --- | --- | --- | --- | --- |
| deck | 4 | 169.254.95.108 | 02:54:4d:27:ef:06 | SAM1 in its own USB | Inigo Kennedy, "Kepler", id 132, 128.00 BPM |
| a | 1 | 169.254.5.189 | 02:43:44:13:05:bd | SAM1 in USB | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.230.162 | 02:43:44:92:e6:a2 | SAM1 in USB | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture: E18's two NXSs, their sliders then centred (`../README.md`,
"Before every session"), nothing loaded on them; on the deck, SAM1 in its own
USB, Kepler loaded from it and cued. Nobody master. The deck's controls are
its touch screen and its emulated panel (`cmd.txt`).

a still re-claims its number every 3.8-6.3 s here, as in E18, its keep-alives
5.2 s apart; the deck's and b's came every 2.000 s (median). The Mac's load
was not recorded.

## Timeline

Times are seconds from the first frame; each press lands between the step's
start and the end of its `pi-qemu` command. "Named" is the old master's
status `0x9f`; "claims" is `0x9e` = 1.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | The deck cued, not master; a and b idle. |
| 4.53 | deck: PLAY | 4.619: the deck plays, not master. 10.618: the deck says master, **6.0 s after it started playing**. |
| 19.53 | a: load Kosh | 37.548: a plays (an emulated NXS plays what it loads), not master. |
| 49.53 | a: MASTER | 49.644: a's `0x26` to the deck; 49.645: the deck's `0x27`, granted. 49.816: the deck names a; 49.877: a claims; 50.017: the deck lets go. |
| 64.53 | deck: MASTER | 64.790: the deck's `0x26` to a; 64.794: a's `0x27`, granted, and a names the deck. 65.017: the deck claims; 65.074: a lets go. |
| 79.53 | deck: PLAY (stops) | 79.615: the deck stops, **still master, naming nobody**, while a plays out of sync. It keeps master until it plays again (94.816). |
| 94.54 | deck: PLAY | 94.816: the deck plays, master. |
| 99.53 | a: SYNC | 99.692: a in sync, +1.59%: 128.00. |
| 114.53 | deck: PLAY (stops) | 114.616: the deck stops and names a in the same packet; 114.686: a claims; 114.816: the deck lets go. |
| 129.53 | deck: PLAY | 129.616: the deck plays, not master. |
| 134.53 | deck: SYNC | 135.016: the deck in sync on a. |
| 154.53 | a: PLAY (stops) | 154.665: a stops and names the deck; 155.016: the deck claims; 155.032: a lets go. |
| 174.53 | a: PLAY | 174.662: a plays, in sync. |
| 184.53 | b: load Booster | 209.019: b plays, not master, out of sync. |
| 214.53 | deck: SYNC off | 214.816: the deck master, out of sync. |
| 224.53 | deck: PLAY (stops) | 224.615: the deck stops and names a, the deck in sync (b plays out of sync); 224.681: a claims; 224.815: the deck lets go. |
| 249.6 | (capture ends) | |

## What came of it

On 0.3, the deck takes and gives master by its own rules, at its own pace:

- **The first play took master 6.0 s late**: D2's delay, which waits for a
  master that might already be playing.
- **Hand-overs took 284-373 ms** from the `0x26` to the old master letting
  go. The deck named its successor 171 ms after its own `0x27`, and claimed
  223-351 ms after being named. Its status goes out every 200 ms, and the
  fork's glue checks for news every 100 ms.
- **A stopped master gave master only to a deck in sync** (114.616,
  224.615). Beside a deck playing out of sync it kept master (79.615-94.816).

## Against the hardware

- **The first play:** S28's first deck to play took master at once (22.058),
  and so do emulated NXSs (E01 23.269).
- **Hand-overs:** S28's five completed in 67-133 ms. Each old master named its
  successor within 1 ms of its `0x27`, and the new one claimed 16-73 ms after
  asking (`../README.md`, "Against S28").
- **A stopped master** hands master to the deck that plays, in sync or not
  (S28 193.655; E01, E05), unless it is in sync itself and that deck is not
  (E06 A-E).
