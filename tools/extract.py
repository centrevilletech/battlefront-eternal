#!/usr/bin/env python3
"""Extract one map, a side's playable units, and the shared soldier data.

    python3 tools/extract.py nab2 [--side rep] [--unit rep_inf_jet_trooper ...]

tools/prepare.py runs the parts as their own steps instead: each side once
(--only side --side rep), what every side shares once (--only common), then
every map with its battles' sides (nab2 --side rep --side cis --skip-shared).

Reads the game as installed, copied into data/game/install (gitignored), with
the add-ons beside it (GameData/AddOn, as Jabba's Palace is; tools/zero_addon.py),
and writes public/ (also gitignored). Nothing from the retail game goes into git.

    public/maps/<map>/    world meshes, collision, terrain, spawn paths, regions
    public/sides/<side>/  unit classes, their bodies and first-person arms, weapons, textures
    public/common/        the soldier animations, effects, and HUD art every side shares
"""

import argparse
import array
import json
import math
import re
import shutil
import struct
import sys
from pathlib import Path

import zero_addon
from portable_math import cos, sin
from ucfb import child, chunks, cstr_at, iter_archives
from xform import IDENTITY, mat4, mul, transform_dir, transform_point
from zero_anim import FIRST_PERSON, FIRST_PERSON_BANK, write_anims
from zero_hash import crc, fnv1a
from zero_atmosphere import atmosphere_textures, parse_atmosphere
from zero_config import configs, section, value
from zero_font import collect_fonts
from zero_foliage import foliage_layers, terrain_foliage
from zero_fx import effect, effects, textures as effect_textures
from zero_locl import localization, localize
from zero_lua import calls as lua_calls, data_path, global_tables, load as load_lua, mission_scripts, mission_setup
from zero_model import ADDITIVE, HARD_EDGED, LIT, SPECULAR, STATIC_LIGHTING, TRANSPARENT, collect_models
from zero_model import BOUNDS as MODEL_BOUNDS
from zero_odf import FOLIAGE, entity_classes, property_name, soldier_class
from zero_sound import extract_audio
from zero_spawn_screen import RASTER_HUD, SPAWN_MODELS, SPAWN_TEXTURES, map_spawn_screen, spawn_strings, unit_name, weapon_name
from zero_scoreboard import SCOREBOARD_RASTER, SCOREBOARD_TEXTURES, scoreboard_strings
from zero_sky import mission_roots as sky_mission_roots, write_sky_flyers
from zero_shell import SHELL_COMMON_TEXTURES, SHELL_RASTER, shell_strings, shell_textures
from zero_terrain import parse_terrain
from zero_texture import collect_textures, safe_name, write_png
# vehicle_fx reads a building's damage effects and chunks as a hull's;
# importing it also names their keys before any class is read.
from zero_vehicle import vehicle_fx
from zero_water import parse_water, water_textures
from zero_weather import parse_weather, weather_textures
from zero_weapons import classes as weapon_classes
from zero_weapons import explosion_class, item_class, loadout, weapon_chain
from zero_trap import trap_settings
from zero_placed_items import placed_items
from zero_world import (
    coll_meshes,
    instance_records,
    parse_instances,
    place_mesh,
    place_shape,
    prim_shapes,
    regions,
    spawn_paths,
)

ROOT = Path(__file__).resolve().parents[1]
# The game as Steam installs it, copied whole: data/game/install holds
# GameData (the game program, and Data/_LVL_PC with the archives) and Manuals.
# BFE_GAME_DIR reads an installation in place instead (tools/zero_paths.py).
from zero_paths import GAME, shared_lock  # noqa: E402
LANGUAGE = "ENG"  # common.lvl comes per language
from zero_paths import PUBLIC  # noqa: E402


def common_archive():
    return GAME / "Common" / LANGUAGE / "common.lvl"


def map_archive(name):
    """Maps live in a folder named for their planet: bes2 is BES/bes2.lvl, an
    add-on's in its own folder (Jabba's Palace is AddOn/Tat3's TAT/tat3.lvl;
    tools/zero_addon.py)."""
    return zero_addon.map_archive(name, GAME)


def side_archive(side):
    """SIDE/<side>.lvl, the game's or an add-on's own army's."""
    return zero_addon.side_archive(side, GAME)


def first_person_archive(name):
    """The FPM archive a class's FirstPerson names ("all\\alltroop" is
    FPM/ALL/alltroop.lvl): the ODFs spell the folder in either case and the
    game's loader folds it, so each part matches any case on disk."""
    parts = [part for part in name.replace("\\", "/").split("/") if part]
    if not parts:
        return None
    parts[-1] = parts[-1] if parts[-1].lower().endswith(".lvl") else parts[-1] + ".lvl"
    path = GAME / "FPM"
    for part in parts:
        exact = path / part
        if not exact.exists():
            try:
                exact = next((entry for entry in path.iterdir() if entry.name.lower() == part.lower()), exact)
            except OSError:
                pass
        path = exact
    return path


def shared_archives():
    """Archives every map can draw models and textures from."""
    return [path.read_bytes() for path in (common_archive(), GAME / "core.lvl") if path.exists()]


# Material bits a mesh keeps (the rest only matter to the original loader).
KEPT_FLAGS = LIT | HARD_EDGED | TRANSPARENT | SPECULAR | STATIC_LIGHTING | ADDITIVE


def material_key(part, dynamic):
    material = part["material"]
    return (
        part["texture"],
        part.get("bump") or "",
        material["flags"] & KEPT_FLAGS,
        tuple(material.get("scroll") or ()),
        tuple(material.get("specular") or ()),
        part.get("colorKind") or "",
        dynamic,
    )


class Bucket:
    """World geometry sharing one material, merged into one mesh. Vertices are
    position, normal, uv, then a colour (rgba) when the material has one, and a
    tangent and binormal when it is bump mapped."""

    def __init__(self, key):
        self.key = key
        self.colored = bool(key[5])
        self.bumped = bool(key[1])
        self.verts = array.array("f")
        self.indices = []
        self.count = 0

    @property
    def stride(self):
        return 8 + (4 if self.colored else 0) + (6 if self.bumped else 0)

    def add(self, part, matrix):
        base = self.count
        colors = part.get("colors")
        tangents = part.get("tangents")
        binormals = part.get("binormals")
        for i, (p, n, uv) in enumerate(zip(part["positions"], part["normals"], part["uvs"])):
            self.verts.extend(transform_point(matrix, p))
            self.verts.extend(transform_dir(matrix, n))
            self.verts.extend(uv)
            if self.colored:
                self.verts.extend(colors[i] if colors else (1.0, 1.0, 1.0, 1.0))
            if self.bumped:
                self.verts.extend(transform_dir(matrix, tangents[i]))
                self.verts.extend(transform_dir(matrix, binormals[i]))
        self.count += len(part["positions"])
        self.indices.extend(base + i for i in part["indices"])


def save_mesh_buffers(buckets, extra=()):
    """Pack every bucket (and extra ready-made meshes) into one vertex buffer
    and one index buffer, with a record per mesh."""
    verts = array.array("f")
    indices = array.array("I")
    meshes = []
    for bucket in list(buckets.values()) + list(extra):
        if not bucket.indices:
            continue
        texture, bump, flags, scroll, specular, color_kind, dynamic = bucket.key
        attributes = ["position", "normal", "uv"] + (["color"] if bucket.colored else []) + (["tangent", "binormal"] if bucket.bumped else [])
        record = {
            "texture": texture,
            "flags": flags,
            "attributes": attributes,
            "stride": bucket.stride,
            "vertexOffset": len(verts) * 4,
            "vertexCount": bucket.count,
            "indexOffset": len(indices) * 4,
            "indexCount": len(bucket.indices),
        }
        if bump:
            record["bump"] = bump
        if scroll:
            record["scroll"] = list(scroll)
        if specular:
            record["specular"] = list(specular)
        if color_kind:
            record["colorKind"] = color_kind
        if dynamic:
            record["dynamic"] = True
        record.update(getattr(bucket, "extra", {}))
        verts.extend(bucket.verts)
        indices.extend(bucket.indices)
        meshes.append(record)
    return verts, indices, meshes


def write_textures(textures, names, out_dir):
    """Write the named textures that decoded; return name -> relative path.
    Texture names are matched without regard to case, as the game does."""
    out_dir.mkdir(parents=True, exist_ok=True)
    by_lower = {name.lower(): image for name, image in textures.items()}
    found = {}
    for name in sorted(names):
        image = textures.get(name) or by_lower.get(name.lower())
        if image is None:
            continue
        width, height, rgba = image
        file = f"{safe_name(name)}.png"
        write_png(out_dir / file, width, height, rgba)
        found[name] = f"textures/{file}"
    return found


def collision_sources(archives):
    """Every model's coll meshes and prim shapes; the first archive to define
    a model's collision wins."""
    meshes = {}
    shapes = {}
    for data in archives:
        for kind, found in coll_meshes(data).items():
            meshes.setdefault(kind.lower(), found)
        for kind, found in prim_shapes(data).items():
            shapes.setdefault(kind.lower(), found)
    return meshes, shapes


def new_polys():
    return {"pos": array.array("f"), "idx": array.array("I"), "poly": array.array("I"), "count": 0}


def soldier_collision_exclusions(archives):
    """Primitives explicitly reserved for another collision layer.

    VehicleCollisionOnly boxes deliberately span pedestrian doorways and
    arcade openings. They must not be added to the soldier triangle soup.
    Class and primitive identifiers in the retail archives are case-insensitive.
    """
    excluded = {}
    for data in archives:
        for name, cls in entity_classes(data).items():
            excluded.setdefault(name.lower(), {
                value.lower() for key, value in cls["props"]
                if key in ("VehicleCollisionOnly", "OrdnanceCollisionOnly", "BuildingCollisionOnly")
            })
    return excluded


def place_collision(kind, matrix, meshes, shapes, skeleton, only, polys, excluded=(), bone_shapes=False):
    """A model's collision at `matrix`: its coll meshes, and its prim shapes
    (only those named in `only`, when given)."""
    # ODF class names preserve case; coll/prim resource names need not. The
    # native lookup folds both (Yavin's Temple and Arch_stairs are examples).
    kind = kind.lower()
    only = {name.lower() for name in only} if only else None
    for positions, faces in meshes.get(kind, []):
        place_mesh(positions, faces, matrix, polys)
    for shape in shapes.get(kind, []):
        if shape.get("name", "").lower() in excluded:
            continue
        # Shapes hung off a bone ride an animated prop; the static pose
        # would be wrong. A live prop's own body is the exception: at bind
        # it is the closed door / the creature at rest, and the runtime
        # toggles it (bone_shapes, from write_props).
        if shape.get("parent", "").lower().startswith("bone_") and not bone_shapes:
            continue
        if only and shape.get("name", "").lower() not in only:
            continue
        local = shape["matrix"]
        parent = shape.get("parent")
        if skeleton and parent in skeleton["names"]:
            local = mul(skeleton["model"][skeleton["names"].index(parent)], local)
        place_shape(shape["kind"], shape["size"], mul(matrix, local), polys)


def collision_model(kind, classes, meshes, shapes):
    """The name a placed class's collision is filed under.

    coll meshes and prim shapes are named for the model, and a class loads
    the model its GeometryName names: Yavin's yav_prop_leaftree draws and
    collides as yav_prop_leaftree1, Kashyyyk's kas2_bldg_platform_1.msh as
    kas2_bldg_platform_1. Looked up by the class name, those trees and
    platforms (and every com_item_vehiclerecharge, whose model is spelled
    vehicalrecharge) had no collision at all. A class without a GeometryName,
    or one the archives do not define (a painted File mesh), collides under
    its own name, which is then the model's."""
    cls = classes.get(kind) if classes else None
    geometry = class_prop(cls, "GeometryName", "") if cls else ""
    for name in (geometry, kind):
        if name and (name.lower() in meshes or name.lower() in shapes):
            return name
    return geometry or kind


def write_collision(archives, instances, terrain, out, skeletons=None, soldier_shapes=None, live=(), anchors=(), everywhere=False,
                    classes=None):
    """Soldier collision. Each instance's coll mesh and prim shapes go out as
    polygons; the terrain under the city goes out on its own, because the
    engine tests soldiers against the terrain as a height field. The first
    archive to define a model's collision wins. Live objects (turrets, power-up
    droids) are left to props: theirs comes and goes as they are built and
    destroyed. Collision is the model's (collision_model): `classes` maps
    each placed class to its GeometryName; without it a class collides
    under its own name only.

    The terrain goes out where play happens: around every placed object and
    every path point (`anchors`: spawn, patrol and boundary paths), with a
    margin. A map without a boundary (Yavin's temple) lets soldiers roam the
    whole height field, so `everywhere` keeps all of it; a soldier past the
    exported terrain has no ground and falls.

    A shape's XFRM is relative to the node it hangs from (PRNT), so it goes
    through that node's model matrix. A class that lists SoldierCollision
    shapes collides with soldiers through those alone."""
    skeletons = {name.lower(): value for name, value in (skeletons or {}).items()}
    soldier_shapes = {name.lower(): value for name, value in (soldier_shapes or {}).items()}
    meshes, shapes = collision_sources(archives)
    excluded = soldier_collision_exclusions(archives)
    polys = new_polys()
    placed = []
    for kind, raw in instances:
        matrix = mat4(raw)
        placed.append(matrix)
        if kind in live:
            continue
        # The collision flags (SoldierCollision, VehicleCollisionOnly) are
        # the class's; the meshes, shapes and skeleton are the model's.
        model = collision_model(kind, classes, meshes, shapes)
        place_collision(model, matrix, meshes, shapes, skeletons.get(model.lower()), soldier_shapes.get(kind.lower()), polys,
                        excluded.get(kind.lower(), ()))
    (out / "collision.bin").write_bytes(polys["pos"].tobytes())
    (out / "collision_index.bin").write_bytes(polys["idx"].tobytes())
    (out / "collision_poly.bin").write_bytes(polys["poly"].tobytes())
    print(f"  collision polygons {polys['count']} tris {len(polys['idx']) // 3}", flush=True)

    ground = {"pos": array.array("f"), "idx": array.array("I"), "poly": array.array("I"), "count": 0}
    if terrain and (placed or everywhere):
        positions, tris = terrain["positions"], terrain["indices"]
        margin = 64.0
        xs = [m[12] for m in placed] + [p[0] for p in anchors]
        zs = [m[14] for m in placed] + [p[1] for p in anchors]
        if everywhere or not xs:
            lo_x = lo_z = -math.inf
            hi_x = hi_z = math.inf
        else:
            lo_x, hi_x = min(xs) - margin, max(xs) + margin
            lo_z, hi_z = min(zs) - margin, max(zs) + margin
        used = {}
        kept = []
        keep = []
        for i in range(0, len(tris), 3):
            a, b, c = positions[tris[i]], positions[tris[i + 1]], positions[tris[i + 2]]
            cx = (a[0] + b[0] + c[0]) / 3
            cz = (a[2] + b[2] + c[2]) / 3
            if not (lo_x <= cx <= hi_x and lo_z <= cz <= hi_z):
                continue
            tri = []
            for v in tris[i:i + 3]:
                if v not in used:
                    used[v] = len(kept)
                    kept.append(positions[v])
                tri.append(used[v])
            keep.append(tri)
        place_mesh(kept, keep, IDENTITY, ground)
    (out / "terrain_collision.bin").write_bytes(ground["pos"].tobytes())
    (out / "terrain_collision_index.bin").write_bytes(ground["idx"].tobytes())
    print(f"  terrain tris {len(ground['idx']) // 3}", flush=True)


