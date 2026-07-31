// SPDX-License-Identifier: GPL-3.0-only

//! The boundary: the `extern "C"` entry points and the session behind them.
//!
//! This is the only file in the workspace that contains `unsafe`, and it is
//! confined to three things: turning a caller's pointer into a reference,
//! writing through an out-pointer, and handing back a `'static` C string. Each
//! is spelled out at the site with what the caller must guarantee.

#![allow(
    unsafe_code,
    reason = "this file *is* the C ABI: `extern \"C\"` entry points and the \
              pointer handling behind them cannot be written without it. The \
              crate denies unsafe so that no other module acquires any, and \
              every block below carries a SAFETY note naming what the caller \
              must guarantee."
)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use prolink::{Discovery, Interface, Monitor, VirtualCdj, VirtualCdjConfig};
use tokio::runtime::Runtime;

use crate::convert;
use crate::types::{
    ProlinkDevice, ProlinkEvent, ProlinkEventKind, ProlinkInterface, ProlinkPlayer, ProlinkSlot,
    ProlinkStatus,
};

/// How many events a session queues before it starts discarding the oldest.
///
/// A host polling on a UI timer at 60 Hz sees at most a handful per poll; this
/// is two seconds of a busy four-deck network, so it only bites when the host
/// has stopped polling altogether. Discarding the *oldest* keeps the queue
/// current rather than stale, and the count is reported so the host knows to
/// re-read the tables (see [`ProlinkEvent::dropped`]).
const EVENT_QUEUE: usize = 512;

thread_local! {
    /// The last error, per thread, so two threads cannot overwrite each other's.
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// Record an error for [`prolink_last_error`].
fn set_error(message: &str) {
    // A NUL in the middle would truncate the message; replace rather than drop
    // it, since this is a diagnostic and losing it entirely is worse.
    let cleaned = message.replace('\0', "?");
    let text = CString::new(cleaned).unwrap_or_default();
    LAST_ERROR.with(|slot| *slot.borrow_mut() = text);
}

/// Run an entry point, turning a panic into [`ProlinkStatus::Panic`].
fn guard(body: impl FnOnce() -> ProlinkStatus) -> ProlinkStatus {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|_| {
        set_error("a panic was caught at the FFI boundary; this is a bug in prolink");
        ProlinkStatus::Panic
    })
}

/// How to start a session.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProlinkConfig {
    /// The interface to use, NUL-terminated UTF-8, or all-zero to choose one.
    ///
    /// Choosing automatically prefers a link-local address, which is what a DJ
    /// network always is; on a machine with two candidates it is a coin toss,
    /// so a host with a settings screen should name one.
    pub interface: [u8; crate::PROLINK_NAME_LEN],

    /// Whether to announce as a virtual CDJ.
    ///
    /// **Off means the loaded track, the play state and the tempo master are
    /// never populated**, because a player unicasts status to peers that have
    /// announced themselves and to nobody else (F21). Tempo and beat phase
    /// still arrive, since beats are broadcast.
    pub announce: bool,

    /// The device number to announce as, or zero to take one outside the 1–6
    /// player range.
    ///
    /// Zero is the safe choice and cannot collide with hardware. A number in
    /// 1–4 is required only to *browse* a player's library (F45), and taking
    /// one contends with the decks for it.
    pub device_number: u8,
}

/// A running session: sockets, timers, and the state they maintain.
///
/// Opaque to C. Created by [`prolink_open`] and destroyed by [`prolink_close`].
#[derive(Debug)]
pub struct ProlinkSession {
    /// Dropped last, because the tasks below are running on it.
    runtime: Runtime,
    monitor: Monitor,
    discovery: Discovery,
    /// Held for as long as the session announces: dropping it releases the
    /// device number, so it must outlive the monitor that depends on peers
    /// unicasting status to us (F21).
    cdj: Option<VirtualCdj>,
    events: Arc<Mutex<Events>>,
    /// Hands out transfer ids. Starts at 1, so zero always means "not a
    /// transfer" in an event.
    next_transfer: AtomicU32,
}

/// The polled event queue.
#[derive(Debug, Default)]
struct Events {
    queue: VecDeque<ProlinkEvent>,
    /// Discarded since the last one the host took.
    dropped: u32,
}

