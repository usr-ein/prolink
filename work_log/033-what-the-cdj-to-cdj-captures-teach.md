# 033 — What the CDJ-to-CDJ captures teach

Done in this library, and checked on emulated CDJs; no real CDJ has seen it. The question was where this
library, as a player on a link, behaves differently from two real CDJ-2000NXS
talking to each other, in the hardware captures first and the emu-captures
(E01-E17) as support. To see what it does today, two captures were taken of a
TriMixxx deck (player 4, on 0.3) between two emulated NXSs on one link:

- **E18** (the plan's B1): the NXSs joining the deck.
- **E19** (B2): master and sync both ways.

The whole list, ranked, with what was left alone and why, is TriMixxx's
`docs/prolink-learnings.md`. Three changes landed here. Each is a wire change
in its own commit, with its `PROTOCOL.md` and `CHANGELOG.md` lines.

## A deck re-claiming its number for ever (§2.5)

**Keep-alive byte `0x25` is not latched at boot,** which is what F9 led us to
believe. It is a role: exactly one device on a link says `0x02`.

- **How decks settle it.** An AUTO deck holding the role answers another
  deck's stage-3 claim with a unicast `0x05` carrying its own number, and the
  claimer says `0x01` from then on (S13 17.389-17.392, a joiner; S26
  3.602-5.606, two decks booted together).
- **What happens when nobody answers.** A deck saying `0x02` that hears
  another say it re-claims its number every few seconds for as long as it
  hears it. The twenty sessions S10-S24, with this library's ancestors on the
  link saying `0x02`, hold 18-249 claims each. S24b and S18 show the claims
  stopping while the other device is silent, and starting again when it comes
  back saying `0x02`.
- **E18 is the same on 0.3.** The NXS that joined the deck re-claimed every
  4.5-5.2 s for as long as the capture ran, its keep-alives as far apart. That
  is Sam's problem 01. It is also why "only the last CDJ to join re-claims":
  an NXS answered by another NXS says `0x01` and stops.

**We now step aside.** Another device's keep-alive saying `0x02`, or a `0x05`
answering our own claim, makes us say `0x01` for good; a `0x05` also ends our
stage-3 burst. We never send `0x05`.

**The other half, not taken:** answering claims with `0x05` ourselves, as an
AUTO deck holding the role does. No capture shows whether a deck saying
`0x01` still defends its own number against an AUTO joiner: every `0x05` and
every `0x03` came from the device saying `0x02`. So holding the role could
have us confirm a duplicate number we never saw contested. We also announce
as MANUAL, and MANUAL decks send no `0x05` (S02, S2c). Stepping aside leaves
the role with Pioneer's firmware.

## Status on change, and a hand-over at its pace (§3.2, §3.3b)

**A real deck sends status every 200 ms and at once on a change,** never
closer than 61.7 ms to one peer (S28). A hand-over runs at that pace. The
holder names its successor at `0x9f` within 1 ms of its `0x27`, and the
requester claims 16-73 ms after asking. E19, on 0.3:

| hand-over | successor named | complete |
|---|---:|---:|
| an NXS asking the deck | 172 ms after the `0x27` | 373 ms |
| the deck asking an NXS | | 284 ms |

**Now:** a status packet goes at once when the master claim, the successor,
sync, the play state, the loaded track or a slot's state changes, with a
62 ms floor. In `prolink-cxx`, taking and handing over master wake on the
monitor's events instead of a 100 ms poll. The resend at 300 ms, the give-up
at 2 s and the stand-down at 2 s stay.

## Asking about a slot as a deck does (§3.3)

**A real deck asks about a slot when it turns loaded,** 0.3-0.9 s later in
all eight answered deck-to-deck queries. It asks again every 5.03 s until
answered (S10, S10b), and never once answered. We asked about both slots of
every peer every 5 s, answered or not (E18: 28 rounds to one NXS in 146 s;
Sam's 02).

**Now we ask as it does.** One thing had to move with it. The fork learnt
that a peer's medium was gone only from the zeroed answer a deck gives about
an empty slot, and that answer came because every slot was asked about. So
`PeerMedia` now forgets a slot's description once it is empty, and the
bridge reports that as a media event: the host reads an empty slot, as
before.

## What is checked, and what is not

**Checked, with `cargo test --workspace`** (780 passed):
- The two step-aside rules against every discovery packet in the corpus.
- Our claim burst against a stand-in answering with S13's own `0x05`.
- The status pacing, the hand-over clock, and the asker:
  - a query answered at its third try;
  - a swap;
  - an empty slot never asked about;
  - a torn-down player let go (below).
- The media events.

`tools/emudump` reads E10-E17 into the same transcripts, byte for byte.

**Checked on emulated CDJs.** Sam waived the checks on a real CDJ
(2026-10-13): "Emulated test is good enough for merging the changes." So a
TriMixxx deck between two emulated NXSs decided it, before and after 0.4.0,
in emu-captures E18-E22:
- **E20:** the NXS stops re-claiming once we say `0x01`, as S24b's and S18's
  pattern predicted: one claim at its start-up, where E18 has 96. Every
  device ends on its own number.
- **E20, the deck's cable pulled and put back:** the deck joins as an NXS
  joins a populated link. One number claim, answered by the holder's `0x05`
  within 3 ms, then `0x01` from its first keep-alive.
- **E21:** the hand-overs run in the NXS's order. E21 ran under heavy load,
  so its timings stand for nothing.
- **E20 and E22:** each slot is asked about once, 0.21-0.41 s after it turns
  loaded. A stick taken out of an NXS leaves the deck's library with nothing
  asked.

**The first run of E20 found a leak.** The asker held the player it asked
for with a strong reference, and the asker outlives every session. So the
session torn down when the cable came out kept its player running: its
keep-alives as player 4, and its file server on UDP 111. The session rebuilt
when the cable came back found the port taken, waited `PORTMAP_RELEASE` out,
and settled for watching as device 7, both on the link at once. The asker
now holds the player weakly (`57b2d8d`); E20 is the run after the fix. On 0.3
the survey took the player afresh each round, so this never shipped.

None of this has met a real CDJ. TriMixxx's `docs/prolink-learnings.md` §5
keeps, for a later session on real decks, what each change must show.
