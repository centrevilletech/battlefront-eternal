#!/usr/bin/env python3
"""Prepare the single-player battle interface from a Battlefront II installation."""
import argparse
import hashlib
import json
import mmap
from pathlib import Path
import re
import struct

from ucfb import chunks as ucfb_chunks, iter_all_archives, node_name
from zero_hud2 import (ArchiveBudget, ConfigError, archive_nodes, archive_path, atomic_output,
                       checked_chunks, config_chunks, lvl_pc_dir, read_archive, to_jsonable, write_json)
from zero_load2 import _language_data, _locale_table, _picture
from zero_shell import write_picture
from zero_texture import SWBF2_DECODABLE, decode_pixels, decode_texture
import zero_hud_icon

HUD = Path("common/hud")
MAX_TEXTURES = 2048
MAX_TOTAL_PIXELS = 48 * 1024 * 1024
SAFE = re.compile(r"[a-z0-9][a-z0-9_.-]{0,126}\Z")


def _name(data, b, e):
    if not 1 < e - b <= 128 or data[e - 1] != 0 or b"\0" in data[b:e - 1]:
        raise ConfigError("invalid interface resource name")
    return data[b:e - 1].decode("latin1").lower()


def _resource(data, b, e, budget):
    fields = list(checked_chunks(data, b, e, budget))
    names = [_name(data, a, z) for tag, a, z in fields if tag == b"NAME"]
    if len(names) != 1:
        raise ConfigError("missing or ambiguous interface resource name")
    return names[0], fields


def _weapon_mesh_info(entries):
    """Collect {mesh_name: rotation_or_none} from 1playerhud weapon MeshInfo blocks."""
    result = {}

    def walk(nodes):
        for node in nodes or ():
            if (node.get("name") == "Model3D"
                    and node.get("values") in (["player1weapon1mesh"], ["player1weapon2mesh"])):
                for child in node.get("children") or ():
                    if child.get("name") != "MeshInfo":
                        continue
                    vals = child.get("values") or ()
                    if not vals or not isinstance(vals[0], str):
                        continue
                    name = vals[0].lower()
                    rot = None
                    for prop in child.get("children") or ():
                        if prop.get("name", "").lower() == "rotation":
                            rv = prop.get("values") or ()
                            if len(rv) == 3 and all(isinstance(v, (int, float)) for v in rv):
                                rot = (float(rv[0]), float(rv[1]), float(rv[2]))
                    result.setdefault(name, rot)
            walk(node.get("children"))

    walk(entries)
    return result


def _is_rotated_3d(rotation):
    return bool(rotation) and any(min(abs(a % 360.0), 360.0 - abs(a % 360.0)) > 15.0 for a in rotation)


def _mesh_textures(data, fields, budget):
    names = []
    for field, start, end in fields:
        if field != b"segm":
            continue
        for prop, a, z in checked_chunks(data, start, end, budget):
            if prop == b"TNAM" and z - a > 5 and struct.unpack_from("<I", data, a)[0] == 0:
                names.append(_name(data, a + 4, z))
    return names


def _hud_meshes(data, entries, budget, extra=()):
    """Diffuse art of the flat mesh used by a selected equipment or marker icon."""
    result = {}
    for tag, b, e in entries:
        if tag != b"modl":
            continue
        name, fields = _resource(data, b, e, budget)
        if not name.startswith("hud_") and name not in extra:
            continue
        for tex in _mesh_textures(data, fields, budget):
            result.setdefault(name, tex)
    return result


