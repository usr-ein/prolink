# 007 — the corpus test

`crates/prolink-proto/tests/corpus.rs`: every packet in every capture, replayed
through every codec. 22 tests, ~5 s over 240 MB.

Files added: `crates/prolink-proto/tests/corpus.rs` and
`testdata/corpus-fixtures.hex`. The only other change is a
`[dev-dependencies] prolink-capture` section in
`crates/prolink-proto/Cargo.toml` (and the resulting one-line `Cargo.lock`
entry). Nothing under `src/` was touched.

## What it asserts

**UDP 50000.** Every datagram addressed to the discovery port decodes with
`djl::Packet::decode` and **re-encodes byte for byte**, unknown bytes included.
Each also has to agree with itself: `stype` equals the datagram length (C2),
`PacketKind::wire_length` agrees where it has an opinion, `subtype`, byte `0x20`
and byte `0x22` hold their invariant values, nothing trails the fields the kind
declares, and where the body carries an IP address it is the address the IP
header says the datagram came from. That last one is a cross-check the packet
cannot make on its own.

**UDP 50002.** Every datagram addressed to the status port decodes and none of
them comes back as `Packet::Other` — every kind in the corpus is a kind this
crate models, and every packet is long enough for the layout its kind implies.
Then the fields are cross-checked: byte `0x1f` is the structural `0x01` (C14),
the declared body length is exactly `len - 0x24`, a `cdj_status` carries its
device number at both `0x21` and `0x24`, a `media_query`'s `requester_ip` is the
address it was sent from, and a `media_response` describes the device that sent
it.

The filter is `packet.destination.port()` and never "either endpoint". A
separate test censuses which destination ports carry the Pro DJ Link magic at
all and asserts the answer is exactly {50000, 50001, 50002} — which is what
makes a destination-port filter sufficient rather than merely conventional.

**TCP dbserver.** Every TCP flow in every capture is reassembled — not only the
ones on 1051 — and classified by content: a stream is dbserver if, after the
optional five-byte preamble, it begins with the message magic. Each is framed
end to end, and **the consumed bytes must account for the stream exactly**, with
every message re-encoding byte for byte. A stream with a hole is reported rather
than concatenated over, and a stream that is neither a port query nor dbserver
is reported too, so nothing is quietly skipped.

The TCP-12523 port query gets its own cross-check: the client direction must be
byte-equal to `dbserver::PORT_QUERY`, the server direction must be two bytes,
and **every port advertised there must be a port a dbserver conversation was
then observed on**. The two facts are arrived at independently — one from the
query, one from which direction of a connection was captured first — and they
agree exactly.

**ONC RPC.** Two readings, both asserted. The narrow one is the brief's: every
datagram addressed to 111, 2049 or 48276 that is a call must parse. The wide one
is every UDP datagram anywhere that parses as an RPC v2 call, which turns out to
be seven times as many (see the findings). For each: the program must be one the
crate models, the argument block must be the shape its `(program, procedure)`
implies, and re-encoding that shape must reproduce the wire bytes — same length,
same content, except in the trailing XDR pad. The credential must be `AUTH_UNIX`
with uid 0, gid 0, no machine name and no supplementary gids, and where
`stamp_for_xid` knows the xid the stamp must match.

Every `LOOKUP` name and every `MNT` path is checked as UTF-16 little-endian
counted in bytes: an even byte count, a round trip through `to_string_lossy`
back to the same bytes, a byte count that is exactly twice the UTF-16 unit
count, and — for ASCII-only names — the character byte first and the zero
second, which is the difference between little-endian and big.

## The numbers

33 captures, all read end to end, 4.7–5.2 s wall clock in a debug build. The
files are read in parallel, one thread per core taking the next unclaimed file;
serially it is about four times slower. Nothing else was needed to stay inside
the two-minute budget.

| | |
|---|---|
| captures read | 33 |
| UDP → 50000 | 7 519 |
| UDP → 50001 (counted here, decoded by `beat.rs`) | 1 110 |
| UDP → 50002 | 35 103 |
| TCP streams | 104 |
| dbserver streams framed | 58 |
| dbserver bytes framed | 18 726 298 |
| dbserver messages | 59 205 |
| TCP-12523 port queries | 23 |
| RPC calls to 111 / 2049 / 48276 | 8 406 |
| RPC calls, any port | 56 957 |
| RPC replies (counted, not decoded) | 56 947 |
| argument blocks with a stale XDR pad | 1 048 |
| distinct `LOOKUP` names | 1 204 (72 not ASCII, longest 96 bytes) |

