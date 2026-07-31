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
//!
//! # Going away is a sequence, not a `drop`
//!
//! Stopping is not the reverse of starting. A deck that is reading our files
//! has a mount open and a filehandle in hand, and sockets that simply vanish
//! leave it retrying against nothing — the same dead end as a missing
//! portmapper, arrived at from the other side. Real hardware does not do that:
//! ejecting a stick walks the slot through two unmounting states first, and the
//! consuming deck releases its mount when it sees the second (see
//! [`ProLinkServer::shutdown`]). So we eject before we stop.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use prolink_proto::status::MediaState;
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
///
/// Each slot also carries the [`MediaState`] currently being published for it,
/// behind an atomic rather than a lock: the status timer reads it five times a
/// second from its own task, and an eject writes it from another.
#[derive(Debug, Default)]
pub struct MediaSet {
    media: Vec<Arc<Medium>>,
    /// One per entry of `media`, at the same index. A slot with no medium has
    /// no cell, and is empty by construction rather than by bookkeeping.
    states: Vec<AtomicU8>,
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
        set.states = set
            .media
            .iter()
            .map(|_| AtomicU8::new(MediaState::LOADED.0))
            .collect();
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

    /// What peers are currently being told about a slot.
    pub fn state(&self, slot: Slot) -> MediaState {
        self.cell(slot).map_or(MediaState::EMPTY, |cell| {
            MediaState(cell.load(Ordering::Relaxed))
        })
    }

    /// Publish a state for a slot, from the next status packet onwards.
    ///
    /// A slot holding nothing is ignored rather than invented: there is nowhere
    /// to record a state for a medium that does not exist, and such a slot is
    /// reported empty anyway.
    pub fn publish(&self, slot: Slot, state: MediaState) {
        if let Some(cell) = self.cell(slot) {
            cell.store(state.0, Ordering::Relaxed);
        }
    }

    /// Publish one state for every slot being served.
    pub fn publish_all(&self, state: MediaState) {
        for cell in &self.states {
            cell.store(state.0, Ordering::Relaxed);
        }
    }

    fn cell(&self, slot: Slot) -> Option<&AtomicU8> {
        let index = self
            .media
            .iter()
            .position(|medium| medium.slot().slot() == slot)?;
        self.states.get(index)
    }
}

impl MediaSource for MediaSet {
    fn occupied_slots(&self) -> std::collections::BTreeSet<Slot> {
        self.media
            .iter()
            .map(|medium| medium.slot().slot())
            .filter(|slot| self.state(*slot).has_media())
            .collect()
    }

    fn describe(&self, slot: Slot) -> Option<MediaDescription> {
        // A slot on its way out draws no reply, for the same reason an unserved
        // one does not: a description is what makes a deck offer a medium, and
        // a medium being ejected must not be offered afresh.
        if !self.state(slot).has_media() {
            return None;
        }
        Some(self.get(slot)?.description())
    }

    fn slot_state(&self, slot: Slot) -> MediaState {
        self.state(slot)
    }

    fn settings(&self, slot: Slot) -> Vec<u8> {
        self.get(slot)
            .map(|medium| medium.settings().to_vec())
            .unwrap_or_default()
    }
}

/// How long a slot stays [`MediaState::UNMOUNTING`] before the state a consumer
/// acts on.
///
/// Hardware, twice, to within 8 ms: a CDJ-2000NXS ejecting a USB held `0x02`
/// for 1.506 s (`captures/S15b-sd-and-usb`, frames 3160→3183) and for 1.514 s
/// (`captures/S4b-media-insert`, frames 641→668). Consistent enough to be a
/// fixed dwell rather than the time some piece of work happened to take.
const SPIN_DOWN: Duration = Duration::from_millis(1500);

/// How long to hold [`MediaState::UNMOUNTING_ALT`] waiting for consumers to
/// release their mounts.
///
/// This is the state a deck answers: in S15b it sent `UMNT` 9 ms after the SD
/// went to `0x03` (frames 3003→3004) and 16 ms after the USB did (frames
/// 3183→3184). So this is a ceiling for a deck busy with something else, not an
/// expected wait — the mount table going quiet is what normally ends it.
const UMNT_GRACE: Duration = Duration::from_secs(1);

