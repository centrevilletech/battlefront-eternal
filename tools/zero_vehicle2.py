"""Vehicle and turret bind geometry from the 2005 mission's loaded packages."""

from collections import defaultdict
import hashlib
import json
import math
import re

from ucfb import child, chunks, cstr_at
from xform import IDENTITY, mul
from zero_hash import fnv1a
from zero_model import bind_bones
from zero_swbf2 import resolve_class
from zero_world import coll_meshes, shape_polygons, triangulate_polygon


VEHICLES = {"flyer", "commandflyer", "hover", "commandhover", "walker", "commandwalker", "vehicle",
            "tracked", "wheeled", "turret", "portableturret", "defensegridturret", "remoteterminal"}
MASK_BITS = (("soldier", 1), ("vehicle", 2), ("building", 4), ("terrain", 8), ("ordnance", 16), ("flyer", 32))
SELECTORS = {kind.title() + "Collision" + suffix for kind, _ in MASK_BITS for suffix in ("", "Only")} | {"TargetableCollision"}
FIRE = {"FirePointName", "FireNodeName", "AimerNodeName", "BarrelNodeName"}
POINTS = FIRE | set("""
PilotPosition PassengerPosition TurretNodeName EyePoint EyePointName FirstPersonEyePoint LandingGear
LegBoneLeft LegBoneRight LegBoneTopLeft LegBoneTopRight FootBoneLeft FootBoneRight TerrainLeft TerrainRight
ThrustAttachPoint DamageAttachPoint ContrailAttachPoint ChunkSmokeNodeName AttachToHardPoint
EngineAttachPoint ExhaustAttachPoint AddonAttachJoint
""".split())
OFFSETS = {"EyePointOffset": "TurretNodeName", "ThrustAttachOffset": "ThrustAttachPoint",
           "DamageAttachOffset": "DamageAttachPoint", "ContrailAttachOffset": "ContrailAttachPoint",
           "FirePointExtraOffset": None}
SECTION_MARKERS = {name.lower(): name for name in
                   ("FlyerSection", "WalkerSection", "BuildingSection", "TurretSection")}
LABELS = {f"{fnv1a(k):08x}": k for k in POINTS | OFFSETS.keys() | SELECTORS |
          set(SECTION_MARKERS.values()) |
          {"NextAimer", "NextBarrel", "WeaponSection", "WeaponName", "GeometryName", "ChunkSection",
           "CollisionGeometryName", "CollisionModel", "ChunkGeometryName"}}


def _props(obj):
    return [(LABELS.get(k, k), v) for k, v in obj["props"]]


def vehicle_sections(cls, catalog):
    """Ordered control declarations; selecting a weapon or debris part keeps the current control."""
    base = (cls["engineBase"] or "").lower()
    if base not in VEHICLES:
        armed_building = base in {"armedbuilding", "armedbuildingdynamic", "commandarmedanimatedbuilding"}
        for key, value in _props(cls):
            marker = SECTION_MARKERS.get(key.lower())
            if marker and (marker != "BuildingSection" or value.lower() != "body"):
                break
            if armed_building and re.fullmatch(r"WeaponName\d*", key):
                break
        else:
            return None

    sections = []
    for source in [*cls["parents"], cls["name"]]:
        block = None
        for key, value in _props(catalog["entc"][source.lower()]):
            marker = SECTION_MARKERS.get(key.lower())
            if marker or block is None:
                name = value if marker else "BODY"
                kind = "chassis" if name.lower() == "body" else "seat"
                block = {"declaredBy": source, "kind": kind, "name": name, "marker": marker, "properties": []}
                sections.append(block)
            if marker is None:
                block["properties"].append((key, value))
    return sections


def _vector(text):
    try:
        values = [float(x) for x in text.split()]
    except ValueError:
        return None
    return values if len(values) == 3 and all(map(math.isfinite, values)) else None


def _translated(matrix, offset):
    if matrix is None:
        return None
    move = list(IDENTITY)
    move[12:15] = offset
    return mul(matrix, move)


def _mask(bits):
    return {"mask": bits, "collidesWith": [name for name, bit in MASK_BITS if not bits or bits & bit]}


def _meshes(sources, wanted):
    found = {}
    for data, spans in sources:
        own = defaultdict(list)
        for start, end in spans:
            for tag, b, e in chunks(data, start, end):
                if tag != b"coll":
                    continue
                field = child(data, b, e, b"NAME")
                name = cstr_at(data, *field) if field else ""
                key = name.lower()
                if key not in wanted or key in found:
                    continue
                records = coll_meshes(data, sections=[(b - 8, e)]).get(name)
                if not records:
                    raise ValueError(f"Invalid collision mesh for vehicle model {name}")
                positions, polygons = records[0]
                node = child(data, b, e, b"NODE")
                mask = child(data, b, e, b"MASK")
                own[key].append({"node": cstr_at(data, *node) if node else None,
                                 **_mask(data[mask[0]] if mask and mask[0] < mask[1] else None),
                                 "positions": positions, "polygons": polygons,
                                 "indices": [i for p in polygons for tri in triangulate_polygon(positions, p) for i in tri]})
        found.update(own)
    return found