def map_scripts(name):
    """The map's mission scripts, name -> compiled chunk: each era's battle
    (the map and an era letter: bes2r, bes2a) and the historical campaign's
    version of it (bes2a_h)."""
    scripts = zero_addon.missions(GAME)
    return {m: scripts[m] for m in sorted(scripts) if re.fullmatch(re.escape(name) + r"[a-z](_h)?", m)}


def mission_sides_of(name):
    """Every side the map's missions read (SIDE\\<name>.lvl), in script
    order: both eras' armies and the locals."""
    sides = []
    for chunk in map_scripts(name).values():
        try:
            found = mission_setup(chunk)["sides"]
        except Exception:  # noqa: BLE001 - one odd script must not stop the rest
            continue
        sides += [side for side in found if side not in sides]
    return sides


# A mission's era, by the letter after its map's name, as the shell's
# playlist names them (tools/zero_shell.py SIDE_LETTERS): r and c are the
# Clone Wars, a and i the Galactic Civil War.
MISSION_ERAS = {"r": "cw", "c": "cw", "a": "gcw", "i": "gcw"}
# The era each army fights in.
SIDE_ERAS = {"rep": "cw", "cis": "cw", "all": "gcw", "imp": "gcw"}
# The era a map plays when nothing names one, where it has that era.
DEFAULT_ERA = "cw"


def mission_world(chunk):
    """The world archive a mission script reads (ReadDataFile of
    <PLANET>\\<world>.lvl), by name in lower case; None without one."""
    world = None
    for call, args in lua_calls(load_lua(chunk)):
        if call != "ReadDataFile" or not args or not isinstance(args[0], str):
            continue
        folder, _, file = data_path(args[0]).replace("/", "\\").rpartition("\\")
        stem = file.rsplit(".", 1)[0].lower()
        if file.lower().endswith(".lvl") and folder.upper() == stem[:3].upper():
            world = stem
    return world


def same_world(world, name):
    """Whether a mission's world archive is the map's own: by name (the map's,
    or its archive's when its battles read a world named otherwise, as an
    add-on's "georuins" reads GEO\\geo1.lvl), or byte for byte (the Dune
    Sea's historical battle reads TAT/tat1h.lvl, a copy of tat1.lvl, and
    Yavin's Arena's YAV/yav2_h.lvl one of yav2.lvl)."""
    ours = map_archive(name)
    if world in (name, ours.stem.lower()):
        return True
    theirs = map_archive(world)
    return (ours.exists() and theirs.exists() and ours.stat().st_size == theirs.stat().st_size
            and ours.read_bytes() == theirs.read_bytes())


def map_missions(name):
    """The map's battles, mission name -> its setup (zero_lua.mission_setup)
    with its era ("cw" or "gcw"), whether it is the historical campaign's
    version (_h), and the world it is fought in when that is not the map's
    own: the campaign's Theed and Naboo Plains read NAB/nab2_h.lvl and
    nab1_h.lvl, worlds of their own that no map extracts yet."""
    found = {}
    for mission, chunk in map_scripts(name).items():
        try:
            setup = mission_setup(chunk)
        except Exception:  # noqa: BLE001 - one odd script must not stop the rest
            continue
        setup["era"] = MISSION_ERAS.get(mission[len(name)])
        setup["historical"] = mission.endswith("_h")
        world = mission_world(chunk)
        if world and not same_world(world, name):
            setup["world"] = world
        found[mission] = setup
    return found


def pick_mission(name, side):
    """The battle a map plays for a side, as the runtime picks it
    (src/mission.js): the side's era's (rep and cis fight the Clone Wars,
    all and imp the Galactic Civil War), or for a local side the battle that
    fields it, the default era's first. Never a historical version, nor one
    fought in another world. (name, setup), or (None, None) when the map has
    no such battle."""
    battles = {m: s for m, s in map_missions(name).items() if not s["historical"] and not s.get("world")}
    era = SIDE_ERAS.get(side)
    for mission in sorted(battles, key=lambda m: battles[m]["era"] != DEFAULT_ERA):
        setup = battles[mission]
        if (setup["era"] == era) if era else (side in setup["sides"]):
            return mission, setup
    return None, None


def class_props(cls, key):
    return [value for k, value in cls["props"] if k == key] if cls else []


def class_prop(cls, key, default=None):
    found = class_props(cls, key)
    return found[0] if found else default


def floats(text):
    return [float(v) for v in str(text).split()]


def command_post_paths(paths, posts):
    """Resolve each post's actual path reference, including native name casing.

    Maps do not share a naming suffix: Kamino uses CP2SpawnPath, while other
    worlds use CP2Spawn. Boundary and AI patrol paths are not spawn points.
    """
    lookup = {name.lower(): points for name, points in paths.items()}
    return {name: lookup[name.lower()]
            for name in sorted({post['spawnPath'] for post in posts if post.get('spawnPath')})
            if name.lower() in lookup}


def command_posts(records, classes, skeletons, world_regions, world_name, locl):
    """Command posts: placed objects whose class derives from "commandpost"."""
    by_region = {r["name"].lower(): r for r in world_regions}
    posts = []
    post_class = None
    for record in records:
        cls = classes.get(record["class"])
        if not cls or (cls["base"] or "").lower() != "commandpost":
            continue
        post_class = post_class or cls
        props = dict(record["props"])
        matrix = mat4(record["xfrm"])
        skeleton = skeletons.get(class_prop(cls, "GeometryName", record["class"]))
        mount = (0.0, 0.0, 0.0)
        if skeleton and "hp_hologram" in skeleton["names"]:
            mount = tuple(skeleton["model"][skeleton["names"].index("hp_hologram")][12:15])
        region = by_region.get(props.get("CaptureRegion", "").lower())
        label = props.get("Label", record["name"])
        posts.append({
            "name": record["name"],
            "label": label,
            # Mos Eisley's HANGAR string carries trailing blanks and newlines.
            "displayName": (localize(locl, f"level.{world_name}.{label}") or label).strip(),
            "team": int(float(props.get("Team", "0"))),
            "matrix": matrix,
            "hologram": list(transform_point(matrix, mount)),
            "capture": {k: region[k] for k in ("type", "matrix", "size")} if region else None,
            "spawnPath": props.get("SpawnPath"),
            # What holding the post counts for against reinforcement bleed.
            "valueBleed": float(props.get("ValueBleed", class_prop(cls, "ValueBleed", 100))),
            # How the maps draw it (Tatooine's Tusken campfires and Hoth's
            # probe droid drop have no MapTexture and a MapScale of 0: they
            # aren't drawn), and the hologram over it (none without HoloOdf).
            "mapTexture": class_prop(cls, "MapTexture"),
            "mapScale": float(class_prop(cls, "MapScale", 1)),
            "holoOdf": class_prop(cls, "HoloOdf"),
        })
        if destructible_post(cls):
            # An objective shot down rather than captured: its building in
            # props.json (write_props) has the same name. Its KillRegion
            # kills whoever stands in it as it goes (src/objectives.js).
            kill = by_region.get(props.get("KillRegion", "").lower())
            posts[-1]["maxHealth"] = float(class_prop(cls, "MaxHealth"))
            posts[-1]["killRegion"] = {k: kill[k] for k in ("name", "type", "matrix", "size")} if kill else None
    return posts, post_class


FOLIAGE_BASES = ("grasspatch", "leafpatch")
# Numbers among the foliage keys; the rest are strings or number lists.
FOLIAGE_TEXT = {"Texture"}


def foliage_class(cls):
    """A grasspatch or leafpatch class's settings. Seed wraps to 32 bits."""
    out = {"base": cls["base"].lower()}
    for key in FOLIAGE:
        values = class_props(cls, key)
        if not values:
            continue
        raw = values[-1]
        if key in FOLIAGE_TEXT:
            out[key] = raw
        elif key == "Seed":
            out[key] = int(float(raw)) % (1 << 32)
        else:
            numbers = floats(raw)
            out[key] = numbers if len(numbers) > 1 else numbers[0]
    return out


def attachments(cls):
    """A class's (attached class, hardpoint) pairs, in ODF order: each
    AttachOdf names the class every AttachToHardPoint after it attaches, until
    the next AttachOdf. A jungle tree hangs nine leaf patches and three
    godrays this way; reading only the first pair of each left one."""
    pairs = []
    attached = None
    for key, value in cls["props"]:
        if key == "AttachOdf":
            attached = value
        elif key == "AttachToHardPoint" and attached:
            pairs.append((attached, value))
    return pairs


def placed_foliage(records, classes, skeletons, painted=()):
    """Explicit patches, crowns attached to a tree's authored hardpoints, and
    the patches the terrain's painted foliage layers plant (terrain_foliage)."""
    definitions = {name: foliage_class(cls) for name, cls in classes.items()
                   if (cls["base"] or "").lower() in FOLIAGE_BASES}
    instances = [inst for inst in painted if inst["class"] in definitions]
    for record in records:
        name = record["class"]
        matrix = mat4(record["xfrm"])
        if name in definitions:
            instances.append({"class": name, "matrix": matrix})
        cls = classes.get(name)
        if not cls:
            continue
        model = class_prop(cls, "GeometryName", name)
        skeleton = skeletons.get(model)
        for attached, hardpoint in attachments(cls):
            if attached not in definitions or not skeleton or hardpoint not in skeleton["names"]:
                continue
            local = skeleton["model"][skeleton["names"].index(hardpoint)]
            instances.append({"class": attached, "matrix": mul(matrix, local)})
    used = {inst["class"] for inst in instances}
    return {"classes": {name: definition for name, definition in definitions.items() if name in used}, "instances": instances}


def placed_attachments(records, classes, skeletons, models):
    """Geometry hung on a placed prop's hardpoints (AttachOdf), as ordinary
    instances: Jabba on his dais bed, the rancor in its pit and the sarlacc's
    tentacles are animatedprops attached this way, never placed themselves,
    and the foliage expansion above is the only other AttachOdf reader, so
    they never became instances at all. Foliage stays with placed_foliage;
    lights and godrays are effects, not geometry. Returned in the instance
    list's (class, 12-float XFRM) shape, so baking and collision treat them
    like anything the world placed directly (a directly placed animatedprop
    already bakes the same way)."""
    folded = {}
    for key, parts in models.items():
        folded.setdefault(key.lower(), parts)
    canonical = {name.lower(): name for name in classes}
    found = []
    for record in records:
        cls = classes.get(record["class"])
        if not cls:
            continue
        model = class_prop(cls, "GeometryName", record["class"])
        skeleton = skeletons.get(model) or skeletons.get(str(model).lower())
        if not skeleton:
            continue
        matrix = mat4(record["xfrm"])
        for attached, hardpoint in attachments(cls):
            kind = canonical.get(attached.lower(), attached)
            attached_cls = classes.get(kind)
            if not attached_cls or hardpoint not in skeleton["names"]:
                continue
            if (attached_cls["base"] or "").lower() in FOLIAGE_BASES + ("light", "godray"):
                continue
            geometry = class_prop(attached_cls, "GeometryName", kind)
            if not (folded.get(kind.lower()) or folded.get(str(geometry).lower())):
                continue
            m = mul(matrix, skeleton["model"][skeleton["names"].index(hardpoint)])
            found.append((kind, upright_xfrm(m)))
    return found


def upright_xfrm(m):
    """An attached prop's XFRM from its hardpoint matrix, standing upright:
    the hardpoint gives where it sits and which way it faces, not a tilt.
    The rancor's hp_rancor is authored tipped 67 degrees (its Y axis
    mostly forward), which lays the creature on its back if taken whole;
    the original stands it in the pit. Yaw-only hardpoints (Jabba's bed,
    the sarlacc pit) pass through unchanged."""
    fx, fz = m[8], m[10]
    span = math.sqrt(fx * fx + fz * fz)
    if span < 1e-6:
        fx, fz = m[0], m[2]
        span = math.sqrt(fx * fx + fz * fz) or 1.0
        fx, fz = -fz / span, fx / span
    else:
        fx, fz = fx / span, fz / span
    # Basis vectors as XFRM rows: right = up x forward, up, forward.
    return [fz, 0.0, -fx, 0.0, 1.0, 0.0, fx, 0.0, fz] + list(m[12:15])


def animated_prop_animation(cls):
    """An animatedprop's or door's animation, from its merged class props,
    as the original game's animated prop class reads them: its
    clips (each Animation, at most 5; the first is the one it idles in),
    what sets off each action (AnimationTrigger "grab hp_hotspot 5.0", at
    most 3; "open hp_trapradius 1.0 trap_open" names a sound too; a door's
    names only the node and reach, its action is open), a creature's grabs
    (AttachTrigger "hp_active 1.5 grab human_rifle_thrown_flail 2.5
    hp_camera die": the node that seizes, its reach, the action it seizes
    in, the victim's clip, how many seconds into the action it may still
    seize, then a camera node and a word the original game reads and leaves
    unused, and an optional sound; at most 3), the pause between idle
    loops (IdleDelay), the swallow's sound (KillSoldierSound), the flag
    0550a3b2 that turns a seized player's view to his death camera at
    once, and a door's sounds. None where the class names no animation.
    `trigger` is the first of `triggers`, as earlier imports wrote it."""
    clips = [v.strip().lower() for v in class_props(cls, "Animation") if isinstance(v, str) and v.strip()][:5]
    if not clips:
        return None
    out = {"idle": clips[0], "clips": clips, "trigger": None, "triggers": []}
    for key, name in (("OpenSound", "openSound"), ("CloseSound", "closeSound"), ("KillSoldierSound", "killSound")):
        sound = class_prop(cls, key)
        if isinstance(sound, str) and sound.strip():
            out[name] = sound.strip()
    for raw in class_props(cls, "AnimationTrigger"):
        tokens = raw.split() if isinstance(raw, str) else []
        if (cls["base"] or "").lower() == "door":
            if len(tokens) >= 2 and not out["triggers"]:
                out["triggers"].append({"action": "open", "node": tokens[0], "radius": float(tokens[1])})
        elif len(tokens) >= 3 and len(out["triggers"]) < 3:
            trigger = {"action": tokens[0].lower(), "node": tokens[1], "radius": float(tokens[2])}
            if len(tokens) >= 4:
                trigger["sound"] = tokens[3]
            out["triggers"].append(trigger)
    out["trigger"] = out["triggers"][0] if out["triggers"] else None
    attach = []
    for raw in class_props(cls, "AttachTrigger"):
        tokens = raw.split() if isinstance(raw, str) else []
        if len(tokens) >= 5 and len(attach) < 3:
            grab = {"node": tokens[0], "radius": float(tokens[1]), "action": tokens[2].lower(), "clip": tokens[3].lower(), "window": float(tokens[4])}
            if len(tokens) >= 8:
                grab["sound"] = tokens[7]
            attach.append(grab)
    if attach:
        out["attach"] = attach
    delay = class_prop(cls, "IdleDelay")
    if delay is not None:
        out["idleDelay"] = float(delay)
    if int(float(class_prop(cls, "0550a3b2", "0") or 0)):
        out["deathCamera"] = True
    # The flag 6b014234 starts the idle at its first frame (the original game
    # starts every other prop's at a random one) and stops its update in
    # some network states, which the port doesn't model.
    if int(float(class_prop(cls, "6b014234", "0") or 0)):
        out["fromStart"] = True
    return out


