// SPDX-License-Identifier: GPL-3.0-only

//! What can be checked without a DJ network on the bench.
//!
//! The bridge's shapes are checked by the compiler on both sides, so these
//! cover the things a type cannot: that the conventions the C++ side is told
//! about actually hold.

#![expect(
    clippy::unwrap_used,
    reason = "a test that cannot unwrap is a test \
                                         that hides its own failure"
)]

use prolink_cxx::{default_config, interfaces};

#[test]
fn the_default_config_chooses_an_interface_and_announces() {
    let config = default_config();
    assert!(
        config.interface.is_empty(),
        "empty means choose, which is what a host without a settings screen wants"
    );
    assert!(
        config.announce,
        "without announcing there is no loaded track, play state or tempo master (F21)"
    );
}

#[test]
fn every_interface_offered_describes_itself_completely() {
    // Runs on any machine: a host with no network gets an empty list, which is
    // a legitimate answer rather than an error.
    for found in interfaces() {
        assert!(!found.name.is_empty(), "an interface without a name");
        assert!(
            found.address.parse::<std::net::Ipv4Addr>().is_ok(),
            "{} has an unparseable address {:?}",
            found.name,
            found.address
        );
        assert!(
            found.broadcast.parse::<std::net::Ipv4Addr>().is_ok(),
            "{} has an unparseable broadcast {:?}",
            found.name,
            found.broadcast
        );
        let address: std::net::Ipv4Addr = found.address.parse().unwrap();
        assert_eq!(
            found.is_link_local,
            address.is_link_local(),
            "{} disagrees with its own address about being link-local",
            found.name
        );
    }
}

#[test]
fn naming_an_interface_that_is_not_there_fails_with_a_reason() {
    // The message is what a host shows the user, so it has to name the thing
    // that was not found rather than say "error".
    let mut config = default_config();
    config.interface = "definitely-not-an-interface".to_owned();
    let error = prolink_cxx::open(&config).expect_err("that interface cannot exist");
    let message = error.to_string();
    assert!(
        message.contains("definitely-not-an-interface"),
        "the message should name the interface: {message}"
    );
}

#[test]
fn serving_nothing_is_refused_rather_than_started_empty() {
    // A server with no media announces itself, is offered on a deck's LINK
    // screen, and then has nothing behind it. Refusing is the honest answer.
    let config = prolink_cxx::ServeConfig {
        interface: String::new(),
        usb_path: String::new(),
        sd_path: String::new(),
    };
    let error = prolink_cxx::serve(&config).expect_err("nothing to serve");
    assert!(
        error.to_string().contains("nothing to serve"),
        "the message should say what is missing: {error}"
    );
}

#[test]
fn a_medium_that_is_not_a_rekordbox_export_is_refused_by_path() {
    // The message names the path, because a host serving two media has to know
    // which one it could not read.
    let config = prolink_cxx::ServeConfig {
        interface: String::new(),
        usb_path: "/definitely/not/a/rekordbox/stick".to_owned(),
        sd_path: String::new(),
    };
    let error = prolink_cxx::serve(&config).expect_err("that path cannot exist");
    let message = error.to_string();
    assert!(
        message.contains("/definitely/not/a/rekordbox/stick"),
        "the message should name the medium: {message}"
    );
}

#[test]
fn a_row_reports_the_two_live_marks_a_server_puts_on_it() {
    // Bit 0 is "in the tag list" and bit 8 is "the track this deck has
    // loaded" (F53, F55). A host draws both, so the bridge has to surface
    // them rather than leave C++ to mask a flags word.
    use prolink_proto::dbserver::{ItemType, MenuItem};

    let mut item = MenuItem::track(
        472,
        "Anti Gravity Racing",
        "Dax J",
        ItemType::TRACK_TITLE,
        0,
    );
    assert!(!prolink_cxx::row_for_test(&item).is_loaded);
    assert!(!prolink_cxx::row_for_test(&item).is_tagged);

    item.flags |= MenuItem::LOADED;
    assert!(prolink_cxx::row_for_test(&item).is_loaded);
    item.flags |= MenuItem::TAGGED;
    let row = prolink_cxx::row_for_test(&item);
    assert!(
        row.is_loaded && row.is_tagged,
        "both marks survive together"
    );
    assert_eq!(row.id, 472);
    assert_eq!(row.label, "Anti Gravity Racing");
    assert_eq!(row.detail, "Dax J");
}

#[test]
fn a_transfer_done_event_carries_the_path_it_wrote() {
    // Mixxx's signal carries the local path, and a host that keyed its own
    // state on the path it asked for should not need an id table to get back
    // to it. Both are reported.
    let event = prolink_cxx::transfer_done_for_test(7, "/tmp/export.pdb", None);
    assert_eq!(event.transfer, 7);
    assert_eq!(event.path, "/tmp/export.pdb");
    assert!(event.ok);
    assert!(event.detail.is_empty());

    let failed = prolink_cxx::transfer_done_for_test(8, "/tmp/x", Some("no such file"));
    assert!(!failed.ok);
    assert_eq!(failed.detail, "no such file");
    assert_eq!(failed.path, "/tmp/x", "the path is reported either way");
}

