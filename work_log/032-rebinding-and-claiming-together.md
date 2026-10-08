# 032 — Rebinding without losing 111, and two players claiming at once

Done. Both found by running several TriMixxx decks on one network inside a Mac
(pi-qemu's `deck up --link`): emulated Pis on a shared virtual switch, each
with its own MAC and link-local address. Nothing either fix changes is sent on
the wire to a peer that holds its number; see "What real hardware sees" below.

## A rebuilt session settled for device 7

The supervisor (`prolink-cxx` `session.rs`) tears the session down and builds
a new one whenever the interface it is bound to changes: the cable comes out
and the session falls back to the wireless (or, on an emulated deck, its
management NIC), the cable goes back in and it returns. Tearing down drops the
old `Live`; its NFS servers are aborted, and an aborted task lets go of its
socket only when the runtime next polls it. A call being answered holds the
socket until it is done. The new session binds UDP 111 straight away, so it
raced its own predecessor for the port, and when it lost it took the observer
path: "cannot bind UDP 111 ... watching as device 7 instead". Device 7 can be
neither browsed nor browse, and nothing ever retried, so the deck stayed out
of every LINK menu until Mixxx restarted.

Two emulated decks moving networks at once both lost the race. A deck whose
cable is out for longer than NetworkManager's 4 s carrier debounce plus the
supervisor's 2 s look takes the same path, out and back, and nothing stopped
it losing the same race; that one was only tried with the fix in place.

Starting a player now retries for up to 5 s while 111 is taken, 100 ms apart.
Nothing is sent while it waits: files are served before a number is claimed,
as before. A port held for good (an `rpcbind`) costs 5 s more to fall back.
On the decks the second try gets 111, 48276 and 2049.

The supervisor also rebuilds when the interface's **MAC** changes, not only
its name or address: the MAC goes verbatim into every keep-alive and claim, and
peers key their device tables on it.

## Two players claiming one number

Only a holder defends a number, and only once its own chain is done, so two
devices claiming the same one at the same moment hear no conflict: both finish
the chain and both announce it. Two emulated decks restarted together went for
the same number: their sessions started 50 ms apart, each watched an empty
network for the whole prescan, and both chose 4. Each would have begun
defending only after the other's last claim packet, so nothing but the rule
below could have moved either. That is the timing; the duplicate itself was not
seen, since the rule was already in place.

`claim_one` now also treats a rival's CLAIM_IP or CLAIM_NUMBER for our
candidate as a contest, and the lower address keeps the number: the higher
backs off to its next candidate, as it would from a NUMBER_CONFLICT. Both
apply the rule to the same two addresses, so exactly one moves. Our own claims
come back to us (we bind 0.0.0.0) and are ours by their address; a sender
with no address outranks nobody.

On the decks, restarted together:

```
a  starting virtual CDJ first_on_network=true peers=0      19:20:31.448
b  starting virtual CDJ first_on_network=true peers=0      19:20:31.399
b  number is taken; backing off number=4 holder=169.254.220.216
a  claimed a browsable device number number=4
b  claimed a browsable device number number=3
```

## What real hardware sees

Real decks do not arbitrate at all (F58, S26): each asks for a remembered
number and never moves. So the rule only ever makes **us** yield a number
another device is claiming at that moment, and sends nothing new:

- against a deck that holds a number, nothing changes: the prescan already
  excludes it, and a holder answers a claim with NUMBER_CONFLICT as before;
- against a deck claiming the same number as we do, below us, we move rather
  than both ending up with it; above us, both end up with it, as before.

`real_hardware_takes_a_number_from_a_claim_only_by_claiming_it` replays every
discovery packet in the corpus (9308 of them, 37 captures, 1805 of them real
claims for 1–4) through the rule, as if we were claiming each number from the
lowest and the highest link-local address: it fires only for a claim of that
very number from below, or a defence. The whole suite, corpus included, passes.
