"""Feature inventory of the retail game against what the port prepared.

Reads the Steam archives (tools/zero_paths.GAME) and the prepared output
(BFE_CONTENT_DIR, or public/) and writes one JSON inventory plus a short
report, so an audit of "what does the original do that the engine ignores"
can be rerun after every extractor or engine change:

- every vehicle class (flyer, hover, walker, command vehicles, animals,
  droidekas) with its seats, each seat's weapons, aimers, fire points and
  pilot keys;
- every armed building, power-up droid, command post, vehicle pad and the
  other placed classes (buildings, traps, godrays, dust, rumble, ambience,
  animated props) per map, with instance names and the class-name case
  mismatches the retail lookup tolerates and a case-sensitive one drops;
- every weapon and ordnance class per side with its base and the keys that
  give it a special behaviour (lock-on, charge levels, heat, zoom, sticking,
  homing, bouncing, cables);
- every mission script's calls (teams, heroes, fly heights, death regions,
  bonuses, birds and fish, stay-in-turrets);
- what the prepared data holds for each of those (live objects, vehicle
  classes, pad choices, posts, sides, eras) and the gaps between the two.

Usage: python3 tools/audit-features.py [--out extract/audit/features.json]
                                        [--maps bes1 hot1] [--quiet]
Only the inventory is written; docs/feature-audit.md holds the reading.
Nothing from the archives is copied into the repository: the default output
is under extract/, which is gitignored.
"""
import argparse
import collections
import json
import os
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ucfb import chunks, cstr_at, iter_archives, nodes  # noqa: E402
from zero_addon import add_ons, child, missions as all_missions  # noqa: E402
from zero_hash import fnv1a  # noqa: E402
from zero_lua import calls as lua_calls, load as lua_load, mission_scripts  # noqa: E402,F401
from zero_odf import NAMES as ODF_NAMES  # noqa: E402
from zero_paths import GAME, PUBLIC, ROOT  # noqa: E402
from zero_vehicle import NAMES as VEHICLE_NAMES  # noqa: E402
from zero_weapons import NAMES as WEAPON_NAMES  # noqa: E402
from zero_world import instance_records, regions as world_regions  # noqa: E402

# Property names the other tools do not know, each checked against the
# archives' hashes (tools/zero_hash.fnv1a). The mod tools document these
# ODF keys; the values seen in the data agree with the names.
EXTRA_KEYS = """
ClassLabel MinSpeed MidSpeed BankAngle BankFilter LevelFilter TakeoffHeight TakeoffSpeed TakeoffTime
LandingSpeed LandingTime LandedHeight PCPitchRate StrafeRollAngle VelocitySpring VelocityDamp PitchDamp
BarrelNodeName BarrelRecoil BarrelLength NextBarrel CockpitTension WalkerLegPair StompEffect
SpawnPointCount NoCombatInterrupt TargetableCollision TerrainCollision LowResModel IdleAnimation
DamageStartPercent DamageStopPercent DamageEffect DamageAttachPoint DamageEffectScale DamageInheritVelocity
ExplosionOffset ExplosionCritical FinalExplosion FinalExplosionOffset DeathEffect
ChunkBounciness ChunkStickiness ChunkUpFactor ChunkSmokeEffect ChunkStartDistance
EngineSound TakeoffSound LandSound TurnOnSound TurnOffSound TurretYawSound TurretYawSoundPitch
TurretPitchSound TurretPitchSoundPitch FoleyFXGroup VehicleCollisionSound CollisionSound DeathSound
HurtSound AmbientSound MusicDelay FireSound ChargeSound ReloadSound ChangeModeSound WeaponChangeSound
JumpSound OverheatSound OverheatStopSound SoundProperty AttachEffect ImpactEffect CableLength
CaptureRegion ControlRegion SpawnPath AllyPath AllyCount KillRegion Team Label ValueBleed Radius Layer
SpawnTime DecayTime ControlZone ExpireTimeEnemy ExpireTimeField
""".split()
for _faction in ("All", "CIS", "Imp", "Rep", "Loc", "His"):
    for _role in ("ATK", "DEF"):
        EXTRA_KEYS.append(f"Class{_faction}{_role}")
NAMES = {**ODF_NAMES, **WEAPON_NAMES, **VEHICLE_NAMES, **{fnv1a(k): k for k in EXTRA_KEYS}}

VEHICLE_BASES = ("flyer", "hover", "walker", "commandwalker", "commandhover", "commandflyer", "walkerdroid", "carrier")
LIVE_BASES = ("armedbuilding", "powerupstation")
# Placed classes the retail game gives behaviour beyond a static model.
SPECIAL_BASES = ("commandpost", "vehiclespawn", "vehiclepad", "building", "destructablebuilding", "trap", "godray",
                 "dusteffect", "rumbleeffect", "animatedprop", "door", "soundambiencestatic", "soundambiencestreaming",
                 "powerupitem", "hologram", "light")
