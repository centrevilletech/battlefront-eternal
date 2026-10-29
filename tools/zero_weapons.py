"""Weapons (wpnc), ordnance (ordc), and explosions (expc).

These are laid out like entity classes (zero_odf): TYPE, BASE (the engine
class: cannon, grenade, bolt, sticky, explosion, ...), then PROP chunks with
the FNV hash of the property name and the value as a C string. The ODF chain
is flattened, so a key can repeat; the last value wins.

A weapon's `NextCharge t` starts a charge level: a copy of the class as read
so far, which the keys after it change, used once the trigger has been held
for more than t of MaxPressedTime (the mortar's tap fires a shell and its
full charge a sticky grenade). A shot pattern lists ShotPatternPitchYaw once
per round of the salvo.

A dispenser's OrdnanceName is an entity class instead (entc): a mine or a
power-up item, which it throws. A remote weapon's is a droid (EntityDroid, the
recon droid) with weapons of its own in WeaponSection groups: a blaster, and a
destruct weapon whose StrikeOrdnanceName rains down as the orbital strike.

A soldier class carries its loadout as WeaponName<N>, WeaponAmmo<N> (clips,
counting the loaded one; 0 or less is unlimited), and WeaponChannel<N> (0 the
primary trigger, 1 the secondary).
"""

import struct
import re

from ucfb import chunks, cstr_at, nodes
from zero_hash import fnv1a

