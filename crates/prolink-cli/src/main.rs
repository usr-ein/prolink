// Copyright (C) 2026 the prolink authors.
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, version 3.
//
// SPDX-License-Identifier: GPL-3.0-only

//! Command line tools for the Pioneer Pro DJ Link protocol.
//!
//! Every command except `announce` and `serve` is **passive**: it transmits
//! nothing on any Pro DJ Link port, so it can be run beside a live rig without
//! contending for a device number.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use prolink::virtual_cdj::{Numbering, VirtualCdjConfig};
use prolink::{
    BrowsableDeviceNumber, DeviceName, DeviceNumber, Discovery, Interface, VirtualCdj,
    discovery::SCAN_DURATION,
};
use tracing_subscriber::EnvFilter;

/// Browse and serve Pioneer Pro DJ Link media.
#[derive(Debug, Parser)]
#[command(name = "prolink", version, about, long_about = None)]
struct Cli {
    /// The interface facing the CDJs. Guessed from the link-local address when
    /// not given, which is right on a single-homed host and a coin toss on a
    /// multi-homed one.
    #[arg(long, short, global = true)]
    interface: Option<String>,

    /// More detail. Repeat for more still.
    #[arg(long, short, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List the network interfaces that could carry Pro DJ Link traffic.
    Interfaces,

    /// Watch the network and list the players on it. Transmits nothing.
    Devices {
        /// Keep watching and report changes as they happen.
        #[arg(long, short)]
        watch: bool,

        /// How long to listen for before reporting, in seconds.
        #[arg(long, default_value_t = SCAN_DURATION.as_secs_f32())]
        seconds: f32,
    },

    /// Announce as a virtual CDJ, so peers will unicast their status to us.
    ///
    /// **This transmits.** Without `--claim` it takes a device number outside
    /// the 1–6 player range and cannot collide with hardware; with it, it
    /// contends for a real player slot.
    Announce {
        /// Claim a browsable number in 1–4. Required to be browsed by a peer,
        /// and the only mode that can disturb a live rig.
        #[arg(long)]
        claim: bool,

        /// The number to try first when claiming.
        #[arg(long, value_parser = browsable_number)]
        number: Option<BrowsableDeviceNumber>,

        /// The name to announce. `CDJ-2000nexus` is what real hardware sends.
        #[arg(long, default_value = DeviceName::CDJ_2000_NEXUS)]
        name: String,

        /// Keep-alive byte 0x35. `0x64` is required to coexist with CDJ-3000s
        /// set to player 5 or 6.
        #[arg(long, default_value_t = 0x00, value_parser = maybe_hex)]
        generation: u8,
    },

    /// Read a rekordbox `export.pdb` and list what is on it.
    Tracks {
        /// The database, usually `PIONEER/rekordbox/export.pdb` on the medium.
        file: PathBuf,

        /// Show the playlist tree instead of the tracks.
        #[arg(long)]
        playlists: bool,

        /// Only tracks matching this term, in title, artist or album.
        #[arg(long)]
        search: Option<String>,
    },

    /// Summarise the Pro DJ Link traffic in a pcap or pcapng capture.
    Pcap {
        /// The capture file.
        file: PathBuf,
    },
}

fn browsable_number(text: &str) -> Result<BrowsableDeviceNumber, String> {
    let raw: u8 = text
        .parse()
        .map_err(|_| format!("{text} is not a number"))?;
    BrowsableDeviceNumber::new(raw).ok_or_else(|| {
        format!(
            "{raw} is not browsable: a peer only ever browses devices 1-{}, and outside that \
             range it accepts the announcement and then silently never asks",
            DeviceNumber::MAX_BROWSABLE
        )
    })
}

