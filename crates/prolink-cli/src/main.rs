// Copyright (C) 2026 the prolink authors.
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, version 3.
//
// SPDX-License-Identifier: GPL-3.0-only

//! Command line tools for the Pioneer Pro DJ Link protocol.
//!
//! Every command except `announce`, `serve` and `status --announce` is
//! **passive**: it transmits nothing on any Pro DJ Link port, so it can be run
//! beside a live rig without contending for a device number.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

use std::io::IsTerminal;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use prolink::consume::{DbClient, NfsClient};
use prolink::serve::{Medium, ServedSlot, VirtualPlayer, VirtualPlayerConfig};
use prolink::virtual_cdj::{Numbering, VirtualCdjConfig};
use prolink::{
    BeatInBar, BrowsableDeviceNumber, DeviceName, DeviceNumber, Discovery, Interface, Monitor,
    PeerSlot, PlayerState, Slot, VirtualCdj, discovery::SCAN_DURATION,
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

    /// Watch what the players are doing: tempo, beat and bar phase, and master.
    ///
    /// Passive by default, which is enough for tempo and phase — beat packets
    /// are broadcast. The loaded track, the play state and the tempo master
    /// are published only in status packets, which a player unicasts to peers
    /// that have announced themselves, so those need `--announce`.
    Status {
        /// Keep watching and repaint the table as it changes.
        #[arg(long, short)]
        watch: bool,

        /// How long to listen for before reporting, in seconds.
        #[arg(long, default_value_t = SCAN_DURATION.as_secs_f32())]
        seconds: f32,

        /// Announce as a virtual CDJ, so players unicast their status to us.
        ///
        /// **This transmits.** It takes a device number outside the 1–6 player
        /// range, so it cannot collide with hardware, and it does not emit
        /// status of its own — announcing is all it takes to be told.
        #[arg(long)]
        announce: bool,
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

    /// List what media the players have in their slots, and what is on it.
    ///
    /// **This transmits.** Slot occupancy is published only in status packets,
    /// which a player unicasts to peers that have announced themselves, and the
    /// volume name and the track and playlist counts have to be asked for with
    /// a media query. So this announces as a virtual CDJ and then asks.
    Media {
        /// How long to wait for the answers, in seconds.
        ///
        /// A deck answers in about a millisecond, so this is a bound on a lost
        /// datagram rather than on a slow player.
        #[arg(long, default_value_t = 0.5)]
        seconds: f32,

        /// Claim a browsable number in 1–4 rather than announcing outside the
        /// player range.
        ///
        /// **Untested either way.** Every captured media query came from a real
        /// player number, so whether a deck answers one from device 7 is not
        /// something the corpus settles. This is the fallback if it does not.
        #[arg(long)]
        claim: bool,

        /// The number to try first when claiming.
        #[arg(long, value_parser = browsable_number)]
        number: Option<BrowsableDeviceNumber>,
    },

    /// Ask a player what ONC RPC services it runs. Passive.
    Rpcinfo {
        /// The player's address, from `prolink devices`.
        address: Ipv4Addr,
    },

    /// Pull a player's rekordbox database over NFS. Passive.
    PullDb {
        /// The player's address.
        address: Ipv4Addr,

        /// Which of its slots to read.
        #[arg(long, default_value = "usb", value_parser = slot)]
        slot: Slot,

        /// Where to write it. Defaults to `export.pdb` in the current directory.
        #[arg(long, short)]
        output: Option<PathBuf>,
    },

    /// Browse a player's library the way its LINK button does.
    ///
    /// **This transmits.** dbserver needs a device number in 1–4, so this
    /// announces as a virtual CDJ first and contends for one.
    Browse {
        /// The player's address.
        address: Ipv4Addr,

        /// Which of its slots to browse.
        #[arg(long, default_value = "usb", value_parser = slot)]
        slot: Slot,

        /// List every track instead of the root menu.
        #[arg(long)]
        tracks: bool,

        /// Search as the deck does, one request per keystroke.
        #[arg(long)]
        search: Option<String>,

        /// Show one track's metadata and the path a load would read.
        #[arg(long)]
        track: Option<u32>,

        /// The device number to try first.
        #[arg(long, value_parser = browsable_number)]
        number: Option<BrowsableDeviceNumber>,
    },

    /// Serve local rekordbox media to real CDJs, as a virtual player.
    ///
    /// **This transmits, and needs UDP/111.** That port is privileged: run as
    /// root, or on Linux set `net.ipv4.ip_unprivileged_port_start=111`. Without
    /// it a deck retries `GETPORT` once a second for ever and never finds us.
    Serve {
        /// The medium to present in the USB slot.
        #[arg(long, required_unless_present = "sd")]
        usb: Option<PathBuf>,

        /// The medium to present in the SD slot. A second USB stick shown as an
        /// SD card is exactly what a CDJ expects to see.
        #[arg(long)]
        sd: Option<PathBuf>,

        /// The device number to try first. Must be 1–4 to be browsable.
        #[arg(long, value_parser = browsable_number)]
        number: Option<BrowsableDeviceNumber>,

        /// The name to announce.
        #[arg(long, default_value = DeviceName::CDJ_2000_NEXUS)]
        name: String,

        /// Keep-alive byte 0x35. `0x64` is required to coexist with CDJ-3000s.
        #[arg(long, default_value_t = 0x00, value_parser = maybe_hex)]
        generation: u8,

        /// Put the portmapper somewhere else. Useful only for experiments: at
        /// anything but 111 no real player will ever find us.
        #[arg(long, default_value_t = 111)]
        portmap_port: u16,
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

fn slot(text: &str) -> Result<Slot, String> {
    Slot::parse(text).ok_or_else(|| format!("{text} is not a slot; try usb, sd, cd or rekordbox"))
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
        Command::Status {
            watch,
            seconds,
            announce,
        } => {
            let interface = choose_interface(cli.interface.as_deref())?;
            watch_status(interface, watch, Duration::from_secs_f32(seconds), announce).await
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
        Command::Media {
            seconds,
            claim,
            number,
        } => {
            let interface = choose_interface(cli.interface.as_deref())?;
            list_media(interface, Duration::from_secs_f32(seconds), claim, number).await
        }
        Command::Rpcinfo { address } => {
            rpcinfo(
                address,
                choose_interface(cli.interface.as_deref()).ok().as_ref(),
            )
            .await
        }
        Command::PullDb {
            address,
            slot,
            output,
        } => {
            let interface = choose_interface(cli.interface.as_deref()).ok();
            pull_db(address, slot, output.as_deref(), interface.as_ref()).await
        }
        Command::Browse {
            address,
            slot,
            tracks,
            search,
            track,
            number,
        } => {
            let interface = choose_interface(cli.interface.as_deref())?;
            browse(
                interface,
                address,
                slot,
                BrowseWhat {
                    tracks,
                    search,
                    track,
                },
                number,
            )
            .await
        }
        Command::Serve {
            usb,
            sd,
            number,
            name,
            generation,
            portmap_port,
        } => {
            let interface = choose_interface(cli.interface.as_deref())?;
            serve(
                interface,
                usb.as_deref(),
                sd.as_deref(),
                number,
                &name,
                generation,
                portmap_port,
            )
            .await
        }
        Command::Tracks {
            file,
            playlists,
            search,
        } => list_tracks(&file, playlists, search.as_deref()),
        Command::Pcap { file } => summarise_capture(&file),
    }
}

/// Ask a player's portmapper what it runs. The go/no-go check for the whole
/// file-access path, and entirely passive.
async fn rpcinfo(address: Ipv4Addr, interface: Option<&Interface>) -> Result<(), BoxedError> {
    let mut client = NfsClient::connect(address, interface).await?;
    let ports = client.ports();
    println!("mountd on {}, nfsd on {}", ports.mount, ports.nfs);
    println!();
    println!(
        "{:<10} {:<8} {:<6} {:<6}",
        "PROGRAM", "VERSION", "PROTO", "PORT"
    );
    for mapping in client.dump().await? {
        println!(
            "{:<10} {:<8} {:<6?} {:<6}  {:?}",
            mapping.program.0, mapping.version, mapping.protocol, mapping.port, mapping.program,
        );
    }
    Ok(())
}

/// Pull a player's rekordbox database. Passive: a CDJ exports to the whole
/// link-local subnet, so a host that has never announced is already permitted.
async fn pull_db(
    address: Ipv4Addr,
    slot: Slot,
    output: Option<&std::path::Path>,
    interface: Option<&Interface>,
) -> Result<(), BoxedError> {
    let mut client = NfsClient::connect(address, interface).await?;
    let mounted = client.mount_slot(slot).await?;
    eprintln!("mounted {} on {address}", mounted.export());

    let file = client
        .open(&mounted, prolink::consume::nfs::EXPORT_PDB)
        .await?;
    eprintln!("export.pdb is {} bytes", file.size());
    let bytes = client.read_file(&file).await?;

    let default = PathBuf::from("export.pdb");
    let path = output.unwrap_or(&default);
    std::fs::write(path, &bytes)?;

    let stats = client.stats();
    eprintln!(
        "wrote {} to {} ({} reads, {} retries)",
        bytes.len(),
        path.display(),
        stats.reads,
        stats.retries,
    );

    let library = prolink_rekordbox::Library::parse(&bytes)?;
    let summary = library.summary();
    println!(
        "{} tracks, {} artists, {} albums, {} playlists",
        summary.tracks, summary.artists, summary.albums, summary.playlists
    );
    let _ = client.unmount(&mounted).await;
    Ok(())
}

/// What a browse should show.
struct BrowseWhat {
    tracks: bool,
    search: Option<String>,
    track: Option<u32>,
}

/// Browse a player's library. Needs a device number in 1–4, so it announces.
async fn browse(
    interface: Interface,
    address: Ipv4Addr,
    slot: Slot,
    what: BrowseWhat,
    preferred: Option<BrowsableDeviceNumber>,
) -> Result<(), BoxedError> {
    let discovery = Discovery::start(interface).await?;
    eprintln!("watching before claiming a device number…");
    tokio::time::sleep(SCAN_DURATION).await;

    let cdj = VirtualCdj::observe(
        &discovery,
        VirtualCdjConfig {
            numbering: Numbering::Claim { preferred },
            ..VirtualCdjConfig::default()
        },
    )
    .await?;
    let device = cdj
        .browsable_number()
        .ok_or("no browsable device number is free")?;
    eprintln!("announced as device {device}");

    let mut client = DbClient::connect(address, device).await?;
    eprintln!(
        "connected to device {} on port {}",
        client.server(),
        client.port()
    );

    if let Some(id) = what.track {
        let metadata = client.metadata(slot, id).await?;
        println!("title    {}", metadata.title);
        println!("artist   {}", metadata.artist);
        println!("album    {}", metadata.album);
        println!("genre    {}", metadata.genre);
        println!("key      {}", metadata.key);
        println!("tempo    {:.2}", metadata.tempo());
        println!("duration {}s", metadata.duration().as_secs());
        let info = client.track_info(slot, id).await?;
        println!("path     {}", info.path);
        println!("size     {} bytes", info.size);
        return Ok(());
    }

    let items = if let Some(term) = what.search.as_deref() {
        client
            .search(slot, term, prolink_proto::dbserver::SortOrder::DEFAULT)
            .await?
    } else if what.tracks {
        client
            .tracks(slot, prolink_proto::dbserver::SortOrder::DEFAULT)
            .await?
    } else {
        client.root_menu(slot).await?
    };

    for item in &items {
        let second = if item.label2.is_empty() {
            String::new()
        } else {
            format!("  —  {}", item.label2)
        };
        println!("{:>10}  {}{second}", item.id, item.label1);
    }
    eprintln!("{} items", items.len());
    let _ = client.close().await;
    Ok(())
}

/// Serve local media to real CDJs.
async fn serve(
    interface: Interface,
    usb: Option<&std::path::Path>,
    sd: Option<&std::path::Path>,
    number: Option<BrowsableDeviceNumber>,
    name: &str,
    generation: u8,
    portmap_port: u16,
) -> Result<(), BoxedError> {
    let mut media = Vec::new();
    for (path, slot) in [(usb, ServedSlot::USB), (sd, ServedSlot::SD)] {
        let Some(path) = path else { continue };
        let medium = Medium::from_volume(path, slot)?;
        eprintln!(
            "{}: {} — {} tracks, {} playlists",
            slot.export_path(),
            path.display(),
            medium.library().tracks.len(),
            medium.library().summary().playlists,
        );
        media.push(std::sync::Arc::new(medium));
    }

    let config = VirtualPlayerConfig {
        preferred_number: number,
        name: DeviceName::new(name),
        generation,
        portmap_port,
        ..VirtualPlayerConfig::new(interface)
    };
    let server = VirtualPlayer::start(config, media).await?;

    let ports = server.nfs_ports();
    println!("announcing as device {}", server.device_number());
    println!(
        "portmap {} · mountd {} · nfsd {} · dbserver {}",
        ports.portmap,
        ports.mount,
        ports.nfs,
        server.dbserver_port(),
    );
    if server.is_discoverable() {
        println!("press LINK on a CDJ; we should be listed as a source");
    } else {
        println!(
            "WARNING: the portmapper is not on 111, so no real player will find us. \
             Run as root, or on Linux set net.ipv4.ip_unprivileged_port_start=111."
        );
    }
    println!("ctrl-c to stop");

    tokio::signal::ctrl_c().await?;

    // Not a `drop`: a deck reading from us holds a mount, and the eject is what
    // tells it to let go. Two to three seconds when something is actually
    // reading and about half of one when nothing is, so say what is happening —
    // and take a second ctrl-c as "I know, go now".
    println!("ejecting our media so the players reading it can unmount cleanly...");
    tokio::select! {
        () = server.shutdown() => println!("media ejected; stopped"),
        result = tokio::signal::ctrl_c() => {
            result?;
            eprintln!("stopping now; a player may be left holding a stale mount");
        }
    }
    Ok(())
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

/// What a passive run cannot answer, said once rather than shown as blanks.
const NO_STATUS_NOTE: &str = "\
note: without --announce only UDP 50001 is available. It is broadcast, so tempo
      and beat phase are visible — but the loaded track, the play state and the
      tempo master are published only in status packets, which a player unicasts
      to peers that have announced themselves. Those columns read '?', meaning
      unknown rather than absent.";

/// Watch tempo, phase and mastership.
async fn watch_status(
    interface: Interface,
    watch: bool,
    duration: Duration,
    announce: bool,
) -> Result<(), BoxedError> {
    // Held for as long as the monitor runs: dropping either stops announcing,
    // and peers stop unicasting status a few seconds later.
    let mut announcing: Option<(Discovery, VirtualCdj)> = None;

    let monitor = if announce {
        eprintln!("listening on {interface} (announcing, which transmits)");
        let discovery = Discovery::start(interface.clone()).await?;
        // Watch first, so the virtual CDJ knows whether it was alone.
        tokio::time::sleep(SCAN_DURATION).await;
        let config = VirtualCdjConfig {
            // Announcing is all it takes to be *told* status; emitting our own
            // would take port 50002 away from the monitor for nothing, since
            // two sockets in a SO_REUSEPORT group do not both get a unicast
            // datagram.
            emit_status: false,
            ..VirtualCdjConfig::default()
        };
        let cdj = VirtualCdj::observe(&discovery, config).await?;
        eprintln!(
            "announcing as device {} — players will unicast their status to us",
            cdj.number()
        );
        let monitor = Monitor::with_status(interface, &cdj).await?;
        announcing = Some((discovery, cdj));
        monitor
    } else {
        eprintln!("listening on {interface} (transmitting nothing)");
        eprintln!("{NO_STATUS_NOTE}");
        Monitor::start(interface).await?
    };
    let _held = &announcing;

    if !watch {
        tokio::time::sleep(duration).await;
        for line in render_players(&monitor) {
            println!("{line}");
        }
        return Ok(());
    }

    let repaint = std::io::stdout().is_terminal();
    let mut ticker = tokio::time::interval(Duration::from_millis(200));
    let mut painted = 0usize;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let lines = render_players(&monitor);
                if repaint && painted > 0 {
                    // Back over the last table and clear each line, so a
                    // shrinking table does not leave a ghost behind.
                    print!("\x1b[{painted}F");
                }
                for line in &lines {
                    if repaint {
                        print!("\x1b[2K");
                    }
                    println!("{line}");
                }
                painted = lines.len();
            }
            result = tokio::signal::ctrl_c() => {
                result?;
                return Ok(());
            }
        }
    }
}

/// One line per player, plus a header.
fn render_players(monitor: &Monitor) -> Vec<String> {
    let known = monitor.watches_status();
    let mut lines = vec![format!(
        "{:>3}  {:<20}  {:>7}  {:<6}  {:>5}  {:<4}  {:<8}  {:<10}  {}",
        "NUM", "NAME", "TEMPO", "BAR", "PHASE", "SYNC", "MASTER", "STATE", "LOADED"
    )];
    let players = monitor.players();
    if players.is_empty() {
        lines.push(String::from(
            "  (nothing yet — a CDJ sends beat packets only while playing a rekordbox track)",
        ));
        return lines;
    }
    for player in players {
        lines.push(format!(
            "{:>3}  {:<20}  {:>7}  {:<6}  {:>5}  {:<4}  {:<8}  {:<10}  {}",
            // `.get()`, because `DeviceNumber`'s `Display` writes through to
            // the inner number and so ignores the field width.
            player.device.get(),
            truncate(&player.name.as_str(), 20),
            tempo_cell(&player),
            bar_cell(&player),
            phase_cell(&player),
            sync_cell(&player, known),
            master_cell(&player, known),
            state_cell(&player, known),
            loaded_cell(&player, known),
        ));
    }
    lines
}

/// The tempo actually playing — the track's BPM with the pitch fader applied.
fn tempo_cell(player: &PlayerState) -> String {
    player
        .effective_bpm()
        .map_or_else(|| String::from("--"), |bpm| format!("{bpm:.2}"))
}

/// Four cells, one per beat of the bar, with the current one marked.
///
/// Empty when the player is not beating, and also when it is beating without a
/// bar to be in — a deck on a track rekordbox has not analysed sends beat
/// packets with `0` at byte `0x5c`, which is not beat zero.
fn bar_cell(player: &PlayerState) -> String {
    let current = player
        .beat_in_bar()
        .filter(|_| player.is_beating())
        .map(BeatInBar::get);
    let mut cells = String::from("[");
    for beat in 1..=BeatInBar::PER_BAR {
        cells.push(if current == Some(beat) { '#' } else { '.' });
    }
    cells.push(']');
    cells
}

/// Position within the current beat, `0.00` on the beat.
fn phase_cell(player: &PlayerState) -> String {
    player
        .beat_phase()
        .map_or_else(|| String::from("--"), |phase| format!("{phase:.2}"))
}

fn master_cell(player: &PlayerState, known: bool) -> String {
    if !known {
        // Not "no": a listener that has not announced cannot tell "not master"
        // from "cannot know", and must not report the first when it means the
        // second.
        return String::from("?");
    }
    // A handoff in progress is shown as the arrow rather than as plain
    // "master", because for those one or two packets the *other* deck is
    // reporting itself master too and a bare label would look like a fault.
    if let Some(successor) = player.yielding_to() {
        return format!("→{successor}");
    }
    String::from(match player.is_tempo_master() {
        Some(true) => "master",
        Some(false) => "-",
        None => "?",
    })
}

/// Whether SYNC is lit. Published only in status packets, like master.
fn sync_cell(player: &PlayerState, known: bool) -> &'static str {
    if !known {
        return "?";
    }
    match player.is_synced() {
        Some(true) => "sync",
        Some(false) => "-",
        None => "?",
    }
}

