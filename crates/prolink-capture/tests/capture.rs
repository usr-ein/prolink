// SPDX-License-Identifier: GPL-3.0-only

//! Two real capture files, small enough to commit, and the corpus behind them.
//!
//! The fixtures are **byte-for-byte extracts of real `tcpdump` output**: the
//! file header of a capture followed by a handful of its records, unmodified.
//! Nothing here was synthesised, so a test that passes proves this crate reads
//! what `tcpdump` writes rather than what we would have written. They are the
//! floor: the corpus tests below skip when there are no captures on this
//! machine, and a coverage regression must not be able to hide behind that.

// A test asserts by panicking; propagating errors here would report a failure
// as a pass.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::as_conversions
)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use prolink_capture::tcp::Reassembler;
use prolink_capture::{
    BEAT_PORT, Capture, Corpus, DISCOVERY_PORT, Error, Format, Packet, STATUS_PORT, Transport,
};

/// Four records of `captures/S24b-e9-control/run.pcap`, with its 24-byte
/// global header: a UDP-50000 keep-alive, a UDP-50002 media query, and the two
/// directions of the TCP-12523 dbserver port query. Classic pcap,
/// little-endian, link type Ethernet.
const PCAP: &[u8] = &[
    0xd4, 0xc3, 0xb2, 0xa1, 0x02, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x08, 0x00, 0x01, 0x00, 0x00, 0x00, 0xbf, 0x2a, 0x6b, 0x6a, 0x9f, 0x2f, 0x0f, 0x00,
    0x60, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x74, 0x5e,
    0x1c, 0x56, 0xca, 0x54, 0x08, 0x00, 0x45, 0x00, 0x00, 0x52, 0x19, 0x5f, 0x00, 0x00, 0x40, 0x11,
    0x42, 0xeb, 0xa9, 0xfe, 0xca, 0x54, 0xa9, 0xfe, 0xff, 0xff, 0xc3, 0x50, 0xc3, 0x50, 0x00, 0x3e,
    0x85, 0x65, 0x51, 0x73, 0x70, 0x74, 0x31, 0x57, 0x6d, 0x4a, 0x4f, 0x4c, 0x06, 0x00, 0x43, 0x44,
    0x4a, 0x2d, 0x32, 0x30, 0x30, 0x30, 0x6e, 0x65, 0x78, 0x75, 0x73, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x02, 0x00, 0x36, 0x02, 0x02, 0x74, 0x5e, 0x1c, 0x56, 0xca, 0x54, 0xa9, 0xfe,
    0xca, 0x54, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0xeb, 0x2a, 0x6b, 0x6a, 0x01, 0xd7, 0x04, 0x00,
    0x5a, 0x00, 0x00, 0x00, 0x5a, 0x00, 0x00, 0x00, 0xa0, 0xce, 0xc8, 0xe2, 0x26, 0xde, 0x74, 0x5e,
    0x1c, 0x56, 0xca, 0x54, 0x08, 0x00, 0x45, 0x60, 0x00, 0x4c, 0x19, 0xca, 0x00, 0x00, 0x40, 0x11,
    0xde, 0xc1, 0xa9, 0xfe, 0xca, 0x54, 0xa9, 0xfe, 0x63, 0x64, 0x0e, 0xae, 0xc3, 0x52, 0x00, 0x38,
    0xd5, 0x3f, 0x51, 0x73, 0x70, 0x74, 0x31, 0x57, 0x6d, 0x4a, 0x4f, 0x4c, 0x05, 0x43, 0x44, 0x4a,
    0x2d, 0x32, 0x30, 0x30, 0x30, 0x6e, 0x65, 0x78, 0x75, 0x73, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x02, 0x00, 0x0c, 0xa9, 0xfe, 0xca, 0x54, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00,
    0x00, 0x03, 0xeb, 0x2a, 0x6b, 0x6a, 0xda, 0x10, 0x05, 0x00, 0x49, 0x00, 0x00, 0x00, 0x49, 0x00,
    0x00, 0x00, 0xa0, 0xce, 0xc8, 0xe2, 0x26, 0xde, 0x74, 0x5e, 0x1c, 0x56, 0xca, 0x54, 0x08, 0x00,
    0x45, 0x00, 0x00, 0x3b, 0x19, 0xcc, 0x00, 0x00, 0x40, 0x06, 0xdf, 0x3b, 0xa9, 0xfe, 0xca, 0x54,
    0xa9, 0xfe, 0x63, 0x64, 0x04, 0x1d, 0x30, 0xeb, 0x00, 0x00, 0x48, 0xb9, 0x9b, 0x3e, 0x53, 0x7e,
    0x50, 0x18, 0x20, 0x00, 0xfd, 0xab, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0f, 0x52, 0x65, 0x6d, 0x6f,
    0x74, 0x65, 0x44, 0x42, 0x53, 0x65, 0x72, 0x76, 0x65, 0x72, 0x00, 0xeb, 0x2a, 0x6b, 0x6a, 0xd4,
    0x1b, 0x05, 0x00, 0x38, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00, 0x74, 0x5e, 0x1c, 0x56, 0xca,
    0x54, 0xa0, 0xce, 0xc8, 0xe2, 0x26, 0xde, 0x08, 0x00, 0x45, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x40,
    0x00, 0xff, 0x06, 0xfa, 0x17, 0xa9, 0xfe, 0x63, 0x64, 0xa9, 0xfe, 0xca, 0x54, 0x30, 0xeb, 0x04,
    0x1d, 0x9b, 0x3e, 0x53, 0x7e, 0x00, 0x00, 0x48, 0xcc, 0x50, 0x18, 0xff, 0xff, 0x00, 0x93, 0x00,
    0x00, 0xc0, 0xf0,
];

