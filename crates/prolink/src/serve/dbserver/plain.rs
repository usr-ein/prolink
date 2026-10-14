// SPDX-License-Identifier: GPL-3.0-only

//! The browse surface of a plain medium: what a CDJ-2000NXS answers about a
//! stick rekordbox never saw, answered the same way.
//!
//! Every shape here is Pioneer's firmware's, read off the emu-captures
//! E10-E17 (`emu-captures/PLAIN-STICKS.md`; §10.1 lists what a server must
//! answer, and the section numbers below are that document's):
//!
//! - **The root menu** is one category, FOLDER, whatever track type it is
//!   asked with (§4.1).
//! - **A folder** (`MENU_FOLDER [descriptor, sort, folder id, files only]`,
//!   the root `0xffffffff`) lists its folders, then its files: argument 0 the
//!   row's index, a file row's label its file name with the extension, its
//!   flags `0x02000000` and its artwork id its own (§4.2). Asked at a load
//!   with the menu targets 3 and 4, the rows carry the id and nothing else
//!   (§7.1, E12 f669 and f926).
//! - **A file's metadata** (`GET_GENERIC_METADATA 0x2202`) is ten items from
//!   its tags (§5), and its **track info** five, two of which carry the
//!   numbers a player hands back to open it over NFS (§7.2, §7.3).
//! - **The tag list** holds track rows with the artist column (§8).
//!
//! Nothing here reads a file but through [`Medium::tags`], which reads one
//! when a player first asks about it and keeps what it read.

use prolink_proto::dbserver::{
    Arguments, Descriptor, ItemType, MenuItem, MenuTarget, MessageKind, ROOT_CATEGORIES,
};

use crate::serve::Medium;
use crate::serve::folder::{FolderTree, ROOT_FOLDER};

/// The flags of a plain file's row, where a rekordbox track's carry
/// [`MenuItem::TRACK_FLAGS`]: the track type, 2, in the top byte (§4.2).
pub(super) const PLAIN_TRACK_FLAGS: u32 = 0x0200_0000;

/// The item type of a tag list row: a title with the artist column (§8).
const TITLE_AND_ARTIST: ItemType = ItemType(0x0704);

/// The menu targets a player asks a folder with at a load, and is answered
/// rows carrying only their ids (§7.1).
const LOAD_TARGETS: [MenuTarget; 2] = [MenuTarget::PREVIEW, MenuTarget(0x04)];

/// Turn a request about a plain medium into the rows it should produce, or
/// `None` for one that is not a menu request at all.
pub(super) fn build(
    kind: MessageKind,
    args: &Arguments,
    medium: &Medium,
    folders: &FolderTree,
    tags: &[u32],
) -> Option<Vec<MenuItem>> {
    let target = args
        .number(0)
        .and_then(Descriptor::parse)
        .map(|descriptor| descriptor.menu);
    Some(match kind {
        MessageKind::MENU_ROOT => root(),
        MessageKind::MENU_FOLDER => {
            let rows = folder(
                folders,
                args.number(2).unwrap_or(ROOT_FOLDER),
                args.number(3) == Some(1),
            );
            if target.is_some_and(|target| LOAD_TARGETS.contains(&target)) {
                rows.into_iter().map(bare).collect()
            } else {
                rows
            }
        }
        // A player asks 0x2202 for a plain file; 0x2002 would be a player
        // that took it for a rekordbox track, and is answered the same.
        MessageKind::GET_GENERIC_METADATA | MessageKind::GET_METADATA => {
            metadata(medium, args.number(1).unwrap_or(0))
        }
        MessageKind::GET_TRACK_INFO => track_info(medium, folders, args.number(1).unwrap_or(0)),
        MessageKind::MENU_TAG_LIST => tag_list(medium, tags),
        _ => return None,
    })
}

/// The root menu: FOLDER alone, with the id and type it has on a rekordbox
/// stick (E11 f122).
fn root() -> Vec<MenuItem> {
    ROOT_CATEGORIES
        .iter()
        .filter(|category| category.label == "FOLDER")
        .map(|category| category.to_item())
        .collect()
}

/// A folder's rows: its folders, then its files; only its files when asked
/// for them alone (§4.2, §7.1).
fn folder(folders: &FolderTree, id: u32, files_only: bool) -> Vec<MenuItem> {
    let Some(found) = folders.folder(id) else {
        return Vec::new();
    };
    let subfolders = found
        .folders
        .iter()
        .filter(|_| !files_only)
        .filter_map(|id| folders.folder(*id))
        .map(|sub| MenuItem::named(sub.id, ItemType::FOLDER, &sub.name));
    let files = found
        .files
        .iter()
        .filter_map(|id| folders.file(*id))
        .map(|file| {
            let mut row = MenuItem::named(file.id, ItemType::TRACK_TITLE, &file.name);
            row.flags = PLAIN_TRACK_FLAGS;
            row.artwork_id = file.id;
            row
        });
    subfolders
        .chain(files)
        .enumerate()
        .map(|(index, mut row)| {
            row.argument0 = u32::try_from(index).unwrap_or(u32::MAX);
            row
        })
        .collect()
}

