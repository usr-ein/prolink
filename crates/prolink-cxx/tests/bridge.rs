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
