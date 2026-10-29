#!/usr/bin/env python3
"""Audit what the extractor can and cannot resolve, for every map and side.

    BFE_GAME_DIR=<install> python3 tools/audit-import.py [--maps bes1 geo1] [--json out.json]

Runs the extractor's own lookups (tools/extract.py) without writing output:

- placed objects: every world instance's class and model, from the map's
  archive, the shared ones (common.lvl, core.lvl) and, as the map step
  does for the rest, the archives of the sides its battles field;
- effects: every effect a class of the map, its sides or the shared
  archives names (…Effect properties, explosions), looked up in the fx__
  chunks of the shared, side and map archives;
- first person: every FirstPerson rig and cockpit a side's classes name,
  its FPM archive (matched without regard to case) and the model in it;
- units and weapons: each side's playable classes, their bodies, and the
  weapon chains the extractor follows;
- textures: every texture the models of a map and its sides draw.

Each unresolved name is listed with what referenced it and where, if
anywhere, the archives hold it. Classes the engine draws without a model
(godrays, dust, vehicle spawn markers) are listed apart, not as gaps.
"""

import argparse
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from extract import (FOLIAGE_BASES, GAME, catalog_map_archives, class_prop, first_person_archive, map_archive,  # noqa: E402
                     mission_sides_of, playable_units, side_archive)
from ucfb import chunks, cstr_at, iter_archives, nodes  # noqa: E402
from zero_fx import effect, effects  # noqa: E402
from zero_hash import fnv1a  # noqa: E402
from zero_model import collect_models  # noqa: E402
from zero_odf import entity_classes, property_name, soldier_class  # noqa: E402
from zero_texture import collect_textures  # noqa: E402
from zero_weapons import NAMES as WEAPON_NAMES  # noqa: E402
from zero_weapons import classes as weapon_classes  # noqa: E402
from zero_weapons import weapon_chain  # noqa: E402
from zero_world import parse_instances  # noqa: E402

# Classes the engine draws (or places) without a model of their own.
MODELLESS_BASES = {
    "godray": "light shafts the renderer draws",
    "dusteffect": "ambient dust the renderer draws",
    "rumbleeffect": "camera rumble region",
    "vehiclespawn": "vehicle spawn marker (read for the vehicle pads)",
    "hologram": "command post hologram (drawn from the post's class)",
    "soundambiencestatic": "ambient sound emitter (read for the audio)",
    "soundambiencestreaming": "ambient sound emitter (read for the audio)",
    "pathfollower": "path marker",
    "prop": "marker without geometry",
}
EFFECT_KEYS = ("Effect", "WaterEffect", "JetEffect", "DeathDustEffect", "WakeEffect", "TrailEffect", "ImpactEffectSoft",
               "ImpactEffectRigid", "ImpactEffectStatic", "ImpactEffectTerrain", "ImpactEffectWater", "ImpactEffectShield",
               "MuzzleFlashEffect", "ChargeUpEffect", "ShieldEffect", "LightningEffect", "HealthGainEffect", "AmmoGainEffect",
               "ChunkTrailEffect", "ChunkTerrainEffect", "DestroyedEffect", "ExhaustEffect", "DamageEffect",
               "AmbientEffect", "AmbientEffect1", "AmbientEffect2", "AmbientEffect3", "SmokeEffect", "FireEffect",
               "EngineEffect", "BurnEffect", "LandedEffect", "VehicleEffect", "ShieldDownEffect")


def read_classes(data, tag):
    """Every class of a chunk tag with its properties named by whichever
    table (the ODF reader's or the weapon reader's) knows the hash."""
    out = {}
    for body, end in nodes(data, tag):
        typ = base = None
        props = []
        for t, b, e in chunks(data, body, end):
            if t == b"TYPE":
                typ = cstr_at(data, b, e)
            elif t == b"BASE":
                base = cstr_at(data, b, e)
            elif t == b"PROP" and e - b >= 4:
                key = struct.unpack_from("<I", data, b)[0]
                props.append((WEAPON_NAMES.get(key) or property_name(key), cstr_at(data, b + 4, e)))
        if typ:
            out[typ] = {"base": base, "props": props}
    return out


