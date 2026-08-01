# 030 — Becoming a real player, and never blocking the host

Five defects reported from a Raspberry Pi running Mixxx against two CDJ-2000nexus.
Four had the same shape underneath: **we behaved like a spectator when the host
needed us to be a participant, and we did our waiting on the caller's thread.**

## What was wrong

| Reported | Cause |
|---|---|
| "It auto assigned player 7" and we do not appear in the CDJ's LINK menu | The bridge announced with `Numbering::Observer(7)`. A deck offers LINK sources and answers browse requests only for players 1–4; at any other number it accepts the announcement in full and then silently never asks (F45) |
| Track downloads to 100% and then nothing happens | The destination's parent directories were never created, so every write ended `ENOENT` after the whole file had come over the wire |
| No cover art in the browse menu | Same `ENOENT`, plus: the fetch blocked, and a medium has some six hundred covers |
| The phase meter freezes while browsing | Blocking calls on the host's UI thread |

## Claiming a number

The claim chain was already written and already used by the serve side. The
consumer side simply never invoked it. `Numbering::Claim` now runs by default,
and the config carries a `preferred_number` so a host restarting a session
comes back as the player the decks already have in their tables.

If every browsable number is defended, the session settles for observing rather
than refusing to start: tempo, beats, play state and what each deck has loaded
all still work without a player number, and only being *browsed* does not. The
host is told which of the two it got, in those words, rather than being handed
a number and left to work out that it is useless.

## One socket for UDP 50002

Claiming a real number means emitting status — a player that never says what is
in its slots is not offered as a source (F24) — and emitting status means the
virtual CDJ binds UDP 50002. The monitor used to bind it too.

That does not work, and it does not fail loudly either. Only one member of a
`SO_REUSEPORT` group receives a given **unicast** datagram, and status is only
ever unicast (F21) — so two sockets would each get an arbitrary half of every
deck's status and both halves would look like a deck that keeps going quiet.

So the virtual CDJ now publishes a tap: a broadcast of every datagram its
status socket receives, which `Monitor::with_status` reads instead of binding.
The two status paths were folded into one function so they cannot drift.

## Not blocking the caller

Claiming takes about five seconds: 2.5 s of watching, then nine packets 300 ms
apart. That is five seconds during which a host calling `open()` from its UI
thread is frozen — and Mixxx calls it again on every "refresh".

`open()` therefore returns as soon as the interface is resolved, and everything
else happens on the session's own runtime. `device_number()` reads zero until a
number is held; `is_ready()` says whether startup finished; `last_error()`
carries a bind failure that no longer has an exception to travel on. Every
accessor answers from a `RwLock` and returns a default rather than waiting.

Artwork moved the same way. It returns a transfer id and reports completion as
a `TransferDone` event, over a dbserver connection of its own — separate from
the browse connections, because covers arrive while the user is scrolling a
menu and one connection would interleave a cover into the middle of it.

## Still open

The browse calls — `root_menu`, `track_rows`, `search`, `metadata` — are still
synchronous round trips. They are fast against a deck on a quiet network, but
they are the last thing on this side that can stall a caller's UI thread.
