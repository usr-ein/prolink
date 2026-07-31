// SPDX-License-Identifier: GPL-3.0-only

//! What keeps `include/prolink.h` honest.
//!
//! The header is hand-written, so that it can carry the same explanations the
//! Rust side does. The risk of a hand-written header is drift: a field added
//! on one side and not the other does not fail to compile, it corrupts the
//! caller's stack at run time, on a machine the author does not have.
//!
//! So every struct's size and alignment is pinned here, and the offset of
//! every field a host actually reads with it. A field added, removed, reordered or resized fails the test
//! suite with the name of the struct that moved, which is a reminder to update
//! the header rather than a guarantee that somebody did — but it is the
//! difference between finding out now and finding out in Mixxx.

#![cfg(test)]

use std::mem::{align_of, offset_of, size_of};

use crate::{
    ProlinkConfig, ProlinkDevice, ProlinkEvent, ProlinkInterface, ProlinkPlayer, ProlinkStatus,
};

#[test]
fn every_c_visible_struct_has_the_layout_the_header_declares() {
    // Sizes first: a change here means the header is wrong about the whole
    // struct, which is the case a caller's stack pays for.
    assert_eq!(
        size_of::<ProlinkDevice>(),
        64,
        "ProlinkDevice grew or shrank"
    );
    assert_eq!(align_of::<ProlinkDevice>(), 8);
    assert_eq!(
        size_of::<ProlinkPlayer>(),
        88,
        "ProlinkPlayer grew or shrank"
    );
    assert_eq!(align_of::<ProlinkPlayer>(), 8);
    assert_eq!(
        size_of::<ProlinkInterface>(),
        57,
        "ProlinkInterface changed"
    );
    assert_eq!(align_of::<ProlinkInterface>(), 1);
    assert_eq!(size_of::<ProlinkEvent>(), 40, "ProlinkEvent changed");
    assert_eq!(align_of::<ProlinkEvent>(), 8);
    assert_eq!(size_of::<ProlinkConfig>(), 26, "ProlinkConfig changed");
    // The status enum is what every fallible entry point returns.
    assert_eq!(size_of::<ProlinkStatus>(), 4, "ProlinkStatus is not an int");
}

#[test]
fn the_fields_a_host_reads_are_where_the_header_says() {
    // Only the fields whose position a host actually depends on. Pinning every
    // offset would make the test a copy of the struct rather than a check on
    // it, and would fail for a reordering that changes nothing.
    assert_eq!(offset_of!(ProlinkDevice, number), 0);
    assert_eq!(offset_of!(ProlinkDevice, mac), 4);
    assert_eq!(offset_of!(ProlinkDevice, name), 10);

    assert_eq!(offset_of!(ProlinkPlayer, number), 0);
    assert_eq!(offset_of!(ProlinkPlayer, name), 1);
    // The doubles must be 8-aligned, which is the thing a hand-written header
    // most easily gets wrong by putting a bool in the wrong place.
    assert_eq!(offset_of!(ProlinkPlayer, effective_bpm) % 8, 0);
    assert_eq!(offset_of!(ProlinkPlayer, beat_phase) % 8, 0);

    assert_eq!(offset_of!(ProlinkEvent, kind), 0);
    assert_eq!(offset_of!(ProlinkEvent, done) % 8, 0);
    assert_eq!(offset_of!(ProlinkEvent, total) % 8, 0);
}

#[test]
fn a_status_code_is_the_number_the_header_gives_it() {
    // A host switching on these gets them from the header, so the two have to
    // agree on the numbers and not merely on the names.
    assert_eq!(ProlinkStatus::Ok as i32, 0);
    assert_eq!(ProlinkStatus::InvalidArgument as i32, -1);
    assert_eq!(ProlinkStatus::NoInterface as i32, -2);
    assert_eq!(ProlinkStatus::Bind as i32, -3);
    assert_eq!(ProlinkStatus::NoDeviceNumber as i32, -4);
    assert_eq!(ProlinkStatus::BadMedium as i32, -5);
    assert_eq!(ProlinkStatus::Internal as i32, -6);
    assert_eq!(ProlinkStatus::Panic as i32, -7);
}

#[test]
fn a_name_longer_than_the_buffer_is_truncated_on_a_character_boundary() {
    // A device name is 20 bytes of whatever the deck sends. Truncating one
    // mid-character would put invalid UTF-8 in a buffer a host reads as a C
    // string, and the buffer must always keep room for the NUL.
    let long = "日本語のとても長いデバイス名です".repeat(4);
    let filled = crate::convert::fill::<{ crate::PROLINK_NAME_LEN }>(&long);
    assert_eq!(filled.len(), crate::PROLINK_NAME_LEN);
    assert_eq!(
        filled.last(),
        Some(&0),
        "there must always be room for the terminator"
    );
    let end = filled.iter().position(|byte| *byte == 0).unwrap_or(0);
    std::str::from_utf8(&filled[..end]).expect("a truncated name is still UTF-8");
}

#[test]
fn a_name_that_fits_survives_whole() {
    let filled = crate::convert::fill::<{ crate::PROLINK_NAME_LEN }>("CDJ-2000nexus");
    let end = filled.iter().position(|byte| *byte == 0).unwrap_or(0);
    assert_eq!(&filled[..end], b"CDJ-2000nexus");
}
