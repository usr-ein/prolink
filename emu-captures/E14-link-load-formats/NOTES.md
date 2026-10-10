# E14-link-load-formats

- recorded: 2026-10-10T18:49:42Z
- rig: `../README.md`. A, player 2 at 169.254.173.12 (PLAINMP3 in its USB
  slot); B, player 1 at 169.254.209.145, no media. Two CDJ-2000NXS on firmware
  1.44, cdj2000-emulator `dbc4e31`, TriMixxx's pi-qemu at `3c0c2aa`
- DSP: A and B on the behavioural DSP (`--dsp-model`): each loaded track plays
  at once, until the next load replaces it; both tempo sliders centred before
  the capture's first action
- tap: `pi-qemu link capture` on the link, every frame once
- time: keep-alives A 158 at a median 2.000 s, B 166 at 2.000 s (a real NXS:
  2.003 s)
- commands: `cmd.txt`, which sources `../plain-stick/rig.sh`
- reading: `transcript.txt` (`../tools/emudump`); what these sessions show,
  together, is in `../PLAIN-STICKS.md`

B loads, one after another over LINK, every other kind of file on A's stick,
and shows INFO for each: the root's tag variants, the three `Unicode/` names,
then `Formats/`, and last what should not play (FLAC, random bytes, the
AppleDouble file).

## What was done

Seconds from the capture's first frame; `fN` is frame N.

- 46.1 `02 Full v24.mp3` (ID3v2.4, PNG cover); 61.6 `03 ID3v1 only.mp3`;
  77.1 `04 No tags.mp3`; 94.1 `SHORT.MP3`; 109.6 `Filename Artist - …`
- 127.2, 142.7, 158.2: the decomposed, the emoji and the long name of
  `Unicode/`
- 180.1 `AAC.m4a`, 195.5 `AIFF PCM.aiff`, 211.0 `CBR no Info tag.mp3`,
  227.6 `MPEG2 22k.mp3`, 243.1 `VBR V2.mp3`, 258.6 `WAV PCM.wav` (it plays to
  its end: play state 17, f38114)
- ~274 `FLAC.flac`, ~290 `Corrupt.mp3`, ~319 `._01 Full v23.mp3`: pressed,
  nothing loads
- 39 MB, mostly WAV and AIFF read whole for each load beside them

## What came of it

- Track info item 1's id is the container as pdb `0x5a` numbers it: MP3 1,
  AAC 4, FLAC 5, WAV 11, AIFF 12; its argument 0 the folder's cluster, and
  item 2's the file's directory slot, as in E12. The `?\0D` LOOKUPs name
  `578` (`Unicode/`) and `4FC` (`Formats/`) for those folders.
- Duration 0 in the track info of a file A has not read yet, the real one
  once it has (AIFF: f15730, f19098).
- `0x3100` answers each file's index in its folder (f28204, f28336, f28558,
  f29072).
- After each load B opens and reads the files beside it (f924, f15756,
  f18099, …), giving each `GET_TRACK_INFO`, `0x3100`, the VBR index and the
  waveform preview first.
- FLAC, `Corrupt.mp3` and the AppleDouble file: B reads each (as a neighbour),
  greys it, and never sends a load for it; A answered their metadata and
  track info as for any file.
- Requests A never answers: `0x2005` with a 900-byte zero waveform preview
  for each track B played (fourteen, from f4042), `0x2205` with a 401-word
  VBR index B worked out for `VBR V2.mp3` (f28741), `0x3503` for each file it
  found unplayable (f6317, f19005, f25761, f28357), `0x3001` once (f39006).
- INFO's tenth item, the bitrate: 192 for the CBR MP3s, 64 for MPEG-2, 1411
  for WAV and AIFF, 0 for VBR and AAC.
- `PLAIN-STICKS.md` §5, §7.
