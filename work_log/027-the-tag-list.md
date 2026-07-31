# 027 — The tag list, and what the flags word means

Done. Decoded from `~/prolink-issue6.pcap`, a hardware session with tagging,
sorting, sync and master switches in one capture, taken on the bridge so both
the Mac's own traffic and deck-to-deck unicast were recorded.

## The two requests

```
0x3002  [descriptor, track id, 1]   put this track in the tag list
0x100f  [descriptor, 0]             give me the tag list
```

`0x100f` had been falling through to the generic acknowledgement — `SUCCESS`
with zero items — which is why the menu stayed empty however many tracks were
tagged. A real deck answers it with **track rows**, which is what distinguishes
it from `MENU_KEY` (`0x1014`), the other unnamed menu kind, which a real deck
answers with key names.

**The tag list is the server's state, not the browsing deck's.** A deck asks its
source for the list rather than remembering what it tagged. So a server that
acknowledges the add without storing it is a server whose TAG LIST button does
nothing, and no amount of correctness elsewhere fixes that.

It is keyed on the descriptor's requesting-device byte: two decks browsing one
medium keep separate lists, which is what the button means on each of them. It
lives in `Shared` rather than on a `Session` because a deck tags on one
connection and opens the menu on another.

In memory only, as the user asked. A real deck writes its tag list back to the
medium; this server treats the medium as read-only, so the list is lost when the
server stops. That is documented behaviour rather than an omission.

## The flags word, settled by elimination

Four values appear across the corpus:

| flags | meaning |
|---|---|
| `0x01000000` | an ordinary track row |
| `0x01000001` | the track is in the tag list |
| `0x01000100` | the track is the one the deck has loaded |
| `0x01000101` | both |

Bit 0 was worth being careful about, because it is exactly the shape a
key-matching indicator would have and that is what was being looked for at the
time. Three things rule that out:

- it is set on **446 rows and all of them are in the tagging session**;
- within a *single* reply the same musical key appears both set and clear;
- the set of keys carrying it walks — `11A`, then `2A`, then `2A 7B`, then
  `7B`, then `5B 7B` — as tracks are tagged and untagged one at a time.

Bit 8 is named but never written. It marks the row the browsing deck has
loaded, and we do not know what that is; writing it would mark the wrong row.

## REMOVE ALL TRACKS is `0x3202`, and its twin is not

A later deck-to-deck capture settled the clear. Two request kinds appear once
each, both carrying nothing but a descriptor, both drawing `SUCCESS[type, 0]` —
identical on the wire. What separates them is the item count either side:

```
0x3002 x3          tag three tracks
0x100f -> 3
0x3402 -> SUCCESS   ... 0x100f still -> 3, so this is not the clear
0x3202 -> SUCCESS   ... 0x100f -> 0
```

So `0x3202` is REMOVE ALL TRACKS and `0x3402` is something else sent while the
menu is open. Guessing from shape alone would have been a coin toss, and
getting it backwards would have made the tag list clear itself at random.

The same capture showed `MENU_TAG_LIST` carrying a **sort** in argument 1 —
`0x0c` from a deck browsing with KEY selected — so the list is now sorted like
any other, with `DEFAULT` keeping tag order.

## What is deliberately not implemented

**Creating a playlist from the tag list.** The user's instruction was to ignore
it, and ignoring it needs no code: an unrecognised request falls through to
`SUCCESS[type, 0]`, which is never a refusal (F25).

**Untagging** is a reading rather than an observation. Argument 2 is `1` in all
nine captured requests, so `0` is taken to mean "remove" on the strength of what
a one-word flag in that position conventionally means. It is exercised by a test
and has never been seen on the wire.

## The collation is unresolved

A real deck's tag-list reply is ordered `AAC 128k`, `AAC 256k`, `Am I Feelin`,
`antidepressant o44`, `Anti Gravity Racing`, `Approximation`. That is
alphabetical **only if spaces are ignored** — any space-respecting comparison
puts `Anti Gravity` before `antidepressant`. Rather than guess at a collation
from six rows, this serves the list in tag order, which is what a tag list is.
If a deck turns out to care, the evidence to settle it is one capture of a
longer tag list.