def backdrop_canopies(instances, models, foliage_classes):
    """Native leaf art and the original jungle-card model footprints for the
    optional reconstructed near canopy renderer. Far models stay untouched."""
    texture = next((cls.get("Texture") for cls in foliage_classes.values()
                    if cls.get("base") == "leafpatch" and not cls.get("Vine")), None)
    if not texture:
        return None
    trees = []
    for kind, raw in instances:
        parts = models.get(kind, [])
        if not any("treebillboard" in part.get("texture", "").lower() for part in parts):
            continue
        points = [p for part in parts for p in part["positions"]]
        bounds = [[min(p[d] for p in points) for d in range(3)], [max(p[d] for p in points) for d in range(3)]]
        trees.append({"matrix": mat4(raw), "bounds": bounds})
    return {"texture": texture, "instances": trees} if trees else None


def tree_lines(data, classes, paths):
    """Props a map's prp_ TreeLine blocks set along paths: every Distance
    along each Path's polyline (not restarting at its nodes), a BorderOdf
    class's model at the middle of each step, turned across the path. Its
    strings are FNV hashes."""
    from zero_hash import fnv1a as hash_name
    as_hash = lambda v: struct.unpack("<I", struct.pack("<f", v))[0] if isinstance(v, float) else hash_name(v)
    path_by_hash = {hash_name(n): pts for n, pts in paths.items()}
    class_by_hash = {hash_name(n): n for n in classes}
    placed = []
    for config in configs(data, b"prp_"):
        for name, values, children in config:
            if name != "%08x" % hash_name("TreeLine"):
                continue
            for pname, pvalues, pchildren in children or []:
                if pname != "%08x" % hash_name("Path") or not pvalues:
                    continue
                points = path_by_hash.get(as_hash(pvalues[0]))
                step = next((v[0] for n, v, _ in pchildren if n == "Distance" and v), 18.0)
                borders = [class_by_hash.get(as_hash(v[0])) for n, v, _ in pchildren if n == "%08x" % hash_name("BorderOdf") and v]
                borders = [b for b in borders if b]
                flip = any(n == "%08x" % hash_name("Flip") for n, _, _ in pchildren)
                if not points or not borders:
                    continue
                cursor = list(points[0][:3])
                i = 1
                k = 0
                while i < len(points):
                    target = points[i][:3]
                    d = [target[j] - cursor[j] for j in range(3)]
                    length = math.sqrt(sum(x * x for x in d))
                    if length <= step:
                        i += 1
                        continue
                    u = [x / length for x in d]
                    at = [cursor[j] + u[j] * step / 2 for j in range(3)]
                    # The original game takes cross(direction, up) as the
                    # FORWARD axis and derives right=up×forward.
                    # Treating it as right mirrored the model and turned the
                    # jungle backdrop across paths, including Temple's bridge.
                    norm = math.hypot(u[0], u[2]) or 1.0
                    sign = -1 if flip else 1
                    right = [sign * u[0] / norm, 0.0, sign * u[2] / norm]
                    forward = [-right[2], 0.0, right[0]]
                    placed.append((borders[k % len(borders)], right + [0.0, 1.0, 0.0] + forward + at))
                    k += 1
                    cursor = [cursor[j] + u[j] * step for j in range(3)]
    return placed


def hologram_look(holo):
    """A hologram class's look: size, reach, intensities, motion."""
    return {
        "visibleDistance": float(class_prop(holo, "HoloVisibleDistance", 200)),
        "height": float(class_prop(holo, "HoloHeight", 3.5)),
        "size": float(class_prop(holo, "HoloSize", 1)),
        "beamIntensity": float(class_prop(holo, "HoloBeamIntensity", 1)),
        "lightIntensity": float(class_prop(holo, "HoloLightIntensity", 1)),
        "imageIntensity": float(class_prop(holo, "HoloImageIntensity", 1)),
        "flareIntensity": float(class_prop(holo, "HoloFlareIntensity", 1)),
        "lightRadius": float(class_prop(holo, "HoloLightRadius", 8)),
        "rotateRate": float(class_prop(holo, "HoloRotateRate", 0)),
        "flickerRate": float(class_prop(holo, "HoloFlickerRate", 0)),
        "flickerAlpha": [float(class_prop(holo, "HoloFlickerAlphaMin", 1)), float(class_prop(holo, "HoloFlickerAlphaMax", 1))],
        "fadeInTime": float(class_prop(holo, "HoloFadeInTime", 2)),
    }


def minimap_frame(posts):
    """The minimap covers the command posts' bounding box plus 100 m, as a
    square: u = 0.5 + (x - cx) / (2 half), v = 0.5 + (z - cz) / (2 half)."""
    if not posts:
        return None
    xs = [p["matrix"][12] for p in posts]
    zs = [p["matrix"][14] for p in posts]
    center = [(min(xs) + max(xs)) / 2, (min(zs) + max(zs)) / 2]
    half = max(max(xs) - min(xs), max(zs) - min(zs)) / 2 + 100
    return {"center": center, "half": half}


def lighting(sky_info, sun_info):
    """The map's lights, as the 2004 renderer sets them up from the sky config
    (colours 0..255, gamma space). Props take the flat ambient; soldiers and
    "Lighting dynamic" classes take a sky-to-ground ambient by normal. The sun
    direction is where its light travels."""
    ambient = value(sky_info, "AmbientColor", [128, 128, 128])
    top = value(sky_info, "TopDirectionalAmbientColor") or value(sky_info, "CharacterAmbientColor") or ambient
    bottom = value(sky_info, "BottomDirectionalAmbientColor") or value(sky_info, "CharacterAmbientColor") or ambient
    angle = value(sun_info, "Angle")
    if angle and len(angle) >= 2:
        a0, a1 = math.radians(angle[0]), math.radians(angle[1])
        direction = [-sin(a0) * cos(a1), cos(a0), -sin(a0) * sin(a1)]
    else:
        direction = [0.0, -1.0, 0.0]
    character = value(sky_info, "CharacterAmbientColor") or ambient
    return {
        "ambient": ambient[:3],
        "character": character[:3],
        "top": top[:3],
        "bottom": bottom[:3],
        "sunColor": (value(sun_info, "Color") or [128, 128, 128])[:3],
        "sunDirection": direction,
    }


def low_res_terrain(entries, texture_paths):
    """The far scene's terrain and its fog. Fog runs linearly from FogNear,
    and its colour's alpha caps it: the far end is stretched by 255 / alpha,
    so at alpha 100 the fog never passes 100/255 inside FogFar. An alpha of 0
    or 255 leaves it. The range goes out as authored: the game
    fogs only when 0 <= FogNear < FogFar, which the renderer decides
    (src/materials.js fogs; Kashyyyk: Islands sets 9000 and 8000)."""
    if not entries:
        return None
    color = value(entries, "FogColor")
    near = (value(entries, "FogNear") or [0])[0]
    far = (value(entries, "FogFar") or [0])[0]
    alpha = color[3] if color and len(color) > 3 and color[3] > 0 else 255
    texture = (value(entries, "Texture") or [None])[0]
    return {
        "texture": texture_paths.get(texture) and texture,
        "scale": (value(entries, "TextureScale") or [1])[0],
        "maxDistance": (value(entries, "MaxDistance") or [0])[0],
        "fog": color[:3] if color else None,
        "fogRange": [near, near + (far - near) * 255 / alpha],
    }


def dome_models(dome_info):
    """The sky's DomeModels in drawing order: (model, follow), follow being
    [MovementScale s, Offset] when either is set (the model then stands at
    height s * eye + (1 - s) * Offset instead of the eye's), else None."""
    found = []
    for name, _, settings in dome_info:
        if name != "DomeModel":
            continue
        follow = None
        if value(settings, "Offset") is not None or value(settings, "MovementScale") is not None:
            follow = [(value(settings, "MovementScale") or [1.0])[0], (value(settings, "Offset") or [0.0])[0]]
        found.append(((value(settings, "Geometry") or [None])[0], follow))
    return found


def post_effects(fx_configs):
    """Full-screen effects from the map's fx config: the soft-focus blur."""
    for entries in fx_configs:
        for name, values, children in entries:
            if values and values[0] == "Blur":
                enabled = (value(children, "Enable") or [0])[0]
                return {
                    "blur": {
                        "enabled": bool(enabled),
                        "mode": (value(children, "Mode") or [1])[0],
                        "blend": (value(children, "ConstantBlend") or [0])[0],
                        "downsize": (value(children, "DownSizeFactor") or [0.25])[0],
                    }
                }
    return {}


def light_regions(world_regions):
    """Regions named like "shadow02 a=1.0 d=0.0" change the light for soldiers
    inside them: d scales the sun, a the ambient (prefixes of directional,
    ambient)."""
    found = []
    for region in world_regions:
        tokens = region["name"].split()
        settings = {}
        for token in tokens[1:]:
            if "=" not in token:
                continue
            key, raw = token.split("=", 1)
            for full in ("directional", "ambient", "colortop", "colorbottom", "color"):
                if full.startswith(key.lower()) or key.lower() == full:
                    try:
                        settings[full] = float(raw)
                    except ValueError:
                        settings[full] = raw
                    break
        if settings:
            found.append({"name": tokens[0], "type": region["type"], "matrix": region["matrix"], "size": region["size"], **settings})
    return found


# Placed objects the runtime brings to life rather than baking into the
# world: power-up droids (powerupstation), turrets (armedbuilding), animated
# props and doors, and traps (Endor's Ewok log traps, tools/zero_trap.py).
LIVE_BASES = ("powerupstation", "armedbuilding", "animatedprop", "door", "trap")


def destructible_post(cls):
    """A command post with a MaxHealth: an objective that is shot down, not
    captured (Hoth's shield generator, Geonosis' Techno Union ships and the
    Republic forward command center, Endor's bunker control panel). It
    lives as a building too (write_props), so it is never baked."""
    return bool(cls) and (cls["base"] or "").lower() == "commandpost" and class_prop(cls, "MaxHealth") is not None


def live_class(cls):
    """Whether a class comes to life at runtime rather than baking."""
    return bool(cls) and ((cls["base"] or "").lower() in LIVE_BASES or destructible_post(cls))
# Class settings written out for them.
LIVE_SETTINGS = (
    "Label", "GeometryName", "MaxHealth", "PowerupDelay", "SoldierHealth", "SoldierAmmo", "VehicleHealth", "VehicleAmmo", "IdleRotateSpeed",
    "IdleWaitTime", "IdleWobbleNode", "IdleWobbleFactor", "ActiveRotateNode", "ActiveSpinNode",
    "TurretNodeName", "YawLimits", "PitchLimits", "PitchTurnFactor", "YawTurnFactor", "MaxTurnSpeed",
    "MaxPitchSpeed", "TurnFilter", "PitchFilter", "PilotPosition", "Pilot9Pose", "WeaponName", "WeaponAmmo",
    "AimerNodeName", "AimerPitchLimits", "AimerYawLimits", "AimerPitchRate", "AimerYawRate", "FirePointName",
    "EyePointOffset", "TrackCenter", "TrackOffset", "TiltValue", "UnbuiltGeometryName", "UnbuiltHoloOdf",
    # Being destroyed and built again.
    "DestroyedGeometryName", "ExplosionName", "Explosion", "RespawnTime", "HealthTexture", "AddHealth",
)
# How long a destroyed building takes to come back by itself (armed
# buildings rebuild; others stay down).
RESPAWN_TIME = {"armedbuilding": 120.0}


def invert_rigid(m):
    """The inverse of a rotation-and-translation 4x4 (column-major)."""
    r = [m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]]
    t = m[12:15]
    rt = [r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]]
    inv_t = [-(rt[0] * t[0] + rt[3] * t[1] + rt[6] * t[2]),
             -(rt[1] * t[0] + rt[4] * t[1] + rt[7] * t[2]),
             -(rt[2] * t[0] + rt[5] * t[1] + rt[8] * t[2])]
    return [rt[0], rt[1], rt[2], 0.0, rt[3], rt[4], rt[5], 0.0, rt[6], rt[7], rt[8], 0.0, inv_t[0], inv_t[1], inv_t[2], 1.0]


def rig_parts(parts, skeleton):
    """A model's parts for posing at runtime, each in the space of the bone it
    hangs from. A hard-skinned part is split into one part per bone, each
    triangle going with its first vertex's bone. A soft-skinned part (the
    sarlacc tentacle, Jabba, the rancor) stays whole, in the model's space,
    each vertex with its three bones and weights: split by bone, its
    pieces came apart wherever the creature bent (metres apart at the
    tentacle's joints)."""
    out = []
    for part in parts:
        if part.get("skinJoints") and skeleton:
            out.append({**part, "bone": None, "local_positions": part["positions"], "local_normals": part["normals"]})
            continue
        joints = part.get("joints")
        if not joints and part.get("skin") and part.get("bmap") and skeleton:
            bmap = part["bmap"]
            joints = [bmap[k] if k < len(bmap) and bmap[k] < len(skeleton["names"]) else 0 for k in part["skin"]]
        if not joints or not skeleton:
            out.append(part)
            continue
        names = skeleton["names"]
        groups = {}
        for i in range(0, len(part["indices"]), 3):
            tri = part["indices"][i:i + 3]
            groups.setdefault(joints[tri[0]], []).extend(tri)
        for joint, tris in groups.items():
            to_local = invert_rigid(skeleton["model"][joint])
            remap = {}
            positions, normals, uvs, indices = [], [], [], []
            for v in tris:
                if v not in remap:
                    remap[v] = len(positions)
                    positions.append(transform_point(to_local, part["positions"][v]))
                    normals.append(transform_dir(to_local, part["normals"][v]))
                    uvs.append(part["uvs"][v])
                indices.append(remap[v])
            out.append({**part, "bone": names[joint], "local_positions": positions, "local_normals": normals,
                        "positions": positions, "normals": normals, "uvs": uvs, "indices": indices})
    return out


