// SPDX-License-Identifier: GPL-3.0-only

//! A stick rekordbox never saw, served the way a CDJ serves one: by its
//! folders.
//!
//! What a CDJ-2000NXS answers for such a stick is in
//! `emu-captures/PLAIN-STICKS.md`, from Pioneer's own firmware: one root
//! category, FOLDER; folders then files in each folder, in the player's own
//! collation; a file opened over NFS not by its path but by one `LOOKUP` of
//! the name `?\0D <folder> <entry>` on the export's root. This module is the
//! tree those answers are read off: which folders and files there are, in
//! what order, under which ids. The tags are read per file, when a player
//! asks (`super::tags`).
//!
//! # What is listed
//!
//! What this deck itself would play, and nothing a player could only fail on:
//! MP3, AAC (`.m4a`), WAV and AIFF, the formats a CDJ-2000NXS plays. A CDJ
//! lists FLAC, empty folders and macOS's `._*` files on its own stick and then
//! greys each one out when it cannot play it (§2, §7.6); here they are left
//! out, with hidden names, empty files, the other DJ programs' folders and
//! folders with nothing playable anywhere below them.
//!
//! # Ids
//!
//! A CDJ names a folder or a file by its first cluster on the FAT volume
//! (§4.2): one id space, unique on the medium, stable for as long as the stick
//! is unchanged. Ours are counted instead, in one depth-first walk of the
//! sorted tree, so they have the same three properties on any filesystem: the
//! root is 1, and a folder's files are numbered before its sub-folders. The
//! two numbers a player hands back in its `?\0D` lookup are ours too, the
//! folder's id and the file's.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::Path;

/// The id `MENU_FOLDER` names the top of the stick by (E11 f370).
pub const ROOT_FOLDER: u32 = 0xffff_ffff;

/// The root folder's own id: the folder number of a file at the top of the
/// stick, in its `?\0D` name.
const ROOT_ID: u32 = 1;

/// How far a walk goes before it stops and serves what it found.
#[derive(Clone, Copy, Debug)]
pub struct WalkLimits {
    /// Files, in all.
    pub max_files: usize,
    /// Folders below the root. A CDJ opens seven levels over LINK (§2).
    pub max_depth: usize,
}

impl Default for WalkLimits {
    /// The deck's own browser's limits (`docs/plain-usb-plan.md` §4.1).
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_depth: 16,
        }
    }
}

/// One folder on the stick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Its id. The root's is 1, and `MENU_FOLDER` names it [`ROOT_FOLDER`].
    pub id: u32,
    /// Its name as the filesystem spells it. Empty for the root.
    pub name: String,
    /// The folder holding it; the root's is itself.
    pub parent: u32,
    /// Its folders, in the player's order.
    pub folders: Vec<u32>,
    /// Its files, in the player's order.
    pub files: Vec<u32>,
}

/// One file a player can be offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    /// Its id, the one every request about it carries.
    pub id: u32,
    /// The folder holding it.
    pub folder: u32,
    /// Its name with its extension, as the filesystem spells it: what a
    /// player's FOLDER row shows (§4.2).
    pub name: String,
    /// Where it is from the medium's root, `/`-separated, with no leading
    /// slash.
    pub relative: String,
    /// Its size in bytes.
    pub size: u64,
    /// The container byte rekordbox would store for it: MP3 1, AAC 4, WAV 11,
    /// AIFF 12 (§7.2).
    pub container: u8,
}

/// The stick, as a player browsing it sees it.
#[derive(Clone, Debug, Default)]
pub struct FolderTree {
    folders: BTreeMap<u32, Folder>,
    files: BTreeMap<u32, File>,
    /// A file's path from the medium's root → its id.
    by_path: BTreeMap<String, u32>,
    /// The walk stopped at a limit before it saw everything.
    truncated: bool,
}

