#!/usr/bin/env python3
"""The AI's mission data: what a map's mission scripts tell the AI, in the
order they say it, and what its command posts mean to the AI.

    python3 tools/zero_ai_mission.py nab2 [--side rep]

Writes public/maps/<map>/ai-mission.json (gitignored), and nothing else, so
refreshing AI data never rewrites a map's geometry. Schema: docs/ai-population.md.

Mission setters are operations, not settings: SetEnemyTeamDifficultyHard(3)
adds 3 only on Hard, a later SetSpawnDelay replaces an earlier one, and the
order of difficulty calls matters. So every AI-relevant call is kept in source
order with its arguments, for the map's mission and every other script for the
map (its historical "_h" variant and the other era), each under its own name.
A per-script team summary (faction index, unit count, classes, hero, relations)
is derived for convenience; the runtime applies the calls itself.

Command posts, as the original game reads them: the ten Value_ATK_*/Value_DEF_*
weights start at 100 in the class, class properties then instance properties
override in order (parsed with %d, so "12.5" is 12 and "x" leaves the value
alone); the six *Ban properties only ever OR their bit in, and only when the
value's atoi is positive; SpawnPath and AllyPath count only when the named
path exists in the world (names compare case-insensitively, as their hashes
do); AllyCount is parsed with %i; ValueBleed with %d (class default 100).
"""

import argparse
import hashlib
import json
import math
import re
import struct
import sys
from pathlib import Path

import zero_addon
from portable_math import atan2
from ucfb import chunks, cstr_at, iter_archives, nodes
from xform import mat4, transform_point
from zero_hash import fnv1a
from zero_locl import localize
from zero_lua import calls as lua_calls
from zero_lua import load as lua_load
from zero_lua import data_path, mission_scripts
from zero_world import instance_records, regions, spawn_paths

SCHEMA = 1
GENERATOR = "zero_ai_mission.py 1"

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import GAME  # noqa: E402
from zero_paths import PUBLIC  # noqa: E402

# Mission calls the AI reads (Lua names as the original game registers them).
DIFFICULTY_CALLS = (
    "SetPlayerTeamDifficulty", "SetPlayerTeamDifficultyEasy", "SetPlayerTeamDifficultyMedium",
    "SetPlayerTeamDifficultyHard", "SetEnemyTeamDifficulty", "SetEnemyTeamDifficultyEasy",
    "SetEnemyTeamDifficultyMedium", "SetEnemyTeamDifficultyHard",
)
AI_CALLS = frozenset(DIFFICULTY_CALLS + (
    # Population and spawning.
    "SetUnitCount", "AddUnitClass", "SetHeroClass", "ActivateBonus", "SetSpawnDelay",
    "SetReinforcementCount", "AddBleedThreshold", "SetHistorical",
    # Teams: faction (SetTeamName) and relations.
    "SetTeamName", "SetTeamAsEnemy", "SetTeamAsNeutral", "SetTeamAsFriend",
    # Mission controls.
    "SetAttackingTeam", "SetTeamAggressiveness", "SetDenseEnvironment", "SetUrbanEnvironment",
    "SetStayInTurrets", "SetStationaryWaitTime", "SetAIVehicleNotifyRadius", "SetAttackerSnipeRange",
    "SetDefenderSnipeRange", "SetAIFormationRadius", "SetAllowBlindJetJumps", "SetCarrierClass",
    "SetMinFlyHeight", "SetMaxFlyHeight", "SetMinPlayerFlyHeight", "SetMaxPlayerFlyHeight",
    "AddDeathRegion",
))

# SetTeamName's faction parser, by the lowercase-folded name hash: Alliance 1,
# Empire 2, Republic 3, CIS 4; "neutral" and one unidentified name give 0;
# anything else (including "locals" and an empty name) gives Locals, 5.
FACTION_BY_HASH = {
    fnv1a("all"): 1, fnv1a("alliance"): 1,
    fnv1a("imp"): 2, fnv1a("empire"): 2,
    fnv1a("rep"): 3, fnv1a("republic"): 3,
    fnv1a("cis"): 4,
    fnv1a("neutral"): 0, 0x269992EB: 0,
}
FACTION_NAMES = {0: "none", 1: "alliance", 2: "empire", 3: "republic", 4: "cis", 5: "locals"}
# Default hero per faction when a team's bonus asks for a hero and the mission
# named none with SetHeroClass.
DEFAULT_HEROES = {1: "all_inf_lukeskywalker", 2: "imp_inf_darthvader", 3: "rep_inf_macewindu", 4: "cis_inf_countdooku"}

