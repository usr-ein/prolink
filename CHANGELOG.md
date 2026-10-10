# Changelog

What changed between releases of the prolink crates, and above all what
changed **on the wire**. A real CDJ sees only the wire, so every change there
is listed under its own heading, with the capture that justifies it.

Releases before 0.4.0 are described by their tags (`v0.1.0` to `v0.3.0`) and
by [`work_log/`](work_log/README.md).

## Unreleased

### On the wire

- **We give up keep-alive byte `0x25` to a real deck.** It says whether a
  device was first on the network, and exactly one device on a link says so:
  a deck saying it beside another that says it too re-claims its number every
  few seconds for as long as it hears it (S10-S24: 18-249 claims a session
  beside this library's ancestors; S24b and S18). We used to say it for good
  after starting into an empty network, which kept a CDJ re-claiming for ever
  beside us. Now another device's keep-alive saying `0x02`, or a `0x05`
  answering our own stage-3 claim (S13 17.391, S26 5.606), makes us say `0x01`
  for good, and a `0x05` ends our stage-3 burst there. No new packet type: we
  never send `0x05`. `docs/PROTOCOL.md` §2.5.
- **Status goes out at once when what we publish changes**: the master claim,
  the successor at `0x9f`, sync, play state, the loaded track, a slot's state.
  Never sooner than 62 ms after the last packet (S28's floor, 61.7 ms), and
  otherwise every 200 ms as before. Same packets, same contents; only when
  they leave. A real deck sends status the same way, and a hand-over waits for
  it: one asked for master named its successor 172 ms after its `0x27` here
  before, against 3.3-5.2 ms on S28's decks. `docs/PROTOCOL.md` §3.2, §3.3b.

### Behind the API

- **prolink-cxx:** taking and handing over tempo master wake on the monitor's
  events instead of a 100 ms poll, so we claim in the packet after a holder's
  `0x9f` names us, and let go in the packet after our successor claims. The
  deadlines stay: the `0x26` once more after 300 ms without a grant, nothing
  more after 2 s, and a successor that never picks up is still given up on.