impl FolderTree {
    /// Walk a mounted stick. Only directories are listed: no file is opened.
    ///
    /// # Errors
    ///
    /// When the stick's root cannot be listed. A folder below it that cannot
    /// be is left out.
    pub fn walk(root: &Path, limits: WalkLimits) -> std::io::Result<Self> {
        let mut budget = limits.max_files;
        let mut truncated = false;
        // Listed first, so the error that matters is the one returned.
        std::fs::read_dir(root)?;
        let scanned = scan(root, "", 0, limits, &mut budget, &mut truncated);
        let mut tree = Self {
            truncated,
            ..Self::default()
        };
        let mut next = ROOT_ID;
        tree.number(scanned, String::new(), ROOT_ID, &mut next);
        Ok(tree)
    }

    /// Give ids to what a scan found, the folder first, then its files, then
    /// each of its folders in turn, and record it all.
    fn number(&mut self, scanned: Scanned, name: String, parent: u32, next: &mut u32) -> u32 {
        let id = *next;
        *next = next.saturating_add(1);
        let mut folder = Folder {
            id,
            name,
            parent: if id == ROOT_ID { ROOT_ID } else { parent },
            folders: Vec::new(),
            files: Vec::new(),
        };
        for found in scanned.files {
            let file_id = *next;
            *next = next.saturating_add(1);
            folder.files.push(file_id);
            self.by_path.insert(found.relative.clone(), file_id);
            self.files.insert(
                file_id,
                File {
                    id: file_id,
                    folder: id,
                    name: found.name,
                    relative: found.relative,
                    size: found.size,
                    container: found.container,
                },
            );
        }
        for (child_name, child) in scanned.folders {
            let child_id = self.number(child, child_name, id, next);
            folder.folders.push(child_id);
        }
        self.folders.insert(id, folder);
        id
    }

    /// A folder, by the id a request names it by: [`ROOT_FOLDER`] for the root.
    pub fn folder(&self, id: u32) -> Option<&Folder> {
        self.folders
            .get(&if id == ROOT_FOLDER { ROOT_ID } else { id })
    }

    /// A file, by its id.
    pub fn file(&self, id: u32) -> Option<&File> {
        self.files.get(&id)
    }

    /// A file by where it is from the medium's root, with or without a
    /// leading slash: how a host that plays the stick itself names it.
    pub fn file_by_path(&self, relative: &str) -> Option<&File> {
        self.files
            .get(self.by_path.get(relative.trim_start_matches('/'))?)
    }

    /// Every file, by id.
    pub fn files(&self) -> impl Iterator<Item = &File> {
        self.files.values()
    }

    /// How many files there are.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// How many folders hold something, the root not counted.
    pub fn folder_count(&self) -> usize {
        self.folders.len().saturating_sub(1)
    }

    /// Whether there is nothing to play at all.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether the walk stopped at a limit before it saw everything.
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    /// Where a file is among its folder's files, from 0: what `0x3100`
    /// answers, and what a player shows as `TRACK 003/007` (§7.1).
    pub fn index_in_folder(&self, file: u32) -> Option<u32> {
        let folder = self.folders.get(&self.files.get(&file)?.folder)?;
        let index = folder.files.iter().position(|&id| id == file)?;
        u32::try_from(index).ok()
    }

    /// The file a player's `?\0D <folder> <entry>` lookup names, if it names
    /// one of ours.
    pub fn file_by_lookup(&self, name: &[u8]) -> Option<&File> {
        let (folder, entry) = parse_lookup_name(name)?;
        self.files.get(&entry).filter(|file| file.folder == folder)
    }

    /// The two numbers a file's track info carries, for a player to hand
    /// back in its lookup: the folder's id and the file's.
    pub fn lookup_numbers(&self, file: u32) -> Option<(u32, u32)> {
        let found = self.files.get(&file)?;
        Some((found.folder, found.id))
    }
}

/// The name a player looks a plain file up by: `?` as two bytes of UTF-16LE,
/// then in ASCII `D`, the folder's number and the entry's, in upper-case hex
/// (§7.3: `3f 00 44 20 32 20 35` is `D 2 5`).
pub fn lookup_name(folder: u32, entry: u32) -> Vec<u8> {
    let mut name = vec![0x3f, 0x00];
    name.extend_from_slice(format!("D {folder:X} {entry:X}").as_bytes());
    name
}

