#!/usr/bin/env python3
"""Audit the extracted collision against the retail archives, map by map.

    BFE_GAME_DIR=<install> BFE_CONTENT_DIR=<game data> python3 tools/audit-collision.py [--maps yav1 kas2] [--json out.json]

A placed object's retail collision is what the archives file under the model
its class draws (GeometryName): coll meshes and prim shapes
(tools/zero_world.py), less the primitives the class reserves for vehicles
(VehicleCollisionOnly) or restricts soldiers to (SoldierCollision), at the
instance's transform, placed through the extractor's own code
(tools/extract.py). The object has extracted collision when the map's
buffers hold those triangles' vertices: collision.bin for the world's
objects, props_collision.bin for the live ones (turrets, power-up droids),
whose collision comes and goes with them.

Reports, per map, the objects with retail collision but none in the extract
(the gaps, by class, naming the model whose collision was expected), the
objects with only part of it, and, apart, the classes without any collision
in the archives (Theed's tall trees, Hoth's cables and rafters), which are
not gaps. Covers the world's instances and the props its tree lines set
along paths; painted foliage meshes are not audited. Exits 1 when an audited
map has a gap, so the check can gate an intake.
"""

import argparse
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from extract import (LIVE_BASES, catalog_map_archives, class_prop, class_props, collision_model, collision_sources,  # noqa: E402
                     map_archive, mission_sides_of, new_polys, place_collision, shared_archives, side_archive,
                     soldier_collision_exclusions, tree_lines)
from ucfb import chunks, iter_archives, nodes  # noqa: E402
from xform import mat4  # noqa: E402
from zero_model import parse_skeleton  # noqa: E402
from zero_odf import entity_classes  # noqa: E402
from zero_paths import GAME, PUBLIC  # noqa: E402
from zero_world import parse_instances, spawn_paths  # noqa: E402


def vertex_keys(positions):
    """Vertices as the exporter writes them (float32), so a placed vertex
    matches its extracted copy exactly."""
    return {struct.unpack("<3f", struct.pack("<3f", *positions[i:i + 3])) for i in range(0, len(positions) - len(positions) % 3, 3)}


def read_vertices(path):
    """The vertices of an extracted positions buffer, or None without one."""
    if not path.exists():
        return None
    data = path.read_bytes()
    return {struct.unpack_from("<3f", data, i) for i in range(0, len(data) - len(data) % 12, 12)}


def skeletons_of(archives):
    """Every skeleton by name, the first archive's winning, as the
    extractor's collect_models keeps them."""
    out = {}
    for data in archives:
        for body, end in nodes(data, b"skel"):
            parsed = parse_skeleton(data, body, end)
            if parsed is not None:
                out.setdefault(parsed["name"], parsed)
    return out


def placed_objects(instances, classes, meshes, shapes, skeletons, live, soldier_shapes, excluded):
    """Each placed object's expected collision: its class, the model the
    collision is filed under, whether it is live (props_collision), where it
    stands, and the vertices the extractor places for it."""
    folded = {name.lower(): value for name, value in skeletons.items()}
    out = []
    for kind, raw in instances:
        model = collision_model(kind, classes, meshes, shapes)
        polys = new_polys()
        # write_collision folds skeleton names; write_props (live objects) does not.
        skeleton = skeletons.get(model) if kind in live else folded.get(model.lower())
        place_collision(model, mat4(raw), meshes, shapes, skeleton, soldier_shapes.get(kind), polys, excluded.get(kind.lower(), ()))
        out.append({"class": kind, "model": model, "base": ((classes or {}).get(kind) or {}).get("base") or "", "live": kind in live,
                    "at": [round(v, 2) for v in raw[9:12]], "vertices": vertex_keys(polys["pos"])})
    return out


def compare(objects, static_vertices, live_vertices):
    """Which objects' expected collision the extracted buffers hold: a
    report with counts and the gaps by class."""
    report = {"placed": len(objects), "withCollision": 0, "present": 0, "missing": {}, "partial": {}, "noRetailCollision": {}}
    for obj in objects:
        expected = obj["vertices"]
        label = obj["class"]
        if not expected:
            entry = report["noRetailCollision"].setdefault(label, {"count": 0, "model": obj["model"], "base": obj["base"]})
            entry["count"] += 1
            continue
        report["withCollision"] += 1
        extracted = live_vertices if obj["live"] else static_vertices
        found = len(expected & extracted) if extracted else 0
        bucket = "present" if found == len(expected) else "partial" if found else "missing"
        if bucket == "present":
            report["present"] += 1
            continue
        entry = report[bucket].setdefault(label, {"count": 0, "model": obj["model"], "live": obj["live"], "at": obj["at"]})
        entry["count"] += 1
    return report


