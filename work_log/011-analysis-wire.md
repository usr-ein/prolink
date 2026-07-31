# 011 — ANLZ → dbserver analysis transforms

## Bytes in, bytes out

These functions take the **raw payload of an ANLZ tag** — the bytes after that
tag's own header — rather than a parsed analysis file. That is a layering
decision: `prolink-proto` is the wire layer and must not depend on
`prolink-rekordbox`. The caller has already parsed the file for its own reasons
and passes the payload through.

The side benefit is that every transform is testable from a byte literal, which
is how the captured evidence is written down anyway.

## The one field that defeats derivation

`PrefixWord` — the fifth prefix word of the beat grid and the detail waveform.
The two observed values are for the same track in the same load, 2.58 s apart,
differing by 104,044: a free-running counter or an allocator address on the
serving deck, at roughly 40,000 per second.

The reasoning that predicted it could be zero was: a value the client cannot
recompute is a value the client cannot check, therefore it is ignored. That is
wrong, and hardware settled it — **with zero the main waveform does not draw**
(F33). A receiver does not have to *validate* a field to *reject* it; zero is a
perfectly good sentinel for "absent".

So the type is `NonZeroU32` underneath, and `PrefixWord::from_elapsed` emits a
counter of the same shape. What the number means is still unknown.

## What each transform does

| Blob | Transform |
|---|---|
| `PVBR` → `0x4502` | every 32-bit word byte-swapped; nothing else changes |
| `PQTZ` → `0x4602` | 20-byte LE prefix, then the file's 8-byte `(beat, tempo, time)` entries byte-swapped and padded to 16 with eight `0xff` |
| `PWAV`+`PWV2` → `0x4402` | each packed byte split into `(height = b & 0x1f, whiteness = b >> 5)`, then the 100-byte tiny waveform appended — **900 bytes, not 800** |
| `PWV3` → `0x4a02` | 20-byte LE prefix, payload verbatim (single bytes have no byte order) |
| `PCOB` → `0x4702` | two blobs, cues **sorted by time**, positions as frame indices at 150 fps, **truncated not rounded** |
