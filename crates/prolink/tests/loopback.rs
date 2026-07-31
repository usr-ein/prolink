// SPDX-License-Identifier: GPL-3.0-only

//! Our servers against our clients, over loopback.
//!
//! Every other test in this workspace checks one side of one protocol. This one
//! checks that the halves fit: a real `export.pdb` goes into a `Medium`, the
//! server serves it, the client browses it, and what comes back out is what went
//! in.
//!
//! It is not a substitute for hardware — two implementations agreeing is weaker
//! evidence than one implementation agreeing with a CDJ, and the codecs are
//! separately pinned against 33 captures of real traffic for exactly that
//! reason. What this proves is the wiring: that the server answers the requests
//! the client actually sends, in the shapes it expects, in the order a browse
//! goes in.
//!
//! Nothing here binds a privileged port. The portmapper lives on an ephemeral
//! one, which is why `NfsConfig::portmap_port` is configurable at all — against
//! real hardware it must be 111 or a deck never looks (F46).

// An assertion *is* the failure mode of a test; propagating errors carefully
// would report them as passes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::as_conversions,
    clippy::cast_possible_truncation
)]

use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use prolink::consume::{DbClient, DbConfig, NfsClient};
use prolink::serve::dbserver::{DbServer, DbServerConfig};
use prolink::serve::nfs::{NfsConfig as ServerNfsConfig, NfsServer};
use prolink::serve::{Medium, ServedSlot, Vfs};
use prolink::{BrowsableDeviceNumber, Slot};
use prolink_proto::dbserver::{
    CamelotKey, Drill, ItemType, MediaInfo, MenuTarget, MessageKind, SortOrder, TrackType,
};
use prolink_rekordbox::Library;

const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

fn export_pdb() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/export.pdb");
    std::fs::read(path).expect("the committed rekordbox export")
}

fn library() -> Library {
    Library::parse(&export_pdb()).expect("it parses")
}

fn device(number: u8) -> BrowsableDeviceNumber {
    BrowsableDeviceNumber::new(number).expect("a browsable device number")
}

/// A dbserver on an ephemeral port with the real library in the USB slot.
async fn serve_usb() -> (DbServer, u16) {
    let medium = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "TEST STICK"));
    let server = DbServer::start(
        DbServerConfig {
            device: device(4),
            address: LOOPBACK,
            port: 0,
            // 12523 is fixed on the wire and a second test would collide with
            // it; the client is told the port directly instead.
            query_port: None,
        },
        [medium],
    )
    .await
    .expect("the dbserver starts");
    let port = server.port();
    (server, port)
}

async fn client(port: u16) -> DbClient {
    DbClient::connect_at(LOOPBACK, port, device(2), DbConfig::default())
        .await
        .expect("the client connects and introduces itself")
}

#[tokio::test]
async fn a_client_sees_the_root_menu_a_deck_would_see() {
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let root = client.root_menu(Slot::USB).await.expect("the root menu");
    let labels: Vec<&str> = root
        .iter()
        .filter_map(|item| prolink_proto::dbserver::unwrap_menu_label(&item.label1))
        .collect();

    // A deck renders these and nothing else. An unimplemented category and an
    // empty one look identical on its screen, so the set served is a
    // user-visible surface rather than an internal detail.
    for expected in ["ARTIST", "ALBUM", "TRACK", "GENRE", "KEY", "PLAYLIST"] {
        assert!(labels.contains(&expected), "no {expected} in {labels:?}");
    }
    // Category items carry no flags, unlike a track row.
    assert!(
        root.iter().all(|item| item.flags == 0),
        "a category is not openable as a track"
    );
    assert!(
        root.iter().all(|item| item.label1.starts_with('\u{fffa}')),
        "labels must be wrapped, or a deck renders them and then declines to open them (F26)",
    );
}

#[tokio::test]
async fn a_client_can_list_every_track() {
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let tracks = client
        .tracks(Slot::USB, SortOrder::DEFAULT)
        .await
        .expect("the track list");
    assert_eq!(
        tracks.len(),
        library().tracks.len(),
        "every track, paged 64 at a time"
    );
    assert!(
        tracks.iter().all(|item| !item.label1.is_empty()),
        "every row has a title"
    );
}

