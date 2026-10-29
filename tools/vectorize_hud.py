#!/usr/bin/env python3
"""Trace extracted HUD art/font pages to SVG and bitmap glyphs to outline TTF.

Run after extract.py. Outputs stay beside the private assets, under public/.
Original PNGs and font metrics are never overwritten. Color and alpha are
traced separately: VTracer's color conversion alone discards partial opacity.
"""
import argparse
import hashlib
import html
import json
import re
import xml.etree.ElementTree as ET
from pathlib import Path

from PIL import Image
import vtracer
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.svgLib.path import parse_path

ART = ["hud1", "hud2", "hud_health", "hud_flag_timer", "hud_flag_icon", "player_icon", "map_mask", "ambush_icon", "hud_jetpack_icon"]
NS = "http://www.w3.org/2000/svg"
ET.register_namespace("", NS)


def trace(image, binary=False):
    return ET.fromstring(vtracer.convert_pixels_to_svg(
        list(image.get_flattened_data() if hasattr(image, "get_flattened_data") else image.getdata()), image.size, colormode="binary" if binary else "color",
        hierarchical="stacked", mode="polygon" if binary else "spline", filter_speckle=0,
        color_precision=7, layer_difference=8, corner_threshold=90,
        length_threshold=1.0, max_iterations=10, splice_threshold=45, path_precision=3,
    ))


def vector_image(image):
    image = image.convert("RGBA")
    width, height = image.size
    color = image.copy(); color.putalpha(255)
    alpha = image.getchannel("A").convert("RGBA")
    root = ET.Element(f"{{{NS}}}svg", {"width": str(width), "height": str(height), "viewBox": f"0 0 {width} {height}"})
    defs = ET.SubElement(root, f"{{{NS}}}defs")
    mask = ET.SubElement(defs, f"{{{NS}}}mask", {"id": "opacity", "maskUnits": "userSpaceOnUse", "x": "0", "y": "0", "width": str(width), "height": str(height), "style": "mask-type:luminance"})
    mask.extend(list(trace(alpha)))
    group = ET.SubElement(root, f"{{{NS}}}g", {"mask": "url(#opacity)"})
    group.extend(list(trace(color)))
    return ET.tostring(root, encoding="unicode")


def glyph_outline(image):
    # Black foreground, white background; retain counters and single-pixel dots.
    alpha = image.convert("RGBA").getchannel("A")
    # Trace the original antialias contour at subpixel resolution. Polygon
    # fitting retains the narrow font's straight stems; spline fitting at the
    # native bitmap size rounded corners and distorted letters such as E/R.
    alpha = alpha.resize((alpha.width * 4, alpha.height * 4), Image.Resampling.LANCZOS)
    binary = alpha.point(lambda value: 0 if value >= 128 else 255).convert("RGBA")
    return trace(binary, binary=True)


