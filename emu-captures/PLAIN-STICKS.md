# Plain sticks over Pro DJ Link

What a CDJ-2000NXS does with a USB stick that rekordbox never touched, what
another NXS asks it over LINK to browse and play one, and what it is
answered. Written from the emu-captures `E10`–`E17`: Pioneer's own firmware
1.44 on two emulated NXSs (`README.md` here says how far that can be
trusted). The hardware corpus in `../captures/` has no plain stick; where it
shows the same messages, for the FOLDER category of a rekordbox stick, it
agrees.

A reference such as **E11 f374** is frame 374 of
`E11-link-browse/run.pcap`, numbered as Wireshark numbers frames and as each
session's `transcript.txt` prints them. In the rig, **A** is player 2 and
holds the stick; **B** is player 1 and reaches it over LINK. Message names
are those of `crates/prolink-proto/src/dbserver.rs`; a descriptor is written
`D/M/S/T` (requesting device, menu target, slot, track type, PROTOCOL.md
§5.4).

---

## 1. The whole exchange, in one table

What B sent A, in the order it sends it, for a stick with no `PIONEER/`:

| When | B sends | A answers | |
|---|---|---|---|
| A's stick appears | portmap `GETPORT` ×2, `MNT /C/`, media query `0x05` | ports 48276 and 2049, the root handle, a media response | §3 |
| | port query (12523), `INTRODUCE`, `0x3e03` | 1051, `SUCCESS`, `0x4b02` | §3 |
| LINK | `MENU_ROOT 0x1000`, T2 | one item: FOLDER | §4.1 |
| a folder | `MENU_FOLDER 0x2006 [desc, 0, folder, 0]` + `RENDER` | `SUCCESS [0x2006, n]`, folder rows then file rows | §4.2 |
| the cursor rests on a folder | `0x2006` with M2 | its first rows, for the pane beside the list | §4.4 |
| the cursor rests on a file | `GET_GENERIC_METADATA 0x2202` M2, `GET_ARTWORK 0x2003` | ten items from the file's tags; a JPEG or nothing | §4.4, §5, §6 |
| a load | `0x2006` M3 and M4, `0x3100`, `GET_TRACK_INFO 0x2102`, `0x2004`, `0x2104`, `0x2504`, `0x2003` | five items with the file's place on the FAT volume; zeros and empty blobs for the analysis | §7 |
| | NFS `LOOKUP "?\0D <dir> <entry>"` on the export's root, `READ`s | the file | §7.3 |
| after a load | the same for one or two files beside it in the folder | | §7.5 |
| after a track | `0x2005` (a waveform preview), `0x2205` (a VBR index), `0x3503` (unplayable) | nothing | §7.7 |
| TAG TRACK, TAG LIST | `TAG_LIST_ADD 0x3002`, `MENU_TAG_LIST 0x100f` | `SUCCESS`, the tagged files | §8 |

Every request B sends carries track type **2** (unanalysed), `MENU_ROOT`
included, but the one `0x3e03` it sends as it connects, which carries 1 for
either stick (E11 f66, E16 f64). A rekordbox stick's FOLDER category is
browsed with 2 too (S20 f3880).

---

## 2. On the CDJ that holds the stick (E10)

Nothing of a CDJ's own browsing reaches the network; this section is A's
screen, in words.

**The source.** USB opens on one category, `[FOLDER]`, and for a few
seconds a red notice over it: *rekordbox Database not found! For optimum
performance, use rekordbox. Free download: www.pioneerdj.com*. Pushing
`[FOLDER]` lists the stick's root. Over LINK, B shows the same notice over
the same single category (E11, on B's screen as LINK opens).

**What is listed.** Folders first, then files, each group in one order (below).
- Folders: every folder, the empty ones included (`Empty`, and `Only Docs`,
  which holds only a `readme.txt`). Their pane beside the list says `EMPTY`
  and they do not open. `System Volume Information`, hidden and system, is
  not listed.
- Files: every file named `.mp3`, `.m4a`, `.wav`, `.aiff` or `.flac` (the
  stick has no other audio extension), whatever is in it: random bytes named
  `Corrupt.mp3` and macOS's AppleDouble `._01 Full v23.mp3` (4096 bytes, no
  audio) are listed. `Zero.mp3`, empty, is not. Nor are `cover.jpg`,
  `notes.txt`, `readme.txt` or `.DS_Store`.
- A file row shows the **file name, extension included**
  (`01 Full v23.mp3`, not its title tag).

