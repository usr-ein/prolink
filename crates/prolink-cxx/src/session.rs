// SPDX-License-Identifier: GPL-3.0-only

//! The session behind the bridge, and the free functions C++ calls.
//!
//! All safe Rust. `cxx` owns the boundary, so there is no pointer handling
//! here and nothing in this crate outside the bridge macro's own expansion is
//! `unsafe` — which is the whole reason for choosing it over a C ABI.

use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use prolink::consume::NfsClient;
use prolink::{Discovery, Interface, Monitor, VirtualCdj, VirtualCdjConfig};
use tokio::runtime::Runtime;

use crate::convert::{self, plain};
use crate::ffi::{Config, Device, Event, EventKind, NetworkInterface, Player, Slot};

/// How many events a session queues before it discards the oldest.
///
/// A host draining on a UI timer sees a handful per tick; this is two seconds
/// of a busy four-deck network, so it bites only when the host has stopped
/// draining altogether. The *oldest* goes, keeping the queue current rather
/// than stale, and the count is reported so the host knows to re-read the
/// tables — see `Event::dropped`.
const EVENT_QUEUE: usize = 512;

/// A running session: sockets, timers, and the state they maintain.
///
/// Opaque to C++, which holds it as a `rust::Box<Session>` and drops it the
/// way it drops anything else. Dropping releases the device number.
pub struct Session {
    /// Dropped last, because the tasks below run on it.
    runtime: Runtime,
    monitor: Monitor,
    discovery: Discovery,
    /// Held for as long as the session announces: dropping it gives up the
    /// device number that makes peers unicast their status to us (F21).
    cdj: Option<VirtualCdj>,
    events: Arc<Mutex<Events>>,
    /// Hands out transfer ids, from 1, so zero always means "not a transfer".
    next_transfer: AtomicU32,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("device_number", &self.device_number())
            .field("interface", &self.monitor.interface().name)
            .finish_non_exhaustive()
    }
}

/// The drained event queue.
#[derive(Debug, Default)]
struct Events {
    queue: VecDeque<Event>,
    /// Discarded since the host last drained.
    dropped: u32,
}

impl Events {
    fn push(&mut self, event: Event) {
        if self.queue.len() >= EVENT_QUEUE {
            self.queue.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.queue.push_back(event);
    }

    /// Everything queued, with the discarded count carried on the first.
    fn drain(&mut self) -> Vec<Event> {
        let mut taken: Vec<Event> = self.queue.drain(..).collect();
        if let Some(first) = taken.first_mut() {
            first.dropped = std::mem::take(&mut self.dropped);
        }
        taken
    }
}

/// The interfaces that could carry Pro DJ Link traffic.
#[must_use]
pub fn interfaces() -> Vec<NetworkInterface> {
    Interface::list()
        .unwrap_or_default()
        .iter()
        .map(convert::interface)
        .collect()
}

/// A config that chooses an interface and announces.
#[must_use]
pub fn default_config() -> Config {
    Config {
        interface: String::new(),
        announce: true,
    }
}

/// Start a session.
///
/// # Errors
///
/// When no interface matches, or a socket cannot be bound — usually another
/// Pro DJ Link program already running. `cxx` turns this into a C++ exception
/// carrying the message.
pub fn open(config: &Config) -> Result<Box<Session>, Error> {
    let interface = if config.interface.is_empty() {
        Interface::best_guess().map_err(|error| Error(format!("no usable interface: {error}")))?
    } else {
        Interface::named(&config.interface)
            .map_err(|error| Error(format!("no interface {}: {error}", config.interface)))?
    };

    let runtime = Runtime::new().map_err(|error| Error(format!("no runtime: {error}")))?;
    let started = runtime.block_on(async {
        let discovery = Discovery::start(interface.clone()).await?;
        let cdj = if config.announce {
            Some(
                VirtualCdj::observe(
                    &discovery,
                    VirtualCdjConfig {
                        emit_status: false,
                        ..VirtualCdjConfig::default()
                    },
                )
                .await?,
            )
        } else {
            None
        };
        // With a virtual CDJ the monitor also reads UDP 50002, which is what
        // carries the loaded track and the tempo master (F21).
        let monitor = match cdj.as_ref() {
            Some(cdj) => Monitor::with_status(interface.clone(), cdj).await?,
            None => Monitor::start(interface.clone()).await?,
        };
        Ok::<_, prolink::Error>((discovery, cdj, monitor))
    });
    let (discovery, cdj, monitor) =
        started.map_err(|error| Error(format!("could not start: {error}")))?;

    let events = Arc::new(Mutex::new(Events::default()));
    let sink = Arc::clone(&events);
    let mut incoming = monitor.subscribe();
    // Ends when the session is dropped, because the sender goes with it.
    runtime.spawn(async move {
        while let Ok(event) = incoming.recv().await {
            if let Ok(mut queue) = sink.lock() {
                queue.push(convert::event(&event));
            }
        }
    });

    Ok(Box::new(Session {
        runtime,
        monitor,
        discovery,
        cdj,
        events,
        next_transfer: AtomicU32::new(1),
    }))
}

impl Session {
    /// The number we announced as, or zero if we did not announce.
    #[must_use]
    pub fn device_number(&self) -> u8 {
        self.cdj.as_ref().map_or(0, |cdj| cdj.number().get())
    }

