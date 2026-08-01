// SPDX-License-Identifier: GPL-3.0-only

//! Browsing a player's library, and the connection cache behind it.
//!
//! # Why these block
//!
//! A browse is request/response and small: a menu is a few hundred rows and
//! arrives in one round trip. Making it asynchronous would mean a result queue
//! separate from the event queue — events are flat and cannot carry a list —
//! and a host that has to correlate replies with requests it made. Blocking
//! and returning the rows is what a host actually wants, and Mixxx already has
//! a network thread to call it from.
//!
//! A **file** transfer is the opposite: megabytes, seconds long, and worth a
//! progress bar. Those stay asynchronous, in [`crate::session`].
//!
//! # The connection is cached, and that is not an optimisation
//!
//! A dbserver connection is *stateful*: a menu request establishes a result
//! set and the render that follows pages through it (F27). Reconnecting per
//! call would also mean claiming a device number per call, and a number in 1–4
//! is contended with the decks (F45). So the first browse claims one and every
//! later one reuses it.

use std::collections::BTreeMap;

use prolink::consume::{DbClient, TrackMetadata};
use prolink_proto::dbserver::{MenuItem, SortOrder};
use prolink_proto::{BrowsableDeviceNumber, Slot as LibSlot};

use crate::convert;
use crate::ffi::{MediaInfo, Metadata, Row, Slot};
use crate::session::Events;
use crate::session::{Error, Session};

/// The dbserver connections a session holds open, one per player.
#[derive(Debug, Default)]
pub(crate) struct Connections {
    by_device: BTreeMap<u8, DbClient>,
}

impl Session {
    /// What every player's slots hold, as of now.
    #[must_use]
    pub fn media(&self) -> Vec<MediaInfo> {
        let Some(cdj) = self.cdj() else {
            // Without announcing, slot occupancy never reaches us: it is
            // published in status packets and nowhere else (F20, F21).
            return Vec::new();
        };
        cdj.peer_media()
            .all()
            .iter()
            .map(convert::media_info)
            .collect()
    }

    /// The root menu of a player's slot, as its LINK button shows it.
    ///
    /// # Errors
    ///
    /// When the player is not on the network, when no browsable device number
    /// is free, or when the request fails.
    pub fn root_menu(&mut self, device_number: u8, slot: Slot) -> Result<Vec<Row>, Error> {
        self.browse(device_number, slot, |client, slot| {
            Box::pin(async move { client.root_menu(slot).await })
        })
    }

    /// Every track on a player's slot, under the given sort.
    ///
    /// # Errors
    ///
    /// As [`Self::root_menu`].
    pub fn track_rows(
        &mut self,
        device_number: u8,
        slot: Slot,
        sort: u32,
    ) -> Result<Vec<Row>, Error> {
        self.browse(device_number, slot, move |client, slot| {
            Box::pin(async move { client.tracks(slot, SortOrder(sort)).await })
        })
    }

    /// Search a player's slot, the way its on-screen keyboard does.
    ///
    /// # Errors
    ///
    /// As [`Self::root_menu`].
    pub fn search(&mut self, device_number: u8, slot: Slot, term: &str) -> Result<Vec<Row>, Error> {
        let term = term.to_owned();
        self.browse(device_number, slot, move |client, slot| {
            Box::pin(async move { client.search(slot, &term, SortOrder::DEFAULT).await })
        })
    }

    /// Everything a player will say about one track.
    ///
    /// # Errors
    ///
    /// As [`Self::root_menu`].
    pub fn metadata(
        &mut self,
        device_number: u8,
        slot: Slot,
        track_id: u32,
    ) -> Result<Metadata, Error> {
        let slot = convert::slot_back(slot);
        let found: TrackMetadata = self.with_client(device_number, |runtime, client| {
            runtime.block_on(client.metadata(slot, track_id))
        })?;
        Ok(convert::metadata(&found))
    }

    /// Fetch a track's artwork into `local_path`, without blocking.
    ///
    /// # Errors
    ///
    /// When the player is not on the network, or no browsable device number is
    /// free. Everything after that is asynchronous and reported as a
    /// `TransferDone` event.
    pub fn fetch_artwork(
        &self,
        device_number: u8,
        slot: Slot,
        artwork_id: u32,
        local_path: &str,
    ) -> Result<u32, Error> {
        let peer = self
            .address_of(device_number)
            .ok_or_else(|| Error::new(format!("no device {device_number} on the network")))?;
        let number = self.browsable_number().ok_or_else(|| {
            Error::new("artwork needs a device number in 1-4 and every one is taken".to_owned())
        })?;

        let id = self.next_transfer_id();
        let events = self.events_handle();
        let queue = self.artwork_queue();
        let slot = convert::slot_back(slot);
        let local = local_path.to_owned();

        self.runtime_handle().spawn(async move {
            // One connection, one request at a time. A host asks for these by
            // the hundred, and a deck answers a dbserver connection in order
            // anyway — opening one per cover would claim a device number per
            // cover and get nowhere faster.
            let mut held = queue.lock().await;
            let client = match held.as_mut() {
                Some(client) => client,
                None => match DbClient::connect(peer, number).await {
                    Ok(opened) => held.insert(opened),
                    Err(error) => {
                        finish(&events, id, &local, Err(format!("connecting: {error}")));
                        return;
                    }
                },
            };

            let outcome = match client.artwork(slot, artwork_id).await {
                Ok(bytes) => write_artwork(&local, &bytes),
                Err(error) => {
                    // The connection may be mid-message; drop it so the next
                    // cover reconnects rather than reading the wrong reply.
                    *held = None;
                    Err(format!("fetching artwork {artwork_id}: {error}"))
                }
            };
            finish(&events, id, &local, outcome);
        });
        Ok(id)
    }

