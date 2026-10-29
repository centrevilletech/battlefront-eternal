#!/usr/bin/env python3
"""Navigation data for the Classic AI: a map archive's planning graph, hints,
barriers, boundary, paths and regions, as stored.

    python3 tools/zero_ai.py nab2 [bes2 ...] [--out DIR] [--archive PATH]

Writes public/maps/<map>/ai-nav.json and ai-nav.bin (gitignored) and nothing
else, so refreshing AI data never rewrites a map's geometry. Schema:
docs/ai-nav.md.

plan is the planning graph. INFO holds the hub and arc counts, signed 16-bit
as the loader sign-extends them, and a third word the loader discards (5 in
stock data). NODE is one record per hub of 40 + 20 * hubs bytes: a 16-byte
name, the position and radius, the indices of up to eight arcs (255 for an
unused slot), then five u32[hubs] routing tables indexed by destination hub,
table t for traversal bit 1 << t. ARCS is 22 bytes per arc: a 16-byte name,
its two hubs, and a u32 traversal mask. A routing word packs up to four
candidates, least significant byte first for as long as the rest of the word
is nonzero: the low three bits pick one of the hub's arc slots, the next five
are the candidate's weight. The tables go to ai-nav.bin byte for byte.

wrld holds Hint (an INFO with TYPE, a decimal string; NAME; XFRM, three basis
rows and a translation; then PROP children, a name hash and a string each),
BARR (an INFO with NAME, XFRM, SIZE and FLAG, a traversal mask), and regn (an
INFO with TYPE, NAME, XFRM and SIZE, then PROP children like a hint's). PATH
holds path children: NAME; INFO, a signed point count and two more 16-bit
words; PROP, raw bytes that are not hash/string pairs; PNTS, a position and a
w, x, y, z quaternion per point. bnd_ is a config tree (zero_config.py): a
NAME hash, then DATA entries (key hash, argument count, raw u32 arguments, a
string table), each followed by a SCOP of children when it has any. The Path
children of a Boundary entry name PATHs by the hash of their names.

Everything keeps its source order and its stored values; nothing is flipped
or converted. Containers are walked strictly: every byte belongs to a chunk
or to the zero padding that aligns the next one. Structural problems (sizes,
indices out of range, non-finite floats) stop the export, and nothing is
written. Oddities that shipped data does contain, such as a routed arc that
lacks its table's traversal bit, are kept and listed under `warnings`.
"""

import argparse
import hashlib
import json
import math
import struct
import sys
from pathlib import Path

from zero_hash import fnv1a

SCHEMA = 1
GENERATOR = "tools/zero_ai.py 1"

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import GAME  # noqa: E402
from zero_paths import PUBLIC  # noqa: E402
MAPS = PUBLIC / "maps"
JSON_FILE = "ai-nav.json"
BIN_FILE = "ai-nav.bin"

TABLES = 5            # routing tables per hub, for traversal bits 1 << 0 .. 1 << 4
UNUSED = 255          # an empty arc slot
HUB_HEAD = 40         # name, position, radius, eight arc slots; the tables follow
ARC_BYTES = 22
POINT_BYTES = 28
FULL_WEIGHT = 31      # the selector draws modulo 31: weights summing to 30 or 31 always pick in one pass
JET_TOLERANCE = 1e-4
BASIS_TOLERANCE = 1e-3


def _named(text):
    return {fnv1a(name): name for name in text.split()}


# Hint classes by the hash of the TYPE string: the loader compares hashes, and
# a TYPE it does not know (stock data has no "3") makes no hint.
HINT_KINDS = {fnv1a(code): (kind, bit) for code, kind, bit in (
    ("0", "snipe", 1), ("1", "patrol", 2), ("2", "cover", 4),
    ("4", "jetjump", 8), ("5", "minefield", 16), ("6", "land", 32))}
KINDS = ("snipe", "patrol", "cover", "jetjump", "minefield", "land")
HINT_PROPERTIES = _named("PrimaryStance SecondaryStance Mode Radius CommandPost")
# Region and barrier PROP names. Shipped regions carry only Layer; no shipped
# barrier has a PROP.
WORLD_PROPERTIES = _named("Layer")
BOUNDARY = fnv1a("Boundary")
PATH = fnv1a("Path")
CONFIG_KEYS = {BOUNDARY: "Boundary", PATH: "Path"}


# ---------------------------------------------------------------- chunks


