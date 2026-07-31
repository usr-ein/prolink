# Work log

The ordered plan for building `prolink`, and where it currently stands. This
file is the index and is updated as work proceeds; `NNN-*.md` files hold the
notes for individual steps that needed more than a line.

Status key: `todo` · `wip` · `done` · `blocked` · `dropped`

## Order of work

| # | Step | Status | Notes |
|---|---|---|---|
| 00 | Study the reference material: `prolinks-compat` PoC, `docs/PROTOCOL.md`, the `.ksy` schemas, the Mixxx C++ port, the capture corpus | done | [000](000-groundwork.md) |
| 01 | Repository scaffolding: workspace, GPLv3, README, lint config | wip | [001](001-scaffolding.md) |
| 02 | `prolink-proto`: UDP 50000 discovery / claim / keep-alive codec | todo | |
| 03 | `prolink-proto`: UDP 50002 status, media query/response, settings | todo | |
| 04 | `prolink-proto`: dbserver (TCP 1051) message codec, both directions | todo | |
| 05 | `prolink-proto`: XDR / ONC RPC v2 / portmap / MOUNT / NFSv2, calls **and** replies | todo | |
| 06 | `prolink-capture`: pcapng reader + TCP reassembly, so tests can replay real captures | todo | |
| 07 | Corpus tests: replay every capture through the codecs; distil committed fixtures | todo | |
| 08 | `prolink-rekordbox`: `export.pdb` reader | todo | |
| 09 | `prolink-rekordbox`: ANLZ reader (raw payloads for serving, structured for consuming) + `PIONEER/*SETTING*.DAT` | todo | |
| 10 | `prolink-rekordbox`: the `Library` model — pdb rows joined into tracks/playlists | todo | |
| 11 | `prolink-proto`: analysis wire transforms (ANLZ → dbserver blobs) | todo | |
| 12 | `prolink`: network interface discovery, UDP plumbing, device table, passive discovery | todo | |
| 13 | `prolink`: virtual CDJ — claim chain, keep-alive, status emission, media/settings answers | todo | |
| 14 | `prolink`: ONC RPC / NFSv2 **client** — mount, walk, streaming reads | todo | |
| 15 | `prolink`: dbserver **client** — browse a player's library the way LINK does | todo | |
| 16 | `prolink`: consume facade — devices → slots → menus → tracks → file bytes | todo | |
| 17 | `prolink`: VFS + filehandle table (12-byte keying, NFC/case folding) | todo | |
| 18 | `prolink`: portmap + mountd + nfsd **servers** | todo | |
| 19 | `prolink`: dbserver **server** — root menu, drill-down grid, sorts, search, metadata, analysis | todo | |
| 20 | `prolink`: serve facade — two media as USB + SD, wired to the virtual CDJ | todo | |
| 21 | `prolink-cli`: `devices`, `rpcinfo`, `pull-db`, `tracks`, `browse`, `serve`, `pcap` | todo | |
| 22 | Docs: `PROTOCOL.md`, `ARCHITECTURE.md`, rustdoc, examples | todo | |
| 23 | Final pass: clippy, fmt, full test run, README polish, commit and push | todo | |

## Decisions taken along the way

Recorded here so they do not have to be re-derived.

| Decision | Where |
|---|---|
| `binrw` rather than Kaitai Struct for the wire formats | [001](001-scaffolding.md) |
| Workspace of five crates rather than one | [001](001-scaffolding.md) |
