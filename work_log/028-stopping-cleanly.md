# 028 — Stopping cleanly: eject before the sockets close

Done. `ctrl-c` on `prolink serve` used to drop the server: three listeners and
an announcement vanishing between one status packet and the next. A deck reading
from us has an NFS mount and a filehandle, and it does not poll us for signs of
life, so what it gets from that is a mount into nothing — the same dead end as a
missing portmapper (F46), reached from the other side.

Hardware does not stop that way, and the captures say exactly how it does.

## What the captures show

`0x6f` and `0x73` take four values, not two: `0x00` loaded, `0x02` unmounting,
`0x03` a second unmounting state, `0x04` empty. F20 saw an eject pass through
them; what it did not need to ask was **which one the consumer reacts to**.

Two ejects, one with a deck actively mounted on the ejecting deck and one
without — `captures/S15b-sd-and-usb` and `captures/S4b-media-insert`, both on
the `pktap,en12,en9` tap that can see deck-to-deck unicast (F17):

```
S15b USB (deck B is reading)          S4b USB (nobody has mounted)
t=68.171  0x00 → 0x02                 t=50.055  0x00 → 0x02
t=69.677  0x02 → 0x03                 t=51.569  0x02 → 0x03    +1.514 s
t=69.693  UMNT '/C/' from deck B      t=51.633  0x03 → 0x04    +64 ms
t=69.877  0x03 → 0x04
```

and the SD eject in the same S15b session, 12 s earlier: `0x03` at frame 3003,
deck B's `UMNT '/B/'` at frame 3004, **9 ms later**.

Three readings:

- **The `0x02` dwell is 1.506 s and 1.514 s.** Two sessions, two media, 8 ms
  apart. That is a fixed delay, not the time some piece of work took.
- **`0x03` is the signal.** The `UMNT` follows it by 9 and 16 ms and never
  follows `0x02`. A server that goes `0x00` → `0x04` skips the only state its
  consumer acts on.
- **`0x04` is not the signal**, so nothing is waiting on it, which is why the
  deck with no consumer moved on 64 ms later while the one with a consumer took
  200 ms.

This ties C9 — "real players do call `UMNT`" — to the thing that causes it.
`UMNT` is not politeness at the end of a session; it is the answer to `0x03`.

## What was built

`MediaSource::slot_state` replaces the derived loaded-or-empty reading, so a
source can publish a state rather than a boolean. `MediaSet` keeps one atomic
per served slot; the status timer reads it five times a second from its own
task and the eject writes it from another.

`ProLinkServer::shutdown` runs the sequence: `0x02` for `SPIN_DOWN` (1.5 s, the
hardware figure), then `0x03` until every mount is released or `UMNT_GRACE`
(1 s) passes, then `0x04` held for `EMPTY_HOLD` (600 ms — three status packets,
so losing one does not cost the deck the news). Then the listeners close in the
order a deck reaches them, reversed: dbserver, NFS, and the virtual CDJ last,
because it is what was emitting the eject.

Media queries go unanswered from the first transition on. Describing a medium is
what makes a deck offer it (F24), and a medium being ejected must not be offered
afresh — so `describe` is gated on the slot still holding media, which also
keeps the media response and the status byte from disagreeing.

**A server nobody has mounted skips the sequence**: it publishes `0x04`, waits
`EMPTY_HOLD` and stops. There is no mount to release and the wait would be a
wait for nothing — a fifth of a second rather than three, which is what `ctrl-c`
feels like in the common case of a session where no deck ever linked.

The CLI says what it is doing and takes a second `ctrl-c` as "go now". Dropping
the server without `shutdown` still works and still stops everything; it is the
right behaviour for a failed start and the wrong one for a DJ pressing `ctrl-c`.

## What is not settled

What distinguishes `0x02` from `0x03` inside the deck. We know what each is
worth on the wire — one is a delay, the other is the trigger — which is enough
to imitate and not enough to name. The S15b SD eject also flapped `0x02` →
`0x00` → `0x02` before settling, which suggests the first state is revocable and
that the DJ did something; a deliberate capture of a cancelled eject would
answer it.