#[tokio::test]
async fn the_sort_order_selects_the_second_column() {
    // The feature that makes sorting useful rather than cosmetic: an item's type
    // is `(column field type << 8) | 0x04`, so 0x0704 is not "title and artist"
    // but "a track whose second column is the ARTIST field" (F43).
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let by_artist = client
        .tracks(Slot::USB, SortOrder::ARTIST)
        .await
        .expect("sorted by artist");
    let first = by_artist.first().expect("a first row");
    assert_eq!(first.item_type.0 & 0xff, 0x04);
    assert_eq!(
        first.item_type.0 >> 8,
        ItemType::ARTIST.0,
        "column 2 is the artist"
    );

    let by_bpm = client
        .tracks(Slot::USB, SortOrder::BPM)
        .await
        .expect("sorted by BPM");
    let first = by_bpm.first().expect("a first row");
    assert_eq!(
        first.item_type.0 >> 8,
        ItemType::TEMPO.0,
        "column 2 is the tempo"
    );
    assert!(
        first.label2.is_empty(),
        "a numeric column sends an empty label and the raw number in argument 0; the deck \
         formats it",
    );

    let tempos: Vec<u32> = by_bpm.iter().map(|item| item.argument0).collect();
    assert!(
        tempos.windows(2).all(|pair| pair[0] <= pair[1]),
        "and it really is sorted"
    );
}

#[tokio::test]
async fn a_client_can_drill_from_a_category_to_tracks() {
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let artists = client
        .category(Slot::USB, MessageKind::MENU_ARTIST, SortOrder::DEFAULT)
        .await
        .expect("the artists");
    assert!(!artists.is_empty());

    // Drilling one level into an artist gives its albums, not its tracks.
    let artist = artists
        .iter()
        .find(|item| item.id != prolink_proto::dbserver::FILTER_ALL);
    let artist = artist.expect("a real artist, not the ALL row");
    let one_level_into_artist = Drill {
        depth: 1,
        category: u8::try_from(MessageKind::MENU_ARTIST.0 & 0xff).expect("a category byte"),
    };
    let albums = client
        .drill(
            Slot::USB,
            one_level_into_artist,
            SortOrder::DEFAULT,
            &[artist.id],
        )
        .await
        .expect("that artist's albums");
    assert!(
        !albums.is_empty(),
        "an artist with no albums would not be listed"
    );
}

#[tokio::test]
async fn searching_finds_a_track_that_is_there_and_none_that_is_not() {
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let library = library();
    let sample = library
        .tracks
        .values()
        .find(|track| track.title.len() > 4)
        .expect("a track");
    let term: String = sample.title.chars().take(4).collect();

    let found = client
        .search(Slot::USB, &term, SortOrder::DEFAULT)
        .await
        .expect("a search");
    assert!(!found.is_empty(), "searching for {term:?} found nothing");

    let missing = client
        .search(Slot::USB, "zzzzz no such track zzzzz", SortOrder::DEFAULT)
        .await
        .expect("a search that matches nothing is still an answer");
    assert!(missing.is_empty());
}

#[tokio::test]
async fn metadata_and_track_info_describe_the_same_track() {
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let library = library();
    let track = library
        .tracks
        .values()
        .find(|track| !track.title.is_empty() && !track.file_path.is_empty())
        .expect("a track with a title and a path");

    let metadata = client
        .metadata(Slot::USB, track.id)
        .await
        .expect("its metadata");
    assert_eq!(metadata.title, track.title);
    assert_eq!(metadata.artist, track.artist);
    assert_eq!(metadata.album, track.album);
    assert_eq!(metadata.tempo_centibpm, track.tempo);

    let info = client
        .track_info(Slot::USB, track.id)
        .await
        .expect("its track info");
    assert_eq!(info.path, track.file_path);
    // The one field a load needs that browsing does not (F31).
    assert_eq!(
        info.size,
        u64::from(track.file_size),
        "argument 0 of the path item is the size"
    );
    // Item 1 is the container, from pdb row offset 0x5a. The wrong value makes a
    // deck fetch the whole file and then refuse to decode it (F34, F35).
    assert_eq!(info.container, u32::from(track.container.0));
}

