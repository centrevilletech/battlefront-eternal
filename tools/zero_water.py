"""Terrain water: the WATR chunk in a map's tern, and its look from the
world's fx config (the `Water` effect section).

WATR holds INFO (the grid: cells per side, cells per WMAP byte, cell size,
x and z offsets, and a scale for the look's tiling and speeds), LAYR (the
layer name, a u32, the surface height, then values only the editor reads),
WMAP (a nibble per cell: nonzero is water, bit 3 a cell with its own patch),
and CPAT (one LPTC per patched cell, row by row: INFO bounds, an IBUF strip,
and a VBUF of world-space position, normal, and uv). Water exists only where
both WATR and the Water section are present.
"""

import struct

from ucfb import child, chunks
from zero_config import value
from zero_model import TRI_STRIP, triangles
from zero_terrain import read_ibuf

PATCH = 0x8

# The look's keys with the engine's defaults. Colours are 0..255 RGBA.
SETTINGS = {
    "PatchDivisions": [16, 16],
    "LODDecimation": [2],
    "Tile": [1, 1],
    "Velocity": [0, 0],
    "MainTexture": None,
    "RefractionColor": [255, 255, 255, 255],
    "ReflectionColor": [255, 255, 255, 255],
    "UnderwaterColor": [255, 255, 255, 255],
    "FresnelMinMax": [0.2, 0.9],
    "FarSceneRange": None,
    "NormalMapTextures": None,
    "BumpMapTextures": None,
    "SpecularMaskTextures": None,
    "SpecularMaskTile": [1, 1],
    "SpecularMaskScrollSpeed": [0, 0],
    "OscillationEnable": [1],
    "OceanEnable": [0],
    "FoamTexture": None,
    "FoamTile": [1, 1],
    "WindDirection": [1, 0],
    "WindSpeed": [0],
}


def water_section(fx_configs):
    """The world's `Water` effect section, or None."""
    for entries in fx_configs:
        for name, values, children in entries:
            if name == "Effect" and values and values[0] == "Water":
                return children
    return None


def parse_water(data, tern_body, tern_end, fx_configs):
    """The map's water, or None: the grid, the surface height, the water
    cells, the patched cells' own triangles, and the look."""
    watr = child(data, tern_body, tern_end, b"WATR")
    section = water_section(fx_configs)
    if not watr or section is None:
        return None
    info = child(data, *watr, b"INFO")
    layr = child(data, *watr, b"LAYR")
    wmap = child(data, *watr, b"WMAP")
    if not (info and layr and wmap):
        return None
    n, per_byte, cell, x_off, z_off, scale = struct.unpack_from("<IIfffI", data, info[0])
    name_end = data.index(b"\x00", layr[0], layr[1])
    height = struct.unpack_from("<f", data, name_end + 1 + 4)[0]
    cells = []
    row = n // per_byte
    for j in range(n):
        for i in range(n):
            byte = data[wmap[0] + i // per_byte + row * j]
            nibble = (byte >> 4) & 15 if i % 2 == 0 else byte & 15
            if nibble:
                cells.append([i, j, 1 if nibble & PATCH else 0])

    # The patched cells' own triangles, in world space.
    patches = []
    cpat = child(data, *watr, b"CPAT")
    if cpat:
        for tag, b, e in chunks(data, *cpat):
            if tag != b"LPTC":
                continue
            ibuf = child(data, b, e, b"IBUF")
            verts = None
            for t, vb, ve in chunks(data, b, e):
                if t == b"VBUF" and ve - vb >= 12:
                    count, stride, flags = struct.unpack_from("<III", data, vb)
                    if flags == 0x222 and stride == 32:
                        verts = [struct.unpack_from("<8f", data, vb + 12 + k * stride) for k in range(count)]
            if not verts or not ibuf:
                continue
            patches.append({
                "positions": [v[0:3] for v in verts],
                "uvs": [v[6:8] for v in verts],
                "indices": triangles(TRI_STRIP, read_ibuf(data, ibuf)),
            })

    look = {}
    for key, default in SETTINGS.items():
        look[key] = value(section, key, default)
    # The PC parser accepts these as aliases, with the last authored key winning.
    for key, values, _ in section:
        if key in ("RefractionColor", "DiffuseColor"):
            look["RefractionColor"] = values
    return {
        "grid": n,
        "cell": cell,
        "offset": [x_off, z_off],
        "scale": scale,
        "height": height,
        "cells": cells,
        "patches": patches,
        "look": look,
    }


def water_textures(water):
    """Every texture the water's look names, frames numbered from 0."""
    look = water["look"]
    names = set()
    for key in ("MainTexture", "FoamTexture"):
        if look.get(key):
            names.add(look[key][0])
    for key in ("NormalMapTextures", "BumpMapTextures", "SpecularMaskTextures"):
        frames = look.get(key)
        if frames and len(frames) >= 2:
            names |= {f"{frames[0]}{k}" for k in range(int(frames[1]))}
    return names | {"FXrippl1", "fx_ringbump"}
