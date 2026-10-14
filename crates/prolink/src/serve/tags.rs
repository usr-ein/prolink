// SPDX-License-Identifier: GPL-3.0-only

//! What a plain file's tags say, as a player is told it.
//!
//! A CDJ serving a stick rekordbox never saw reads a file's tags when another
//! player first asks about it, and answers from them (PLAIN-STICKS.md §5):
//! title, artist, album, genre, comment, the BPM tag, the duration and the
//! bitrate, and the embedded JPEG or the folder's `cover.jpg` for artwork.
//! This module reads the same, the same way: one file at a time, when asked.

use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, PictureType};
use lofty::tag::{Accessor, ItemKey, Tag};

use crate::serve::folder::File;

/// The largest picture served as artwork, in bytes.
///
/// A CDJ draws a cover a few hundred pixels across, and the covers rekordbox
/// exports for it are a few kilobytes; a print-size scan in a tag is not
/// something to hand a player's JPEG decoder over the network.
const MAX_ARTWORK: usize = 1024 * 1024;

/// The names a folder's own picture goes by, best first: the deck's browser
/// takes the same ones (`folderlibrary.cpp`). JPEG only, the one format a CDJ
/// was seen to show (§6).
const FOLDER_PICTURES: [&str; 5] = ["cover", "folder", "front", "album", "artwork"];

/// A file's tags, as `GET_GENERIC_METADATA` and `GET_TRACK_INFO` answer them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileTags {
    /// The title tag, else the file name with its extension (§5).
    pub title: String,
    /// The artist tag, or empty.
    pub artist: String,
    /// The album tag, or empty.
    pub album: String,
    /// The genre, by name: ID3v1's genre byte too (§5: `Techno`, from 18).
    pub genre: String,
    /// The comment tag, or empty.
    pub comment: String,
    /// Whole seconds; zero when the file could not be read.
    pub duration_seconds: u32,
    /// The BPM tag ×100, as `TBPM` 125 is 12500; zero without one (§5).
    pub tempo_centibpm: u32,
    /// The audio's bitrate in kbps, or zero.
    pub bitrate_kbps: u32,
    /// The embedded JPEG, else the folder's, else empty (§6).
    pub artwork: Vec<u8>,
    read: bool,
}

impl FileTags {
    /// What can be said of a file without opening it: its name.
    pub fn untagged(file: &File) -> Self {
        Self {
            title: file.name.clone(),
            ..Self::default()
        }
    }

    /// Whether the file itself was read, rather than only named.
    pub fn is_read(&self) -> bool {
        self.read
    }
}

/// Read one file's tags. A file that cannot be read is answered by its name,
/// as a CDJ answers a file with no tags.
pub fn read(path: &Path, file: &File) -> FileTags {
    let mut tags = FileTags::untagged(file);
    let parsed = lofty::probe::Probe::open(path).and_then(|probe| {
        probe
            .options(ParseOptions::new().read_cover_art(true))
            .read()
    });
    let tagged = match parsed {
        Ok(tagged) => tagged,
        Err(error) => {
            tracing::debug!(path = %path.display(), %error, "a plain file's tags could not be read");
            // Its folder's picture all the same: a file with no tags gets one
            // on a CDJ too (§6, `04 No tags.mp3`).
            tags.artwork = folder_picture(path);
            return tags;
        }
    };
    tags.read = true;

    let properties = tagged.properties();
    let millis = properties.duration().as_millis();
    tags.duration_seconds = u32::try_from(millis.saturating_add(500) / 1000).unwrap_or(u32::MAX);
    tags.bitrate_kbps = properties.audio_bitrate().unwrap_or(0);

    // The primary tag first (ID3v2 in an MP3), then any other: an MP3 with
    // only ID3v1 is still a tagged file (§5).
    let mut ordered: Vec<&Tag> = tagged.primary_tag().into_iter().collect();
    ordered.extend(
        tagged
            .tags()
            .iter()
            .filter(|tag| Some(tag.tag_type()) != tagged.primary_tag().map(Tag::tag_type)),
    );
    let first = |field: fn(&Tag) -> Option<String>| {
        ordered
            .iter()
            .find_map(|tag| field(tag).filter(|value| !value.trim().is_empty()))
    };
    if let Some(title) = first(|tag| tag.title().map(std::borrow::Cow::into_owned)) {
        tags.title = title;
    }
    tags.artist = first(|tag| tag.artist().map(std::borrow::Cow::into_owned)).unwrap_or_default();
    tags.album = first(|tag| tag.album().map(std::borrow::Cow::into_owned)).unwrap_or_default();
    tags.genre = first(|tag| tag.genre().map(std::borrow::Cow::into_owned)).unwrap_or_default();
    tags.comment = first(|tag| tag.comment().map(std::borrow::Cow::into_owned)).unwrap_or_default();
    tags.tempo_centibpm = first(|tag| {
        tag.get_string(ItemKey::IntegerBpm)
            .or_else(|| tag.get_string(ItemKey::Bpm))
            .map(str::to_owned)
    })
    .and_then(|bpm| centibpm(&bpm))
    .unwrap_or(0);

    tags.artwork = ordered
        .iter()
        .flat_map(|tag| tag.pictures())
        // The front cover first, as a player would want it, then any.
        .filter(|picture| is_jpeg(picture.mime_type(), picture.data()))
        .min_by_key(|picture| picture.pic_type() != PictureType::CoverFront)
        .map(|picture| picture.data().to_vec())
        .filter(|data| data.len() <= MAX_ARTWORK)
        .unwrap_or_else(|| folder_picture(path));
    tags
}

