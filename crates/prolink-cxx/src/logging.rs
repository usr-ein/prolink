// SPDX-License-Identifier: GPL-3.0-only

//! Letting a host see the library's own log.
//!
//! Everything this library knows about why a rig is not working, it says
//! through `tracing`: which player number was claimed and which were defended,
//! which stick was mounted and how many files were walked, and whether the
//! portmapper got UDP 111 — without which no deck ever lists us, however
//! correct the rest is (F46).
//!
//! None of it reaches anyone unless a subscriber is installed, and installing
//! one is a process-wide act that belongs to the host rather than to a library.
//! So this is opt-in, and idempotent: a host that calls it twice, or that has
//! its own subscriber already, gets no complaint and no second copy.

use std::sync::Once;

use tracing_subscriber::EnvFilter;

/// Installed at most once, whatever the host does.
static ONCE: Once = Once::new();

/// Send this library's log to standard error.
///
/// See the bridge declaration for what `filter` accepts.
pub fn init_logging(filter: &str) {
    ONCE.call_once(|| {
        // The environment wins, so a machine in a booth can be turned up
        // without a rebuild — which is the only way to change this on a deck
        // that runs an installed binary.
        let directive = std::env::var("PROLINK_LOG").unwrap_or_else(|_| {
            if filter.is_empty() {
                "prolink=info".to_owned()
            } else {
                filter.to_owned()
            }
        });
        let filter =
            EnvFilter::try_new(&directive).unwrap_or_else(|_| EnvFilter::new("prolink=info"));

        // `try_init`, not `init`: a host may have its own subscriber, and
        // failing to install a second one is the correct outcome rather than a
        // reason to take the process down.
        let installed = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .with_target(true)
            .try_init()
            .is_ok();
        if installed {
            tracing::info!(directive, "logging Pro DJ Link to standard error");
        }
    });
}
