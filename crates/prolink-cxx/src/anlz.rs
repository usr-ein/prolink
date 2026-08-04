// SPDX-License-Identifier: GPL-3.0-only

//! Reading an `ANLZ####.DAT`/`.EXT` for a host that is not written in Rust.
//!
//! The parsing is [`prolink_rekordbox`]'s; this only reshapes it. What a host
//! wants out of these files is a beat grid, a list of cues and a waveform, and
//! each of those is spread over tags in a way that is the file format's problem
//! rather than the host's:
//!
//! **Both cue lists, flattened.** A file carries one `PCOB` of memory cues and
//! one of hot cues, and `PCO2` repeats the pair with comments and colours
//! attached. A reader that takes the first tag of each fourcc gets half the
//! cues and no error, so this returns every entry of every list with the two
//! facts a caller needs to tell them apart — which list, and which tag.
//!
//! **The colour waveform, decoded.** `PWV5` packs a hue and an amplitude into
//! each 16-bit word, and getting the bit layout wrong draws a plausible
//! waveform rather than an obviously broken one. That decode belongs on this
//! side of the boundary, once.
//!
//! **A bad file is a value, not an exception**, as with the database beside it.
//! Analysis is an enhancement: a track whose `.EXT` is unreadable still plays,
//! just without its cues.

use prolink_rekordbox::AnlzFile;
use prolink_rekordbox::anlz::{Content, CueListType, CueType, FourCc};

use crate::ffi::{AnlzBeat, AnlzColourColumn, AnlzContents, AnlzCue};

/// Read one analysis file off disk.
#[must_use]
pub fn read_anlz(path: &str) -> AnlzContents {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return failed(format!("reading {path}: {error}")),
    };
    let file = match AnlzFile::parse(&bytes) {
        Ok(file) => file,
        Err(error) => return failed(format!("parsing {path}: {error}")),
    };

    AnlzContents {
        ok: true,
        error: String::new(),
        beats: beats(&file),
        cues: cues(&file),
        colour_detail: colour_detail(&file),
        preview: preview(&file),
    }
}

