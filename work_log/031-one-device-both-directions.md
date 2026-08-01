# 031 — One device, both directions

Mixxx held a real player number and still did not appear in any deck's LINK
menu. The number is only half of it: a deck lists a player as a source when it
has **media in a slot**, and asks about nothing otherwise (F24). Mixxx served
nothing, because the library could not consume and serve from one identity.

## What was in the way

`ProLinkServer` owned its own `Discovery` and its own `VirtualCdj`, and so did a
consuming session. Running both meant two devices pretending to be one: two
claims on the wire, two keep-alive streams, and two sockets on UDP 50002 —
where only one member of a `SO_REUSEPORT` group receives a given unicast
datagram, so each would have got an arbitrary half of every deck's status.

Three other things assumed media never changed:

- `MediaSet` was a `Vec` fixed at construction.
- `Vfs` could `mount` but not unmount.
- The dbserver took a `BTreeMap` **snapshot** of the media at start, so it would
  have gone on serving a stick that had been pulled.

## What it is now

`ProLinkServer` is `VirtualPlayer`: *this machine's presence on the network* —
one claimed number, one keep-alive, one status stream, and whatever media it is
offering, which may be none. Serving nothing is now an ordinary state rather
than `Error::NothingToServe`, which is what lets a host start before a stick is
plugged in.

A caller composes the other direction on top:

```rust
let player = VirtualPlayer::start(VirtualPlayerConfig::new(interface.clone()), []).await?;
let monitor = Monitor::with_status(interface, player.cdj()).await?;   // shares the tap
player.mount(medium)?;
player.unmount(Slot::USB).await;   // walks the eject states a deck acts on
```

`MediaSet` is now the one registry all three readers share — the virtual CDJ
answering media queries, the dbserver resolving a request's slot byte, and a
host drawing a status page — so they cannot disagree about what is plugged in.

## Unmounting is not just forgetting

Two halves, and both are load-bearing:

- The **slot state** walks `0x02` → `0x03` → `0x04`, which is what a consuming
  deck acts on. It answered `UMNT` 9 and 16 ms after `0x03` in S15b and did
  nothing at all on `0x02` (see [028](028-stopping-cleanly.md)).
- The **VFS subtree** goes, so handles under it stop resolving and the deck gets
  `NFSERR_STALE` — which is what a real player answers for a pulled stick. The
  mount point is also removed from its parent's listing: a `READDIR` naming an
  entry that `LOOKUP` then cannot find reads as a corrupt medium rather than an
  absent one.

## Finding the sticks

`prolink::volumes` scans the mount points for `PIONEER/rekordbox/export.pdb`,
carried over from the C++ this replaced along with the part that is easy to get
wrong: on Linux the mount point is not the label. An automounter calls a stick
`/media/DJ_USB_1` while the deck shows `NHK_2024`, and a DJ reading one off each
screen cannot tell they are the same stick. So the label is resolved mount point
→ device via `/proc/self/mounts` → label via the `/dev/disk/by-label` symlinks,
with the mount point's own name as the fallback (which is correct on macOS).

The bridge turns this into a two-second scan that mounts what it finds, USB
first, and ejects what has gone.

## Starting before there is a network

A host is routinely started before its ethernet is plugged in. `open()` now
binds nothing itself; a supervisor task picks an interface, builds the session
on it, and **rebinds when a better one appears** — so plugging the cable in is
enough, and no host has to poll for the link and call refresh.

The same task is what makes the interface choice recoverable at all: it is made
when the session starts, and a Mixxx that started on wireless was listening to a
network the CDJs were not on, with nothing about it that would ever resolve.
