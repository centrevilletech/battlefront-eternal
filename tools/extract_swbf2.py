"""2005 (Star Wars: Battlefront II) map, side, and common extractors.

Called by tools/extract.py when BFE_FAMILY=swbf2. Keeps the 2004 extraction
pipeline completely separate and untouched, while writing the same prepared
file layout (maps/<map>/, sides/<side>/, common/) extended with optional keys
for 2005-only features (layers, lights, portals, world bounds, full ODF
properties, cloth, combos, animation banks).
"""

import array
import json
import math
import os
import re
import sys
from pathlib import Path
import struct

from portable_math import atan2, cos, sin
from ucfb import child, chunks, cstr_at, iter_all_archives, named_subarchives, root_section
from xform import IDENTITY, mat4, mul, transform_point
import zero_addon
from zero_ai import world_navigation_records
from zero_class_art2 import loose_textures, write_map_art
from zero_config import configs, parse as parse_config, section, value
import zero_family
from zero_font import collect_fonts
from zero_fx import effect, effects, textures as effect_textures
from zero_hash import crc, fnv1a
from zero_locl import localize
from zero_mission2 import OPENING_MISSIONS, layer_name_parts, mission_assets, opening_manifest
from zero_model import (
    ADDITIVE,
    BOUNDS as MODEL_BOUNDS,
    HARD_EDGED,
    LIT,
    TRANSPARENT,
    bind_bones,
    collect_models,
    swbf2_surface,
)
from zero_paths import GAME, PUBLIC, write_text_lf
from zero_saber import saber_hilts
from zero_scripts2 import script_chain, script_index, write_if_changed
import zero_swbf2
from zero_vehicle2 import vehicle_pack, vehicle_sections, write_pack
from zero_terrain import parse_terrain
from zero_texture import SWBF2_DECODABLE, collect_textures
from zero_water import parse_water, water_textures
from zero_weather import parse_weather, weather_textures
from zero_world import (
    coll_meshes,
    instance_records,
    place_mesh,
    place_shape,
    prim_shapes,
    regions,
)

# Placed entity bases that are live gameplay objects or non-static world records.
LIVE_OR_SPECIAL_BASES = frozenset({
    "commandpost",
    "armedbuilding",
    "armedbuildingdynamic",
    "destructablebuilding",
    "door",
    "powerupstation",
    "vehiclespawn",
    "flag",
    "remoteterminal",
    "animatedbuilding",
    "animatedprop",
    "shield",
    "hinge",
    "light",
    "soundambiencestatic",
    "soundambiencestreaming",
    "godray",
    "dusteffect",
    "rumbleeffect",
    "pathfollower",
    "hologram",
})

MODELLESS_BASES = frozenset({
    "light",
    "vehiclespawn",
    "soundambiencestatic",
    "soundambiencestreaming",
    "godray",
    "dusteffect",
    "rumbleeffect",
    "pathfollower",
    "hologram",
})

PLAYABLE_SIDE_BASES = frozenset({
    "soldier",
    "walkerdroid",
    "flyer",
    "commandflyer",
    "hover",
    "walker",
    "armedbuilding",
    "armedbuildingdynamic",
    "destructablebuilding",
    "portableturret",
    "turret",
    "commandwalker",
    "droid",
})


def _prop_dict(cls):
    """Last value per key from a class's ordered (key, value) props."""
    out = {}
    for k, v in (cls or {}).get("props", ()):
        out[k] = v
    return out


def _prop_list(cls, key):
    return [v for k, v in (cls or {}).get("props", ()) if k == key]


def _prop(cls, key, default=None):
    vals = _prop_list(cls, key)
    return vals[-1] if vals else default


def _update_bounds(bounds, point):
    for axis, coord in enumerate(point):
        if not math.isfinite(coord):
            raise ValueError(f"Nonfinite world position {point}")
        bounds[0][axis] = min(bounds[0][axis], coord)
        bounds[1][axis] = max(bounds[1][axis], coord)


def _empty_bounds():
    return [[float("inf"), float("inf"), float("inf")], [float("-inf"), float("-inf"), float("-inf")]]


def _finalize_bounds(bounds):
    if not math.isfinite(bounds[0][0]):
        return {"min": [0.0, 0.0, 0.0], "max": [0.0, 0.0, 0.0]}
    return {
        "min": [math.floor(v * 1000) / 1000 for v in bounds[0]],
        "max": [math.ceil(v * 1000) / 1000 for v in bounds[1]],
    }


_AUDIO_REGION_PREFIXES = ("soundstream", "soundstatic", "soundspace")


def _update_region_bounds(bounds, reg):
    """Expand bounds by all 8 transformed corners of a gameplay region.

    Ambient sound reach regions (`soundstream`, `soundstatic`, `soundspace`)
    span kilometres of audio falloff rather than reachable play space and are
    excluded from wire bounds.
    """
    rname = (reg.get("name") or "").strip().lower()
    if any(rname.startswith(prefix) for prefix in _AUDIO_REGION_PREFIXES):
        return
    m = reg.get("matrix")
    size = reg.get("size")
    if not m or len(m) < 16:
        return
    if not size or len(size) < 3:
        _update_bounds(bounds, (m[12], m[13], m[14]))
        return
    rtype = (reg.get("type") or "").strip().lower()
    sx, sy, sz = abs(float(size[0])), abs(float(size[1])), abs(float(size[2]))
    if rtype == "sphere":
        extent = [math.hypot(sx, sy, sz)] * 3
    elif rtype == "cylinder":
        axes = [m[i:i + 3] for i in (0, 4, 8)]
        lengths = [math.hypot(*v) for v in axes]
        radius = math.hypot(sx * lengths[0], sz * lengths[2])
        extent = [radius * math.hypot(axes[0][i] / (lengths[0] or 1),
                                     axes[2][i] / (lengths[2] or 1)) + sy * abs(axes[1][i])
                  for i in range(3)]
    else:
        extent = [sx * abs(m[i]) + sy * abs(m[i+4]) + sz * abs(m[i+8]) for i in range(3)]
    for sign in (-1, 1):
        _update_bounds(bounds, [m[12+i] + sign * extent[i] for i in range(3)])


def _motion_radius(clip):
    """Maximum planar distance of constant forward travel through linear yaw keys.

    Reflection of either local axis leaves this radius unchanged. On straights
    only endpoints can be farthest. On an arc also test the two radial extrema.
    """
    positions, angles = clip["positions"], clip["rotations"]
    travel = sum(math.dist(a["value"], b["value"]) for a, b in zip(positions, positions[1:]))
    if not travel:
        return 0.0, True
    planar = (clip["localTranslation"] and len(positions) == 2 and len(angles) >= 2
              and positions[0]["time"] == angles[0]["time"] == 0
              and positions[-1]["time"] == angles[-1]["time"] > 0
              and all(k["type"] == 1 and k["value"][:2] == [0, 0] for k in positions)
              and all(k["type"] == 1 and k["value"][0] == k["value"][2] == 0 for k in angles))
    if not planar:
        return travel, False
    speed = travel / positions[-1]["time"]
    x = z = radius = 0.0
    for a, b in zip(angles, angles[1:]):
        start, stop = a["value"][1], b["value"][1]
        distance = speed * (b["time"] - a["time"])
        if stop == start:
            x += distance * sin(start)
            z += distance * cos(start)
        else:
            r = distance / (stop - start)
            cx, cz = x + r * cos(start), z - r * sin(start)
            lo, hi = sorted((start, stop))
            peak = atan2(-cz, cx)
            for n in range(math.ceil((lo - peak) / math.pi), math.floor((hi - peak) / math.pi) + 1):
                angle = peak + n * math.pi
                radius = max(radius, math.hypot(cx - r * cos(angle), cz + r * sin(angle)))
            x, z = cx - r * cos(stop), cz + r * sin(stop)
        radius = max(radius, math.hypot(x, z))
    return radius, True


def _update_motion_bounds(bounds, worlds, objects, models, collisions, polys):
    by_name = {obj["name"].lower(): obj for obj in objects}
    radius = {}
    for model, parts in models.items():
        points = (p for part in parts for p in part["positions"])
        radius[model] = max((math.hypot(*p) for p in points), default=0)
        if model in collisions:
            info = collisions[model]
            start = info["firstTriangle"] * 3
            for i in set(polys["idx"][start:start + info["triangleCount"] * 3]):
                radius[model] = max(radius[model], math.hypot(*polys["pos"][3*i:3*i+3]))
    for motion in worlds:
        children = {h["parent"].lower(): h["children"] for h in motion["hierarchies"]}
        for group in motion["groups"].values():
            for binding in group["bindings"]:
                name = binding["object"].lower()
                obj = by_name.get(name)
                clip = motion["clips"].get(binding["clip"])
                if not obj or not clip:
                    continue
                matrix = obj["matrix"]
                center = matrix[12:15]
                body = radius.get((obj.get("model") or obj["class"]).lower(), 0)
                for member in children.get(name, ()):
                    attached = by_name.get(member.lower())
                    if attached:
                        size = radius.get((attached.get("model") or attached["class"]).lower(), 0)
                        body = max(body, math.dist(center, attached["matrix"][12:15]) + size)
                travel, planar = _motion_radius(clip)
                ext = [body + travel * (math.hypot(matrix[i], matrix[8+i]) if planar else 1) for i in range(3)]
                for sign in (-1, 1):
                    _update_bounds(bounds, [v + sign * r for v, r in zip(center, ext)])