def objective_post(entry, cls, record, add_anims, animations):
    """A destructible command post's building (docs/objectives.md): the post
    it is (by name), and an animated one's clips and death, as the original
    game's EntityBuildingAnimated reads them: the IdleAnimation it loops and
    the DeathAnimation it dies in, from the bank AnimationName names; while
    that plays, with DeathAnimationExplosions set, an explosion,
    med_explosion or small_explosion effect every 0.4 s at a node picked at
    random; as it ends, each DeathEffect ("<effect> <hardpoint>", up to 5) at
    its hardpoint, then the wreck.
    Keys the post's readers never take (ExplosionOffset, ExplosionDestruct,
    the Techno Union's hulk key 0403d917) are left out."""
    entry["post"] = record["name"]
    bank = class_prop(cls, "AnimationName")
    idle = class_prop(cls, "IdleAnimation")
    death = class_prop(cls, "DeathAnimation")
    if not isinstance(bank, str) or not (idle or death):
        return
    clips = [c.lower() for c in (idle, death) if isinstance(c, str) and c]
    add_anims(entry["model"], {"idle": clips[0], "clips": clips})
    meta = animations.get(entry["model"])
    if not meta:
        return
    effects_at = []
    for raw in class_props(cls, "DeathEffect")[:5]:
        tokens = raw.split() if isinstance(raw, str) else []
        if len(tokens) >= 2:
            effects_at.append({"effect": tokens[0], "node": tokens[1].lower()})
    entry["animation"] = {"idle": idle.lower() if isinstance(idle, str) else None,
                          "death": death.lower() if isinstance(death, str) else None}
    clip = meta["clips"].get(entry["animation"]["death"] or "")
    if clip:
        random_fx = str(class_prop(cls, "DeathAnimationExplosions", "0")).strip() not in ("", "0")
        fx = dict(entry.get("fx") or {})
        fx["dying"] = {"effects": ["explosion", "med_explosion", "small_explosion"] if random_fx else [],
                       "interval": 0.4, "span": round(clip["frames"] / 30, 4), "final": effects_at}
        entry["fx"] = fx


def write_props(records, classes, live, models, skeletons, archives, out, locl=None, soldier_shapes=None):
    """props.json and props.bin: each live object's rig (bones and parts) and
    each placed instance with its class settings (and a turret's weapons).
    A building's other looks go along: the build pad it starts as (with the
    hologram over it), and the wreck it leaves. Each instance's collision, as
    built and as a wreck, goes to props_collision.bin (positions),
    props_collision_index.bin and props_collision_poly.bin (each triangle's
    polygon), as [first triangle, count] ranges. A class's damage effects
    and chunks (fx: the building reader takes them as a hull's) go with each
    of its instances, the effects they play in props.json's effects, and the
    pieces' models (and the effects' own meshes) in the model's space in
    props_extras.bin and props_extras_index.bin (props.json's extras).
    Returns the textures they use, including turret HUD art and ordnance
    sprites."""
    locl = locl or {}
    soldier_shapes = soldier_shapes or {}
    hud = set()
    rigs = {}
    pack = {}
    instances = []
    building_fx = {}  # class -> its damage effects and chunks (vehicle_fx)
    meshes, shapes = collision_sources(archives)
    excluded = soldier_collision_exclusions(archives)
    polys = new_polys()
    # Animated props' and doors' clips, per model, in one shared buffer
    # (props_anims.bin), as the vehicles' are (tools/zero_vehicle.py).
    animations = {}
    anim_floats = array.array("f")

    def add_anims(model, anim):
        if model in animations:
            return
        animations[model] = None
        skeleton = skeletons.get(model)
        if not skeleton:
            return
        from ucfb import node_name, nodes
        actions = {anim["idle"], "idle", "grab", "grab1", "grab2", "open", "close", "activate"}
        actions |= set(anim.get("clips") or [])
        actions |= {t["action"] for t in anim.get("triggers") or ([anim["trigger"]] if anim.get("trigger") else [])}
        names = {}
        for action in actions:
            names[crc(action.capitalize())] = action
            names[crc(action)] = action
        source = next((a for a in archives if any(node_name(a, b, e).lower() == model.lower() for b, e in nodes(a, b"zaa_"))), None)
        if not source:
            return
        banks = [node_name(source, b, e) for b, e in nodes(source, b"zaa_") if node_name(source, b, e).lower() == model.lower()]
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp)
            write_anims(source, skeleton["names"], path, banks=banks, names=names, file="prop")
            meta = json.loads((path / "prop.json").read_text())
            floats = array.array("f")
            floats.frombytes((path / "prop.bin").read_bytes())
        if not meta["clips"]:
            return
        # A bank may record one bone twice (the rancor's root: a static
        # record, then the animated one). Posing would blend both; the
        # last record is the one that moves.
        rebuilt = array.array("f")
        for clip in meta["clips"].values():
            frames, origin = clip["frames"], clip["offset"]
            last = {}
            for k, bone in enumerate(clip["tracks"]):
                last[bone] = k
            keep = sorted(last.values())
            clip["offset"] = len(anim_floats) + len(rebuilt)
            clip["tracks"] = [clip["tracks"][k] for k in keep]
            for k in keep:
                start = origin + k * frames * 7
                rebuilt.extend(floats[start:start + frames * 7])
        anim_floats.extend(rebuilt)
        animations[model] = meta

    def add_rig(model):
        if model in pack or not models.get(model):
            return model in pack
        pack[model] = rig_parts(models[model], skeletons.get(model))
        skeleton = skeletons.get(model)
        names = skeleton["names"] if skeleton else []
        rigs[model] = {
            "bones": [{"name": n, "parent": names.index(p) if p in names and p != n else -1, "local": skeleton["local"][i]}
                      for i, (n, p) in enumerate(zip(names, skeleton["parents"] if skeleton else []))],
            **MODEL_BOUNDS.get(model, {}),
        }
        return True

    def collide(model, matrix, only=None, kind=None, bone_shapes=False):
        """The model's collision at `matrix`, under the class `kind`'s
        collision flags (the model's own when no class is given: a wreck,
        a build pad)."""
        start = len(polys["idx"]) // 3
        place_collision(model, matrix, meshes, shapes, skeletons.get(model), only, polys, excluded.get((kind or model).lower(), ()), bone_shapes)
        count = len(polys["idx"]) // 3 - start
        return [start, count] if count else None

    for record in records:
        cls = classes.get(record["class"])
        if record["class"] not in live or not cls:
            continue
        model = class_prop(cls, "GeometryName", record["class"])
        add_rig(model)
        props = dict(record["props"])
        settings = {}
        for key in LIVE_SETTINGS:
            values = class_props(cls, key)
            if values:
                settings[key] = values if len(values) > 1 else values[0]
        entry = {
            "class": record["class"],
            "base": cls["base"].lower(),
            "model": model,
            "name": record["name"],
            # com_weap_gunturret is entity.com.weap_gunturret.
            "label": localize(locl, "entity." + record["class"].replace("_", ".", 1)) or settings.get("Label") or record["class"],
            "matrix": mat4(record["xfrm"]),
            "team": int(float(props.get("Team", "0"))),
            "radius": float(props.get("Radius", "0")),
            "settings": settings,
        }
        base = cls["base"].lower()
        matrix = entry["matrix"]
        # A turret collides as its model (Yavin's tower turret is the
        # watchtower model), not as its class. An animated prop's shapes
        # all hang off its bones: at bind they are its closed/resting
        # body, and every one of them counts (the trapdoor's
        # SoldierCollision names its doors p_door_1, the shapes p_door1).
        animated = base in ("animatedprop", "door")
        entry["collision"] = collide(collision_model(record["class"], classes, meshes, shapes), matrix,
                                     None if animated else soldier_shapes.get(record["class"]), record["class"],
                                     bone_shapes=animated)
        if settings.get("RespawnTime") is None and base in RESPAWN_TIME:
            settings["RespawnTime"] = RESPAWN_TIME[base]
        blast = settings.get("ExplosionName") or settings.get("Explosion")
        if isinstance(blast, str):
            entry["explosion"] = explosion_class(archives, blast)
        if settings.get("HealthTexture"):
            hud.add(settings["HealthTexture"])
        wreck = settings.get("DestroyedGeometryName")
        if isinstance(wreck, str) and add_rig(wreck):
            entry["destroyed"] = {"model": wreck, "collision": collide(wreck, matrix)}
        pad = settings.get("UnbuiltGeometryName")
        if isinstance(pad, str) and add_rig(pad):
            # The hologram stands on the pad's hp_hologram.
            skeleton = skeletons.get(pad)
            mount = (0.0, 0.0, 0.0)
            if skeleton and "hp_hologram" in skeleton["names"]:
                mount = tuple(skeleton["model"][skeleton["names"].index("hp_hologram")][12:15])
            entry["unbuilt"] = {"model": pad, "collision": collide(pad, matrix), "hologram": list(transform_point(matrix, mount))}
            holo = classes.get(settings.get("UnbuiltHoloOdf") or "")
            if holo:
                image = class_prop(holo, "HoloImageGeometry", "")
                entry["unbuilt"]["holo"] = {**hologram_look(holo), "color": floats(class_prop(holo, "Color", "255 255 255")),
                                            "type": (class_prop(holo, "HoloType", "") or "").lower(),
                                            "image": image if add_rig(image) else None}
        weapon = settings.get("WeaponName")
        if isinstance(weapon, str):
            chain = weapon_chain(archives, weapon)
            if chain:
                entry["weapon"] = {"slot": 1, "weapon": weapon, "ammo": int(float(settings.get("WeaponAmmo", 0))),
                                   "channel": 0, **chain,
                                   "label": localize(locl, "weapons." + weapon.replace("_", ".", 2)) or weapon}
                hud |= weapon_texture_dependencies(chain)
        # Its damage effects and the pieces it flies apart into, as a hull's
        # (the building reader falls through to the same one). Yavin's
        # tower turret writes its chunk's keys as "\\ChunkGeometryName" and
        # so on, names nothing reads: its one chunk has no model.
        if record["class"] not in building_fx:
            fx = vehicle_fx(cls["props"])
            building_fx[record["class"]] = {key: fx[key] for key in ("damage", "chunks") if fx.get(key)}
        if building_fx[record["class"]]:
            entry["fx"] = building_fx[record["class"]]
        if base in ("animatedprop", "door"):
            anim = animated_prop_animation(cls)
            if anim:
                entry["animation"] = anim
                add_anims(model, anim)
        if destructible_post(cls):
            objective_post(entry, cls, record, add_anims, animations)
        if base == "trap":
            # Its swing and build clips, and its build pad (zero_trap.py).
            trap = trap_settings(cls, class_prop, classes, skeletons)
            if trap:
                add_anims(model, {"idle": trap["trigger"], "clips": [c for c in (trap["trigger"], trap["build"]) if c]})
                if trap.get("pad") and not add_rig(trap["pad"]["model"]):
                    trap.pop("pad")
                entry["trap"] = trap
        instances.append(entry)
    records_out = write_meshes(pack, sorted(pack), out, "props", local=True)
    for model, parts in records_out.items():
        # A soft skin's bones go with its part: three joints and weights a
        # vertex (its positions are in the model's space).
        for record, part in zip(parts, pack[model]):
            if part.get("skinJoints"):
                record["joints"] = part["joints"]
                record["skinJoints"] = part["skinJoints"]
                record["skinWeights"] = [round(w, 4) for w in part["skinWeights"]]
        rigs[model]["parts"] = parts
    # Power-ups the world places (zero_placed_items.py): items from the
    # battle's start, their models with the pieces'.
    items = [{**{k: v for k, v in placed.items() if k != "xfrm"}, "matrix": mat4(placed["xfrm"])}
             for placed in placed_items(records, classes, archives)]
    # What those effects and pieces need: the effects, and the pieces' and
    # the effects' own models, by the lower-case name the chunks give.
    wanted, flying = set(), set()
    flying |= {str(i["item"]["GeometryName"]).lower() for i in items if isinstance(i["item"].get("GeometryName"), str)}
    for entry in instances:
        dying = (entry.get("fx") or {}).get("dying") or {}
        wanted |= set(dying.get("effects") or []) | {f["effect"] for f in dying.get("final") or []}
        if isinstance((entry.get("trap") or {}).get("deathEffect"), str):
            wanted.add(entry["trap"]["deathEffect"])
    for fx in building_fx.values():
        wanted |= {d["effect"] for d in fx.get("damage", [])}
        for chunk in fx.get("chunks", []):
            wanted |= {chunk[k] for k in ("ChunkTrailEffect", "ChunkTerrainEffect", "ChunkSmokeEffect") if isinstance(chunk.get(k), str)}
            if isinstance(chunk.get("ChunkGeometryName"), str):
                flying.add(chunk["ChunkGeometryName"])
    emitters = {}
    if wanted:
        sources = [effects(archive) for archive in archives]
        emitters = {name: effect(sources, name) for name in sorted(wanted)}
        emitters = {name: found for name, found in emitters.items() if found}
    fx_textures = set()
    for found in emitters.values():
        used, meshes_used = effect_textures(found)
        fx_textures |= used
        flying |= {m.lower() for m in meshes_used}
    folded = {name.lower(): parts for name, parts in models.items()}
    extras = write_meshes(folded, sorted(n for n in flying if folded.get(n)), out, "props_extras")
    (out / "props_collision.bin").write_bytes(polys["pos"].tobytes())
    (out / "props_collision_index.bin").write_bytes(polys["idx"].tobytes())
    (out / "props_collision_poly.bin").write_bytes(polys["poly"].tobytes())
    animations = {model: meta for model, meta in animations.items() if meta}
    if animations:
        (out / "props_anims.bin").write_bytes(anim_floats.tobytes())
    (out / "props.json").write_text(json.dumps({"models": rigs, "instances": instances, "effects": emitters, "extras": extras, "animations": animations,
                                                **({"items": items} if items else {})}))
    print(f"  live objects {len(instances)}: {sorted({i['class'] for i in instances})}", flush=True)
    if items:
        print(f"  placed items {len(items)}: {sorted({i['class'] for i in items})}", flush=True)
    return ({part["texture"] for parts in records_out.values() for part in parts} | hud | fx_textures
            | {part["texture"] for parts in extras.values() for part in parts})


