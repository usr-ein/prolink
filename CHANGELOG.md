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

### Behind the API
