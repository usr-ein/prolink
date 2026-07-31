<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Architecture

How the crates fit together, and why the seams are where they are.

## The layers

```
                       prolink-cli
                            │
                        prolink                 sockets, timers, state machines
                       ╱        ╲
        prolink-rekordbox      prolink-proto    files on a medium │ wire codecs
                                    │
                              prolink-capture   (dev/test: pcap replay)
```

`prolink-proto` has **no I/O, no clock and no async runtime**. Every format it
speaks is a function from bytes to values and back, which is what makes the
whole protocol surface testable from byte literals — and testable against 33
pcapng captures of real hardware without a network.

`prolink-rekordbox` reads the files on a medium and knows nothing about the
network. `prolink` is the only crate that opens a socket.

### The one seam that is not obvious

The analysis transforms — turning what rekordbox wrote into what dbserver puts
on the wire — live in `prolink-proto::analysis` and take **raw tag payload
bytes**, not a parsed analysis file. `prolink-proto` must not depend on
`prolink-rekordbox`, and the caller has parsed the file anyway. The side benefit
is that each transform is testable from the byte literal the evidence was
written down as.

## Consume: what a passive listener can and cannot learn

Keep-alives on UDP 50000 are **broadcast**, so listening alone gives every
device, its number, name, address and MAC. That is enough to reach a player's
NFS export and pull its `export.pdb`.

Status packets on UDP 50002 are **unicast to peers that have announced
themselves**. In one session all 1507 of them went deck-to-deck and not one
reached a host that had been on the network the whole time without announcing
(F21). Slot occupancy and tempo master are published there and nowhere else, so:

| Mode | Media state | dbserver | Risk |
|---|---|---|---|
| Passive | poll the NFS mount | unavailable | none |
| Announced | pushed, every ~200 ms | available if the number is ≤ 4 | contends for a number |

That is why `Discovery` and `VirtualCdj` are separate types rather than one
object with a flag: the second is the one that can disturb a live rig.

## Serve: the order is load-bearing

Learned by getting each step wrong in turn, and each failure is silent:

1. **Announce** on UDP 50000 and hold a number **in 1–4**. Outside that range a
   deck accepts the announcement in full and never browses it (F45). Encoded as
   `BrowsableDeviceNumber`, so it cannot be configured by accident.
2. **Emit status** on UDP 50002, unicast per peer at 200 ms, with slot state set.
3. **Answer media queries** with true counts. A deck told a medium holds nothing
   has no reason ever to ask again (F24).
4. **Serve NFS** — *before* dbserver. A portmapper on UDP/111 is mandatory: with
   nothing there a deck retries `GETPORT` once a second forever and never falls
   back to the well-known ports (F46). UDP/111 is privileged, so this is the
   step that decides whether the process needs elevation.
5. **Answer the port query** on TCP 12523.
6. **Serve dbserver**, and never answer an unknown request with an error (F25).
7. Optionally answer the settings query, for LOAD SETTINGS (F38).

The observed sequence from one real load:

```
media query ──► portmap GETPORT ──► MNT ──► 12523 ──► dbserver ──► READ
   (t=7.6s)         (t=44.09s)              (t=44.11s)             (t=52s)
```

## Two media at once

A player browsing two media on one peer opens **one** dbserver connection and
tells them apart purely by the slot byte in each request's descriptor (F37). So
serving two media is one server holding a medium per slot, resolved **per
message** — caching the medium per connection would serve the wrong library the
moment the DJ switches slots.

One VFS holds both media under separate subtrees, and that is what keeps their
filehandles distinct: a handle is a hash of its path, and a CDJ preserves only
the leading 12 bytes (F28), so two media sharing a root would mint
indistinguishable handles for the same relative path — the root most obviously.

## Concurrency

`tokio`, with one task per socket and one per dbserver connection.

- **Discovery** owns the UDP-50000 socket; the virtual CDJ *shares* it, because
  replies to a claim are unicast to port 50000 and a second socket would never
  hear them.
- **Status** goes out on an ephemeral socket while a second socket bound to
  50002 receives queries. A real deck sends each status packet from a different
  incrementing source port; we do not imitate that, but we do keep 50002 free to
  listen on.
- **dbserver** connections are long-lived and independent, so one task each,
  holding that connection's menu state.
- **NFS reads** must answer random access with low latency during playback: a
  stall is an audio dropout on someone's deck. Measured working on the reference
  implementation: 75 MB lossless files read across their whole length and
  scrubbed without delay (F39).

## What is deliberately unlike the hardware

One thing, and it is a considered choice. A CDJ sorts key names as text, so a
library in Camelot notation comes out `1A 1B 10A 10B 11A 11B 12A 12B 2A 2B` —
the wheel positions interleave and two harmonically adjacent keys land eleven
screens apart. We sort by `(position, letter)`. The sort happens entirely on the
server and the deck renders whatever order it is handed, so there is no
interoperability cost.

Everywhere else the goal is to be indistinguishable from a real deck.