**The order.** Folders, then files, each sorted by a collation measured with
`Sort/` (E10, on A's screen; E11 f1539 onwards, the same order on the wire):
1. letters A–Z, case ignored: `aardvark`, `abc`, `ABD`, `Zed`
2. digits: `10 ten`, `1 one`, `2 two`. Not numeric, and a space sorts after
   a digit: the space is ordered as punctuation
3. ASCII punctuation, in ASCII order: `!bang`, `#hash`, `&amp`, `'quote`,
   `(paren`, `-dash`, `_under`, `~tilde`
4. everything outside ASCII, by code point: `Äpfel`, `Éclair`, `Øre`,
   `Ωmega`, `Ярь`, `日本`

The folders of `Sort/` came out the same way: `a folder`, `B folder`,
`1 folder`, `_ folder`, `Ä folder`. So the root reads `Filename Artist -
Filename Title.mp3`, `SHORT.MP3`, `01 Full v23.mp3` … `04 No tags.mp3`,
`._01 Full v23.mp3`: letters, then digits, then the dot. Neither the order on
the FAT volume (`Order/` is written in reverse) nor the title tags nor the
track numbers enter it.

**Depth.** The browser opens folders down to the seventh level below the
root (`Deep/L01/…/L06`). `L07`, at the eighth, is listed greyed and does not
open, so `Deep L07.mp3` and below cannot be reached. Over LINK the limit is
B's own: A answered the preview of `L07` (E11 f6694), B never opened it.

**The pane beside the list**, for the file under the cursor: artist,
duration, BPM, genre, comment and artwork, from the file's tags (§5). The BPM
there is the ID3 `TBPM` tag: `125.0 bpm` for `01 Full v23.mp3`, whose clicks
are at 120.

**A loaded track** (A loads `01 Full v23.mp3`, E10 f1107):
- `TRACK 03`: its place among the folder's seven files. INFO says
  `TRACK 003/007`, `USB@PLAYER2`, and lists title, artist, album, duration,
  BPM, genre and comment from the tags, with the embedded JPEG.
- The header shows the title tag.
- No waveform, no beat grid, no key, and the BPM display beside TEMPO stays
  empty: nothing is decoded to sound in the emulator, so nothing could be
  measured there (§11). `NEEDLE` is lit.

---

## 3. What a player does when the stick appears (E10 f45–f68)

A second after A's first keep-alive, B, untouched, does what PROTOCOL.md §6
has a player do for a rekordbox medium (F46):

```
f45  portmap GETPORT mountd 1 udp     -> 48276
f47  portmap GETPORT nfs 2 udp        -> 2049
f49  mount MNT "/C/"                  -> root fh 012538a8 012538a8 012538a8 00…
f50  media query: P1 asks about P2 usb
f51  media response
f54  port query (12523)               -> dbserver on 1051
f63  INTRODUCE                        -> SUCCESS [0, 2]
f66  0x3e03 [D1/M1/S3/T1]              -> 0x4b02 [0x3e03, 0, 2, ""]
```

The media response (E10 f51), against a rekordbox stick in the same emulator
(E16 f49) and two real ones (S4b f155, S15a f262):

| Offset | Field | PLAINMP3 | SAM1, rekordbox | |
|---|---|---|---|---|
| `0x2c` | name | `PLAINMP3`: the FAT volume label | `SAM1`: the export's name | SAM1's image has no label entry and `CDJ2000` in its boot sector; S15a's medium is named `Sam CDJ1000mk3`, longer than any FAT label |
| `0x6c` | created | empty | `2026-09-29` | |
| `0x84` | (text) | empty | `1000` | in S4b and S15a too |
| `0xa4` | tracks | 0 | 141 | |
| `0xaa` | (byte) | `02` | `01` | `01` in S4b and S15a |
| `0xab` | (byte) | `00` | `01` | `01` in S4b and S15a |
| `0xac` | playlists | 0 | 4 | |
| `0xb4`, `0xbc` | total, free | the volume's | the volume's | |

A stick of loose files beside an export (E17 f61) answers exactly as SAM1
does: the loose files count for nothing.

---

## 4. Browsing over LINK (E11)

### 4.1 The root menu

```
E11 f115  MENU_ROOT [D1/M1/S3/T2, 0, 0xffffff]  -> SUCCESS [0x1000, 1]
E11 f122  MENU_ITEM id=0x11 type=0x0090 "⟦FOLDER⟧"
```

One item, FOLDER, with the root-menu id and type it has on a rekordbox stick
(PROTOCOL.md §5.5). B asks with T2 from its first menu request on; for SAM1
it asks with T1 (E16 f114) and gets its twelve categories. So a plain stick
has no SEARCH, no TRACK list and no other category, over LINK or on A
itself: it is browsed by folder or not at all.

### 4.2 A folder

```
E11 f370  MENU_FOLDER [D1/M1/S3/T2, 0, 0xffffffff, 0]  -> SUCCESS [0x2006, 15]
E11 f374  RENDER [D1/M1/S3/T2, 0, 6, 0, 15, 0]
```

Arguments: descriptor, sort (always 0), the folder's id (`0xffffffff` for
the root), and 0. With 1 in that last argument (only ever with M3, at a load:
§7.1) the count is of the folder's files alone.