Failures of any kind: **zero**. Every 50000 datagram round-trips byte for byte,
every 50002 datagram agrees with its own header, all 58 dbserver streams frame
with nothing left over, and all 56 957 RPC calls parse into the shape their
procedure implies.

UDP 50000 by kind: `keep_alive` 5 697, `claim_number` 1 758, `hello` 21,
`claim_mac` 21, `claim_ip` 21, `number_in_use` 1. UDP 50002 by kind:
`cdj_status` 35 016, `media_query` 54, `media_response` 29, `settings_query` 2,
`settings_response` 2. RPC by procedure: `nfs READ` 53 322, `nfs LOOKUP` 3 453,
`portmap GETPORT` 101, `nfs GETATTR` 38, `mount MNT` 31, `mount UMNT` 4,
`portmap NULL` 3, `portmap DUMP` 3, `mount EXPORT` 2.

Per capture (dbserver messages in the fourth column, RPC calls to a well-known
port and to any port in the last two):

```
capture                        50000   50002 streams  messages  rpc/wk     rpc
S01-cold-boot-a                   42       0       0         0       0       0
S02-deck-b-joins                 154       0       0         0       0       0
S04-media-insert                 844       0       0         0     864     864
S05-link-browse                  288    1440       2      2352       0       0
S06-load-and-play                360    2020       3       592     427     427
S10-serve-to-cdj                 201    1124       0         0       2       3
S10b-serve-to-cdj                 73     403       4        51       2       3
S10c-serve-to-cdj                 42     217       2        26       2       3
S10d-serve-to-cdj                 60     319       2       549       2       3
S10e-serve-to-cdj                105     563       2      2924       2       5
S10f-serve-to-cdj                 66     331       2       225       2      13
S10g-serve-to-cdj                 86     307       2       209       2      13
S10h-serve-to-cdj                215    1176       2     20907       2    2775
S10i-serve-to-cdj                281    1845       2       397       2    1242
S10j-serve-to-cdj                148    1033       2       246       2     225
S11-format-matrix                400    2355       2      2747       2   32686
S12-format-matrix                188    1013       2       348       2    8430
S13-format-ground-truth          458    2654       2      1341    6137    6137
S15a-sd-alone                    236    1276       2       880     603     603
S15b-sd-and-usb                  156     881       2       528     286     286
S16a-settings-over-link           88     447       2        71       3       3
S17-serve-formats                219    1219       2      1089       2    1530
S18-two-slots                    201    1173       2      1160       4     187
S19-drilldowns                   134     683       2      1692       4       6
S1b-cold-boot-b-alone             29       0       0         0       0       0
S20-browse-ground-truth          896    4559       2      5829       0       0
S21-drilldowns-v2                316    1676       2      6303       4       6
S22-sorting                      507    2869       2      5398       4    1185
S23-search-and-keys              208    1236       2      3085       4       6
S24b-e9-control                   94     471       2       248       2     278
S24c-e9-noportmap                 66     306       0         0      31      31
S2c-deck-a-joins                  58       0       0         0       0       0
S4b-media-insert                 300    1507       5         8       7       7
```

`S24c-e9-noportmap` having **zero** dbserver streams while `S24b-e9-control` has
two is F46 visible in the totals: with nothing on UDP/111 the deck never opens
the dbserver connection at all.

`cargo test -p prolink-proto -- --nocapture` prints all of this, including the
full histograms and the twenty most-requested `LOOKUP` names.

## How these compare with the brief's expectations

The brief quoted the reference project's figures: 7833 packets on UDP 50000,
38371 on 50002, 11809 dbserver messages, 8415 RPC calls. Those include JSONL
journals this repository does not have and two dysentery `LinkInfo` captures
that are not part of the corpus. Against `captures/S*/run.pcap` alone the
comparable figures are 7 519, 35 103, 59 205 and 8 406 — the two UDP counts a
few percent lower for the missing journals, RPC nine calls short of 8415 for the
same reason, and dbserver messages **five times higher**, because the reference
reassembled only port 1051 and this reassembles every flow. See below.

