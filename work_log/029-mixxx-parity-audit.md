# 029 — The Mixxx parity audit

Done. `prolink-cxx` was declared complete once and was not. This records what a
line-by-line read of `mixxx/src/network/prolink` turned up, because "it has the
same features" is a claim that has to be checked against the other side rather
than asserted from this one.

## What the read found missing

Nine gaps, five of them behavioural rather than cosmetic.

| # | Gap | Why it mattered |
|---|---|---|
| 1 | **No stale-handle retry** | A deck churns its filehandle table and then answers `NFSERR_STALE` to every lookup made against the old handles (F28). The C++ re-mounts once. The bridge did not, so a transfer failed permanently where the C++ recovered. `NfsClient::refresh` already existed and was simply not called. |
| 2 | **Transfers ran concurrently** | The C++ serialises them through one network thread. Two pulls from the same deck contend for the same filehandle table, which is what provokes the staleness in 1. Now queued through a semaphore. |
| 3 | **Requests keyed on device number only** | Mixxx deliberately keys on **MAC**, because a number can be reassigned and a request can outlive the device it named. Added `device_number_of(mac)` so a host can hold the stable identifier and resolve late. |
| 4 | **`ServeStatus` had no media or consumers** | The C++ publishes both: which slots we offer, and which players have taken a track off us. Visible in its UI. `ProLinkServer::consumers` now reads the registry that marks the loaded row in a browse listing (F55), the other way round. |
| 5 | **`TransferDone` had no path** | The C++ signal carries the local path. A host that keyed its own state on the path it asked for should not need an id table to get back to it. Both are reported now. |
| 6 | **No media-info event** | A deck describes a slot **once**, when it first browses it (F37), so the description arrives at a moment nothing else signals. Polling `media()` would eventually notice; the C++ emits `mediaInfoFound`. A 500 ms poller now turns a change into an event. |
| 7 | **No `is_listening` / `last_error`** | Both are on the C++ service and feed a status line. `last_error` is filled from the browse and transfer paths rather than only thrown, so a host does not have to catch every exception to populate it. |
| 8 | **No `refresh`** | Renamed in intent as well as added: the *device table* needs no refreshing, since it is rebuilt from keep-alives every two seconds. What goes stale is a dbserver **connection**, which is keyed on a device number that can move to a different deck. |
| 9 | **`Error::is_stale` did not exist** | Every caller would have had to match on the variant. It is the one NFS status with a defined remedy, and getting the test wrong means either an unrecoverable transfer or a loop against a genuinely missing file. |

## What was deliberately not copied

**The `[ProLink]` ControlObjects** — `master_device`, `master_bpm`,
`master_bar_phase`, `pull_db`. These are Mixxx's own control bus, read by the
phase-meter widget. The bridge supplies the values; publishing them as controls
is host glue and belongs in the thin C++ shell.

**`serveStatusChanged` as an event.** `Server::status()` is cheap and a host
already runs a timer to drain events; a second event queue on a second object
to report a struct that can just be read would be machinery for its own sake.
The one signal that *cannot* be reproduced by polling promptly is the media
description, because it is sent once — which is why that one got a poller and
this one did not.

**`ServeConsumer::playing`.** The C++ fills it; this reports `false` and says
so. A serving virtual CDJ holds UDP 50002 to answer media queries, and a
unicast datagram is delivered to one socket only — so the serving path cannot
also read peer status. Reporting a guess would be worse than reporting a
documented gap.

## Method note

The first "complete" claim was made from this side of the boundary: every item
the last message listed had been built, so the surface looked finished. What it
had not been checked against was the *other* side's header, and five of the
nine gaps were behaviours rather than functions — a retry, a queue, a key
choice — which no list of function names would have surfaced.