#[tokio::test]
async fn a_dip_into_metadata_does_not_lose_the_list_being_scrolled() {
    // A deck does not browse one menu at a time: it dips into metadata and then
    // resumes the list at the next offset *without re-issuing the menu request*.
    // Keying result sets on the item count alone worked until a metadata reply
    // became 13 items and collided with a 13-track album (F27, then F41).
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let all = client
        .tracks(Slot::USB, SortOrder::DEFAULT)
        .await
        .expect("the track list");
    let victim = all.get(5).expect("a sixth row").id;

    let _ = client
        .metadata(Slot::USB, victim)
        .await
        .expect("a dip into metadata");

    let again = client
        .tracks(Slot::USB, SortOrder::DEFAULT)
        .await
        .expect("the list again");
    assert_eq!(again.len(), all.len(), "the list survived the dip");
}

#[tokio::test]
async fn a_list_being_scrolled_survives_a_minute_of_metadata_polling() {
    // What hardware did: while a track played, the deck polled its metadata
    // every couple of seconds, and every poll minted a fresh result set. The
    // menu table evicted by *insertion* order, so after 32 polls it threw away
    // the long-lived list the DJ was scrolling — the one set certainly still
    // wanted. On the deck that looked like every menu going blank about a
    // minute into any track, independently of which track, and staying blank
    // until the DJ left LINK and came back (which opens a new connection and so
    // a new table).
    //
    // One connection in that capture minted 44 sets against a bound of 32.
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let all = client
        .tracks(Slot::USB, SortOrder::DEFAULT)
        .await
        .expect("the track list");
    let ids: Vec<u32> = all.iter().take(40).map(|item| item.id).collect();
    assert!(ids.len() >= 40, "need enough tracks to overflow the table");

    // Well past MAX_PENDING_MENUS, interleaved with paging the list the way a
    // deck interleaves them.
    for (polls, id) in ids.iter().enumerate() {
        let _ = client
            .metadata(Slot::USB, *id)
            .await
            .expect("a metadata dip");
        if polls % 4 == 0 {
            let again = client
                .tracks(Slot::USB, SortOrder::DEFAULT)
                .await
                .expect("the list must still be there");
            assert_eq!(
                again.len(),
                all.len(),
                "the list vanished after {polls} metadata polls",
            );
        }
    }

    let after = client
        .tracks(Slot::USB, SortOrder::DEFAULT)
        .await
        .expect("the list must still be there");
    assert_eq!(
        after.len(),
        all.len(),
        "the list did not survive the polling"
    );
    assert_eq!(
        after.first().map(|item| item.id),
        all.first().map(|item| item.id),
        "and it is the same list, not another menu served in its place",
    );
}

#[tokio::test]
async fn a_deck_asking_to_describe_the_medium_gets_a_description() {
    // `0x3903` is not an unknown request and must not be answered as one. A
    // deck sends it during a load and expects `0x4902` with a 148-byte body;
    // a bare SUCCESS costs it the whole browse session — menus blank, the track
    // title falls back to the medium's own name, the scrolling waveform stops —
    // until the DJ leaves LINK and comes back. Observed on hardware.
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let descriptor = client.descriptor(Slot::USB, MenuTarget::BINARY, TrackType::REKORDBOX);
    let request = prolink_proto::dbserver::Message::new(
        0x0380_0001,
        MessageKind::GET_MEDIA_INFO,
        [prolink_proto::dbserver::Field::U32(descriptor.to_raw())],
    );
    let reply = client.request(request).await.expect("a reply");

    assert_eq!(
        reply.kind,
        MessageKind::MEDIA_INFO,
        "a bare SUCCESS here is what broke a real deck",
    );
    let body = reply.blob(3).expect("the 148-byte body");
    assert_eq!(body.len(), MediaInfo::LEN);

    let info = MediaInfo::parse(body).expect("it parses");
    assert_eq!(info.volume_name, "TEST STICK");
    assert_eq!(
        info.track_count, 651,
        "the true count, the same one the UDP media query is answered with",
    );
}

