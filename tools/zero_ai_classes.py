"""AI properties of a side's classes and weapons: public/sides/<side>/ai.json.

    python3 tools/zero_ai_classes.py rep [cis ...]
    python3 tools/zero_ai_classes.py --archive some.lvl --out ai.json   (one archive, no shared ones)

Reads data/game/install/GameData/Data/_LVL_PC/SIDE/<side>.lvl (and the shared
common.lvl and core.lvl, where some weapons live) and writes, for every
entity class (entc) and weapon class (wpnc) the side defines, what the Classic
AI reads (docs/ai-combat.md has the schema):

- the values the original game's class parsers leave, replayed in the archive's
  property order (a munged class holds its whole ODF chain flattened, so a
  key can repeat and order matters): weapon ranges and the implicit-range
  flag (an authored MaxRange clears it), the derived extreme range (the
  larger of ExtremeRange and LockOnRange as each is read), the relationship
  and category target masks (TargetAll sets or clears the whole category
  mask; each Target* flag its bit), burst lengths (BarrageMin/Max/Delay), the
  zoom and trigger keys the firing code tests, the AnimationBank action, and
  each NextCharge level's own values;
- what was authored (`explicit`, last raw value per key) and what was left to
  the engine's class defaults (`defaulted`);
- for classes: UnitType and its role, AISizeType and its traversal mask,
  VehicleType, the hero inputs (MaxHealth, AddHealth, CapturePosts,
  NoEnterVehicles), speeds, the FleeSound and ChargeSound callbacks, the 13
  AI and 13 human tactical VO bank entries plus the acknowledgements, and
  the loadout;
- every property of the class, named or not, in archive order (`raw`), so
  nothing the first milestone ignores is lost.

Values are retail data: the output stays under the gitignored public/.
"""

import hashlib
import json
import re
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ucfb import chunks, cstr_at, nodes  # noqa: E402
from zero_addon import side_archive, source_name  # noqa: E402
from zero_hash import fnv1a  # noqa: E402
import zero_odf  # noqa: E402
import zero_weapons  # noqa: E402

GENERATOR = "tools/zero_ai_classes.py 1"
SCHEMA = 1
ROOT = Path(__file__).resolve().parents[1]
from zero_paths import GAME  # noqa: E402
SHARED = [GAME / "Common" / "ENG" / "common.lvl", GAME / "core.lvl"]
from zero_paths import PUBLIC  # noqa: E402

# ---------------------------------------------------------------------------
# Names. Every key the repository's extractors already know, plus the AI keys
# whose hashes the class parsers compare.

AI_WEAPON_KEYS = (
    "MinRange", "OptimalRange", "MaxRange", "ExtremeRange", "LockOnRange",
    "TargetEnemy", "TargetNeutral", "TargetFriendly", "TargetPerson", "TargetAnimal", "TargetDroid",
    "TargetVehicle", "TargetBuilding", "TargetBuildingDead", "TargetBuildingUnbuilt", "TargetAll",
    "BarrageMin", "BarrageMax", "BarrageDelay", "TriggerSingle", "ZoomMin", "ZoomMax", "MaxPressedTime",
    "AnimationBank", "LockTime", "ClearLockOnFire", "CanDeflect", "OrdnanceName", "NextCharge",
    "RoundsPerClip", "ShotsPerSalvo", "HeatPerShot", "YawSpread", "PitchSpread", "SpreadPerShot",
    "AutoAimSize", "LockOnAngle", "LockOffAngle",
)
TACTICAL = (
    "FieldMoveOut", "FieldHold", "FieldFollow", "DriverGetIn", "DriverGetOut", "PassengerMoveOut",
    "PassengerStop", "PassengerGetIn", "PassengerGetOut", "GunnerAllClear", "GunnerSteady",
    "GunnerGetIn", "GunnerGetOut",
)
TACTICAL_AI = tuple(f"AISC{n}Sound" for n in TACTICAL)
TACTICAL_HUMAN = tuple(f"SC{n}Sound" for n in TACTICAL)
ACKNOWLEDGE = ("AISCResponseYessirSound", "AISCResponseNosirSound", "SCResponseYessirSound", "SCResponseNosirSound")
AI_CLASS_KEYS = (
    "UnitType", "AISizeType", "VehicleType", "PilotType", "HealthType", "MaxHealth", "AddHealth",
    "CapturePosts", "NoEnterVehicles", "MaxSpeed", "MaxStrafeSpeed", "MaxTurnSpeed", "MaxPitchSpeed",
    "ForwardSpeed", "StrafeSpeed", "TurnRate", "PitchRate", "MinSpeed", "MidSpeed",
    "JetJump", "JetPush", "JetFuelRechargeRate", "JetFuelCost", "JetFuelInitialCost", "JetFuelMinBorder",
    "FleeSound", "ChargeSound", "AimerPitchLimits", "AimerYawLimits", "LandingTime", "LandingSpeed",
    "LandedHeight", "TakeoffTime", "SoldierCollision",
) + TACTICAL_AI + TACTICAL_HUMAN + ACKNOWLEDGE