# Placed (or AttachOdf-attached) classes a player or a bot can set off,
# use, damage or be hurt by: the gameplay objects, as against the cosmetic
# ones (godray, dusteffect, rumbleeffect, hologram, light, sound emitters,
# foliage). Each must come out of the extractor as a live instance in the
# map's props.json (tools/extract.py LIVE_BASES, destructible posts).
INTERACTIVE_BASES = ("armedbuilding", "powerupstation", "animatedprop", "door", "trap", "powerupitem")
COSMETIC_BASES = ("godray", "dusteffect", "rumbleeffect", "hologram", "light", "soundambiencestatic", "soundambiencestreaming",
                  "grasspatch", "leafpatch")
SEAT_KEYS = ("WeaponName", "WeaponAmmo", "WeaponChannel", "WeaponSection", "AimerNodeName", "FirePointName", "BarrelNodeName",
             "TurretNodeName", "PilotPosition", "PilotAnimation", "Pilot9Pose", "YawLimits", "PitchLimits", "AimerYawLimits",
             "AimerPitchLimits", "FireOutsideLimits", "NextAimer", "EyePointOffset", "FirstPerson", "FirstPersonFOV",
             "ThirdPersonFOV", "TurnRate", "PitchRate", "MaxTurnSpeed", "MaxPitchSpeed", "f2aa4a90", "2b60e217")
HULL_KEYS = ("Label", "ClassLabel", "VehicleType", "MaxHealth", "GeometryName", "AnimationName", "FirstPerson", "MinSpeed",
             "MidSpeed", "MaxSpeed", "ForwardSpeed", "ReverseSpeed", "StrafeSpeed", "SetAltitude", "TakeoffHeight", "LandedHeight",
             "SpawnPointCount", "ValueBleed", "WaterDamageInterval", "WaterDamageAmount", "FinalExplosion", "ExplosionCritical",
             "DamageEffect", "AISizeType", "BallRadius", "DecayTime", "SpawnTime", "ScanningRange")
WEAPON_KEYS = ("Label", "OrdnanceName", "RoundsPerClip", "ShotDelay", "ReloadTime", "LockOnRange", "LockOnAngle", "LockOffAngle",
               "LockTime", "AutoAimSize", "NextCharge", "HeatPerShot", "HeatThreshold", "ZoomMax", "SniperScope", "SalvoCount",
               "ShotsPerSalvo", "TriggerSingle", "MaxPressedTime", "MaxItems", "AllowDestruct", "StrikeOrdnanceName",
               "TargetVehicle", "TargetPerson", "TargetBuilding", "MaxShield", "SwingTime", "CanDeflect")
ORDNANCE_KEYS = ("Velocity", "LifeSpan", "Gravity", "Rebound", "TurnRate", "MaxDamage", "Damage", "StickPerson", "StickVehicle",
                 "StickBuilding", "StickTerrain", "ExplosionTrigger", "ExplosionExpire", "TriggerRadius", "CableLength",
                 "ConeLength", "MaxTargets", "LaserLength", "GeometryName")
# The mission calls the engine or the extractor consumes (verified by
# grepping src/ and tools/; the audit re-checks when the repository is there).
MISSION_CALLS = ("ActivateBonus", "AddBleedThreshold", "AddCameraShot", "AddDeathRegion", "AddMissionObjective", "AddUnitClass",
                 "AddWalkerType", "OpenAudioStream", "ReadDataFile", "SetAIVehicleNotifyRadius", "SetAllowBlindJetJumps",
                 "SetAmbientMusic", "SetAttackingTeam", "SetBirdType", "SetBleedingVoiceOver", "SetDefeatMusic",
                 "SetDefenderSnipeRange", "SetDenseEnvironment", "SetEnemyTeamDifficultyEasy", "SetEnemyTeamDifficultyMedium",
                 "SetEnemyTeamDifficultyHard", "SetFishType", "SetHeroClass", "SetHistorical", "SetLowReinforcementsVoiceOver",
                 "SetMaxFlyHeight", "SetMaxPlayerFlyHeight", "SetMinFlyHeight", "SetMinPlayerFlyHeight", "SetOutOfBoundsVoiceOver",
                 "SetPlanetaryBonusVoiceOver", "SetReinforcementCount", "SetSoundEffect", "SetSpawnDelay", "SetStayInTurrets",
                 "SetTeamAggressiveness", "SetTeamAsEnemy", "SetTeamAsFriend", "SetTeamIcon", "SetTeamName", "SetUnitCount",
                 "SetVictoryMusic", "SetMemoryPoolSize", "ClearWalkers", "SetBirdFlockMinHeight", "SetNumBirdTypes", "SetNumFishTypes")
