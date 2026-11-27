#!/usr/bin/env python3
"""The 2005 game's HUD definitions (`hud_` chunks in ingame.lvl), and the
strict reader for the config chunks both 2005 interface files use.

    BFE_SWBF2_DIR=<install> python3 tools/zero_hud2.py [--out DIR]

BFE_SWBF2_DIR (or --game) is the installation folder that holds GameData,
like BFE_GAME_DIR for the 2004 game (tools/zero_paths.py). The five chunks
(hudtransforms and one HUD per split-screen player count) are written as
JSON element trees under <out>/common/hud/. The 2005 preparation entry
`write_hud_files` also publishes the battle presentation catalog, art and languages.

A `hud_` chunk, like the 2005 `load` chunk (tools/zero_load2.py), is a
config chunk in the shape tools/zero_config.py reads for the 2004 game,
with one addition: the body starts with a NAME chunk holding the file's
name as a u32 hash (zero_hash.fnv1a), then DATA chunks each followed by
an optional SCOP chunk with that DATA's children. A DATA chunk is the
hashed field name (u32), a value count (u8), that many 4-byte values,
then a string table (u32 size, then the strings). A value is a float, a
name hash, or an offset into the string table (the string sits at
values_start + offset + 4, so string offsets start at 4 * count).

This reader is strict where zero_config is lenient: every size must tile
exactly, every string offset must land in the table on a terminated
string, every table byte must belong to a referenced string, and no
child tag but NAME, DATA and SCOP may appear. A chunk that parses here
has every byte accounted for. docs/formats-2005-interface.md has the
layout and what the trees mean.

All 193 field and file hashes occur in the stock interface chunks. Of the
spellings in NAMES, 172 match the swbf-unmunge dictionary (MIT) exactly
and 20 differ in case only. LoadDisplay comes from tools/zero_shell.py.
"""

import argparse
import json
import math
import os
import struct
import sys
import tempfile
from contextlib import contextmanager
from collections import Counter
from pathlib import Path

from zero_hud_chunks2 import ConfigError, checked_chunks
from zero_hash import fnv1a

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from zero_paths import PUBLIC, child as _child, write_text_lf  # noqa: E402

# Every field, element type and file name the 2005 load and hud_ chunks
# use, by hash. One table for both files: they share most of it.
NAMES = {fnv1a(name): name for name in """
1playerhud 2playerhud 3playerhud 4playerhud AddToTop AI AIText alpha
AlwaysScroll AmbientColor AngleEnd AngleStart AttackerOffScreen AttackerOffScreenBehind AttackerOnScreen AttackerOnScreenBehind
Backdrop BackdropLarge BackdropSmall BackdropSpawn background BarBitmap BarSegmented Bitmap
BitmapRect BitmapStyle BlendMode Border BorderedBox CampaignLayout CampaignName Circular
color ColorChange ColorChangeRate ColorFriendly ColorPulseRate DisplayTime Empty EmptyText
EventAlpha EventBitmap EventChangeMapMode EventColor EventDisable EventEnable EventInput EventMesh
EventNameFilter EventNumber EventOutput EventPlayerIndex EventPosition EventPostHide EventPulseRate EventRefreshMarker
EventRefreshPost EventRefreshTarget EventResetTargetCommon EventResetTargetPlayer EventRotation EventScale EventText EventToggleMapMode
EventValue FadeHoldTime FadeInTime fadeouttime FadeSustainTime FileInfo FlagOffScreen FlagOffScreenBehind
FlagOnScreen FlagOnScreenBehind FlashyDecFadeOutTime FlashyIncFadeOutTime FlashyScale FloatFormat format FourSegmentLockOn
group HintFlagOffScreen HintFlagOffScreenBehind HintFlagOnScreen HintFlagOnScreenBehind HintOffScreen HintOffScreenBehind HintOnScreen
HintOnScreenBehind hudtransforms InfiniteDashes lighting load LoadDisplay LoadingTextBlinkRate LoadingTextColorPallete
LockedOffScreen LockedOffScreenBehind LockedOnScreen LockedOnScreenBehind map MarkerLarge MarkerSmall MarkerSpawn
MASK MaxScale Mesh MeshInfo MinScale Model3D MultilineText NameMesh
NorthLarge NorthSmall NorthSpawn NumberColor NumberToTime NumberVector3 NumLines NumSegments
ObjectiveList ObjectiveOffScreen ObjectiveOffScreenBehind ObjectiveOnScreen ObjectiveOnScreenBehind Player PlayerDirectionLarge PlayerDirectionSmall
PlayerDirectionSpawn PlayerFOVLarge PlayerFOVSmall PlayerFOVSpawn PlayerText position PositionLarge PositionSmall
PositionSpawn PostLarge PostSelectLarge PostSelectSmall PostSelectSpawn PostSmall PostSpawn PostTextLarge
PostTextSmall PostTextSpawn ProceduralBarBitmap ProgressBarLEDColor ProgressBarLEDHeight ProgressBarLEDMinIntensity ProgressBarLEDSpacing ProgressBarLEDTexture
ProgressBarLEDWidth ProgressBarNumLEDs ProgressBarTotalTime PropagateAlpha RandomBackdrop Rect rotation Scale
ScaleTexture ScrollSpeed Segment self SelfText SunColor SunDirection target
TargetLarge TargetSmall TargetSpawn texcoords text TextAlignment TextBox TextBreak
TextCharacterSpacing TextFont TextScale TextStyle TipsBoxTexture TipsColor TipsPrefix TipsTime
TransformNameMesh TransformNumberColor TransformNumberVector3 UseChangeColor value VehicleSeating Viewport Viewports
ZOrder
""".split()}


