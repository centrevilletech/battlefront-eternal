#!/usr/bin/env python3
"""Faithfully enlarge extracted HUD art to lossless WebP; never trace glyphs.

Enlarges every font page, the textures named like HUD art (hud*, *_icon,
map_mask, flag_icon), and the textures common.json lists in rasterHud (the
spawn screen's windows, room and logos).

Requires Pillow. Outputs are private, derived assets under public/common/.
"""
import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image


def enlarge(image, scale):
    # Filter premultiplied color so hidden RGB in transparent texels cannot
    # produce dark fringes. Retain the original graded coverage of tiny type.
    return image.convert("RGBa").resize(
        (image.width * scale, image.height * scale), Image.Resampling.LANCZOS
    ).convert("RGBA")


def convert(common, scale=4):
    common = Path(common)
    data = json.loads((common / "common.json").read_text())
    output = common / "raster-hud"
    output.mkdir(parents=True, exist_ok=True)
    pages = {page for font in data.get("fonts", {}).values() for page in font["pages"]}
    names = pages | {name for name in data["textures"] if name.startswith("hud") or name.endswith("_icon") or name in ("map_mask", "flag_icon")}
    names |= set(data.get("rasterHud", []))
    manifest = {"version": 1, "scale": scale, "textures": {}}
    for name in sorted(names):
        relative = data["textures"].get(name)
        if not relative:
            continue
        source = common / relative
        with Image.open(source) as original:
            image = original.convert("RGBA")
        result = enlarge(image, scale)
        digest = hashlib.sha256(source.read_bytes()).hexdigest()
        filename = f"{name}-{digest[:12]}-{scale}x.webp"
        result.save(output / filename, "WEBP", lossless=True, exact=True, method=6)
        manifest["textures"][relative] = {"file": filename, "width": image.width, "height": image.height, "sha256": digest}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"  raster HUD: {len(manifest['textures'])} lossless WebP atlases at {scale}x", flush=True)
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--common", type=Path, default=Path("public/common"))
    parser.add_argument("--scale", type=int, choices=(2, 4), default=4)
    args = parser.parse_args()
    convert(args.common, args.scale)