NAMES = {}
for _name in (list(zero_odf.KNOWN) + list(zero_weapons.WEAPON + zero_weapons.ORDNANCE + zero_weapons.EXPLOSION
              + zero_weapons.LOADOUT + zero_weapons.DISPENSER + zero_weapons.ITEM + zero_weapons.REPAIR
              + zero_weapons.REMOTE + zero_weapons.DESTRUCT + zero_weapons.DROID + zero_weapons.EMITTER
              + zero_weapons.SHIELD) + list(AI_WEAPON_KEYS) + list(AI_CLASS_KEYS)
              + [f"WeaponSection", "WeaponName", "WeaponAmmo", "WeaponChannel"]):
    NAMES.setdefault(fnv1a(_name), _name)
NAMES.update(zero_odf.UNNAMED)
NAMES[0xFC0D5E74] = "DamageDropPerTarget"

# The engine classes behind the ODF class labels.
WEAPON_CLASSES = {
    "weapon": "WeaponCannon", "cannon": "WeaponCannon", "launcher": "WeaponLauncher",
    "grapplinghookweapon": "WeaponGrapplingHook", "grenade": "WeaponGrenade", "melee": "WeaponMelee",
    "dispenser": "WeaponDispenser", "remote": "WeaponRemote", "detonator": "WeaponDetonator",
    "repair": "WeaponRepair", "shield": "WeaponShield", "laser": "WeaponLaser", "destruct": "WeaponDestruct",
    "binoculars": "WeaponBinoculars", "catapult": "WeaponCatapult", "disguise": "WeaponDisguise",
    "towcableweapon": "WeaponTowCable",
}
CANNON_FAMILY = {"WeaponCannon", "WeaponLauncher", "WeaponGrapplingHook"}
ORDNANCE_CLASSES = {
    "beam": "OrdnanceBeam", "bullet": "OrdnanceBullet", "shell": "OrdnanceShell", "grenade": "OrdnanceGrenade",
    "sticky": "OrdnanceSticky", "beacon": "OrdnanceBeacon", "haywire": "OrdnanceHaywire", "laser": "OrdnanceLaser",
    "bolt": "OrdnanceLaser", "missile": "OrdnanceMissile", "towcable": "OrdnanceTowCable",
    "grapplinghook": "OrdnanceGrapplingHook", "emitterordnance": "OrdnanceEmitter", "fatray": "OrdnanceFatRay",
}

# UnitType roles: the authored string's case-folded hash.
ROLES = {"scout": 0, "anti-armor": 1, "assault": 2, "support": 3, "repair": 3, "special": 3, "pilot": 4, "trooper": 5}
# AISizeType traversal bits (case-insensitive; unrecognized 1).
SIZES = {"soldier": 1, "hover": 2, "small": 4, "medium": 8, "huge": 16, "flyer": 32}
# VehicleType: unrecognized 7.
VEHICLE_TYPES = {"scout": 0, "light": 1, "medium": 2, "heavy": 3, "fighter": 4, "transport": 5, "capitol": 6}
# AnimationBank -> bank enum -> AI action.
BANKS = ["rifle", "bazooka", "pistol", "grenade", "tool", "lightsabre", "sbdroid", "bdroid", "cannon", "gam"]
BANK_ACTION = [1, 3, 0, 2, 6, 4, 1, 1, 3, 4]

RELATION_BITS = {"TargetEnemy": 1, "TargetNeutral": 2, "TargetFriendly": 4}
CATEGORY_BITS = {"TargetPerson": 1, "TargetAnimal": 2, "TargetDroid": 4, "TargetVehicle": 8, "TargetBuilding": 16,
                 "TargetBuildingDead": 32, "TargetBuildingUnbuilt": 64}

# ---------------------------------------------------------------------------
# The parsers' number conversions.