def _walk(data, start, end, where):
    """A container's chunks as (tag, body, end, path), strictly: every byte
    belongs to a chunk or to the zero padding (under four bytes) that aligns
    the next one. The path names the chunk in error messages."""
    found = []
    pos = start
    while pos < end:
        at = pos - start
        if end - pos < 8:
            raise ValueError(f"{where}: {end - pos} stray bytes at +{at}")
        tag = data[pos:pos + 4]
        if not all(32 <= c < 127 for c in tag):
            raise ValueError(f"{where}: no chunk at +{at} (tag bytes {tag.hex()})")
        tag = tag.decode("latin-1")
        size = struct.unpack_from("<I", data, pos + 4)[0]
        body = pos + 8
        stop = body + size
        if stop > end:
            raise ValueError(f"{where}: {tag} at +{at} ({size} bytes) overruns its container by {stop - end} bytes")
        pos = (stop + 3) & ~3
        if any(data[stop:min(pos, end)]):
            raise ValueError(f"{where}: nonzero padding after {tag} at +{at}")
        found.append((tag, body, stop))
    total = {}
    for tag, _, _ in found:
        total[tag] = total.get(tag, 0) + 1
    seen = {}
    out = []
    for tag, body, stop in found:
        index = seen.get(tag, 0)
        seen[tag] = index + 1
        out.append((tag, body, stop, f"{where}/{tag}" if total[tag] == 1 else f"{where}/{tag}[{index}]"))
    return out


def _sections(data):
    """The archive's top-level chunks: the ucfb root's children, and the
    children of any lvl_ wrapper (an 8-byte header, then chunks)."""
    top = _walk(data, 0, len(data), "archive")
    if len(top) != 1 or top[0][0] != "ucfb":
        raise ValueError(f"archive: expected one ucfb chunk, found {[chunk[0] for chunk in top]}")
    out = []
    for tag, body, end, where in _walk(data, top[0][1], top[0][2], "ucfb"):
        if tag == "lvl_":
            if end - body < 8:
                raise ValueError(f"{where} is {end - body} bytes, shorter than its 8-byte header")
            out.extend(_walk(data, body + 8, end, where))
        else:
            out.append((tag, body, end, where))
    return out


def _parts(kids, where, required=(), optional=()):
    """A container's named children as tag -> (body, end, path), each at most
    once, the required ones present. Other children are framing-checked and
    otherwise ignored, as the loaders skip tags they do not read."""
    parts = {}
    for tag, body, end, path in kids:
        if tag in required or tag in optional:
            if tag in parts:
                raise ValueError(f"{where} has more than one {tag}")
            parts[tag] = (body, end, path)
    for tag in required:
        if tag not in parts:
            raise ValueError(f"{where} has no {tag}")
    return parts


def _text(data, part):
    """A string up to its first NUL (or the end of the chunk)."""
    return data[part[0]:part[1]].split(b"\0", 1)[0].decode("latin-1")


def _fixed(data, at):
    """A char[16] name: up to its first NUL, all sixteen bytes when it has none."""
    return data[at:at + 16].split(b"\0", 1)[0].decode("latin-1")


def _finite(values, where):
    values = list(values)
    if not all(math.isfinite(value) for value in values):
        raise ValueError(f"{where} holds a non-finite float")
    return values


def _floats(data, part, count):
    body, end, path = part
    if end - body != 4 * count:
        raise ValueError(f"{path} is {end - body} bytes, not {4 * count}")
    return _finite(struct.unpack_from(f"<{count}f", data, body), path)


def _u32(data, part):
    body, end, path = part
    if end - body != 4:
        raise ValueError(f"{path} is {end - body} bytes, not 4")
    return struct.unpack_from("<I", data, body)[0]


def _property(data, body, end, path, names):
    """A hashed PROP: u32 name hash, then a NUL-terminated string filling the chunk."""
    value = data[body + 4:end]
    if end - body < 5 or value[-1] != 0 or 0 in value[:-1]:
        raise ValueError(f"{path} is not a name hash and a NUL-terminated string")
    key = struct.unpack_from("<I", data, body)[0]
    return [f"{key:08x}", names.get(key), value[:-1].decode("latin-1")]


def _length(vector):
    return math.sqrt(sum(v * v for v in vector))


def _warning(kind, message, **fields):
    return {"kind": kind, "message": message, **fields}


# ---------------------------------------------------------------- plan


