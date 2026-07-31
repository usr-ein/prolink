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

    /// Fetch a track's artwork and write it to `local_path`.
    ///
    /// # Errors
    ///
    /// As [`Self::root_menu`], and when the file cannot be written.
    pub fn fetch_artwork(
        &mut self,
        device_number: u8,
        slot: Slot,
        artwork_id: u32,
        local_path: &str,
    ) -> Result<(), Error> {
        let slot = convert::slot_back(slot);
        let bytes = self.with_client(device_number, |runtime, client| {
            runtime.block_on(client.artwork(slot, artwork_id))
        })?;
        std::fs::write(local_path, &bytes)
            .map_err(|error| Error::new(format!("writing {local_path}: {error}")))
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
                Err(Error::new(format!(
                    "browsing device {device_number}: {error}"
                )))
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
