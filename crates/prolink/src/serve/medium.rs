// SPDX-License-Identifier: GPL-3.0-only

//! One served medium: a slot, a library, and the files behind it.
//!
//! A medium is one of two kinds. A **rekordbox** medium is served from its
//! `export.pdb`, the way a CDJ serves one. A **plain** medium, a stick with
//! music and no rekordbox library, is served by its folders, the way a CDJ
//! serves one of those (`emu-captures/PLAIN-STICKS.md` §10): one root category,
//! FOLDER, and each file's tags read when a player first asks about it.
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use prolink_proto::Slot;
use prolink_rekordbox::{AnlzFile, Library};

use crate::media::MediaDescription;
use crate::serve::folder::{FolderTree, WalkLimits};
use crate::serve::tags::{self, FileTags};

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
    /// Empty for a plain medium, whose files are in `folders`.
    library: Library,
    /// A plain medium's folders and files: what is served when there is no
    /// rekordbox library. `None` for a rekordbox medium.
    folders: Option<FolderTree>,
    root: Option<PathBuf>,
    volume_name: String,
    created: String,
    settings: Vec<u8>,
    /// The volume's total and free bytes, read once when it is mounted. A
    /// deck plays off it read-only.
    size: Option<(u64, u64)>,
    /// file id → its tags, for a plain medium: read when a player first asks,
    /// as a CDJ does (§5), and kept. They are the only copy once the stick is
    /// gone.
    tags: Mutex<BTreeMap<u32, Arc<FileTags>>>,
    /// track id → its analysis. A load asks for four tags across two files
    /// within milliseconds, and asks again when the DJ reloads the same track.
    analysis: Mutex<BTreeMap<u32, Arc<Analysis>>>,
    /// artwork id → the image. Memoised for the same reason the analysis is,
    /// and for one more: these are the only copy once the medium is gone.
    artwork: Mutex<BTreeMap<u32, Arc<Vec<u8>>>>,
    /// Whether the medium behind this is gone while a player is still playing
    /// from it. See [`Self::go_phantom`].
    phantom: AtomicBool,
}

impl Medium {
    /// Where the rekordbox database lives on every medium.
    pub const PDB_PATH: &'static str = "PIONEER/rekordbox/export.pdb";
    /// Where the saved utility settings live.
    pub const SETTINGS_PATH: &'static str = "PIONEER/MYSETTING.DAT";

    /// Read a mounted medium of either kind: rekordbox if it holds an
    /// `export.pdb`, plain otherwise.
    ///
    /// A pdb that does not parse fails, rather than serving the stick by its
    /// folders: a CDJ would serve it from its database, and telling a player
    /// the stick has none would be a claim about it we cannot back.
    pub fn open(volume: &Path, slot: ServedSlot) -> crate::Result<Self> {
        if volume.join(Self::PDB_PATH).is_file() {
            Self::from_volume(volume, slot)
        } else {
            Self::from_folders(volume, slot)
        }
    }

    /// Read a mounted rekordbox medium.
    ///
    /// Fails if it holds no `export.pdb`: serving a medium we cannot enumerate
    /// would put an empty library on the network, and a deck told a medium has
    /// no tracks has no reason to offer it (F24).
    pub fn from_volume(volume: &Path, slot: ServedSlot) -> crate::Result<Self> {
        let pdb = std::fs::read(volume.join(Self::PDB_PATH))
            .map_err(crate::Error::io("reading export.pdb"))?;
        let library = Library::parse(&pdb)?;
        let settings = Self::read_settings(volume);

        let volume_name = volume
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        Ok(Self {
            slot,
            library,
            folders: None,
            root: Some(volume.to_path_buf()),
            volume_name,
            created: String::new(),
            settings,
            size: crate::volumes::size_of(volume),
            tags: Mutex::new(BTreeMap::new()),
            analysis: Mutex::new(BTreeMap::new()),
            artwork: Mutex::new(BTreeMap::new()),
            phantom: AtomicBool::new(false),
        })
    }

