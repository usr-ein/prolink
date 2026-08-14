//! Every page in a pdb, by walking the file rather than the table directory.
//!
//! The directory says where a table's chain starts and ends; this says what is
//! actually in the file. The two disagreeing is the interesting case.
use prolink_rekordbox::pdb::{PAGE_SIZE, PageHeader};

fn main() {
    let path = std::env::args().nth(1).expect("usage: all_pages <file.pdb>");
    let bytes = std::fs::read(&path).expect("read");
    let pages = bytes.len() / PAGE_SIZE as usize;
    let mut per_type: std::collections::BTreeMap<u32, (usize, u32)> = Default::default();
    for page in 0..pages {
        let off = page * PAGE_SIZE as usize;
        let mut cur = std::io::Cursor::new(&bytes[off..]);
        let Ok(header) = <PageHeader as binrw::BinRead>::read(&mut cur) else {
            continue;
        };
        if header.page_index as usize != page {
            continue; // not a real page header
        }
        let e = per_type.entry(header.page_type.0).or_insert((0, 0));
        e.0 += 1;
        if !header.is_strange() {
            e.1 += u32::from(header.num_rows());
        }
    }
    println!("{path}: {pages} pages");
    for (ty, (count, rows)) in per_type {
        println!("  type {ty:<3} pages={count:<4} declared_rows={rows}");
    }
}