# Limits apply to one interface archive, including nested archive sections.
MAX_ARCHIVE_BYTES = 128 << 20
MAX_NODES = 100_000
MAX_DEPTH = 48
MAX_STRING_BYTES = 8 << 20


class ArchiveBudget:
    def __init__(self):
        self.nodes = MAX_NODES
        self.strings = MAX_STRING_BYTES


def read_archive(path):
    with path.open("rb") as stream:
        if os.fstat(stream.fileno()).st_size > MAX_ARCHIVE_BYTES:
            raise ConfigError(f"{path.name}: archive too large")
        data = stream.read(MAX_ARCHIVE_BYTES + 1)
    if len(data) > MAX_ARCHIVE_BYTES:
        raise ConfigError(f"{path.name}: archive too large")
    return data


def archive_nodes(data, budget=None):
    """Validate the outer ucfb and lvl_ envelopes, including unrelated siblings."""
    if not 8 <= len(data) <= MAX_ARCHIVE_BYTES or data[:4] != b"ucfb":
        raise ConfigError("invalid ucfb archive")
    end = 8 + struct.unpack_from("<I", data, 4)[0]
    if end != len(data):
        raise ConfigError("ucfb size does not match file length")
    budget = budget or ArchiveBudget()

    def walk(start, stop, depth):
        if depth > MAX_DEPTH:
            raise ConfigError("too many nested archive sections")
        for tag, b, e in checked_chunks(data, start, stop, budget):
            if tag == b"lvl_":
                if e - b < 8:
                    raise ConfigError("short lvl_ header")
                yield from walk(b + 8, e, depth + 1)
            else:
                yield tag, b, e

    yield from walk(8, end, 0)


def _float_like(bits):
    """Whether a 4-byte value reads as one of the small floats these files
    store (positions, colours, times), as opposed to a name hash. The bits
    of a float_like value are recoverable from its float, so the choice
    loses nothing; a non-float-like value keeps its bits instead."""
    value = struct.unpack("<f", struct.pack("<I", bits))[0]
    return math.isfinite(value) and (value == 0.0 or 1e-30 < abs(value) < 1e7)