Each row is a `MENU_ITEM`:

| | arg0 | id | label1 | type | flags | artwork |
|---|---|---|---|---|---|---|
| folder | its row index | its id | its name | `0x0001` | 0 | 0 |
| file | its row index | its id | its file name, extension included | `0x0004` | `0x02000000` | its own id |

- **The ids are the FAT volume's first cluster numbers.** `01 Full v23.mp3`
  is 6 and `02 Full v24.mp3` 361: the first file is 355 clusters (1 453 552
  bytes at 4 KiB) long. `Deep` is 1156, the cluster of its directory.
  `plain-stick/LISTING.txt` gives every entry's first cluster, and every id
  of E11 is one of them. On E17's image `Loose` is 4 (E17 f573). B uses an
  id to name the file or folder in its later requests, and in its status
  once it has loaded the file (§7.8).
- **label2 is empty** and argument 0 is the row's index, not a parent's id:
  a row carries no artist or BPM column.
- A file row's flags put the track type, 2, in the top byte where a rekordbox
  track has `0x01000000`.
- Names travel as the long name stored on the volume, intact: the 139-character
  name in `Unicode/`, the decomposed `Café Crème NFD name.mp3`,
  the surrogate pair of `Emoji 🎧 name.mp3` (E11 f568). B's screen draws the
  combining marks and the emoji as `~`.

### 4.3 Paging

B asks for exactly the six rows on its screen, again each time the window
moves. Scrolling `Many/` (150 files) one row at a time is one
`RENDER [desc, offset, 6, 0, 150, 0]` per row, offset counting up from 0
(E11 f2828 onwards). A server must answer any window of any list it has
counted.

### 4.4 The row under the cursor

When the cursor rests on a row, B fills the pane beside the list:
- **a folder:** `MENU_FOLDER` with **M2** for it, and a `RENDER` of its first
  rows (E11 f417 for `Empty`, answered with 0 items; f460 for `Many`).
- **a file:** `GET_GENERIC_METADATA 0x2202 [D1/M2/S3/T2, id]`, a `RENDER` of
  9 of its 10 items, then `GET_ARTWORK 0x2003 [D1/M8/S3/T2, id]`
  (E11 f589, f598).

It is `0x2202` and not `GET_METADATA 0x2002`, which is what B sends for a
rekordbox track (E16 f1643). INFO, for a loaded track, asks for the same
`0x2202` with M1 and renders all 10 items (E14 f2114: the tenth is the bitrate).

---

## 5. Track information: what there is, and from where

`GET_GENERIC_METADATA` answers ten items, in this order (E12 f720):

| # | type | id carries | label1 carries |
|---|---|---|---|
| 1 | `0x0004` title | the file's id | the title tag, else the file name with its extension |
| 2 | `0x0007` artist | the file's id | the artist tag |
| 3 | `0x0002` album | the file's id | the album tag |
| 4 | `0x000b` duration | seconds | |
| 5 | `0x000d` tempo | BPM ×100, from the BPM tag; 0 without one | |
| 6 | `0x0023` comment | the file's id | the comment tag |
| 7 | `0x0006` genre | the file's id | the genre tag, or ID3v1's genre byte as a name |
| 8 | `0x000a` rating | `0xffffffff` | |
| 9 | `0x0013` colour | 255 | |
| 10 | `0x0010` bitrate | kbps; 0 for VBR and AAC | |

Every item's argument 0 is 0, its artwork id the file's id; only the title
item has flags (`0x02000000`). The artist and album items carry the file's own
id: a plain stick has no artist or album rows to point at, so a player is
given nothing to offer "more by this artist" from.

What the stick's variants gave (E11 f589–f2741, E14 for the bitrates):