impl Events {
    fn push(&mut self, event: ProlinkEvent) {
        if self.queue.len() >= EVENT_QUEUE {
            self.queue.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.queue.push_back(event);
    }

    fn pop(&mut self) -> Option<ProlinkEvent> {
        let mut event = self.queue.pop_front()?;
        event.dropped = std::mem::take(&mut self.dropped);
        Some(event)
    }
}

/// The library's version, as a NUL-terminated string.
///
/// Valid for the life of the process and must not be freed.
#[unsafe(no_mangle)]
pub extern "C" fn prolink_version() -> *const c_char {
    // SAFETY: the bytes are a literal with a trailing NUL and no interior one,
    // and they live in static storage, so the pointer is valid for ever.
    const VERSION: &CStr = c"0.1.0";
    VERSION.as_ptr()
}

/// The last error on this thread, as a NUL-terminated string.
///
/// Never null. Valid until the next call on this thread, so a host that keeps
/// it must copy it.
#[unsafe(no_mangle)]
pub extern "C" fn prolink_last_error() -> *const c_char {
    LAST_ERROR.with(|slot| slot.borrow().as_ptr())
}

/// Fill `config` with defaults: choose an interface, announce, take a
/// non-colliding number.
///
/// # Safety
///
/// `config` must be a valid, writable `ProlinkConfig`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_config_default(config: *mut ProlinkConfig) -> ProlinkStatus {
    guard(|| {
        if config.is_null() {
            set_error("prolink_config_default: config is null");
            return ProlinkStatus::InvalidArgument;
        }
        // SAFETY: the caller guarantees `config` points at a writable
        // `ProlinkConfig`; it is not read first, so it need not be initialised.
        unsafe {
            config.write(ProlinkConfig {
                interface: [0; crate::PROLINK_NAME_LEN],
                announce: true,
                device_number: 0,
            });
        }
        ProlinkStatus::Ok
    })
}

/// How many interfaces could carry Pro DJ Link traffic.
#[unsafe(no_mangle)]
pub extern "C" fn prolink_interface_count() -> i32 {
    let count = catch_unwind(|| Interface::list().map_or(0, |found| found.len())).unwrap_or(0);
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// Copy up to `capacity` interfaces into `out`. Returns how many were written,
/// or a negative [`ProlinkStatus`].
///
/// # Safety
///
/// `out` must point at `capacity` writable `ProlinkInterface`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_interfaces(out: *mut ProlinkInterface, capacity: i32) -> i32 {
    let mut written = 0;
    let status = guard(|| {
        let Ok(capacity) = usize::try_from(capacity) else {
            set_error("prolink_interfaces: capacity is negative");
            return ProlinkStatus::InvalidArgument;
        };
        if out.is_null() && capacity > 0 {
            set_error("prolink_interfaces: out is null");
            return ProlinkStatus::InvalidArgument;
        }
        let found = Interface::list().unwrap_or_default();
        for (index, found) in found.iter().take(capacity).enumerate() {
            // SAFETY: `index` is below `capacity`, and the caller guarantees
            // `out` points at that many writable `ProlinkInterface`s.
            unsafe { out.add(index).write(convert::interface(found)) };
            written += 1;
        }
        ProlinkStatus::Ok
    });
    if status == ProlinkStatus::Ok {
        written
    } else {
        status as i32
    }
}

/// Start a session. Writes the handle to `out` on success.
///
/// # Safety
///
/// `config` must point at a valid `ProlinkConfig` and `out` at a writable
/// pointer. The handle must be released with [`prolink_close`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_open(
    config: *const ProlinkConfig,
    out: *mut *mut ProlinkSession,
) -> ProlinkStatus {
    guard(|| {
        if config.is_null() || out.is_null() {
            set_error("prolink_open: config or out is null");
            return ProlinkStatus::InvalidArgument;
        }
        // SAFETY: the caller guarantees `config` points at an initialised
        // `ProlinkConfig`, which is `Copy` and has no invalid bit patterns.
        let config = unsafe { *config };

        let Ok(name) = c_string(&config.interface) else {
            set_error("prolink_open: the interface name is not valid UTF-8");
            return ProlinkStatus::InvalidArgument;
        };
        let interface = match pick_interface(name.as_deref()) {
            Ok(interface) => interface,
            Err(status) => return status,
        };

        match open(&interface, config) {
            Ok(session) => {
                let boxed = Box::into_raw(Box::new(session));
                // SAFETY: the caller guarantees `out` is a writable pointer
                // slot; `boxed` is a fresh allocation this call owns.
                unsafe { out.write(boxed) };
                ProlinkStatus::Ok
            }
            Err(status) => status,
        }
    })
}

