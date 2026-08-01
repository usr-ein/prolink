# Work log

The ordered plan for building `prolink`, and where it currently stands. This
file is the index and is updated as work proceeds; `NNN-*.md` files hold the
notes for individual steps that needed more than a line.

Status key: `todo` · `wip` · `done` · `blocked` · `dropped`

## Order of work

| # | Step | Status | Notes |
|---|---|---|---|
| 00 | Study the reference material: `prolinks-compat` PoC, `docs/PROTOCOL.md`, the `.ksy` schemas, the Mixxx C++ port, the capture corpus | done | [000](000-groundwork.md) |
| 01 | Repository scaffolding: workspace, GPLv3, README, strict lint config | done | [001](001-scaffolding.md) |
| 02 | `prolink-proto`: UDP 50000 discovery / claim / keep-alive codec | done | `djl.rs`; a captured keep-alive round-trips byte-for-byte |
| 03 | `prolink-proto`: UDP 50002 status, media query/response, settings | done | `status.rs`; emitted packets diff against the skeleton |
| 04 | `prolink-proto`: dbserver (TCP 1051) message codec, both directions | done | [004](004-proto-dbserver.md) |
| 05 | `prolink-proto`: XDR / ONC RPC v2 / portmap / MOUNT / NFSv2, calls **and** replies | done | [005](005-proto-rpc.md) |
| 06 | `prolink-capture`: pcapng reader + TCP reassembly, so tests can replay real captures | done | [006](006-capture.md); 244,501 packets across 33 files, matching the Python reference exactly |
| 07 | Corpus tests: replay every capture through the codecs; distil committed fixtures | done | [007](007-corpus-tests.md); 33 captures, zero failures of any kind |
| 08 | `prolink-rekordbox`: `export.pdb` reader | done | [008](008-rekordbox.md); 651 tracks / 329 artists / 274 albums from the real export |
| 09 | `prolink-rekordbox`: ANLZ reader (raw payloads for serving, structured for consuming) + `PIONEER/*SETTING*.DAT` | done | [008](008-rekordbox.md) |
| 10 | `prolink-rekordbox`: the `Library` model — pdb rows joined into tracks/playlists | done | [008](008-rekordbox.md) |
| 11 | `prolink-proto`: analysis wire transforms (ANLZ → dbserver blobs) | done | `analysis.rs`; takes raw tag payloads so the wire layer stays free of the file layer |
| 12 | `prolink`: network interface discovery, UDP plumbing, device table, passive discovery | done | [012](012-discovery.md) |
| 13 | `prolink`: virtual CDJ — claim chain, keep-alive, status emission, media/settings answers | done | [012](012-discovery.md); conflict back-off untested against hardware |
| 14 | `prolink`: ONC RPC / NFSv2 **client** — mount, walk, streaming reads | done | [014](014-consume.md) |
| 15 | `prolink`: dbserver **client** — browse a player's library the way LINK does | done | [014](014-consume.md) |
| 16 | `prolink`: consume facade — devices → slots → menus → tracks → file bytes | done | [014](014-consume.md) |
| 17 | `prolink`: VFS + filehandle table (12-byte keying, NFC/case folding) | done | keyed on `FileHandleKey`, so a CDJ rewriting a handle's tail still resolves |
| 18 | `prolink`: portmap + mountd + nfsd **servers** | done | [018](018-nfs-server.md) |
| 19 | `prolink`: dbserver **server** — root menu, drill-down grid, sorts, search, metadata, analysis | done | [019](019-dbserver-server.md) |
| 20 | `prolink`: serve facade — two media as USB + SD, wired to the virtual CDJ | done | `ProLinkServer`, plus an end-to-end loopback test of server against client |
| 21 | `prolink-cli`: `devices`, `rpcinfo`, `pull-db`, `tracks`, `browse`, `serve`, `pcap`, `status` | done | ten commands, covering every capability on the acceptance list |
| 22 | Docs: `PROTOCOL.md`, `ARCHITECTURE.md`, rustdoc, examples | done | plus `CONVENTIONS.md`, CI and a dependency licence gate |
| 23 | Final pass: clippy, fmt, full test run, README polish, commit and push | done | 596 tests, clippy silent, rustdoc clean, pushed to `origin/main` |
| 25 | The one-minute failure: `0x3001` must draw **no reply** | done | [025](025-the-one-minute-failure.md); found in deck-to-deck captures S25/S27 |
| 26 | Sort by DJ PLAY COUNT ("PLAYSTATE") | done | works; the deck sends sort `0x10` and gets the list in descending play-count order, verified in `prolink-issue6.pcap` |
| 27 | The key-matching indicator beside a playing track | done | argument 11 of a track row is the Camelot wheel index; decoded by correlating 1265 real deck rows in S27 against `testdata/export.pdb` |
| 28 | Beat sync and tempo sharing: UDP 50001 `0x26` master request / `0x27` handoff response | done | [026](026-beat-sync-and-tempo-master.md); both packets modelled, plus the sync/pitch/yield status fields |
| 29 | Tag list: tagging and the TAG LIST menu | done | [027](027-the-tag-list.md); `0x3002` adds, `0x100f` lists, flags bit 0 marks a tagged row. Playlist creation deliberately ignored |
| 30 | How AUTO device numbering resolves when two decks boot simultaneously, both on AUTO | done | S26: it does not resolve anything — each deck asks for a remembered number in its first CLAIM_IP and neither moves (F58). Nothing to implement |
| 32 | Mixxx parity audit: nine gaps a read of the C++ turned up | done | [029](029-mixxx-parity-audit.md); five were behaviours rather than functions |
| 31 | The key-matching indicator: why it does not light against this server | open | [028](028-the-key-matching-indicator.md); every field byte-compared against a real CDJ serving the same medium |
| 31 | Stopping cleanly: eject the media on ctrl-c so consumers unmount | done | [028](028-stopping-cleanly.md); `0x02` for 1.5 s then `0x03`, which is what draws the deck's `UMNT` — 9 and 16 ms later in S15b |
| 24 | `prolink-proto::beat` + `prolink::monitor`: beat packets, phase, tempo and tempo master | done | [024](024-beats-and-status.md); all 1110 beat packets in the corpus re-encode byte for byte |
| 34 | One device, both directions: `VirtualPlayer` consumes and serves from one identity, media hot-plug, and binding when the cable appears | done | [031](031-one-device-both-directions.md) |
| 33 | Becoming a real player: claim 1–4 from the consumer side, share UDP 50002, and stop blocking the host's UI thread | done | [030](030-becoming-a-real-player.md); five defects from a Pi running Mixxx against two CDJs |