/// A row as a load asks for it: its id, type and flags, and nothing else.
fn bare(mut row: MenuItem) -> MenuItem {
    row.argument0 = 0;
    row.label1.clear();
    row.label2.clear();
    row.artwork_id = 0;
    row
}

/// `GET_GENERIC_METADATA`: ten items, in a CDJ's order (§5, E12 f720).
///
/// Every item's artwork id is the file's own. The artist and album items carry
/// the file's id too: a plain stick has no artist or album rows to point at.
fn metadata(medium: &Medium, file_id: u32) -> Vec<MenuItem> {
    let Some(tags) = medium.tags(file_id) else {
        return Vec::new();
    };
    let item = |id: u32, item_type: ItemType, label: &str| {
        let mut item = MenuItem::named(id, item_type, label);
        item.artwork_id = file_id;
        item
    };
    let mut title = item(file_id, ItemType::TRACK_TITLE, &tags.title);
    title.flags = PLAIN_TRACK_FLAGS;
    vec![
        title,
        item(file_id, ItemType::ARTIST, &tags.artist),
        item(file_id, ItemType::ALBUM, &tags.album),
        item(tags.duration_seconds, ItemType::DURATION, ""),
        item(tags.tempo_centibpm, ItemType::TEMPO, ""),
        item(file_id, ItemType::COMMENT, &tags.comment),
        item(file_id, ItemType::GENRE, &tags.genre),
        // No rating and no colour on a plain file (§5).
        item(u32::MAX, ItemType::RATING, ""),
        item(255, ItemType::COLOR, ""),
        item(tags.bitrate_kbps, ItemType::BITRATE, ""),
    ]
}

/// `GET_TRACK_INFO`: five items (§7.2, E12 f682). Arguments 0 of the first
/// two are the numbers a player looks the file up by, `?\0D <folder> <entry>`
/// (§7.3); the fifth's is the file's size, and there is no path.
fn track_info(medium: &Medium, folders: &FolderTree, file_id: u32) -> Vec<MenuItem> {
    let (Some(file), Some((folder, entry)), Some(tags)) = (
        folders.file(file_id),
        folders.lookup_numbers(file_id),
        medium.tags(file_id),
    ) else {
        return Vec::new();
    };
    let with_argument = |argument0: u32, id: u32, item_type: ItemType, label: &str| {
        let mut item = MenuItem::named(id, item_type, label);
        item.argument0 = argument0;
        item
    };
    vec![
        // `0x0004` is the container here, not the title (F34).
        with_argument(folder, u32::from(file.container), ItemType::TRACK_TITLE, ""),
        with_argument(entry, tags.duration_seconds, ItemType::DURATION, ""),
        with_argument(0, tags.tempo_centibpm, ItemType::TEMPO, ""),
        with_argument(0, file_id, ItemType::COMMENT, &tags.comment),
        with_argument(
            u32::try_from(file.size).unwrap_or(u32::MAX),
            0,
            ItemType::PATH,
            "",
        ),
    ]
}

