#!/usr/bin/env bash
# Capture deck-to-deck Pro DJ Link traffic through a bridging Mac.
#
#   sudo tools/capture-deck-to-deck.sh ~/prolink-key
#
# Writes ~/prolink-key-<member>.pcap for each bridge member, and reports which
# of them actually caught the unicast.
#
# ---------------------------------------------------------------------------
# Why this taps the bridge *members* and not the bridge.
#
# A BSD bridge floods broadcast to every member and to the host, but forwards
# **learned unicast directly from one member port to the other**. A bpf tap on
# `bridge1` therefore sees the host's own traffic and the broadcast — UDP 50000
# keep-alives and UDP 50001 beats — and none of the unicast that carries
# everything interesting: status on 50002, dbserver on 1051, NFS on 2049.
#
# The capture looks healthy and is worthless. That has cost this project three
# sessions, and `docs/TESTING.md` has warned about it the whole time.
#
# A frame between the decks enters one member and is transmitted out the other,
# so it crosses both and a *single* member sees both directions. Both are
# tapped anyway, to separate files, because which one a given direction appears
# on is a property of the bridge implementation and not worth betting a session
# on. Separate files rather than merged, so the same frame seen on two
# interfaces cannot be counted twice.
# ---------------------------------------------------------------------------
set -euo pipefail

PREFIX="${1:-$HOME/prolink-capture}"
BRIDGE="${BRIDGE:-bridge1}"

die() { printf '%s\n' "$*" >&2; exit 1; }

[ "$(id -u)" = 0 ] || die "needs root for packet capture: sudo $0 $PREFIX"

members=$(ifconfig "$BRIDGE" 2>/dev/null | awk '/member:/ { print $2 }')
[ -n "$members" ] || die "$BRIDGE has no members — is the bridge up?"

# The bridge itself as well as its members, because the two carry *different*
# traffic and which one you need depends on who is serving:
#
#   deck  <-> deck : forwarded member-to-member, seen on a MEMBER, not on the bridge
#   Mac   <-> deck : the host's own traffic, seen on the BRIDGE, not on a member
#
# Tapping only one of them has now cost a session in each direction.
members="$members $BRIDGE"

# No port filter. The whole point of these captures is fields we cannot name
# yet, so excluding a protocol nobody has thought of is exactly the mistake to
# avoid. Only the Mac's own chatter is dropped, and only if it has an address.
mac_ip=$(ifconfig "$BRIDGE" 2>/dev/null | awk '/inet /{ print $2; exit }')
filter=""
[ -n "$mac_ip" ] && filter="not host $mac_ip"

pids=()
files=()
for member in $members; do
    out="$PREFIX-$member.pcap"
    files+=("$out")
    # -s 0 whole frames, -B 8192 an 8 MB buffer so an NFS burst does not drop.
    tcpdump -i "$member" -s 0 -B 8192 -n -w "$out" $filter 2>/dev/null &
    pids+=($!)
    printf 'tapping %-5s -> %s\n' "$member" "$out"
done

reported=0
report() {
    [ "$reported" = 1 ] && return
    reported=1
    any_dbserver=0
    for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
    wait "${pids[@]}" 2>/dev/null || true
    printf '\n'
    for out in "${files[@]}"; do
        chown "${SUDO_USER:-root}" "$out" 2>/dev/null || true
        db=$(tcpdump -r "$out" -nn 'tcp port 1051' 2>/dev/null | wc -l | tr -d ' ')
        st=$(tcpdump -r "$out" -nn 'udp port 50002' 2>/dev/null | wc -l | tr -d ' ')
        nfs=$(tcpdump -r "$out" -nn 'udp port 2049' 2>/dev/null | wc -l | tr -d ' ')
        printf '%-40s dbserver %-6s status %-6s nfs %s\n' "$(basename "$out")" "$db" "$st" "$nfs"
        if [ "$db" -gt 0 ]; then
            printf '   ^ TCP 1051 present — this one is good.\n'
            any_dbserver=1
        fi
    done
    if [ "$any_dbserver" = 0 ]; then
        printf '\nNO dbserver traffic on either tap. Do not trust this capture.\n' >&2
    fi
}
trap report EXIT INT TERM

printf '\ncapturing — browse a menu on the deck, then Ctrl-C\n'
wait "${pids[@]}" 2>/dev/null || true