/// The beat grid, or empty where the file has none.
fn beats(file: &AnlzFile) -> Vec<AnlzBeat> {
    file.beat_grid()
        .map(|grid| {
            grid.beats
                .iter()
                .map(|beat| AnlzBeat {
                    beat_number: beat.beat_number,
                    tempo: beat.tempo,
                    time_ms: beat.time,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Every cue of every list, plain and extended.
///
/// Deliberately **not** merged or deduplicated. The two tags are not two views
/// of one truth — a file can carry a `PCOB` entry with no `PCO2` counterpart,
/// and which of them a host applies depends on what it can display — so
/// flattening them here while keeping `extended` is the most a reader can do
/// without deciding for the caller.
fn cues(file: &AnlzFile) -> Vec<AnlzCue> {
    let mut out = Vec::new();
    for list in file.cue_lists() {
        let hot_list = list.list_type == CueListType::HOT;
        for cue in &list.cues {
            let is_loop = cue.cue_type == CueType::LOOP;
            out.push(AnlzCue {
                extended: false,
                hot_list,
                hot_cue: cue.hot_cue,
                is_loop,
                time_ms: cue.time,
                loop_time_ms: if is_loop { cue.loop_time } else { 0 },
                comment: String::new(),
                color_id: 0,
                color_red: 0,
                color_green: 0,
                color_blue: 0,
            });
        }
    }
    for list in file.extended_cue_lists() {
        let hot_list = list.list_type == CueListType::HOT;
        for cue in &list.cues {
            let is_loop = cue.cue_type == CueType::LOOP;
            out.push(AnlzCue {
                extended: true,
                hot_list,
                hot_cue: cue.hot_cue,
                is_loop,
                time_ms: cue.time,
                loop_time_ms: if is_loop { cue.loop_time } else { 0 },
                comment: cue.comment.clone(),
                color_id: u32::from(cue.color_id),
                color_red: cue.hot_cue_color_red,
                color_green: cue.hot_cue_color_green,
                color_blue: cue.hot_cue_color_blue,
            });
        }
    }
    out
}

/// The `PWV5` colour waveform, decoded into bands and an envelope.
///
/// The names are the bands', not the colours' — see [`AnlzColourColumn`] for
/// which is drawn as what. Keeping the two apart matters: the field at bits
/// 12-10 is the *treble* band and it is drawn *blue*, and a decode that follows
/// the field order into the colour order swaps two thirds of the picture.
fn colour_detail(file: &AnlzFile) -> Vec<AnlzColourColumn> {
    let Some(Content::WaveformColorDetail(detail)) =
        file.tag(FourCc::PWV5).and_then(|tag| tag.content.as_ref())
    else {
        return Vec::new();
    };
    detail
        .columns
        .iter()
        .map(|column| AnlzColourColumn {
            bass: column.bass(),
            mid: column.mid(),
            treble: column.treble(),
            height: column.height(),
        })
        .collect()
}

/// The `PWAV` preview, as its 400 packed bytes.
///
/// Handed over packed rather than split into height and shade: it is one byte
/// per column either way, and the caller that draws it is also the one that
/// decides what to do with the shade.
fn preview(file: &AnlzFile) -> Vec<u8> {
    file.payload(FourCc::PWAV).unwrap_or_default().to_vec()
}

/// A file that could not be read at all.
fn failed(error: String) -> AnlzContents {
    AnlzContents {
        ok: false,
        error,
        beats: Vec::new(),
        cues: Vec::new(),
        colour_detail: Vec::new(),
        preview: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tag: fourcc, header length, total length, then the body.
    fn tag(fourcc: &[u8; 4], header_len: u32, body: &[u8]) -> Vec<u8> {
        let mut out = fourcc.to_vec();
        out.extend_from_slice(&header_len.to_be_bytes());
        out.extend_from_slice(&(12 + body.len() as u32).to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    /// A whole file: the `PMAI` header, then the tags.
    fn file(tags: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = tags.concat();
        let mut out = b"PMAI".to_vec();
        out.extend_from_slice(&28u32.to_be_bytes());
        out.extend_from_slice(&(28 + body.len() as u32).to_be_bytes());
        out.extend_from_slice(&[0; 16]);
        out.extend_from_slice(&body);
        out
    }

    fn parse(bytes: &[u8]) -> AnlzContents {
        let anlz = AnlzFile::parse(bytes).expect("the fixture should parse");
        AnlzContents {
            ok: true,
            error: String::new(),
            beats: beats(&anlz),
            cues: cues(&anlz),
            colour_detail: colour_detail(&anlz),
            preview: preview(&anlz),
        }
    }

    #[test]
    fn a_missing_file_is_a_value_and_not_a_panic() {
        let contents = read_anlz("/nowhere/ANLZ0000.DAT");
        assert!(!contents.ok);
        assert!(contents.error.contains("/nowhere/ANLZ0000.DAT"));
        assert!(contents.beats.is_empty());
    }

    #[test]
    fn beats_carry_their_bar_position_and_their_tempo() {
        // Two beats: number, tempo in centi-BPM, time in ms.
        let mut body = 0u32.to_be_bytes().to_vec();
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(&2u32.to_be_bytes());
        for (number, tempo, time) in [(1u16, 14400u16, 23u32), (2, 14400, 440)] {
            body.extend_from_slice(&number.to_be_bytes());
            body.extend_from_slice(&tempo.to_be_bytes());
            body.extend_from_slice(&time.to_be_bytes());
        }
        let contents = parse(&file(&[tag(b"PQTZ", 24, &body)]));

        assert_eq!(contents.beats.len(), 2);
        assert_eq!(contents.beats[0].beat_number, 1);
        assert_eq!(contents.beats[0].tempo, 14400);
        assert_eq!(contents.beats[0].time_ms, 23);
        assert_eq!(contents.beats[1].time_ms, 440);
    }

    #[test]
    fn both_cue_lists_survive_and_say_which_they_are() {
        // The trap this exists for: one PCOB of memory cues and one of hot
        // cues. Taking the first tag would return half of these.
        let cue = |hot_cue: u32, cue_type: u8, time: u32, loop_time: u32| {
            let mut entry = b"PCPT".to_vec();
            entry.extend_from_slice(&12u32.to_be_bytes()); // header_len
            entry.extend_from_slice(&40u32.to_be_bytes()); // entry_len
            entry.extend_from_slice(&hot_cue.to_be_bytes());
            entry.extend_from_slice(&1u32.to_be_bytes()); // status: enabled
            entry.extend_from_slice(&0u32.to_be_bytes()); // unknown1
            entry.extend_from_slice(&0u16.to_be_bytes()); // order_first
            entry.extend_from_slice(&0u16.to_be_bytes()); // order_last
            entry.push(cue_type);
            entry.extend_from_slice(&[0, 3, 0xe8]); // unknown2
            entry.extend_from_slice(&time.to_be_bytes());
            entry.extend_from_slice(&loop_time.to_be_bytes());
            entry
        };
        let list = |list_type: u32, entries: Vec<Vec<u8>>| {
            let mut body = list_type.to_be_bytes().to_vec();
            body.extend_from_slice(&0u16.to_be_bytes()); // unknown
            body.extend_from_slice(&(entries.len() as u16).to_be_bytes());
            body.extend_from_slice(&0u32.to_be_bytes()); // memory_count
            body.extend(entries.concat());
            tag(b"PCOB", 24, &body)
        };

        let contents = parse(&file(&[
            list(0, vec![cue(0, 1, 1000, 0), cue(0, 2, 2000, 4000)]),
            list(1, vec![cue(1, 1, 500, 0)]),
        ]));

        assert_eq!(contents.cues.len(), 3, "both lists, every entry");
        assert!(!contents.cues[0].hot_list);
        assert!(!contents.cues[0].is_loop);
        assert_eq!(contents.cues[0].time_ms, 1000);

        assert!(contents.cues[1].is_loop);
        assert_eq!(contents.cues[1].loop_time_ms, 4000);

        assert!(contents.cues[2].hot_list, "the second list is the hot one");
        assert_eq!(contents.cues[2].hot_cue, 1, "hot cue A is 1, not 0");
        assert!(!contents.cues[2].extended, "PCOB carries no comment");
    }

    #[test]
    fn a_loop_time_is_dropped_for_a_point() {
        // A point's loop_time is whatever rekordbox left in the field, and a
        // caller that trusted it would draw a loop that is not there.
        let mut entry = b"PCPT".to_vec();
        entry.extend_from_slice(&12u32.to_be_bytes());
        entry.extend_from_slice(&40u32.to_be_bytes());
        entry.extend_from_slice(&0u32.to_be_bytes());
        entry.extend_from_slice(&1u32.to_be_bytes());
        entry.extend_from_slice(&0u32.to_be_bytes());
        entry.extend_from_slice(&0u16.to_be_bytes());
        entry.extend_from_slice(&0u16.to_be_bytes());
        entry.push(1); // a point
        entry.extend_from_slice(&[0, 3, 0xe8]);
        entry.extend_from_slice(&1000u32.to_be_bytes());
        entry.extend_from_slice(&9999u32.to_be_bytes()); // leftover

        let mut body = 0u32.to_be_bytes().to_vec();
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend_from_slice(&1u16.to_be_bytes());
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend(entry);

        let contents = parse(&file(&[tag(b"PCOB", 24, &body)]));
        assert_eq!(contents.cues[0].loop_time_ms, 0);
    }

    #[test]
    fn the_colour_waveform_keeps_bands_and_colours_apart() {
        // bass 5, treble 2, mid 3, height 20.
        let word: u16 = (5 << 13) | (2 << 10) | (3 << 7) | (20 << 2);
        let mut body = 2u32.to_be_bytes().to_vec(); // len_entry_bytes
        body.extend_from_slice(&1u32.to_be_bytes()); // len_entries
        body.extend_from_slice(&0u32.to_be_bytes()); // unknown
        body.extend_from_slice(&word.to_be_bytes());

        let contents = parse(&file(&[tag(b"PWV5", 24, &body)]));
        assert_eq!(contents.colour_detail.len(), 1);
        let column = &contents.colour_detail[0];
        assert_eq!(column.bass, 5);
        assert_eq!(column.treble, 2, "bits 12-10 are treble, drawn blue");
        assert_eq!(column.mid, 3, "bits 9-7 are mid, drawn green");
        assert_eq!(column.height, 20);
    }

    #[test]
    fn the_preview_comes_over_packed() {
        let mut body = 400u32.to_be_bytes().to_vec();
        body.extend_from_slice(&0x0010_0000u32.to_be_bytes());
        body.extend_from_slice(&[0b1010_0110; 400]);

        let contents = parse(&file(&[tag(b"PWAV", 20, &body)]));
        assert_eq!(contents.preview.len(), 400);
        assert_eq!(contents.preview[0] & 0x1f, 6, "height in the low five bits");
        assert_eq!(contents.preview[0] >> 5, 5, "shade in the top three");
    }
}