def extract_map(name, shared, sides):
    """The map's world, once, and every battle fought in it (map_missions:
    both eras' and the historical versions), each with what follows its
    mission: map.json's missions[<mission>] holds its mission, spawn screen
    and vehicle pack, and the runtime plays the one a battle asks for
    (src/mission.js). sides (one, or several in the order tools/prepare.py
    lists them) are searched for placed classes the map's archive lacks."""
    sides = [sides] if isinstance(sides, str) else list(sides)
    out = PUBLIC / "maps" / name
    out.mkdir(parents=True, exist_ok=True)
    data = map_archive(name).read_bytes()
    print(f"map {name}...", flush=True)
    weather = parse_weather(configs(data, b"fx__"))

    # The map's own models first; the shared archives fill in the rest. The
    # low-detail (LOWD) versions are what the far scene draws.
    models = {}
    far_models = {}
    skeletons = {}
    collect_models(data, models, skeletons, far=far_models)
    for archive in shared:
        collect_models(archive, models, skeletons, far=far_models)

    instances = []
    records = []
    world_regions = []
    world_name = name
    terrain = None
    water = None
    for start, end in iter_archives(data):
        for t, b, e in chunks(data, start, end):
            if t == b"wrld":
                instances.extend(parse_instances(data, b, e))
                records.extend(instance_records(data, b, e, property_name))
                world_regions.extend(regions(data, b, e))
                wname = child(data, b, e, b"NAME")
                if wname:
                    world_name = cstr_at(data, *wname)
            elif t == b"tern" and terrain is None:
                terrain = parse_terrain(data, b, e)
                water = parse_water(data, b, e, configs(data, b"fx__"))
    # Ocean/platform maps can retain an editor height field while explicitly
    # disabling terrain. Importing it draws a gray sheet over the sea and
    # leaves an invisible collision floor when the visual sheet is hidden.
    sky_config = (configs(data, b"sky_") or [[]])[0]
    sky_info = section(sky_config, "SkyInfo")
    sun_info = section(sky_config, "SunInfo")
    dome_info = section(sky_config, "DomeInfo")
    if not value(dome_info, "TerrainEnable", [1])[0]:
        terrain = None
    print(f"  instances {len(instances)} terrain {'yes' if terrain else 'no'}", flush=True)

    # Classes lit like soldiers (ODF "Lighting dynamic"): command posts, droids.
    classes = {}
    for archive in [data, *shared]:
        for cname, cls in entity_classes(archive).items():
            classes.setdefault(cname, cls)
    # The original game matches a placed instance's class name without regard
    # to case (bes1 places bes_bldg_aaa_turret, the class is
    # bes_bldg_AAA_turret; Hoth, Rhen Var and the Citadel place many static
    # props the same way): every record and instance takes the class's own
    # spelling here, so each lookup below finds it.
    canonical = {cname.lower(): cname for cname in classes}
    for record in records:
        record["class"] = canonical.get(record["class"].lower(), record["class"])
    instances = [(canonical.get(kind.lower(), kind), raw) for kind, raw in instances]
    # What the map places but neither it nor the shared archives define
    # (Geonosis places the Republic's forward command center, a class and
    # model of SIDE/rep.lvl): the archives of the sides its battles field,
    # read only when a placed class needs one, then searched like the
    # shared ones for models, textures and collision.
    unresolved = {kind for kind, _ in instances
                  if not (models.get(kind) or models.get(class_prop(classes.get(kind) or {"props": []}, "GeometryName", kind)))
                  and (classes.get(kind) or {}).get("base", "").lower() not in LIVE_BASES + FOLIAGE_BASES}
    for fielded in [s for s in sides if s not in mission_sides_of(name)] + mission_sides_of(name):
        if not unresolved or not side_archive(fielded).exists():
            continue
        side_data = side_archive(fielded).read_bytes()
        if not any(kind.encode("latin1", "replace") in side_data for kind in unresolved):
            continue
        side_classes = entity_classes(side_data)
        side_models, side_skeletons, side_far = {}, {}, {}
        collect_models(side_data, side_models, side_skeletons, far=side_far)
        hits = {kind for kind in unresolved
                if kind in side_models or class_prop(side_classes.get(kind) or classes.get(kind) or {"props": []}, "GeometryName", kind) in side_models}
        if not hits:
            continue
        print(f"  from SIDE/{fielded}.lvl: {sorted(hits)}", flush=True)
        unresolved -= hits
        for cname, cls in side_classes.items():
            classes.setdefault(cname, cls)
        for table, found in ((models, side_models), (skeletons, side_skeletons), (far_models, side_far)):
            for model_name, parts in found.items():
                table.setdefault(model_name, parts)
        shared = [*shared, side_data]
        canonical = {cname.lower(): cname for cname in classes}
        for record in records:
            record["class"] = canonical.get(record["class"].lower(), record["class"])
        instances = [(canonical.get(kind.lower(), kind), raw) for kind, raw in instances]
    dynamic_classes = {c for c, cls in classes.items() if (class_prop(cls, "Lighting", "") or "").lower() == "dynamic"}
    # Trees a TreeLine sets along a path (Theed's billboard trees).
    lined = tree_lines(data, classes, spawn_paths(data))
    if lined:
        print(f"  tree lines: {len(lined)} props", flush=True)
        instances.extend(lined)
    # Geometry attached to a placed prop's hardpoints (Jabba, the rancor,
    # the sarlacc's tentacles): live classes among them become live objects
    # below (write_props, as synthetic records at the hardpoint); anything
    # else bakes as an ordinary instance, collision included, which is why
    # this precedes the collision list's snapshot.
    hung = placed_attachments(records, classes, skeletons, models)
    live_hung = [(kind, raw) for kind, raw in hung if live_class(classes.get(kind))]
    if hung:
        print(f"  attached props: {len(hung)} ({', '.join(sorted({k for k, _ in hung}))})", flush=True)
        instances.extend(h for h in hung if h not in live_hung)
    # Foliage: grass and leaf patches, drawn as cards at runtime; the terrain's
    # painted foliage layers plant more of them (and the odd prop).
    collision_instances = list(instances)
    painted = terrain_foliage(data, terrain, classes, water, models=models,
                              executable=GAME.parent.parent / "Battlefront.exe")
    collision_instances.extend(painted["collision"])
    if painted["foliage"] or painted["instances"]:
        print(f"  painted foliage: {len(painted['foliage'])} patches, {len(painted['instances'])} props", flush=True)
        instances.extend(painted["instances"])
    foliage_data = placed_foliage(records, classes, skeletons, painted["foliage"])
    foliage_classes = foliage_data["classes"]
    foliage = foliage_data["instances"]
    live = {c for c, cls in classes.items() if live_class(cls)}

    buckets = {}
    missing = {}
    placed = 0
    # Use the same case-insensitive resource lookup for visible geometry as
    # collision, so restored floors cannot become invisible obstacles.
    folded_models, folded_far_models = {}, {}
    for source, folded in ((models, folded_models), (far_models, folded_far_models)):
        for key, parts in source.items():
            folded.setdefault(key.lower(), parts)
    for kind, raw in instances:
        if kind in live or kind in foliage_classes:
            continue
        parts = folded_models.get(kind.lower()) or folded_models.get(class_prop(classes.get(kind) or {"props": []}, "GeometryName", kind).lower())
        if not parts:
            missing[kind] = missing.get(kind, 0) + 1
            continue
        matrix = mat4(raw)
        for part in parts:
            key = material_key(part, kind in dynamic_classes)
            if key not in buckets:
                buckets[key] = Bucket(key)
            buckets[key].add(part, matrix)
        placed += 1
    print(f"  placed {placed}, missing types {len(missing)}, materials {len(buckets)}", flush=True)
    if missing:
        top = sorted(missing.items(), key=lambda kv: -kv[1])[:12]
        print("  missing:", ", ".join(f"{k}x{v}" for k, v in top), flush=True)

    # The far scene: every object's low-detail model, where it has one.
    far_buckets = {}
    for kind, raw in instances:
        if kind in live:
            continue
        matrix = mat4(raw)
        for part in folded_far_models.get(kind.lower()) or []:
            key = material_key(part, kind in dynamic_classes)
            if key not in far_buckets:
                far_buckets[key] = Bucket(key)
                far_buckets[key].extra = {"far": True}
            far_buckets[key].add(part, matrix)
    print(f"  far scene materials {len(far_buckets)}", flush=True)

    extra = list(far_buckets.values())
    if terrain:
        # Terrain: position, normal, uv, colour (baked light, layer weight).
        ground = Bucket((terrain["layers"][0], "", LIT, (), (), "lit", False))
        ground.add({"positions": terrain["positions"], "normals": terrain["normals"], "uvs": terrain["uvs"],
                    "colors": terrain["colors"], "indices": terrain["indices"]}, IDENTITY)
        ground.extra = {"terrain": True, "layers": terrain["layers"], "scales": terrain["scales"], "detail": terrain["detail"]}
        if terrain["weights"]:
            (out / "terrain_weights.bin").write_bytes(terrain["weights"])
            ground.extra["weights"] = "terrain_weights.bin"
        extra.append(ground)
        if terrain["lumi"]:
            (out / "lumi.bin").write_bytes(terrain["lumi"])

    # The sky: SkyInfo, SunInfo, and DomeInfo from the sky config. DomeInfo
    # lists DomeModels drawn around the eye in order, without depth: the dome,
    # then what stands on its horizon (Geonosis's arena and spires, Hoth's
    # mountains and ion cannon, Endor's Death Star, Rhen Var's cloud layers).
    dome_texture = (value(dome_info, "Texture") or [None])[0]
    for layer, (dome_model, follow) in enumerate(dome_models(dome_info)):
        # Archive model names are folded to lowercase; sky configs can retain
        # mixed case (e.g. Rhen Var). Match them like the native resource lookup.
        dome_parts = (models.get(dome_model) or models.get(dome_model.lower()) or []) if dome_model else []
        for part in dome_parts:
            # The first model is the dome itself, drawn opaque; those over it
            # keep their alpha (mountain and cloud silhouettes).
            flags = part["material"]["flags"] & (HARD_EDGED | TRANSPARENT | ADDITIVE) if layer else 0
            dome = Bucket((part["texture"] or dome_texture or "", "", flags, (), (), "", False))
            dome.add(part, IDENTITY)
            dome.extra = {"sky": True, **({"skyLayer": layer} if layer else {}), **({"skyFollow": follow} if follow else {})}
            extra.append(dome)

    verts, indices, meshes = save_mesh_buffers(buckets, extra)
    (out / "geom.bin").write_bytes(verts.tobytes())
    (out / "index.bin").write_bytes(indices.tobytes())

    print("  collision...", flush=True)
    soldier_shapes = {c: set(class_props(cls, "SoldierCollision")) for c, cls in classes.items() if class_props(cls, "SoldierCollision")}
    paths = spawn_paths(data)
    bounded = any(t == b"bnd_" for start, end in iter_archives(data) for t, _, _ in chunks(data, start, end))
    write_collision([data, *shared], collision_instances, terrain, out, skeletons, soldier_shapes, live,
                    anchors=[(p[0], p[2]) for points in paths.values() for p in points], everywhere=not bounded, classes=classes)
    # The text, an add-on's own strings (its command posts' names) included.
    locl = zero_addon.localization(GAME)
    prop_textures = write_props(records + [{"class": kind, "name": "", "xfrm": raw, "props": {}} for kind, raw in live_hung],
                                classes, live, models, skeletons, [data, *shared], out, locl, soldier_shapes)

    # The haze: the fx config's FogCloud and the dusteffect objects placed.
    atmosphere = parse_atmosphere(configs(data, b"fx__"), records, classes)
    textures = {}
    for archive in [data, *shared]:
        collect_textures(archive, textures)
    sky = sorted(n for n in textures if n.lower().startswith("sky_"))
    minimap_texture = f"{world_name}_map"
    low_res = (value(section(dome_info, "LowResTerrain"), "Texture") or [None])[0]
    wanted = {m["texture"] for m in meshes} | {m["bump"] for m in meshes if m.get("bump")} | set(sky[:1]) | {minimap_texture}
    wanted |= prop_textures
    # The posts' map icons (MapTexture: the objectives' hud_dest_icon) with
    # the map, so a map re-prepared at its generation has them without common
    # (and its fingerprinted common.json) changing.
    wanted |= {class_prop(classes.get(r["class"]), "MapTexture") for r in records
               if (classes.get(r["class"]) or {}).get("base", "").lower() == "commandpost"} - {None}
    wanted |= weather_textures(weather)
    wanted |= atmosphere_textures(atmosphere)
    wanted |= {c["Texture"] for c in foliage_classes.values() if c.get("Texture")}
    if water:
        wanted |= water_textures(water)
    if low_res:
        wanted.add(low_res)
    for m in meshes:
        wanted |= set(m.get("layers") or []) | ({m["detail"]} if m.get("detail") else set())
    texture_paths = write_textures(textures, wanted, out / "textures")

    # Death regions: the original game kills in the regions a battle's script
    # names with AddDeathRegion (looked up by name without regard to
    # case), Tatooine: Dune Sea's sarlacc pit (Sarlac01) among them; the
    # port also keeps every region named deathregion*. The runtime picks a
    # battle's own (src/game.js).
    named = {n.lower() for setup in map_missions(name).values() for n in setup.get("deathRegions") or []}
    deaths = [r for r in world_regions if r["name"].lower().startswith("deathregion") or r["name"].lower() in named]

    # The battles: each mission's teams from its script, the command posts
    # (the same for every battle) from the world.
    skeletons = {}
    for archive in [data, *shared]:
        collect_models(archive, {}, skeletons)
    posts, post_class = command_posts(records, classes, skeletons, world_regions, world_name, locl)
    spawns = command_post_paths(spawn_paths(data), posts)
    print(f"  spawn paths {list(spawns)} death regions {len(deaths)}", flush=True)
    holo_class = classes.get(class_prop(post_class, "HoloOdf", "")) if post_class else None
    battles = {}
    for mission, setup in map_missions(name).items():
        # Each team's display name, from its side (the prefix of its classes).
        for team in setup["teams"].values():
            prefix = (team.get("classes") or [[""]])[0][0].split("_")[0]
            team["label"] = localize(locl, f"common.sides.{prefix}.name") or team.get("name") or prefix
        battles[mission] = {"mission": {"name": mission, **setup}}
        if not setup.get("world"):
            # What the spawn screen shows of the battle: its camera shots, each
            # team's side and objectives, and the vehicles at each post.
            battles[mission]["spawnScreen"] = map_spawn_screen(data, mission, [p["name"] for p in posts], locl)
    kinds = {m: f"{b['mission']['era']}{' historical' if b['mission']['historical'] else ''}" for m, b in battles.items()}
    print(f"  missions {kinds} command posts {[p['name'] for p in posts]}", flush=True)
    elsewhere = {m: b["mission"]["world"] for m, b in battles.items() if b["mission"].get("world")}
    if elsewhere:
        print(f"  fought in worlds of their own (no spawn screen or vehicles here): {elsewhere}", flush=True)
    frame = minimap_frame(posts)
    if frame and minimap_texture in texture_paths:
        frame["texture"] = texture_paths[minimap_texture]

    manifest = {
        "name": name,
        "world": world_name,
        "stride": 32,
        "meshes": meshes,
        "textures": texture_paths,
        "sky": texture_paths.get(sky[0]) if sky else None,
        "spawns": spawns,
        "deathRegions": deaths,
        # Every battle fought in this world, by mission name: its mission,
        # spawnScreen and vehicles, which the runtime plays as map.json's
        # own once a battle picks it (src/mission.js).
        "missions": battles,
        "commandPosts": posts,
        "commandPost": {
            "neutralizeTime": float(class_prop(post_class, "NeutralizeTime", 12)),
            "captureTime": float(class_prop(post_class, "CaptureTime", 10)),
            "holoTurnOnTime": float(class_prop(post_class, "HoloTurnOnTime", 2)),
            "mapScale": float(class_prop(post_class, "MapScale", 1)),
            # Side name -> hologram logo model.
            "logos": {v.split()[0].lower(): v.split()[1].lower() for v in class_props(post_class, "HoloImageGeometry") if len(v.split()) == 2},
        } if post_class else None,
        "hologram": {
            "neutral": floats(class_prop(holo_class, "NeutralColor", "255 255 255")),
            "friendly": floats(class_prop(holo_class, "FriendlyColor", "30 220 30")),
            "enemy": floats(class_prop(holo_class, "EnemyColor", "240 30 30")),
            "locals": floats(class_prop(holo_class, "LocalsColor", "240 220 30")),
            **hologram_look(holo_class),
        } if holo_class else None,
        "minimap": frame,
        "lighting": lighting(sky_info, sun_info),
        "distance": {
            "nearScene": value(sky_info, "NearSceneRange"),
            "farScene": value(sky_info, "FarSceneRange"),
            "lowRes": low_res_terrain(section(dome_info, "LowResTerrain"), texture_paths),
        },
        "post": post_effects(configs(data, b"fx__")),
        "water": water,
        "weather": weather,
        "atmosphere": atmosphere,
        "foliage": {"classes": foliage_classes, "instances": foliage} if foliage else None,
        "treeCanopies": backdrop_canopies(instances, models, foliage_classes),
        "lightRegions": light_regions(world_regions),
        # The engine only darkens soldiers by LUMI when the sky's DomeInfo asks
        # for it with TerrainColorDarkening; Bespin's doesn't.
        "lumi": {"file": "lumi.bin", **terrain["grid"]}
        if terrain and terrain["lumi"] and value(dome_info, "TerrainColorDarkening") is not None
        else None,
    }
    # Each battle's vehicles, a pack of its own (zero_vehicle.pack_file),
    # named in map.json as it is written. A regular battle whose pack fails
    # fails the map after map.json is written (it plays without it); a
    # historical version's is only reported, as no battle plays one yet.
    from zero_vehicle import clear_single_pack, extract_vehicles, pack_file
    clear_single_pack(name)
    failed = []
    for mission, battle in battles.items():
        if battle["mission"].get("world"):
            continue
        try:
            extract_vehicles(name, mission, shared, battle["mission"])
            battle["vehicles"] = pack_file(mission)
        except Exception as error:  # noqa: BLE001 - reported; an era's battle fails the map below
            print(f"  vehicles {mission}: {error}", file=sys.stderr, flush=True)
            if not battle["mission"]["historical"]:
                failed.append(error)
    (out / "map.json").write_text(json.dumps(manifest))
    print(f"  wrote {out} geom={len(verts) * 4 / 1e6:.1f}MB meshes={len(meshes)}", flush=True)
    # The sky's flyers (SkyObject), for the battles whose scripts load their
    # models: sky/, beside map.json and outside the content fingerprint, as
    # they are only drawn (tools/zero_sky.py). A failure leaves the map as
    # it is, without them.
    try:
        here = {}
        if any(entry[0] == "SkyObject" for entry in sky_config):
            scripts, roots = map_scripts(name), sky_mission_roots()
            here = {m: (scripts[m], roots.get(m)) for m, b in battles.items() if m in scripts and not b["mission"].get("world")}
        write_sky_flyers(out / "sky", sky_config, here, textures, eras={m: b["mission"].get("era") for m, b in battles.items()})
    except Exception as error:  # noqa: BLE001 - the map stands without them
        print(f"  sky flyers: {error}", file=sys.stderr, flush=True)
    if failed:
        raise failed[0]
    # The effects its live objects' explosions play.
    live_props = json.loads((out / "props.json").read_text())
    return {i["explosion"][k] for i in live_props["instances"] for k in ("Effect", "WaterEffect") if (i.get("explosion") or {}).get(k)}


