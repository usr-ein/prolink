# E13-link-load-pioneer-dsp

- recorded: 2026-10-10T18:47:14Z
- rig: `../README.md`. A, player 2 at 169.254.173.12 (PLAINMP3 in its USB
  slot); B, player 1 at 169.254.209.145, no media. Two CDJ-2000NXS on firmware
  1.44, cdj2000-emulator `dbc4e31`, TriMixxx's pi-qemu at `3c0c2aa`
- DSP: B on Pioneer's own DSP code, interpreted (~0.04x), as `pi-qemu cdj up`
  runs it by default; A on the behavioural DSP (it only serves); both tempo
  sliders centred before the capture's first action
- tap: `pi-qemu link capture` on the link, every frame once
- time: keep-alives A 48 at a median 2.000 s; B 55 at 2.033 s, 1.994-2.433 s
  apart, its interpreted DSP taking the cores (a real NXS: 2.003 s)
- commands: `cmd.txt`, which sources `../plain-stick/rig.sh`
- reading: `transcript.txt` (`../tools/emudump`); what these sessions show,
  together, is in `../PLAIN-STICKS.md`

The load of E12 again, with B running Pioneer's DSP code: to see whether the
requests of a load depend on the DSP, and what B does after a load when its
DSP is the real one.

## What was done

Seconds from the capture's first frame; `fN` is frame N.

- 21.6 LINK, then `[FOLDER]`; the cursor down to the root's row 10
- 47.9 the load: the same requests as E12, in the same order (f670-f735)
- 48.0 B's status names the track, play state 2 (f691); 48.1 the LOOKUP
  `?\0D 2 5` (f713)
- 50.2 B opens and reads `SHORT.MP3`, the file before it (f980)
- 52.1 play state 3, with no PLAY pressed (f1198)
- 72.4 PLAY: play state 5 (f1760)
- 91.1 B opens and reads `Filename Artist - Filename Title.mp3`, the file
  before that (f2502)
- 105.6 PLAY: play state 3 (f3047)

## What came of it

- The dbserver and NFS requests of a load are the same with Pioneer's DSP
  code as with the behavioural DSP.
- B's screen after the load: cued at the start (REMAIN 01:00), waiting, and
  still there after PLAY: the interpreted DSP barely moves. Its status
  nevertheless went to play state 3 four seconds after the load, unprompted.
  So that is not only the behavioural DSP's doing; a real NXS's status after
  a load is not shown here.
- B read 0.61 MB of the file over the session, half of E12's.
- No tempo, no beat packets, the playing flag clear, as in E12.
