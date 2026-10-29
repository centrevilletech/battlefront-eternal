#!/usr/bin/env python3
"""KTX2 copies of the extracted textures: Basis UASTC, Zstandard, full mips.

    python3 tools/ktx2.py [--force] [--jobs N] [path ...]
    python3 tools/ktx2.py --check [path ...]

A path is a `textures` directory, or a directory above some (a pack root such
as public/maps/nab2, public/sides/rep or public/common), whose `textures`
directories are all processed. With none, every pack under public/ is.

Each PNG gets a KTX2 beside it (foo.png -> foo.ktx2) and an entry in its
directory's ktx2.json. A PNG whose bytes and encoder settings are unchanged
is not encoded again, and KTX2s whose PNG is gone are removed. --check decodes
the KTX2s back and compares them with their PNGs. docs/textures-ktx2.md has
the details.

The encoder is KTX-Software's `ktx` (4.4 or later), found through
$KTX2_ENCODER, then PATH, then ~/.cache/bf-tools. KTX2_ENCODER=off turns it
off. Without an encoder this only warns and exits 0: the game loads the PNGs.
"""

import argparse
import hashlib
import json
import math
import operator
import os
import re
import shutil
import struct
import subprocess
import sys
import time
import zlib
from concurrent.futures import ProcessPoolExecutor, ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import PUBLIC  # noqa: E402
CACHE = Path.home() / ".cache" / "bf-tools"
MANIFEST = "ktx2.json"
MINIMUM = (4, 4)  # `ktx create --assign-tf` arrived in KTX-Software 4.4

# The engine samples textures raw and lights and blends in gamma space, so the
# data is tagged linear. ktx then neither converts the values nor linearizes
# them to filter mipmaps: the mips are box-filtered on the raw values, as
# gl.generateMipmap made them. Tagging sRGB would only switch on basisu's
# perceptual metrics, which its UASTC encoder doesn't read. There is no
# --convert-texcoord-origin, so rows stay in PNG order for flipY = false.
# Alpha is kept wherever any texel has some: basisu drops an all-opaque
# channel by itself.
FLAGS = (
    "--format", "R8G8B8A8_UNORM", "--assign-tf", "linear",
    "--generate-mipmap", "--mipmap-filter", "box",
    "--encode", "uastc", "--uastc-quality", "2", "--zstd", "18",
)

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
KTX2_IDENTIFIER = b"\xabKTX 20\xbb\r\n\x1a\n"
TEMPORARY = re.compile(r"\..+\.(\d+)\.tmp")
OFF = ("off", "none", "0", "false", "no")


class Encoder:
    def __init__(self, path, version):
        self.path = str(path)
        self.version = version
        self.description = f"ktx create {version} {' '.join(FLAGS)}"


def ktx_version(path):
    """The version of KTX-Software's `ktx` at `path` as (text, (major, minor)),
    or None if it isn't one."""
    try:
        run = subprocess.run([str(path), "--version"], capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.SubprocessError):
        return None
    found = re.search(r"ktx version: (v?(\d+)\.(\d+)\S*)", run.stdout + run.stderr)
    return (found[1], (int(found[2]), int(found[3]))) if found else None


def candidates():
    configured = os.environ.get("KTX2_ENCODER", "").strip()
    if configured:
        path = Path(configured).expanduser()
        yield path / "bin" / "ktx" if path.is_dir() else path
        return
    if shutil.which("ktx"):
        yield Path(shutil.which("ktx"))
    yield CACHE / "bin" / "ktx"
    unpacked = CACHE.glob("KTX-Software-*/bin/ktx")
    yield from sorted(unpacked, key=lambda p: [int(n) for n in re.findall(r"\d+", p.parent.parent.name)], reverse=True)


def find_encoder():
    """(Encoder, None), or (None, why there is none)."""
    if os.environ.get("KTX2_ENCODER", "").strip().lower() in OFF:
        return None, "KTX2_ENCODER is off"
    tried = []
    for path in candidates():
        if not path.is_file():
            if os.environ.get("KTX2_ENCODER"):
                tried.append(f"{path} (missing)")
            continue
        found = ktx_version(path)
        if found and found[1] >= MINIMUM:
            return Encoder(path, found[0]), None
        tried.append(f"{path} ({found[0] + ', too old' if found else 'not KTX-Software'})")
    wanted = "no KTX2 encoder: KTX-Software's `ktx` 4.4+ is not in $KTX2_ENCODER, PATH or ~/.cache/bf-tools"
    return None, wanted + (f" (tried {', '.join(tried)})" if tried else "")