/// Five blocks of `captures/S20-browse-ground-truth/run.pcap`: the section
/// header, the two interface descriptions (`en12` and `en9`, both Ethernet,
/// neither declaring `if_tsresol`), and the same keep-alive as recorded by each
/// of them — which is what a capture of a bridge looks like.
const PCAPNG: &[u8] = &[
    0x0a, 0x0d, 0x0d, 0x0a, 0xbc, 0x00, 0x00, 0x00, 0x4d, 0x3c, 0x2b, 0x1a, 0x01, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02, 0x00, 0x05, 0x00, 0x61, 0x72, 0x6d, 0x36,
    0x34, 0x00, 0x00, 0x00, 0x03, 0x00, 0x66, 0x00, 0x44, 0x61, 0x72, 0x77, 0x69, 0x6e, 0x20, 0x4b,
    0x65, 0x72, 0x6e, 0x65, 0x6c, 0x20, 0x56, 0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x20, 0x32, 0x35,
    0x2e, 0x35, 0x2e, 0x30, 0x3a, 0x20, 0x4d, 0x6f, 0x6e, 0x20, 0x41, 0x70, 0x72, 0x20, 0x32, 0x37,
    0x20, 0x32, 0x30, 0x3a, 0x33, 0x39, 0x3a, 0x30, 0x39, 0x20, 0x50, 0x44, 0x54, 0x20, 0x32, 0x30,
    0x32, 0x36, 0x3b, 0x20, 0x72, 0x6f, 0x6f, 0x74, 0x3a, 0x78, 0x6e, 0x75, 0x2d, 0x31, 0x32, 0x33,
    0x37, 0x37, 0x2e, 0x31, 0x32, 0x31, 0x2e, 0x36, 0x7e, 0x32, 0x2f, 0x52, 0x45, 0x4c, 0x45, 0x41,
    0x53, 0x45, 0x5f, 0x41, 0x52, 0x4d, 0x36, 0x34, 0x5f, 0x54, 0x36, 0x30, 0x32, 0x30, 0x00, 0x00,
    0x04, 0x00, 0x20, 0x00, 0x74, 0x63, 0x70, 0x64, 0x75, 0x6d, 0x70, 0x20, 0x28, 0x6c, 0x69, 0x62,
    0x70, 0x63, 0x61, 0x70, 0x20, 0x76, 0x65, 0x72, 0x73, 0x69, 0x6f, 0x6e, 0x20, 0x31, 0x2e, 0x31,
    0x30, 0x2e, 0x31, 0x29, 0x00, 0x00, 0x00, 0x00, 0xbc, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x20, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x02, 0x00, 0x03, 0x00,
    0x65, 0x6e, 0x39, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x20, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x02, 0x00, 0x04, 0x00,
    0x65, 0x6e, 0x31, 0x32, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00,
    0x94, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc7, 0x57, 0x06, 0x00, 0x64, 0x4e, 0xaa, 0x81,
    0x60, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x74, 0x5e,
    0x1c, 0x56, 0x67, 0xac, 0x08, 0x00, 0x45, 0x00, 0x00, 0x52, 0x66, 0x6d, 0x00, 0x00, 0x40, 0x11,
    0x58, 0x85, 0xa9, 0xfe, 0x67, 0xac, 0xa9, 0xfe, 0xff, 0xff, 0xc3, 0x50, 0xc3, 0x50, 0x00, 0x3e,
    0xae, 0x5f, 0x51, 0x73, 0x70, 0x74, 0x31, 0x57, 0x6d, 0x4a, 0x4f, 0x4c, 0x06, 0x00, 0x43, 0x44,
    0x4a, 0x2d, 0x32, 0x30, 0x30, 0x30, 0x6e, 0x65, 0x78, 0x75, 0x73, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x02, 0x00, 0x36, 0x01, 0x01, 0x74, 0x5e, 0x1c, 0x56, 0x67, 0xac, 0xa9, 0xfe,
    0x67, 0xac, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00, 0x04, 0x00, 0x02, 0x00, 0x00, 0x00,
    0x02, 0x80, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x94, 0x00, 0x00, 0x00,
    0x06, 0x00, 0x00, 0x00, 0x94, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xc7, 0x57, 0x06, 0x00,
    0xb3, 0x4e, 0xaa, 0x81, 0x60, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0x74, 0x5e, 0x1c, 0x56, 0x67, 0xac, 0x08, 0x00, 0x45, 0x00, 0x00, 0x52, 0x66, 0x6d,
    0x00, 0x00, 0x40, 0x11, 0x58, 0x85, 0xa9, 0xfe, 0x67, 0xac, 0xa9, 0xfe, 0xff, 0xff, 0xc3, 0x50,
    0xc3, 0x50, 0x00, 0x3e, 0xae, 0x5f, 0x51, 0x73, 0x70, 0x74, 0x31, 0x57, 0x6d, 0x4a, 0x4f, 0x4c,
    0x06, 0x00, 0x43, 0x44, 0x4a, 0x2d, 0x32, 0x30, 0x30, 0x30, 0x6e, 0x65, 0x78, 0x75, 0x73, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x36, 0x01, 0x01, 0x74, 0x5e, 0x1c, 0x56,
    0x67, 0xac, 0xa9, 0xfe, 0x67, 0xac, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00, 0x04, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x02, 0x80, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x94, 0x00, 0x00, 0x00,
];

