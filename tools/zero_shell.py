#!/usr/bin/env python3
"""The front end's data: the menus' art and text, the shell's movies, music
and transition sounds, the Instant Action map catalog, and the loading
screen. docs/shell-assets.md has the schema.

    python3 tools/zero_shell.py                  # all of it, the movies last
    python3 tools/zero_shell.py --common         # common.json: the menus' textures, strings and rasterHud
    python3 tools/zero_shell.py --audio          # public/audio/shell: music, whooshes, interface sounds
    python3 tools/zero_shell.py --catalog        # shell.json and the loading screen's pictures
    python3 tools/zero_shell.py --movies [--jobs 2] [--only main kam1fly ...] [--force]

It reads the game as installed (data/game/install), with the add-ons beside
it (GameData/AddOn; tools/zero_addon.py: their maps join the catalog, their
flyovers the movies), and writes only gitignored, retail-derived data:

    public/common/common.json         gains the menus' textures, strings and rasterHud entries
    public/common/textures/           their PNGs, with KTX2 copies (tools/ktx2.py)
    public/common/raster-hud/         4x WebP atlases (tools/upscale_hud.py; needs Pillow)
    public/common/shell/shell.json    movies by screen, the map catalog, music, whooshes, the loading screen
    public/common/shell/movies/       Movies/shell.mvs's Bink movies as VP9 WebM, with manifest.json
    public/common/shell/load/         the loading screen's pictures
    public/audio/shell/               the music (Ogg Opus), the whooshes (FLAC), and the shell's
                                      sound for the audio engine (sound.json, bank.bin)

Each part only adds to what is there, so running it again is harmless. A
movie whose WebM was made with the same settings isn't encoded again
(--force encodes it anyway). The movies need ffmpeg with the Bink decoders,
libvpx-vp9 and libopus; the music and whooshes need ffmpeg with libopus.

Movies/shell.mvs is a ucfb tree whose tags are name hashes, as in the sound
archives: an Info chunk (Name "shell", NumSegments, then SegmentInfo and a
Segment ... SegmentEnd record per movie with its Name, Length and Padding)
and a Data chunk holding the movies back to back, each followed by its
padding. The movie config (shell.lvl's mcfg) maps MovieProperties names to
segments, loops and fades.
"""

import argparse
import json
import mmap
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import zlib
from array import array
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import zero_addon
from ucfb import child, chunks, cstr_at, iter_archives
from zero_ai_mission import faction_of
from zero_config import parse as parse_config
from zero_hash import fnv1a
from zero_locl import localize
from zero_lua import global_tables, mission_scripts, mission_setup
from zero_lua import load as lua_load
from zero_sound import archive_contents, archive_contents_flat, config_objects, decode_segment, hexname, raw_chunks
from zero_sound import stream_substreams
from zero_spawn_screen import display_text
from zero_texture import decode_texture, safe_name

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import GAME  # noqa: E402
from zero_paths import PUBLIC  # noqa: E402
LANGUAGE = "ENG"


def archives(game=GAME):
    """The archives the front end reads."""
    return {
        "common": game / "Common" / LANGUAGE / "common.lvl",
        "core": game / "core.lvl",
        "shell": game / "Shell" / LANGUAGE / "shell.lvl",
        "load": game / "Load" / "load.lvl",
        "sound": game / "Sound" / "shell.lvl",
        "bank": game / "Sound" / "common.bnk",
        "movies": game / "Movies" / "shell.mvs",
        "mission": game / "mission.lvl",
    }


# ---------------------------------------------------------------- art

# The menus' art in common.lvl that the HUD, spawn screen and scoreboard
# didn't already take: a menu row's bar (left end, stretched middle, and the
# right end's triangle), a window's skin while the pointer is over it, the
# sliders' and scroll bars' track and thumb, white, the segment lines' and
# tooltips' fills, ifelem_titlebar's ends, the gamepad hints, and the player
# list's icons.
MENU_TEXTURES = (
    "buttonleft", "buttonmid", "buttonright", "border_3a_pieces", "slider_bg", "slider_fg", "white",
    "gray_rect", "tooltip_bg", "titlebar_l", "titlebar_r", "btna", "btnb", "btnmisc", "btnmisc2",
    "lobby_icons", "ping_icon",
)
# shell.lvl's screen art, drawn at about its size: Instant Action's backdrop
# (the blue grid with the hole the flyover shows through), the option arrows,
# the session list's icons, the big title bar, the briefing's map ring, and
# the Galactic Conquest stubs' side emblems and icons.
SHELL_TEXTURES = (
    "his_brief_bg", "arrow_l", "arrow_r", "arrow_large", "lock_icon", "version_icon", "titlebar_large",
    "small_title", "ring_small", "ring_mask", "side_a", "side_c", "side_i", "side_r", "side_alt_a", "side_alt_c",
    "side_alt_i", "side_alt_r", "cis_own_icon", "imp_own_icon", "reb_own_icon", "rep_own_icon",
)
# shell.lvl's pictures, kept at their size: the multiplayer screens' backdrop,
# the stills of the screen movies (unused by the original; posters here), the
# movie frames, Galactic Conquest's configurations and the maps' pictures.
SHELL_PICTURES = (
    "iface_bgmeta_space", "main", "atat_screen", "coreship_screen", "atte_screen", "hoth_screen",
    "generic2_screen", "generic3_screen", "gc_screen", "movie_bg", "movie_bg2",
    *(f"gc_cfg{n}" for n in range(1, 9)),
    *(f"map_{name}" for name in ("bes1", "bes2", "end1", "geo1", "hot1", "kam1", "kas1", "kas2", "nab1", "nab2",
                                 "rhn1", "rhn2", "tat1", "tat2", "yav1", "yav2", "unk")),
)
# load.lvl's sprites: the LOADING disk and the LEDs around it, the crosshair's
# lines and crossing (the config's frame edges), the title bar, the side
# panels' frames and the planet's backdrops.
LOAD_TEXTURES = (
    "load_rotating_disk", "load_led_0", "load_led_1", "load_led_2", "load_edge_horz", "load_edge_vert",
    "load_edge_corner", "load_title_bar_left", "load_title_bar_middle", "load_title_bar_right",
    *(f"load_side_animation_{n}" for n in range(6)), "load_planet_backdrop", "load_planet_rotating_backdrop",
)
# load.lvl's tiled patterns, kept at their size: enlarging noise smooths it.
LOAD_PATTERNS = (*(f"load_noise_{n}" for n in range(6)), "load_scanline")
SHELL_COMMON_TEXTURES = MENU_TEXTURES + SHELL_TEXTURES + SHELL_PICTURES + LOAD_TEXTURES + LOAD_PATTERNS
# What tools/upscale_hud.py enlarges, after the spawn screen's and the
# scoreboard's entries: all the sprites above, not the pictures or patterns.
SHELL_RASTER = MENU_TEXTURES + SHELL_TEXTURES + LOAD_TEXTURES


