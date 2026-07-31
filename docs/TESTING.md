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

**Capture on the machine running `prolink`, not on a bridge interface.** A
bridge floods broadcast but forwards learned unicast directly between members,
so the capture looks healthy while missing exactly the traffic of interest — two
findings in the research record were contaminated that way. Verify a tap with
unicast, not broadcast.