    /// Read a mounted stick with no rekordbox library, to serve by its
    /// folders.
    ///
    /// Only directories are listed; no file is opened until a player asks
    /// about it. Named by its volume label, as a CDJ names one (§3).
    ///
    /// Fails if there is nothing on it a player could play: a plain medium is
    /// offered with no track count at all (§10.1), so nothing but its files
    /// can say it is worth offering.
    pub fn from_folders(volume: &Path, slot: ServedSlot) -> crate::Result<Self> {
        let folders = FolderTree::walk(volume, WalkLimits::default())
            .map_err(crate::Error::io("listing a plain medium"))?;
        if folders.is_empty() {
            return Err(crate::Error::NoMusic {
                path: volume.to_path_buf(),
            });
        }
        if folders.is_truncated() {
            tracing::warn!(
                path = %volume.display(),
                files = folders.file_count(),
                "a plain medium past the walk's limits; serving what was found"
            );
        }
        Ok(Self {
            slot,
            library: Library::default(),
            folders: Some(folders),
            root: Some(volume.to_path_buf()),
            volume_name: crate::volumes::label_of(volume),
            created: String::new(),
            // A CDJ writes its MYSETTING.DAT to a plain stick too, and loads
            // it from one.
            settings: Self::read_settings(volume),
            size: crate::volumes::size_of(volume),
            tags: Mutex::new(BTreeMap::new()),
            analysis: Mutex::new(BTreeMap::new()),
            artwork: Mutex::new(BTreeMap::new()),
            phantom: AtomicBool::new(false),
        })
    }

    /// A medium with no files behind it, for tests and for a synthesised
    /// library. Artwork and analysis are simply unavailable.
    pub fn synthetic(slot: ServedSlot, library: Library, volume_name: &str) -> Self {
        Self {
            slot,
            library,
            folders: None,
            root: None,
            volume_name: volume_name.to_owned(),
            created: String::new(),
            settings: Vec::new(),
            size: None,
            tags: Mutex::new(BTreeMap::new()),
            analysis: Mutex::new(BTreeMap::new()),
            artwork: Mutex::new(BTreeMap::new()),
            phantom: AtomicBool::new(false),
        }
    }

    /// The 32 settings bytes on a medium, or none.
    fn read_settings(volume: &Path) -> Vec<u8> {
        // A medium with no saved settings is ordinary, so a missing file is not
        // an error.
        std::fs::read(volume.join(Self::SETTINGS_PATH))
            .ok()
            .and_then(|raw| prolink_rekordbox::SettingsFile::parse(&raw).ok())
            .map(|file| file.wire_settings().to_vec())
            .unwrap_or_default()
    }

    /// Which slot this medium is in.
    pub fn slot(&self) -> ServedSlot {
        self.slot
    }

    /// The parsed library. Empty for a plain medium.
    pub fn library(&self) -> &Library {
        &self.library
    }

    /// A plain medium's folders and files, or `None` for a rekordbox medium.
    pub fn folders(&self) -> Option<&FolderTree> {
        self.folders.as_ref()
    }

    /// Whether this is a stick with no rekordbox library, served by its
    /// folders.
    pub fn is_plain(&self) -> bool {
        self.folders.is_some()
    }

    /// How many tracks a host should say it holds: the library's, or a
    /// plain medium's files.
    pub fn track_count(&self) -> usize {
        self.folders
            .as_ref()
            .map_or(self.library.tracks.len(), FolderTree::file_count)
    }

    /// A plain medium's file's tags, read the first time anyone asks and
    /// kept; `None` for a file it does not have, and on a rekordbox medium.
    ///
    /// Read from the file while the stick is in, and from what was kept once
    /// it has gone phantom.
    pub fn tags(&self, file_id: u32) -> Option<Arc<FileTags>> {
        let file = self.folders.as_ref()?.file(file_id)?;
        if let Ok(cache) = self.tags.lock()
            && let Some(held) = cache.get(&file_id)
        {
            return Some(Arc::clone(held));
        }
        let read = Arc::new(match self.root.as_deref() {
            Some(root) if !self.is_phantom() => tags::read(&root.join(&file.relative), file),
            _ => FileTags::untagged(file),
        });
        // Not kept when the file could not be read: the stick may be busy, and
        // a file worked out from its name alone would stay that way.
        if read.is_read()
            && let Ok(mut cache) = self.tags.lock()
        {
            cache.insert(file_id, Arc::clone(&read));
        }
        Some(read)
    }