## Findings

**1. A CDJ leaves stale bytes in the XDR pad.** 1 048 of the 56 957 argument
blocks do not re-encode byte for byte, and in every one of them the difference
is confined to the one to three bytes XDR adds to round a variable-length field
up to a multiple of four. RFC 4506 asks for zeros there; a CDJ sends whatever
was in the buffer, and the leftovers are visibly fragments of the previous
name — a `LOOKUP` of `6 SENSE` (14 bytes) is followed by the pad `87 58`, and a
`UMNT /C/` by `3c d2`. Both endpoints in the corpus that do this are real
CDJ-2000NXS. Consequences: our encoder writing zeros is correct and must not be
changed to imitate it; a server that hashed or compared argument blocks byte for
byte would see the same request as two different ones; and a corpus test can
only assert equality *outside* the pad, which is what this one does. The
distinction is not a loophole — a length prefix read in the wrong units or a
field read in the wrong order moves bytes that are not the pad, and the test
still fails on those. `rpc/nfs_lookup_stale_pad` pins the case.

**2. Almost all NFS traffic in the corpus is not on port 2049.** Filtering to
{111, 2049, 48276} finds 8 406 calls; the corpus contains 56 957. The other
48 551 are addressed to whatever ephemeral port the serving side's NFS daemon
bound and advertised through its own portmapper — 50978, 63525, 61886, 65392,
57707, 62662 and a dozen more. The narrow filter is not wrong so much as
incomplete, and the shape of its incompleteness is instructive: it finds all the
`GETPORT`s and all the calls a *deck* serves, and misses most of the calls a
deck *makes to us*. `prolink-proto`'s own `rpc` module documentation cites
"56,966 of 56,966 calls" for the credential claim, which is this wider number,
so the module was already written against all of it. The test asserts both
counts and asserts that they differ, because if they were ever equal the
portmapper would have stopped being used.

**3. Twelve dbserver streams open mid-connection, and they are the large ones.**
`tcpdump` was started after the connection in twelve cases, so those streams
carry no preamble. Detecting dbserver by content rather than by the preamble
alone brings in 1.2 MB and tens of thousands of messages that a preamble-only
filter drops — including the 306 KB and 681 KB browse streams. All twelve happen
to begin exactly on a message boundary, because a player writes one message per
segment. They frame end to end like the rest.

**4. `crates/prolink-capture/src/tcp.rs`'s claim about dbserver ports is the
wrong way round.** Its module documentation says the corpus "carries dbserver
conversations on 1054, 1056, 1058 … 1078, because a CDJ publishes the port it is
listening on through the TCP-12523 query and the answer is whatever it happened
to bind". Those numbers are the **client** ports the decks chose, not server
ports: every flow of the form `169.254.202.84:1054 → 169.254.103.172:1051` has
deck A serving on 1051 and deck B connecting from 1054. The server ports actually
observed are 1051 (11 conversations) and eighteen ephemeral high ports belonging
to the Mac's software server — 49392, 55518, 55633, 55750, 55887, 55989, 56070,
57003, 57302, 57732, 58572, 58915, 60038, 60426, 61016, 61787, 62354, 62842 —
and that set is *exactly* the set of ports advertised on TCP 12523, which is the
independent corroboration. The conclusion the paragraph draws is still right —
reassembling only 1051 finds a fraction of the traffic — but the evidence given
for it is misread. Not my file to change; flagged for whoever owns it. The same
sentence appears in `work_log/006-capture.md`.

**5. Non-ASCII names are common and all of them survive.** 72 of the 1 204
distinct `LOOKUP` names contain a character outside ASCII: Japanese
(`02. Akiba - カガミ.mp3`), Scandinavian (`Nørbak`, `3ISBÄR`), typographic
punctuation (`Brat and it’s completely different…`), and dingbats
(`'❂RAINDAAMAGE'✯how do you like your tea_`). Every one round-trips through
`Utf16LeString` to the bytes it arrived as and has a byte count exactly twice its
UTF-16 unit count. That matters because the bug this project replaces (O6) only
showed on non-ASCII input, so the test asserts that at least one non-ASCII name
was seen — an ASCII-only sample would prove nothing about the case that has
actually broken before.