#[test]
fn a_stale_filehandle_is_the_one_nfs_error_with_a_remedy() {
    // The transfer path retries once on this and on nothing else: a deck
    // churns its filehandle table and then answers STALE to every lookup made
    // against the old handles (F28). Getting the test wrong means either an
    // unrecoverable transfer or a loop against a genuinely missing file.
    use prolink_proto::rpc::nfs2::ErrorStatus;

    let stale = prolink::Error::Nfs {
        operation: "LOOKUP",
        path: "/PIONEER/rekordbox/export.pdb".to_owned(),
        status: ErrorStatus::STALE,
    };
    assert!(stale.is_stale());

    for other in [ErrorStatus::NOENT, ErrorStatus::ACCES] {
        let error = prolink::Error::Nfs {
            operation: "LOOKUP",
            path: "/x".to_owned(),
            status: other,
        };
        assert!(
            !error.is_stale(),
            "{other:?} has no remedy and must not trigger a re-mount"
        );
    }
}

#[test]
fn the_media_slot_conversion_round_trips() {
    // A host names a slot and the library has to receive the same one; USB is
    // the deliberate default for anything unnamed, since it is the slot a deck
    // browses first.
    use prolink_cxx::Slot;
    for (given, expected) in [
        (Slot::Usb, prolink_proto::Slot::USB),
        (Slot::Sd, prolink_proto::Slot::SD),
        (Slot::Cd, prolink_proto::Slot::CD),
        (Slot::Rekordbox, prolink_proto::Slot::REKORDBOX),
        (Slot::None, prolink_proto::Slot::USB),
    ] {
        assert_eq!(
            prolink_cxx::slot_back_for_test(given),
            expected,
            "{given:?} did not map as documented"
        );
    }
}

#[test]
fn a_player_with_no_status_reports_absent_rather_than_zero() {
    // Without announcing there is no status packet, so tempo and phase are
    // unknown rather than zero -- a host drawing a tempo has to be able to
    // tell "not playing" from "0.00 BPM" (F21).
    let player = prolink_cxx::empty_player_for_test();
    assert!(!player.has_status);
    assert!(
        player.effective_bpm < 0.0,
        "an unknown tempo must be negative, not zero: {}",
        player.effective_bpm
    );
    assert!(player.track_bpm < 0.0);
    assert!(player.beat_phase < 0.0);
    assert!(player.bar_phase < 0.0);
    assert_eq!(player.beat_in_bar, 0);
    assert_eq!(player.track_id, 0);
}

#[test]
fn a_real_export_pdb_reads_through_the_bridge() {
    // The 651-track export the library is pinned against elsewhere. This is
    // the shape a host actually consumes, so it is worth checking that the
    // reshaping keeps the joins and the ids.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/export.pdb");
    if !std::path::Path::new(path).exists() {
        return;
    }
    let contents = prolink_cxx::read_pdb(path);
    assert!(contents.ok, "should parse: {}", contents.error);
    assert!(contents.error.is_empty());
    assert_eq!(contents.tracks.len(), 651);
    assert!(!contents.playlists.is_empty());
    assert!(!contents.artists.is_empty());

    // Names resolved *and* ids given, which is the point of the shape.
    let track = contents
        .tracks
        .iter()
        .find(|track| track.title == "Anti Gravity Racing")
        .expect("the track every other test uses");
    assert_eq!(track.id, 472);
    assert_eq!(track.artist, "Dax J");
    assert_eq!(track.key, "9A");
    assert_eq!(track.tempo_centibpm, 14500, "145.00 BPM in hundredths");
    assert!(track.artist_id > 0, "the row id is there for a browse tree");
    assert!(
        track.file_path.starts_with('/'),
        "a fetch takes this verbatim, leading slash and all: {}",
        track.file_path
    );
    assert!(
        track.analyze_path.ends_with(".DAT"),
        "{}",
        track.analyze_path
    );

    // A playlist keeps the DJ's order rather than being sorted.
    assert!(
        contents
            .playlists
            .iter()
            .any(|playlist| !playlist.is_folder && !playlist.track_ids.is_empty()),
        "at least one playlist should have tracks in it"
    );
}

#[test]
fn a_database_that_does_not_parse_is_a_value_and_not_an_exception() {
    // A host has usually just pulled several megabytes over NFS to get here,
    // and what it wants is to tell the user why that was wasted.
    let contents = prolink_cxx::read_pdb("/definitely/not/a/database.pdb");
    assert!(!contents.ok);
    assert!(
        contents.error.contains("/definitely/not/a/database.pdb"),
        "the message should name the file: {}",
        contents.error
    );
    assert!(contents.tracks.is_empty());
}