_FLOAT = re.compile(r"\s*([+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)")
_INT = re.compile(r"\s*([+-]?\d+)")


def f32(x):
    return struct.unpack("<f", struct.pack("<f", float(x)))[0]


def scan_float(raw, old):
    """sscanf("%f"): the leading number, or the old value when there is none."""
    m = _FLOAT.match(raw or "")
    return f32(m.group(1)) if m else old


def scan_int(raw, old):
    """sscanf("%d")."""
    m = _INT.match(raw or "")
    return int(m.group(1)) if m else old


def atoi(raw):
    m = _INT.match(raw or "")
    return int(m.group(1)) if m else 0


# ---------------------------------------------------------------------------
# Reading classes with every property kept.

def all_classes(data, tag):
    """name -> {base, props: [(hash, name or None, raw)]}, first definition wins."""
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
                props.append((key, NAMES.get(key), cstr_at(data, b + 4, e)))
        if typ and typ not in out:
            out[typ] = {"base": base, "props": props}
    return out


def raw_list(props):
    return [["%08x" % key, name, value] for key, name, value in props]


# ---------------------------------------------------------------------------
# Weapons.

def weapon_state():
    """WeaponClass defaults for the AI's keys."""
    return {
        "minRange": 0.0, "optimalRange": 30.0, "maxRange": 60.0, "implicitRange": True,
        "extremeRange": 0.0, "lockOnRange": 0.0,
        "relationMask": 1, "categoryMask": 0xFFFFFF9F,
        "barrageMin": 3, "barrageMax": 6, "barrageDelay": 2.0,
        "zoomMin": 1.0, "zoomMax": 1.0,
    }


def apply_weapon_key(state, name, raw):
    """One property, as the WeaponClass parser takes it."""
    if name == "MinRange":
        state["minRange"] = scan_float(raw, state["minRange"])
    elif name == "OptimalRange":
        state["optimalRange"] = scan_float(raw, state["optimalRange"])
    elif name == "MaxRange":
        state["maxRange"] = scan_float(raw, state["maxRange"])
        state["implicitRange"] = False
    elif name == "ExtremeRange":
        state["extremeRange"] = scan_float(raw, state["extremeRange"])
        if state["lockOnRange"] > state["extremeRange"]:
            state["extremeRange"] = state["lockOnRange"]
    elif name == "LockOnRange":
        state["lockOnRange"] = scan_float(raw, state["lockOnRange"])
        if state["lockOnRange"] > state["extremeRange"]:
            state["extremeRange"] = state["lockOnRange"]
    elif name in RELATION_BITS:
        bit = RELATION_BITS[name]
        state["relationMask"] = (state["relationMask"] | bit) if atoi(raw) else (state["relationMask"] & ~bit)
    elif name in CATEGORY_BITS:
        bit = CATEGORY_BITS[name]
        state["categoryMask"] = (state["categoryMask"] | bit) if atoi(raw) else (state["categoryMask"] & ~bit)
    elif name == "TargetAll":
        state["categoryMask"] = 0xFFFFFFFF if atoi(raw) else 0
    elif name == "BarrageMin":
        state["barrageMin"] = scan_int(raw, state["barrageMin"])
    elif name == "BarrageMax":
        state["barrageMax"] = scan_int(raw, state["barrageMax"])
    elif name == "BarrageDelay":
        state["barrageDelay"] = scan_float(raw, state["barrageDelay"])
    elif name == "ZoomMin":
        state["zoomMin"] = scan_float(raw, state["zoomMin"])
    elif name == "ZoomMax":
        value = scan_float(raw, state["zoomMax"])
        state["zoomMax"] = 1.0 if value == 0 else value
    elif name == "TriggerSingle":
        state["triggerSingle"] = atoi(raw) != 0
    elif name == "MaxPressedTime":
        state["maxPressedTime"] = scan_float(raw, state.get("maxPressedTime", 0.0))
    elif name == "AnimationBank":
        state["animationBank"] = raw
    elif name == "LockTime":
        state["lockTime"] = scan_float(raw, state.get("lockTime", 0.0))
    elif name == "ClearLockOnFire":
        state["clearLockOnFire"] = atoi(raw) != 0
    elif name == "CanDeflect":
        state["canDeflect"] = atoi(raw) != 0