def textures_named(data, names):
    """The named textures an archive holds, decoded: name -> (width, height,
    rgba). Names match without regard to case, as the game's do; the rest
    aren't decoded."""
    wanted = {name.lower() for name in names}
    found = {}
    for start, end in iter_archives(data):
        for tag, b, e in chunks(data, start, end):
            if tag != b"tex_":
                continue
            name = child(data, b, e, b"NAME")
            name = cstr_at(data, *name) if name else None
            if not name or name.lower() not in wanted or name in found:
                continue
            parsed = decode_texture(data, b, e)
            if parsed and parsed[1]:
                found[name] = parsed[1]
    return found


def shell_textures(game=GAME):
    """Every texture of SHELL_COMMON_TEXTURES the install has."""
    paths = archives(game)
    found = {}
    for archive, names in (("common", MENU_TEXTURES), ("shell", SHELL_TEXTURES + SHELL_PICTURES),
                           ("load", LOAD_TEXTURES + LOAD_PATTERNS)):
        if paths[archive].exists():
            found.update(textures_named(paths[archive].read_bytes(), names))
    return found


def write_picture(path, width, height, rgba):
    """A PNG at full compression, without its alpha channel when every texel
    is opaque (the loading screen's pictures are DXT1)."""
    pixels = memoryview(rgba)
    alpha = bytes(pixels[3::4])
    opaque = alpha.count(255) == len(alpha)
    channels = 3 if opaque else 4
    stride = width * channels
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        row = pixels[y * width * 4:(y + 1) * width * 4]
        if opaque:
            packed = bytearray(stride)
            packed[0::3], packed[1::3], packed[2::3] = row[0::4], row[1::4], row[2::4]
            raw += packed
        else:
            raw += row

    def chunk(tag, payload):
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 2 if opaque else 6, 0, 0, 0)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
                     + chunk(b"IEND", b""))


# ---------------------------------------------------------------- text


# Text the original game draws rather than the scripts: the LOADING disk's.
EXE_STRINGS = ("common.loading",)


def script_strings(data):
    """Every string constant of every script in an archive, in order."""
    def constants(function):
        out = [s for s in function.strings if s]
        for nested in function.functions:
            out += constants(nested)
        return out
    found = []
    for name, chunk in mission_scripts(data).items():
        try:
            found += constants(lua_load(chunk))
        except (ValueError, struct.error):
            continue
    return found


def shell_strings(locl, game=GAME):
    """Every localization key the shell's and common's scripts use (as they
    spell it, in English, any trailing line break trimmed), and the
    original game's few: key -> text."""
    paths = archives(game)
    keys = []
    for archive in ("shell", "common"):
        if paths[archive].exists():
            keys += [s for s in script_strings(paths[archive].read_bytes()) if "." in s]
    keys += EXE_STRINGS
    return {key: display_text(locl, key) for key in dict.fromkeys(keys) if localize(locl, key) is not None}


# ---------------------------------------------------------------- movies

MVS = {name: fnv1a(name) for name in ("Info", "Data", "Name", "NumSegments", "SegmentInfo", "Segment", "SegmentEnd",
                                      "Length", "Padding")}
# Segment names neither the scripts nor the textures spell, found by hashing
# candidates. Each matches its entry's hash.
MOVIE_NAMES = ("main_trans", "atat_in", "coreship_in", "generic2_in", "generic3_in", "hoth_in")
# The movie config's keys (shell.lvl's mcfg).
MCFG = {fnv1a(name): name for name in ("MovieProperties", "Name", "Movie", "Inherit", "FadeInTime", "FadeOutTime",
                                       "SegmentList", "Segment", "CyclePlayback")}
MCFG_NAMED = ("Name", "Movie", "Inherit")

# Each shell screen with a movie, by its script's name: the MovieProperties
# its movieIntro and movieBackground name. Pushing the screen plays the intro
# with the background queued after it; coming back to it restarts only the
# background. Screens without a movie keep the one that plays (the options
# pages), or have their own backdrop (Instant Action, multiplayer).
SCREEN_MOVIES = {
    "ifs_start": (None, "ifs_start"),
    "ifs_login": (None, "ifs_start"),
    "ifs_main": ("ifs_main_intro", "ifs_main"),
    "ifs_sp": ("ifs_sp_intro", "ifs_sp"),
    "ifs_opt_contmain": ("ifs_opt_top_intro", "ifs_opt_top"),
    "ifs_sp_briefing": ("ifs_sp_briefing_intro", "ifs_sp_briefing"),
    "ifs_meta_top": (None, "ifs_meta"),
    "ifs_meta_new_load": ("ifs_meta_intro", "ifs_meta"),
    "ifs_meta_opts": (None, "ifs_meta"),
    "ifs_meta_load": (None, "ifs_sp"),
    "ifs_meta_main": (None, "ifs_meta_screen"),
    "ifs_split_profile": ("ifs_split_profile_intro", "ifs_split_profile"),
    "ifs_tutorials": ("ATTE_in", "ATTE_screen"),
}

