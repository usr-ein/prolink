// SPDX-License-Identifier: GPL-3.0-only

//! What can go wrong once sockets are involved.

use std::net::Ipv4Addr;

use prolink_proto::DeviceNumber;

/// An error from the Pro DJ Link runtime.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A socket operation failed.
    #[error("{context}: {source}")]
    Io {
        /// What was being attempted.
        context: &'static str,
        /// The underlying failure.
        #[source]
        source: std::io::Error,
    },

    /// The system's interface list could not be read.
    #[error("cannot enumerate network interfaces: {0}")]
    Interfaces(String),

    /// The named interface does not exist, or has no IPv4 address and MAC.
    #[error("no interface named {name} with both an IPv4 address and a MAC")]
    NoSuchInterface {
        /// The name that was asked for.
        name: String,
    },

    /// Nothing on this host could carry Pro DJ Link traffic.
    #[error("no usable network interface")]
    NoUsableInterface,

    /// A peer sent something this crate could not decode.
    #[error(transparent)]
    Protocol(#[from] prolink_proto::Error),

    /// A file on a rekordbox medium could not be read.
    #[error(transparent)]
    Medium(#[from] prolink_rekordbox::Error),

    /// Every device number a peer would browse is taken.
    ///
    /// Serving is impossible in this state. The right response is to degrade to
    /// an observer number with serving switched off, which is why this is an
    /// error the caller has to handle rather than a silent fallback (F45).
    #[error("device numbers 1-4 are all in use ({} taken); nothing browsable is free", .taken.len())]
    NoBrowsableNumber {
        /// The numbers observed in use.
        taken: Vec<u8>,
    },

    /// Another device defended the number we were claiming, and we ran out of
    /// candidates.
    #[error("device {number} is held by {holder}, and no candidate remains")]
    NumberConflict {
        /// The number we wanted.
        number: DeviceNumber,
        /// Who defended it.
        holder: Ipv4Addr,
    },

    /// A peer stopped answering.
    #[error("{what} timed out after {}ms", .after.as_millis())]
    Timeout {
        /// What we were waiting for.
        what: &'static str,
        /// How long we waited.
        after: std::time::Duration,
    },
}

impl Error {
    pub(crate) fn io(context: &'static str) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { context, source }
    }

    pub(crate) fn interfaces(error: impl std::fmt::Display) -> Self {
        Self::Interfaces(error.to_string())
    }
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
