# The emulated rig of E10-E19, sourced by each session's commands.
#
# Two CDJ-2000NXS on Pioneer's firmware 1.44 (`pi-qemu cdj`), on one link:
#   A, player 2, holds the session's stick in its USB slot;
#   B, player 1, has no media and reaches A's stick over LINK.
# B boots first, so the NXS's AUTO numbering gives it 1 and A 2. Run a
# session's cmd.txt from its own directory: the capture goes to run.pcap.
PQ=${PQ:-pi-qemu}                  # TriMixxx's pi-qemu: cdj verbs, link capture
NET=emu-captures-usb-net
A=emu-captures-usb-a
B=emu-captures-usb-b
STICK=${STICK:?the stick image, PLAINMP3 from plain-stick/make_stick.py OUT --image}
SHOTS=${SHOTS:-/tmp}               # screenshots, never committed

# The TEMPO slider at 0 %. An emulated NXS boots with the slider's position
# and centre both at 0, so it reports a pitch of -100 % until it moves: set
# the centre (field 3) first, then the position (field 2), through the
# emulator's control channel.
centre() {
    python3 - "$HOME/.pi-qemu/cdj/$1/run/run.json" <<'PY'
import json, socket, sys
port = json.load(open(sys.argv[1]))["endpoints"]["panel_port"]
with socket.create_connection(("127.0.0.1", port), timeout=5) as s:
    f = s.makefile("rw")
    assert f.readline().startswith("ok")          # "ok cdj2000-input"
    for line in ("analog 3 32768", "analog 2 32768", "state"):
        f.write(line + "\n"); f.flush()
        answer = f.readline()
        assert answer.startswith("ok"), (line, answer)
    print(sys.argv[1].split("/")[-3], "slider:",
          " ".join(w for w in answer.split() if w.startswith(("a2=", "a3="))))
PY
}
key()  { "$PQ" cdj press "$1" "$2" >/dev/null; sleep "${3:-1.5}"; }   # a panel key, then a pause
shot() { "$PQ" cdj shot "$1" "$SHOTS/$2.png" >/dev/null; }
# The browse encoder, N rows (signed), one detent at a time and DELAY seconds
# apart (default 0.4): several detents sent at once are not all counted.
move() {
    n=$2; d=1; [ "$n" -lt 0 ] && { n=$((-n)); d=-1; }
    while [ "$n" -gt 0 ]; do
        "$PQ" cdj rotary "$1" "$d" >/dev/null; sleep "${3:-0.4}"; n=$((n - 1))
    done
}

# Start the rig: B, then the capture, then A with the stick.
#   rig_up OPTS_FOR_B    e.g. --dsp-model, or nothing for Pioneer's DSP code
rig_up() {
    "$PQ" cdj up "$B" --link "$NET" "$@"
    centre "$B"
    "$PQ" link capture "$NET" run.pcap & capture=$!
    sleep 2
    "$PQ" cdj up "$A" --link "$NET" --dsp-model --usb "$STICK"
    centre "$A"
}
rig_down() {
    kill "$capture"
    "$PQ" cdj rm "$A"
    "$PQ" cdj rm "$B"
}
