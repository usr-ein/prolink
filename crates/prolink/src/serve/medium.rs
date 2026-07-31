// SPDX-License-Identifier: GPL-3.0-only

//! One served medium: a slot, a library, and the files behind it.
//!
//! A unit with two USB ports should present them to the network as a USB and an
//! SD, which is what a CDJ expects to see. The shape of that is not the obvious
//! one: a player browsing two media on the same peer opens **one** dbserver
//! connection and distinguishes them purely by the slot byte in each request's
//! descriptor (F37). So serving two media is one server holding a medium per
//! slot, resolved **per message** — caching the medium per connection would
//! serve the wrong library the moment the DJ switches slots.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use prolink_proto::Slot;
use prolink_rekordbox::{AnlzFile, Library};

use crate::media::MediaDescription;

/// A slot a medium can actually be served from.
///
/// Only two of the five slot numbers name something a CDJ will browse on a
/// peer, and each has a fixed NFS export path. Making it a type means the
/// export path is derived rather than remembered, and a medium cannot be
/// configured into a slot that has nowhere to be mounted from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ServedSlot(Slot);

impl ServedSlot {
    /// The SD card slot, exported as `/B/`.
    pub const SD: Self = Self(Slot::SD);
    /// The USB slot, exported as `/C/`.
    pub const USB: Self = Self(Slot::USB);

    /// Narrow a slot to one that can be served, or `None`.
    pub const fn new(slot: Slot) -> Option<Self> {
        match slot {
            Slot::SD | Slot::USB => Some(Self(slot)),
            _ => None,
        }
    }

    /// The slot as it goes on the wire.
    pub const fn slot(self) -> Slot {
        self.0
    }

    /// The NFS export path a player mounts for this slot.
    ///
    /// `/C/EXPORT` has also been seen for USB on other hardware, so a *client*
    /// should match on the prefix rather than the whole string (C6). As a
    /// server we choose, and these are what our own hardware answers with.
    pub const fn export_path(self) -> &'static str {
        match self.0 {
            Slot::SD => "/B/",
            _ => "/C/",
        }
    }

    /// The subtree this medium occupies in the shared VFS.
    ///
    /// Derived from the export so the two never disagree. Keeping the media in
    /// separate subtrees is what keeps their filehandles distinct: a handle is a
    /// hash of its path, and a CDJ preserves only the leading twelve bytes
    /// (F28), so two media sharing a root would be indistinguishable afterwards.
    pub const fn vfs_prefix(self) -> &'static str {
        match self.0 {
            Slot::SD => "B",
            _ => "C",
        }
    }
}

/// The `.DAT` and `.EXT` analysis for one track, either possibly absent.
///
/// Both are read together because a load asks for tags from each within a few
/// milliseconds, and parsing a container is walking a tag list — far cheaper
/// than the second file read it saves.
#[derive(Debug, Default)]
pub struct Analysis {
    /// `ANLZ####.DAT`: beat grid, cues, preview waveform, VBR index.
    pub dat: Option<AnlzFile>,
    /// `ANLZ####.EXT`: the scrolling waveform and the newer tags.
    pub ext: Option<AnlzFile>,
}

impl Analysis {
    /// The raw payload of a tag, preferring the `.DAT` and falling back to the
    /// `.EXT`.
    ///
    /// Empty rather than absent when neither has it: a track analysed by an
    /// older rekordbox legitimately lacks the newer tags, and a missing waveform
    /// should cost the waveform, not the load.
    pub fn payload(&self, fourcc: prolink_rekordbox::FourCc) -> &[u8] {
        self.dat
            .as_ref()
            .and_then(|file| file.payload(fourcc))
            .or_else(|| self.ext.as_ref().and_then(|file| file.payload(fourcc)))
            .unwrap_or_default()
    }
}

/// A library plus the medium it came from, bound to a slot.
#[derive(Debug)]
pub struct Medium {
    slot: ServedSlot,
    library: Library,
    root: Option<PathBuf>,
    volume_name: String,
    created: String,
    settings: Vec<u8>,
    /// track id → its analysis. A load asks for four tags across two files
    /// within milliseconds, and asks again when the DJ reloads the same track.
    analysis: Mutex<BTreeMap<u32, std::sync::Arc<Analysis>>>,
}

impl Medium {
    /// Where the rekordbox database lives on every medium.
    pub const PDB_PATH: &'static str = "PIONEER/rekordbox/export.pdb";
    /// Where the saved utility settings live.
    pub const SETTINGS_PATH: &'static str = "PIONEER/MYSETTING.DAT";