# How each kind of movie is encoded: VP9 (profile 0, 4:2:0), tagged BT.601
# limited range, as the Bink decoder's output is; two passes at constant
# quality; a keyframe at least every 10 s. Every frame is kept, with its
# timing, and nothing is added: a loop's last frame runs into its first.
# One VP9 pass at speed 6: measured on the title screen's backdrop, 2.8x
# faster than two passes at speed 4 for the same size and SSIM 0.9918 against
# 0.9925 (the desktop app encodes every movie on first launch).
ENCODES = {
    # The backdrops and intros of the screens the shell always shows (800x600,
    # silent): as they are, lower CRF preserving the logo and grid edges. They
    # were stored at three times the size (Lanczos and a luma sharpen), which
    # looked the same as the source stretched and cost most of the encode;
    # the shell now upscales and sharpens them as it draws them
    # (src/shell/video-sharpen.js).
    "backdrop": {"scale": 1, "crf": 18, "speed": 6},
    # The other screens' backdrops (the campaign, Galactic Conquest,
    # splitscreen and tutorial stubs): as they are.
    "screen": {"scale": 1, "crf": 20, "speed": 6},
    # Instant Action's flyovers (720x480; Naboo: Theed's 640x480), drawn in a
    # corner: as they are.
    "flyover": {"scale": 1, "crf": 20, "speed": 6},
    # The campaigns' and Galactic Conquest's movies, with sound: as they
    # are, the sound as Opus. The front end's stubs don't play them yet, so
    # they get the fastest of these settings.
    "story": {"scale": 1, "crf": 22, "speed": 6, "audio": "160k"},
}
KEYFRAMES = 300
# The screens the shell always shows: the title gate (the profile screen's
# movie), the main menu, singleplayer and options. Encoded first.
MAIN_SCREENS = ("start_screen", "main_trans", "main", "atat_in", "atat_screen", "coreship_in", "coreship_screen")


def find_chunk(data, start, end, tag, depth=0):
    """The first chunk with this (hashed) tag in a raw ucfb tree, not
    descending into Data."""
    for t, b, e in raw_chunks(data, start, end):
        if t == tag:
            return b, e
        if depth < 3 and t != MVS["Data"]:
            found = find_chunk(data, b, e, tag, depth + 1)
            if found:
                return found
    return None


def movie_index(data):
    """shell.mvs's movies in file order: [{"hash", "offset", "length"}]."""
    info = find_chunk(data, 8, len(data), MVS["Info"])
    body = find_chunk(data, 8, len(data), MVS["Data"])
    if info is None or body is None:
        raise ValueError("no Info and Data in the movie archive")
    records, current = [], None
    pos, end = info
    while pos + 4 <= end:
        key = struct.unpack_from("<I", data, pos)[0]
        if key == MVS["SegmentInfo"]:
            pos += 4
        elif key == MVS["Segment"]:
            current = {}
            pos += 4
        elif key == MVS["SegmentEnd"]:
            records.append(current)
            current = None
            pos += 4
        elif key == 0:
            break
        else:
            if current is not None:
                current[key] = struct.unpack_from("<I", data, pos + 4)[0]
            pos += 8
    out = []
    offset = body[0]
    for record in records:
        length = record[MVS["Length"]]
        out.append({"hash": record[MVS["Name"]], "offset": offset, "length": length})
        offset += length + record.get(MVS["Padding"], 0)
    for movie in out:
        if bytes(data[movie["offset"]:movie["offset"] + 3]) != b"BIK":
            raise ValueError(f"no Bink movie at {movie['offset']:#x}")
    return out


def bink_info(data, offset):
    """A Bink header's size, frames and rate, and its audio tracks."""
    size, frames, _largest, _, width, height, rate, scale, _flags, tracks = struct.unpack_from("<10I", data, offset + 4)
    return {"width": width, "height": height, "frames": frames, "fps": [rate, scale], "audio": tracks,
            "duration": round(frames * scale / rate, 3) if rate else 0}


def movie_config(data):
    """shell.lvl's movie config: MovieProperties by name hash, with what they
    inherit filled in: {"fadeIn", "fadeOut", "segments": [[hash, loops]]}.
    A Segment's loops is -1 to loop forever, 0 to play once."""
    raw = []
    for start, end in iter_archives(data):
        for tag, b, e in chunks(data, start, end):
            if tag != b"mcfg":
                continue
            for name, values, kids in _mcfg_entries(data, b, e):
                if name == "MovieProperties":
                    raw.append(kids)
    resolved = {}
    for entries in raw:
        name = parent = None
        fades = {}
        segments = []
        cycle = False
        for key, values, kids in entries:
            if key == "Name" and values:
                name = values[0]
            elif key == "Inherit" and values:
                parent = values[0]
                # Fades set before Inherit give way to the parent's.
                fades = {}
            elif key in ("FadeInTime", "FadeOutTime") and values:
                fades[key] = round(values[0], 6)
            elif key == "SegmentList":
                segments = [[k[1][0], int(k[1][2]) if len(k[1]) > 2 else 0] for k in kids if k[0] == "Segment"]
            elif key == "CyclePlayback" and values:
                cycle = bool(values[0])
        base = resolved.get(parent, {})
        props = {"fadeIn": fades.get("FadeInTime", base.get("fadeIn", 0.0)),
                 "fadeOut": fades.get("FadeOutTime", base.get("fadeOut", 0.0)), "segments": segments}
        if cycle:
            props["cycle"] = True
        resolved[name] = props
    return resolved


def _mcfg_entries(data, start, end):
    """A config body as (name, values, children): values of the name-valued
    keys (and a Segment's first) as hashes, the rest as floats."""
    entries = []
    for tag, b, e in chunks(data, start, end):
        if tag == b"DATA":
            key = struct.unpack_from("<I", data, b)[0]
            name = MCFG.get(key, "%08x" % key)
            count = data[b + 4]
            words = [data[b + 5 + 4 * i:b + 9 + 4 * i] for i in range(count)]
            values = [struct.unpack("<I", w)[0] if name in MCFG_NAMED or (name == "Segment" and i == 0)
                      else struct.unpack("<f", w)[0] for i, w in enumerate(words)]
            entries.append([name, values, []])
        elif tag == b"SCOP" and entries:
            entries[-1][2] = _mcfg_entries(data, b, e)
    return entries


def movie_names(words):
    """Name hash -> name, for the words given and MOVIE_NAMES."""
    names = {}
    for word in list(words) + list(MOVIE_NAMES):
        names.setdefault(fnv1a(word), word)
    return names


def movie_catalog(index, config, names):
    """Each movie of the archive: name -> {hash, offset, length, file}. A
    movie is named for its segment; one whose segment name isn't known, for
    the MovieProperties that plays it (Galactic Conquest's Rebel_win...)."""
    played_by = {}
    for prop, props in config.items():
        for segment, _loops in props["segments"]:
            played_by.setdefault(segment, prop)
    out = {}
    for movie in index:
        name = names.get(movie["hash"]) or names.get(played_by.get(movie["hash"]))
        if not name:
            name = hexname(movie["hash"])
        name = name.lower()
        out[name] = {**movie, "file": f"{name}.webm"}
    return out