def candidates(word):
    """A routing word's (slot, weight) candidates, least significant byte
    first while the rest of the word is nonzero: a zero low byte under
    nonzero ones is a candidate (slot 0, weight 0)."""
    out = []
    while word:
        byte = word & 0xFF
        out.append((byte & 7, (byte >> 3) & 31))
        word >>= 8
    return out


def _table(parts, tag, expected, formula, where):
    """Where a fixed-size table starts; a missing one is empty."""
    part = parts.get(tag)
    size = part[1] - part[0] if part else 0
    if size != expected:
        if part is None:
            raise ValueError(f"{where} has no {tag} ({formula} = {expected} bytes)")
        raise ValueError(f"{part[2]} is {size} bytes, not {formula} = {expected}")
    return part[0] if part else 0


def _route(word, hub, table, destination, slots, arcs, where, warnings):
    """Validate one nonzero routing word and note its oddities; its weight sum."""
    found = candidates(word)
    place = {"hub": hub, "table": table, "destination": destination}
    route = f"hub {hub} table {table} to hub {destination} (word {word:#x})"
    flagged = set()
    for slot, _ in found:
        arc = slots[slot]
        if arc == UNUSED:
            raise ValueError(f"{where}: {route} routes through unused slot {slot}")
        mask = arcs[arc]["mask"]
        if not mask & (1 << table) and slot not in flagged:
            flagged.add(slot)
            warnings.append(_warning(
                "routeCapability", f"{route}: slot {slot} is arc {arc}, whose mask {mask:#x} lacks bit {1 << table}",
                **place, slot=slot, arc=arc, arcMask=mask, word=word))
    total = sum(weight for _, weight in found)
    if total not in (FULL_WEIGHT - 1, FULL_WEIGHT):
        warnings.append(_warning("routeWeightSum", f"{route}: weights sum to {total}", **place, word=word, sum=total))
    if total == 0:
        warnings.append(_warning("routeHang", f"{route}: every weight is zero", **place, word=word, reason="zeroWeights"))
    elif len({slot for slot, _ in found}) == 1 and found[0][1] < FULL_WEIGHT:
        warnings.append(_warning(
            "routeHang", f"{route}: every candidate is slot {found[0][0]}, first weight {found[0][1]}",
            **place, word=word, reason="singleSlot"))
    return total