/// Offset of the first record's UDP source port inside [`PCAP`].
///
/// 24-byte global header, 16-byte record header, 14-byte Ethernet header,
/// 20-byte IPv4 header. Asserted before it is used, so it cannot rot quietly.
const FIRST_UDP_SOURCE_PORT: usize = 24 + 16 + 14 + 20;

/// Offset of the link type in a classic pcap global header.
const PCAP_LINK_TYPE: usize = 20;

/// The five bytes a dbserver conversation opens with. Written out rather than
/// imported: this crate is link and transport only, and the magic belongs to
/// whoever asked for the bytes.
const DBSERVER_PREAMBLE: &[u8] = &[0x11, 0x00, 0x00, 0x00, 0x01];

fn read(bytes: &[u8]) -> Vec<Packet> {
    Capture::new(Cursor::new(bytes.to_vec()))
        .expect("a capture")
        .collect::<Result<Vec<_>, _>>()
        .expect("every record to dissect")
}

// -- the committed fixtures ------------------------------------------------

#[test]
fn a_classic_pcap_reads_every_record_it_holds() {
    let capture = Capture::new(Cursor::new(PCAP)).unwrap();
    assert_eq!(capture.format(), Format::Pcap);

    let packets = read(PCAP);
    assert_eq!(packets.len(), 4);

    let ports: Vec<(u16, u16)> = packets
        .iter()
        .map(|packet| (packet.source.port(), packet.destination.port()))
        .collect();
    assert_eq!(
        ports,
        vec![(50000, 50000), (3758, 50002), (1053, 12523), (12523, 1053)]
    );

    let lengths: Vec<usize> = packets.iter().map(|packet| packet.payload.len()).collect();
    assert_eq!(lengths, vec![54, 48, 19, 2]);

    assert!(packets[0].transport.is_udp());
    assert!(packets[2].transport.is_tcp());
    assert_eq!(packets[0].index, 1, "the first record of the file");
    assert!(
        packets.iter().all(|packet| packet.interface == 0),
        "classic pcap has one interface"
    );
}