# Whole-word call names; a prefix match ("SetEnemyTeamDifficulty") counts.
CALL_PREFIXES = {"SetEnemyTeamDifficultyEasy": "SetEnemyTeamDifficulty", "SetEnemyTeamDifficultyMedium": "SetEnemyTeamDifficulty",
                 "SetEnemyTeamDifficultyHard": "SetEnemyTeamDifficulty", "SetPlayerTeamDifficultyEasy": "SetPlayerTeamDifficulty",
                 "SetPlayerTeamDifficultyMedium": "SetPlayerTeamDifficulty", "SetPlayerTeamDifficultyHard": "SetPlayerTeamDifficulty"}


def property_name(key):
    return NAMES.get(key, "%08x" % key)


# ---------------------------------------------------------------- archives


def classes_of(data, tag=b"entc"):
    """Every class of one chunk kind in an archive: name -> base, (name, value)
    properties in file order (the whole ODF chain flattened)."""
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
                props.append((property_name(struct.unpack_from("<I", data, b)[0]), cstr_at(data, b + 4, e)))
        if typ:
            out.setdefault(typ, {"base": base, "props": props})
    return out


def world_instances(data):
    """Placed objects and regions of a map archive."""
    records, regions = [], []
    for start, end in iter_archives(data):
        for tag, b, e in chunks(data, start, end):
            if tag == b"wrld":
                records.extend(instance_records(data, b, e, name_of=property_name))
                regions.extend(world_regions(data, b, e))
    return records, regions


def native_base(classes, name):
    """The engine class at the root of a class's ODF chain (lower case)."""
    seen = set()
    while name and name in classes and name not in seen:
        seen.add(name)
        base = classes[name]["base"]
        if not base or base not in classes:
            return (base or name).lower()
        name = base
    return (name or "").lower()


# ---------------------------------------------------------------- classes


def seats_of(props):
    """A vehicle or armed building's control sections: the hull (BODY), then
    each turret, with the weapons (and their aimers and fire points) and the
    seat keys each one sets. The weapon list is in file order; a weapon
    section (WeaponSection) opens a new weapon, as does a WeaponName after
    the previous weapon's aimers."""
    seats = []
    seat = None

    def open_seat(name):
        nonlocal seat
        seat = {"name": name, "weapons": [], "keys": {}}
        seats.append(seat)

    for key, value in props:
        if key in ("ControlSection", "BuildingSection", "4cbe283c", "1cd9f762"):
            if seat is None or seat["name"] != value:
                open_seat(value)
            continue
        if key == "TurretSection":
            # The turret's weapons follow its own section marker.
            for existing in seats:
                if existing["name"] == value:
                    seat = existing
            continue
        if seat is None:
            open_seat("BODY")
        if key == "WeaponSection":
            seat["weapons"].append({"section": value, "aimers": [], "firePoints": [], "barrels": []})
        elif key == "WeaponName":
            if not seat["weapons"] or seat["weapons"][-1].get("name"):
                seat["weapons"].append({"aimers": [], "firePoints": [], "barrels": []})
            seat["weapons"][-1]["name"] = value
        elif key in ("WeaponAmmo", "WeaponChannel") and seat["weapons"]:
            seat["weapons"][-1][key[6:].lower()] = value
        elif key == "AimerNodeName" and seat["weapons"]:
            seat["weapons"][-1]["aimers"].append(value)
        elif key == "FirePointName" and seat["weapons"]:
            seat["weapons"][-1]["firePoints"].append(value)
        elif key == "BarrelNodeName" and seat["weapons"]:
            seat["weapons"][-1]["barrels"].append(value)
        elif key in SEAT_KEYS:
            seat["keys"].setdefault(key, []).append(value)
    for s in seats:
        s["rider"] = bool(s["keys"].get("PilotAnimation") or s["keys"].get("Pilot9Pose"))
        s["enclosed"] = not s["rider"]
    return seats


def picked(props, keys):
    out = {}
    for key, value in props:
        if key in keys:
            out.setdefault(key, []).append(value)
    return {k: (v if len(v) > 1 else v[0]) for k, v in out.items()}


def vehicle_record(name, cls, source):
    return {"class": name, "base": cls["base"], "source": source, "hull": picked(cls["props"], HULL_KEYS), "seats": seats_of(cls["props"])}


def weapon_record(name, cls, ordnance, entities, source):
    d = dict(cls["props"])
    ord_name = d.get("OrdnanceName", "")
    ord_cls = ordnance.get(ord_name)
    ent = entities.get(ord_name) if not ord_cls else None
    return {
        "class": name, "base": cls["base"], "source": source, "label": d.get("Label", ""),
        "keys": picked(cls["props"], WEAPON_KEYS),
        "ordnance": {"class": ord_name, "base": ord_cls["base"], "keys": picked(ord_cls["props"], ORDNANCE_KEYS)} if ord_cls
        else {"class": ord_name, "base": "entc:" + (ent["base"] or "") if ent else None,
              "keys": picked(ent["props"], ORDNANCE_KEYS) if ent else {}} if ord_name else None,
    }


