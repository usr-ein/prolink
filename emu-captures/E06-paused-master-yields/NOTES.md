# E06-paused-master-yields

- started: 2026-10-10T18:19:16Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: a master, b in sync. Both stop, a last, so the master is
  paused with nobody playing. b (in sync) plays: does the paused master hand
  over? Then the same with the roles changed and sync off on the deck that
  plays: b master, both stopped, a (not in sync) plays.

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded, then b's; each played as it loaded,
and a became master.

The Mac's load averages were 6.9 at the start and 6.5 at the end. Keep-alives
came every 2.0001 s (median; 49 intervals each, 1.9998-2.0022 s).

## Timeline

Times are seconds from the first frame; each press lands between the step's
start and the end of its `pi-qemu` command, about 0.13 s later. PLAY toggles
play and pause.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00. b: 0xc4 (playing), 132.00. |
| 3.807 | b: SYNC | 3.969: b's status 0xd4 (playing, sync), pitch -4.55%: 126.00. |
| 7.803 | b: PLAY, so b stops | 7.913: b's status 0x94 (sync, not playing), play state 0x05. |
| 13.807 | a (master): PLAY, so a stops | 13.935: a's status 0xa4 (master, not playing), `0x9e` = 1, `0x9f` = 0xff. **Nobody is playing, and a keeps master:** it names no successor. |
| 23.805 | b (in sync): PLAY | 23.947: b's status 0xd4 (playing, sync). |
| | | 23.983 (+36 ms): a's status 0xa4, `0x9e` = 1, **`0x9f` = 2**: the paused master hands master to the deck that started playing. |
| | | 24.011 (+64 ms): b's status 0xf4 (playing, master, sync). 24.047 (+100 ms): a's status 0x84, `0x9e` = 0. No 0x26/0x27. The MASTER lamps changed at 24.04 (a dark) and 24.05 (b lit). |
| 35.806 | a: PLAY, not in sync | 35.919: a's status 0xc4 (playing), not master, at its own 126.00. |
| 47.803 | a: PLAY, so a stops | 47.921: a's status 0x84, play state 0x05. |
| 53.805 | b (master, in sync): PLAY, so b stops | 53.972: b's status 0xb4 (master, sync, not playing), `0x9e` = 1, `0x9f` = 0xff: nobody is playing, b keeps master. |
| 63.804 | a (**not** in sync): PLAY | 63.916: a's status 0xc4 (playing), `0x9e` = 0. **b, paused, keeps master** (`0x9e` = 1, `0x9f` = 0xff in every packet up to 77.963): it does not hand over to a deck that plays out of sync. a plays at its own 126.00. |
| 77.804 | b: PLAY | 77.963: b's status 0xf4 (playing, master, sync). a, not in sync, stays where it is: its beats 0.24 beat from b's. |
| 99.9 | (capture ends) | |

## What came of it

- **A master that stops while nobody else plays keeps master** (13.9, 53.9).
- **A paused master hands master to a deck that starts playing in sync**
  (23.9): it names that deck in `0x9f` 36 ms after the deck's first "playing"
  packet; the deck claims `0x9e` 28 ms after that; the old master clears it
  36 ms later. Nothing on 50001.
- **It does not hand over to a deck that starts playing out of sync** (63.9):
  b stayed master, paused, for the 14 s that a played alone.
- Put with E01 (a master that stops while another deck plays hands over to
  it, in sync or not): what decides is whether the master stops while
  someone else plays, or someone starts playing while the master is stopped;
  only in the second case does sync matter.

## Against S28

- **A paused master and a deck that starts playing in sync:** S28 208.276 is
  the same. Deck 1 (in sync) started playing; deck 2, the paused master,
  named deck 1 in `0x9f` 4 ms later; deck 1 claimed `0x9e` 63 ms after it
  started; deck 2 cleared its own 76 ms after (here: 36, 64 and 100 ms).
- **A deck that starts playing out of sync** beside a paused master never
  happened in S28: what this session shows for it rests on the emulator.
- **Stopped,** the emulated decks keep pitch copies 2 and 4 non-zero; S28's
  read 0 there while paused (`../README.md`).
