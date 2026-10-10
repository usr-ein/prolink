# E16-rekordbox-control

- recorded: 2026-10-10T18:58:54Z
- rig: `../README.md`. A, player 2 at 169.254.173.12, with SAM1 in its USB
  slot: a copy of Sam's rekordbox stick, `~/.pi-qemu/cdj/sam1-usb.img`, made
  by `make_sd_image` from a copy of its files, so `PIONEER/` is not hidden and
  the image has no volume label but `CDJ2000` in its boot sector. B, player 1
  at 169.254.209.145, no media. Two CDJ-2000NXS on firmware 1.44,
  cdj2000-emulator `dbc4e31`, TriMixxx's pi-qemu at `3c0c2aa`
- DSP: A and B on the behavioural DSP (`--dsp-model`); both tempo sliders
  centred before the capture's first action
- tap: `pi-qemu link capture` on the link, every frame once
- time: keep-alives A 68 at a median 2.000 s, B 78 at 2.000 s (a real NXS:
  2.003 s)
- commands: `cmd.txt`, which sources `../plain-stick/rig.sh`
- reading: `transcript.txt` (`../tools/emudump`); what these sessions show,
  together, is in `../PLAIN-STICKS.md`

The control session: the same rig and keys as E11 and E12, with a rekordbox
stick in A instead of the plain one. It shows what differs between the two,
and whether the emulated NXSs exchange what the real ones of `../../captures/`
do.

## What was done

Seconds from the capture's first frame; `fN` is frame N.

- 20.2 the media response (f49); 23.4 LINK: `MENU_ROOT` with T1, twelve
  categories (f114)
- 23-67 the cursor down the root menu, a second on each category (B
  previews each with M2)
- 65.8 FOLDER's preview answers 0 rows (f815); it does not open
- 121.9 TRACK opened, 141 tracks (f1543)
- 125.5 `AM FM` loaded (f1630): it plays at once and takes master
- B reads `Aaron2`, the track before it, too (f1972)
- ~140 INFO

## What came of it

- The media response: `SAM1` (the export's name), created `2026-09-29`, 141
  tracks, 4 playlists, `01 01` at `0xaa`-`0xab`.
- Twelve root categories, the eleven B rendered in S20's order, without
  DATE ADDED between BITRATE and TRACK.
- **FOLDER is empty on this rekordbox stick**, though `Contents/` and
  `PIONEER/` are on it (E17 shows why).
- The load: `GET_TRACK_INFO` with six items and the path (f1636),
  `GET_METADATA 0x2002` with thirteen (f1650), the path walked by LOOKUP, a
  component at a time (f1675-f1681), a VBR index ending in the sample count
  (f1696), `GET_BEAT_GRID`, `GET_WAVEFORM_DETAIL`. The shape of S06's and
  S20's.
- B's status: track type 1, id 58, tempo 175.01, playing and master
  (`0xe4`); 43 beat packets.
- `PLAIN-STICKS.md` §9.