def soldier_record(name, cls, source):
    d = dict(cls["props"])
    weapons = []
    for n in range(1, 9):
        w = d.get(f"WeaponName{n}")
        if w:
            weapons.append({"slot": n, "weapon": w, "ammo": d.get(f"WeaponAmmo{n}"), "channel": d.get(f"WeaponChannel{n}")})
    return {"class": name, "source": source, "label": d.get("Label", ""), "unitType": d.get("UnitType"),
            "health": d.get("MaxHealth"), "jet": "JetJump" in d, "weapons": weapons}


# ---------------------------------------------------------------- missions


def call_summary(chunk):
    """A mission script's calls as {name: [args...]} lists, in file order,
    the noisy ones counted only."""
    out = collections.OrderedDict()
    counts = collections.Counter()
    for name, args in lua_calls(lua_load(chunk)):
        counts[name] += 1
        if name in ("SetMemoryPoolSize", "AddWalkerType", "ClearWalkers", "AddBleedThreshold", "AddCameraShot"):
            continue
        out.setdefault(name, []).append([a for a in args])
    return {"calls": out, "counts": dict(counts)}


def engine_consumes(call, root=ROOT):
    """Whether the engine (src/) or the extractor (tools/) reads a mission
    call, judged by its name appearing in either; None when the repository
    is not there."""
    needle = CALL_PREFIXES.get(call, call)
    found = False
    for folder in ("src", "tools"):
        base = root / folder
        if not base.exists():
            return None
        for path in base.rglob("*"):
            if path.name == Path(__file__).name:
                continue  # this audit names every call itself
            if path.suffix in (".js", ".mjs", ".py") and "__pycache__" not in path.parts:
                try:
                    if needle in path.read_text(errors="ignore"):
                        return True
                except OSError:
                    continue
    return found


# ---------------------------------------------------------------- maps


def map_inventory(name, data, classes, scripts):
    records, regions = world_instances(data)
    lower = {k.lower(): k for k in classes}
    by_base = collections.defaultdict(list)
    mismatches = {}
    unknown = collections.Counter()
    for r in records:
        exact = classes.get(r["class"])
        ci = classes.get(lower.get(r["class"].lower(), ""))
        if not exact and ci:
            mismatches[r["class"]] = lower[r["class"].lower()]
        if not exact and not ci:
            unknown[r["class"]] += 1
        cls_name = r["class"] if exact else lower.get(r["class"].lower(), r["class"])
        base = native_base(classes, cls_name) if (exact or ci) else "?"
        props = dict(r["props"])
        entry = {"name": r["name"], "class": r["class"], "resolved": cls_name, "base": base,
                 "position": [round(x, 2) for x in r["xfrm"][9:12]], "props": r["props"]}
        if base == "armedbuilding" or base == "powerupstation":
            d = dict(classes[cls_name]["props"])
            entry["label"] = d.get("Label")
            entry["geometry"] = d.get("GeometryName")
            entry["team"] = props.get("Team", "0")
            entry["seats"] = seats_of(classes[cls_name]["props"]) if base == "armedbuilding" else []
            entry["weapons"] = [w.get("name") for s in entry["seats"] for w in s["weapons"]]
        elif base == "commandpost":
            d = dict(classes[cls_name]["props"])
            entry["team"] = props.get("Team", "0")
            entry["label"] = props.get("Label", d.get("Label"))
            entry["destructible"] = "MaxHealth" in d
            entry["maxHealth"] = d.get("MaxHealth")
            entry["wreck"] = d.get("DestroyedGeometryName")
            entry["captureRegion"] = props.get("CaptureRegion")
            entry["killRegion"] = props.get("KillRegion")
            entry["allyPath"] = props.get("AllyPath")
            entry["animated"] = "AnimationName" in d
        elif base == "vehiclespawn":
            entry["team"] = props.get("Team", "0")
            entry["post"] = props.get("ControlZone")
            entry["choices"] = {k: v for k, v in r["props"] if k.startswith("Class")}
        by_base[base].append(entry)
    counts = collections.Counter(r["class"] for r in records)
    interactive, cosmetic = interactive_census(records, classes)
    mission = {s: call_summary(scripts[s]) for s in scripts if s.startswith(name)}
    death_regions = sorted({a[0] for m in mission.values() for a in m["calls"].get("AddDeathRegion", []) if a and a[0]})
    region_types = collections.Counter(r.get("type", "?") for r in regions)
    return {
        "map": name, "instances": len(records),
        "classCounts": dict(sorted(counts.items())),
        "caseMismatches": mismatches, "unknownClasses": dict(unknown),
        "turrets": by_base.get("armedbuilding", []),
        "droids": by_base.get("powerupstation", []),
        "posts": by_base.get("commandpost", []),
        "pads": by_base.get("vehiclespawn", []),
        "special": {b: [{"name": e["name"], "class": e["class"], "resolved": e["resolved"]} for e in by_base[b]]
                    for b in SPECIAL_BASES if b in by_base and b not in ("commandpost", "vehiclespawn")},
        "regions": {"count": len(regions), "types": dict(region_types),
                    "names": sorted({r.get("name", "") for r in regions if r.get("name")})[:200]},
        "deathRegions": death_regions,
        "interactive": interactive,
        "cosmetic": cosmetic,
        "missions": mission,
    }


