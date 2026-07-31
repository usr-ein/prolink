// SPDX-License-Identifier: GPL-3.0-only

//! Announcing ourselves as a player: the claim chain, the keep-alive and the
//! status stream.
//!
//! This is the **only** part of the library that transmits on a Pro DJ Link
//! port, which is what makes everything else passive by construction.
//!
//! # Two modes, and the difference is not cosmetic
//!
//! [`Numbering::Observer`] emits keep-alives at a number outside the player
//! range and never contends for anything. It is enough to be *seen*, and enough
//! for peers to start unicasting status to us — which is what makes their slot
//! contents and the tempo master visible at all. It is **not** enough to be
//! browsed, and it cannot issue dbserver queries.
//!
//! [`Numbering::Claim`] runs the full handshake and takes a real player slot in
//! 1–4. Necessary to be browsable, and the only mode that can disturb a live
//! rig, so it is opt-in, it refuses a number it has seen anyone else use, and it
//! backs off on conflict.
//!
//! The range is a requirement rather than a preference. At device 5 a deck
//! accepts the announcement completely — it puts us in its device table and
//! unicasts hundreds of status packets to us — and then never sends a single
//! media query, so it never offers us as a source. The check precedes the whole
//! browse path and the failure is silent (F45). [`BrowsableDeviceNumber`] exists
//! so that this cannot be configured by accident.
//!
//! # The handshake
//!
//! ```text
//! 3× hello → 3× claim_mac → 3× claim_ip → N× claim_number → keep_alive forever
//! ```
//!
//! All broadcast, ~300 ms apart. **N is 3 when the device boots into an empty
//! network and 1 when it joins one that already has peers** (C13) — it is not
//! governed by the auto/manual assignment setting, which three controlled boots
//! with the setting held constant settle.
//!
//! Before any of that, we watch for [`PRESCAN`]. Silence is not evidence a
//! number is free: an XDJ-XZ and an Opus Quad do not defend their numbers with
//! conflict packets at all, so only having watched the network is.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use prolink_proto::djl::{self, Body};
use prolink_proto::status::{self, CdjStatus, MediaState};
use prolink_proto::{
    BrowsableDeviceNumber, DISCOVERY_PORT, DeviceKind, DeviceName, DeviceNumber, STATUS_PORT, Slot,
};
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::Result;
use crate::discovery::Discovery;
use crate::media::{MediaSource, NoMedia};
use crate::socket::{self, MAX_DATAGRAM};

/// How long to watch the network before claiming a number.
pub const PRESCAN: Duration = Duration::from_millis(2500);

/// The number an observer takes: outside the 1–6 player range, so it can never
/// collide with hardware — and, being above 4, one a peer will never browse.
pub const OBSERVER_NUMBER: DeviceNumber = match DeviceNumber::new(7) {
    Some(number) => number,
    // Unreachable, and settled at compile time: 7 is not zero.
    None => DeviceNumber::ONE,
};

/// Keep-alive byte `0x25` when this device was first onto the network.
const FIRST_ON_NETWORK: u8 = 0x02;
/// ...and when peers were already present. Latched at start and never
/// re-evaluated: a deck held `0x02` while its peer count went 1→2 (F9).
const JOINED_PEERS: u8 = 0x01;

/// How a virtual CDJ gets its device number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Numbering {
    /// Run the claim chain and hold a browsable number.
    ///
    /// Required to be browsed by a peer, and to issue dbserver queries.
    Claim {
        /// The number to try first. Any other browsable number is tried if it
        /// turns out to be taken; if all four are, starting fails with
        /// [`Error::NoBrowsableNumber`] rather than silently degrading, because
        /// the degraded state cannot serve and the caller has to know.
        preferred: Option<BrowsableDeviceNumber>,
    },
    /// Announce at a fixed number without contending for it.
    ///
    /// Safe next to a live rig. Cannot be browsed, and cannot browse over
    /// dbserver.
    Observer(DeviceNumber),
}

impl Default for Numbering {
    fn default() -> Self {
        Self::Observer(OBSERVER_NUMBER)
    }
}