def write_actor(models, skeletons, model_name, out):
    """A unit's skinned model (hard skinned, one bone per vertex), packed as
    position, normal, uv, bone (9 floats), as <model>.json, .bin, and
    _index.bin in `out`. Weapons go to the hand's hp_weapons."""
    skeleton = skeletons.get(model_name)
    body = models.get(model_name) or []
    if skeleton is None or not body:
        print(f"  {model_name}: missing skeleton or body", flush=True)
        return None
    verts = array.array("f")
    indices = array.array("I")

    def pack(positions, normals, uvs, idx, joints):
        base = len(verts) // 9
        for i, (pos, nrm, uv) in enumerate(zip(positions, normals, uvs)):
            verts.extend(pos)
            verts.extend(nrm)
            verts.extend(uv)
            verts.append(float(joints[i] if joints is not None else 0))
        index_at = len(indices)
        indices.extend(base + v for v in idx)
        return {"vertexOffset": base * 36, "vertexCount": len(positions), "indexOffset": index_at * 4, "indexCount": len(idx)}

    names = skeleton["names"]
    parts = []
    for part in body:
        joints = part.get("joints")
        if not joints or len(joints) != len(part["positions"]):
            # A rigid part (the jet trooper's jetpack) hangs whole from the
            # node its segment names (BNAM).
            bone = part.get("bone")
            if bone not in names:
                print(f"  {model_name}: a part has neither bones per vertex nor a node", flush=True)
                return None
            joints = [names.index(bone)] * len(part["positions"])
        rec = pack(part["positions"], part["normals"], part["uvs"], part["indices"], joints)
        rec["texture"] = part["texture"]
        parts.append(rec)
    lookup = {bone: i for i, bone in enumerate(names)}
    bones = []
    for i, bone in enumerate(names):
        parent = skeleton["parents"][i]
        parent_i = lookup[parent] if parent and parent in lookup and parent != bone else -1
        bones.append({"name": bone, "parent": parent_i, "local": skeleton["local"][i]})
    actor = {"stride": 36, "bones": bones, "parts": parts, "attach": "hp_weapons"}
    out.mkdir(parents=True, exist_ok=True)
    (out / f"{model_name}.json").write_text(json.dumps(actor))
    (out / f"{model_name}.bin").write_bytes(verts.tobytes())
    (out / f"{model_name}_index.bin").write_bytes(indices.tobytes())
    print(f"  body {model_name}: bones={len(bones)} vertices={sum(p['vertexCount'] for p in parts)}", flush=True)
    return actor


def fire_point(skeleton, name):
    """A hardpoint's position in its model's space."""
    if not skeleton:
        return None
    names = [n.lower() for n in skeleton["names"]]
    key = str(name).lower()
    # Several retail weapon classes name a stale hardpoint, despite their
    # model carrying the standard hp_fire (including shared rifle models).
    if key not in names:
        key = "hp_fire"
    if key not in names:
        return None
    m = skeleton["model"][names.index(key)]
    return [m[12], m[13], m[14]]


def weapon_fire_points(carried, skeletons, models):
    """Muzzles in each displayed model's coordinates, shared by every slot.

    Retail hp_fire records may be absent, zeroed, or recessed. For held
    projectile weapons recover the mouth from the forward end of the mesh.
    The tiny SBD wrist marker has no barrel geometry and keeps its hardpoint.
    """
    # These stock markers miss the visible opening's axis. Axes measured
    # from the central barrel cap (not the crossbow arms or sonic prongs).
    mouth_axes = {
        "all_weap_inf_bowcaster": (.0245, .053),
        "all_1st_weap_inf_bowcaster": (-.0005, .004),
        "wok_weap_inf_bowcaster": (-.0005, .004),
        "wok_1st_weap_inf_bowcaster": (-.0005, .004),
        "geo_weap_inf_blunderbuss": (0, .17),
    }
    points = {}
    for weapon in carried:
        for key in ("GeometryName", "HighResGeometry"):
            name = weapon.get(key)
            if not name:
                continue
            point = fire_point(skeletons.get(name), weapon.get("FirePointName") or "hp_fire")
            vertices = [p for part in models.get(name, []) for p in part["positions"]]
            if weapon.get("ordnance") and weapon.get("base") != "grenade" and vertices:
                front, back = max(p[2] for p in vertices), min(p[2] for p in vertices)
                if front - back >= .05:
                    seed = point
                    if key == "GeometryName" and (not seed or seed[2] <= .05):
                        for other in ("GeometryName", "HighResGeometry"):
                            candidate = fire_point(skeletons.get(weapon.get(other)), weapon.get("FirePointName") or "hp_fire")
                            if candidate and candidate[2] > .05:
                                seed = candidate
                                break
                    if name in mouth_axes:
                        seed = [*mouth_axes[name], seed[2] if seed else front]
                    if seed and seed[2] > .05:
                        # Intersect the model along the authored firing axis.
                        # Rails/sights/prongs often extend past the actual mouth.
                        hits = []
                        for part in models.get(name, []):
                            positions, indices = part["positions"], part.get("indices", [])
                            for i in range(0, len(indices), 3):
                                a, b, c = (positions[j] for j in indices[i:i + 3])
                                bx, by, cx, cy = b[0]-a[0], b[1]-a[1], c[0]-a[0], c[1]-a[1]
                                determinant = bx*cy - by*cx
                                if abs(determinant) < 1e-12:
                                    continue
                                x, y = seed[0]-a[0], seed[1]-a[1]
                                u, v = (x*cy-y*cx)/determinant, (bx*y-by*x)/determinant
                                if u >= -1e-7 and v >= -1e-7 and u+v <= 1+1e-7:
                                    hits.append(a[2] + u*(b[2]-a[2]) + v*(c[2]-a[2]))
                        if hits and max(hits) > (front+back)/2 and (name in mouth_axes or max(hits) >= seed[2] - .08):
                            point = [seed[0], seed[1], max(hits)]
                        else:
                            point = [seed[0], seed[1], front if abs(seed[2] - front) < .05 else seed[2]]
                    else:
                        mouth = [p for p in vertices if p[2] >= front - .003]
                        point = [(min(p[a] for p in mouth) + max(p[a] for p in mouth)) / 2 for a in (0, 1)] + [front]
            if point is not None or name not in points:
                points[name] = point
    return points


def playable_units(side):
    """Ordinary roster classes and mission hero content, in mission order.
    Hero content is available without making it a player-selectable class.
    """
    found = []
    scripts = zero_addon.missions(GAME)
    if scripts:
        for name in sorted(scripts):
            for team in mission_setup(scripts[name])["teams"].values():
                for cls, _ in team.get("classes", []):
                    if (cls.startswith(side + "_") or cls in archive_classes(side)) and cls not in found:
                        found.append(cls)
                hero = team.get("hero")
                if hero and hero.startswith(side + "_") and hero not in found:
                    found.append(hero)
    # Missions may rely on the engine's faction fallback instead of
    # SetHeroClass. Snow Luke is a separate stock content variant.
    for hero in {"rep": ["rep_inf_macewindu"], "cis": ["cis_inf_countdooku"],
                 "all": ["all_inf_lukeskywalker", "all_inf_lukeskywalkersnow"],
                 "imp": ["imp_inf_darthvader"]}.get(side, []):
        if hero not in found:
            found.append(hero)
    return found


ARCHIVE_CLASSES = {}


def archive_classes(side):
    """The classes a side's archive defines: its units are the mission's
    classes named for it, and these whatever their names (SIDE\\des.lvl holds
    Tatooine's tat_inf_tuskenraider, tat_inf_tuskenhunter and tat_inf_jawa,
    and was extracted with no unit at all before)."""
    if side not in ARCHIVE_CLASSES:
        archive = side_archive(side)
        ARCHIVE_CLASSES[side] = frozenset(entity_classes(archive.read_bytes())) if archive.exists() else frozenset()
    return ARCHIVE_CLASSES[side]


