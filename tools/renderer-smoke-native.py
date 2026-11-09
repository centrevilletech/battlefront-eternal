#!/usr/bin/env python3
"""Native software-OpenGL renderer smoke test over synthetic public fixtures.

This script exercises the native Rust renderer, shader catalog, content loader,
and window host on Linux without prepared retail game files:

1. Cross-checks the shader manifest catalog (`rust/bfe-render/shaders/*/manifest.json`)
   against `tools/renderer-smoke-manifests.mjs` across all 6 families (`world`: 536,
   `actors`: 310, `effects`: 149, `interface`: 15, `post-hud`: 12, `utility`: 24;
   total 1,046) and all sub-families, then runs `render-smoke` under software GL
   (`LIBGL_ALWAYS_SOFTWARE=1`) to compile and link all 1,046 shader variants
   through `bfe-render`'s own program builder and draw one representative scene
   for each of the 6 families (`world`, `actors`, `effects`, `interface`,
   `post-hud`, `utility`) with RGBA8 framebuffer readback verification.
2. Reuses the public synthetic foliage (`grasspatch`, `leafpatch`), `water`,
   and `ocean` inputs from `tools/native-static-fixture.mjs` and verifies the
   `static-fixture` binary against `tests/fixtures/static-geometry-hashes.json`.
3. Constructs a self-contained synthetic map directory (`maps/room/`) using
   those same foliage and water definitions plus a lit near-world mesh, a sky
   layer, and a 1-bone placed prop, then checks `fingerprint` admission.
4. Runs `static-frame` over EGL surfaceless software OpenGL (`llvmpipe`) for
   each static component (`sky`, `props`, `foliage`, `water`), writes a PNG image
   from the rendered RGBA framebuffer, and verifies the PNG header, dimensions,
   and non-blank pixel distribution.
5. Runs `bfe-client --map room` under X11 (`xvfb-run` + `native-client-window.py`)
   with `LIBGL_ALWAYS_SOFTWARE=1` to prove the windowed native client admits
   the synthetic map and completes its `battle_frame` redraw loop.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import shutil
try:
    import resource
except ImportError:
    resource = None
import struct
import subprocess
import sys
import tempfile
import time
import zlib

# Expected shader family order, variant counts, and sub-family breakdown shared
# with `tools/renderer-smoke-manifests.mjs`.
EXPECTED_FAMILY_ORDER = ("world", "actors", "effects", "interface", "post-hud", "utility")
EXPECTED_TOTAL_VARIANTS = 1046
EXPECTED_FAMILY_COUNTS = {
    "world": 536,
    "actors": 310,
    "effects": 149,
    "interface": 15,
    "post-hud": 12,
    "utility": 24,
}
EXPECTED_SUB_FAMILIES = {
    "world": {"canopies": 2, "foliage": 4, "static": 474, "terrain": 56},
    "actors": {"depth": 6, "rigid": 144, "skinned": 160},
    "effects": {
        "arc": 2,
        "basic": 8,
        "blaster": 2,
        "death": 2,
        "destruction": 6,
        "far": 2,
        "flame": 2,
        "fountain": 2,
        "glow": 4,
        "jet": 8,
        "jets": 2,
        "motion": 4,
        "ocean": 12,
        "particle": 6,
        "rain": 6,
        "shield": 4,
        "shock": 2,
        "sky": 14,
        "sniper": 4,
        "snow": 2,
        "spray": 2,
        "sprite": 2,
        "square": 2,
        "storm": 8,
        "trail": 2,
        "view": 3,
        "volume": 4,
        "wake": 4,
        "water": 26,
        "wisps": 2,
    },
    "interface": {
        "basic": 2,
        "beam": 1,
        "column": 1,
        "face": 1,
        "floor": 1,
        "metal": 1,
        "model": 4,
        "points": 1,
        "quad": 1,
        "room": 1,
        "vignette": 1,
    },
    "post-hud": {
        "bilinear": 1,
        "bloom": 2,
        "composite": 1,
        "copy": 1,
        "fxaa": 2,
        "hud": 1,
        "motion": 1,
        "resolve": 1,
        "sharpen": 1,
        "spatial": 1,
    },
    "utility": {"line": 24},
}

# BfeFrame std140 uniform block layout from rust/bfe-render/src/shader.rs.
# Offsets are in f64 elements; total length is FRAME_FLOATS (200).
FRAME_FLOATS = 200
FRAME_OFFSETS = {
    "projectionMatrix": 0,
    "viewMatrix": 16,
    "cameraPosition": 32,
    "time": 35,
    "sunDirection": 36,
    "bump": 39,
    "sunColor": 40,
    "shadowStrength": 43,
    "ambient": 44,
    "aaSamples": 47,
    "ambientTop": 48,
    "terrainShade": 51,
    "ambientBottom": 52,
    "farAaSamples": 55,
    "characterAmbient": 56,
    "cascadeNear": 59,
    "fogColor": 60,
    "cascadeFar": 63,
    "fade": 64,
    "resolution": 66,
    "fogRange": 68,
}

# Synthetic foliage definitions shared with tools/native-static-fixture.mjs.
# Reusing these definitions exercises the same card generators in both the
# standalone geometry probe and the full native scene admission path.
STATIC_FIXTURE_FOLIAGE = [
    {
        "base": "grasspatch",
        "NumParticles": 35,
        "BoxSize": [2.8, 2.8],
        "MinSize": 1.2,
        "MaxSize": 1.5,
        "DarknessMin": 0.4,
        "DarknessMax": 1,
    },
    {
        "base": "grasspatch",
        "NumParticles": 50,
        "RadiusFadeMin": 2,
        "RadiusFadeMax": 8,
        "MinSize": 1,
        "MaxSize": 3,
        "YOffset": 0.2,
    },
    {
        "base": "leafpatch",
        "NumParticles": 55,
        "Seed": 321651321,
        "Radius": 4,
        "Height": 4,
        "HeightScale": 1,
        "MinSize": 1.7,
        "MaxSize": 2,
        "DarknessMin": 0.4,
        "DarknessMax": 1,
        "Offset": [1, 2, 3],
    },
    {
        "base": "leafpatch",
        "NumParticles": 50,
        "Seed": 651684651,
        "Radius": 1.3,
        "Height": 13,
        "HeightScale": 1.6,
        "ConeHeight": 8,
        "MinSize": 1.2,
        "MaxSize": 1.4,
    },
    {
        "base": "leafpatch",
        "NumParticles": 50,
        "Seed": 937377218,
        "Vine": [2, 0],
        "MinSize": 2,
        "MaxSize": 2.5,
        "VineLength": [2, 4],
        "VineSpread": 0.6,
    },
    {
        "base": "leafpatch",
        "NumParticles": 13,
        "Seed": 328552794,
        "BoxSize": [4, 5, 6],
        "MinSize": 1,
        "MaxSize": 2,
    },
]

# Synthetic water patch input shared with tools/native-static-fixture.mjs and
# tests/fixtures/static-geometry-hashes.json.
STATIC_FIXTURE_WATER_DATA = {
    "cell": 32,
    "height": -4,
    "offset": [16, 16],
    "cells": [[0, 0, 0], [1, 0, 1]],
    "patches": [
        {
            "positions": [[0, -4, 0], [1, -4, 0], [0, -4, 1]],
            "uvs": [[0, 0], [1, 0], [0, 1]],
            "indices": [0, 1, 2],
        }
    ],
}


def sub_family_of(key: str) -> str:
    """Extract the sub-family prefix from a shader key (`<family>/<sub-family>-...`)."""
    rest = key.split("/", 1)[1] if "/" in key else key
    return rest.split("-", 1)[0]


def load_shader_manifests(repo_root: Path) -> dict[str, list[str]]:
    """Load and validate `rust/bfe-render/shaders/*/manifest.json` on disk."""
    shaders_dir = repo_root / "rust" / "bfe-render" / "shaders"
    disk_families: dict[str, list[str]] = {}
    seen_keys: set[str] = set()
    for entry in sorted(shaders_dir.iterdir(), key=lambda p: p.name):
        if not entry.is_dir():
            continue
        manifest_file = entry / "manifest.json"
        if not manifest_file.is_file():
            continue
        manifest = json.loads(manifest_file.read_text(encoding="utf-8"))
        if manifest.get("version") != 1 or not isinstance(manifest.get("programs"), list):
            raise RuntimeError(f"invalid shader family manifest: {manifest_file}")
        keys: list[str] = []
        for prog in manifest["programs"]:
            key = prog.get("key")
            if not isinstance(key, str) or not key.startswith(f"{entry.name}/") or key in seen_keys:
                raise RuntimeError(f"invalid or duplicate shader key in {manifest_file}: {key}")
            seen_keys.add(key)
            keys.append(key)
        disk_families[entry.name] = keys
    return disk_families


def encode_png_rgba(width: int, height: int, rgba_bytes: bytes, flip_vertical: bool = False) -> bytes:
    """Encode raw 8-bit RGBA pixels into a standard PNG image using stdlib zlib.

    OpenGL `glReadPixels` returns rows starting from the bottom-left corner,
    so `flip_vertical=True` converts framebuffer readbacks into top-down order.
    """
    stride = width * 4
    if len(rgba_bytes) != stride * height:
        raise ValueError(f"expected {stride * height} RGBA bytes, got {len(rgba_bytes)}")

    raw_rows = bytearray()
    for row_index in range(height):
        src_row = (height - 1 - row_index) if flip_vertical else row_index
        start = src_row * stride
        # Filter type 0 (None) prefixes each scanline.
        raw_rows.append(0)
        raw_rows.extend(rgba_bytes[start : start + stride])

    def chunk(tag: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(tag + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + tag + payload + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    idat = zlib.compress(bytes(raw_rows), level=6)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")


def verify_png_file(path: Path, expected_width: int, expected_height: int) -> dict:
    """Parse a PNG file header and verify that its dimensions match expectations."""
    data = path.read_bytes()
    if len(data) < 33 or data[:8] != b"\x89PNG\r\n\x1a\n":
        raise RuntimeError(f"{path}: invalid PNG signature")
    ihdr_len = struct.unpack(">I", data[8:12])[0]
    ihdr_tag = data[12:16]
    if ihdr_tag != b"IHDR" or ihdr_len != 13:
        raise RuntimeError(f"{path}: invalid IHDR chunk")
    width, height, bit_depth, color_type, comp, filt, interlace = struct.unpack(">IIBBBBB", data[16:29])
    if (width, height) != (expected_width, expected_height):
        raise RuntimeError(
            f"{path}: PNG size {width}x{height} does not match expected {expected_width}x{expected_height}"
        )
    if (bit_depth, color_type) != (8, 6):
        raise RuntimeError(f"{path}: expected 8-bit RGBA PNG, got depth={bit_depth} color_type={color_type}")
    return {
        "bytes": len(data),
        "width": width,
        "height": height,
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def make_checker_png(width: int, height: int, color_a: tuple[int, int, int, int], color_b: tuple[int, int, int, int]) -> bytes:
    """Build a small procedural RGBA PNG texture so drawn triangles have visible contrast."""
    pixels = bytearray()
    for y in range(height):
        for x in range(width):
            cell = ((x // 2) + (y // 2)) & 1
            pixels.extend(color_a if cell == 0 else color_b)
    return encode_png_rgba(width, height, bytes(pixels), flip_vertical=False)


def analyze_rgba(width: int, height: int, rgba_bytes: bytes) -> dict:
    """Measure pixel coverage and color diversity in a rendered RGBA8 framebuffer.

    Scene::draw_selected clears the target to `[0.04, 0.06, 0.09, 1.0]`, which
    quantizes to approximately `(10, 15, 23, 255)`. Any pixel whose RGB differs
    from that clear color by more than 3 counts is counted as drawn foreground.
    """
    total_pixels = width * height
    if len(rgba_bytes) != total_pixels * 4:
        raise RuntimeError(f"RGBA buffer length {len(rgba_bytes)} != {total_pixels * 4}")

    clear_r, clear_g, clear_b = 10, 15, 23
    non_clear = 0
    unique_colors = set()
    for offset in range(0, len(rgba_bytes), 4):
        r, g, b, a = rgba_bytes[offset : offset + 4]
        unique_colors.add((r, g, b, a))
        if abs(r - clear_r) > 3 or abs(g - clear_g) > 3 or abs(b - clear_b) > 3:
            non_clear += 1

    return {
        "pixels": total_pixels,
        "nonClearPixels": non_clear,
        "nonClearRatio": round(non_clear / max(1, total_pixels), 4),
        "uniqueColors": len(unique_colors),
    }


def perspective_webgl(fov_deg: float, aspect: float, near: float, far: float) -> list[float]:
    """Build a column-major WebGL perspective projection matrix matching three.js."""
    top = near * math.tan(math.radians(fov_deg) * 0.5)
    bottom = -top
    right = top * aspect
    left = -right
    x = 2.0 * near / (right - left)
    y = 2.0 * near / (top - bottom)
    a = (right + left) / (right - left)
    b = (top + bottom) / (top - bottom)
    c = -(far + near) / (far - near)
    d = -2.0 * far * near / (far - near)
    return [
        x, 0.0, 0.0, 0.0,
        0.0, y, 0.0, 0.0,
        a, b, c, -1.0,
        0.0, 0.0, d, 0.0,
    ]


def build_frame_bank(width: int, height: int, eye: list[float]) -> list[float]:
    """Construct the 200-float BfeFrame uniform bank looking along -Z from `eye`."""
    values = [0.0] * FRAME_FLOATS
    proj = perspective_webgl(60.0, width / height, 0.3, 20000.0)
    # Identity rotation view matrix translated by -eye.
    view = [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -eye[0], -eye[1], -eye[2], 1.0,
    ]

    def put(name: str, data: list[float]) -> None:
        at = FRAME_OFFSETS[name]
        values[at : at + len(data)] = data

    put("projectionMatrix", proj)
    put("viewMatrix", view)
    put("cameraPosition", eye)
    put("time", [0.25])
    put("sunDirection", [0.4, 0.8, 0.4])
    put("bump", [1.0])
    put("sunColor", [0.7, 0.7, 0.7])
    put("shadowStrength", [0.0])
    put("ambient", [0.6, 0.6, 0.6])
    put("aaSamples", [1.0])
    put("ambientTop", [0.6, 0.6, 0.6])
    put("terrainShade", [1.0])
    put("ambientBottom", [0.35, 0.35, 0.35])
    put("farAaSamples", [1.0])
    put("characterAmbient", [0.6, 0.6, 0.6])
    put("fogColor", [0.08, 0.12, 0.18])
    put("fade", [0.0, 20000.0])
    put("resolution", [float(width), float(height)])
    put("fogRange", [19999.0, 20000.0])
    return values


def pack_f32(values: list[float]) -> bytes:
    return struct.pack(f"<{len(values)}f", *values)


def pack_u32(values: list[int]) -> bytes:
    return struct.pack(f"<{len(values)}I", *values)


def write_synthetic_content(root: Path) -> None:
    """Populate `root` with a minimal synthetic content tree for map `room`.

    The foliage classes and water surface reuse the public synthetic fixtures
    from `tools/native-static-fixture.mjs` so no game assets are needed.
    """
    map_dir = root / "maps" / "room"
    common_dir = root / "common"
    side_dir = root / "sides" / "rep"
    actors_dir = side_dir / "actors"
    for directory in (map_dir, common_dir, side_dir, actors_dir):
        directory.mkdir(parents=True, exist_ok=True)

    # Distinct procedural RGBA textures for world, sky, prop, foliage, and water.
    (map_dir / "wall.png").write_bytes(make_checker_png(8, 8, (220, 140, 70, 255), (120, 70, 35, 255)))
    (map_dir / "sky.png").write_bytes(make_checker_png(8, 8, (70, 145, 235, 255), (180, 215, 250, 255)))
    (map_dir / "prop.png").write_bytes(make_checker_png(8, 8, (235, 85, 65, 255), (245, 195, 65, 255)))
    (map_dir / "grass.png").write_bytes(make_checker_png(8, 8, (65, 190, 80, 255), (40, 135, 55, 255)))
    (map_dir / "leaf.png").write_bytes(make_checker_png(8, 8, (95, 210, 95, 255), (50, 155, 60, 255)))
    (map_dir / "water.png").write_bytes(make_checker_png(8, 8, (45, 130, 215, 220), (85, 175, 240, 220)))
    (map_dir / "foam.png").write_bytes(make_checker_png(8, 8, (235, 245, 255, 255), (170, 205, 235, 255)))

    # World geometry buffer:
    # - Mesh 0 (vertices 0..4, indices 0..6): near lit quad at z = -12 spanning [-10..10, -4..8].
    # - Mesh 1 (vertices 4..8, indices 6..12): sky backdrop quad at z = -50 spanning [-80..80, -60..60].
    # Each vertex has 8 floats: position (3), normal (3), uv (2).
    world_vertices = [
        # Near world quad (4 vertices)
        -10.0, -4.0, -12.0, 0.0, 0.0, 1.0, 0.0, 0.0,
         10.0, -4.0, -12.0, 0.0, 0.0, 1.0, 1.0, 0.0,
         10.0,  8.0, -12.0, 0.0, 0.0, 1.0, 1.0, 1.0,
        -10.0,  8.0, -12.0, 0.0, 0.0, 1.0, 0.0, 1.0,
        # Sky quad (4 vertices; indices are local to the slice per Geometry::world)
        -80.0, -60.0, -50.0, 0.0, 0.0, 1.0, 0.0, 0.0,
         80.0, -60.0, -50.0, 0.0, 0.0, 1.0, 1.0, 0.0,
         80.0,  60.0, -50.0, 0.0, 0.0, 1.0, 1.0, 1.0,
        -80.0,  60.0, -50.0, 0.0, 0.0, 1.0, 0.0, 1.0,
    ]
    world_indices = [
        0, 1, 2, 0, 2, 3,
        0, 1, 2, 0, 2, 3,
    ]
    (map_dir / "geom.bin").write_bytes(pack_f32(world_vertices))
    (map_dir / "index.bin").write_bytes(pack_u32(world_indices))

    # Collision geometry: one ground triangle below the camera so BVH and
    # TriangleGrid construction succeed cleanly.
    coll_pos = [-16.0, -4.0, -16.0, 16.0, -4.0, -16.0, 0.0, -4.0, 16.0]
    coll_idx = [0, 1, 2]
    (map_dir / "collision.bin").write_bytes(pack_f32(coll_pos))
    (map_dir / "collision_index.bin").write_bytes(pack_u32(coll_idx))
    (map_dir / "collision_poly.bin").write_bytes(pack_u32([0]))
    (map_dir / "terrain_collision.bin").write_bytes(pack_f32(coll_pos))
    (map_dir / "terrain_collision_index.bin").write_bytes(pack_u32(coll_idx))

    # Placed prop model: a large lit quad centered around the origin and placed
    # at (0, 2, -5) directly in front of the inspection camera.
    prop_vertices = [
        -3.5, -3.5, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0,
         3.5, -3.5, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0,
         3.5,  3.5, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0,
        -3.5,  3.5, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0,
    ]
    prop_indices = [0, 1, 2, 0, 2, 3]
    (map_dir / "props.bin").write_bytes(pack_f32(prop_vertices))
    (map_dir / "props_index.bin").write_bytes(pack_u32(prop_indices))
    props_json = {
        "models": {
            "banner_panel": {
                "bones": [
                    {
                        "name": "root",
                        "parent": -1,
                        "local": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
                    }
                ],
                "parts": [
                    {
                        "texture": "prop",
                        "flags": 1,
                        "bone": "root",
                        "vertexOffset": 0,
                        "vertexCount": 4,
                        "indexOffset": 0,
                        "indexCount": 6,
                    }
                ],
            }
        },
        "instances": [
            {
                "base": "prop",
                "model": "banner_panel",
                "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 2, -5, 1],
            }
        ],
    }
    (map_dir / "props.json").write_text(json.dumps(props_json), encoding="utf-8")

    # Foliage classes reuse the first grasspatch and first leafpatch from
    # tools/native-static-fixture.mjs, with Texture/NumParts metadata attached.
    grass_def = dict(STATIC_FIXTURE_FOLIAGE[0], Texture="grass", NumParts=1, Alpha=1.0)
    leaf_def = dict(STATIC_FIXTURE_FOLIAGE[2], Texture="leaf", NumParts=1, Alpha=1.0)

    # Water block reuses STATIC_FIXTURE_WATER_DATA with a broad surface patch
    # visible from the default inspection camera.
    water_block = {
        "cell": STATIC_FIXTURE_WATER_DATA["cell"],
        "grid": 2,
        "scale": 1.0,
        "height": -1.5,
        "offset": [-32.0, -32.0],
        "cells": STATIC_FIXTURE_WATER_DATA["cells"],
        "patches": [
            {
                "positions": [
                    [-12.0, -1.5, -2.0],
                    [12.0, -1.5, -2.0],
                    [12.0, -1.5, -18.0],
                    [-12.0, -1.5, -18.0],
                ],
                "uvs": [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                "indices": [0, 1, 2, 0, 2, 3],
            }
        ],
        "look": {
            "MainTexture": ["water"],
            "FoamTexture": ["foam"],
            "PatchDivisions": [3, 7],
            "Tile": [2.0, 2.0],
            "Velocity": [0.05, 0.05],
            "RefractionColor": [70, 150, 230, 235],
            "WindDirection": [1.0, 0.0],
            "WindSpeed": [4.0],
        },
    }

    map_json = {
        "mission": {"name": "room", "era": "cw", "sides": ["rep"]},
        "lighting": {"sunDirection": [0.2, -0.9, 0.3]},
        "distance": {"farScene": [50.0, 3000.0]},
        "textures": {
            "wall": "wall.png",
            "sky": "sky.png",
            "prop": "prop.png",
            "grass": "grass.png",
            "leaf": "leaf.png",
            "water": "water.png",
            "foam": "foam.png",
        },
        "meshes": [
            {
                "texture": "wall",
                "vertexOffset": 0,
                "vertexCount": 4,
                "indexOffset": 0,
                "indexCount": 6,
                "stride": 8,
                "attributes": ["position", "normal", "uv"],
                "flags": 1,
            },
            {
                "texture": "sky",
                "sky": True,
                "skyLayer": 0,
                "vertexOffset": 128,
                "vertexCount": 4,
                "indexOffset": 24,
                "indexCount": 6,
                "stride": 8,
                "attributes": ["position", "normal", "uv"],
                "flags": 0,
            },
        ],
        "foliage": {
            "classes": {
                "grass_fixture": grass_def,
                "leaf_fixture": leaf_def,
            },
            "instances": [
                {
                    "class": "grass_fixture",
                    "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 1.5, -5.0, 1],
                    "tint": [0.85, 0.95, 0.85],
                    "terrain": False,
                },
                {
                    "class": "leaf_fixture",
                    "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -1.0, 0.0, -8.0, 1],
                    "tint": [0.8, 0.95, 0.8],
                    "terrain": False,
                },
            ],
        },
        "water": water_block,
    }
    (map_dir / "map.json").write_text(json.dumps(map_json), encoding="utf-8")

    # Shared common and side definitions required by bfe_content::battle::Battle::load.
    (common_dir / "anims.json").write_text(
        json.dumps({"fps": 30, "bones": ["root"], "clips": {}}),
        encoding="utf-8",
    )
    (common_dir / "anims.bin").write_bytes(b"")
    (common_dir / "common.json").write_text("{}", encoding="utf-8")
    (common_dir / "effects.json").write_text("{}", encoding="utf-8")
    (common_dir / "fxmodels.bin").write_bytes(b"")
    (common_dir / "fxmodels_index.bin").write_bytes(b"")

    (side_dir / "units.json").write_text(
        json.dumps(
            {
                "units": {
                    "rep_inf_jet_trooper": {
                        "base": "soldier",
                        "model": "trooper",
                    },
                    "rep_inf_clone_trooper": {
                        "base": "soldier",
                        "model": "trooper",
                    },
                }
            }
        ),
        encoding="utf-8",
    )
    (side_dir / "weapons.json").write_text(
        json.dumps({"models": {}, "firePoints": {}}),
        encoding="utf-8",
    )
    (side_dir / "weapons.bin").write_bytes(b"")
    (side_dir / "weapons_index.bin").write_bytes(b"")
    (actors_dir / "trooper.json").write_text(
        json.dumps(
            {
                "bones": [
                    {
                        "name": "root",
                        "parent": -1,
                        "local": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
                    }
                ],
                "parts": [],
            }
        ),
        encoding="utf-8",
    )
    (actors_dir / "trooper.bin").write_bytes(b"")
    (actors_dir / "trooper_index.bin").write_bytes(b"")


def js_normalize_numbers(value):
    """Match JavaScript JSON.stringify number formatting for integer-valued floats."""
    if isinstance(value, float):
        return int(value) if value.is_integer() else value
    if isinstance(value, list):
        return [js_normalize_numbers(item) for item in value]
    if isinstance(value, dict):
        return {k: js_normalize_numbers(value[k]) for k in sorted(value.keys())}
    return value


def check_static_fixture_probe(repo_root: Path, static_fixture_bin: Path, records: list[dict]) -> None:
    """Verify `static-fixture` against the frozen hashes and synthetic foliage definitions."""
    t0 = time.monotonic()
    norm = math.sqrt(0.2 * 0.2 + (-0.9) * (-0.9) + 0.3 * 0.3)
    sun = [0.2 / norm, -0.9 / norm, 0.3 / norm]

    for index, definition in enumerate(STATIC_FIXTURE_FOLIAGE):
        payload = json.dumps({"kind": "foliage", "definition": definition, "sun": sun})
        proc = subprocess.run(
            [str(static_fixture_bin)],
            input=payload,
            text=True,
            capture_output=True,
            check=True,
        )
        cards = json.loads(proc.stdout)
        expected_len = definition["NumParticles"] * 9
        if len(cards) != expected_len or not all(isinstance(x, (int, float)) and math.isfinite(x) for x in cards):
            raise RuntimeError(f"static-fixture foliage[{index}] returned invalid card array (len={len(cards)})")

    hashes_path = repo_root / "tests" / "fixtures" / "static-geometry-hashes.json"
    for entry in json.loads(hashes_path.read_text(encoding="utf-8")):
        proc = subprocess.run(
            [str(static_fixture_bin)],
            input=json.dumps(entry["input"]),
            text=True,
            capture_output=True,
            check=True,
        )
        actual = js_normalize_numbers(json.loads(proc.stdout))
        canonical = json.dumps(actual, separators=(",", ":"))
        digest = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
        if digest != entry["hash"]:
            raise RuntimeError(
                f"static-fixture hash mismatch for {entry['input']}: got {digest}, expected {entry['hash']}"
            )

    records.append(
        {
            "step": "static-fixture",
            "ok": True,
            "foliageDefinitions": len(STATIC_FIXTURE_FOLIAGE),
            "frozenGeometryHashes": 4,
            "elapsedMs": round((time.monotonic() - t0) * 1000.0, 1),
        }
    )


def check_fingerprint_probe(fingerprint_bin: Path, content_dir: Path, records: list[dict]) -> None:
    """Verify native gameplay content hashing on the synthetic `room` content tree."""
    t0 = time.monotonic()
    proc = subprocess.run(
        [str(fingerprint_bin), str(content_dir), "map=room&side=rep"],
        text=True,
        capture_output=True,
        check=True,
    )
    payload = json.loads(proc.stdout)
    digest = payload.get("hash", "")
    manifest = payload.get("manifest", [])
    if not digest.startswith("bfe-content-sha256-") or len(digest) != len("bfe-content-sha256-") + 64 or not manifest:
        raise RuntimeError(f"unexpected fingerprint output: {proc.stdout}")
    records.append(
        {
            "step": "fingerprint",
            "ok": True,
            "hash": digest,
            "files": len(manifest),
            "elapsedMs": round((time.monotonic() - t0) * 1000.0, 1),
        }
    )


def compose_grid_rgba(width: int, height: int, panels: list[bytes]) -> tuple[int, int, bytes]:
    """Combine four (width x height) RGBA panels into a 2x2 contact sheet."""
    out_w = width * 2
    out_h = height * 2
    out = bytearray(out_w * out_h * 4)
    stride = width * 4
    out_stride = out_w * 4
    for idx, panel in enumerate(panels):
        gx = (idx % 2) * width * 4
        gy = (idx // 2) * height
        for row in range(height):
            # Flip each OpenGL panel vertically so every quadrant is top-down.
            src_row = height - 1 - row
            dst_offset = (gy + row) * out_stride + gx
            src_offset = src_row * stride
            out[dst_offset : dst_offset + stride] = panel[src_offset : src_offset + stride]
    return out_w, out_h, bytes(out)


def run_render_smoke(
    repo_root: Path,
    render_smoke_bin: Path,
    content_dir: Path,
    work_dir: Path,
    width: int,
    height: int,
    backend: str,
    env: dict[str, str],
    stream_lines: list[str],
    records: list[dict],
) -> None:
    """Run `render-smoke` to compile/link all 1,046 variants and draw all 6 families."""
    t0 = time.monotonic()
    shader_manifests = load_shader_manifests(repo_root)

    # Verify the shader manifest catalog against EXPECTED_FAMILY_COUNTS and EXPECTED_SUB_FAMILIES.
    manifest_total = sum(len(keys) for keys in shader_manifests.values())
    if manifest_total != EXPECTED_TOTAL_VARIANTS:
        raise RuntimeError(
            f"Shader manifest total {manifest_total} != expected {EXPECTED_TOTAL_VARIANTS}"
        )

    frame_json = work_dir / "frame.json"
    frame_payload = {
        "size": [width, height],
        "values": build_frame_bank(width, height, [0.0, 2.0, 3.0]),
        "foliageRandom": None,
    }
    frame_json.write_text(json.dumps(frame_payload), encoding="utf-8")

    smoke_jsonl = work_dir / "render-smoke-events.jsonl"
    families_png = work_dir / "render-smoke-families.png"
    families_dir = work_dir / "families"

    proc = subprocess.run(
        [
            str(render_smoke_bin),
            "--content",
            str(content_dir),
            "--map",
            "room",
            "--frame-json",
            str(frame_json),
            "--width",
            str(width),
            "--height",
            str(height),
            "--backend",
            backend,
            "--output",
            str(smoke_jsonl),
            "--png",
            str(families_png),
            "--png-dir",
            str(families_dir),
        ],
        env=env,
        text=True,
        capture_output=True,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"render-smoke failed (exit {proc.returncode}):\nstderr: {proc.stderr.strip()}"
        )

    raw_lines = [
        line.strip()
        for line in smoke_jsonl.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]
    if not raw_lines:
        raise RuntimeError("render-smoke wrote an empty JSONL file")
    stream_lines.extend(raw_lines)

    events = [json.loads(line) for line in raw_lines]
    variant_starts = [e for e in events if e.get("stage") == "variant-start"]
    variant_results = [e for e in events if e.get("stage") == "variant-result"]
    family_results = [e for e in events if e.get("stage") == "family-result"]
    family_draws = [e for e in events if e.get("stage") == "family-draw"]
    final_report = next((e for e in reversed(events) if "runs" in e), None)
    if final_report is None or not final_report.get("runs"):
        raise RuntimeError("render-smoke did not emit a final `runs` summary record")

    run_summary = final_report["runs"][0]
    if not run_summary.get("ok"):
        raise RuntimeError(f"render-smoke run reported failure: {run_summary}")

    if len(variant_starts) != EXPECTED_TOTAL_VARIANTS or len(variant_results) != EXPECTED_TOTAL_VARIANTS:
        raise RuntimeError(
            f"expected {EXPECTED_TOTAL_VARIANTS} variant-start/result records, "
            f"got {len(variant_starts)} / {len(variant_results)}"
        )

    rejected = [v for v in variant_results if not v.get("accepted") or v.get("cached") or v.get("shaders") != 2 or v.get("links") != 1]
    if rejected:
        raise RuntimeError(f"render-smoke had rejected or cached variants: {rejected[:5]}")

    # Cross-check family and sub-family counts between `tools/renderer-smoke-manifests.mjs`
    # and the native `render-smoke` execution, and print the comparison.
    native_by_family = {f["family"]: f for f in family_results}
    comparison_summary: dict[str, dict] = {}
    print("=== Shader Manifest Cross-Check (tools/renderer-smoke-manifests.mjs vs native render-smoke) ===")
    for family in EXPECTED_FAMILY_ORDER:
        if family not in shader_manifests or family not in native_by_family:
            raise RuntimeError(f"missing shader family '{family}' in cross-check")
        e_keys = shader_manifests[family]
        n_entry = native_by_family[family]
        n_keys = [v["key"] for v in n_entry["variants"]]
        expected_count = EXPECTED_FAMILY_COUNTS[family]
        if len(e_keys) != expected_count or len(n_keys) != expected_count or e_keys != n_keys:
            raise RuntimeError(
                f"family '{family}' mismatch: manifest={len(e_keys)}, native={len(n_keys)}, expected={expected_count}"
            )

        e_subs: dict[str, int] = {}
        for key in e_keys:
            sub = sub_family_of(key)
            e_subs[sub] = e_subs.get(sub, 0) + 1
        n_subs = dict(n_entry.get("subFamilies", {}))
        expected_subs = EXPECTED_SUB_FAMILIES[family]
        if e_subs != expected_subs or n_subs != expected_subs:
            raise RuntimeError(
                f"sub-family mismatch in '{family}': manifest={e_subs}, native={n_subs}, expected={expected_subs}"
            )

        subs_formatted = ", ".join(f"{k}:{v}" for k, v in sorted(n_subs.items()))
        print(
            f"  {family:<10} manifest={len(e_keys):>3}  native={len(n_keys):>3}  "
            f"compileMs={n_entry['compileMs']:>7.2f}  subFamilies=[{subs_formatted}]"
        )
        comparison_summary[family] = {
            "manifest": len(e_keys),
            "native": len(n_keys),
            "compileMs": n_entry["compileMs"],
            "subFamilies": n_subs,
        }
    print(
        f"  {'TOTAL':<10} manifest={manifest_total:>3}  native={len(variant_results):>3}  "
        f"compileMs={run_summary['compileMs']:>7.2f}"
    )

    # Verify all 6 representative family draws.
    if len(family_draws) != len(EXPECTED_FAMILY_ORDER):
        raise RuntimeError(f"expected {len(EXPECTED_FAMILY_ORDER)} family-draw records, got {len(family_draws)}")
    draws_by_family = {d["family"]: d for d in family_draws}
    for family in EXPECTED_FAMILY_ORDER:
        draw = draws_by_family.get(family)
        if not draw or not draw.get("ok"):
            raise RuntimeError(f"family draw failed for '{family}': {draw}")
        if draw.get("nonClearPixels", 0) <= 0 or draw.get("uniqueColors", 0) < 2:
            raise RuntimeError(f"family '{family}' produced blank output: {draw}")
        if draw.get("glError", 0) != 0 or draw.get("fault") is not None:
            raise RuntimeError(f"family '{family}' reported GL error or renderer fault: {draw}")
        family_png = families_dir / f"family-{family}.png"
        verify_png_file(family_png, width, height)

    sheet_meta = verify_png_file(families_png, width * 2, height * 3)
    records.append(
        {
            "step": "manifest-crosscheck",
            "ok": True,
            "totalVariants": EXPECTED_TOTAL_VARIANTS,
            "families": comparison_summary,
        }
    )
    records.append(
        {
            "step": "render-smoke",
            "ok": True,
            "renderer": run_summary["renderer"],
            "vendor": run_summary["vendor"],
            "version": run_summary["version"],
            "totalVariants": run_summary["totalVariants"],
            "acceptedVariants": run_summary["acceptedVariants"],
            "compiledShaders": run_summary["compiledShaders"],
            "linkedPrograms": run_summary["linkedPrograms"],
            "familiesDrawn": run_summary["familiesDrawn"],
            "compileMs": run_summary["compileMs"],
            "drawMs": run_summary["drawMs"],
            "familyDraws": [
                {
                    "family": d["family"],
                    "ms": d["ms"],
                    "draws": d["draws"],
                    "nonClearPixels": d["nonClearPixels"],
                    "uniqueColors": d["uniqueColors"],
                    "digest": d["digest"],
                }
                for d in family_draws
            ],
            "contactSheetPng": sheet_meta,
            "elapsedMs": round((time.monotonic() - t0) * 1000.0, 1),
        }
    )


def run_static_frame_components(
    static_frame_bin: Path,
    content_dir: Path,
    work_dir: Path,
    png_out: Path,
    width: int,
    height: int,
    env: dict[str, str],
    records: list[dict],
) -> None:
    """Run `static-frame` over EGL surfaceless software GL for sky, props, foliage, and water."""
    frame_json = work_dir / "frame.json"
    frame_payload = {
        "size": [width, height],
        "values": build_frame_bank(width, height, [0.0, 2.0, 3.0]),
        "foliageRandom": None,
    }
    frame_json.write_text(json.dumps(frame_payload), encoding="utf-8")

    panels: list[bytes] = []
    for component in ("sky", "props", "foliage", "water"):
        t0 = time.monotonic()
        rgba_path = work_dir / f"{component}.rgba"
        proc = subprocess.run(
            [
                str(static_frame_bin),
                str(content_dir),
                "room",
                str(frame_json),
                str(rgba_path),
                component,
            ],
            env=env,
            text=True,
            capture_output=True,
        )
        if proc.returncode != 0:
            raise RuntimeError(
                f"static-frame ({component}) failed (exit {proc.returncode}): {proc.stderr.strip()}"
            )
        summary = json.loads(proc.stdout.strip().splitlines()[-1])
        rgba_bytes = rgba_path.read_bytes()
        stats = analyze_rgba(width, height, rgba_bytes)
        if summary.get("admitted", 0) < 2 or summary.get("draws", 0) <= 0:
            raise RuntimeError(f"static-frame ({component}) admitted/draw check failed: {summary}")
        if stats["nonClearPixels"] <= 0 or stats["uniqueColors"] < 2:
            raise RuntimeError(f"static-frame ({component}) produced a blank image: {stats}")

        panels.append(rgba_bytes)
        records.append(
            {
                "step": f"static-frame:{component}",
                "ok": True,
                "hash": summary["hash"],
                "admitted": summary["admitted"],
                "draws": summary["draws"],
                "nonClearPixels": stats["nonClearPixels"],
                "uniqueColors": stats["uniqueColors"],
                "elapsedMs": round((time.monotonic() - t0) * 1000.0, 1),
            }
        )

    # Write a 2x2 contact sheet PNG covering all four rendered components and
    # verify its PNG header, dimensions, and non-blank content on disk.
    sheet_w, sheet_h, sheet_rgba = compose_grid_rgba(width, height, panels)
    png_out.parent.mkdir(parents=True, exist_ok=True)
    png_out.write_bytes(encode_png_rgba(sheet_w, sheet_h, sheet_rgba, flip_vertical=False))
    png_meta = verify_png_file(png_out, sheet_w, sheet_h)
    sheet_stats = analyze_rgba(sheet_w, sheet_h, sheet_rgba)
    if sheet_stats["nonClearPixels"] <= 0 or sheet_stats["uniqueColors"] < 4:
        raise RuntimeError(f"composite PNG is blank or lacks expected component colors: {sheet_stats}")
    records.append(
        {
            "step": "png-verification",
            "ok": True,
            "path": str(png_out),
            **png_meta,
            **sheet_stats,
        }
    )


def run_windowed_client_smoke(
    repo_root: Path,
    client_bin: Path,
    content_dir: Path,
    env: dict[str, str],
    records: list[dict],
) -> None:
    """Run `bfe-client --map room` under Xvfb and `native-client-window.py`."""
    t0 = time.monotonic()
    window_helper = repo_root / "tools" / "native-client-window.py"
    cmd = [
        sys.executable,
        str(window_helper),
        str(client_bin),
        "--map",
        "room",
        str(content_dir),
        "--no-audio",
        "--seconds",
        "0.5",
    ]
    if not env.get("DISPLAY"):
        xvfb = shutil.which("xvfb-run")
        if not xvfb:
            raise RuntimeError("DISPLAY is unset and xvfb-run is not installed")
        cmd = [xvfb, "-a", "-s", "-screen 0 1280x720x24"] + cmd

    proc = subprocess.run(cmd, env=env, text=True, capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"bfe-client --map room failed (exit {proc.returncode}):\nstdout: {proc.stdout}\nstderr: {proc.stderr}"
        )

    lines = [line.strip() for line in proc.stdout.splitlines() if line.strip().startswith("{")]
    if not lines:
        raise RuntimeError(f"bfe-client --map room did not print a JSON summary:\n{proc.stdout}\n{proc.stderr}")
    summary = json.loads(lines[-1])
    if summary.get("frames", 0) < 1 or summary.get("admitted", 0) < 2 or summary.get("draws", 0) <= 0:
        raise RuntimeError(f"bfe-client --map room summary failed checks: {summary}")
    for key in ("initial", "final"):
        digest = summary.get(key)
        if not isinstance(digest, str) or len(digest) != 64:
            raise RuntimeError(f"bfe-client --map room missing {key} framebuffer hash: {summary}")

    records.append(
        {
            "step": "bfe-client-map-window",
            "ok": True,
            "frames": summary["frames"],
            "admitted": summary["admitted"],
            "draws": summary["draws"],
            "initial": summary["initial"],
            "final": summary["final"],
            "elapsedMs": round((time.monotonic() - t0) * 1000.0, 1),
        }
    )


def resolve_binary(explicit: str | None, candidates: list[Path]) -> Path | None:
    if explicit:
        path = Path(explicit)
        if not path.is_file():
            raise FileNotFoundError(f"specified binary does not exist: {path}")
        return path
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--render-smoke-bin", help="Path to render-smoke executable")
    parser.add_argument("--client-bin", help="Path to bfe-client executable")
    parser.add_argument("--static-frame-bin", help="Path to static-frame executable")
    parser.add_argument("--static-fixture-bin", help="Path to static-fixture example executable")
    parser.add_argument("--fingerprint-bin", help="Path to fingerprint example executable")
    parser.add_argument(
        "--backend",
        default=os.environ.get("BFE_SMOKE_BACKEND", "llvmpipe"),
        help="Backend tag recorded in JSONL output (default: llvmpipe)",
    )
    parser.add_argument(
        "--hardware-gl",
        action="store_true",
        help="Do not force LIBGL_ALWAYS_SOFTWARE=1 (for hardware GPU comparison runs)",
    )
    parser.add_argument(
        "--output",
        "--report",
        dest="output",
        default=os.environ.get("BFE_SMOKE_OUTPUT", "renderer-smoke-native.jsonl"),
        help="Path to write JSON-lines smoke report",
    )
    parser.add_argument(
        "--png-out",
        "--png",
        dest="png_out",
        default=os.environ.get("BFE_SMOKE_PNG", "renderer-smoke-native.png"),
        help="Path to write rendered PNG artifact",
    )
    parser.add_argument("--width", type=int, default=320, help="Viewport width per component")
    parser.add_argument("--height", type=int, default=180, help="Viewport height per component")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    target_dirs = [
        repo_root / "rust" / "target" / "debug",
        repo_root / "rust" / "target" / "release",
    ]
    render_smoke_bin = resolve_binary(args.render_smoke_bin, [d / "render-smoke" for d in target_dirs])
    client_bin = resolve_binary(args.client_bin, [d / "bfe-client" for d in target_dirs])
    static_frame_bin = resolve_binary(args.static_frame_bin, [d / "static-frame" for d in target_dirs])
    static_fixture_bin = resolve_binary(
        args.static_fixture_bin,
        [d / "examples" / "static-fixture" for d in target_dirs],
    )
    fingerprint_bin = resolve_binary(
        args.fingerprint_bin,
        [d / "examples" / "fingerprint" for d in target_dirs],
    )

    if not render_smoke_bin or not client_bin or not static_frame_bin:
        raise SystemExit(
            "missing render-smoke, bfe-client, or static-frame binary; build with: "
            "cargo build --manifest-path rust/Cargo.toml -p bfe-client --bin render-smoke --bin bfe-client --bin static-frame"
        )

    # Unoptimized debug builds of bfe-client place SceneDriver (Scene + Bridge)
    # on the main thread stack before entering winit. Raise RLIMIT_STACK to at
    # least 64 MiB so child processes do not overflow the default 8 MiB stack.
    if resource is not None:
        try:
            soft, hard = resource.getrlimit(resource.RLIMIT_STACK)
            wanted = 64 * 1024 * 1024
            if soft != resource.RLIM_INFINITY and soft < wanted:
                target = wanted if hard == resource.RLIM_INFINITY else min(wanted, hard)
                resource.setrlimit(resource.RLIMIT_STACK, (target, hard))
        except (ValueError, OSError):
            pass

    env = dict(os.environ)
    if not args.hardware_gl:
        env["LIBGL_ALWAYS_SOFTWARE"] = "1"
        env["GALLIUM_DRIVER"] = env.get("GALLIUM_DRIVER", "llvmpipe")

    stream_lines: list[str] = []
    records: list[dict] = []
    output_path = Path(args.output)
    png_path = Path(args.png_out)

    try:
        with tempfile.TemporaryDirectory(prefix="bfe-native-smoke-") as tmp:
            tmp_path = Path(tmp)
            content_dir = tmp_path / "content"
            work_dir = tmp_path / "work"
            work_dir.mkdir(parents=True, exist_ok=True)
            write_synthetic_content(content_dir)

            if static_fixture_bin:
                check_static_fixture_probe(repo_root, static_fixture_bin, records)
            if fingerprint_bin:
                check_fingerprint_probe(fingerprint_bin, content_dir, records)

            run_render_smoke(
                repo_root,
                render_smoke_bin,
                content_dir,
                work_dir,
                args.width,
                args.height,
                args.backend,
                env,
                stream_lines,
                records,
            )
            run_static_frame_components(
                static_frame_bin,
                content_dir,
                work_dir,
                png_path,
                args.width,
                args.height,
                env,
                records,
            )
            run_windowed_client_smoke(repo_root, client_bin, content_dir, env, records)
    except Exception as exc:
        records.append({"step": "error", "ok": False, "error": str(exc)})
        all_lines = stream_lines + [json.dumps(r) for r in records]
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text("\n".join(all_lines) + "\n", encoding="utf-8")
        for record in records:
            print(json.dumps(record), file=sys.stderr)
        return 1

    all_lines = stream_lines + [json.dumps(r) for r in records]
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text("\n".join(all_lines) + "\n", encoding="utf-8")
    for record in records:
        print(json.dumps(record))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