/// How to present ourselves.
#[derive(Clone, Debug)]
pub struct VirtualCdjConfig {
    /// The name to announce. `CDJ-2000nexus` is the exact casing real hardware
    /// uses (F1).
    pub name: DeviceName,
    /// What kind of device to claim to be.
    pub kind: DeviceKind,
    /// How to get a device number.
    pub numbering: Numbering,
    /// Keep-alive byte `0x35`, a product-generation code.
    ///
    /// `0x00` is what real nexus gear sends — 148 packets from a CDJ-2000nexus
    /// and 91 from a DJM-2000nexus, all `0x00`, where the literature says
    /// `0x01` (C3). **`0x64` is required to coexist with CDJ-3000s**: the wrong
    /// value can make a CDJ-3000 set to player 5 or 6 repeatedly kick itself off
    /// the network.
    pub generation: u8,
    /// The firmware string to report in status packets.
    pub firmware: String,
    /// Whether to emit status packets on UDP 50002.
    ///
    /// Required for a peer to learn what is in our slots, and therefore for
    /// being browsable at all. Off means we are visible but have nothing to
    /// offer.
    pub emit_status: bool,
}

impl Default for VirtualCdjConfig {
    fn default() -> Self {
        Self {
            name: DeviceName::default(),
            kind: DeviceKind::CDJ,
            numbering: Numbering::default(),
            generation: 0x00,
            firmware: "1.44".to_owned(),
            emit_status: true,
        }
    }
}

/// Where the claim chain has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Watching the network before claiming.
    Watching,
    /// Broadcasting the claim chain.
    Claiming,
    /// Holding a number and sending keep-alives.
    Announcing,
    /// Every candidate number was defended.
    Failed,
}

/// A device on the network: keep-alives, and optionally status.
#[derive(Debug)]
pub struct VirtualCdj {
    config: VirtualCdjConfig,
    interface: crate::Interface,
    /// The number currently held. Changes during the claim chain, so it is
    /// shared rather than copied.
    number: Arc<AtomicU8>,
    phase: watch::Sender<Phase>,
    media: Arc<dyn MediaSource>,
    status_counter: Arc<AtomicU32>,
    tasks: Vec<JoinHandle<()>>,
}

impl VirtualCdj {
    /// Announce on the network `discovery` is listening to.
    ///
    /// Returns once a number is held — after the claim chain in
    /// [`Numbering::Claim`], immediately in [`Numbering::Observer`].
    pub async fn start(
        discovery: &Discovery,
        config: VirtualCdjConfig,
        media: Arc<dyn MediaSource>,
    ) -> Result<Self> {
        let interface = discovery.interface().clone();
        // We bind 0.0.0.0 and so receive our own broadcasts. Without this we
        // would list ourselves as a peer, which corrupts the peer count we put
        // in our own keep-alive.
        discovery.ignore(interface.mac);

        let was_first = discovery.is_alone();
        info!(
            first_on_network = was_first,
            peers = discovery.devices().len(),
            "starting virtual CDJ"
        );

        let (phase, _) = watch::channel(Phase::Watching);
        let number = match config.numbering {
            Numbering::Observer(number) => number,
            Numbering::Claim { preferred } => {
                match claim(discovery, &config, &interface, preferred, was_first, &phase).await {
                    Ok(number) => number,
                    Err(error) => {
                        let _ = phase.send(Phase::Failed);
                        return Err(error);
                    }
                }
            }
        };
        let _ = phase.send(Phase::Announcing);

        let mut cdj = Self {
            config,
            interface,
            number: Arc::new(AtomicU8::new(number.get())),
            phase,
            media,
            status_counter: Arc::new(AtomicU32::new(0)),
            tasks: Vec::new(),
        };
        cdj.tasks.push(cdj.spawn_keep_alive(discovery));
        cdj.tasks.push(cdj.spawn_defender(discovery));
        if cdj.config.emit_status {
            cdj.tasks.push(cdj.spawn_status(discovery)?);
            cdj.tasks.push(cdj.spawn_query_responder()?);
        }
        Ok(cdj)
    }