/// How long to keep saying [`MediaState::EMPTY`] before the sockets close.
///
/// Status is unicast every 200 ms and nothing acknowledges it, so a deck that
/// misses a packet has two more before we are gone. Hardware moved on 64 ms
/// after the last `UMNT` in S4b and 200 ms after it in S15b, and went on
/// emitting either way.
const EMPTY_HOLD: Duration = Duration::from_millis(600);

/// How often to read the mount table while waiting for the `UMNT`s.
const UMNT_POLL: Duration = Duration::from_millis(25);

/// A virtual CDJ with media in its slots, browsable by real players.
///
/// [`ProLinkServer::shutdown`] is the clean way to stop: it ejects the media
/// first, so a deck reading from us is told to let go rather than left holding
/// a mount on a server that has vanished. Dropping it stops everything in the
/// same order without the ejection, which is what a failed start wants and not
/// what a DJ pressing ctrl-c does.
#[derive(Debug)]
pub struct ProLinkServer {
    // Declaration order is drop order, and this is the order a deck reaches
    // these in, reversed: the last thing it gets to is the first thing to go.
    dbserver: DbServer,
    nfs: NfsServer,
    cdj: VirtualCdj,
    discovery: Discovery,
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

        // Step 3: dbserver, which needs the number — and the virtual CDJ's
        // view of what each deck has loaded from us, so a track row can carry
        // the mark a browsing deck reads its reference key from (F55). The
        // dbserver never sees UDP 50002 itself; the virtual CDJ already holds
        // that socket to answer media queries.
        let dbserver = DbServer::start_watching(
            DbServerConfig {
                device,
                address: Ipv4Addr::UNSPECIFIED,
                ..DbServerConfig::default()
            },
            media.all().iter().map(Arc::clone),
            cdj.loaded(),
        )
        .await?;

