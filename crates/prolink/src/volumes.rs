// SPDX-License-Identifier: GPL-3.0-only

//! Finding the sticks plugged into *this* machine: rekordbox ones, and sticks of
//! plain music files.
//!
//! A host that wants to offer its own media to the players on the network has
//! to know what it has, and there is no event for that worth waiting on — so
//! this is a scan, cheap enough to run every couple of seconds forever.
//!
//! # Why the label is not the mount point
//!
//! On macOS the mount point *is* the label, so the two agree. On Linux they do
//! not: an automounter names the directory whatever it likes — a Raspberry Pi
//! set up for this puts sticks at `/media/DJ_USB_1` — while the deck shows the
//! filesystem's own label. A DJ reading `DJ_USB_1` off one screen and `NHK_2024`
//! off the other has no way to tell they are the same stick.
//!
//! So the label is resolved properly: mount point → device, through
//! `/proc/self/mounts`, then device → label, through the `/dev/disk/by-label`
//! symlinks. The mount point's own name is the fallback, which is correct on
//! macOS and merely unhelpful elsewhere.

use std::path::{Path, PathBuf};

/// Where `export.pdb` lives on a rekordbox medium, relative to its root.
///
/// The presence of this file *is* the definition of a rekordbox stick: a deck
/// looks for nothing else before it will browse one.
pub const PDB_PATH: &str = "PIONEER/rekordbox/export.pdb";

/// A medium mounted on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// Where it is mounted — the directory holding `PIONEER/`, on a rekordbox
    /// medium.
    pub path: PathBuf,
    /// What the DJ's deck calls it, resolved as described above.
    pub label: String,
    /// Whether it holds a rekordbox library. A volume without one may be a
    /// stick of plain music files, or nothing a player could use at all.
    pub rekordbox: bool,
}

/// Every rekordbox medium currently mounted on this machine.
///
/// Ordered by path, so two scans of an unchanged machine agree — a caller
/// assigning slots by position needs that or a stick would move between slots
/// for no reason.
#[must_use]
pub fn rekordbox_volumes() -> Vec<Volume> {
    let mut found: Vec<Volume> = candidates()
        .into_iter()
        .filter(|path| path.join(PDB_PATH).is_file())
        .map(|path| Volume {
            label: label_of(&path),
            path,
            rekordbox: true,
        })
        .collect();
    found.sort_by(|left, right| left.path.cmp(&right.path));
    found.dedup_by(|left, right| left.path == right.path);
    found
}

/// Every medium mounted on this machine a player might be offered: the
/// rekordbox ones [`rekordbox_volumes`] finds, and every other mounted
/// filesystem, which may be a stick of plain files.
///
/// **A plain candidate must be a mount point**: a directory, not a link, on
/// another device than the directory holding it. `/Volumes` lists the Mac's
/// own disk as a link to `/`, and a `/media/DJ_USB_1` with nothing mounted on
/// it is an empty directory on the card; neither is a stick. Whether one holds
/// anything a player could play is for the medium to find out, by walking it.
///
/// Ordered by path, as [`rekordbox_volumes`] is.
#[must_use]
pub fn local_volumes() -> Vec<Volume> {
    let mut found = rekordbox_volumes();
    for path in candidates() {
        if found.iter().any(|volume| volume.path == path) || !is_mount_point(&path) {
            continue;
        }
        found.push(Volume {
            label: label_of(&path),
            path,
            rekordbox: false,
        });
    }
    found.sort_by(|left, right| left.path.cmp(&right.path));
    found
}

/// Whether a directory is where a filesystem is mounted: on another device
/// than its parent, and not reached through a link.
fn is_mount_point(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(own), Some(parent)) = (std::fs::symlink_metadata(path), path.parent()) else {
        return false;
    };
    own.is_dir() && std::fs::metadata(parent).is_ok_and(|parent| parent.dev() != own.dev())
}

/// The directories a removable medium is mounted under, one level deep.
///
/// **Immediate children only.** An automounter puts a stick at
/// `/media/DJ_USB_1` or `/media/<user>/NHK`, never deeper, and recursing would
/// walk the contents of every stick looking for another stick.
fn candidates() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/media"), PathBuf::from("/Volumes")];
    if let Ok(user) = std::env::var("USER")
        && !user.is_empty()
    {
        roots.push(PathBuf::from("/media").join(&user));
        roots.push(PathBuf::from("/run/media").join(&user));
    }

    let mut volumes = Vec::new();
    for root in roots {
        let Ok(listing) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in listing.flatten() {
            if entry.path().is_dir() {
                volumes.push(entry.path());
            }
        }
    }
    volumes
}