/// Stop a session and release everything it holds, including its device number.
///
/// Null is accepted and does nothing, so a host may close unconditionally.
///
/// # Safety
///
/// `session` must be a handle from [`prolink_open`] that has not been closed.
/// It must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_close(session: *mut ProlinkSession) {
    let _ = guard(|| {
        if session.is_null() {
            return ProlinkStatus::Ok;
        }
        // SAFETY: the caller guarantees this came from `prolink_open` and has
        // not been closed, so reclaiming the Box is sound and happens once.
        // Dropping it stops the tasks and then the runtime they ran on.
        drop(unsafe { Box::from_raw(session) });
        ProlinkStatus::Ok
    });
}

/// The device number we announced as, or zero if we did not announce.
///
/// # Safety
///
/// `session` must be a live handle from [`prolink_open`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_device_number(session: *const ProlinkSession) -> u8 {
    // SAFETY: the caller guarantees the handle is live.
    let Some(session) = (unsafe { session.as_ref() }) else {
        return 0;
    };
    session.cdj.as_ref().map_or(0, |cdj| cdj.number().get())
}

/// Copy up to `capacity` devices into `out`. Returns how many were written.
///
/// # Safety
///
/// `session` must be live and `out` must point at `capacity` writable
/// `ProlinkDevice`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_devices(
    session: *const ProlinkSession,
    out: *mut ProlinkDevice,
    capacity: i32,
) -> i32 {
    // SAFETY: the caller guarantees the handle is live and `out` has room for
    // `capacity` items.
    unsafe {
        copy_out(session, out, capacity, |session| {
            session.discovery.devices()
        })
    }
    .unwrap_or_else(|status| status as i32)
}

/// Copy up to `capacity` players into `out`. Returns how many were written.
///
/// # Safety
///
/// `session` must be live and `out` must point at `capacity` writable
/// `ProlinkPlayer`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_players(
    session: *const ProlinkSession,
    out: *mut ProlinkPlayer,
    capacity: i32,
) -> i32 {
    // SAFETY: as above.
    unsafe { copy_out(session, out, capacity, |session| session.monitor.players()) }
        .unwrap_or_else(|status| status as i32)
}

/// Take the next event, or return false if there are none.
///
/// A host should drain this in a loop from its own event loop. Nothing is
/// pushed from a network thread.
///
/// # Safety
///
/// `session` must be live and `out` must point at a writable `ProlinkEvent`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_next_event(
    session: *const ProlinkSession,
    out: *mut ProlinkEvent,
) -> bool {
    let mut got = false;
    let _ = guard(|| {
        // SAFETY: the caller guarantees the handle is live.
        let Some(session) = (unsafe { session.as_ref() }) else {
            set_error("prolink_next_event: session is null");
            return ProlinkStatus::InvalidArgument;
        };
        if out.is_null() {
            set_error("prolink_next_event: out is null");
            return ProlinkStatus::InvalidArgument;
        }
        let Ok(mut events) = session.events.lock() else {
            set_error("prolink_next_event: the event queue is poisoned");
            return ProlinkStatus::Internal;
        };
        if let Some(event) = events.pop() {
            // SAFETY: the caller guarantees `out` points at a writable
            // `ProlinkEvent`; it is not read first.
            unsafe { out.write(event) };
            got = true;
        }
        ProlinkStatus::Ok
    });
    got
}

// -- file transfer ---------------------------------------------------------

/// Fetch one file from a player's medium into `local_path`.
///
/// Returns a transfer id, or a negative [`ProlinkStatus`]. The transfer runs on
/// the session's runtime; progress arrives as
/// [`ProlinkEventKind::TransferProgress`] events carrying that id, and it ends
/// with exactly one [`ProlinkEventKind::TransferDone`].
///
/// `remote_path` is taken verbatim from `export.pdb`, which stores paths
/// relative to the medium root with a leading slash.
///
/// **Nothing partial is ever written.** The file is assembled in memory and
/// written once, because a truncated `export.pdb` parses far enough to look
/// plausible and then yields a library missing its last few hundred tracks.
///
/// # Safety
///
/// `session` must be live. `remote_path` and `local_path` must be
/// NUL-terminated UTF-8 and are copied before this returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_fetch_file(
    session: *const ProlinkSession,
    device_number: u8,
    slot: ProlinkSlot,
    remote_path: *const c_char,
    local_path: *const c_char,
) -> i32 {
    let mut id = 0i32;
    let status = guard(|| {
        // SAFETY: the caller guarantees the handle is live.
        let Some(session) = (unsafe { session.as_ref() }) else {
            set_error("prolink_fetch_file: session is null");
            return ProlinkStatus::InvalidArgument;
        };
        // SAFETY: the caller guarantees both are NUL-terminated UTF-8.
        let (Some(remote), Some(local)) = (unsafe { borrow(remote_path) }, unsafe {
            borrow(local_path)
        }) else {
            set_error("prolink_fetch_file: a path is null or not valid UTF-8");
            return ProlinkStatus::InvalidArgument;
        };
        let Some(peer) = session.address_of(device_number) else {
            set_error(&format!("prolink_fetch_file: no device {device_number}"));
            return ProlinkStatus::InvalidArgument;
        };
        match i32::try_from(session.spawn_fetch(peer, slot, remote, local)) {
            Ok(handed) => {
                id = handed;
                ProlinkStatus::Ok
            }
            Err(_) => ProlinkStatus::Internal,
        }
    });
    if status == ProlinkStatus::Ok {
        id
    } else {
        status as i32
    }
}