/// A BPM tag as hundredths: `125` is 12500, `125.5` is 12550. Nothing for one
/// that is not a tempo at all.
fn centibpm(text: &str) -> Option<u32> {
    let bpm: f64 = text.trim().replace(',', ".").parse().ok()?;
    if !(1.0..=999.0).contains(&bpm) {
        return None;
    }
    format!("{:.0}", bpm * 100.0).parse().ok()
}

/// Whether a picture is a JPEG, by its declared type or by its first bytes.
fn is_jpeg(mime: Option<&MimeType>, data: &[u8]) -> bool {
    matches!(mime, Some(MimeType::Jpeg)) || data.starts_with(&[0xff, 0xd8, 0xff])
}

/// The picture of the folder a file is in: `cover.jpg` and its kin.
fn folder_picture(path: &Path) -> Vec<u8> {
    let Some(folder) = path.parent() else {
        return Vec::new();
    };
    let Ok(listing) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut best: Option<(usize, std::path::PathBuf)> = None;
    for entry in listing.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let Some((stem, extension)) = name.rsplit_once('.') else {
            continue;
        };
        if !extension.eq_ignore_ascii_case("jpg") && !extension.eq_ignore_ascii_case("jpeg") {
            continue;
        }
        let Some(rank) = FOLDER_PICTURES
            .iter()
            .position(|wanted| stem.eq_ignore_ascii_case(wanted))
        else {
            continue;
        };
        if best.as_ref().is_none_or(|(held, _)| rank < *held) {
            best = Some((rank, entry.path()));
        }
    }
    best.and_then(|(_, picture)| std::fs::read(picture).ok())
        .filter(|data| data.len() <= MAX_ARTWORK && data.starts_with(&[0xff, 0xd8, 0xff]))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> File {
        File {
            id: 6,
            folder: 1,
            name: name.to_owned(),
            relative: name.to_owned(),
            size: 10,
            container: 1,
        }
    }

    #[test]
    fn a_file_that_cannot_be_read_is_named_by_its_file_name() {
        // §5: no tag, the file name with its extension.
        let tags = read(Path::new("/nowhere/at/all.mp3"), &file("all.mp3"));
        assert_eq!(tags.title, "all.mp3");
        assert!(!tags.is_read());
        assert_eq!(tags.duration_seconds, 0);
    }

    #[test]
    fn a_bpm_tag_is_in_hundredths() {
        assert_eq!(centibpm("125"), Some(12500));
        assert_eq!(centibpm("125.5"), Some(12550));
        assert_eq!(centibpm(" 130,25 "), Some(13025));
        assert_eq!(centibpm("0"), None);
        assert_eq!(centibpm("fast"), None);
    }
}
