# 004 — `prolink-proto::dbserver`

The dbserver / "remotedb" metadata protocol on TCP 1051, both directions.
One file, `crates/prolink-proto/src/dbserver.rs`, ~3200 lines including tests.
Nothing outside it was touched.

## What is in it

Roughly two halves.

**The codec.** `Message` (transaction id, `MessageKind`, `Arguments`) with
`decode(&[u8]) -> Result<(Message, usize)>` and `encode() -> Vec<u8>`; `Field`,
the tagged-argument enum; `FieldTag` and `ArgTag`, the two numberings;
`Arguments`, capped at twelve; a private `Reader` cursor whose every short read
is `Error::Truncated`. Plus the connection preamble, the port query, and the
string helpers.

**The vocabulary.** `MessageKind` (51 named types), `Descriptor` /
`MenuTarget` / `TrackType`, `Drill` and `drill_kind`, `ItemType`, `MenuItem`,
`ROOT_CATEGORIES`, `SortOrder` / `SortOption` / `SORT_MENU`, `METADATA_ITEMS`,
`TRACK_INFO_ITEMS`, `menu_label` / `unwrap_menu_label`, `FILTER_ALL`.

Request and reply builders sit on `Message`: `introduce`, `disconnect`,
`render` / `render_of`, `menu_request`, `search`, `success`,
`introduce_reply`, `unknown_3e03_reply`, `menu_header`, `menu_footer`,
`binary_reply`.

## Hand-rolled rather than `binrw`

The read direction expresses fine declaratively — `ksy/prolink_dbserver.ksy`
does it. The write direction does not: the twelve-byte argument-tag blob is a
*summary* of the arguments that follow, and whether an argument appears at all
depends on the **value** of the argument before it. In `binrw` both rules would
have to be written once for reading and again for writing, and duplicating the
omitted-blob rule is exactly the bug it causes. A cursor over `&[u8]` states
each rule once and hands back the consumed-byte count a stream reader needs.

## The traps, and where each comes from

| Trap | Handling | Finding |
|---|---|---|
| Two type numberings for the same five types | `Field::tag()` and `Field::arg_tag()` both come from one match on one enum; the header blob is derived at encode time, never stored | §5.1 |
| A zero-length blob is **absent** from the wire | `Message::encode` omits an argument iff `Message::decode` would infer its absence — the two are exact inverses. 37 real messages exercise it | §5.1, F7 |
| Strings are UTF-16BE counted in characters incl. NUL | `encode_string` / `string_characters`; `label_bytes` is the separate byte-counted helper for menu-item arguments 2 and 4 | §5.1 |
| Truncated ≠ malformed | every short read is `Error::Truncated`; a wrong first byte is `BadMagic` before anything is read | CONVENTIONS §5 |
| `0x3e03` must be answered, never errored | `Message::unknown_3e03_reply`, byte-identical to a real `0x4b02` | F25 |
| `0x0001` draws no reply and must not discard state | `MessageKind::expects_no_reply()`, and the doc says explicitly not to release the menu | F16, F27 |
| Result sets keyed on `(descriptor, count)` | documented on `Message::render_of`; both halves travel in the render | F27, F41 |
| Root ids are listed, not derived | `ROOT_CATEGORIES`, all twelve, in the order a real deck serves them | F26, F40, F43 |
| Drill-downs are one grid | `drill_kind(depth, category)`, with the thirteen observed types pinned in a test | F42 |
| Sort picks the second column, `(type << 8) \| 0x04` | `SortOrder::column()` / `track_item_type()` / `column_is_numeric()` | F43 |
| U+FFFA/U+FFFB label wrapping | `menu_label`; a bare label renders and is then not openable | F26 |
| Argument 10 tracks argument 7 | `MenuItem::argument10()` is derived from the flags | F32 |
| Argument 0 of a path item is the file size | `MenuItem::argument0`, whose doc enumerates all four things that slot carries | F31, F43, F44 |
| `0x04` is the title in one reply and the container in another | both tables name it; neither shadows the other | F35 |
| Slot byte separates two media on one connection | `Descriptor.slot`, resolved per message | F37 |
| Transaction ids start near `0x03800001` | `FIRST_TRANSACTION_ID` | C10 |

## Two type-level decisions worth knowing

`Descriptor::new` takes a `BrowsableDeviceNumber` (1–4) but `Descriptor::parse`
yields a plain `DeviceNumber`. Strict to build, permissive to parse: a server
outside 1–4 is never browsed (F45) and should be unwritable, while a *request*
from a device numbered 5 is still a request and dropping it would lose a message
we can answer.

