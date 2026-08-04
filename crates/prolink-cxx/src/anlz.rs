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

use std::io::{Read, Seek, SeekFrom};

use crate::ffi::{AnlzBeat, AnlzColourColumn, AnlzContents, AnlzCue, AnlzPreview, AnlzPreviewColumn};

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

/// Bytes of the fixed part of the container header and of every tag header.
const TAG_HEADER: usize = 12;

/// Columns in a `PWV4`, and bytes per column.
const COLOUR_ENTRY: usize = 6;

/// Read only the preview waveforms, seeking past everything else.
///
/// # Why this is not `read_anlz(path).preview`
///
/// It is the same 400 bytes, but not the same cost. A `.EXT` is around 157 kB
/// and almost all of it is `PWV3` and `PWV5`, two waveforms of some fifty
/// thousand entries each; parsing the file to reach the 1200-column `PWV4`
/// decodes both and throws them away. This reads each tag's 12-byte header and
/// seeks over any tag it does not want, so the colour preview costs about 10 kB
/// of reads.
///
/// That matters because this runs once per row a DJ pauses on, off a USB stick
/// that may be busy copying the track they are about to play.
#[must_use]
pub fn read_anlz_preview(path: &str) -> AnlzPreview {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return preview_failed(format!("reading {path}: {error}")),
    };

    // The container header: `PMAI`, its own length, the file length.
    let mut header = [0u8; TAG_HEADER];
    if let Err(error) = file.read_exact(&mut header) {
        return preview_failed(format!("reading {path}: {error}"));
    }
    if &header[..4] != b"PMAI" {
        return preview_failed(format!("{path} is not an ANLZ file"));
    }
    let len_header = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
    let mut offset = u64::from(len_header);

    let mut out = AnlzPreview {
        ok: true,
        error: String::new(),
        mono: Vec::new(),
        colour: Vec::new(),
    };

    loop {
        if file.seek(SeekFrom::Start(offset)).is_err() {
            break;
        }
        let mut tag = [0u8; TAG_HEADER];
        if file.read_exact(&mut tag).is_err() {
            break; // The end of the file, which is how the tag list ends.
        }
        let len_tag = u32::from_be_bytes([tag[8], tag[9], tag[10], tag[11]]);
        // A zero-length tag would loop here forever, and a file that claims one
        // is a file we cannot walk any further.
        if len_tag < TAG_HEADER as u32 {
            break;
        }

        match &tag[..4] {
            b"PWAV" => out.mono = mono_columns(&mut file, len_tag),
            b"PWV4" => out.colour = colour_columns(&mut file, len_tag),
            // Everything else is skipped without being read at all. This is the
            // whole point of the function.
            _ => {}
        }

        offset += u64::from(len_tag);
        if !out.mono.is_empty() && !out.colour.is_empty() {
            break; // Both in hand; the rest of the file cannot add anything.
        }
    }
    out
}

/// The `PWAV` payload, positioned just past the tag's common header.
///
/// The tag declares a 20-byte header: the twelve common bytes, a length and an
/// unknown word. So the columns start eight bytes further on.
fn mono_columns(file: &mut std::fs::File, len_tag: u32) -> Vec<u8> {
    let Some(body_len) = (len_tag as usize).checked_sub(TAG_HEADER) else {
        return Vec::new();
    };
    let mut body = vec![0u8; body_len];
    if file.read_exact(&mut body).is_err() || body.len() < 8 {
        return Vec::new();
    }
    let len_data = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let columns = &body[8..];
    if len_data == 0 || len_data > columns.len() {
        return Vec::new();
    }
    columns[..len_data].to_vec()
}

