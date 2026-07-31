// SPDX-License-Identifier: GPL-3.0-only

//! The read-only tree an NFS server hands out.
//!
//! Whatever backs it — a mounted USB stick, or a synthesised `/PIONEER/` layout
//! generated from some other library — the wire layer above should not have to
//! change, so it talks only to this.
//!
//! # Filehandles, and the one place a CDJ breaks the spec
//!
//! RFC 1094 says a filehandle is 32 *opaque* bytes that the client echoes back
//! verbatim. **A CDJ-2000NXS does not.** Handed a handle, it returns one whose
//! first twelve bytes match ours and whose remaining twenty it has rewritten
//! with its own file reference:
//!
//! ```text
//! served:   8a5edab282632443219e051e 4ade2d1d5bbc671c781051bf1437897cbdfea0f1
//! returned: 8a5edab282632443219e051e 03012d0000001b58000000000303010000000162
//!           |____ first 12 kept ____| |______ replaced by the player _______|
//! ```
//!
//! That fits the shape of a real player's own handles — a four-byte value
//! repeated three times followed by zeros — so the leading twelve bytes are
//! evidently the volume identity and the rest is the player's own file
//! reference, which it feels free to overwrite.
//!
//! A server that trusts the spec works perfectly for browsing and fails at
//! exactly the moment a DJ loads a track (F28). This table is therefore keyed on
//! [`FileHandleKey`], which is that twelve-byte prefix and a distinct type, so
//! "I looked the handle up correctly" is a property of the type rather than of
//! remembering to slice.
//!
//! # Two media need two subtrees
//!
//! A handle is a hash of its path. Two media sharing a root would mint identical
//! handles for the same relative path — the root most obviously — and after
//! truncation nothing would distinguish them. So each medium is grafted under
//! its own prefix, and [`Vfs::mount`] takes one.
//!
//! # Unicode, and why an exact match is not enough
//!
//! A name can reach us spelled differently from how the filesystem spells it,
//! and the two must still match.
//!
//! A rekordbox medium is **FAT32** — case-insensitive, case-preserving — and
//! `export.pdb` does not necessarily record a directory entry's case: on the
//! reference stick the database says `Gesaffelstein` where the directory is
//! `GESAFFELSTEIN`. The pdb also stores **NFC** where the filesystem reports
//! **NFD** (`カガミ`), because rekordbox wrote the two through different APIs.
//!
//! A player looking up the path the database gave it therefore asks for a name
//! that, byte for byte, is not the one in our listing, and an exact comparison
//! answers `NFSERR_NOENT` for a file that is plainly there. So: match exactly
//! first, fall back to comparing `NFC(name).casefold()`, and **always return the
//! handle for the name as stored** — hashing the requested spelling would mint a
//! handle that is not in the table, and every later use of it would come back
//! `NFSERR_STALE` (O6).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use prolink_proto::rpc::nfs2::{FType, Fattr, FileHandle, FileHandleKey};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// Fixed timestamp for synthesised attributes.
///
/// Deterministic on purpose: byte-identical replies across runs make a capture
/// diff meaningful.
const EPOCH: u32 = 1_600_000_000;

/// A file or a directory in the served tree.
#[derive(Clone, Debug)]
pub enum Node {
    /// A directory, with its children in the order they will be listed.
    Directory {
        /// Child names, in listing order.
        children: Vec<String>,
    },
    /// A file whose bytes are held in memory.
    Memory {
        /// The contents.
        data: Vec<u8>,
    },
    /// A file whose bytes stay on disk until a `READ` asks for them.
    ///
    /// This is what makes serving a mounted USB stick practical: reading a 60 GB
    /// library into memory to answer an 8 KB request is not an option.
    Disk {
        /// Where to read from.
        path: PathBuf,
        /// Its size, taken once at mount time.
        size: u64,
    },
}

impl Node {
    /// Whether this is a directory.
    pub fn is_dir(&self) -> bool {
        matches!(self, Self::Directory { .. })
    }

