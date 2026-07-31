# 028 — The key-matching indicator, and everything it is not

**Open.** A deck browsing this server does not light the green key indicator.
Browsing another CDJ, on the same stick, it does. This records what has been
eliminated by direct byte comparison, so the next attempt does not re-tread it.

## The behaviour is confirmed and the rule is known

With `Anti Gravity Racing` (9A) loaded, a deck browsing another deck lit exactly
these rows:

| Track | Key | Relation to 9A |
|---|---|---|
| Anti Gravity Racing | 9A | same |
| Am I Feelin, All Cries Are Beautiful | 10A | +1 |
| Alert, Artificial Recovery, Astray Red | 8A | −1 |
| Aloe (Sole Dosi Remix) | 9B | relative major |

Textbook Camelot: same, ±1 around the wheel, or the same number in the other
mode. That is the rule this library implements, and `CamelotKey::index` produces
the wheel index the deck expects — 44 of 44 real deck rows agree with it.

## What has been eliminated, and how

The decisive experiment was **holding the medium constant**: the same USB stick
was served first by a real CDJ and then by this server, with the same track
loaded on the same browsing deck, and both sessions captured.

| Thing | Result |
|---|---|
| **The whole track listing** | 125 rows compared field by field. **Zero differences.** |
| **Argument 11**, the key index | Identical on every row. 370 of 370 correct on the user's other stick too. |
| **The loaded-track mark**, flags bit 8 | Byte-identical once implemented — `0x01000100` on the right row, confirmed *on the wire* in a live capture, with the deck reporting that track loaded from us. Indicator stayed dark. |
| **`GET_TRACK_INFO`** | All six items, identical arguments. |
| **`GET_METADATA`** | Identical, including the `0x000f` KEY item — same name, same pdb row id. |
| **The status packet** | Identical apart from device number, counters, genuine state, and `0x46` (F57), which we are right to zero. |
| **The keep-alive** | Identical apart from MAC, address and device number. The peer count matches: both decks go 2→3 when we announce. |
| **Device settings `0x35`/`0x36`** | Never exchanged in either session. |

## Three theories, all dead

- **Argument 11 is wrong.** It is not. Verified against 44 real deck rows and
  against two different media.
- **The server marks compatible rows.** It does not. Across 2,142 rows with a
  known key and a known reference, the best-correlating bit of any argument
  reaches 93% where a permanently-zero field scores 92% — the 18-row difference
  is the loaded track's own row. The deck computes compatibility itself.
- **The deck classifies us as foreign**, via the `0x3e03` probe. It does not:
  `0x3e03` is sent deck-to-deck in four sessions of this corpus and absent from
  two others between the same two decks (F56). The note in `MessageKind` that
  claimed otherwise has been corrected.

## Two mistakes worth not repeating

**Marks are live state and belong at render time.** A menu request establishes a
result set and the deck pages through it without re-issuing the request (F27),
so a deck opens the track list, loads a track *from that list*, and keeps
scrolling. A mark applied when the set was built is a mark the deck is never
sent. This was a real bug, is fixed, and had the same effect on tagging
mid-scroll.

**A test helper that renders one batch is not a test of the listing.** With a
651-track library, `render_all` was asserting about the first three rows by
title, so a working implementation looked broken and a broken one looked fine.
It pages to exhaustion now.

## What is left

Nothing in the dbserver exchange, the status stream or the discovery stream
differs from a real CDJ in a way that has survived measurement. The remaining
class of explanation is that the indicator is gated on something about source
identity that is not carried in any field compared above — or on hardware state
this corpus does not contain.

The one experiment not yet run: **serve two media at once**, load from one and
browse the other, and see whether the indicator behaves differently. It probes
identity rather than bytes, which is the only class left standing. It is a shot
in the dark and is recorded as such.
