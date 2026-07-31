// SPDX-License-Identifier: GPL-3.0-only

//! Turning the library's types into the C-visible ones.
//!
//! All of it is safe Rust: the boundary itself is in [`crate::session`], and
//! keeping the conversions here means the part that has to be reviewed for
//! soundness is small enough to read in one sitting.

use prolink::monitor::PlayState;
use prolink::{Device, PlayerState};
use prolink_proto::{DeviceKind, Slot};

use crate::types::{
    PROLINK_ADDRESS_LEN, PROLINK_NAME_LEN, ProlinkDevice, ProlinkDeviceKind, ProlinkEvent,
    ProlinkEventKind, ProlinkInterface, ProlinkPlayState, ProlinkPlayer, ProlinkSlot,
};

/// Copy a string into a fixed NUL-padded buffer, truncating on a character
/// boundary so the result is always valid UTF-8.
pub(crate) fn fill<const N: usize>(text: &str) -> [u8; N] {
    let mut out = [0u8; N];
    // Leave at least one NUL, so a caller may treat the buffer as a C string.
    let room = N.saturating_sub(1);
    let end = text
        .char_indices()
        .map(|(at, ch)| at + ch.len_utf8())
        .take_while(|end| *end <= room)
        .last()
        .unwrap_or(0);
    if let (Some(slot), Some(source)) = (out.get_mut(..end), text.get(..end)) {
        slot.copy_from_slice(source.as_bytes());
    }
    out
}

impl From<DeviceKind> for ProlinkDeviceKind {
    fn from(kind: DeviceKind) -> Self {
        match kind {
            DeviceKind::CDJ => Self::Cdj,
            DeviceKind::MIXER => Self::Mixer,
            DeviceKind::REKORDBOX_OR_CDJ3000 => Self::Rekordbox,
            _ => Self::Unknown,
        }
    }
}

impl From<Slot> for ProlinkSlot {
    fn from(slot: Slot) -> Self {
        match slot {
            Slot::CD => Self::Cd,
            Slot::SD => Self::Sd,
            Slot::USB => Self::Usb,
            Slot::REKORDBOX => Self::Rekordbox,
            _ => Self::None,
        }
    }
}

impl From<PlayState> for ProlinkPlayState {
    fn from(state: PlayState) -> Self {
        match state {
            PlayState::NO_TRACK => Self::NoTrack,
            PlayState::LOADING => Self::Loading,
            PlayState::PLAYING => Self::Playing,
            PlayState::LOOPING => Self::Looping,
            PlayState::PAUSED => Self::Paused,
            PlayState::CUED => Self::Cued,
            PlayState::CUE_PLAY => Self::CuePlay,
            PlayState::SEARCHING => Self::Searching,
            PlayState::SPUN_DOWN => Self::SpunDown,
            PlayState::EMERGENCY_LOOP => Self::Emergency,
            _ => Self::Other,
        }
    }
}

/// A device, as C sees it.
pub(crate) fn device(device: &Device) -> ProlinkDevice {
    ProlinkDevice {
        number: device.number.get(),
        kind: device.kind.into(),
        is_player: device.number.is_player(),
        online: !device.offline,
        mac: device.mac.0,
        name: fill::<PROLINK_NAME_LEN>(&device.name.as_str()),
        address: fill::<PROLINK_ADDRESS_LEN>(&device.ip.to_string()),
        last_seen_ms: u64::try_from(device.last_seen.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

/// A player's live state, as C sees it.
///
/// Absent numbers become negative rather than zero: a host drawing a tempo has
/// to be able to tell "not playing" from "0.00 BPM", and zero cannot.
pub(crate) fn player(player: &PlayerState) -> ProlinkPlayer {
    let track = player.track();
    let status = player.status.map(|observed| observed.status);
    ProlinkPlayer {
        number: player.device.get(),
        name: fill::<PROLINK_NAME_LEN>(&player.name.as_str()),

        has_status: player.play_state().is_some(),
        is_beating: player.is_beating(),

        effective_bpm: player.effective_bpm().unwrap_or(-1.0),
        // From the status packet where there is one, since it is published
        // while the deck is paused and a beat packet is not.
        track_bpm: status
            .and_then(|status| status.bpm_centi)
            .map_or(-1.0, |centi| f64::from(centi) / 100.0),
        pitch_percent: status
            .and_then(|status| status.pitch)
            .map_or(0.0, prolink::Pitch::percent),

        beat_phase: player.beat_phase().unwrap_or(-1.0),
        bar_phase: player.bar_phase().unwrap_or(-1.0),
        beat_in_bar: player.beat_in_bar().map_or(0, prolink::BeatInBar::get),

        is_master: player.is_tempo_master().unwrap_or(false),
        is_synced: player.is_synced().unwrap_or(false),
        yielding_to: player.yielding_to().map_or(0, prolink::DeviceNumber::get),

        play_state: player
            .play_state()
            .map_or(ProlinkPlayState::NoTrack, Into::into),
        track_id: track.map_or(0, |track| track.id),
        track_source_player: track.map_or(0, |track| track.source_player.get()),
        track_source_slot: track.map_or(ProlinkSlot::None, |track| track.slot.into()),
    }
}

/// An interface, as C sees it.
pub(crate) fn interface(interface: &prolink::Interface) -> ProlinkInterface {
    ProlinkInterface {
        name: fill::<PROLINK_NAME_LEN>(&interface.name),
        address: fill::<PROLINK_ADDRESS_LEN>(&interface.ip.to_string()),
        broadcast: fill::<PROLINK_ADDRESS_LEN>(&interface.broadcast().to_string()),
        is_link_local: interface.is_link_local(),
    }
}

/// A monitor event, as C sees it, or `None` for one C has no shape for.
pub(crate) fn event(event: &prolink::MonitorEvent) -> ProlinkEvent {
    let flat = |kind, device: u8, beat_in_bar: u8| ProlinkEvent {
        kind,
        device,
        beat_in_bar,
        dropped: 0,
        transfer: 0,
        done: 0,
        total: 0,
        status: crate::ProlinkStatus::Ok,
    };
    match event {
        prolink::MonitorEvent::Beat(state) => flat(
            ProlinkEventKind::Beat,
            state.device.get(),
            state.beat_in_bar().map_or(0, prolink::BeatInBar::get),
        ),
        prolink::MonitorEvent::Status(state) => {
            flat(ProlinkEventKind::PlayerChanged, state.device.get(), 0)
        }
        prolink::MonitorEvent::Stopped(device) => flat(ProlinkEventKind::Stopped, device.get(), 0),
        prolink::MonitorEvent::TempoMaster(master) => flat(
            ProlinkEventKind::TempoMaster,
            master.map_or(0, prolink::DeviceNumber::get),
            0,
        ),
        prolink::MonitorEvent::Gone(device) => flat(ProlinkEventKind::DeviceLost, device.get(), 0),
    }
}