def _update_manifest_flight_bounds(bounds, manifest):
    """Expand vertical wire bounds by SetMin/Max(Player)FlyHeight calls when a
    bfe-script-tool mission manifest is supplied."""
    if not isinstance(manifest, dict):
        return
    lo, hi = bounds[0], bounds[1]
    if not math.isfinite(lo[0]):
        return
    cx = 0.5 * (lo[0] + hi[0])
    cz = 0.5 * (lo[2] + hi[2])
    for call in manifest.get("calls") or []:
        fn = call.get("function") or call.get("name") or ""
        args = call.get("args") or []
        if not args:
            continue
        if fn == "SetWorldExtents":
            radius = abs(float(args[0]))
            for sign in (-1, 1):
                _update_bounds(bounds, (sign * radius, 0, sign * radius))
        if fn in ("SetMinFlyHeight", "SetMaxFlyHeight", "SetMinPlayerFlyHeight", "SetMaxPlayerFlyHeight"):
            try:
                y = float(args[0])
            except (TypeError, ValueError):
                continue
            _update_bounds(bounds, (cx, y, cz))


def _load_sky_subarchive(map_id, sky_sub=None):
    """For space maps (spa*), read the planet's sub-archive inside spa/spa_sky.lvl."""
    if not sky_sub:
        return None, None, None
    sky_path = GAME / zero_family.profile("swbf2").planet_folder(map_id) / "spa_sky.lvl"
    sky_bytes = sky_path.read_bytes()
    # Sky packages use the planet archive directory ids. Some never appear
    # as requests in a stock setup trace, but their folders still exist.
    planet_names = [p.name for p in GAME.iterdir() if p.is_dir()]
    sections = zero_swbf2.world_sections(sky_bytes, sky_path.stem, [sky_sub, *planet_names], keep_unnamed=True)
    selected, = zero_swbf2.requested_layers(sections["layers"], [sky_sub], sky_path.stem)
    return sky_bytes, selected["range"], f"{sky_path.relative_to(GAME).as_posix()};{selected['name']}"


def _lighting_from_swbf2(sky_info, lights_list, global_lights):
    """Build the map.json `lighting` block from SkyInfo + 2005 lght chunks."""
    by_lower = {l["name"].lower(): l for l in lights_list if l.get("name")}
    l1_name = (global_lights.get("light1") or "").lower()
    l2_name = (global_lights.get("light2") or "").lower()
    sun = by_lower.get(l1_name) or next((l for l in lights_list if l.get("type") == "directional"), None)
    back = by_lower.get(l2_name) if l2_name else None

    # Prepared lighting uses 0..255 RGB. Individual lght records use 0..1.
    def rgb(light, default):
        return [round(v * 255, 4) for v in light[:3]] if light else default

    ambient = value(sky_info, "AmbientColor") or rgb(global_lights.get("top"), [128, 128, 128])
    fog_range = value(sky_info, "FogRange") or []
    return {
        "ambient": ambient,
        "top": value(sky_info, "TopDirectionalAmbientColor") or rgb(global_lights.get("top"), ambient),
        "bottom": value(sky_info, "BottomDirectionalAmbientColor") or rgb(global_lights.get("bottom"), ambient),
        "character": value(sky_info, "CharacterAmbientColor") or ambient,
        "sunDirection": sun.get("direction") if sun else [0, -1, 0],
        "sunColor": rgb(sun.get("color") if sun else None, [255, 255, 255]),
        "backDirection": back.get("direction") if back else None,
        "backColor": rgb(back.get("color"), None) if back else None,
        "fogColor": value(sky_info, "FogColor") or [0, 0, 0],
        "fogNear": float(fog_range[0]) if len(fog_range) >= 1 else 0.0,
        "fogFar": float(fog_range[1]) if len(fog_range) >= 2 else 0.0,
    }


def _collect_section_resources(archives_with_sections):
    """Collect models, far_models, skeletons, textures, coll, prim, classes across
    a list of (bytes, sections_list) pairs (first definition wins)."""
    models, far_models, skeletons, textures = {}, {}, {}, {}
    c_meshes, p_shapes = {}, {}
    for data, sections in archives_with_sections:
        collect_models(data, models, skeletons, far=far_models, sections=sections, swbf2=True)
        collect_textures(data, textures, sections=sections, decodable=SWBF2_DECODABLE)
        for k, v in coll_meshes(data, sections=sections, with_masks=True).items():
            c_meshes.setdefault(k.lower(), v)
        for k, v in prim_shapes(data, sections=sections).items():
            p_shapes.setdefault(k.lower(), v)
    classes = zero_swbf2.class_catalog(archives_with_sections)
    return models, far_models, skeletons, textures, c_meshes, p_shapes, classes


def _fix_material_textures(model_sets, textures):
    """Resolve image extensions against archive names and retain broken source references."""
    available = {name.lower(): name for name in textures}
    missing = set()
    for models in model_sets:
        for parts in models.values():
            for part in parts:
                for field in ("texture", "bump"):
                    name = part.get(field)
                    if not name or name.lower() in available:
                        continue
                    stem, ext = os.path.splitext(name)
                    match = available.get(stem.lower()) if ext.lower() == ".tga" else None
                    if match:
                        part[field] = match
                    elif field == "texture" and name.lower() == "noicon":
                        # Retail uses this unmapped TNAM on deliberately bare
                        # parts. A supplied texture of that name wins above.
                        part["material"]["sourceTexture"] = name
                        part[field] = ""
                    else:
                        part["material"]["missing" + field.title()] = name
                        part[field] = "" if field == "texture" else None
                        missing.add(name)
                material = part["material"]
                material["textureMode"] = ("image" if part["texture"] else
                                           "missing" if "missingTexture" in material else "none")
    return missing


def _material_bucket(buckets, part, dynamic, far=False):
    from extract import Bucket, material_key
    key = material_key(part, dynamic)
    material = part["material"]
    full_key = (key, json.dumps(material, sort_keys=True))
    if full_key not in buckets:
        bucket = Bucket(key)
        bucket.extra = {"material": material}
        if far:
            bucket.extra["far"] = True
        buckets[full_key] = bucket
    return buckets[full_key]


def _write_prop_attributes(models, records, skeletons, out):
    """Attach rest-pose vertex streams to the existing model-space mesh records."""
    rigs = {}
    filename = "props_attributes.bin"
    with (out / filename).open("wb") as buffer:
        def stream(offsets, name, typecode, values):
            offsets[name] = buffer.tell()
            packed = array.array(typecode, values)
            if sys.byteorder != "little":
                packed.byteswap()
            buffer.write(packed)

        for name, parts in records.items():
            skeleton = skeletons.get(name)
            bone_indices = {}
            if skeleton:
                rigs[name] = bind_bones(skeleton)
                bone_indices = {bone["name"]: i for i, bone in enumerate(rigs[name])}
            for rec, part in zip(parts, models[name], strict=True):
                rec["bindBone"] = bone_indices.get(part.get("bone"), -1)
                offsets = rec["vertexStreams"] = {}
                for source, target in (("colors", part.get("colorKind")), ("tints", "tint"),
                                       ("tangents", "tangent"), ("binormals", "binormal")):
                    if part.get(source):
                        stream(offsets, target, "f", (x for vertex in part[source] for x in vertex))
                if part.get("skinJoints"):
                    rec["influencesPerVertex"] = 3
                    stream(offsets, "joints", "I", part["skinJoints"])
                    stream(offsets, "weights", "f", part["skinWeights"])
                elif part.get("joints"):
                    rec["influencesPerVertex"] = 1
                    stream(offsets, "joints", "I", part["joints"])
    return {"skeletons": rigs, "attributesFile": filename}


def _place_collision(model, matrix, meshes, shapes, skeleton, polys):
    # Keep the archive mask per triangle for soldier, flyer and vehicle queries.
    masks = polys.setdefault("masks", bytearray())
    for positions, faces, mask in meshes.get(model, []):
        place_mesh(positions, faces, matrix, polys)
        masks.extend(bytes([mask]) * (len(polys["poly"]) - len(masks)))
    for shape in shapes.get(model, []):
        local = shape["matrix"]
        parent = shape.get("parent")
        if skeleton and parent in skeleton["names"]:
            local = mul(skeleton["model"][skeleton["names"].index(parent)], local)
        place_shape(shape["kind"], shape["size"], mul(matrix, local), polys)
        masks.extend(bytes([shape.get("mask", 0)]) * (len(polys["poly"]) - len(masks)))


