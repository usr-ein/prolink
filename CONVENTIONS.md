# Conventions

How code in this repository is written. Read this before adding a module.

## 1. Parse, don't validate

The organising principle
([Alexis King, 2019](https://lexi-lambda.github.io/blog/2019/11/05/parse-dont-validate/)).
Checks happen **once**, at the boundary between this program and the outside
world, and the result is a value whose *type* records that the check passed.
Nothing downstream re-checks, and nothing downstream can forget to.

In practice:

- **Make illegal states unrepresentable.** `djl::Packet` has no `kind` field
  because the kind is a function of the body; a packet whose header says
  keep-alive and whose body is a hello cannot be built. Where a wire field and a
  derived value could disagree, only one of them is a field.
- **Give an invariant a type, not a comment.** `DeviceNumber` is non-zero.
  `BrowsableDeviceNumber` is 1–4, because outside that range a deck accepts an
  announcement in full and then silently never browses it (F45) — a bug worth
  making unwritable rather than documenting.
- **Constructors are the checkpoint.** `CdjStatus::parse` proves magic, kind and
  minimum length, so every accessor below that length is total. Accessors past
  it return `Option`, because a short packet from older firmware genuinely does
  not contain the field — that `Option` encodes a fact about the wire, not a
  re-check of something already established.
- **Return the parsed thing, not a boolean.** `Slot::parse(&str) -> Option<Slot>`,
  never `is_valid_slot(&str) -> bool` followed by a second pass.

## 2. Newtypes for wire enumerations, real enums for dispatch

A value we might not have seen every case of — a packet kind, a device kind, a
slot, a media state — is a newtype over its integer with associated constants
and a hand-written `Debug`. A decoder that refused an unknown value would take
out the cases it *does* understand for the sake of one field it may not need.

A value we dispatch on and own every case of — a message body, a decoded packet
— is a real `enum`, with an explicit `Unknown` variant that carries enough to
re-encode.

## 3. Byte-exactness is a feature

The goal is to be indistinguishable from a real CDJ. So:

- Decoding preserves bytes we do not understand, and re-encoding reproduces
  them. Round-trip tests assert this against captured traffic.
- Where a packet is mostly unknown, build it from a **captured skeleton** and
  substitute only understood fields. Sending a plausible zero has broken
  playback twice (F33, F35).
- A test that asserts "we changed only these offsets" is worth more than one
  that asserts "the field reads back".

## 4. Provenance in the doc comment

Every non-obvious constant carries the finding that establishes it: `F<n>` for a
finding, `C<n>` for a correction to the pre-hardware literature, `O<n>` for an
observation. If a value is reproduced without being understood, the doc comment
says so in those words. The question a maintainer asks first — "where did this
magic number come from" — is answered in place.

Where the pre-hardware literature is wrong, say what it claims and what the
hardware does. Five bugs in the reference implementation came from *deriving* a
value that looked derivable; each derivation was a proxy that happened to be
unique in the evidence then available. Prefer a table of observed values to a
formula, and say which it is.

## 5. Errors

One error type per crate, `thiserror`-derived. The distinction that matters is
`Error::is_truncated()`: on a stream protocol, running off the end of the buffer
means "wait for more bytes" and anything else means "this peer is not speaking
the protocol". Never conflate them.

An error and an empty result are not the same thing. On a CDJ's screen an error
and an empty folder look identical, so the set of cases handled is a
user-visible surface, not an internal detail.

## 6. Lints

`cargo clippy --workspace --all-targets` must be **silent**. The workspace
denies `unwrap`, `expect`, `panic`, indexing, `as` conversions and integer
division outside tests. That is deliberate: each of them is a place where a
check was assumed rather than proven.

When one is genuinely unavoidable, allow it at the narrowest possible scope with
`#[expect(lint, reason = "...")]` and a reason that explains why it cannot fail.
Do not add crate-wide allows.

`cargo fmt --all --check` must be clean. `rustfmt.toml` is the authority; do not
hand-format around it.

## 7. Tests

- **Name the behaviour, not the function.** `a_zero_length_blob_is_omitted_from_the_wire`,
  not `test_encode_2`.
- **Assert against captured bytes** wherever captured bytes exist. A round-trip
  test between our own encoder and our own decoder proves they agree with each
  other, which is not the same as agreeing with a CDJ — the reference
  implementation had an encoder and a decoder that agreed perfectly on a bug
  that only showed on non-ASCII input (O6).
- Every test file that consumes the capture corpus must also carry a **committed
  fixture floor**, so a coverage regression cannot hide behind an empty corpus
  on a machine that has no captures.

## 8. Documentation

Module docs explain *what the format is and what is easy to get wrong*, not what
the code does. Prose, not bullet soup. Assume the reader is about to change the
code and needs to know which parts are load-bearing.

## 9. Layering

```
prolink-proto       pure codecs; no I/O, no clock, no allocation policy
prolink-rekordbox   the files on a rekordbox medium; no network
prolink-capture     reading pcap/pcapng; test and analysis support
prolink             sockets, timers, state machines; depends on all of the above
prolink-cli         the binary
```

`prolink-proto` must not depend on `prolink-rekordbox`. Where a wire transform
needs data from an analysis file, it takes the raw payload bytes as an argument
and the caller supplies them. That keeps the wire layer free of file-format
concerns and testable from a byte literal.