# The properties the port reads, by class kind.
WEAPON = (
    "AnimationBank", "GeometryName", "HighResGeometry", "OrdnanceName", "FirePointName",
    "RoundsPerClip", "ReloadTime", "ShotDelay", "TriggerSingle", "MaxPressedTime",
    "SalvoCount", "ShotsPerSalvo", "SalvoDelay", "InitialSalvoDelay", "ShotElevate",
    "HeatPerShot", "HeatRecoverRate", "HeatThreshold",
    "YawSpread", "PitchSpread", "SpreadPerShot", "SpreadRecoverRate", "SpreadThreshold", "SpreadLimit",
    "StandStillSpread", "StandMoveSpread", "StandAimSpread", "CrouchStillSpread", "CrouchMoveSpread",
    "CrouchAimSpread", "ProneStillSpread", "ProneMoveSpread", "ProneAimSpread",
    "ZoomMin", "ZoomMax", "ZoomRate", "ZoomFirstPerson",
    "IconTexture", "ReticuleTexture", "ScopeTexture",
    "MuzzleFlash", "FlashColor", "FlashLength", "FlashLightColor", "FlashLightRadius", "FlashLightDuration",
    "MuzzleFlashEffect",
    # Targeting: what a weapon can lock or auto-aim at, and how.
    "LockOnRange", "LockOnAngle", "LockOffAngle", "LockTime", "ExtremeRange", "AutoAimSize", "ClearLockOnFire",
    "TargetEnemy", "TargetNeutral", "TargetFriendly", "TargetPerson", "TargetAnimal", "TargetDroid",
    "TargetVehicle", "TargetBuilding", "TargetBuildingDead", "TargetBuildingUnbuilt", "TargetAll",
    # Charge levels, shot patterns, the aim kick, and the held model.
    "NextCharge", "ShotPatternCount", "ShotPatternPitchYaw", "KickStrength", "KickBuildup",
    "HideOnFire", "SniperScope", "ChargeUpEffect", "ChargeSoundPitch",
    # Older names for keys above (see ALIASES).
    "KickSpread", "SpreadRecover",
)
ORDNANCE = (
    "GeometryName", "Velocity", "LifeSpan", "Gravity", "MaxDamage", "Damage",
    "PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale", "ShieldScale",
    "LaserTexture", "LaserLength", "LaserWidth", "LaserGlowColor", "GlowLength", "BlurLength", "FadeOutTime",
    "LightColor", "LightRadius", "TrailEffect", "ExplosionExpire", "ExplosionImpact",
    "ImpactEffectSoft", "ImpactEffectRigid", "ImpactEffectStatic", "ImpactEffectTerrain",
    "ImpactEffectWater", "ImpactEffectShield",
    "StickPerson", "StickAnimal", "StickDroid", "StickVehicle", "StickBuilding", "StickTerrain",
    # Missiles' turning and waver.
    "TurnRate", "WaverRate", "WaverTurn", "ScaleTime",
    # The tow cable's length (the snowspeeder's harpoon; hashed d9b746b2).
    "CableLength",
    # What a beam leaves wherever it touches (only beams read it).
    "ImpactEffect",
    # Other names for keys above (see ALIASES).
    "Explosion", "ExplosionName", "HealthScale", "ArmorScale",
)
# The arc caster's emitter ordnance: lightning down a cone, jumping between
# targets.
EMITTER = (
    "ConeLength", "ConeAngle", "FirstRadius", "MaxTargets", "MaxJumpDistance", "DamageThreshold",
    "DamageDropPerTarget", "LightningEffect", "Effect", "Push", "Damage", "MaxDamage",
)
# The emitter ordnance's own defaults.
EMITTER_DEFAULTS = {"ConeLength": 10, "ConeAngle": 60, "FirstRadius": 1.5, "MaxTargets": 5, "MaxJumpDistance": 10, "Damage": 0, "MaxDamage": 0}
EXPLOSION = (
    "Damage", "DamageRadiusInner", "DamageRadiusOuter", "Push", "PushRadiusInner", "PushRadiusOuter",
    "Shake", "ShakeLength", "ShakeRadiusInner", "ShakeRadiusOuter", "Effect", "WaterEffect",
    "LightColor", "LightRadius", "LightDuration",
    "PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale", "ShieldScale",
    # Older one-number radii: the outer radius (the original game stores them
    # there), with the inner left at 0.
    "DamageRadius", "ShakeRadius", "PushRadius",
)
# Dispensers: how hard they throw, and how many of their items may lie about.
DISPENSER = ("Velocity", "MinStrength", "MaxStrength", "MaxItems", "FireAnim")
# Repair weapons (the fusion cutter): what each tick gives, by the target's
# health type, a wrecked building, and a building still to be built.
REPAIR = (
    "PersonHealth", "AnimalHealth", "DroidHealth", "VehicleHealth", "BuildingHealth", "BuildingRebuild", "BuildingBuild",
    "PersonAmmo", "AnimalAmmo", "DroidAmmo", "VehicleAmmo", "BuildingAmmo", "SoldierHealth", "SoldierAmmo",
)
# The droideka's shield: its strength, how fast it drains while up and
# refills while down, and the bubble's place, size, and look.
SHIELD = ("MaxShield", "AddShield", "AddShieldOff", "ShieldOffset", "ShieldRadius", "ShieldEffect")
MELEE = ("SwingTime", "CanDeflect", "Texture", "HitSound", "DeflectSound")
# Remote weapons: whether control goes to the droid at once, and whether the
# reload key may blow it up.
REMOTE = ("SwitchImmediately", "AllowDestruct")
# A remote droid's destruct weapon: the strike it calls down.
DESTRUCT = ("StrikeOrdnanceName", "AimElevation", "AimAzimuth", "AimDistance", "ScatterDistance", "ExplosionClass")
# The remote droid itself (EntityDroid): hover, drive, camera, and weapons.
DROID = (
    "GeometryName", "MaxHealth", "AddHealth", "HealthType", "ExplosionName", "SetAltitude", "GravityScale",
    "LiftSpring", "LiftDamp", "Acceleraton", "MaxSpeed", "MaxStrafeSpeed", "MaxTurnSpeed", "MaxPitchSpeed",
    "PitchLimits", "EyePointOffset", "TrackCenter", "TrackOffset", "TiltValue", "CollisionThreshold",
    "CollisionScale", "HealthTexture", "ScanningRange", "TransmitRange", "AimerPitchLimits", "AimerYawLimits",
    "MoveTension", "AimTension", "FirstPersonFOV", "ThirdPersonFOV",
)
DROID_SECTION = ("WeaponSection", "WeaponName", "WeaponAmmo", "WeaponChannel")
# The droid class's own defaults (its hover keys are the hover template's and
# the droid ignores them).
DROID_DEFAULTS = {
    "MaxHealth": 100, "AddHealth": 0, "SetAltitude": 0, "GravityScale": 1, "LiftSpring": 5, "LiftDamp": 2,
    "Acceleraton": 70, "MaxSpeed": 9, "MaxStrafeSpeed": 7, "MaxTurnSpeed": 2, "MaxPitchSpeed": 0.5,
    "CollisionThreshold": [7.5, 2.5, 0], "CollisionScale": [10, 0, 0], "FirstPersonFOV": 48, "ThirdPersonFOV": 48,
    "MoveTension": 20, "AimTension": 30, "TiltValue": 5,
}
# Items a dispenser throws or a soldier drops (mines and power-ups).
ITEM = (
    "GeometryName", "LifeSpan", "Gravity", "Rebound", "Friction", "LightColor", "LightRadius",
    "MaxHealth", "HealthType",
    # Mines.
    "ExplosionTrigger", "ExplosionExpire", "ExplosionDeath", "TriggerRadius", "SuppressRadius", "TriggerContact",
    # Power-ups: what they give, by the receiver's health type.
    "PersonHealth", "AnimalHealth", "DroidHealth", "VehicleHealth", "BuildingHealth",
    "PersonAmmo", "AnimalAmmo", "DroidAmmo", "VehicleAmmo", "BuildingAmmo",
    # Other names for keys above (see ITEM_ALIASES).
    "ExplosionName", "Explosion", "SoldierHealth", "SoldierAmmo",
)
LOADOUT = tuple(f"Weapon{kind}{n}" for n in range(1, 9) for kind in ("Name", "Ammo", "Channel"))
NAMES = {fnv1a(name): name for name in WEAPON + ORDNANCE + EXPLOSION + LOADOUT + DISPENSER + ITEM + REPAIR + REMOTE + DESTRUCT + DROID + DROID_SECTION + EMITTER + SHIELD + MELEE}
# The emitter's damage lost per link; its real name is unknown.
NAMES[0xFC0D5E74] = "DamageDropPerTarget"

