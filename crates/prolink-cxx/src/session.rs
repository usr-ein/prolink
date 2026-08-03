// SPDX-License-Identifier: GPL-3.0-only

//! The session behind the bridge, and the free functions C++ calls.
//!
//! All safe Rust. `cxx` owns the boundary, so there is no pointer handling
//! here and nothing in this crate outside the bridge macro's own expansion is
//! `unsafe` — which is the whole reason for choosing it over a C ABI.

use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use prolink::consume::NfsClient;
use prolink::serve::preserve::Preserve;
use prolink::{
    Discovery, Interface, Monitor, Numbering, VirtualCdj, VirtualCdjConfig, VirtualPlayer,
    VirtualPlayerConfig,
};
use tokio::runtime::Runtime;

use crate::convert::{self, plain};
use crate::ffi::{Config, Device, Event, EventKind, NetworkInterface, Player, ServeStatus, Slot};

/// How many events a session queues before it discards the oldest.
///
/// A host draining on a UI timer sees a handful per tick; this is two seconds
/// of a busy four-deck network, so it bites only when the host has stopped
/// draining altogether. The *oldest* goes, keeping the queue current rather
/// than stale, and the count is reported so the host knows to re-read the
/// tables — see `Event::dropped`.
const EVENT_QUEUE: usize = 512;

/// How often a peer's slot descriptions are re-read for changes.
///
/// A deck describes a slot **once**, when it first browses it, and never
/// repeats it (F37). So the description arrives at a moment nothing else
/// signals, and the only way to turn it into an event is to notice it. Half a
/// second is far below what a person notices and far above what this costs.
const MEDIA_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// A running session: sockets, timers, and the state they maintain.
///
/// Opaque to C++, which holds it as a `rust::Box<Session>` and drops it the
/// way it drops anything else. Dropping releases the device number.
pub struct Session {
    /// Dropped last, because the tasks below run on it.
    runtime: Runtime,
    /// The interface currently in use. Empty until something is bound.
    ///
    /// Not fixed at `open`: a host is often started before its ethernet is
    /// plugged in, and the whole session moves when a better interface appears.
    interface: Arc<Mutex<Option<Interface>>>,
    /// Media a host has asked us to serve, by local path.
    ///
    /// Held here rather than only in the player, because the player is rebuilt
    /// when the interface moves and what we serve must survive that.
    served: Arc<Mutex<Vec<(prolink_proto::Slot, String)>>>,
    /// The sockets, once they are up. `None` while starting, and again if
    /// starting failed — in which case `last_error` says why.
    ///
    /// **Starting is asynchronous**, and that is not an optimisation. Claiming
    /// a player number means watching the network for 2.5 s and then sending a
    /// nine-packet chain 300 ms apart: about five seconds during which a host
    /// that called this on its UI thread would be frozen. So `open` returns at
    /// once and the number appears when it is held.
    live: Arc<RwLock<Option<Live>>>,
    events: Arc<Mutex<Events>>,
    /// Hands out transfer ids, from 1, so zero always means "not a transfer".
    next_transfer: AtomicU32,
    /// The dbserver connections held open for browsing, one per player.
    connections: crate::browse::Connections,
    /// The connection artwork is fetched over, opened on first use.
    ///
    /// Separate from the browse connections, and behind an async lock: a host
    /// asks for covers by the hundred while the user is browsing, and sharing
    /// one connection would interleave a cover reply into the middle of a menu
    /// the user is scrolling.
    artwork: Arc<tokio::sync::Mutex<Option<prolink::consume::DbClient>>>,
    /// The last thing that went wrong, for a host's status line.
    last_error: Arc<Mutex<String>>,
    /// Transfers waiting their turn. See `fetch_file`.
    transfers: Arc<tokio::sync::Semaphore>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("device_number", &self.device_number())
            .field("interface", &self.interface().map(|found| found.name))
            .finish_non_exhaustive()
    }
}

/// The parts that only exist once the sockets are up.
struct Live {
    /// What everyone else is doing. Always present: watching costs nothing and
    /// needs no permission.
    monitor: Monitor,
    /// What we are, which depends on what the network let us be.
    role: Role,
}

/// Which of the two things a session turned out to be able to do.
///
/// Not a configuration choice. A host asks to be a player; whether it gets to
/// be one depends on whether a number in 1–4 was free, and the difference
/// matters to every caller — a player can be browsed and can serve, an observer
/// can do neither.
#[expect(
    clippy::large_enum_variant,
    reason = "see below; there is one of these per session"
)]
enum Role {
    /// A claimed player number. Browsable, and serving whatever is mounted.
    Player(Arc<VirtualPlayer>),
    // The two differ in size by a discovery listener and a virtual CDJ. Boxing
    // the larger would cost an allocation and an indirection on every accessor
    // to save a word on a value there is exactly one of per session.
    /// Every player number was defended, or the host asked not to announce.
    ///
    /// Still sees beats, tempo and — if it announced at all — status.
    Observer {
        discovery: Discovery,
        cdj: Option<VirtualCdj>,
    },
}

impl Role {
    fn discovery(&self) -> &Discovery {
        match self {
            Self::Player(player) => player.discovery(),
            Self::Observer { discovery, .. } => discovery,
        }
    }

    fn cdj(&self) -> Option<&VirtualCdj> {
        match self {
            Self::Player(player) => Some(player.cdj()),
            Self::Observer { cdj, .. } => cdj.as_ref(),
        }
    }

    fn number(&self) -> u8 {
        match self {
            Self::Player(player) => player.device_number().number().get(),
            Self::Observer { cdj, .. } => cdj.as_ref().map_or(0, |cdj| cdj.number().get()),
        }
    }

    fn player(&self) -> Option<&Arc<VirtualPlayer>> {
        match self {
            Self::Player(player) => Some(player),
            Self::Observer { .. } => None,
        }
    }
}

/// The drained event queue.
#[derive(Debug, Default)]
pub(crate) struct Events {
    queue: VecDeque<Event>,
    /// Discarded since the host last drained.
    dropped: u32,
}