def _plan(data, chunk, warnings):
    """The graph, the routing tables as stored (ai-nav.bin), and route counts."""
    _, body, end, where = chunk
    parts = _parts(_walk(data, body, end, where), where, optional=("INFO", "NODE", "ARCS"))
    info = parts.get("INFO")
    if info is None or info[1] - info[0] != 6:
        raise ValueError(f"{info[2]} is {info[1] - info[0]} bytes, not 6" if info else f"{where} has no INFO")
    hub_count, arc_count, third = struct.unpack_from("<hhH", data, info[0])
    if hub_count < 0 or arc_count < 0:
        raise ValueError(f"{info[2]} has a negative count: {hub_count} hubs, {arc_count} arcs")
    stride = HUB_HEAD + 4 * TABLES * hub_count
    node = _table(parts, "NODE", hub_count * stride, f"{hub_count} * (40 + 20 * {hub_count})", where)
    arcs_at = _table(parts, "ARCS", ARC_BYTES * arc_count, f"22 * {arc_count}", where)

    arcs = []
    for i in range(arc_count):
        at = arcs_at + ARC_BYTES * i
        first, second, mask = struct.unpack_from("<BBI", data, at + 16)
        if first >= hub_count or second >= hub_count:
            raise ValueError(f"{where}/ARCS: arc {i} joins hubs {first} and {second}; there are {hub_count} hubs")
        arcs.append({"name": _fixed(data, at), "hubs": [first, second], "mask": mask})

    hubs = []
    for h in range(hub_count):
        at = node + h * stride
        position = _finite(struct.unpack_from("<3f", data, at + 16), f"{where}/NODE hub {h} position")
        radius = _finite(struct.unpack_from("<f", data, at + 28), f"{where}/NODE hub {h} radius")[0]
        slots = list(data[at + 32:at + 40])
        for s, arc in enumerate(slots):
            if arc == UNUSED:
                continue
            if arc >= arc_count:
                raise ValueError(f"{where}/NODE: hub {h} slot {s} names arc {arc}; there are {arc_count} arcs")
            if h not in arcs[arc]["hubs"]:
                a, b = arcs[arc]["hubs"]
                raise ValueError(f"{where}/NODE: hub {h} slot {s} names arc {arc}, which joins hubs {a} and {b}")
        hubs.append({"name": _fixed(data, at), "position": position, "radius": radius, "arcs": slots})

    for i, arc in enumerate(arcs):
        a, b = arc["hubs"]
        if hubs[a]["position"] == hubs[b]["position"]:
            warnings.append(_warning("zeroLengthArc", f"arc {i} ({arc['name']}) joins hubs {a} and {b} at one position",
                                     arc=i, hubs=[a, b]))
    touching = [0] * hub_count
    for arc in arcs:
        for h in set(arc["hubs"]):
            touching[h] |= arc["mask"]
    for h, hub in enumerate(hubs):
        slots = hub["arcs"]
        if UNUSED in slots and any(arc != UNUSED for arc in slots[slots.index(UNUSED):]):
            warnings.append(_warning("hubSlotGap", f"hub {h} lists an arc after an unused slot: {slots}",
                                     hub=h, arcs=list(slots)))
        mask = 0
        for arc in slots:
            if arc != UNUSED:
                mask |= arcs[arc]["mask"]
        if mask != touching[h]:
            warnings.append(_warning(
                "hubMaskDiffers", f"hub {h}: its slots' masks OR to {mask:#x}, every arc touching it to {touching[h]:#x}",
                hub=h, slotMask=mask, touchingMask=touching[h]))

    sums = {}
    nonzero = 0
    tables = bytearray()
    for h, hub in enumerate(hubs):
        at = node + h * stride + HUB_HEAD
        tables += data[at:at + 4 * TABLES * hub_count]
        words = struct.unpack_from(f"<{TABLES * hub_count}I", data, at)
        for t in range(TABLES):
            for d in range(hub_count):
                word = words[t * hub_count + d]
                if word:
                    nonzero += 1
                    total = _route(word, h, t, d, hub["arcs"], arcs, f"{where}/NODE", warnings)
                    sums[total] = sums.get(total, 0) + 1

    graph = {
        "info": [hub_count, arc_count, third],
        "hubs": hubs,
        "arcs": arcs,
        "routes": {"file": BIN_FILE, "byteOffset": 0, "count": TABLES * hub_count * hub_count,
                   "tables": TABLES, "order": "hub,table,destination"},
    }
    return graph, bytes(tables), nonzero, sums


# ---------------------------------------------------------------- world


def _info_and_properties(data, body, end, where, names, info_first):
    """A Hint, BARR or regn: its one INFO and its hashed PROP children in order.
    A hint's PROPs apply to the hint its INFO made, so none may come first."""
    info, properties = None, []
    for tag, b, e, path in _walk(data, body, end, where):
        if tag == "INFO":
            if info is not None:
                raise ValueError(f"{where} has more than one INFO")
            info = (b, e, path)
        elif tag == "PROP":
            if info_first and info is None:
                raise ValueError(f"{path} comes before its INFO")
            properties.append(_property(data, b, e, path, names))
    if info is None:
        raise ValueError(f"{where} has no INFO")
    return info, properties


def _hint(data, body, end, where, index, warnings):
    info, properties = _info_and_properties(data, body, end, where, HINT_PROPERTIES, True)
    parts = _parts(_walk(data, *info), info[2], required=("TYPE", "NAME", "XFRM"))
    code = _text(data, parts["TYPE"])
    name = _text(data, parts["NAME"])
    xfrm = _floats(data, parts["XFRM"], 12)
    kind, bit = HINT_KINDS.get(fnv1a(code), (None, 0))
    if kind is None:
        warnings.append(_warning("hintType", f"hint {index} ({name}) has TYPE {code!r}, which the loader drops",
                                 hint=index, name=name, type=code))
    elif kind == "jetjump":
        length = _length(xfrm[6:9])
        if abs(length - 1) > JET_TOLERANCE:
            warnings.append(_warning("jetForward", f"jet hint {index} ({name}) has a forward vector of length {length:.6g}",
                                     hint=index, name=name, length=length))
    return {"type": code, "kind": kind, "bit": bit, "name": name, "xfrm": xfrm, "properties": properties}


