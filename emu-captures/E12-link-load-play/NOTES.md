# E12-link-load-play

- recorded: 2026-10-10T18:45:14Z
- rig: `../README.md`. A, player 2 at 169.254.173.12 (PLAINMP3 in its USB
  slot); B, player 1 at 169.254.209.145, no media. Two CDJ-2000NXS on firmware
  1.44, cdj2000-emulator `dbc4e31`, TriMixxx's pi-qemu at `3c0c2aa`
- DSP: A and B on the behavioural DSP (`--dsp-model`): B plays the track the
  moment it loads it, where a real NXS cues and waits; both tempo sliders
  centred before the capture's first action
- tap: `pi-qemu link capture` on the link, every frame once
- time: keep-alives A 33 at a median 2.000 s, B 41 at 2.000 s (a real NXS:
  2.003 s)
- commands: `cmd.txt`, which sources `../plain-stick/rig.sh`
- reading: `transcript.txt` (`../tools/emudump`); what these sessions show,
  together, is in `../PLAIN-STICKS.md`

B loads `01 Full v23.mp3` (full ID3v2.3 tags, a JPEG cover) from A's plain
stick over LINK, plays it, pauses, and shows INFO.

## What was done

Seconds from the capture's first frame; `fN` is frame N.

- 21.6 LINK, then `[FOLDER]`; the cursor down to the root's row 10
- 47.7 the load: `MENU_FOLDER` M3 and M4, `0x3100`, `GET_TRACK_INFO`,
  `GET_WAVEFORM_PREVIEW`, `GET_CUE_POINTS` (f658-f691)
- 47.8 B's status names the track (f697); 47.86 NFS `LOOKUP "?\0D 2 5"`
  (f712); 47.96 `GET_VBR_INDEX` (f734); 48.0 the first READs (f749)
- 48.2 B opens and reads `SHORT.MP3`, the file before it (`D 2 21`, f946)
- 48.4 play state 3 (f1215); 67.0 PLAY pauses it (f2671)
- ~75 INFO: `0x2202` with M1, all ten items (the tenth, bitrate 192)

## What came of it

- The load's requests, all with T2 (`PLAIN-STICKS.md` §7.1).
- `0x3100` answers 2: the file's index among the root's seven files, shown as
  `TRACK 003/007`.
- `GET_TRACK_INFO`, five items: container 1 (MP3) with the directory's
  cluster (2) in argument 0, the duration (60) with the file's directory
  entry slot (5) in argument 0, the BPM tag (12500), the comment, and a path
  item with the size (1 453 552) and an empty label (f682).
- The NFS name is `?` (UTF-16LE) then ASCII `D 2 5`: that cluster and slot, in
  hex; one LOOKUP on the export's root.
- The analysis: a 900-byte zero waveform preview, empty cue points, a
  1 604-byte zero VBR index; no beat grid or detailed waveform asked for.
- B read 1.21 MB of the 1.45 MB file in its first 17 s of playing.
- While playing: track type 2, id 6, no tempo published (`0xffff`), though
  the track info carried the tag's 125.00; the playing flag clear, and no
  beat packets, from a player that makes no sound (`PLAIN-STICKS.md` §7.8).
