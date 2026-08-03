// SPDX-License-Identifier: GPL-3.0-only

//! Reading a rekordbox `export.pdb` for a host that is not written in Rust.
//!
//! The parsing is [`prolink_rekordbox`]'s; this only reshapes it. Two things
//! about the shape are deliberate:
//!
//! **Names are resolved *and* the row ids are given.** A host that only wants
//! to show a track never has to join anything, and one building a browse tree
//! by artist still can. The joins are cheap here and awkward in C++.
//!
//! **A bad database is a value, not an exception.** A host has usually just
//! pulled several megabytes over NFS to get this far, and what it wants at
//! that point is to tell the user why it was wasted — not to unwind.

use std::collections::BTreeMap;

use crate::ffi::{PdbContents, PdbHistoryPlaylist, PdbNamed, PdbPlaylist, PdbTrack};

/// Read a rekordbox `export.pdb` off disk.
#[must_use]
pub fn read_pdb(path: &str) -> PdbContents {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return failed(format!("reading {path}: {error}")),
    };
    parse(&bytes, path)
}

/// Read a rekordbox `export.pdb` a host already has in memory.
#[must_use]
pub fn read_pdb_bytes(bytes: &[u8]) -> PdbContents {
    parse(bytes, "the database")
}

/// The shared body, named for whatever the caller can say about the source.
fn parse(bytes: &[u8], what: &str) -> PdbContents {
    let library = match prolink_rekordbox::Library::parse(bytes) {
        Ok(library) => library,
        Err(error) => return failed(format!("parsing {what}: {error}")),
    };

    PdbContents {
        ok: true,
        error: String::new(),
        tracks: library.tracks.values().map(track).collect(),
        playlists: playlists(&library),
        history: history(&library),
        artists: named(&library.artists),
        albums: named(&library.albums),
        genres: named(&library.genres),
        keys: named(&library.keys),
        labels: named(&library.labels),
        colors: named(&library.colors),
        artwork: named(&library.artwork),
    }
}

/// An empty result carrying the reason.
fn failed(error: String) -> PdbContents {
    PdbContents {
        ok: false,
        error,
        tracks: Vec::new(),
        playlists: Vec::new(),
        history: Vec::new(),
        artists: Vec::new(),
        albums: Vec::new(),
        genres: Vec::new(),
        keys: Vec::new(),
        labels: Vec::new(),
        colors: Vec::new(),
        artwork: Vec::new(),
    }
}

/// One lookup table, as a list.
///
/// Generic over the key because the colour palette is keyed on a byte — it is
/// fixed and small — while every other table is keyed on a row id.
fn named<K: Copy + Into<u32>>(table: &BTreeMap<K, String>) -> Vec<PdbNamed> {
    table
        .iter()
        .map(|(id, name)| PdbNamed {
            id: (*id).into(),
            name: name.clone(),
        })
        .collect()
}

fn track(from: &prolink_rekordbox::Track) -> PdbTrack {
    PdbTrack {
        id: from.id,
        title: from.title.clone(),
        artist: from.artist.clone(),
        album: from.album.clone(),
        genre: from.genre.clone(),
        key: from.key.clone(),
        label: from.label.clone(),
        color: from.color.clone(),
        comment: from.comment.clone(),
        file_path: from.file_path.clone(),
        analyze_path: from.analyze_path.clone(),
        artwork_path: from.artwork_path.clone(),
        date_added: from.date_added.clone(),
        year: u32::from(from.year),
        duration_seconds: u32::from(from.duration),
        bitrate: from.bitrate,
        tempo_centibpm: from.tempo,
        rating: u32::from(from.rating),
        artwork_id: from.artwork_id,
        sample_rate: from.sample_rate,
        file_size: from.file_size,
        track_number: from.track_number,
        disc_number: u32::from(from.disc_number),
        play_count: u32::from(from.play_count),
        // The container byte, which is what a deck uses to decide how to
        // decode the file — and getting it wrong makes a deck fetch the whole
        // track and then refuse to play it (F34).
        file_type: u32::from(from.container.0),
        artist_id: from.artist_id,
        album_id: from.album_id,
        genre_id: from.genre_id,
        key_id: from.key_id,
        label_id: from.label_id,
        color_id: u32::from(from.color_id),
    }
}

/// The history playlists, in row-id order -- which is session order, and the
/// nearest thing to a clock the format offers.
fn history(library: &prolink_rekordbox::Library) -> Vec<PdbHistoryPlaylist> {
    library
        .history
        .values()
        .map(|playlist| PdbHistoryPlaylist {
            id: playlist.id,
            name: playlist.name.clone(),
            track_ids: playlist.track_ids.clone(),
        })
        .collect()
}

fn playlists(library: &prolink_rekordbox::Library) -> Vec<PdbPlaylist> {
    library
        .playlists
        .values()
        .map(|playlist| PdbPlaylist {
            id: playlist.id,
            parent_id: playlist.parent_id,
            sort_order: playlist.sort_order,
            name: playlist.name.clone(),
            is_folder: playlist.is_folder,
            // In the DJ's own order. A playlist re-sorted alphabetically is a
            // different playlist.
            track_ids: playlist.track_ids.clone(),
        })
        .collect()
}