def movie_kind(name, info):
    if info["audio"]:
        return "story"
    if name.endswith("fly"):
        return "flyover"
    return "backdrop" if name in MAIN_SCREENS else "screen"


def encode_settings(kind, info):
    """The ffmpeg arguments for one kind of movie, and a label of them."""
    spec = ENCODES[kind]
    width, height = info["width"] * spec["scale"], info["height"] * spec["scale"]
    video = []
    if spec["scale"] != 1:
        video += ["-vf", f"scale={width}:{height}:flags=lanczos+accurate_rnd+full_chroma_int,unsharp=5:5:0.65:5:5:0"]
    video += [
        "-c:v", "libvpx-vp9", "-pix_fmt", "yuv420p", "-colorspace", "smpte170m", "-color_range", "tv",
        "-crf", str(spec["crf"]), "-b:v", "0", "-deadline", "good", "-cpu-used", str(spec["speed"]),
        "-row-mt", "1", "-tile-columns", "2" if width > 1024 else "1", "-threads", "4", "-g", str(KEYFRAMES),
        "-auto-alt-ref", "1", "-lag-in-frames", "25",
    ]
    audio = ["-c:a", "libopus", "-b:a", spec["audio"], "-ar", "48000", "-ac", "2"] if spec.get("audio") and info["audio"] else ["-an"]
    label = " ".join(video + audio)
    return video, audio, label, width, height