        info!(
            device = %device,
            slots = media.all().len(),
            dbserver = dbserver.port(),
            "serving",
        );
        Ok(Self {
            dbserver,
            nfs,
            cdj,
            discovery,
            media,
            vfs,
        })
    }

    /// Eject the media the way a deck does, then stop.
    ///
    /// A consuming deck is not watching for our health; it holds an NFS mount
    /// and a filehandle and expects to be told when the medium beneath them
    /// goes. It is told through the slot state in our status packets, and the
    /// sequence is hardware's, `0x02` and `0x03` included:
    ///
    /// ```text
    /// loaded ──1.5 s──▶ unmounting ─────▶ unmounting_alt ─────▶ empty ──▶ sockets close
    ///  0x00               0x02                 0x03              0x04
    ///                                            └──▶ the deck sends UMNT, ~10 ms later
    /// ```
    ///
    /// The middle state is the load-bearing one. In both captured ejects the
    /// deck released its mount within 16 ms of `0x03` and did nothing at all on
    /// `0x02`, so going straight to empty would skip the only signal it acts
    /// on: the dwell is not politeness but the difference between a deck that
    /// unmounts and a deck left holding a stale handle.
    ///
    /// Media queries go unanswered from the first transition onwards, so a deck
    /// that asks mid-eject is not told about a medium that is leaving.
    ///
    /// **A server nobody has mounted skips the sequence**, publishes empty and
    /// stops — 0.6 s rather than the 2 to 3 s the full eject takes. There is no
    /// mount to release, so the dwell and the wait would both be waits for
    /// nothing.
    pub async fn shutdown(self) {
        let held = self.nfs.mounts();
        if held.is_empty() {
            info!("nothing has mounted us; stopping");
            self.media.publish_all(MediaState::EMPTY);
            tokio::time::sleep(EMPTY_HOLD).await;
        } else {
            let peers: std::collections::BTreeSet<_> =
                held.iter().map(|mount| mount.peer).collect();
            info!(
                mounts = held.len(),
                ?peers,
                "ejecting our media so the players reading it can let go",
            );

            self.media.publish_all(MediaState::UNMOUNTING);
            tokio::time::sleep(SPIN_DOWN).await;

            // The state a deck answers with UMNT.
            self.media.publish_all(MediaState::UNMOUNTING_ALT);
            let released = self.await_unmounts().await;

            self.media.publish_all(MediaState::EMPTY);
            if released {
                info!("every mount was released");
            } else {
                warn!(
                    still_held = self.nfs.mounts().len(),
                    "some mounts were not released in {} ms; going anyway",
                    UMNT_GRACE.as_millis(),
                );
            }
            tokio::time::sleep(EMPTY_HOLD).await;
        }

        // Explicit, and in the order a deck depends on them, because "the
        // fields happen to be declared that way" is not something a later edit
        // will preserve. Dbserver first: it is the only one holding a TCP
        // connection open, and a deck that loses it has by now been told why.
        drop(self.dbserver);
        drop(self.nfs);
        // Last, so that the eject above was actually emitted: status stops here.
        drop(self.cdj);
        drop(self.discovery);
        info!("stopped");
    }

    /// Wait for every peer to release its mount, up to [`UMNT_GRACE`].
    ///
    /// True if they all did. Polling rather than a notification because a mount
    /// is released by a datagram arriving on another task, and 25 ms of latency
    /// on a wait hardware ends in 16 ms costs nothing a DJ can perceive.
    async fn await_unmounts(&self) -> bool {
        let deadline = tokio::time::Instant::now() + UMNT_GRACE;
        while tokio::time::Instant::now() < deadline {
            if self.nfs.mounts().is_empty() {
                return true;
            }
            tokio::time::sleep(UMNT_POLL).await;
        }
        self.nfs.mounts().is_empty()
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
    /// Which players have taken a track off our media, and which one each
    /// holds.
    ///
    /// A serving host shows this. It comes from the peers' own status packets,
    /// which name the source player and slot, so a track loaded from another
    /// deck is not counted as ours (F55).
    pub fn consumers(&self) -> Vec<(u8, Slot, u32)> {
        self.cdj.loaded().consumers()
    }

    /// The media we are serving, one per slot.
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

    #[test]
    fn a_served_slot_starts_loaded() {
        let usb = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "MY STICK"));
        let set = MediaSet::new([usb]);
        assert_eq!(set.state(Slot::USB), MediaState::LOADED);
        assert_eq!(
            set.state(Slot::SD),
            MediaState::EMPTY,
            "a slot with no medium has no state to publish but empty"
        );
    }

    #[test]
    fn an_ejecting_slot_publishes_its_state_and_stops_describing_itself() {
        let usb = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "MY STICK"));
        let set = MediaSet::new([usb]);

        for state in [
            MediaState::UNMOUNTING,
            MediaState::UNMOUNTING_ALT,
            MediaState::EMPTY,
        ] {
            set.publish(Slot::USB, state);
            assert_eq!(
                set.slot_state(Slot::USB),
                state,
                "the status packet has to carry the state a consumer reacts to"
            );
            assert!(
                set.occupied_slots().is_empty(),
                "a medium on its way out is not a medium a deck should reach for"
            );
            assert!(
                set.describe(Slot::USB).is_none(),
                "and describing it is what would make the deck offer it again"
            );
        }
    }

    #[test]
    fn ejecting_publishes_every_slot_at_once() {
        // Ctrl-c ejects the whole unit, not one slot: a deck holding tracks
        // from both would otherwise release one mount and keep the other.
        let usb = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "USB"));
        let sd = Arc::new(Medium::synthetic(ServedSlot::SD, library(), "SD"));
        let set = MediaSet::new([usb, sd]);

        set.publish_all(MediaState::UNMOUNTING_ALT);
        assert_eq!(set.state(Slot::USB), MediaState::UNMOUNTING_ALT);
        assert_eq!(set.state(Slot::SD), MediaState::UNMOUNTING_ALT);
    }

    #[test]
    fn a_slot_we_do_not_serve_cannot_be_published_into() {
        let usb = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "MY STICK"));
        let set = MediaSet::new([usb]);
        set.publish(Slot::SD, MediaState::LOADED);
        assert_eq!(
            set.state(Slot::SD),
            MediaState::EMPTY,
            "or we would announce an SD card that does not exist"
        );
        assert!(set.describe(Slot::SD).is_none());
    }

    #[test]
    fn the_eject_dwells_are_the_ones_hardware_uses() {
        // S15b: 0x02 for 1.506 s, then UMNT 16 ms after 0x03. S4b: 1.514 s.
        // Shortening the first is the tempting change and the wrong one — the
        // deck acts on the state that follows it, and only once it has seen it.
        assert_eq!(SPIN_DOWN, Duration::from_millis(1500));
        assert!(
            UMNT_GRACE > Duration::from_millis(16),
            "the observed reply took 16 ms; a grace shorter than that is no grace"
        );
        assert!(
            EMPTY_HOLD >= Duration::from_millis(600),
            "three status packets, so a lost one does not cost the deck the news"
        );
    }
}