def interactive_census(records, classes):
    """The map's gameplay objects and its cosmetic ones, by resolved class:
    {class: {"base", "placed", "attached"}}. Attached means hung on a placed
    class's hardpoint by AttachOdf (the sarlacc's tentacles, Jabba, the
    rancor), which the game creates as it does a placed one. A command post
    with a MaxHealth (a destructible objective) counts as interactive."""
    lower = {k.lower(): k for k in classes}
    interactive, cosmetic = {}, {}

    def count(kind, how):
        name = kind if kind in classes else lower.get(str(kind).lower())
        if not name:
            return
        base = native_base(classes, name)
        props = dict(classes[name]["props"])
        if base in INTERACTIVE_BASES or (base == "commandpost" and "MaxHealth" in props):
            table = interactive
        elif base in COSMETIC_BASES:
            table = cosmetic
        else:
            return
        entry = table.setdefault(name, {"base": base, "placed": 0, "attached": 0})
        entry[how] += 1

    for r in records:
        count(r["class"], "placed")
        name = r["class"] if r["class"] in classes else lower.get(r["class"].lower())
        for key, value in (classes[name]["props"] if name else []):
            if key == "AttachOdf" and isinstance(value, str) and value.split():
                count(value.split()[0], "attached")
    return dict(sorted(interactive.items())), dict(sorted(cosmetic.items()))


# ---------------------------------------------------------------- prepared


def prepared_inventory(public, name):
    """What the prepared output holds for a map: live objects by class,
    vehicle classes with seat and weapon counts, pad choices, posts, sides."""
    folder = public / "maps" / name
    if not (folder / "map.json").exists():
        return None
    out = {"map": name}
    try:
        manifest = json.loads((folder / "map.json").read_text())
    except (OSError, ValueError):
        return None
    # Every battle the map holds (map.json missions), or an older import's one.
    battles = manifest.get("missions")
    if battles is None:
        battles = {(manifest.get("mission") or {}).get("name"): manifest}
    battles = {m: b for m, b in battles.items() if m and not (b.get("mission") or {}).get("world")}
    out["mission"] = sorted(battles)
    out["sides"] = sorted({s for b in battles.values() for s in (b.get("mission") or {}).get("sides") or []})
    out["deathRegions"] = len(manifest.get("deathRegions") or [])
    out["water"] = bool(manifest.get("water"))
    out["posts"] = [{"name": p.get("name"), "team": p.get("team"), "valueBleed": p.get("valueBleed"), "keys": sorted(p.keys())}
                    for p in manifest.get("commandPosts") or []]
    props = folder / "props.json"
    out["live"] = {}
    if props.exists():
        try:
            live = json.loads(props.read_text())
            out["live"] = dict(collections.Counter(i.get("class") for i in live.get("instances", [])))
            out["livePosts"] = sorted(i.get("post") or i.get("name") for i in live.get("instances", []) if i.get("base") == "commandpost")
            out["placedItems"] = dict(collections.Counter(i.get("class") for i in live.get("items", [])))
        except (OSError, ValueError):
            pass
    out["vehicles"] = {}
    choices = set()
    # Each battle's pack (vehicles/<mission>/vehicles.json), or an older import's one.
    for vehicles in sorted({folder / b["vehicles"] for b in battles.values() if b.get("vehicles")}):
        if not vehicles.exists():
            continue
        try:
            v = json.loads(vehicles.read_text())
            for cls, c in (v.get("classes") or {}).items():
                seats = c.get("seats") or []
                out["vehicles"][cls] = {"base": c.get("base"), "seats": len(seats),
                                        "weapons": [[w.get("class") for w in (s.get("weapons") or [])] for s in seats]}
            for s in v.get("spawns") or []:
                for team, cls in (s.get("classes") or {}).items():
                    choices.add((team, cls))
        except (OSError, ValueError):
            pass
    out["padChoices"] = sorted(choices)
    return out