def extract_side(side, units, shared):
    """A side's playable units from its archive: each one's class, body,
    first-person arms, and weapons. Bodies go to actors/<model>, arms rigs to
    firstperson/<model>, and the weapons all units carry to one pack. Returns
    the soldiers' skeleton, the effects their weapons use, and the
    first-person skeleton."""
    out = PUBLIC / "sides" / side
    out.mkdir(parents=True, exist_ok=True)
    for stale in ("actor.json", "actor.bin", "actor_index.bin", "firstperson.json", "firstperson.bin", "firstperson_index.bin"):
        (out / stale).unlink(missing_ok=True)
    data = side_archive(side).read_bytes()
    print(f"side {side}...", flush=True)
    models = {}
    skeletons = {}
    collect_models(data, models, skeletons)
    for archive in shared:
        collect_models(archive, models, skeletons)
    entities = weapon_classes(data, b"entc")
    locl = zero_addon.localization(GAME)

    defs = {}
    carried = []
    items = {}  # what dispensers throw and soldiers drop, by class
    used = set()
    fp_textures = {}
    fp_done = set()
    skeleton = fp_skeleton = None
    for unit in units:
        unit_class = soldier_class(data, unit)
        if not unit_class:
            print(f"  {unit}: no such class", flush=True)
            continue
        model_name = unit_class.get("GeometryName") or ""
        actor = write_actor(models, skeletons, model_name, out / "actors")
        if not actor:
            continue
        bounds = MODEL_BOUNDS.get(model_name, {}).get("box")
        if bounds:
            unit_class["bounds"] = bounds
        walker = (unit_class.get("base") or "").lower() == "walkerdroid"
        if walker:
            # The droideka walks by its own bank (cis_walk_droideka), its
            # clips named bare.
            bank = unit_class.get("AnimationName") or model_name
            names = skeletons[model_name]["names"]
            clips = write_anims(data, names, out / "actors", banks=(bank,), names={crc(n): n for n in WALKER_CLIPS}, file=f"{bank}_anims")
            unit_class["clips"] = f"{bank}_anims"
            print(f"  {unit}: walker clips {sorted(clips)}", flush=True)
        elif skeleton is None:
            skeleton = skeletons.get(model_name)
        elif set(skeletons.get(model_name, {}).get("names", [])) != set(skeleton["names"]):
            print(f"  {unit}: its skeleton has other bones than the side's first unit's; clips match by name", flush=True)
        used |= {p["texture"] for p in actor["parts"]}
        used |= {unit_class[k] for k in ("OverrideTexture", "OverrideTexture2", "HealthTexture") if unit_class.get(k)}

        # The loadout: each slot's weapon, its ordnance, and their explosions.
        # Each weapon's display name is weapons. and the class name with its
        # first two underscores as dots (rep_weap_inf_rifle is
        # weapons.rep.weap.inf_rifle).
        weapons = []
        for slot in loadout((entities.get(unit) or {"props": []})["props"]):
            chain = weapon_chain([data, *shared], slot["weapon"])
            if chain:
                weapons.append({**slot, **chain})
        # A droideka's weapons come in WeaponSection groups, each with the
        # aimers that take turns firing it; the first is on the primary
        # trigger, the second on the secondary.
        for n, section in enumerate(unit_class.pop("sections", [])):
            chain = weapon_chain([data, *shared], section.get("WeaponName"))
            if chain:
                weapons.append({"slot": n + 1, "weapon": section["WeaponName"], "ammo": int(float(section.get("WeaponAmmo", 0))),
                                "channel": int(float(section.get("WeaponChannel", n))), "aimers": section["aimers"], **chain})
        for w in weapons:
            w["label"] = localize(locl, "weapons." + w["class"].replace("_", ".", 2)) or w["class"]
            # The spawn screen's name for it, without the line break some end in.
            w["displayName"] = weapon_name(locl, w["class"])
            if w.get("item"):
                items[w["item"]["class"]] = w["item"]
        carried += weapons
        # The explosion a droid blows apart with, and what it may drop.
        if unit_class.get("ExplosionName"):
            unit_class["explosion"] = explosion_class([data, *shared], unit_class["ExplosionName"])
        for drop in unit_class.get("drops", []):
            if drop["class"] not in items:
                item = item_class([data, *shared], drop["class"])
                if item:
                    items[drop["class"]] = item

        # First person: the arms rig the class names ("REP\\reptroop;rep_1st_trooper"
        # is model rep_1st_trooper in FPM/REP/reptroop.lvl).
        archive_name, _, fp_model = (unit_class.get("FirstPerson") or "").partition(";")
        fp_archive = first_person_archive(archive_name) if archive_name else None
        fp_file = out / "firstperson" / f"{fp_model}.json"
        if fp_archive and fp_archive.exists() and fp_model not in fp_done:
            fp_done.add(fp_model)
            fp_data = fp_archive.read_bytes()
            fp_models, fp_skeletons = {}, {}
            collect_models(fp_data, fp_models, fp_skeletons)
            collect_textures(fp_data, fp_textures)
            rig_skeleton = fp_skeletons.get(fp_model)
            (out / "firstperson").mkdir(exist_ok=True)
            # Skinned arms (the droideka's) split into a part per bone.
            pack = {fp_model: rig_parts(fp_models.get(fp_model) or [], rig_skeleton)}
            rig = write_meshes(pack, [fp_model], out / "firstperson", fp_model, local=True)
            if rig_skeleton and rig.get(fp_model):
                names = rig_skeleton["names"]
                bones = [{"name": n, "parent": names.index(p) if p in names and p != n else -1, "local": rig_skeleton["local"][i]}
                         for i, (n, p) in enumerate(zip(names, rig_skeleton["parents"]))]
                fp_file.write_text(json.dumps({"model": fp_model, "bones": bones, "parts": rig[fp_model]}))
                # Every rig's bones, so no arms rig loses tracks (the super
                # battle droid's has only a right arm).
                fp_names = fp_skeleton["names"] if fp_skeleton else []
                fp_skeleton = {"names": fp_names + [n for n in names if n not in fp_names]}
                print(f"  first person {fp_model}: bones {names}", flush=True)
        elif archive_name and not (fp_archive and fp_archive.exists()):
            print(f"  first person: no {fp_archive}", flush=True)
        if fp_file.exists():
            used |= {part["texture"] for part in json.loads(fp_file.read_text())["parts"]}
        defs[unit] = {**unit_class, "model": model_name, "firstPerson": fp_model if fp_file.exists() else None, "weapons": weapons,
                      "displayName": unit_name(locl, unit)}
        print(f"  unit {unit}: body {model_name}, speed {unit_class.get('MaxSpeed')}, weapons {[w['class'] for w in weapons]}", flush=True)

    # Held models with their fire points (third and first person), and the
    # ordnance models in flight, for every unit's weapons (a remote droid's
    # included), and the remote droids themselves.
    carried = with_remotes(carried)
    droids = {w["droid"]["GeometryName"] for w in carried if (w.get("droid") or {}).get("GeometryName")}
    held = sorted({w[key] for w in carried for key in ("GeometryName", "HighResGeometry") if w.get(key)})
    flying = sorted({o["GeometryName"] for w in carried for o in rounds(w) if o.get("GeometryName")}
                    | {c["ChunkGeometryName"] for d in defs.values() for c in d.get("chunks", []) if c.get("ChunkGeometryName")}
                    | {i["GeometryName"] for i in items.values() if i.get("GeometryName")}
                    | droids)
    meshes = write_meshes(models, held + flying, out, "weapons")
    points = weapon_fire_points(carried, skeletons, models)
    points.update({name: fire_point(skeletons.get(name), "hp_fire") for name in droids})

    textures = {}
    collect_textures(data, textures)
    for archive in shared:
        for name, image in collect_textures_from(archive).items():
            textures.setdefault(name, image)
    for name, image in fp_textures.items():
        textures.setdefault(name, image)
    used |= {part["texture"] for parts in meshes.values() for part in parts}
    for w in carried:
        used |= {w.get(k) for k in ("IconTexture", "ReticuleTexture", "ScopeTexture") if w.get(k)}
        if w.get("base") == "melee":
            used |= {w.get("Texture"), "lightsabreglow"} - {None}
        if (w.get("droid") or {}).get("HealthTexture"):
            used.add(w["droid"]["HealthTexture"])
        for ordnance in rounds(w):
            if ordnance.get("LaserTexture"):
                used |= {ordnance["LaserTexture"], "lightglow"}

    # Effects the units themselves play: the jetpack's flames, a droid's
    # explosion and its flying pieces' smoke and dust, and gaining health or
    # ammo; and the explosions of what they drop.
    wanted = {d["JetEffect"] for d in defs.values() if d.get("JetEffect")}
    for d in defs.values():
        wanted |= {d.get("HealthGainEffect") or "Powerup_blue", d.get("AmmoGainEffect") or "Powerup_red"}
        # A droideka dies in blasts (explosion, med_explosion, small_explosion
        # at its nodes), then its dust cloud.
        if (d.get("base") or "").lower() == "walkerdroid":
            wanted |= set(WALKER_DEATH_BLASTS) | {d[k] for k in ("DeathDustEffect", "WakeEffect") if d.get(k)}
        for explosion in [d.get("explosion") or {}]:
            wanted |= {explosion[k] for k in ("Effect", "WaterEffect") if explosion.get(k)}
        for c in d.get("chunks", []):
            wanted |= {c[k] for k in ("ChunkTrailEffect", "ChunkTerrainEffect") if isinstance(c.get(k), str)}
    for item in items.values():
        for key in ("ExplosionTrigger", "ExplosionExpire", "ExplosionDeath"):
            if isinstance(item.get(key), dict):
                wanted |= {item[key][k] for k in ("Effect", "WaterEffect") if item[key].get(k)}
    for w in carried:
        if w.get("base") == "melee":
            wanted |= {"lightsabre_hit", "lightsabre_deflect"}
        for ordnance in rounds(w):
            for key in ("TrailEffect", "ImpactEffectSoft", "ImpactEffectRigid", "ImpactEffectStatic",
                        "ImpactEffectTerrain", "ImpactEffectWater", "ImpactEffectShield"):
                if ordnance.get(key):
                    wanted.add(ordnance[key])
            for key in ("ExplosionExpire", "ExplosionImpact"):
                for effect_key in ("Effect", "WaterEffect"):
                    if ordnance.get(key, {}).get(effect_key):
                        wanted.add(ordnance[key][effect_key])
        if w.get("MuzzleFlashEffect"):
            wanted.add(w["MuzzleFlashEffect"])
        # The arc caster's glow while charging, and its lightning; the
        # droideka's shield.
        if w.get("ChargeUpEffect"):
            wanted.add(w["ChargeUpEffect"])
        if w.get("ShieldEffect"):
            wanted.add(w["ShieldEffect"])
        wanted |= {o["LightningEffect"] for o in rounds(w) if o.get("LightningEffect")}
        blast = (w.get("droid") or {}).get("explosion") or {}
        wanted |= {blast[k] for k in ("Effect", "WaterEffect") if blast.get(k)}

    # Effects only the side's archive holds (the droideka's shield and
    # explosion) stay with the side, with their textures and meshes; the rest
    # go to common.
    shared_fx = [effects(archive) for archive in shared]
    side_fx = [effects(data)]
    own = {}
    for name in sorted(wanted):
        if name in ("lightsabre_hit", "lightsabre_deflect") and effect(shared_fx, name) is not None:
            # Keep hero additions self-contained for a side-only refresh;
            # this does not rewrite the shared effects/animation packs.
            own[name] = effect(shared_fx, name)
        elif effect(shared_fx, name) is None and effect(side_fx, name) is not None:
            own[name] = effect(side_fx, name)
    fx_models = set()
    for emitters in own.values():
        t, m = effect_textures(emitters)
        used |= t
        fx_models |= m
    fx_meshes = write_meshes(models, sorted(fx_models), out, "fxmodels")
    used |= {part["texture"] for parts in fx_meshes.values() for part in parts}
    (out / "effects.json").write_text(json.dumps({"effects": own, "models": fx_meshes}))

    paths = write_textures(textures, used, out / "textures")
    # What the shared step (extract_common) takes from the side: its
    # soldiers' bones, its arms rigs' bones, and the effects it wants.
    (out / "units.json").write_text(json.dumps({
        "units": defs, "items": items, "textures": paths,
        "skeleton": skeleton["names"] if skeleton else [],
        "firstPersonBones": fp_skeleton["names"] if fp_skeleton else [],
        "wantedEffects": sorted(wanted - set(own)),
    }))
    (out / "weapons.json").write_text(json.dumps({"models": meshes, "firePoints": points}))
    if own:
        print(f"  side effects {sorted(own)}", flush=True)
    return skeleton, wanted - set(own), fp_skeleton


# The droideka's clips, named bare in its bank, and the blasts it dies in.
WALKER_CLIPS = (
    "idle", "turnl", "turnr", "death01", "idle_to_leftfoot", "walk_leftfoot_rightfoot", "walk_rightfoot_leftfoot",
    "leftfoot_to_idle", "rightfoot_to_idle", "fold", "roll", "unfold",
)
WALKER_DEATH_BLASTS = ("explosion", "med_explosion", "small_explosion")


def rounds(weapon):
    """A weapon's ordnance classes: its own, each charge level's, and a
    destruct weapon's strike."""
    found = [weapon.get("ordnance"), weapon.get("strike")] + [level.get("ordnance") for level in weapon.get("charges", [])]
    return [o for o in found if o]


def weapon_texture_dependencies(weapon):
    """Sprites referenced by a weapon, including every possible projectile.
    Map turrets need these even when the player's side never uses that bolt."""
    used = {weapon[k] for k in ("IconTexture", "ReticuleTexture", "ScopeTexture") if weapon.get(k)}
    for ordnance in rounds(weapon):
        if ordnance.get("LaserTexture"):
            used.update((ordnance["LaserTexture"], "lightglow"))
    return used


def with_remotes(weapons):
    """Weapons, and the weapons of the droids remote weapons launch."""
    out = []
    for w in weapons:
        out.append(w)
        out += with_remotes((w.get("droid") or {}).get("weapons", []))
    return out


def collect_textures_from(archive):
    found = {}
    collect_textures(archive, found)
    return found


# HUD art the port draws: the panels, command post and map icons, team logos.
HUD_TEXTURES = (
    "hud_flag_icon", "hud_flag_timer", "flag_icon", "hud_icon_flash", "player_icon", "troop_icon",
    "hud_map_raster", "commandpostflare", "rep_icon", "cis_icon", "all_icon", "imp_icon",
    "hud_health", "hud1", "hud2", "player_shadow", "map_mask", "hit_indicator", "ambush_icon",
    "hud_jetpack_icon", "hud_vehiclehealth", "hud_active_repair_icon", "hud_shield_icon",
    "stats_kills", "stats_deaths", "stats_cpp", "turret_icon", "rechargedroid_icon",
)
# The HUD's fonts, and the text it shows (localization keys).
HUD_FONTS = ("gamefont_tiny", "gamefont_small", "gamefont_medium", "gamefont_large")
HUD_STRINGS = (
    "game.infomsg.captured", "game.infomsg.lost", "game.infomsg.killed", "game.infomsg.died",
    "game.infomsg.suicide", "game.infomsg.teamkill", "game.tooltips.leaving_battlefield", "game.tooltips.flyertakeoff",
    "common.sides.rep.name", "common.sides.cis.name", "common.sides.all.name", "common.sides.imp.name",
    "common.vehiclepositions.pilot", "common.vehiclepositions.copilot", "common.vehiclepositions.gunner",
    "common.vehiclepositions.bomber", "common.vehiclepositions.leftturret", "common.vehiclepositions.rightturret",
    "common.vehiclepositions.passenger",
)
# Text the original game looks up by hash rather than by key.
HUD_HASHED_STRINGS = {"hud.victory_in": 0x634F877, "hud.defeat_in": 0xEDD20CDE}
# First-person camera motions from the globals script, and the curves' keys.
CAMERA_MOTIONS = ("RollLeft", "RollRight", "CrouchToProne", "Land", "StandToProne", "ProneToStand", "CrouchToStand")
MOTION_KEYS = ("Vec", "Slope", "Ore", "RSlope", "Time")
HOLOGRAM_LOGOS = ("com_icon_republic", "com_icon_cis", "com_icon_alliance", "com_icon_imperial")


def write_meshes(models, names, out, file, local=False):
    """Small static models (position, normal, uv) packed into one buffer, in
    the model's space, or with `local` in the space of the bone each part
    hangs from (for rigs posed at runtime)."""
    verts = array.array("f")
    indices = array.array("I")
    records = {}
    for name in names:
        parts = models.get(name) or []
        for part in parts:
            base = len(verts) // 8
            positions = part["local_positions"] if local else part["positions"]
            normals = part["local_normals"] if local else part["normals"]
            for p, n, uv in zip(positions, normals, part["uvs"]):
                verts.extend(p)
                verts.extend(n)
                verts.extend(uv)
            index_at = len(indices)
            indices.extend(base + i for i in part["indices"])
            records.setdefault(name, []).append({
                "texture": part["texture"],
                "vertexOffset": base * 32,
                "vertexCount": len(part["local_positions"]),
                "indexOffset": index_at * 4,
                "indexCount": len(part["indices"]),
                "additive": part["additive"],
                "transparent": part["transparent"],
                "flags": part["material"]["flags"],
                "bone": part.get("bone"),
                **({"scroll": part["material"]["scroll"]} if part["material"].get("scroll") else {}),
            })
    (out / f"{file}.bin").write_bytes(verts.tobytes())
    (out / f"{file}_index.bin").write_bytes(indices.tobytes())
    return records


