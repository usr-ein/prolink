# The capture corpus

272 MB of Pro DJ Link traffic recorded off real hardware. Every codec in this
workspace was written from these files, and `cargo test` replays them: the
specification in `docs/` is a description of what is in here, not the other way
round.

Thirty-seven sessions, one directory each:

```
S13-format-ground-truth/
├── run.pcap    the capture
├── cmd.txt     the exact tcpdump invocation that produced it
└── NOTES.md    what was being done, in what order, and what came of it
```

`hardware-ports.txt` and `hardware-ports-unplugged.txt` are `networksetup
-listallhardwareports` before and after the adapters were attached, which is how
`en9` and `en12` in the `cmd.txt` files can still be identified as the two USB
Ethernet adapters years from now.

## The rig

Two CDJ-2000NXS, each with its own USB or SD media, and a Mac between them. The
Mac bridges the two adapters (`en9`, `en12`) so the decks see one flat
link-local network, and in the `S10*`/`S17`–`S24` sessions it is also a
participant: those record *this library's* servers answering a real deck, which
is why their notes read like a changelog. There is no DJM in the rig, so the two
mixer-side discovery packets — `mixer_assign_intent` (`0x01`) and `mixer_assign`
(`0x03`) — appear nowhere in the corpus and are named as a gap by
`crates/prolink-proto/tests/corpus.rs`.

Capture the bridge **members**, not the bridge. A BSD bridge floods broadcast to
`bridge1` but forwards learned unicast directly between member ports, so a
`bridge1` tap catches the keep-alives on 50000 and misses every dbserver and NFS
packet — a capture that looks healthy and is worthless. The sessions taken with
`-i pktap,en12,en9` get both members at once; `tools/capture-deck-to-deck.sh`
does the same thing and explains it at length.

## Two formats, one name

Every file is named `run.pcap` and eleven of the thirty-seven are pcapng:
`tcpdump -i pktap,...` writes pcapng because per-packet interface metadata does
not fit classic pcap. Nothing in this workspace dispatches on the extension —
`prolink_capture::Capture::open` reads the first four bytes and picks a reader.
That mismatch is the reason the check exists, so it is left in place rather than
tidied away by renaming files.

## How the tests find it

`prolink_capture::Corpus::locate` looks at `PROLINK_CAPTURES`, and falls back to
this directory relative to the workspace root — so a plain `cargo test
--workspace` in a full checkout replays the corpus with nothing set. Set the
variable to point at a different corpus; if neither exists the corpus tests skip
and the committed fixtures in `testdata/corpus-fixtures.hex` run the same checks
over real packets extracted from these captures.

```sh
cargo test --workspace                          # replays what is here
PROLINK_CAPTURES=/other/captures cargo test -p prolink-proto -- --nocapture
```

The count assertions in those tests are floors rather than equalities, so adding
a session here cannot break them — but `--nocapture` prints per-capture and
total counts, which is how a coverage *drop* is meant to be noticed.

## What is in the packets

Link-local addresses (169.254.0.0/16), the decks' MAC addresses, and the
contents of two USB sticks of the author's own music: track and artist names,
artwork, analysis files, and — because a deck reads tracks over NFS while it
plays them — stretches of the audio itself, which is why `S11` is 78 MB. There
are no credentials in Pro DJ Link to leak; the ONC RPC calls carry the deck's
own uid/gid of 0 and nothing else.

## Sessions

| Session | Format | Size | What it records |
| --- | --- | --- | --- |
| `S01-cold-boot-a` | pcap | 14 KB | deck A cold boot, alone, no media |
| `S1b-cold-boot-b-alone` | pcap | 13 KB | deck B cold boots into an empty network |
| `S02-deck-b-joins` | pcap | 18 KB | deck A settled at D=1, deck B joins the occupied network |
| `S2c-deck-a-joins` | pcap | 8 KB | the mirror image: deck B settled, deck A joins |
| `S04-media-insert` | pcap | 1.4 MB | a stick into each deck, with both up |
| `S4b-media-insert` | pcapng | 0.6 MB | insert, eject, re-insert — on the correct tap |
| `S05-link-browse` | pcapng | 1.2 MB | deck A browses deck B's USB over LINK |
| `S06-load-and-play` | pcapng | 4.3 MB | deck A loads and plays a track from deck B's USB |
| `S10-serve-to-cdj` | pcap | 0.4 MB | the Mac serves a stick to deck B; first LINK attempt |
| `S10b-serve-to-cdj` | pcap | 0.2 MB | media queries answered |
| `S10c-serve-to-cdj` | pcap | 0.1 MB | `0x3e03` answered; can the deck drill in? |
| `S10d-serve-to-cdj` | pcap | 0.2 MB | root items matching a real player |
| `S10e-serve-to-cdj` | pcap | 0.8 MB | pagination and artwork |
| `S10f-serve-to-cdj` | pcap | 0.2 MB | filehandle prefix fix; first load attempt |
| `S10g-serve-to-cdj` | pcap | 0.2 MB | analysis data served |
| `S10h-serve-to-cdj` | pcap | 16 MB | VBR index and `0x3100` answered |
| `S10i-serve-to-cdj` | pcap | 10 MB | `GET_TRACK_INFO` grown to six items |
| `S10j-serve-to-cdj` | pcap | 2.1 MB | 13-item metadata, referenced ids, arg 10 |
| `S11-format-matrix` | pcap | 78 MB | 40 format variants across discs 0/1/2 |
| `S12-format-matrix` | pcap | 20 MB | container announced from pdb `0x5a` |
| `S13-format-ground-truth` | pcapng | 61 MB | deck B loads every format variant off deck A's USB |
| `S15a-sd-alone` | pcapng | 4.8 MB | deck A has SD only; deck B browses and loads from it |
| `S15b-sd-and-usb` | pcapng | 2.5 MB | deck A has SD *and* USB |
| `S16a-settings-over-link` | pcapng | 0.2 MB | UTILITY > LOAD SETTINGS across the link |
| `S17-serve-formats` | pcap | 13 MB | all four containers served from one pdb |
| `S18-two-slots` | pcap | 1.8 MB | two media exported as USB and SD from one dbserver |
| `S19-drilldowns` | pcap | 0.6 MB | drill-downs, with the KEY id fixed |
| `S20-browse-ground-truth` | pcapng | 3.1 MB | deck B drills every category and exercises every sort |
| `S21-drilldowns-v2` | pcap | 1.8 MB | the drill grid and ALL entries, twelve deep |
| `S22-sorting` | pcap | 11 MB | the second column per sort, eleven roots |
| `S23-search-and-keys` | pcap | 1.2 MB | search filtering, drill sort, harmonic key menu |
| `S24b-e9-control` | pcap | 2.3 MB | control: portmap on 111, as root |
| `S24c-e9-noportmap` | pcap | 0.1 MB | portmap off 111, mountd 48276, nfsd 2049, no root |
| `S25-long-play` | pcapng | 20 MB | playing past a minute, for the fields that only appear late |
| `S26-initialization-both-auto` | pcapng | 0.1 MB | both decks in AUTO from cold: which becomes which |
| `S27-sort-by-and-key` | pcapng | 3.5 MB | sort by play state, and the key-match indicator |
| `S28-master-beat-sync-taglist` | pcapng | 14 MB | master, beat sync, and the tag list |
