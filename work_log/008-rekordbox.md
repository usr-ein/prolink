# 008–010 — `prolink-rekordbox`

The files on a rekordbox medium: `export.pdb`, the ANLZ analysis files, the
`PIONEER/*SETTING*.DAT` container, and the joined `Library` the browse surface
is built from. Steps 08, 09 and 10 of the plan, written together because the
`Library` is only the pdb's foreign keys resolved and does not stand alone.

## Module layout

```
src/lib.rs        crate docs, re-exports
src/error.rs      Error / Result, with is_truncated()
src/string.rs     DeviceSqlString — the four-form variable-length string
src/pdb.rs        the page store: header, table directory, page chains, row index, stable_digest
src/pdb/row.rs    the row layouts, one type per table
src/anlz.rs       AnlzFile, FourCc, Tag — the container and the raw payloads
src/anlz/content.rs  Content — the structured decodes
src/settings.rs   SettingsFile — the 96-byte container and its four variants
src/library.rs    Library, Track, Playlist, HistoryPlaylist, Summary
tests/pdb.rs      a synthetic pdb writer + assertions against the real export
```

`Pdb<'a>` borrows the file's bytes rather than owning them, because random
access is required throughout — page indices are absolute and rows point at
their own strings by relative offset — so the whole file has to be resident and
copying it would serve nothing. Rows are decoded on demand through
`Pdb::rows::<R: Row>()`; there is no cache and therefore no interior mutability.

## What the real `testdata/export.pdb` contains

675,840 bytes, 165 pages, 20 tables. Measured by this reader and cross-checked
against the Python PoC:

| Table | Rows |
|---|---|
| tracks | 651 |
| artists | 329 |
| albums | **274** |
| genres | 22 |
| labels | 50 |
| keys | 24 |
| colours | 8 |
| artwork | 576 |
| playlist tree | 1 (`test_formats`) |
| playlist entries | 40 |
| history playlists | 7 |
| history entries | 83 |
| columns | 27 |
| page type 17 | 22 (undecoded) |
| page type 18 | 17 (undecoded) |
| page type 19 (history) | 1 (undecoded) |

Containers: mp3 626, m4a 8, flac 1, wav 12, aiff 4. This is the format-matrix
medium from F34, so every value in that finding's table is exercised.

**It does contain non-ASCII strings**, which is what makes it able to catch the
O6 class of bug: 17 of 329 artists (`Разные исполнители`, `Rene Wise & Rødhåd`,
`Chlär`, `Félicie`, …), 18 track titles (`人々の繋がり`, `Obéissance`,
`Impulskörper`), 7 album names, and 40 file paths. The tests assert on the exact
strings, not on a count.

Header at the time of capture: `unknown1` = 5, `sequence` = 4119.

## Where each tricky part comes from

**O6 — the UTF-16 string.** `src/string.rs`. Little-endian from `offset + 4`,
not big-endian from `offset + 3`. Pinned by literal bytes lifted from a real
`export.pdb` in both directions (decode and encode), plus the whole-file
assertions above; a round-trip test provably cannot catch it.

**The ISRC form.** Not in the Python reference at all, and it is present here:
245 of the 651 track rows carry a slot-0 string whose `0x90` framing hides a
`0x03` magic byte and NUL-terminated ASCII. Read as UTF-16 it becomes two CJK
characters with no error. `rekordcrate` calls it "a bug/flaw in Pioneer's
implementation"; the treatment here follows theirs. This is a correctness
improvement over the Python PoC, which returns mojibake for all 245.

**F34 — row offset `0x5a` is the container.** `pdb::Container`, a newtype with
consts and a hand-written `Debug`, never `unknown6`. Tested for all five values
synthetically and as a histogram over the real 651 rows.

**F47 — the presence bitmask outranks the header's row count.** `Pdb::row_offsets`.
The real file exercises it directly: playlist-entries page 18 declares
`num_rows_small` 28 and `num_rows_large` 39 while its three group masks mark
16 + 16 + 8 = 40 live, and the fortieth is track 651. The test asserts all four
numbers, so a regression names itself.

