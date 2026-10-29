#!/usr/bin/env python3
"""The spawn screen's data: the art and text the original's unit and spawn
point select screen draws, each unit's and weapon's display name, and each
map's camera shots, teams and post vehicles. docs/spawn-screen-assets.md has
the schema.

    python3 tools/zero_spawn_screen.py [map ...] [--side rep ...] [--common]
    python3 tools/zero_spawn_screen.py --all-extracted

tools/extract.py writes all of it as it extracts a map, a side and common.
This command adds it to an extraction made before, and rewrites nothing else:
map.json gains spawnScreen; units.json each unit's and each weapon's
displayName; common.json the window textures, the shell_cube room (packed
after the hologram logos), the screen's strings and rasterHud. The textures
it adds get KTX2 copies (tools/ktx2.py) and, with Pillow, 4x WebP atlases
(tools/upscale_hud.py).
"""

import argparse
import json
import math
import sys
import tempfile
from pathlib import Path

import zero_addon
from ucfb import nodes
from zero_ai_mission import faction_of
from zero_hash import fnv1a
from zero_locl import localize
from zero_lua import calls as lua_calls
from zero_lua import load as lua_load
from zero_world import instance_records

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import GAME  # noqa: E402
from zero_paths import PUBLIC  # noqa: E402

# The windows' art in common.lvl: the window background (a 9-slice), the
# title bar's left end, middle and right end, the title intro's sweep block,
# the SPAWN box, the mouse pointer, and the background room's grid.
SPAWN_TEXTURES = (
    "border_3_pieces", "headerbuttonleft", "headerbuttonmid", "headerbuttonright", "cursor", "opaque_rect",
    "mousecursor", "shell_cube",
)
# The background room, packed with the hologram logos the team icons use.
SPAWN_MODELS = ("shell_cube",)
# Art tools/upscale_hud.py enlarges besides what its name rule picks: the
# above, and the logos' texture.
RASTER_HUD = SPAWN_TEXTURES + ("com_icon_controlzone",)
# The screen's text, its tooltips, and the pause menu's RESPAWN prompt.
SPAWN_STRINGS = (
    "ifs.SpawnDisplay.Spawn", "game.mapdisplay.objectives", "game.spawndisplay.launchin", "common.none",
    "common.back", "game.tooltips.select_spawn_location", "game.tooltips.reinforcements",
    "game.tooltips.select_soldier", "game.pause.suicide", "game.pause.suicide_prompt",
)

# The two teams the screen offers.
TEAMS = (1, 2)
# A team's side, by the faction SetTeamName's parser makes of its name.
SIDES = {1: "all", 2: "imp", 3: "rep", 4: "cis"}
# A vehicle spawn's class properties, as the original game reads them for a
# team: Class<Faction> for any team of that faction, Class<Short>ATK for team
# 1 and Class<Short>DEF for team 2. Once a mission calls SetHistorical, teams
# 1 and 2 read the Historical ones.
VEHICLE_CLASS_KEYS = {
    0: ("ClassNeutral", "ClassNeutral", "ClassNeutral"),
    1: ("ClassAlliance", "ClassAllATK", "ClassAllDEF"),
    2: ("ClassEmpire", "ClassImpATK", "ClassImpDEF"),
    3: ("ClassRepublic", "ClassRepATK", "ClassRepDEF"),
    4: ("ClassCIS", "ClassCISATK", "ClassCISDEF"),
    5: ("ClassLocals", "ClassLocATK", "ClassLocDEF"),
    6: ("ClassHistorical", "ClassHisATK", "ClassHisDEF"),
}
HISTORICAL = 6
VEHICLE_SPAWN = "com_item_vehicle_spawn"
CONTROL_ZONE = fnv1a("ControlZone")


# ---------------------------------------------------------------- names


def display_text(locl, key):
    """A key's English text without the line break some strings end in, or
    None."""
    text = localize(locl, key)
    return text.rstrip("\r\n") if text is not None else None


def unit_name(locl, name):
    """A class's display name: entity. and the class name with its first
    underscore as a dot (rep_inf_clone_trooper is entity.rep.inf_clone_trooper),
    or the class name when there is none."""
    return display_text(locl, "entity." + name.replace("_", ".", 1)) or name


def weapon_name(locl, name):
    """A weapon's display name: weapons. and the class name with its first two
    underscores as dots (rep_weap_inf_rifle is weapons.rep.weap.inf_rifle), or
    the class name when there is none."""
    return display_text(locl, "weapons." + name.replace("_", ".", 2)) or name


def spawn_strings(locl):
    """The screen's strings that the language has, by key."""
    return {key: display_text(locl, key) for key in SPAWN_STRINGS if localize(locl, key) is not None}


# ---------------------------------------------------------------- maps


