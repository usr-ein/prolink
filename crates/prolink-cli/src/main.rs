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
    }
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
