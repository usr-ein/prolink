# 012–013 — The runtime core: interfaces, discovery, the virtual CDJ

## Runtime

`tokio`, one task per socket. The alternative — a hand-rolled reactor, as the
Python proof of concept has — buys nothing here and costs the ecosystem's
timers, channels and cancellation.

Three tasks make up discovery and announcing:

| Task | Cadence | Why it is its own task |
|---|---|---|
| discovery receiver | per datagram | blocking on `recv_from` must not delay a timer |
| reaper | 1 s | staleness is time-driven, not packet-driven |
| keep-alive | 2.0 s | measured on hardware; must not drift behind a slow receive |
| status | 200 ms | unicast per peer, so the work scales with the peer count |
| query responder | per datagram | media and settings queries arrive on a different port |

## The socket, and why it is shared

Replies to a device-number claim — conflicts, and mixer assignments — are
unicast **to port 50000**, so the virtual CDJ has to transmit from the socket
discovery is listening on. Two sockets would mean never hearing the answer, and
the failure would look like "nobody ever objects to my claim".

Status is different: it goes out on an ephemeral socket while a second socket
bound to 50002 stays free to receive queries. A real deck sends each status
packet from a different, incrementing source port (6688, 6689, …); we do not
imitate that, and nothing has ever suggested a peer looks.

## Where types replaced comments

- **`Interface`** is proof that both an IPv4 address and a MAC are available.
  Enumeration drops anything that cannot announce, so no caller re-checks.
- **`Numbering`** makes the two announcing modes distinct decisions rather than
  a boolean flag. `Observer` takes a number outside 1–6 that cannot collide with
  hardware; `Claim` takes a `BrowsableDeviceNumber`. Running out of browsable
  numbers is an `Error` the caller must handle, not a silent degrade to an
  observer number — the degraded state accepts every announcement and is then
  never browsed, and a caller that thought it was serving would see nothing at
  all and have nothing to look at.
- **`MediaSource`** is the seam between announcing and serving. The virtual CDJ
  answers media and settings queries knowing nothing about rekordbox, and a slot
  the source cannot describe gets **no reply at all** — an empty one would tell
  the deck the medium exists and holds nothing, and it would then offer it.

## The device table

Keyed by **MAC**. A number is reassigned during the handshake and an address
changes when DHCP gives up and link-local takes over, so both of the obvious
keys are unstable in ways that show up in ordinary use. Tests pin both cases as
`Updated`, not as a second device.

Two-tier lifetime — offline, then forgotten — so a nudged cable does not tear
down whatever a caller has built on top of the table.

`numbers_seen` is never pruned. Silence is not evidence a number is free: an
XDJ-XZ and an Opus Quad do not defend their numbers with conflict packets at
all, so only "I have never seen anyone use it" is evidence, and that means
remembering numbers belonging to devices that have since gone quiet.

## Still to do here

- The claim chain is written and unit-tested against its own packets, but has
  never met a real conflict: no capture in the corpus contains a type-`0x08`
  packet, because nothing has ever contested a number on that rig. Our back-off
  is therefore untested against hardware, and that is worth saying out loud.
- `Phase` is exposed as a `watch` channel but nothing consumes it yet.