def _data_values(data, body, end, names, budget):
    """One DATA chunk's values: strings for string-table offsets, floats
    for float-like words, {"bits", "name"} for the rest (name hashes)."""
    if end - body < 5:
        raise ConfigError("DATA chunk shorter than its header")
    count = data[body + 4]
    at = body + 5
    table = at + 4 * count
    if table + 4 > end:
        raise ConfigError("DATA chunk shorter than its values")
    table_size = struct.unpack_from("<I", data, table)[0]
    blob = table + 4
    if blob + table_size != end:
        raise ConfigError("DATA string table does not end the chunk")
    if table_size > budget.strings:
        raise ConfigError("too much config string data")
    values = []
    strings = {}
    offset = 0
    while offset < table_size:
        stop = data.find(b"\0", blob + offset, end)
        if stop < 0:
            raise ConfigError("unterminated string in the string table")
        strings[offset] = stop
        if len(strings) > count:
            raise ConfigError("more strings than values")
        offset = stop + 1 - blob
    used = set()
    for i in range(count):
        bits = struct.unpack_from("<I", data, at + 4 * i)[0]
        offset = bits - 4 * count
        if table_size and 0 <= offset < table_size + 4:
            if offset not in strings:
                raise ConfigError("offset is not a string start")
            used.add(offset)
            start = blob + offset
            stop = strings[offset]
            budget.strings -= stop + 1 - start
            if budget.strings < 0:
                raise ConfigError("too much config string data")
            values.append(data[start:stop].decode("latin1"))
        elif _float_like(bits):
            values.append(struct.unpack("<f", struct.pack("<I", bits))[0])
        else:
            values.append({"bits": "0x%08x" % bits, "name": names.get(bits)})
    if len(used) != len(strings):
        raise ConfigError("string table bytes no value references")
    return values


def parse_entries(data, body, end, names=NAMES, *, depth=0, budget=None):
    """A config body (or one SCOP) as entries in file order:
    {"name", "hash", "values", "children"} per DATA, its children from the
    SCOP that follows it. A leading NAME chunk becomes a "name" entry with
    the hashed file name. Names come from `names`, else "0x........"."""
    entries = []
    if depth > MAX_DEPTH:
        raise ConfigError("too many nested config scopes")
    budget = budget or ArchiveBudget()
    kids = list(checked_chunks(data, body, end, budget))
    if depth == 0 and (not kids or kids[0][0] != b"NAME"):
        raise ConfigError("config has no root NAME")
    i = 0
    while i < len(kids):
        tag, b, e = kids[i]
        if tag == b"NAME":
            if depth or entries or e - b != 4:
                raise ConfigError("NAME is not one u32 at the body's start")
            hashed = struct.unpack_from("<I", data, b)[0]
            entries.append({"name": names.get(hashed, "0x%08x" % hashed),
                            "hash": hashed, "values": [], "fileName": True})
            i += 1
            continue
        if tag != b"DATA":
            raise ConfigError(f"unexpected chunk {tag!r} in a config body")
        values = _data_values(data, b, e, names, budget)
        hashed = struct.unpack_from("<I", data, b)[0]
        entry = {"name": names.get(hashed, "0x%08x" % hashed), "hash": hashed,
                 "values": values}
        i += 1
        if i < len(kids) and kids[i][0] == b"SCOP":
            entry["children"] = parse_entries(data, kids[i][1], kids[i][2], names,
                                              depth=depth + 1, budget=budget)
            i += 1
        entries.append(entry)
    return entries


def config_chunks(data, tag):
    """Every top-level chunk with this tag in an archive, parsed strictly:
    {"file": the NAME hash's name, "entries": [...]} each, in file order.
    The required NAME entry stays first in entries, marked fileName."""
    found = []
    seen = set()
    budget = ArchiveBudget()
    for chunk_tag, b, e in archive_nodes(data, budget):
        if chunk_tag != tag:
            continue
        entries = parse_entries(data, b, e, budget=budget)
        name = entries[0]["name"]
        if name in seen:
            raise ConfigError(f"duplicate config name: {name}")
        seen.add(name)
        found.append({"file": name, "entries": entries})
    return found


def to_jsonable(entries):
    """Only entries with an authored scope have a children array (possibly empty)."""
    result = []
    for entry in entries:
        node = {"name": entry["name"], "hash": "0x%08x" % entry["hash"], "values": entry["values"]}
        if "children" in entry:
            node["children"] = to_jsonable(entry["children"])
        result.append(node)
    return result