impl Events {
    pub(crate) fn push(&mut self, event: Event) {
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

/// A config that chooses an interface, announces, and claims any free number.
#[must_use]
pub fn default_config() -> Config {
    Config {
        interface: String::new(),
        announce: true,
        preferred_number: 0,
        share_local_media: true,
    }
}

/// How often the supervisor looks for a better interface.
///
/// A DJ plugs the ethernet in and expects the rig to find itself; two seconds
/// is below what that feels like a wait for, and enumerating interfaces is a
/// handful of syscalls.
const INTERFACE_RECHECK: std::time::Duration = std::time::Duration::from_secs(2);

/// Start a session.
///
/// Returns immediately. **Nothing is bound yet**, and that is deliberate twice
/// over: claiming a player number takes about five seconds of watching and
/// negotiating, and there may be no network at all yet — a host is routinely
/// started before its ethernet is plugged in. A supervisor task binds when an
/// interface appears, and rebinds if a better one turns up later.
///
/// `device_number` is zero until a number is held, `is_ready` says whether the
/// sockets are up, and `last_error` carries anything that went wrong.
///
/// # Errors
///
/// When the runtime cannot be created, or a named interface does not exist.
/// Choosing automatically never fails here: an absent network is something to
/// wait for, not to refuse.
pub fn open(config: &Config) -> Result<Box<Session>, Error> {
    if !config.interface.is_empty() {
        Interface::named(&config.interface)
            .map_err(|error| Error(format!("no interface {}: {error}", config.interface)))?;
    }

    let runtime = Runtime::new().map_err(|error| Error(format!("no runtime: {error}")))?;
    let session = Session {
        runtime,
        interface: Arc::new(Mutex::new(None)),
        served: Arc::new(Mutex::new(Vec::new())),
        live: Arc::new(RwLock::new(None)),
        events: Arc::new(Mutex::new(Events::default())),
        next_transfer: AtomicU32::new(1),
        connections: crate::browse::Connections::default(),
        artwork: Arc::new(tokio::sync::Mutex::new(None)),
        last_error: Arc::new(Mutex::new(String::new())),
        // One at a time. Two NFS pulls from the same deck contend for the same
        // reply socket and the same filehandle table, and a deck answers
        // NFSERR_STALE to everything once that table churns (F28) -- so this
        // serialises them the way the C++ this replaces did with a queue.
        transfers: Arc::new(tokio::sync::Semaphore::new(1)),
    };

    let supervisor = Supervisor {
        config: config.clone(),
        live: Arc::clone(&session.live),
        interface: Arc::clone(&session.interface),
        served: Arc::clone(&session.served),
        events: Arc::clone(&session.events),
        last_error: Arc::clone(&session.last_error),
    };
    session.runtime.spawn(supervisor.run());
    spawn_media_survey(&session.runtime, Arc::clone(&session.live));
    if config.share_local_media {
        spawn_volume_watch(
            &session.runtime,
            Arc::clone(&session.live),
            Arc::clone(&session.served),
        );
    }
    Ok(Box::new(session))
}

/// Keeps the session bound to the right interface, for as long as it lives.
///
/// A single task rather than a rebuild triggered from the host: the host has no
/// way to know that the link came up, and polling `is_ready` and calling
/// `refresh` would make every host reimplement this.
struct Supervisor {
    config: Config,
    live: Arc<RwLock<Option<Live>>>,
    interface: Arc<Mutex<Option<Interface>>>,
    served: Arc<Mutex<Vec<(prolink_proto::Slot, String)>>>,
    events: Arc<Mutex<Events>>,
    last_error: Arc<Mutex<String>>,
}

impl Supervisor {
    async fn run(self) {
        let mut announced_failure = false;
        loop {
            let wanted = self.choose();
            let current = self.current();

            match (wanted, current) {
                // Nothing to bind to. Keep the last error current so a host can
                // say why, and wait: an unplugged cable is not a failure to
                // report once and give up on.
                (None, _) => {
                    if !announced_failure {
                        self.note("no usable network interface; waiting for one");
                        announced_failure = true;
                    }
                    self.teardown();
                }
                // Already on it.
                (Some(wanted), Some(current))
                    if wanted.name == current.name && wanted.ip == current.ip =>
                {
                    announced_failure = false;
                }
                (Some(wanted), current) => {
                    announced_failure = false;
                    if let Some(current) = current {
                        tracing::info!(
                            from = current.name,
                            to = wanted.name,
                            "the network moved; rebinding"
                        );
                    }
                    // Torn down before the rebuild, not after: the old session
                    // holds UDP 50000 and 50002, and the new one cannot bind
                    // what the old one still has.
                    self.teardown();
                    self.build(wanted).await;
                }
            }
            tokio::time::sleep(INTERFACE_RECHECK).await;
        }
    }

    /// The interface this session should be on, if there is one.
    fn choose(&self) -> Option<Interface> {
        if self.config.interface.is_empty() {
            Interface::best_guess().ok()
        } else {
            Interface::named(&self.config.interface).ok()
        }
    }

    fn current(&self) -> Option<Interface> {
        self.interface.lock().ok().and_then(|held| held.clone())
    }

    /// Give up the device number and close every socket.
    fn teardown(&self) {
        let had = match self.live.write() {
            Ok(mut slot) => slot.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if had.is_some() {
            tracing::info!("released the device number");
        }
        drop(had);
        if let Ok(mut held) = self.interface.lock() {
            *held = None;
        }
    }

    async fn build(&self, interface: Interface) {
        let served = self
            .served
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default();
        match start(&self.config, &interface, &served, &self.events).await {
            Ok(live) => {
                if let Ok(mut held) = self.interface.lock() {
                    *held = Some(interface);
                }
                match self.live.write() {
                    Ok(mut slot) => *slot = Some(live),
                    Err(poisoned) => *poisoned.into_inner() = Some(live),
                }
                self.note("");
            }
            Err(error) => self.note(&format!("could not start: {error}")),
        }
    }

    fn note(&self, message: &str) {
        if !message.is_empty() {
            tracing::warn!("{message}");
        }
        if let Ok(mut held) = self.last_error.lock() {
            message.clone_into(&mut held);
        }
    }
}

/// Bring up one session on one interface.
///
/// The order is the protocol's. Files first, because a deck asks the portmapper
/// where our mount service is *before* it opens dbserver and retries for ever
/// if nothing answers (F46). Then the device number, then the database — and
/// only then the monitor, which shares the socket the announcement created.
async fn start(
    config: &Config,
    interface: &Interface,
    served: &[(prolink_proto::Slot, String)],
    events: &Arc<Mutex<Events>>,
) -> Result<Live, prolink::Error> {
    let role = if config.announce {
        announce_as_player(config, interface, served).await?
    } else {
        Role::Observer {
            discovery: Discovery::start(interface.clone()).await?,
            cdj: None,
        }
    };

    // With a virtual CDJ the monitor also reads UDP 50002, which is what
    // carries the loaded track and the tempo master (F21). It shares the CDJ's
    // socket rather than binding its own; see `Monitor::with_status`.
    let monitor = match role.cdj() {
        Some(cdj) => Monitor::with_status(interface.clone(), cdj).await?,
        None => Monitor::start(interface.clone()).await?,
    };

    let sink = Arc::clone(events);
    let mut incoming = monitor.subscribe();
    // Ends when the session is dropped, because the sender goes with it.
    tokio::spawn(async move {
        while let Ok(event) = incoming.recv().await {
            if let Ok(mut queue) = sink.lock() {
                queue.push(convert::event(&event));
            }
        }
    });

    if let Some(watching) = role.cdj() {
        spawn_media_watch(watching.peer_media(), Arc::clone(events));
    }

    Ok(Live { monitor, role })
}

/// Take a real player number, falling back to watching from outside the range.
///
/// A number in 1–4 is not a preference. A deck will not offer us as a LINK
/// source at any other number, and will not browse us: the check precedes the
/// whole browse path and fails silently (F45). So the full player is tried
/// first, and only if every browsable number is defended do we settle for
/// observing — which still shows tempo, beats and what each deck has loaded,
/// and is a great deal better than refusing to start at all.
async fn announce_as_player(
    config: &Config,
    interface: &Interface,
    served: &[(prolink_proto::Slot, String)],
) -> Result<Role, prolink::Error> {
    let mut media = Vec::new();
    for (slot, path) in served {
        match load_medium(*slot, path) {
            Ok(medium) => media.push(medium),
            // One unreadable stick must not cost the device number, and with it
            // every other thing this session does.
            Err(error) => tracing::warn!(path, "not serving it: {error}"),
        }
    }

    let settings = VirtualPlayerConfig {
        preferred_number: prolink::BrowsableDeviceNumber::new(config.preferred_number),
        ..VirtualPlayerConfig::new(interface.clone())
    };
    match VirtualPlayer::start(settings, media).await {
        Ok(player) => {
            tracing::info!(number = %player.device_number(), "claimed a player number");
            Ok(Role::Player(Arc::new(player)))
        }
        Err(error) => {
            tracing::warn!(
                "{error}; watching as device {} instead, which cannot be browsed or serve",
                prolink::OBSERVER_NUMBER
            );
            let discovery = Discovery::start(interface.clone()).await?;
            let cdj = VirtualCdj::observe(
                &discovery,
                VirtualCdjConfig {
                    numbering: Numbering::Observer(prolink::OBSERVER_NUMBER),
                    emit_status: false,
                    ..VirtualCdjConfig::default()
                },
            )
            .await?;
            Ok(Role::Observer {
                discovery,
                cdj: Some(cdj),
            })
        }
    }
}

/// Read a rekordbox medium off a local path.
fn load_medium(
    slot: prolink_proto::Slot,
    path: &str,
) -> Result<Arc<prolink::Medium>, prolink::Error> {
    let served = match slot {
        prolink_proto::Slot::SD => prolink::serve::ServedSlot::SD,
        _ => prolink::serve::ServedSlot::USB,
    };
    prolink::Medium::from_volume(std::path::Path::new(path), served).map(Arc::new)
}

/// Turn a peer's slot description into an event the first time it appears.
///
/// A deck describes a slot **once**, when it first browses it, and never
/// repeats it (F37) — so there is nothing to subscribe to and the change has to
/// be noticed.
fn spawn_media_watch(media: Arc<prolink::PeerMedia>, sink: Arc<Mutex<Events>>) {
    tokio::spawn(async move {
        let mut seen: std::collections::BTreeMap<(u8, prolink_proto::Slot), String> =
            std::collections::BTreeMap::new();
        loop {
            tokio::time::sleep(MEDIA_POLL).await;
            for slot in media.all() {
                let Some(description) = slot.description.as_ref() else {
                    continue;
                };
                let key = (slot.device.get(), slot.slot);
                let now = format!(
                    "{}/{}/{}",
                    description.volume_name, description.track_count, description.playlist_count
                );
                if seen.get(&key) == Some(&now) {
                    continue;
                }
                seen.insert(key, now);
                let mut event = plain(EventKind::MediaInfo, slot.device.get(), 0);
                event.slot = convert::slot(slot.slot);
                if let Ok(mut queue) = sink.lock() {
                    queue.push(event);
                }
            }
        }
    });
}

impl Session {
    /// The interface currently bound, or empty if none is.
    #[must_use]
    pub fn interface_name(&self) -> String {
        self.interface()
            .map_or_else(String::new, |found| found.name)
    }

    /// The interface currently bound, if any.
    pub(crate) fn interface(&self) -> Option<Interface> {
        self.interface.lock().ok().and_then(|held| held.clone())
    }

    /// Read the live parts, if the sockets are up yet.
    ///
    /// Everything a host can ask about the network goes through here, and
    /// answers with a default while starting rather than blocking: a host polls
    /// this from its UI thread several times a second.
    fn with_live<T>(&self, read: impl FnOnce(&Live) -> T) -> Option<T> {
        let held = self.live.read().ok()?;
        held.as_ref().map(read)
    }

    /// Whether the sockets are up and the device number, if any, is settled.
    ///
    /// False means starting or failed; `last_error` distinguishes the two.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.with_live(|_| true).unwrap_or(false)
    }

    /// The number we announced as, or zero if we have not announced yet.
    #[must_use]
    pub fn device_number(&self) -> u8 {
        self.with_live(|live| live.role.number()).unwrap_or(0)
    }

    /// Everyone on the network, as of now.
    #[must_use]
    pub fn devices(&self) -> Vec<Device> {
        self.with_live(|live| {
            live.role
                .discovery()
                .devices()
                .iter()
                .map(convert::device)
                .collect()
        })
        .unwrap_or_default()
    }

    /// What every player is doing, as of now.
    #[must_use]
    pub fn players(&self) -> Vec<Player> {
        self.with_live(|live| live.monitor.players().iter().map(convert::player).collect())
            .unwrap_or_default()
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
        let Some(interface) = self.interface() else {
            return Err(Error("the network is not up yet".to_owned()));
        };
        let queue = Arc::clone(&self.transfers);
        let last_error = Arc::clone(&self.last_error);
        let slot = convert::slot_back(slot);
        let (remote, local) = (remote_path.to_owned(), local_path.to_owned());

        self.runtime.spawn(async move {
            // One transfer at a time. Two pulls from the same deck contend for
            // its filehandle table, and a deck answers NFSERR_STALE to
            // everything once that table churns (F28) -- so they queue, as the
            // C++ this replaces queued them.
            let _turn = queue.acquire().await;
            let outcome = fetch(&interface, peer, slot, &remote, &local, id, &events).await;
            let mut done = plain(EventKind::TransferDone, 0, 0);
            done.transfer = id;
            done.path.clone_from(&local);
            if let Err(reason) = outcome {
                tracing::warn!(%peer, remote, "transfer failed: {reason}");
                done.ok = false;
                done.detail.clone_from(&reason);
                if let Ok(mut held) = last_error.lock() {
                    *held = reason;
                }
            }
            if let Ok(mut queue) = events.lock() {
                queue.push(done);
            }
        });
        Ok(id)
    }

    /// Fetch a track the way a CDJ plays one: enough to start, then the rest.
    ///
    /// A whole-file fetch makes loading a remote track take as long as
    /// downloading it, which is the wait this exists to remove. Instead the
    /// local file is created at its FULL size immediately and filled in from
    /// the network, head first. That matters more than it sounds: a decoder
    /// opening a short file reads a short duration off it and will not play
    /// past what was there when it looked, whereas a full-size file that is
    /// still filling has the right length from the first moment.
    ///
    /// `head_bytes` is fetched before the first progress event is emitted, so a
    /// caller can wait for that and start playing knowing there is a runway.
    /// The rest arrives behind the playhead -- a network measured in tens of
    /// megabytes a second against playback measured in tens of KILOBYTES a
    /// second, so it is not a close race.
    ///
    /// **The tail is fetched second, before the middle.** MP3 decodes happily
    /// from the front, but M4A and MP4 commonly keep the `moov` atom at the END
    /// of the file, and a decoder cannot open one at all without it. Fetching
    /// the last chunk early costs one round trip and is the difference between
    /// AAC working and AAC not opening.
    ///
    /// # Errors
    ///
    /// As [`Self::fetch_file`].
    pub fn fetch_file_streaming(
        &self,
        device_number: u8,
        slot: Slot,
        remote_path: &str,
        local_path: &str,
        head_bytes: u32,
    ) -> Result<u32, Error> {
        let peer = self
            .address_of(device_number)
            .ok_or_else(|| Error(format!("no device {device_number} on the network")))?;

        let id = self.next_transfer.fetch_add(1, Ordering::Relaxed);
        let events = Arc::clone(&self.events);
        let Some(interface) = self.interface() else {
            return Err(Error("the network is not up yet".to_owned()));
        };
        let queue = Arc::clone(&self.transfers);
        let last_error = Arc::clone(&self.last_error);
        let slot = convert::slot_back(slot);
        let (remote, local) = (remote_path.to_owned(), local_path.to_owned());

        self.runtime.spawn(async move {
            let _turn = queue.acquire().await;
            let outcome = fetch_streaming(
                &interface,
                peer,
                slot,
                &remote,
                &local,
                u64::from(head_bytes),
                id,
                &events,
            )
            .await;
            let mut done = plain(EventKind::TransferDone, 0, 0);
            done.transfer = id;
            done.path.clone_from(&local);
            if let Err(reason) = outcome {
                tracing::warn!(%peer, remote, "streaming transfer failed: {reason}");
                done.ok = false;
                done.detail.clone_from(&reason);
                if let Ok(mut held) = last_error.lock() {
                    *held = reason;
                }
            } else {
                // The line a host follows to know the switch from network to
                // local reads is safe to make.
                tracing::info!(remote, local, "background download complete");
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

    /// Whether the sockets are up and we are hearing the network.
    #[must_use]
    pub fn is_listening(&self) -> bool {
        // Discovery is the socket everything else depends on, and a device
        // table that has ever seen anything proves traffic is arriving.
        self.with_live(|live| {
            !live.role.discovery().devices().is_empty() || live.role.cdj().is_some()
        })
        .unwrap_or(false)
    }

    /// Offer a local rekordbox medium to the players on the network.
    ///
    /// `path` is the mount point of a stick — the directory holding `PIONEER/`.
    /// Replaces whatever was in that slot.
    ///
    /// Returns at once and does the work on the session's own runtime: reading
    /// the database and walking the files takes a second or two for a full
    /// stick, and a host calling this from a UI thread must not wear that. A
    /// path that turns out not to be a rekordbox export is reported through
    /// `last_error`.
    ///
    /// Remembered across a rebind, so a stick stays served when the session
    /// moves to another interface, and it does not matter whether the network
    /// is up yet: a host should call this when it notices the medium.
    pub fn serve_media(&self, slot: Slot, path: &str) {
        let slot = convert::slot_back(slot);
        let path = path.to_owned();
        let served = Arc::clone(&self.served);
        let live = Arc::clone(&self.live);
        let last_error = self.error_sink();
        self.runtime.spawn(async move {
            if let Err(error) = mount(&live, &served, slot, &path).await {
                last_error.note(&format!("serving {path}: {error}"));
            }
        });
    }

    /// Stop offering whatever is in a slot.
    ///
    /// Ejects it first, which takes a couple of seconds when a deck is reading
    /// from us: it is told through the slot state in our status packets and
    /// answers with `UMNT` (F20). Pulling the files out from under it instead
    /// leaves it holding a filehandle onto nothing. That wait happens on the
    /// session's runtime, so this returns at once.
    pub fn stop_serving(&self, slot: Slot) {
        let slot = convert::slot_back(slot);
        let served = Arc::clone(&self.served);
        let live = Arc::clone(&self.live);
        self.runtime
            .spawn(async move { unmount(&live, &served, slot).await });
    }

    /// What we are offering, and to whom.
    #[must_use]
    pub fn serve_status(&self) -> ServeStatus {
        let interface = self.interface();
        let name = interface
            .as_ref()
            .map_or_else(String::new, |found| found.name.clone());
        let address = interface.map_or_else(String::new, |found| found.ip.to_string());
        self.with_live(|live| {
            live.role
                .player()
                .map(|player| crate::serve::describe(player, name.clone(), address))
        })
        .flatten()
        .unwrap_or_else(|| crate::serve::nothing(name))
    }

    /// The last thing that went wrong, or empty.
    #[must_use]
    pub fn last_error(&self) -> String {
        self.last_error
            .lock()
            .map(|held| held.clone())
            .unwrap_or_default()
    }

    /// A handle on the error slot, for a caller that already holds a mutable
    /// borrow of the session and so cannot call [`Self::note_error`].
    pub(crate) fn error_sink(&self) -> ErrorSink {
        ErrorSink(Arc::clone(&self.last_error))
    }

    /// Drop every held connection so the next browse reconnects.
    ///
    /// The device table needs no refreshing — it is rebuilt from keep-alives
    /// every two seconds and reaps what stops sending. What does go stale is a
    /// dbserver connection, which is keyed on a device *number*, and a number
    /// can move to a different deck between one browse and the next.
    pub fn refresh(&mut self) {
        let mut connections = std::mem::take(&mut self.connections);
        connections.close_all(&self.runtime);
    }

    /// The device number a MAC currently holds, or zero.
    #[must_use]
    pub fn device_number_of(&self, mac: &str) -> u8 {
        let wanted = mac.trim().to_ascii_lowercase();
        self.with_live(|live| {
            live.role
                .discovery()
                .devices()
                .into_iter()
                .find(|device| device.mac.to_string().to_ascii_lowercase() == wanted)
                .map_or(0, |device| device.number.get())
        })
        .unwrap_or(0)
    }

    /// The next transfer id. From 1, so zero always means "not a transfer".
    pub(crate) fn next_transfer_id(&self) -> u32 {
        self.next_transfer.fetch_add(1, Ordering::Relaxed)
    }

    /// The event queue, for a task that finishes after the caller returns.
    pub(crate) fn events_handle(&self) -> Arc<Mutex<Events>> {
        Arc::clone(&self.events)
    }

    /// The connection artwork is fetched over.
    pub(crate) fn artwork_queue(
        &self,
    ) -> Arc<tokio::sync::Mutex<Option<prolink::consume::DbClient>>> {
        Arc::clone(&self.artwork)
    }

    /// The runtime, for spawning work that outlives the call.
    pub(crate) fn runtime_handle(&self) -> &Runtime {
        &self.runtime
    }

    /// What our peers have in their own slots, if we have announced.
    ///
    /// An `Arc` rather than a borrow: the virtual CDJ lives behind the lock
    /// that `with_live` takes, and nothing may hold that across a browse.
    pub(crate) fn peer_media(&self) -> Option<Arc<prolink::PeerMedia>> {
        self.with_live(|live| live.role.cdj().map(VirtualCdj::peer_media))
            .flatten()
    }

    /// The number this session can browse with, if it holds a browsable one.
    pub(crate) fn browsable_number(&self) -> Option<prolink::BrowsableDeviceNumber> {
        crate::browse::browsable(self.device_number())
    }

    /// The runtime and the connection cache, borrowed together.
    ///
    /// One method because the borrow checker will not allow two `&mut self`
    /// calls to be live at once, and every browse needs both.
    pub(crate) fn runtime_and_connections(
        &mut self,
    ) -> (&Runtime, &mut crate::browse::Connections) {
        (&self.runtime, &mut self.connections)
    }

    /// The connection cache, to look in without borrowing mutably.
    pub(crate) fn connections_ref(&self) -> &crate::browse::Connections {
        &self.connections
    }

    /// The address of a device by number, if it is on the network.
    pub(crate) fn address_of(&self, number: u8) -> Option<Ipv4Addr> {
        self.with_live(|live| {
            live.role
                .discovery()
                .devices()
                .into_iter()
                .find(|device| device.number.get() == number)
                .map(|device| device.ip)
        })
        .flatten()
    }
}

/// Put a medium in a slot, and remember it for the next rebind.
///
/// The registry is updated whether or not a player is running: a session that
/// has not yet claimed a number still knows what it is meant to be offering,
/// and serves it the moment it does.
#[expect(
    clippy::unused_async,
    reason = "the sibling unmount awaits an eject, and a caller should not have \
              to know which of the pair blocks"
)]
async fn mount(
    live: &Arc<RwLock<Option<Live>>>,
    served: &Arc<Mutex<Vec<(prolink_proto::Slot, String)>>>,
    slot: prolink_proto::Slot,
    path: &str,
) -> Result<(), prolink::Error> {
    let medium = load_medium(slot, path)?;
    if let Ok(mut held) = served.lock() {
        held.retain(|(existing, _)| *existing != slot);
        held.push((slot, path.to_owned()));
    }
    if let Some(player) = player_of(live) {
        player.mount(medium)?;
    }
    Ok(())
}

/// Take a medium out of a slot, ejecting it the way a deck does.
async fn unmount(
    live: &Arc<RwLock<Option<Live>>>,
    served: &Arc<Mutex<Vec<(prolink_proto::Slot, String)>>>,
    slot: prolink_proto::Slot,
) {
    if let Ok(mut held) = served.lock() {
        held.retain(|(existing, _)| *existing != slot);
    }
    if let Some(player) = player_of(live) {
        player.unmount(slot).await;
    }
}

/// The running player, cloned out from under the lock.
///
/// Cloned rather than borrowed because the caller then awaits, and the guard on
/// this lock is what every accessor takes thirty times a second.
fn player_of(live: &Arc<RwLock<Option<Live>>>) -> Option<Arc<VirtualPlayer>> {
    let held = live.read().ok()?;
    held.as_ref()?.role.player().map(Arc::clone)
}

/// How often peers are asked what is in their slots.
///
/// **Nothing arrives unasked.** A player publishes its volume name and its
/// counts *only* in answer to a media query, and it answers each one once and
/// never repeats it (F37) — so a host that does not ask never learns that the
/// deck across the booth has a stick in at all.
///
/// That is exactly what happened: everything else worked against real hardware
/// — the CDJ found us, queried our media and opened a dbserver connection — and
/// its own stick simply never appeared in our source list, because the only
/// thing that sends a query lived in the CLI.
///
/// Five seconds, and two small UDP packets per peer per round. A deck answers
/// one in about a millisecond.
const MEDIA_SURVEY: std::time::Duration = std::time::Duration::from_secs(5);

/// How long to wait for the answers before reading the table. A deck-to-deck
/// query is answered in ~1 ms; this is generous by three orders of magnitude
/// and costs nothing, because it is a sleep inside a task of its own.
const MEDIA_SURVEY_WAIT: std::time::Duration = std::time::Duration::from_millis(400);

/// Keep asking the players what they have in their slots.
///
/// Repeated rather than done once per device, because media are swapped mid-set
/// and a slot's description is not re-sent when that happens.
fn spawn_media_survey(runtime: &Runtime, live: Arc<RwLock<Option<Live>>>) {
    runtime.spawn(async move {
        loop {
            tokio::time::sleep(MEDIA_SURVEY).await;
            let Some(player) = player_of(&live) else {
                continue;
            };
            if let Err(error) = player
                .cdj()
                .survey_media(player.discovery(), MEDIA_SURVEY_WAIT)
                .await
            {
                tracing::debug!(%error, "a media survey did not complete");
            }
        }
    });
}

/// How often the local sticks are re-scanned.
///
/// Two seconds, which is what the C++ this replaces used: fast enough that a
/// stick appears before the DJ has finished reaching for the deck, cheap enough
/// to run forever.
const VOLUME_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// Serve whatever rekordbox media is plugged into this machine.
///
/// A CDJ has two slots and so do we, so at most two sticks are offered: USB
/// first, because that is where a DJ expects one to appear.
///
/// **Reconciled every tick, not driven by changes.** A stick plugged in while
/// the session is still claiming its number has nowhere to go yet — the player
/// does not exist — and an edge-triggered watcher would record it as done and
/// never look again, which is exactly what happened on hardware: the medium was
/// mounted eight seconds before the player came up and the player served
/// nothing for the rest of the session. So each tick compares what is plugged
/// in against what the *player* is actually serving, and anything the two
/// disagree about is put right, however it came to be wrong.
fn spawn_volume_watch(
    runtime: &Runtime,
    live: Arc<RwLock<Option<Live>>>,
    served: Arc<Mutex<Vec<(prolink_proto::Slot, String)>>>,
) {
    // Through the runtime, not `tokio::spawn`. This is called from `open`, on
    // the host's own thread, where there is no runtime in context -- and the
    // panic that causes crosses the `cxx` boundary, where an unwind is an
    // abort. The host does not get an exception; it gets SIGABRT at startup.
    runtime.spawn(async move {
        // The slots this watcher manages. A medium a host mounted by hand
        // through `serve_media` is none of its business and is left alone.
        let mut ours: std::collections::BTreeMap<prolink_proto::Slot, String> =
            std::collections::BTreeMap::new();
        let mut keeping = Preserving::new();
        loop {
            tokio::time::sleep(VOLUME_POLL).await;
            keeping.tick(&live);
            reconcile(&live, &served, &mut ours, &mut keeping).await;
        }
    });
}

/// Where copies of files a player is using are kept.
///
/// tmpfs, so this costs no writes to the SD card and evaporates at reboot --
/// which is exactly right for a copy of somebody's stick.
///
/// `/tmp` before `/run`, and that ordering is measured rather than stylistic:
/// systemd sizes `/run` at a fraction of RAM and `/tmp` at half of it, so on
/// the deck they are 760 MB and 1.9 GB. A cache sized for the wrong one fills
/// the filesystem instead of reaching its cap, and a full `/run` takes systemd
/// with it. The host picks the same way; see its `RamStore`.
fn preserve_root() -> std::path::PathBuf {
    for candidate in ["/tmp/trimixxx/serve", "/run/trimixxx/serve"] {
        let path = std::path::PathBuf::from(candidate);
        if std::fs::create_dir_all(&path).is_ok() {
            return path;
        }
    }
    let fallback = std::env::temp_dir().join("trimixxx-serve");
    let _ = std::fs::create_dir_all(&fallback);
    fallback
}

/// How much may be held against a stick being pulled.
///
/// Two lossless tracks, which is what two players can have loaded at once. Past
/// it nothing new is kept and the log says so, rather than the machine filling
/// its tmpfs -- and it has to share that tmpfs with the host's own track cache,
/// which is why this is not larger.
const PRESERVE_CAP: u64 = 192 * 1024 * 1024;

/// Keeping a consuming player's tracks readable across an eject.
///
/// # The problem, stated once
///
/// A real CDJ does not buffer a whole track. It holds an emergency loop of what
/// is under the needle and streams the rest off the medium — so a stick pulled
/// while a player is playing from us is a player that stops several seconds
/// later, mid-set, with no warning.
///
/// # What this does about it
///
/// Two things, on the same two-second poll the volume watch already runs:
///
///  * **While the stick is in**, copy whatever a player is seen to have loaded.
///    The load is announced in the player's own status packets, which name the
///    source player, the slot and the track id — so this needs no hook in the
///    read path and cannot miss a load.
///  * **When the stick goes and a player is still holding a track from it**,
///    the medium goes phantom instead of being ejected: still announced, so the
///    consumer's mount stays valid; served from the copies, so its reads are
///    answered exactly as before; and unbrowsable, so no DJ can start something
///    we could not finish.
///
/// Then, when the last consumer has moved on, the medium is ejected properly —
/// so it disappears from their screens the way a stick does, rather than
/// vanishing under a playing track.
struct Preserving {
    /// The copies, keyed by their path in the served tree.
    files: Preserve,
    /// Slots being served as phantoms, and whether anyone is still consuming
    /// them.
    phantom: std::collections::BTreeSet<prolink_proto::Slot>,
}

impl Preserving {
    fn new() -> Self {
        Self {
            files: Preserve::new(preserve_root(), PRESERVE_CAP),
            phantom: std::collections::BTreeSet::new(),
        }
    }

    /// Copy what every consumer is currently using.
    ///
    /// Idempotent per file, so this is cheap on every poll but the first after
    /// a load.
    fn tick(&mut self, live: &Arc<RwLock<Option<Live>>>) {
        let Some(player) = player_of(live) else {
            return;
        };
        for (device, slot, track) in player.consumers() {
            let copied = player.preserve_track(&mut self.files, slot, track);
            if copied > 0 {
                tracing::info!(
                    device,
                    %slot,
                    track,
                    files = copied,
                    "a player loaded one of our tracks; holding on to it"
                );
            }
        }
    }

    /// Whether any player is still using this slot.
    fn consumed(live: &Arc<RwLock<Option<Live>>>, slot: prolink_proto::Slot) -> bool {
        player_of(live).is_some_and(|player| {
            player
                .consumers()
                .iter()
                .any(|(_, consumed, _)| *consumed == slot)
        })
    }

    /// The stick in `slot` has gone. Returns true if it went phantom rather
    /// than being ejected, in which case the caller must leave it alone.
    fn removed(&mut self, live: &Arc<RwLock<Option<Live>>>, slot: prolink_proto::Slot) -> bool {
        if !Self::consumed(live, slot) {
            return false;
        }
        let Some(player) = player_of(live) else {
            return false;
        };
        player.go_phantom(slot, &self.files);
        self.phantom.insert(slot);
        true
    }

    /// Whether a phantom slot's last consumer has gone, so it can leave.
    fn finished(&mut self, live: &Arc<RwLock<Option<Live>>>) -> Vec<prolink_proto::Slot> {
        let done: Vec<_> = self
            .phantom
            .iter()
            .copied()
            .filter(|slot| !Self::consumed(live, *slot))
            .collect();
        for slot in &done {
            self.phantom.remove(slot);
        }
        done
    }

    /// A phantom slot has been ejected for real, or a fresh stick has taken it.
    fn release(&mut self, slot: prolink_proto::Slot) {
        self.phantom.remove(&slot);
        if self.phantom.is_empty() {
            // Nothing is being fed from copies any more, so the copies are
            // just occupying RAM.
            self.files.clear();
        }
    }
}

/// The slots a local stick may be offered in, in the order they are filled.
const LOCAL_SLOTS: [prolink_proto::Slot; 2] = [prolink_proto::Slot::USB, prolink_proto::Slot::SD];

/// Make what we are serving match what is plugged in.
async fn reconcile(
    live: &Arc<RwLock<Option<Live>>>,
    served: &Arc<Mutex<Vec<(prolink_proto::Slot, String)>>>,
    ours: &mut std::collections::BTreeMap<prolink_proto::Slot, String>,
    keeping: &mut Preserving,
) {
    let found: Vec<String> = prolink::rekordbox_volumes()
        .into_iter()
        .map(|volume| volume.path.display().to_string())
        .collect();

    // A phantom whose last consumer has moved on. Ejected properly now, so it
    // disappears from their screens the way a stick does rather than vanishing
    // under a playing track.
    for slot in keeping.finished(live) {
        tracing::info!(%slot, "the last player let go; the medium can leave now");
        keeping.release(slot);
        unmount(live, served, slot).await;
    }

    // Gone first, so a stick swapped between two scans frees its slot before
    // the replacement asks for one.
    for slot in LOCAL_SLOTS {
        // Taken by value: the entry is removed below, and the path is still
        // wanted for the log lines after that.
        if let Some(path) = ours.get(&slot).cloned()
            && !found.contains(&path)
        {
            ours.remove(&slot);
            // A player still playing off it gets to finish. The medium stays
            // announced and is served from the copies made while the stick was
            // in; only browsing it stops working.
            if keeping.removed(live, slot) {
                tracing::info!(
                    %slot,
                    path,
                    "the medium was removed while a player was using it; going phantom"
                );
                continue;
            }
            tracing::info!(%slot, path, "the medium was removed");
            unmount(live, served, slot).await;
        }
    }

    for path in found {
        let already = ours
            .iter()
            .find(|(_, held)| *held == &path)
            .map(|(slot, _)| *slot);
        let slot = match already {
            // Known. Mounted again only if the player is not in fact serving
            // it, which is the case a stick found before the player existed
            // falls into.
            Some(slot) if serving(live, slot) => continue,
            Some(slot) => slot,
            None => {
                let Some(slot) = LOCAL_SLOTS
                    .into_iter()
                    .find(|slot| !ours.contains_key(slot))
                else {
                    // A third stick and nowhere to put it. Said once per scan
                    // rather than silently ignored: the DJ plugged something in
                    // and nothing happened.
                    tracing::info!(path, "no free slot; a CDJ has only USB and SD");
                    break;
                };
                slot
            }
        };

        // A stick going into a slot a phantom still occupies. The phantom is
        // let go first: nothing guarantees this is the same stick, so its
        // handles and its library are not the ones a consumer was reading.
        if keeping.phantom.contains(&slot) {
            keeping.release(slot);
            unmount(live, served, slot).await;
        }
        match mount(live, served, slot, &path).await {
            Ok(()) => {
                tracing::info!(%slot, path, offered = serving(live, slot), "serving a local medium");
                ours.insert(slot, path);
            }
            // Not every stick is a rekordbox export, and a DJ plugging in an
            // ordinary one has done nothing wrong. Not recorded, so it is not
            // holding a slot a real medium could use.
            Err(error) => tracing::debug!(path, "not a rekordbox medium: {error}"),
        }
    }
}

/// Whether the running player has something in a slot right now.
///
/// False when there is no player at all, which is the point: it means "this is
/// not being offered to anyone", whatever the reason.
fn serving(live: &Arc<RwLock<Option<Live>>>, slot: prolink_proto::Slot) -> bool {
    player_of(live).is_some_and(|player| player.media().get(slot).is_some())
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
    let mut mounted = client
        .mount_slot(slot)
        .await
        .map_err(|error| format!("mounting {slot}: {error}"))?;

    // A deck hands out filehandles from a table it churns, and once it has,
    // it answers NFSERR_STALE to every lookup made against the old ones. The
    // cure is to re-mount and walk again, once -- bounded, so a genuinely
    // missing file cannot loop (F28).
    let file = match client.open(&mounted, remote).await {
        Ok(file) => file,
        Err(error) if error.is_stale() => {
            mounted = client
                .refresh(mounted)
                .await
                .map_err(|error| format!("re-mounting {slot} after a stale handle: {error}"))?;
            client
                .open(&mounted, remote)
                .await
                .map_err(|error| format!("opening {remote} after a re-mount: {error}"))?
        }
        Err(error) => return Err(format!("opening {remote}: {error}")),
    };

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

    // The caller names a destination inside a tree that mirrors the medium —
    // `/Contents/<artist>/<album>/<track>.mp3`, or `/PIONEER/Artwork/000NN/` —
    // and none of those directories exist until something makes them. Without
    // this the transfer completes, the progress bar reaches 100%, and the
    // write fails with ENOENT: the track never appears and neither does a
    // cover image.
    if let Some(parent) = std::path::Path::new(local).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }
    // Written once, never incrementally: a truncated `export.pdb` parses far
    // enough to look plausible and then yields a library missing its last few
    // hundred tracks.
    std::fs::write(local, &bytes).map_err(|error| format!("writing {local}: {error}"))?;
    let _ = client.unmount(&mounted).await;
    Ok(())
}

/// Fill `local` from `remote`, head first, tail second, middle last.
#[allow(clippy::too_many_arguments)]
async fn fetch_streaming(
    interface: &Interface,
    peer: Ipv4Addr,
    slot: prolink_proto::Slot,
    remote: &str,
    local: &str,
    head_bytes: u64,
    id: u32,
    events: &Arc<Mutex<Events>>,
) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};

    /// Enough of the end to carry an M4A/MP4 `moov` atom.
    const TAIL: u64 = 256 * 1024;
    /// How much of the middle to ask for per round trip.
    const CHUNK: u64 = 1024 * 1024;

    let mut client = NfsClient::connect(peer, Some(interface))
        .await
        .map_err(|error| format!("connecting to {peer}: {error}"))?;
    let mut mounted = client
        .mount_slot(slot)
        .await
        .map_err(|error| format!("mounting {slot}: {error}"))?;

    // Same stale-handle dance as the whole-file path: a deck churns its
    // filehandle table and then answers NFSERR_STALE to everything made against
    // the old ones (F28). Re-mount and walk again, once.
    let file = match client.open(&mounted, remote).await {
        Ok(file) => file,
        Err(error) if error.is_stale() => {
            mounted = client
                .refresh(mounted)
                .await
                .map_err(|error| format!("re-mounting {slot} after a stale handle: {error}"))?;
            client
                .open(&mounted, remote)
                .await
                .map_err(|error| format!("opening {remote} after a re-mount: {error}"))?
        }
        Err(error) => return Err(format!("opening {remote}: {error}")),
    };

    if let Some(parent) = std::path::Path::new(local).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("creating {}: {error}", parent.display()))?;
    }

    let size = file.size();
    let mut handle =
        std::fs::File::create(local).map_err(|error| format!("creating {local}: {error}"))?;
    // Full size from the outset, sparse. This is what gives a decoder the right
    // duration while the bytes are still arriving.
    handle
        .set_len(size)
        .map_err(|error| format!("sizing {local}: {error}"))?;

    // `offset`/`len` describe the range that just landed; `done` is only ever a
    // fraction to show a user. A host must not infer one from the other -- the
    // head and the tail together are `done` bytes but are not the first `done`
    // bytes, and the gap between them reads back as silence rather than as an
    // error.
    let emit = |offset: u64, len: u64, done: u64| {
        let mut event = plain(EventKind::TransferProgress, 0, 0);
        event.transfer = id;
        event.offset = offset;
        event.len = len;
        event.done = done;
        event.total = size;
        if let Ok(mut queue) = events.lock() {
            queue.push(event);
        }
    };

    // The size, the moment it is known and before a single byte of content.
    //
    // This is what a host waits for, and waiting for anything more would be
    // waiting for nothing: a reader that blocks on absent ranges needs only to
    // know how long the file is, and every byte after that it can simply ask
    // for and be made to wait. So the caller is held for one open and one stat
    // rather than for a megabyte of head.
    emit(0, 0, 0);

    let write_at = |handle: &mut std::fs::File, offset: u64, bytes: &[u8]| -> Result<(), String> {
        handle
            .seek(SeekFrom::Start(offset))
            .map_err(|error| format!("seeking {local}: {error}"))?;
        handle
            .write_all(bytes)
            .map_err(|error| format!("writing {local}: {error}"))
    };

    // The order lives in prolink::consume::nfs and is tested there, exhaustively
    // and without a network. Duplicating it here would mean the tested plan and
    // the shipped one could drift, and the way that failure shows up is AAC
    // quietly not opening.
    let plan = prolink::consume::nfs::progressive_plan(size, head_bytes.min(size), TAIL, CHUNK);

    // The first step is the head, so the runway is down first. The second is
    // the tail. Everything after is the middle, in playhead order.
    //
    // Every step is announced, including the head: a range that lands without
    // being announced is a range a reader blocks on forever, which is the one
    // way this can hang rather than merely stutter.
    let mut fetched = 0u64;
    for step in &plan {
        let bytes = client
            .read_range(&file, step.offset, step.len)
            .await
            .map_err(|error| format!("reading {remote} at {}: {error}", step.offset))?;
        if bytes.is_empty() {
            return Err(format!("{remote} returned no bytes at {}", step.offset));
        }
        let landed = bytes.len() as u64;
        // Flushed before it is announced, not after the loop: the reader is
        // another process's view of this same file, and announcing a range
        // still sitting in this handle's buffer invites a read of bytes that
        // are not there yet.
        write_at(&mut handle, step.offset, &bytes)?;
        handle
            .flush()
            .map_err(|error| format!("flushing {local}: {error}"))?;
        fetched += landed;
        emit(step.offset, landed, fetched);
    }

    handle
        .flush()
        .map_err(|error| format!("flushing {local}: {error}"))?;
    let _ = client.unmount(&mounted).await;
    Ok(())
}

impl Drop for Session {
    fn drop(&mut self) {
        // Closing politely rather than dropping the sockets: a deck holds
        // dbserver state per connection, and a `DISCONNECT` is what tells it
        // to let go.
        let mut connections = std::mem::take(&mut self.connections);
        connections.close_all(&self.runtime);
    }
}

/// Somewhere to record a failure while the session is mutably borrowed.
#[derive(Debug, Clone)]
pub(crate) struct ErrorSink(Arc<Mutex<String>>);

impl ErrorSink {
    /// Record a message.
    pub(crate) fn note(&self, message: &str) {
        if let Ok(mut held) = self.0.lock() {
            message.clone_into(&mut held);
        }
    }
}

/// What a failed call tells C++.
///
/// `cxx` turns this into an exception carrying [`Display`](std::fmt::Display),
/// so the message is what a host shows the user.
#[derive(Debug)]
pub struct Error(String);

impl Error {
    /// One with the given message.
    pub(crate) fn new(message: String) -> Self {
        Self(message)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