#[test]
fn a_payload_stops_where_the_ip_header_says_it_does() {
    let packets = read(PCAP);
    let keep_alive = &packets[0];
    // 54 bytes, the length a CDJ-2000nexus keep-alive is, and `stype` at 0x23
    // equals the datagram length. An Ethernet frame shorter than 60 bytes is
    // padded and the padding is not payload; trusting the captured frame
    // length rather than the IP header's would show it as payload here.
    assert_eq!(keep_alive.payload.len(), 54);
    assert_eq!(&keep_alive.payload[..10], b"Qspt1WmJOL");
    assert_eq!(
        usize::from(keep_alive.payload[0x23]),
        keep_alive.payload.len()
    );
}

#[test]
fn a_classic_pcap_timestamp_is_a_real_wall_clock() {
    let packets = read(PCAP);
    // 2026-07-30, when the corpus was recorded. Anything near the epoch would
    // mean the resolution was misread.
    let seconds = packets[0].timestamp.as_secs();
    assert!(
        (1_780_000_000..2_000_000_000).contains(&seconds),
        "implausible time {seconds}"
    );
    assert!(
        packets[0].timestamp < packets[3].timestamp,
        "records are in capture order"
    );
}

#[test]
fn udp_to_selects_by_destination() {
    let to_discovery: Vec<Packet> = Capture::new(Cursor::new(PCAP))
        .unwrap()
        .udp_to(DISCOVERY_PORT)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(to_discovery.len(), 1);
    assert_eq!(to_discovery[0].payload.len(), 54);

    let to_status: Vec<Packet> = Capture::new(Cursor::new(PCAP))
        .unwrap()
        .udp_to(STATUS_PORT)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(to_status.len(), 1);
    assert_eq!(to_status[0].payload.len(), 48);

    let to_beats: Vec<Packet> = Capture::new(Cursor::new(PCAP))
        .unwrap()
        .udp_to(BEAT_PORT)
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(to_beats.is_empty(), "an empty result is not an error");
}

#[test]
fn a_keep_alive_sent_from_the_status_port_is_still_not_a_status_packet() {
    // The trap, with the real bytes. A tool that binds one socket sends its
    // keep-alives *from* 50002; under the 50002 layout a type-0x06 keep-alive
    // decodes as a media response, with no error and every field wrong. So
    // rewrite this real keep-alive's source port to 50002 and nothing else.
    let mut doctored = PCAP.to_vec();
    assert_eq!(
        u16::from_be_bytes([
            doctored[FIRST_UDP_SOURCE_PORT],
            doctored[FIRST_UDP_SOURCE_PORT + 1]
        ]),
        50000,
        "the constant no longer points at the first record's UDP source port"
    );
    doctored[FIRST_UDP_SOURCE_PORT..FIRST_UDP_SOURCE_PORT + 2]
        .copy_from_slice(&STATUS_PORT.to_be_bytes());

    let packets = read(&doctored);
    assert_eq!(packets[0].source.port(), STATUS_PORT);
    assert_eq!(packets[0].destination.port(), DISCOVERY_PORT);

    let to_status: Vec<Packet> = Capture::new(Cursor::new(doctored.clone()))
        .unwrap()
        .udp_to(STATUS_PORT)
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(to_status.len(), 1, "only the datagram addressed to 50002");
    assert_eq!(
        to_status[0].payload.len(),
        48,
        "the media query, not the keep-alive"
    );
    assert_eq!(packets[0].udp_payload_to(STATUS_PORT), None);
    assert!(packets[0].udp_payload_to(DISCOVERY_PORT).is_some());
}

