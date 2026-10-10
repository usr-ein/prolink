# E02-sync-follows-master-tempo

- started: 2026-10-10T18:32:26Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: a master at 126.00, b playing at 132.00. SYNC on b: b follows
  a's tempo. a's fader up 5.1%, down to -5.1%, back to 0: b follows. b's own
  fader moved while in sync. SYNC off on b, then a's fader again. Then b's
  own fader, out of sync: up, away from the tempo b holds, then down
  through it, and back to 0.

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded, then b's; each played as it loaded,
and a became master.

The slider ("fader" below) is the panel's analogue field 2, moved by hand:
a value every 0.1 s (`cmd.txt`). 0 is -10%, 32768 0%, 65535 +10%; the pitch
it gives moves in steps of 0.05%.

The Mac's load averages were 7.9 at the start and 6.8 at the end. Keep-alives
came every 2.0001 s (median; 61 intervals each, 1.9997-2.0050 s).

## Timeline

Times are seconds from the first frame (here 1.1 s after the capture
started); each press lands between the step's start and the end of its
`pi-qemu` command, about 0.13 s later.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00, pitch 0. b: 0xc4 (playing), 132.00, pitch 0. |
| 3.900 | b: SYNC | 4.032: b's status 0xd4 (playing, sync), and **in the same packet** pitch -4.55% (0x000f45d1): 132.00 x 0.95455 = 126.000, a's tempo. b's beat packets then say -5.45% (124.8) for four beats while its status says -4.55%: from 6.291 its beats come with a's (0.0 ms), and b plays at -4.54%. |
| 13.903-15.909 | a's fader 0% to +5.1% over 2 s | a's pitch steps up by 0.05%-0.30% a packet to +5.10% (132.426 at 16.43). After each of a's steps, b's status brings its own pitch to the same tempo, a few ms later: +0.32% (132.00 x 1.00320 = 132.426) at 16.47. |
| 23.898-26.903 | a's fader +5.1% to -5.1% over 3 s | a to -5.10% (119.574); b to -9.41% (119.574), inside its ±10%. |
| 33.898-35.903 | a's fader back to 0% over 2 s | a to 0% (126.00); b to -4.55% (126.00). |
| 43.901-45.904 | b's fader 0% to +5.1% over 2 s, b in sync | **Nothing**: b's pitch stays -4.55%, no status packet changes. |
| 53.900-55.904 | b's fader back to 0% | Nothing. |
| 63.901 | b: SYNC off | 64.163: b's status 0xc4 (playing). **b stays at -4.55% (126.00),** not at its fader's 0% (132.00). Pitch copies 2 and 4 read 0x000f4546 in that packet, then pitch 1 again. |
| 73.898-75.902 | a's fader 0% to +5.1% | a to +5.10% (132.426); **b, out of sync, stays at 126.00.** Their beats drift apart. |
| 83.899-85.902 | a's fader back to 0% | a to 126.00 again; b still 126.00, now 0.07 beat from a's beats. |
| 93.902-95.908 | b's fader 0% to +5.1%: away from the -4.55% it holds | Nothing: b stays at -4.55%. |
| 101.901-106.909 | b's fader +5.1% down to -10% over 5 s, through -4.55% | b holds -4.55% until its fader reaches it (by the ramp, about 105.05); at 105.112 its pitch moves to -4.65%, and from then follows the fader, to -10.00% (118.800) at 107.47. |
| 111.899-114.901 | b's fader -10% back to 0% over 3 s | b follows the fader, to 0% (132.00) at 115.43. |
| 124.0 | (capture ends) | |

The SYNC lamp on b lit at 4.09 and went dark at 64.20; the MASTER lamps did
not change.

## What came of it

- **SYNC takes up the master's effective tempo** at once: here in the very
  packet that lights sync (E03: the next one, 64 ms later).
- **The follower follows each of the master's tempo changes:** 70 of a's
  changes while b was in sync, each matched in b's status, to 0.005 BPM,
  4.8-65 ms later (median 8.6 ms). `bpm x pitch` is the master's to the last
  digit in every one.
- **A follower's own fader does nothing** while in sync.
- **Out of sync, a deck keeps the tempo it had,** and its fader does nothing
  until it passes that tempo; from there the tempo follows the fader. Moving
  the fader away from the held tempo does nothing (93.9-95.9). The same
  catch-up as a new master's (E04).
- **Beat phase:** from the fifth beat after SYNC, b's beats fell within 0.01
  beat of a's (10-s windows: mean -0.0014 to +0.0023 beat, sd 0.002-0.005)
  through all of a's fader moves. Out of sync, they drifted, and stayed 0.07
  beat apart once both were back at 126.00.

## Against S28

- **SYNC on:** S28's follower (102.111) lit sync, and its pitch reached the
  master's tempo one packet later (+65 ms); here the same packet. Both
  happen in the emulated decks (E03 is the other).
- **The master's fader:** S28 130.8-138.5 is the same picture, a master's
  pitch stepping in 0.05% units on ~64 ms ticks and its follower matching
  each in its next status packet. S28's follower matched 276 changes,
  2.2-68 ms later (median 6.0 ms; 10 skipped in fast moves); here 4.8-65 ms
  (median 8.6 ms), none skipped. S28's follower also kept pitch 4 one step
  behind pitch 1 while slewing; the emulated one moves copies 2 and 4 near,
  not equal to, pitch 1.
- **A follower's fader doing nothing** is in S28's own notes ("when a cdj is
  not the master, its tempo fader does nothing"); the catch-up out of sync
  was not tried in S28.
- **Catching the phase:** S28's follower was in phase on its first beat
  after SYNC (it jumped); here b bent its tempo for four beats (-5.45%) to
  get there (`../README.md`).