# ActivateBonus(team, name): the bonus mask its name parser sets. "Hero" (8)
# makes the team field its hero; "Training" (0x80) is Advanced Training.
BONUS_BY_HASH = {
    fnv1a("sabotage"): 0x20, fnv1a("hero"): 0x08, fnv1a("training"): 0x80,
    0x5BC5C854: 0x40, 0x707C9372: 0x01, 0x7B0F9B57: 0x04, 0xA32B2CE9: 0x02, 0xDDB5D928: 0x10,
}

# Command post properties, by name.
ATK = ("Value_ATK_Alliance", "Value_ATK_Empire", "Value_ATK_Republic", "Value_ATK_CIS", "Value_ATK_Locals")
DEF = ("Value_DEF_Alliance", "Value_DEF_Empire", "Value_DEF_Republic", "Value_DEF_CIS", "Value_DEF_Locals")
BANS = (("SoldierBan", 1), ("HoverBan", 2), ("SmallBan", 4), ("MediumBan", 8), ("HugeBan", 16), ("FlyerBan", 32))
POST_PROPS = ATK + DEF + tuple(name for name, _ in BANS) + (
    "Team", "Label", "SpawnPath", "AllyPath", "AllyCount", "CaptureRegion", "ControlRegion", "KillRegion",
    "Radius", "ValueBleed", "SpawnPointCount", "SpawnPointLocation",
)
PROP_NAMES = {fnv1a(name): name for name in POST_PROPS}
# Classes that are command posts (the native class label at the root of the
# ODF chain). Command vehicles and buildings carry spawn points in the class.
COMMAND_BASES = ("commandpost", "commandwalker", "commandhover", "commandarmedanimatedbuilding", "commandflyer")
WEIGHT_DEFAULT = 100
VALUE_BLEED_DEFAULT = 100
CLASS_PROP_KEYS = set(ATK + DEF + ("ValueBleed", "SpawnPointCount", "SpawnPointLocation"))
INSTANCE_PROP_KEYS = set(ATK + DEF + tuple(name for name, _ in BANS) + (
    "SpawnPath", "AllyPath", "AllyCount", "CaptureRegion", "ControlRegion", "KillRegion", "Radius", "ValueBleed"))


# ---------------------------------------------------------------- C parsing


def atoi(text):
    """C atoi: leading space, an optional sign, then digits; 0 when none."""
    m = re.match(r"\s*([+-]?)(\d+)", text or "")
    if not m:
        return 0
    value = int(m.group(2))
    return -value if m.group(1) == "-" else value


def scan_d(text):
    """sscanf "%d": the integer, or None when no digits convert (the target
    then keeps its value)."""
    m = re.match(r"\s*([+-]?\d+)", text or "")
    return int(m.group(1)) if m else None


def scan_i(text):
    """sscanf "%i": like %d, with C's 0x (hex) and leading-0 (octal) prefixes."""
    m = re.match(r"\s*([+-]?)(0[xX][0-9a-fA-F]+|0[0-7]*|[1-9]\d*)", text or "")
    if not m:
        return None
    digits = m.group(2)
    if digits[:2].lower() == "0x":
        value = int(digits, 16)
    elif digits.startswith("0") and len(digits) > 1:
        value = int(digits, 8)
    else:
        value = int(digits)
    return -value if m.group(1) == "-" else value


def scan_f(text):
    """sscanf "%f"."""
    m = re.match(r"\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)", text or "")
    return float(m.group(1)) if m else None


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def lua_int(value):
    """A number argument as the wrappers read it: float32, truncated."""
    if value is None:
        return None
    return int(math.trunc(f32(float(value))))


def faction_of(name):
    """SetTeamName's parser: the faction index for a team name."""
    if not name:
        return 5
    return FACTION_BY_HASH.get(fnv1a(name), 5)


def bonus_mask(name):
    return BONUS_BY_HASH.get(fnv1a(name), 0) if name else 0


# ---------------------------------------------------------------- scripts


def number(value):
    """Lua numbers as JSON: integral values as ints, the rest as given."""
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return value


