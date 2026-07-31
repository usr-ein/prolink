// SPDX-License-Identifier: GPL-3.0-only

//! The C-visible types.
//!
//! Every string field is a fixed-size, NUL-padded UTF-8 buffer rather than a
//! pointer, so a caller may keep any struct here indefinitely and nothing it
//! receives ever needs freeing. The sizes are the wire's: a device name is 20
//! bytes on UDP 50000 and an IPv4 address is fifteen characters.

/// Bytes reserved for a device name. 20 on the wire, plus a NUL.
pub const PROLINK_NAME_LEN: usize = 24;
/// Bytes reserved for an IPv4 address in dotted form, plus a NUL.
pub const PROLINK_ADDRESS_LEN: usize = 16;
/// Bytes reserved for a track title or artist. Titles are long; this is what
/// fits on a deck's screen twice over.
pub const PROLINK_TEXT_LEN: usize = 128;

/// The result of a call.
///
/// Negative values are failures. A caller that only checks `!= PROLINK_OK` is
/// correct; the distinctions exist so a host can tell "the user has no network"
/// from "this is a bug".
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProlinkStatus {
    /// The call succeeded.
    Ok = 0,
    /// A pointer argument was null, or a count was negative.
    InvalidArgument = -1,
    /// No interface matched, or the one named has no usable address.
    NoInterface = -2,
    /// A socket could not be bound. Usually a privileged port without root, or
    /// another Pro DJ Link program already running.
    Bind = -3,
    /// Every device number in 1–4 is taken, so we cannot be browsed (F45).
    NoDeviceNumber = -4,
    /// The medium could not be read as a rekordbox export.
    BadMedium = -5,
    /// Something failed that the caller cannot act on. See
    /// [`crate::prolink_last_error`].
    Internal = -6,
    /// A panic was caught at the boundary. Always a bug in this library.
    Panic = -7,
}

/// What a device says it is.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProlinkDeviceKind {
    /// Anything we have no name for.
    Unknown = 0,
    /// A player.
    Cdj = 1,
    /// A mixer.
    Mixer = 2,
    /// rekordbox on a computer or phone.
    Rekordbox = 3,
}

/// Which slot a track came from.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProlinkSlot {
    /// Nothing.
    None = 0,
    /// The CD drive.
    Cd = 1,
    /// The SD card.
    Sd = 2,
    /// The USB slot.
    Usb = 3,
    /// Another player's media, over LINK.
    Rekordbox = 4,
}

/// What a platter is doing. The wire's own byte, so a value this enumeration
/// does not name still arrives intact.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProlinkPlayState {
    /// Nothing loaded.
    NoTrack = 0x00,
    /// Loading.
    Loading = 0x02,
    /// Playing.
    Playing = 0x03,
    /// Playing inside a loop.
    Looping = 0x04,
    /// Paused.
    Paused = 0x05,
    /// Stopped at the cue point.
    Cued = 0x06,
    /// Playing from the cue point while the button is held.
    CuePlay = 0x07,
    /// Searching.
    Searching = 0x09,
    /// Spun down.
    SpunDown = 0x0e,
    /// The medium went away mid-play and the deck is looping its buffer.
    Emergency = 0x12,
    /// A value this library has never seen.
    Other = 0xff,
}

/// A device on the network.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProlinkDevice {
    /// 1–6 for a player, higher for other things. Zero means "not yet known".
    pub number: u8,
    /// What kind of device it says it is.
    pub kind: ProlinkDeviceKind,
    /// Whether it is a player, and so something that can be browsed.
    pub is_player: bool,
    /// Whether it is still sending keep-alives.
    pub online: bool,
    /// Its hardware address.
    pub mac: [u8; 6],
    /// Its name, NUL-padded UTF-8. `CDJ-2000nexus` and the like.
    pub name: [u8; PROLINK_NAME_LEN],
    /// Its IPv4 address in dotted form, NUL-padded.
    pub address: [u8; PROLINK_ADDRESS_LEN],
    /// Milliseconds since its last keep-alive.
    pub last_seen_ms: u64,
}