    /// Announce without serving anything: enough to make peers talk to us.
    pub async fn observe(discovery: &Discovery, config: VirtualCdjConfig) -> Result<Self> {
        Self::start(discovery, config, Arc::new(NoMedia)).await
    }

    /// The number currently held.
    pub fn number(&self) -> DeviceNumber {
        DeviceNumber::new(self.number.load(Ordering::Relaxed)).unwrap_or(OBSERVER_NUMBER)
    }

    /// The number currently held, if a peer would browse it.
    ///
    /// `None` means serving is pointless: a peer will accept everything and
    /// then never ask (F45). The serve side requires this rather than a plain
    /// device number, so a server that can never be browsed cannot be started.
    pub fn browsable_number(&self) -> Option<BrowsableDeviceNumber> {
        self.number().browsable()
    }

    /// Watch the claim state machine.
    pub fn phase(&self) -> watch::Receiver<Phase> {
        self.phase.subscribe()
    }

    /// The status packet we are emitting, for byte-diffing against a real deck.
    pub fn status_packet(&self, peers: usize) -> CdjStatus {
        let occupied = self.media.occupied_slots();
        let state = |slot: Slot| {
            if occupied.contains(&slot) {
                MediaState::LOADED
            } else {
                MediaState::EMPTY
            }
        };
        CdjStatus::builder()
            .device_number(self.number())
            .name(self.config.name)
            .slot_state(Slot::USB, state(Slot::USB))
            .slot_state(Slot::SD, state(Slot::SD))
            .link_available(!occupied.is_empty() || peers > 0)
            .firmware(&self.config.firmware)
            .packet_counter(self.status_counter.load(Ordering::Relaxed))
            .build()
    }

    /// The keep-alive we are broadcasting, for byte-diffing against a real deck.
    pub fn keep_alive(&self, peers: usize, was_first: bool) -> djl::Packet {
        djl::Packet::new(
            self.config.name,
            self.config.kind,
            Body::KeepAlive {
                device_number: self.number().get(),
                was_first_on_network: if was_first {
                    FIRST_ON_NETWORK
                } else {
                    JOINED_PEERS
                },
                mac: self.interface.mac,
                ip: self.interface.ip,
                // Includes ourselves.
                peer_count: u8::try_from(peers.saturating_add(1)).unwrap_or(u8::MAX),
                pad_31: [0; 3],
                flags: self.config.kind.role(),
                trailing: self.config.generation,
            },
        )
    }