| Field | ID3v2.3 | ID3v2.4 (UTF-8) | ID3v1.1 only | no tag | AAC (`.m4a`) | WAV, AIFF |
|---|---|---|---|---|---|---|
| title | tag | tag | tag | file name | tag | file name |
| artist, album | tags | tags | tags | empty | tags | empty |
| genre | tag | tag | `Techno`, from byte 18 | empty | (none set) | empty |
| comment | tag | tag | tag | empty | (none set) | empty |
| tempo | `TBPM` 125 → 12500 | `TBPM` 130 → 13000 | 0 | 0 | `tmpo` 150 → 15000 | 0 |
| duration | 60 | 45 | 50 | 40 | 15 | 15 |
| bitrate | 192 (E12) | 192 | 192 | 192 | 0 | 1411 |
| artwork | the APIC JPEG | **none** (a PNG) | the folder's `cover.jpg` | the folder's `cover.jpg` | the `covr` JPEG | none |

Never served, whatever the tag holds: the **key** (`TKEY` was `Am` and `8A`),
the **year**, the **label** (`TPUB`), the track number, the composer, the
remixer, the album artist, and the **rating** (`POPM` 196 still gives
`0xffffffff`). Nor any date: a plain file has no "date added".

Also seen:
- **File names are not parsed.** `Filename Artist - Filename Title.mp3`, with no
  tags, has an empty artist and that whole name as its title.
- **Non-ASCII tags come through**, in ID3v2.3 UTF-16 and v2.4 UTF-8:
  `曲名 Ünïcødé Title Tag`, `Артист Tag`, `Ελληνικά Album Tag`, `Жанр`,
  `Ç'est un commentaire`.
- **A tag stops at its first character outside the BMP.** The title
  `Emoji 🎧 title` is served as `Emoji `, and the artist `🎛 artist` as nothing
  (E11 f2365).
- **The duration is worked out when A first reads the file.** Asked for a file
  it has not parsed yet (a neighbour, §7.5), A answers duration 0, then the
  real one once it has read it (E14: AIFF 0 at f15730, 15 at f19098).
- `VBR V2.mp3` (LAME `-V 2`, a Xing header) gives 30 s and its `TBPM`;
  `CBR no Info tag.mp3` (no Xing header) 20 s; `MPEG2 22k.mp3` 20 s at 64 kbps.

---

## 6. Artwork

`ARTWORK [0x2003, x, length, blob]`, for `GET_ARTWORK` with the file's id:

| The file | x | blob |
|---|---|---|
| an embedded JPEG (`APIC`, or `covr` in an `.m4a`) | 0 | that JPEG: 11 838 bytes for `01 Full v23.mp3` (E12 f605), 3 391 for `AAC.m4a`, each its picture's own size |
| no picture, a `cover.jpg` in its folder | 0 | `cover.jpg`, 9 326 bytes (E11 f600), the AppleDouble file included |
| no picture, no image in its folder | `0x32` | empty (E11 f1154) |
| an embedded **PNG** | 0 | empty (E11 f734), though `cover.jpg` is beside it |

The stick's only folder image is the root's `cover.jpg`, so which other names
(`folder.jpg`, …) count is not shown. B asks again when it gets nothing
(E11 f1152, f1156).

---

## 7. Loading and playing over LINK (E12, E13, E14)

### 7.1 The requests of a load

