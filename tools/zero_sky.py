"""The sky's flyers: the `SkyObject`s of a map's sky config, and the battles
that draw them (docs/formats.md, "Sky flyers").

A SkyObject is a class of ships crossing the sky: `NumObjects` copies of one
model (`Geometry`, a low-detail `*_DOME` model) flying in straight lines
around the world's origin. The original game reads it with these
defaults: one object, `Height` 70 to 100 m, `VelocityX`, `VelocityY` and
`VelocityZ` (each a min and max in the ship's own axes) 0, `Acceleration`
0, `Distance` 300 m, `InDirectionFactor` 3 and `LifeTime` 1000 s. A key
given twice keeps its last value; a range given as one value is that value.

The model is looked up by name among the models loaded when the battle
starts, and a SkyObject whose model isn't loaded has no ships. The dome
models live in the side archives' sub-levels of the same name
(SIDE/rep.lvl's `rep_fly_assault_dome`), which only some battles' scripts
read (ReadDataFile("SIDE\\rep.lvl", "rep_fly_assault_dome", ...)). So each
battle draws its own set: Cloud City's Republic assault ships in the Clone
Wars and none in the Galactic Civil War, Bespin: Platforms none at all.

This port's Enhanced sky (src/sky-flyers.js) draws every class of the
battle's era instead, models loaded or not: a class's era is its side's,
the side archive whose sub-level holds its model (rep and cis the Clone
Wars, all and imp the Galactic Civil War), else its model name's prefix; a
battle's era is its mission's (tools/extract.py MISSION_ERAS). So Bespin:
Platforms gets capital ships too, and Cloud City's Galactic Civil War
battle Star Destroyers and Mon Calamari cruisers.

write_sky_flyers writes <map>/sky/: flyers.json (the classes some battle
draws, which battles draw which, as the original (`missions`) and as the
Enhanced sky (`enhanced`), the models' parts and textures), flyers.bin and
flyers_index.bin (the models, as extract.write_meshes packs them) and
textures/. None of it is in the content fingerprint
(src/runtime/content-hash.js): it is presentation only (src/sky-flyers.js).
"""

import json
import shutil
import struct
from pathlib import Path

import zero_addon
from ucfb import child, chunks, cstr_at
from zero_hash import fnv1a
from zero_lua import calls, load, mission_scripts
from zero_paths import GAME

# The original game's defaults (SkyObjectClass constructor).
DEFAULT_COUNT = 1
DEFAULT_HEIGHT = (70.0, 100.0)
DEFAULT_DISTANCE = 300.0
DEFAULT_IN_DIRECTION = 3.0
DEFAULT_LIFETIME = 1000.0

# The era each army fights in (tools/extract.py SIDE_ERAS).
SIDE_ERAS = {"rep": "cw", "cis": "cw", "all": "gcw", "imp": "gcw"}

# The velocity ranges, in the ship's right, up and forward axes.
AXES = {"VelocityX": 0, "VelocityY": 1, "VelocityZ": 2}


def _number(value, default=0.0):
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def sky_objects(sky_config):
    """The sky config's SkyObjects in order, with the original game's defaults:
    {model, count, height [min, max], velocity [[min, max] x, y, z],
    acceleration [x, y, z], distance, inDirection, lifeTime}. A SkyObject
    without a model is kept (it draws nothing), so indices follow the
    config."""
    found = []
    for name, _, settings in sky_config:
        if name != "SkyObject":
            continue
        flyer = {"model": None, "count": DEFAULT_COUNT, "height": list(DEFAULT_HEIGHT),
                 "velocity": [[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]], "acceleration": [0.0, 0.0, 0.0],
                 "distance": DEFAULT_DISTANCE, "inDirection": DEFAULT_IN_DIRECTION, "lifeTime": DEFAULT_LIFETIME}
        for key, values, _ in settings:
            if not values:
                continue
            if key == "Geometry":
                flyer["model"] = str(values[0]) or None
            elif key == "NumObjects":
                # Read as a float and truncated (cvttss2si).
                flyer["count"] = int(_number(values[0]))
            elif key == "Height" or key in AXES:
                # The minimum, and the maximum when a second value is given.
                low = _number(values[0])
                span = [low, _number(values[1], low) if len(values) > 1 else low]
                if key == "Height":
                    flyer["height"] = span
                else:
                    flyer["velocity"][AXES[key]] = span
            elif key == "Acceleration":
                flyer["acceleration"] = [_number(values[i]) if i < len(values) else 0.0 for i in range(3)]
            elif key == "Distance":
                flyer["distance"] = _number(values[0], DEFAULT_DISTANCE)
            elif key == "InDirectionFactor":
                flyer["inDirection"] = _number(values[0], DEFAULT_IN_DIRECTION)
            elif key == "LifeTime":
                flyer["lifeTime"] = _number(values[0], DEFAULT_LIFETIME)
        found.append(flyer)
    return found


