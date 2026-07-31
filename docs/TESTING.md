<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Testing against real hardware

## Serving needs a privileged port

A CDJ finds a peer's file server by asking its portmapper on **UDP/111**, and
with nothing there it retries `GETPORT` once a second for ever rather than
falling back to the well-known ports (F46). So `prolink serve` must be able to
bind 111.

- **Linux**: no root needed. `sudo sysctl -w net.ipv4.ip_unprivileged_port_start=111`,
  or put `net.ipv4.ip_unprivileged_port_start=111` in `/etc/sysctl.d/` to make it
  survive a reboot. This is the right answer on a deployed unit.
- **macOS**: there is no equivalent. It needs `sudo`.

Everything else — `devices`, `status`, `rpcinfo`, `pull-db`, `browse`, `tracks`,
`pcap` — runs as an ordinary user.

## Not retyping your password on macOS

Install to a root-owned path and add a `sudo` rule scoped to exactly that path:

```sh
./install.sh                       # builds, and installs to /usr/local/bin/prolink

sudo tee /etc/sudoers.d/prolink >/dev/null <<'RULE'
your-username ALL=(root) NOPASSWD: /usr/local/bin/prolink
RULE
sudo chmod 0440 /etc/sudoers.d/prolink
sudo visudo -cf /etc/sudoers.d/prolink   # verify before trusting it
```

Then `sudo prolink serve --usb /Volumes/MY_STICK` runs without a prompt. Re-run
`./install.sh` after each rebuild — the rule points at the installed copy, not
at `target/release`.

**What this does and does not give away.** It does not grant you any privilege
you did not already have: you can run `sudo` anyway, so this only removes the
prompt. What it *does* remove is the pause in which you would notice something
running `prolink` as root on your behalf. Because the rule names a path only
root can write, a program running as you cannot replace the binary — which is
why `install.sh` installs to `/usr/local/bin` as root rather than pointing the
rule at your build directory. **Do not point it at `target/release/prolink`**:
anything you run could then overwrite that file and be executed as root without
a password.

Remove it with `sudo rm /etc/sudoers.d/prolink`.

If your Mac is managed and `/etc/sudoers.d` is overwritten by MDM, there is no
good way around it; use `sudo -v` to refresh the timestamp before a session, so
one password lasts five minutes of testing.

### Why not make the binary setuid root

`sudo chmod u+s` would also work and is worse. A setuid binary is root for
*anyone* who can run it, not just for you; it stays root across reboots with no
audit trail; and if you ever `cargo build` over an installed copy you own, you
have handed root to anything that can write that file. The sudoers rule is
narrower, revocable in one command, and logged.

## Capturing when something misbehaves

```sh
sudo tcpdump -i en9 -w ~/prolink-issue.pcap -s 0 \
  'udp port 50000 or udp port 50001 or udp port 50002 or udp port 111 \
   or udp port 2049 or udp port 48276 or tcp port 1051 or tcp port 12523'
```

`prolink pcap ~/prolink-issue.pcap` gives a first read. A capture plus the `-v`
log is what every fix in this repository was made from — three separate bugs
were found by diffing our replies against a real deck's for the same requests,
and none of them produced an error message anywhere.

### Capturing deck-to-deck

**Which interface depends on who is serving.** A bridge member carries the
traffic the bridge *forwards* between the decks; the bridge interface carries
the host's own. Tapping one and not the other has cost a session in each
direction, so `capture-deck-to-deck.sh` now taps all of them.

**Tap a bridge *member* for deck-to-deck.** A BSD bridge floods
broadcast to every member and to the host, but forwards learned unicast
**directly from one member port to the other**. A tap on `bridge1` therefore
sees the host's own traffic and the broadcast — UDP 50000 keep-alives and UDP
50001 beats — and none of the unicast that carries everything worth having:
status on 50002, dbserver on 1051, NFS on 2049.

The resulting capture looks healthy and is worthless. This has now cost four
sessions, two of them after this file already warned about it.

```sh
sudo tools/capture-deck-to-deck.sh ~/prolink-key
```

taps every member of the bridge to its own file and, on Ctrl-C, prints how much
dbserver, status and NFS traffic each one caught. A frame between the decks
crosses both members, so one file is normally enough; both are taken because
which direction lands on which member is a property of the bridge and not worth
betting a session on.

**Verify with unicast, never with broadcast.** Seeing keep-alives proves only
that the cable is live. The check that matters is `tcp port 1051`.