def prepared_sides(public):
    folder = public / "sides"
    if not folder.exists():
        return {}
    out = {}
    for side in sorted(folder.iterdir()):
        units = side / "units.json"
        if units.exists():
            try:
                data = json.loads(units.read_text()).get("units") or {}
                out[side.name] = sorted(data) if isinstance(data, dict) else [u.get("class") for u in data]
            except (OSError, ValueError):
                out[side.name] = []
    return out


def shell_eras(public):
    shell = public / "common" / "shell" / "shell.json"
    if not shell.exists():
        return {}
    try:
        maps = json.loads(shell.read_text()).get("maps") or []
    except (OSError, ValueError):
        return {}
    if isinstance(maps, dict):
        maps = [{"id": k, **v} for k, v in maps.items()]
    return {m["id"].lower(): m.get("eras") or {} for m in maps if isinstance(m, dict) and m.get("id")}


# ---------------------------------------------------------------- gaps


def pad_era(key):
    """Which era a vehicle pad's Class<Faction><Role> key belongs to."""
    faction = key[5:-3]
    return {"All": "gcw", "Imp": "gcw", "Rep": "cw", "CIS": "cw", "His": "his", "Loc": "loc"}.get(faction, faction.lower())


def map_gaps(inventory, prepared, sides_prepared, eras=None):
    """Where the prepared map falls short of the retail one, by comparison
    alone (no engine judgement): turrets and droids never prepared, pad
    choices never prepared, posts with retail health, sides the mission
    names that no units.json exists for."""
    gaps = []
    if prepared is None:
        return [{"kind": "map", "detail": "not prepared"}]
    retail_live = collections.Counter(e["resolved"] for e in inventory["turrets"] + inventory["droids"])
    for cls, n in sorted(retail_live.items()):
        have = sum(v for k, v in prepared["live"].items() if k.lower() == cls.lower())
        if have < n:
            names = [e["name"] or "(unnamed)" for e in inventory["turrets"] + inventory["droids"] if e["resolved"] == cls]
            placed = [e["class"] for e in inventory["turrets"] + inventory["droids"] if e["resolved"] == cls][0]
            gaps.append({"kind": "live", "class": cls, "placedAs": placed, "retail": n, "prepared": have, "instances": names[:12]})
    for p in inventory["posts"]:
        # A destructible post is prepared as a live building named after it
        # (props.json, base commandpost) beside its post in map.json.
        if p["destructible"] and p["name"] not in (prepared.get("livePosts") or []):
            gaps.append({"kind": "post", "name": p["name"], "class": p["resolved"], "maxHealth": p["maxHealth"], "wreck": p["wreck"],
                         "killRegion": p["killRegion"], "preparedKeys": next((q["keys"] for q in prepared["posts"] if q["name"] == p["name"]), None)})
    prepared_names = {p["name"] for p in prepared["posts"]}
    for p in inventory["posts"]:
        if p["name"] and p["name"] not in prepared_names:
            gaps.append({"kind": "postMissing", "name": p["name"], "class": p["resolved"], "placedAs": p["class"]})
    retail_choices = collections.Counter()
    for pad in inventory["pads"]:
        for key, cls in pad["choices"].items():
            retail_choices[(key, cls.lower())] += 1
    prepared_classes = {cls.lower() for _, cls in prepared["padChoices"]}
    by_era = collections.defaultdict(set)
    for (key, cls) in retail_choices:
        if cls not in prepared_classes:
            by_era[pad_era(key)].add(cls)
    for era, missing in sorted(by_era.items()):
        extracted = (eras or {}).get(era, {}).get("extracted")
        gaps.append({"kind": "pads", "era": era, "eraExtracted": extracted, "classesNotPrepared": sorted(missing),
                     "keys": sorted({key for (key, cls) in retail_choices if cls in missing and pad_era(key) == era})})
    # Every gameplay object (placed or attached) against the live instances
    # and placed items the prepared props.json holds, by class.
    have = collections.Counter()
    for table in (prepared.get("live") or {}, prepared.get("placedItems") or {}):
        for cls, n in table.items():
            have[str(cls).lower()] += n
    for cls, entry in (inventory.get("interactive") or {}).items():
        if entry["base"] in ("armedbuilding", "powerupstation", "commandpost"):
            continue  # compared above
        retail = entry["placed"] + entry["attached"]
        if have[cls.lower()] < retail:
            gaps.append({"kind": "interactive", "class": cls, "base": entry["base"], "retail": retail, "prepared": have[cls.lower()]})
    for side in prepared.get("sides") or []:
        if side not in sides_prepared:
            gaps.append({"kind": "side", "side": side, "detail": "mission reads this side; no sides/<side>/units.json"})
    return gaps


# ---------------------------------------------------------------- main