def finish_weapon_state(state, label, defaults):
    out = dict(state)
    out["relationMask"] &= 0xFFFFFFFF
    out["categoryMask"] &= 0xFFFFFFFF
    out.setdefault("triggerSingle", bool(defaults.get("TriggerSingle", 0)))
    bank = (out.get("animationBank") or "rifle").lower()
    out["action"] = BANK_ACTION[BANKS.index(bank)] if bank in BANKS else BANK_ACTION[0]
    engine = WEAPON_CLASSES.get((label or "").lower())
    out["engineClass"] = engine
    out["cannon"] = engine in CANNON_FAMILY
    return out


def weapon_record(name, cls, archive, shared_archives):
    label = (cls["base"] or "").lower()
    defaults = dict(zero_weapons.WEAPON_DEFAULTS)
    if label == "grenade":
        defaults.update(zero_weapons.GRENADE_DEFAULTS)
    if label in ("dispenser", "remote"):
        defaults.update(zero_weapons.DISPENSER_DEFAULTS)
    if label == "repair":
        defaults.update(zero_weapons.REPAIR_DEFAULTS)
    state = weapon_state()
    explicit = {}
    levels = []
    level_explicit = None
    for key, pname, raw in cls["props"]:
        if pname == "NextCharge":
            # A charge level: a copy of the class as read so far.
            levels.append({"at": scan_float(raw, 0.0), "state": dict(state), "explicit": {}})
            level_explicit = levels[-1]["explicit"]
            continue
        if pname in AI_WEAPON_KEYS:
            target = levels[-1]["state"] if levels else state
            apply_weapon_key(target, pname, raw)
            (level_explicit if levels else explicit)[pname] = raw
    record = {
        "class": name,
        "base": cls["base"],
        "archive": archive,
        "ai": finish_weapon_state(state, label, defaults),
        "explicit": explicit,
        "defaulted": [k for k in ("MinRange", "OptimalRange", "MaxRange", "ExtremeRange", "LockOnRange", "BarrageMin",
                                  "BarrageMax", "BarrageDelay", "TriggerSingle", "ZoomMax") if k not in explicit],
    }
    if levels:
        record["levels"] = [{"at": lv["at"], "ai": finish_weapon_state(lv["state"], label, defaults), "explicit": lv["explicit"]}
                            for lv in levels]
    ordnance_name = next((raw for _, n, raw in cls["props"] if n == "OrdnanceName"), None)
    chain = zero_weapons.weapon_chain(shared_archives, name)
    ordnance = (chain or {}).get("ordnance")
    if ordnance:
        record["ordnance"] = {
            "class": ordnance.get("class"),
            "base": ordnance.get("base"),
            "engineClass": ORDNANCE_CLASSES.get((ordnance.get("base") or "").lower()),
            **{k: ordnance[k] for k in ("Velocity", "Gravity", "Damage", "PersonScale", "AnimalScale", "DroidScale",
                                         "VehicleScale", "BuildingScale", "ShieldScale") if k in ordnance},
            "explosions": {k: {kk: v.get(kk) for kk in ("class", "Damage", "DamageRadiusInner", "DamageRadiusOuter",
                                                       "PersonScale", "AnimalScale", "DroidScale", "VehicleScale",
                                                       "BuildingScale") if kk in v}
                           for k, v in ((k, ordnance.get(k)) for k in ("ExplosionImpact", "ExplosionExpire"))
                           if isinstance(v, dict)},
        }
    elif ordnance_name:
        record["ordnanceName"] = ordnance_name
    record["raw"] = raw_list(cls["props"])
    return record


# ---------------------------------------------------------------------------
# Classes.

