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
| 07 | Corpus tests: replay every capture through the codecs; distil committed fixtures | wip | agent `corpus-tests` |
| 08 | `prolink-rekordbox`: `export.pdb` reader | done | [008](008-rekordbox.md); 651 tracks / 329 artists / 274 albums from the real export |
| 09 | `prolink-rekordbox`: ANLZ reader (raw payloads for serving, structured for consuming) + `PIONEER/*SETTING*.DAT` | done | [008](008-rekordbox.md) |
| 10 | `prolink-rekordbox`: the `Library` model — pdb rows joined into tracks/playlists | done | [008](008-rekordbox.md) |
| 11 | `prolink-proto`: analysis wire transforms (ANLZ → dbserver blobs) | done | `analysis.rs`; takes raw tag payloads so the wire layer stays free of the file layer |
| 12 | `prolink`: network interface discovery, UDP plumbing, device table, passive discovery | done | [012](012-discovery.md) |
| 13 | `prolink`: virtual CDJ — claim chain, keep-alive, status emission, media/settings answers | done | [012](012-discovery.md); conflict back-off untested against hardware |
| 14 | `prolink`: ONC RPC / NFSv2 **client** — mount, walk, streaming reads | wip | agent `consume-stack` |
| 15 | `prolink`: dbserver **client** — browse a player's library the way LINK does | wip | agent `consume-stack` |
| 16 | `prolink`: consume facade — devices → slots → menus → tracks → file bytes | wip | agent `consume-stack` |
| 17 | `prolink`: VFS + filehandle table (12-byte keying, NFC/case folding) | done | keyed on `FileHandleKey`, so a CDJ rewriting a handle's tail still resolves |
| 18 | `prolink`: portmap + mountd + nfsd **servers** | wip | agent `nfs-server` |
| 19 | `prolink`: dbserver **server** — root menu, drill-down grid, sorts, search, metadata, analysis | wip | agent `dbserver-server` |
| 20 | `prolink`: serve facade — two media as USB + SD, wired to the virtual CDJ | todo | |
| 21 | `prolink-cli`: `devices`, `rpcinfo`, `pull-db`, `tracks`, `browse`, `serve`, `pcap` | wip | `interfaces`, `devices`, `announce` done |
| 22 | Docs: `PROTOCOL.md`, `ARCHITECTURE.md`, rustdoc, examples | wip | spec and architecture written; CI and a licence gate added |
| 23 | Final pass: clippy, fmt, full test run, README polish, commit and push | todo | |

## Decisions taken along the way

Recorded here so they do not have to be re-derived.

| Decision | Where |
|---|---|
| `binrw` rather than Kaitai Struct for the wire formats | [001](001-scaffolding.md) |
| Workspace of five crates rather than one | [001](001-scaffolding.md) |
| Template-substitution rather than field-by-field construction for the packets we do not fully understand | [003](003-proto-status.md) |
| Our own `export.pdb` and ANLZ readers; `rekordcrate` behind an optional feature | [008](008-rekordbox.md) |
| `prolink-proto::analysis` takes raw tag payloads, not a parsed ANLZ file, so the wire layer does not depend on the file layer | [011](011-analysis-wire.md) |