def build(game=GAME, public=PUBLIC, only=None, root=ROOT):
    shared = {"core": (game / "core.lvl"), "common": (game / "Common" / "ENG" / "common.lvl")}
    shared_data = {k: p.read_bytes() for k, p in shared.items() if p.exists()}
    sides = {p.stem: p.read_bytes() for p in sorted((game / "SIDE").glob("*.lvl"))}
    maps = []
    for folder in sorted(game.iterdir()):
        if folder.is_dir() and len(folder.name) == 3 and folder.name.isupper():
            for p in sorted(folder.glob("*.lvl")):
                stem = p.stem
                if stem.endswith("_h") or stem == "tat1h":
                    continue  # historical variants: the same world, loaded with other layers
                if only and stem not in only:
                    continue
                maps.append((stem, p))
    # The add-ons' maps and armies (Jabba's Palace: AddOn/Tat3, its world
    # TAT/tat3.lvl and the Gamorreans' Side/gam.lvl), found as the
    # extractor finds them (tools/zero_addon.py); their missions too.
    stock = {stem for stem, _ in maps}
    for add_on in add_ons(game):
        for folder in sorted(add_on.root.iterdir()):
            if not folder.is_dir():
                continue
            if folder.name.lower() == "side":
                for p in sorted(folder.glob("*.lvl")):
                    sides.setdefault(p.stem.lower(), p.read_bytes())
                continue
            if len(folder.name) != 3 or folder.name.lower() in ("sou",):
                continue
            for p in sorted(folder.glob("*.lvl")):
                stem = p.stem.lower()
                if stem in stock or stem.endswith("_h") or (only and stem not in only) or not stem.startswith(folder.name.lower()):
                    continue
                maps.append((stem, p))
    scripts = all_missions(game)

    # Shared and side classes, first definition wins (the game's order).
    classes, weapons, ordnance = {}, {}, {}
    sources = {}
    for src, data in list(shared_data.items()) + list(sides.items()):
        for k, v in classes_of(data).items():
            if k not in classes:
                classes[k] = v
                sources[k] = src
        for k, v in classes_of(data, b"wpnc").items():
            weapons.setdefault(k, (src, v))
        for k, v in classes_of(data, b"ordc").items():
            ordnance.setdefault(k, v)

    out = {"game": str(game), "prepared": str(public), "maps": {}, "classes": {}, "weapons": [], "soldiers": [], "mission": {}}
    map_data = {}
    for name, path in maps:
        data = path.read_bytes()
        map_data[name] = data
        local = classes_of(data)
        for k, v in local.items():
            if k not in classes:
                classes[k] = v
                sources[k] = name
        for k, v in classes_of(data, b"wpnc").items():
            weapons.setdefault(k, (name, v))
        for k, v in classes_of(data, b"ordc").items():
            ordnance.setdefault(k, v)

    vehicles = [vehicle_record(k, v, sources[k]) for k, v in classes.items() if (v["base"] or "").lower() in VEHICLE_BASES]
    turret_classes = [vehicle_record(k, v, sources[k]) for k, v in classes.items() if (v["base"] or "").lower() == "armedbuilding"]
    droid_classes = [{"class": k, "source": sources[k], "keys": picked(v["props"], ("Label", "GeometryName", "SoldierHealth", "SoldierAmmo", "VehicleHealth", "VehicleAmmo", "PowerupDelay", "Radius"))}
                     for k, v in classes.items() if (v["base"] or "").lower() == "powerupstation"]
    out["classes"] = {
        "vehicles": vehicles, "armedBuildings": turret_classes, "powerupStations": droid_classes,
        "bases": dict(collections.Counter((v["base"] or "").lower() for v in classes.values())),
    }
    out["weapons"] = [weapon_record(k, v, ordnance, classes, src) for k, (src, v) in sorted(weapons.items())]
    out["weaponBases"] = dict(collections.Counter(f"{w['base']}/{(w['ordnance'] or {}).get('base') or ''}" for w in out["weapons"]))
    out["soldiers"] = [soldier_record(k, v, sources[k]) for k, v in sorted(classes.items()) if (v["base"] or "").lower() == "soldier"]

    sides_prepared = prepared_sides(public)
    eras = shell_eras(public)
    for name, data in map_data.items():
        inventory = map_inventory(name, data, classes, scripts)
        prepared = prepared_inventory(public, name)
        inventory["prepared"] = prepared
        inventory["eras"] = eras.get(name, {})
        inventory["gaps"] = map_gaps(inventory, prepared, sides_prepared, inventory["eras"])
        out["maps"][name] = inventory
    out["sidesPrepared"] = sides_prepared
    out["sidesRetail"] = {s: sorted(k for k, v in classes_of(d).items() if (v["base"] or "").lower() in ("soldier",) + VEHICLE_BASES) for s, d in sides.items()}

    used = collections.Counter()
    for s, chunk in scripts.items():
        try:
            for name, _ in lua_calls(lua_load(chunk)):
                used[name] += 1
        except Exception:  # noqa: BLE001 - a script the reader cannot follow is reported, not fatal
            used["(unreadable:" + s + ")"] += 1
    out["mission"] = {"scripts": sorted(scripts), "calls": {k: {"scripts": n, "consumed": engine_consumes(k, root)} for k, n in sorted(used.items())}}
    return out