No name exceeds 96 bytes, i.e. 48 UTF-16 units; the longest are all truncated
rekordbox exports. `xdr::MAX_STRING` is 1024, so there is ample headroom, but
48 characters is what the hardware has been observed to ask for.

**6. Nothing failed to decode.** Every datagram addressed to 50000 carries the
magic; every datagram addressed to 50002 does too and every one is a kind this
crate models. Every capture reads to the end — none was cut short. Every one of
the 104 TCP streams is contiguous, so the hole-reporting path is exercised by
`prolink-capture`'s unit tests and not by the corpus. Every RPC call names one of
the three programs a CDJ runs, at versions portmap 2, mount 1 and NFS 2, with no
exceptions.

## The fixture floor

`testdata/corpus-fixtures.hex` — 24 records, 1 979 bytes of real packets, in a
`label = hex` format with continuation lines and a provenance comment naming the
capture and frame each came from. It is `include_str!`d, so a missing fixture
file is a compile error rather than a skipped test. Ten of the twenty-two tests
run off it and they use the **same** checking functions as the corpus scan, so
the floor is a floor and not a second, weaker opinion.

- **`djl/`** — `hello`, `claim_mac`, `claim_ip`, `claim_number`,
  `number_in_use`, `keep_alive`. Six of the nine kinds `PacketKind` names.
- **`status/`** — `cdj_status`, `media_query`, `media_response`,
  `settings_query`, `settings_response`. All five kinds this crate models.
- **`dbserver/`** — both directions of the smallest complete conversation in the
  corpus, 212 bytes between them, preamble included: a deck introducing itself,
  asking `0x3e03` and disconnecting. Plus a 42-byte `waveform_preview` reply
  whose declared blob argument is **absent from the wire**, which is the rule
  that silently desynchronises a naive reader.
- **`rpc/`** — one call per procedure the corpus contains: `portmap NULL`,
  `GETPORT`, `DUMP`; `mount MNT`, `UMNT`, `EXPORT`; `nfs GETATTR`, `LOOKUP`,
  `READ`. Plus a second `LOOKUP` whose XDR pad is stale, pinning finding 1.

One of the ten runs the checks *backwards*. Every assertion in this file is of
the form "collect the problems, assert the list is empty", which passes just as
happily when the checker is broken as when the packets are good, so
`the_checks_notice_a_packet_that_has_been_tampered_with` breaks a committed
packet of each kind in the one way its check exists to catch — a `stype` that
disagrees with the length, a `claim_ip` whose body disagrees with the IP header,
a status packet missing its structural `0x01`, a dbserver message that *sends*
the argument the wire omits, a stream cut short mid-message, and a `LOOKUP`
whose length prefix counts characters instead of bytes — and requires each one
to be reported.

## Gaps, deliberate

- **Three discovery kinds have no fixture**, for two different reasons.
  `mixer_assign_intent` (0x01) and `mixer_assign` (0x03) need a mixer and this
  rig is two CDJs; dysentery's `LinkInfo.pcapng` has both, but it is
  EPL-licensed and outside the corpus this brief defines, so copying bytes out
  of it is a call I did not make unilaterally. `number_conflict` (0x08) exists
  in no capture anywhere. `no_capture_has_ever_contested_a_device_number`
  asserts that last absence, so the day a capture does contain one the test says
  so and the announcer's back-off can be tested against hardware for the first
  time.
- **RPC replies are counted, not decoded.** 56 947 of them. Decoding needs the
  procedure, which needs pairing each reply to its call by xid within a capture;
  it is a real gap and the obvious next increment, and it is where `Fattr`,
  `Status` and the `READ` payload path would get their corpus coverage.
- **UDP 50001** is counted here and decoded by `beat.rs`'s own corpus tests,
  which landed while this was being written. Doing it in both places would be
  two things to change rather than one.
- **The hole-reporting path is unexercised by the corpus**, because all 104
  streams are contiguous. The test would report a gapped stream as a failure if
  one appeared, which is the right behaviour and is currently untested against
  data.
