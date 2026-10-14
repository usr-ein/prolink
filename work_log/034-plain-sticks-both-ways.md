# 034 — Plain sticks both ways, and a medium's size

Done in this library, and checked on emulated CDJs; no real CDJ has seen it.
Sam's folder of open problems held two that are this library's: a medium's
size read and written as four bytes (his 08), and sticks with no rekordbox
library, which a CDJ shares by their folders and this library neither served
nor read (his 07). What a CDJ does with one is in
[`emu-captures/PLAIN-STICKS.md`](../emu-captures/PLAIN-STICKS.md), read off
Pioneer's firmware on emulated NXSs (E10-E17); §10.1 there is the list a
server must answer. TriMixxx's `docs/plain-usb-plan.md` §6 has the deck's side.

Each wire change is its own commit, with its `PROTOCOL.md` and
`CHANGELOG.md` lines.

## A medium's size is eight bytes (§3.3)

The media response carries total and free bytes as eight big-endian bytes at
`0xb0` and `0xb8`; we read four at `0xb4` and `0xbc`. Every medium in the
corpus is past 4 GiB, so S13 f98's 28.6 GiB stick read as 0.68 GB with more
free than total. We also wrote four, into the captured answer the builder
starts from, whose high words stayed: every medium we served was described
as that captured stick. `0x4902` carries the same sizes as two little-endian
words, the high one first; its two "unknown" words, 7 and 5, are those high
words. A served medium now says its volume's own size (`statvfs`), and an
NXS's LINK shows the deck's stick as 30.1 MB used and 1.04 GB free, `df`'s
numbers (E23).

## Serving a plain stick (§5.12)

`Medium::open` serves a stick with no `export.pdb` by its folders: described
with its label, no date, no counts and `02 00` at `0xaa`; FOLDER alone at the
root; folders then files, a CDJ's collation, file names with their
extensions; ten items of `0x2202` from the file's tags, read with `lofty`
when a player first asks and kept; five items of track info and no path;
`0x3100` the file's place in its folder; zeros for the preview and the seek
index; the file's JPEG or its folder's `cover.jpg`; and NFS opening the file
by `?\0D <folder> <entry>`, the two numbers its track info gives.

We list less than a CDJ does: only what a player can play, MP3, AAC, WAV and
AIFF, no `._*`, no empty file, no `PIONEER/` or `System Volume Information`,
no folder with nothing playable below it (§8). Sam's call, for a deck whose
own browser shows the same.

**Three requests are answered with nothing.** A player hands back the preview
(`0x2005`) and seek table (`0x2205`) it worked out for a plain file, and
names a file it cannot play (`0x3503`); a CDJ answers none, and an answer
would be read as the reply to the next request. E23 has three `0x2005`s,
unanswered, and nothing one behind after them.

**A plain file is announced as one:** track type 2 at `0x2a`, with the id
the server gives the file, so a player names it with `0x2202`.

## Reading a player's plain stick

A player's plain stick has no `export.pdb` to fetch, and none of it can be
listed over NFS. The client asks as a player does: FOLDER menus with track
type 2 to walk it, `0x2202` for a file's tags, the track info and artwork by
the file's own id, and one `LOOKUP` of the `?\0D` name to open it. The
bridge walks a stick, asks its tags a file at a time, and streams a file.

**Every player's questions went to one player.** The bridge asked for
covers, previews, and now a plain stick's walk, tags and track info over one
dbserver connection, opened to whichever player it first asked. A player asked
about another's file answers for its own file of that id. Two emulated NXSs
with the same stick hid it on the deck's screen; their capture showed the
deck walking "player 2's" stick over its connection to player 1, and never
opening one to player 2. The connections are one per player now: E24, with
the two sticks made different, has each player asked about its own. Covers
and previews had the same fault since they were bridged: a second player's
came from the first.

## CI, and the Rust floor

CI had been red since 2026-10-10: rustfmt and the pinned clippy on four
probe examples, `anlz.rs` and a doc comment. Sam asked for it green for this
release, and it is, with no behaviour change. Its MSRV job had never run the
version it named: `rust-toolchain.toml` overrode it. Run as named, 1.88
cannot build `lofty` 0.24, whatever its manifest says. Sam: "(b) I want to
use the most modern rust": the floor is 1.97, the pinned toolchain's.

## What is checked, and what is not

**Checked, with `cargo test --workspace`** (802 passed): the sizes against
the corpus (S13 f98, SAM2's `0x4902`); the walk, the tags, the menus, the
`?\0D` name and PLAINMP3's description; the three requests left unanswered;
a plain track in our status; and a loopback of the client against our own
server, browsing a plain stick, naming two files and opening one by its
numbers.

**Checked on emulated CDJs** (emu-captures E23-E25; Sam's call: the emulated
checks gate the merge):
- **E23:** an NXS browses the deck's plain stick, previews its files, loads
  three in a row and one off its own stick, each named right; its LINK shows
  the stick's real size; the deck says type 2 for its plain file.
- **E24:** two NXSs' sticks, one unlabelled, are listed, walked and named on
  the deck, each over its own connection, and a file of one streams and
  plays.
- **E25:** an NXS playing a plain file takes master; the deck in sync keeps
  its own tempo.

None of this has met a real CDJ. TriMixxx's `docs/plain-usb-plan.md` §7
keeps, for a later session on real decks, what each change must show.