`Arguments` cannot hold more than twelve fields. Twelve is structural — the tag
blob has twelve slots — so a thirteenth argument is unencodable rather than
merely unusual. `Arguments::new` is fallible; `From<[Field; N]>` rejects N > 12
at compile time with a `const` assertion.

## A new observation: a CDJ does not always write a NUL terminator

Not in `docs/FINDINGS.md`. **It wants a finding number.**

28 of the 12 638 string fields in the corpus terminate in `0x0009` instead of
`0x0000`. Every one is a label of a `MENU_ITEM` title row whose byte-length
argument (2) says that label is *empty*: the deck announces one character —
"the terminator and nothing else" — and the pair of bytes it writes there is
stale rather than zero. It appears in four independent sessions
(`S06-load-and-play`, `S13-format-ground-truth`, `S15a-sd-alone`,
`S15b-sd-and-usb`), so it is the hardware, not the tap.

Both reference implementations normalise it to a NUL, and neither round-trip
test notices: the Python corpus test reads only `LinkInfo*.pcapng`, which is the
one capture family that happens not to contain the case.

Handled by splitting the string body on **position** rather than on the unit
being NUL — the count says the last unit is the terminator, whatever it holds —
and storing it: `Field::Text { text, terminator: Option<u16> }`. `Field::text()`
never returns the terminator, so callers are unaffected; `Field::from("…")`
terminates properly. `terminator` is `Option` because a zero-character field is
expressible, though the corpus contains none.

A real one is committed as the `STALE_TERMINATOR` fixture.

## How it was validated

Twelve real messages are committed as byte literals in the test module, each
extracted from a named capture by reassembling the TCP-1051 stream and slicing
one message out. None was written by hand. They cover: the omitted blob
(`GET_WAVEFORM_PREVIEW`, five arguments declared and four on the wire), an empty
*string* that **is** on the wire (`0x4b02`), a real root-category row with the
U+FFFA wrapping, the metadata title item with its artwork id, the track-info
path item with `argument0 = 7 633 531`, a present 148-byte blob on an undecoded
message type (`0x4902`), and the stale terminator above.

Beyond the committed floor, a throwaway harness (a `/tmp` crate depending on
`prolink-proto` by path, fed a hex dump of every TCP-1051 stream in
`prolinks-compat/captures/` plus dysentery's `LinkInfo*.pcapng`) checked:

- **11 809 messages** framed by *this* decoder alone out of 30 raw streams,
  every stream consumed end to end with nothing left over. That is the check
  that validates the omitted-blob rule, since one misstep desynchronises
  everything after it;
- every one of the 11 809 re-encodes **byte for byte**;
- 58 distinct message types seen against the 51 named here;
- **2 381 748 partial buffers** — every proper prefix of every message — all
  report `Error::is_truncated()`, so a stream reader can never mistake "not yet
  arrived" for "drop the connection".

The harness is not committed: it reads files outside this repository. Once
`prolink-capture` lands its corpus support this should be re-expressed as a
corpus test in `prolink-proto/tests/`, and the numbers above are what it should
reproduce.

## Not settled

- **The stale terminator has no finding number** and no explanation beyond
  "uninitialised buffer". Reproduced, not understood.
- **`ArgTag::U8` (`0x04`) and `ArgTag::U16` (`0x05`) are inferred.** No capture
  contains a `UInt8` or `UInt16` *argument*; only the header's own fields use
  those field tags. If a peer ever sends one and the mapping is wrong, the
  agreement check in `Reader::field` will reject the message rather than
  mis-parse it, which is the failure we want.
- **`MetadataSlot::argument0`** — `1` on eight of the thirteen, `0` on five, and
  no rule fits. Reproduced as observed.
- **The drill chains** (which filter field each level narrows on, and KEY's
  extra harmonic-tolerance level) are library semantics rather than wire
  vocabulary, so only the *formula* and the observed grid live here. A server
  needs the chains; they belong wherever the library layer ends up.
- **`0x3d03`'s reply is a guess** — no capture shows a real one. Documented as
  such on the constant.
- `0x3b03`, `0x3903`, `0x3001`, `0x3401` appear in the corpus and are undecoded;
  they round-trip as unknown types.
- The `MENU_ITEM` label byte-length arguments count UTF-16 **code units** here.
  The Python reference uses `len(str)`, i.e. code points, which differs for
  astral characters — its field encoder is right and its label helper is not.
  Pinned by `a_label_length_counts_utf16_units_not_code_points`.

## A note for whoever owns `lib.rs`

The crate doc links `dbserver::encode_string`; that name is kept, so the link
resolves. `cargo doc -p prolink-proto --no-deps` is clean.