def _build_geometry_and_collision(
    records,
    classes_by_lower,
    folded_models,
    folded_far_models,
    skeletons_by_lower,
    c_meshes,
    p_shapes,
    bounds,
    animated_names,
):
    """Bucket static geometry and build world-space collision for a list of
    instance records. Returns (buckets, far_buckets, polys, stats, missing)."""
    from extract import new_polys

    buckets = {}
    far_buckets = {}
    polys = new_polys()
    missing = {}
    placed_visual = 0
    placed_coll = 0

    for rec in records:
        cls = classes_by_lower.get(rec["class"].lower())
        base = ((cls or {}).get("base") or "").lower()
        if base in MODELLESS_BASES:
            _update_bounds(bounds, rec["xfrm"][9:12])
            continue
        is_live = base in LIVE_OR_SPECIAL_BASES or rec["name"].lower() in animated_names
        geom = _prop(cls, "GeometryName", rec["class"])
        matrix = mat4(rec["xfrm"])
        _update_bounds(bounds, rec["xfrm"][9:12])

        # Visuals: static geometry always baked; live objects still expand world bounds
        # so capital ships/frigates/shields are enclosed in map.json["bounds"].
        parts = folded_models.get(geom.lower()) or folded_models.get(rec["class"].lower())
        if parts and is_live:
            for part in parts:
                for p in part["positions"]:
                    _update_bounds(bounds, transform_point(matrix, p))
        if not is_live:
            if parts:
                dyn = (_prop(cls, "Lighting", "") or "").lower() == "dynamic"
                for part in parts:
                    _material_bucket(buckets, part, dyn).add(part, matrix)
                    for p in part["positions"]:
                        _update_bounds(bounds, transform_point(matrix, p))
                placed_visual += 1
            else:
                missing[rec["class"]] = missing.get(rec["class"], 0) + 1
            for part in folded_far_models.get(geom.lower()) or folded_far_models.get(rec["class"].lower()) or []:
                _material_bucket(far_buckets, part, False, far=True).add(part, matrix)
                for p in part["positions"]:
                    _update_bounds(bounds, transform_point(matrix, p))

        # Collision: static geometry goes into world collision; live objects keep their
        # own collision ranges / models so destructible systems and shields can toggle.
        if not is_live:
            coll_key = geom.lower() if (geom.lower() in c_meshes or geom.lower() in p_shapes) else rec["class"].lower()
            if coll_key in c_meshes or coll_key in p_shapes:
                skel = skeletons_by_lower.get(coll_key)
                before = len(polys["idx"])
                _place_collision(coll_key, matrix, c_meshes, p_shapes, skel, polys)
                if len(polys["idx"]) > before:
                    placed_coll += 1

    for point in struct.iter_unpack("<3f", polys["pos"]):
        _update_bounds(bounds, point)
    return buckets, far_buckets, polys, {"visual": placed_visual, "collision": placed_coll}, missing


def _instances(records):
    return [{"class": r["class"], "name": r["name"], "matrix": mat4(r["xfrm"]),
             "properties": r["props"]} for r in records]


def _summarize_placed_objects(records, classes_by_lower, locl, world_regions, animated_names):
    """Export every live/special placed instance (command posts, turrets,
    destructible systems, shields, vehicle spawns, flags, lights, doors,
    terminals) with its transform and instance properties."""
    by_region = {r["name"].lower(): r for r in world_regions}
    posts = []
    placed = []
    for rec in records:
        cls = classes_by_lower.get(rec["class"].lower())
        base = ((cls or {}).get("base") or "").lower()
        if base not in LIVE_OR_SPECIAL_BASES and rec["name"].lower() not in animated_names:
            continue
        props = dict(rec["props"])
        matrix = mat4(rec["xfrm"])
        entry = {
            "name": rec["name"],
            "class": rec["class"],
            "base": base,
            "matrix": matrix,
            "position": [round(float(v), 4) for v in rec["xfrm"][9:12]],
            "properties": [[k, v] for k, v in rec["props"]],
        }
        geom = _prop(cls, "GeometryName")
        if geom:
            entry["model"] = geom.lower()
        placed.append(entry)
        if base == "commandpost":
            cap = by_region.get((props.get("CaptureRegion") or "").lower())
            posts.append({
                "name": rec["name"],
                "class": rec["class"],
                "label": localize(locl, props.get("HUDIndexDisplay", "")) or rec["name"],
                "team": int(float(props.get("Team", 0))) if props.get("Team") else 0,
                "position": [round(float(v), 4) for v in rec["xfrm"][9:12]],
                "matrix": matrix,
                "spawnPath": props.get("SpawnPath"),
                "captureRegion": cap["name"] if cap else props.get("CaptureRegion"),
                "controlRegion": props.get("ControlRegion"),
                "killRegion": props.get("KillRegion"),
                "allyPath": props.get("AllyPath"),
                "allyCount": int(float(props.get("AllyCount", 0))) if props.get("AllyCount") else 0,
                "valueBleed": int(float(props.get("ValueBleed", 100))) if props.get("ValueBleed") else 100,
                "properties": [[k, v] for k, v in rec["props"]],
            })
    return posts, placed