def summarize(entries):
    """Counts that say what a HUD tree holds, without its game data:
    element nodes by type (entries with children), properties (entries
    without), the Event* binding strings each names, the fonts TextFont
    names, and the bitmap and mesh names Bitmap and Mesh name."""
    summary = {"elements": Counter(), "properties": Counter(), "bindings": Counter(),
               "fonts": Counter(), "bitmaps": Counter(), "meshes": Counter()}

    def visit(items):
        for entry in items:
            if entry.get("fileName"):
                continue
            (summary["elements"] if "children" in entry else summary["properties"])[entry["name"]] += 1
            strings = [v for v in entry["values"] if isinstance(v, str)]
            if entry["name"].startswith("Event"):
                summary["bindings"].update(strings)
            elif entry["name"] == "TextFont":
                summary["fonts"].update(strings)
            elif entry["name"] == "Bitmap":
                summary["bitmaps"].update(strings)
            elif entry["name"] == "Mesh":
                summary["meshes"].update(strings)
            visit(entry.get("children", ()))

    visit(entries)
    return {key: dict(counts.most_common()) for key, counts in summary.items()}


def lvl_pc_dir(install=None):
    """The 2005 install's _lvl_pc folder: `install` (or BFE_SWBF2_DIR) is
    the folder holding GameData, matched without regard to case like the
    2004 tools match theirs. None when there is no such folder."""
    base = Path(install or os.environ.get("BFE_SWBF2_DIR") or "")
    if not str(base):
        return None
    return _child(base, "GameData", "data", "_lvl_pc")


def archive_path(lvl_pc, *parts):
    return _child(lvl_pc, *parts) or Path(lvl_pc).joinpath(*parts)


def hud_files(lvl_pc):
    """ingame.lvl's HUD definitions as {file name: parsed chunk}."""
    data = read_archive(archive_path(lvl_pc, "ingame.lvl"))
    chunks = config_chunks(data, b"hud_")
    if not chunks:
        raise ConfigError("ingame.lvl contains no HUD definitions")
    return {chunk["file"]: chunk for chunk in chunks}


@contextmanager
def atomic_output(root, relative):
    """Replace a complete file from a sibling temporary file under the family root."""
    root = Path(root).resolve()
    dest = root / relative
    if not dest.resolve().is_relative_to(root):
        raise ConfigError(f"output path escapes the family root: {relative}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=dest.parent, delete=False) as temp:
        path = Path(temp.name)
    try:
        yield path
        if not dest.resolve().is_relative_to(root):
            raise ConfigError(f"output path escapes the family root: {relative}")
        path.replace(dest)
    finally:
        path.unlink(missing_ok=True)


def write_json(root, relative, payload):
    with atomic_output(root, relative) as path:
        write_text_lf(path, json.dumps(payload, indent=1, allow_nan=False) + "\n")


def write_hud_files(lvl_pc, out):
    """Prepare the battle catalog and the standalone element trees in a family root."""
    return _write_family_hud(lvl_pc, out, hud_files(lvl_pc))


def _write_family_hud(lvl_pc, out, huds):
    # The catalog publisher also uses this module's strict chunk reader.
    from zero_battle_ui2 import write_battle_interface
    write_battle_interface(lvl_pc, out, huds=huds)
    return _write_huds(huds, out)


def _write_huds(huds, out):
    written = []
    for name, chunk in huds.items():
        payload = {"file": name, "entries": to_jsonable(chunk["entries"])}
        write_json(out, Path("common/hud") / f"{name}.json", payload)
        written.append(name)
    return written


def main(argv=None):
    parser = argparse.ArgumentParser(description="Dump the 2005 game's hud_ definitions as JSON.")
    parser.add_argument("--game", help="install folder holding GameData (default: BFE_SWBF2_DIR)")
    parser.add_argument("--out", default=str(PUBLIC / "swbf2"), help="content root to write under")
    args = parser.parse_args(argv)
    lvl_pc = lvl_pc_dir(args.game)
    if lvl_pc is None or not archive_path(lvl_pc, "ingame.lvl").is_file():
        parser.error("no ingame.lvl under the install (set BFE_SWBF2_DIR or --game)")
    huds = hud_files(lvl_pc)
    written = _write_huds(huds, args.out)
    for name, chunk in huds.items():
        counts = summarize(chunk["entries"])
        print(f"{name}: {sum(counts['elements'].values())} elements, "
              f"{sum(counts['properties'].values())} properties, "
              f"{len(counts['bindings'])} binding names")
    print(f"wrote {len(written)} HUD files under {Path(args.out) / 'common' / 'hud'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