def mission_roots(game=GAME):
    """mission -> the add-on folder whose mission.lvl holds it (where its
    "dc:" paths are), or None for the game's own. The first file to hold a
    name wins, as in zero_addon.missions."""
    roots = {}
    for path, root in zero_addon.mission_files(game):
        for name in mission_scripts(path.read_bytes()):
            roots.setdefault(name, root)
    return roots


def script_reads(chunk, root=None, game=GAME):
    """The archives a mission script reads (ReadDataFile), in order, each
    with the sub-levels it names: [(path, [sub-level, ...])]. A ";name"
    after the path names a sub-level too; files that aren't there are left
    out."""
    reads = []
    for call, args in calls(load(chunk)):
        if call != "ReadDataFile" or not args or not isinstance(args[0], str):
            continue
        path, _, inline = args[0].partition(";")
        subs = ([inline] if inline else []) + [arg for arg in args[1:] if isinstance(arg, str) and arg]
        found = zero_addon.data_file(path, root, game)
        if found and found.is_file():
            reads.append((found, subs))
    return reads


def _model_names(data, start, end):
    """Lower-case names of the models in a run of chunks."""
    names = set()
    for tag, b, e in chunks(data, start, end):
        if tag == b"modl":
            name = child(data, b, e, b"NAME")
            if name:
                names.add(cstr_at(data, *name).lower())
    return names


def loaded_sections(data, subs):
    """The parts of an archive a ReadDataFile of these sub-levels loads, as
    (start, end) runs of chunks: the named lvl_ sub-levels (each led by the
    FNV hash of its name), or the archive's top level when none is named."""
    top = chunks(data, 0, len(data))
    if not top or top[0][0] != b"ucfb":
        return []
    root_b, root_e = top[0][1], top[0][2]
    if not subs:
        return [(root_b, root_e)]
    wanted = {fnv1a(sub) for sub in subs}
    sections = []
    for tag, b, e in chunks(data, root_b, root_e):
        if tag == b"lvl_" and e - b >= 8 and struct.unpack_from("<I", data, b)[0] in wanted:
            sections.append((b + 8, e))
    return sections


class Loads:
    """Which models each battle loads, reading each archive once."""

    def __init__(self, game=GAME):
        self.game = game
        self.archives = {}
        self.sections = {}

    def archive(self, path):
        if path not in self.archives:
            self.archives[path] = path.read_bytes()
        return self.archives[path]

    def models(self, chunk, root=None):
        """model name (lower case) -> (archive bytes, start, end, side) of
        the section that loads it, for every model a mission script loads;
        side is the archive's name when it is a side archive (SIDE/rep.lvl:
        rep), else None. The first section to hold a name wins, as the
        first loaded does."""
        found = {}
        for path, subs in script_reads(chunk, root, self.game):
            key = (str(path), tuple(sub.lower() for sub in subs))
            if key not in self.sections:
                data = self.archive(path)
                side = path.stem.lower() if path.parent.name.lower() == "side" else None
                self.sections[key] = [(data, b, e, side, _model_names(data, b, e)) for b, e in loaded_sections(data, subs)]
            for data, b, e, side, names in self.sections[key]:
                for name in names:
                    found.setdefault(name, (data, b, e, side))
        return found


def side_archives(game=GAME):
    """The armies' archives by side name: the game's SIDE/<side>.lvl, then
    the add-ons' own (Jabba's Palace's Gamorreans). The game's win."""
    found = {}
    for base in [Path(game)] + [add_on.root for add_on in zero_addon.add_ons(game)]:
        folder = zero_addon.child(base, "SIDE")
        try:
            paths = sorted(folder.iterdir()) if folder else []
        except OSError:
            paths = []
        for path in paths:
            if path.suffix.lower() == ".lvl":
                found.setdefault(path.stem.lower(), path)
    return found


def dome_sources(models, loads, game=GAME):
    """model name -> (archive bytes, start, end, side) for each of these
    models (lower case) that a side archive holds in a sub-level of the
    model's own name (SIDE/rep.lvl's rep_fly_assault_dome), as a battle's
    script would load it. The side its name starts with is looked in
    first."""
    archives = side_archives(game)
    found = {}
    for model in sorted(models):
        prefix = model.split("_", 1)[0]
        for side in sorted(archives, key=lambda name: name != prefix):
            data = loads.archive(archives[side])
            hit = next(((b, e) for b, e in loaded_sections(data, [model]) if model in _model_names(data, b, e)), None)
            if hit:
                found[model] = (data, *hit, side)
                break
    return found


def flyer_era(model, side=None):
    """A class's era, "cw" or "gcw": its side's (the side archive whose
    sub-level holds its model, else its model name's prefix: rep_ and cis_
    the Clone Wars, all_ and imp_ the Galactic Civil War), or None."""
    for name in (side, (model or "").split("_", 1)[0]):
        if name and name.lower() in SIDE_ERAS:
            return SIDE_ERAS[name.lower()]
    return None