def ai_calls(all_calls):
    """The AI-relevant calls, in source order, each with its index among all
    of the script's calls. SetTeamName and ActivateBonus also carry what the
    original game's hash parsers make of their names (faction, bonus mask)."""
    out = []
    for at, (name, args) in enumerate(all_calls):
        if name in AI_CALLS:
            call = {"at": at, "name": name, "args": [number(a) for a in args]}
            text = args[1] if len(args) > 1 and isinstance(args[1], str) else None
            if name == "SetTeamName":
                call["faction"] = faction_of(text)
            elif name == "ActivateBonus":
                call["mask"] = bonus_mask(text)
            out.append(call)
    return out


def team_summary(calls):
    """What the calls leave each team with, applied in order the way the
    original game's setters apply them. Heroes are not appended here: whether a
    team fields its hero depends on the match's hero option and bonuses."""
    teams = {}

    def team(n):
        t = lua_int(n)
        if t is None or not 0 <= t <= 7:
            return None
        if t not in teams:
            teams[t] = {
                "name": None, "faction": 0, "units": 0,
                # Teams 3-7 start with effectively unlimited reinforcements.
                "reinforcements": 0 if t <= 2 else 0x7FFFFFFF,
                "classes": [], "hero": None, "bonus": 0, "aggressiveness": 1.0,
                "bleed": [], "relations": {},
            }
        return teams[t]

    for call in calls:
        name, args = call["name"], call["args"] + [None] * 4
        if name == "SetTeamName" and team(args[0]) is not None:
            t = team(args[0])
            t["name"] = args[1] if isinstance(args[1], str) else None
            t["faction"] = faction_of(t["name"])
        elif name == "AddUnitClass" and team(args[0]) is not None and isinstance(args[1], str):
            t = team(args[0])
            if len(t["classes"]) < 12:  # a team holds at most 12 classes
                count = lua_int(args[2]) if isinstance(args[2], (int, float)) else 0
                t["classes"].append([args[1], count])
        elif name == "SetUnitCount" and team(args[0]) is not None and isinstance(args[1], (int, float)):
            team(args[0])["units"] = lua_int(args[1])
        elif name == "SetReinforcementCount" and team(args[0]) is not None and isinstance(args[1], (int, float)):
            team(args[0])["reinforcements"] = lua_int(args[1])
        elif name == "SetHeroClass" and team(args[0]) is not None and isinstance(args[1], str):
            team(args[0])["hero"] = args[1]
        elif name == "ActivateBonus" and team(args[0]) is not None and isinstance(args[1], str):
            team(args[0])["bonus"] |= bonus_mask(args[1])
        elif name == "SetTeamAggressiveness" and team(args[0]) is not None and isinstance(args[1], (int, float)):
            team(args[0])["aggressiveness"] = f32(float(args[1]))
        elif name == "AddBleedThreshold" and team(args[0]) is not None:
            if isinstance(args[1], (int, float)) and isinstance(args[2], (int, float)):
                team(args[0])["bleed"].append([lua_int(args[1]), f32(float(args[2]))])
        elif name in ("SetTeamAsFriend", "SetTeamAsEnemy", "SetTeamAsNeutral") and team(args[0]) is not None:
            other = lua_int(args[1]) if isinstance(args[1], (int, float)) else None
            if other is not None and 0 <= other <= 7:
                team(args[0])["relations"][str(other)] = {"SetTeamAsFriend": 1, "SetTeamAsEnemy": -1}.get(name, 0)
    for t in teams.values():
        t["factionName"] = FACTION_NAMES[t["faction"]]
        # The original game sorts the thresholds (lowest first) before play.
        t["bleed"].sort(key=lambda row: row[0])
    return {str(k): v for k, v in sorted(teams.items())}


def script_record(name, all_calls):
    calls = ai_calls(all_calls)
    delay = None
    for call in calls:
        if call["name"] == "SetSpawnDelay" and len(call["args"]) >= 2:
            delay = call["args"][:2]
    sides = []
    for call_name, args in all_calls:
        if call_name == "ReadDataFile" and args and isinstance(args[0], str) and data_path(args[0]).upper().startswith("SIDE\\"):
            sides.append(args[0].split("\\")[-1].rsplit(".", 1)[0].lower())
    return {
        "variant": "historical" if name.endswith("_h") else "regular",
        "historical": any(call["name"] == "SetHistorical" for call in calls),
        "sides": sides,
        "callCount": len(all_calls),
        "calls": calls,
        "teams": team_summary(calls),
        # The last SetSpawnDelay wins; without one the manager keeps 15 / 0.5.
        "spawnDelay": delay or [15, 0.5],
        "difficulty": [call for call in calls if call["name"] in DIFFICULTY_CALLS],
    }


