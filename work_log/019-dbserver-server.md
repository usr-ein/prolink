# 019 — The dbserver server

The half nobody else has built. Every reference project in this space is a
*consumer*; the two implementations that do serve are this project's own Python
proof of concept and its Mixxx C++ port, both written from `research/04` and
from a handful of captures. This one was written from `docs/PROTOCOL.md` §5 and
then **checked against the corpus**, which moved four things.

## Layout

| File | What is in it |
|---|---|
| `serve/dbserver.rs` | the two listeners, the connection task, per-connection menu state, dispatch |
| `serve/dbserver/menu.rs` | the browse surface: root, categories, the drill grid, sorts, search, metadata, track info |
| `serve/dbserver/analysis.rs` | artwork and the five transformed analysis blobs |
| `serve/dbserver/keys.rs` | the Camelot wheel: parsing either notation, harmonic tolerances, ordering |
| `serve/dbserver/tests.rs` | 42 tests over the real `testdata/export.pdb`, plus 24 captured menu rows and 44 captured requests |

## Public API

```rust
let usb = Arc::new(Medium::from_volume(Path::new("/Volumes/DJ"), ServedSlot::USB)?);
let sd  = Arc::new(Medium::from_volume(Path::new("/Volumes/SD"),  ServedSlot::SD)?);
let server = DbServer::start(DbServerConfig::default(), [usb, sd]).await?;
server.port();        // what the port query announces
server.query_port();  // None if 12523 was taken
server.slots();
```

`DbServerConfig` carries a `BrowsableDeviceNumber` rather than a
`DeviceNumber`, because a device outside 1–4 is never offered as a browse source
and so is never asked for a dbserver connection at all (F45) — serving from one
is not a thing that can go wrong, it is a thing that never happens. Dropping the
`DbServer` aborts both listeners.

Two media are **one server**, not two: a player browsing both opens a single
connection and names the slot in every request's descriptor (F37).

## What the corpus moved

Each of these was settled by reading a real CDJ-2000NXS's *replies* out of
`S20-browse-ground-truth`, where one deck browses another and both halves of the
conversation are on the wire. Each contradicts what the reference
implementations do.

- **The DEFAULT sort of a track list is by title.** A real deck answering
  `MENU_TRACK` with sort 0 returned `Acidité, Acid Lunch, Acid Storm, ACXD, Add
  Some More` — title order, whatever the artist. The references used the
  library's own artist-then-title order. Inside a playlist or a history list
  DEFAULT still means the curated order, which is what a playlist is for.
- **Argument 9 of a track row is a position, and it is not always zero.** In a
  plain list it is the track's number within its album (`ACXD` came back as 23);
  inside a playlist or a history list it is the 1-based position in that list.
  The references left it zero except in playlists, where they used a 1-based
  index for everything.
- **A search result is not a track list.** It is matching artists, then matching
  albums, then matching tracks, with argument 0 carrying `1`, `2` and `3`
  respectively and the item type naming what the row is — `0x0007` for an
  artist, `0x0004` with the track flags for a track, *not* the sort's column
  type. That is how the deck knows to answer a click on the first row with
  `0x1102` (an ARTIST drill) and a click on the last with a load, which is
  exactly what it did. The references returned tracks only, so drilling into a
  search result asked for a drill they did not implement.
- **The BITRATE listing is descending** — 2116, 320, 256, 224, 192, 160 off a
  real deck. The references sorted ascending.

Two more, from the same source and not contradicting anything, because nobody
had implemented them:

- **HISTORY drills.** `0x1112` — depth 1 into category `0x12` — takes a history
  playlist id and returns its tracks in play order. The Python has no chain for
  `0x12` at all, so opening a history list got a `0x4003`.
- **A category lists the rows a track references, not the table.** 329 artist
  rows in that medium's `export.pdb`, **290** in the deck's ARTIST menu. Rows
  arrive through `original_artist_id`, `remixer_id` and `composer_id`, which no
  track list browses by, so listing the table puts entries in the menu that open
  onto nothing. (The C++ port already does this; the Python does not.)
- **The playlist tree lists folders before playlists**, and argument 9 of each
  row is the node's own `sort_order` — 18 and 24 for two folders, 0…5 for the
  playlists beside them, in two independent captures.

## Which findings the tricky parts come from

| Part | Finding |
|---|---|
| Result sets keyed on `(descriptor, item count)`, not the count alone | F27, F41 |
| `MENU_CLOSE` draws no reply and discards nothing | F16, F27 |
| Medium resolved per message from the descriptor's slot byte | F37 |
| Root categories listed, not derived; labels wrapped in U+FFFA/U+FFFB; flags zero | F26, F40, F43 |
| The drill grid `0x1000 \| depth << 8 \| category` | F42 |
| `ALL` only when there is more than one entry | F42 |
| KEY's extra harmonic-tolerance level | F44 |
| Search text at argument **3**, not 2 | F44 |
| The sort selects the second column; item type `(column << 8) \| 0x04` | F43 |
| Thirteen metadata items, each with the referenced row's id, artwork on the title | F32 |
| Six track-info items; item 1 the container; argument 0 of the path the file size | F31, F34, F35 |
| `GET_WAVEFORM_PREVIEW` carries the track id at argument **2** | §5.11 |
| The fifth prefix word must be non-zero | F33 |
| 64 rows per render | §5.11 / `MAX_RENDER_BATCH` |
| Camelot keys sorted by `(position, letter)` | §8 |
| Never answer an unknown request with an error | F25 |

## Never an error, and what that means concretely