def audit_map(map_id, shared, shared_classes, shared_sources, shared_skeletons, content):
    """One map's report, from its archive (and the mission's sides' when a
    placed class needs one, as the extractor reads them) against the
    extract at `content`/maps/<map>."""
    data = map_archive(map_id).read_bytes()
    classes = dict(entity_classes(data))
    for cname, cls in shared_classes.items():
        classes.setdefault(cname, cls)
    meshes, shapes = collision_sources([data])
    for table, found in ((meshes, shared_sources[0]), (shapes, shared_sources[1])):
        for key, value in found.items():
            table.setdefault(key, value)
    skeletons = skeletons_of([data])
    for name, value in shared_skeletons.items():
        skeletons.setdefault(name, value)
    sources = [data, *shared]
    canonical = {cname.lower(): cname for cname in classes}
    instances = []
    for start, end in iter_archives(data):
        for tag, body, stop in chunks(data, start, end):
            if tag == b"wrld":
                instances.extend(parse_instances(data, body, stop))
    instances = [(canonical.get(kind.lower(), kind), raw) for kind, raw in instances]
    # What neither the map nor the shared archives define, or define without
    # collision: the archives of the sides the map's battles field, read
    # after the shared ones, as the map step reads them.
    unresolved = {kind for kind, _ in instances
                  if kind not in classes or collision_model(kind, classes, meshes, shapes).lower() not in meshes
                  and collision_model(kind, classes, meshes, shapes).lower() not in shapes}
    for side in mission_sides_of(map_id):
        if not unresolved or not side_archive(side).exists():
            continue
        side_data = side_archive(side).read_bytes()
        if not any(kind.encode("latin1", "replace") in side_data for kind in unresolved):
            continue
        for cname, cls in entity_classes(side_data).items():
            classes.setdefault(cname, cls)
        side_meshes, side_shapes = collision_sources([side_data])
        for table, found in ((meshes, side_meshes), (shapes, side_shapes)):
            for key, value in found.items():
                table.setdefault(key, value)
        for name, value in skeletons_of([side_data]).items():
            skeletons.setdefault(name, value)
        sources.append(side_data)
        canonical = {cname.lower(): cname for cname in classes}
        instances = [(canonical.get(kind.lower(), kind), raw) for kind, raw in instances]
        unresolved = {kind for kind in unresolved if kind not in classes}
    instances.extend(tree_lines(data, classes, spawn_paths(data)))
    live = {cname for cname, cls in classes.items() if (cls["base"] or "").lower() in LIVE_BASES}
    soldier_shapes = {cname: set(class_props(cls, "SoldierCollision")) for cname, cls in classes.items() if class_props(cls, "SoldierCollision")}
    excluded = soldier_collision_exclusions(sources)
    objects = placed_objects(instances, classes, meshes, shapes, skeletons, live, soldier_shapes, excluded)
    out = content / "maps" / map_id
    report = compare(objects, read_vertices(out / "collision.bin"), read_vertices(out / "props_collision.bin"))
    report["extract"] = str(out) if (out / "collision.bin").exists() else None
    return report


def describe(map_id, report):
    gaps = sum(entry["count"] for entry in report["missing"].values())
    partial = sum(entry["count"] for entry in report["partial"].values())
    lines = [f"{map_id}: {report['placed']} placed, {report['withCollision']} with retail collision: "
             f"{report['present']} present in the extract, {gaps} MISSING, {partial} partial"]
    if report["extract"] is None:
        lines.append("  no extract (collision.bin not found); every object counts as missing")
    for bucket in ("missing", "partial"):
        for label, entry in sorted(report[bucket].items(), key=lambda item: -item[1]["count"]):
            where = "props_collision.bin (live object)" if entry["live"] else "collision.bin"
            lines.append(f"  {bucket}: {label} x{entry['count']}, collision of model {entry['model']} expected in {where}, "
                         f"e.g. at ({entry['at'][0]}, {entry['at'][1]}, {entry['at'][2]})")
    none = report["noRetailCollision"]
    if none:
        listed = ", ".join(f"{label} ({entry['base'] or 'no class'}) x{entry['count']}"
                           for label, entry in sorted(none.items(), key=lambda item: -item[1]["count"]))
        lines.append(f"  no soldier collision in the archives (not a gap): {listed}")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--maps", nargs="*", help="map ids (default: the catalog's, or every map on disk)")
    parser.add_argument("--content", type=Path, default=PUBLIC, help="the extract to audit (default: BFE_CONTENT_DIR, or public/)")
    parser.add_argument("--json", type=Path, help="write the findings here as JSON")
    args = parser.parse_args()
    if not (GAME / "SIDE").exists():
        print(f"no installation at {GAME}", file=sys.stderr)
        return 2
    map_ids = args.maps or [path.stem for path in catalog_map_archives()]
    shared = shared_archives()
    shared_classes = {}
    for archive in shared:
        for cname, cls in entity_classes(archive).items():
            shared_classes.setdefault(cname, cls)
    shared_sources = collision_sources(shared)
    shared_skeletons = skeletons_of(shared)
    print(f"auditing {len(map_ids)} maps against {args.content}", flush=True)
    reports = {}
    for map_id in map_ids:
        if not map_archive(map_id).exists():
            print(f"{map_id}: no archive at {map_archive(map_id)}", flush=True)
            continue
        reports[map_id] = audit_map(map_id, shared, shared_classes, shared_sources, shared_skeletons, args.content)
        print(describe(map_id, reports[map_id]), flush=True)
    gaps = sum(entry["count"] for report in reports.values() for bucket in ("missing", "partial") for entry in report[bucket].values())
    print(f"\n{len(reports)} maps audited, {gaps} placed objects with retail collision {'missing from' if gaps else 'all present in'} the extract")
    if args.json:
        args.json.write_text(json.dumps(reports, indent=1))
    return 1 if gaps else 0


if __name__ == "__main__":
    sys.exit(main())