/// Fetch a player's `export.pdb`, the database a browse is built from.
///
/// The same contract as [`prolink_fetch_file`]; this is the path a host does
/// not have to know.
///
/// # Safety
///
/// As [`prolink_fetch_file`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn prolink_fetch_database(
    session: *const ProlinkSession,
    device_number: u8,
    slot: ProlinkSlot,
    local_path: *const c_char,
) -> i32 {
    let export = c"/PIONEER/rekordbox/export.pdb";
    // SAFETY: the caller guarantees `session` is live and `local_path` is a
    // NUL-terminated string; `export` is a literal with static storage.
    unsafe { prolink_fetch_file(session, device_number, slot, export.as_ptr(), local_path) }
}

/// Borrow a C string as `&str`, or `None` if it is null or not UTF-8.
///
/// # Safety
///
/// `text` must be null or point at a NUL-terminated string that outlives the
/// borrow.
unsafe fn borrow<'a>(text: *const c_char) -> Option<&'a str> {
    if text.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees `text` is NUL-terminated and outlives the
    // returned borrow, which is bounded by the caller's own lifetime.
    unsafe { CStr::from_ptr(text) }.to_str().ok()
}

impl ProlinkSession {
    /// The address of a device by number, if it is on the network.
    fn address_of(&self, number: u8) -> Option<std::net::Ipv4Addr> {
        self.discovery
            .devices()
            .into_iter()
            .find(|device| device.number.get() == number)
            .map(|device| device.ip)
    }

    /// Start a transfer and return its id.
    fn spawn_fetch(
        &self,
        peer: std::net::Ipv4Addr,
        slot: ProlinkSlot,
        remote: &str,
        local: &str,
    ) -> u32 {
        let id = self.next_transfer.fetch_add(1, Ordering::Relaxed);
        let events = Arc::clone(&self.events);
        let interface = self.monitor.interface().clone();
        let (remote, local) = (remote.to_owned(), local.to_owned());
        let slot = match slot {
            ProlinkSlot::Sd => prolink_proto::Slot::SD,
            ProlinkSlot::Cd => prolink_proto::Slot::CD,
            _ => prolink_proto::Slot::USB,
        };

        self.runtime.spawn(async move {
            let outcome = fetch(&interface, peer, slot, &remote, &local, id, &events).await;
            let (status, message) = match outcome {
                Ok(()) => (ProlinkStatus::Ok, None),
                Err(error) => (ProlinkStatus::Internal, Some(error)),
            };
            if let Some(message) = message {
                tracing::warn!(%peer, remote, "transfer failed: {message}");
            }
            if let Ok(mut queue) = events.lock() {
                queue.push(ProlinkEvent {
                    kind: ProlinkEventKind::TransferDone,
                    device: 0,
                    beat_in_bar: 0,
                    dropped: 0,
                    transfer: id,
                    done: 0,
                    total: 0,
                    status,
                });
            }
        });
        id
    }
}

