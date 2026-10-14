# E23-deck-plain-stick-on-nxs

- started: 2026-10-14T15:31:27Z (the first frame)
- link: `prolink-todos-net`, recorded with `pi-qemu link capture` (every frame once)
- description: a TriMixxx deck on prolink 0.5.0 serves a stick with no
  rekordbox library, PLAINMP3, and plays a file of it; an NXS browses it over
  LINK, previews its files, loads three in a row off it and one off its own
  stick.

## Devices

| | player | address | MAC | media | loaded |
| --- | --- | --- | --- | --- | --- |
| deck | 4 | 169.254.190.254 | 02:54:4d:ed:fe:82 | PLAINMP3 in its own USB | "01 Full v23.mp3", served as id 4 |
| a | 1 | 169.254.45.24 | 02:43:44:6e:2d:18 | PLAINMP3's image in USB, SAM1 in SD | the deck's ids 4, 5, 4; then its own 361 |
| b | 2 | 169.254.249.106 | 02:43:44:16:f9:6a | PLAINMP3's image, unlabelled, in USB | nothing |

The deck's stick is PLAINMP3's folder (`plain-stick/`) as a pi-qemu folder
stick: 211 files, of which the deck serves 203 (no `._*`, no empty file, no
FLAC, nothing in `System Volume Information`) and lists 204 itself (the FLAC
too). Its volume: 1071628288 bytes, 1041440768 free (`df -B1` on the deck).

## Timeline

Times are seconds from the first frame.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 12.30-23.14 | deck: All tracks, "Title Tag v23" loaded | 23.204: **the deck's status says track type 2, id 4,** `track P4 usb type=2 id=4`: the id its server gives the file. |
| 27.29 | deck: PLAY | 27.373: it plays and says master (first play). |
| 35.76 | a: LINK | a's screen lists `2 USB` and `4 USB`. On `4 USB` it shows **PLAINMP3, 30.1 MB used, 1.04 GB free**, the volume's own size; before 0.5.0 every medium the deck served was described as a captured 28.6 GiB stick. |
| 64.05 | a: ENTER on `4 USB` | 64.294: `MENU_ROOT` with track type 2; answered with one row, **FOLDER**. a shows "rekordbox Database not found!", as for any plain stick. |
| 67.36 | a: ENTER on FOLDER | 67.580: `MENU_FOLDER` of the root (`0xffffffff`): six folders, then six files, by file name with the extension, letters before digits (`Filename Artist - Filename Title.mp3`, `SHORT.MP3`, `01 Full v23.mp3` ...). |
| 80.84-103.41 | a: down the list | One `GET_GENERIC_METADATA` (`0x2202`, type 2) and one `GET_ARTWORK` for each row the cursor rests on, answered with the file's tags and picture: "Artist Tag v23", 01:00, 125.0 bpm, its embedded JPEG; `cover.jpg (root)` for a file whose only picture is a PNG. |
| 133.66 | a: load "01 Full v23.mp3" (id 4) | 133.887: the load's two `MENU_FOLDER`s (menu targets 3 and 4), `0x3100` answered `2` (a shows TRACK 03), `GET_TRACK_INFO` answered with five items and no path, the container's and the duration's argument 0 being 1 and 4; 134.024: **NFS `LOOKUP` of `?\0D 1 4`**, then the file read; a preview of 900 zeros, a seek index of zeros. 134.382: a plays it, `track P4 usb type=2 id=4`. Its neighbours, ids 3 and 2, are looked up the same way. |
| 156.40 | a: load "02 Full v24.mp3" (id 5) | 156.654: the same requests; 157.018: a plays id 5, TRACK 04. |
| 161.99 | | a hands back its preview of id 4, `0x2005` with 900 bytes: **nothing is answered.** |
| 169.86 | a: load id 4 again | 170.340: a plays it, TRACK 03. |
| 175.34 | | `0x2005` for id 5: nothing answered. |
| 193.32-226.96 | a: USB, FOLDER, load its own "02 Full v24.mp3" | 227.761: a plays its own id 361, named right. 228.001: the deck's History asks a about it with `0x2202` (type 2, id 0x169). |
| 232.74 | | `0x2005` for id 4: nothing answered. |
| 267.55 | | a plays on to its own next file, id 627; 267.740: the deck asks a about it. |
| 347-368 | deck: its History | The History names a's plays of the deck's files as the deck's own rows, with their covers, and a's plays of its own files by what a says of them. |
| 600 | (capture ends) | |

## What came of it

- **An NXS browses a deck's plain stick as a CDJ's:** FOLDER alone at the
  root, folders then files, file names with their extensions, each file's
  tags, BPM tag and picture in the preview, a load that plays, and the place
  in its folder as TRACK n.
- **Three loads in a row off the deck, then one off a's own stick:** each
  named and played right. The three `0x2005` hand-backs drew nothing, and
  nothing after them came one reply behind.
- **The deck playing its plain file says so:** track type 2 and the id its
  server gives the file, where 0.4 said nothing for a folder stick.
- **The deck's size is its volume's own** on a's LINK screen.

## Against the hardware

No real CDJ has served or browsed this; E10-E17 are Pioneer's firmware
serving PLAINMP3 to itself, and this is the deck in its place. Set against
E11 and E12: the same root menu and folder rows, the same five-item track
info, the same `?\0D` name, the same 900-byte preview of zeros and a seek
index of zeros. Where they differ, the deck lists less (no `._*`, no empty
file, no FLAC, no empty folder; `docs/PROTOCOL.md` §8) and gives a file with
only a PNG its folder's `cover.jpg`. The Mac was shared with other work;
the times are as captured.
