# E17-mixed-stick

- recorded: 2026-10-10T19:02:34Z
- rig: `../README.md`. A, player 2 at 169.254.173.12 (SAM1's export and
  `Contents/` with loose files beside them: `Loose/` (three MP3s of PLAINMP3)
  and `Loose root.mp3` (cmd.txt says how the image was made)); B, player 1 at
  169.254.209.145, no media. Two CDJ-2000NXS on firmware 1.44,
  cdj2000-emulator `dbc4e31`, TriMixxx's pi-qemu at `3c0c2aa`
- DSP: A and B on the behavioural DSP (`--dsp-model`); both tempo sliders
  centred before the capture's first action
- tap: `pi-qemu link capture` on the link, every frame once
- time: keep-alives A 49 at a median 2.000 s, B 58 at 2.000 s (a real NXS:
  2.003 s)
- commands: `cmd.txt`, which sources `../plain-stick/rig.sh`
- reading: `transcript.txt` (`../tools/emudump`); what these sessions show,
  together, is in `../PLAIN-STICKS.md`

A rekordbox stick with loose files beside its export, which rekordbox never
saw: B browses it over LINK by FOLDER, loads a loose file, then opens TRACK.

## What was done

Seconds from the capture's first frame; `fN` is frame N.

- 20.9 the media response (f61): the same as E16's
- LINK; the cursor down to FOLDER, passing TRACK, whose preview counts 141
  (f491)
- 48.7 FOLDER: two rows, `Loose` and `Loose root.mp3` (f569, f573)
- 50.4 `Loose/` previewed, then opened: three files (f606)
- 75.5 `Loose/01 Full v23.mp3` loaded (f1068, f1091)
- ~92 back to the root menu; TRACK opened

## What came of it

- **FOLDER on a rekordbox stick lists only what the export does not:** the
  loose folder and file, not `Contents/` (whose tracks are all exported) nor
  `PIONEER/`.
- The loose files are served as on PLAINMP3: track type 2, `0x2202`, ids that
  are clusters (`Loose` is 4), the same order, the same five-item track info
  and `?\0D` lookup.
- TRACK, and the media response, still count the export's 141 tracks: the
  loose files are in neither.
- `PLAIN-STICKS.md` §9.