fn state_cell(player: &PlayerState, known: bool) -> String {
    match player.play_state() {
        Some(state) => state.to_string(),
        None if known => String::from("-"),
        None => String::from("?"),
    }
}

fn loaded_cell(player: &PlayerState, known: bool) -> String {
    match player.track() {
        Some(track) => format!(
            "player {} {} #{} ({})",
            track.source_player, track.slot, track.id, track.kind
        ),
        None if known => String::from("nothing"),
        None => String::from("? (needs --announce)"),
    }
}

/// How long to let peers notice us before asking about their slots.
///
/// Occupancy comes from status packets, which a player only unicasts to peers
/// that have announced themselves (F21). Announcing and asking in the same
/// breath gets the counts but leaves every slot's state unknown, so this is the
/// gap in between: two keep-alive periods, which is the interval a deck adds a
/// peer on.
const STATUS_SETTLE: Duration = Duration::from_secs(4);

/// List what the players have in their slots.
async fn list_media(
    interface: Interface,
    wait: Duration,
    claim: bool,
    preferred: Option<BrowsableDeviceNumber>,
) -> Result<(), BoxedError> {
    let numbering = if claim {
        Numbering::Claim { preferred }
    } else {
        if preferred.is_some() {
            eprintln!("note: --number only applies with --claim; announcing as an observer");
        }
        Numbering::default()
    };

    eprintln!("listening on {interface} (announcing, which transmits)");
    let discovery = Discovery::start(interface).await?;
    tokio::time::sleep(SCAN_DURATION).await;

    // Status emission stays on, and that is the load-bearing part: it is what
    // takes UDP 50002, which is the port a media response comes back to.
    let config = VirtualCdjConfig {
        numbering,
        ..VirtualCdjConfig::default()
    };
    let cdj = VirtualCdj::observe(&discovery, config).await?;
    eprintln!("asking as device {}", cdj.number());

    // A moment for peers to notice us and start unicasting status, which is
    // where occupancy comes from. Without it every slot reads unknown.
    tokio::time::sleep(STATUS_SETTLE).await;
    let slots = cdj.survey_media(&discovery, wait).await?;

    let devices = discovery.online();
    let mut listed = 0usize;
    for device in &devices {
        if device.number == cdj.number() {
            continue;
        }
        let mine: Vec<_> = slots
            .iter()
            .filter(|entry| entry.device == device.number)
            .collect();
        if mine.is_empty() {
            continue;
        }
        listed += 1;
        println!(
            "device {} — {} at {}",
            device.number, device.name, device.ip
        );
        for entry in mine {
            print_slot(entry);
        }
    }

    if listed == 0 {
        println!("no player answered; none is online, or none would talk to us");
    }
    Ok(())
}

