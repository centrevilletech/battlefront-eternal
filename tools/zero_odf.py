"""Entity classes (entc) and their ODF properties.

A munged class holds its whole ODF inheritance chain flattened into PROP
chunks: the FNV-1a hash of the property name, then the value as a C string.
Repeated keys keep their order, which is how CAMERASECTION groups work, and
CHUNKSECTION groups (the pieces a droid blows apart into), and the drop list
(DropItemClass and its DropItemProbability, NextDropItem moving to the next).

The 2004 soldier's acceleration property really is spelled "Acceleraton".
"""

import struct
import re

from ucfb import chunks, cstr_at, nodes
from zero_hash import fnv1a

NUMBERS = (
    "MaxHealth", "MaxSpeed", "MaxStrafeSpeed", "MaxTurnSpeed", "MaxPitchSpeed",
    "MaxJumpSpeed", "Acceleraton", "FirstPersonFOV", "ThirdPersonFOV",
    # The jetpack (the jet trooper).
    "JetJump", "JetPush", "JetFuelRechargeRate", "JetFuelCost", "JetFuelInitialCost", "JetFuelMinBorder",
    "ChunkFrequency", "AddHealth",
    "MapScale", "ScanningRange", "TransmitRange", "CapturePosts", "NoEnterVehicles", "CollisionRootScale",
)
# CollisionThreshold and CollisionScale: the speed into a surface that hurts
# and what each m/s past it costs, each as base, x the contact normal's up and
# x its forward (src/collision-damage.js). The jet trooper, the dark trooper
# and the Geonosian set the scale to 0 0 0: they take no fall damage.
VECTORS = ("PitchLimits", "CollisionThreshold", "CollisionScale")
STRINGS = (
    "Label", "UnitType", "GeometryName", "AnimationName", "SkeletonName", "FirstPerson", "JetEffect", "JetType",
    # Droids: damage taken as a droid; bodies re-skinned (the assault droid's
    # blue, the assassin's red); exploding into chunks.
    "HealthType", "OverrideTexture", "OverrideTexture2", "ExplosionName",
    # What gaining health or ammo plays on the soldier.
    "HealthGainEffect", "AmmoGainEffect",
    "MapTexture", "HealthTexture", "AISizeType", "AnimatedAddon", "GeometryAddon", "AnimationAddon", "AddonAttachJoint",
)
# The droideka (walkerdroid): walking by its clips, rolling as a ball. Two of
# its keys' names are unknown; the port calls them BallSpinScale (dc524dd9,
# rolling speed = spin x BallRadius / it) and AnimationSpeed (6b8d8679).
WALKER = (
    "StoppedTurnSpeed", "ForwardTurnSpeed", "TurnThreshold", "BallRadius", "BallMoveSpeed", "BallTurnSpeed",
    "BallAcceleration", "BallRollingFriction", "BallMaxLean", "MaxBallAngle", "BallMoveThreshold",
    "WaterDamageInterval", "WaterDamageAmount", "UprightWaterDamageHeight", "BallWaterDamageHeight",
    "DeathDustDelay", "MouseTurnDecay", "BallSpinScale", "AnimationSpeed",
)
WALKER_STRINGS = ("BallCollision", "DeathDustEffect", "WakeEffect", "HealthTexture", "UprightLowResModel", "BallLowResModel")
WALKER_VECTORS = ("DeathDustOffset", "YawLimits")
UNNAMED = {0xDC524DD9: "BallSpinScale", 0x6B8D8679: "AnimationSpeed",
           # Random explosions while an animated building dies.
           0x3E4E9BA0: "DeathAnimationExplosions"}
