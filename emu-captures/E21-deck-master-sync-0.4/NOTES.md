# E21-deck-master-sync-0.4

- started: 2026-10-13T12:53:17Z (the first frame)
- link: `prolink-learnings-net`, recorded with `pi-qemu link capture` (every frame once)
- description: tempo master and sync between a TriMixxx deck on prolink 0.4.0
  and two NXSs: the first play, MASTER both ways, and the deck stopping as
  master in E06's five cases, once with both NXSs playing.

## Devices

| | player | address | MAC | media | track |
| --- | --- | --- | --- | --- | --- |
| deck | 4 | 169.254.95.108 | 02:54:4d:27:ef:06 | SAM1 in its own USB | Inigo Kennedy, "Kepler", id 132, 128.00 BPM |
| a | 1 | 169.254.5.189 | 02:43:44:13:05:bd | SAM1 in USB | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.230.162 | 02:43:44:92:e6:a2 | SAM1 in USB and in SD | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture: E20's NXSs as it left them, sliders centred, nothing
loaded; on the deck, SAM1 in its own USB, Kepler loaded from it and cued.
Nobody master.

Keep-alives every 2.000 s from the deck (median; 171 intervals), 2.015 s from
a, 2.018 s from b. **The Mac was loaded:** its load average was 11.4 when the
capture began (15.3 over five minutes), from these three machines and others'
work. The times below are as captured. This session is for what happened,
not how fast: a deck emulated under that load says little about a real one's
timing.

## Timeline

Times are seconds from the first frame; each press lands between the step's
start and the end of its `pi-qemu` command. "Named" is the old master's
status `0x9f`; "claims" is `0x9e` = 1.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | The deck cued, not master; a and b idle. |
| 4.53 | deck: PLAY | 4.680: **the deck plays and says master in the same packet.** |
| 14.54 | a: load Kosh | 38.097: a plays (an emulated NXS plays what it loads), not master. |
| 44.53 | b: load Booster | 70.782: b plays, not master. |
| 74.53 | a: MASTER | 74.824: a's `0x26` to the deck; 74.826: the deck's `0x27`, granted, and the deck names a. 74.897: a claims; 74.902: the deck lets go. |
| 89.53 | deck: MASTER | 90.531: the deck's `0x26` to a; 90.534: a's `0x27`, granted; 90.535: a names the deck. 90.587: the deck claims; 90.599: a lets go. |
| 104.54 | deck: PLAY (stops) | Master and out of sync, with a and b both playing out of sync. 105.261: the deck stops and **names a, the lower of the two, and only a**; 105.302: a claims; 105.327: the deck lets go. b never claims. (E06 C.) |
| 119.55 | deck: PLAY | 120.432: the deck plays, not master. |
| 124.54 | deck: MASTER | 125.562: `0x26` to a; 125.565: a names the deck; 125.572: the deck claims; 125.634: a lets go. |
| 139.59 | b: SYNC | 139.975: b in sync on the deck: -3.03%, 128.00. |
| 149.56 | a: PLAY (pauses) | 149.901: a paused. |
| 154.60 | deck: SYNC | 155.532: the deck master and in sync. |
| 164.55 | deck: PLAY (stops) | Master in sync, b playing in sync. 164.937: the deck stops and names b; 164.997: b claims; 165.002: the deck lets go. (E06 D.) |
| 179.62 | deck: PLAY | 180.082: the deck plays, in sync on b. |
| 184.54 | deck: MASTER | 185.418: `0x26` to b; 185.427: b names the deck; 185.432: the deck claims; 185.489: b lets go. |
| 199.55 | b: PLAY (pauses) | 200.955: b paused. |
| 209.56 | deck: PLAY (stops) | Master in sync, nobody playing. 210.282: the deck stops and **keeps master**, naming nobody. |
| 219.54 | a: PLAY | 219.891: a plays, out of sync. The deck, stopped and in sync, **keeps master**. (E06 B.) |
| 234.54 | deck: SYNC off | 235.003: the deck out of sync. 235.191: it names a; 235.206: a claims; 235.259: the deck lets go. (E06 E.) b, paused in sync, follows a: 126.00. |
| 249.61 | deck: PLAY | 250.260: the deck plays, out of sync. |
| 254.54 | deck: MASTER | 254.877: `0x26` to a; 254.880: a names the deck; 254.881: the deck claims; 254.950: a lets go. |
| 269.55 | a: PLAY (pauses) | 269.861: a paused. |
| 274.56 | deck: PLAY (stops) | Master out of sync, nobody playing. 275.128: the deck stops and keeps master. |
| 284.54 | b: PLAY | 286.651: b plays, in sync. 287.366: the deck names b; 287.389: b claims; 287.429: the deck lets go. (E06 A.) |
| 299.54 | b: SD OPEN | Nothing on the wire; b's SD stayed in. E22 takes a stick out. |
| 344.7 | (capture ends) | No media query: every slot had been asked about and answered in E20's session, and none turned loaded since. |

## What came of it

- **The first play took master at once,** where 0.3 waited 6.0 s (E19).
- **Every hand-over ran in the NXS's order:** `0x26`, `0x27`, the holder
  naming its successor, the successor claiming, the holder letting go. That
  held both ways and with either NXS. Exactly one device said master at the
  end of each.
- **A stopped master named one successor, the lowest-numbered player
  playing,** and nobody else while it was taken (105.261: a, with b playing
  too).
- **E06's five cases came out as an NXS's do:** it handed over (A, C, D),
  kept master (B, and twice with nobody playing), and handed over the moment
  its own sync went off (E).

## Against the hardware

- **The first play** is S28's (22.058) and E01's (23.269).
- **A stopped master handing over to the deck playing** is S28's (193.655,
  in sync); out of sync, only the emulator shows it (E01, E05).
- **E06's cases** rest on the emulator alone (`../README.md`, "Seen only
  here"), and this deck now follows them.
- **Timing:** S28's hand-overs completed in 67-133 ms. Here the five took
  68-78 ms from the `0x26` to the old master letting go, but under the load
  above that is no measure of a real deck.
