# E07-master-leaves-link

- started: 2026-10-10T18:36:12Z (the first frame)
- link: `emu-captures-sync`, recorded with `pi-qemu link capture` (every frame once)
- description: both decks playing, b in sync, a master. a is switched off
  (`pi-qemu cdj rm`: its emulator stops; it sends nothing more). What does b
  do, alone? Then a boots again, rejoins, and loads its track.

## Decks

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

Before the capture (`../README.md`, "Before every session"): both booted
fresh, sliders centred, a's track loaded, then b's; each played as it loaded,
and a became master. When a boots again inside the capture, its slider is
centred again right after (`cmd.txt`, 56.17), before it says anything on
50002.

The Mac's load averages were 6.8 at the start and 7.5 at the end. b's
keep-alives came every 2.0001 s (median; 50 intervals) but for the 4.6 s
gap below.

## Timeline

Times are seconds from the first frame; each press lands between the step's
start and the end of its `pi-qemu` command.

| t (s) | what was done | what came of it |
| ---: | --- | --- |
| 0.000 | (capture running) | a: 0xe4 (playing, master), 126.00. b: 0xc4 (playing), 132.00. |
| 4.865-5.020 | b: SYNC | 5.026: b's status 0xd4 (playing, sync), pitch -4.55%: 126.00. Its beat packets say -5.56% for four beats (5.799-7.242), bending into a's phase (`../README.md`), then -4.55%. |
| 11.866-14.693 | a switched off (`cdj rm`) | a's last keep-alive at 11.156, its last beat at 12.004, its last status at 12.016. Then nothing from a. |
| | | b keeps playing, at 126.00, and keeps sending its status to a's address, `0x9e` = 0 (not master), until 20.787: 8.8 s after a's last packet, 9.6 s after its last keep-alive. Then b sends a no more status: it has dropped a. |
| | | **b takes master then, by itself**: its MASTER lamp lit at 20.97 (the panel poll). Nobody on the link hears it: status goes only to peers, and b has none. b's beats go on at 126.00, its tempo when a left. |
| | | 23.611, 23.911, 24.211: b broadcasts **claim-number** packets (UDP 50000, kind 0x04) for number 2, three of them, 0.3 s apart, and its next keep-alive comes at 24.511, 4.6 s after the one before: b re-claims the number it holds, once, 12 s after a's last keep-alive. |
| 37.869-56.168 | a booted again (`cdj up`) | 53.466-56.164: a's start-up on 50000, as a real NXS's: three hellos, three MAC claims, three IP claims (number 1, AUTO), one number claim (it found the link populated). |
| | | 56.165: b answers a's number claim with a **number-in-use** packet (kind 0x05) for 2, unicast, 0.4 ms after it. 56.166: a's first keep-alive, as player 1. |
| | | 56.220: b's first status to a since 20.787: 0xf4 (**playing, master**, sync), `0x9e` = 1, -4.55%, 126.00. |
| 56.173-56.479 | a's slider centred | 57.998: a's first status: no track, pitch 0x00100000. |
| | | 58.196, 58.933: each asks the other what is in its USB slot (media query 0x05, answered 0x06 at once, UDP 50002). |
| 61.865 | a: USB, browse, load its track | 81.544: a's status: loading, track 1:3:77. 82.247: 0xc4 (playing), `0x9e` = 0: a plays (by itself, as an emulated NXS does) and stays a follower out of sync, at its own 126.00. b remains master. |
| 104.9 | (capture ends) | |

## What came of it

- **When its master disappears, a deck in sync keeps playing at the master's
  last tempo,** and after about 10 s without a keep-alive drops the master
  and takes master itself.
- **Master is not announced to nobody:** with no peer, b's status, master
  flag and all, goes nowhere. Its mastership shows on the wire only when a
  peer appears (56.220). A listener that had announced itself would have
  seen it at once; this capture had none.
- **The survivor re-claims its number** (three claim-number packets) about
  12 s after its peer's last keep-alive, its own keep-alives pausing 4.6 s,
  and answers a newcomer's claim with a number-in-use packet.
- **The returning deck becomes a follower,** not master: b, the master now,
  keeps it.

## Against S28

- **No deck left the link in S28,** so how a player treats a vanished
  master, the 10 s it waits, and its taking master alone rest on the
  emulator.
- **The start-up and joining packets** are the corpus's: `S02-deck-b-joins`
  and `S2c-deck-a-joins` record a deck joining a populated link (hello,
  claims, one number claim), and F36 (the protocol notes) records the
  number-in-use answer seen here from b.
- **The re-claim:** a real CDJ re-claimed its number every few seconds
  while this library's server was on the link (`S10-serve-to-cdj`); here a
  single re-claim followed a peer's vanishing. Whether a real CDJ does that
  when its peer goes, no capture shows.