def _node(name, joints, bones):
    if not name:
        return {"name": None, "joint": None, "parent": None, "matrix": IDENTITY}
    index = joints.get(name.lower())
    bone = bones[index] if index is not None else None
    return {"name": name, "joint": index, "parent": bone["parent"] if bone else None,
            "matrix": bone["matrix"] if bone else None}


def _primitive_surface(kind, size):
    positions = []
    indices = []
    vertices = {}
    for face in shape_polygons(kind, size):
        polygon = []
        for vertex in face:
            if vertex not in vertices:
                vertices[vertex] = len(positions)
                positions.append(vertex)
            polygon.append(vertices[vertex])
        if len(polygon) >= 3:
            indices.extend(i for tri in triangulate_polygon(positions, polygon) for i in tri)
    return {"positions": positions, "indices": indices}


def _model(rig, meshes, shapes):
    bones = [dict(bone, matrix=matrix) for bone, matrix in zip(bind_bones(rig), rig["model"])] if rig else []
    joints = {b["name"].lower(): i for i, b in enumerate(bones)}
    for mesh in meshes:
        mesh["joint"] = joints.get((mesh["node"] or "").lower())
    primitives = []
    for shape in shapes:
        anchor = _node(shape.get("parent"), joints, bones)
        local = shape["matrix"]
        primitives.append({"name": shape["name"], "parent": anchor["name"], "joint": anchor["joint"],
                           "local": local, "matrix": mul(anchor["matrix"], local) if anchor["matrix"] else None,
                           "kind": shape["kind"], "size": shape["size"], **_mask(shape.get("mask")),
                           **_primitive_surface(shape["kind"], shape["size"])})
    named = {}
    for role in ("pilot", "gunner", "passenger", "cockpit", "eye", "gear", "foot", "engine", "exhaust", "thrust"):
        matches = [i for n, i in joints.items() if role in n]
        if matches:
            named[role] = matches
    unresolved = [p["parent"] for p in primitives if p["parent"] and p["joint"] is None]
    return {"skeleton": bones, "nodes": joints, "namedNodes": named, "meshes": meshes,
            "primitives": primitives, "unresolvedParents": unresolved}


def _seat(name):
    return {"name": name, "points": [], "weapons": {}}


def _mounts(sections, geometry, models, weapons):
    seats = {}
    points = []
    missing = []

    def add(key, value, anchor, offset=None, owner=geometry):
        rig = models[owner]
        ref = _node(anchor, rig["nodes"], rig["skeleton"])
        ref.update(property=key, value=value, model=owner)
        if offset is not None:
            ref.update(offset=offset, matrix=_translated(ref["matrix"], offset))
        if anchor and ref["joint"] is None:
            missing.append({"property": key, "name": anchor, "model": owner})
        points.append(ref)
        return len(points) - 1

    def weapon(number):
        return seat["weapons"].setdefault(number, {"section": number, "class": None, "points": [], "firePoints": []})

    # A seat's pivot may follow its offset, including in a later declaration.
    pivots = {s["name"].lower(): value for s in sections
              for key, value in s["properties"] if key == "TurretNodeName"}
    for section in sections:
        name = section["name"].lower()
        seat = seats.setdefault(name, _seat(section["name"]))
        slot, aimer, barrel = "1", 0, 0
        anchors = {}
        chunk_model = geometry
        for key, value in section["properties"]:
            if key.lower() == "chunksection":
                chunk_model = geometry
                continue
            if key == "ChunkGeometryName" and value:
                chunk_model = value.lower().removesuffix(".msh")
            elif key in {"DamageEffect", "ThrustEffect", "ContrailEffect"}:
                anchors.pop(key.removesuffix("Effect") + "AttachPoint", None)
            if key == "WeaponSection":
                slot, aimer, barrel = value, 0, 0
            elif match := re.fullmatch(r"WeaponName(\d*)", key):
                weapon(match[1] or slot)["class"] = value.lower()
            elif key == "NextAimer":
                aimer, barrel = aimer + 1, 0
            elif key == "NextBarrel":
                barrel += 1
            elif key in OFFSETS:
                vector = _vector(value)
                if vector is not None:
                    anchor = pivots.get(name) if key == "EyePointOffset" else anchors.get(OFFSETS[key])
                    index = add(key, value, anchor, vector)
                    seat["points"].append(index)
            else:
                owner = chunk_model if key == "ChunkSmokeNodeName" else geometry
                nodes = models[owner]["nodes"]
                words = value.split()
                unknown = re.fullmatch(r"[0-9a-fA-F]{8}", key)
                if key not in POINTS and not unknown and not any(
                        word in key.lower() for word in ("node", "point", "position", "bone", "attach")):
                    continue
                matches = [w for w in words if w.lower() in nodes or w.lower().startswith("hp_")]
                if key not in POINTS and not matches:
                    continue
                vector = _vector(value)
                if len(words) == 4 and matches == words[:1]:
                    vector = _vector(" ".join(words[1:]))
                for anchor in matches or [None if vector is not None else value]:
                    index = add(key, value, anchor, vector, owner)
                    anchors[key] = anchor
                    if key != "ChunkSmokeNodeName":
                        seat["points"].append(index)
                    if key in FIRE:
                        weapon(slot)["points"].append({"point": index, "aimer": aimer, "barrel": barrel})

    for seat in seats.values():
        seat["weapons"] = list(seat["weapons"].values())
        for w in seat["weapons"]:
            groups = defaultdict(list)
            for ref in w["points"]:
                groups[ref["aimer"], ref["barrel"]].append(ref["point"])
            for refs in groups.values():
                for keys in ({"FirePointName", "FireNodeName"}, {"BarrelNodeName"}, {"AimerNodeName"}):
                    preferred = [i for i in refs if points[i]["property"] in keys]
                    if preferred:
                        w["firePoints"].extend(preferred)
                        break
            offsets = []
            for key, value in weapons.get(w["class"], ()):
                if key in {"FirePointName", "FireNodeName"}:
                    w["firePoints"].append(add(key, value, value))
                elif key == "FirePointExtraOffset" and (offset := _vector(value)) is not None:
                    offsets.append((value, offset))
            w["extraOffsets"] = [offset for _, offset in offsets]
            w["offsetPoints"] = []
            for value, offset in offsets:
                for origin in w["firePoints"] or [None]:
                    anchor = points[origin]["name"] if origin is not None else None
                    w["offsetPoints"].append({"origin": origin, "point": add("FirePointExtraOffset", value, anchor, offset)})

    body = seats.get("body")
    if body and len(seats) > 1 and not (body["weapons"] or body["points"]):
        del seats["body"]
    return {"points": points, "seats": list(seats.values()) or [_seat("BODY")], "unresolvedPoints": missing}


