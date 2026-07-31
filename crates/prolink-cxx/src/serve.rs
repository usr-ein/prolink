// SPDX-License-Identifier: GPL-3.0-only

//! Serving local media to real players.
//!
//! Separate from a [`crate::Session`] on purpose: a server claims a device
//! number in 1–4 and emits status of its own, while a session announces
//! outside the player range and emits none. A host that did both from one
//! handle would be two devices pretending to be one.

use std::path::Path;
use std::sync::Arc;

use prolink::Interface;
use prolink::serve::{Medium, ProLinkServer, ServedSlot, ServerConfig};
use tokio::runtime::Runtime;

use crate::ffi::{ServeConfig, ServeStatus};
use crate::session::Error;

/// A running server: our media, offered to real players.
///
/// Opaque to C++, which holds it as a `rust::Box<Server>`.
pub struct Server {
    runtime: Runtime,
    /// `None` once stopped. Held in an `Option` because shutting down consumes
    /// the server, and C++ calls `stop` on a reference.
    inner: Option<ProLinkServer>,
    interface: String,
    /// Kept from the interface we started on: the server does not hand its own
    /// address back, and a host shows this beside the port numbers.
    address: String,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("running", &self.inner.is_some())
            .field("interface", &self.interface)
            .finish_non_exhaustive()
    }
}

/// Start serving.
///
/// # Errors
///
/// When no interface matches, when a medium cannot be read as a rekordbox
/// export, when every browsable device number is taken, or when a socket
/// cannot be bound — most often UDP/111 without root.
pub fn serve(config: &ServeConfig) -> Result<Box<Server>, Error> {
    if config.usb_path.is_empty() && config.sd_path.is_empty() {
        return Err(Error::new(
            "nothing to serve: give a USB or an SD path".to_owned(),
        ));
    }
    let interface = if config.interface.is_empty() {
        Interface::best_guess()
            .map_err(|error| Error::new(format!("no usable interface: {error}")))?
    } else {
        Interface::named(&config.interface)
            .map_err(|error| Error::new(format!("no interface {}: {error}", config.interface)))?
    };

    let mut media = Vec::new();
    for (path, slot) in [
        (&config.usb_path, ServedSlot::USB),
        (&config.sd_path, ServedSlot::SD),
    ] {
        if path.is_empty() {
            continue;
        }
        let medium = Medium::from_volume(Path::new(path), slot)
            .map_err(|error| Error::new(format!("reading {path}: {error}")))?;
        media.push(Arc::new(medium));
    }

    let runtime = Runtime::new().map_err(|error| Error::new(format!("no runtime: {error}")))?;
    let name = interface.name.clone();
    let address = interface.ip.to_string();
    let started = runtime.block_on(ProLinkServer::start(ServerConfig::new(interface), media));
    let inner = started.map_err(|error| Error::new(format!("could not serve: {error}")))?;

    Ok(Box::new(Server {
        runtime,
        inner: Some(inner),
        interface: name,
        address,
    }))
}

impl Server {
    /// What this server is doing.
    #[must_use]
    pub fn status(&self) -> ServeStatus {
        let Some(server) = self.inner.as_ref() else {
            return ServeStatus {
                active: false,
                device_number: 0,
                address: String::new(),
                interface: self.interface.clone(),
                portmap_port: 0,
                mount_port: 0,
                nfs_port: 0,
                dbserver_port: 0,
                is_discoverable: false,
            };
        };
        let ports = server.nfs_ports();
        ServeStatus {
            active: true,
            device_number: server.device_number().get(),
            address: self.address.clone(),
            interface: self.interface.clone(),
            portmap_port: ports.portmap,
            mount_port: ports.mount,
            nfs_port: ports.nfs,
            dbserver_port: server.dbserver_port(),
            is_discoverable: server.is_discoverable(),
        }
    }

    /// Stop serving, ejecting the media first.
    ///
    /// Idempotent, so a host may call it and then drop the handle.
    pub fn stop(&mut self) {
        let Some(server) = self.inner.take() else {
            return;
        };
        // Ejecting walks each slot through the unmounting states a consuming
        // deck waits for; sockets that simply vanish leave it retrying against
        // nothing (F20).
        self.runtime.block_on(server.shutdown());
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // A host that drops the handle without calling stop still gets the
        // eject, because the alternative is a deck left mounting a server that
        // is no longer there.
        self.stop();
    }
}
