# E04-handoff-tempo-catch-up

- started: 2026-10-10T18:12:23Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: both decks in sync, a master with its fader at -5.1%. b takes
  master with its own fader at 0%: does the tempo jump to b's fader, or wait
  for the fader to come to it? b's fader is moved down through the tempo and
  back. Then a takes master back, its fader at 0%, and moves it up through
  the tempo and back. What S28's own notes describe: "when it takes the
  master, but its tempo fader is at a different position than that of the
  other cdj, it needs to 'catch' the tempo from the other one".

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded, then b's; each played as it loaded,
and a became master.

The slider ("fader" below) is the panel's analogue field 2, moved by hand:
`panel_control analog 2 V` every 0.1 s (`cmd.txt`). 0 is -10%, 32768 is 0%,
65535 is +10% (the factory tempo range, ±10%); the pitch it gives moves in
steps of 0.05%.

The Mac's load averages were 8.5 at the start and 7.9 at the end. Keep-alives
came every 2.0001 s (median; 56 intervals each, 1.9998-2.0116 s).

## Timeline

Times are seconds from the first frame; each press lands between the step's
start and the end of its `pi-qemu` command, about 0.14 s later.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00, pitch 0. b: 0xc4 (playing), 132.00, pitch 0. |
| 4.807-4.959 | b: SYNC | 4.943: b's status 0xd4 (playing, sync); the next packets put its pitch at -4.55%: 126.00. **Its beat packets say -5.45% for four beats (5.689-7.129)** while its status says -4.55%: b slows by 0.9 points to bring its beats onto a's (an emulated NXS's way into phase, `../README.md`), then plays at -4.55%. |
| 7.807-7.938 | a: SYNC | 7.928: a's status 0xf4 (playing, master, sync). |
| 11.806-13.809 | a's fader 0% to -5.1% over 2 s | a's pitch steps down to -5.10% (119.574); b follows each step, to -9.41% (132.00 x 0.90586 = 119.574). |
| 21.807-21.949 | b: MASTER | 21.937: 0x26 b to a. +7.6 ms: 0x27, granted. +7.8 ms: a's `0x9f` = 2. +24.2 ms: b's `0x9e` = 1. +71.7 ms: a's `0x9e` = 0. **b, now master, stays at -9.41%: 119.574,** the tempo that was playing. Its fader is at 0% (132.00), and is not taken up. |
| 31.809-37.815 | b's fader 0% down to -10% over 6 s | **Nothing, until the fader reaches the tempo.** b's pitch holds at -9.41% while its fader travels from 0% to -9.41%. At 37.437 (by its ramp the fader passed -9.41% at about 37.44) b's pitch moves to -9.50%, and from then follows the fader: -9.70%, -9.85%, -10.00% at 37.717 (118.800). a follows, to -5.71%. |
| 44.813-48.815 | b's fader -10% back to 0% over 4 s | b's pitch follows the fader all the way, to 0% (132.00); a follows, to +4.76% (126.00 x 1.04762 = 132.000). |
| 54.810-56.814 | a's fader -5.1% to 0% over 2 s, a a follower in sync | Nothing: a stays at +4.76%. **A follower's fader does nothing.** |
| 64.807-64.944 | a: MASTER | 64.930: 0x26 a to b. +5.5 ms: 0x27, granted. **+61.6 ms:** b's `0x9f` = 1. +127.7 ms: a's `0x9e` = 1. +141.5 ms: b's `0x9e` = 0. a, now master, stays at +4.76%: 132.00. Its fader is at 0% (126.00). |
| 74.807-79.812 | a's fader 0% to +10% over 5 s | a's pitch holds at +4.76% until its fader passes it (about 77.15, by the ramp); at 77.211 it moves to +4.90%, then follows the fader to +10.00% (138.600). b follows, to +5.00%. |
| 87.806-91.809 | a's fader +10% back to 0% over 4 s | a follows its fader to 0% (126.00); b follows, to -4.55%. |
| 114.9 | (capture ends) | |

## What came of it

- **A deck that takes master keeps the tempo that was playing.** Its own
  fader is not taken up until it moves through that tempo: from there on, the
  tempo follows the fader. Both hand-overs show it, down (b, at 37.437) and
  up (a, at 77.211), each in the first status packet after its ramp reached
  the held pitch (the packets go out on ~64 ms ticks).
- **Until then the master's fader does nothing** on the wire: no pitch change
  in its status, nothing for its follower to follow.
- **A follower's fader does nothing either** (54.8-56.8).
- **The follower tracks the master's tempo packet by packet:** each of the
  master's 130 changes of tempo was matched by the follower's status, to
  0.005 BPM, 4.8-64 ms later (median 7.9 ms).
- **Beat phase,** once b's four-beat bend after SYNC was done (7.13), stayed
  within 0.01 beat through all of it (10-s windows:
  mean -0.0025 to +0.0016 beat, sd 0.002-0.005), the catch-ups included.
- In the second hand-over (64.930) the old master named its successor
  (`0x9f`) 56 ms after its 0x27, on its next status tick, not with the 0x27.

## Against S28

- **The catch-up is what the real decks did,** in S28's notes. S28 has
  it at 156.542: deck 1 took master at pitch -2.86%, its follower's pitch,
  and held it until 163.18, when its pitch began to move in 0.05% steps, the
  fader having reached it. S28 does not record where the fader was, so this
  session is the first to show the moment the tempo is caught against the
  fader's own position.
- **The hand-overs** are S28's sequence (E03 has the comparison). Here the
  second one named its successor 56 ms after the 0x27; in all five of S28's
  the old master did it within 1 ms of its 0x27.
- **A follower's lag:** S28's follower matched its master's tempo changes 2-68
  ms later (276 changes, median 6.0 ms), here 4.8-64 ms (median 7.9 ms).