# The engine's built-in muzzle flash draws this mesh.
MUZZLE_FLASH = "muzzleflash"


def merged_bones(path, names):
    """A clip bank's bone list: the bones it had when another side was
    extracted, then any new ones, so one side doesn't drop another's."""
    try:
        known = json.loads(path.read_text()).get("bones", []) if path.exists() else []
    except (OSError, ValueError, AttributeError):
        known = []
    return known + [n for n in names if n not in known]


def catalog_map_archives():
    """The archives of the maps the front end lists (common/shell/shell.json,
    tools/zero_shell.py --catalog), or of every map on disk before it is
    written: a planet folder's maps, without the campaign's (_h) versions,
    and the add-ons' maps."""
    try:
        ids = [entry["id"] for entry in json.loads((PUBLIC / "common" / "shell" / "shell.json").read_text())["maps"]]
        paths = [map_archive(map_id) for map_id in ids]
        if paths and all(path.exists() for path in paths):
            return paths
    except (OSError, ValueError, KeyError, TypeError):
        pass
    stock = [path for path in sorted(GAME.glob("*/*.lvl"))
             if path.stem[:3].upper() == path.parent.name and "_" not in path.stem and not path.stem.endswith("h")]
    added = [map_archive(entry["mapluafile"].lower()) for entry in zero_addon.listed_maps(GAME)]
    return stock + [path for path in added if path.exists()]


def gathered_sides():
    """What every extracted side recorded for the shared step (extract_side):
    its soldiers' bones, its arms rigs' bones, and the effects it wants."""
    names, fp_names, wanted = [], [], set()
    for path in sorted((PUBLIC / "sides").glob("*/units.json")):
        try:
            info = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        names += [n for n in info.get("skeleton", []) if n not in names]
        fp_names += [n for n in info.get("firstPersonBones", []) if n not in fp_names]
        wanted |= set(info.get("wantedEffects", []))
    return names, fp_names, wanted


# What reading an archive the tools can't make sense of raises.
UNREADABLE = (OSError, ValueError, IndexError, KeyError, TypeError, struct.error)


def add_on_archive(path):
    """Whether an archive is an add-on's (GameData/AddOn): one that can't be
    read is passed over where every map is scanned, and its map is left out
    (tools/prepare.py), rather than failing what every side shares."""
    return zero_addon.source_name(path, GAME).startswith("AddOn/")


def live_effects(map_paths, shared):
    """The explosions the maps' live objects (turrets, recharge droids) play,
    from each map archive's classes and the shared ones."""
    wanted = set()
    for path, archive in [*((None, data) for data in shared), *((path, None) for path in map_paths)]:
        try:
            archive = path.read_bytes() if archive is None else archive
            classes = entity_classes(archive).values()
        except UNREADABLE as error:
            if path is None or not add_on_archive(path):
                raise
            print(f"  {zero_addon.source_name(path, GAME)}: unreadable, passed over ({error})", flush=True)
            continue
        for cls in classes:
            if not live_class(cls):
                continue
            blast = class_prop(cls, "ExplosionName") or class_prop(cls, "Explosion")
            explosion = explosion_class([archive, *shared], blast) if blast else None
            wanted |= {explosion[k] for k in ("Effect", "WaterEffect") if (explosion or {}).get(k)}
    return wanted


def effects_elsewhere(names, map_paths):
    """Effects the shared archives lack, from whichever map archive holds
    them (the droideka's hailfire_wake is in the maps hover vehicles fight
    on, not in common.lvl): name -> emitters. Archives are read one at a
    time and parsed only when a name's hash is in them."""
    found = {}
    missing = set(names)
    for path in map_paths:
        if not missing:
            break
        data = path.read_bytes()
        hit = [n for n in sorted(missing) if struct.pack("<I", fnv1a(n)) in data]
        if not hit:
            continue
        try:
            fx = effects(data)
        except UNREADABLE as error:
            if not add_on_archive(path):
                raise
            print(f"  {zero_addon.source_name(path, GAME)}: unreadable, passed over ({error})", flush=True)
            continue
        for name in hit:
            emitters = effect([fx], name)
            if emitters is not None:
                found[name] = emitters
                missing.discard(name)
                print(f"  effect {name}: from {path.name}", flush=True)
    return found


def extract_common(skeleton, shared, wanted_effects, fp_skeleton=None, maps=None):
    """What every side shares: the soldier and arms clips for every side's
    bones, the hologram logos, the effects the sides and the maps' live
    objects play, the HUD's art, fonts and text. Takes what this run's side
    gave (skeleton, wanted_effects, fp_skeleton), and what the extracted
    sides recorded (sides/<side>/units.json), and scans maps (archive paths,
    default: the catalog's) for their live objects' effects, and for the
    effects the shared archives lack."""
    out = PUBLIC / "common"
    out.mkdir(parents=True, exist_ok=True)
    common = common_archive().read_bytes()
    print("common...", flush=True)
    map_paths = catalog_map_archives() if maps is None else list(maps)
    bones, fp_bones, gathered = gathered_sides()
    bones = merged_bones(out / "anims.json", bones + [n for n in (skeleton or {}).get("names", []) if n not in bones])
    fp_bones = merged_bones(out / "anims_fp.json", fp_bones + [n for n in (fp_skeleton or {}).get("names", []) if n not in fp_bones])
    wanted_effects = set(wanted_effects) | gathered | live_effects(map_paths, shared)
    clips = write_anims(common, bones, out) if bones else {}
    print(f"  clips {len(clips)} for {len(bones)} bones", flush=True)
    if fp_bones:
        fp_clips = write_anims(common, fp_bones, out, banks=(FIRST_PERSON_BANK,), names={crc(n): n for n in FIRST_PERSON}, file="anims_fp")
        print(f"  first-person clips {len(fp_clips)}", flush=True)
    models = {}
    for archive in shared:
        collect_models(archive, models)
    # The spawn screen's background room rides after the logos.
    holograms = write_meshes(models, HOLOGRAM_LOGOS + SPAWN_MODELS, out, "holograms")

    # Particle effects, the meshes some of them draw, and the muzzle flash;
    # with those another side's extraction asked for.
    known = out / "effects.json"
    if known.exists():
        try:
            wanted_effects = set(wanted_effects) | set(json.loads(known.read_text()).get("effects", {}))
        except (OSError, ValueError, AttributeError):
            print("  effects.json from an earlier run is unreadable; written afresh", flush=True)
    found = [effects(archive) for archive in shared]
    elsewhere = effects_elsewhere([n for n in sorted(wanted_effects) if effect(found, n) is None], map_paths)
    fx = {}
    fx_textures, fx_models = set(), {MUZZLE_FLASH}
    for name in sorted(wanted_effects):
        emitters = effect(found, name)
        if emitters is None:
            emitters = elsewhere.get(name)
        if emitters is None:
            print(f"  effect {name}: not found", flush=True)
            continue
        fx[name] = emitters
        t, m = effect_textures(emitters)
        fx_textures |= t
        fx_models |= m
    fx_meshes = write_meshes(models, sorted(fx_models), out, "fxmodels")

    textures = {}
    for archive in shared:
        collect_textures(archive, textures)
    # The front end's art, from common.lvl, Shell/ENG/shell.lvl and Load/load.lvl.
    for name, image in shell_textures().items():
        textures.setdefault(name, image)
    used = {part["texture"] for parts in holograms.values() for part in parts}
    used |= {part["texture"] for parts in fx_meshes.values() for part in parts} | fx_textures

    # The HUD's fonts and text, from core.lvl.
    core = GAME / "core.lvl"
    core_data = core.read_bytes() if core.exists() else b""
    fonts, pages = collect_fonts(core_data)
    fonts = {name: font for name, font in fonts.items() if name in HUD_FONTS}
    for font in fonts.values():
        for page in font["pages"]:
            textures.setdefault(page, pages[page])
        used |= set(font["pages"])
    locl = zero_addon.localization(GAME)
    strings = {key: localize(locl, key) for key in HUD_STRINGS if localize(locl, key)}
    strings.update({key: locl[h] for key, h in HUD_HASHED_STRINGS.items() if h in locl})
    strings.update(spawn_strings(locl))
    strings.update(scoreboard_strings(locl))
    strings.update(shell_strings(locl))

    # The first-person camera motions the globals script sets up.
    scripts = mission_scripts(common)
    found_motions = global_tables(load_lua(scripts["globals"])) if "globals" in scripts else {}
    motions = {
        name: {key: [[round(x, 6) for x in v] if isinstance(v, list) else round(v, 6) for v in found_motions[name][key]] for key in MOTION_KEYS}
        for name in CAMERA_MOTIONS
        if isinstance(found_motions.get(name), dict) and all(key in found_motions[name] for key in MOTION_KEYS)
    }

    wanted = set(HUD_TEXTURES) | set(SPAWN_TEXTURES) | set(SCOREBOARD_TEXTURES) | set(SHELL_COMMON_TEXTURES) | used
    paths = write_textures(textures, wanted, out / "textures")
    (out / "common.json").write_text(json.dumps({
        "holograms": holograms, "textures": paths, "fonts": fonts, "strings": strings, "cameraMotions": motions,
        # Art tools/upscale_hud.py enlarges besides its name rule: the spawn
        # screen's, then the scoreboard's, then the front end's.
        "rasterHud": [name for name in RASTER_HUD + SCOREBOARD_RASTER + SHELL_RASTER if name in paths],
    }))
    (out / "effects.json").write_text(json.dumps({"effects": fx, "models": fx_meshes}))
    try:
        from upscale_hud import convert as upscale_hud
    except ImportError:
        print("  raster HUD skipped: install Pillow to generate enlarged lossless WebP atlases", flush=True)
    else:
        upscale_hud(out)
    print(f"  hologram logos {sorted(holograms)} effects {sorted(fx)} textures {len(paths)}", flush=True)
    print(f"  fonts {sorted(fonts)} strings {len(strings)} camera motions {sorted(motions)}", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("map", nargs="?", default="bes2")
    parser.add_argument("--side", action="append", help="a side of the map's battles (repeatable; default rep): its units, and its archive "
                                                         "for placed classes the map's lacks. Every battle of the map is extracted either way")
    parser.add_argument("--unit", action="append", help="a unit to extract (repeatable; default: every unit the side's battles use)")
    parser.add_argument("--only", choices=("side", "common", "map", "audio"),
                        help="one part alone: the sides' units (--side), what every side shares (from the sides extracted so far "
                             "and the catalog's maps), the map with its audio and AI, or the audio alone")
    parser.add_argument("--skip-shared", action="store_true", help="the map alone, its sides and common/ being extracted already (tools/prepare.py); --only map")
    args = parser.parse_args()
    sides = args.side or ["rep"]
    only = "map" if args.skip_shared else args.only
    shared = shared_archives()
    # KTX2 copies of the textures written (tools/ktx2.py). Optional: without
    # an encoder it only warns, and the game loads the PNGs.
    from ktx2 import encode_optional
    # What other extractions share is written under a lock (zero_paths), so
    # tools/prepare.py can run maps in parallel; the map itself is this run's.
    if only == "common":
        with shared_lock("common"):
            extract_common(None, shared, set(), None)
            encode_optional([PUBLIC / "common"])
        return 0
    if only == "side":
        for side in sides:
            if not side_archive(side).exists():
                print(f"no archive for side {side}: expected {side_archive(side)}", file=sys.stderr)
                return 1
            with shared_lock(f"side-{side}"):
                extract_side(side, args.unit or playable_units(side), shared)
                extract_side_ai(side)
                encode_optional([PUBLIC / "sides" / side])
        return 0
    if not map_archive(args.map).exists():
        print(f"no archive for {args.map}: expected {map_archive(args.map)}", file=sys.stderr)
        return 1
    if only == "audio":
        extract_map_audio(args.map)
        return 0
    skeleton = fp_skeleton = None
    wanted_effects = set()
    if only is None:
        for side in sides:
            with shared_lock(f"side-{side}"):
                side_skeleton, side_wanted, side_fp = extract_side(side, args.unit or playable_units(side), shared)
                extract_side_ai(side)
                encode_optional([PUBLIC / "sides" / side])
            skeleton = skeleton or side_skeleton
            wanted_effects |= side_wanted
            if side_fp:
                names = (fp_skeleton or {"names": []})["names"]
                fp_skeleton = {"names": names + [n for n in side_fp["names"] if n not in names]}
    wanted_effects |= extract_map(args.map, shared, sides)
    encode_optional([PUBLIC / "maps" / args.map])
    if only is None:
        with shared_lock("common"):
            extract_common(skeleton, shared, wanted_effects, fp_skeleton, maps=[map_archive(args.map)])
            encode_optional([PUBLIC / "common"])
    extract_map_audio(args.map)
    extract_ai(args.map)
    return 0


def extract_map_audio(name):
    """The sound of each of the map's battles: its mission's sound
    sub-level, which public/audio/index.json names for the mission. A
    historical version shares its era's sub-level, so it is written first
    and the sub-level keeps the era's own battle. A battle fought in a world
    of its own (map_missions) is that world's to extract."""
    missions = {m: s for m, s in map_missions(name).items() if not s.get("world")}
    for mission in sorted(missions, key=lambda m: not missions[m]["historical"]):
        extract_audio(name, mission=mission)


def extract_ai(name, side="rep"):
    """The Classic AI's data for a map (docs/ai.md): its planning graph,
    hints and barriers (ai-nav.json), and its mission calls and command-post
    properties (ai-mission.json): every mission script of the map's, its
    `mission` the side's battle until the runtime picks one
    (src/mission.js). Each is optional: a failure is reported and the rest
    of the extraction stands."""
    from zero_ai import export_map
    from zero_ai_mission import write as write_ai_mission
    for label, job in (("ai-nav", lambda: export_map(name)), ("ai-mission", lambda: write_ai_mission(name, side))):
        try:
            job()
        except (ValueError, OSError, KeyError) as error:
            print(f"  {label}: {error}", file=sys.stderr)


def extract_side_ai(side):
    """The side's class and weapon AI properties (sides/<side>/ai.json);
    the caller holds the side's lock. Optional, like extract_ai."""
    from zero_ai_classes import extract as extract_ai_classes
    try:
        extract_ai_classes(side)
    except (ValueError, OSError, KeyError) as error:
        print(f"  ai classes ({side}): {error}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