    /// Run one menu request and turn the rows into the shared shape.
    fn browse<F>(&mut self, device_number: u8, slot: Slot, request: F) -> Result<Vec<Row>, Error>
    where
        F: for<'a> FnOnce(
            &'a mut DbClient,
            LibSlot,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = prolink::Result<Vec<MenuItem>>> + Send + 'a>,
        >,
    {
        let slot = convert::slot_back(slot);
        let items = self.with_client(device_number, |runtime, client| {
            runtime.block_on(request(client, slot))
        })?;
        Ok(items.iter().map(convert::row).collect())
    }

    /// Borrow this player's connection, opening one if there is none.
    fn with_client<T>(
        &mut self,
        device_number: u8,
        body: impl FnOnce(&tokio::runtime::Runtime, &mut DbClient) -> prolink::Result<T>,
    ) -> Result<T, Error> {
        self.ensure_client(device_number)?;
        let self_error = self.error_sink();
        let (runtime, connections) = self.runtime_and_connections();
        let client = connections
            .by_device
            .get_mut(&device_number)
            .ok_or_else(|| Error::new(format!("no connection to device {device_number}")))?;

        match body(runtime, client) {
            Ok(value) => Ok(value),
            Err(error) => {
                // A failed request may have left the connection mid-message,
                // and the protocol has no resynchronisation point: the next
                // reply would be read as the answer to the failed request and
                // every one after it would be one behind (F16). Drop it and
                // let the next call reconnect.
                connections.by_device.remove(&device_number);
                let message = format!("browsing device {device_number}: {error}");
                self_error.note(&message);
                Err(Error::new(message))
            }
        }
    }

    /// Open a connection to this player if there is not one already.
    fn ensure_client(&mut self, device_number: u8) -> Result<(), Error> {
        if self
            .connections_ref()
            .by_device
            .contains_key(&device_number)
        {
            return Ok(());
        }
        let peer = self
            .address_of(device_number)
            .ok_or_else(|| Error::new(format!("no device {device_number} on the network")))?;
        // A dbserver request carries the *requester's* number and a deck
        // validates it, so browsing needs one in 1–4 — which is contended with
        // the decks (F45).
        let number = self.browsable_number().ok_or_else(|| {
            Error::new(
                "browsing needs a device number in 1-4 and every one is taken; \
                 open the session with a browsable number, or free one on the rig"
                    .to_owned(),
            )
        })?;

        let (runtime, connections) = self.runtime_and_connections();
        let client = runtime
            .block_on(DbClient::connect(peer, number))
            .map_err(|error| {
                Error::new(format!("connecting to device {device_number}: {error}"))
            })?;
        connections.by_device.insert(device_number, client);
        Ok(())
    }
}

impl Connections {
    /// Close every connection, politely.
    pub(crate) fn close_all(&mut self, runtime: &tokio::runtime::Runtime) {
        for (_, client) in std::mem::take(&mut self.by_device) {
            let _ = runtime.block_on(client.close());
        }
    }
}

/// The number a session browses with, if it has one.
pub(crate) fn browsable(number: u8) -> Option<BrowsableDeviceNumber> {
    BrowsableDeviceNumber::new(number)
}

/// Write a cover, making the directory it lives in.
///
/// The path mirrors the medium's own tree, `PIONEER/Artwork/000NN/aNNN.jpg`,
/// and none of those directories exist until something makes them.
fn write_artwork(local: &str, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(local).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    std::fs::write(local, bytes).map_err(|error| format!("writing {local}: {error}"))
}

/// Report a finished artwork fetch the way a file transfer is reported.
fn finish(
    events: &std::sync::Arc<std::sync::Mutex<Events>>,
    id: u32,
    local: &str,
    outcome: Result<(), String>,
) {
    let mut done = crate::convert::plain(crate::ffi::EventKind::TransferDone, 0, 0);
    done.transfer = id;
    local.clone_into(&mut done.path);
    if let Err(reason) = outcome {
        tracing::warn!(local, "artwork failed: {reason}");
        done.ok = false;
        done.detail = reason;
    }
    if let Ok(mut queue) = events.lock() {
        queue.push(done);
    }
}