    fn spawn_keep_alive(&self, discovery: &Discovery) -> JoinHandle<()> {
        let socket = discovery.socket();
        let broadcast = SocketAddr::V4(SocketAddrV4::new(
            self.interface.broadcast(),
            DISCOVERY_PORT,
        ));
        let was_first = discovery.is_alone();
        let config = self.config.clone();
        let interface = self.interface.clone();
        let number = Arc::clone(&self.number);
        let peers = PeerAddresses::new(discovery);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(djl::KEEPALIVE_INTERVAL);
            loop {
                ticker.tick().await;
                let packet =
                    keep_alive_packet(&config, &interface, &number, peers.len(), was_first);
                if let Err(error) = socket.send_to(&packet.encode(), broadcast).await {
                    warn!(%error, "keep-alive failed");
                }
            }
        })
    }

    /// Emit status, unicast, one copy per announced peer.
    ///
    /// Status is unicast on real hardware — not one of 1507 captured packets was
    /// broadcast (F21) — so this sends per peer rather than once. Peers that
    /// have not announced themselves get nothing, which mirrors why we received
    /// nothing until we announced.
    fn spawn_status(&self, discovery: &Discovery) -> Result<JoinHandle<()>> {
        // A real deck sends each status packet from a different, incrementing
        // source port. We do not imitate that; an ephemeral port is enough, and
        // the socket bound to 50002 is kept free to receive queries.
        let socket = socket::bind_at(self.interface.ip, 0, Some(&self.interface))?;
        let config = self.config.clone();
        let number = Arc::clone(&self.number);
        let media = Arc::clone(&self.media);
        let counter = Arc::clone(&self.status_counter);
        let table = PeerAddresses::new(discovery);

        Ok(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(status::STATUS_INTERVAL);
            loop {
                ticker.tick().await;
                let peers = table.get();
                let packet = status_packet(&config, &number, media.as_ref(), &counter, peers.len());
                counter.fetch_add(1, Ordering::Relaxed);
                for peer in peers {
                    let to = SocketAddr::V4(SocketAddrV4::new(peer, STATUS_PORT));
                    if let Err(error) = socket.send_to(packet.as_bytes(), to).await {
                        debug!(%error, %peer, "status unicast failed");
                    }
                }
            }
        }))
    }

    /// Answer media and settings queries on UDP 50002.
    ///
    /// **The step no reference implementation performs, because none of them
    /// serve.** Until these are answered, a deck that has otherwise fully
    /// accepted us — it is unicasting status to us and has completed a portmap
    /// and mount against our NFS server — still refuses to list us as a LINK
    /// source, because as far as it knows our slots hold nothing (F24).
    fn spawn_query_responder(&self) -> Result<JoinHandle<()>> {
        let socket = socket::bind(STATUS_PORT, Some(&self.interface))?;
        let name = self.config.name;
        let number = Arc::clone(&self.number);
        let media = Arc::clone(&self.media);

        Ok(tokio::spawn(async move {
            let mut buffer = vec![0u8; MAX_DATAGRAM];
            loop {
                let (len, from) = match socket.recv_from(&mut buffer).await {
                    Ok(received) => received,
                    Err(error) => {
                        warn!(%error, "status socket closed");
                        return;
                    }
                };
                let SocketAddr::V4(from) = from else { continue };
                let Some(datagram) = buffer.get(..len) else {
                    continue;
                };
                let Some(ours) = DeviceNumber::new(number.load(Ordering::Relaxed)) else {
                    continue;
                };

                let reply = match status::decode(datagram) {
                    Ok(status::Packet::MediaQuery(query)) => {
                        answer_media_query(query, ours, name, media.as_ref())
                            .map(|reply| (reply, query.requester_ip))
                    }
                    Ok(status::Packet::SettingsQuery(query)) => {
                        let settings = media.settings(query.slot);
                        let response = status::SettingsResponse::build(
                            name,
                            ours,
                            query.requester,
                            query.slot,
                            &settings,
                        );
                        info!(slot = %query.slot, bytes = settings.len(), "answered a settings query");
                        Some((response.into_bytes(), *from.ip()))
                    }
                    _ => None,
                };

                if let Some((datagram, to)) = reply {
                    let to = SocketAddr::V4(SocketAddrV4::new(to, STATUS_PORT));
                    if let Err(error) = socket.send_to(&datagram, to).await {
                        warn!(%error, "could not answer a query");
                    }
                }
            }
        }))
    }

    /// Defend our number, and watch for anyone defending it against us.
    ///
    /// A device that takes a number but never defends it simply loses it to the
    /// next player that boots.
    fn spawn_defender(&self, discovery: &Discovery) -> JoinHandle<()> {
        let mut announcements = discovery.announcements();
        let socket = discovery.socket();
        let config = self.config.clone();
        let interface = self.interface.clone();
        let number = Arc::clone(&self.number);

        tokio::spawn(async move {
            loop {
                let Ok(announcement) = announcements.recv().await else {
                    return;
                };
                let ours = number.load(Ordering::Relaxed);
                let (Body::ClaimIp {
                    device_number: proposed,
                    ..
                }
                | Body::ClaimNumber {
                    device_number: proposed,
                    ..
                }) = announcement.packet.body
                else {
                    continue;
                };
                if proposed != ours || announcement.packet.body.mac() == Some(interface.mac) {
                    continue;
                }

                info!(number = ours, challenger = %announcement.from, "defending our device number");
                let packet = djl::Packet::new(
                    config.name,
                    config.kind,
                    Body::NumberConflict {
                        device_number: ours,
                        ip: interface.ip,
                    },
                );
                let to = SocketAddr::V4(SocketAddrV4::new(announcement.from, DISCOVERY_PORT));
                if let Err(error) = socket.send_to(&packet.encode(), to).await {
                    warn!(%error, "could not defend our number");
                }
            }
        })
    }
}