def vector_font(name, font, pages, output):
    em = 1024
    scale = em / font["lineHeight"]
    capitals = [g[6] for g in font["glyphs"] if 48 <= g[0] <= 57 or 65 <= g[0] <= 90]
    baseline = font.get("baseline") or max(capitals, default=font["lineHeight"])
    glyph_order = [".notdef"]
    empty = TTGlyphPen(None).glyph()
    glyphs = {".notdef": empty}
    metrics = {".notdef": (round(em / 2), 0)}
    cmap = {}
    symbols = ET.Element(f"{{{NS}}}svg")
    defs = ET.SubElement(symbols, f"{{{NS}}}defs")
    atlases = {page: ET.Element(f"{{{NS}}}svg", {"width": str(pages[page].width), "height": str(pages[page].height), "viewBox": f"0 0 {pages[page].width} {pages[page].height}"}) for page in font["pages"]}
    for record in font["glyphs"]:
        code, page, advance, x0, x1, y0, y1, u0, v0, u1, v1 = record
        glyph_name = f"uni{code:04X}"
        glyph_order.append(glyph_name)
        cmap[code] = glyph_name
        pen = TTGlyphPen(None)
        source = pages[font["pages"][page]]
        box = tuple(round(v) for v in (u0 * source.width, v0 * source.height, u1 * source.width, v1 * source.height))
        width, height = box[2] - box[0], box[3] - box[1]
        symbol = ET.SubElement(defs, f"{{{NS}}}symbol", {"id": glyph_name, "viewBox": f"0 0 {max(1, width)} {max(1, height)}", "data-advance": str(advance)})
        if width > 0 and height > 0 and x1 > x0 and y1 > y0:
            outline = glyph_outline(source.crop(box))
            symbol.set("viewBox", f"0 0 {width * 4} {height * 4}")
            atlas = ET.SubElement(atlases[font["pages"][page]], f"{{{NS}}}g", {"transform": f"translate({box[0]},{box[1]}) scale(0.25)"})
            sx, sy = (x1 - x0) * scale / (width * 4), (y1 - y0) * scale / (height * 4)
            for path in outline.findall(f"{{{NS}}}path"):
                symbol.append(path)
                atlas_path = ET.SubElement(atlas, f"{{{NS}}}path", dict(path.attrib))
                atlas_path.set("fill", "#ffffff")
                match = re.fullmatch(r"translate\(([-.\d]+),\s*([-.\d]+)\)", path.get("transform", "translate(0,0)"))
                if not match:
                    raise ValueError(f"Unexpected tracer transform: {path.get('transform')}")
                tx, ty = map(float, match.groups())
                transform = (sx, 0, 0, -sy, x0 * scale + tx * sx, (baseline - y0) * scale - ty * sy)
                parse_path(path.get("d", ""), TransformPen(Cu2QuPen(pen, max_err=0.5), transform))
        glyph = pen.glyph()
        glyph.recalcBounds(None)
        glyphs[glyph_name] = glyph
        metrics[glyph_name] = (round(advance * scale), getattr(glyph, "xMin", 0))
    builder = FontBuilder(em, isTTF=True)
    builder.setupGlyphOrder(glyph_order)
    builder.setupCharacterMap(cmap)
    builder.setupGlyf(glyphs)
    builder.setupHorizontalMetrics(metrics)
    ascent = round(baseline * scale)
    descent = -round(max(0, max(g[6] for g in font["glyphs"]) - baseline) * scale)
    builder.setupHorizontalHeader(ascent=ascent, descent=descent)
    builder.setupNameTable({"familyName": f"Original {name} Vector", "styleName": "Regular", "uniqueFontIdentifier": f"{name}-vector-1", "fullName": f"Original {name} Vector", "psName": f"{name}-Vector"})
    builder.setupOS2(sTypoAscender=ascent, sTypoDescender=descent, sTypoLineGap=0, usWinAscent=ascent, usWinDescent=-descent)
    builder.setupPost()
    builder.font.recalcTimestamp = False
    builder.font["head"].created = builder.font["head"].modified = 2082844800
    builder.save(output / f"{name}.ttf")
    (output / f"{name}-symbols.svg").write_text(ET.tostring(symbols, encoding="unicode"))
    for page, atlas in atlases.items():
        (output / f"{page}.svg").write_text(ET.tostring(atlas, encoding="unicode"))
    return {**font, "outlineBaseline": baseline, "ttf": f"{name}.ttf", "symbols": f"{name}-symbols.svg", "unitsPerEm": em}


def convert(common, output, art=ART):
    common = Path(common); output = Path(output)
    data = json.loads((common / "common.json").read_text())
    output.mkdir(parents=True, exist_ok=True)
    fonts = data.get("fonts", {})
    names = set(art) | {page for font in fonts.values() for page in font["pages"]}
    manifest = {"version": 1, "generator": "vectorize_hud.py", "textures": {}, "fonts": {}}
    images = {}
    for name in sorted(names):
        if name not in data["textures"]:
            raise ValueError(f"Missing source texture {name}")
        path = common / data["textures"][name]
        image = Image.open(path).convert("RGBA"); images[name] = image
        svg_name = f"{name}.svg"
        (output / svg_name).write_text(vector_image(image))
        manifest["textures"][data["textures"][name]] = {"svg": svg_name, "width": image.width, "height": image.height, "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        print(f"SVG {name}", flush=True)
    for name, font in fonts.items():
        manifest["fonts"][name] = vector_font(name, font, images, output)
        print(f"TTF {name}: {len(font['glyphs'])} glyphs", flush=True)
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    style = "".join(f"@font-face{{font-family:'{name}';src:url('{name}.ttf')}}" for name in fonts)
    samples = "".join(f"<p style=\"font-family:'{name}';font-size:48px\">REPUBLIC 200 — AaBb 0123456789</p>" for name in fonts)
    art_samples = "".join(f"<figure><figcaption>{html.escape(name)} — traced SVG at 4×</figcaption><img src='{name}.svg' width='{images[name].width * 4}'></figure>" for name in art)
    (output / "preview.html").write_text(f"<!doctype html><meta charset='utf-8'><title>Original HUD vector review</title><style>body{{background:#202633;color:white;font:16px sans-serif}}figure{{display:inline-block;vertical-align:top}}{style}</style><h1>HUD vectors and outline fonts</h1>{samples}{art_samples}")
    return manifest


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--common", type=Path, default=Path("public/common"))
    parser.add_argument("--output", type=Path)
    parser.add_argument("--art", nargs="+", default=ART, help="Texture names in common.json to trace, in addition to all font pages")
    args = parser.parse_args()
    convert(args.common, args.output or args.common / "vector-hud", args.art)
