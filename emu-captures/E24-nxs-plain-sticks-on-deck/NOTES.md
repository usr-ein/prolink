# E24-nxs-plain-sticks-on-deck

- started: 2026-10-14T16:02:31Z (the first frame)
- link: `prolink-todos-net`, recorded with `pi-qemu link capture` (every frame once)
- description: two NXSs' plain sticks, one with no volume label, plugged in
  beside a TriMixxx deck on prolink 0.5.0: the deck lists them, walks them
  by their FOLDER menus, asks each file's tags, streams a file of one, and
  names an NXS's plays of its own files.

## Devices

| | player | address | MAC | media |
| --- | --- | --- | --- | --- |
| deck | 4 | 169.254.190.254 | 02:54:4d:ed:fe:82 | PLAINMP3 in its own USB, as in E23 |
| a | 1 | 169.254.45.24 | 02:43:44:6e:2d:18 | PLAINMP3's image in USB (201 files the deck would play), SAM1 in SD |
| b | 2 | 169.254.249.106 | 02:43:44:16:f9:6a | the same image, unlabelled, its `Many` folder deleted (51 files), in USB |

b's stick differs from a's on purpose. A run before this one, with the two
sticks the same, showed nothing wrong on the deck's screen, and the capture
showed the deck walking "b's" stick, asking its tags and opening its file
over its connection to a: prolink-cxx held one dbserver connection for every
such request, opened to whichever player it asked first. It holds one per
player now, and this session is that fix's.

## Timeline

Times are seconds from the first frame.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 1.94 | a's stick in | 3.836: the deck asks about a's USB: `PLAINMP3`, no date, no tracks, no playlists, 1070579712 bytes with 1039937536 free, read as eight-byte sizes. |
| | | 4.113-5.334: **the deck walks a's stick**, 23 `MENU_FOLDER`s with track type 2 over its connection to a, and from 5.369 asks a about each of its 201 files, one `0x2202` every 96 ms (median), to 25.22. |
| 3.99 | b's stick in | 5.336: the deck asks about b's USB: **no name**, no tracks, 1070579712 bytes with 1049804800 free. Listed all the same: occupancy is the status's. |
| | | 5.616-6.279: the deck walks b's stick, 22 `MENU_FOLDER`s over **its own connection to b**, and asks b about its 51 files, 6.329 to 11.141, and for 12 covers. |
| 34.05 | deck: its library | Its sources: its own PLAINMP3, 204 tracks in 17 folders; `1 SAM1`; `1 PLAINMP3`, 201 tracks in 14 folders; `2 USB`, **51 tracks in 13 folders**. |
| 47.08-65.47 | deck: b's stick, Folders, its root, "Title Tag v24" | The folders as b has them, titles, artists, BPM tags and covers from b. 65.504: `GET_TRACK_INFO` of b's file 0x169 asked **of b**; 65.525: NFS `LOOKUP` of `?\0D 2 8` on b, then 133 `READ`s. |
| 71.62 | deck: PLAY | The deck plays it, analysed at 128.00, and says `track P2 usb type=2 id=361`; 71.814: its History asks b what file 361 is. |
| 89.40-112.99 | a: USB, FOLDER, load its own "03 ID3v1 only.mp3" | a plays its own id 627, then goes on to id 921; 157.792: the History asks a what 921 is. |
| 179.14 | deck: History | "Title Tag v1 · Artist Tag v1 · PLAINMP3" and "04 No tags.mp3 · PLAINMP3" for a's plays of its own files, "Title Tag v24 · USB" for the deck's of b's. |
| 600 | (capture ends) | |

## What came of it

- **A player's plain stick is a source on the deck,** labelled or not: an
  unlabelled one is "USB" (Sam's 07b and 07c).
- **Each player is asked about its own stick,** on its own connection: 23
  menus and 202 tags of a, 22 menus, 52 tags, the covers, the track info and
  the lookup of b.
- **A file off a player's plain stick plays on the deck,** opened by the name
  the player's track info gives, with no path.
- **The History names a player's plain plays** by what that player says of
  them, an untagged file by its file name (07d).

## Against the hardware

No hardware session has a player browsing a plain stick on another; E11-E15
are Pioneer's firmware doing it, and the deck asks what B asks there: FOLDER
menus with track type 2, `0x2202` per file, the five-item track info, the
`?\0D` lookup. The deck's tags pass, a file at a time and paced, is ours: a
CDJ asks only for the rows its cursor rests on.
