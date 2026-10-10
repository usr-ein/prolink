# E05-master-stops

- started: 2026-10-10T18:16:00Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: both decks playing, b in sync, a master. The master stops
  (PLAY pressed): who is master then? a plays again, out of sync, then in
  sync; b, master by then, stops and plays again; a, master again, stops and
  plays again.

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded, then b's; each played as it loaded,
and a became master.

The Mac's load averages were 7.6 at the start and 6.9 at the end. Keep-alives
came every 2.0001 s (median; 46 intervals each, 1.9995-2.0027 s).

## Timeline

Times are seconds from the first frame; each press lands between the step's
start and the end of its `pi-qemu` command, about 0.13 s later. PLAY toggles
play and pause.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00. b: 0xc4 (playing), 132.00. |
| 4.388-4.515 | b: SYNC | 4.500: b's status 0xd4 (playing, sync); its pitch goes to -4.55%: 126.00. |
| 11.397-11.521 | a (master, not in sync): PLAY, so a stops | 11.532: a's status 0xa4 (master, not playing), play state 0x05, `0x9e` = 1, **`0x9f` = 2**: the packet that says a stopped names b. 11.600 (+68 ms): b's status 0xf4 (playing, master, sync). 11.659 (+127 ms): a's status 0x84, `0x9e` = 0. No 0x26/0x27 on 50001. b, master now, stays at 126.00. |
| 24.397-24.533 | a: PLAY | 24.547: a's status 0xc4 (playing): not master, not in sync, at its own 126.00, the tempo b plays. Its beats fall 160 ms (0.33 beat) after b's, and stay there: nothing pulls them. |
| 34.389-34.521 | a: SYNC | 34.542: a's status 0xd4 (playing, sync), pitch -0.00% (0x000fffff). a's beat packets then say +8.49% (136.7 BPM) for five beats while its status says -0.00%: a catches up b's phase, and from 36.69 its beats fall within 5 ms of b's. |
| 44.389-44.520 | b (master, in sync): PLAY, so b stops | 44.524: b's status 0xb4 (master, sync, not playing), `0x9e` = 1, **`0x9f` = 1**. 44.590 (+66 ms): a's status 0xf4 (playing, master, sync). 44.604 (+80 ms): b's status 0x94 (sync), `0x9e` = 0. |
| 57.392-57.526 | b: PLAY | 57.549: b's status 0xd4 (playing, sync), -4.55%. It starts where it stopped, out of phase (its first beat 0.29 beat after a's); its beat packets say +2.90% (135.8 BPM) for five beats, and from 59.553 its beats fall within 4 ms of a's. |
| 69.392-69.529 | a (master, in sync): PLAY, so a stops | 69.511: a's status 0xb4 (master, sync, not playing), `0x9e` = 1, **`0x9f` = 2**. 69.549 (+38 ms): b's status 0xf4 (playing, master, sync). 69.575 (+64 ms): a's status 0x94 (sync), `0x9e` = 0. |
| 81.387-81.515 | a: PLAY | 81.495: a's status 0xd4 (playing, sync). Its first beat (81.540) falls 80 ms after b's; its beat packets say +4.18% (131.3 BPM) for five beats, and from 84.318 its beats fall within 3 ms of b's. |
| 94.5 | (capture ends) | |

The lamps: a's MASTER lamp went dark at 11.62 and b's lit at 11.59; a's
lit at 44.56 and b's went dark at 44.62; b's lit at 69.61 and a's went dark
at 69.59. A stopped deck's CUE lamp lit about 0.3 s after its PLAY lamp went
dark.

## What came of it

- **A master that stops hands master to the deck that is playing,** whether
  the master itself was in sync (44.5, 69.5) or not (11.5). It names the
  successor in `0x9f` in the very packet that says it stopped; the successor
  claims `0x9e` 38-68 ms later; the old master lets go 64-127 ms after naming
  it. Nothing on 50001: no request, no response.

  | stop | successor named | successor `0x9e` = 1 | old master `0x9e` = 0 |
  | --- | ---: | ---: | ---: |
  | 11.532, a | in the stop packet | +68 ms | +127 ms |
  | 44.524, b | in the stop packet | +66 ms | +80 ms |
  | 69.511, a | in the stop packet | +38 ms | +64 ms |

- **The new master keeps the tempo:** 126.00 throughout.
- **A deck that plays again does not take master back**, in sync or not.
- **A deck in sync that starts again, or turns SYNC on, catches the master's
  phase by bending its tempo** for five beats or so (+2.9% to +8.5% in
  its beat packets; its status packets keep the target pitch), then plays in
  phase.

## Against S28

- **A master that stops:** S28 193.655 is the same sequence. Deck 1 (master,
  in sync) stopped with deck 2 playing in sync; deck 1 named deck 2 in `0x9f`
  in its stop packet, deck 2 claimed `0x9e` 37 ms later, deck 1 dropped it 64
  ms after naming it (here: 38-68 ms and 64-127 ms). S28 never stopped a
  master that was not in sync; here (11.5) it made no difference.
- **Catching the phase is not the same.** S28's decks are in phase from
  their first beat after SYNC: at 102.111 deck 1 was 0.12-0.15 beat behind
  deck 2, and its next beat (102.269) came 339 ms after the one before, not
  414 ms, and 0.1 ms after deck 2's. The real deck jumps its playhead into
  phase; the emulated one bends its tempo for several beats to get there
  (the README's "What an emu-capture can't stand for").
- **Stopped,** the emulated decks keep pitch copies 2 and 4 at the pitch; S28's
  decks read 0 there while paused (`../README.md`).