class Archives:
    """Every archive read once, by path, with its parsed classes, models,
    effects and textures made on demand."""

    def __init__(self):
        self.data = {}
        self.cache = {}

    def bytes(self, path):
        if path not in self.data:
            self.data[path] = path.read_bytes() if path.exists() else b""
        return self.data[path]

    def forget(self, path):
        """Drops a map archive's bytes and parses (the maps are 1.7 GB in
        all; the shared and side archives stay)."""
        self.data.pop(path, None)
        for key in [key for key in self.cache if key[0] == path]:
            del self.cache[key]

    def parsed(self, path, what):
        key = (path, what)
        if key not in self.cache:
            data = self.bytes(path)
            if what == "classes":
                found = {}
                for tag in (b"entc", b"wpnc", b"ordc", b"expc"):
                    for name, cls in read_classes(data, tag).items():
                        found.setdefault(name, {**cls, "tag": tag.decode()})
                self.cache[key] = found
            elif what == "models":
                models, skeletons = {}, {}
                collect_models(data, models, skeletons)
                self.cache[key] = models
            elif what == "effects":
                self.cache[key] = effects(data) if data else {}
            elif what == "textures":
                textures = {}
                collect_textures(data, textures)
                self.cache[key] = {name.lower() for name in textures}
        return self.cache[key]

    def holding(self, name, what, paths):
        """The archives among paths holding name (a model, effect or texture)."""
        found = []
        for path in paths:
            keep = path in self.data
            data = self.bytes(path)
            if what == "effect":
                if struct.pack("<I", fnv1a(name)) in data and effect([self.parsed(path, "effects")], name) is not None:
                    found.append(path)
            elif name.encode("latin1", "replace") in data or name.lower().encode("latin1", "replace") in data:
                table = self.parsed(path, "models" if what == "model" else "textures")
                if name in table or name.lower() in table or name.lower() in {k.lower() for k in table}:
                    found.append(path)
            if not keep:
                self.forget(path)
        return found


def relative(path):
    try:
        return str(path.relative_to(GAME))
    except ValueError:
        return str(path)


