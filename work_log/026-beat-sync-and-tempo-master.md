# 026 — Beat sync, tempo master and the handoff

Done. Priority 4 of the user's list. Decoded entirely from
`captures/S28-master-beat-sync-taglist`, which is the first session in this
corpus where the two decks were bridged through the Mac, so their **unicast**
traffic was recorded. The handoff packets do not appear in the thirty-two
sessions before it, and never could have: nothing about them is broadcast.

## What the wire says

Six fields, four in the status packet and two new packet types on 50001.

```
0x89  flags   0x40 playing  0x20 master  0x10 sync  0x08 on-air
0x8c  pitch   multiplier, 0x00100000 = 0%
0x92  tempo   ×100, 0xffff = no track
0x9e  master  1 = master, 2 = master with no usable grid, 0 = not
0x9f  yield   the device being handed master, 0xff = none
```

```
0x26  40B  unicast at the master     body: requesting device number
0x27  44B  unicast at the requester  body: answering device number, then 1
```

## Three things that were not obvious

**A synced follower's pitch field is not its fader.** When `0x10` lights the
deck slews `0x8c` to hold `bpm × pitch` equal to the master's. Watching the
master's fader sweep to −4.7%: master `145.00 × 0.95300 = 138.19`, follower
`143.00 × 0.96633 = 138.19`, agreeing to the last digit at every one of the 30
intermediate samples. Anything reporting a synced deck's "pitch" as a fader
position is reporting the wrong thing.

**Both decks claim master during a handoff.** Three status packets, 81 ms:
the old master sets `0x9f` to its successor while still reporting `0x9e = 1`;
the new master claims; the old master drops. The two that matter are 14 ms
apart. `settle_master` previously took `0x9e` alone, which meant that whether a
listener saw a spurious `TempoMaster(None)` in between depended on packet
ordering. It now reads `0x9f` and moves mastership atomically —
`a_master_handoff_moves_mastership_exactly_once` runs both orderings and would
have failed on the reversed one before.

**`0xffff` at `0x92` is not 655.35 BPM.** It is "no track", and it is 31,424 of
the 46,012 status packets in the corpus — two thirds. `bpm_centi()` now returns
`None` there. Nothing broke when this changed, which means nothing was reading
it, which means the CLI would have printed 655.35 for an idle deck the moment
anything did.

## Evidence

Cross-tabulated over all 46,012 status packets in the corpus rather than the one
session:

- `0x89 & 0x20` and `0x9e != 0` agree in **46,011**. The single disagreement is
  one frame inside a handoff. `0x9e` is used, since it is what the decks act on.
- `0x89` takes exactly **eight** values — `0x84`, `0x94`, `0xa4`, `0xb4`,
  `0xc4`, `0xd4`, `0xe4`, `0xf4` — so five of its eight bits never move and are
  left unnamed rather than guessed at.
- `0x9f` is `0xff` in 46,003 and names a device in the other 9, all in S28.
- `0x9e == 2` appears 309 times and only in `S11-format-matrix`, which is the
  session of unanalysed files — so it is "master without a beat grid".
- `0x40` is **not** a restatement of play state `3`: 289 of the 8,859 packets
  with that play state have the bit clear. Two separate observations.

## What is not implemented

Sending the handoff. This library can now read who is master, who is synced, at
what effective tempo, and watch mastership move — but it does not ask for
master itself, and `MasterResponse { granted: false }` is untested because a
refusal has never been observed. Becoming master would also mean generating a
beat grid to be master *of*, which is a different feature.

`0x2a` (sync control) is still unmodelled and has never been observed: SYNC is
toggled on a deck's own front panel, not over the network. So there is nothing
to send to make another deck follow us.