def _as_archive(data, start, end):
    """A run of chunks as an archive of its own, for the archive readers."""
    body = data[start:end]
    return b"ucfb" + struct.pack("<I", len(body)) + body


def battle_flyers(flyers, missions, loads=None):
    """mission -> indices of the flyers whose model it loads, and model name
    -> where it was found (archive bytes, start, end, side), for missions
    {name: (script chunk, add-on root)}."""
    loads = loads or Loads()
    drawn, sources = {}, {}
    for mission, (chunk, root) in sorted(missions.items()):
        try:
            models = loads.models(chunk, root)
        except Exception as error:  # noqa: BLE001 - one odd script must not stop the rest
            print(f"  sky flyers: {mission} unreadable ({error})", flush=True)
            continue
        drawn[mission] = []
        for i, flyer in enumerate(flyers):
            model = (flyer.get("model") or "").lower()
            if model and flyer["count"] > 0 and model in models:
                drawn[mission].append(i)
                sources.setdefault(model, models[model])
    return drawn, sources


def era_flyers(flyers, drawn, eras, sources):
    """mission -> indices of the flyers this port's Enhanced sky draws: the
    battle's era's classes whose model a side archive holds, and those the
    battle's script loads that have no era. Never another era's."""
    era = [flyer_era(f.get("model"), (sources.get((f.get("model") or "").lower()) or (None,) * 4)[3]) for f in flyers]
    enhanced = {}
    for mission, indices in drawn.items():
        battle = (eras or {}).get(mission)
        enhanced[mission] = [i for i, f in enumerate(flyers)
                             if f["count"] > 0 and (f.get("model") or "").lower() in sources
                             and ((battle and era[i] == battle) or (era[i] is None and i in indices))]
    return enhanced, era


def write_sky_flyers(out, sky_config, missions, fallback_textures=None, loads=None, eras=None, game=GAME):
    """Write out/ (a map's sky/ folder) for the battles that draw flyers, or
    remove it when none does. missions: {name: (script chunk, add-on root
    or None)} of the battles fought in this world; eras: {name: "cw" or
    "gcw"}. Returns flyers.json's content, or None."""
    from extract import write_meshes, write_textures
    from zero_model import collect_models
    from zero_texture import collect_textures

    flyers = sky_objects(sky_config)
    loads = loads or Loads(game)
    drawn, sources = battle_flyers(flyers, missions, loads) if flyers else ({}, {})
    # The models no battle loads, for the Enhanced sky: from the side
    # archive's own sub-level.
    unloaded = {(f.get("model") or "").lower() for f in flyers if f.get("model") and f["count"] > 0} - set(sources)
    for model, source in (dome_sources(unloaded, loads, game) if unloaded else {}).items():
        sources.setdefault(model, source)
    enhanced, era = era_flyers(flyers, drawn, eras, sources)
    used = sorted({i for lists in (drawn, enhanced) for indices in lists.values() for i in indices})
    if not used:
        shutil.rmtree(out, ignore_errors=True)
        if flyers:
            print(f"  sky flyers: {len(flyers)} SkyObjects, no model found for them", flush=True)
        return None
    # Only the classes some battle draws, renumbered in the config's order.
    number = {old: new for new, old in enumerate(used)}
    kept = [{**flyers[i], "model": flyers[i]["model"].lower(), "era": era[i]} for i in used]
    models, textures = {}, {}
    for name, (data, start, end, _side) in sources.items():
        if name not in {f["model"] for f in kept}:
            continue
        archive = _as_archive(data, start, end)
        found = {}
        collect_models(archive, found)
        parts = next((p for n, p in found.items() if n.lower() == name), None)
        if parts:
            models[name] = parts
        collect_textures(archive, textures)
    for name, image in (fallback_textures or {}).items():
        textures.setdefault(name, image)
    missing = sorted({f["model"] for f in kept} - set(models))
    if missing:
        raise ValueError(f"sky flyer models without geometry: {missing}")
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    records = write_meshes(models, sorted(models), out, "flyers")
    wanted = {part["texture"] for parts in models.values() for part in parts if part["texture"]}
    table = write_textures(textures, wanted, out / "textures")
    content = {
        "flyers": kept,
        # The battles' classes as the original draws them, and as this
        # port's Enhanced sky does.
        "missions": {mission: [number[i] for i in indices] for mission, indices in sorted(drawn.items())},
        "enhanced": {mission: [number[i] for i in indices] for mission, indices in sorted(enhanced.items())},
        "models": records,
        "textures": table,
    }
    (out / "flyers.json").write_text(json.dumps(content))
    for kind in ("missions", "enhanced"):
        summary = {mission: [kept[i]["model"] for i in indices] for mission, indices in content[kind].items()}
        print(f"  sky flyers ({'classic' if kind == 'missions' else 'enhanced'}): {summary}", flush=True)
    return content