impl Drop for VirtualCdj {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Build the answer to a media query, or nothing if the slot is not ours.
///
/// Saying nothing about a slot we do not serve is right: an empty reply would
/// tell the deck the slot exists and holds no tracks, and it would then offer an
/// empty medium.
fn answer_media_query(
    query: status::MediaQuery,
    ours: DeviceNumber,
    name: DeviceName,
    media: &dyn MediaSource,
) -> Option<Vec<u8>> {
    if query.target != ours {
        return None;
    }
    let description = media.describe(query.slot)?;
    info!(
        requester = %query.requester,
        slot = %query.slot,
        tracks = description.track_count,
        playlists = description.playlist_count,
        "answered a media query",
    );
    let mut builder = status::MediaResponse::builder()
        .device_number(ours)
        .slot(query.slot)
        .name(name)
        .volume_name(&description.volume_name)
        .counts(description.track_count, description.playlist_count);
    if let (Some(total), Some(free)) = (description.total_bytes, description.free_bytes) {
        builder = builder.size(total, free);
    }
    Some(builder.build().into_bytes())
}

fn keep_alive_packet(
    config: &VirtualCdjConfig,
    interface: &crate::Interface,
    number: &AtomicU8,
    peers: usize,
    was_first: bool,
) -> djl::Packet {
    djl::Packet::new(
        config.name,
        config.kind,
        Body::KeepAlive {
            device_number: number.load(Ordering::Relaxed),
            was_first_on_network: if was_first {
                FIRST_ON_NETWORK
            } else {
                JOINED_PEERS
            },
            mac: interface.mac,
            ip: interface.ip,
            peer_count: u8::try_from(peers.saturating_add(1)).unwrap_or(u8::MAX),
            pad_31: [0; 3],
            flags: config.kind.role(),
            trailing: config.generation,
        },
    )
}

fn status_packet(
    config: &VirtualCdjConfig,
    number: &AtomicU8,
    media: &dyn MediaSource,
    counter: &AtomicU32,
    peers: usize,
) -> CdjStatus {
    let occupied = media.occupied_slots();
    let state = |slot: Slot| {
        if occupied.contains(&slot) {
            MediaState::LOADED
        } else {
            MediaState::EMPTY
        }
    };
    let mut builder = CdjStatus::builder()
        .name(config.name)
        .slot_state(Slot::USB, state(Slot::USB))
        .slot_state(Slot::SD, state(Slot::SD))
        .link_available(!occupied.is_empty() || peers > 0)
        .firmware(&config.firmware)
        .packet_counter(counter.load(Ordering::Relaxed));
    if let Some(number) = DeviceNumber::new(number.load(Ordering::Relaxed)) {
        builder = builder.device_number(number);
    }
    builder.build()
}

/// Run the claim chain and return the number we ended up holding.
async fn claim(
    discovery: &Discovery,
    config: &VirtualCdjConfig,
    interface: &crate::Interface,
    preferred: Option<BrowsableDeviceNumber>,
    was_first: bool,
    phase: &watch::Sender<Phase>,
) -> Result<DeviceNumber> {
    debug!(?preferred, "watching before claiming");
    let _ = phase.send(Phase::Watching);
    tokio::time::sleep(PRESCAN).await;

    let seen = discovery.numbers_seen();
    let mut candidates: Vec<BrowsableDeviceNumber> = preferred
        .into_iter()
        .chain(BrowsableDeviceNumber::descending())
        .filter(|number| !seen.contains(&number.get()))
        .collect();
    candidates.dedup();
    if candidates.is_empty() {
        return Err(discovery.no_browsable_number());
    }

    let socket = discovery.socket();
    let to = SocketAddr::V4(SocketAddrV4::new(interface.broadcast(), DISCOVERY_PORT));
    let _ = phase.send(Phase::Claiming);

    for candidate in candidates {
        let mut conflicts = discovery.announcements();
        match claim_one(
            &socket,
            to,
            config,
            interface,
            candidate,
            was_first,
            &mut conflicts,
        )
        .await
        {
            Ok(()) => {
                info!(number = %candidate, "claimed a browsable device number");
                return Ok(candidate.number());
            }
            Err(holder) => {
                warn!(number = %candidate, %holder, "number is taken; backing off");
            }
        }
    }
    Err(discovery.no_browsable_number())
}

/// One pass of the claim chain for one candidate.
///
/// `Err(holder)` means somebody defended it.
async fn claim_one(
    socket: &UdpSocket,
    to: SocketAddr,
    config: &VirtualCdjConfig,
    interface: &crate::Interface,
    candidate: BrowsableDeviceNumber,
    was_first: bool,
    conflicts: &mut tokio::sync::broadcast::Receiver<crate::discovery::Announcement>,
) -> std::result::Result<(), Ipv4Addr> {
    // N is 3 into an empty network and 1 into a populated one (C13).
    let final_stage = if was_first { 3 } else { 1 };
    let mut stages: Vec<Body> = Vec::new();
    for iteration in 1..=3 {
        stages.push(Body::Hello {
            payload: config.kind.role(),
        });
        let _ = iteration;
    }
    for iteration in 1..=3 {
        stages.push(Body::ClaimMac {
            iteration,
            flags: config.kind.role(),
            mac: interface.mac,
        });
    }
    for iteration in 1..=3 {
        stages.push(Body::ClaimIp {
            ip: interface.ip,
            mac: interface.mac,
            device_number: candidate.get(),
            iteration,
            role: config.kind.role(),
            // We always ask for a specific number, having picked it ourselves.
            assignment_mode: djl::AssignmentMode::MANUAL,
        });
    }
    for iteration in 1..=final_stage {
        stages.push(Body::ClaimNumber {
            device_number: candidate.get(),
            iteration,
        });
    }

    for body in stages {
        let packet = djl::Packet::new(config.name, config.kind, body);
        if let Err(error) = socket.send_to(&packet.encode(), to).await {
            warn!(%error, "claim packet failed");
        }
        // Sleep between packets, watching for a conflict the whole time.
        let deadline = tokio::time::sleep(djl::DISCOVERY_INTERVAL);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                () = &mut deadline => break,
                received = conflicts.recv() => {
                    let Ok(announcement) = received else { break };
                    if let Body::NumberConflict { device_number, ip } = announcement.packet.body
                        && device_number == candidate.get()
                    {
                        return Err(if ip.is_unspecified() { announcement.from } else { ip });
                    }
                }
            }
        }
    }
    Ok(())
}

