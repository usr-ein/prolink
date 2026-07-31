// SPDX-License-Identifier: GPL-3.0-only

//! Everything a CDJ needs before it will browse us, started in the right order.
//!
//! The seven steps are in the crate documentation. Four of them can fail, and
//! the order they are attempted in is not arbitrary:
//!
//! 1. **The NFS servers go up first**, before anything contends for a device
//!    number. The portmapper wants UDP/111, which is privileged, and that is
//!    the one failure that makes everything else pointless: with nothing on 111
//!    a deck retries `GETPORT` once a second for ever, never falls back to the
//!    well-known ports, and never reaches dbserver (F46). Failing here before
//!    claiming a number means a failed start disturbs nothing.
//! 2. **Then the device number**, which must be in 1–4. Outside that range a
//!    deck accepts the announcement in full and then never asks (F45), so
//!    [`BrowsableDeviceNumber`] is what the dbserver takes and running out of
//!    them is an error rather than a silent degrade.
//! 3. **Then dbserver**, which needs the number for its `INTRODUCE` reply.
//!
//! Status emission begins with the virtual CDJ at step 2, a moment before
//! dbserver is listening. That gap does not matter in practice: in the one full
//! load we have timed, a deck sent its media query at t=7.6 s and did not open
//! dbserver until t=44 s.

use std::net::Ipv4Addr;
use std::sync::{Arc, RwLock};

use prolink_proto::{BrowsableDeviceNumber, DeviceKind, DeviceName, Slot};
use tracing::{info, warn};

use crate::discovery::Discovery;
use crate::interface::Interface;
use crate::media::{MediaDescription, MediaSource};
use crate::serve::dbserver::{DbServer, DbServerConfig};
use crate::serve::medium::Medium;
use crate::serve::nfs::{NfsConfig, NfsServer, Ports};
use crate::serve::vfs::Vfs;
use crate::virtual_cdj::{Numbering, VirtualCdj, VirtualCdjConfig};
use crate::{Error, Result};

/// How to present ourselves as a player with media.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The interface facing the CDJs.
    pub interface: Interface,
    /// The number to try first. Any other browsable number is tried if it is
    /// taken; if all four are, starting fails.
    pub preferred_number: Option<BrowsableDeviceNumber>,
    /// The name to announce. `CDJ-2000nexus` is what real hardware sends.
    pub name: DeviceName,
    /// What kind of device to claim to be.
    pub kind: DeviceKind,
    /// Keep-alive byte `0x35`. `0x64` is required to coexist with CDJ-3000s
    /// set to player 5 or 6.
    pub generation: u8,
    /// Where to put the portmapper. Anything but 111 makes us invisible to real
    /// hardware and is useful only for tests.
    pub portmap_port: u16,
}

impl ServerConfig {
    /// Sensible defaults for an interface.
    pub fn new(interface: Interface) -> Self {
        Self {
            interface,
            preferred_number: None,
            name: DeviceName::default(),
            kind: DeviceKind::CDJ,
            generation: 0x00,
            portmap_port: prolink_proto::rpc::portmap::PORT,
        }
    }
}

/// The media we are serving, one per slot.
///
/// This is the [`MediaSource`] the virtual CDJ answers media and settings
/// queries from, so what a peer is told about our slots and what the dbserver
/// will actually serve come from the same place and cannot disagree.
#[derive(Debug, Default)]
pub struct MediaSet {
    media: Vec<Arc<Medium>>,
}

impl MediaSet {
    /// Collect media, keeping the last one given for any slot.
    pub fn new(media: impl IntoIterator<Item = Arc<Medium>>) -> Self {
        let mut set = Self::default();
        for medium in media {
            set.media
                .retain(|existing| existing.slot() != medium.slot());
            set.media.push(medium);
        }
        set
    }

    /// The medium in a slot, if any.
    pub fn get(&self, slot: Slot) -> Option<&Arc<Medium>> {
        self.media
            .iter()
            .find(|medium| medium.slot().slot() == slot)
    }

    /// Every medium.
    pub fn all(&self) -> &[Arc<Medium>] {
        &self.media
    }
}

impl MediaSource for MediaSet {
    fn occupied_slots(&self) -> std::collections::BTreeSet<Slot> {
        self.media
            .iter()
            .map(|medium| medium.slot().slot())
            .collect()
    }

    fn describe(&self, slot: Slot) -> Option<MediaDescription> {
        Some(self.get(slot)?.description())
    }

    fn settings(&self, slot: Slot) -> Vec<u8> {
        self.get(slot)
            .map(|medium| medium.settings().to_vec())
            .unwrap_or_default()
    }
}

/// A virtual CDJ with media in its slots, browsable by real players.
///
/// Dropping it stops everything and releases the device number.
#[derive(Debug)]
pub struct ProLinkServer {
    discovery: Discovery,
    cdj: VirtualCdj,
    nfs: NfsServer,
    dbserver: DbServer,
    media: Arc<MediaSet>,
    vfs: Arc<RwLock<Vfs>>,
}