fn maybe_hex(text: &str) -> Result<u8, String> {
    let parsed = match text.strip_prefix("0x") {
        Some(hex) => u8::from_str_radix(hex, 16),
        None => text.parse(),
    };
    parsed.map_err(|_| format!("{text} is not a byte"))
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            let mut source = std::error::Error::source(&*error);
            while let Some(inner) = source {
                eprintln!("  caused by: {inner}");
                source = inner.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn init_tracing(verbosity: u8) {
    let default = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("prolink={default},prolink_cli={default}")));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

type BoxedError = Box<dyn std::error::Error + Send + Sync>;

async fn run(cli: Cli) -> Result<(), BoxedError> {
    match cli.command {
        Command::Interfaces => list_interfaces(),
        Command::Devices { watch, seconds } => {
            let interface = choose_interface(cli.interface.as_deref())?;
            watch_devices(interface, watch, Duration::from_secs_f32(seconds)).await
        }
        Command::Announce {
            claim,
            number,
            name,
            generation,
        } => {
            let interface = choose_interface(cli.interface.as_deref())?;
            announce(interface, claim, number, &name, generation).await
        }
        Command::Tracks {
            file,
            playlists,
            search,
        } => list_tracks(&file, playlists, search.as_deref()),
        Command::Pcap { file } => summarise_capture(&file),
    }
}

/// Read a rekordbox database and print what is on the medium.
fn list_tracks(
    path: &std::path::Path,
    playlists: bool,
    search: Option<&str>,
) -> Result<(), BoxedError> {
    let raw = std::fs::read(path)?;
    let library = prolink_rekordbox::Library::parse(&raw)?;
    let summary = library.summary();
    eprintln!(
        "{} tracks, {} artists, {} albums, {} genres, {} keys, {} playlists in {} folders",
        summary.tracks,
        summary.artists,
        summary.albums,
        summary.genres,
        summary.keys,
        summary.playlists,
        summary.folders,
    );

    if playlists {
        print_playlists(&library, &library.root_playlists(), 0);
        return Ok(());
    }

    let tracks = match search {
        Some(term) => library.search(term),
        None => library.track_list(),
    };
    for track in tracks {
        println!(
            "{:>8}  {:>6}  {:>6.1}  {:<4} {:<28}  {}",
            track.id,
            track.duration_text(),
            track.bpm(),
            track.key,
            truncate(&track.artist, 28),
            truncate(&track.title, 40),
        );
    }
    Ok(())
}

fn print_playlists(
    library: &prolink_rekordbox::Library,
    playlists: &[&prolink_rekordbox::Playlist],
    depth: usize,
) {
    for playlist in playlists {
        let indent = "  ".repeat(depth);
        let marker = if playlist.is_folder { "[+]" } else { "   " };
        let count = if playlist.is_folder {
            String::new()
        } else {
            format!("  ({} tracks)", playlist.track_count())
        };
        println!(
            "{indent}{marker} {}{count}  #{}",
            playlist.name, playlist.id
        );
        let children: Vec<&prolink_rekordbox::Playlist> = playlist
            .children
            .iter()
            .filter_map(|id| library.playlists.get(id))
            .collect();
        print_playlists(library, &children, depth + 1);
    }
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

/// Count the Pro DJ Link traffic in a capture.
///
/// Filters on the **destination** port throughout. The type byte at 0x0a is
/// shared across ports and the layouts behind it are not — 0x06 is a keep-alive
/// on 50000 and a media response on 50002 — so counting "either endpoint" would
/// attribute a tool's keep-alives, which it sends *from* 50002, to the wrong
/// protocol and decode them into confident nonsense.
fn summarise_capture(path: &std::path::Path) -> Result<(), BoxedError> {
    use std::collections::BTreeMap;

    let capture = prolink_capture::Capture::open(path)?;
    eprintln!("{} ({:?})", path.display(), capture.format());

    let mut per_port: BTreeMap<u16, usize> = BTreeMap::new();
    let mut djl_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut status_kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut tcp_bytes = 0usize;

    for packet in capture {
        let packet = packet?;
        let port = packet.destination.port();
        *per_port.entry(port).or_default() += 1;
        if packet.transport.is_tcp() {
            tcp_bytes += packet.payload.len();
            continue;
        }

        match port {
            prolink_capture::DISCOVERY_PORT => {
                if let Ok(decoded) = prolink_proto::djl::Packet::decode(&packet.payload) {
                    let kind = decoded.kind();
                    let name = kind
                        .name()
                        .map_or_else(|| format!("{kind:?}"), str::to_owned);
                    *djl_kinds.entry(name).or_default() += 1;
                }
            }
            prolink_capture::STATUS_PORT => {
                if let Ok(decoded) = prolink_proto::status::decode(&packet.payload) {
                    let kind = decoded.kind();
                    let name = kind
                        .name()
                        .map_or_else(|| format!("{kind:?}"), str::to_owned);
                    *status_kinds.entry(name).or_default() += 1;
                }
            }
            _ => {}
        }
    }

    println!("packets by destination port:");
    for (port, count) in &per_port {
        let label = match *port {
            prolink_capture::DISCOVERY_PORT => "  discovery",
            prolink_capture::BEAT_PORT => "  beat",
            prolink_capture::STATUS_PORT => "  status",
            111 => "  portmap",
            2049 => "  nfs",
            48276 => "  mountd",
            1051 => "  dbserver",
            12523 => "  dbserver port query",
            _ => "",
        };
        println!("  {port:>6}  {count:>8}{label}");
    }
    if tcp_bytes > 0 {
        println!("  TCP payload bytes: {tcp_bytes}");
    }
    for (title, counts) in [("UDP 50000", &djl_kinds), ("UDP 50002", &status_kinds)] {
        if counts.is_empty() {
            continue;
        }
        println!("{title} by kind:");
        for (kind, count) in counts {
            println!("  {kind:<20} {count:>8}");
        }
    }
    Ok(())
}

fn choose_interface(name: Option<&str>) -> Result<Interface, BoxedError> {
    let interface = match name {
        Some(name) => Interface::named(name)?,
        None => Interface::best_guess()?,
    };
    if !interface.is_link_local() {
        eprintln!(
            "note: {} is not on 169.254.0.0/16, which is what Pro DJ Link self-assigns from. \
             Pass --interface if this is the wrong one.",
            interface.name
        );
    }
    Ok(interface)
}

fn list_interfaces() -> Result<(), BoxedError> {
    let interfaces = Interface::list()?;
    if interfaces.is_empty() {
        println!("no interface has both an IPv4 address and a MAC");
        return Ok(());
    }
    println!(
        "{:<10}  {:<15}  {:<15}  {:<17}",
        "NAME", "ADDRESS", "BROADCAST", "MAC"
    );
    for interface in interfaces {
        println!(
            "{:<10}  {:<15}  {:<15}  {:<17}  {}",
            interface.name,
            interface.ip,
            interface.broadcast(),
            interface.mac,
            if interface.is_link_local() {
                "link-local — probably this one"
            } else {
                ""
            },
        );
    }
    Ok(())
}

async fn watch_devices(
    interface: Interface,
    watch: bool,
    duration: Duration,
) -> Result<(), BoxedError> {
    eprintln!("listening on {interface} (transmitting nothing)");
    let discovery = Discovery::start(interface).await?;

    if !watch {
        tokio::time::sleep(duration).await;
        let devices = discovery.devices();
        if devices.is_empty() {
            println!(
                "nothing found in {:.1}s. A CDJ broadcasts every 2 s and takes about nine \
                 seconds to say anything after power-on.",
                duration.as_secs_f32()
            );
        }
        for device in devices {
            println!("{device}");
        }
        return Ok(());
    }

    let mut events = discovery.subscribe();
    for device in discovery.devices() {
        println!("{device}");
    }
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => println!("{:<12} {}", label(&event), event.device()),
                Err(_) => return Ok(()),
            },
            result = tokio::signal::ctrl_c() => {
                result?;
                return Ok(());
            }
        }
    }
}