/// The two numbers in a `?\0D` lookup name, or `None` for any other name.
pub fn parse_lookup_name(name: &[u8]) -> Option<(u32, u32)> {
    let text = std::str::from_utf8(name.strip_prefix(&[0x3f, 0x00])?).ok()?;
    let mut words = text.strip_prefix("D ")?.split(' ');
    let folder = u32::from_str_radix(words.next()?, 16).ok()?;
    let entry = u32::from_str_radix(words.next()?, 16).ok()?;
    words.next().is_none().then_some((folder, entry))
}

/// The container byte for a file this module lists, by its extension, or
/// `None` for one it does not.
///
/// The formats a CDJ-2000NXS plays, as `plain-usb-plan.md` D4 settled: MP3,
/// AAC in an `.m4a`, WAV and AIFF. Not FLAC: an NXS lists it on its own stick
/// and cannot play it (§7.6).
pub fn container_for(name: &str) -> Option<u8> {
    let (_, extension) = name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "mp3" => Some(1),
        "m4a" => Some(4),
        "wav" => Some(11),
        "aif" | "aiff" => Some(12),
        _ => None,
    }
}

/// Whether a directory is never walked into: hidden ones, the operating
/// systems' clutter, and other DJ programs' folders. The deck's own browser
/// skips the same ones (`folderlibrary.cpp`).
fn is_skipped_directory(name: &str) -> bool {
    name.starts_with('.')
        || [
            "System Volume Information",
            "$RECYCLE.BIN",
            "RECYCLER",
            "LOST.DIR",
            "PIONEER",
            "Engine Library",
            "_Serato_",
        ]
        .iter()
        .any(|skipped| name.eq_ignore_ascii_case(skipped))
}

/// The order a CDJ-2000NXS lists a folder in (§2, measured with `Sort/`):
/// letters A-Z with case ignored, then digits, then the rest of ASCII
/// (punctuation, the space among it) in ASCII order, then everything else by
/// code point. Not numeric: `10 ten` comes before `1 one`, a space sorting
/// after a digit.
///
/// Total: two names differing only in case compare by their bytes.
pub fn collate(left: &str, right: &str) -> Ordering {
    let key = |character: char| -> (u8, u32) {
        if character.is_ascii_alphabetic() {
            (0, u32::from(character.to_ascii_lowercase()))
        } else if character.is_ascii_digit() {
            (1, u32::from(character))
        } else if character.is_ascii() {
            (2, u32::from(character))
        } else {
            (3, u32::from(character))
        }
    };
    left.chars()
        .map(key)
        .cmp(right.chars().map(key))
        .then_with(|| left.cmp(right))
}

/// One folder as the scan found it, before anything is numbered.
#[derive(Debug, Default)]
struct Scanned {
    files: Vec<ScannedFile>,
    folders: Vec<(String, Scanned)>,
}

#[derive(Debug)]
struct ScannedFile {
    name: String,
    relative: String,
    size: u64,
    container: u8,
}