# A vehicle-like class's weapons, in WeaponSection groups, each with the
# aimers (hardpoints) that take turns firing it.
SECTION_KEYS = ("WeaponSection", "WeaponName", "WeaponAmmo", "WeaponChannel", "AimerNodeName", "NextAimer", "FireOutsideLimits")
# Foliage classes: grasspatch (cards around a point, Theed's flower beds) and
# leafpatch (leaves around a point or along a vine).
FOLIAGE = (
    "MinSize", "MaxSize", "NumParticles", "Texture", "NumParts", "Alpha", "MaxDistance", "BoxSize",
    "RadiusFadeMin", "RadiusFadeMax", "DarknessMin", "DarknessMax", "YOffset", "Seed", "Radius", "Height",
    "HeightScale", "ConeHeight", "Offset", "Vine", "VineLength", "VineSpread", "WiggleSpeed", "WiggleAmount",
)
# Dust and mist (dusteffect, tools/zero_atmosphere.py), beyond the foliage
# keys it shares; CameraDistance (follow the camera) and SpawnSound are unused.
DUST = ("MinPos", "MaxPos", "MinVel", "MaxVel", "MinLifetime", "MaxLifetime", "MinDistance", "CameraDistance", "SpawnSound")
CHUNK_KEYS = (
    "ChunkGeometryName", "ChunkNodeName", "ChunkSpeed", "ChunkOmega", "ChunkTerrainCollisions",
    "ChunkTerrainEffect", "ChunkTrailEffect", "ChunkPhysics",
)
DROP_KEYS = ("DropItemClass", "DropItemProbability", "NextDropItem")
CAMERA_VECTORS = ("EyePointOffset", "TrackCenter", "TrackOffset")
# Command posts and their holograms.
COMMAND_POST = (
    "NeutralizeTime", "CaptureTime", "HoloOdf", "HoloImageGeometry", "HoloTurnOnTime", "MapTexture",
    "MapScale", "Color", "NeutralColor", "FriendlyColor", "EnemyColor", "LocalsColor", "HoloType",
    "HoloVisibleDistance", "HoloHeight", "HoloSize", "HoloBeamIntensity", "HoloLightIntensity",
    "HoloImageIntensity", "HoloFlareIntensity", "HoloLightRadius", "HoloRotateRate", "HoloPopRate",
    "HoloFlickerRate", "HoloFlickerAlphaMin", "HoloFlickerAlphaMax", "HoloFadeInTime",
)
# Properties set on placed objects in a world.
INSTANCE = ("Team", "Label", "Layer", "CaptureRegion", "ControlRegion", "KillRegion", "SpawnPath", "AllyPath", "AllyCount", "Radius", "ValueBleed")
# Rendering: "Lighting dynamic" lights a class like a soldier.
RENDER = ("Lighting", "AttachOdf", "AttachToHardPoint")
# Which of a model's collision primitives each kind of thing collides with.
COLLISION = ("SoldierCollision", "VehicleCollision", "OrdnanceCollision", "BuildingCollision",
             "SoldierCollisionOnly", "VehicleCollisionOnly", "OrdnanceCollisionOnly", "BuildingCollisionOnly")
# Power-up droids (powerupstation) and turrets (armedbuilding).
POWERUP = (
    "PowerupDelay", "SoldierHealth", "SoldierAmmo", "VehicleHealth", "VehicleAmmo", "IdleRotateSpeed", "IdleWaitTime", "IdleWobbleNode",
    "IdleWobbleFactor", "ActiveRotateNode", "ActiveSpinNode", "MaxHealth",
)
TURRET = (
    "BuildingSection", "TurretSection", "TurretNodeName", "YawLimits", "PitchTurnFactor", "YawTurnFactor",
    "MaxPitchSpeed", "TurnFilter", "PitchFilter", "PilotPosition", "Pilot9Pose", "WeaponName", "WeaponAmmo",
    "AimerNodeName", "AimerPitchLimits", "AimerYawLimits", "AimerPitchRate", "AimerYawRate", "FirePointName",
    "UnbuiltGeometryName", "UnbuiltHoloOdf",
)
# Buildings: destroyed and built again.
BUILDING = ("DestroyedGeometryName", "Explosion", "RespawnTime", "HealthTexture", "BuildingSound", "BuiltSound")
# An animated building's clips (EntityBuildingAnimatedClass): the
# idle it loops and the clip it dies in, from the bank AnimationName names,
# and the explosions ("<class> <hardpoint>", up to 5) set off as that clip
# ends; a command post's AmbientSound ("<side> <sound>"). Names matched to
# the original game's hashes (297bc6eb, 5fac58b5, f138015c, 65a920ba).
ANIMATED_BUILDING = ("IdleAnimation", "DeathAnimation", "DeathEffect", "AmbientSound")
# Animated props and doors: their clips, what sets off their actions, a
# door's sounds, and a creature's grab (AttachTrigger), the pause between
# its idle loops (IdleDelay) and its swallow (KillSoldierSound). The last
# three names were matched to the original game's hashes (72306628, ff701082,
# f949d898); two flags it reads, 0550a3b2 and 6b014234, keep their hashes.
ANIMATED = ("Animation", "AnimationTrigger", "OpenSound", "CloseSound", "AttachTrigger", "IdleDelay", "KillSoldierSound")
# Traps (EntityTrapClass; Endor's Ewok log trap): the
# clips it swings in and is built back up in, where its build pad stands and
# what that pad is, the side whose vehicles set it off and the side it
# belongs to, how big a thing must be to set it off or be crushed, when into
# the swing it crushes, and what plays where. None of these names is in the
# original game; each was matched to its hash (docs/live-objects.md "Ewok log
# traps"). The stock trap leaves TriggerNode, TriggerOffset, TriggerRadius,
# TriggerSound, TriggerCollision and HideUnbuiltModel at their defaults.
TRAP = (
    "TriggerAnimation", "BuildAnimation", "BuildPoint", "BuildModelOdf", "TriggerTeam", "ResetTeam",
    "MinEnemyRadius", "DeathTime", "DeathEffectOffset", "BuiltCollision", "TriggerNode", "TriggerOffset",
    "TriggerRadius", "TriggerSound", "TriggerCollision", "HideUnbuiltModel",
)
KNOWN = (
    NUMBERS + VECTORS + STRINGS + CAMERA_VECTORS + ("CAMERASECTION", "TiltValue") + COMMAND_POST + INSTANCE
    + RENDER + COLLISION + POWERUP + TURRET + BUILDING + ANIMATED_BUILDING + ANIMATED + TRAP + ("CHUNKSECTION",) + CHUNK_KEYS + DROP_KEYS
    + WALKER + WALKER_STRINGS + WALKER_VECTORS + SECTION_KEYS + FOLIAGE + DUST
)
NAMES = {fnv1a(name): name for name in KNOWN}
NAMES.update(UNNAMED)