/// What one player is doing, as a host would render it.
///
/// Fields that need a status packet — the loaded track, the play state, the
/// tempo master — are only populated when the session announced itself, since
/// status is unicast to announced peers and to nobody else (F21). Without that,
/// `has_status` is false and those fields are inert rather than zero-and-lying.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProlinkPlayer {
    /// The player's number.
    pub number: u8,
    /// Its name, NUL-padded UTF-8.
    pub name: [u8; PROLINK_NAME_LEN],

    /// Whether anything below the beat fields is meaningful.
    pub has_status: bool,
    /// Whether it is sending beat packets, and so playing a rekordbox track.
    pub is_beating: bool,

    /// The tempo actually playing: the track's BPM with the pitch applied.
    /// Negative when unknown.
    pub effective_bpm: f64,
    /// The track's own BPM, before pitch. Negative when unknown.
    pub track_bpm: f64,
    /// The pitch fader as a percentage. Zero at centre.
    ///
    /// **Not the fader position while [`Self::is_synced`]**: a synced deck
    /// slews this to hold its effective tempo equal to the master's (F51).
    pub pitch_percent: f64,

    /// Position within the current beat, `0.0` on the beat. Negative when the
    /// player is not beating or the estimate has gone stale.
    pub beat_phase: f64,
    /// Position within the four-beat bar, `0.0` on the downbeat. Negative when
    /// unknown.
    pub bar_phase: f64,
    /// Which beat of the bar, 1–4. Zero when the player has no bar to be in,
    /// which is what a track rekordbox has not analysed reports.
    pub beat_in_bar: u8,

    /// Whether this player holds tempo master.
    pub is_master: bool,
    /// Whether SYNC is lit.
    pub is_synced: bool,
    /// The device it is handing master to, or zero.
    ///
    /// Non-zero for only the one or two packets of a handoff, during which
    /// **both** decks report themselves master; a host that ignores this will
    /// see mastership flicker (F52).
    pub yielding_to: u8,

    /// What the platter is doing.
    pub play_state: ProlinkPlayState,
    /// The loaded track's row id, or zero.
    pub track_id: u32,
    /// Which player the loaded track came from.
    pub track_source_player: u8,
    /// Which of that player's slots.
    pub track_source_slot: ProlinkSlot,
}

/// A network interface that could carry Pro DJ Link traffic.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProlinkInterface {
    /// The system's name for it, NUL-padded.
    pub name: [u8; PROLINK_NAME_LEN],
    /// Its IPv4 address, dotted and NUL-padded.
    pub address: [u8; PROLINK_ADDRESS_LEN],
    /// Its broadcast address, dotted and NUL-padded.
    pub broadcast: [u8; PROLINK_ADDRESS_LEN],
    /// Whether the address is link-local, which a DJ network's always is.
    ///
    /// A host choosing an interface automatically should prefer these: a CDJ
    /// self-assigns in `169.254.0.0/16` after DHCP fails (F8).
    pub is_link_local: bool,
}

/// What happened.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProlinkEventKind {
    /// A device appeared.
    DeviceFound = 1,
    /// A device's details changed.
    DeviceChanged = 2,
    /// A device stopped sending keep-alives.
    DeviceLost = 3,
    /// A player started a beat. `beat_in_bar` says which.
    Beat = 4,
    /// A player's status changed: what is loaded, or what the platter is doing.
    PlayerChanged = 5,
    /// Tempo master moved. `device` is the new master, or zero for nobody.
    TempoMaster = 6,
    /// A player stopped sending beats.
    Stopped = 7,
    /// A file transfer advanced. `transfer`, `done` and `total` are set.
    ///
    /// Emitted once per NFS reply — 128 times for a 1 MB database at the read
    /// size a CDJ uses — which is frequent enough to drive a progress bar and
    /// cheap enough to ignore.
    TransferProgress = 8,
    /// A file transfer finished. `status` says whether it succeeded, and on
    /// success the bytes are at the local path the caller gave.
    TransferDone = 9,
}

/// One thing that happened, as [`crate::prolink_next_event`] reports it.
///
/// Deliberately flat: a host switches on `kind` and reads the two or three
/// fields that kind gives meaning to, rather than unpacking a union.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ProlinkEvent {
    /// What happened.
    pub kind: ProlinkEventKind,
    /// Which device, or zero where the event is not about one.
    pub device: u8,
    /// Which beat of the bar, for [`ProlinkEventKind::Beat`]. Zero otherwise.
    pub beat_in_bar: u8,
    /// How many events were discarded before this one because the host did not
    /// poll fast enough.
    ///
    /// Non-zero means state was missed and the host should re-read the device
    /// and player tables rather than trust its incremental picture.
    pub dropped: u32,

    /// Which transfer, for the two transfer kinds. Zero otherwise.
    ///
    /// The id [`crate::prolink_fetch_file`] returned, so a host with several
    /// downloads in flight can tell them apart without matching on paths.
    pub transfer: u32,
    /// Bytes transferred so far, contiguous from the start of the file.
    ///
    /// Contiguous rather than a running total, so it can never exceed what is
    /// safely on disk.
    pub done: u64,
    /// Bytes the file holds, from the lookup that opened it. Zero if unknown.
    pub total: u64,
    /// For [`ProlinkEventKind::TransferDone`], how it ended.
    pub status: ProlinkStatus,
}