/// List one directory and everything below it, sorted, with the folders that
/// hold nothing playable left out.
fn scan(
    directory: &Path,
    relative: &str,
    depth: usize,
    limits: WalkLimits,
    budget: &mut usize,
    truncated: &mut bool,
) -> Scanned {
    let mut scanned = Scanned::default();
    let Ok(listing) = std::fs::read_dir(directory) else {
        return scanned;
    };
    let mut files = Vec::new();
    let mut folders = Vec::new();
    for entry in listing.flatten() {
        // A name that is not valid Unicode cannot be shown or sent; a FAT
        // stick mounted with `utf8` never has one.
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        // Not followed, so a link back up the tree cannot loop, and not
        // listed: a link is nothing a CDJ's own stick can hold.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if !is_skipped_directory(&name) {
                folders.push(name);
            }
        } else if kind.is_file()
            && !name.starts_with('.')
            && let Some(container) = container_for(&name)
            && let Ok(metadata) = entry.metadata()
            && metadata.len() > 0
        {
            files.push(ScannedFile {
                relative: join(relative, &name),
                name,
                size: metadata.len(),
                container,
            });
        }
    }
    files.sort_by(|left, right| collate(&left.name, &right.name));
    folders.sort_by(|left, right| collate(left, right));

    for file in files {
        if *budget == 0 {
            *truncated = true;
            break;
        }
        *budget -= 1;
        scanned.files.push(file);
    }
    if depth < limits.max_depth {
        for name in folders {
            let below = scan(
                &directory.join(&name),
                &join(relative, &name),
                depth + 1,
                limits,
                budget,
                truncated,
            );
            if !below.files.is_empty() || !below.folders.is_empty() {
                scanned.folders.push((name, below));
            }
        }
    } else if !folders.is_empty() {
        *truncated = true;
    }
    scanned
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch stick, removed when the test ends.
    struct Stick(std::path::PathBuf);

    impl Stick {
        fn new(name: &str, files: &[(&str, usize)]) -> Self {
            let root =
                std::env::temp_dir().join(format!("prolink-folder-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("a scratch directory");
            for (path, size) in files {
                let full = root.join(path);
                if path.ends_with('/') {
                    std::fs::create_dir_all(&full).expect("a folder");
                    continue;
                }
                std::fs::create_dir_all(full.parent().expect("a parent")).expect("its folder");
                std::fs::write(&full, vec![0x55u8; *size]).expect("a file");
            }
            Self(root)
        }
    }

    impl Drop for Stick {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(tree: &FolderTree, folder: u32) -> Vec<String> {
        let found = tree.folder(folder).expect("the folder");
        found
            .folders
            .iter()
            .map(|id| format!("{}/", tree.folder(*id).expect("a folder").name))
            .chain(
                found
                    .files
                    .iter()
                    .map(|id| tree.file(*id).expect("a file").name.clone()),
            )
            .collect()
    }

    #[test]
    fn a_folder_lists_its_folders_then_its_files_in_the_players_order() {
        // §2, measured with `Sort/` on Pioneer's firmware: letters with case
        // ignored, then digits (not numerically, a space after a digit), then
        // ASCII punctuation, then everything else by code point.
        let mut names = vec![
            "Zed", "abc", "ABD", "aardvark", "2 two", "1 one", "10 ten", "_under", "!bang",
            "(paren", "Éclair", "Äpfel", "日本", "Ωmega",
        ];
        names.sort_by(|left, right| collate(left, right));
        assert_eq!(
            names,
            [
                "aardvark", "abc", "ABD", "Zed", "10 ten", "1 one", "2 two", "!bang", "(paren",
                "_under", "Äpfel", "Éclair", "Ωmega", "日本",
            ]
        );
    }

    #[test]
    fn the_root_reads_as_on_plainmp3() {
        // §2: `Filename Artist - Filename Title.mp3`, `SHORT.MP3`, then
        // `01 Full v23.mp3`: letters before digits.
        let stick = Stick::new(
            "root",
            &[
                ("01 Full v23.mp3", 10),
                ("SHORT.MP3", 10),
                ("Filename Artist - Filename Title.mp3", 10),
                ("Order/a.mp3", 10),
                ("Deep/L01/b.mp3", 10),
            ],
        );
        let tree = FolderTree::walk(&stick.0, WalkLimits::default()).expect("it walks");
        assert_eq!(
            names(&tree, ROOT_FOLDER),
            [
                "Deep/",
                "Order/",
                "Filename Artist - Filename Title.mp3",
                "SHORT.MP3",
                "01 Full v23.mp3",
            ]
        );
    }

    #[test]
    fn only_what_a_player_can_play_is_listed() {
        let stick = Stick::new(
            "playable",
            &[
                ("a.mp3", 10),
                ("b.M4A", 10),
                ("c.wav", 10),
                ("d.aif", 10),
                ("e.AIFF", 10),
                ("f.flac", 10),
                ("._a.mp3", 10),
                (".hidden.mp3", 10),
                ("empty.mp3", 0),
                ("cover.jpg", 10),
                ("Empty/", 0),
                ("Only Docs/readme.txt", 10),
                ("PIONEER/MYSETTING.DAT", 10),
                ("PIONEER/x.mp3", 10),
                ("System Volume Information/y.mp3", 10),
                (".Trashes/z.mp3", 10),
                ("Nested/Empty/", 0),
                ("Nested/Deeper/n.mp3", 10),
            ],
        );
        let tree = FolderTree::walk(&stick.0, WalkLimits::default()).expect("it walks");
        assert_eq!(
            names(&tree, ROOT_FOLDER),
            ["Nested/", "a.mp3", "b.M4A", "c.wav", "d.aif", "e.AIFF"]
        );
        let nested = tree.folder(ROOT_FOLDER).expect("root").folders[0];
        assert_eq!(
            names(&tree, nested),
            ["Deeper/"],
            "its empty folder is left out"
        );
        assert_eq!(tree.file_count(), 6);
        assert_eq!(tree.folder_count(), 2);
    }

    #[test]
    fn ids_are_one_space_counted_depth_first() {
        let stick = Stick::new(
            "ids",
            &[
                ("x.mp3", 10),
                ("y.mp3", 10),
                ("A/p.mp3", 10),
                ("B/q.mp3", 10),
            ],
        );
        let tree = FolderTree::walk(&stick.0, WalkLimits::default()).expect("it walks");
        let root = tree.folder(ROOT_FOLDER).expect("root");
        assert_eq!(root.id, 1);
        assert_eq!(root.files, [2, 3], "the root's files first");
        assert_eq!(root.folders, [4, 6], "then each folder, its files after it");
        assert_eq!(tree.file(5).expect("A's file").relative, "A/p.mp3");
        assert_eq!(tree.file_by_path("/A/p.mp3").map(|file| file.id), Some(5));
        assert_eq!(tree.file_by_path("A/p.mp3").map(|file| file.id), Some(5));
        assert_eq!(tree.file_by_path("A/q.mp3"), None);
        assert_eq!(tree.file(7).expect("B's file").folder, 6);
        assert_eq!(tree.index_in_folder(3), Some(1));

        // The same stick walks to the same ids.
        let again = FolderTree::walk(&stick.0, WalkLimits::default()).expect("it walks");
        assert_eq!(again.file(5), tree.file(5));
    }

    #[test]
    fn a_player_opens_a_file_by_its_two_numbers() {
        // E12 f712: `3f 00 44 20 32 20 35`, `D 2 5`.
        assert_eq!(
            lookup_name(2, 5),
            [0x3f, 0x00, 0x44, 0x20, 0x32, 0x20, 0x35]
        );
        // E14 f10367 and f28547: upper-case hex.
        assert_eq!(&lookup_name(0x578, 2)[2..], b"D 578 2");
        assert_eq!(&lookup_name(0x4fc, 0x11)[2..], b"D 4FC 11");
        assert_eq!(
            parse_lookup_name(&lookup_name(0x4fc, 0x11)),
            Some((0x4fc, 0x11))
        );
        assert_eq!(parse_lookup_name(b"Contents"), None);
        assert_eq!(parse_lookup_name(&[0x3f, 0x00, b'D', b' ', b'2']), None);

        let stick = Stick::new("lookup", &[("A/p.mp3", 10)]);
        let tree = FolderTree::walk(&stick.0, WalkLimits::default()).expect("it walks");
        let (folder, entry) = tree.lookup_numbers(3).expect("the file");
        assert_eq!((folder, entry), (2, 3));
        assert_eq!(
            tree.file_by_lookup(&lookup_name(folder, entry))
                .map(|file| file.relative.as_str()),
            Some("A/p.mp3")
        );
        assert_eq!(
            tree.file_by_lookup(&lookup_name(1, entry)),
            None,
            "another folder's"
        );
    }

    #[test]
    fn a_walk_stops_at_its_limits_and_says_so() {
        let stick = Stick::new(
            "limits",
            &[
                ("a.mp3", 10),
                ("b.mp3", 10),
                ("c.mp3", 10),
                ("D1/D2/d.mp3", 10),
            ],
        );
        let tree = FolderTree::walk(
            &stick.0,
            WalkLimits {
                max_files: 2,
                max_depth: 16,
            },
        )
        .expect("it walks");
        assert_eq!(tree.file_count(), 2);
        assert!(tree.is_truncated());

        let shallow = FolderTree::walk(
            &stick.0,
            WalkLimits {
                max_files: 100,
                max_depth: 1,
            },
        )
        .expect("it walks");
        assert_eq!(shallow.file_count(), 3, "D2 is past the depth");
        assert!(shallow.is_truncated());
    }
}
