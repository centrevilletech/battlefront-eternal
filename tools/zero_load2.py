#!/usr/bin/env python3
"""The 2005 game's loading screen: its `load` config, the per-map picture
sets and the tips, written as prepared data.

    BFE_SWBF2_DIR=<install> python3 tools/zero_load2.py [--out DIR]

BFE_SWBF2_DIR (or --game) is the install folder holding GameData
(tools/zero_hud2.lvl_pc_dir). Writes <out>/common/shell/load2.json (the
screen's art, lights and timings, each world's display name
(`mapname.name.<world>` in the text tables) and picture set, and the
tips with their text) and the pictures as PNGs under
<out>/common/shell/load/, decoded like the 2004 shell art
(tools/zero_texture.py, tools/zero_shell.write_picture; KTX2 copies are
tools/ktx2.py's step, as for every texture). The 2005 preparation
profile should call `write_loading_screen` where the 2004 one calls
zero_shell's catalog step; nothing here touches 2004 output.

The config is the single `load` chunk in load/load.lvl, a config chunk
of the shape tools/zero_hud2.py reads strictly (docs/
formats-2005-interface.md): a LoadDisplay section (the tips box, the
LED progress bar, the loading text, the lighting, the defaults), then
one `map` section per world naming its backdrop archive under load/
(RandomBackdrop: load/<stem>.lvl, whose tex_ pictures are the world's
set) and its ProgressBarTotalTime in seconds. Campaign worlds add
CampaignLayout and a CampaignName text key, and the Galactic Conquest
worlds override TipsPrefix. Tips are the localization keys
"<TipsPrefix>.1", ".2", ... in core.lvl (tools/zero_locl.py), read
until a number is missing.

Two values are stored raw because no name is known for them: the second
value of TipsTime and of ProgressBarLEDMinIntensity (one shared name
hash). They ride along as {"bits", "name"} words, as zero_hud2 emits
them. The PC screen selects one backdrop per load. TipsTime controls tip
changes. The per-map ProgressBarTotalTime sets the fixed length.
"""

import argparse
import hashlib
import math
import json
import re
import struct
from dataclasses import dataclass
from pathlib import Path

from zero_hash import fnv1a
from zero_hud2 import (ArchiveBudget, ConfigError, archive_nodes, archive_path, atomic_output, checked_chunks,
                       config_chunks, lvl_pc_dir, read_archive)
from zero_texture import A8R8G8B8, X8R8G8B8, DECODABLE, DXT1, DXT3, DXT5, decode_pixels, safe_name

from zero_paths import PUBLIC, write_text_lf  # noqa: E402


def _values(entries, name):
    return next((entry["values"] for entry in entries if entry["name"] == name), None)


def _first(entries, name, default=None):
    values = _values(entries, name)
    return values[0] if values else default


def tip_keys(prefix, table, limit=512):
    """The tip keys one TipsPrefix names in a localization table
    (hash -> text): "<prefix>.1" upward until a gap."""
    keys = []
    for number in range(1, limit + 1):
        key = f"{prefix}.{number}"
        if fnv1a(key) not in table:
            break
        keys.append(key)
    if len(keys) == limit and fnv1a(f"{prefix}.{limit + 1}") in table:
        raise ConfigError("too many tips in a group")
    return keys


def load_config(lvl_pc):
    """load/load.lvl's `load` chunk, parsed strictly (its NAME hashes to
    "load"). Missing files and corrupt configs fail preparation."""
    path = archive_path(lvl_pc, "load", "load.lvl")
    found = config_chunks(read_archive(path), b"load")
    if len(found) != 1 or found[0]["file"] != "load":
        raise ConfigError("expected one named load config")
    return found[0]


def _natural(name):
    """Sort key putting spa_2 before spa_10 (numbers as numbers)."""
    return [int(part) if part.isdigit() else part for part in re.split(r"(\d+)", name)]


MAX_EDGE = 4096
MAX_PIXELS = 2048 * 2048
MAX_LEDS = 64
MAX_TIP_UNITS = 1024
MAX_MANIFEST_BYTES = 4 << 20


@dataclass
class Picture:
    name: str
    fmt: int
    width: int
    height: int
    start: int
    end: int


def _parts(data, b, e, budget):
    result = {}
    for tag, start, end in checked_chunks(data, b, e, budget):
        if tag in result:
            raise ConfigError(f"duplicate {tag!r} in texture")
        result[tag] = (start, end)
    return result