#[test]
fn a_pcapng_capture_reads_its_blocks() {
    let capture = Capture::new(Cursor::new(PCAPNG)).unwrap();
    assert_eq!(capture.format(), Format::PcapNg);

    let packets = read(PCAPNG);
    assert_eq!(packets.len(), 2);
    assert_eq!(
        packets[0].payload, packets[1].payload,
        "one datagram, recorded twice"
    );
    assert_eq!(packets[0].destination.port(), DISCOVERY_PORT);
    assert_eq!(packets[0].payload.len(), 54);
}

#[test]
fn a_bridged_capture_records_the_same_datagram_once_per_interface() {
    let packets = read(PCAPNG);
    // Nothing on the wire distinguishes this from a genuine repeat, so the
    // interface id is the only thing that can, and it is carried for that.
    assert_eq!(packets[0].interface, 0);
    assert_eq!(packets[1].interface, 1);
    assert_eq!(packets[0].source, packets[1].source);
}

#[test]
fn a_pcapng_timestamp_is_scaled_by_the_interface_resolution() {
    let packets = read(PCAPNG);
    // Neither interface declares if_tsresol, so the counter is microseconds.
    // Read as nanoseconds — which is what the underlying reader hands back —
    // this would be 1 785 364 s, three weeks after the epoch instead of 2026.
    let seconds = packets[0].timestamp.as_secs();
    assert_eq!(seconds, 1_785_364_245, "1785364245.794404, July 2026");
    assert_eq!(packets[0].timestamp.subsec_micros(), 794_404);
    assert!(packets[0].timestamp < packets[1].timestamp);
}

#[test]
fn the_two_directions_of_a_connection_become_two_streams() {
    let mut reassembler = Reassembler::new();
    for packet in read(PCAP) {
        reassembler.push(&packet);
    }
    let streams = reassembler.finish();
    assert_eq!(streams.len(), 2);

    // The deck asks the dbserver port-query service its one question...
    let query = streams[0].contiguous().expect("no segment is missing");
    assert_eq!(&query[4..], b"RemoteDBServer\0");
    // ...and the answer travels the other way, so it is a stream of its own.
    assert_eq!(streams[1].flow(), streams[0].flow().reversed());
    assert_eq!(streams[1].contiguous(), Some([0xc0, 0xf0].as_slice()));

    // The capture opens mid-connection: the SYN is not in these four records,
    // so offset zero is the earliest byte seen and not the connection's first.
    assert!(!streams[0].from_connection_start());
}

#[test]
fn a_port_filter_keeps_only_the_flows_it_names() {
    let mut reassembler = Reassembler::on_ports([12523]);
    for packet in read(PCAP) {
        reassembler.push(&packet);
    }
    assert_eq!(
        reassembler.finish().len(),
        2,
        "both directions belong to the server's port"
    );

    let mut elsewhere = Reassembler::on_ports([1051]);
    for packet in read(PCAP) {
        elsewhere.push(&packet);
    }
    assert!(elsewhere.finish().is_empty());
}

#[test]
fn a_file_that_is_not_a_capture_is_rejected_rather_than_read_as_one() {
    let error = Capture::new(Cursor::new(b"{\"hex\": \"5173707431\"}".to_vec())).unwrap_err();
    assert!(matches!(error, Error::NotACapture { .. }), "got {error:?}");
    assert!(
        !error.is_truncated(),
        "a JSONL journal is not a cut-short capture"
    );

    let short = Capture::new(Cursor::new(b"ab".to_vec())).unwrap_err();
    assert!(matches!(short, Error::NotACapture { .. }), "got {short:?}");
}

