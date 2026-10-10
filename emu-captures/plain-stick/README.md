# PLAINMP3, the stick of E10-E19

A 1 GiB FAT32 stick of music files that rekordbox never saw: no `PIONEER/`,
no export. Every file is generated, so every tag on it is one we chose. Where
a CDJ could take a value from two places, the two differ: a title tag that is
not the file name, a BPM tag (125) that is not the tempo of the clicks (120).

```sh
uv run make_stick.py OUT --image    # OUT/files, OUT/plainmp3.img, OUT/MANIFEST.tsv, OUT/LISTING.txt
```

It needs macOS (`afconvert`, for the AAC, AIFF and FLAC files), LAME 3.100
(`brew install lame`), and the CDJ emulator's image builder, which it finds in
a TriMixxx checkout (`--emulator` otherwise). It takes about ten seconds.

- `MANIFEST.tsv` lists the files of the stick E10-E17 used, with their
  sha256. A remake with LAME 3.100, Pillow 11.3.0 and mutagen 1.47.0 gives the
  same bytes for every file but `Formats/AAC.m4a`, whose MP4 header holds the
  time `afconvert` wrote it.
- `LISTING.txt` is the image's directories in their order on the volume, each
  entry with its 8.3 name, size, first cluster and attributes. A CDJ uses the
  first clusters as the ids of its rows (`../PLAIN-STICKS.md` §4.2).
- `rig.sh` is the emulated rig the sessions' `cmd.txt` source.

## What is on it

| Where | What it tests |
| --- | --- |
| `01 Full v23.mp3` | ID3v2.3: title, artist, album, genre, BPM, key, comment, year, label, track, album artist, composer, remixer, rating (POPM), a JPEG cover |
| `02 Full v24.mp3` | ID3v2.4 in UTF-8, the same, with a PNG cover |
| `03 ID3v1 only.mp3` | ID3v1.1 alone, genre byte 18 |
| `04 No tags.mp3`, `SHORT.MP3` | no tags; and an 8.3 name with no long name |
| `Filename Artist - Filename Title.mp3` | no tags, a name that looks like artist and title |
| `._01 Full v23.mp3`, `.DS_Store` | what macOS leaves on a FAT stick |
| `cover.jpg`, `notes.txt` | not music, at the root |
| `System Volume Information/` | hidden and system, as Windows leaves it |
| `Empty/`, `Only Docs/` | a folder with nothing, one with only a text file |
| `Order/` | names, title tags and track numbers that sort three different ways, written to the volume in the reverse of name order, and a subfolder |
| `Sort/` | untagged files and folders whose names start with every kind of character |
| `Unicode/` | a 139-character name in five scripts, a decomposed (NFD) name, an emoji in a name and in tags |
| `Deep/L01/…/L10/` | a file at each of ten levels |
| `Many/` | 150 untagged files in one folder |
| `Formats/` | AAC, AIFF, WAV, FLAC, VBR (Xing), CBR without its Info header, MPEG-2 at 22.05 kHz, random bytes named `.mp3`, an empty `.mp3` |

The audio is a click on every beat, the first of four accented, each file at a
pitch of its own, at the tempo `make_stick.py` gives it. The image is written
by `cdj2000-emulator/tools/cdj_main/make_sd_image.py`: an MBR, one FAT32
partition at LBA 2048, 4 KiB clusters, long names for every name that is not
plain 8.3. Five touches then make it look like a stick a DJ formatted and
filled on a computer: the volume label `PLAINMP3`, in the boot sector and as a
root entry; short names without a leading dot, as Windows and macOS make them
(`._01 Full v23.mp3` has `_01FUL~1.MP3`); no cluster for the empty file;
`System Volume Information` hidden and system; and `Order/` reversed.