def extract_map_swbf2(name, shared, sides=(), sky_sub=None, manifest=None):
    """Extract a 2005 map (base world + every named mode layer) into
    PUBLIC/maps/<name>/."""
    from extract import (
        Bucket,
        dome_models,
        low_res_terrain,
        map_archive,
        post_effects,
        save_mesh_buffers,
        side_archive,
        write_textures,
    )

    out = PUBLIC / "maps" / name
    out.mkdir(parents=True, exist_ok=True)
    archive_path = map_archive(name)
    data = archive_path.read_bytes()
    print(f"map {name} (swbf2, {archive_path.name})...", flush=True)

    assets = mission_assets(manifest, name)
    ws = zero_swbf2.world_sections(data, name, assets["layers"], name_parts=layer_name_parts(GAME / "mission.lvl"),
                                   keep_unnamed=True)
    zero_swbf2.requested_layers(ws["layers"], assets["layers"], name)
    base_sec = ws["base"]
    if base_sec is None:
        raise ValueError(f"{archive_path}: not a valid ucfb map archive")

    # Optional space skybox sub-archive (spa/spa_sky.lvl;<planet>)
    sky_bytes, sky_range, sky_source = _load_sky_subarchive(name, sky_sub=sky_sub or assets["sky"])

    # Every layer needs its placed entities, including ones that live in a
    # turret package which the opening mission itself does not request.
    animated_names = set()
    placed_types = set()
    for sec in [base_sec, *ws["layers"].values()]:
        motion = zero_swbf2.world_animations(data, sec["worlds"])
        sec["motion"] = motion
        animated_names.update(b["object"].lower() for g in motion["groups"].values() for b in g["bindings"])
        for h in motion["hierarchies"]:
            animated_names.update(n.lower() for n in [h["parent"], *h["children"]])
        for _, wb, we in sec["worlds"]:
            placed_types.update(r["class"].lower() for r in instance_records(data, wb, we))
    sources = [(data, list(iter_all_archives(data)))]
    if sky_bytes and sky_range:
        sources.append((sky_bytes, [sky_range]))
    sources.extend((d, list(iter_all_archives(d))) for d in shared)
    defined = {k.lower() for d, spans in sources for k in zero_swbf2.read_all_classes(d, spans)["entc"]}
    for side in dict.fromkeys([*sides, "tur"]):
        path = side_archive(side)
        if path.exists():
            side_data = path.read_bytes()
            requested = (placed_types - defined) | set(assets["sides"].get(side, ()))
            if requested:
                sources.append((side_data, zero_swbf2.package_sections(side_data, requested)))

    models, far_models, skeletons, textures, c_meshes, p_shapes, classes = _collect_section_resources(sources)
    _fix_material_textures((models, far_models), textures)
    classes_by_lower = {}
    for class_id in classes["entc"]:
        resolved = zero_swbf2.resolve_class("entc", class_id, classes)
        classes_by_lower[class_id] = {**resolved, "base": resolved["engineBase"]}
    folded_models = {k.lower(): v for k, v in models.items()}
    folded_far_models = {k.lower(): v for k, v in far_models.items()}
    skeletons_by_lower = {k.lower(): v for k, v in skeletons.items()}

    # Parse base terrain, water, weather, sky, plan
    terrain = water = None
    root_b, root_e = base_sec["range"]
    for t, b, e in chunks(data, root_b, root_e):
        if t == b"tern" and terrain is None:
            terrain = parse_terrain(data, b, e)
            if terrain is not None:
                water = parse_water(data, b, e, configs(data, b"fx__"))
    sky_configs = [parse_config(data, b, e) for t, b, e in chunks(data, root_b, root_e) if t == b"sky_"]
    sky_config = sky_configs[0] if sky_configs else []
    sky_info = section(sky_config, "SkyInfo")
    dome_info = section(sky_config, "DomeInfo")

    # Merge spa_sky.lvl DomeInfo when map's own DomeInfo has no DomeModel
    if sky_bytes and sky_range and not dome_models(dome_info):
        for t, b, e in chunks(sky_bytes, sky_range[0], sky_range[1]):
            if t == b"sky_":
                ext_sky = parse_config(sky_bytes, b, e)
                ext_dome = section(ext_sky, "DomeInfo")
                if ext_dome:
                    dome_info = dome_info + ext_dome
                if not sky_info:
                    sky_info = section(ext_sky, "SkyInfo")
                break

    if terrain is not None and not value(dome_info, "TerrainEnable", [1])[0]:
        terrain = None
    weather = parse_weather(configs(data, b"fx__"))
    plan_data = None
    for t, b, e in chunks(data, root_b, root_e):
        if t == b"plan":
            plan_data = zero_swbf2.parse_plan_v2(data, b, e)
            break

    # Base world records, regions, lights, paths, portals
    world_name = base_sec["worlds"][0][0] if base_sec["worlds"] else name
    base_records = []
    base_regions = []
    for _, wb, we in base_sec["worlds"]:
        base_records.extend(instance_records(data, wb, we, zero_swbf2.property_name))
        base_regions.extend(regions(data, wb, we))

    base_lights = []
    global_lights = {}
    for lrec in base_sec["lights"]:
        base_lights.extend(lrec["lights"])
        if lrec["global"]:
            global_lights.update(lrec["global"])

    bounds = _empty_bounds()
    for reg in base_regions:
        _update_region_bounds(bounds, reg)
    for pts in base_sec["paths"].values():
        for pt in pts:
            _update_bounds(bounds, pt[:3])
    if base_sec["portals"]:
        for sec in base_sec["portals"]["sectors"]:
            for px, pz in sec["points"]:
                _update_bounds(bounds, (px, sec["base"], pz))
                _update_bounds(bounds, (px, sec["base"] + sec["height"], pz))
        for ptl in base_sec["portals"]["portals"]:
            pos = ptl.get("position")
            if pos and len(pos) >= 3:
                hw = abs(float(ptl.get("width", 0.0))) * 0.5
                hh = abs(float(ptl.get("height", 0.0))) * 0.5
                _update_bounds(bounds, (pos[0] - hw, pos[1] - hh, pos[2] - hw))
                _update_bounds(bounds, (pos[0] + hw, pos[1] + hh, pos[2] + hw))
    if plan_data:
        for hub in plan_data.get("hubs") or []:
            pos = hub.get("position")
            r = abs(float(hub.get("radius", 0.0)))
            if pos and len(pos) >= 3:
                _update_bounds(bounds, (pos[0] - r, pos[1], pos[2] - r))
                _update_bounds(bounds, (pos[0] + r, pos[1], pos[2] + r))

    buckets, far_buckets, base_polys, base_stats, missing = _build_geometry_and_collision(
        base_records,
        classes_by_lower,
        folded_models,
        folded_far_models,
        skeletons_by_lower,
        c_meshes,
        p_shapes,
        bounds,
        animated_names,
    )

    extra = list(far_buckets.values())
    if terrain:
        for p in terrain["positions"]:
            _update_bounds(bounds, p)
        ground = Bucket((terrain["layers"][0], "", LIT, (), (), "lit", False))
        ground.add(
            {
                "positions": terrain["positions"],
                "normals": terrain["normals"],
                "uvs": terrain["uvs"],
                "colors": terrain["colors"],
                "indices": terrain["indices"],
            },
            IDENTITY,
        )
        ground.extra = {
            "terrain": True,
            "material": {**swbf2_surface(0), "textureMode": "image" if terrain["layers"][0] else "none"},
            "layers": terrain["layers"],
            "scales": terrain["scales"],
            "detail": terrain["detail"],
        }
        if terrain["weights"]:
            (out / "terrain_weights.bin").write_bytes(terrain["weights"])
            ground.extra["weights"] = "terrain_weights.bin"
        extra.append(ground)
        if terrain["lumi"]:
            (out / "lumi.bin").write_bytes(terrain["lumi"])
        # Terrain collision buffer
        t_pos = array.array("f")
        t_idx = array.array("I")
        for p in terrain["positions"]:
            t_pos.extend(p)
        t_idx.extend(terrain["indices"])
        (out / "terrain_collision.bin").write_bytes(t_pos.tobytes())
        (out / "terrain_collision_index.bin").write_bytes(t_idx.tobytes())

    sky_bounds = _empty_bounds()
    # Sky dome models
    dome_texture = (value(dome_info, "Texture") or [None])[0]
    for layer_idx, (dome_model, follow) in enumerate(dome_models(dome_info)):
        dome_parts = (folded_models.get(dome_model.lower()) or []) if dome_model else []
        for part in dome_parts:
            flags = part["material"]["flags"] & (HARD_EDGED | TRANSPARENT | ADDITIVE) if layer_idx else 0
            texture = part["texture"] or dome_texture or ""
            dome = Bucket((texture, "", flags, (), (), "", False))
            dome.add(part, IDENTITY)
            for point in part["positions"]:
                _update_bounds(sky_bounds, point)
            dome.extra = {
                "sky": True,
                "material": {**part["material"], "textureMode": "image" if texture else part["material"]["textureMode"],
                             **({} if layer_idx else {"blendMode": "opaque", "alphaCutout": None})},
                **({"skyLayer": layer_idx} if layer_idx else {}),
                **({"skyFollow": follow} if follow else {}),
            }
            extra.append(dome)

    verts, indices, meshes = save_mesh_buffers(buckets, extra)
    (out / "geom.bin").write_bytes(verts.tobytes())
    (out / "index.bin").write_bytes(indices.tobytes())
    (out / "collision.bin").write_bytes(base_polys["pos"].tobytes())
    (out / "collision_index.bin").write_bytes(base_polys["idx"].tobytes())
    (out / "collision_poly.bin").write_bytes(base_polys["poly"].tobytes())
    (out / "collision_mask.bin").write_bytes(base_polys.get("masks", b""))

    locl = zero_addon.localization(GAME)
    base_posts, base_placed = _summarize_placed_objects(base_records, classes_by_lower, locl, base_regions, animated_names)

    # Export all packages, including unreferenced packages whose spelling did not survive.
    all_class_names = {r["class"].lower() for r in base_records}
    layers_out = {}
    layer_textures = set()
    layers_dir = out / "layers"
    layer_index = {}
    layer_errors = []
    for lsec in sorted(ws["layers"].values(), key=lambda sec: sec["name"] or sec["hash"]):
        lname = lsec["name"] or lsec["hash"]
        layer_index[lsec["hash"]] = lname
        if lname in layers_out:
            raise ValueError(f"{name}: layer key {lname!r} collides with an unnamed package")
        if "nameError" in lsec:
            layer_errors.append(lsec["nameError"])
            print(lsec["nameError"]["message"], file=sys.stderr)
        folder = lname if re.fullmatch(r"[a-z0-9][a-z0-9_.-]*", lname) else lsec["hash"]
        l_records = []
        l_regions = []
        for _, wb, we in lsec["worlds"]:
            l_records.extend(instance_records(data, wb, we, zero_swbf2.property_name))
            l_regions.extend(regions(data, wb, we))
        all_class_names.update(r["class"].lower() for r in l_records)
        l_lights = []
        for lrec in lsec["lights"]:
            l_lights.extend(lrec["lights"])
        for reg in l_regions:
            _update_region_bounds(bounds, reg)
        for pts in lsec["paths"].values():
            for pt in pts:
                _update_bounds(bounds, pt[:3])

        lbuckets, lfar, lpolys, lstats, lmiss = _build_geometry_and_collision(
            l_records,
            classes_by_lower,
            folded_models,
            folded_far_models,
            skeletons_by_lower,
            c_meshes,
            p_shapes,
            bounds,
            animated_names,
        )
        l_verts, l_indices, l_meshes = save_mesh_buffers(lbuckets, list(lfar.values()))
        l_geom_file = l_idx_file = l_coll_file = None
        if l_meshes:
            ldir = layers_dir / folder
            ldir.mkdir(parents=True, exist_ok=True)
            (ldir / "geom.bin").write_bytes(l_verts.tobytes())
            (ldir / "index.bin").write_bytes(l_indices.tobytes())
            l_geom_file = f"layers/{folder}/geom.bin"
            l_idx_file = f"layers/{folder}/index.bin"
            for m in l_meshes:
                if m.get("texture"):
                    layer_textures.add(m["texture"])
                if m.get("bump"):
                    layer_textures.add(m["bump"])
        if lpolys["idx"]:
            ldir = layers_dir / folder
            ldir.mkdir(parents=True, exist_ok=True)
            (ldir / "collision.bin").write_bytes(lpolys["pos"].tobytes())
            (ldir / "collision_index.bin").write_bytes(lpolys["idx"].tobytes())
            (ldir / "collision_poly.bin").write_bytes(lpolys["poly"].tobytes())
            (ldir / "collision_mask.bin").write_bytes(lpolys.get("masks", b""))
            l_coll_file = f"layers/{folder}/collision.bin"

        l_posts, l_placed = _summarize_placed_objects(l_records, classes_by_lower, locl, base_regions + l_regions, animated_names)
        # Resolve spawn paths referenced by layer command posts (against base + layer paths)
        combined_paths = {**base_sec["paths"], **lsec["paths"]}
        path_lookup = {k.lower(): v for k, v in combined_paths.items()}
        l_spawns = {}
        for post in l_posts:
            sp = post.get("spawnPath")
            if sp and sp.lower() in path_lookup:
                l_spawns[sp] = path_lookup[sp.lower()]

        layers_out[lname] = {
            "hash": lsec["hash"],
            "name": lsec["name"],
            "worlds": [w[0] for w in lsec["worlds"]],
            "worldAnimations": lsec["motion"],
            "instanceCount": len(l_records),
            "instances": _instances(l_records),
            "meshes": l_meshes,
            "geom": l_geom_file,
            "index": l_idx_file,
            "collision": l_coll_file,
            "collisionTriangles": len(lpolys["idx"]) // 3,
            "commandPosts": l_posts,
            "spawns": l_spawns,
            "paths": lsec["pathsDetail"],
            "regions": l_regions,
            **world_navigation_records(data, lsec["worlds"]),
            "lights": l_lights,
            "placedObjects": l_placed,
        }
        if lmiss:
            layers_out[lname]["missingModels"] = lmiss

    # Collect textures used by base + layer meshes, live objects, terrain, sky, weather
    wanted_tex = {m["texture"] for m in meshes if m.get("texture")}
    wanted_tex |= {m["bump"] for m in meshes if m.get("bump")}
    wanted_tex |= layer_textures
    if dome_texture:
        wanted_tex.add(dome_texture)
    for m in meshes:
        wanted_tex |= set(m.get("layers") or [])
        if m.get("detail"):
            wanted_tex.add(m["detail"])
    low_res = (value(section(dome_info, "LowResTerrain"), "Texture") or [None])[0]
    if low_res:
        wanted_tex.add(low_res)
    if water:
        wanted_tex |= water_textures(water)
    wanted_tex |= weather_textures(weather)

    # Also collect live placed object models (turrets, destructible systems, shields, doors, props)
    # into props.json + props.bin / props_index.bin
    live_models = {}
    all_placed = base_placed + [p for l in layers_out.values() for p in l["placedObjects"]]
    # Variant and detachable meshes are already placed relative to the live object's model.
    variant_keys = {"DestroyedGeometryName", "UnbuiltGeometryName", "ChunkGeometryName",
                    "CarriedGeometryName", "DroppedInLandingZoneGeometryName", "HoloImageGeometry"}
    live_model_names = set()
    for obj in all_placed:
        cls = classes_by_lower.get(obj["class"].lower(), {})
        model = (obj.get("model") or obj["class"]).lower()
        if model in folded_models:
            live_model_names.add(model)
        for key, reference in cls.get("props", ()):
            if key not in variant_keys or not reference.split():
                continue
            variant = reference.split()[0].lower()
            if variant in folded_models:
                live_model_names.add(variant)
                if key in ("DestroyedGeometryName", "UnbuiltGeometryName"):
                    for part in folded_models[variant]:
                        for point in part["positions"]:
                            _update_bounds(bounds, transform_point(obj["matrix"], point))
    live_model_names = sorted(live_model_names)
    for mname in live_model_names:
        parts = folded_models[mname]
        live_models[mname] = parts
        for part in parts:
            if part.get("texture"):
                wanted_tex.add(part["texture"])
            if part.get("bump"):
                wanted_tex.add(part["bump"])

    from extract import write_meshes
    prop_mesh_records = write_meshes(live_models, live_model_names, out, "props", swbf2=True) if live_model_names else {}
    prop_attributes = _write_prop_attributes(live_models, prop_mesh_records, skeletons_by_lower, out)
    # Also pack live object collision meshes/shapes so destructible systems & shields have collision
    prop_coll = {}
    from extract import new_polys
    prop_polys = new_polys()
    for mname in live_model_names:
        if mname in c_meshes or mname in p_shapes:
            start_tri = len(prop_polys["idx"]) // 3
            _place_collision(mname, IDENTITY, c_meshes, p_shapes, skeletons_by_lower.get(mname), prop_polys)
            end_tri = len(prop_polys["idx"]) // 3
            if end_tri > start_tri:
                prop_coll[mname] = {"firstTriangle": start_tri, "triangleCount": end_tri - start_tri}
    for obj in all_placed:
        model = (obj.get("model") or obj["class"]).lower()
        info = prop_coll.get(model)
        if info:
            first = info["firstTriangle"] * 3
            for vertex in set(prop_polys["idx"][first:first + info["triangleCount"] * 3]):
                p = prop_polys["pos"][3 * vertex:3 * vertex + 3]
                _update_bounds(bounds, transform_point(obj["matrix"], p))
    if prop_polys["idx"]:
        (out / "props_collision.bin").write_bytes(prop_polys["pos"].tobytes())
        (out / "props_collision_index.bin").write_bytes(prop_polys["idx"].tobytes())
        (out / "props_collision_poly.bin").write_bytes(prop_polys["poly"].tobytes())
        (out / "props_collision_mask.bin").write_bytes(prop_polys.get("masks", b""))

    # Full ODF class definitions for every class placed on the map (base + all layers)
    map_classes_out = {}
    placed_class_names = sorted(all_class_names)
    for cl in placed_class_names:
        cls = classes_by_lower.get(cl)
        if cls:
            map_classes_out[cl] = {
                "base": cls.get("base"),
                "parents": cls["parents"],
                "properties": [[k, v] for k, v in cls.get("props", ())],
            }

    write_text_lf(out / "props.json", json.dumps({
        "family": "swbf2",
        "models": prop_mesh_records,
        **prop_attributes,
        "collision": prop_coll,
        "instances": base_placed,
    }))

    sky_texture = next((m["texture"] for m in meshes if m.get("sky") and m.get("texture")), None)
    minimap_texture = f"{world_name}_map"
    wanted_tex.add(minimap_texture)
    texture_paths = write_textures(textures, wanted_tex, out / "textures")

    _update_motion_bounds(bounds, [sec["motion"] for sec in [base_sec, *ws["layers"].values()]],
                          all_placed, live_models, prop_coll, prop_polys)
    _update_manifest_flight_bounds(bounds, manifest)
    world_bounds = _finalize_bounds(bounds)
    if math.isfinite(sky_bounds[0][0]):
        for point in sky_bounds:
            _update_bounds(bounds, point)
    path_names = {fnv1a(n): n for n in base_sec["paths"]}
    boundary = child(data, root_b, root_e, b"bnd_")
    boundary_ids = zero_swbf2.boundary_paths(data, *boundary) if boundary else []
    manifest_out = {
        "family": "swbf2",
        "name": name,
        "world": world_name,
        "worldAnimations": base_sec["motion"],
        "stride": 32,
        "bounds": world_bounds,
        "wireBounds": world_bounds,
        "renderBounds": _finalize_bounds(bounds),
        "boundaryPaths": [path_names.get(h, f"{h:08x}") for h in boundary_ids],
        "meshes": meshes,
        "textures": texture_paths,
        "sky": texture_paths.get(sky_texture),
        "skySource": sky_source,
        "spawns": base_sec["paths"],
        "paths": base_sec["pathsDetail"],
        "regions": base_regions,
        **world_navigation_records(data, base_sec["worlds"]),
        "deathRegions": [r for r in base_regions if "death" in r["name"].lower()],
        "commandPosts": base_posts,
        "placedObjects": base_placed,
        "instances": _instances(base_records),
        "worlds": [w[0] for w in base_sec["worlds"]],
        "lights": base_lights,
        "globalLights": global_lights,
        "lighting": _lighting_from_swbf2(sky_info, base_lights, global_lights),
        "distance": {
            "nearScene": value(sky_info, "NearSceneRange"),
            "farScene": value(sky_info, "FarSceneRange"),
            "lowRes": low_res_terrain(section(dome_info, "LowResTerrain"), texture_paths),
        },
        "post": post_effects(configs(data, b"fx__")),
        "water": water,
        "weather": weather,
        "layers": layers_out,
        "layerIndex": layer_index,
        "layerErrors": layer_errors,
        "classes": map_classes_out,
        "missions": {},
    }
    if manifest and manifest.get("script"):
        chain = script_chain(name, manifest, script_index((GAME / "mission.lvl").read_bytes()))
        chain.write(PUBLIC)
        for record in chain.manifest["scripts"]:
            if "error" in record:
                print(f"{manifest['script']}: {record['error']}", flush=True)
        manifest_out["missions"][manifest["script"]] = {**assets, **chain.record()}
    if base_sec["portals"] is not None:
        manifest_out["portals"] = base_sec["portals"]
    if plan_data is not None:
        manifest_out["plan"] = plan_data
    supplement = child(data, root_b, root_e, b"plnp")
    if supplement:
        manifest_out["planningGroups"], packed = zero_swbf2.parse_plnp(data, *supplement)
        (out / "planning.bin").write_bytes(packed)
    if terrain and terrain["lumi"] and value(dome_info, "TerrainColorDarkening") is not None:
        manifest_out["lumi"] = {"file": "lumi.bin", **terrain["grid"]}
    unresolved = sorted(all_class_names - set(map_classes_out))
    if unresolved:
        manifest_out["missingClasses"] = unresolved
    if missing:
        manifest_out["missingModels"] = missing

    zero_swbf2.write_animation_banks([(data, iter_all_archives(data))], out)
    vehicles = vehicle_pack(classes_by_lower, classes, skeletons_by_lower, p_shapes, c_meshes, sources)
    manifest_out.update(write_pack(out, vehicles))
    manifest_out["unresolvedParents"] = _write_classes(out, classes)
    write_map_art(out / "hud", classes, textures, GAME, sources,
                  loose=loose_textures(textures, (models, far_models), wanted_tex))
    write_if_changed(out / "map.json", json.dumps(manifest_out).encode("utf-8"))
    print(
        f"  swbf2 map {name}: base meshes={len(meshes)} tris={len(indices)//3} "
        f"coll_tris={len(base_polys['idx'])//3} lights={len(base_lights)} "
        f"paths={len(base_sec['paths'])} layers={sorted(layers_out.keys())} "
        f"wireBounds={world_bounds['min']}..{world_bounds['max']}",
        flush=True,
    )
    return set()