fn label(event: &prolink::DeviceEvent) -> &'static str {
    match event {
        prolink::DeviceEvent::Found(_) => "found",
        prolink::DeviceEvent::Updated(_) => "updated",
        prolink::DeviceEvent::WentOffline(_) => "offline",
        prolink::DeviceEvent::CameBack(_) => "back",
        prolink::DeviceEvent::Forgotten(_) => "gone",
    }
}

async fn announce(
    interface: Interface,
    claim: bool,
    preferred: Option<BrowsableDeviceNumber>,
    name: &str,
    generation: u8,
) -> Result<(), BoxedError> {
    let numbering = if claim {
        Numbering::Claim { preferred }
    } else {
        if preferred.is_some() {
            eprintln!("note: --number only applies with --claim; announcing as an observer");
        }
        Numbering::default()
    };

    let discovery = Discovery::start(interface).await?;
    // Watch first, so the claim chain knows whether we were alone — a real deck
    // latches that at boot and it drives both the repeat count and keep-alive
    // byte 0x25.
    tokio::time::sleep(SCAN_DURATION).await;

    let config = VirtualCdjConfig {
        name: DeviceName::new(name),
        numbering,
        generation,
        ..VirtualCdjConfig::default()
    };
    let cdj = VirtualCdj::observe(&discovery, config).await?;

    match cdj.browsable_number() {
        Some(number) => {
            println!("announcing as device {number} — a peer will offer us as a source");
        }
        None => {
            println!(
                "announcing as device {} — visible, but outside 1-{} no peer will ever browse us",
                cdj.number(),
                DeviceNumber::MAX_BROWSABLE
            );
        }
    }
    println!("ctrl-c to stop");

    tokio::signal::ctrl_c().await?;
    Ok(())
}
