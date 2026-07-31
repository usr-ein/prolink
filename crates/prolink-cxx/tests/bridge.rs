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
