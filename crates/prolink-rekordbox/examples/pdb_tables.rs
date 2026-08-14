//! Every table a pdb declares, and how many rows we read from each.
//!
//! The counterpart to pl_probe: that one asks whether the rows we read are
//! attributed correctly, this one asks whether we read them at all.
use prolink_rekordbox::pdb::{Pdb, PlaylistEntryRow, PlaylistTreeRow};

fn main() {
    let path = std::env::args().nth(1).expect("usage: pdb_tables <file.pdb>");
    let bytes = std::fs::read(&path).expect("read");
    match Pdb::new(&bytes) {
        Ok(pdb) => {
            println!("{path}: parsed");
            for table in pdb.tables() {
                println!(
                    "  type={:?} raw={} first_page={} last_page={}",
                    table.page_type, table.page_type.0, table.first_page, table.last_page
                );
            }
            println!("  playlist_tree rows: {}", pdb.rows::<PlaylistTreeRow>().len());
            println!("  playlist_entry rows: {}", pdb.rows::<PlaylistEntryRow>().len());
        }
        Err(e) => println!("{path}: parse failed: {e}"),
    }
}
