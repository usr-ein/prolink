# E09-master-pressed-paused

- started: 2026-10-10T18:29:20Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: MASTER pressed on the master itself, with the other deck
  playing; then MASTER pressed on a stopped deck, while the master plays;
  then that deck plays; then MASTER on the other again, and SYNC on both.

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded, then b's; each played as it loaded,
and a became master. Neither deck is in sync until 52 s, so each plays at its
own tempo: a 126.00, b 132.00.

The Mac's load averages were 7.5 at the start and the end. Keep-alives came
every 2.0001-2.0002 s (median; 34 intervals each, 1.9903-2.0102 s).

## Timeline

Times are seconds from the first frame; each press lands between the step's
start and the end of its `pi-qemu` command, about 0.13 s later. PLAY toggles
play and pause.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00. b: 0xc4 (playing), 132.00. |
| 4.913 | a, the master: MASTER | 5.032: a's status `0x9e` = 1, **`0x9f` = 2**. 5.047 (+15 ms): b's status 0xe4 (playing, master). 5.096 (+64 ms): a's status 0xc4, `0x9e` = 0. **MASTER on the master hands master to the other playing deck,** in the status packets alone: no 0x26/0x27. Each deck keeps its own tempo. |
| 9.915 | b, the master now: PLAY, so b stops | 10.040: b's status 0xa4 (master, not playing), `0x9f` = 1. 10.065 (+25 ms): a's status 0xe4, `0x9e` = 1. 10.103 (+63 ms): b's `0x9e` = 0. (A master that stops, as in E05.) |
| 15.915 | b, stopped: MASTER | 16.034: **0x26** b to a. 16.042 (+8.0 ms): **0x27** a to b, granted. +8.2 ms: a's `0x9f` = 2. +63.7 ms: b's status **0xa4 (master, not playing)**, `0x9e` = 1. +79.9 ms: a's status 0xc4 (playing), `0x9e` = 0. **A stopped deck can ask for master, and is given it,** while the deck that gives it up plays on. |
| 27.915 | b: PLAY | 28.082: b's status 0xe4 (playing, master), 132.00. |
| 39.915 | a: MASTER | 40.020: 0x26 a to b. 40.021 (+0.7 ms): 0x27, granted. +1.0 ms: b's `0x9f` = 1. +48.0 ms: a's `0x9e` = 1. +64.6 ms: b's `0x9e` = 0. |
| 51.914 | b: SYNC | 52.033: b's status 0xd4 (playing, sync); its pitch goes to -4.55%: 126.00. Its beats were 0.58 beat after a's. **For four beats (52.681-54.277) its beat packets say -14.49%** (112.87 BPM), 10 points under its status and beyond the ±10% its slider allows, while its status says -4.55%: b holds back until a's beats have caught up with its own. From 54.754 its beats come 1 ms from a's, at -4.55%. This is how an emulated NXS gets into phase (`../README.md`); a real one jumps. |
| 59.917 | a, the master: SYNC | 60.036: a's status 0xf4 (playing, master, sync). |
| 70.0 | (capture ends) | |

The MASTER lamps: b lit at 5.04 and a dark at 5.10; a lit at 10.06, b dark
at 10.08; b lit at 16.06 (stopped), a dark at 16.16; a lit at 40.13, b dark
at 40.14.

## What came of it

- **MASTER on the master gives master away** to the other deck that plays,
  the way a master that stops does: `0x9f` names it, nothing on 50001.
- **A stopped deck may take master** by MASTER: the ordinary 0x26/0x27
  exchange, granted, and from then on a stopped master and a playing
  follower (not in sync here, so it kept its own tempo).
- The two request hand-overs: 0x27 at +8.0 and +0.7 ms, `0x9f` with it,
  the successor's `0x9e` at +63.7 and +48.0 ms, the old master's cleared at
  +79.9 and +64.6 ms.
- **SYNC's way into phase went past the tempo range:** b's beat packets said
  -14.49% for four beats (52.681-54.277) against its status's -4.55%, the
  largest bend in these sessions.

## Against S28

- **Getting into phase:** a real NXS put in sync jumps into phase on its next
  beat (S28 102.269); b here played four beats at 112.87 BPM instead.
- **Neither MASTER on the master nor MASTER on a stopped deck happened in
  S28:** all five of its requests came from a deck that was playing and not
  master. What this session shows for both rests on the emulator.
- **The request hand-overs** are S28's sequence, within its timings
  (0x27 2.4-5.0 ms, successor 16-73 ms, old master cleared 67-133 ms), the
  first 0x27 a little later (8.0 ms).