def default_paths():
    return [p for p in (*sorted(PUBLIC.glob("maps/*")), *sorted(PUBLIC.glob("sides/*")), PUBLIC / "common") if p.is_dir()]


def texture_dirs(paths):
    """The `textures` directories at or under each path, and the paths that
    don't exist."""
    found, missing = {}, []
    for path in map(Path, paths):
        if not path.is_dir():
            missing.append(path)
            continue
        for directory in [path] if path.name == "textures" else sorted(path.rglob("textures")):
            if directory.is_dir():
                found.setdefault(directory.resolve(), directory)
    return list(found.values()), missing


def shown(path):
    try:
        return str(Path(path).resolve().relative_to(ROOT))
    except ValueError:
        return str(path)


def png_size(data):
    """A PNG's (width, height) from its header, or None if it isn't a PNG."""
    if data[:8] != PNG_SIGNATURE or data[12:16] != b"IHDR":
        return None
    return struct.unpack(">II", data[16:24])


def mip_chain(width, height):
    """Every level's size, down to 1x1, as ktx --generate-mipmap makes them."""
    sizes = [(width, height)]
    while sizes[-1] != (1, 1):
        w, h = sizes[-1]
        sizes.append((max(1, w // 2), max(1, h // 2)))
    return sizes


def rgba8_bytes(width, height):
    return sum(w * h * 4 for w, h in mip_chain(width, height))


def bc7_bytes(width, height):
    """BC7 (and ASTC 4x4): 16 bytes per 4x4 block, a block at least per level."""
    return sum(((w + 3) // 4) * ((h + 3) // 4) * 16 for w, h in mip_chain(width, height))


def entry(ktx2, width, height, sha1, size):
    """A ktx2.json record; the runtime reads exactly these keys."""
    return {"ktx2": ktx2, "width": width, "height": height, "sha1": sha1, "bytes": size}


def read_manifest(directory):
    try:
        manifest = json.loads((directory / MANIFEST).read_text())
    except (OSError, ValueError):
        return {}
    return manifest if isinstance(manifest, dict) and manifest.get("version") == 1 else {}


class Folder:
    """One textures directory: its manifest as it will be written, and the
    PNGs it still needs encoded."""

    def __init__(self, path):
        self.path = path
        self.files = {}  # PNG name -> entry()
        self.skipped = {}  # PNG name -> reason
        self.png_bytes = {}  # PNG name -> size, for the summary
        self.jobs = []
        self.pending = 0
        self.encoded = self.unchanged = self.failed = self.removed = 0


class Job:
    def __init__(self, folder, png, sha1, width, height):
        self.folder, self.png, self.sha1, self.width, self.height = folder, png, sha1, width, height
        self.target = png.with_suffix(".ktx2")


def survey(path, encoder, force):
    """Sort a directory's PNGs into unchanged, skipped and to encode."""
    folder = Folder(path)
    old = read_manifest(path)
    previous = old.get("files") if old.get("encoder") == encoder.description and not force else None
    previous = previous if isinstance(previous, dict) else {}
    claimed = {}
    for png in sorted(p for p in path.iterdir() if p.suffix.lower() == ".png" and p.is_file()):
        data = png.read_bytes()
        size = png_size(data)
        if size is None:
            folder.skipped[png.name] = "not a PNG"
            continue
        width, height = size
        if width % 4 or height % 4:
            folder.skipped[png.name] = (f"{width}x{height}: both sides must be multiples of 4 (so at least 4) "
                                        "for WebGL to take a block-compressed level 0")
            continue
        target = png.with_suffix(".ktx2").name
        if target in claimed:
            folder.skipped[png.name] = f"{target} is already {claimed[target]}'s"
            continue
        claimed[target] = png.name
        sha1 = hashlib.sha1(data).hexdigest()
        folder.png_bytes[png.name] = len(data)
        old_entry = previous.get(png.name) or {}
        ktx2 = path / target
        if (old_entry.get("sha1") == sha1 and old_entry.get("ktx2") == target and ktx2.is_file()
                and ktx2.stat().st_size == old_entry.get("bytes")):
            folder.files[png.name] = entry(target, width, height, sha1, old_entry["bytes"])
            folder.unchanged += 1
        else:
            folder.jobs.append(Job(folder, png, sha1, width, height))
    return folder


def encode_one(encoder, job):
    """Encode one PNG into a temporary file, then move that into place.
    Returns (job, KTX2 size or None, error or None)."""
    temporary = job.target.with_name(f".{job.target.name}.{os.getpid()}.tmp")
    command = [encoder.path, "create", *FLAGS, "--threads", "1", str(job.png), str(temporary)]
    try:
        run = subprocess.run(command, capture_output=True, text=True)
    except OSError as error:
        return job, None, str(error)
    if run.returncode or not temporary.is_file():
        temporary.unlink(missing_ok=True)
        lines = (run.stderr or run.stdout).strip().splitlines()
        return job, None, lines[-1] if lines else f"ktx exited with {run.returncode}"
    os.replace(temporary, job.target)
    return job, job.target.stat().st_size, None


def stale(path):
    """A temporary file whose run is gone (interrupted)."""
    match = TEMPORARY.fullmatch(path.name)
    if not match:
        return False
    try:
        os.kill(int(match[1]), 0)
    except ProcessLookupError:
        return True
    except (OSError, OverflowError):
        pass
    return False


def finish(folder, encoder, log):
    """Remove KTX2s nothing in the manifest owns, then write the manifest."""
    keep = {record["ktx2"] for record in folder.files.values()}
    for path in folder.path.iterdir():
        if path.is_file() and path.suffix == ".ktx2" and path.name not in keep:
            path.unlink()
            folder.removed += 1
        elif path.is_file() and stale(path):
            path.unlink()
    manifest = {
        "version": 1,
        "encoder": encoder.description,
        "files": {name: folder.files[name] for name in sorted(folder.files)},
        "skipped": {name: folder.skipped[name] for name in sorted(folder.skipped)},
    }
    text = json.dumps(manifest, indent=2) + "\n"
    target = folder.path / MANIFEST
    if not target.is_file() or target.read_text() != text:
        temporary = folder.path / f".{MANIFEST}.{os.getpid()}.tmp"
        temporary.write_text(text)
        os.replace(temporary, target)
    note = f", {folder.failed} failed" if folder.failed else ""
    note += f", {folder.removed} stale removed" if folder.removed else ""
    log(f"ktx2: {shown(folder.path)}: {folder.encoded} encoded, {folder.unchanged} unchanged, "
        f"{len(folder.skipped)} skipped{note}")


def megabytes(n):
    return f"{n / 1e6:.1f} MB"


def count(n, one, many=None):
    return f"{n} {one if n == 1 else many or one + 's'}"


def encode(paths=None, force=False, jobs=None, log=print):
    """Encode the PNGs in the textures directories at or under `paths`
    (default: every pack under public/). Returns a summary, or None when
    there is no encoder."""
    encoder, problem = find_encoder()
    if encoder is None:
        log(f"ktx2: warning: {problem}. Skipping KTX2 compression; the game loads the PNGs. "
            "docs/textures-ktx2.md says how to install it.")
        return None
    started = time.monotonic()
    directories, missing = texture_dirs(paths or default_paths())
    for path in missing:
        log(f"ktx2: warning: {path}: no such directory")
    jobs = max(1, jobs or os.cpu_count() or 1)
    folders = [survey(directory, encoder, force) for directory in directories]
    # Largest first, so no big texture is left running alone at the end.
    work = sorted((job for folder in folders for job in folder.jobs), key=lambda j: j.width * j.height, reverse=True)
    log(f"ktx2: {len(work)} to encode in {count(len(folders), 'directory', 'directories')} "
        f"with {encoder.description} ({jobs} jobs)")
    for folder in folders:
        folder.pending = len(folder.jobs)
        if not folder.pending:
            finish(folder, encoder, log)
    pool = ThreadPoolExecutor(jobs)
    try:
        for future in as_completed([pool.submit(encode_one, encoder, job) for job in work]):
            job, size, error = future.result()
            folder = job.folder
            if error:
                folder.skipped[job.png.name] = f"encode failed: {error}"
                folder.failed += 1
                log(f"ktx2: warning: {shown(job.png)}: {error}")
            else:
                folder.files[job.png.name] = entry(job.target.name, job.width, job.height, job.sha1, size)
                folder.encoded += 1
            folder.pending -= 1
            if not folder.pending:
                finish(folder, encoder, log)
    finally:
        # On Ctrl-C, don't start the queued encodes; finished directories
        # already have their manifests.
        pool.shutdown(cancel_futures=True)

    records = [(folder, name, record) for folder in folders for name, record in folder.files.items()]
    summary = {
        "encoder": encoder.description,
        "directories": len(folders),
        "missing": len(missing),
        "encoded": sum(f.encoded for f in folders),
        "unchanged": sum(f.unchanged for f in folders),
        "skipped": sum(len(f.skipped) for f in folders),
        "failed": sum(f.failed for f in folders),
        "removed": sum(f.removed for f in folders),
        "png_bytes": sum(folder.png_bytes[name] for folder, name, _ in records),
        "ktx2_bytes": sum(record["bytes"] for _, _, record in records),
        "rgba8_bytes": sum(rgba8_bytes(r["width"], r["height"]) for _, _, r in records),
        "bc7_bytes": sum(bc7_bytes(r["width"], r["height"]) for _, _, r in records),
        "seconds": round(time.monotonic() - started, 1),
    }
    s = summary
    log(f"ktx2: {s['encoded']} encoded, {s['unchanged']} unchanged, {s['skipped']} skipped ({s['failed']} failed), "
        f"{s['removed']} stale removed, in {count(s['directories'], 'directory', 'directories')}, {s['seconds']:.0f} s")
    if records:
        log(f"ktx2: {count(len(records), 'texture')}: PNG {megabytes(s['png_bytes'])} -> KTX2 {megabytes(s['ktx2_bytes'])} "
            f"({100 * s['ktx2_bytes'] / s['png_bytes']:.0f}%); GPU with full mips: RGBA8 {megabytes(s['rgba8_bytes'])} "
            f"-> BC7 {megabytes(s['bc7_bytes'])}")
    return summary


def encode_optional(paths=None, **options):
    """encode() for the extractors, prefixing their indent. An extraction
    never fails over compression: a missing encoder or any error only warns."""
    log = options.pop("log", None) or (lambda line: print(f"  {line}", flush=True))
    try:
        return encode(paths, log=log, **options)
    except Exception as error:  # noqa: BLE001 - the PNGs still work
        log(f"ktx2: warning: compression stopped ({type(error).__name__}: {error}); the game loads the PNGs")
        return None


# --check: decode the KTX2s back (ktx extract) and compare them with their PNGs
# (decoded here, independently of ktx), so a flip, a lost alpha channel or
# linear-light mip filtering would show.

def decode_png(data):
    """An 8-bit, non-interlaced PNG without a palette as (width, height, RGBA)."""
    if data[:8] != PNG_SIGNATURE:
        raise ValueError("not a PNG")
    position, idat, header = 8, [], None
    while position + 8 <= len(data):
        length, kind = struct.unpack_from(">I4s", data, position)
        body = data[position + 8:position + 8 + length]
        position += 12 + length
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat.append(body)
        elif kind == b"IEND":
            break
    width, height, depth, color, _, _, interlace = header
    channels = {0: 1, 2: 3, 4: 2, 6: 4}.get(color)
    if depth != 8 or channels is None or interlace:
        raise ValueError(f"unsupported PNG (depth {depth}, colour type {color}, interlace {interlace})")
    raw = zlib.decompress(b"".join(idat))
    stride = width * channels
    pixels = bytearray(height * stride)
    previous = bytes(stride)
    for y in range(height):
        start = y * (stride + 1)
        line = bytearray(raw[start + 1:start + 1 + stride])
        unfilter(raw[start], line, previous, channels)
        pixels[y * stride:(y + 1) * stride] = line
        previous = line
    if channels == 4:
        return width, height, bytes(pixels)
    rgba = bytearray(b"\xff" * (width * height * 4))
    for c in range(3):
        rgba[c::4] = pixels[c::3] if channels == 3 else pixels[0::channels]
    if channels == 2:
        rgba[3::4] = pixels[1::2]
    return width, height, bytes(rgba)


def unfilter(kind, line, previous, bpp):
    if kind == 1:
        for i in range(bpp, len(line)):
            line[i] = (line[i] + line[i - bpp]) & 255
    elif kind == 2:
        line[:] = bytes(map(lambda a, b: (a + b) & 255, line, previous))
    elif kind == 3:
        for i in range(len(line)):
            line[i] = (line[i] + (((line[i - bpp] if i >= bpp else 0) + previous[i]) >> 1)) & 255
    elif kind == 4:
        for i in range(len(line)):
            a, b, c = (line[i - bpp], previous[i], previous[i - bpp]) if i >= bpp else (0, previous[i], 0)
            p = a + b - c
            pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
            line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
    elif kind:
        raise ValueError(f"bad PNG filter {kind}")


def ktx2_header(data):
    """The KTX2 header and DFD fields --check looks at."""
    if data[:12] != KTX2_IDENTIFIER:
        raise ValueError("not a KTX2 file")
    width, height = struct.unpack_from("<II", data, 20)
    levels, scheme, dfd = struct.unpack_from("<III", data, 40)
    return {"width": width, "height": height, "levels": levels, "scheme": scheme,
            "model": data[dfd + 12], "transfer": data[dfd + 14], "channel": data[dfd + 31] & 15}


def extract(ktx, path, level):
    """One level of a KTX2, transcoded to RGBA8 by ktx."""
    run = subprocess.run([ktx, "extract", "--transcode", "rgba8", "--raw", "--level", str(level), str(path), "-"],
                         capture_output=True)
    if run.returncode:
        raise ValueError(f"ktx extract: {run.stderr.decode(errors='replace').strip()}")
    return run.stdout


def squared_error(a, b, channels):
    """Sum of squared differences and the largest difference, over `channels`."""
    sse = worst = 0
    for c in channels:
        d = list(map(operator.sub, a[c::4], b[c::4]))
        sse += sum(map(operator.mul, d, d))
        worst = max(worst, max(map(abs, d), default=0))
    return sse, worst


def psnr(sse, count):
    return math.inf if not sse else 10 * math.log10(255 * 255 * count / sse)


def flip_rows(rgba, width, height):
    stride = width * 4
    return b"".join(rgba[y * stride:(y + 1) * stride] for y in reversed(range(height)))


def to_linear(v):
    v /= 255
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def to_srgb(v):
    v = 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055
    return min(255, max(0, round(v * 255)))


LINEAR = [to_linear(v) for v in range(256)]


def half(rgba, width, height, linear_light=False):
    """A 2x2 box filter, on the raw values or (linear_light) on the RGB
    decoded from sRGB, as a mip filter that linearizes would do."""
    def raw(a, b, c, d):
        return (a + b + c + d + 2) >> 2

    def light(a, b, c, d):
        return to_srgb((LINEAR[a] + LINEAR[b] + LINEAR[c] + LINEAR[d]) / 4)

    stride = width * 4
    out = bytearray((width // 2) * (height // 2) * 4)
    row = (width // 2) * 4
    for y in range(height // 2):
        top, bottom = rgba[2 * y * stride:(2 * y + 1) * stride], rgba[(2 * y + 1) * stride:(2 * y + 2) * stride]
        line = bytearray(row)
        for c in range(4):
            average = light if linear_light and c < 3 else raw
            line[c::4] = bytes(map(average, top[c::8], top[c + 4::8], bottom[c::8], bottom[c + 4::8]))
        out[y * row:(y + 1) * row] = line
    return bytes(out)


def check_one(ktx, png, path):
    """Compare a KTX2 with its PNG: (ok, report line, RGB PSNR or None)."""
    try:
        return compare(ktx, png, path)
    except (OSError, ValueError, struct.error, zlib.error) as error:
        return False, f"{shown(png)}: PROBLEM: {error}", None


def compare(ktx, png, path):
    width, height, rgba = decode_png(png.read_bytes())
    header = ktx2_header(path.read_bytes())
    level0 = extract(ktx, path, 0)
    problems = []
    if (header["width"], header["height"]) != (width, height) or len(level0) != len(rgba):
        return False, f"{shown(png)}: KTX2 is {header['width']}x{header['height']}, PNG {width}x{height}", None
    if header["levels"] != len(mip_chain(width, height)):
        problems.append(f"{header['levels']} levels, not {len(mip_chain(width, height))}")
    if (header["model"], header["scheme"], header["transfer"]) != (166, 2, 1):
        problems.append(f"model {header['model']}, supercompression {header['scheme']}, transfer {header['transfer']} "
                        "(want UASTC 166, Zstandard 2, linear 1)")
    has_alpha = min(rgba[3::4]) < 255
    if has_alpha != (header["channel"] == 3):
        problems.append(f"alpha {'lost' if has_alpha else 'added'} (DFD channel {header['channel']})")
    pixels = width * height
    sse, worst = squared_error(rgba, level0, (0, 1, 2))
    alpha_sse, alpha_worst = squared_error(rgba, level0, (3,))
    flipped = squared_error(flip_rows(rgba, width, height), level0, (0, 1, 2, 3))[0]
    if flipped < sse + alpha_sse:
        problems.append("closer to the PNG upside down")
    orientation = "symmetric" if flipped == sse + alpha_sse else f"upside down {db(psnr(flipped, 4 * pixels))}"
    line = (f"{shown(png)} {width}x{height} {header['levels']} levels {'RGBA' if header['channel'] == 3 else 'RGB'}: "
            f"RGB {db(psnr(sse, 3 * pixels))} (max {worst}), A {db(psnr(alpha_sse, pixels))} (max {alpha_worst}), "
            f"{orientation}")
    if header["levels"] > 1:
        # Where the raw and the linear-light box filters disagree, which one
        # did the encoder's level 1 follow?
        level1 = extract(ktx, path, 1)
        raw, light = half(rgba, width, height), half(rgba, width, height, True)
        nearer = telling = 0
        for c in range(3):
            for d, r, l in zip(level1[c::4], raw[c::4], light[c::4]):
                if abs(r - l) >= 4:
                    telling += 1
                    nearer += abs(d - r) < abs(d - l)
        small = width // 2 * (height // 2)
        line += f"; mip 1 vs raw box {db(psnr(squared_error(raw, level1, (0, 1, 2))[0], 3 * small))}"
        if telling:
            line += f", nearer raw than linear-light on {100 * nearer / telling:.0f}% of {telling}"
            if telling >= 100 and nearer < telling / 2:
                problems.append("mips look filtered in linear light")
    return not problems, line + (f": PROBLEM: {'; '.join(problems)}" if problems else ": ok"), psnr(sse, 3 * pixels)


def db(value):
    return "exact" if value == math.inf else f"{value:.1f} dB"


def check(paths, jobs, log=print):
    """--check: compare KTX2s with their PNGs. Files (a .png or its .ktx2)
    are checked alone; directories by their ktx2.json."""
    encoder, problem = find_encoder()
    if encoder is None:
        log(f"ktx2: {problem}; --check needs it to transcode")
        return 1
    pairs, missing = [], []
    for path in map(Path, paths or default_paths()):
        if path.is_file():
            png = path if path.suffix.lower() == ".png" else path.with_suffix(".png")
            pairs.append((png, png.with_suffix(".ktx2")))
            continue
        directories, gone = texture_dirs([path])
        missing += gone
        for directory in directories:
            for name, record in sorted(read_manifest(directory).get("files", {}).items()):
                pairs.append((directory / name, directory / record["ktx2"]))
    for path in missing:
        log(f"ktx2: {path}: no such file or directory")
    failures, scores = 0, []
    with ProcessPoolExecutor(max(1, jobs)) as pool:
        pngs, ktx2s = [png for png, _ in pairs], [ktx2 for _, ktx2 in pairs]
        for ok, line, score in pool.map(check_one, [encoder.path] * len(pairs), pngs, ktx2s):
            log(line)
            failures += not ok
            if score is not None:
                scores.append(score)
    scores.sort()
    spread = f"; RGB PSNR min {db(scores[0])}, median {db(scores[len(scores) // 2])}" if scores else ""
    log(f"ktx2 check: {len(pairs)} files, {failures} with problems{spread}")
    return 1 if failures or missing else 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("paths", nargs="*", type=Path,
                        help="textures directories or pack roots (default: public/maps/*, public/sides/*, public/common)")
    parser.add_argument("--force", action="store_true", help="re-encode every PNG, changed or not")
    parser.add_argument("--jobs", type=int, default=os.cpu_count() or 1, help="parallel encodes (default: %(default)s)")
    parser.add_argument("--check", action="store_true",
                        help="instead of encoding, decode the KTX2s and compare them with their PNGs "
                             "(paths may also be single .png or .ktx2 files)")
    args = parser.parse_args(argv)
    if args.jobs < 1:
        parser.error("--jobs must be at least 1")
    if args.check:
        return check(args.paths, args.jobs)
    summary = encode(args.paths, args.force, args.jobs, log=lambda line: print(line, flush=True))
    return 1 if summary and (summary["failed"] or summary["missing"]) else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)