def _barrier(data, body, end, where, index, warnings):
    info, properties = _info_and_properties(data, body, end, where, WORLD_PROPERTIES, False)
    parts = _parts(_walk(data, *info), info[2], required=("NAME", "XFRM", "SIZE", "FLAG"))
    name = _text(data, parts["NAME"])
    xfrm = _floats(data, parts["XFRM"], 12)
    x_axis, z_axis = xfrm[0:3], xfrm[6:9]
    x_length, z_length = _length(x_axis), _length(z_axis)
    dot = sum(a * b for a, b in zip(x_axis, z_axis))
    if max(abs(x_length - 1), abs(z_length - 1), abs(dot)) > BASIS_TOLERANCE:
        warnings.append(_warning(
            "barrierBasis", f"barrier {index} ({name}): |X| {x_length:.6g}, |Z| {z_length:.6g}, X.Z {dot:.6g}",
            barrier=index, name=name, xLength=x_length, zLength=z_length, dot=dot))
    return {"name": name, "xfrm": xfrm, "size": _floats(data, parts["SIZE"], 3),
            "flags": _u32(data, parts["FLAG"]), "properties": properties}


def _region(data, body, end, where):
    info, properties = _info_and_properties(data, body, end, where, WORLD_PROPERTIES, False)
    parts = _parts(_walk(data, *info), info[2], required=("TYPE", "NAME", "XFRM", "SIZE"))
    return {"type": _text(data, parts["TYPE"]), "name": _text(data, parts["NAME"]),
            "xfrm": _floats(data, parts["XFRM"], 12), "size": _floats(data, parts["SIZE"], 3),
            "properties": properties}


def _world(data, chunk, out, warnings):
    """Append one wrld's hints, barriers and regions to out; its NAME."""
    _, body, end, where = chunk
    name = None
    for tag, b, e, path in _walk(data, body, end, where):
        if tag == "NAME" and name is None:
            name = _text(data, (b, e, path))
        elif tag == "Hint":
            out["hints"].append(_hint(data, b, e, path, len(out["hints"]), warnings))
        elif tag == "BARR":
            out["barriers"].append(_barrier(data, b, e, path, len(out["barriers"]), warnings))
        elif tag == "regn":
            out["regions"].append(_region(data, b, e, path))
    return name


# ---------------------------------------------------------------- paths and boundary


def _paths(data, chunk):
    _, body, end, where = chunk
    out = []
    for tag, b, e, path in _walk(data, body, end, where):
        if tag != "path":
            continue
        parts = _parts(_walk(data, b, e, path), path, required=("NAME", "INFO"), optional=("PROP", "PNTS"))
        info = parts["INFO"]
        if info[1] - info[0] != 6:
            raise ValueError(f"{info[2]} is {info[1] - info[0]} bytes, not 6")
        count, word2, word3 = struct.unpack_from("<hhH", data, info[0])
        if count < 0:
            raise ValueError(f"{info[2]} has a negative point count ({count})")
        pnts = parts.get("PNTS")
        if pnts is None:
            if count:
                raise ValueError(f"{path} has no PNTS; its INFO says {count} points")
            points = []
        else:
            size = pnts[1] - pnts[0]
            if size % POINT_BYTES:
                raise ValueError(f"{pnts[2]} is {size} bytes, not a multiple of {POINT_BYTES}")
            if size // POINT_BYTES != count:
                raise ValueError(f"{pnts[2]} holds {size // POINT_BYTES} points; its INFO says {count}")
            points = [_finite(struct.unpack_from("<7f", data, pnts[0] + POINT_BYTES * i), f"{pnts[2]} point {i}")
                      for i in range(count)]
        prop = parts.get("PROP")
        out.append({"name": _text(data, parts["NAME"]), "info": [count, word2, word3], "points": points,
                    "prop": data[prop[0]:prop[1]].hex() if prop else None})
    return out


def _entry(data, body, end, path):
    """A config DATA: key hash, argument count, raw u32 arguments, then a
    string table (u32 size, bytes) that must end exactly at the chunk's end."""
    size = end - body
    if size < 5:
        raise ValueError(f"{path} is {size} bytes, too short for a key and an argument count")
    key = struct.unpack_from("<I", data, body)[0]
    count = data[body + 4]
    table = body + 5 + 4 * count
    if table + 4 > end:
        raise ValueError(f"{path}: {count} arguments and a string table size need {table + 4 - body} bytes; it has {size}")
    table_size = struct.unpack_from("<I", data, table)[0]
    if table + 4 + table_size != end:
        raise ValueError(f"{path}: {count} arguments and a {table_size}-byte string table fill "
                         f"{table + 4 + table_size - body} bytes, not its {size}")
    return {"key": f"{key:08x}", "keyName": CONFIG_KEYS.get(key),
            "args": [f"{value:08x}" for value in struct.unpack_from(f"<{count}I", data, body + 5)],
            "strings": data[table + 4:end].hex() if table_size else None, "children": []}


