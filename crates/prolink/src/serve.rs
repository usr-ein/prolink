// SPDX-License-Identifier: GPL-3.0-only

//! Serving a local medium to real players: a rekordbox one, or a stick of
//! plain files served by its folders.
//!
//! Seven things have to be true before a CDJ will browse us, and each of them
//! fails silently when it is not — see the crate documentation for the list and
//! the order. This module is steps 4 through 7: the read-only tree, the ONC RPC
//! servers over it, and the dbserver that makes it browsable.

pub mod dbserver;
pub mod folder;
pub mod medium;
pub mod nfs;
pub mod player;
pub mod preserve;
pub mod tags;
pub mod vfs;

pub use medium::{Analysis, Medium, ServedSlot};
pub use player::{MediaSet, VirtualPlayer, VirtualPlayerConfig};
pub use vfs::{Node, Vfs};