    /// The size an NFS `fattr` should report.
    pub fn size(&self) -> u64 {
        match self {
            Self::Directory { .. } => 0,
            Self::Memory { data } => data.len().try_into().unwrap_or(u64::MAX),
            Self::Disk { size, .. } => *size,
        }
    }
}

/// One entry in the table: its path, its node, and its handle.
#[derive(Clone, Debug)]
struct Entry {
    /// The path this entry's handle was derived from.
    path: String,
    node: Node,
}

/// A read-only tree addressable by filehandle.
#[derive(Debug, Default)]
pub struct Vfs {
    entries: BTreeMap<FileHandleKey, Entry>,
}

impl Vfs {
    /// An empty tree with just a root directory.
    pub fn new() -> Self {
        let mut vfs = Self {
            entries: BTreeMap::new(),
        };
        vfs.insert(
            "/",
            Node::Directory {
                children: Vec::new(),
            },
        );
        vfs
    }

    /// The handle for a path.
    ///
    /// A truncated SHA-256, so the same tree yields the same handles and a
    /// client's cached root handle survives a restart of this process.
    pub fn handle_for(path: &str) -> FileHandle {
        let digest = Sha256::digest(path.as_bytes());
        let mut handle = [0u8; FileHandle::LEN];
        for (slot, byte) in handle.iter_mut().zip(digest.iter()) {
            *slot = *byte;
        }
        FileHandle(handle)
    }

    /// The root directory's handle, which is what a `MNT` reply carries.
    pub fn root(&self) -> FileHandle {
        Self::handle_for("/")
    }

