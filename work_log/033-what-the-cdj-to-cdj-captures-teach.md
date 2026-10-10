# 033 — What the CDJ-to-CDJ captures teach

Done in this library, waiting on a real CDJ. The question was where this
library, as a player on a link, behaves differently from two real CDJ-2000NXS
talking to each other, in the hardware captures first and the emu-captures
(E01-E17) as support. To see what it does today, two captures were taken of a
TriMixxx deck (player 4, on 0.3) between two emulated NXSs on one link:

- **B1:** the NXSs joining the deck.
- **B2:** master and sync both ways.

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
- **B1 is the same on 0.3.** The NXS that joined the deck re-claimed every
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
requester claims 16-73 ms after asking. B2, on 0.3:

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
every peer every 5 s, answered or not (B1: 28 rounds to one NXS in 160 s;
Sam's 02).

**Now we ask as it does.** One thing had to move with it. The fork learnt
that a peer's medium was gone only from the zeroed answer a deck gives about
an empty slot, and that answer came because every slot was asked about. So
`PeerMedia` now forgets a slot's description once it is empty, and the
bridge reports that as a media event: the host reads an empty slot, as
before.

## What is checked, and what is not

**Checked, with `cargo test --workspace`** (777 passed):
- The two step-aside rules against every discovery packet in the corpus.
- Our claim burst against a stand-in answering with S13's own `0x05`.
- The status pacing, the hand-over clock, and the asker:
  - a query answered at its third try;
  - a swap;
  - an empty slot never asked about.
- The media events.

`tools/emudump` reads E10-E17 into the same transcripts, byte for byte.

**Not shown yet:**
- **On emulated CDJs, waiting for a Mixxx build:**
  - whether an NXS stops re-claiming once we say `0x01`, which S24b's and
    S18's pattern predicts;
  - that every device ends on its own number;
  - the hand-over timings.
- **On a real CDJ, which none of this has met:** each wire change needs Sam's
  check before it merges. `docs/prolink-learnings.md` §5 says what each must
  show.