def camera_shot(args):
    """AddCameraShot(qw, qx, qy, qz, x, y, z): a camera's rotation, then its
    position, in the world frame the archive and map.json use. The engine's
    camera looks down its own -z, as a three.js camera does, so the rotation
    (normalized, as the engine normalizes it) goes onto one unchanged; three
    orders a quaternion x, y, z, w."""
    w, x, y, z, px, py, pz = (float(v) for v in args[:7])
    norm = math.sqrt(w * w + x * x + y * y + z * z) or 1.0
    return {"position": [px, py, pz], "quaternion": [x / norm, y / norm, z / norm, w / norm]}


def team_number(value):
    return int(value) if isinstance(value, (int, float)) and float(value).is_integer() else None


def post_vehicles(records, posts, factions, historical, locl):
    """Post name -> team -> the vehicles the post's spawns give that team.

    A vehicle spawn (com_item_vehicle_spawn) belongs to the command post its
    ControlZone names; names compare by their hash, as the engine looks them
    up. Each team takes the class its faction's properties name (the last one
    wins); a post lists each class once, in world order, by display name."""
    by_hash = {}
    for post in posts:
        by_hash.setdefault(fnv1a(post), post)
    keys = {}
    for team in TEAMS:
        faction = HISTORICAL if historical else factions.get(team)
        if faction in VEHICLE_CLASS_KEYS:
            names = VEHICLE_CLASS_KEYS[faction]
            keys[team] = {fnv1a(names[0]), fnv1a(names[team])}
    found = {}
    for record in records:
        if record["class"].lower() != VEHICLE_SPAWN:
            continue
        zone = None
        chosen = {}
        for key, value in record["props"]:
            if key == CONTROL_ZONE:
                zone = value
            for team, wanted in keys.items():
                if key in wanted and value:
                    chosen[team] = value
        post = by_hash.get(fnv1a(zone)) if zone is not None else None
        if post is None:
            continue
        classes = found.setdefault(post, {team: [] for team in TEAMS})
        for team, name in chosen.items():
            if name.lower() not in (c.lower() for c in classes[team]):
                classes[team].append(name)
    return {post: {str(team): [unit_name(locl, c) for c in found[post][team]] for team in TEAMS}
            for post in posts if post in found}


def spawn_screen(calls, records, posts, locl):
    """map.json's spawnScreen. From the mission's calls (zero_lua.calls), in
    their order: its camera shots, and each team's side (SetTeamName) and
    objectives (AddMissionObjective, in English). From the world's placed
    objects (instance records with raw property hashes) and its command post
    names: the vehicles each post gives each team."""
    names = {}
    objectives = {team: [] for team in TEAMS}
    shots = []
    historical = False
    for name, args in calls:
        team = team_number(args[0]) if args else None
        if name == "SetTeamName" and team in TEAMS and len(args) > 1 and isinstance(args[1], str):
            names[team] = args[1]
        elif name == "AddMissionObjective" and team in TEAMS and len(args) > 2 and isinstance(args[2], str):
            text = display_text(locl, args[2])
            if text:
                objectives[team].append(text)
        elif name == "AddCameraShot" and len(args) >= 7 and all(isinstance(v, float) for v in args[:7]):
            shots.append(camera_shot(args))
        elif name == "SetHistorical":
            historical = True
    factions = {team: faction_of(names[team]) for team in names}
    return {
        "cameraShots": shots,
        "teams": {str(team): {"side": SIDES.get(factions.get(team)), "objectives": objectives[team]} for team in TEAMS},
        "postVehicles": post_vehicles(records, posts, factions, historical, locl),
    }


def world_records(data):
    """Every placed object in a map archive, with raw property hashes."""
    return [record for b, e in nodes(data, b"wrld") for record in instance_records(data, b, e, name_of=lambda key: key)]


def map_spawn_screen(data, mission, posts, locl, game=GAME):
    """A map's spawnScreen, for its mission script (by name, the game's or an
    add-on's) and command post names; None without the script."""
    scripts = zero_addon.missions(game)
    if mission not in scripts:
        return None
    return spawn_screen(lua_calls(lua_load(scripts[mission])), world_records(data), posts, locl)


# ---------------------------------------------------------------- refresh


def core_localization(game=GAME):
    """core.lvl's text, with what the add-ons add (tools/zero_addon.py)."""
    return zero_addon.localization(game)


def refresh_map(name, locl, game=GAME, public=PUBLIC):
    """Add spawnScreen to an extracted map's map.json: to each battle it
    holds (missions; one fought in a world of its own has none here), or to
    the one mission an older extraction named. The last one written, or
    None when there was none to write."""
    from extract import map_archive
    path = public / "maps" / name / "map.json"
    if not path.exists():
        print(f"{name}: no map.json; tools/extract.py {name} writes spawnScreen with the map", flush=True)
        return None
    world = json.loads(path.read_text())
    battles = world.get("missions")
    if battles is None:
        battles = {(world.get("mission") or {}).get("name"): world}
    posts = [post["name"] for post in world.get("commandPosts") or []]
    data = map_archive(name).read_bytes()
    screen = None
    for mission, battle in battles.items():
        if (battle.get("mission") or {}).get("world"):
            continue
        found = map_spawn_screen(data, mission, posts, locl, game)
        if found is None:
            print(f"{name}: no mission script {mission}", flush=True)
            continue
        battle["spawnScreen"] = screen = found
        teams = screen["teams"]
        print(f"{name}: {mission} shots {len(screen['cameraShots'])} sides {teams['1']['side']}/{teams['2']['side']}"
              f" objectives {len(teams['1']['objectives'])}/{len(teams['2']['objectives'])}"
              f" post vehicles {screen['postVehicles']}", flush=True)
    if screen is not None:
        path.write_text(json.dumps(world))
    return screen


