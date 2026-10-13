# E22-deck-stick-pulled-0.4

- started: 2026-10-13T13:01:58Z (the first frame)
- link: `prolink-learnings-net`, recorded with `pi-qemu link capture` (every frame once)
- description: a stick taken out of an NXS and put back, beside a TriMixxx
  deck on prolink 0.4.0 whose library lists the NXSs' media. Right after E21.

## Devices

| | player | address | MAC | media |
| --- | --- | --- | --- | --- |
| deck | 4 | 169.254.95.108 | 02:54:4d:27:ef:06 | SAM1 in its own USB |
| a | 1 | 169.254.5.189 | 02:43:44:13:05:bd | SAM1 in USB: **the stick taken out** |
| b | 2 | 169.254.230.162 | 02:43:44:92:e6:a2 | SAM1 in USB and in SD |

Before the capture, as E21 left them: a paused (Kosh, from its USB), b
playing and master (Booster, from its USB), the deck stopped (Kepler). The
deck's library was open on its list of sources: a's USB, b's SD, b's USB, its
own USB, each "141 tracks · 4 playlists".

The stick is taken out through the emulator's panel channel (`usb detach`):
it leaves a's USB port as if pulled, and `usb attach` brings it back. SD can't
be taken out of an emulated NXS that way, which is why E21's SD OPEN came to
nothing.

Keep-alives every 2.000 s from all three (median). The Mac's load average was
8.1 three minutes after the capture.

## Timeline

Times are seconds from the first frame.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | |
| 2.55 | screenshot of the deck | Its sources, as above. |
| 4.53 | a's stick out (`usb detach`) | 4.672: a's status says its USB slot `0x03` (unmounting); 4.741: `0x04`, empty. The deck sends nothing. Its log: "remote medium gone: SAM1". |
| 8.53 | screenshot of the deck | **a's medium is gone from its sources**; b's two and its own remain. |
| 24.53 | a's stick back (`usb attach`) | 25.829: a's status says its USB loaded. **26.140: the deck asks a about it, once**, 0.31 s later; 26.141: answered (SAM1, 141 tracks, 4 playlists). Its log: "remote medium found: SAM1 on player 1", then the stick's 141 tracks ingested. |
| 32.53 | screenshot of the deck | a's medium listed again. |
| 44.6 | (capture ends) | One media query from the deck in all, none to the empty slot. b asked a about the stick too (26.20), as an NXS does. |

## What came of it

- **A medium taken out of an NXS left the deck's library with nothing asked:**
  the slot's state in the NXS's status was enough. prolink 0.4.0 drops the
  slot's description when its status says it is empty, and tells the host,
  which takes the medium out of its library. On 0.3 the deck learnt of it
  from the zeroed answer to its next 5-second survey.
- **Put back, it was asked about once,** when its slot turned loaded, and
  listed again.

## Against the hardware

- **The slot states** are a real NXS's: in `S4b-media-insert` player 2's
  stick goes `0x02`, `0x03`, `0x04` and back to `0x00` (50.055-83.252). A real
  eject passes through `0x02` for 1.5 s first; the stick pulled here goes
  straight to `0x03`.
- **The one query, 0.31 s after the slot turned loaded,** is what a real deck
  does: 0.3-0.9 s in all eight deck-to-deck queries of the corpus (S10,
  S10b).