    /// Everyone on the network, as of now.
    #[must_use]
    pub fn devices(&self) -> Vec<Device> {
        self.discovery
            .devices()
            .iter()
            .map(convert::device)
            .collect()
    }

    /// What every player is doing, as of now.
    #[must_use]
    pub fn players(&self) -> Vec<Player> {
        self.monitor.players().iter().map(convert::player).collect()
    }

    /// Everything that has happened since the last call.
    #[must_use]
    pub fn drain_events(&self) -> Vec<Event> {
        self.events
            .lock()
            .map(|mut events| events.drain())
            .unwrap_or_default()
    }

    /// Fetch one file from a player's medium.
    ///
    /// # Errors
    ///
    /// When no device has that number. Everything after that is asynchronous
    /// and reported as a `TransferDone` event rather than here, because the
    /// call returns as soon as the transfer is queued.
    pub fn fetch_file(
        &self,
        device_number: u8,
        slot: Slot,
        remote_path: &str,
        local_path: &str,
    ) -> Result<u32, Error> {
        let peer = self
            .address_of(device_number)
            .ok_or_else(|| Error(format!("no device {device_number} on the network")))?;

        let id = self.next_transfer.fetch_add(1, Ordering::Relaxed);
        let events = Arc::clone(&self.events);
        let interface = self.monitor.interface().clone();
        let slot = convert::slot_back(slot);
        let (remote, local) = (remote_path.to_owned(), local_path.to_owned());

        self.runtime.spawn(async move {
            let outcome = fetch(&interface, peer, slot, &remote, &local, id, &events).await;
            let mut done = plain(EventKind::TransferDone, 0, 0);
            done.transfer = id;
            if let Err(reason) = outcome {
                tracing::warn!(%peer, remote, "transfer failed: {reason}");
                done.ok = false;
                done.detail = reason;
            }
            if let Ok(mut queue) = events.lock() {
                queue.push(done);
            }
        });
        Ok(id)
    }

    /// Fetch a player's `export.pdb`.
    ///
    /// # Errors
    ///
    /// As [`Self::fetch_file`].
    pub fn fetch_database(
        &self,
        device_number: u8,
        slot: Slot,
        local_path: &str,
    ) -> Result<u32, Error> {
        self.fetch_file(
            device_number,
            slot,
            prolink::consume::nfs::EXPORT_PDB,
            local_path,
        )
    }

    /// The address of a device by number, if it is on the network.
    fn address_of(&self, number: u8) -> Option<Ipv4Addr> {
        self.discovery
            .devices()
            .into_iter()
            .find(|device| device.number.get() == number)
            .map(|device| device.ip)
    }
}

/// One transfer, start to finish, reporting progress as it goes.
async fn fetch(
    interface: &Interface,
    peer: Ipv4Addr,
    slot: prolink_proto::Slot,
    remote: &str,
    local: &str,
    id: u32,
    events: &Arc<Mutex<Events>>,
) -> Result<(), String> {
    let mut client = NfsClient::connect(peer, Some(interface))
        .await
        .map_err(|error| format!("connecting to {peer}: {error}"))?;
    let mounted = client
        .mount_slot(slot)
        .await
        .map_err(|error| format!("mounting {slot}: {error}"))?;
    let file = client
        .open(&mounted, remote)
        .await
        .map_err(|error| format!("opening {remote}: {error}"))?;

    let bytes = client
        .read_file_with(&file, |progress| {
            let mut event = plain(EventKind::TransferProgress, 0, 0);
            event.transfer = id;
            event.done = progress.read;
            event.total = progress.total;
            if let Ok(mut queue) = events.lock() {
                queue.push(event);
            }
        })
        .await
        .map_err(|error| format!("reading {remote}: {error}"))?;

    // Written once, never incrementally: a truncated `export.pdb` parses far
    // enough to look plausible and then yields a library missing its last few
    // hundred tracks.
    std::fs::write(local, &bytes).map_err(|error| format!("writing {local}: {error}"))?;
    let _ = client.unmount(&mounted).await;
    Ok(())
}

/// What a failed call tells C++.
///
/// `cxx` turns this into an exception carrying [`Display`](std::fmt::Display),
/// so the message is what a host shows the user.
#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