B loads `01 Full v23.mp3` (id 6, the third of the root's seven files), E12:

```
f658  MENU_FOLDER [D1/M3/S3/T2, 0, 0xffffffff, 1]   -> SUCCESS [0x2006, 7]   the folder's files
f663  MENU_FOLDER [D1/M4/S3/T2, 0, 0xffffffff, 0]   -> SUCCESS [0x2006, 15]
f667  RENDER [D1/M4/…, 10, 1, 0, 15, 0]               -> the loaded row, label empty
f671  0x3100 [D1/M3/S3/T2, 6, 0, 0]                   -> SUCCESS [0x3100, 2]
f675  GET_TRACK_INFO [D1/M8/S3/T2, 6]                 -> 5 items (§7.2)
f684  GET_WAVEFORM_PREVIEW [D1/M8/…, 3, 6, 0, blob[0]] -> 900 bytes, all zero
f689  GET_CUE_POINTS [D1/M8/…, 6]                     -> [0x2104, 0, 0, blob[0], 0x24, 0, 0, 0, blob[0]]
f702  GET_TRACK_INFO again
f712  NFS LOOKUP "?\0D 2 5" on the root handle         (§7.3)
f713  GET_GENERIC_METADATA [D1/M2/…, 6]
f722  0x3100 and GET_TRACK_INFO again
f734  GET_VBR_INDEX 0x2504 [D1/M8/…, 6]               -> 1 604 bytes, all zero
f739  GET_WAVEFORM_PREVIEW again
f744  NFS GETATTR, then READs from f749
f747  GET_ARTWORK                                     -> the APIC JPEG
```

- **`0x3100` answers the file's index among its folder's files**: 2 for the
  third file, which B shows as `TRACK 003/007`. Over the Formats folder it
  counted 4, 5, 6, 7 for its fifth to eighth files (E14 f28336, f28204,
  f28558, f29072). A real NXS answers rekordbox tracks with a number too
  (S15a f1143: `0x1e`), where PROTOCOL.md §5.2 has a bare `SUCCESS`.
- **The analysis requests are all made, and all answered empty**: no waveform,
  no cue points, no VBR index, and B asks for neither a beat grid nor a
  detailed waveform, which it does ask for a rekordbox track (E16 f1699,
  f2303).
- **The status B publishes** from then on names the track as A's USB, type 2,
  id 6 (E12 f697).

### 7.2 `GET_TRACK_INFO`: five items, two of them carrying the file's place

```
E12 f682
  id=1      type=0x0004   arg0=2          the container; arg0 = the directory's first cluster
  id=60     type=0x000b   arg0=5          the duration in s; arg0 = the file's entry in that directory
  id=12500  type=0x000d   arg0=0          the BPM tag ×100
  id=6      type=0x0023   "Comment tag v23"
  id=0      type=0x0000   label1=""  arg0=0x162df0 (1 453 552, the file's size)
```

Against a rekordbox track (S06 f923 on real hardware, E16 f1636 emulated):
six items, item 1's and 2's argument 0 both 0, the path in item 5's label1,
and a sixth item, `0x002f` with id 1.

| | plain file | rekordbox track |
|---|---|---|
| item 1, id | container, as pdb `0x5a` numbers it: MP3 1, AAC 4, FLAC 5, WAV 11, AIFF 12 (E14) | the same |
| item 1, arg0 | the directory's first cluster: 2 for the root, `0x578` for `Unicode/`, `0x4fc` for `Formats/` | 0 |
| item 2, arg0 | the slot of the file's first directory entry: its first long-name entry, else its 8.3 entry | 0 |
| item 3 | the BPM tag | rekordbox's BPM |
| item 5, label1 | empty | `/Contents/…/file.mp3` |
| item 6 | absent | `0x002f`, id 1 |

### 7.3 Finding the file over NFS

B does not walk a path, as it does for a rekordbox track (S06 f974–f980:
`Contents`, `Tomcraft`, `Loneliness`, the file). It sends **one `LOOKUP` on
the export's root handle**, and the name it looks up is not a file name:

```
E12 f712   LOOKUP dir=root  name = 3f 00 44 20 32 20 35        "?" (UTF-16LE), then ASCII "D 2 5"
E14 f10367 LOOKUP dir=root  name = 3f 00 44 20 35 37 38 20 32   "?" then "D 578 2"
E14 f28547 LOOKUP dir=root  name = 3f 00 44 20 34 46 43 20 31 31 "?" then "D 4FC 11"
```

The name is `?` and a NUL, as two bytes of UTF-16LE, then in plain ASCII `D`,
the directory's first cluster in upper-case hex, and the file's directory
entry slot in hex: exactly items 1's and 2's argument 0 of `GET_TRACK_INFO`.
`D 2 15` is the entry group at slot `0x15` of the root (cluster 2), where
`Filename Artist - Filename Title.mp3`'s long-name entries begin; `D 2 21`
is `SHORT.MP3`, 8.3 only, at slot `0x21`. A answers with the file's handle and
attributes, and B reads with that.

As for every LOOKUP a player makes, B writes its own file reference into the
directory handle's last 20 bytes (F28): `0301 0000 0000 1b58 0000 0000 0203
0200 0000 0006` here, against `…0203 0100 0000 00c8` for the rekordbox track
200 of S06 f974. The track type, 2 or 1, sits in that reference.

A's handles (E10 f49, E12 f712): the root is `012538a8` three times and
zeros; a file's first word changes and the next two are the root's. They
look like addresses in A's memory and are opaque to B.

### 7.4 The reads

B reads the file itself; A serves it as any NFS file (E12 f749 onwards):
- `GETATTR`, then the last 160 bytes (where ID3v1 would be), the first 2 048
  (the ID3v2 header), 8 192 from byte 383 (inside the tag, where its picture
  begins), then the audio from byte 12 221, where the tag ends
- the audio in 2 048-byte reads at first, 8 192 later; and a jump to the middle
  of the file (f837, @732 886) within the first 40 ms
- 1.21 MB of the 1.45 MB file within about 17 s of the 60 s track, with the
  behavioural DSP (E12); with Pioneer's DSP code, which runs far slower,
  0.61 MB over the whole session (E13)

### 7.5 The neighbours

**After a load B also opens and reads the files beside it in the folder**:
the one before, and in places the one after. Loading `01 Full v23.mp3` it
read `SHORT.MP3` (E12 f946, `D 2 21`); in E13 also `Filename Artist …`,
the one before that (f2502). Over Formats, loading `AAC.m4a` it read
`AIFF PCM.aiff` and `CBR no Info tag.mp3` after it (E14 f15756, f18099).
Each neighbour gets `GET_TRACK_INFO`, `0x3100`, the VBR index, the waveform
preview, its metadata and its artwork, as for a load (E12 f928 on). A rekordbox load does the same (E16
f1972: the previous track of the list). The reads are whole files: E14's 39 MB
are mostly WAV and AIFF read again for each load beside them.

### 7.6 Formats, and what does not play (E14)

- MP3 (CBR with and without its Info header, VBR, MPEG-2 at 22.05 kHz),
  AAC, WAV and AIFF load and play.
- `FLAC.flac`, `Corrupt.mp3` and the AppleDouble `._01 Full v23.mp3` do not:
  each is greyed in B's list once B has looked at it, and pressing it loads
  nothing (no `0x3100` or `GET_TRACK_INFO` follows). B had read each first as
  a neighbour: the AppleDouble file at E14 f6278, 6 312 bytes of its 4 096.
  The NXS does not play FLAC, so this is B's own judgement of the file, not
  anything A said: A served the FLAC's metadata and track info like any
  other's (container 5, duration 0).

### 7.7 What B hands back to A

Three requests a server is sent and that A never answers:
- **`0x2005 [desc M8, 3, id, 0, blob[0], 900, blob[900]]`**, for each track
  B played, a few seconds after loading the next (fourteen in E14, the first
  at f4042; two in E15, from f3678). The blob has the size of a waveform
  preview. Here it is 900 zero bytes, because the behavioural DSP makes no
  audio: what a real NXS sends is not shown.
- **`0x2205 [desc M8, id, 1604, blob[1604]]`**, once, for `VBR V2.mp3` (E14
  f28741): 401 little-endian words, the last 1 325 352, the file's sample
  count (30.05 s at 44.1 kHz), the other 400 rising like byte offsets: 1,
  417, 1 252, … up to 167 070 of the file's 168 085 bytes. The VBR index a
  rekordbox track is served has that length and that last word, but its 400
  others are zero (E16 f1696, and every one of the 741 VBR index answers in
  `../captures/`, 25 of them all zero). So B worked out a seek table, with
  real offsets, for a file A had answered with zeros, and handed it to A.
- **`0x3503 [desc M8, id]`** for the files B found it cannot play: the
  AppleDouble file (E14 f6317), `Corrupt.mp3` (f19005, f25761) and the FLAC
  (f28357).

Once, `0x3001 [desc M8, id]` too (E14 f39006), as B left a folder; it is
among PROTOCOL.md §9's undecoded requests.

### 7.8 What B publishes while it plays

B's status playing a plain file against a rekordbox track, both on the
behavioural DSP, which makes no sound (E12 f1215, E16 f2362):

| Offset | | plain file | rekordbox track |
|---|---|---|---|
| `0x28`–`0x2b` | source: player, slot, type | `02 03 02 00` | `02 03 01 00` |
| `0x2c` | track id | 6, the cluster | 58, the pdb row |
| `0x30` | its place in the list it was loaded from | 3 | 2 |
| `0x34` | the menu it was loaded from | `0x11`, FOLDER | 4, TRACK |
| `0x46` | the list's size | 7, the folder's files | 141 |
| `0x89` | flags | `0x84`: the playing bit clear at play state 3 | `0xe4`: playing, master |
| `0x92` | tempo ×100 | `ffff`: none | 17501 |
| `0x9e` | master | 0 | 1 |
| `0xa0` | beat | `ffffffff` | 1 |

What the packets carry: the firmware held the file's BPM tag (track info
item 3, 12500, E12 f682) and published no tempo (`0x92` = `ffff`).

The rest is the emulated players' only, with no sound anywhere: no beat
packets in any session with a plain file loaded (43 from B in E16), the
playing bit clear at play state 3, and never master. In these captures beats
and master go with the playing bit. Whether a real NXS playing a plain file
measures and publishes a tempo, sends beats or takes master is not shown
(§11). With Pioneer's DSP code (E13) B's status was the same: play state 3,
flags `0x84`, no tempo.

`0x66` holds `ff ff` in single packets only, chiefly the one in which play
starts after a load, with either stick (E12 f1215, E16 f2258), and `00 00`
in all the others.

---

## 8. The tag list (E15)

The tag list of a linked stick is kept by the CDJ that holds it: B adds to
and reads A's.

```
f713   TAG_LIST_ADD [D1/M8/S3/T2, 6, 1]     -> SUCCESS [0x3002, 0]      tag 01 Full v23.mp3
f3541  TAG_LIST_ADD [D1/M8/S3/T2, 0x1c1c, 0] -> SUCCESS [0x3002, 0]      untag the long-named file
f1664  MENU_TAG_LIST [D1/M6/S3/T2, 0]       -> SUCCESS [0x100f, 4]
f1668  RENDER [D1/M6/…, 0, 4, 0, 4, 0xc]
f1670  MENU_ITEM id=6 type=0x0704 "Title Tag v23" / "Artist Tag v23" flags=0x02000001 pos=1
```

Its rows are track rows with the artist column, as a rekordbox track's
(`0x0704`): label1 the title (tag or file name), label2 the artist, `pos` the
1-based place in the list, flags `0x02000001`. Argument 0 is the row's id,
except for `04 No tags.mp3`, which has no artist: it carried the previous
row's (f1670). Loading from the tag list is a load as in §7, but for the
first request: the track is found in its list with `MENU_TAG_LIST` and M3
(f1814) where a folder's load uses `MENU_FOLDER` (f1822 on).

---

## 9. A rekordbox stick, and one with loose files (E16, E17)

**Is the emulator faithful here?** For SAM1, a copy of a rekordbox stick, the
emulated NXS answered what the real ones of `../captures/` answered:
- twelve root categories (E16 f116), the eleven B rendered in S20's order,
  with no DATE ADDED between BITRATE and TRACK
- `GET_METADATA` with the thirteen items of PROTOCOL.md §5.9, in its order
  (E16 f1650)
- six-item track info with the path (E16 f1636, as S06 f923), and the path
  walked by LOOKUP
- a VBR index of 400 zero words and the sample count (E16 f1696), a beat
  grid, a detailed waveform
- beat packets while it plays.

**FOLDER on a rekordbox stick leaves out `Contents/` and `PIONEER/`.** On
SAM1, FOLDER answered 0 items (E16 f815, f817): its pane says `EMPTY` and it
does not open. Both folders are on that image, `PIONEER/` not hidden, and
`Contents/` holds, beside its 141 tracks, 47 AppleDouble `._*.mp3` files
that the export does not list, of the kind FOLDER lists on PLAINMP3 (§2).
With `Loose/` (three MP3s) and `Loose root.mp3` beside the export, FOLDER
lists those two and nothing else (E17 f573), and they browse, load and play
as on PLAINMP3 (T2, `0x2202`, five-item track info, the `?\0D` lookup: E17).
Whether it leaves the two folders out by their names or as rekordbox's own
is not shown. TRACK still counts the export's 141 (E17 f491). In S20 the
real NXS's FOLDER held one folder, `ALPHATHETA REC`, itself empty (f3886,
f3916).

| | plain stick | rekordbox stick |
|---|---|---|
| media response | label, no date, 0 tracks, 0 playlists, `0xaa` 2 | the export's name, date and counts, `0xaa` 1 |
| root menu | FOLDER only | the export's categories, FOLDER among them |
| FOLDER | every folder and audio file | what is beside `Contents/` and `PIONEER/` |
| track type | 2 | 1 (2 in FOLDER) |
| ids | FAT clusters | pdb rows |
| metadata | `0x2202`, 10 items, the file's tags | `0x2002`, 13 items, the export's rows |
| key, label, date added, rating, colour | none | the export's |
| track info | 5 items, the file's place in arg0 of items 1 and 2, no path | 6 items, the path in item 5 |
| NFS | one LOOKUP `?\0D <dir> <entry>` | the path, a LOOKUP per component |
| VBR index, waveforms, cues, beat grid | zero or empty, beat grid and detail not asked | the analysis files' |
| artwork | the embedded JPEG or the folder's `cover.jpg` | `PIONEER/Artwork` |
| while playing | no tempo published, though the track info has the BPM tag; on the emulated players, no beats and never master | tempo, beats, master |

---

## 10. What a server would have to answer

To offer a folder-only medium to an NXS as A offers PLAINMP3, from what A was
asked and answered above:

1. **The media response.** A's for PLAINMP3 is, byte for byte, the one it
   sends for SAM1 but in these fields (E10 f51 against E16 f49):
   - `0x2c`, the name: the FAT volume label
   - `0x6c`, the date: empty (SAM1: the export's, `2026-09-29`)
   - `0x84`: empty (SAM1: the text `1000`, whose meaning is not known)
   - `0xa4` tracks and `0xac` playlists: 0
   - `0xaa`-`0xab`: `02 00` (SAM1, and every media response in
     `../captures/`: `01 01`)
   - `0xb4` and `0xbc`: the volume's total and free bytes

   prolink's `MediaResponseBuilder` starts from a rekordbox answer
   (`status_templates.rs`: the date `2025-06-24`, `1000`, `01 01`) and
   replaces only the device, slot, name, counts and sizes: as it is, it
   would announce a folder medium with rekordbox's markers.

   B offered PLAINMP3 with 0 tracks: PROTOCOL.md §3.3's "a deck told there
   are no tracks has no reason to offer the medium" does not hold for a
   folder medium.

   B chose T2 for its first menu request, `MENU_ROOT` (E11 f115; T1 for SAM1, E16
   f114), before any dbserver answer differed between the sticks:
   `INTRODUCE` and `0x3e03` were answered alike (E11 f64, f68; E16 f62,
   f66). What differed was this response, and A's own status at
   `0xdc`-`0xdd`: `00 00` in every A status of E10-E15, `02 01` in E16-E17.
   The hardware sends `02 01` from a deck with both slots empty (S4b f1) and
   `00 00` from another throughout S15a, so those two bytes may not be the
   medium's type. These captures do not isolate which field makes B ask with
   T2.
2. **`MENU_ROOT` with T2**: one item, id `0x11`, type `0x0090`,
   `⟦FOLDER⟧` (§4.1).
3. **`MENU_FOLDER`**: `0xffffffff` for the root; folder rows (`0x0001`) before
   file rows (`0x0004`, flags `0x02000000`, artwork id = the file's id), argument
   0 the row index, label1 the name; with its last argument 1, the count of
   files only. Ids unique on the medium; A's are cluster numbers. Rows sorted
   as in §2 if it is to look like a CDJ (§4.2).
4. **`RENDER`** of any window (§4.3).
5. **`GET_GENERIC_METADATA 0x2202`**: the ten items of §5.
6. **`GET_ARTWORK`**: a JPEG, or an empty blob (§6).
7. **`0x3100`**: `SUCCESS [0x3100, index of the file among its folder's files]`.
8. **`GET_TRACK_INFO`**: the five items of §7.2, the container in item 1's id,
   the file size in item 5's argument 0, and two numbers in the arguments 0 of
   items 1 and 2 that its NFS server will be handed back as
   `?\0D <hex> <hex>`.
9. **NFS**: a `LOOKUP` of that name on the export's root handle, answered with
   the file's handle; `GETATTR`; `READ`s, of the neighbours too (§7.3–7.5).
10. **The analysis requests**: `GET_WAVEFORM_PREVIEW` (900 zero bytes),
    `GET_CUE_POINTS` (empty), `GET_VBR_INDEX` (1 604 zero bytes), as A did.
11. **Requests that get no answer from A**: `0x2005`, `0x2205`, `0x3503`,
    `0x3001` (§7.7), and `MENU_CLOSE` as ever.
12. **The tag list**, if it is to work: `TAG_LIST_ADD` with 1 and 0,
    `MENU_TAG_LIST` and its `0x0704` rows (§8).

And as a consumer, browsing a CDJ's plain stick: the same requests from the
other side, with T2; the file is opened with the `?\0D` name, never by path,
since the track info has none.

---

## 11. What these captures cannot show

- **Audio.** No audio is ever produced. Whether a real NXS measures a plain
  file's BPM as it plays, publishes it, or sends beat packets for it is not
  shown; nor the waveform preview it would hand back in `0x2005`.
- **The play state at load.** With the behavioural DSP a loaded track plays
  at once, a known limit of that DSP; with Pioneer's DSP code it went to play
  state 3 four seconds after the load too (E13 f1198), with no PLAY pressed.
  A real NXS cues and waits. The requests of a load are the firmware's either
  way (E12 and E13 send the same).
- **What A writes to the stick.** The emulator discards a medium's writes, and
  does not keep them to be read.
- **Hardware.** No plain stick has been in a real NXS for these captures. The
  rekordbox side agrees with the hardware corpus (§9), which is the evidence
  that the emulator's firmware is the firmware; a real CDJ capture of
  PLAINMP3 would settle the rest.
- **Untested here:** a folder of both exported and loose files, other folder
  image names than `cover.jpg`, a stick without a volume label, an SD card,
  more than eight levels over LINK from a server that would open them, and a
  medium served by anything but an NXS.
