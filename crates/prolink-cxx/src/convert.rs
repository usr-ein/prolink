// SPDX-License-Identifier: GPL-3.0-only

//! Turning the library's types into the ones the bridge shares with C++.

use prolink::monitor::PlayState as LibState;
use prolink_proto::{DeviceKind as LibKind, Slot as LibSlot};

use crate::ffi::{
    Device, DeviceKind, Event, EventKind, MediaInfo, Metadata, NetworkInterface, PlayState, Player,
    Row, Slot,
};

/// An absent number, for a field C++ sees as a plain `double`.
///
/// A host drawing a tempo has to tell "not playing" from "0.00 BPM", and zero
/// cannot; `cxx` has no `Option` in a shared struct, so the convention is
/// explicit and used everywhere one is needed.
pub(crate) const ABSENT: f64 = -1.0;

pub(crate) fn kind(kind: LibKind) -> DeviceKind {
    match kind {
        LibKind::CDJ => DeviceKind::Cdj,
        LibKind::MIXER => DeviceKind::Mixer,
        LibKind::REKORDBOX_OR_CDJ3000 => DeviceKind::Rekordbox,
        _ => DeviceKind::Unknown,
    }
}

pub(crate) fn slot(slot: LibSlot) -> Slot {
    match slot {
        LibSlot::CD => Slot::Cd,
        LibSlot::SD => Slot::Sd,
        LibSlot::USB => Slot::Usb,
        LibSlot::REKORDBOX => Slot::Rekordbox,
        _ => Slot::None,
    }
}

/// The library's slot for one C++ named.
pub(crate) fn slot_back(from: Slot) -> LibSlot {
    match from {
        Slot::Cd => LibSlot::CD,
        Slot::Sd => LibSlot::SD,
        Slot::Rekordbox => LibSlot::REKORDBOX,
        // USB is the default rather than an error: it is the slot a host means
        // when it has not thought about it, and the one a deck browses first.
        _ => LibSlot::USB,
    }
}

pub(crate) fn play_state(state: LibState) -> PlayState {
    match state {
        LibState::NO_TRACK => PlayState::NoTrack,
        LibState::LOADING => PlayState::Loading,
        LibState::PLAYING => PlayState::Playing,
        LibState::LOOPING => PlayState::Looping,
        LibState::PAUSED => PlayState::Paused,
        LibState::CUED => PlayState::Cued,
        LibState::CUE_PLAY => PlayState::CuePlay,
        LibState::SEARCHING => PlayState::Searching,
        LibState::SPUN_DOWN => PlayState::SpunDown,
        LibState::EMERGENCY_LOOP => PlayState::Emergency,
        _ => PlayState::Other,
    }
}

pub(crate) fn device(from: &prolink::Device) -> Device {
    Device {
        number: from.number.get(),
        kind: kind(from.kind),
        is_player: from.number.is_player(),
        online: !from.offline,
        mac: from.mac.to_string(),
        name: from.name.as_str(),
        address: from.ip.to_string(),
        last_seen_ms: u64::try_from(from.last_seen.elapsed().as_millis()).unwrap_or(u64::MAX),
    }
}

pub(crate) fn player(from: &prolink::PlayerState) -> Player {
    let track = from.track();
    let status = from.status.map(|observed| observed.status);
    Player {
        number: from.device.get(),
        name: from.name.as_str(),

        has_status: from.play_state().is_some(),
        is_beating: from.is_beating(),

        effective_bpm: from.effective_bpm().unwrap_or(ABSENT),
        // From the status packet where there is one: it is published while the
        // deck is paused and a beat packet is not.
        track_bpm: status
            .and_then(|status| status.bpm_centi)
            .map_or(ABSENT, |centi| f64::from(centi) / 100.0),
        pitch_percent: status
            .and_then(|status| status.pitch)
            .map_or(0.0, prolink::Pitch::percent),

        beat_phase: from.beat_phase().unwrap_or(ABSENT),
        bar_phase: from.bar_phase().unwrap_or(ABSENT),
        beat_in_bar: from.beat_in_bar().map_or(0, prolink::BeatInBar::get),

        is_master: from.is_tempo_master().unwrap_or(false),
        is_synced: from.is_synced().unwrap_or(false),
        yielding_to: from.yielding_to().map_or(0, prolink::DeviceNumber::get),

        play_state: from.play_state().map_or(PlayState::NoTrack, play_state),
        track_id: track.map_or(0, |track| track.id),
        track_source_player: track.map_or(0, |track| track.source_player.get()),
        track_source_slot: track.map_or(Slot::None, |track| slot(track.slot)),
    }
}

