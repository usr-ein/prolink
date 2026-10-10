# E15-link-tag-list

- recorded: 2026-10-10T18:55:59Z
- rig: `../README.md`. A, player 2 at 169.254.173.12 (PLAINMP3 in its USB
  slot); B, player 1 at 169.254.209.145, no media. Two CDJ-2000NXS on firmware
  1.44, cdj2000-emulator `dbc4e31`, TriMixxx's pi-qemu at `3c0c2aa`
- DSP: A and B on the behavioural DSP (`--dsp-model`); both tempo sliders
  centred before the capture's first action
- tap: `pi-qemu link capture` on the link, every frame once
- time: keep-alives A 47 at a median 2.000 s, B 56 at 2.000 s (a real NXS:
  2.003 s)
- commands: `cmd.txt`, which sources `../plain-stick/rig.sh`
- reading: `transcript.txt` (`../tools/emudump`); what these sessions show,
  together, is in `../PLAIN-STICKS.md`

B tags four files of A's stick over LINK, from three folders (TAG TRACK on the
row under the cursor), opens its TAG LIST, loads a track from it, untags it,
and opens the list again.

## What was done

Seconds from the capture's first frame; `fN` is frame N.

- 51.1 TAG TRACK on `01 Full v23.mp3` (f713); 56.8 `04 No tags.mp3`;
  70.0 `Order/Track 2.mp3`; 79.9 the long name in `Unicode/`
- 84.2 TAG LIST: four rows (f1664)
- 91.3 the fourth row loaded from the tag list (f1814-f1832)
- 98.7 TAG TRACK again, from the player view: the loaded track untagged
  (f3541)
- 104.1 TAG LIST: three rows (f3684)

## What came of it

- `TAG_LIST_ADD 0x3002 [desc M8, id, 1]` tags, the same with 0 untags; A
  answers `SUCCESS [0x3002, 0]` to both. The tag list of a linked stick is
  kept by the player that holds it.
- `MENU_TAG_LIST 0x100f [desc M6, 0]` counts it; its rows are track rows
  `0x0704` with the title (tag, or file name) and the artist, flags
  `0x02000001`, their 1-based place in `pos` (f1670). `04 No tags.mp3`,
  which has no artist, carried the previous row's id in argument 0.
- A load from the tag list locates the track with `MENU_TAG_LIST` and M3,
  where a load from a folder uses `MENU_FOLDER`; the rest is E12's.
- `PLAIN-STICKS.md` §8.