/// One line per slot: what is in it, and what is on that.
fn print_slot(slot: &PeerSlot) {
    let name = match slot.slot {
        Slot::USB => "USB",
        Slot::SD => "SD ",
        other => {
            let _ = other;
            "?  "
        }
    };
    if !slot.has_media() {
        // The state, not just "empty": a slot mid-eject is neither.
        println!("  {name}  {:?}", slot.state);
        return;
    }
    let Some(description) = slot.description.as_ref() else {
        println!("  {name}  loaded, but it did not describe itself");
        return;
    };
    // An unlabelled stick reports no name while carrying a full library, so
    // say so rather than printing nothing.
    let volume = if description.volume_name.is_empty() {
        "(unlabelled)"
    } else {
        &description.volume_name
    };
    print!(
        "  {name}  {volume} — {} tracks, {} playlists",
        description.track_count, description.playlist_count,
    );
    if let (Some(total), Some(free)) = (description.total_bytes, description.free_bytes) {
        print!(" — {} free of {}", gibibytes(free), gibibytes(total));
    }
    if !description.created.is_empty() {
        print!(" — created {}", description.created);
    }
    println!();
}

/// Bytes as a human-readable size. NFSv2 is 32-bit, and so is this field.
fn gibibytes(bytes: u32) -> String {
    let gib = f64::from(bytes) / 1024.0 / 1024.0 / 1024.0;
    format!("{gib:.1} GiB")
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
