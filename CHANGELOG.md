# Changelog

What changed between releases of the prolink crates, and above all what
changed **on the wire**. A real CDJ sees only the wire, so every change there
is listed under its own heading, with the capture that justifies it.

Releases before 0.4.0 are described by their tags (`v0.1.0` to `v0.3.0`) and
by [`work_log/`](work_log/README.md).

## Unreleased

### On the wire

- **A medium's size is eight bytes, and ours is the volume's own.** The media
  response carries total and free bytes as eight big-endian bytes at `0xb0`
  and `0xb8` (S13 f98: `00000007 28ca8000` and `00000005 439f8000`, 28.6 GiB
  with 21.1 free; S4b, S15a), and `0x4902` as two little-endian words each,
  the high one first. We read and wrote four bytes at `0xb4` and `0xbc`, and
  left the captured answer's high words in place, so every medium we served
  was announced as that captured 32 GB stick. `docs/PROTOCOL.md` §3.3.
- **Three requests about a plain file are answered with nothing.** A player
  hands the server of a plain file the preview waveform (`0x2005`) and the
  seek table (`0x2205`) it worked out for it, and says which files it cannot
  play (`0x3503`); an emulated CDJ-2000NXS serving a plain stick answers none
  of them (`emu-captures/PLAIN-STICKS.md` §7.7, E14, E15). We acknowledged
  them, as any request we do not implement, and a player waiting for no reply
  takes one for the answer to its next request (F16).
- **A stick with no rekordbox library can be served, by its folders**, as a
  CDJ serves one (`emu-captures/PLAIN-STICKS.md` §10.1, read off emulated
  NXSs). It is described with its volume label, no date, no `1000`, no
  counts and `02 00` at `0xaa`, in the media response and in `0x4902`; its
  root menu is one category, FOLDER; `MENU_FOLDER` lists folders, then
  files; `GET_GENERIC_METADATA` answers a file's tags, read when a player
  first asks; `GET_TRACK_INFO` answers five items and no path; `0x3100`
  answers the file's place in its folder; the preview and the seek index
  are zeros; the artwork is the file's JPEG or its folder's `cover.jpg`; and
  NFS opens the file by the name a player asks for, `?` then `D <folder>
  <entry>`. Until now such a stick was refused. An emulated NXS browses one,
  reads its tags, loads a file and plays it (E23). We list less than a CDJ
  does: only what a player can play (`docs/PROTOCOL.md` §8).
- **Our status can say a plain file is loaded**: track type `2` at `0x2a`,
  with the file's id on its medium, as a CDJ playing one says (`02 03 02 00`
  at `0x28`, `PLAIN-STICKS.md` §7.8); a player then names it with
  `GET_GENERIC_METADATA`. We said `1`, a rekordbox track, for anything
  loaded.
- **We ask about a player's plain files as a player does**: a file's tags
  with `GET_GENERIC_METADATA` and track type 2, never the `0x2002` a
  rekordbox track is asked with; its track info and artwork by its own id;
  and the file itself with one NFS `LOOKUP` of `?` then `D <folder> <entry>`
  on the export's root, the two numbers its track info gives
  (`PLAIN-STICKS.md` §4.4, §7.1-§7.3). An emulated NXS's plain stick is named
  and streamed this way (E24).

### Behind the API

- **prolink-proto:** `MediaResponse::total_bytes` and `free_bytes` are `u64`,
  and `MediaResponseBuilder::size` takes `u64`; a size not given is sent as
  zero rather than as the template's. `MediaInfo`'s sizes are `u64`, its two
  "unknown" words (7 and 5 in the capture) being their high words.
- **prolink-proto:** `MessageKind::HAND_BACK_WAVEFORM_PREVIEW`,
  `HAND_BACK_VBR_INDEX` and `UNPLAYABLE`. `MessageKind::expects_no_reply` is
  true for them, and for `UNKNOWN_3001`, which the server already left
  unanswered.
- **prolink:** `MediaDescription::total_bytes` and `free_bytes` are
  `Option<u64>`.
- **prolink-proto:** `MediaResponse::is_plain` and
  `MediaResponseBuilder::plain`; `MediaInfo::plain`.
- **prolink-proto:** `LoadedTrack::track_type`.
- **prolink:** `DbClient::generic_metadata`, `plain_track_info` and
  `plain_artwork`; `TrackInfo::plain_lookup`; `NfsClient::open_plain`.