def write_battle_interface(lvl_pc, out, *, huds=None):
    """Publish a catalog after its archive-qualified elements, art and language tables."""
    art, meshes, languages, trees, extents = {}, {}, {}, {}, {}
    weapon_meshes = {}
    remaining = MAX_TOTAL_PIXELS
    for stem in ("core", "common", "ingame"):
        source = read_archive(archive_path(lvl_pc, stem + ".lvl"))
        base = Path(stem) / hashlib.sha256(source).hexdigest()
        if stem == "core":
            locales, _ = _language_data(source)
            for language, bounds in sorted(locales.items()):
                if not SAFE.fullmatch(language):
                    raise ConfigError("invalid interface language identifier")
                table = {f"{key:08x}": value for key, value in _locale_table(source, *bounds).items()}
                path = base / (language + ".json")
                payload = json.dumps(table, ensure_ascii=False, separators=(",", ":")).encode()
                if len(payload) > 8 << 20:
                    raise ConfigError("oversized interface language table")
                with atomic_output(out, HUD / path) as dest:
                    dest.write_bytes(payload)
                languages[language] = path.as_posix()
            continue
        budget = ArchiveBudget()
        entries = list(archive_nodes(source, budget))
        if stem == "ingame":
            for tree in (config_chunks(source, b"hud_") if huds is None else huds.values()):
                name = tree["file"]
                if not SAFE.fullmatch(name):
                    raise ConfigError("invalid HUD tree name")
                path = base / (name + ".json")
                jsonable = to_jsonable(tree["entries"])
                write_json(out, HUD / path, {"file": name, "entries": jsonable})
                trees[name] = path.as_posix()
                if name == "1playerhud":
                    weapon_meshes = _weapon_mesh_info(jsonable)
            meshes.update(_hud_meshes(source, entries, budget, extra=weapon_meshes))
        for tag, b, e in entries:
            if tag != b"tex_":
                continue
            name, _ = _resource(source, b, e, budget)
            if stem == "ingame" and not (name.startswith(("hud_", "flag_", "com_icon", "map_", "player_"))
                                          or name.endswith("_icon") or name in meshes.values()
                                          or name in ("statsscreens_bg", "tooltips_borderbox", "whitecircle")):
                continue
            if not SAFE.fullmatch(name) and not name.startswith("_localize_"):
                raise ConfigError("invalid HUD bitmap identifier")
            if not re.fullmatch(r"[a-z0-9_.-]+", name):
                raise ConfigError("invalid HUD bitmap path")
            pic = _picture(source, b, e, budget)
            remaining -= pic.width * pic.height
            if remaining < 0 or (name not in art and len(art) >= MAX_TEXTURES):
                raise ConfigError("HUD art exceeds the admission limit")
            path = base / (name + ".png")
            rgba = decode_pixels(pic.fmt, memoryview(source)[pic.start:pic.end], pic.width, pic.height)
            with atomic_output(out, HUD / path) as dest:
                write_picture(dest, pic.width, pic.height, rgba)
            art[name] = path.as_posix()
        if stem == "ingame":
            from ktx2 import decode_png
            cached = {}
            def diffuse(name):
                if name not in art:
                    return None
                if name not in cached:
                    # These bounded images were written by this preparation step.
                    cached[name] = decode_png((Path(out) / HUD / art[name]).read_bytes())
                return cached[name]
            for tag, b, e in entries:
                if tag != b"modl":
                    continue
                name, fields = _resource(source, b, e, budget)
                if name not in meshes or not SAFE.fullmatch(name):
                    continue
                rot = weapon_meshes.get(name)
                model_parts = zero_hud_icon.parts(source, fields, budget)
                oriented = not name.startswith("hud_") and _is_rotated_3d(rot)
                if oriented:
                    icon = zero_hud_icon.raster_oriented(model_parts, diffuse, rot)
                else:
                    icon = zero_hud_icon.raster(model_parts, diffuse)
                if icon:
                    icon_name = "mesh." + name
                    remaining -= icon[0] * icon[1]
                    if remaining < 0 or (icon_name not in art and len(art) >= MAX_TEXTURES):
                        raise ConfigError("oversized interface art")
                    path = base / (icon_name + ".png")
                    with atomic_output(out, HUD / path) as dest:
                        write_picture(dest, *icon)
                    art[icon_name] = path.as_posix()
                    meshes[name] = icon_name
                    extents[icon_name] = zero_hud_icon.extent(model_parts, rot if oriented else None)
    side_dir = lvl_pc / "side"
    pending = {name for name in weapon_meshes if name not in meshes and SAFE.fullmatch(name)}
    if pending and side_dir.is_dir():
        for side_stem in ("all", "cis", "imp", "rep"):
            if not pending:
                break
            side_file = side_dir / (side_stem + ".lvl")
            if not side_file.is_file() or side_file.stat().st_size < 8:
                continue
            digest = hashlib.sha256()
            with side_file.open("rb") as stream:
                for block in iter(lambda: stream.read(1 << 20), b""):
                    digest.update(block)
            base = Path("side") / side_stem / digest.hexdigest()
            with side_file.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
                sections = list(iter_all_archives(data))
                budget = ArchiveBudget()
                found = {}
                needed_tex = set()
                for s_start, s_end in sections:
                    for tag, b, e in ucfb_chunks(data, s_start, s_end):
                        if tag != b"modl":
                            continue
                        name = (node_name(data, b, e) or "").lower()
                        if name in pending and name not in found:
                            fields = list(checked_chunks(data, b, e, budget))
                            found[name] = fields
                            needed_tex.update(_mesh_textures(data, fields, budget))
                if not found:
                    continue
                side_tex = {}
                for s_start, s_end in sections:
                    for tag, b, e in ucfb_chunks(data, s_start, s_end):
                        if tag != b"tex_":
                            continue
                        name = (node_name(data, b, e) or "").lower()
                        if name not in needed_tex or name in side_tex:
                            continue
                        parsed = decode_texture(data, b, e, lazy=True, decodable=SWBF2_DECODABLE)
                        if parsed and parsed[1] is not None:
                            img = parsed[1]
                            side_tex[name] = (img.width, img.height, bytes(img.rgba))
                def side_diffuse(name):
                    return side_tex.get(name) or diffuse(name)
                for name in sorted(found):
                    rot = weapon_meshes.get(name)
                    model_parts = zero_hud_icon.parts(data, found[name], budget)
                    oriented = not name.startswith("hud_") and _is_rotated_3d(rot)
                    if oriented:
                        icon = zero_hud_icon.raster_oriented(model_parts, side_diffuse, rot)
                    else:
                        icon = zero_hud_icon.raster(model_parts, side_diffuse)
                    if not icon:
                        continue
                    icon_name = "mesh." + name
                    remaining -= icon[0] * icon[1]
                    if remaining < 0 or (icon_name not in art and len(art) >= MAX_TEXTURES):
                        raise ConfigError("oversized interface art")
                    path = base / (icon_name + ".png")
                    with atomic_output(out, HUD / path) as dest:
                        write_picture(dest, *icon)
                    art[icon_name] = path.as_posix()
                    meshes[name] = icon_name
                    extents[icon_name] = zero_hud_icon.extent(model_parts, rot if oriented else None)
                    pending.discard(name)
    if not {"1playerhud", "hudtransforms"}.issubset(trees):
        raise ConfigError("missing battle viewport or transforms")
    catalog = {"trees": trees, "textures": art, "meshes": meshes, "languages": languages,
               "bounds": {k: v for k, v in sorted(extents.items()) if v}}
    write_json(out, HUD / "presentation.json", catalog)
    return catalog


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", help="2005 installation, defaults to BFE_SWBF2_DIR")
    parser.add_argument("--out", required=True, type=Path, help="prepared 2005 family root")
    args = parser.parse_args()
    root = lvl_pc_dir(args.game)
    if root is None:
        parser.error("could not locate the installation")
    result = write_battle_interface(root, args.out)
    print(", ".join(f"{len(items)} {name}" for name, items in result.items()))


if __name__ == "__main__":
    main()