def vehicle_pack(classes, catalog, rigs, shape_table, mesh_table, sources):
    selected = {}
    names = set()
    weapons = {}
    for name, cls in sorted(classes.items()):
        sections = vehicle_sections(cls, catalog)
        if sections is None:
            continue
        props = _props(cls)
        fields = dict(props)
        geometry = fields.get("GeometryName", name).lower().removesuffix(".msh")
        declared = fields.get("CollisionGeometryName") or fields.get("CollisionModel")
        collision = declared.lower().removesuffix(".msh") if declared else geometry
        if not declared and collision not in mesh_table and collision not in shape_table and (name in mesh_table or name in shape_table):
            collision = name
        names.update((geometry, collision))
        selected[name] = (sections, props, geometry, collision)
        for key, value in props:
            if key == "ChunkGeometryName" and value:
                names.add(value.lower().removesuffix(".msh"))
            if re.fullmatch(r"WeaponName\d*", key) and value.lower() not in weapons:
                cls_weapon = resolve_class("wpnc", value, catalog)
                weapons[value.lower()] = _props(cls_weapon) if cls_weapon else []

    meshes = _meshes(sources, names)
    models = {n: _model(rigs.get(n), meshes.get(n, []), shape_table.get(n, [])) for n in sorted(names)}
    result = {}
    for name, (sections, props, geometry, collision) in selected.items():
        geom = models[collision]
        selectors = []
        for key, value in props:
            if key in SELECTORS:
                selectors.append({"property": key, "name": value,
                     "primitives": [i for i, row in enumerate(geom["primitives"]) if row["name"].lower() == value.lower()],
                     "meshes": [i for i, row in enumerate(geom["meshes"]) if value.lower() in
                                {"collisionmesh", (row["node"] or "").lower()}]})
        cls = classes[name]
        result[name] = {"base": cls["engineBase"], "model": geometry, "collisionModel": collision,
                        "properties": cls["props"], "vehicleSections": sections, "collisionSelections": selectors,
                        **_mounts(sections, geometry, models, weapons)}
    return {"family": "swbf2", "version": 1, "classes": result, "models": models}


PACK_PATH = "vehicles/vehicles.json"


def write_pack(directory, pack):
    from zero_scripts2 import write_if_changed
    data = json.dumps(pack, separators=(",", ":")).encode()
    write_if_changed(directory / PACK_PATH, data)
    return {"vehicles": PACK_PATH, "vehiclePackSha256": hashlib.sha256(data).hexdigest()}


def pack_current(directory, record=None):
    try:
        if record is None:
            record = json.loads((directory / "map.json").read_bytes())
        return (record.get("vehicles") == PACK_PATH and
                hashlib.sha256((directory / PACK_PATH).read_bytes()).hexdigest() == record.get("vehiclePackSha256"))
    except (FileNotFoundError, ValueError, AttributeError):
        return False