def audit(map_ids, archives):
    shared_paths = [p for p in (GAME / "Common" / "ENG" / "common.lvl", GAME / "core.lvl") if p.exists()]
    all_maps = catalog_map_archives()
    all_sides = sorted(GAME.glob("SIDE/*.lvl"))
    everywhere = [*shared_paths, *all_sides, *all_maps]
    gaps = []  # {"kind", "name", "where": [referrers], "found": [archives], "note"}
    modelless = {}
    resolved_elsewhere = []

    def gap(kind, name, where, found, note=""):
        for g in gaps:
            if g["kind"] == kind and g["name"] == name:
                if where not in g["where"]:
                    g["where"].append(where)
                return
        gaps.append({"kind": kind, "name": name, "where": [where], "found": [relative(p) for p in found], "note": note})

    # Effects referenced by every class of the shared and side archives.
    def effect_refs(path, label):
        for cname, cls in archives.parsed(path, "classes").items():
            for key, value in cls["props"]:
                if (key in EFFECT_KEYS or key.endswith("Effect")) and value and value.strip() and not value.strip().replace(".", "").replace("-", "").isdigit():
                    yield value.strip(), f"{label} {cname}.{key}"

    for side_path in all_sides:
        side = side_path.stem
        for name, where in effect_refs(side_path, f"side {side}"):
            if effect([archives.parsed(p, "effects") for p in (*shared_paths, side_path)], name) is None:
                found = archives.holding(name, "effect", everywhere)
                if found:
                    resolved_elsewhere.append(("effect", name, where, [relative(p) for p in found]))
                else:
                    gap("effect", name, where, [], "no fx__ chunk in any archive")
        # First person rigs and cockpits.
        for cname, cls in archives.parsed(side_path, "classes").items():
            for key, value in cls["props"]:
                if key != "FirstPerson" or ";" not in value:
                    continue
                archive_name, _, model = value.partition(";")
                fpm = first_person_archive(archive_name)
                if fpm is None or not fpm.exists():
                    gap("first person archive", value, f"side {side} {cname}", [], f"no {relative(fpm) if fpm else archive_name}")
                elif model not in archives.parsed(fpm, "models") and model.lower() not in {m.lower() for m in archives.parsed(fpm, "models")}:
                    gap("first person model", value, f"side {side} {cname}", [], f"not in {relative(fpm)}")
        # Playable units, their bodies and weapon chains.
        data = archives.bytes(side_path)
        shared = [archives.bytes(p) for p in shared_paths]
        models = {}
        for p in (side_path, *shared_paths):
            models.update(archives.parsed(p, "models"))
        for unit in playable_units(side):
            unit_class = soldier_class(data, unit)
            if not unit_class:
                gap("unit class", unit, f"side {side} mission roster", archives.holding(unit, "model", []), "no such class in the side")
                continue
            body = unit_class.get("GeometryName") or ""
            if body and body not in models:
                gap("unit body", body, f"side {side} {unit}", archives.holding(body, "model", everywhere))
            entities = weapon_classes(data, b"entc")
            for key, value in (entities.get(unit) or {"props": []})["props"]:
                if key == "WeaponName" and value and weapon_chain([data, *shared], value) is None:
                    gap("weapon", value, f"side {side} {unit}", [], "weapon_chain found no class")

    for map_id in map_ids:
        path = map_archive(map_id)
        if not path.exists():
            continue
        data = archives.bytes(path)
        sides = mission_sides_of(map_id)
        side_paths = [side_archive(s) for s in sides if side_archive(s).exists()]
        classes = {}
        for p in (path, *shared_paths):
            for cname, cls in entity_classes(archives.bytes(p)).items():
                classes.setdefault(cname, cls)
        models = {}
        for p in (path, *shared_paths):
            models.update(archives.parsed(p, "models"))
        # Placed objects.
        counts = {}
        for start, end in iter_archives(data):
            for t, b, e in chunks(data, start, end):
                if t == b"wrld":
                    for kind, _ in parse_instances(data, b, e):
                        counts[kind] = counts.get(kind, 0) + 1
        for kind, n in sorted(counts.items()):
            cls = classes.get(kind)
            geometry = class_prop(cls, "GeometryName", kind) if cls else kind
            if kind in models or geometry in models:
                continue
            base = (cls or {}).get("base") or ""
            if base.lower() in MODELLESS_BASES:
                modelless.setdefault(f"{kind} ({base})", []).append(f"{map_id} x{n}")
                continue
            if base.lower() in FOLIAGE_BASES:
                continue  # grass and leaf patches: cards the engine draws from the class
            if cls is None:
                # The map step reads the archives of the sides its battles
                # field for these (Geonosis places the Republic's forward center).
                in_sides = [p for p in side_paths if kind in archives.parsed(p, "classes")]
                if in_sides:
                    resolved_elsewhere.append(("class", kind, f"map {map_id} x{n}", [relative(p) for p in in_sides]))
                else:
                    gap("placed class", kind, f"map {map_id} x{n}", archives.holding(kind, "model", everywhere), "no class in the map, shared or side archives")
                continue
            found = archives.holding(geometry, "model", everywhere)
            if found and any(p in side_paths for p in found):
                resolved_elsewhere.append(("model", geometry, f"map {map_id} {kind} x{n}", [relative(p) for p in found]))
            else:
                gap("placed model", geometry, f"map {map_id} {kind} ({base}) x{n}", found, "" if found else "no modl chunk in any archive")
        # Effects the map's own classes name, and the live objects' explosions.
        fx_sources = [archives.parsed(p, "effects") for p in (*shared_paths, path)]
        for name, where in effect_refs(path, f"map {map_id}"):
            if effect(fx_sources, name) is None:
                found = archives.holding(name, "effect", everywhere)
                if found:
                    resolved_elsewhere.append(("effect", name, where, [relative(p) for p in found]))
                else:
                    gap("effect", name, where, [], "no fx__ chunk in any archive")
        # Textures the placed models draw.
        textures = set()
        for p in (path, *shared_paths, *side_paths):
            textures |= archives.parsed(p, "textures")
        for kind in counts:
            for part in models.get(kind) or models.get(class_prop(classes.get(kind), "GeometryName", kind) if classes.get(kind) else kind) or []:
                name = part.get("texture")
                if name and name.lower() not in textures:
                    gap("texture", name, f"map {map_id} {kind}", archives.holding(name, "texture", everywhere))
        archives.forget(path)
    return gaps, modelless, resolved_elsewhere


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--maps", nargs="*", help="map ids (default: the catalog's, or every map on disk)")
    parser.add_argument("--json", type=Path, help="write the findings here as JSON")
    args = parser.parse_args()
    if not (GAME / "SIDE").exists():
        print(f"no installation at {GAME}", file=sys.stderr)
        return 2
    map_ids = args.maps or [p.stem for p in catalog_map_archives()]
    archives = Archives()
    gaps, modelless, elsewhere = audit(map_ids, archives)
    print(f"audited {len(map_ids)} maps, {len(list(GAME.glob('SIDE/*.lvl')))} sides\n")
    print(f"UNRESOLVED ({len(gaps)}):")
    for g in sorted(gaps, key=lambda g: (g["kind"], g["name"])):
        print(f"  {g['kind']}: {g['name']}")
        print(f"    referenced by: {', '.join(g['where'][:6])}{' ...' if len(g['where']) > 6 else ''}")
        print(f"    in the archives: {', '.join(g['found']) or 'nowhere'}{'  (' + g['note'] + ')' if g['note'] else ''}")
    print(f"\nRESOLVED OUTSIDE THE MAP AND SHARED ARCHIVES ({len(elsewhere)}), which the extractor now searches:")
    seen = set()
    for kind, name, where, found in elsewhere:
        if (kind, name) in seen:
            continue
        seen.add((kind, name))
        print(f"  {kind}: {name}  <- {where}  in {', '.join(found)}")
    print(f"\nPLACED WITHOUT A MODEL BY DESIGN ({len(modelless)}):")
    for name, places in sorted(modelless.items()):
        base = name.split("(")[1].rstrip(")")
        print(f"  {name}: {MODELLESS_BASES.get(base.lower(), '')}; {', '.join(places)}")
    if args.json:
        args.json.write_text(json.dumps({"unresolved": gaps, "resolvedElsewhere": elsewhere, "modelless": modelless}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
