#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["mutagen==1.47.0", "pillow==11.3.0"]
# ///
# SPDX-License-Identifier: GPL-3.0-only
"""Make PLAINMP3, the stick of the E10-E19 emu-captures: music files in
folders, and no rekordbox export.

    uv run make_stick.py OUT            # the files, in OUT/files
    uv run make_stick.py OUT --image    # and OUT/plainmp3.img, a 1 GiB FAT32 stick

Every file is generated, so every tag is one we chose: the audio is a click
track at a known tempo (an accented click every fourth beat), each file at a
pitch of its own. Where a CDJ could take a value from two places, the two
differ on purpose -- a title tag that is not the file name, a BPM tag that is
not the tempo of the clicks -- so what it shows says where it read it.

Needs macOS for `afconvert` (the AAC, AIFF and FLAC files) and LAME 3.100 for
the MP3s (`brew install lame`). `MANIFEST.tsv` beside this script is the
manifest of the files as made on 2026-10-10 (LAME 3.100, Pillow 11.3.0,
mutagen 1.47.0); a remake with the same versions gives the same bytes but
for `Formats/AAC.m4a`, whose MP4 header holds the time `afconvert` wrote it.

The image is written by the CDJ emulator's own builder,
`cdj2000-emulator/tools/cdj_main/make_sd_image.py` (`--emulator`; found by
itself in a TriMixxx checkout): an MBR, one FAT32 partition at LBA 2048,
4 KiB clusters, long names for every name that is not plain 8.3. Then five
touches make it look like a stick a DJ formatted and filled on a computer:
- the volume label PLAINMP3, in the boot sector and as a root entry
- short names with no leading dot, as Windows and macOS make them
  (`._01 Full v23.mp3` is `_01FUL~1.MP3`, not `._01FU~1.MP3`)
- an empty file with no cluster, where the builder gives it one
- `System Volume Information` hidden and system, as Windows leaves it
- `Order/` written in the reverse of its name order (see LISTING.txt)
"""

from __future__ import annotations

import argparse
import hashlib
import io
import math
import random
import shutil
import struct
import subprocess
import sys
import unicodedata
import wave
from pathlib import Path

from mutagen.flac import FLAC
from mutagen.id3 import (
    APIC, COMM, ID3, POPM, TALB, TBPM, TCOM, TCON, TDRC, TIT2, TKEY, TPE1,
    TPE2, TPE4, TPUB, TRCK, TYER,
)
from mutagen.mp4 import MP4, MP4Cover
from PIL import Image, ImageDraw, ImageFont

RATE = 44100
LABEL = "PLAINMP3"
HERE = Path(__file__).resolve().parent


# ---- audio ------------------------------------------------------------------

def burst(pitch: float, accent: bool) -> bytes:
    """One click: 25 ms of a decaying sine, as stereo 16-bit frames."""
    amp = 0.7 if accent else 0.45
    freq = pitch * (1.5 if accent else 1.0)
    out = bytearray()
    for i in range(int(0.025 * RATE)):
        v = int(32767 * amp * math.exp(-i / (0.006 * RATE))
                * math.sin(2 * math.pi * freq * i / RATE))
        out += struct.pack("<hh", v, v)
    return bytes(out)


def clicks(bpm: float, seconds: float, pitch: float) -> bytes:
    """A click on every beat at BPM, the first of every four accented.

    Each onset is placed at its own rounded frame, so the tempo is exact on
    average whatever the BPM, not rounded to a whole beat length.
    """
    frames = int(seconds * RATE)
    out = bytearray(frames * 4)
    loud, soft = burst(pitch, True), burst(pitch, False)
    beat = 0
    while (start := round(beat * RATE * 60 / bpm)) < frames:
        click = loud if beat % 4 == 0 else soft
        piece = click[:(frames - start) * 4]
        out[start * 4:start * 4 + len(piece)] = piece
        beat += 1
    return bytes(out)


def write_wav(path: Path, pcm: bytes) -> None:
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm)


def mp3(path: Path, bpm: float, seconds: float, pitch: float, *lame: str) -> None:
    """An MP3 of a click track, by LAME: CBR 192 kbps with its Info tag unless
    LAME's options say otherwise. LAME writes no ID3 tag of its own."""
    wav = path.with_suffix(".tmp.wav")
    write_wav(wav, clicks(bpm, seconds, pitch))
    subprocess.run(["lame", "--quiet", *(lame or ("-b", "192", "--cbr")),
                    str(wav), str(path)], check=True)
    wav.unlink()


