# E10-local-folder-load

- recorded: 2026-10-10T18:36:16Z
- rig: `../README.md`. A, player 2 at 169.254.173.12 (PLAINMP3 in its USB
  slot); B, player 1 at 169.254.209.145, no media. Two CDJ-2000NXS on firmware
  1.44, cdj2000-emulator `dbc4e31`, TriMixxx's pi-qemu at `3c0c2aa`
- DSP: A and B on the behavioural DSP (`--dsp-model`); both tempo sliders
  centred before the capture's first action
- tap: `pi-qemu link capture` on the link, every frame once
- time: keep-alives A 68 at a median 2.000 s, B 76 at 2.000 s (a real NXS:
  2.003 s)
- commands: `cmd.txt`, which sources `../plain-stick/rig.sh`
- reading: `transcript.txt` (`../tools/emudump`); what these sessions show,
  together, is in `../PLAIN-STICKS.md`

A browses its own plain stick, then loads and plays a track from it. B is on
the link and never touched: it is there to receive A's status, and to show
what a stick appearing on the link makes another player do by itself.

## What was done

Seconds from the capture's first frame; `fN` is frame N.

- 0.0 the capture starts, B alone on the link
- 14.7 A's first hello (f15); 17.4 its first keep-alive as player 2 (f31)
- 18.4-18.5 B, by itself: portmap, `MNT /C/`, the media query, the
  dbserver handshake (f45-f68)
- ~24 A's screen still the player's, `Not Loaded.`; by ~34 the USB browser,
  `[FOLDER]` its only row (the emulator presses USB after the boot). The
  notice *rekordbox Database not found!*, shown for a few seconds as the
  source opens (on B in E11), is in neither shot
- ~35 `[FOLDER]` opened: the root, eight folders then seven files; every row
  passed, a second each
- ~55 `Order/`, its six rows; ~65 `Sort/`, its 26 rows five at a time
- 91.5 `01 Full v23.mp3` loaded (f1107): it plays at once, the behavioural
  DSP's way; 105.7 PLAY pauses it (f1285)
- ~108 the player view, then INFO
- ~115 `Deep/`, down its first row: it opens down to `L06`; `L07` is listed
  greyed and does not open

## What came of it

- Nothing of A's own browsing reaches the network: no dbserver or NFS traffic
  but B's, at A's appearance.
- B mounts A's USB and asks about it unprompted (f45-f68); the media response
  (f51) names the volume label, PLAINMP3, no creation date, 0 tracks and 0
  playlists, and has `02 00` at `0xaa`-`0xab` where a rekordbox stick has
  `01 01`.
- A's status for the loaded file (f1107, f1113, f1285): source A's USB, track
  type 2, id 6 (the file's first cluster), its place 3 in a list of 7 (the
  root's files), loaded from menu `0x11` (FOLDER); no tempo (`0xffff`); the
  playing flag clear at play state 3. No beat packets, from players that make
  no sound (`PLAIN-STICKS.md` §7.8).
- On A's screen: the folders-then-files order and its collation, which files
  are listed, the depth limit, the info pane and INFO from the tags:
  `PLAIN-STICKS.md` §2.