# ---------------------------------------------------------------- sides and classes


def _write_classes(out, classes):
    """Own ordered properties, inherited vehicle sections and explicit parent errors."""
    table = {tag: {name: {"name": cls["name"], "base": cls["base"], "properties": cls["props"]}
                   for name, cls in bucket.items()} for tag, bucket in classes.items()}
    problems = []
    for tag, bucket in classes.items():
        for name in sorted(bucket):
            resolved = zero_swbf2.resolve_class(tag, name, classes)
            if tag == "entc":
                sections = vehicle_sections(resolved, classes)
                if sections is not None:
                    table[tag][name]["vehicleSections"] = sections
            problem = resolved.get("parentProblem")
            if problem:
                problems.append({"kind": tag, "class": name, **problem})
    write_text_lf(out / "classes.json", json.dumps(table))
    return problems


def _weapon_chain_swbf2(weapon_name, all_classes, unresolved):
    """Build a weapon record keeping EVERY ODF key/value from wpnc -> ordc -> expc,
    with 2005 ODF parent inheritance resolved."""
    if not weapon_name:
        return None
    wcls = zero_swbf2.resolve_class("wpnc", weapon_name, all_classes)
    if wcls is None:
        return None
    wprops = _prop_dict(wcls)
    rec = {
        "class": wcls["name"],
        "base": wcls["engineBase"],
        "parents": wcls.get("parents") or [],
        "properties": [[k, v] for k, v in wcls.get("props", ())],
        **{k: v for k, v in wprops.items() if k not in ("class", "base", "parents", "properties")},
    }
    for ord_key, field in (("OrdnanceName", "ordnance"), ("SecondaryOrdnanceName", "secondaryOrdnance")):
        oname = wprops.get(ord_key)
        if not oname:
            continue
        ocls = zero_swbf2.resolve_class("ordc", oname, all_classes) or zero_swbf2.resolve_class("entc", oname, all_classes)
        if ocls is None:
            unresolved.add((wcls["name"], wcls["name"], ord_key, oname))
            continue
        oprops = _prop_dict(ocls)
        orec = {
            "class": ocls["name"],
            "base": ocls["engineBase"],
            "parents": ocls.get("parents") or [],
            "properties": [[k, v] for k, v in ocls.get("props", ())],
            **{k: v for k, v in oprops.items() if k not in ("class", "base", "parents", "properties")},
        }
        for ekey in ("Explosion", "ExplosionName", "ExplosionExpire", "ExplosionImpact", "ExplosionTrigger", "ExplosionDeath"):
            ename = oprops.get(ekey)
            if ename:
                ecls = zero_swbf2.resolve_class("expc", ename, all_classes)
                if ecls is None:
                    unresolved.add((wcls["name"], ocls["name"], ekey, ename))
                else:
                    eprops = _prop_dict(ecls)
                    orec[ekey] = {
                        "class": ecls["name"],
                        "base": ecls["engineBase"],
                        "parents": ecls.get("parents") or [],
                        "properties": [[k, v] for k, v in ecls.get("props", ())],
                        **{k: v for k, v in eprops.items() if k not in ("class", "base", "parents", "properties")},
                    }
        rec[field] = orec
    return rec