# The engine's own defaults for what a class leaves unset.
WEAPON_DEFAULTS = {
    "RoundsPerClip": 1, "ReloadTime": 0.6, "ShotDelay": 0.01, "TriggerSingle": 0, "MaxPressedTime": 0,
    "SalvoCount": 1, "ShotsPerSalvo": 1, "SalvoDelay": 0, "InitialSalvoDelay": 0, "ShotElevate": 0,
    "HeatPerShot": 0, "HeatRecoverRate": 0, "HeatThreshold": 1,
    "ZoomMin": 1, "ZoomMax": 1, "FirePointName": "hp_fire", "FlashLength": 0.1,
}
GRENADE_DEFAULTS = {"ShotDelay": 0.125, "InitialSalvoDelay": 0.3, "TriggerSingle": 1}
DISPENSER_DEFAULTS = {
    "Velocity": 10, "MinStrength": 0.3, "MaxStrength": 1.0, "ShotDelay": 0.125, "InitialSalvoDelay": 0.3,
    "MaxItems": 0, "HideOnFire": 1, "TriggerSingle": 1,
}
ITEM_DEFAULTS = {"Rebound": 0.3, "Friction": 0.5, "Gravity": 1.0}
REPAIR_DEFAULTS = {"ShotDelay": 0.125, "TriggerSingle": 0}
ORDNANCE_DEFAULTS = {"Velocity": 150, "LifeSpan": 2, "ImpactEffectTerrain": "dirtspray", "ImpactEffectWater": "watersplash_ord"}
# Missiles and shells start slower and live longer; missiles turn at 1 rad/s.
BASE_DEFAULTS = {
    "missile": {"Velocity": 35, "LifeSpan": 3, "TurnRate": 1, "ScaleTime": 1},
    "shell": {"Velocity": 35, "LifeSpan": 3},
}
SHELLS = ("shell", "sticky", "grenade", "mine")
# Keys that set others: the impact explosion goes by three names, and the
# health and armor scales stand for groups of damage scales. Applied in the
# class's own order, so whichever comes last wins.
ALIASES = {
    "Explosion": ("ExplosionImpact",),
    "ExplosionName": ("ExplosionImpact",),
    "MaxDamage": ("Damage",),
    "HealthScale": ("PersonScale", "AnimalScale", "DroidScale"),
    "ArmorScale": ("VehicleScale", "BuildingScale"),
    "KickSpread": ("SpreadPerShot",),
    "SpreadRecover": ("SpreadRecoverRate",),
    "DamageRadius": ("DamageRadiusOuter",),
    "ShakeRadius": ("ShakeRadiusOuter",),
    "PushRadius": ("PushRadiusOuter",),
}
ITEM_ALIASES = {
    "ExplosionName": ("ExplosionTrigger",),
    "Explosion": ("ExplosionTrigger",),
    "SoldierHealth": ("PersonHealth", "DroidHealth"),
    "SoldierAmmo": ("PersonAmmo", "DroidAmmo"),
}
REPAIR_ALIASES = {**ALIASES, "SoldierHealth": ("PersonHealth", "DroidHealth"), "SoldierAmmo": ("PersonAmmo", "DroidAmmo")}
# Keys that list a value each time they appear.
LISTS = {"ShotPatternPitchYaw"}


