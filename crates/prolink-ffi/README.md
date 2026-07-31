# prolink-ffi

A C ABI over [`prolink`](../prolink), for hosts that are not written in Rust.

Built for Mixxx, whose Pro DJ Link support is being replaced by calls into this
library, and shaped by that: the host is meant to be a thin shell that renders,
so everything here hands over state that is **ready to display** rather than
parts a caller has to reassemble. There is no packet, no socket and no parsing
in this API.

```c
#include "prolink.h"

ProlinkConfig config;
prolink_config_default(&config);

ProlinkSession* session = NULL;
if (prolink_open(&config, &session) != PROLINK_OK) {
    fprintf(stderr, "%s\n", prolink_last_error());
    return 1;
}

// From a UI timer, on your own thread:
ProlinkEvent event;
while (prolink_next_event(session, &event)) { /* ... */ }

ProlinkPlayer players[8];
int32_t count = prolink_players(session, players, 8);

prolink_close(session);
```

Link against `libprolink_ffi.a` or `libprolink_ffi.dylib`/`.so`, and include
[`include/prolink.h`](include/prolink.h).

## Three rules the whole surface follows

**Strings are fixed-size UTF-8 buffers inside the structs**, not pointers. A
device name is 20 bytes on the wire; a caller can hold a `ProlinkDevice` for as
long as it likes without a lifetime, a free function, or a rule about when the
pointer goes stale. **Nothing this API returns ever needs freeing.**

**Events are polled, never pushed.** A callback would be invoked from a tokio
worker thread, which for a Qt host means every handler has to be thread-safe
and re-entrant, and a slow one stalls the network. `prolink_next_event` drains
a queue on the caller's own thread; `ProlinkEvent::dropped` reports what was
missed rather than letting the queue grow without bound.

**Nothing unwinds across the boundary.** Every entry point catches panics and
returns `PROLINK_PANIC`, because a panic crossing into C++ is undefined
behaviour rather than a crash you can debug.

## The header is hand-written, and tested

`include/prolink.h` is written by hand so it can carry the same explanations
the Rust does. `src/layout.rs` pins the size, alignment and key field offsets
of every `#[repr(C)]` type, so a field added on one side and not the other
fails the test suite instead of corrupting a caller's stack on a machine the
author does not have. It caught a wrong size the first time it ran.

## Unsafe

This is the only crate in the workspace that does not inherit the workspace
lints, which **forbid** `unsafe_code`. A C ABI cannot be written without it, so
this crate *denies* it instead: `src/session.rs` allows it for the file, with a
`SAFETY` note at every block naming what the caller must guarantee, and no
other module — here or anywhere else in the workspace — can acquire any.