`MessageKind::ERROR` is never constructed anywhere in this module. Three
undocumented requests get the reply a real player sends — `0x3e03` → `0x4b02`,
`0x3100` and `0x3d03` → a bare `SUCCESS` — and **everything else this module
does not understand is answered `SUCCESS[type, 0]`**, which is a shape real
hardware also uses and is never a refusal. That covers `0x3001`, `0x3401`,
`0x3503`, `0x3903` and `0x3b03`, the five request types in the corpus that
nobody has decoded.

The one place a real deck is richer than we are: `0x3903` draws a `0x4902`
carrying 148 undecoded bytes, and we answer `SUCCESS`. Reproducing that would
mean inventing a payload.

## Bounded state

`Session::menus` is capped at `MAX_PENDING_MENUS` (32) with oldest-first
eviction, and so is the per-descriptor fallback beside it. Both references grow
without limit: the key includes the item count, so a DJ scrolling through albums
of every size mints a new key per album and the table grows for the life of the
connection. Nothing a deck has been observed to do comes within a factor of five
of the bound.

A render that names a `(descriptor, count)` we never established falls back to
the most recent set **for that descriptor**, and only then to the most recent of
all. A client that took the count from us cannot get there; one whose idea of
the library is stale can, and a blank page reads as a menu that vanished. This
was worth doing: it took the corpus replay below from 119 blank pages to zero.

## Testing

Three layers, and they do different work.

**Captured requests.** `CAPTURED_REQUESTS` is one real message of each of the 44
types a deck was observed to send, across five sessions. Replaying them through
a session proves the property F25 is about: not one draws an error, every one
that expects a reply gets one under its own transaction id, and `MENU_CLOSE`
gets nothing.

**Captured replies.** The twelve root rows and the twelve SORT rows one
CDJ-2000NXS sent another. The root menu is compared as values (we serve eleven
of the twelve; `FOLDER` browses unanalysed files by directory, which a pdb does
not describe). **The SORT menu is compared as bytes** — all twelve rows
re-encode byte for byte against what the deck sent. A round trip between our own
encoder and our own decoder could not have established that.

**The real library.** Every menu test runs against `testdata/export.pdb`: 651
tracks, 329 artists of which 290 are referenced, 274 albums, 24 Camelot keys,
seven history playlists, a 40-track playlist. So `290`, `13`, `651` and
`1A 1B 2A …` in the assertions are real numbers off a real stick.

Plus a socket test — the server on an ephemeral port, a loopback client, the
preamble both ways, introduce, root, render, a close mid-scroll, a resumed
render — and a test that a peer talking nonsense is dropped rather than left
hanging.

### The corpus replay

Not committed, because `prolink` has no dev-dependency on `prolink-capture` and
the manifests were out of scope. Run during development from a scratch crate:
reassemble the TCP streams of every capture, stand this server on loopback with
`testdata/export.pdb` in both slots, replay every request in order, and compare
the shape of each reply sequence with what the real server sent.

```
25 sessions, 14 611 requests
  every request answered            ✓
  zero errors                       ✓
  zero blank renders                ✓ (after the per-descriptor fallback)
  reply-sequence shape matched      ✓ except 0x3903, see above
```

The 45 distinct request types in the corpus are the complete surface a
CDJ-2000NXS presents, and there is nothing in it we do not answer.

Where the *Python* server errored — `0x1101`, `0x1102`, `0x1103`,
`menu_bitrate`, `0x1202`, `0x3b03`, in `S18`, `S19`, `S21` and `S22` — this one
answers. That is the F25 improvement, visible.

Reproducing it needs `prolink-capture` as a dev-dependency of `prolink`; that is
a one-line manifest change and the replay is worth keeping.

## Where we deliberately differ from the hardware

One place, and §8 argues it: a CDJ sorts key names as text, so a library in
Camelot notation comes out `1A 1B 10A 10B 11A … 2A` and two harmonically
adjacent keys land eleven screens apart. `keys::sort_text` sorts by
`(position, letter)`. The deck renders whatever order it is handed.

Everything else aims to be indistinguishable. Where there was no evidence at all
— the *direction* of four numeric sorts — the rule is stated rather than
implied: where more is better (RATING, DJ PLAY COUNT, DATE ADDED) the largest
comes first; BPM and BITRATE ascend.

## Untested against hardware

- **The whole thing.** No CDJ has browsed this server. Everything above is
  replayed traffic and a real library, which is not the same as a deck's screen.
- **DATE ADDED.** The request type a deck sends for it has **never been
  observed** — not in either direction, in any of the 25 sessions. The category
  was offered by our own server in four of them and the deck never asked for
  anything. `MENU_TIME` (`0x1010`) is what both reference implementations answer
  and what this one answers; if it is wrong, DATE ADDED opens onto an empty list
  rather than an error. Everything else in the root menu has been watched from
  root item to request type.
- **The analysis blobs**, end to end. `testdata/` holds an `export.pdb` and no
  ANLZ files, so the transforms are exercised by `prolink-proto`'s own tests
  from byte literals and this module is only tested for the *envelope* — the
  right reply type, argument 0 echoing the request type, an absent blob when
  there is nothing to send. A stick with analysis files would close that.
- **The cue record's order field.** A real deck wrote `00 01` there for all
  three cues of the reference load, which is the entry's `cue_type` byte and the
  zero beside it read as a big-endian pair. Reproduced, not explained.
- **`0x3001` draws no reply from a real deck** — the only request other than
  `MENU_CLOSE` observed to draw nothing. We answer it with a `SUCCESS`, on the
  grounds that a stray reply is cheaper than a deck waiting on one. One
  observation is thin evidence either way.
- **Two slots at once.** Resolution per message is unit-tested with two
  libraries on one session, and `S18-two-slots` replays clean, but no deck has
  switched slots against this server.
