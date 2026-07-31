# 000 — Groundwork

What this library is built from, and what was read before a line of Rust was
written.

## Sources

| Source | What it gives |
|---|---|
| `prolinks-compat/docs/PROTOCOL.md` | The authoritative specification. Every offset, every constant, every "confirmed / inferred / unknown" marking. **Implement from this.** |
| `prolinks-compat/docs/FINDINGS.md` | F1–F49, C1–C14, O1–O7 — the evidence behind each claim, including the wrong turns. Cited in the code as `F<n>`. |
| `prolinks-compat/prolinks_poc/` | The working Python proof of concept: ~11.8 kLOC, both objectives working against two CDJ-2000NXS on firmware 1.44. This is the behavioural reference. |
| `prolinks-compat/ksy/*.ksy` | Kaitai schemas for UDP 50000, UDP 50002, dbserver and ONC RPC calls, validated against 7833 + 38371 + 11809 + 8415 real packets. |
| `mixxx/src/network/prolink/` | The C++ port (~16.8 kLOC including `src/library/prolink`) that this library is meant to replace. Covers both directions including the full serve stack. |
| `prolinks-compat/captures/S*/run.pcap` | 33 pcapng captures of real hardware, ~240 MB, plus JSONL journals. The test corpus. |

## What the reference implementations cover

Both objectives are complete in the Python PoC and in the Mixxx C++ port, so
"everything the Mixxx C++ code does" means:

**Consume** — passive discovery, device table, ONC RPC/NFSv2 client (portmap →
mountd → nfsd), pulling `export.pdb` over NFS, parsing it, the dbserver client,
browsing a player's menus, streaming a track's bytes.

**Serve** — the virtual CDJ (claim chain, keep-alive, unicast status, media
query and settings answers), portmapper + mountd + nfsd servers over a VFS,
the dbserver server with the full browse surface (12 root categories, the
drill-down grid, 12 sorts, search, harmonic key matching, metadata, track info,
artwork and the five transformed analysis blobs), two media at once as USB + SD.

## The traps, collected

These are the places where a plausible implementation is silently wrong. Each
is reproduced as a doc comment at the site in the Rust code that has to get it
right.

- The type byte at `0x0a` is shared across UDP ports and the layouts behind it
  are not: `0x06` is a keep-alive on 50000 and a media response on 50002. Two
  decoders, never one.
- The 50002 header puts the device name at `0x0b`–`0x1e` with a structural
  `0x01` at `0x1f`; on 50000 the name is `0x0c`–`0x1f` and the constant is at
  `0x20` (C14).
- dbserver strings are UTF-16 **big**-endian counted in **characters including
  the NUL**; NFS names are UTF-16 **little**-endian counted in **bytes**. They
  must never share a helper.
- A zero-length dbserver binary argument is **omitted from the wire**, not sent
  empty. Reading one blindly desynchronises the stream.
- A CDJ keeps only the **first 12 bytes** of a 32-byte NFS filehandle and
  overwrites the rest (F28). Key the handle table on those 12 bytes, and give
  each medium its own subtree so two media never mint the same handle.
- `export.pdb` PioStrings in the UTF-16 form (`0x90`) are **little-endian from
  offset + 4** (O6). Big-endian-from-offset+3 round-trips perfectly for ASCII,
  which is how the bug survived three sessions.
- Row offset `0x5a` of a track row is the **container** (`.mp3` 1, `.m4a` 4,
  `.flac` 5, `.wav` 11, `.aiff` 12 — F34), not `unknown6`. Announce the wrong
  one and a deck fetches the file and then refuses to decode it.
- Analysis blobs are **transformed**, not forwarded: the file is big-endian and
  the wire little-endian, and three of the five change layout too (F30).
- The fifth prefix word of the beat grid and detail waveform must be **non-zero**
  and monotonic; with zero the main waveform does not draw (F33).
- Argument 0 of a `GET_TRACK_INFO` path item is the **file size** — zero on
  every other menu item ever captured (F31).
- Menus must be keyed on `(descriptor, item count)`, not the count alone (F41).
- Answering an unknown dbserver request with `0x4003` stops a browse dead (F25).
- A device number outside 1–4 is accepted, statused, and then never browsed
  (F45). The failure is silent.
- UDP/111 is mandatory for serving: a deck retries `GETPORT` once a second
  forever and never falls back to the well-known ports (F46).