def transcode(name, movie, source, out_dir, kind, known=None, force=False):
    """Encode one movie, unless its WebM exists with the same settings (its
    manifest entry `known` says so). Returns its manifest entry."""
    with open(source, "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as data:
        info = bink_info(data, movie["offset"])
        video, audio, label, width, height = encode_settings(kind, info)
        target = out_dir / movie["file"]
        if not force and target.exists() and known and known.get("settings") == label and known.get("bytes") == target.stat().st_size:
            return known
        bink_bytes = data[movie["offset"]:movie["offset"] + movie["length"]]
    with tempfile.TemporaryDirectory(prefix="bf-shell-") as scratch:
        scratch = Path(scratch)
        bink = scratch / f"{name}.bik"
        bink.write_bytes(bink_bytes)
        del bink_bytes
        base = ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i", str(bink)]
        part = scratch / movie["file"]
        subprocess.run(base + video + audio + ["-f", "webm", str(part)], check=True)
        shutil.move(str(part), target)
    return {
        "file": movie["file"], "hash": hexname(movie["hash"]), "kind": kind, "width": width, "height": height,
        "frames": info["frames"], "fps": info["fps"], "duration": info["duration"], "audio": bool(info["audio"]),
        "bytes": target.stat().st_size, "settings": label,
        "source": {"offset": movie["offset"], "length": movie["length"], "width": info["width"], "height": info["height"]},
    }


def movie_sources(game=GAME):
    """The archive's movies, named, with the movie config: (catalog, config).
    The add-ons' movies (their maps' flyovers, named by their own movie
    configs) join the catalog with the archive they are in ("archive"). None
    when the install lacks the shell's."""
    paths = archives(game)
    if not (paths["movies"].exists() and paths["shell"].exists()):
        return None
    shell = paths["shell"].read_bytes()
    config = movie_config(shell)
    words = script_strings(shell)
    if paths["common"].exists():
        words += script_strings(paths["common"].read_bytes())
    added = [entry["mapluafile"].lower() for entry in zero_addon.listed_maps(game)]
    words += [name for name in textures_in(shell)] + [f"{m}fly" for m in ia_maps(shell) + added]
    with open(paths["movies"], "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as data:
        index = movie_index(data)
    catalog = movie_catalog(index, config, movie_names(words))
    for movies, add_on_shell in zero_addon.movie_archives(game, LANGUAGE):
        try:
            with open(movies, "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as data:
                index = movie_index(data)
        except (OSError, ValueError, KeyError) as error:
            print(f"  movies {movies.name}: {error}", flush=True)
            continue
        own = movie_config(add_on_shell.read_bytes()) if add_on_shell else {}
        for name, movie in movie_catalog(index, own, movie_names(words)).items():
            catalog.setdefault(name, {**movie, "archive": str(movies)})
    return catalog, config


def textures_in(data):
    """Every texture name in an archive."""
    for start, end in iter_archives(data):
        for tag, b, e in chunks(data, start, end):
            if tag == b"tex_":
                name = child(data, b, e, b"NAME")
                if name:
                    yield cstr_at(data, *name)


def write_movies(catalog, game=GAME, public=PUBLIC, jobs=2, only=None, force=False):
    """Encode the movies (the front end's first), keeping manifest.json up to
    date as each lands."""
    out = public / "common" / "shell" / "movies"
    out.mkdir(parents=True, exist_ok=True)
    # The shell's archive, or the add-on's a movie is in.
    sources = {name: Path(movie.get("archive") or archives(game)["movies"]) for name, movie in catalog.items()}
    manifest_path = out / "manifest.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
    manifest = {"version": 1, "movies": manifest.get("movies", {})}
    infos = {}
    for source in set(sources.values()):
        with open(source, "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as data:
            infos.update({name: bink_info(data, movie["offset"]) for name, movie in catalog.items() if sources[name] == source})
    rank = {"backdrop": 0, "flyover": 1, "screen": 2, "story": 3}
    order = sorted(catalog, key=lambda n: (MAIN_SCREENS.index(n) if n in MAIN_SCREENS else len(MAIN_SCREENS),
                                            rank[movie_kind(n, infos[n])], catalog[n]["offset"]))
    if only:
        order = [name for name in order if name in only]
    lock = threading.Lock()

    def job(name):
        kind = movie_kind(name, infos[name])
        with lock:
            known = manifest["movies"].get(name)
        try:
            entry = transcode(name, catalog[name], sources[name], out, kind, known, force)
        except (subprocess.CalledProcessError, OSError) as error:
            print(f"  movie {name}: {error}", flush=True)
            return
        with lock:
            manifest["movies"][name] = entry
            manifest["movies"] = dict(sorted(manifest["movies"].items(), key=lambda kv: kv[1]["source"]["offset"]))
            # Written whole, then renamed: a reader never sees half of it.
            part = manifest_path.with_suffix(".json.part")
            part.write_text(json.dumps(manifest, indent=1) + "\n")
            part.replace(manifest_path)
            if entry is not known:
                print(f"  movie {name}: {entry['width']}x{entry['height']} {entry['duration']} s"
                      f" {entry['bytes'] / 1e6:.1f} MB", flush=True)

    with ThreadPoolExecutor(max_workers=max(1, jobs)) as pool:
        list(pool.map(job, order))
    return manifest


def screen_movies(catalog, config):
    """shell.json's movies: each screen's intro and loop, and their fades."""
    by_hash = {movie["hash"]: name for name, movie in catalog.items()}

    def segment_file(prop):
        props = config.get(fnv1a(prop))
        if not props or not props["segments"]:
            return None, props
        name = by_hash.get(props["segments"][0][0])
        return (f"movies/{catalog[name]['file']}" if name else None), props

    out = {}
    for screen, (intro, background) in SCREEN_MOVIES.items():
        loop, loop_props = segment_file(background)
        if loop is None:
            continue
        entry = {"intro": None, "loop": loop, "fadeIn": loop_props["fadeIn"], "fadeOut": loop_props["fadeOut"]}
        if intro:
            file, props = segment_file(intro)
            if file:
                entry.update({"intro": file, "introFadeIn": props["fadeIn"], "introFadeOut": props["fadeOut"]})
        out[screen] = entry
    return out


# ---------------------------------------------------------------- catalog

# A playlist entry's side letter: the era, and the side that attacks.
SIDE_LETTERS = (("r", "cw", "rep"), ("c", "cw", "cis"), ("a", "gcw", "all"), ("i", "gcw", "imp"))
# Each era's two sides.
ERA_SIDES = {"cw": ("rep", "cis"), "gcw": ("all", "imp")}
# A team's side, by the faction SetTeamName's parser makes of its name.
FACTION_SIDES = {1: "all", 2: "imp", 3: "rep", 4: "cis"}


def ia_maps(shell_data):
    """Instant Action's list (sp_missionselect_listbox_contents in the
    missionlist script): map ids in order."""
    scripts = mission_scripts(shell_data)
    table = global_tables(lua_load(scripts["missionlist"])).get("sp_missionselect_listbox_contents") or []
    return [item["mapluafile"].lower() for item in table if isinstance(item, dict) and item.get("mapluafile")]


def catalog_table(table, added):
    """Instant Action's list as the game shows it: the missionlist's maps, then
    those the add-ons' addme scripts append (tools/zero_addon.py), each map
    once."""
    seen = {item["mapluafile"].lower() for item in table if isinstance(item, dict) and item.get("mapluafile")}
    out = list(table)
    for item in added:
        if item["mapluafile"].lower() not in seen:
            seen.add(item["mapluafile"].lower())
            out.append(item)
    return out


def era_entry(map_id, letter, era, attacker_side, scripts):
    """One era of a map: its two teams' sides (team 1, team 2), the team that
    attacks, and the mission. From the mission script's SetTeamName and
    SetAttackingTeam; where those don't say, the playlist's rule (team 1 is
    the side the letter names)."""
    mission = map_id + letter
    setup = mission_setup(scripts[mission]) if mission in scripts else {"teams": {}, "attacker": None}
    teams = []
    for team in ("1", "2"):
        name = (setup["teams"].get(team) or {}).get("name")
        teams.append(FACTION_SIDES.get(faction_of(name)) if name else None)
    defender = next(side for side in ERA_SIDES[era] if side != attacker_side)
    if teams[0] is None or teams[1] is None:
        teams = [attacker_side, defender]
    attacker = setup.get("attacker") or (teams.index(attacker_side) + 1 if attacker_side in teams else 1)
    return {"teams": teams, "attacker": attacker, "mission": mission}


def extracted_missions(public=PUBLIC):
    """Map id -> the missions its extraction holds (every battle fought in
    its world; an older extraction's one), for every map.json."""
    found = {}
    for path in sorted((public / "maps").glob("*/map.json")):
        world = json.loads(path.read_text())
        battles = world.get("missions")
        if battles is None:
            battles = {(world.get("mission") or {}).get("name"): world}
        found[path.parent.name] = sorted(m for m, b in battles.items() if m and not (b.get("mission") or {}).get("world"))
    return found


def map_catalog(table, scripts, locl, extracted, catalog, load):
    """shell.json's maps, in Instant Action's order."""
    out = []
    for item in table:
        map_id = item["mapluafile"].lower()
        eras = {}
        for letter, era, side in SIDE_LETTERS:
            if not item.get(f"side_{letter}") or era in eras:
                continue
            entry = era_entry(map_id, letter, era, side, scripts)
            held = extracted.get(map_id) or ()
            entry["extracted"] = entry["mission"] in ([held] if isinstance(held, str) else held)
            eras[era] = entry
        fly = catalog.get(f"{map_id}fly")
        out.append({
            "id": map_id,
            "name": display_text(locl, item["showstr"]) or item["showstr"],
            "extracted": map_id in extracted,
            "eras": eras,
            "fly": f"movies/{fly['file']}" if fly else None,
            "load": load.get(map_id),
        })
    return out


# ---------------------------------------------------------------- loading screen

# load.lvl's config keys, found by hashing candidates (each matches).
LOAD_NAMES = {fnv1a(name): name for name in (
    "LoadDisplay", "TargetTime", "AmbientColor", "SunColor", "SunDirection", "TeamModel", "Backdrop", "RotatingDisk",
    "LogoTexture", "LEDTextures", "NoiseTextures", "ScanlineTexture", "TitleBarTextures", "AnimatedTextures",
    "PlanetBackdrops", "XTrackingSound", "YTrackingSound", "ZoomSound", "TransitionSound", "BarSoundInterval",
    "BarSound", "World", "PlanetInfo", "PlanetLevel",
)}
# The crosshair's lines and crossing; this key's name isn't known.
LOAD_NAMES[0x6AE7B95F] = "EdgeTextures"


def load_config(data):
    """load.lvl's config: [(name, values, children)]."""
    for start, end in iter_archives(data):
        for tag, b, e in chunks(data, start, end):
            if tag == b"load":
                return parse_config(data, b, e, LOAD_NAMES)
    return []


def _values(entries, name):
    return next((values for key, values, _ in entries if key == name), None)


def numbers(values):
    return [int(v) if isinstance(v, float) and v.is_integer() else round(v, 6) for v in values]


def texture_url(name):
    """A common texture's URL (public/common/textures/<name>.png), which
    RasterHudAssets and the raster manifest know by its path."""
    return f"/common/textures/{safe_name(name)}.png" if name else None


def loading_screen(entries, locl):
    """shell.json's load (the screen's art, sounds and timings) and each
    map's: {map id: {planet, planetKey, planetTexture, stages}}. Sprites are
    common textures' URLs; the pictures are paths under
    public/common/shell/."""
    top = next((kids for name, _, kids in entries if name == "LoadDisplay"), [])

    def first(name):
        return (_values(top, name) or [None])[0]

    def urls(name):
        return [texture_url(value) for value in _values(top, name) or [] if isinstance(value, str)]

    def frames(name):
        # A texture prefix, the frame count, then the config's other numbers.
        values = _values(top, name) or []
        if len(values) < 2:
            return None
        return {"textures": [texture_url(f"{values[0]}{n}") for n in range(int(values[1]))], "args": numbers(values[2:])}

    scanline = _values(top, "ScanlineTexture") or []
    screen = {
        "backdrop": f"load/{first('Backdrop')}.png" if first("Backdrop") else None,
        "disk": texture_url(first("RotatingDisk")),
        "text": "common.loading",
        "leds": urls("LEDTextures"),
        "edges": dict(zip(("horizontal", "vertical", "corner"), urls("EdgeTextures"))),
        "titleBar": urls("TitleBarTextures"),
        "sideAnimation": frames("AnimatedTextures"),
        "noise": frames("NoiseTextures"),
        "scanline": {"texture": texture_url(scanline[0]), "args": numbers(scanline[1:])} if scanline else None,
        "planetBackdrops": urls("PlanetBackdrops"),
        "teams": [values[0] for key, values, _ in top if key == "TeamModel" and values],
        "light": {"ambient": numbers(_values(top, "AmbientColor") or []), "color": numbers(_values(top, "SunColor") or []),
                  "direction": numbers(_values(top, "SunDirection") or [])},
        "timing": numbers(_values(top, "TargetTime") or []),
        "sounds": {key: first(name) for key, name in (
            ("xTracking", "XTrackingSound"), ("yTracking", "YTrackingSound"), ("zoom", "ZoomSound"),
            ("transition", "TransitionSound"), ("bar", "BarSound"))},
        "barSoundInterval": numbers([first("BarSoundInterval")])[0] if first("BarSoundInterval") is not None else None,
    }
    maps = {}
    for name, values, kids in entries:
        if name != "World" or not values:
            continue
        planet = _values(kids, "PlanetInfo") or [None, None]
        stages = []
        for key, level, _ in kids:
            if key != "PlanetLevel" or len(level) < 6:
                continue
            rect = numbers(level[2:6])
            stages.append({"image": f"load/{level[1]}.png", "target": rect if any(rect) else None})
        maps[values[0]] = {
            "planet": display_text(locl, planet[0]) if planet[0] else None,
            "planetKey": planet[0],
            "planetTexture": f"load/{planet[1]}.png" if planet[1] else None,
            "stages": stages,
        }
    return screen, maps


def write_load_pictures(screen, maps, game=GAME, public=PUBLIC):
    """The loading screen's pictures: the simple backdrop, and each map's
    planet and stage pictures from Load/<map>.lvl (the galaxy, which every
    one holds, once)."""
    out = public / "common" / "shell" / "load"
    out.mkdir(parents=True, exist_ok=True)
    wanted = {}
    if screen.get("backdrop"):
        wanted.setdefault(archives(game)["load"], set()).add(Path(screen["backdrop"]).stem)
    for map_id, entry in maps.items():
        names = {Path(stage["image"]).stem for stage in entry["stages"]}
        if entry["planetTexture"]:
            names.add(Path(entry["planetTexture"]).stem)
        wanted.setdefault(game / "Load" / f"{map_id}.lvl", set()).update(names)
    written = []
    for archive, names in wanted.items():
        missing = {name for name in names if not (out / f"{name}.png").exists()}
        if not missing or not archive.exists():
            continue
        for name, (width, height, rgba) in textures_named(archive.read_bytes(), missing).items():
            write_picture(out / f"{name}.png", width, height, rgba)
            written.append(name)
    return written


# ---------------------------------------------------------------- audio

# The shell's music, the stream every SoundStreamProperties on shellmusic
# plays from, and the property the shell starts with: it cycles five of the
# six segments, in turn, ten seconds apart (SetShellMusicInterval).
MUSIC_STREAM = "shell_music"
SOUNDTRACK = "shell_soundtrack"
MUSIC_PROPERTIES = ("shell_soundtrack", "shell_galacticconquest", "shell_droidinvasion", "shell_clonewars",
                    "shell_imperialmarch", "shell_clash")
# A forward transition's whoosh: shell_transition1 to 10, in turn.
WHOOSHES = tuple(f"shell_transition{n}" for n in range(1, 11))
OPUS_RATE = "256k"


def sound_objects(game=GAME):
    """core.lvl's sound objects (the buses, the interface sounds) and
    Sound/shell.lvl's (the music's and whooshes' properties), in that
    order, with the shell archive's contents."""
    paths = archives(game)
    core = paths["core"].read_bytes()
    configs = []
    for start, end in iter_archives(core):
        for tag, b, e in chunks(core, start, end):
            if tag in (b"snd_", b"mus_", b"ffx_"):
                configs.append((tag.decode(), b, e))
    sound = paths["sound"].read_bytes()
    contents = archive_contents(sound)
    return core, config_objects(core, configs), sound, contents, config_objects(sound, contents["configs"])


def music_tracks(objects, stream):
    """The music's segments: the soundtrack's, in its cycle order, then the
    rest in the stream's. Each is named for the most particular property
    that plays it (shell_soundtrack_<n> when the soundtrack alone does)."""
    names = {fnv1a(name): name for name in MUSIC_PROPERTIES}
    plays = {}
    for obj in objects:
        if obj.get("type") != "SoundStreamProperties" or obj.get("Stream") != hexname(fnv1a(MUSIC_STREAM)):
            continue
        prop = names.get(int(obj["Name"], 16))
        if not prop:
            continue
        for segment, *_ in obj.get("SegmentList") or []:
            plays.setdefault(int(segment, 16), []).append(prop)
    soundtrack = next((o for o in objects if o.get("Name") == hexname(fnv1a(SOUNDTRACK))), {})
    order = [int(s[0], 16) for s in soundtrack.get("SegmentList") or []]
    order += [s["id"] for s in stream["items"] if s["id"] not in order]
    tracks = []
    for segment in order:
        item = next((s for s in stream["items"] if s["id"] == segment), None)
        if item is None:
            continue
        props = plays.get(segment, [])
        particular = [p for p in props if p != SOUNDTRACK]
        name = particular[-1] if particular else f"{SOUNDTRACK}_{order.index(segment) + 1}"
        tracks.append({"name": name, "segment": hexname(segment), "properties": props,
                       "duration": round(item["frames"] / item["rate"], 3), "item": item})
    return tracks


def encode_audio(path, pcm, rate, channels, codec):
    """16-bit PCM to Ogg Opus or FLAC through ffmpeg."""
    args = ["-c:a", "libopus", "-b:a", OPUS_RATE, "-vbr", "on", "-application", "audio"] if codec == "opus" \
        else ["-c:a", "flac", "-compression_level", "8"]
    subprocess.run(["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-f", "s16le", "-ar", str(rate),
                    "-ac", str(channels), "-i", "pipe:0", *args, str(path)], input=bytes(pcm), check=True)


def interleave(channels):
    frames = min(len(c) for c in channels)
    out = array("h", bytes(2 * frames * len(channels)))
    for i, channel in enumerate(channels):
        out[i::len(channels)] = channel[:frames]
    return out


def resample(pcm, rate, pitch, target):
    """Mono 16-bit PCM played at `pitch`, resampled to `target` Hz (soxr)."""
    played = round(rate * pitch)
    run = subprocess.run(["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error", "-f", "s16le", "-ar", str(played),
                          "-ac", "1", "-i", "pipe:0", "-af", f"aresample={target}:resampler=soxr:precision=28",
                          "-f", "s16le", "-ac", "1", "pipe:1"], input=bytes(pcm), capture_output=True, check=True)
    out = array("h")
    out.frombytes(run.stdout)
    return out


def bank_samples(game=GAME):
    """Every sample in common.bnk and core.lvl's bank: id -> (bank, item,
    archive bytes)."""
    paths = archives(game)
    found = {}
    bnk = paths["bank"].read_bytes()
    for bank in archive_contents(bnk)["banks"]:
        for item in bank["items"]:
            found[item["id"]] = (bank, item, bnk)
    core = paths["core"].read_bytes()
    for bank in archive_contents_flat(core):
        for item in bank["items"]:
            found.setdefault(item["id"], (bank, item, core))
    return found


def sample_pcm(samples, sid):
    """A sample's 16-bit PCM and rate, following an alias."""
    bank, item, source = samples[sid]
    rate = item["rate"]
    if "alias" in item:
        bank, item, source = samples[item["alias"]]
    start = bank["data"][0] + item["offset"]
    pcm = array("h")
    pcm.frombytes(source[start:start + item["size"] // 2 * 2])
    return pcm, rate


def resolved(objects, name):
    """A SoundProperties with what it inherits (the parent's keys, then its own)."""
    by_name = {o.get("Name"): o for o in objects}
    obj = by_name.get(hexname(fnv1a(name)) if not name.startswith("0x") else name)
    if obj is None:
        return None
    chain = [obj]
    while chain[-1].get("Inherit") in by_name and len(chain) < 8:
        chain.append(by_name[chain[-1]["Inherit"]])
    out = {}
    for parent in reversed(chain):
        out.update({k: v for k, v in parent.items() if k not in ("Name", "Inherit")})
    return out


def render_whoosh(objects, samples, name, rate=44100):
    """A transition whoosh (a SoundLayered of two mono samples, one panned
    hard left at pitch 0.99, one hard right) as stereo PCM: each layer at
    its Pitch and Pan, from the start. Pitch and start each play rolls
    (PitchDev 0.1, a layer delay of up to 0.5 s) aren't baked in."""
    layered = next((o for o in objects if o.get("type") == "SoundLayered" and o.get("Name") == hexname(fnv1a(name))), None)
    if layered is None:
        return None
    sides = {}
    for layer in layered.get("Layer") or []:
        props = resolved(objects, layer[1])
        if not props or not props.get("SampleList"):
            continue
        pcm, source_rate = sample_pcm(samples, int(props["SampleList"][0][0], 16))
        channel = "left" if props.get("Pan", 0) < 0 else "right"
        sides[channel] = resample(pcm, source_rate, props.get("Pitch", 1.0), rate)
    if set(sides) != {"left", "right"}:
        return None
    frames = max(len(c) for c in sides.values())
    for channel in sides.values():
        channel.extend([0] * (frames - len(channel)))
    return interleave([sides["left"], sides["right"]])


def write_shell_audio(game=GAME, public=PUBLIC):
    """The music as Ogg Opus, the whooshes as FLAC, and the shell's sound
    sub-level ("shell": core.lvl's and Sound/shell.lvl's objects, their
    samples) for the audio engine's SoundData. Returns shell.json's music and
    whooshes."""
    out = public / "audio" / "shell"
    out.mkdir(parents=True, exist_ok=True)
    core, core_objects, sound, contents, shell_objects = sound_objects(game)
    objects = core_objects + shell_objects
    stream = next((s for s in contents["streams"] if s["head"].get("Name") == fnv1a(MUSIC_STREAM)), None)
    music = []
    if stream:
        substreams = stream_substreams(stream, sound)
        for track in music_tracks(shell_objects, stream):
            file = f"{track['name']}.opus"
            item = track.pop("item")
            if not (out / file).exists():
                channels = decode_segment(stream, item, substreams)
                encode_audio(out / file, interleave(channels), item["rate"], len(channels), "opus")
            music.append({"name": track["name"], "file": f"/audio/shell/{file}", **track})
            print(f"  music {track['name']}: {track['duration']} s", flush=True)
    samples = bank_samples(game)
    whooshes = []
    for name in WHOOSHES:
        file = f"{name}.flac"
        if not (out / file).exists():
            pcm = render_whoosh(shell_objects, samples, name)
            if pcm is None:
                print(f"  whoosh {name}: not found", flush=True)
                continue
            encode_audio(out / file, pcm, 44100, 2, "flac")
        whooshes.append(f"/audio/shell/{file}")
    write_sound_sublevel(out, objects, contents, samples, core)
    return music, whooshes


def write_sound_sublevel(out, objects, contents, samples, core):
    """sound.json and bank.bin as tools/zero_sound.py writes a battle's, for
    SoundData.load("/audio", "shell"): the objects, and the samples of the
    shell's bank and core.lvl's (16-bit PCM, mono). No streams: the music
    plays from its own files."""
    wanted = [item["id"] for bank in contents["banks"] for item in bank["items"]]
    wanted += [item["id"] for bank in archive_contents_flat(core) for item in bank["items"]]
    pcm = bytearray()
    index = {}
    for sid in wanted:
        if hexname(sid) in index or sid not in samples:
            continue
        data, rate = sample_pcm(samples, sid)
        index[hexname(sid)] = [len(pcm), len(data), rate]
        pcm += data.tobytes()
        if len(pcm) % 4:
            pcm += b"\x00" * (4 - len(pcm) % 4)
    words = list(MUSIC_PROPERTIES) + list(WHOOSHES) + [MUSIC_STREAM, "shell_vo"]
    names = {hexname(fnv1a(w)): w for w in words}
    manifest = {"sublevel": "shell", "map": None, "sides": [], "objects": objects, "samples": index, "streams": [],
                "mission": {}, "classes": {}, "world": {"regions": [], "emitters": [], "posts": {}, "materials": []},
                "names": names}
    (out / "bank.bin").write_bytes(bytes(pcm))
    (out / "sound.json").write_text(json.dumps(manifest))
    print(f"  sound: {len(objects)} objects, {len(index)} samples ({len(pcm) / 1e6:.1f} MB)", flush=True)


# ---------------------------------------------------------------- the parts


def refresh_common(locl, game=GAME, public=PUBLIC):
    """Add the menus' textures, strings and rasterHud entries to an extracted
    common.json, writing only the new PNGs."""
    from extract import write_textures
    out = public / "common"
    path = out / "common.json"
    if not path.exists():
        print("common: no common.json; tools/extract.py writes it", flush=True)
        return None
    common = json.loads(path.read_text())
    textures = shell_textures(game)
    added = write_textures(textures, set(textures) - set(common["textures"]), out / "textures")
    common["textures"] = dict(sorted({**common["textures"], **added}.items()))
    strings = shell_strings(locl, game)
    common["strings"] = {**common.get("strings", {}), **strings}
    raster = list(common.get("rasterHud", []))
    raster += [name for name in SHELL_RASTER if name in common["textures"] and name not in raster]
    common["rasterHud"] = raster
    path.write_text(json.dumps(common))
    missing = sorted(set(SHELL_COMMON_TEXTURES) - set(common["textures"]))
    print(f"common: textures {len(added)} added{f', missing {missing}' if missing else ''},"
          f" strings {len(strings)}, rasterHud {len(raster)}", flush=True)
    return common


def instant_action_table(shell, game=GAME):
    """Instant Action's list, the add-ons' maps after the game's
    (catalog_table), from shell.lvl's bytes."""
    table = global_tables(lua_load(mission_scripts(shell)["missionlist"])).get("sp_missionselect_listbox_contents") or []
    return catalog_table(table, zero_addon.listed_maps(game))


def write_catalog(locl, game=GAME, public=PUBLIC, music=None, whooshes=None):
    """shell.json, and the loading screen's pictures. Keeps the music and
    whooshes already there unless given."""
    paths = archives(game)
    shell = paths["shell"].read_bytes()
    out = public / "common" / "shell"
    out.mkdir(parents=True, exist_ok=True)
    path = out / "shell.json"
    old = json.loads(path.read_text()) if path.exists() else {}
    sources = movie_sources(game)
    catalog, config = sources if sources else ({}, {})
    screen, load = loading_screen(load_config(paths["load"].read_bytes()), locl) if paths["load"].exists() else ({}, {})
    written = write_load_pictures(screen, load, game, public)
    scripts = zero_addon.missions(game)
    table = instant_action_table(shell, game)
    maps = map_catalog(table, scripts, locl, extracted_missions(public), catalog, load)
    data = {
        "movies": screen_movies(catalog, config),
        "maps": maps,
        "music": music if music is not None else old.get("music", []),
        "whooshes": whooshes if whooshes is not None else old.get("whooshes", []),
        "load": screen,
    }
    path.write_text(json.dumps(data, indent=1) + "\n")
    playable = [f"{m['id']}:{','.join(e for e, v in m['eras'].items() if v['extracted'])}" for m in maps if m["extracted"]]
    print(f"shell.json: {len(data['movies'])} screens, {len(maps)} maps (extracted {playable}),"
          f" {len(data['music'])} music, {len(data['whooshes'])} whooshes; load pictures {len(written)} written", flush=True)
    return data


def finish_common(public=PUBLIC):
    """KTX2 copies of the new common textures, then the 4x WebP atlases."""
    from zero_spawn_screen import finish_common as finish
    finish(public)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--common", action="store_true", help="common.json: the menus' textures, strings and rasterHud")
    parser.add_argument("--audio", action="store_true", help="public/audio/shell: music, whooshes, interface sounds")
    parser.add_argument("--catalog", action="store_true", help="shell.json and the loading screen's pictures")
    parser.add_argument("--movies", action="store_true", help="transcode Movies/shell.mvs")
    parser.add_argument("--jobs", type=int, default=2, help="movies encoded at once (each uses four threads)")
    parser.add_argument("--only", nargs="*", help="only these movies (by file name without .webm)")
    parser.add_argument("--force", action="store_true", help="encode movies again even if up to date")
    args = parser.parse_args()
    every = not (args.common or args.audio or args.catalog or args.movies)
    paths = archives()
    missing = [str(p) for p in paths.values() if not p.exists()]
    if missing:
        print(f"missing from the install: {missing}", file=sys.stderr)
        return 1
    # The game's text and what the add-ons add (an add-on map's name, if its
    # list entry names a key).
    locl = zero_addon.localization()
    if every or args.common:
        if refresh_common(locl) is not None:
            finish_common()
    music = whooshes = None
    if every or args.audio:
        music, whooshes = write_shell_audio()
    if every or args.catalog or args.audio:
        write_catalog(locl, music=music, whooshes=whooshes)
    if every or args.movies:
        catalog, _ = movie_sources()
        write_movies(catalog, jobs=args.jobs, only=set(args.only) if args.only else None, force=args.force)
    return 0


if __name__ == "__main__":
    sys.exit(main())
