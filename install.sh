#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
#
# Build prolink and install it where a passwordless sudo rule can point at it.
#
# Serving needs UDP/111, which is privileged. On Linux you can avoid root
# entirely with `sysctl net.ipv4.ip_unprivileged_port_start=111`; macOS has no
# equivalent, so it needs sudo. See docs/TESTING.md for the sudoers rule that
# stops it asking for a password every time.
set -eu

cargo build --release
sudo install -o root -g wheel -m 755 target/release/prolink /usr/local/bin/prolink
echo "installed $(/usr/local/bin/prolink --version) to /usr/local/bin/prolink"