- **prolink:** `Medium::open` reads a mounted medium of either kind, and
  `Medium::from_folders` a plain one: `folders`, `is_plain`, `track_count`,
  `tags` and `track_files` say what it holds. `serve::folder` walks a stick
  as a player is shown it (`FolderTree`, `WalkLimits`) and `serve::tags`
  reads a file's tags (`FileTags`). `MediaDescription::plain`.
  `local_volumes` finds every mounted stick, `Volume::rekordbox` says which
  kind it is, and `volumes::label_of` is public. `Error::NoMusic` refuses a
  plain stick with nothing to play.

### New dependencies

- **lofty 0.24** (MIT OR Apache-2.0), default features off: a plain file's
  tags, duration, bitrate and picture. 0.25 wants Rust 1.89, past the
  workspace's 1.88.
- **rustix 1** (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT), `fs`
  only: `statvfs`, for a served medium's size, where the workspace forbids
  `unsafe`.

## 0.4.0

What the CDJ-to-CDJ captures teach ([work log 033](work_log/033-what-the-cdj-to-cdj-captures-teach.md)).
None of it has met a real CDJ yet.

### On the wire

- **We give up keep-alive byte `0x25` to a real deck.** It says whether a
  device was first on the network, and exactly one device on a link says so:
  a deck saying it beside another that says it too re-claims its number every
  few seconds for as long as it hears it (S10-S24: 18-249 claims a session
  beside this library's ancestors; S24b and S18). We used to say it for good
  after starting into an empty network, which kept a CDJ re-claiming for ever
  beside us. Now another device's keep-alive saying `0x02`, or a `0x05`
  answering our own stage-3 claim (S13 17.391, S26 5.606), makes us say `0x01`
  for good, and a `0x05` ends our stage-3 burst there. No new packet type: we
  never send `0x05`. `docs/PROTOCOL.md` §2.5.
- **Status goes out at once when what we publish changes**: the master claim,
  the successor at `0x9f`, sync, play state, the loaded track, a slot's state.
  Never sooner than 62 ms after the last packet (S28's floor, 61.7 ms), and
  otherwise every 200 ms as before. Same packets, same contents; only when
  they leave. A real deck sends status the same way, and a hand-over waits for
  it: one asked for master named its successor 172 ms after its `0x27` here
  before, against 3.3-5.2 ms on S28's decks. `docs/PROTOCOL.md` §3.2, §3.3b.
- **We ask a peer about a slot as a deck does** (prolink-cxx): when its status
  shows the slot loaded, again every 5 s while unanswered, and never once
  answered, until the slot empties and loads again. A real deck asks 0.3-0.9 s
  after a slot turns loaded and retries every 5.03 s until answered (all eight
  answered deck-to-deck queries in the corpus; S10, S10b). We used to ask
  about both slots of every peer every 5 s, answered or not, loaded or not.
  `docs/PROTOCOL.md` §3.3.

### Behind the API

- **prolink-cxx:** taking and handing over tempo master wake on the monitor's
  events instead of a 100 ms poll, so we claim in the packet after a holder's
  `0x9f` names us, and let go in the packet after our successor claims. The
  deadlines stay: the `0x26` once more after 300 ms without a grant, nothing
  more after 2 s, and a successor that never picks up is still given up on.
- **prolink:** `PeerMedia` forgets a slot's description when the slot is empty
  (kept through the eject's unmounting states), so the next medium is
  described afresh; `VirtualCdj::ask_media` asks about one slot without waiting
  for the answer.
- **prolink-cxx:** a slot losing its description is a `MediaInfo` event, so a
  medium taken out of a player still reaches the host as an empty slot. It used
  to reach it as the zeroed answer about an empty slot, which only came because
  every slot was asked about every 5 s.
- **prolink-cxx:** `Player::is_playing`, the status flag `0x40`: what goes
  with beats and with taking master, where the play state does not (a deck
  playing a plain file is at play state 3 with the flag clear; emulated, E12).
- **prolink:** `VirtualCdj::offer_tempo_master` refuses while a hand-over is
  in flight, so a master names one successor at a time: a player claims
  12-69 ms after it is named (S28), and a second name written over the first
  could leave two of them claiming master. A hand-over asked for with a `0x26`
  still overrides an offer.