/// One transfer, start to finish.
async fn fetch(
    interface: &Interface,
    peer: std::net::Ipv4Addr,
    slot: prolink_proto::Slot,
    remote: &str,
    local: &str,
    id: u32,
    events: &Arc<Mutex<Events>>,
) -> Result<(), String> {
    use prolink::consume::NfsClient;

    let mut client = NfsClient::connect(peer, Some(interface))
        .await
        .map_err(|error| format!("connecting to {peer}: {error}"))?;
    let mounted = client
        .mount_slot(slot)
        .await
        .map_err(|error| format!("mounting {slot:?}: {error}"))?;
    let file = client
        .open(&mounted, remote)
        .await
        .map_err(|error| format!("opening {remote}: {error}"))?;
    let bytes = client
        .read_file_with(&file, |progress| {
            if let Ok(mut queue) = events.lock() {
                queue.push(ProlinkEvent {
                    kind: ProlinkEventKind::TransferProgress,
                    device: 0,
                    beat_in_bar: 0,
                    dropped: 0,
                    transfer: id,
                    done: progress.read,
                    total: progress.total,
                    status: ProlinkStatus::Ok,
                });
            }
        })
        .await
        .map_err(|error| format!("reading {remote}: {error}"))?;
    // Written once, never incrementally: see `prolink_fetch_file`.
    std::fs::write(local, &bytes).map_err(|error| format!("writing {local}: {error}"))?;
    let _ = client.unmount(&mounted).await;
    Ok(())
}

// -- the safe half ---------------------------------------------------------

/// Read a NUL-terminated name out of a fixed buffer, or `None` if it is empty.
fn c_string(buffer: &[u8]) -> Result<Option<String>, std::str::Utf8Error> {
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    let text = std::str::from_utf8(buffer.get(..end).unwrap_or_default())?;
    Ok((!text.is_empty()).then(|| text.to_owned()))
}

/// The interface a config names, or the best guess.
fn pick_interface(name: Option<&str>) -> Result<Interface, ProlinkStatus> {
    match name {
        Some(name) => Interface::named(name).map_err(|error| {
            set_error(&format!("no interface named {name}: {error}"));
            ProlinkStatus::NoInterface
        }),
        None => Interface::best_guess().map_err(|error| {
            set_error(&format!("no interface with an IPv4 address: {error}"));
            ProlinkStatus::NoInterface
        }),
    }
}

/// Start everything a session owns.
fn open(interface: &Interface, config: ProlinkConfig) -> Result<ProlinkSession, ProlinkStatus> {
    let runtime = Runtime::new().map_err(|error| {
        set_error(&format!("could not start a runtime: {error}"));
        ProlinkStatus::Internal
    })?;

    let events = Arc::new(Mutex::new(Events::default()));
    let session = runtime.block_on(async {
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

    let (discovery, cdj, monitor) = session.map_err(|error| {
        set_error(&format!("could not start: {error}"));
        ProlinkStatus::Bind
    })?;

    // One task draining the monitor into the polled queue. It ends when the
    // session is dropped, because the broadcast sender goes with it.
    let sink = Arc::clone(&events);
    let mut incoming = monitor.subscribe();
    runtime.spawn(async move {
        while let Ok(event) = incoming.recv().await {
            if let Ok(mut queue) = sink.lock() {
                queue.push(convert::event(&event));
            }
        }
    });

    Ok(ProlinkSession {
        runtime,
        monitor,
        discovery,
        cdj,
        events,
        next_transfer: AtomicU32::new(1),
    })
}

/// The shared body of [`prolink_devices`] and [`prolink_players`].
///
/// # Safety
///
/// `session` must be live, and `out` must point at `capacity` writable `T`s.
unsafe fn copy_out<T, S>(
    session: *const ProlinkSession,
    out: *mut T,
    capacity: i32,
    read: impl FnOnce(&ProlinkSession) -> Vec<S>,
) -> Result<i32, ProlinkStatus>
where
    T: Copy,
    S: Into<T>,
{
    let mut written = 0i32;
    let status = guard(|| {
        // SAFETY: the caller guarantees the handle is live.
        let Some(session) = (unsafe { session.as_ref() }) else {
            set_error("session is null");
            return ProlinkStatus::InvalidArgument;
        };
        let Ok(capacity) = usize::try_from(capacity) else {
            set_error("capacity is negative");
            return ProlinkStatus::InvalidArgument;
        };
        if out.is_null() && capacity > 0 {
            set_error("out is null");
            return ProlinkStatus::InvalidArgument;
        }
        for (index, item) in read(session).into_iter().take(capacity).enumerate() {
            // SAFETY: `index` is below `capacity`, and the caller guarantees
            // `out` has room for that many `T`s.
            unsafe { out.add(index).write(item.into()) };
            written = written.saturating_add(1);
        }
        ProlinkStatus::Ok
    });
    if status == ProlinkStatus::Ok {
        Ok(written)
    } else {
        Err(status)
    }
}

impl From<prolink::Device> for ProlinkDevice {
    fn from(from: prolink::Device) -> Self {
        convert::device(&from)
    }
}

impl From<prolink::PlayerState> for ProlinkPlayer {
    fn from(from: prolink::PlayerState) -> Self {
        convert::player(&from)
    }
}