/// The `PWV4` columns, decoded to the four bytes a renderer needs.
///
/// The tag's header carries the entry width, the entry count and an unknown
/// word before the entries themselves.
fn colour_columns(file: &mut std::fs::File, len_tag: u32) -> Vec<AnlzPreviewColumn> {
    let Some(body_len) = (len_tag as usize).checked_sub(TAG_HEADER) else {
        return Vec::new();
    };
    let mut body = vec![0u8; body_len];
    if file.read_exact(&mut body).is_err() || body.len() < 12 {
        return Vec::new();
    }
    let width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let count = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    // Six is what the format says and what every file has. Anything else is a
    // variant we cannot read, and guessing at it would draw a plausible but
    // wrong waveform -- worse than falling back to the monochrome one.
    if width != COLOUR_ENTRY {
        return Vec::new();
    }
    let entries = &body[12..];
    let usable = count.min(entries.len() / COLOUR_ENTRY);
    (0..usable)
        .map(|index| {
            let entry = &entries[index * COLOUR_ENTRY..];
            AnlzPreviewColumn {
                // Bytes 0 and 1 are deliberately not read: byte 1 correlates
                // *negatively* with the envelope across every track measured,
                // so whatever it is, it is not an energy.
                envelope: entry[2],
                bass: entry[3],
                mid: entry[4],
                treble: entry[5],
            }
        })
        .collect()
}

/// A preview file that could not be read at all.
fn preview_failed(error: String) -> AnlzPreview {
    AnlzPreview {
        ok: false,
        error,
        mono: Vec::new(),
        colour: Vec::new(),
    }
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
    fn the_seeking_reader_finds_both_previews() {
        // A PWAV, then a big tag it must seek over, then a PWV4. The middle
        // tag is what the function exists to not read.
        let mut pwav_body = 400u32.to_be_bytes().to_vec();
        pwav_body.extend_from_slice(&0x0010_0000u32.to_be_bytes());
        pwav_body.extend_from_slice(&[0b1010_0110; 400]);

        let mut pwv5_body = 2u32.to_be_bytes().to_vec();
        pwv5_body.extend_from_slice(&20_000u32.to_be_bytes());
        pwv5_body.extend_from_slice(&0u32.to_be_bytes());
        pwv5_body.extend_from_slice(&vec![0xab; 40_000]);

        let mut pwv4_body = 6u32.to_be_bytes().to_vec();
        pwv4_body.extend_from_slice(&2u32.to_be_bytes());
        pwv4_body.extend_from_slice(&0u32.to_be_bytes());
        // Two columns: unknown, unknown, envelope, bass, mid, treble.
        pwv4_body.extend_from_slice(&[0x11, 0x22, 90, 100, 50, 20]);
        pwv4_body.extend_from_slice(&[0x11, 0x22, 30, 40, 60, 70]);

        let bytes = file(&[
            tag(b"PWAV", 20, &pwav_body),
            tag(b"PWV5", 24, &pwv5_body),
            tag(b"PWV4", 24, &pwv4_body),
        ]);
        let dir = std::env::temp_dir().join("prolink-cxx-preview-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ANLZ0000.EXT");
        std::fs::write(&path, &bytes).unwrap();

        let preview = read_anlz_preview(path.to_str().unwrap());
        assert!(preview.ok, "{}", preview.error);
        assert_eq!(preview.mono.len(), 400);
        assert_eq!(preview.mono[0] & 0x1f, 6, "height in the low five bits");
        assert_eq!(preview.colour.len(), 2);
        assert_eq!(preview.colour[0].envelope, 90);
        assert_eq!(preview.colour[0].bass, 100, "byte 3 is the bottom third");
        assert_eq!(preview.colour[0].mid, 50);
        assert_eq!(preview.colour[0].treble, 20, "byte 5 is the top third");
        assert_eq!(preview.colour[1].bass, 40);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_colour_preview_of_the_wrong_width_is_refused() {
        // Six bytes per entry is what the format says. A variant we cannot read
        // would draw a plausible but wrong waveform, which is worse than
        // falling back to the monochrome one.
        let mut body = 8u32.to_be_bytes().to_vec();
        body.extend_from_slice(&1u32.to_be_bytes());
        body.extend_from_slice(&0u32.to_be_bytes());
        body.extend_from_slice(&[0; 8]);

        let dir = std::env::temp_dir().join("prolink-cxx-preview-test-width");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ANLZ0000.EXT");
        std::fs::write(&path, file(&[tag(b"PWV4", 24, &body)])).unwrap();

        let preview = read_anlz_preview(path.to_str().unwrap());
        assert!(preview.ok);
        assert!(preview.colour.is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_preview_file_is_a_value_and_not_a_panic() {
        let preview = read_anlz_preview("/nowhere/ANLZ0000.EXT");
        assert!(!preview.ok);
        assert!(preview.mono.is_empty());
        assert!(preview.colour.is_empty());
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
