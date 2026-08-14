//! What a pdb actually says about its playlists, and whether we lose any of it.
//!
//! Written to answer "this playlist shows up empty on the deck, is that us?" --
//! a question that cannot be answered by reading the parser, because the two
//! possible causes look identical from the outside: entries dropped on the way
//! in, or entries that were never in the export.
//!
//! It prints every playlist tree row with the number of entry rows that name
//! it, plus the count of entries whose playlist_id matches no tree row. That
//! last number is the one that says whether the loss is ours: zero means every
//! entry in the file was attributed to a playlist, so an empty playlist is
//! empty in the export.
//!
//!     cargo run --example pl_probe -p prolink-rekordbox -- /path/to/export.pdb
//!
//! MediaRegistry keeps a copy of whatever it last ingested at
//! ~/.mixxx/last-ingest.pdb, which is where the deck's own copy comes from.
use prolink_rekordbox::pdb::{Pdb, PlaylistEntryRow, PlaylistTreeRow};

fn main() {
    let path = std::env::args().nth(1).expect("usage: pl_probe <export.pdb>");
    let bytes = std::fs::read(&path).expect("read the pdb");
    let pdb = Pdb::new(&bytes).expect("parse the pdb");

    let tree = pdb.rows::<PlaylistTreeRow>();
    let entries = pdb.rows::<PlaylistEntryRow>();
    println!("tree rows: {}, entry rows: {}", tree.len(), entries.len());

    let mut counts: std::collections::BTreeMap<u32, usize> = Default::default();
    for entry in &entries {
        *counts.entry(entry.playlist_id).or_default() += 1;
    }
    for row in &tree {
        println!(
            "id={:<4} parent={:<4} folder={:<5} entries={:<5} {}",
            row.id,
            row.parent_id,
            row.node_is_folder != 0,
            counts.get(&row.id).copied().unwrap_or(0),
            row.name.text
        );
    }
    let orphaned: usize = counts
        .iter()
        .filter(|(id, _)| !tree.iter().any(|row| row.id == **id))
        .map(|(_, n)| *n)
        .sum();
    println!("entries whose playlist_id matches no tree row: {orphaned}");
}