def property_name(key):
    """A property hash's name, or its hex when the name is not known."""
    return NAMES.get(key, "%08x" % key)


def entity_classes(data):
    out = {}
    for body, end in nodes(data, b"entc"):
        typ = base = None
        props = []
        for t, b, e in chunks(data, body, end):
            if t == b"TYPE":
                typ = cstr_at(data, b, e)
            elif t == b"BASE":
                base = cstr_at(data, b, e)
            elif t == b"PROP" and e - b >= 4:
                key = struct.unpack_from("<I", data, b)[0]
                props.append((property_name(key), cstr_at(data, b + 4, e)))
        if typ:
            out[typ] = {"base": base, "props": props}
    return out


def soldier_class(data, name):
    cls = entity_classes(data).get(name)
    if cls is None:
        return None
    out = {"class": name, "base": cls["base"], "cameras": {}}
    section = None
    chunks_, drops = [], [{}]
    sections = []
    for key, value in cls["props"]:
        if key == "WeaponSection":
            sections.append({"aimers": []})
        elif key in ("WeaponName", "WeaponAmmo", "WeaponChannel") and sections:
            sections[-1][key] = value
        elif key == "AimerNodeName" and sections:
            sections[-1]["aimers"].append(value)
        elif key == "CHUNKSECTION":
            chunks_.append({})
        elif key in CHUNK_KEYS and chunks_:
            numbers = value.split()
            try:
                chunks_[-1][key] = [float(v) for v in numbers] if len(numbers) > 1 else float(value)
            except ValueError:
                chunks_[-1][key] = value
        elif key == "DropItemClass":
            drops[-1]["class"] = value
        elif key == "DropItemProbability":
            drops[-1]["probability"] = float(value)
        elif key == "NextDropItem":
            drops.append({})
        elif key == "CAMERASECTION":
            section = out["cameras"].setdefault(value.upper(), {})
        elif key in CAMERA_VECTORS and section is not None:
            section[key] = [float(v) for v in value.split()]
        elif key == "TiltValue" and section is not None:
            section[key] = float(value)
        elif key in NUMBERS or key in WALKER:
            # The native numeric parser accepts the first numeric prefix;
            # stock Imperial infantry includes a malformed "7.5.0" value.
            match = re.match(r"\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)", value)
            out[key] = float(match.group(1)) if match else 0.0
        elif key in VECTORS or key in WALKER_VECTORS:
            out[key] = [float(v) for v in value.split()]
        elif key in STRINGS or key in WALKER_STRINGS:
            out[key] = value
    if chunks_:
        out["chunks"] = chunks_
    if sections:
        out["sections"] = sections
    drops = [d for d in drops if d.get("class")]
    if drops:
        out["drops"] = drops
    return out