def classes(data, tag):
    """Every class in an archive with this chunk tag: name -> base, props."""
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
                if key in NAMES:
                    props.append((NAMES[key], cstr_at(data, b + 4, e)))
        if typ:
            out.setdefault(typ, {"base": base, "props": props})
    return out


def _value(raw):
    """A property value as a number, a list of numbers, or a string."""
    parts = raw.split()
    try:
        numbers = [float(p) for p in parts]
    except ValueError:
        # "1.0//2": a number with a comment after it reads as the number.
        head = raw.split("//")[0].split()
        try:
            numbers = [float(p) for p in head] if head and head != parts else None
        except ValueError:
            numbers = None
        if not numbers:
            return raw
    if len(numbers) == 1:
        return numbers[0]
    return numbers or raw


def effective(cls, wanted, defaults=None, aliases=ALIASES):
    """A class's properties with the last value of each key winning, aliases
    written through to the keys they stand for. A shot pattern starts over at
    each ShotPatternCount and at each NextCharge, as the original game's does: a
    charge level copies the class without its pattern."""
    out = dict(defaults or {})
    for key, raw in cls["props"]:
        if key in ("NextCharge", "ShotPatternCount") and "ShotPatternPitchYaw" in wanted:
            out.pop("ShotPatternPitchYaw", None)
        if key in LISTS:
            if key in wanted:
                out[key] = out.get(key, []) + [_value(raw)]
        elif key in aliases:
            for target in aliases[key]:
                if target in wanted:
                    out[target] = _value(raw)
        elif key in wanted:
            out[key] = _value(raw)
    return out


def loadout(props):
    """A soldier's weapon slots from its (name, value) properties, in order."""
    slots = {}
    for key, raw in props:
        for kind in ("Name", "Ammo", "Channel"):
            prefix = f"Weapon{kind}"
            if key.startswith(prefix) and key[len(prefix):].isdigit():
                slots.setdefault(int(key[len(prefix):]), {})[kind.lower()] = raw
    out = []
    for n in sorted(slots):
        slot = slots[n]
        if not slot.get("name"):
            continue
        out.append({
            "slot": n,
            "weapon": slot["name"],
            "ammo": int(float(slot.get("ammo", 0))),
            "channel": int(float(slot.get("channel", 0))),
        })
    return out


