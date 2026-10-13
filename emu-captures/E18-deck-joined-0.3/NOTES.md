# E18-deck-joined-0.3

- started: 2026-10-10T20:34:32Z (the first frame)
- link: `prolink-learnings-net`, recorded with `pi-qemu link capture` (every frame once)
- description: a TriMixxx deck on prolink 0.3.0, alone on the link; two NXSs
  boot and join it, a and then b. E20 is the same on 0.4.0.

## Devices

| | player | address | MAC | media |
| --- | --- | --- | --- | --- |
| deck | 4 | 169.254.95.108 | 02:54:4d:27:ef:06 | none |
| a | 1 | 169.254.5.189 | 02:43:44:13:05:bd | SAM1 in USB |
| b | 2 | 169.254.230.162 | 02:43:44:92:e6:a2 | SAM1 in USB |

The deck and the rig: `../README.md`, "E18-E22". Before the capture the deck
was alone on the link, with no stick, and held player 4. Both NXSs boot inside
the capture.

The deck's keep-alives came every 2.000 s (median; 88 intervals), b's every
2.000 s; a's, 5.2 s apart, are the subject (below). The Mac's load was not
recorded.

## Timeline

Times are seconds from the first frame.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | The deck alone: a keep-alive every 2 s, number 4, byte `0x25` = 2 ("first on the network"). No status: a player sends it to peers only. |
| 3.17 | a booted (`cdj up`) | 19.223-22.521: a's start-up: three hellos, three MAC claims, three IP claims (number 1, AUTO), three number claims, unanswered. 22.822: a's first keep-alive: number 1, `0x25` = 2. Two devices now say they were first. |
| | | **a re-claims its number** from then to the end: three number claims and a keep-alive, every 4.5-5.2 s (26.721, 31.921, 36.421, ...). 96 claim packets in all. Its keep-alives come 5.2 s apart instead of 2.0 s. Nothing answers the claims. |
| | | 27.448: the deck asks a about both its slots (media query, UDP 50002): USB loaded, SD empty. It asks again every 5.40 s to the end, and is answered each time. |
| 48.34 | b booted | 64.350: b's start-up. 66.148: b's IP claim proposes number 1; 66.149: a answers it with a `0x03`, unicast, and b proposes 2 in the same millisecond. 67.048 and 67.348: b's number claims for 2; 67.348: a answers the second with a `0x05` naming its own number, 1. 67.350: b's first keep-alive: number 2, `0x25` = 1. b never re-claims. |
| | | 70.674: the deck asks b about both its slots, again every 5.40 s to the end. |
| 177.2 | (capture ends) | 96 media queries from the deck, every one answered. |

## What came of it

- **Two devices said they were first, and the one that joined re-claimed its
  number for as long as it heard the other.** The deck said `0x25` = 2 for
  the whole capture, as prolink 0.3 does once it starts into an empty network.
  a, unanswered, took the byte too, and re-claimed every 4.5-5.2 s.
- **b, answered by a, said 1 and never re-claimed.** So "only the CDJ that
  joined last re-claims" is, more exactly, "only the NXS that holds the role
  beside the deck re-claims".
- **The deck asked about every slot of every peer every 5.4 s**, answered or
  not, loaded or not: a's empty SD slot included.

## Against the hardware

- **The re-claims** are S10-S24's: in each of the twenty sessions with this
  library's ancestors on the link saying `0x25` = 2 beside a real NXS, the NXS
  re-claimed, 18-249 claims a session. In S24b and S18 the claims stop while
  the other device is silent, and start again when it comes back
  (`../docs/PROTOCOL.md` §2.5).
- **How two decks settle it:** S13 17.389-17.392 (a joiner) and S26
  5.605-5.606 (two decks booted together). The holder answers a number claim
  with a unicast `0x05` naming its own number, and the claimer says `0x25` = 1
  from then on, as b does here.
- **The media queries:** a real deck asks about a slot once, 0.3-0.9 s after
  it turns loaded, retries every 5.03 s until answered, and never asks once
  answered (S10, S10b; all eight deck-to-deck queries in the corpus).
