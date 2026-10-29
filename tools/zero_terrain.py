"""Terrain (`tern`): the patch-exact height field the city stands on."""

import math
import struct

from ucfb import child, children_of, chunks, cstrings, f32s, u32
from zero_model import TRI_STRIP, triangles

# Per-vertex layer weights are written for this many map layers, one byte
# each (terrain_weights.bin, in terrain vertex order). No retail map paints
# with more than six.
WEIGHT_SLOTS = 8


def parse_terrain(data, body, end):
    """Rebuild the terrain from its patches, exactly as munged.

    PCHS holds a COMN chunk with the triangle strip every plain patch shares,
    then one PTCH per patch, row by row. A patch's position VBUF (kind 290:
    position, normal, colour; 28 bytes) is in patch-local meters. Patches under
    a terrain cutter were re-tessellated around the hole and carry their own
    VBUF and IBUF. Patch (col, row) sits at x = col * span - extent and
    z = row * span - extent + one grid unit; the hole edges in the cut patches
    and the spawn points on the sunken plazas both land exactly there.

    The colour is baked light at half strength (the renderer doubles it). LTEX
    names the layer textures, SCAL their meters per repeat, DTLX the detail
    texture, and LUMI a 256x256 brightness map the game uses to darken
    soldiers on the ground.

    Each patch paints with its own one to three layers: its INFO is a count,
    then that many LTEX indices in drawing order (then the patch's height
    range). The weights are in the patch's compact VBUF (kind 20770, 16
    bytes): byte 15 is the first layer's weight, byte 11 the second's, and
    the third gets what is left of 255. The kind-290 colour alpha is only
    second / (first + second), for drawing two layers at a time. Weights
    agree where patches meet even when the patches list their layers in a
    different order, so they come out per vertex for every map layer.

    FOLG is the editor's painted foliage: a u32 byte count, then one
    nibble per cell of a fixed 16 m grid, with rows running toward -Z,
    independent of the height grid. The high nibble is the even column. Each bit is one of the
    world's four foliage layers (the prp_ config names their classes); a set
    bit plants that layer's patch at the cell. Only a few hundred cells are
    painted on any retail map.
    """
    info = child(data, body, end, b"INFO")
    if info is None or info[1] - info[0] < 28:
        return None
    grid_size = struct.unpack_from("<H", data, info[0] + 16)[0]
    patch_cells = struct.unpack_from("<H", data, info[0] + 18)[0] or 8
    unit = struct.unpack_from("<f", data, info[0])[0]
    if grid_size == 0 or grid_size > 2048 or unit <= 0:
        return None
    layers = []
    ltex = child(data, body, end, b"LTEX")
    if ltex is not None:
        layers = [name for name in cstrings(data, ltex[0], ltex[1], 16) if name]
    scales = []
    scal = child(data, body, end, b"SCAL")
    if scal is not None:
        scales = list(struct.unpack_from("<%df" % ((scal[1] - scal[0]) // 4), data, scal[0]))
    scales = [s if s > 0.01 else 8.0 for s in scales[:len(layers)]] or [8.0]
    detail = None
    dtlx = child(data, body, end, b"DTLX")
    if dtlx is not None:
        names = cstrings(data, dtlx[0], dtlx[1], 1)
        detail = names[0] if names and names[0] else None
    lumi = child(data, body, end, b"LUMI")
    foliage = painted_foliage(data, child(data, body, end, b"FOLG"), grid_size, unit)
    tex_scale = scales[0]
    pchs = child(data, body, end, b"PCHS")
    if pchs is None:
        return None
    comn = child(data, pchs[0], pchs[1], b"COMN")
    common_ibuf = child(data, comn[0], comn[1], b"IBUF") if comn else None
    common = read_ibuf(data, common_ibuf) if common_ibuf else []
    patches = children_of(data, pchs[0], pchs[1], b"PTCH")
    per_row = round(math.sqrt(len(patches)))
    span = patch_cells * unit
    extent = grid_size * unit / 2
    positions = []
    normals = []
    uvs = []
    colors = []
    indices = []
    weights = bytearray()
    for i, (pb, pe) in enumerate(patches):
        ox = (i % per_row) * span - extent
        oz = (i // per_row) * span - extent + unit
        verts = None
        compact = None
        strip = common
        vertex_kind = None
        painted = [0, 1]
        for t, b, e in chunks(data, pb, pe):
            if t == b"INFO" and e > b and 1 <= data[b] <= 3 and e - b >= 1 + data[b]:
                painted = list(data[b + 1:b + 1 + data[b]])
            elif t == b"VBUF" and e - b >= 12:
                count, stride, kind = struct.unpack_from("<III", data, b)
                vertex_kind = kind
                if kind == 290 and stride >= 28:
                    verts = [
                        (f32s(data, b + 12 + n * stride, 6), data[b + 12 + n * stride + 24:b + 12 + n * stride + 28])
                        for n in range(count)
                    ]
                elif kind == 20770 and stride >= 16 and compact is None:
                    compact = (b + 12, stride, count)
            elif t == b"IBUF" and vertex_kind == 290:
                # A trailing kind-130 buffer and its indices are the low LOD.
                # Indexing the detailed vertices with that strip opens holes.
                strip = read_ibuf(data, (b, e))
        if not verts:
            continue
        base = len(positions)
        for n, ((x, y, z, nx, ny, nz), (cb, cg, cr, ca)) in enumerate(verts):
            wx, wz = x + ox, z + oz
            positions.append((wx, y, wz))
            normals.append((nx, ny, nz))
            # Editor texture scale is meters per repeat.
            uvs.append((wx / tex_scale, wz / tex_scale))
            colors.append((cr / 255.0, cg / 255.0, cb / 255.0, ca / 255.0))
            if compact and n < compact[2]:
                at = compact[0] + n * compact[1]
                first, second = data[at + 15], data[at + 11]
            else:
                first, second = 255 - ca, ca
            local = [255] if len(painted) == 1 else [255 - second, second] if len(painted) == 2 else [first, second, max(0, 255 - first - second)]
            vertex = [0] * WEIGHT_SLOTS
            for layer, weight in zip(painted, local):
                if layer < WEIGHT_SLOTS:
                    vertex[layer] = min(255, vertex[layer] + weight)
            weights.extend(vertex)
        indices.extend(base + v for v in triangles(TRI_STRIP, strip))
    if not positions:
        return None
    return {
        "positions": positions,
        "normals": normals,
        "uvs": uvs,
        "colors": colors,
        "indices": indices,
        "layers": layers or ["__terrain__"],
        "scales": scales,
        "detail": detail,
        "weights": bytes(weights),
        "lumi": data[lumi[0]:lumi[1]] if lumi and lumi[1] - lumi[0] == grid_size * grid_size else None,
        "grid": {"size": grid_size, "unit": unit, "origin": [-extent, -extent + unit]},
        "foliage": foliage,
    }


def painted_foliage(data, folg, grid_size, unit):
    """The painted foliage cells: [column, row, layer bits] for every nonzero
    nibble, with the cell size and the grid's origin (world x, z of cell 0)."""
    if folg is None or folg[1] - folg[0] < 4:
        return None
    count = u32(data, folg[0])  # bytes, two cells each
    side = round(math.sqrt(count * 2))
    if side * side != count * 2 or folg[1] - folg[0] < 4 + count:
        return None
    cell = 16.0  # PC foliage manager; independent of terrain resolution
    extent = side * cell / 2
    cells = []
    for j in range(side):
        row = folg[0] + 4 + (j * side) // 2
        for i in range(side):
            byte = data[row + i // 2]
            bits = (byte >> 4) & 15 if i % 2 == 0 else byte & 15
            if bits:
                cells.append([i, j, bits])
    return {"side": side, "cell": cell, "origin": [-extent, extent], "rowDirection": -1, "cells": cells}


def height_lookup(terrain):
    """height(x, z) on the munged terrain: bilinear between the four height
    grid vertices around the point, or None off the grid (in a cut)."""
    unit = terrain["grid"]["unit"]
    at = {}
    for x, y, z in terrain["positions"]:
        at[(int(round(x / unit)), int(round(z / unit)))] = y

    def height(x, z):
        fx, fz = x / unit, z / unit
        ix, iz = int(fx // 1), int(fz // 1)
        tx, tz = fx - ix, fz - iz
        corners = [at.get((ix, iz)), at.get((ix + 1, iz)), at.get((ix, iz + 1)), at.get((ix + 1, iz + 1))]
        if any(c is None for c in corners):
            known = [c for c in corners if c is not None]
            return sum(known) / len(known) if known else None
        a, b, c, d = corners
        return (a * (1 - tx) + b * tx) * (1 - tz) + (c * (1 - tx) + d * tx) * tz

    return height


def read_ibuf(data, ibuf):
    count = u32(data, ibuf[0])
    return list(struct.unpack_from("<%dH" % count, data, ibuf[0] + 4))


def refresh_weights(name):
    """Write terrain_weights.bin for an already extracted map and point its
    terrain record at it, without re-running the whole extraction."""
    import json
    from pathlib import Path

    root = Path(__file__).resolve().parents[1]
    from zero_paths import PUBLIC

    out = PUBLIC / "maps" / name
    world = json.loads((out / "map.json").read_text())
    record = next((m for m in world["meshes"] if m.get("terrain")), None)
    if record is None:
        print(f"{name}: no terrain")
        return
    from zero_paths import GAME

    data = (GAME / name[:3].upper() / f"{name}.lvl").read_bytes()
    terrain = None
    at = data.find(b"tern")
    while at >= 0 and terrain is None:
        size = u32(data, at + 4)
        if at + 8 + size <= len(data):
            terrain = parse_terrain(data, at + 8, at + 8 + size)
        at = data.find(b"tern", at + 4)
    if terrain is None or len(terrain["weights"]) != record["vertexCount"] * WEIGHT_SLOTS:
        raise SystemExit(f"{name}: terrain doesn't match map.json ({record['vertexCount']} vertices)")
    (out / "terrain_weights.bin").write_bytes(terrain["weights"])
    record["weights"] = "terrain_weights.bin"
    (out / "map.json").write_text(json.dumps(world))
    used = sorted({i for i in range(WEIGHT_SLOTS) if any(terrain["weights"][i::WEIGHT_SLOTS])})
    print(f"{name}: {record['vertexCount']} vertices, layers painted {[terrain['layers'][i] for i in used if i < len(terrain['layers'])]}")


if __name__ == "__main__":
    import sys

    for map_name in sys.argv[1:]:
        refresh_weights(map_name)