/// The addresses of every peer currently answering, refreshed as the table
/// changes.
///
/// Status is unicast per peer and the keep-alive carries a peer count, so both
/// timers need this and neither wants to lock the whole device table at 200 ms.
#[derive(Debug)]
struct PeerAddresses {
    devices: Arc<Mutex<Vec<Ipv4Addr>>>,
    task: JoinHandle<()>,
}

impl PeerAddresses {
    fn new(discovery: &Discovery) -> Self {
        let devices = Arc::new(Mutex::new(
            discovery
                .online()
                .into_iter()
                .map(|device| device.ip)
                .collect::<Vec<_>>(),
        ));
        let mut events = discovery.subscribe();
        let shared = Arc::clone(&devices);
        let task = tokio::spawn(async move {
            loop {
                let Ok(event) = events.recv().await else {
                    return;
                };
                let ip = event.device().ip;
                let add = !matches!(
                    event,
                    crate::device::DeviceEvent::Forgotten(_)
                        | crate::device::DeviceEvent::WentOffline(_)
                );
                let mut devices = match shared.lock() {
                    Ok(devices) => devices,
                    Err(poisoned) => poisoned.into_inner(),
                };
                devices.retain(|known| *known != ip);
                if add {
                    devices.push(ip);
                }
            }
        });
        Self { devices, task }
    }