def map_scripts(scripts, name):
    """The map's scripts: the era battles and their historical variants."""
    return sorted(k for k in scripts if re.fullmatch(re.escape(name) + r"[a-z](_h)?", k))


def pick_mission(records, side, preferred=None):
    """The map's mission as the port loads it: map.json's choice when given,
    else the regular battle whose sides include the player's side."""
    if preferred in records:
        return preferred
    regular = sorted(k for k, r in records.items() if r["variant"] == "regular")
    for name in regular:
        if side in records[name]["sides"]:
            return name
    return regular[0] if regular else (sorted(records)[0] if records else None)


# ---------------------------------------------------------------- posts


def class_records(archives):
    """Entity classes with their properties as raw (hash, value) pairs in file
    order, the map's own first, as the game resolves them."""
    out = {}
    for data in archives:
        for body, end in nodes(data, b"entc"):
            typ = base = None
            props = []
            for tag, b, e in chunks(data, body, end):
                if tag == b"TYPE":
                    typ = cstr_at(data, b, e)
                elif tag == b"BASE":
                    base = cstr_at(data, b, e)
                elif tag == b"PROP" and e - b >= 4:
                    props.append((struct.unpack_from("<I", data, b)[0], cstr_at(data, b + 4, e)))
            if typ and typ not in out:
                out[typ] = {"base": base, "props": props}
    return out


def native_base(classes, name):
    """The native class label at the root of a class's ODF chain."""
    seen = set()
    while name and name in classes and name not in seen:
        seen.add(name)
        base = classes[name]["base"]
        if not base or base not in classes:
            return (base or name).lower()
        name = base
    return (name or "").lower()


def named(props, wanted):
    """(name, value) for the properties we know by name, in order."""
    return [(PROP_NAMES[key], value) for key, value in props if key in PROP_NAMES and PROP_NAMES[key] in wanted]


def lookup(table, name):
    """A world object by name, as the engine finds it: by the case-folded hash."""
    if not name:
        return None
    key = fnv1a(name)
    for k in table:
        if fnv1a(k) == key:
            return k
    return None


def region_center(region):
    m = region["matrix"]
    return [m[12], m[13], m[14]]


def yaw_of(matrix):
    """The yaw of a column-major 4x4 transform's forward (+Z) axis."""
    return atan2(matrix[8], matrix[10])