#[test]
fn a_capture_cut_short_costs_its_tail_and_says_which_it_was() {
    // `tcpdump` killed rather than stopped: the last record is half-written.
    let truncated = &PCAP[..PCAP.len() - 20];
    let mut capture = Capture::new(Cursor::new(truncated.to_vec())).unwrap();
    let mut good = 0;
    let mut failure = None;
    for packet in capture.by_ref() {
        match packet {
            Ok(_) => good += 1,
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    assert_eq!(good, 3, "everything before the cut is still good");
    let failure = failure.expect("the cut must be reported, not passed off as the end of file");
    assert!(failure.is_truncated(), "got {failure:?}");
    assert!(
        capture.next().is_none(),
        "iteration stops after a hard error"
    );
}

#[test]
fn a_link_layer_we_do_not_dissect_is_an_error_and_not_an_empty_result() {
    let mut foreign = PCAP.to_vec();
    // 113 is Linux cooked capture. A capture of one would otherwise read as a
    // capture of a network with no Pro DJ Link traffic on it.
    foreign[PCAP_LINK_TYPE..PCAP_LINK_TYPE + 4].copy_from_slice(&113u32.to_le_bytes());
    let error = Capture::new(Cursor::new(foreign)).unwrap_err();
    assert!(
        matches!(error, Error::UnsupportedLinkType { link_type: 113 }),
        "got {error:?}"
    );
}

#[test]
fn a_capture_is_not_read_into_memory_to_be_iterated() {
    // Not a memory measurement — just the shape that makes one possible: the
    // reader is consumed lazily, so a 78 MB file costs a frame at a time.
    let mut capture = Capture::new(Cursor::new(PCAP)).unwrap();
    let first = capture.next().expect("a first packet").unwrap();
    assert_eq!(first.index, 1);
    let rest: Vec<Packet> = capture.collect::<Result<_, _>>().unwrap();
    assert_eq!(rest.len(), 3);
}

#[test]
fn a_udp_packet_carries_no_sequence_number() {
    // The kind of state the type makes unrepresentable: `Transport::Udp` has
    // no room for one, so nothing downstream can read a stale value.
    let packets = read(PCAP);
    assert_eq!(packets[0].transport, Transport::Udp);
    let Transport::Tcp { sequence, syn, .. } = packets[2].transport else {
        panic!("record 3 is TCP");
    };
    assert_eq!(sequence, 18617);
    assert!(!syn);
}

// -- the corpus ------------------------------------------------------------

/// What one walk of the corpus measured.
#[derive(Default)]
struct Census {
    files: usize,
    packets: u64,
    by_destination_port: BTreeMap<u16, u64>,
    from_status_port: u64,
    udp_payload_bytes: u64,
    largest_udp_payload: usize,
    streams: usize,
    contiguous_streams: usize,
    dbserver_streams: usize,
    dbserver_endpoints: BTreeSet<u16>,
    stream_bytes: u64,
    truncated: Vec<String>,
}

/// One pass over every capture, measuring everything the assertions below need.
///
/// Kept to a single pass because the corpus is 240 MB and a test that walked it
/// once per question would not be run.
fn census(corpus: &Corpus) -> Census {
    let mut census = Census::default();
    for path in corpus.captures() {
        let capture = match Capture::open(&path) {
            Ok(capture) => capture,
            Err(error) => {
                assert!(error.is_truncated(), "{}: {error}", path.display());
                census.truncated.push(path.display().to_string());
                continue;
            }
        };
        census.files += 1;

        let mut reassembler = Reassembler::new();
        for packet in capture {
            let packet = match packet {
                Ok(packet) => packet,
                Err(error) => {
                    // A file cut short costs that file and not the run, which
                    // is the caller's choice to make and this caller makes it.
                    assert!(error.is_truncated(), "{}: {error}", path.display());
                    census.truncated.push(path.display().to_string());
                    break;
                }
            };
            census.packets += 1;
            if packet.transport.is_udp() {
                *census
                    .by_destination_port
                    .entry(packet.destination.port())
                    .or_default() += 1;
                if packet.source.port() == STATUS_PORT {
                    census.from_status_port += 1;
                }
                census.udp_payload_bytes += packet.payload.len() as u64;
                census.largest_udp_payload = census.largest_udp_payload.max(packet.payload.len());
            }
            reassembler.push(&packet);
        }

        for stream in reassembler.finish() {
            census.streams += 1;
            census.stream_bytes += stream.len();
            if let Some(bytes) = stream.contiguous() {
                census.contiguous_streams += 1;
                if bytes.starts_with(DBSERVER_PREAMBLE) {
                    census.dbserver_streams += 1;
                    census
                        .dbserver_endpoints
                        .insert(stream.flow().source.port());
                    census
                        .dbserver_endpoints
                        .insert(stream.flow().destination.port());
                }
            }
        }
    }
    census
}

#[test]
fn the_corpus_reads_and_holds_what_it_is_supposed_to() {
    let Some(corpus) = Corpus::locate() else {
        eprintln!(
            "skipping: no capture corpus. Set {} to a directory of pcap files.",
            prolink_capture::CORPUS_ENV
        );
        return;
    };
    let census = census(&corpus);

    eprintln!("corpus: {}", corpus.root().display());
    eprintln!("{} files, {} packets", census.files, census.packets);
    for (port, count) in &census.by_destination_port {
        if *count >= 500 {
            eprintln!("  udp to {port:>5}: {count}");
        }
    }
    eprintln!("  udp from {STATUS_PORT}: {}", census.from_status_port);
    eprintln!(
        "  udp payload: {} bytes, largest {}",
        census.udp_payload_bytes, census.largest_udp_payload
    );
    eprintln!(
        "  tcp: {} streams ({} whole), {} bytes; {} dbserver, between ports {:?}",
        census.streams,
        census.contiguous_streams,
        census.stream_bytes,
        census.dbserver_streams,
        census.dbserver_endpoints
    );
    for path in &census.truncated {
        eprintln!("  truncated: {path}");
    }

    assert!(
        census.files >= 1,
        "the corpus directory holds no capture files"
    );
    let to = |port: u16| census.by_destination_port.get(&port).copied().unwrap_or(0);

    // Floors, not equalities: another machine's corpus is a different size.
    // Measured on the 33-file corpus this crate was written against:
    // 7519 to 50000, 1110 to 50001, 35103 to 50002, 8286 to 2049.
    assert!(
        to(DISCOVERY_PORT) >= 5_000,
        "only {} datagrams to 50000",
        to(DISCOVERY_PORT)
    );
    assert!(
        to(STATUS_PORT) >= 25_000,
        "only {} datagrams to 50002",
        to(STATUS_PORT)
    );
    assert!(
        to(BEAT_PORT) >= 500,
        "only {} datagrams to 50001",
        to(BEAT_PORT)
    );
    assert!(to(2049) >= 5_000, "only {} datagrams to NFS", to(2049));

    // A CDJ reads 8192 bytes at a time over NFSv2, which crosses the wire as
    // five or six IP fragments of which only the first has a UDP header. A
    // reader that dropped fragments would still find NFS traffic and would
    // still count datagrams; it would just report a fraction of the bytes.
    assert!(
        census.udp_payload_bytes >= 150_000_000,
        "only {} bytes of UDP payload; the corpus this was written against held 205 641 007",
        census.udp_payload_bytes
    );
    assert!(
        census.largest_udp_payload > 4_000,
        "largest UDP payload is {} bytes: IP fragments are being dropped",
        census.largest_udp_payload
    );

    assert!(census.streams >= 50, "only {} TCP streams", census.streams);
    assert!(
        census.dbserver_streams >= 5,
        "only {} dbserver streams",
        census.dbserver_streams
    );
    assert!(
        census.stream_bytes >= 1_000_000,
        "only {} bytes of TCP reassembled",
        census.stream_bytes
    );
}

#[test]
fn the_corpus_shows_why_the_filter_belongs_on_the_destination() {
    let Some(corpus) = Corpus::locate() else {
        eprintln!("skipping: no capture corpus");
        return;
    };
    // One file is enough for this: it is a claim about which end of a datagram
    // carries the protocol, not about how much traffic there is.
    let mut addressed = 0u64;
    let mut sent_from = 0u64;
    for path in corpus.captures().into_iter().take(4) {
        let Ok(capture) = Capture::open(&path) else {
            continue;
        };
        for packet in capture.flatten() {
            if !packet.transport.is_udp() {
                continue;
            }
            if packet.destination.port() == STATUS_PORT {
                addressed += 1;
            }
            if packet.source.port() == STATUS_PORT {
                sent_from += 1;
            }
        }
    }
    assert!(
        addressed > 0,
        "no status traffic in the first files of the corpus"
    );
    // A CDJ unicasts status from a fresh ephemeral port each boot, so the
    // source port is not the protocol's and a filter placed on it finds
    // almost nothing. Corpus-wide the ratio is 35103 to 24.
    assert!(
        sent_from * 10 < addressed,
        "{sent_from} datagrams sent from 50002 against {addressed} addressed to it: \
         if these are close, the corpus changed and the doc comments should say so"
    );
}