def afconvert(path: Path, bpm: float, seconds: float, pitch: float, *fmt: str) -> None:
    wav = path.with_suffix(".tmp.wav")
    write_wav(wav, clicks(bpm, seconds, pitch))
    subprocess.run(["afconvert", *fmt, str(wav), str(path)], check=True)
    wav.unlink()


# ---- tags ---------------------------------------------------------------------

def artwork(text: str, size: int, colour: str, fmt: str) -> bytes:
    img = Image.new("RGB", (size, size), colour)
    draw = ImageDraw.Draw(img)
    font = ImageFont.load_default(size=size // 7)
    draw.multiline_text((size // 2, size // 2), text, fill="white", font=font,
                        anchor="mm", align="center")
    buf = io.BytesIO()
    img.save(buf, fmt, **({"quality": 85} if fmt == "JPEG" else {}))
    return buf.getvalue()


def id3v2(path: Path, version: int, encoding: int, art: tuple[str, bytes] | None = None,
          **f: str) -> None:
    """An ID3v2 tag at VERSION (3 or 4) whose text frames use ENCODING
    (0 Latin-1, 1 UTF-16 with BOM, 3 UTF-8). Only the fields given."""
    tag = ID3()
    frames = {"title": TIT2, "artist": TPE1, "album": TALB, "genre": TCON,
              "bpm": TBPM, "key": TKEY, "label": TPUB, "track": TRCK,
              "album_artist": TPE2, "composer": TCOM, "remixer": TPE4}
    for name, frame in frames.items():
        if name in f:
            tag.add(frame(encoding=encoding, text=[f[name]]))
    if "year" in f:
        tag.add((TYER if version == 3 else TDRC)(encoding=encoding, text=[f["year"]]))
    if "comment" in f:
        tag.add(COMM(encoding=encoding, lang="eng", desc="", text=[f["comment"]]))
    if "rating" in f:
        tag.add(POPM(email="Windows Media Player 9 Series", rating=int(f["rating"]), count=0))
    if art:
        tag.add(APIC(encoding=0, mime=art[0], type=3, desc="", data=art[1]))
    tag.save(path, v1=0, v2_version=version, padding=lambda info: 0)


def id3v1(path: Path, title: str, artist: str, album: str, year: str,
          comment: str, track: int, genre: int) -> None:
    """ID3v1.1, by hand: 128 bytes at the end, and no ID3v2 anywhere."""
    def field(text: str, n: int) -> bytes:
        return text.encode("latin-1")[:n].ljust(n, b"\0")
    tag = (b"TAG" + field(title, 30) + field(artist, 30) + field(album, 30)
           + field(year, 4) + field(comment, 28) + b"\0" + bytes([track, genre]))
    with open(path, "ab") as f:
        f.write(tag)


def appledouble() -> bytes:
    """What macOS writes beside a file it copies to FAT: `._NAME`, 4096 bytes,
    an AppleDouble header (Finder info and an empty resource fork)."""
    data = bytearray(4096)
    struct.pack_into(">II16sH", data, 0, 0x00051607, 0x00020000, b"Mac OS X        ", 2)
    struct.pack_into(">IIIIII", data, 26, 9, 50, 3634, 2, 3684, 286)
    struct.pack_into(">IIII", data, 3684, 0x100, 0x100, 0, 0x1E)
    return bytes(data)


# ---- the tree -------------------------------------------------------------------

def make_files(root: Path) -> None:
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)

    def d(*parts: str) -> Path:
        p = root.joinpath(*parts)
        p.mkdir(parents=True, exist_ok=True)
        return p

    # Root: one file per tag variant. Each name differs from its title tag,
    # each BPM tag from its clicks.
    f = root / "01 Full v23.mp3"
    mp3(f, 120, 60, 880)
    id3v2(f, 3, 0, art=("image/jpeg", artwork("01\nv2.3\nJPEG", 500, "#b03030", "JPEG")),
          title="Title Tag v23", artist="Artist Tag v23", album="Album Tag v23",
          genre="Genre Tag v23", bpm="125", key="Am", comment="Comment tag v23",
          year="2023", label="Label Tag v23", track="1/4",
          album_artist="Album Artist Tag v23", composer="Composer Tag v23",
          remixer="Remixer Tag v23", rating="196")
    f = root / "02 Full v24.mp3"
    mp3(f, 128, 45, 660)
    id3v2(f, 4, 3, art=("image/png", artwork("02\nv2.4\nPNG", 300, "#3050b0", "PNG")),
          title="Title Tag v24", artist="Artist Tag v24", album="Album Tag v24",
          genre="Genre Tag v24", bpm="130", key="8A", comment="Comment tag v24",
          year="2024", label="Label Tag v24", track="2/4")
    f = root / "03 ID3v1 only.mp3"
    mp3(f, 124, 50, 990)
    id3v1(f, "Title Tag v1", "Artist Tag v1", "Album Tag v1", "2001",
          "Comment tag v1", 3, 18)          # genre 18: Techno
    mp3(root / "04 No tags.mp3", 132, 40, 740)
    mp3(root / "SHORT.MP3", 136, 20, 830)          # an 8.3 name: no long name at all
    mp3(root / "Filename Artist - Filename Title.mp3", 138, 20, 620)
    (root / "._01 Full v23.mp3").write_bytes(appledouble())
    (root / ".DS_Store").write_bytes(b"\0\0\0\1Bud1" + bytes(6148 - 8))
    (root / "cover.jpg").write_bytes(artwork("cover.jpg\n(root)", 400, "#30a050", "JPEG"))
    (root / "notes.txt").write_text("Not music: a text file at the root of PLAINMP3.\n")
    (d("System Volume Information") / "IndexerVolumeGuid").write_bytes(
        "{7c3e1d2a-0000-4000-8000-0000000e1000}".encode("utf-16-le"))
    d("Empty")
    (d("Only Docs") / "readme.txt").write_text("A folder with no music in it.\n")

    # Order: the names, the titles and the track numbers sort three different
    # ways, and the directory is written in the reverse of the names' order.
    order = d("Order")
    for name, title, track, bpm in (("a lower.mp3", "Echo", "4", 100),
                                    ("B upper.mp3", "Delta", "5", 101),
                                    ("Track 10.mp3", "Charlie", "1", 102),
                                    ("Track 2.mp3", "Bravo", "3", 103),
                                    ("Ä umlaut.mp3", "Alpha", "2", 104)):
        f = order / name
        mp3(f, bpm, 20, 400 + 40 * int(track))
        id3v2(f, 3, 1, title=title, artist="Order Artist", album="Order Album", track=track)
    f = d("Order", "M folder") / "inside.mp3"
    mp3(f, 105, 20, 450)
    id3v2(f, 3, 0, title="Inside M folder")

    # Sort: untagged files and folders whose names start with each kind of
    # character -- letters of both cases, digits, punctuation, letters
    # outside ASCII -- to show the order a CDJ lists a folder in.
    sort = d("Sort")
    names = ["abc", "ABD", "aardvark", "Zed", "1 one", "2 two", "10 ten",
             "_under", "-dash", "(paren", "!bang", "#hash", "~tilde", "&amp",
             "'quote", "Äpfel", "Éclair", "Øre", "Ωmega", "Ярь", "日本"]
    for n, name in enumerate(names):
        mp3(sort / (name + ".mp3"), 120, 2, 300 + 25 * n, "-b", "128", "--cbr")
    for name in ("B folder", "a folder", "1 folder", "_ folder", "Ä folder"):
        d("Sort", name)

    # Unicode: a long name, a decomposed (NFD) one, and one outside the BMP.
    uni = d("Unicode")
    long_name = ("Ünïcødé Ελληνικά Кириллица 日本語の曲名 — a long name that runs well "
                 "past sixty-four characters to see where the screen and the protocol cut it.mp3")
    f = uni / unicodedata.normalize("NFC", long_name)
    mp3(f, 118, 30, 560)
    id3v2(f, 3, 1, title="曲名 Ünïcødé Title Tag", artist="Артист Tag",
          album="Ελληνικά Album Tag", genre="Жанр", comment="Ç'est un commentaire")
    f = uni / unicodedata.normalize("NFD", "Café Crème NFD name.mp3")
    mp3(f, 116, 30, 590)
    id3v2(f, 4, 3, title=unicodedata.normalize("NFC", "Café Crème NFC title"),
          artist="Artiste Écrit")
    f = uni / "Emoji 🎧 name.mp3"
    mp3(f, 114, 30, 610)
    id3v2(f, 4, 3, title="Emoji 🎧 title", artist="🎛 artist")

    # Deep: a file at each of ten levels.
    path: list[str] = ["Deep"]
    for level in range(1, 11):
        path.append("L%02d" % level)
        f = d(*path) / ("Deep L%02d.mp3" % level)
        mp3(f, 110, 10, 300 + 20 * level)
        id3v2(f, 3, 0, title="Deep L%02d title" % level, artist="Deep Artist")

    # Many: 150 untagged files in one folder, to page through.
    many = d("Many")
    for n in range(1, 151):
        mp3(many / ("Many %03d.mp3" % n), 90 + n % 30, 4, 200 + 5 * n, "-b", "128", "--cbr")

    # Formats: the NXS plays MP3, AAC, WAV and AIFF; FLAC it does not. And
    # three MP3s of other shapes, and two that are not MP3s at all.
    fm = d("Formats")
    afconvert(fm / "WAV PCM.wav", 140, 15, 700, "-f", "WAVE", "-d", "LEI16")
    afconvert(fm / "AIFF PCM.aiff", 141, 15, 720, "-f", "AIFF", "-d", "BEI16")
    f = fm / "AAC.m4a"
    afconvert(f, 142, 15, 740, "-f", "m4af", "-d", "aac")
    tag = MP4(f)
    if tag.tags is None:
        tag.add_tags()
    tag["\xa9nam"], tag["\xa9ART"], tag["\xa9alb"] = ["AAC Title Tag"], ["AAC Artist Tag"], ["AAC Album Tag"]
    tag["tmpo"] = [150]
    tag["covr"] = [MP4Cover(artwork("AAC", 300, "#806020", "JPEG"), MP4Cover.FORMAT_JPEG)]
    tag.save()
    f = fm / "FLAC.flac"
    afconvert(f, 143, 15, 760, "-f", "flac", "-d", "flac")
    tag = FLAC(f)
    if tag.tags is None:
        tag.add_tags()
    tag["title"], tag["artist"], tag["bpm"] = ["FLAC Title Tag"], ["FLAC Artist Tag"], ["151"]
    tag.save()
    f = fm / "VBR V2.mp3"
    mp3(f, 144, 30, 780, "-V", "2")
    id3v2(f, 3, 0, title="VBR V2 Title Tag", artist="VBR Artist Tag", bpm="152")
    mp3(fm / "CBR no Info tag.mp3", 145, 20, 800, "-b", "192", "--cbr", "-t")
    mp3(fm / "MPEG2 22k.mp3", 146, 20, 820, "--resample", "22.05", "-b", "64")
    (fm / "Corrupt.mp3").write_bytes(random.Random(1).randbytes(65536))
    (fm / "Zero.mp3").write_bytes(b"")


def manifest(root: Path) -> str:
    rows = ["path\tbytes\tsha256"]
    for p in sorted(root.rglob("*")):
        if p.is_file():
            rows.append("%s\t%d\t%s" % (p.relative_to(root).as_posix(), p.stat().st_size,
                                        hashlib.sha256(p.read_bytes()).hexdigest()))
    return "\n".join(rows) + "\n"


# ---- the image ----------------------------------------------------------------

class Fat:
    """Just enough FAT32 to find a directory by path and rewrite its entries."""

    def __init__(self, image: bytearray, part_lba: int) -> None:
        self.img, self.base = image, part_lba * 512
        bpb = image[self.base:self.base + 512]
        self.sector, self.per_cluster = struct.unpack_from("<HB", bpb, 11)
        reserved, fats = struct.unpack_from("<HB", bpb, 14)
        self.fat_sectors, self.root = struct.unpack_from("<I4xI", bpb, 36)
        self.fats = fats
        self.fat0 = self.base + reserved * self.sector
        self.data = self.fat0 + fats * self.fat_sectors * self.sector
        self.cluster_bytes = self.sector * self.per_cluster

    def next(self, cluster: int) -> int:
        return struct.unpack_from("<I", self.img, self.fat0 + 4 * cluster)[0] & 0x0FFFFFFF

    def chain(self, cluster: int) -> list[int]:
        out = [cluster]
        while (cluster := self.next(cluster)) < 0x0FFFFFF8:
            out.append(cluster)
        return out

    def offset(self, cluster: int) -> int:
        return self.data + (cluster - 2) * self.cluster_bytes

    def read_dir(self, cluster: int) -> bytes:
        return b"".join(bytes(self.img[self.offset(c):self.offset(c) + self.cluster_bytes])
                        for c in self.chain(cluster))

    def write_dir(self, cluster: int, blob: bytes) -> None:
        for i, c in enumerate(self.chain(cluster)):
            piece = blob[i * self.cluster_bytes:(i + 1) * self.cluster_bytes]
            self.img[self.offset(c):self.offset(c) + len(piece)] = piece

    @staticmethod
    def groups(blob: bytes) -> list[tuple[str, bytes]]:
        """(name, long-name entries + short entry) for each entry in use."""
        out, run = [], b""
        for i in range(0, len(blob), 32):
            e = blob[i:i + 32]
            if e[0] == 0:
                break
            run += e
            if e[11] == 0x0F:
                continue
            parts = [run[j:j + 32] for j in range(0, len(run) - 32, 32)]
            text = b"".join(p[1:11] + p[14:26] + p[28:32] for p in reversed(parts))
            name = text.decode("utf-16-le").split("\0")[0] if parts else \
                (e[0:8].decode("ascii").rstrip() + ("." + e[8:11].decode("ascii").rstrip()
                                                    if e[8:11].strip() else ""))
            out.append((name, run))
            run = b""
        return out

    def find(self, path: str) -> int:
        cluster = self.root
        for part in [p for p in path.split("/") if p]:
            for name, run in self.groups(self.read_dir(cluster)):
                if name == part:
                    e = run[-32:]
                    cluster = struct.unpack_from("<H", e, 20)[0] << 16 | struct.unpack_from("<H", e, 26)[0]
                    break
            else:
                raise SystemExit("not on the image: " + path)
        return cluster

    def walk(self, cluster: int, prefix: str = "") -> list[str]:
        lines = []
        for name, run in self.groups(self.read_dir(cluster)):
            e = run[-32:]
            if name in (".", ".."):
                continue
            attr, size = e[11], struct.unpack_from("<I", e, 28)[0]
            short = e[0:11].decode("ascii")
            flags = "".join(c for c, bit in (("H", 2), ("S", 4), ("V", 8)) if attr & bit)
            if attr & 0x08:
                lines.append("%s[volume label %s]" % (prefix, short.rstrip()))
                continue
            first = struct.unpack_from("<H", e, 20)[0] << 16 | struct.unpack_from("<H", e, 26)[0]
            kind = "dir" if attr & 0x10 else "%d B" % size
            lines.append("%s%s    (8.3 %s, %s, cluster %d%s)" % (
                prefix, name, short, kind, first, ", " + flags if flags else ""))
            if attr & 0x10:
                lines += self.walk(first, prefix + "    ")
        return lines


def lfn_checksum(short: bytes) -> int:
    total = 0
    for byte in short:
        total = (((total & 1) << 7) + (total >> 1) + byte) & 0xFF
    return total


def windows_short(name: str, taken: set[bytes]) -> bytes:
    """The 8.3 name Windows makes for NAME: leading dots and every space
    dropped, upper case, `_` for what 8.3 cannot hold, six characters and ~N."""
    name = name.lstrip(".")
    stem, dot, ext = name.rpartition(".")
    if not dot:
        stem, ext = name, ""

    def clean(text: str) -> str:
        return "".join("_" if not c.isascii() or c in '+,;=[]"*?<>|:/\\' else c
                       for c in text.upper() if c != " ")
    stem, ext = clean(stem), clean(ext)[:3]
    for n in range(1, 100):
        tail = "~%d" % n
        short = ((stem[:8 - len(tail)] + tail).ljust(8) + ext.ljust(3)).encode("ascii")
        if short not in taken:
            taken.add(short)
            return short
    raise SystemExit("no short name for " + name)


def touch_up(fat: Fat) -> None:
    # 1. No short name starts with a dot but "." and "..": the builder keeps
    #    the dot of `.DS_Store` and `._x.mp3`; Windows and macOS drop it.
    def fix(cluster: int) -> None:
        blob = bytearray(fat.read_dir(cluster))
        groups = Fat.groups(bytes(blob))
        taken = {run[-32:-21] for _, run in groups}
        pos = 0
        for name, run in groups:
            pos += len(run)
            short_at = pos - 32
            if name in (".", "..") or blob[short_at] != ord("."):
                continue
            short = windows_short(name, taken)
            blob[short_at:short_at + 11] = short
            for lfn in range(pos - len(run), short_at, 32):
                blob[lfn + 13] = lfn_checksum(short)
        fat.write_dir(cluster, bytes(blob))
        for name, run in groups:
            e = run[-32:]
            if e[11] & 0x10 and name not in (".", ".."):
                fix(struct.unpack_from("<H", e, 20)[0] << 16 | struct.unpack_from("<H", e, 26)[0])
    fix(fat.root)

    # 2. A zero-length file owns no cluster: the builder gives it one.
    blob = bytearray(fat.read_dir(fat.find("Formats")))
    for i in range(0, len(blob), 32):
        if blob[i + 11] != 0x0F and blob[i] not in (0, 0xE5) and struct.unpack_from("<I", blob, i + 28)[0] == 0 \
                and not blob[i + 11] & 0x10:
            cluster = struct.unpack_from("<H", blob, i + 20)[0] << 16 | struct.unpack_from("<H", blob, i + 26)[0]
            struct.pack_into("<HH", blob, i + 20, 0, 0)
            struct.pack_into("<H", blob, i + 26, 0)
            for copy in range(fat.fats):
                struct.pack_into("<I", fat.img, fat.fat0 + copy * fat.fat_sectors * fat.sector + 4 * cluster, 0)
    fat.write_dir(fat.find("Formats"), bytes(blob))

    # 3. System Volume Information: hidden and system, as Windows leaves it.
    blob = bytearray(fat.read_dir(fat.root))
    pos = 0
    for name, run in Fat.groups(bytes(blob)):
        pos += len(run)
        if name == "System Volume Information":
            blob[pos - 32 + 11] = 0x10 | 0x02 | 0x04

    # 4. The volume label, as a root entry too (the builder writes it in the
    #    boot sector only), in the first free slot.
    free = blob.find(b"\0" * 32, pos)
    if free < 0 or free % 32:
        raise SystemExit("no free root entry for the label")
    blob[free:free + 32] = LABEL.ljust(11).encode("ascii") + bytes([0x08]) + bytes(20)
    fat.write_dir(fat.root, bytes(blob))

    # 5. Order/ in the reverse of its name order.
    cluster = fat.find("Order")
    groups = Fat.groups(fat.read_dir(cluster))
    dots, rest = groups[:2], groups[2:]
    blob = b"".join(run for _, run in dots + rest[::-1])
    fat.write_dir(cluster, blob.ljust(len(fat.read_dir(cluster)), b"\0"))


def make_image(files: Path, image: Path, emulator: Path) -> str:
    sys.path.insert(0, str(emulator))
    from tools.cdj_main import make_sd_image as builder   # noqa: E402

    b = builder.Builder(1 << 30, image)
    if b.alloc(1)[0] != 2:
        raise SystemExit("root cluster must be 2")
    b.add_dir(files, 2, 0, is_root=True)
    b.finish(label=LABEL)
    fat = Fat(b.image, builder.PART_LBA)
    touch_up(fat)
    b.image.flush()
    return "\n".join(fat.walk(fat.root)) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("out", type=Path, help="a directory: OUT/files, OUT/plainmp3.img")
    parser.add_argument("--image", action="store_true", help="also build OUT/plainmp3.img")
    parser.add_argument("--emulator", type=Path, default=HERE.parents[4] / "cdj2000-emulator",
                        help="a cdj2000-emulator checkout (default: TriMixxx's)")
    args = parser.parse_args()

    files = args.out / "files"
    make_files(files)
    (args.out / "MANIFEST.tsv").write_text(manifest(files))
    print("%s: %d files" % (files, sum(1 for p in files.rglob("*") if p.is_file())))
    if args.image:
        if not (args.emulator / "tools/cdj_main/make_sd_image.py").exists():
            raise SystemExit("no make_sd_image.py under %s: --emulator" % args.emulator)
        image = args.out / "plainmp3.img"
        listing = make_image(files, image, args.emulator)
        (args.out / "LISTING.txt").write_text(listing)
        print("%s: 1 GiB FAT32, label %s" % (image, LABEL))
    return 0


if __name__ == "__main__":
    sys.exit(main())