    /// Read a mounted rekordbox medium.
    ///
    /// Fails if it holds no `export.pdb`: serving a medium we cannot enumerate
    /// would put an empty library on the network, and a deck told a medium has
    /// no tracks has no reason to offer it (F24).
    pub fn from_volume(volume: &Path, slot: ServedSlot) -> crate::Result<Self> {
        let pdb = std::fs::read(volume.join(Self::PDB_PATH))
            .map_err(crate::Error::io("reading export.pdb"))?;
        let library = Library::parse(&pdb)?;

        // A medium with no saved settings is ordinary, so a missing file is not
        // an error.
        let settings = std::fs::read(volume.join(Self::SETTINGS_PATH))
            .ok()
            .and_then(|raw| prolink_rekordbox::SettingsFile::parse(&raw).ok())
            .map(|file| file.wire_settings().to_vec())
            .unwrap_or_default();

        let volume_name = volume
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        Ok(Self {
            slot,
            library,
            root: Some(volume.to_path_buf()),
            volume_name,
            created: String::new(),
            settings,
            analysis: Mutex::new(BTreeMap::new()),
        })
    }

    /// A medium with no files behind it, for tests and for a synthesised
    /// library. Artwork and analysis are simply unavailable.
    pub fn synthetic(slot: ServedSlot, library: Library, volume_name: &str) -> Self {
        Self {
            slot,
            library,
            root: None,
            volume_name: volume_name.to_owned(),
            created: String::new(),
            settings: Vec::new(),
            analysis: Mutex::new(BTreeMap::new()),
        }
    }

    /// Which slot this medium is in.
    pub fn slot(&self) -> ServedSlot {
        self.slot
    }

    /// The parsed library.
    pub fn library(&self) -> &Library {
        &self.library
    }

    /// Where the medium is mounted locally, if anywhere.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The volume label a peer is told about.
    pub fn volume_name(&self) -> &str {
        &self.volume_name
    }

    /// The 32 settings bytes for a `0x36` reply, or empty.
    pub fn settings(&self) -> &[u8] {
        &self.settings
    }

    /// What a media query should be answered with.
    ///
    /// The counts are the true ones. A deck told a medium holds nothing has no
    /// reason ever to ask again (F24).
    pub fn description(&self) -> MediaDescription {
        MediaDescription {
            volume_name: self.volume_name.clone(),
            created: self.created.clone(),
            track_count: u32::try_from(self.library.tracks.len()).unwrap_or(u32::MAX),
            playlist_count: u32::try_from(
                self.library
                    .playlists
                    .values()
                    .filter(|playlist| !playlist.is_folder)
                    .count(),
            )
            .unwrap_or(u32::MAX),
            total_bytes: None,
            free_bytes: None,
        }
    }

    /// The cover image for an artwork row, or empty if unavailable.
    ///
    /// Empty rather than an error: a track without art is ordinary, and the
    /// protocol has a representation for it — a zero-length binary argument is
    /// omitted from the wire entirely, so "no artwork" and "here is the
    /// artwork" share one shape.
    pub fn artwork(&self, artwork_id: u32) -> Vec<u8> {
        let (Some(root), Some(path)) =
            (self.root.as_deref(), self.library.artwork.get(&artwork_id))
        else {
            return Vec::new();
        };
        std::fs::read(root.join(path.trim_start_matches('/'))).unwrap_or_default()
    }

    /// The parsed analysis for a track, read once and cached.
    pub fn analysis(&self, track_id: u32) -> std::sync::Arc<Analysis> {
        if let Ok(cache) = self.analysis.lock()
            && let Some(cached) = cache.get(&track_id)
        {
            return std::sync::Arc::clone(cached);
        }

        let parsed = std::sync::Arc::new(self.read_analysis(track_id));
        if let Ok(mut cache) = self.analysis.lock() {
            cache.insert(track_id, std::sync::Arc::clone(&parsed));
        }
        parsed
    }

    fn read_analysis(&self, track_id: u32) -> Analysis {
        let (Some(root), Some(track)) = (self.root.as_deref(), self.library.tracks.get(&track_id))
        else {
            return Analysis::default();
        };
        let load = |relative: &str| {
            if relative.is_empty() {
                return None;
            }
            let bytes = std::fs::read(root.join(relative.trim_start_matches('/'))).ok()?;
            AnlzFile::parse(&bytes).ok()
        };
        Analysis {
            dat: load(&track.analyze_path),
            ext: track.analyze_ext_path().as_deref().and_then(load),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_sd_and_usb_can_be_served() {
        assert_eq!(ServedSlot::new(Slot::USB), Some(ServedSlot::USB));
        assert_eq!(ServedSlot::new(Slot::SD), Some(ServedSlot::SD));
        assert_eq!(
            ServedSlot::new(Slot::CD),
            None,
            "a CD has no rekordbox export"
        );
        assert_eq!(ServedSlot::new(Slot::REKORDBOX), None);
        assert_eq!(ServedSlot::new(Slot::NONE), None);
    }

    #[test]
    fn a_slot_knows_where_it_is_exported_from() {
        assert_eq!(ServedSlot::SD.export_path(), "/B/");
        assert_eq!(ServedSlot::USB.export_path(), "/C/");
    }

    #[test]
    fn two_media_occupy_different_subtrees() {
        // A handle is a hash of its path, and a CDJ keeps only twelve bytes of
        // it, so a shared root would leave nothing to tell them apart.
        assert_ne!(ServedSlot::SD.vfs_prefix(), ServedSlot::USB.vfs_prefix());
    }
}