def _combo_actions(machine):
    for group in ("animations", "offhandAnimations", "throwAnimations"):
        for record in machine[group]:
            yield None, group, record

    def walk(nodes):
        for node in nodes:
            if node["key"] in ("Animation", "DeflectAnimation", "OffhandAnimation", "ThrowAnimation"):
                yield node
            yield from walk(node.get("children", ()))

    for state in machine["states"]:
        for record in walk(state.get("children", ())):
            yield state["name"], record["key"], record


def _combo_bindings(weapon, combos, available_banks, fps, animation_name=None):
    bindings = []
    clips = {bank: {clip["crc"]: clip for clip in record["clips"]} for bank, record in available_banks.items()}
    for key, value in weapon["properties"]:
        if key != "ComboAnimationBank":
            continue
        words = value.split()
        if len(words) != 3:
            continue
        prefix, mode, machine = words
        combo = f"{fnv1a(machine):08x}"
        binding = {"bank": prefix, "mode": mode, "machine": machine, "combo": combo, "clips": {},
                   "clipTiming": {}, "stateAnimations": {}, "unresolvedAnimations": []}
        bindings.append(binding)
        if combo not in combos:
            binding["missingMachine"] = True
            continue
        suffix = prefix.rsplit("_", 1)[-1]
        # Stable within each tier: the hero package's authored bank order,
        # with its selected AnimationName first and the common prefix last.
        priority = sorted(clips, key=lambda b: (b.lower() != (animation_name or "").lower(), b.lower() == prefix.lower()))
        rank = {bank: i for i, bank in enumerate(priority)}
        for state, role, record in _combo_actions(combos[combo]):
            values = record.get("values", ())
            if not values or not isinstance(values[0], str):
                raise ValueError(f"{machine} {state or role}: animation has no clip name")
            action = values[0]
            if action not in binding["clips"]:
                stem = action.removesuffix("_upper").removesuffix("_lower")
                matches = []
                for bank, ids in clips.items():
                    namespace = bank if bank.endswith("_" + suffix) else f"{bank}_{suffix}"
                    for variant in dict.fromkeys((action, action + "_full", stem + "_full")):
                        clip = f"{crc(namespace + '_' + variant):08x}"
                        if clip in ids:
                            matches.append({"bank": bank, "crc": clip})
                binding["clips"][action] = matches
                usable = [m for m in matches if clips[m["bank"]][m["crc"]]["frames"] > 0]
                timing = None
                if usable:
                    chosen = min(usable, key=lambda m: rank[m["bank"]])
                    timing = {**chosen, "frames": clips[chosen["bank"]][chosen["crc"]]["frames"], "fps": fps}
                binding["clipTiming"][action] = timing
            if state is not None:
                playback = "once"
                for child in record.get("children", ()):
                    if child["key"] == "Loop":
                        mode_args = child.get("values", [])
                        if mode_args and mode_args != ["FinalFrame"]:
                            raise ValueError(f"{machine} {state} {action}: unknown Loop arguments {mode_args}")
                        playback = "holdFinalFrame" if mode_args else "loop"
                binding["stateAnimations"].setdefault(state, []).append({"name": action, "key": role, "playback": playback})
            if binding["clipTiming"][action] is None:
                binding["unresolvedAnimations"].append({"machine": machine, "state": state, "name": action})
    return bindings


def _loadout_swbf2(props):
    """Extract ordered weapon slots from a 2005 unit/vehicle/turret class's props."""
    slots = []
    # Numbered WeaponName / WeaponName1..N (soldiers & heroes)
    numbered = {}
    sections = []
    cur_sec = None
    for k, v in props:
        if k == "WeaponSection":
            cur_sec = {"section": v, "weapons": []}
            sections.append(cur_sec)
        elif k.startswith("WeaponName"):
            suffix = k[len("WeaponName"):]
            if cur_sec is not None and not suffix:
                cur_sec["weapons"].append({"weapon": v})
            else:
                idx = int(suffix) if suffix.isdigit() else 0
                numbered.setdefault(idx, {})["weapon"] = v
        elif k.startswith("WeaponAmmo"):
            suffix = k[len("WeaponAmmo"):]
            if cur_sec is not None and not suffix and cur_sec["weapons"]:
                cur_sec["weapons"][-1]["ammo"] = v
            else:
                idx = int(suffix) if suffix.isdigit() else 0
                numbered.setdefault(idx, {})["ammo"] = v
        elif k.startswith("WeaponChannel"):
            suffix = k[len("WeaponChannel"):]
            if cur_sec is not None and not suffix and cur_sec["weapons"]:
                cur_sec["weapons"][-1]["channel"] = v
            else:
                idx = int(suffix) if suffix.isdigit() else 0
                numbered.setdefault(idx, {})["channel"] = v
    for idx in sorted(numbered):
        if numbered[idx].get("weapon"):
            slots.append({
                "slot": idx,
                "weapon": numbered[idx]["weapon"],
                "ammo": int(float(numbered[idx].get("ammo", 0))) if numbered[idx].get("ammo") else 0,
                "channel": int(float(numbered[idx].get("channel", 0))) if numbered[idx].get("channel") else 0,
            })
    for sidx, sec in enumerate(sections):
        for w in sec["weapons"]:
            if w.get("weapon"):
                slots.append({
                    "slot": len(slots) + 1,
                    "weaponSection": sec["section"],
                    "weapon": w["weapon"],
                    "ammo": int(float(w.get("ammo", 0))) if w.get("ammo") else 0,
                    "channel": int(float(w.get("channel", sidx))) if w.get("channel") else sidx,
                })
    return slots