**F13 — the volatile header window.** `pdb::stable_digest` zeroes `0x10..0x18`.
Tested by applying the exact mutation the finding records (`unknown1` 5 → 6,
`sequence` + 1) to the real file and requiring the digest to be unchanged, then
flipping a byte outside the window and requiring it to change.

**F30 — two views of an ANLZ tag.** `Tag::payload()` is byte-exact from the
declared header length, which is precisely what `prolink-proto::analysis` takes
(its `beat_grid` doc says "the entries alone, with the tag's own header already
stripped"). `Tag::header_extra()` supplies the entry-width word `waveform_detail`
also wants. `Tag::body()` is everything after the twelve-byte common header,
which is what the structured decoders parse. Getting `payload` and `body`
confused is silent on any tag whose header really is twelve bytes.

**F38 — settings.** The container is implemented here; the interpretation is
behind `settings-detail`.

## Two things this reader does that the reference implementations do not

**The far-form album row.** 273 of the 274 album rows are subtype `0x80`; one is
`0x84` with its near name offset set to **zero**. A reader following the
published schema decodes that row's own header as text and files the mojibake
under an id read from the same misaligned bytes, overwriting a real album. The
Mixxx port skips the row deliberately (losing the name but corrupting nothing);
the Python PoC also drops it, which is why it reports 273 albums where this
reader reports 274.

This reader reads a `u16` far offset at `0x16`, by exact analogy with the artist
row's documented `0x64` form, which sits in the same relationship to its near
offset. That is **one observation and an analogy, not a measurement**, and it is
labelled as such in `AlbumRow`'s doc comment. The supporting evidence: the far
offset is `0x18`, a well-formed UTF-16 string starts there, it decodes to a
131-character kaomoji, and the resulting id (166) collides with no other album —
274 rows, 274 distinct ids.

**History entries have a different field order from playlist entries.** Both are
three little-endian `u32`s; a playlist entry is `(entry_index, track_id,
playlist_id)` and a history entry is `(track_id, playlist_id, entry_index)`.
Confirmed on the real file — field 1 takes values 1–7 matching the seven history
playlists, with 13/13/16/20/8/8/5 entries. Reading one with the other's layout
puts every track in playlist 1 and looks plausible.

## On `rekordcrate` — the evaluation, confirmed

The conclusion in the assignment is right, and here is the check:

- `rekordcrate::pdb::Track` declares `tempo`, `duration`, `bitrate`, `key_id`,
  `album_id`, `analyze_path`, `comment`, `date_added`, `file_size` and
  `unknown6` (the container) **without `pub`**, so a Pro DJ Link server cannot
  read a single one of the fields it must serve. Verified by reading
  `~/.cargo/registry/.../rekordcrate-0.3.0/src/pdb/mod.rs`.
- `anlz::VBR` holds `unknown1: u32` and `unknown2: Vec<u8>`, both private. The
  VBR seek index is what gates MP3 playback (F30), so that alone rules it out
  for serving.
- It pins `binrw` 0.14 against this workspace's 0.15.
- Its `anlz::CueType` names `0` as the point value; the crate-digger Kaitai
  schema — the older and more widely exercised source — says `1` is a cue point
  and `2` a loop. No file was available to arbitrate; this crate follows the
  schema and says so in the doc comment.
- Its `WaveformColorDetailColumn` packs the `PWV5` bitfield **LSB-first**, which
  disagrees with the measurement recorded in Mixxx's `rekordboxwaveform.cpp`
  (see below).

So the split stands: own readers, `rekordcrate` behind `settings-detail` for the
one thing it does that nothing else does. Two notes on the mechanics:

1. `rekordcrate` re-exports neither `binrw` nor a from-bytes helper, so calling
   its readers needs binrw 0.14's `BinRead` in scope. `crates/prolink-rekordbox/Cargo.toml`
   therefore carries an optional `binrw-014 = { package = "binrw", version = "0.14" }`,
   compiled only with the feature. Both versions were already in the lock file.
2. `settings::detail` takes the **whole file** rather than a parsed
   `SettingsFile`, deliberately: it is a second, independent read of the same
   bytes, so a disagreement about the container shows up rather than hiding
   behind a shared parse. The feature test writes a `MYSETTING.DAT` with
   `rekordcrate`'s own `BinWrite` and reads it back with this module —
   including the CRC-16/XMODEM, which agrees. That is the closest thing to a
   captured settings file available.

## The `PWV5` bit layout

The interpretation in common circulation (`blue = w >> 2`, `green = w >> 5`,
`red = w >> 8`) does not fit real files. `mixxx/src/library/rekordbox/rekordboxwaveform.cpp`
records the measurement: that reading's implied magnitude correlates at **0.13**
with the `PWV3` monochrome waveform of the same track, while the field at bits
6–2 correlates at **0.99**. So the layout, most significant bit first, is three
bits of bass, three of treble, three of mid, five of overall height, two unused.
`ColorDetailColumn` implements that and cites it. The band *assignment* is
inference from two agreeing signals (level and roughness) over six tracks; the
bit *positions* are measured. None of `PWV4`, `PWV5`, `PSSI` or `PCO2` is on the
dbserver wire, so an error here costs a waveform's colour, not a load.

## What could not be settled

- **No ANLZ or settings file was available on this machine.** Neither reader is
  pinned against captured bytes; both are built from the crate-digger Kaitai
  schema (`mixxx/lib/rekordbox-metadata/rekordbox_anlz.ksy`), `PROTOCOL.md` §7
  and the Python reference, and their tests build synthetic fixtures. Both
  module docs say so in those words. **Nothing in this crate should be read as
  "confirmed against hardware" for those two formats.** The first `.DAT` off a
  real stick is worth a session.
- **Which nibble of a `PWV2` byte is the height.** The schema stores the bytes
  without interpreting them. `TinyColumn::height` takes the low nibble, which is
  what the prose analysis describes, and the raw byte is public. Nothing in the
  serve path depends on it — the whole 100-byte payload is appended verbatim.
- **`PQT2`, `PWV6`, `PWV7`** are recognised as identifiers and left raw. `PQT2`
  is an extended beat grid whose layout the Kaitai schema does not model either;
  the two CDJ-3000 three-band waveforms live in `.2EX` files.
- **`PSSI` phrase-kind numbering** comes from the published analysis, not from a
  file read here. The XOR unmasking *is* implemented, with the mask-detection
  rule the schema uses (an unmasked mood is 1–3, so a value above 20 means the
  bytes are masked).
- **Page types 17, 18 and 19** carry rows on the real medium and nothing names
  them. The page walk reaches them and `Pdb::row_counts` reports their row
  counts, but there is no row type; an unknown table must not look like an empty
  one.
- **The settings checksum is computed and reported, not enforced.** With no
  captured file, a rule that rejects a real `MYSETTING.DAT` because our CRC
  convention is subtly wrong would be worse than one that reports a mismatch a
  caller can ignore. `rekordcrate` says `DJMMYSETTING.DAT` computes it over all
  preceding bytes rather than over the payload; that is reported, not tested.
- **Nothing in the `0x35` request obviously selects between the four settings
  variants**, which `PROTOCOL.md` §7.3 already marks unknown and this does not
  advance.

## Test shape

76 tests: 45 unit (string, ANLZ container, ANLZ content, settings) and 31
integration over `tests/pdb.rs`.

`tests/pdb.rs` is in two halves, per CONVENTIONS §7. The second asserts against
the real export. The first is the **committed fixture floor**: a synthetic pdb
writer in the test file itself, covering the structural cases one real medium
may not contain — a deleted row, a row index spanning three groups, a malformed
row, both competing row-count fields including the `0x1fff` sentinel, both far
name-offset forms, an unknown row subtype, a buffer of zeroes. Those run whether
or not `testdata/export.pdb` is present, so a missing file cannot hide a
regression; the real-file tests print a line and return.

`cargo clippy -p prolink-rekordbox --all-targets --all-features` is silent,
`cargo fmt` is clean, and `cargo doc --no-deps` has no warnings.

## Files touched outside this crate

None. `crates/prolink-rekordbox/**` and this note only. `Cargo.lock` changed as
a side effect of building, which is unavoidable in a shared workspace.
