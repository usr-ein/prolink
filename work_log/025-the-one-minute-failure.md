# 025 — The one-minute failure, and what three captures ruled out

Open. Three rounds of hardware testing, three captures, and the cause is still
not identified. This records what has been **eliminated by measurement**, so the
next round does not re-tread it, and what the fourth capture has to contain to
discriminate.

## The symptom

Serving a rekordbox USB to a CDJ-2000NXS. A track loads and plays. About a
minute into playback the deck's UI comes apart:

- the scrolling waveform disappears — at one minute, independently of the track;
- the track title is replaced by the source name, `USB@PLAYER4`;
- every browse menu is empty, including ones already visited;
- in round three, leaving LINK and returning no longer repaired it.

The audio keeps playing and stays seekable throughout.

## What has been measured and eliminated

All of this is from `~/prolink-issue3.pcap`, taken with the current build, and
compared against the deck-to-deck captures in the corpus.

| Layer | Finding |
|---|---|
| **dbserver** | Every request answered. No desync — all streams frame with nothing left over. No `ERROR` replies. No unanswered transactions. |
| **dbserver, during the failure window** | **There is no dbserver traffic at all.** In our capture the connection closes cleanly 5 s after the load; in the real deck-to-deck `S06-load-and-play` the conversation ends at 43.6 s of a 180 s capture and the deck asks nothing more. A real deck goes quiet after a load exactly as ours does, so whatever fails at t+60 s cannot be a dbserver reply. |
| **NFS** | 633 reads, 4.8 MB, **zero** errors of any kind. Offsets strictly sequential. The deck never asks us for more than 8192 bytes, so the reply cap never bites and no read is ever answered short of its request. Procedure mix (1 `GETATTR`, 8 `LOOKUP`, 633 `READ`) and read-size distribution match `S06` closely. |
| **UDP 50002 status** | Flows in both directions at 5–7 per second for the whole capture, never interrupted. Byte-drift analysis over 1377 of our packets: **only `0xca`/`0xcb` ever change** — the low half of the packet counter. A real serving deck in `S06`, over 1032 packets, changes `0x6a` once and the same two counter bytes. Ours is not distinguishable. |
| **UDP 50001 beats** | Present throughout. |
| **UDP 50000 keep-alive** | Steady, one per two seconds, throughout. |
| **Analysis blobs** | Beat-grid and detail-waveform prefixes structurally identical to a real deck's: `[count, width, byte length, 0x96, opaque]`, with the counts and lengths internally consistent. Blob lengths are full-track (76 KB of detail waveform = 8.5 minutes at 150 entries/s), so nothing is truncated at 60 s worth of data. |
| **ANLZ parsing** | 619 `.DAT` files on the real stick, 200 sampled: **zero parse failures**. |

## Three fixes made along the way, and what they were worth

- **`0x3903` is `GET_MEDIA_INFO`.** Real, evidence-based, and a genuine protocol
  finding — it is answered with `0x4902` and a 148-byte medium description, not
  a bare `SUCCESS`. Fixed and verified on the wire in round three. **It did not
  fix the symptom.**
- **Search terms go out upper-cased.** Real, evidence-based, and confirmed fixed
  by the user.
- **Menu eviction is now LRU rather than FIFO.** A real bug — one connection
  minted 44 result sets against a bound of 32 — but **not** the cause.
- **Cue points falling back to `PCO2`.** *Over-claimed.* The reply was compared
  against a real deck's for a *different* track. Only 58 of 200 tracks on the
  stick have any cues at all, so the empty reply was most likely correct. The
  fallback is harmless and stays, but it fixed nothing that was demonstrated to
  be broken.

## What the next capture has to contain

The failure happens in a window where the only traffic is NFS reads, and those
are clean. So the discriminating comparison is **not** "what do we send that is
malformed" — nothing is. It is **"what does a real serving deck send that we do
not send at all"**, in exactly the state we are imitating.

That state is: **a deck acting purely as a source**, with its own platter
stopped, while another deck plays a track off its USB.

Record that, for **at least four minutes of continuous playback**, so the
one-minute mark is well inside the window with two minutes either side. Both
decks, no Mac serving.

Specifically worth having in the same capture:

1. **Load and play, four minutes, untouched.** The primary case.
2. **The browse menus revisited at t+90 s**, to see whether the deck re-opens
   dbserver and what it asks — we never see it re-open, and it is the one
   behaviour that would explain menus recovering on a real rig and not on ours.
3. **A playlist sorted by DJ PLAY COUNT**, on a fresh LINK entry with no track
   loaded, so it is isolated from the load. In three captures the deck has
   **never sent a play-count sort request to us** — it only ever sends `0x0`
   and `0x4` — so it is rejecting something before it asks, and the request a
   real deck sends is not in the corpus at all.

### Capturing deck-to-deck is not the same as capturing deck-to-Mac

Deck-to-deck traffic is **unicast between the two decks** and will not reach a
Mac plugged into an ordinary switch. Use a **mirror/SPAN port** or a real hub,
and **verify the tap before trusting it**: run `prolink pcap` on the first thirty
seconds and confirm it shows NFS traffic on port 2049 between the two decks'
addresses. If it shows only ports 50000 and 50001, the tap is seeing broadcast
only and the capture is worthless for this — which is exactly how two earlier
findings in the research record were contaminated.
