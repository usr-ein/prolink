# E11-link-browse

- recorded: 2026-10-10T18:39:49Z
- rig: `../README.md`. A, player 2 at 169.254.173.12 (PLAINMP3 in its USB
  slot); B, player 1 at 169.254.209.145, no media. Two CDJ-2000NXS on firmware
  1.44, cdj2000-emulator `dbc4e31`, TriMixxx's pi-qemu at `3c0c2aa`
- DSP: A and B on the behavioural DSP (`--dsp-model`); both tempo sliders
  centred before the capture's first action
- tap: `pi-qemu link capture` on the link, every frame once
- time: keep-alives A 125 at a median 2.000 s, B 133 at 2.000 s (a real NXS:
  2.003 s)
- commands: `cmd.txt`, which sources `../plain-stick/rig.sh`
- reading: `transcript.txt` (`../tools/emudump`); what these sessions show,
  together, is in `../PLAIN-STICKS.md`

B browses A's plain stick over LINK, every folder of it, resting a second on
each row of the root, `Order/`, `Sort/`, `Unicode/`, `Formats/` and the first
ten of `Many/`, so that B fetches what it shows beside the list.

## What was done

Seconds from the capture's first frame; `fN` is frame N.

- 18.4 B mounts A's USB and opens its dbserver, by itself (f45-f68)
- 21.6 LINK: `MENU_ROOT` with T2, one item, FOLDER (f115, f122)
- 38.1 `[FOLDER]` opened: `MENU_FOLDER` on `0xffffffff`, 15 rows (f370);
  48.3-62 every root row hovered (`0x2202` and `0x2003` for each file)
- 61.3 `Order/` (f1098); `M folder`; 79.2 `Sort/` (f1539); `Unicode/`;
  `Formats/`
- 136.5 `Many/`: ten rows a second apart, then 139 more, 0.4 s apart: one
  six-row `RENDER` per step (from f2828)
- `Only Docs/` and `Empty/`: their previews answer 0 rows; they do not open
- 254.1 `Deep/` opened down to `L06` (f6682); B fetches `L07`'s preview
  (f6694), but never opens it

## What came of it

- The root menu of a plain stick: FOLDER alone, id `0x11`, type `0x90`.
- Folder rows (`0x0001`) and file rows (`0x0004`, flags `0x02000000`), ids the
  volume's first clusters (checked against `../plain-stick/LISTING.txt`
  for every row of E11, E12, E14 and E15: 2 430), argument 0 the row index, the full long name
  as label1, label2 empty.
- The server's order is the one A's screen shows (`Sort/`, f1539 on).
- Hovering a file: `GET_GENERIC_METADATA 0x2202` (M2, 9 of 10 items
  rendered) and `GET_ARTWORK`; hovering a folder: `MENU_FOLDER` with M2.
- The depth limit is the browsing player's: A served `L07` (f6694).
- `PLAIN-STICKS.md` §4-§6.