def weapon_chain(archives, name):
    """A weapon class, its ordnance, and that ordnance's explosions, read from
    the first archive that defines each."""
    def find(tag, cname):
        for data in archives:
            found = classes(data, tag).get(cname)
            if found:
                return found
        return None

    def ordnance_chain(name):
        ordnance = find(b"ordc", name or "")
        if not ordnance:
            return None
        if ordnance["base"] == "emitterordnance":
            # Damage at no charge and MaxDamage at full, kept apart.
            return {"class": name, "base": ordnance["base"], **effective(ordnance, EMITTER, EMITTER_DEFAULTS, aliases={})}
        props = effective(ordnance, ORDNANCE, {**ORDNANCE_DEFAULTS, **BASE_DEFAULTS.get(ordnance["base"], {})})
        # Only shells fall; bolts and bullets fly straight whatever Gravity says.
        if ordnance["base"] not in SHELLS:
            props.pop("Gravity", None)
        # OrdnanceBeamClass alone reads ImpactEffect and plays it
        # where the beam touches, each update; bolts, rounds and grenades
        # that name one ignore it.
        if ordnance["base"] != "beam":
            props.pop("ImpactEffect", None)
        out = {"class": name, "base": ordnance["base"], **props}
        for key in ("ExplosionExpire", "ExplosionImpact"):
            explosion = find(b"expc", props.get(key) or "")
            if explosion:
                out[key] = {"class": props[key], "base": explosion["base"], **effective(explosion, EXPLOSION)}
        return out

    weapon = find(b"wpnc", name)
    if weapon is None:
        return None
    defaults = dict(WEAPON_DEFAULTS)
    wanted = WEAPON
    if weapon["base"] == "grenade":
        defaults.update(GRENADE_DEFAULTS)
    if weapon["base"] == "launcher":
        # The launcher class's own defaults.
        defaults.update({"LockTime": 0, "ClearLockOnFire": 1})
    aliases = ALIASES
    if weapon["base"] in ("dispenser", "remote"):
        defaults.update(DISPENSER_DEFAULTS)
        wanted = WEAPON + DISPENSER
    if weapon["base"] == "remote":
        defaults.update({"SwitchImmediately": 0, "AllowDestruct": 1})
        wanted = WEAPON + DISPENSER + REMOTE
    if weapon["base"] == "destruct":
        wanted = WEAPON + DESTRUCT
    if weapon["base"] == "shield":
        wanted = WEAPON + SHIELD
    if weapon["base"] == "melee":
        wanted = WEAPON + MELEE
    if weapon["base"] == "repair":
        defaults.update(REPAIR_DEFAULTS)
        wanted = WEAPON + REPAIR
        aliases = REPAIR_ALIASES
    props = weapon["props"]
    cuts = [i for i, (key, _) in enumerate(props) if key == "NextCharge"]
    out = {"class": name, "base": weapon["base"], **effective({"props": props[: (cuts or [len(props)])[0]]}, wanted, defaults, aliases)}
    if weapon["base"] == "melee":
        out.update(melee_properties(props))
        return out
    if weapon["base"] == "dispenser":
        item = item_class(archives, out.get("OrdnanceName"))
        if item:
            out["item"] = item
        return out
    if weapon["base"] == "remote":
        droid = droid_class(archives, out.get("OrdnanceName"))
        if droid:
            out["droid"] = droid
        return out
    if weapon["base"] == "destruct":
        strike = ordnance_chain(out.get("StrikeOrdnanceName"))
        if strike:
            out["strike"] = strike
        return out
    ordnance = ordnance_chain(out.get("OrdnanceName"))
    if ordnance:
        out["ordnance"] = ordnance
    # Each charge level: its threshold and what it changes.
    levels = []
    for n, cut in enumerate(cuts):
        end = cuts[n + 1] if n + 1 < len(cuts) else len(props)
        level = effective({"props": props[:end]}, WEAPON, defaults)
        changed = {key: value for key, value in level.items() if key != "NextCharge" and out.get(key) != value}
        # A level that lists no pattern of its own has none.
        if out.get("ShotPatternPitchYaw") and "ShotPatternPitchYaw" not in level:
            changed["ShotPatternPitchYaw"] = []
        if "OrdnanceName" in changed:
            changed["ordnance"] = ordnance_chain(changed["OrdnanceName"])
        levels.append({"at": level["NextCharge"], **changed})
    if levels:
        out["charges"] = levels
    return out


