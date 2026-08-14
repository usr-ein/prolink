//! Walk the playlist_entries chain page by page, and say what each page claims.
//!
//! pl_probe answers "are the rows we read attributed correctly". This answers
//! the prior question: are we reading every row the file holds? A chain that
//! stops early, or a page whose declared rows exceed what we decode, loses
//! entries silently and always from the end.
use prolink_rekordbox::pdb::{PAGE_SIZE, PageHeader, PageType, Pdb, PlaylistEntryRow};

fn main() {
    let path = std::env::args().nth(1).expect("usage: entries_pages <export.pdb>");
    let bytes = std::fs::read(&path).expect("read");
    let pdb = Pdb::new(&bytes).expect("parse");

    let table = pdb
        .tables()
        .iter()
        .find(|t| t.page_type == PageType::PLAYLIST_ENTRIES)
        .expect("no playlist_entries table");
    println!(
        "playlist_entries: first_page={} last_page={}",
        table.first_page, table.last_page
    );

    // Follow the chain the same way the reader does, and report each hop.
    let mut page = table.first_page;
    let mut seen = std::collections::HashSet::new();
    let mut hops = 0;
    let mut declared_total: u32 = 0;
    loop {
        if !seen.insert(page) {
            println!("  page {page}: ALREADY SEEN -- cycle, stopping");
            break;
        }
        let off = (u64::from(page) * PAGE_SIZE) as usize;
        if off + PAGE_SIZE as usize > bytes.len() {
            println!("  page {page}: past end of file, stopping");
            break;
        }
        let mut cur = std::io::Cursor::new(&bytes[off..]);
        let header: PageHeader = binrw::BinRead::read(&mut cur).expect("page header");
        let strange = header.is_strange();
        if !strange {
            declared_total += u32::from(header.num_rows());
        }
        hops += 1;
        println!(
            "  page {page}: type={:?} next={} strange={} num_rows={}",
            header.page_type,
            header.next_page,
            strange,
            header.num_rows()
        );
        if page == table.last_page {
            println!("  ^^ this is last_page, but next_page={} -- continuing to see what is there", header.next_page);
        }
        if header.next_page == 0 || hops > 5000 {
            println!("  chain ended at next_page={}", header.next_page);
            break;
        }
        page = header.next_page;
    }
    println!("pages walked: {hops}, rows declared by headers: {declared_total}");
    println!("rows the reader returns: {}", pdb.rows::<PlaylistEntryRow>().len());
}
