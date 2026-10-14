# E25-plain-nxs-master

- started: 2026-10-14T15:53:04Z (the first frame)
- link: `prolink-todos-net`, recorded with `pi-qemu link capture` (every frame once)
- description: an NXS playing a file off its plain stick takes master from a
  TriMixxx deck on prolink 0.5.0, twice: the deck, in sync, never takes its
  tempo, and takes master back.

## Devices

| | player | address | MAC | playing |
| --- | --- | --- | --- | --- |
| deck | 4 | 169.254.190.254 | 02:54:4d:ed:fe:82 | b's "Title Tag v24", streamed, 128.00 BPM by its own analysis |
| a | 1 | 169.254.45.24 | 02:43:44:6e:2d:18 | its own "02 Full v24.mp3" (id 361), then "03 ID3v1 only.mp3" (627) |
| b | 2 | 169.254.249.106 | 02:43:44:16:f9:6a | nothing |

An NXS playing a plain file says play state 3 with the playing flag clear,
no BPM (`0xffff`), and sends no beats it could be synced to (PLAIN-STICKS.md
§7.8): a master with no tempo.

## Timeline

Times are seconds from the first frame.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | | The deck master (flags `0xa4`), stopped at the end of its track. a at a plain file, `bpm=-`. |
| 4.29 | a: load its own id 361 | 4.968: a plays it: play state 3, flags `0x84` (the playing flag clear), `bpm=-`. |
| 21.73 | a: MASTER | 21.858: a asks the deck for master (`0x26` on 50001); 21.914: **a is master** (`0xa4`), with no tempo; 21.928: the deck lets go (`0x84`). a sends no beat packet in the whole session: its three packets on 50001 are the hand-overs' `0x26` and `0x27`. |
| 35.96-38.36 | deck: CUE, PLAY, SYNC | 36.798: the deck plays (`0xc4`); 38.503: in sync (`0xd4`), **at 128.00, its own**: never 655.35, the `0xffff` a sends, nor 0. |
| 44.76 | | a goes on to its next file (id 627) and stops for it; 44.789: the deck, playing, takes master (`0xf4`) as a stopped master hands it over. |
| 64.25 | a: MASTER | 64.413: a is master again; 64.464: the deck in sync and not master (`0xd4`), **still at 128.00**. |
| 81.29 | deck: MASTER | 81.433: the deck takes master (`0xf4`); a lets go. 81.904: its track ends. |
| 300 | (capture ends) | |

## What came of it

- **No tempo is taken from a plain-file master,** nor a phase: the deck in
  sync keeps its own 128.00 with a master that says `0xffff` (Sam's 07a).
- **Master moves both ways** between the deck and an NXS playing a plain
  file: MASTER on either takes it.

## Against the hardware

An NXS's own status for a plain file is E12's: play state 3, the playing
flag clear, `0xffff`. What a real NXS in sync does beside a master with no
tempo is in no capture.