#[tokio::test]
async fn every_track_row_carries_its_key_for_the_matching_indicator() {
    // The twelfth argument is the row's key as a Camelot index, and the deck
    // draws its key-matching indicator from it. We sent zero there, which
    // leaves the indicator dark beside every track. Decoded by correlating all
    // 1265 track rows a real deck served against those tracks' keys.
    let (_server, port) = serve_usb().await;
    let mut client = client(port).await;

    let rows = client
        .tracks(Slot::USB, SortOrder::DEFAULT)
        .await
        .expect("the track list");
    let library = library();

    let mut checked = 0;
    for row in &rows {
        let Some(track) = library.tracks.get(&row.id) else {
            continue;
        };
        let expected = CamelotKey::parse(&track.key).map_or(0, CamelotKey::index);
        assert_eq!(
            row.key_index, expected,
            "track {} in key {:?}",
            row.id, track.key
        );
        if expected != 0 {
            checked += 1;
        }
    }
    assert!(
        checked > 100,
        "only {checked} rows carried a key; the fixture should have plenty"
    );

    // And the indices really are the wheel: 1A is 1, 12B is 24.
    let indices: std::collections::BTreeSet<u32> = rows
        .iter()
        .map(|row| row.key_index)
        .filter(|index| *index != 0)
        .collect();
    assert!(
        indices.iter().all(|index| (1..=24).contains(index)),
        "off the wheel: {indices:?}"
    );
}

#[tokio::test]
async fn two_media_are_told_apart_by_the_descriptor_alone() {
    // A player browsing two media on one peer opens a *single* connection and
    // distinguishes them purely by the slot byte in each request (F37).
    let usb = Arc::new(Medium::synthetic(ServedSlot::USB, library(), "USB"));
    let mut sd_library = library();
    sd_library.tracks.retain(|id, _| *id % 2 == 0);
    let sd = Arc::new(Medium::synthetic(ServedSlot::SD, sd_library, "SD"));

    let server = DbServer::start(
        DbServerConfig {
            device: device(4),
            address: LOOPBACK,
            port: 0,
            query_port: None,
        },
        [usb, sd],
    )
    .await
    .expect("a two-slot dbserver");

    let mut client = client(server.port()).await;
    let from_usb = client
        .tracks(Slot::USB, SortOrder::DEFAULT)
        .await
        .expect("the USB");
    let from_sd = client
        .tracks(Slot::SD, SortOrder::DEFAULT)
        .await
        .expect("the SD");

    assert_eq!(from_usb.len(), library().tracks.len());
    assert!(
        from_sd.len() < from_usb.len(),
        "the two slots really are different libraries"
    );
    assert!(!from_sd.is_empty());
}