def _picture(data, b, e, budget):
    """Read the selected 2D format and validate its top mip without decoding it."""
    name = None
    best = None
    rank = len(DECODABLE)
    for tag, start, end in checked_chunks(data, b, e, budget):
        if tag == b"NAME":
            if name is not None or not 2 <= end - start <= 128 or data[end - 1] != 0:
                raise ConfigError("invalid loading picture name")
            name = data[start:end - 1].decode("latin1")
            if "\0" in name:
                raise ConfigError("invalid loading picture name")
        if tag != b"FMT_":
            continue
        parts = _parts(data, start, end, budget)
        info = parts.get(b"INFO")
        if not info or info[1] - info[0] < 16 or b"FACE" not in parts:
            raise ConfigError("short texture format")
        fmt, w, h, _, _, kind = struct.unpack_from("<IHHHHI", data, info[0])
        if kind & 255 != 1 or fmt not in DECODABLE or DECODABLE.index(fmt) >= rank:
            continue
        if not (0 < w <= MAX_EDGE and 0 < h <= MAX_EDGE and w * h <= MAX_PIXELS):
            raise ConfigError("loading picture exceeds pixel budget")
        start, end = parts[b"FACE"]
        if data[start:start + 4] == b"LVL_":
            levels = list(checked_chunks(data, start, end, budget))
            if not levels or any(tag != b"LVL_" for tag, _, _ in levels):
                raise ConfigError("invalid texture mip chain")
            mip = _parts(data, levels[0][1], levels[0][2], budget)
            if b"BODY" not in mip:
                raise ConfigError("missing top mip")
            start, end = mip[b"BODY"]
        if fmt in (DXT1, DXT3, DXT5):
            required = ((w + 3) // 4) * ((h + 3) // 4) * (8 if fmt == DXT1 else 16)
        else:
            required = w * h * (4 if fmt in (A8R8G8B8, X8R8G8B8) else 2)
        if end - start != required:
            raise ConfigError("incomplete texture mip")
        best = (fmt, w, h, start, end)
        rank = DECODABLE.index(fmt)
    if not name or best is None:
        raise ConfigError("unreadable loading picture")
    return Picture(name, *best)


@dataclass
class PictureArchive:
    source: Path
    digest: str
    paths: dict
    pictures: dict


def picture_sets(lvl_pc):
    """Index texture names without decoding the pixels. The archive digest
    keeps a previously published manifest valid while a new one is prepared."""
    sets = {}
    destinations = set()
    archives = sorted((p for p in archive_path(lvl_pc, "load").iterdir() if p.suffix.casefold() == ".lvl"),
                      key=lambda p: (p.name.casefold(), p.name))
    if len(archives) > 512 or sum(path.stat().st_size for path in archives) > 2 << 30:
        raise ConfigError("too many loading archives or bytes")
    decoded_bytes = 0
    for path in archives:
        stem = path.stem.lower()
        folder = safe_name(_identifier(stem))
        if folder.casefold() in destinations or not folder:
            raise ConfigError(f"colliding archive name: {stem}")
        destinations.add(folder.casefold())
        data = read_archive(path)
        digest = hashlib.sha256(data).hexdigest()
        names = {}
        pictures = {}
        budget = ArchiveBudget()
        seen = set()
        for tag, b, e in archive_nodes(data, budget):
            if tag != b"tex_":
                continue
            pic = _picture(data, b, e, budget)
            name = pic.name
            decoded_bytes += pic.width * pic.height * 4
            if len(names) >= 256 or decoded_bytes > 1 << 30:
                raise ConfigError("loading pictures exceed archive budget")
            filename = safe_name(name)
            if not filename or filename.casefold() in seen:
                raise ConfigError(f"colliding texture name in {path.name}: {name}")
            seen.add(filename.casefold())
            names[name] = f"load/{folder}/{digest}/{filename}.png"
            pictures[name] = pic
        ordered = {n: names[n] for n in sorted(names, key=lambda n: (_natural(n), n))}
        sets[stem] = PictureArchive(path, digest, ordered, pictures)
    return sets


def _backdrop_stem(value):
    """'load\\\\tat2' -> 'tat2' (the archive under load/, lowercased)."""
    if value is None:
        return None
    return _identifier(value).replace("/", "\\").rsplit("\\", 1)[-1].lower() or None


TIP_WIDTH = 400 - 2 * 18  # text area with the minimum panel insets


def _locale_table(data, start, end):
    table = {}
    while start < end:
        if end - start <= 8 and not any(data[start:end]):
            break
        if end - start < 6:
            raise ConfigError("short localization record")
        key, size = struct.unpack_from("<IH", data, start)
        if size < 8 or size % 2 or start + size > end or data[start + size - 2:start + size] != b"\0\0":
            raise ConfigError("invalid localization record size or terminator")
        if len(table) >= 65536:
            raise ConfigError("too many localized strings")
        table[key] = data[start + 6:start + size].decode("utf-16-le").rstrip("\0")
        start += size
    return table


def _language_data(data):
    """Locate each language once, and read just the small font's advance table."""
    locales = {}
    advances = None
    budget = ArchiveBudget()
    for tag, start, end in archive_nodes(data, budget):
        if tag not in (b"Locl", b"font"):
            continue
        parts = {}
        for key, b, e in checked_chunks(data, start, end, budget):
            if key in parts:
                raise ConfigError(f"duplicate {key!r} in {tag!r}")
            parts[key] = (b, e)
        if b"NAME" not in parts:
            raise ConfigError(f"missing name in {tag!r}")
        b, e = parts[b"NAME"]
        name = data[b:e].rstrip(b"\0").decode("latin1").lower()
        if tag == b"Locl":
            if not name or name in locales or b"BODY" not in parts:
                raise ConfigError("missing or duplicate localization table")
            if len(locales) >= 32 or len(name) > 64:
                raise ConfigError("too many languages or oversized language name")
            locales[name] = parts[b"BODY"]
        elif name == "gamefont_small":
            if b"FBOD" not in parts or b"HEAD" not in parts:
                raise ConfigError("missing small font metrics")
            b, e = parts[b"HEAD"]
            if e - b < 5:
                raise ConfigError("short font header")
            count = struct.unpack_from("<H", data, b)[0]
            b, e = parts[b"FBOD"]
            if e - b != count * 24:
                raise ConfigError("invalid small font glyph count")
            advances = {struct.unpack_from("<H", data, i)[0]: data[i + 3] for i in range(b, e, 24)}
    if advances is None or not locales:
        raise ConfigError("missing loading font or localization")
    return locales, advances


def tip_line_ends(text, advances, width=TIP_WIDTH):
    """Exclusive UTF-8 offsets in normalized text. Split long unbroken words too."""
    ends = []
    offset = used = 0
    for row in text.splitlines(keepends=True):
        words = row.split()
        used = 0
        for i, word in enumerate(words):
            if i:
                offset += 1
                space = advances.get(32, 0)
                if used + space + sum(advances.get(ord(c), 0) for c in word) > width:
                    ends.append(offset)
                    used = 0
                else:
                    used += space
            for char in word:
                advance = advances.get(ord(char), 0)
                if used and used + advance > width:
                    ends.append(offset)
                    used = 0
                used += advance
                offset += len(char.encode("utf-8"))
        offset += int(row.endswith("\n"))
        ends.append(offset)
    return ends


def _number(value, lower, upper, field):
    if type(value) not in (int, float) or not math.isfinite(value) or not lower <= value <= upper:
        raise ConfigError(f"invalid {field}")
    return value


def _identifier(value):
    if not isinstance(value, str) or not 1 <= len(value.encode("utf-8")) <= 128:
        raise ConfigError("invalid interface identifier")
    return value


def _ticks(seconds):
    if type(seconds) not in (int, float) or not math.isfinite(seconds) or not 0 < seconds <= 600:
        raise ConfigError("expected a loading duration in (0, 600] seconds")
    return math.ceil(seconds * 60)


def _language_key(name):
    tag = name.casefold().replace("_", "-")
    if tag in ("uk-english", "en-gb"):
        return "uk-english"
    base, sep, region = tag.partition("-")
    base = {"de": "german", "en": "english", "es": "spanish",
            "fr": "french", "it": "italian"}.get(base, base)
    return base + sep + region


def _default_language(requested, locales):
    requested = requested.casefold().replace("_", "-")
    tag = _language_key(requested)
    base = "english" if tag == "uk-english" else requested.partition("-")[0]
    base_tag = _language_key(base)

    def rank(name):
        literal = name.casefold().replace("_", "-")
        canonical = _language_key(literal)
        matches = (literal == requested, canonical == tag, literal == base, canonical == base_tag,
                   literal == "english", canonical == "english", True)
        return (matches.index(True), name)

    return min(locales, key=rank)


def _loading_screen(lvl_pc, language, sets):
    """load2.json's contents: {"display": ..., "maps": {...}}.
    `display` is LoadDisplay (tips box, LED bar, text, lighting, defaults,
    and shared defaults). Languages contain the localized tip groups. Each map names its backdrop
    archive stem, its pictures (prepared paths), its total time and its
    tips prefix. Colours stay in the stored 0..255 range."""
    chunk = load_config(lvl_pc)
    entries = chunk["entries"]

    displays = [entry.get("children") for entry in entries if entry["name"] == "LoadDisplay"]
    if len(displays) != 1 or not displays[0]:
        raise ConfigError("expected one LoadDisplay scope")
    top = displays[0]
    palette_entry = next((entry for entry in top if entry["name"] == "LoadingTextColorPallete"), None)
    tips_box = _values(top, "TipsBoxTexture") or []
    if tips_box and len(tips_box) != 3:
        raise ConfigError("expected a panel texture and two border sizes")
    border = [_number(value, 0, 64, "tip border") for value in tips_box[1:]]
    wrap_width = 400 - 2 * max(18, border[0] + 6) if border else TIP_WIDTH
    prefix = _identifier(_first(top, "TipsPrefix"))
    def display_texture(name):
        if name is None:
            return None
        if _identifier(name) not in sets["load"].paths:
            raise ConfigError(f"missing loading texture: {name}")
        return sets["load"].paths[name]

    display = {
        "loadingTextBlinkRate": _first(top, "LoadingTextBlinkRate"),
        "loadingTextColors": [kid["values"] for kid in palette_entry.get("children", [])
                              if kid["name"] == "color"] if palette_entry else [],
        "tipsBox": {"texture": display_texture(tips_box[0]) if tips_box else None,
                    "border": border},
        "tipsColor": _values(top, "TipsColor") or [],
        "tipsPrefix": prefix,
        "tipsTime": _values(top, "TipsTime") or [],
        "progressBar": {
            "ledTexture": display_texture(_first(top, "ProgressBarLEDTexture")),
            "numLeds": _first(top, "ProgressBarNumLEDs"),
            "ledWidth": _first(top, "ProgressBarLEDWidth"),
            "ledHeight": _first(top, "ProgressBarLEDHeight"),
            "ledSpacing": _first(top, "ProgressBarLEDSpacing"),
            "ledColor": _values(top, "ProgressBarLEDColor") or [],
            "ledMinIntensity": _values(top, "ProgressBarLEDMinIntensity") or [],
            "totalTime": _first(top, "ProgressBarTotalTime"),
        },
        "backdrop": _backdrop_stem(_first(top, "RandomBackdrop")),
        "light": {"ambient": _values(top, "AmbientColor") or [],
                  "sunColor": _values(top, "SunColor") or [],
                  "sunDirection": _values(top, "SunDirection") or []},
    }

    display["tipTicks"] = _ticks(_first(top, "TipsTime"))
    bar = display["progressBar"]
    count = _number(bar["numLeds"], 1, MAX_LEDS, "LED count")
    if int(count) != count:
        raise ConfigError("fractional LED count")
    bar["numLeds"] = int(count)
    _number(bar["ledWidth"], 0.25, 256, "LED width")
    _number(bar["ledHeight"], 0.25, 48, "LED height")
    _number(bar["ledSpacing"], 0, 4, "LED spacing")
    if not bar["ledMinIntensity"]:
        raise ConfigError("missing LED intensity")
    _number(bar["ledMinIntensity"][0], 0, 1, "LED intensity")
    if count * bar["ledWidth"] + (count - 1) * bar["ledSpacing"] > 720:
        raise ConfigError("LED bar exceeds the loading area")
    _number(display["loadingTextBlinkRate"], 0, 60, "blink rate")
    maps = {}
    for entry in entries:
        if entry["name"] != "map":
            continue
        if len(entry["values"]) != 1 or not isinstance(entry["values"][0], str):
            raise ConfigError("map needs one world name")
        if len(maps) >= 512:
            raise ConfigError("too many loading worlds")
        if _identifier(entry["values"][0]) in maps:
            raise ConfigError("duplicate loading world")
        kids = entry.get("children", [])
        stem = _backdrop_stem(_first(kids, "RandomBackdrop")) or display["backdrop"]
        name_key = f"mapname.name.{entry['values'][0]}"
        duration = _first(kids, "ProgressBarTotalTime", display["progressBar"]["totalTime"])
        maps[entry["values"][0]] = {
            "nameKey": name_key,
            "backdrop": stem,
            "pictures": list(sets[stem].paths.values()) if stem in sets else [],
            "durationTicks": _ticks(duration),
            "tipsPrefix": _identifier(_first(kids, "TipsPrefix", prefix)),
            "campaignLayout": _first(kids, "CampaignLayout"),
            "campaignName": _first(kids, "CampaignName"),
        }
    if not maps:
        raise ConfigError("load config contains no worlds")
    core = read_archive(archive_path(lvl_pc, "core.lvl"))
    locales, advances = _language_data(core)
    language = _default_language(language, locales)
    prefixes = sorted({world["tipsPrefix"] for world in maps.values() if world["tipsPrefix"]})
    languages = {}
    max_lines = 0
    string_bytes = 0
    for locale, (b, e) in locales.items():
        table = _locale_table(core, b, e)
        tips = {}
        for prefix in prefixes:
            group = []
            for key in tip_keys(prefix, table):
                text = "\n".join(" ".join(row.split()) for row in table[fnv1a(key)].strip().splitlines())
                string_bytes += len(text.encode("utf-8"))
                if len(text.encode("utf-16-le")) > 2 * MAX_TIP_UNITS or string_bytes > MAX_MANIFEST_BYTES:
                    raise ConfigError("loading tips exceed text budget")
                ends = tip_line_ends(text, advances, wrap_width)
                if len(ends) > 64:
                    raise ConfigError("too many lines in loading tip")
                max_lines = max(max_lines, len(ends))
                group.append({"key": key, "text": text, "lineEnds": ends})
            tips[prefix] = group
        titles = {key: table[fnv1a(world["nameKey"])] for key, world in maps.items()
                  if fnv1a(world["nameKey"]) in table}
        label = table.get(fnv1a("common.loading"))
        if any(len(value.encode("utf-16-le")) > 320 for value in [*titles.values(), label or ""]):
            raise ConfigError("oversized loading label")
        languages[locale] = {"tips": tips, "titles": titles, "label": label}
    display["maxTipLines"] = max_lines
    fallback = sets.get(display["backdrop"])
    display["fallbackPicture"] = next(iter(fallback.paths.values()), None) if fallback else None
    return {"display": display, "maps": maps, "language": language, "languages": languages}


def loading_screen(lvl_pc, language="english"):
    return _loading_screen(lvl_pc, language, picture_sets(lvl_pc))


def write_loading_screen(lvl_pc, out, language="english"):
    """Publish complete source-qualified pictures before replacing load2.json."""
    from zero_shell import write_picture

    sets = picture_sets(lvl_pc)
    screen = _loading_screen(lvl_pc, language, sets)
    display = screen["display"]
    wanted = {path for world in screen["maps"].values() for path in world["pictures"]}
    wanted.update([display["fallbackPicture"], display["tipsBox"]["texture"], display["progressBar"]["ledTexture"]])
    wanted.discard(None)
    payload = json.dumps(screen, indent=1, allow_nan=False) + "\n"
    if len(payload.encode("utf-8")) > MAX_MANIFEST_BYTES:
        raise ConfigError("loading manifest exceeds byte budget")
    written = []
    for archive in sets.values():
        if wanted.isdisjoint(archive.paths.values()):
            continue
        data = read_archive(archive.source)
        if hashlib.sha256(data).hexdigest() != archive.digest:
            raise ConfigError(f"loading archive changed during preparation: {archive.source.name}")
        for name, rel in archive.paths.items():
            if rel not in wanted:
                continue
            pic = archive.pictures[name]
            rgba = decode_pixels(pic.fmt, memoryview(data)[pic.start:pic.end], pic.width, pic.height)
            with atomic_output(out, Path("common/shell") / rel) as path:
                write_picture(path, pic.width, pic.height, rgba)
            written.append(rel)
    if set(written) != wanted:
        raise ConfigError("missing referenced loading pictures")
    with atomic_output(out, "common/shell/load2.json") as path:
        write_text_lf(path, payload)
    return screen, written


def main(argv=None):
    parser = argparse.ArgumentParser(description="Prepare the 2005 game's loading screen data.")
    parser.add_argument("--game", help="install folder holding GameData (default: BFE_SWBF2_DIR)")
    parser.add_argument("--out", default=str(PUBLIC / "swbf2"), help="content root to write under")
    parser.add_argument("--language", default="english", help="localization table for the tips")
    args = parser.parse_args(argv)
    lvl_pc = lvl_pc_dir(args.game)
    if lvl_pc is None or not archive_path(lvl_pc, "load", "load.lvl").is_file():
        parser.error("no load/load.lvl under the install (set BFE_SWBF2_DIR or --game)")
    screen, written = write_loading_screen(lvl_pc, args.out, args.language)
    print(f"{len(screen['maps'])} worlds, {len(screen['languages'])} languages; "
          f"{len(written)} pictures written under {Path(args.out) / 'common' / 'shell' / 'load'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