    /// Graft a real directory into the tree under `prefix`.
    ///
    /// Only the structure and the file sizes are walked up front; contents are
    /// read per request. Give each medium its own prefix — see the module
    /// documentation for why sharing one would make their handles collide.
    ///
    /// A file that cannot be read or that vanishes between the walk and the
    /// `stat` is skipped: that costs one file, where propagating the error would
    /// cost the whole medium.
    pub fn mount(&mut self, prefix: &str, directory: &Path) -> std::io::Result<usize> {
        let prefix = prefix.trim_matches('/');
        let mount_point = format!("/{prefix}");
        // The mount point itself has to exist before anything can be added to
        // it, and it has to be a child of the root or a `LOOKUP` from the root
        // handle — which is where every walk starts — will not find it.
        self.ensure_parents(&mount_point);
        if !self
            .entries
            .contains_key(&Self::handle_for(&mount_point).key())
        {
            self.insert(
                &mount_point,
                Node::Directory {
                    children: Vec::new(),
                },
            );
        }
        if let Some((parent, name)) = split(&mount_point) {
            self.add_child(&parent, &name);
        }

        let mut mounted = 0;
        let mut stack = vec![(directory.to_path_buf(), mount_point)];

        while let Some((disk, virtual_path)) = stack.pop() {
            let Ok(listing) = std::fs::read_dir(&disk) else {
                continue;
            };
            let mut entries: Vec<_> = listing.flatten().collect();
            entries.sort_by_key(std::fs::DirEntry::file_name);

            for entry in entries {
                let name = entry.file_name().to_string_lossy().into_owned();
                let child_path = join(&virtual_path, &name);
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };

                if metadata.is_dir() {
                    self.insert(
                        &child_path,
                        Node::Directory {
                            children: Vec::new(),
                        },
                    );
                    self.add_child(&virtual_path, &name);
                    stack.push((entry.path(), child_path));
                } else if metadata.is_file() {
                    self.insert(
                        &child_path,
                        Node::Disk {
                            path: entry.path(),
                            size: metadata.len(),
                        },
                    );
                    self.add_child(&virtual_path, &name);
                    mounted += 1;
                }
            }
        }
        Ok(mounted)
    }

    /// Add a file held in memory, creating any directories it needs.
    pub fn add_file(&mut self, path: &str, data: Vec<u8>) {
        self.ensure_parents(path);
        if let Some((parent, name)) = split(path) {
            self.add_child(&parent, &name);
        }
        self.insert(path, Node::Memory { data });
    }

    /// The node a handle names, or `None` — which is `NFSERR_STALE`.
    ///
    /// Matches on [`FileHandleKey`] only, because a CDJ rewrites the rest.
    pub fn resolve(&self, handle: FileHandle) -> Option<&Node> {
        self.entries.get(&handle.key()).map(|entry| &entry.node)
    }

    /// The path a handle names, for logging and for building child paths.
    pub fn path_of(&self, handle: FileHandle) -> Option<&str> {
        self.entries
            .get(&handle.key())
            .map(|entry| entry.path.as_str())
    }

    /// Walk one path component.
    ///
    /// Returns the handle **for the name as stored**, never for the name as
    /// asked for; see the module documentation.
    pub fn lookup(&self, directory: FileHandle, name: &str) -> Option<(FileHandle, &Node)> {
        let parent = self.entries.get(&directory.key())?;
        let Node::Directory { children } = &parent.node else {
            return None;
        };

        // The exact hit is what almost every lookup takes, so a genuinely
        // case-sensitive backing tree still resolves two names differing only in
        // case correctly. Only on a miss do we fall back.
        let stored = children
            .iter()
            .find(|child| child.as_str() == name)
            .or_else(|| children.iter().find(|child| fold(child) == fold(name)))?;

        let path = join(&parent.path, stored);
        let handle = Self::handle_for(&path);
        Some((handle, &self.entries.get(&handle.key())?.node))
    }

    /// The children of a directory, in listing order.
    pub fn read_dir(&self, directory: FileHandle) -> Option<&[String]> {
        match self.resolve(directory)? {
            Node::Directory { children } => Some(children),
            _ => None,
        }
    }

    /// Read a byte range.
    ///
    /// Returns `None` for a handle that names nothing or names a directory. A
    /// file that vanished mid-session — the medium was ejected — reads as empty
    /// rather than taking the server down.
    pub fn read(&self, handle: FileHandle, offset: u64, count: usize) -> Option<Vec<u8>> {
        match self.resolve(handle)? {
            Node::Directory { .. } => None,
            Node::Memory { data } => {
                let start = usize::try_from(offset)
                    .unwrap_or(usize::MAX)
                    .min(data.len());
                let end = start.saturating_add(count).min(data.len());
                Some(data.get(start..end).unwrap_or_default().to_vec())
            }
            Node::Disk { path, .. } => Some(read_range(path, offset, count)),
        }
    }

    /// Synthesise the attributes an NFS reply carries.
    ///
    /// `fileid` is the handle's leading word, which is what a real player puts
    /// there — true in every observed reply, so deriving it this way is
    /// consistent with the hardware for free.
    pub fn attributes(&self, handle: FileHandle) -> Option<Fattr> {
        let node = self.resolve(handle)?;
        let size = u32::try_from(node.size()).unwrap_or(u32::MAX);
        Some(Fattr {
            ftype: if node.is_dir() {
                FType::DIR
            } else {
                FType::REG
            },
            mode: if node.is_dir() { 0o040_755 } else { 0o100_644 },
            nlink: if node.is_dir() { 2 } else { 1 },
            uid: 0,
            gid: 0,
            size,
            blocksize: 512,
            rdev: 0,
            blocks: size / 512 + u32::from(size % 512 != 0),
            fsid: 1,
            fileid: handle.fileid(),
            atime_sec: EPOCH,
            atime_usec: 0,
            mtime_sec: EPOCH,
            mtime_usec: 0,
            ctime_sec: EPOCH,
            ctime_usec: 0,
        })
    }

    /// How many entries the tree holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the tree holds nothing but its root.
    pub fn is_empty(&self) -> bool {
        self.entries.len() <= 1
    }

    fn insert(&mut self, path: &str, node: Node) {
        let handle = Self::handle_for(path);
        self.entries.insert(
            handle.key(),
            Entry {
                path: path.to_owned(),
                node,
            },
        );
    }

    fn ensure_parents(&mut self, path: &str) {
        let mut walked = String::new();
        let components: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let Some((_, parents)) = components.split_last() else {
            return;
        };
        for component in parents {
            let parent = if walked.is_empty() {
                "/".to_owned()
            } else {
                walked.clone()
            };
            walked = join(&parent, component);
            if !self.entries.contains_key(&Self::handle_for(&walked).key()) {
                self.insert(
                    &walked,
                    Node::Directory {
                        children: Vec::new(),
                    },
                );
            }
            self.add_child(&parent, component);
        }
    }

    fn add_child(&mut self, directory: &str, name: &str) {
        let key = Self::handle_for(directory).key();
        if let Some(Entry {
            node: Node::Directory { children },
            ..
        }) = self.entries.get_mut(&key)
            && !children.iter().any(|child| child == name)
        {
            children.push(name.to_owned());
        }
    }
}