## Acceptance: what the CLI must be able to do

Stated by the user, and what "end to end" means. Each maps to a command and to
the steps that have to be finished before it works.

| # | Capability | Command | Needs |
|---|---|---|---|
| 1 | Join a CDJ network, see the other players, and browse a USB in one of them | `prolink devices`, `prolink browse <device> --slot usb` | 12, 13, 14, 15, 16 |
| 2 | The same for an SD card | `prolink browse <device> --slot sd` | as above |
| 3 | A USB in this machine that other CDJs see on their LINK screen | `prolink serve --usb <path>` | 13, 17, 18, 19, 20 |
| 4 | Two USBs, the second presented as an SD card | `prolink serve --usb <path> --sd <path>` | as above, plus per-slot resolution (F37) |
| 5 | Other CDJs browsing categories, drilling down, searching and sorting | (same session) | 19 |
| 6 | Other CDJs **loading and playing** a track from it | (same session) | 18, 19, and the analysis transforms (11) |
| 7 | Live playing information: beat pulses, phase, tempo, the loaded track, who is master | `prolink status --watch` | 24 |

Capability 7 needs two sources and only one of them is passive: beat packets are
broadcast on UDP 50001, but the loaded track, the play state and **who holds
tempo master** are published only in UDP-50002 status, which is unicast to peers
that have announced themselves (F21). So `status` shows tempo and phase without
announcing, and everything else only with `--announce`.

Capabilities 3–6 additionally need a **browsable device number in 1–4** and a
**portmapper on UDP/111** — the latter is privileged, so on Linux either run as
root or set `net.ipv4.ip_unprivileged_port_start=111`; macOS cannot serve it
without elevation (F45, F46).

## What is still unproven

Everything below is implemented and tested without hardware. None of it has met
a real CDJ, and these are the places where that matters most:

| | |
|---|---|
| **The claim chain's back-off** | No capture in the corpus contains a type-`0x08` conflict packet — nothing has ever contested a number on that rig — so our response to one is written from the specification and never observed. The corpus test asserts the absence, so the day a capture contains one, it says so. What hardware *does* send is type `0x05`, and it carries the sender's own number rather than the contested one (F58), so it is not a rejection and we are right not to treat it as one. We can only fail to notice a taken number if its holder sent no keep-alive during the five seconds we watch, and they go out every 2.0026 s. |
| **`0x3d03`** | Acknowledged with a reply shaped like the one a deck sends for `0x3100`. No capture shows a real answer. Erroring is known to be worse (F25). |
| **The `0x35` settings variants** | Four `*SETTING*.DAT` files exist and nothing in the request obviously selects between them. We answer with `MYSETTING.DAT`, which is what the one captured exchange carried. |
| **Serving on macOS** | UDP/111 needs root there and there is no sysctl equivalent. Tested only on ephemeral ports. |
| **Two media at once** | Modelled from F37 and exercised over loopback, but never in front of a deck switching slots mid-browse. |
| **The `PSSI` and colour-waveform tags** | Parsed, but nothing in the serve path uses them, and no deck we have has asked. |

## Decisions taken along the way

Recorded here so they do not have to be re-derived.

| Decision | Where |
|---|---|
| `binrw` rather than Kaitai Struct for the wire formats | [001](001-scaffolding.md) |
| Workspace of five crates rather than one | [001](001-scaffolding.md) |
| Template-substitution rather than field-by-field construction for the packets we do not fully understand | [003](003-proto-status.md) |
| Our own `export.pdb` and ANLZ readers; `rekordcrate` behind an optional feature | [008](008-rekordbox.md) |
| `prolink-proto::analysis` takes raw tag payloads, not a parsed ANLZ file, so the wire layer does not depend on the file layer | [011](011-analysis-wire.md) |