    /// The files on the medium a track needs, relative to its root: the audio,
    /// and for a rekordbox track its analysis.
    pub fn track_files(&self, track_id: u32) -> Vec<String> {
        if let Some(folders) = &self.folders {
            return folders
                .file(track_id)
                .map(|file| vec![file.relative.clone()])
                .unwrap_or_default();
        }
        let Some(track) = self.library.tracks.get(&track_id) else {
            return Vec::new();
        };
        // The `.EXT` because a player asks for tags that are only in it, and
        // rekordbox writes both extensions upper case.
        let mut wanted = vec![track.file_path.clone(), track.analyze_path.clone()];
        if let Some(ext) = track.analyze_ext_path() {
            wanted.push(ext);
        }
        wanted
            .into_iter()
            .map(|path| path.trim_start_matches('/').to_owned())
            .filter(|path| !path.is_empty())
            .collect()
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
    /// reason ever to ask again (F24); except a plain one, which a CDJ
    /// describes with no tracks, no playlists and no date, and another deck
    /// offers all the same (§3, §10.1).
    pub fn description(&self) -> MediaDescription {
        MediaDescription {
            volume_name: self.volume_name.clone(),
            created: self.created.clone(),
            track_count: if self.is_plain() {
                0
            } else {
                u32::try_from(self.library.tracks.len()).unwrap_or(u32::MAX)
            },
            playlist_count: u32::try_from(
                self.library
                    .playlists
                    .values()
                    .filter(|playlist| !playlist.is_folder)
                    .count(),
            )
            .unwrap_or(u32::MAX),
            total_bytes: self.size.map(|(total, _)| total),
            free_bytes: self.size.map(|(_, free)| free),
            plain: self.is_plain(),
        }
    }

    /// The cover image for an artwork row, or empty if unavailable.
    ///
    /// Empty rather than an error: a track without art is ordinary, and the
    /// protocol has a representation for it — a zero-length binary argument is
    /// omitted from the wire entirely, so "no artwork" and "here is the
    /// artwork" share one shape.
    ///
    /// Memoised, and that is not only about round trips: once the medium has
    /// gone phantom this map is the only copy of the image there is.
    pub fn artwork(&self, artwork_id: u32) -> Vec<u8> {
        // A plain file's artwork id is its own: its embedded picture, or its
        // folder's (§6).
        if self.folders.is_some() {
            return self
                .tags(artwork_id)
                .map(|tags| tags.artwork.clone())
                .unwrap_or_default();
        }
        if let Ok(cache) = self.artwork.lock()
            && let Some(held) = cache.get(&artwork_id)
        {
            return held.as_ref().clone();
        }
        let (Some(root), Some(path)) =
            (self.root.as_deref(), self.library.artwork.get(&artwork_id))
        else {
            return Vec::new();
        };
        let bytes = std::fs::read(root.join(path.trim_start_matches('/'))).unwrap_or_default();
        // A read that failed is not remembered: the file may simply not have
        // been reachable this once, and caching the emptiness would make a
        // cover that is there stay missing for the life of the medium.
        if !bytes.is_empty()
            && let Ok(mut cache) = self.artwork.lock()
        {
            cache.insert(artwork_id, Arc::new(bytes.clone()));
        }
        bytes
    }

    /// Read everything about a track that will not survive the medium going.
    ///
    /// Called when a player is seen to have loaded it, which is the point at
    /// which it becomes worth holding on to — a CDJ streams the audio rather
    /// than buffering it, so a stick pulled mid-track needs all of this to
    /// still be answerable afterwards.
    ///
    /// The audio itself is not here: it is served straight out of the NFS tree
    /// and is preserved there, by [`Vfs`](crate::serve::vfs::Vfs) repointing at
    /// a copy. This is the metadata alongside it.
    pub fn warm(&self, track_id: u32) {
        if self.folders.is_some() {
            let _ = self.tags(track_id);
            return;
        }
        let _ = self.analysis(track_id);
        if let Some(track) = self.library.tracks.get(&track_id)
            && track.artwork_id != 0
        {
            let _ = self.artwork(track.artwork_id);
        }
    }

    /// Whether the medium behind this has gone while a player was still using
    /// it.
    pub fn is_phantom(&self) -> bool {
        self.phantom.load(Ordering::Relaxed)
    }

    /// The stick is out, but a player is still playing from it.
    ///
    /// The medium stays **announced**, because that is what keeps the
    /// consumer's mount valid and its player from erroring out mid-track. What
    /// changes is that it stops being **browsable**: menus answer empty, so no
    /// DJ can start something we could not finish. Whatever is already cached
    /// keeps being served exactly as before, and the consumer plays out its
    /// track without ever knowing.
    ///
    /// One way only. A stick that comes back is mounted afresh rather than
    /// un-phantomed, because nothing guarantees it is the same stick.
    pub fn go_phantom(&self) {
        self.phantom.store(true, Ordering::Relaxed);
    }

    /// The parsed analysis for a track, read once and cached.
    pub fn analysis(&self, track_id: u32) -> Arc<Analysis> {
        if let Ok(cache) = self.analysis.lock()
            && let Some(cached) = cache.get(&track_id)
        {
            return Arc::clone(cached);
        }

        let parsed = Arc::new(self.read_analysis(track_id));
        if let Ok(mut cache) = self.analysis.lock() {
            cache.insert(track_id, Arc::clone(&parsed));
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
