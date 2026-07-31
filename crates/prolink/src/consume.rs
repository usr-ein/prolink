// SPDX-License-Identifier: GPL-3.0-only

//! Browsing and streaming from the CDJs on the network.
//!
//! Two independent paths to the same media, and which one is available depends
//! on whether we have announced ourselves:
//!
//! - **ONC RPC / NFSv2** works passively. A CDJ exports to the whole link-local
//!   subnet, so a host that has never announced is inside the permitted set by
//!   default (F11, F12). That is enough to pull `export.pdb` and to read a
//!   track's bytes.
//! - **dbserver** needs a device number of 6 or lower, which means announcing.
//!   It is the only way to get album art out of a player: a real CDJ never asks
//!   NFS for an image.

pub mod dbclient;
pub mod nfs;

pub use dbclient::DbClient;
pub use nfs::NfsClient;