def report(out):
    lines = []
    lines.append(f"retail: {out['game']}")
    lines.append(f"prepared: {out['prepared']}")
    lines.append(f"vehicle classes: {len(out['classes']['vehicles'])}; armed buildings: {len(out['classes']['armedBuildings'])}; "
                 f"weapons: {len(out['weapons'])}; soldiers: {len(out['soldiers'])}")
    lines.append("weapon/ordnance bases: " + ", ".join(f"{k} x{v}" for k, v in sorted(out["weaponBases"].items(), key=lambda kv: -kv[1])))
    lines.append("sides prepared: " + ", ".join(f"{k} ({len(v)})" for k, v in out["sidesPrepared"].items()))
    for name, m in out["maps"].items():
        p = m["prepared"]
        lines.append("")
        lines.append(f"== {name}: {m['instances']} instances; turrets {len(m['turrets'])}, droids {len(m['droids'])}, posts {len(m['posts'])}, "
                     f"pads {len(m['pads'])}; special: " + ", ".join(f"{k} x{len(v)}" for k, v in m["special"].items()))
        others = {c: e for c, e in m.get("interactive", {}).items() if e["base"] not in ("armedbuilding", "powerupstation")}
        if others:
            lines.append("   interactive: " + ", ".join(f"{c} ({e['base']}) x{e['placed']}" + (f" +{e['attached']} attached" if e["attached"] else "")
                                                   for c, e in others.items()))
        if m["caseMismatches"]:
            lines.append(f"   class-name case mismatches: {len(m['caseMismatches'])} (" + ", ".join(list(m["caseMismatches"])[:4]) + ("..." if len(m["caseMismatches"]) > 4 else "") + ")")
        if p:
            lines.append(f"   prepared mission {p['mission']} sides {p['sides']}; eras: " + ", ".join(f"{k}{'' if v.get('extracted') else ' (not extracted)'}" for k, v in m["eras"].items()))
        for g in m["gaps"]:
            if g["kind"] == "live":
                lines.append(f"   GAP live object {g['class']} (placed as {g['placedAs']}): retail {g['retail']}, prepared {g['prepared']}")
            elif g["kind"] == "post":
                lines.append(f"   GAP destructible post {g['name']} ({g['class']}, MaxHealth {g['maxHealth']}, wreck {g['wreck']}, kill region {g['killRegion']}); prepared keys {g['preparedKeys']}")
            elif g["kind"] == "postMissing":
                lines.append(f"   GAP command post {g['name']} ({g['class']}, placed as {g['placedAs']}) is not in the prepared posts")
            elif g["kind"] == "pads":
                era = g["era"] + ("" if g["eraExtracted"] else " era, not extracted" if g["eraExtracted"] is False else " era, unknown to the shell")
                lines.append(f"   GAP pad classes never prepared ({era}): {', '.join(g['classesNotPrepared'])} (keys {', '.join(g['keys'])})")
            elif g["kind"] == "interactive":
                lines.append(f"   GAP interactive {g['class']} (base {g['base']}): retail {g['retail']}, prepared {g['prepared']}")
            elif g["kind"] == "side":
                lines.append(f"   GAP side {g['side']}: {g['detail']}")
            else:
                lines.append(f"   GAP {g}")
    unread = [k for k, v in out["mission"]["calls"].items() if v["consumed"] is False]
    if unread:
        lines.append("")
        lines.append("mission calls nothing in src/ or tools/ reads: " + ", ".join(unread))
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--out", default=str(ROOT / "extract" / "audit" / "features.json"))
    parser.add_argument("--maps", nargs="*", help="only these maps (default: all)")
    parser.add_argument("--game", default=str(GAME))
    parser.add_argument("--prepared", default=str(PUBLIC))
    parser.add_argument("--quiet", action="store_true")
    args = parser.parse_args()
    game = Path(args.game)
    if not (game / "mission.lvl").exists() and not (game / "core.lvl").exists():
        sys.exit(f"no retail archives at {game} (set BFE_GAME_DIR to the folder holding GameData)")
    out = build(game, Path(args.prepared), set(args.maps or []) or None)
    path = Path(args.out)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(out, indent=1))
    if not args.quiet:
        print(report(out))
        print(f"\ninventory: {path} ({os.path.getsize(path) // 1024} KB)")


if __name__ == "__main__":
    main()