def post_record(order, record, cls_name, classes, paths, world_regions):
    cls = classes.get(cls_name) or {"base": None, "props": []}
    atk = [WEIGHT_DEFAULT] * 6
    defs = [WEIGHT_DEFAULT] * 6
    value_bleed = VALUE_BLEED_DEFAULT
    overrides = []
    spawn_points = []
    allocated = 0
    # Class properties first (the class parser), then the instance's.
    for name, value in named(cls["props"], CLASS_PROP_KEYS):
        if name in ATK or name in DEF:
            parsed = scan_d(value)
            overrides.append(["class", name, value])
            if parsed is not None:
                (atk if name in ATK else defs)[(ATK if name in ATK else DEF).index(name) + 1] = parsed
        elif name == "ValueBleed":
            parsed = scan_d(value)
            value_bleed = parsed if parsed is not None else value_bleed
        elif name == "SpawnPointCount":
            parsed = scan_i(value) or 0
            allocated = parsed
            spawn_points = []
        elif name == "SpawnPointLocation" and allocated > 0:
            numbers = [scan_f(v) for v in value.split()[:4]]
            if numbers and numbers[0] is not None:
                spawn_points.append([n if n is not None else 0.0 for n in numbers + [0.0] * (4 - len(numbers))])
    props = record["props"]
    bans = 0
    ban_props = []
    refs = {}
    ally_count = 0
    radius = 0.0
    for name, value in named(props, INSTANCE_PROP_KEYS):
        if name in ATK or name in DEF:
            parsed = scan_d(value)
            overrides.append(["instance", name, value])
            if parsed is not None:
                (atk if name in ATK else defs)[(ATK if name in ATK else DEF).index(name) + 1] = parsed
        elif name == "ValueBleed":
            parsed = scan_d(value)
            value_bleed = parsed if parsed is not None else value_bleed
        elif name.endswith("Ban"):
            ban_props.append([name, value])
            if atoi(value) > 0:  # OR only: a zero never clears a bit
                bans |= dict(BANS)[name]
        elif name == "AllyCount":
            parsed = scan_i(value)
            ally_count = parsed if parsed is not None else ally_count
        elif name == "Radius":
            parsed = scan_f(value)
            radius = parsed if parsed is not None else radius
        else:
            refs[name] = value  # the last of each reference wins
    matrix = mat4(record["xfrm"])

    def path(name):
        if name is None:
            return None
        found = lookup(paths, name)
        return {"name": name, "present": found is not None, "path": found,
                "points": paths[found] if found is not None else []}

    def region(name):
        if name is None:
            return None
        by_name = {r["name"]: r for r in world_regions}
        found = lookup(by_name, name)
        out = {"name": name, "present": found is not None}
        if found is not None:
            r = by_name[found]
            out.update({"type": r["type"], "matrix": r["matrix"], "size": r["size"], "center": region_center(r)})
        return out

    team_values = [v for k, v in named(props, {"Team"})]
    yaw = yaw_of(matrix)
    world_points = [list(transform_point(matrix, p[:3])) + [yaw + math.radians(p[3])] for p in spawn_points]
    return {
        "name": record["name"],
        "order": order,
        "class": cls_name,
        "base": native_base(classes, cls_name),
        "team": atoi(team_values[-1]) if team_values else 0,
        "position": [matrix[12], matrix[13], matrix[14]],
        "yaw": yaw,
        # Index 0 is the class default (no property names it); 1 Alliance,
        # 2 Empire, 3 Republic, 4 CIS, 5 Locals.
        "values": {"atk": atk, "def": defs, "overrides": overrides},
        "valueBleed": value_bleed,
        "bans": bans,
        "banProps": ban_props,
        "spawnPath": path(refs.get("SpawnPath")),
        "allyPath": path(refs.get("AllyPath")),
        "allyCount": ally_count,
        "classSpawnPoints": len(spawn_points),
        "classSpawnLocations": spawn_points,
        "classSpawnWorld": world_points,
        "captureRegion": region(refs.get("CaptureRegion")),
        "controlRegion": region(refs.get("ControlRegion")),
        "killRegion": region(refs.get("KillRegion")),
        "radius": radius,
        "properties": [[name, value] for name, value in named(props, INSTANCE_PROP_KEYS | {"Team", "Label"})],
    }


def command_posts(records, classes, paths, world_regions):
    """Placed command posts in world-list order (the order the world creates
    them, which is the order the AI's post iterator walks)."""
    out = []
    for record in records:
        cls = record["class"]
        if cls not in classes or native_base(classes, cls) not in COMMAND_BASES:
            continue
        out.append(post_record(len(out), record, cls, classes, paths, world_regions))
    return out


def command_classes(classes):
    """Command vehicle and building classes that carry spawn points: they
    become command posts when spawned (an AT-AT, an MTT)."""
    out = {}
    for name, cls in sorted(classes.items()):
        base = native_base(classes, name)
        if base not in COMMAND_BASES or base == "commandpost":
            continue
        rec = post_record(0, {"name": name, "props": [], "xfrm": [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]},
                          name, classes, {}, [])
        if rec["classSpawnPoints"]:
            out[name] = {"base": base, "values": rec["values"], "valueBleed": rec["valueBleed"],
                         "spawnPoints": rec["classSpawnLocations"]}
    return out


# ---------------------------------------------------------------- names


def bot_names(locl, world_name):
    """Bot name lists by faction (firstnames/lastnames, up to 64 each), as the
    teams load them from localization. Drawing a name consumes the shared
    random generator, so the lists' sizes matter."""
    out = {}
    for faction, side in ((0, "neu"), (1, "all"), (2, "imp"), (3, "rep"), (4, "cis")):
        lists = {}
        for kind, key in (("first", "firstnames"), ("last", "lastnames")):
            found = []
            for i in range(1, 65):
                text = localize(locl, f"common.sides.{side}.{key}.{i:02d}")
                if not text:
                    break
                found.append(text)
            lists[kind] = found
        out[str(faction)] = lists
    out["local"] = localize(locl, f"level.{world_name}.local")
    return out