/// A directory shaped like a rekordbox medium, with the real database on it.
///
/// Named per test, not per process: `cargo test` runs these concurrently, and a
/// shared directory means one test's cleanup deletes another's medium mid-read.
/// Which the client reports correctly — it refuses to return a short file rather
/// than truncate one silently — but as a test failure it is noise.
fn fake_medium(test: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("prolink-loopback-{test}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("PIONEER/rekordbox")).expect("a temp medium");
    std::fs::create_dir_all(root.join("Contents/GESAFFELSTEIN")).expect("a contents directory");
    std::fs::write(root.join("PIONEER/rekordbox/export.pdb"), export_pdb()).expect("the database");
    // Not audio, but the server does not care and the client reads it the same
    // way: 40 KB is five 8192-byte reads, which is what a deck does.
    let audio: Vec<u8> = (0..40_960u32).map(|byte| byte as u8).collect();
    std::fs::write(root.join("Contents/GESAFFELSTEIN/track.mp3"), &audio).expect("a track");
    root
}

async fn serve_files(root: &Path) -> (NfsServer, u16) {
    let mut tree = Vfs::new();
    tree.mount(ServedSlot::USB.vfs_prefix(), root)
        .expect("mounting the medium");
    let server = NfsServer::start(
        Arc::new(RwLock::new(tree)),
        ServerNfsConfig {
            interface: None,
            // Ephemeral throughout: nothing here needs root. Against hardware
            // the portmapper must be on 111 or a deck never looks (F46).
            portmap_port: 0,
            mount_port: 0,
            nfs_port: 0,
        },
    )
    .await
    .expect("the file servers start");
    let portmap = server.ports().portmap;
    (server, portmap)
}

#[tokio::test]
async fn a_client_pulls_the_database_back_byte_for_byte() {
    let root = fake_medium("pull");
    let (_server, portmap) = serve_files(&root).await;

    let mut client = NfsClient::connect_with(
        LOOPBACK,
        None,
        prolink::consume::NfsConfig {
            portmap_port: portmap,
            ..Default::default()
        },
    )
    .await
    .expect("portmap discovery finds mountd and nfsd");

    let mounted = client.mount_slot(Slot::USB).await.expect("mounting /C/");
    let file = client
        .open(&mounted, prolink::consume::nfs::EXPORT_PDB)
        .await
        .expect("walking to export.pdb");

    let expected = export_pdb();
    assert_eq!(file.size(), expected.len() as u64);
    let pulled = client.read_file(&file).await.expect("the whole file");
    assert_eq!(
        pulled, expected,
        "an export.pdb that differs by a byte is a different library"
    );

    // And it is still a library after the round trip.
    let parsed = Library::parse(&pulled).expect("the pulled database parses");
    assert_eq!(parsed.tracks.len(), library().tracks.len());

    client
        .unmount(&mounted)
        .await
        .expect("a real player unmounts, once per slot");
    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn a_client_reads_a_track_in_ranges_the_way_a_deck_streams_one() {
    let root = fake_medium("stream");
    let (_server, portmap) = serve_files(&root).await;

    let mut client = NfsClient::connect_with(
        LOOPBACK,
        None,
        prolink::consume::NfsConfig {
            portmap_port: portmap,
            ..Default::default()
        },
    )
    .await
    .expect("connecting");

    let mounted = client.mount_slot(Slot::USB).await.expect("mounting");
    // The case-and-normalisation fallback matters here: `export.pdb` records
    // `Gesaffelstein` where the directory is `GESAFFELSTEIN` (O6).
    let file = client
        .open(&mounted, "/Contents/Gesaffelstein/track.mp3")
        .await
        .expect("a path whose case does not match the directory's");
    assert_eq!(file.size(), 40_960);

    // A deck seeks; it does not read front to back.
    let tail = client
        .read_range(&file, 40_000, 960)
        .await
        .expect("a read near the end");
    assert_eq!(tail.len(), 960);
    assert_eq!(tail[0], 40_000_u32 as u8);

    let middle = client
        .read_range(&file, 16_384, 8_192)
        .await
        .expect("a read in the middle");
    assert_eq!(middle.len(), 8_192);
    assert_eq!(middle[0], 16_384_u32 as u8);

    // Past the end is short, not an error.
    let past = client
        .read_range(&file, 40_960, 4_096)
        .await
        .expect("a read past the end");
    assert!(past.is_empty());

    std::fs::remove_dir_all(&root).ok();
}

#[tokio::test]
async fn a_handle_whose_tail_a_cdj_rewrote_still_resolves() {
    // RFC 1094 says a filehandle is 32 opaque bytes echoed back verbatim. A
    // CDJ-2000NXS keeps the leading twelve and overwrites the rest with its own
    // file reference (F28). A server that trusts the spec browses perfectly and
    // fails at the moment a DJ loads a track, so this is the case worth proving
    // end to end rather than only in the VFS's unit tests.
    let root = fake_medium("handle");
    let (_server, portmap) = serve_files(&root).await;

    let mut client = NfsClient::connect_with(
        LOOPBACK,
        None,
        prolink::consume::NfsConfig {
            portmap_port: portmap,
            ..Default::default()
        },
    )
    .await
    .expect("connecting");

    let mounted = client.mount_slot(Slot::USB).await.expect("mounting");
    let file = client
        .open(&mounted, prolink::consume::nfs::EXPORT_PDB)
        .await
        .expect("walking to the database");

    let mut rewritten = file.handle();
    rewritten.0[12..].copy_from_slice(&[
        0x03, 0x01, 0x2d, 0x00, 0x00, 0x00, 0x1b, 0x58, 0x00, 0x00, 0x00, 0x00, 0x03, 0x03, 0x01,
        0x00, 0x00, 0x00, 0x01, 0x62,
    ]);
    assert_ne!(rewritten, file.handle(), "the bytes really did change");

    let attributes = client
        .attributes(rewritten)
        .await
        .expect("the server must still know it");
    assert_eq!(u64::from(attributes.size), file.size());

    std::fs::remove_dir_all(&root).ok();
}
