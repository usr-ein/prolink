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
use prolink_proto::dbserver::{FILTER_ALL, ItemType, MenuItem, SortOrder};
use prolink_proto::{BrowsableDeviceNumber, Slot as LibSlot};

use crate::convert;
use crate::ffi::{MediaInfo, Metadata, PlainEntry, Row, Slot};

/// How deep a walk of a player's plain stick goes: the deck's own browser's
/// limit for its own sticks. A CDJ itself opens eight levels over LINK.
const PLAIN_DEPTH: usize = 16;

/// How many files a walk of a player's plain stick takes, as for the deck's
/// own sticks.
const PLAIN_FILES: usize = 20_000;
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
        let Some(media) = self.peer_media() else {
            // Without announcing, slot occupancy never reaches us: it is
            // published in status packets and nowhere else (F20, F21).
            return Vec::new();
        };
        media.all().iter().map(convert::media_info).collect()
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

    /// Ask a player for a track's preview waveform, without blocking.
    ///
    /// Over dbserver, sharing the connection artwork uses. Not over NFS: that
    /// runs one transfer at a time and a streaming audio fetch holds its turn
    /// for a whole track, so a preview queued behind one would arrive minutes
    /// after the row it belongs to -- and asking NFS for small files by the
    /// hundred churns the player's filehandle table until it answers
    /// `NFSERR_STALE` to everything, including the track a DJ is loading (F49).
    ///
    /// The reply is 900 bytes: `PWAV` unpacked into (height, shade) pairs with
    /// the tiny `PWV2` appended. It is stashed on the session and collected
    /// with `take_waveform_preview` once the `TransferDone` event is drained.
    ///
    /// # Errors
    ///
    /// When the player is not on the network, or no browsable device number is
    /// free. Everything after that is asynchronous.
    pub fn fetch_waveform_preview(
        &self,
        device_number: u8,
        slot: Slot,
        track_id: u32,
    ) -> Result<u32, Error> {
        let peer = self
            .address_of(device_number)
            .ok_or_else(|| Error::new(format!("no device {device_number} on the network")))?;
        let number = self.browsable_number().ok_or_else(|| {
            Error::new("a preview needs a device number in 1-4 and every one is taken".to_owned())
        })?;

        let id = self.next_transfer_id();
        let events = self.events_handle();
        let queue = self.artwork_queue();
        let stash = self.preview_stash();
        let slot = convert::slot_back(slot);

        self.runtime_handle().spawn(async move {
            let mut held = queue.lock().await;
            let client = match held.as_mut() {
                Some(client) => client,
                None => match DbClient::connect(peer, number).await {
                    Ok(opened) => held.insert(opened),
                    Err(error) => {
                        finish(&events, id, "", Err(format!("connecting: {error}")));
                        return;
                    }
                },
            };

            let outcome = match client
                .analysis(slot, track_id, prolink::consume::Analysis::WaveformPreview)
                .await
            {
                Ok(bytes) => {
                    if let Ok(mut held) = stash.lock() {
                        held.insert(id, bytes);
                    }
                    Ok(())
                }
                Err(error) => {
                    // The connection may be mid-message, and the protocol has
                    // no resynchronisation point; drop it so the next request
                    // reconnects rather than reading the wrong reply (F16).
                    *held = None;
                    Err(format!("fetching preview for track {track_id}: {error}"))
                }
            };
            finish(&events, id, "", outcome);
        });
        Ok(id)
    }

    /// Ask a player what one of its plain files is, without blocking:
    /// `GET_GENERIC_METADATA`, the ten items it reads off the file's tags
    /// (PLAIN-STICKS.md §5).
    ///
    /// A file off a stick with no rekordbox library is named nowhere else: a
    /// player playing one publishes its own id for it, which no database the
    /// host holds can resolve. Over the connection artwork uses, and collected
    /// with `take_metadata` once the `TransferDone` event is drained.
    ///
    /// # Errors
    ///
    /// When the player is not on the network, or no browsable device number is
    /// free. Everything after that is asynchronous.
    pub fn fetch_plain_metadata(
        &self,
        device_number: u8,
        slot: Slot,
        file_id: u32,
    ) -> Result<u32, Error> {
        let peer = self
            .address_of(device_number)
            .ok_or_else(|| Error::new(format!("no device {device_number} on the network")))?;
        let number = self.browsable_number().ok_or_else(|| {
            Error::new("metadata needs a device number in 1-4 and every one is taken".to_owned())
        })?;

        let id = self.next_transfer_id();
        let events = self.events_handle();
        let queue = self.artwork_queue();
        let stash = self.metadata_stash();
        let slot = convert::slot_back(slot);

        self.runtime_handle().spawn(async move {
            let mut held = queue.lock().await;
            let client = match held.as_mut() {
                Some(client) => client,
                None => match DbClient::connect(peer, number).await {
                    Ok(opened) => held.insert(opened),
                    Err(error) => {
                        finish(&events, id, "", Err(format!("connecting: {error}")));
                        return;
                    }
                },
            };

            let outcome = match client.generic_metadata(slot, file_id).await {
                Ok(found) => {
                    if let Ok(mut held) = stash.lock() {
                        held.insert(id, convert::metadata(&found));
                    }
                    Ok(())
                }
                Err(error) => {
                    // As for a preview: the connection may be mid-message.
                    *held = None;
                    Err(format!("fetching metadata for file {file_id}: {error}"))
                }
            };
            finish(&events, id, "", outcome);
        });
        Ok(id)
    }

    /// Walk a player's plain stick, without blocking: every folder and file
    /// its FOLDER menus list (PLAIN-STICKS.md §4.2), asked with track type 2
    /// as a player asks them.
    ///
    /// What a host builds a stick with no rekordbox library from: a CDJ will
    /// not list a directory over NFS (`READDIR` is `PROC_UNAVAIL`), so these
    /// menus are the only way to its files. Names only; the tags are asked
    /// file by file (`fetch_plain_metadata`). Collected with
    /// `take_plain_tree` once the `TransferDone` event is drained.
    ///
    /// # Errors
    ///
    /// When the player is not on the network, or no browsable device number is
    /// free. Everything after that is asynchronous.
    pub fn fetch_plain_tree(&self, device_number: u8, slot: Slot) -> Result<u32, Error> {
        let peer = self
            .address_of(device_number)
            .ok_or_else(|| Error::new(format!("no device {device_number} on the network")))?;
        let number = self.browsable_number().ok_or_else(|| {
            Error::new("browsing needs a device number in 1-4 and every one is taken".to_owned())
        })?;

        let id = self.next_transfer_id();
        let events = self.events_handle();
        let queue = self.artwork_queue();
        let stash = self.tree_stash();
        let slot = convert::slot_back(slot);

        self.runtime_handle().spawn(async move {
            let mut held = queue.lock().await;
            let client = match held.as_mut() {
                Some(client) => client,
                None => match DbClient::connect(peer, number).await {
                    Ok(opened) => held.insert(opened),
                    Err(error) => {
                        finish(&events, id, "", Err(format!("connecting: {error}")));
                        return;
                    }
                },
            };
            let outcome = match walk_plain(client, slot).await {
                Ok(entries) => {
                    tracing::info!(device_number, %slot, entries = entries.len(), "walked a plain stick");
                    if let Ok(mut held) = stash.lock() {
                        held.insert(id, entries);
                    }
                    Ok(())
                }
                Err(error) => {
                    *held = None;
                    Err(format!("walking a plain stick: {error}"))
                }
            };
            finish(&events, id, "", outcome);
        });
        Ok(id)
    }

    /// Fetch a plain file's cover into `local_path`, without blocking: its
    /// embedded picture or its folder's, by the file's own id, asked with
    /// track type 2 (PLAIN-STICKS.md §6). As `fetch_artwork` otherwise.
    ///
    /// # Errors
    ///
    /// As [`Self::fetch_artwork`].
    pub fn fetch_plain_artwork(
        &self,
        device_number: u8,
        slot: Slot,
        file_id: u32,
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
            let outcome = match client.plain_artwork(slot, file_id).await {
                // No picture is an empty answer, and a cover that is not
                // there: written as nothing would be a broken image.
                Ok(bytes) if bytes.is_empty() => Err(format!("file {file_id} has no artwork")),
                Ok(bytes) => write_artwork(&local, &bytes),
                Err(error) => {
                    *held = None;
                    Err(format!("fetching artwork of file {file_id}: {error}"))
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

/// Every folder and file of a player's plain stick, through its FOLDER menus.
///
/// Depth first, one `MENU_FOLDER` and its renders per folder. A top-level
/// entry's parent is 0, which no CDJ gives a folder: its ids are the FAT
/// volume's clusters, from 2.
async fn walk_plain(client: &mut DbClient, slot: LibSlot) -> prolink::Result<Vec<PlainEntry>> {
    let mut entries = Vec::new();
    let mut files = 0usize;
    let mut pending = vec![(FILTER_ALL, 0u32, 0usize)];
    while let Some((folder, parent, depth)) = pending.pop() {
        for row in client.folder(slot, folder).await? {
            match row.item_type.masked() {
                ItemType::FOLDER => {
                    if depth < PLAIN_DEPTH {
                        pending.push((row.id, row.id, depth + 1));
                    }
                    entries.push(PlainEntry {
                        id: row.id,
                        parent,
                        name: row.label1,
                        is_folder: true,
                    });
                }
                ItemType::TRACK_TITLE if files < PLAIN_FILES => {
                    files += 1;
                    entries.push(PlainEntry {
                        id: row.id,
                        parent,
                        name: row.label1,
                        is_folder: false,
                    });
                }
                _ => {}
            }
        }
    }
    Ok(entries)
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use prolink::consume::{DbClient, DbConfig};
    use prolink::serve::dbserver::{DbServer, DbServerConfig};
    use prolink::serve::{Medium, ServedSlot};

    use super::walk_plain;

    #[tokio::test]
    async fn a_walk_finds_every_folder_and_file_of_a_plain_stick() {
        let root = std::env::temp_dir().join(format!("prolink-cxx-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for path in [
            "a.mp3",
            "House/b.mp3",
            "House/Deep/c.wav",
            "Empty/readme.txt",
        ] {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().expect("a parent")).expect("a folder");
            std::fs::write(full, b"not audio").expect("a file");
        }
        let medium =
            Arc::new(Medium::from_folders(&root, ServedSlot::USB).expect("a plain medium"));
        let server = DbServer::start(
            DbServerConfig {
                device: prolink::BrowsableDeviceNumber::new(4).expect("4"),
                address: std::net::Ipv4Addr::LOCALHOST,
                port: 0,
                query_port: None,
            },
            [medium],
        )
        .await
        .expect("the dbserver starts");
        let mut client = DbClient::connect_at(
            std::net::Ipv4Addr::LOCALHOST,
            server.port(),
            prolink::BrowsableDeviceNumber::new(2).expect("2"),
            DbConfig::default(),
        )
        .await
        .expect("connecting");

        let entries = walk_plain(&mut client, prolink_proto::Slot::USB)
            .await
            .expect("the walk");
        let mut shape: Vec<(String, bool, bool)> = entries
            .iter()
            .map(|entry| (entry.name.clone(), entry.is_folder, entry.parent == 0))
            .collect();
        shape.sort();
        assert_eq!(
            shape,
            [
                ("Deep".to_owned(), true, false),
                ("House".to_owned(), true, true),
                ("a.mp3".to_owned(), false, true),
                ("b.mp3".to_owned(), false, false),
                ("c.wav".to_owned(), false, false),
            ]
        );
        let house = entries
            .iter()
            .find(|entry| entry.name == "House")
            .expect("House");
        let deep = entries
            .iter()
            .find(|entry| entry.name == "Deep")
            .expect("Deep");
        assert_eq!(deep.parent, house.id);
        let c = entries
            .iter()
            .find(|entry| entry.name == "c.wav")
            .expect("c.wav");
        assert_eq!(c.parent, deep.id);

        std::fs::remove_dir_all(&root).ok();
    }
}