def _entries(data, kids):
    """DATA entries in order, each SCOP's entries the children of the DATA right before it."""
    entries, previous = [], None
    for tag, body, end, path in kids:
        if tag == "DATA":
            entries.append(_entry(data, body, end, path))
        elif tag == "SCOP":
            if previous != "DATA":
                raise ValueError(f"{path} does not follow a DATA")
            entries[-1]["children"] = _entries(data, _walk(data, body, end, path))
        previous = tag
    return entries


def _config(data, chunk):
    _, body, end, where = chunk
    kids = _walk(data, body, end, where)
    name = _parts(kids, where, optional=("NAME",)).get("NAME")
    return {"name": f"{_u32(data, name):08x}" if name else None, "entries": _entries(data, kids)}


def _path_lookup(paths, warnings):
    """Path index by name hash: the first in source order, as lookups find."""
    found = {}
    for i, path in enumerate(paths):
        key = fnv1a(path["name"])
        if key in found:
            j = found[key]
            warnings.append(_warning(
                "duplicatePath", f"paths {j} ({paths[j]['name']}) and {i} ({path['name']}) share hash {key:08x}; lookups find {j}",
                hash=f"{key:08x}", paths=[j, i]))
        else:
            found[key] = i
    return found


def _references(configs, lookup, warnings):
    """Every Path child of every Boundary entry, in order, resolved to a path index."""
    references = []
    for c, config in enumerate(configs):
        for entry in config["entries"]:
            if entry["key"] != f"{BOUNDARY:08x}":
                continue
            for child in entry["children"]:
                if child["key"] != f"{PATH:08x}":
                    continue
                key = child["args"][0] if child["args"] else None
                index = lookup.get(int(key, 16)) if key else None
                if index is None:
                    warnings.append(_warning(
                        "boundaryPath", f"boundary config {c} names path {key}, which is not in the archive"
                        if key else f"boundary config {c} has a Path entry with no argument", config=c, hash=key))
                references.append({"hash": key, "path": index})
    return references


# ---------------------------------------------------------------- archive


def decode_archive(data, source_path="archive.lvl", map_name=None):
    """One archive's navigation data: the ai-nav.json document and the
    ai-nav.bin bytes (None when the archive has no plan). Raises ValueError,
    naming the chunk, on a structural problem."""
    sections = _sections(data)
    plans = [chunk for chunk in sections if chunk[0] == "plan"]
    if len(plans) > 1:
        raise ValueError(f"archive has {len(plans)} plan chunks ({', '.join(chunk[3] for chunk in plans)}); "
                         "expected at most one")
    warnings = []
    graph, tables, nonzero, sums = None, None, 0, {}
    if plans:
        graph, tables, nonzero, sums = _plan(data, plans[0], warnings)
    world = {"hints": [], "barriers": [], "regions": []}
    names = [_world(data, chunk, world, warnings) for chunk in sections if chunk[0] == "wrld"]
    paths = [path for chunk in sections if chunk[0] == "PATH" for path in _paths(data, chunk)]
    configs = [_config(data, chunk) for chunk in sections if chunk[0] == "bnd_"]
    lookup = _path_lookup(paths, warnings)
    references = _references(configs, lookup, warnings)

    hints = {kind: 0 for kind in KINDS}
    hints["unsupported"] = 0
    for hint in world["hints"]:
        hints[hint["kind"] or "unsupported"] += 1
    hub_count = len(graph["hubs"]) if graph else 0
    doc = {
        "schema": SCHEMA,
        "generator": GENERATOR,
        "map": map_name or Path(source_path).stem,
        "source": {"path": source_path, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()},
        "world": names[0] if names else None,
    }
    if len(names) > 1:
        doc["worlds"] = names
    doc.update({
        "counts": {
            "hubs": hub_count,
            "arcs": len(graph["arcs"]) if graph else 0,
            "routeWords": TABLES * hub_count * hub_count,
            "routeNonzero": nonzero,
            "weightSums": {str(total): sums[total] for total in sorted(sums, reverse=True)},
            "hints": hints,
            "barriers": len(world["barriers"]),
            "paths": len(paths),
            "pathPoints": sum(len(path["points"]) for path in paths),
            "regions": len(world["regions"]),
        },
        "graph": graph,
        "hints": world["hints"],
        "barriers": world["barriers"],
        "boundary": {"configs": configs, "references": references},
        "paths": paths,
        "regions": world["regions"],
        "warnings": warnings,
        "bin": None if tables is None else {
            "file": BIN_FILE, "bytes": len(tables), "sha256": hashlib.sha256(tables).hexdigest()},
    })
    return doc, tables


