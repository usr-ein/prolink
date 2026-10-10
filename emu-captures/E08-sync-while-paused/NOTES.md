# E08-sync-while-paused

- started: 2026-10-10T18:26:25Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: a master and playing. b stops, then SYNC is pressed on it
  while it is stopped; the master's tempo changes while b is stopped; then b
  plays. Does a stopped deck follow the master? Does it start in tempo, and
  in phase?

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded, then b's; each played as it loaded,
and a became master.

The Mac's load averages were 6.8 at the start and 7.5 at the end. Keep-alives
came every 2.0002 s (median; 35 and 36 intervals, 1.9998-2.0667 s).

## Timeline

Times are seconds from the first frame (here 1.0 s after the capture
started); each press lands between the step's start and the end of its
`pi-qemu` command, about 0.13 s later. PLAY toggles play and pause.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00. b: 0xc4 (playing), 132.00. |
| 2.985 | b: PLAY, so b stops | 3.185: b's status 0x84, play state 0x05. |
| 8.983 | b: SYNC, while stopped | 9.107: b's status 0x94 (sync, not playing); its pitch goes to -4.55% (126.00), a's tempo. |
| 16.983-18.988 | a's fader 0% to +5.1% over 2 s | **b follows while stopped**: its status pitch steps with a's (-4.35%, -4.12%, ...), each a's tempo, to +0.32% (132.426) at 20.44. Pitch copies 2 and 4 stay at 0x00100000 all along, the value from before b stopped. |
| 26.985 | b: PLAY | 27.146: b's status 0xd4 (playing, sync), pitch +0.32%: it starts at the master's tempo. Its first beat (27.631) comes 9.8 ms (0.022 beat) before a's; its beat packets say -0.18% for four beats, and from 29.000 its beats fall within 2.3 ms of a's. |
| 43.981-45.984 | a's fader back to 0% over 2 s | b follows, to -4.55% (126.00), packet by packet: 40 changes of a's tempo in this capture, each matched in b's status 0.8-66 ms later (median 9.2 ms). |
| 58.981 | b: PLAY, so b stops | 59.140: b's status 0x94 (sync, not playing), -4.55%. Pitch copies 2 and 4 read 0x000f4690, not pitch 1. |
| 74.1 | (capture ends) | |

## What came of it

- **SYNC works on a stopped deck:** its status takes up the master's tempo
  at once, and follows every change of it while stopped.
- **A stopped deck in sync starts at the master's tempo,** and here nearly
  in phase: 0.022 beat off on its first beat, then a slight bend for four
  beats. (In E05 a deck that had stopped while in sync started 0.29 beat off
  and bent +2.9% for five beats. The difference between the two is that
  here SYNC was pressed while the deck was stopped.)
- **While stopped, pitch copies 2 and 4 keep the value they had when the
  deck stopped** (0x00100000) while pitch 1 follows the master. In the packet
  that says it plays again they read 0; after that they move with pitch 1,
  a few units off it, as a synced deck's do while playing.

## Against S28

- **S28 never pressed SYNC on a stopped deck,** nor moved the master's fader
  while a synced deck was stopped: those two rest on the emulator.
- **A synced deck starting:** S28 210.018, deck 2 (in sync) started while
  deck 1 played; its first beat (210.354) came 0.2 ms after deck 1's, and
  its beat packets carried its target pitch (+2.215%) from the first: in
  phase from the start, with no bend. Here: 9.8 ms off, then four beats of
  bend.
- **Pitch copies while stopped:** S28's decks zero copies 2 and 4 while
  paused (274 of 275 paused packets); here they hold a stale value.