def refresh_side(side, locl, public=PUBLIC):
    """Add displayName to each unit of an extracted side, and to each weapon
    of its loadout."""
    path = public / "sides" / side / "units.json"
    if not path.exists():
        print(f"side {side}: no units.json; tools/extract.py <map> --side {side} writes the names", flush=True)
        return None
    sides = json.loads(path.read_text())
    for name, unit in sides["units"].items():
        unit["displayName"] = unit_name(locl, name)
        for weapon in unit.get("weapons", []):
            weapon["displayName"] = weapon_name(locl, weapon["class"])
    path.write_text(json.dumps(sides))
    named = {name: unit["displayName"] for name, unit in sides["units"].items()}
    print(f"side {side}: {named}", flush=True)
    return sides


def refresh_common(locl, public=PUBLIC):
    """Add the screen's textures, room model, strings and rasterHud to an
    extracted common.json. The room goes after the logos in the hologram
    pack, so their records and bytes stay as they were; if they would not,
    nothing is written and common needs a full extraction."""
    from extract import HOLOGRAM_LOGOS, collect_textures, shared_archives, write_meshes, write_textures
    from zero_model import collect_models
    out = public / "common"
    path = out / "common.json"
    if not path.exists():
        print("common: no common.json; tools/extract.py writes it", flush=True)
        return None
    common = json.loads(path.read_text())
    shared = shared_archives()
    models = {}
    for archive in shared:
        collect_models(archive, models)
    files = ("holograms.bin", "holograms_index.bin")
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        holograms = write_meshes(models, HOLOGRAM_LOGOS + SPAWN_MODELS, scratch, "holograms")
        packed = {name: (scratch / name).read_bytes() for name in files}
    kept = all(holograms.get(name) == records for name, records in common.get("holograms", {}).items())
    kept = kept and all(packed[name].startswith((out / name).read_bytes()) for name in files if (out / name).exists())
    if not kept:
        print("common: the hologram logos would change; re-extract common with tools/extract.py", flush=True)
        return None
    for name in files:
        (out / name).write_bytes(packed[name])
    common["holograms"] = holograms

    textures = {}
    for archive in shared:
        collect_textures(archive, textures)
    wanted = set(SPAWN_TEXTURES) | {part["texture"] for name in SPAWN_MODELS for part in holograms.get(name, [])}
    added = write_textures(textures, wanted - set(common["textures"]), out / "textures")
    common["textures"] = dict(sorted({**common["textures"], **added}.items()))
    strings = spawn_strings(locl)
    common["strings"] = {**common.get("strings", {}), **strings}
    common["rasterHud"] = [name for name in RASTER_HUD if name in common["textures"]]
    path.write_text(json.dumps(common))
    print(f"common: room {[n for n in SPAWN_MODELS if n in holograms]} textures {sorted(added)}"
          f" strings {len(strings)} rasterHud {common['rasterHud']}", flush=True)
    return common


def finish_common(public=PUBLIC):
    """KTX2 copies of new common textures, then the 4x WebP atlases."""
    from ktx2 import encode_optional
    encode_optional([public / "common"])
    try:
        from upscale_hud import convert as upscale_hud
    except ImportError:
        print("  raster HUD skipped: install Pillow to generate enlarged lossless WebP atlases", flush=True)
    else:
        upscale_hud(public / "common")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("maps", nargs="*", help="maps whose map.json gains spawnScreen")
    parser.add_argument("--side", action="append", default=[], help="a side whose units.json gains display names (repeatable)")
    parser.add_argument("--common", action="store_true", help="add the textures, room model, strings and rasterHud to common")
    parser.add_argument("--all-extracted", action="store_true", help="every extracted map and side, and common")
    args = parser.parse_args()
    maps, sides, common = list(args.maps), list(args.side), args.common
    if args.all_extracted:
        maps += sorted(p.parent.name for p in (PUBLIC / "maps").glob("*/map.json"))
        sides += sorted(p.parent.name for p in (PUBLIC / "sides").glob("*/units.json"))
        common = True
    if not (maps or sides or common):
        parser.print_usage()
        return 1
    locl = core_localization()
    for side in dict.fromkeys(sides):
        refresh_side(side, locl)
    for name in dict.fromkeys(maps):
        refresh_map(name, locl)
    if common and refresh_common(locl) is not None:
        finish_common()
    return 0


if __name__ == "__main__":
    sys.exit(main())