def melee_properties(props):
    """Melee timing and deterministic cloned-class CanDeflect profile.

    The native copy constructor omits CanDeflect. Ordinary stock loads
    measured all five sabers as zero; custom absent values use the explicit
    cleared-allocation fallback, never the prototype's default one.
    """
    swings, authored = [], None
    for key, raw in props:
        if key == "SwingTime" and len(swings) < 3:
            parts = raw.split()
            try:
                duration = float(parts[0])
            except (ValueError, IndexError):
                continue
            try:
                hit = float(parts[1])
            except (ValueError, IndexError):
                hit = -1
            swings.append({"duration": duration, "hit": duration * .5 if hit < 0 else hit,
                           "sound": parts[2] if len(parts) > 2 else None})
        elif key == "CanDeflect":
            # Native atoi accepts a signed integer prefix; true/false are 0.
            match = re.match(r"\s*([+-]?\d+)", raw)
            authored = int(match.group(1)) if match else 0
    return {"swings": swings or [{"duration": 1, "hit": .5, "sound": None}],
            "canDeflectAuthored": authored,
            "CanDeflect": int(authored != 0) if authored is not None else 0,
            "deflectionProfile": "bf1-cloned-melee-cleared-v1" if authored is None else "authored",
            "RoundsPerClip": 0, "TriggerSingle": 1}


def find_class(archives, tag, name):
    """A class from the first archive that defines it."""
    for data in archives:
        found = classes(data, tag).get(name or "")
        if found:
            return found
    return None


def explosion_class(archives, name):
    """An explosion class's properties, or None."""
    explosion = find_class(archives, b"expc", name)
    if not explosion:
        return None
    return {"class": name, "base": explosion["base"], **effective(explosion, EXPLOSION)}


def item_class(archives, name):
    """A mine or power-up class (entc) with its explosions, or None."""
    item = find_class(archives, b"entc", name)
    if not item:
        return None
    out = {"class": name, "base": item["base"], **effective(item, ITEM, ITEM_DEFAULTS, ITEM_ALIASES)}
    for key in ("ExplosionTrigger", "ExplosionExpire", "ExplosionDeath"):
        if out.get(key):
            out[key] = explosion_class(archives, out[key]) or out[key]
    return out


def droid_class(archives, name):
    """A remote droid's class (entc, base droid): its settings, its death
    explosion, and its weapons, each with its chain."""
    droid = find_class(archives, b"entc", name)
    if not droid:
        return None
    out = {"class": name, "base": droid["base"], **effective(droid, DROID, DROID_DEFAULTS, aliases={})}
    if out.get("ExplosionName"):
        out["explosion"] = explosion_class(archives, out["ExplosionName"])
    sections = []
    for key, raw in droid["props"]:
        if key == "WeaponSection":
            sections.append({})
        elif key in ("WeaponName", "WeaponAmmo", "WeaponChannel") and sections:
            sections[-1][key] = raw
    weapons = []
    for n, section in enumerate(sections):
        chain = weapon_chain(archives, section.get("WeaponName"))
        if chain:
            weapons.append({"slot": n + 1, "weapon": section["WeaponName"], "ammo": int(float(section.get("WeaponAmmo", 0))),
                            "channel": int(float(section.get("WeaponChannel", 0))), **chain})
    out["weapons"] = weapons
    return out