impl ProLinkServer {
    /// Start serving `media`, in the order described in the module docs.
    pub async fn start(
        config: ServerConfig,
        media: impl IntoIterator<Item = Arc<Medium>>,
    ) -> Result<Self> {
        let media = Arc::new(MediaSet::new(media));
        if media.all().is_empty() {
            return Err(Error::NothingToServe);
        }

        // Each medium gets its own subtree, because a filehandle is a hash of
        // its path and a CDJ keeps only the leading twelve bytes of one (F28).
        let mut tree = Vfs::new();
        for medium in media.all() {
            let Some(root) = medium.root() else { continue };
            let mounted = tree
                .mount(medium.slot().vfs_prefix(), root)
                .map_err(Error::io("walking a medium"))?;
            info!(
                slot = %medium.slot().slot(),
                export = medium.slot().export_path(),
                files = mounted,
                volume = medium.volume_name(),
                tracks = medium.library().tracks.len(),
                "mounted a medium",
            );
        }
        let vfs = Arc::new(RwLock::new(tree));

        // Step 1: the file servers, and the privileged port, before anything
        // contends for a device number.
        let nfs = NfsServer::start(
            Arc::clone(&vfs),
            NfsConfig {
                interface: Some(config.interface.clone()),
                portmap_port: config.portmap_port,
                ..NfsConfig::default()
            },
        )
        .await?;
        if !nfs.ports().is_discoverable() {
            warn!(
                portmap = nfs.ports().portmap,
                "the portmapper is not on 111; a real player will retry GETPORT for ever and \
                 never find us"
            );
        }

        // Step 2: announce, and hold a number a peer will actually browse.
        let discovery = Discovery::start(config.interface.clone()).await?;
        tokio::time::sleep(crate::discovery::SCAN_DURATION).await;
        let source: Arc<dyn MediaSource> = Arc::<MediaSet>::clone(&media);
        let cdj = VirtualCdj::start(
            &discovery,
            VirtualCdjConfig {
                name: config.name,
                kind: config.kind,
                numbering: Numbering::Claim {
                    preferred: config.preferred_number,
                },
                generation: config.generation,
                emit_status: true,
                ..VirtualCdjConfig::default()
            },
            source,
        )
        .await?;

        let device = cdj
            .browsable_number()
            .ok_or_else(|| Error::NoBrowsableNumber {
                taken: discovery.numbers_seen().into_iter().collect(),
            })?;

        // Step 3: dbserver, which needs the number.
        let dbserver = DbServer::start(
            DbServerConfig {
                device,
                address: Ipv4Addr::UNSPECIFIED,
                ..DbServerConfig::default()
            },
            media.all().iter().map(Arc::clone),
        )
        .await?;

        info!(
            device = %device,
            slots = media.all().len(),
            dbserver = dbserver.port(),
            "serving",
        );
        Ok(Self {
            discovery,
            cdj,
            nfs,
            dbserver,
            media,
            vfs,
        })
    }

    /// The number we hold. Always browsable, or we would not have started.
    pub fn device_number(&self) -> BrowsableDeviceNumber {
        self.cdj.browsable_number().unwrap_or_else(|| {
            // Unreachable: `start` refuses to return without one.
            debug_assert!(false, "a running server always holds a browsable number");
            BrowsableDeviceNumber::new(1).unwrap_or_else(unreachable_number)
        })
    }

    /// Where the three ONC RPC servers ended up.
    pub fn nfs_ports(&self) -> Ports {
        self.nfs.ports()
    }

    /// The dbserver port a peer is told to connect to.
    pub fn dbserver_port(&self) -> u16 {
        self.dbserver.port()
    }

    /// Whether a real player can find us at all.
    ///
    /// False means the portmapper is not on UDP/111, and no amount of correct
    /// behaviour afterwards will be reached.
    pub fn is_discoverable(&self) -> bool {
        self.nfs.ports().is_discoverable()
    }

    /// The media being served.
    pub fn media(&self) -> &MediaSet {
        &self.media
    }

    /// The peers we can see.
    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    /// The tree the NFS servers hand out, for inspection.
    pub fn vfs(&self) -> &Arc<RwLock<Vfs>> {
        &self.vfs
    }
}

/// Total fallback for a case `start` has already excluded.
fn unreachable_number() -> BrowsableDeviceNumber {
    const ONE: Option<BrowsableDeviceNumber> = BrowsableDeviceNumber::new(1);
    match ONE {
        Some(number) => number,
        None => ONE.unwrap_or_else(unreachable_number),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve::ServedSlot;
    use prolink_rekordbox::Library;

    fn library() -> Library {
        let raw = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../testdata/export.pdb"
        ))
        .expect("the committed export");
        Library::parse(&raw).expect("it parses")
    }

    #[test]
    fn a_media_set_reports_only_the_slots_it_has() {
        let usb = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "MY STICK"));
        let set = MediaSet::new([usb]);

        assert_eq!(set.occupied_slots(), [Slot::USB].into_iter().collect());
        assert!(
            set.describe(Slot::SD).is_none(),
            "an empty slot gets no reply at all"
        );

        let description = set.describe(Slot::USB).expect("the USB is described");
        assert_eq!(description.volume_name, "MY STICK");
        assert_eq!(
            description.track_count, 651,
            "the true count, or a deck will not offer it"
        );
    }

    #[test]
    fn a_second_medium_in_a_slot_replaces_the_first() {
        let first = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "FIRST"));
        let second = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "SECOND"));
        let set = MediaSet::new([first, second]);
        assert_eq!(set.all().len(), 1);
        assert_eq!(
            set.describe(Slot::USB).map(|d| d.volume_name).as_deref(),
            Some("SECOND")
        );
    }

    #[test]
    fn two_media_are_two_slots_and_two_subtrees() {
        let usb = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "USB"));
        let sd = Arc::new(Medium::synthetic(ServedSlot::SD, library(), "SD"));
        let set = MediaSet::new([usb, sd]);

        assert_eq!(
            set.occupied_slots(),
            [Slot::USB, Slot::SD].into_iter().collect()
        );
        assert_ne!(
            ServedSlot::USB.vfs_prefix(),
            ServedSlot::SD.vfs_prefix(),
            "or their filehandles would collide once a CDJ truncates them",
        );
    }
}