def class_record(name, cls):
    explicit = {}
    loadout = zero_weapons.loadout([(n or "", raw) for _, n, raw in cls["props"] if n])
    sections = []
    for _, pname, raw in cls["props"]:
        if pname == "WeaponSection":
            sections.append({})
        elif pname in ("WeaponName", "WeaponAmmo", "WeaponChannel") and sections:
            sections[-1][pname] = raw
        if pname in AI_CLASS_KEYS:
            explicit[pname] = raw
    ai = {}
    unit_type = explicit.get("UnitType")
    ai["unitType"] = unit_type
    ai["role"] = ROLES.get((unit_type or "").strip().lower(), 6)
    size = explicit.get("AISizeType")
    ai["sizeType"] = size
    ai["sizeMask"] = SIZES.get(size.strip().lower(), 1) if size is not None else None
    vtype = explicit.get("VehicleType")
    ai["vehicleType"] = VEHICLE_TYPES.get(vtype.strip().lower(), 7) if vtype is not None else None
    for key, out_key in (("MaxHealth", "maxHealth"), ("AddHealth", "addHealth"), ("MaxSpeed", "maxSpeed"),
                         ("MaxStrafeSpeed", "maxStrafeSpeed"), ("MaxTurnSpeed", "maxTurnSpeed"),
                         ("MaxPitchSpeed", "maxPitchSpeed"), ("ForwardSpeed", "forwardSpeed"),
                         ("StrafeSpeed", "strafeSpeed"), ("JetJump", "jetJump"), ("JetFuelMinBorder", "jetFuelMinBorder"),
                         ("JetFuelCost", "jetFuelCost"), ("JetFuelInitialCost", "jetFuelInitialCost")):
        if key in explicit:
            ai[out_key] = scan_float(explicit[key], None)
    for key, out_key in (("CapturePosts", "capturePosts"), ("NoEnterVehicles", "noEnterVehicles")):
        if key in explicit:
            ai[out_key] = atoi(explicit[key]) != 0
    for key in ("HealthType", "PilotType", "FleeSound", "ChargeSound"):
        if key in explicit:
            ai[key[0].lower() + key[1:]] = explicit[key]
    tactical = {
        "ai": {n: explicit[n] for n in TACTICAL_AI if n in explicit},
        "human": {n: explicit[n] for n in TACTICAL_HUMAN if n in explicit},
        "acknowledge": {n: explicit[n] for n in ACKNOWLEDGE if n in explicit},
    }
    if any(tactical.values()):
        ai["tactical"] = tactical
    ai["loadout"] = loadout
    if sections:
        ai["sections"] = [{"weapon": s.get("WeaponName"), "ammo": s.get("WeaponAmmo"), "channel": s.get("WeaponChannel")} for s in sections]
    return {
        "class": name,
        "base": cls["base"],
        "ai": ai,
        "explicit": explicit,
        "defaulted": [k for k in ("UnitType", "AISizeType", "VehicleType") if k not in explicit],
        "raw": raw_list(cls["props"]),
    }


def source_info(path):
    data = path.read_bytes()
    name = source_name(path, GAME)
    return data, {"path": name, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def extract(side, archive=None, out_path=None):
    # The game's SIDE/<side>.lvl, or an add-on's own army's.
    path = Path(archive) if archive else side_archive(side, GAME)
    if not path.exists():
        raise SystemExit(f"no archive {path}")
    data, source = source_info(path)
    shared = []
    shared_info = []
    for p in ([] if archive else SHARED):
        if p.exists():
            d, info = source_info(p)
            shared.append(d)
            shared_info.append(info)
    archives = [data, *shared]
    entities = all_classes(data, b"entc")
    weapons_here = all_classes(data, b"wpnc")
    classes = {name: class_record(name, cls) for name, cls in entities.items()}
    # Every weapon the side defines, and those its classes carry from the
    # shared archives.
    wanted = list(weapons_here)
    for rec in classes.values():
        for slot in rec["ai"].get("loadout", []):
            wanted.append(slot["weapon"])
        for section in rec["ai"].get("sections", []):
            if section.get("weapon"):
                wanted.append(section["weapon"])
    weapons = {}
    for name in wanted:
        if name in weapons:
            continue
        if name in weapons_here:
            weapons[name] = weapon_record(name, weapons_here[name], "side", archives)
            continue
        for i, d in enumerate(shared):
            found = all_classes(d, b"wpnc").get(name)
            if found:
                weapons[name] = weapon_record(name, found, shared_info[i]["path"], archives)
                break
    out = {
        "schema": SCHEMA,
        "generator": GENERATOR,
        "side": side,
        "source": source,
        "shared": shared_info,
        "classes": classes,
        "weapons": weapons,
    }
    if out_path:
        target = Path(out_path)
        target.parent.mkdir(parents=True, exist_ok=True)
    else:
        target = PUBLIC / "sides" / side / "ai.json"
        target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(out, indent=1))
    print(f"{side}: {len(classes)} classes, {len(weapons)} weapons -> {target}")
    return out


def main(argv):
    args = argv[1:]
    if "--archive" in args:
        archive = args[args.index("--archive") + 1]
        out_path = args[args.index("--out") + 1] if "--out" in args else None
        extract(Path(archive).stem, archive=archive, out_path=out_path)
        return
    if not args:
        raise SystemExit(__doc__)
    for side in args:
        extract(side)


if __name__ == "__main__":
    main(sys.argv)
