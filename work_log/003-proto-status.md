# 003 — UDP 50002: status, media queries, device settings

## Templates, not constructors

Across 749 consecutive status packets from an idle CDJ-2000nexus only **six**
bytes ever changed. Building one field by field would mean inventing values for
~260 bytes nobody understands, and substituting a plausible zero has broken
playback twice (F33, F35).

So `CdjStatus` and `MediaResponse` are built from captured skeletons —
`src/status_templates.rs`, with the identifying and per-medium fields zeroed —
and only understood fields are substituted. The test that matters is not "the
field reads back" but **"we disturbed no byte we cannot name"**, and that is
what `a_status_differs_from_its_skeleton_only_where_we_substituted` asserts.

## Total accessors versus `Option`

`CdjStatus::parse` establishes, once, that the buffer carries the magic, the
`0x0a` kind byte and at least `MIN_LEN` (`0x76`) bytes. Every accessor below
that offset is therefore **total** — no `Option`, no re-checking. Accessors past
it return `Option`, and that `Option` is not a re-check: a short packet from
older firmware genuinely does not contain the field.

`MIN_LEN` is `0x76` rather than the `0xc8` reference implementations use,
because the media fields — the reason this packet matters at all — live below
`0x76`, and discarding a shorter packet would throw away slot occupancy for
nothing.

## Where illegal states were removed

- `MediaQuery` holds `DeviceNumber`, not `u8`, for both the requester and the
  target, so a query addressed to device 0 cannot be constructed and does not
  parse.
- The 50002 header is a separate code path from the 50000 one, and a test feeds
  a real keep-alive to the media-response parser to prove it is rejected rather
  than decoded into confident nonsense — `0x06` means different things on the
  two ports (C14).