    fn get(&self) -> Vec<Ipv4Addr> {
        match self.devices.lock() {
            Ok(devices) => devices.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn len(&self) -> usize {
        match self.devices.lock() {
            Ok(devices) => devices.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }
}

impl Drop for PeerAddresses {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interface() -> crate::Interface {
        crate::Interface {
            name: "test0".to_owned(),
            index: 1,
            ip: Ipv4Addr::new(169, 254, 99, 100),
            netmask: Ipv4Addr::new(255, 255, 0, 0),
            mac: prolink_proto::MacAddress([0xa0, 0xce, 0xc8, 0xe2, 0x26, 0xde]),
        }
    }

    #[test]
    fn the_default_numbering_cannot_disturb_a_live_rig() {
        let Numbering::Observer(number) = Numbering::default() else {
            panic!("the default must not claim");
        };
        assert_eq!(number, OBSERVER_NUMBER);
        assert!(
            !number.is_player(),
            "outside 1-6, so it cannot collide with hardware"
        );
        assert!(
            number.browsable().is_none(),
            "and cannot be browsed, by construction"
        );
    }

    #[test]
    fn our_keep_alive_matches_a_real_deck_byte_for_byte() {
        // The captured keep-alive from `captures/S24b-e9-control`, which the
        // codec's own test also pins. Emitting a packet that differs from this
        // is the definition of being distinguishable from a real deck.
        const REAL: &[u8] = &[
            0x51, 0x73, 0x70, 0x74, 0x31, 0x57, 0x6d, 0x4a, 0x4f, 0x4c, 0x06, 0x00, 0x43, 0x44,
            0x4a, 0x2d, 0x32, 0x30, 0x30, 0x30, 0x6e, 0x65, 0x78, 0x75, 0x73, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x36, 0x05, 0x02, 0xa0, 0xce, 0xc8, 0xe2,
            0x26, 0xde, 0xa9, 0xfe, 0x63, 0x64, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
        ];
        let config = VirtualCdjConfig {
            numbering: Numbering::Observer(DeviceNumber::new(5).unwrap()),
            ..VirtualCdjConfig::default()
        };
        let number = AtomicU8::new(5);
        let packet = keep_alive_packet(&config, &interface(), &number, 0, true);
        assert_eq!(packet.encode(), REAL);
    }

    #[test]
    fn a_status_packet_reports_only_the_slots_we_serve() {
        #[derive(Debug)]
        struct UsbOnly;
        impl MediaSource for UsbOnly {
            fn occupied_slots(&self) -> std::collections::BTreeSet<Slot> {
                [Slot::USB].into_iter().collect()
            }
            fn describe(&self, slot: Slot) -> Option<crate::MediaDescription> {
                (slot == Slot::USB).then(crate::MediaDescription::default)
            }
        }

        let number = AtomicU8::new(3);
        let counter = AtomicU32::new(0);
        let packet = status_packet(&VirtualCdjConfig::default(), &number, &UsbOnly, &counter, 1);
        assert_eq!(packet.usb_state(), MediaState::LOADED);
        assert_eq!(
            packet.sd_state(),
            MediaState::EMPTY,
            "a slot we do not serve is empty"
        );
        assert!(packet.link_available());
    }

    #[test]
    fn a_media_query_for_a_slot_we_do_not_serve_gets_no_answer() {
        // An empty reply would tell the deck the slot exists and holds no
        // tracks, and it would then offer an empty medium (F24).
        let ours = DeviceNumber::new(3).unwrap();
        let query = status::MediaQuery {
            requester: DeviceNumber::new(2).unwrap(),
            requester_ip: Ipv4Addr::new(169, 254, 1, 2),
            target: ours,
            slot: Slot::SD,
        };
        assert!(answer_media_query(query, ours, DeviceName::default(), &NoMedia).is_none());
    }

    #[test]
    fn a_media_query_addressed_elsewhere_gets_no_answer() {
        let ours = DeviceNumber::new(3).unwrap();
        let query = status::MediaQuery {
            requester: DeviceNumber::new(2).unwrap(),
            requester_ip: Ipv4Addr::new(169, 254, 1, 2),
            target: DeviceNumber::new(4).unwrap(),
            slot: Slot::USB,
        };
        assert!(answer_media_query(query, ours, DeviceName::default(), &NoMedia).is_none());
    }
}
