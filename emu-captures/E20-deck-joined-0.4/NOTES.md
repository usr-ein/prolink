# E20-deck-joined-0.4

- started: 2026-10-13T12:44:33Z (the first frame)
- link: `prolink-learnings-net`, recorded with `pi-qemu link capture` (every frame once)
- description: E18 again on prolink 0.4.0: a TriMixxx deck alone on the
  link, two NXSs boot and join it. Then the deck's cable is pulled for 20 s
  and put back, and it joins a link where an NXS holds the role.

## Devices

| | player | address | MAC | media |
| --- | --- | --- | --- | --- |
| deck | 4 | 169.254.95.108 | 02:54:4d:27:ef:06 | none |
| a | 1 | 169.254.5.189 | 02:43:44:13:05:bd | SAM1 in USB |
| b | 2 | 169.254.230.162 | 02:43:44:92:e6:a2 | SAM1 in USB and in SD |

The deck and the rig: `../README.md`, "E18-E22". Before the capture the deck
was alone on the link, with no stick, and held player 4. Both NXSs boot inside
the capture. Their sliders were centred after it, for E21.

Keep-alives every 2.000 s from the deck (median; its one long gap is the
cable), 2.013 s from a, 2.023 s from b. The Mac's load average was 6.0 when
the capture began and 11.4 two minutes after it ended.

## Timeline

Times are seconds from the first frame.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | The deck alone: a keep-alive every 2 s, number 4, `0x25` = 2. |
| 5.07 | a booted (`cdj up`) | 21.060-24.356: a's start-up: three hellos, three MAC claims, three IP claims (number 1, AUTO), three number claims, unanswered. 24.657: a's first keep-alive: number 1, `0x25` = 2. |
| | | **25.999: the deck's next keep-alive says `0x25` = 1,** and so does every one after. a never claims its number again. |
| | | 24.874: the deck asks a about its USB, which a's status showed loaded at 24.662; answered at once. It never asks about a's SD, which is empty. |
| 59.07 | b booted | 77.587: b's start-up. 79.385: b's IP claim proposes number 1; 79.386: a's `0x03`, and b proposes 2. 80.285: b's number claim for 2; 80.286: a's `0x05`. 80.288: b's first keep-alive: number 2, `0x25` = 1. |
| | | The deck asks b about its USB at 82.375 (loaded at 82.014) and its SD at 89.373 (loaded at 88.965), once each; answered at once. |
| 169.10 | the deck's cable out (`deck link unplug`) | 169.189: the deck's last packet. Its address goes; its session rebinds to another interface, off this link. |
| 189.08 | the cable back in | 198.008-200.745: the deck's start-up on the link: three hellos, three MAC claims, three IP claims (number 4, MANUAL), **one** number claim. It had heard a and b before claiming. 200.748: **a answers it with a `0x05`** naming its own number, 1. 200.755: the deck's first keep-alive: number 4, `0x25` = 1. |
| | | 200.874: the deck asks a about its USB; 201.372, b about both its slots. Once each, as a new session; answered at once. |
| 329.1 | (capture ends) | One device says `0x25` = 2, a. No number claim after b's start-up but the deck's own. 6 media queries in all. |

## What came of it

- **The deck gave up the role to the NXS**, from its first keep-alive after
  a said 2. a claimed its number once, at its start-up; in E18 it sent 96
  number claims in 177 s.
- **Back on the link, the deck joined as an NXS joins a populated link**: one
  number claim, answered by the role's holder with a `0x05` within 3 ms, then
  `0x25` = 1 from its first keep-alive. An AUTO NXS answers a MANUAL claimer
  as it answers another NXS.
- **The deck asked about each loaded slot once**, 0.21-0.41 s after it
  turned loaded, and never about an empty one: 6 queries in 329 s, where E18
  had 96 in 177 s.
- **A first run of this session found a leak:** the session torn down with
  the cable kept its player alive, so after the cable came back the deck was
  on the link twice, as player 4 and as device 7. prolink `57b2d8d` fixed it
  before this run; that capture is not kept.

## Against the hardware

- **The settling** is S13's (17.389-17.392, a deck joining a populated link:
  one number claim, the holder's `0x05`, then `0x25` = 1) and S26's (two
  decks booted together). The deck stepping aside on hearing another device's
  2 has no hardware capture: in the corpus nothing that says 2 ever stops.
  S24b and S18 predicted that the NXS stops re-claiming once it hears no
  other 2, and it does here.
- **One number claim into a populated link** is C13 (`../docs/PROTOCOL.md`
  §2.2).
- **The media queries** are S10's and S10b's, at the same moment: a real deck
  asks 0.3-0.9 s after a slot turns loaded.