pub(crate) fn interface(from: &prolink::Interface) -> NetworkInterface {
    NetworkInterface {
        name: from.name.clone(),
        address: from.ip.to_string(),
        broadcast: from.broadcast().to_string(),
        is_link_local: from.is_link_local(),
    }
}

/// An event with only the fields its kind gives meaning to set.
pub(crate) fn plain(kind: EventKind, device: u8, beat_in_bar: u8) -> Event {
    Event {
        kind,
        device,
        beat_in_bar,
        dropped: 0,
        transfer: 0,
        done: 0,
        total: 0,
        ok: true,
        detail: String::new(),
        path: String::new(),
        slot: Slot::None,
    }
}

pub(crate) fn event(from: &prolink::MonitorEvent) -> Event {
    match from {
        prolink::MonitorEvent::Beat(state) => plain(
            EventKind::Beat,
            state.device.get(),
            state.beat_in_bar().map_or(0, prolink::BeatInBar::get),
        ),
        prolink::MonitorEvent::Status(state) => {
            plain(EventKind::PlayerChanged, state.device.get(), 0)
        }
        prolink::MonitorEvent::Stopped(device) => plain(EventKind::Stopped, device.get(), 0),
        prolink::MonitorEvent::TempoMaster(master) => plain(
            EventKind::TempoMaster,
            master.map_or(0, prolink::DeviceNumber::get),
            0,
        ),
        prolink::MonitorEvent::Gone(device) => plain(EventKind::DeviceLost, device.get(), 0),
    }
}

/// One browse row, as C++ sees it.
pub(crate) fn row(from: &prolink_proto::dbserver::MenuItem) -> Row {
    use prolink_proto::dbserver::MenuItem;
    Row {
        id: from.id,
        label: from.label1.clone(),
        detail: from.label2.clone(),
        item_type: from.item_type.0,
        artwork_id: from.artwork_id,
        position: from.playlist_position,
        // The two live marks a server puts on a track row (F53, F55).
        is_loaded: from.flags & MenuItem::LOADED != 0,
        is_tagged: from.flags & MenuItem::TAGGED != 0,
    }
}

/// One track's metadata, as C++ sees it.
pub(crate) fn metadata(from: &prolink::consume::TrackMetadata) -> Metadata {
    Metadata {
        id: from.id,
        title: from.title.clone(),
        artist: from.artist.clone(),
        album: from.album.clone(),
        genre: from.genre.clone(),
        key: from.key.clone(),
        label: from.label.clone(),
        colour: from.colour.clone(),
        comment: from.comment.clone(),
        date_added: from.date_added.clone(),
        duration_seconds: from.duration_seconds,
        tempo_centibpm: from.tempo_centibpm,
        rating: from.rating,
        bitrate: from.bitrate,
        artwork_id: from.artwork_id,
    }
}

/// One of a player's slots, as C++ sees it.
pub(crate) fn media_info(from: &prolink::PeerSlot) -> MediaInfo {
    let described = from.description.as_ref();
    MediaInfo {
        device: from.device.get(),
        slot: slot(from.slot),
        has_media: from.state.has_media(),
        volume_name: described.map(|d| d.volume_name.clone()).unwrap_or_default(),
        created: described.map(|d| d.created.clone()).unwrap_or_default(),
        track_count: described.map_or(0, |d| d.track_count),
        playlist_count: described.map_or(0, |d| d.playlist_count),
    }
}