/// The filesystem label of whatever is mounted at `mount_point`, resolved as
/// the module documentation describes; the mount point's own name when there
/// is none to find.
pub fn label_of(mount_point: &Path) -> String {
    let fallback = mount_point
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    let Some(device) = device_at(mount_point) else {
        return fallback;
    };
    let Ok(listing) = std::fs::read_dir("/dev/disk/by-label") else {
        return fallback;
    };
    for entry in listing.flatten() {
        // These are symlinks to block devices, and the target is relative
        // (`../../sda1`); canonicalising resolves it into the absolute form the
        // mount table gives.
        if entry
            .path()
            .canonicalize()
            .is_ok_and(|target| target == device)
        {
            // udev escapes a label the same way the mount table escapes a path.
            return unescape(&entry.file_name().to_string_lossy());
        }
    }
    fallback
}

/// A mounted volume's total and free bytes, or `None` if the filesystem will
/// not say.
///
/// Free as an unprivileged user sees it, as `df` reports it. The volume is
/// read once, when it is offered: a deck mounts its sticks read-only.
pub fn size_of(mount_point: &Path) -> Option<(u64, u64)> {
    let stats = rustix::fs::statvfs(mount_point).ok()?;
    let block = stats.f_frsize;
    Some((
        stats.f_blocks.checked_mul(block)?,
        stats.f_bavail.checked_mul(block)?,
    ))
}

/// The block device mounted at a path, from the kernel's mount table.
fn device_at(mount_point: &Path) -> Option<PathBuf> {
    let table = std::fs::read_to_string("/proc/self/mounts").ok()?;
    for line in table.lines() {
        let mut fields = line.split(' ');
        let device = fields.next()?;
        let Some(at) = fields.next() else { continue };
        if Path::new(&unescape(at)) == mount_point {
            return Path::new(device).canonicalize().ok();
        }
    }
    None
}

/// Decode the octal escapes the mount table uses for spaces and the like.
///
/// A stick called `MY SET` appears as `MY\040SET`, and comparing the escaped
/// form against a real path never matches — silently, and the symptom is a
/// stick that is found but shows the wrong name.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes.get(index) == Some(&b'\\')
            && let Some(digits) = text.get(index + 1..index + 4)
            && let Ok(value) = u8::from_str_radix(digits, 8)
        {
            out.push(char::from(value));
            index += 4;
            continue;
        }
        if let Some(rest) = text.get(index..)
            && let Some(character) = rest.chars().next()
        {
            out.push(character);
            index += character.len_utf8();
        } else {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_escapes_are_decoded() {
        assert_eq!(unescape("/media/MY\\040SET"), "/media/MY SET");
        assert_eq!(unescape("plain"), "plain");
        // A backslash that starts nothing is kept, rather than eaten.
        assert_eq!(unescape("a\\b"), "a\\b");
        assert_eq!(unescape("trailing\\"), "trailing\\");
    }

    #[test]
    fn a_directory_is_a_mount_point_only_on_a_device_of_its_own() {
        // The root is on its own device and has no parent to compare with; a
        // directory beside this file is on the same device as its parent.
        assert!(!is_mount_point(Path::new("/")));
        assert!(!is_mount_point(Path::new(env!("CARGO_MANIFEST_DIR"))));
        assert!(!is_mount_point(Path::new("/nonexistent/volume")));
        for volume in local_volumes() {
            assert!(
                volume.rekordbox || is_mount_point(&volume.path),
                "{} is neither",
                volume.path.display()
            );
        }
    }

    #[test]
    fn a_volumes_size_is_its_filesystems() {
        let (total, free) = size_of(Path::new("/")).expect("the root filesystem has a size");
        assert!(total > 0);
        assert!(free <= total, "{free} free of {total}");
        assert_eq!(size_of(Path::new("/nonexistent/volume")), None);
    }

    #[test]
    fn a_label_falls_back_to_the_mount_point_name() {
        // Nothing is mounted here, so neither lookup can succeed and the
        // fallback is all there is. On a machine where /proc does not exist at
        // all this is also the only answer, which is why it is not an error.
        assert_eq!(label_of(Path::new("/nonexistent/NHK_2024")), "NHK_2024");
    }

    #[test]
    fn scanning_a_machine_with_no_sticks_finds_none_rather_than_failing() {
        // Runs against the real machine: the assertion is on the invariant, not
        // on what happens to be plugged in.
        for volume in rekordbox_volumes() {
            assert!(
                volume.path.join(PDB_PATH).is_file(),
                "{} was returned without an export.pdb",
                volume.path.display()
            );
        }
    }
}
