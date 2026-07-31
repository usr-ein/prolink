# 001 — Scaffolding

## binrw, not Kaitai

The reference project expresses these formats as Kaitai `.ksy` schemas, and the
brief suggested carrying that forward. Three reasons not to:

1. **Kaitai generates no serializers except for Java and Python.** This library
   has to *emit* every format it parses — the whole serve side is emission — so
   Kaitai would give us readers and leave the writers hand-written. That is
   exactly the arrangement the C++ port ended up with, and it means two
   definitions of every format that must be kept in step by hand.
2. **`binrw`'s `#[binrw]` derives read *and* write from one definition**, so
   encoder and decoder cannot drift. That property is load-bearing here: the
   corpus test is "decode this captured packet and re-encode it byte-for-byte",
   which is only meaningful if the two directions come from the same source.
3. The Rust ecosystem's rekordbox crates already use `binrw`, so the workspace
   speaks one binary-format dialect throughout.

The `.ksy` files remain a useful independent opinion, and the doc comments here
cite the same findings they do.

**Where `binrw` is used and where it is not.** `djl` (UDP 50000) is contiguous
and fully understood, so it is a `#[binrw]` definition. `status` (UDP 50002) is
not: a 284-byte status packet has about a dozen namable fields and ~260 bytes we
cannot name, and declaring those as `binrw` padding would be inventing
structure. Those packets own their bytes and expose accessors — see [003](003-proto-status.md).

## Five crates, not one

The brief asks for something reusable across projects, which means the
dependency you take should be the smallest one that answers your question:

- a pcap analyser wants `prolink-proto` and `prolink-capture`, and no tokio;
- a rekordbox tool wants `prolink-rekordbox`, and no network stack;
- Mixxx wants `prolink`.

`prolink-proto` deliberately has no I/O, no clock and no async runtime, which is
what makes the entire protocol surface testable from byte literals.

## Strict lints

`cargo clippy --workspace --all-targets` must be silent, with `pedantic` and
`cargo` enabled and `unwrap`, `expect`, `panic`, indexing and `as` conversions
denied outside tests. Each of those is a place where a check is assumed rather
than proven, which is the thing this codebase is trying not to do.

`integer_division` was tried and dropped: in a byte codec every division is a
deliberate truncation (a chunk count, a frame index at 150 fps), so the lint
would have meant an `#[expect]` on each one — noise rather than safety.
`as_conversions` stays, because a silent numeric cast is where a length prefix
quietly wraps.