def classes_from_manifest(manifest, side=None):
    """Extract class names requested by a bfe-script-tool manifest JSON.
    Reads ReadDataFile("SIDE\\<side>.lvl", cls1, cls2, ...), AddUnitClass,
    SetHeroClass, and SetupTeams calls."""
    wanted = []
    calls = manifest.get("calls") or []
    for call in calls:
        fn = call.get("function") or call.get("name") or ""
        args = call.get("args") or []
        if fn == "ReadDataFile" and args and isinstance(args[0], str):
            path = args[0].split(";", 1)[0].replace("/", "\\").lower()
            if path.startswith("side\\") and path.endswith(".lvl"):
                arc_side = Path(path.replace("\\", "/")).stem.lower()
                if side is None or arc_side == side.lower():
                    for a in args[1:]:
                        if isinstance(a, str) and a not in wanted:
                            wanted.append(a)
        elif fn in ("AddUnitClass", "SetHeroClass") and len(args) >= 2 and isinstance(args[1], str):
            cls = args[1]
            if (side is None or cls.lower().startswith(side.lower() + "_")) and cls not in wanted:
                wanted.append(cls)
    return wanted


def extract_side_swbf2(side, units, shared, manifest=None):
    """Extract 2005 side classes (soldiers, heroes, starfighters, turrets,
    ship systems), models, skeletons, weapons, cloth, combos, and animation
    banks into PUBLIC/sides/<side>/."""
    from extract import side_archive, weapon_fire_points, write_actor, write_meshes, write_textures
    from zero_spawn_screen import unit_name, weapon_name

    out = PUBLIC / "sides" / side
    out.mkdir(parents=True, exist_ok=True)
    archive_path = side_archive(side)
    data = archive_path.read_bytes()
    print(f"side {side} (swbf2, {archive_path.name})...", flush=True)

    selected = list(dict.fromkeys(units or classes_from_manifest(manifest or {}, side)))
    side_sections = zero_swbf2.package_sections(data, selected)
    sources = [(data, side_sections)]
    for sdata in shared:
        sources.append((sdata, list(iter_all_archives(sdata))))
    models, _, skeletons, textures, _, _, all_classes = _collect_section_resources(sources)

    # Classes defined specifically in this side archive (for default selection)
    own_classes = zero_swbf2.read_all_classes(data, side_sections)
    _fix_material_textures((models,), textures)

    # Determine which classes to export (using resolved engineBase!)
    if not selected:
        for cname in sorted(own_classes["entc"]):
            if "_default" in cname.lower():
                continue
            resolved = zero_swbf2.resolve_class("entc", cname, all_classes)
            ebase = ((resolved or {}).get("engineBase") or "").lower()
            if ebase in PLAYABLE_SIDE_BASES:
                selected.append(cname)

    # Parse cloth, combos, animation banks (zaa_ / zaf_) in this side archive
    cloth_by_model = {}
    combos = {}
    animations = zero_swbf2.write_animation_banks([(data, side_sections)], out)
    anim_banks = animations["banks"]
    for sb, se in side_sections:
        for t, b, e in chunks(data, sb, se):
            if t == b"CLTH":
                parsed_c = zero_swbf2.parse_cloth(data, b, e)
                if parsed_c:
                    cloth_by_model.setdefault(parsed_c["model"].lower(), []).append(parsed_c)
            elif t == b"comb":
                parsed_cb = zero_swbf2.parse_combo(data, b, e)
                if parsed_cb and parsed_cb.get("nameHash"):
                    combos.setdefault(parsed_cb["nameHash"], parsed_cb)

    locl = zero_addon.localization(GAME)
    folded_models = {k.lower(): v for k, v in models.items()}
    folded_skeletons = {k.lower(): v for k, v in skeletons.items()}
    folded_bounds = {k.lower(): v for k, v in MODEL_BOUNDS.items()}
    folded_textures = {k.lower(): v for k, v in textures.items()}
    animation_rigs = {k.lower(): v for k, v in animations["skeletons"].items()}

    defs = {}
    carried = []
    used_textures = set()
    non_actor_models = set()
    first_skeleton = None

    missing_classes = []
    unresolved = set()
    for req_name in selected:
        cls = zero_swbf2.resolve_class("entc", req_name, all_classes)
        if cls is None:
            if req_name.lower() in folded_models:
                # ReadDataFile can request a model-only package for a sky object.
                non_actor_models.add(req_name.lower())
            else:
                missing_classes.append(req_name)
            continue
        cname = cls["name"]
        props = _prop_dict(cls)
        base = cls["engineBase"].lower() if cls["engineBase"] else None
        model_name = (props.get("GeometryName") or "").lower()
        if model_name and (model_name in (".", "..") or "/" in model_name or "\\" in model_name):
            raise ValueError(f"Invalid model identifier {model_name!r}")
        skel = folded_skeletons.get(model_name)
        actor = None
        if base in ("soldier", "walkerdroid") and model_name in folded_models and skel:
            actor = write_actor(folded_models, folded_skeletons, model_name, out / "actors", swbf2=True)
            if actor:
                used_textures.update(p[k] for p in actor["parts"] for k in ("texture", "bump") if p.get(k))
                if first_skeleton is None:
                    first_skeleton = skel
        elif model_name in folded_models:
            non_actor_models.add(model_name)

        for k in ("OverrideTexture", "OverrideTexture2", "HealthTexture"):
            if props.get(k):
                used_textures.add(props[k])

        package = animations["packages"].get(f"{fnv1a(req_name):08x}", {})
        character_banks = {n: anim_banks[n] for n in package.get("banks", [])}
        weapons = []
        for slot in _loadout_swbf2(cls.get("props", ())):
            chain = _weapon_chain_swbf2(slot["weapon"], all_classes, unresolved)
            if chain:
                w = {**slot, **chain}
                w["displayName"] = weapon_name(locl, w["class"])
                hilts = saber_hilts(w, skel, animation_rigs.get(props.get("SkeletonName", "").lower()),
                                    folded_models, folded_skeletons, folded_textures)
                if hilts:
                    w["saberHilts"] = hilts
                bindings = _combo_bindings(w, combos, character_banks, animations["fps"], props.get("AnimationName"))
                if bindings:
                    w["comboBindings"] = bindings
                weapons.append(w)
                carried.append(w)

        unit_cloth = cloth_by_model.get(model_name.lower(), []) if model_name else []
        cloth_odf = props.get("ClothODF")
        if cloth_odf and cloth_odf.lower() in all_classes["entc"]:
            ccls = zero_swbf2.resolve_class("entc", cloth_odf, all_classes)
            cgeom = _prop(ccls, "attachedmesh") or _prop(ccls, "GeometryName")
            if cgeom and cgeom.lower() in cloth_by_model:
                for cc in cloth_by_model[cgeom.lower()]:
                    if cc not in unit_cloth:
                        unit_cloth.append(cc)
        for c in unit_cloth:
            if c.get("texture"):
                used_textures.add(c["texture"])

        combo_ref = next((binding["combo"] for weapon in weapons for binding in weapon.get("comboBindings", [])
                          if binding["combo"] in combos), None)

        unit_def = {
            "class": cname,
            "base": base,
            "parents": cls.get("parents") or [],
            "displayName": unit_name(locl, cname),
            "model": model_name or None,
            "actor": f"actors/{model_name}.json" if actor else None,
            "weapons": weapons,
            "properties": [[k, v] for k, v in cls.get("props", ())],
        }
        sections = vehicle_sections(cls, all_classes)
        if sections is not None:
            unit_def["vehicleSections"] = sections
        model_bounds = folded_bounds.get(model_name, {})
        for field, part in (("bounds", "box"), ("sphere", "sphere")):
            if model_bounds.get(part):
                unit_def[field] = model_bounds[part]
        if unit_cloth:
            unit_def["cloth"] = [c["name"] for c in unit_cloth]
        if combo_ref:
            unit_def["combo"] = combo_ref
        explosion_name = props.get("ExplosionName") or props.get("Explosion")
        if explosion_name:
            ecls = zero_swbf2.resolve_class("expc", explosion_name, all_classes)
            if ecls is not None:
                unit_def["explosion"] = {
                    "class": ecls["name"],
                    "base": ecls["engineBase"],
                    "parents": ecls.get("parents") or [],
                    "properties": [[k, v] for k, v in ecls.get("props", ())],
                }
        defs[cname] = unit_def

    # Pack held weapon, flying ordnance, and vehicle/starfighter/turret models
    weapon_model_names = set()
    for w in carried:
        for hilt in w.get("saberHilts", ()):
            weapon_model_names.add(hilt["model"])
            used_textures.update(blade["texture"] for blade in hilt["blades"])
        for record in (w, w.get("ordnance") or {}, w.get("secondaryOrdnance") or {}):
            for k in ("GeometryName", "HighResGeometry", "OffhandGeometryName"):
                if record.get(k):
                    record[k] = record[k].lower()
                    if record[k] in folded_models:
                        weapon_model_names.add(record[k])
        for tk in ("IconTexture", "ReticuleTexture", "ScopeTexture", "Texture", "LightSaberTexture"):
            if w.get(tk):
                used_textures.add(w[tk])

    all_pack_models = sorted(weapon_model_names | non_actor_models)
    meshes = write_meshes(folded_models, all_pack_models, out, "weapons", swbf2=True) if all_pack_models else {}
    for parts in meshes.values():
        for part in parts:
            used_textures.update(part[k] for k in ("texture", "bump") if part.get(k))
    points = weapon_fire_points(carried, folded_skeletons, folded_models)

    # Each starfighter's and gunship's model again as a rig: its parts in the
    # space of the bone each hangs from, with its bones, so the craft's takeoff
    # clip (wings, landing gear, the vulture droid's legs) can pose it.
    # Presentation only: craftrigs.json and its buffers, beside weapons.json.
    craft_models = sorted({d["model"].lower() for d in defs.values()
                           if (d.get("base") or "").lower() in ("flyer", "commandflyer")
                           and d.get("model") and folded_models.get(d["model"].lower())
                           and folded_skeletons.get(d["model"].lower())})
    if craft_models:
        from extract import rig_parts
        from zero_model import bind_bones
        rigged = {m: rig_parts(folded_models[m], folded_skeletons[m]) for m in craft_models}
        rig_records = write_meshes(rigged, craft_models, out, "craftrigs", local=True, swbf2=True)
        for parts in rig_records.values():
            for part in parts:
                used_textures.update(part[k] for k in ("texture", "bump") if part.get(k))
        write_text_lf(out / "craftrigs.json", json.dumps({"models": {
            m: {"bones": bind_bones(folded_skeletons[m]), "parts": rig_records.get(m, [])} for m in craft_models}}))

    tex_paths = write_textures(textures, used_textures, out / "textures")
    parent_problems = _write_classes(out, all_classes)
    write_text_lf(out / "units.json", json.dumps({
        "family": "swbf2",
        "side": side,
        "requested": selected,
        "missingClasses": missing_classes,
        "unresolvedParents": parent_problems,
        "unresolvedReferences": [{"weapon": weapon, "class": cls, "property": prop, "name": name}
                                 for weapon, cls, prop, name in sorted(unresolved)],
        "units": defs,
        "textures": tex_paths,
        "skeleton": first_skeleton["names"] if first_skeleton else [],
        "animBanks": sorted(anim_banks.keys()),
    }))
    write_text_lf(out / "weapons.json", json.dumps({"models": meshes, "firePoints": points}))
    for weapon, cls, prop, name in sorted(unresolved):
        print(f"  {side}: weapon {weapon}, {cls}.{prop} references missing class {name}", flush=True)
    if cloth_by_model:
        write_text_lf(out / "cloth.json", json.dumps(cloth_by_model))
    if combos:
        write_text_lf(out / "combos.json", json.dumps(combos))
    print(
        f"  swbf2 side {side}: classes={len(defs)} weapons={len(carried)} "
        f"models={len(meshes)} textures={len(tex_paths)} cloth={sum(len(v) for v in cloth_by_model.values())} "
        f"combos={len(combos)} anim_banks={len(anim_banks)}",
        flush=True,
    )
    return first_skeleton, set(), None


