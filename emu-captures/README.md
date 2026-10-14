# Emu-captures

Pro DJ Link traffic between **emulated** CDJ-2000NXS, as opposed to the
hardware captures in [`../captures/`](../captures/). The players run
Pioneer's own firmware on an emulator, on a software network, so a session
can be set up exactly, repeated, and timed to the step. What they record is
the firmware's behaviour **only as far as the emulator is faithful**: every
session is set against a hardware capture where one exists, and every
difference found is named below.

They are not part of the corpus `cargo test` replays: `Corpus::locate` reads
`captures/` only, and nothing here is a fixture.

```
E03-master-handoff-playing/
├── run.pcap    the link, every frame once (classic pcap)
├── cmd.txt     every command that made it, in order, with its time
└── NOTES.md    what was done, when, what came of it, and against the hardware
```

E10-E25 also carry `transcript.txt`, `tools/emudump`'s reading of `run.pcap`
("Reading them"), and E10-E17's `cmd.txt` is the shell script that ran the
session.

## The rig

Shared by every emu-capture.

- **The players:** CDJ-2000NXS, firmware 1.44 (Pioneer's `C2KNXS.UPD`; it is
  in no repository), each running on
  [cdj2000-emulator](https://github.com/usr-ein/cdj2000-emulator) at
  `dbc4e31` (on the fork's `cdj-gui` branch): cdj2k-revival's emulator with
  geepot's NXS series and TriMixxx's changes. MAIN, the SH-4 that runs the player, the network and the panel,
  is a QEMU machine (QEMU `55347990`); the GUI board's Blackfin is the
  series' fast core. Both boards run Pioneer's code, unchanged.
- **Driven by** TriMixxx's `pi-qemu cdj` (TriMixxx `3c0c2aa`): `cdj up NAME
  --link NET --usb IMAGE`, keys with `cdj press`, the browse encoder with
  `cdj rotary`, and the TEMPO slider through the emulator's panel channel
  (`python -m tools.cdj_main.panel_control analog 2 VALUE`). Each `cmd.txt`
  lists them as they were run.
- **Real time.** An emulated NXS runs at a real one's speed by the wall
  clock. In these captures: keep-alives every 2.000 s (median; a real NXS
  2.003 s), status packets every 0.2 s with extra ones on changes, on ~64 ms
  ticks, as on real decks. The Mac (an M2 Pro, twelve cores) was shared with
  other emulated CDJs and agents: each NOTES.md gives its keep-alives as the
  check that the players kept time, and E01-E09's their load averages too.
- **The link** is `pi-qemu link`, a software switch: every frame reaches every
  member, nothing is lost, and `pi-qemu link capture` records each frame
  once. So there are no bridge copies to fold, unlike the `pktap` captures
  of `captures/`, and unicast is never missed.
- **Addresses.** Each player's MAC comes from its name (`02:43:44:..`), and
  its link-local address from the MAC, as a real NXS takes it.

## E01-E09: tempo master and beat sync

Two NXSs, on link `emu-captures-sync`, both playing analysed tracks from one
rekordbox stick, and the hardware session to hold them against:
`../captures/S28-master-beat-sync-taglist`, the only one in the corpus with
master and beat sync, and the only one whose tap saw the decks' unicast.

| | player | address | MAC | track (USB) |
| --- | --- | --- | --- | --- |
| a | 1 | 169.254.230.33 | 02:43:44:53:e6:21 | Kosh, "Benefit of the Doubt", id 77, 126.00 BPM |
| b | 2 | 169.254.171.207 | 02:43:44:80:ab:cf | PLANETARY ASSAULT SYSTEM, "Booster", id 116, 132.00 BPM |

- **The stick:** a FAT32 image of a copy of the author's own rekordbox stick
  (`SAM1`, 141 tracks), the same image in both players; their
  writes go to an overlay that is thrown away. The two tracks have constant
  beat grids, 4.8% apart, so a deck in sync on the other stays inside its
  ±10% range. MY SETTINGS on the stick were not loaded: both players ran on
  their factory settings.
- **`--dsp-model`.** The NXS's audio DSP, a TI C674x, is interpreted by the
  emulator, too slowly to play a track in real time. `pi-qemu cdj up
  --dsp-model` puts the emulator's behavioural model in its place: it runs
  none of the DSP's code and makes no sound, and keeps the track's position
  on the player's clock at the rate MAIN asks for. So the master, sync and
  hand-over logic, the status and beat packets, and every pitch value in them
  are MAIN's, the firmware's; how the playhead moves (when a beat happens,
  how a phase is caught) is the model's.

### Before every session

1. **Both players booted fresh** (`cdj rm`, then `cdj up`): nothing carries
   over from one session to the next.
2. **Each TEMPO slider centred** through the panel channel, field 3 (the
   slider's centre) = 32768 first, then field 2 (its position) = 32768. An
   emulated NXS boots with both at 0 and its status says pitch 0x00000000
   (-100%) until the slider first moves; a deck in sync on it reads 0 BPM.
   Before each capture both players' status said pitch 0x00100000 (0%).
3. **Each track loaded** with the browser: USB, six detents to TRACK, ENTER,
   down to the track, ENTER. a first, then b. An emulated NXS plays what it
   loads (below), so a, playing first, became master by itself, and every
   session but E01 starts with a master (a) at 126.00 and b at 132.00, both
   playing, b not in sync.

E01 boots and centres both players but loads nothing before the capture: it
records the loads. E07 boots a again within its capture, and centres it.

### Sessions

| Session | Size | What it records |
| --- | --- | --- |
| `E01-first-play-takes-master` | 0.6 MB | nobody master; the first deck to play takes it; master kept while paused; a master that stops beside a deck not in sync |
| `E02-sync-follows-master-tempo` | 0.7 MB | SYNC on b; the master's fader moved; b's own fader in sync and after |
| `E03-master-handoff-playing` | 0.5 MB | MASTER handed back and forth four times in sync, once out of sync |
| `E04-handoff-tempo-catch-up` | 0.7 MB | a new master holds the tempo until its fader comes to it |
| `E05-master-stops` | 0.5 MB | a master that stops hands over to the deck playing; a deck in sync starts again |
| `E06-paused-master-yields` | 0.5 MB | a paused master and a deck that starts, in all four sync combinations: it hands over unless it is in sync and the other is not |
| `E07-master-leaves-link` | 0.4 MB | the master switched off: the other takes master after ~10 s, alone; the master back as a follower |
| `E08-sync-while-paused` | 0.4 MB | SYNC on a stopped deck; the master's tempo changed while it is stopped |
| `E09-master-pressed-paused` | 0.4 MB | MASTER pressed on the master, and on a stopped deck |

### Against S28

**The same as on the real decks:**

- **The packets.** In the sync sessions, the same five kinds at the same
  lengths: keep-alives (54 B, broadcast to UDP 50000), status (284 B,
  unicast to 50002), beats (96 B, broadcast to 50001), master request 0x26
  (40 B) and response 0x27 (44 B), unicast to 50001. Nothing else on 50001 or
  50002 in S28 or here: no sync control (0x2a), nothing from a mixer. (E07
  adds a player's start-up on 50000 and the media questions on 50002, 0x05
  and 0x06, which the corpus has in `S02-deck-b-joins` and
  `S4b-media-insert`.) Byte for byte, the emulated packets carry no value a
  real deck's did not, but in fields that vary by nature: names, numbers,
  addresses, tracks, tempos, pitches, beat timings, counters. Two status
  bytes only look different. `0x75` is 1 here (but for a second in E07,
  while a rejoined player had not yet asked about the other's media), 0 in
  S28; real decks send 1 in S05 and S06, and both values as media come and
  go in `S4b-media-insert`. `0x8a` counts up from a player's boot, about
  three a second, to 0xff, on real decks too (`S26` has it from 0x08); it
  moves here because these captures begin within a minute of a boot.
- **Who becomes master:** the first deck to play, with nothing sent on 50001
  (S28 22.1; E01). A master keeps it while stopped if nobody else plays
  (S28 22.9; E01, E06).
- **A MASTER press:** 0x26 from the deck that wants master, unicast to the
  master; 0x27 back, granted (`1`) every time. Then three status packets: the
  old master names its successor in `0x9f`, the successor sets `0x9e` = 1,
  the old master clears `0x9e`. For 14-64 ms both say master; only `0x9f`
  tells which is leaving. Times from the 0x26:

  | | 0x27 | old master's `0x9f` | new `0x9e` = 1 | old `0x9e` = 0 |
  | --- | ---: | ---: | ---: | ---: |
  | S28, 5 hand-overs | 2.4-5.0 ms | 3.3-5.2 ms | 16-73 ms | 67-133 ms |
  | E03, E04, E09: 9 hand-overs | 0.7-8.1 ms | 1.0-61.6 ms | 16-128 ms | 65-152 ms |

- **A master that stops while another deck plays** hands master to it in its
  status alone: `0x9f` in the very packet that says it stopped, nothing on
  50001 (S28 193.7; E05, E01).
- **A paused master in sync hands master to a deck that starts playing in
  sync** (S28 208.3; E06 case D, like for like), again in status alone.
- **The new master keeps the tempo** that was playing; its fader does
  nothing until it comes to that tempo, then drives it (S28's own notes,
  S28 156.5-163.2; E04).
- **SYNC on:** the follower's status takes up the master's effective tempo
  in the packet that lights sync or the next, 64-65 ms later. From then on
  `bpm x pitch` is the master's to the last digit, and each of the master's
  tempo changes is matched in the follower's status within one ~64 ms tick:
  S28 2.2-68 ms later, median 6.0 ms (276 changes); emulated 0.8-66 ms,
  medians 7.9-9.2 ms (240 changes in E02, E04, E08). A follower's own fader
  does nothing (S28 notes; E02, E04).
- **Beats** come every `60 / (bpm x pitch)` s, the pitch the beat packet
  carries, to an sd of 1.3-1.9 ms on both, but for the beats in which a deck
  catches a phase (2, below). A follower in sync stays in phase with its
  master to 0.01 beat (10-s windows: sd 0.002-0.005 beat on both).
- **Timing:** keep-alives every 2.000 s (S28 2.003 s); status every 0.2 s,
  with extra packets on changes on ~64 ms ticks, so status intervals of 63
  ms to 0.21 s on both.

**Seen only here** (S28 never tried it; what these show rests on the
emulator alone):

- A master that stops hands master to the deck playing even when that deck
  is out of sync (E01), or the master itself is (E05).
- A master that is not playing hands master to a deck that plays,
  **unless it is in sync and that deck is not** (E06: the four combinations,
  one case each); and it gives master away the moment its own sync goes off
  (E06, case E).
- MASTER pressed on the master gives master to the other playing deck,
  without 0x26/0x27 (E09). MASTER pressed on a stopped deck takes master in
  the ordinary exchange, and the stopped deck is then master (E09).
- SYNC pressed on a stopped deck: it follows the master's tempo while
  stopped, and starts in tempo (E08).
- Out of sync, a deck holds the tempo it was synced to until its fader
  passes it, as a new master does (E02).
- A deck in sync whose master vanishes keeps the master's last tempo, drops
  it after about 10 s without a keep-alive, and takes master alone (E07).

**Different from S28, and what an emu-capture can't stand for:**

1. **A load plays at once.** An emulated NXS with `--dsp-model` starts a
   track the moment it has loaded it: play state 0x02 (loading) then 0x03
   (playing), with no 0x06 (cued) and no PLAY pressed. A real one cues it and
   waits for PLAY (S28 20.2-22.1). With nobody master, the deck that loads
   first becomes master by playing. These sessions work with it; any
   emulated session that loads a track starts it. And while a deck still
   plays from its load, CUE does nothing; once it has been stopped and cued,
   CUE in play takes it back to its cue point and stops it there (play state
   0x06), as on a real NXS. (A test beside these sessions, not kept: two CUE
   presses 6 s apart on a deck playing from its load changed nothing; PLAY
   to pause, then CUE, cued it (0x06) and moved it to its cue point at beat
   0; and from then on each CUE in play went to 0x06, beat 0.)
2. **Catching the phase.** A real NXS put in sync jumps into phase: S28's
   deck 1 was 0.12-0.15 beat off at 102.111, and its next beat came after a
   339 ms beat, not 414 ms, 0.1 ms after the master's; a real deck starting in
   sync is in phase from its first beat (S28 210.354). An emulated one bends
   its tempo instead, for four or five beats: its beat packets carry another
   pitch than its status, which keeps the target, and the bend reaches 10
   points either way. In E09 the follower played four beats at -14.49%,
   beyond the ±10% its slider allows. It shows wherever a deck goes into sync
   while its master plays, or starts in sync while its master plays (E03's
   bend, 0.05 points, lasted nine beats); where a deck starts in sync beside
   a paused master (E06 25.5 and 45.5) there is no phase to catch:

   | session | deck | beats (s) | beat packets | status |
   | --- | --- | --- | ---: | ---: |
   | E02 | b | 4.85-6.29 (4) | -5.45% | -4.55% |
   | E03 | b | 4.77-8.58 (9) | -4.50% | -4.55% |
   | E04 | b | 5.69-7.13 (4) | -5.45% | -4.55% |
   | E05 | a | 34.94-36.69 (5) | +8.49% | -0.00% |
   | E05 | b | 57.79-59.55 (5) | +2.90% | -4.55% |
   | E05 | a | 82.01-83.84 (5) | +4.18% | -0.00% |
   | E06 | b | 3.93-5.80 (5) | -2.72% | -4.55% |
   | E07 | b | 5.80-7.24 (4) | -5.56% | -4.55% |
   | E08 | b | 28.09-29.46 (4) | -0.18% | +0.32% |
   | E09 | b | 52.68-54.28 (4) | -14.49% | -4.55% |

   Why it bends is not known. It is not that the DSP model cannot move a
   playing track: CUE pressed in play takes it back to its cue point (1).
3. **Pitch copies 2 and 4 while stopped.** A real NXS zeroes status `0x98`
   and `0xc4` while paused (S28: 274 of 275 paused packets; S06: all 323). An
   emulated one keeps a value there: the pitch, or the pitch it had when it
   stopped (E08).
4. **The slider starts at 0/0** ("Before every session"): until it moves, an
   emulated NXS's status says pitch -100%, and a deck in sync on it reads 0
   BPM. A real one reports its slider where it stands.
5. **Hand-over timing.** In all five of S28's, the old master named its
   successor within 1 ms of its 0x27. Emulated, 7 of 9 did; the other two on
   their next status tick, 16 and 56 ms later. The emulated 0x27 came 0.7-8.1
   ms after the 0x26; the real ones 2.4-5.0 ms.
6. **The first play:** S28's deck said master one status packet (65 ms)
   after the one that said it played; the emulated one in the same packet.
   One case each.
7. **No sound, no jog, no mixer:** no on-air, no fader start, no scratch.
   The beat packets' timing is the DSP model's (2), not Pioneer's DSP.
8. **The network** is ideal: no loss, no collisions, sub-millisecond, and
   nothing on it but the two players. Status is unicast to peers, so a
   player alone on the link sends none, and when E07's master leaves, the
   other player's status goes unseen until a peer comes back.

## E10-E17: a stick without rekordbox

What a CDJ does with a USB stick of music files that rekordbox never saw, on
its own and shared over LINK. What the sessions show is written up in
[`PLAIN-STICKS.md`](PLAIN-STICKS.md); the stick is `plain-stick/`, whose
`make_stick.py` remakes it.

Two NXSs on link `emu-captures-usb-net`:

| | player | address | MAC | media |
| --- | --- | --- | --- | --- |
| A | 2 | 169.254.173.12 | 02:43:44:bc:ad:0c | the session's stick, in USB |
| B | 1 | 169.254.209.145 | 02:43:44:d2:d1:91 | none: it reaches A's stick over LINK |

- **The stick:** PLAINMP3, a 1 GiB FAT32 image of generated files with tags
  chosen for the purpose and no `PIONEER/` (`plain-stick/README.md`). E16
  puts SAM1 in its place, the rekordbox stick of E01-E09, and E17 SAM1 with
  loose MP3s beside its export. A's writes go to an overlay that is thrown
  away.
- **The DSP:** `--dsp-model` on both players, but B in E13, which runs
  Pioneer's DSP code, interpreted (~0.04x). With the model a loaded track
  plays at once, as in E01-E09 ("A load plays at once"). The dbserver and NFS
  requests of a load are the same with either (E12, E13).

### Before every session

`cmd.txt` is the script that ran it, and `plain-stick/rig.sh` the rig it
sources:

1. **Both players booted fresh:** B first, so its AUTO number is 1; then the
   capture; then A with the stick.
2. **Each TEMPO slider centred** through the emulator's panel channel, field 3
   then field 2 = 32768, as for E01-E09.
3. **The browse encoder turned one detent at a time:** several sent at once
   are not all counted.

### Sessions

| Session | Size | What it records |
| --- | --- | --- |
| `E10-local-folder-load` | 0.6 MB | A browses its own plain stick and plays from it; B only listens |
| `E11-link-browse` | 1.8 MB | B browses every folder of A's plain stick over LINK |
| `E12-link-load-play` | 2.2 MB | B loads a fully tagged MP3 from it over LINK and plays it |
| `E13-link-load-pioneer-dsp` | 2.3 MB | the same load, B on Pioneer's DSP code |
| `E14-link-load-formats` | 39 MB | B loads every other kind of file on the stick, one after another |
| `E15-link-tag-list` | 2.4 MB | B tags files of the stick, browses its tag list, loads from it |
| `E16-rekordbox-control` | 5.7 MB | the control: SAM1, a rekordbox stick, in A instead |
| `E17-mixed-stick` | 2.2 MB | SAM1 with loose MP3s beside its export |

E14 is large because a player reads whole files over NFS, and reads the WAV
and the AIFF again for each load beside them.

### Against the hardware

The corpus has no plain stick. E16 is the same rig with a rekordbox stick,
set against `../captures/S05`, `S06` and `S20`: the root categories in S20's
order, thirteen-item metadata, six-item track info with the path, the path
walked by LOOKUP, the analysis served, beat packets while playing, as the
real NXSs exchanged them. And S20's real request for FOLDER on a rekordbox
stick carries the track type, 2, that every request of E11-E15 carries but
the one `0x3e03` B sends as it connects. What
the emulator cannot show (no audio, so nothing measured from it; the play at
load; a medium's writes) is in `PLAIN-STICKS.md` §11.

## E18-E22: a TriMixxx deck between two NXSs

A player of this library's making, a TriMixxx deck, on the link with two NXSs,
before and after prolink 0.4.0. 0.4.0 changed how the deck holds the
first-on-the-network byte, how it takes and gives tempo master, and when it
asks about a peer's media ([work log 033](../work_log/033-what-the-cdj-to-cdj-captures-teach.md)).

| | player | address | MAC |
| --- | --- | --- | --- |
| the deck | 4 | 169.254.95.108 | 02:54:4d:27:ef:06 |
| a | 1 | 169.254.5.189 | 02:43:44:13:05:bd |
| b | 2 | 169.254.230.162 | 02:43:44:92:e6:a2 |

- **The deck** is TriMixxx's emulated deck (`pi-qemu deck up NAME --link
  NET`): the image a real deck runs, on an emulated Pi 4, with the Mixxx fork
  deployed on it (`pi-qemu deck deploy NAME mixxx`). That is `7fddd9f`, with
  prolink 0.3.0, for E18-E19, and `21f8b4d`, with 0.4.0, for E20-E22. It
  claims player 4 (MANUAL), and serves its own stick when one is in.
- **The NXSs** are E01-E17's, with `--dsp-model` and SAM1 in USB; b has it in
  SD too from E20 on.
- **The stick** is SAM1, E01-E09's, in all three. The deck reads an NXS's
  `export.pdb` over NFS when it first hears of the medium, to list it.
- **Before every session:** in E18 and E20 the deck is alone on the link, with
  no stick, and both NXSs boot inside the capture. E19 and E21 start where the
  session before left them, sliders centred, with Kepler (128.00) cued on the
  deck from its own copy of SAM1. Each `cmd.txt` is a timed list, as E01-E09's.

### Sessions

| Session | Size | What it records |
| --- | --- | --- |
| `E18-deck-joined-0.3` | 2.2 MB | 0.3: the deck alone, two NXSs join; one re-claims its number every 4.5-5.2 s; the deck asks about every slot every 5.4 s |
| `E19-deck-master-sync-0.3` | 3.1 MB | 0.3: the first play, MASTER both ways, the deck stopping as master beside a deck out of sync and in sync |
| `E20-deck-joined-0.4` | 4.8 MB | 0.4: E18 again, then the deck's cable pulled and put back |
| `E21-deck-master-sync-0.4` | 4.3 MB | 0.4: the first play, MASTER both ways, E06's five cases with the deck as the stopped master |
| `E22-deck-stick-pulled-0.4` | 0.9 MB | 0.4: a stick taken out of an NXS and put back |

### What they show

- **The first-on-the-network byte** (keep-alive `0x25`, `../docs/PROTOCOL.md`
  §2.5). On 0.3 the deck says 2 throughout, and the NXS that holds the role
  beside it re-claims its number for as long as it hears it (E18, E19). On
  0.4 the deck says 1 from its next keep-alive after an NXS says 2, and the
  NXS claims once (E20). Back after its cable, the deck is answered by the
  holder's `0x05` as a joining NXS is (E20; S13).
- **Tempo master.** On 0.3 the first play took master 6 s late, and a stopped
  master handed over only to a deck in sync (E19). On 0.4 the first play takes
  it at once, and a stopped master hands over as E06 found the NXS does (E21).
- **Media queries.** On 0.3, every slot of every peer every 5.4 s (E18). On
  0.4, once per slot that turns loaded (E20), and a stick taken out leaves the
  deck's library with nothing asked (E22).

The NOTES have the details, and the hardware sessions each stands against.

### What they can't stand for

- **Timing.** The deck is emulated too, on the same shared Mac. E21 ran
  under heavy load (a load average of 11-15 on twelve cores), so its timings
  say nothing about a real deck: it is there for what happened.
- **An emulated NXS's SD can't be taken out** through the emulator's panel
  channel, so E22 takes out a USB stick.
- **E01-E17's differences hold for the NXSs here,** a load playing at once
  first among them.

## E23-E25: plain sticks between a TriMixxx deck and two NXSs

prolink 0.5.0 serves a stick with no rekordbox library by its folders, as
E10-E17 found a CDJ serves one (`PLAIN-STICKS.md` §10.1), and reads one in a
player by its FOLDER menus. These are the deck doing both, on the same rig.

| | player | address | MAC |
| --- | --- | --- | --- |
| the deck | 4 | 169.254.190.254 | 02:54:4d:ed:fe:82 |
| a | 1 | 169.254.45.24 | 02:43:44:6e:2d:18 |
| b | 2 | 169.254.249.106 | 02:43:44:16:f9:6a |

- **The deck** is TriMixxx's emulated deck, with the fork's prolink-todos
  branch and prolink at `b393671` (E23, E25) or `4139302` (E24): 0.5.0's code.
  Player 4, its own stick PLAINMP3 (`plain-stick/`, as a pi-qemu folder
  stick).
- **The NXSs** have `--dsp-model`. a has PLAINMP3's image in USB and SAM1 in
  SD. b has the same image with its label cleared (`mlabel -c`), and in E24
  its `Many` folder deleted too, so that its stick differs from a's.
- **Before every session:** the rig as the session before left it, sliders
  centred. Each `cmd.txt` is a timed list, as E18-E22's.

### Sessions

| Session | Size | What it records |
| --- | --- | --- |
| `E23-deck-plain-stick-on-nxs` | 15.9 MB | the deck plays its plain file and says type 2; an NXS browses its stick, previews files, loads three in a row off it and one off its own |
| `E24-nxs-plain-sticks-on-deck` | 8.7 MB | two NXSs' plain sticks, one unlabelled, go in: the deck lists, walks and tags each over its own connection, streams a file, names an NXS's plays |
| `E25-plain-nxs-master` | 3.5 MB | an NXS playing a plain file takes master twice; the deck in sync keeps its own tempo and takes master back |

E23 is large for E14's reason: the NXS reads each file it loads whole.

### What they show

- **The deck's plain stick on an NXS** (E23): FOLDER alone at the root,
  folders then files in a CDJ's order, each file's tags and picture, loads
  by `?\0D <folder> <entry>`, TRACK n from `0x3100`, and the three hand-backs
  (`0x2005`) left unanswered with nothing one behind after them. The NXS's
  LINK shows the deck's stick at its volume's own size.
- **What the deck says it plays** (E23): track type 2 and the id its server
  gives the file.
- **A player's plain stick on the deck** (E24): listed with or without a
  label, walked by FOLDER menus with track type 2, its files' tags asked one
  at a time, a file streamed by its `?\0D` name, and a player's plays named
  in the History, each player asked about its own.
- **A master with no tempo** (E25): an NXS playing a plain file says `0xffff`
  and sends no beats; the deck in sync never takes it.

### What they can't stand for

- **A real CDJ on either side.** What only one can show, and how each wire
  change of 0.5.0 is to be checked on one, is in TriMixxx's
  `docs/plain-usb-plan.md` §7.
- **Timing,** as for E18-E22: the deck and the NXSs share a Mac with other
  work.

## What is in the packets

Link-local addresses (169.254.0.0/16), MACs made up from the players' names
(`02:43:44:..`), device numbers, and what the players say about what they
play: the loaded tracks' ids in the stick's `export.pdb` (77 and 116), their
tempos, pitches, beat counts and grid timings. In E07 the players also ask
each other about their USB slots, and the answers carry the stick's name
(`SAM1`), its track and playlist counts and its sizes. No titles, artists,
artwork, file paths or audio: nothing in E01-E09 browses another player's
stick or loads from it.

E10-E15 browse and load PLAINMP3, so they carry its made-up names, tags and
artwork, and the click tracks' audio, read over NFS. E16 and E17 browse and
load from SAM1: track and artist names, artwork, and the audio of the tracks
loaded, as the hardware captures carry the author's music.

E18-E22 add the deck: its link-local address, a MAC pi-qemu made up
(`02:54:4d:..`), and the name it announces, `CDJ-2000nexus`. It reads each
NXS's `export.pdb` over NFS, so they carry SAM1's track and artist names,
playlists and file paths, as E16 and E17 do; no artwork and no audio. Its
status names the track it loaded from its own stick, id 132.

E23-E25 carry PLAINMP3's made-up names, tags and artwork both ways, and its
click tracks' audio as each player reads a file it loads.

## Reading them

`prolink pcap run.pcap` counts what is in a capture. The fields that matter
here are in [`docs/PROTOCOL.md`](../docs/PROTOCOL.md) 3.2 and 3.3b: status
`0x89` (flags: `0x40` playing, `0x20` master, `0x10` sync), `0x7b` (play
state), `0x8c` (pitch, `0x00100000` = 0%), `0x92` (BPM x 100), `0x9e`
(master), `0x9f` (the deck master is being handed to, `0xff` none); beat
`0x54` (pitch), `0x5a` (BPM x 100), `0x5c` (beat in bar). The NOTES' numbers
come from those fields, decoded with `prolink-proto`, times from the pcap's
own timestamps.

E10-E25 come with `transcript.txt`, the output of `tools/emudump`, so nothing
needs building to read one: a line per event, by frame (Wireshark's
numbering), with every dbserver message through `prolink-proto`'s own codec,
every NFS, mount and portmap call with its reply and the path it names, media
queries and responses, and a player's status whenever it changes. It is a
crate of its own, outside the workspace, and reads `prolink-proto` and
`prolink-capture` without changing them:

```sh
cargo run --release --manifest-path tools/emudump/Cargo.toml -- E11-link-browse/run.pcap
cargo run --release --manifest-path tools/emudump/Cargo.toml -- --raw-status E12-link-load-play/run.pcap
```
