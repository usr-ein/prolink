// SPDX-License-Identifier: GPL-3.0-only

//! The order a track is fetched in, so it can be played before it has arrived.
//!
//! These are pure: no socket, no server, no timing. That is deliberate — the
//! interesting part of a progressive fetch is *which ranges, in what order*, and
//! testing that against a live server would mean racing it. The wire itself is
//! covered end to end in `loopback.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use prolink::consume::nfs::{FetchStep, progressive_plan};

/// Every byte of the file exactly once, no gaps and no overlaps.
fn assert_covers(size: u64, steps: &[FetchStep]) {
    let mut covered = vec![0u8; usize::try_from(size).unwrap()];
    for step in steps {
        assert!(step.len > 0, "a zero-length step is a wasted round trip");
        assert!(
            step.offset + step.len <= size,
            "step {step:?} runs past the end of a {size}-byte file"
        );
        for i in step.offset..step.offset + step.len {
            let slot = &mut covered[usize::try_from(i).unwrap()];
            assert_eq!(*slot, 0, "byte {i} is fetched twice");
            *slot = 1;
        }
    }
    assert!(
        covered.iter().all(|&seen| seen == 1),
        "the plan leaves holes in a {size}-byte file"
    );
}

#[test]
fn head_comes_first_because_it_is_the_runway() {
    let steps = progressive_plan(10_000, 1_000, 256, 4_096);
    assert_eq!(
        steps[0],
        FetchStep {
            offset: 0,
            len: 1_000
        }
    );
}

#[test]
fn tail_comes_second_or_aac_never_opens() {
    // M4A and MP4 commonly keep the moov atom at the end, and a decoder cannot
    // open one without it. If the tail ever slips behind the middle, AAC stops
    // working and nothing else does -- which is a horrible bug to diagnose.
    let steps = progressive_plan(10_000, 1_000, 256, 4_096);
    assert_eq!(
        steps[1],
        FetchStep {
            offset: 10_000 - 256,
            len: 256
        }
    );
}

#[test]
fn the_middle_follows_the_playhead() {
    let steps = progressive_plan(10_000, 1_000, 256, 4_096);
    let middle: Vec<_> = steps[2..].to_vec();
    assert_eq!(middle[0].offset, 1_000);
    for pair in middle.windows(2) {
        assert!(
            pair[1].offset > pair[0].offset,
            "the middle must be fetched forwards, not {pair:?}"
        );
        assert_eq!(
            pair[1].offset,
            pair[0].offset + pair[0].len,
            "the middle must be contiguous"
        );
    }
}

#[test]
fn covers_the_whole_file_at_a_range_of_sizes() {
    for size in [
        1u64, 2, 255, 256, 257, 999, 1_000, 1_001, 4_095, 4_096, 100_000,
    ] {
        assert_covers(size, &progressive_plan(size, 1_000, 256, 4_096));
    }
}

#[test]
fn covers_the_whole_file_for_odd_parameter_combinations() {
    for head in [0u64, 1, 999, 1_000, 100_000] {
        for tail in [0u64, 1, 256, 100_000] {
            for chunk in [1u64, 7, 4_096, 100_000] {
                let size = 10_000;
                assert_covers(size, &progressive_plan(size, head, tail, chunk));
            }
        }
    }
}

#[test]
fn an_empty_file_needs_no_round_trips() {
    assert!(progressive_plan(0, 1_000, 256, 4_096).is_empty());
}

#[test]
fn a_file_smaller_than_the_head_is_one_step() {
    let steps = progressive_plan(500, 1_000, 256, 4_096);
    assert_eq!(
        steps,
        vec![FetchStep {
            offset: 0,
            len: 500
        }]
    );
}

#[test]
fn head_and_tail_never_overlap_on_a_small_file() {
    // 600 bytes with a 500-byte head and a 256-byte tail: the naive tail start
    // is 344, which is inside the head. Fetching it anyway would download those
    // bytes twice and -- worse -- write them twice at different times.
    let steps = progressive_plan(600, 500, 256, 4_096);
    assert_covers(600, &steps);
    assert_eq!(
        steps,
        vec![
            FetchStep {
                offset: 0,
                len: 500
            },
            FetchStep {
                offset: 500,
                len: 100
            }
        ]
    );
}

#[test]
fn no_head_still_yields_a_usable_plan() {
    let steps = progressive_plan(10_000, 0, 256, 4_096);
    assert_covers(10_000, &steps);
    // With no runway the tail is still first, because the reason for it has
    // nothing to do with the head.
    assert_eq!(
        steps[0],
        FetchStep {
            offset: 10_000 - 256,
            len: 256
        }
    );
}

#[test]
fn no_tail_is_just_head_then_middle() {
    let steps = progressive_plan(10_000, 1_000, 0, 4_096);
    assert_covers(10_000, &steps);
    assert_eq!(
        steps[0],
        FetchStep {
            offset: 0,
            len: 1_000
        }
    );
    assert_eq!(
        steps[1],
        FetchStep {
            offset: 1_000,
            len: 4_096
        }
    );
}

#[test]
fn a_zero_chunk_does_not_hang() {
    // A zero chunk would advance the cursor by nothing and loop forever. It is
    // a caller's mistake, but an infinite loop inside a network fetch is a
    // deck that stops responding, so it is treated as "the rest in one go".
    let steps = progressive_plan(10_000, 1_000, 256, 0);
    assert_covers(10_000, &steps);
    assert_eq!(steps.len(), 3);
}

#[test]
fn a_head_larger_than_the_file_does_not_run_past_the_end() {
    let steps = progressive_plan(100, u64::MAX, 256, 4_096);
    assert_covers(100, &steps);
}

#[test]
fn a_tail_larger_than_the_file_does_not_run_past_the_start() {
    let steps = progressive_plan(100, 0, u64::MAX, 4_096);
    assert_covers(100, &steps);
    assert_eq!(
        steps,
        vec![FetchStep {
            offset: 0,
            len: 100
        }]
    );
}

#[test]
fn a_huge_file_is_chunked_rather_than_asked_for_at_once() {
    // A deck answers a bounded read; asking for a gigabyte in one call is not a
    // request any of them honour.
    let steps = progressive_plan(64 * 1024 * 1024, 1024 * 1024, 256 * 1024, 1024 * 1024);
    assert!(steps.iter().all(|step| step.len <= 1024 * 1024));
    assert_covers(64 * 1024 * 1024, &steps);
}

#[test]
fn the_plan_is_deterministic() {
    // It is used to decide what has already been fetched, so two calls with the
    // same arguments have to agree.
    let a = progressive_plan(12_345, 1_000, 256, 4_096);
    let b = progressive_plan(12_345, 1_000, 256, 4_096);
    assert_eq!(a, b);
}