/// The key two spellings of the same name must agree on.
fn fold(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

fn join(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn split(path: &str) -> Option<(String, String)> {
    let (parent, name) = path.rsplit_once('/')?;
    Some((
        if parent.is_empty() {
            "/".to_owned()
        } else {
            parent.to_owned()
        },
        name.to_owned(),
    ))
}

fn read_range(path: &Path, offset: u64, count: usize) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return Vec::new();
    }
    let mut buffer = vec![0u8; count];
    let mut filled = 0;
    while filled < count {
        match file.read(buffer.get_mut(filled..).unwrap_or_default()) {
            Ok(0) | Err(_) => break,
            Ok(read) => filled += read,
        }
    }
    buffer.truncate(filled);
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Vfs {
        let mut vfs = Vfs::new();
        vfs.add_file("/C/PIONEER/rekordbox/export.pdb", b"pdb".to_vec());
        vfs.add_file("/C/Contents/GESAFFELSTEIN/track.mp3", b"audio".to_vec());
        vfs.add_file(
            "/C/Contents/\u{30ab}\u{3099}\u{30ab}\u{3099}\u{30df}.mp3",
            b"nfd".to_vec(),
        );
        vfs
    }

    #[test]
    fn a_handle_survives_a_cdj_rewriting_its_tail() {
        // A CDJ keeps the first twelve bytes and overwrites the other twenty
        // with its own file reference (F28).
        let vfs = tree();
        let served = Vfs::handle_for("/C/PIONEER");
        let mut returned = served;
        returned.0[12..].copy_from_slice(&[0xab; 20]);

        assert_ne!(served, returned, "the bytes really are different");
        assert!(vfs.resolve(returned).is_some(), "and it must still resolve");
        assert_eq!(vfs.path_of(returned), Some("/C/PIONEER"));
    }

    #[test]
    fn two_media_do_not_share_a_handle() {
        // A handle is a hash of the path, so a shared root would mint identical
        // handles for the same relative path.
        assert_ne!(Vfs::handle_for("/C/PIONEER"), Vfs::handle_for("/B/PIONEER"));
        assert_ne!(Vfs::handle_for("/C"), Vfs::handle_for("/B"));
    }

    #[test]
    fn a_lookup_walks_one_component() {
        let vfs = tree();
        let (contents, node) = vfs.lookup(Vfs::handle_for("/C"), "Contents").unwrap();
        assert!(node.is_dir());
        assert_eq!(vfs.path_of(contents), Some("/C/Contents"));
    }

    #[test]
    fn a_name_differing_only_in_case_still_resolves() {
        // export.pdb says `Gesaffelstein` where the directory is
        // `GESAFFELSTEIN`; a FAT32 driver does not notice and neither may we.
        let vfs = tree();
        let contents = Vfs::handle_for("/C/Contents");
        let (handle, _) = vfs
            .lookup(contents, "Gesaffelstein")
            .expect("the fold must match");
        assert_eq!(
            vfs.path_of(handle),
            Some("/C/Contents/GESAFFELSTEIN"),
            "the handle must be for the name as stored, or every later use is STALE",
        );
    }

    #[test]
    fn a_name_differing_only_in_normalisation_still_resolves() {
        // The pdb stores NFC where the filesystem reports NFD, because
        // rekordbox wrote them through different APIs.
        let vfs = tree();
        let composed = "\u{30ac}\u{30ac}\u{30df}.mp3";
        let decomposed = "\u{30ab}\u{3099}\u{30ab}\u{3099}\u{30df}.mp3";
        assert_ne!(
            composed.as_bytes(),
            decomposed.as_bytes(),
            "genuinely different bytes"
        );

        let (handle, _) = vfs
            .lookup(Vfs::handle_for("/C/Contents"), composed)
            .expect("NFC must find NFD");
        assert_eq!(
            vfs.path_of(handle),
            Some(format!("/C/Contents/{decomposed}").as_str())
        );
    }

    #[test]
    fn an_exact_match_wins_over_a_folded_one() {
        let mut vfs = Vfs::new();
        vfs.add_file("/x/TRACK.mp3", b"upper".to_vec());
        vfs.add_file("/x/track.mp3", b"lower".to_vec());
        let (handle, _) = vfs.lookup(Vfs::handle_for("/x"), "track.mp3").unwrap();
        assert_eq!(
            vfs.read(handle, 0, 16).as_deref(),
            Some(b"lower".as_slice())
        );
    }

    #[test]
    fn an_unknown_handle_is_stale_not_a_panic() {
        let vfs = tree();
        assert!(vfs.resolve(Vfs::handle_for("/nowhere")).is_none());
        assert!(vfs.read(Vfs::handle_for("/nowhere"), 0, 16).is_none());
    }

    #[test]
    fn a_read_past_the_end_is_short_not_an_error() {
        let vfs = tree();
        let handle = Vfs::handle_for("/C/PIONEER/rekordbox/export.pdb");
        assert_eq!(
            vfs.read(handle, 0, 8192).as_deref(),
            Some(b"pdb".as_slice())
        );
        assert_eq!(vfs.read(handle, 2, 8192).as_deref(), Some(b"b".as_slice()));
        assert_eq!(vfs.read(handle, 99, 8192).as_deref(), Some(b"".as_slice()));
    }

    #[test]
    fn a_directory_cannot_be_read() {
        let vfs = tree();
        assert!(vfs.read(Vfs::handle_for("/C"), 0, 16).is_none());
    }

    #[test]
    fn attributes_take_their_fileid_from_the_handle() {
        let vfs = tree();
        let handle = Vfs::handle_for("/C/PIONEER/rekordbox/export.pdb");
        let attributes = vfs.attributes(handle).unwrap();
        assert_eq!(attributes.fileid, handle.fileid());
        assert_eq!(attributes.ftype, FType::REG);
        assert_eq!(attributes.size, 3);
    }

    #[test]
    fn a_mounted_directory_is_walkable() {
        let root = std::env::temp_dir().join(format!("prolink-vfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("PIONEER/rekordbox")).unwrap();
        std::fs::write(root.join("PIONEER/rekordbox/export.pdb"), b"real bytes").unwrap();

        let mut vfs = Vfs::new();
        let mounted = vfs.mount("C", &root).unwrap();
        assert_eq!(mounted, 1);

        let (pioneer, _) = vfs.lookup(Vfs::handle_for("/C"), "PIONEER").unwrap();
        let (rekordbox, _) = vfs.lookup(pioneer, "rekordbox").unwrap();
        let (pdb, node) = vfs.lookup(rekordbox, "export.pdb").unwrap();
        assert!(!node.is_dir());
        assert_eq!(vfs.read(pdb, 5, 5).as_deref(), Some(b"bytes".as_slice()));

        std::fs::remove_dir_all(&root).unwrap();
    }
}
