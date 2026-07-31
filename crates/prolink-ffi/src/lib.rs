// SPDX-License-Identifier: GPL-3.0-only

//! A C ABI over [`prolink`], for hosts that are not written in Rust.
//!
//! Built for Mixxx, whose Pro DJ Link support is being replaced by calls into
//! this library, and shaped by that: the C++ side is meant to be a thin shell
//! that renders, so **everything here hands over state that is ready to
//! display** rather than parts a caller has to reassemble. There is no packet,
//! no socket and no parsing in this API.
//!
//! # Three rules the whole surface follows
//!
//! **Strings are fixed-size UTF-8 buffers inside the structs.** Not pointers.
//! A device name is 20 bytes on the wire and an address is fifteen characters,
//! so the caller can hold a `ProlinkDevice` for as long as it likes without a
//! lifetime, a free function, or a rule about when the pointer goes stale.
//! Nothing this API returns ever needs freeing.
//!
//! **Events are polled, never pushed.** A callback would be invoked from a
//! tokio worker thread, which for a Qt host means every handler has to be
//! thread-safe and re-entrant, and a slow one stalls the network. Instead
//! [`prolink_next_event`] drains a queue on the caller's own thread, from a
//! timer or an idle handler, and back-pressure is the caller's to manage —
//! [`ProlinkEvent::dropped`] says how much was missed rather than letting the
//! queue grow without bound.
//!
//! **Nothing unwinds across the boundary.** Every entry point catches panics
//! and turns them into an error code, because a panic crossing into C++ is
//! undefined behaviour rather than a crash you can debug.
//!
//! # Threading
//!
//! A [`ProlinkSession`] owns its own tokio runtime and its sockets run on it.
//! Every function here may be called from any thread, but a single session
//! pointer must not be used from two threads at once — the natural shape for a
//! host is to own it on the thread that polls it.
//!
//! # Example
//!
//! ```c
//! ProlinkSession* session = NULL;
//! ProlinkConfig config;
//! prolink_config_default(&config);
//! if (prolink_open(&config, &session) != PROLINK_OK) {
//!     fprintf(stderr, "%s\n", prolink_last_error());
//!     return 1;
//! }
//!
//! // ...on a timer:
//! ProlinkEvent event;
//! while (prolink_next_event(session, &event)) {
//!     switch (event.kind) { /* ... */ }
//! }
//!
//! ProlinkPlayer players[8];
//! int32_t count = prolink_players(session, players, 8);
//!
//! prolink_close(session);
//! ```

mod convert;
mod layout;
mod session;
mod types;

pub use session::*;
pub use types::*;