def extract_common_swbf2(shared):
    """Extract 2005 shared data (common.lvl, core.lvl, ingame.lvl): fonts,
    localization strings, shared cloth, animation bank index, and textures."""
    from extract import write_textures
    from zero_language import text as text_language
    from zero_locl import localization

    out = PUBLIC / "common"
    out.mkdir(parents=True, exist_ok=True)
    core_path = GAME / "core.lvl"
    core_data = core_path.read_bytes() if core_path.exists() else b""
    fonts, pages = collect_fonts(core_data) if core_data else ({}, {})
    textures = dict(pages)
    wanted = set(pages.keys())

    animations = zero_swbf2.write_animation_banks([(d, iter_all_archives(d)) for d in shared], out)
    anim_banks = {name: {"clipCount": bank["clipCount"]} for name, bank in animations["banks"].items()}
    anim_skeletons = animations["skeletons"]
    cloth_by_model = {}
    for sdata in shared:
        sections = list(iter_all_archives(sdata))
        collect_textures(sdata, textures, sections=sections, decodable=SWBF2_DECODABLE)
        for sb, se in sections:
            for t, b, e in chunks(sdata, sb, se):
                if t == b"CLTH":
                    parsed_c = zero_swbf2.parse_cloth(sdata, b, e)
                    if parsed_c:
                        wanted.add(parsed_c["texture"])
                        cloth_by_model.setdefault(parsed_c["model"].lower(), []).append(parsed_c)

    tex_paths = write_textures(textures, wanted, out / "textures")
    language = text_language(core_data, GAME, family="swbf2")
    write_text_lf(out / "common.json", json.dumps({
        "family": "swbf2",
        "fonts": fonts,
        "language": language,
        "strings": {f"{h:08x}": v for h, v in localization(core_data, language).items()},
        "textures": tex_paths,
        "animBanks": anim_banks,
        "animSkeletons": sorted(anim_skeletons.keys()),
    }))
    if cloth_by_model:
        write_text_lf(out / "cloth.json", json.dumps(cloth_by_model))
    print(f"  swbf2 common: fonts={len(fonts)} anim_banks={len(anim_banks)} zaf={len(anim_skeletons)} cloth={sum(len(v) for v in cloth_by_model.values())}", flush=True)


def run_extraction(args):
    from extract import shared_archives, side_archive
    from ktx2 import encode_optional
    from zero_paths import shared_lock

    only = "map" if args.skip_shared else args.only
    manifest = zero_family.preparation_manifest(json.loads(args.manifest.read_text(encoding="utf-8"))) if args.manifest else None
    if manifest is None and only not in ("side", "common") and args.map in OPENING_MISSIONS:
        if zero_family.script_tool() or not args.side:
            manifest = opening_manifest(args.map)
    assets = mission_assets(manifest, args.map)
    sides = args.side or list(assets["sides"])
    if only == "audio":
        write_mission_audio(args.map, manifest, sides)
        return 0

    shared = shared_archives()
    if only in (None, "side"):
        for side in sides or (["rep"] if only == "side" else []):
            if not side_archive(side).is_file():
                raise FileNotFoundError(side_archive(side))
            with shared_lock(f"side-{side}"):
                requested = args.unit or classes_from_manifest(manifest or {}, side)
                previous = PUBLIC / "sides" / side / "units.json"
                if only is None and requested and previous.exists():
                    requested = list(dict.fromkeys([*json.loads(previous.read_text()).get("requested", []), *requested]))
                extract_side_swbf2(side, requested, shared, manifest=manifest)
                encode_optional([PUBLIC / "sides" / side])
    if only in (None, "map"):
        extract_map_swbf2(args.map, shared, sides, sky_sub=args.sky_sub, manifest=manifest)
        encode_optional([PUBLIC / "maps" / args.map])
        if assets["sound"]:
            write_mission_audio(args.map, manifest, sides)
    if only in (None, "common"):
        with shared_lock("common"):
            extract_common_swbf2(shared)
            encode_optional([PUBLIC / "common"])
    return 0


def write_mission_audio(map_id, manifest, sides):
    from extract import map_archive, side_archive
    from zero_sound import write_audio
    import zero_sound2

    if not manifest or not manifest.get("script"):
        raise ValueError("A recorded mission is required for audio")
    calls = [(c["name"], c["args"]) for c in manifest["calls"]]
    data = map_archive(map_id).read_bytes()
    assets = mission_assets(manifest, map_id)
    groups = zero_swbf2.world_sections(data, map_id, assets["layers"], keep_unnamed=True)
    active = [groups["base"], *zero_swbf2.requested_layers(groups["layers"], assets["layers"], map_id)]
    world_spans = [(b, e) for group in active for _, b, e in group["worlds"]]
    armies = [side_archive(side).read_bytes() for side in sides]
    ingame = (GAME / "ingame.lvl").read_bytes()
    sources = [(data, [group["range"] for group in active])]
    sources.extend((army, zero_swbf2.package_sections(army, assets["sides"].get(side, [])))
                   for side, army in zip(sides, armies))
    sources.append((ingame, list(iter_all_archives(ingame))))
    for path in zero_family.profile("swbf2").shared_archive_paths(GAME):
        if path != GAME / "ingame.lvl":
            sources.append((path.read_bytes(), ()))
    result = write_audio(map_id, manifest["script"], calls, sides, data, armies, ingame,
                         class_sections=sources, world_spans=world_spans, mission_reader=zero_sound2.mission_audio,
                         config_reader=zero_sound2.config_objects, class_reader=zero_sound2.class_sounds)
    if result is None:
        raise ValueError(f"{map_id}: could not extract the mission's sound bank")
