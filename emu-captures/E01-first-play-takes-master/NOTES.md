# E01-first-play-takes-master

- started: 2026-10-10T18:01:40Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: two emulated CDJ-2000NXS, nothing loaded. a loads a track,
  b loads one 47 s later. Who becomes tempo master, and when. Is master kept
  while paused? What does a paused master do beside a deck that plays but is
  not in sync?

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture, as in every session (`../README.md`, "Before every
session"), both were booted fresh and their TEMPO sliders were centred. Unlike
the other sessions, nothing was loaded before the capture: both decks start it
with no track, and nobody is master.

The Mac's load averages were 10.1 at the start and 11.3 at the end. Both
decks' keep-alives came every 2.0001 s (57 and 56 intervals, 1.9997-2.0022 s).

## Timeline

Times are seconds from the first frame. A press runs from the step's start to
the end of the `pi-qemu` command, about 0.13 s later: the press lands
somewhere in that window.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | Both decks' status: no track (play state 0x00), pitch 0x00100000, `0x9e` = 0. Nobody is master. |
| 2.904 | a: USB, six encoder detents to TRACK, ENTER, five detents to the track | (nothing of the browsing on the link: the stick is a's own) |
| 22.395 | a: ENTER loads it | 22.557: a's status says loading (0x02), track 1:3:77 (player 1, USB, id 77). Pitch copies 2 and 4 (`0x98`, `0xc4`) read 0 while it loads. |
| | | 23.269: a's status `0x89` = 0xe4 (playing, master), play state 0x03, `0x9e` = 1, BPM 126.00. **a plays without PLAY being pressed** (an emulated NXS plays what it loads: `../README.md`), and it is master in the very first packet that says it plays. No packet on 50001 but beats: nobody asked, nobody answered. |
| | | 23.504: a's first beat packet, 126.00 BPM, beat 1 of 4. Beats then every 0.4759-0.4762 s (60/126 = 0.47619). |
| 27.903-28.038 | a: PLAY (pause) | 28.077: a's status 0xa4 (master, not playing), play state 0x05 (paused), `0x9e` = 1. **Master kept while paused.** Its last beat was at 27.787. |
| 35.903-36.036 | a: PLAY | 36.037: a's status 0xe4 (playing, master). Beats from 36.262. |
| 43.903 | b: USB, browse to its track | |
| 70.087 | b: ENTER loads it | 70.252: b's status loading (0x02), track 2:3:116. |
| | | 70.965: b's status 0xc4 (playing), `0x9e` = 0. b plays by itself too, and **is not master**: a holds it. 71.409: b's first beat, 132.00 BPM. b is not in sync, and plays at its own 132.00. |
| 77.908-78.040 | a: PLAY (pause), while b plays, not in sync | 78.031: a's status 0xa4 (master, paused), play state 0x05, `0x9e` = 1, **`0x9f` = 2**: in the packet that says it paused, a names b as the deck it hands master to. |
| | | 78.045 (+14 ms): b's status 0xe4 (playing, master), `0x9e` = 1. |
| | | 78.095 (+64 ms): a's status 0x84, `0x9e` = 0, `0x9f` = 0xff. **No master request or response on 50001**: the hand-over is in the status packets alone. |
| 91.906-92.036 | a: PLAY | 92.023: a's status 0xc4 (playing), not master: b keeps it. Beats from 92.163. |
| 114.9 | (capture ends) | |

The panel lamps the firmware lit, polled every 0.1 s: a's MASTER lamp came on
at 23.24, b's at 78.10, and a's went off at 78.15.

## What came of it

- **The first deck to play takes master by itself,** with nothing sent on
  50001: a's first "playing" status packet already says master.
- **Master is kept while paused**, as long as no other deck is playing.
- **A master that stops hands master to a deck that is playing, even one not
  in sync.** It names the successor in `0x9f` in the packet that says it
  stopped; the successor claims `0x9e` 14 ms later, and the old master lets
  go of it 64 ms after it named the successor. No 0x26/0x27.
- **A deck that is not master, and not in sync, keeps its own tempo:** b
  played its track at 132.00 throughout, a at 126.00.

## Against S28

- **The first play.** S28's deck 2 also took master when it first played with
  nobody master (22.058), with no packet on 50001. It said so one status
  packet later, 65 ms after the packet that said it played (22.123); here it
  is the same packet. And S28's deck had been cued after its load (play state
  0x06) and was played by its PLAY key; here the deck played by itself when it
  loaded (an emulator difference, `../README.md`).
- **Master while paused:** the same (S28 22.923: 0xa4, play state 0x05,
  `0x9e` = 1).
- **A master that stops:** S28 193.655 is the same sequence: the stopping
  master names the other deck in `0x9f` in its "paused" packet, the other deck
  claims `0x9e` 37 ms later, and the old master drops it 64 ms after naming
  it (here: 14 ms and 64 ms). In S28 the other deck was in sync; here it was
  not, which S28 never tried.
- **While paused,** a's pitch copies 2 and 4 stayed at the pitch (all 144
  paused packets). On S28's decks both read 0 while paused (274 of 275 paused
  packets; the other is the one that says it paused). An emulator difference
  (`../README.md`).