def map_archive(name):
    """Maps live in a folder named for their planet: bes2 is BES/bes2.lvl, an
    add-on's in its own folder (tools/zero_addon.py)."""
    from zero_addon import map_archive as find
    return find(name, GAME)


def _source_name(path):
    """The archive's path as the document records it: under the install's
    _LVL_PC when it is there, under its GameData (an add-on's,
    AddOn/Tat3/...), else under the repository, else in full."""
    path = Path(path).resolve()
    for base in (GAME, GAME.parents[1], ROOT):
        try:
            return path.relative_to(base.resolve()).as_posix()
        except ValueError:
            pass
    return path.as_posix()


def _write(path, payload):
    temp = path.with_name(path.name + ".tmp")
    temp.write_bytes(payload)
    temp.replace(path)


def export_map(name, archive=None, out=None):
    """Decode a map and write <out>/<name>/ai-nav.json and ai-nav.bin (out
    defaults to public/maps); the document. On a structural problem this
    raises ValueError and writes nothing."""
    archive = Path(archive) if archive else map_archive(name)
    folder = Path(out if out else MAPS) / name
    doc, tables = decode_archive(archive.read_bytes(), _source_name(archive), name)
    text = json.dumps(doc, separators=(",", ":"), allow_nan=False)
    folder.mkdir(parents=True, exist_ok=True)
    if tables is None:
        (folder / BIN_FILE).unlink(missing_ok=True)
    else:
        _write(folder / BIN_FILE, tables)
    _write(folder / JSON_FILE, text.encode("ascii"))
    return doc


def summary(doc):
    """One line: what the map holds, and its warnings by kind."""
    counts = doc["counts"]
    parts = []
    if doc["graph"]:
        sums = ", ".join(f"{total}: {n}" for total, n in counts["weightSums"].items())
        parts.append(f"{counts['hubs']} hubs, {counts['arcs']} arcs, "
                     f"{counts['routeNonzero']}/{counts['routeWords']} routes ({sums})")
    else:
        parts.append("no plan")
    kinds = ", ".join(f"{kind} {n}" for kind, n in counts["hints"].items() if n)
    parts.append(f"{sum(counts['hints'].values())} hints ({kinds or 'none'})")
    parts.append(f"{counts['barriers']} barriers, {counts['paths']} paths ({counts['pathPoints']} points), "
                 f"{counts['regions']} regions")
    references = doc["boundary"]["references"]
    if references:
        names = [doc["paths"][ref["path"]]["name"] if ref["path"] is not None else f"missing {ref['hash']}"
                 for ref in references]
        parts.append(f"boundary -> {', '.join(names)}")
    else:
        parts.append("no boundary")
    tally = {}
    for warning in doc["warnings"]:
        tally[warning["kind"]] = tally.get(warning["kind"], 0) + 1
    parts.append("warnings: " + (", ".join(f"{kind} {n}" for kind, n in tally.items()) or "none"))
    return f"{doc['map']}: " + "; ".join(parts)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("maps", nargs="+", metavar="map", help="map names, e.g. nab2 bes2")
    parser.add_argument("--out", type=Path, default=MAPS, help="maps folder to write <map>/ai-nav.* in (default public/maps)")
    parser.add_argument("--archive", type=Path, help="read this archive instead of the installed one (one map only)")
    args = parser.parse_args(argv)
    if args.archive and len(args.maps) != 1:
        parser.error("--archive goes with exactly one map")
    status = 0
    for name in args.maps:
        try:
            doc = export_map(name, args.archive, args.out)
        except (OSError, ValueError) as error:
            print(f"{name}: {error}", file=sys.stderr)
            status = 1
            continue
        print(summary(doc))
    return status


if __name__ == "__main__":
    sys.exit(main())