/// The tag list: a track row per tagged file, in the order tagged, with its
/// title and artist (§8, E15 f1670).
fn tag_list(medium: &Medium, tagged: &[u32]) -> Vec<MenuItem> {
    tagged
        .iter()
        .filter_map(|&id| Some((id, medium.tags(id)?)))
        .enumerate()
        .map(|(index, (id, tags))| {
            let mut row = MenuItem::track(id, &tags.title, &tags.artist, TITLE_AND_ARTIST, id);
            row.argument0 = id;
            row.flags = PLAIN_TRACK_FLAGS;
            row.playlist_position = u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1);
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve::ServedSlot;

    /// A stick on disk, removed when the test ends.
    struct Stick(std::path::PathBuf);

    impl Stick {
        fn new(name: &str, files: &[&str]) -> Self {
            let root =
                std::env::temp_dir().join(format!("prolink-plain-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            for path in files {
                let full = root.join(path);
                std::fs::create_dir_all(full.parent().expect("a parent")).expect("a folder");
                std::fs::write(&full, b"not really audio").expect("a file");
            }
            Self(root)
        }

        fn medium(&self) -> Medium {
            Medium::from_folders(&self.0, ServedSlot::USB).expect("a plain medium")
        }
    }

    impl Drop for Stick {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn args(values: &[u32]) -> Arguments {
        Arguments::new(
            values
                .iter()
                .map(|&value| prolink_proto::dbserver::Field::U32(value)),
        )
        .expect("a short argument list")
    }

    /// `D1/M<menu>/S3/T2`.
    fn descriptor(menu: u8) -> u32 {
        u32::from_be_bytes([1, menu, 3, 2])
    }

    #[test]
    fn the_root_menu_is_folder_alone() {
        let stick = Stick::new("root", &["a.mp3"]);
        let medium = stick.medium();
        let folders = medium.folders().expect("plain");
        let rows = build(
            MessageKind::MENU_ROOT,
            &args(&[descriptor(1), 0, 0x00ff_ffff]),
            &medium,
            folders,
            &[],
        )
        .expect("a menu");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 0x11, "E11 f122");
        assert_eq!(rows[0].item_type, ItemType::MENU_FOLDER);
    }

    #[test]
    fn a_folder_lists_folders_then_files_as_a_cdj_does() {
        let stick = Stick::new("folder", &["b.mp3", "A.mp3", "Sub/c.mp3"]);
        let medium = stick.medium();
        let folders = medium.folders().expect("plain");
        let rows = build(
            MessageKind::MENU_FOLDER,
            &args(&[descriptor(1), 0, ROOT_FOLDER, 0]),
            &medium,
            folders,
            &[],
        )
        .expect("a menu");
        let shape: Vec<_> = rows
            .iter()
            .map(|row| (row.argument0, row.label1.as_str(), row.item_type, row.flags))
            .collect();
        assert_eq!(
            shape,
            [
                (0, "Sub", ItemType::FOLDER, 0),
                (1, "A.mp3", ItemType::TRACK_TITLE, PLAIN_TRACK_FLAGS),
                (2, "b.mp3", ItemType::TRACK_TITLE, PLAIN_TRACK_FLAGS),
            ]
        );
        assert_eq!(
            rows[1].artwork_id, rows[1].id,
            "a file's artwork id is its own"
        );
        assert_eq!(rows[0].artwork_id, 0);

        // At a load: the files alone with M3, every row with M4, and no more
        // than the ids (E12 f661, f669).
        let files = build(
            MessageKind::MENU_FOLDER,
            &args(&[descriptor(3), 0, ROOT_FOLDER, 1]),
            &medium,
            folders,
            &[],
        )
        .expect("a menu");
        assert_eq!(files.len(), 2);
        assert!(
            files
                .iter()
                .all(|row| row.label1.is_empty() && row.artwork_id == 0)
        );
        assert_eq!(files[0].id, rows[1].id);
        assert_eq!(files[1].argument0, 0);
    }

    #[test]
    fn a_files_metadata_is_ten_items_and_its_track_info_five() {
        let stick = Stick::new("metadata", &["Sub/Untagged Track.mp3"]);
        let medium = stick.medium();
        let folders = medium.folders().expect("plain");
        let sub = folders.folder(ROOT_FOLDER).expect("root").folders[0];
        let file = folders.folder(sub).expect("Sub").files[0];

        let items = build(
            MessageKind::GET_GENERIC_METADATA,
            &args(&[descriptor(2), file]),
            &medium,
            folders,
            &[],
        )
        .expect("metadata");
        let types: Vec<_> = items.iter().map(|item| item.item_type).collect();
        assert_eq!(
            types,
            [
                ItemType::TRACK_TITLE,
                ItemType::ARTIST,
                ItemType::ALBUM,
                ItemType::DURATION,
                ItemType::TEMPO,
                ItemType::COMMENT,
                ItemType::GENRE,
                ItemType::RATING,
                ItemType::COLOR,
                ItemType::BITRATE,
            ]
        );
        // No tags: the file name with its extension, as a CDJ answers (§5).
        assert_eq!(items[0].label1, "Untagged Track.mp3");
        assert_eq!(items[0].flags, PLAIN_TRACK_FLAGS);
        assert_eq!(items[7].id, u32::MAX);
        assert_eq!(items[8].id, 255);
        assert!(items.iter().all(|item| item.artwork_id == file));

        let info = build(
            MessageKind::GET_TRACK_INFO,
            &args(&[descriptor(8), file]),
            &medium,
            folders,
            &[],
        )
        .expect("track info");
        assert_eq!(info.len(), 5);
        assert_eq!((info[0].id, info[0].argument0), (1, sub), "MP3, in Sub");
        assert_eq!(info[1].argument0, file);
        assert_eq!(info[4].item_type, ItemType::PATH);
        assert_eq!(
            info[4].label1, "",
            "no path: a player looks it up by number"
        );
        assert_eq!(info[4].argument0, 16, "the file's size");
        assert_eq!(
            folders
                .file_by_lookup(&crate::serve::folder::lookup_name(
                    info[0].argument0,
                    info[1].argument0
                ))
                .map(|found| found.id),
            Some(file),
            "the two numbers name the file"
        );
    }

    #[test]
    fn the_tag_list_holds_title_and_artist_rows_in_tag_order() {
        let stick = Stick::new("tags", &["a.mp3", "b.mp3"]);
        let medium = stick.medium();
        let folders = medium.folders().expect("plain");
        let root = folders.folder(ROOT_FOLDER).expect("root");
        let (a, b) = (root.files[0], root.files[1]);
        let rows = build(
            MessageKind::MENU_TAG_LIST,
            &args(&[descriptor(6), 0]),
            &medium,
            folders,
            &[b, a],
        )
        .expect("the tag list");
        let shape: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.id,
                    row.label1.as_str(),
                    row.playlist_position,
                    row.item_type,
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                (b, "b.mp3", 1, TITLE_AND_ARTIST),
                (a, "a.mp3", 2, TITLE_AND_ARTIST)
            ]
        );
    }
}