# ---------------------------------------------------------------- main


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def world_data(data):
    records, world_regions, world_name = [], [], None
    for start, end in iter_archives(data):
        for tag, b, e in chunks(data, start, end):
            if tag != b"wrld":
                continue
            records.extend(instance_records(data, b, e, name_of=lambda key: key))
            world_regions.extend(regions(data, b, e))
            for t, cb, ce in chunks(data, b, e):
                if t == b"NAME" and world_name is None:
                    world_name = cstr_at(data, cb, ce)
    return records, world_regions, world_name


def build(name, side="rep", game=GAME, public=PUBLIC):
    # The game's archives, or an add-on's for its map, armies and missions
    # (tools/zero_addon.py).
    map_path = zero_addon.map_archive(name, game)
    common_path = game / "Common" / "ENG" / "common.lvl"
    core_path = game / "core.lvl"
    if not map_path.exists():
        raise FileNotFoundError(f"no archive for {name}: expected {map_path}")
    scripts = zero_addon.missions(game)
    records = {s: script_record(s, lua_calls(lua_load(scripts[s]))) for s in map_scripts(scripts, name)}
    # mission.lvl, and an add-on's where it holds this map's missions.
    mission_paths = [path for path, root in zero_addon.mission_files(game)
                     if root is None or set(mission_scripts(path.read_bytes())) & set(records)]
    preferred = None
    manifest = public / "maps" / name / "map.json"
    if manifest.exists():
        preferred = (json.loads(manifest.read_text()).get("mission") or {}).get("name")
    mission = pick_mission(records, side, preferred)

    data = map_path.read_bytes()
    shared = [p.read_bytes() for p in (common_path, core_path) if p.exists()]
    classes = class_records([data, *shared])
    records_world, world_regions, world_name = world_data(data)
    paths = spawn_paths(data)
    posts = command_posts(records_world, classes, paths, world_regions)

    side_names = sorted({s for r in records.values() for s in r["sides"]})
    side_paths = [zero_addon.side_archive(s, game) for s in side_names]
    side_classes = class_records([path.read_bytes() for path in side_paths if path.exists()])
    locl = zero_addon.localization(game)

    sources = {}
    for path in [*mission_paths, map_path, common_path, core_path, *side_paths]:
        if path.exists():
            sources[zero_addon.source_name(path, game)] = sha256(path)
    variants = sorted(k for k in records if mission and k != mission and k.startswith(mission))
    return {
        "schema": SCHEMA,
        "generator": GENERATOR,
        "map": name,
        "world": world_name,
        "mission": mission,
        "variants": variants,
        "sources": sources,
        "factions": FACTION_NAMES,
        "defaultHeroes": {str(k): v for k, v in DEFAULT_HEROES.items()},
        "scripts": records,
        "posts": posts,
        "commandClasses": command_classes(side_classes),
        "names": bot_names(locl, world_name or name),
    }


def write(name, side="rep", game=GAME, public=PUBLIC):
    """Build and write public/maps/<map>/ai-mission.json; returns the data.
    extract.py calls this after writing map.json (whose mission it follows)."""
    out = build(name, side, game, public)
    folder = public / "maps" / name
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "ai-mission.json").write_text(json.dumps(out, indent=1))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("map")
    parser.add_argument("--side", default="rep", help="the player's side, to pick the map's mission when map.json is absent")
    args = parser.parse_args()
    try:
        out = write(args.map, args.side)
    except FileNotFoundError as error:
        print(error, file=sys.stderr)
        return 1
    teams = out["scripts"].get(out["mission"], {}).get("teams", {})
    print(f"{args.map}: mission {out['mission']} variants {out['variants']} scripts {sorted(out['scripts'])}")
    print(f"  teams {[(t, v['faction'], v['units'], [c[1] for c in v['classes']]) for t, v in teams.items()]}")
    print(f"  posts {[(p['name'], p['team'], p['values']['atk'][1:], p['values']['def'][1:], p['bans']) for p in out['posts']]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
