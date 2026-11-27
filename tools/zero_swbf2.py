"""2005 (Star Wars: Battlefront II) archive and chunk readers.

Chunk format layouts and config/ODF property key tables incorporate format
knowledge ported with attribution from:
- swbf-unmunge (MIT license): 2005 CLTH, plan, MTRL, world layers, config keys
- LibSWBF2 (GPL-3.0 license): zaf_ animation skeleton (FFAZ/LEKS/TNOJ), plnp

This module decodes the 2005 chunk types that the 2004 tools do not read
(or that differ structurally in 2005):
- `lght`: per-layer light configs (directional/point/spot lights + GlobalLights)
- `path`: lowercase config-tree paths (replacing 2004 uppercase binary PATH)
- `port`: PVS sectors and portals (capital ship interiors in space maps)
- `CLTH`: cloth meshes, fixed weights, triangle/spring constraints, collision
- `comb`: melee/lightsaber combo state machine configs
- `zaf_`: animation skeletons (bind pose joints paired with zaa_ banks)
- `plan`: 2005 AI planning graph (32B node header + variable weights + 26B arcs)
- mixed root + `lvl_` map layers and per-class side sub-archives
"""

import array
import json
import math
import sys
import struct

from portable_math import atan2
from ucfb import child, children_of, chunks, cstr_at, cstrings, f32s, named_subarchives, root_section, u32
from zero_config import parse as parse_config
from zero_hash import fnv1a
import zero_anim
import zero_ai_classes
import zero_odf
import zero_weapons
from zero_paths import write_text_lf  # noqa: E402

# ---------------------------------------------------------------- key tables

LIGHT_KEYS = {fnv1a(name): name for name in (
    "Light", "GlobalLights", "Light1", "Light2", "Top", "Bottom",
    "Rotation", "Position", "Type", "Color", "Range", "Cone",
    "CastShadow", "CastSpecular", "STATIC", "Region", "PS2BlendMode",
    "TileUV", "OffsetUV", "EditorGlobalDirIconSize", "Texture",
)}

PATH_KEYS = {fnv1a(name): name for name in (
    "Version", "PathCount", "Path", "Data", "PathType", "PathSpeedType",
    "PathTime", "OffsetPath", "SplineType", "Properties", "Property",
    "Nodes", "Node", "Position", "Rotation", "Knot", "Time", "PauseTime",
)}

PORTAL_KEYS = {fnv1a(name): name for name in (
    "Sector", "Base", "Height", "Point", "Object",
    "Portal", "Position", "Rotation", "Width", "Sector1", "Sector2",
)}

COMBO_KEYS = {fnv1a(name): name for name in (
    "AimType", "AlignedToThrust", "AnimatedMove", "Animation", "Attack",
    "AttackId", "BlendInTime", "BlendOutTime", "BlendTimeIn", "BlendTimeOut",
    "BlendTimeTo", "Break", "Button", "ButtonsPressed", "Damage",
    "DamageDistance", "DamageLength", "DamageTime", "DamageWidth",
    "Deflect", "DeflectAngle", "DeflectAnimation", "Duration", "Edge",
    "Energy", "EnergyCost", "EnergyRestoreRate", "Gravity",
    "GravityVelocityTarget", "If", "InputLock", "Loop", "LowResPose",
    "MustShowOneFrame", "OffhandAnimation", "Or", "PlayExplosion",
    "Posture", "Push", "RestartAnimation", "Sound", "State",
    "StrafeFactor", "SyncType", "ThrowAnimation", "Thrust", "ThrustAngle",
    "ThrustFactor", "TimeEnd", "TimeInPosture", "TimeStart", "Transition",
    "TurnFactor", "TurnOffLightsaber", "Until", "VelocityFromStrafe",
    "VelocityFromThrust", "VelocityX", "VelocityY", "VelocityZ",
)}

# 2005 ODF property names extending the 2004 zero_odf / zero_weapons /
# zero_ai_classes tables (from swbf-unmunge, MIT license).
SWBF2_ODF_KEYS = (
    "AIBubbleScaleClamp", "AIBubbleScaleDistDivider", "AIBubbleSizeMultiplier",
    "AICollisionRadius", "AITargetAnimal", "AITargetBuilding", "AITargetDroid",
    "AITargetPerson", "AITargetVehicle", "AIUseBubbleCircle", "AIUseHoverPhysics",
    "AddHitEffect", "AffectEnemies", "AffectFriends", "AmmoPerShot",
    "Animation", "AnimationTrigger", "ApplyOnOwnerIfOnOthers", "AreaHeight",
    "AreaRadius", "AreaRange", "AreaType", "AttachEffect", "AttachOdf",
    "AttachToHardPoint", "AttachedFirePoint", "AutoBarrierEnable",
    "AutoBarrierRemoveOnDeath", "AutoBarrierSize", "AutoPitchScreenDist",
    "AutoRespawnEnable", "AutoTurnScreenDist", "BallJumpForwardBoost",
    "BallJumpHeight", "BallSprintSpeedBoost", "BallSprintTurnSpeed",
    "BallStoppedTurnSpeed", "BarrelSound", "BarrelSoundFadeInTime",
    "BarrelSoundFadeOutTime", "BonesPerTentacle", "BonusAmplification",
    "BonusColor", "BoostAcceleration", "BoostFOV", "BoostSound", "BoostSpeed",
    "BoostTurnMultiplier", "BoostTurnSpeed", "BowlingPush", "BuffDefenseMult",
    "BuffDefenseTimer", "BuffHealthRate", "BuffHealthTimer", "BuffOffenseMult",
    "BuffOffenseTimer", "CameraBlendTime", "CarriedColorize",
    "CarriedGeometryName", "CarriedOffset", "ChokeSound", "ChunkGravity",
    "ChunkKeepSoldierCollision", "ChunkVelocityFactor", "CloseSound",
    "ClothODF", "Cockpit1stPersonSound", "Cockpit3rdPersonSound",
    "CollisionHeadOffset", "CollisionTorsoOffset", "CollisionWaterSound",
    "ColorizeByTeam", "ComboAnimationBank", "ContrailAttachOffset",
    "ContrailAttachPoint", "ContrailEffect", "ContrailEffectMaxScale",
    "ContrailEffectMinScale", "ContrailEffectMinSpeed", "ControlSpeed",
    "CoolDownTime", "CorrectAimForSteering", "DamageEdgeLength",
    "DamageEdgeWidth", "DamageEffectSound", "DamageFinalDamage",
    "DamageLength", "DamageRegionSound", "DamageTransitionDelay",
    "DamageTransitionPeriod", "DamageWidth", "Dampening", "DebuffDamageRate",
    "DebuffDamageTimer", "DeflectEffect", "DeflectSound",
    "DestructChargeEffect", "DestructTiltValue", "DestructTrackOffset",
    "DisplayRefire", "Drag", "DroidekaAttachBone", "DroppedColorize",
    "DroppedInLandingZoneGeometryName", "EffectDamage", "EffectFalloff",
    "EffectStrength", "EffectType", "EjectResistance", "EndLaserGlowColor",
    "EndLightColor", "EnergyAutoRestore", "EnergyBar", "EnergyBoostDrain",
    "EnergyCostBowling", "EnergyCostJump", "EnergyCostRoll", "EnergyDrain",
    "EnergyDrainRate", "EnergyDrainSprint", "EnergyMinSprint",
    "EnergyOverheat", "EnergyRestore", "EnergyRestoreIdle",
    "EnergyTrickDrainDoubleTap", "EnergyTrickDrainSingleTap",
    "FOVEFFectMaxCamOffset3rd", "FOVEFFectMinCamOffset3rd", "FOVEffect1st",
    "FOVEffect3rd", "FadeLightColor", "FireAnim", "FirePointExtraOffset",
    "FireType", "Firestop", "FleeLikeAHero", "FlickerAmplitude",
    "FlickerTimeMax", "FlickerTimeMin", "FlipSound", "FloatsOnWater",
    "ForceLockAimerToParentAimer", "GuidedMissile",
    "GuidedMissileBoostExplosionSpeedThreshold", "GuidedMissileDamage",
    "GuidedMissileDamageBoost", "GuidedMissileDeathCamMinDistance",
    "GuidedMissileDeathCamPause", "GuidedMissileDeathCamPullbackSpeed",
    "HUDIndexDisplay", "HUDModel", "HUDTag", "HealthTypeForLockOn",
    "HideOnEmpty", "HitEffect", "HitLocation", "HurtOwner", "ImmuneToMines",
    "InitialEnergyDrain", "InstantPlayFireAnim", "InvisibilityDecRate",
    "InvisibilityIncRate", "InvisibilityMax", "InvisibilityMin",
    "InvisibilityPersistTime", "InvisibilityTrigger", "JetAcceleration",
    "JetEnergyDrain", "JetShowHud", "JumpEnergyPerSec", "JumpForce",
    "JumpForwardSpeedBoost", "JumpForwardSpeedFactor", "JumpHeight",
    "JumpMinSpeedMult", "JumpStrafeSpeedFactor", "JumpTimeMax", "JumpTimeMin",
    "LODPriorityMod", "LevelFilterLanding", "LifeTime", "LightOdf",
    "LightSaberLength", "LightSaberTexture", "LightSaberTrailColor",
    "LightSaberWidth", "LightningEffectScale", "LockedSound", "MapSpeedMax",
    "MapSpeedMin", "MapViewMax", "MapViewMin", "MedalsTypeToLock",
    "MedalsTypeToUnlock", "MinFootHeight", "MineHealth", "MomentumFilter",
    "NeverCrashWhenUnpiloted", "NoChaining", "NoFirstPersonFireAnim",
    "NoLifeSpanDisplay", "NoTargetLightningEffectCount",
    "NoTargetLightningEffectRandomSpread", "NumDamageEdges", "NumTentacles",
    "OffhandFirePointName", "OffhandGeometryName", "OffhandWeapon", "OnSound",
    "OpenSound", "PassThrough", "PathFollowerBranchMaxAngle",
    "PathFollowerBranchPaths", "PathFollowerBranchRange", "PathFollowerClass",
    "PathFollowerClass_CTF", "PathFollowerFlyerVsFlyer",
    "PathFollowerMinBranchDist", "PickupSound", "PitchBuildupMultiplier",
    "PitchFilterDecel", "PlayEffectOnOwner", "PlayEffectOnOwnerAimer",
    "PointsToUnlock", "PrimaryWeaponIndex", "ProximityMaxDist",
    "ProximityMinDist", "PushDeadOnly", "RecoverFromTumble",
    "RemoveFromMapWhenDead", "ReticuleInAimingOnly", "ReturnDuration",
    "ReverseBackwardSteering", "RollRate", "RollSpeedFactor", "RollTime",
    "RotationRate", "ScoreForMedalsType", "SecondaryOrdnanceName",
    "SecondaryOrdnancePeriod", "ShotPerSalvo", "SmolderBone",
    "SmolderDamageRate", "SmolderEffect", "SmolderEffectTimer",
    "SmolderVanishDeath", "SndHeroDefeated", "SndHeroKiller",
    "SndHeroSpawned", "SoldierAnimation", "SoldierAttachBone",
    "SoldierEnergy", "SoldierMusic", "SpeedForInvisibilityMax",
    "SpeedForInvisibilityMin", "SprintAccelerateTime", "SprintTimeForBowling",
    "SquadronFormation", "SteerAtStrafeLimit", "SteerAtYawLimit",
    "SteerTowardAimFactor", "SwitchClassRadius", "TargetLock",
    "TargetLockMaxDistance", "TargetLockMaxDistanceLose", "TargetMine",
    "TargetPointOffsetCrouch", "TargetPointOffsetStand", "TentacleCollType",
    "ThrustAngleToStop", "ThrustAngleToStrafe", "ThrustAngleToTurn",
    "ThrustFactor", "TimeRequiredToEject", "TimeTilReboard",
    "TrickFlipCameraDetach", "TrickFlipSpeed", "TrickRollSpeed",
    "TrickSideRollCameraDetach", "TrickSideRollStrafeSpeed", "TrickSound",
    "TriggerAll", "TurnBuildupMultiplier", "TurnFilterDecel", "UnrollTime",
    "UseDirectionalJumps", "UseVCollForFlyers", "VOUnitType",
    "WakeWaterSplashEffect", "WarmUpSound", "WarmUpSoundFadeInTime",
    "WarmUpSoundFadeOutTime", "WarmUpSoundStartPitch", "WarmUpTime",
    "WaterSplashEffect", "WheelOmegaToU", "WheelOmegaToV", "WheelSection",
    "WheelTexture", "WheelVelocToU", "WheelVelocToV", "WindSpeed",
    "IsPilotExposed", "LevelDamp", "LevelFilter", "LevelSpring",
    "MaxChargeStrengthHeavy", "MaxChargeStrengthLight", "MaxPush", "MaxSpread",
    "MaxTerrainAngle", "MaxYawSpeed", "MinPush", "MinSpread", "ModeTexture",
    "MountPos", "MoveTensionX", "MoveTensionY", "MoveTensionZ",
    "MovingTurnOnly", "MusicDelay", "MusicSpeed", "NextBarrel",
    "NoCombatInterrupt", "NoDeathExplosions", "NoRandomSpring",
    "NormalDirection", "NumChunks", "OmegaXDamp", "OmegaXSpring",
    "OmegaZDamp", "OmegaZSpring", "OmniRadius", "PCMaxPitchSpeed",
    "PCMaxStrafeSpeed", "PCMaxTurnSpeed", "PCMaxYawSpeed", "PCPitchRate",
    "PCPitchTurnFactor", "PCSpinRate", "PCTurnRate", "PCYawTurnFactor",
    "PassengerSlots", "PilotAnimation", "PilotSkillRepairScale", "PitchDamp",
    "PushDeduction", "Range", "RayRadius", "RecoilDecayHeavy",
    "RecoilDecayLight", "RecoilDelayHeavy", "RecoilLengthHeavy",
    "RecoilLengthLight", "RecoilStrengthHeavy", "RecoilStrengthLight",
    "RefillFromItem", "RepMusic", "ReserveOneForPlayer", "ReverseSpeed",
    "RoundsPerSalvo", "STATIC", "SalvoTime", "SelfDestructSoundPitch",
    "SkeletonLowRes", "SkeletonRootScale", "SkeletonRootScaleLowRes",
    "SmashParkedFlyers", "SpawnPointCount", "SpawnPointLocation", "SpinRate",
    "StatusTexture", "StickBuildingDead", "StickBuildingUnbuilt",
    "StompDecal", "StompDecalSize", "StompDetectionType", "StompEffect",
    "StompThreshold", "StrafeRollAngle", "SuspensionLeftArmNodeName",
    "SuspensionMaxOffset", "SuspensionMidOffset", "SuspensionNodeName",
    "SuspensionRightArmNodeName", "TakeoffHeight", "TakeoffSpeed",
    "TargetableCollision", "TerrainCollision", "TerrainLeft", "TerrainRight",
    "ThrustAttachOffset", "ThrustAttachPoint", "ThrustEffect",
    "ThrustEffectMaxScale", "ThrustEffectMinScale", "ThrustEffectScaleStart",
    "ThrustPitchAngle", "TowCableCollision", "Traction", "VehiclePosition",
    "VelocityDamp", "VelocitySpring", "VisibleRadius", "WALKERSECTION",
    "WalkerLegPair", "WalkerOrientRoll", "WalkerWidth",
    "LegBoneLeft", "LegBoneRight", "LegBoneTopLeft", "LegBoneTopRight",
    "LegPairCount", "LegRayHitLength",
    "BuildingSound", "JumpSound", "LandSound", "LostSound", "LowHealthSound",
    "LowHealthThreshold", "OrdnanceEffect", "OrdnanceSound", "OverheatSound",
    "OverheatSoundPitch", "OverheatStopSound", "PowerupSound", "ProneSound",
    "ReloadSound", "RollSound", "SoundPitchDev", "SoundProperty",
    "SquatSound", "TakeoffSound", "TickSound", "TickSoundPitch",
    "TurnOffSound", "TurnOffTime", "TurnOnSound", "TurnOnTime",
    "TurningOffSound", "TurretActivateSound", "TurretAmbientSound",
    "TurretDeactivateSound", "TurretPitchSound", "TurretPitchSoundPitch",
    "TurretYawSound", "TurretYawSoundPitch", "VOSound",
    "VehicleCollisionSound", "WeaponChangeSound",
    "--UnbuiltGeometryName", "--UnbuiltHoloOdf",
    "acceleration", "attachedmesh", "priority", "transparent",
    # Additional property names from LibSWBF2's name dictionary (GPL-3.0).
    "AINoRepair", "AttachDynamic", "ChildAsteroid", "ChunkLifetime", "Collidable",
    "CurHealth", "CustomAnimationBank", "ExplosionBreak", "HaloRadius", "HideHealthBar",
    "IgnoreHintNodes", "IsAcklay", "IsNotTargetableByPlayer", "MaxOmega", "MinOmega",
    "Name", "RadiusDamageSizeScale", "Reflected", "ShadowCaster", "SoundOffset",
    "SoundWhenMoving", "StartMovementSound", "StopMovementSound", "Synchronize",
    "TargetPointOffset", "ttachEffect", "UseDirectionalDeaths",
    "Layer", "Team", "ControlZone", "SpawnPath", "AllyPath", "AllyCount",
    "CaptureRegion", "ControlRegion", "KillRegion", "SpawnRegion",
    "TurretPath", "EffectRegion", "Radius", "Height",
)

PROPERTY_NAMES = {}
PROPERTY_NAMES.update(zero_weapons.NAMES)
PROPERTY_NAMES.update(zero_ai_classes.NAMES)
for _n in zero_odf.KNOWN:
    PROPERTY_NAMES.setdefault(fnv1a(_n), _n)
PROPERTY_NAMES.update(zero_odf.UNNAMED)
for _n in SWBF2_ODF_KEYS:
    PROPERTY_NAMES.setdefault(fnv1a(_n), _n)


def property_name(key):
    return PROPERTY_NAMES.get(key, f"{key:08x}")


# ---------------------------------------------------------------- helpers


def _config_position(values):
    """World-editor config positions use the opposite Z axis to wrld transforms."""
    return [values[0], values[1], -values[2]]


def _config_rotation(values):
    """Inverse of swbf-unmunge's world transform conversion, in wxyz order."""
    w, x, y, z = values[:4]
    return [y, -z, w, -x]


def _quat_forward(qw, qx, qy, qz):
    """Forward direction (+Z rotated by quaternion [w, x, y, z]), normalized."""
    dx = 2.0 * (qx * qz + qw * qy)
    dy = 2.0 * (qy * qz - qw * qx)
    dz = 1.0 - 2.0 * (qx * qx + qy * qy)
    length = math.sqrt(dx * dx + dy * dy + dz * dz) or 1.0
    return [round(dx / length, 6), round(dy / length, 6), round(dz / length, 6)]


def _read_cstr(data, pos, end):
    z = data.find(b"\x00", pos, end)
    if z < 0:
        raise ValueError("Unterminated archive string")
    return data[pos:z].decode("latin1"), z + 1


# ---------------------------------------------------------------- lght


LIGHT_TYPES = {1: "directional", 2: "point", 3: "spot"}


def parse_lights(data, body, end):
    """Parse one `lght` config chunk into {nameHash, lights: [...], global: {...}}."""
    nc = child(data, body, end, b"NAME")
    name_hash = f"{u32(data, nc[0]):08x}" if nc and nc[1] - nc[0] >= 4 else None
    entries = parse_config(data, body, end, LIGHT_KEYS)
    lights = []
    global_lights = {}
    for key, values, kids in entries:
        if key == "Light" and values:
            item = {"name": str(values[0])}
            for k, v, _ in kids:
                if k == "Type" and v:
                    tcode = int(v[0])
                    item["type"] = LIGHT_TYPES.get(tcode, str(tcode))
                    item["typeCode"] = tcode
                elif k == "Color" and len(v) >= 3:
                    item["color"] = v[:3]
                elif k == "Position" and len(v) >= 3:
                    item["position"] = _config_position(v)
                elif k == "Rotation" and len(v) >= 4:
                    q = _config_rotation(v)
                    item["rotation"] = q
                    item["direction"] = _quat_forward(*q)
                elif k == "Range" and v:
                    item["range"] = round(float(v[0]), 6)
                elif k == "Cone" and v:
                    item["cone"] = [round(float(x), 6) for x in v[:2]]
                elif k == "CastShadow":
                    item["castShadow"] = True
                elif k == "CastSpecular" and v:
                    item["castSpecular"] = bool(v[0])
                elif k == "STATIC":
                    item["static"] = True
                elif k == "Region" and v:
                    item["region"] = str(v[0])
                elif k == "Texture" and v:
                    item["texture"] = str(v[0])
            lights.append(item)
        elif key == "GlobalLights":
            for k, v, _ in kids:
                if k == "Light1" and v:
                    global_lights["light1"] = str(v[0])
                elif k == "Light2" and v:
                    global_lights["light2"] = str(v[0])
                elif k == "Top" and len(v) >= 3:
                    global_lights["top"] = [x / 255.0 for x in v[:3]]
                elif k == "Bottom" and len(v) >= 3:
                    global_lights["bottom"] = [x / 255.0 for x in v[:3]]
    return {"nameHash": name_hash, "lights": lights, "global": global_lights}


# ---------------------------------------------------------------- path (lowercase)


def parse_paths(data, body, end):
    """Parse one 2005 lowercase `path` config chunk.
    Returns (spawn_paths_dict, detailed_paths_list):
    - spawn_paths_dict: {name: [[x, y, z, yaw], ...]} matching zero_world.spawn_paths
    - detailed_paths_list: full node positions, rotations, spline/property metadata
    """
    entries = parse_config(data, body, end, PATH_KEYS)
    by_name = {}
    detailed = []
    for key, values, kids in entries:
        if key != "Path" or not values:
            continue
        pname = str(values[0])
        points = []
        nodes_out = []
        spline = None
        path_type = 0
        for k, v, sub in kids:
            if k == "SplineType" and v:
                spline = str(v[0])
            elif k == "PathType" and v:
                path_type = int(v[0])
            elif k == "Nodes":
                for nk, _, nkids in sub:
                    if nk != "Node":
                        continue
                    pos = [0.0, 0.0, 0.0]
                    rot = [1.0, 0.0, 0.0, 0.0]
                    props = {}
                    for pk, pv, psub in nkids:
                        if pk == "Position" and len(pv) >= 3:
                            pos = _config_position(pv)
                        elif pk == "Rotation" and len(pv) >= 4:
                            # Path PNTS has its own rotation swizzle (handle_path.cpp).
                            w, x, y, z = pv[:4]
                            rot = [y, z, w, -x]
                        elif pk == "Properties":
                            for prk, prv, _ in psub:
                                if prk == "Property" and len(prv) >= 2:
                                    props[str(prv[0])] = prv[1]
                    yaw = 2.0 * atan2(rot[2], rot[0])
                    points.append([pos[0], pos[1], pos[2], yaw])
                    node_rec = {"position": pos, "rotation": rot}
                    if props:
                        node_rec["properties"] = props
                    nodes_out.append(node_rec)
        by_name[pname] = points
        detailed.append({
            "name": pname,
            "pathType": path_type,
            "spline": spline,
            "nodes": nodes_out,
        })
    return by_name, detailed


# ---------------------------------------------------------------- port


def parse_portals(data, body, end):
    """Parse one `port` PVS config chunk into {sectors: [...], portals: [...]}."""
    entries = parse_config(data, body, end, PORTAL_KEYS)
    sectors = []
    portals = []
    for key, values, kids in entries:
        if key == "Sector" and values:
            sec = {"name": str(values[0]), "base": 0.0, "height": 0.0, "points": [], "objects": []}
            for k, v, _ in kids:
                if k == "Base" and v:
                    sec["base"] = float(v[0])
                elif k == "Height" and v:
                    sec["height"] = float(v[0])
                elif k == "Point" and len(v) >= 2:
                    sec["points"].append([v[0], -v[1]])
                elif k == "Object" and v:
                    sec["objects"].append(str(v[0]))
            sectors.append(sec)
        elif key == "Portal" and values:
            por = {"name": str(values[0])}
            for k, v, _ in kids:
                if k == "Position" and len(v) >= 3:
                    por["position"] = _config_position(v)
                elif k == "Rotation" and len(v) >= 4:
                    por["rotation"] = _config_rotation(v)
                elif k == "Width" and v:
                    por["width"] = float(v[0])
                elif k == "Height" and v:
                    por["height"] = float(v[0])
                elif k == "Sector1" and v:
                    por["sector1"] = str(v[0])
                elif k == "Sector2" and v:
                    por["sector2"] = str(v[0])
            portals.append(por)
    return {"sectors": sectors, "portals": portals}


# ---------------------------------------------------------------- CLTH


def parse_cloth(data, body, end):
    """Parse one 2005 `CLTH` cloth mesh chunk (ported from swbf-unmunge, MIT).

    Layout: INFO (model name), NAME (cloth name), PRNT (parent bone),
    XFRM (4x3 transform, 12 floats), DATA (texture string, vertex count,
    vec3 positions, vec2 UVs, fixed point count, fixed bone weight names,
    triangle indices, stretch/bend/cross index pairs), COLL (primitive count,
    each parent bone string + type u32 + size vec3 + mat3 + pos vec3).
    """
    info_c = child(data, body, end, b"INFO")
    name_c = child(data, body, end, b"NAME")
    prnt_c = child(data, body, end, b"PRNT")
    xfrm_c = child(data, body, end, b"XFRM")
    data_c = child(data, body, end, b"DATA")
    coll_c = child(data, body, end, b"COLL")
    if not (info_c and name_c and data_c):
        return None
    out = {
        "model": cstr_at(data, *info_c),
        "name": cstr_at(data, *name_c),
        "parent": cstr_at(data, *prnt_c) if prnt_c else "",
        "transform": list(f32s(data, xfrm_c[0], 12)) if xfrm_c and xfrm_c[1] - xfrm_c[0] >= 48 else None,
    }
    pos, stop = data_c
    tex, pos = _read_cstr(data, pos, stop)
    out["texture"] = tex
    def count():
        nonlocal pos
        if pos + 4 > stop:
            raise ValueError("Truncated cloth count")
        pos += 4
        return u32(data, pos - 4)

    def records(n, fmt):
        nonlocal pos
        size = n * struct.calcsize(fmt)
        if size > stop - pos:
            raise ValueError("Truncated cloth array")
        pos += size
        return [list(row) for row in struct.iter_unpack(fmt, memoryview(data)[pos - size:pos])]

    vcount = count()
    out["positions"] = records(vcount, "<3f")
    out["uvs"] = records(vcount, "<2f")
    fixed = count()
    if fixed > vcount:
        raise ValueError("Cloth fixed vertex count exceeds mesh size")
    out["fixedPoints"] = list(range(fixed))
    weights = []
    for _ in range(count()):
        weight, pos = _read_cstr(data, pos, stop)
        weights.append(weight)
    out["fixedWeights"] = weights
    for key, fmt in (("triangles", "<3I"), ("stretchConstraints", "<2I"),
                     ("bendConstraints", "<2I"), ("crossConstraints", "<2I")):
        out[key] = records(count(), fmt)
        if any(i >= vcount for row in out[key] for i in row):
            raise ValueError("Cloth vertex index outside mesh")
    collisions = []
    if coll_c and coll_c[1] - coll_c[0] >= 4:
        cpos, cstop = coll_c
        ccount = u32(data, cpos)
        cpos += 4
        for _ in range(ccount):
            if cpos >= cstop:
                raise ValueError("Truncated cloth collision array")
            parent, cpos = _read_cstr(data, cpos, cstop)
            if cpos + 64 > cstop:
                raise ValueError("Truncated cloth collision primitive")
            ctype = u32(data, cpos)
            cvals = f32s(data, cpos + 4, 15)
            cpos += 64
            collisions.append({
                "parent": parent,
                "type": ctype,
                "size": list(cvals[0:3]),
                "rotation": list(cvals[3:12]),
                "position": list(cvals[12:15]),
            })
    out["collision"] = collisions
    return out


# ---------------------------------------------------------------- comb


def _tree_to_dict(entries):
    out = []
    for key, values, kids in entries:
        node = {"key": key}
        if values:
            node["values"] = values
        if kids:
            node["children"] = _tree_to_dict(kids)
        out.append(node)
    return out


def parse_combo(data, body, end):
    """Parse one 2005 `comb` melee/lightsaber combo state machine config chunk."""
    nc = child(data, body, end, b"NAME")
    name_hash = f"{u32(data, nc[0]):08x}" if nc and nc[1] - nc[0] >= 4 else None
    entries = parse_config(data, body, end, COMBO_KEYS)
    offhand = []
    animations = []
    throws = []
    states = []
    other = []
    for key, values, kids in entries:
        rec = {"values": values}
        if kids:
            rec["children"] = _tree_to_dict(kids)
        if key == "OffhandAnimation":
            offhand.append(rec)
        elif key == "Animation":
            animations.append(rec)
        elif key == "ThrowAnimation":
            throws.append(rec)
        elif key == "State":
            rec["name"] = str(values[0]) if values else ""
            states.append(rec)
        else:
            rec["key"] = key
            other.append(rec)
    out = {
        "nameHash": name_hash,
        "offhandAnimations": offhand,
        "animations": animations,
        "throwAnimations": throws,
        "states": states,
    }
    if other:
        out["extra"] = other
    return out


# ---------------------------------------------------------------- zaf_


def parse_zaf(data, body, end):
    """Parse one `zaf_` animation skeleton chunk (ported from LibSWBF2, GPL-3.0).

    Contains NAME (string) and BIN_ holding reversed-tag chunks:
    FFAZ header -> SREV (version u32 = 8), LEKS -> TNOJ records
    (bone_crc u32, base_rotation 4xf32, base_position 3xf32,
     parent_bone_crc u32, 2x u32).
    """
    nc = child(data, body, end, b"NAME")
    bc = child(data, body, end, b"BIN_")
    if not (nc and bc):
        return None
    name = cstr_at(data, *nc)
    root = child(data, *bc, b"FFAZ")
    if root is None:
        raise ValueError(f"{name}: missing FFAZ")
    version = child(data, *root, b"SREV")
    skeleton = child(data, *root, b"LEKS")
    if not version or version[1] - version[0] != 4 or not skeleton:
        raise ValueError(f"{name}: incomplete skeleton")
    joints = []
    for start, stop in children_of(data, *skeleton, b"TNOJ"):
        if stop - start != 44:
            raise ValueError(f"{name}: invalid joint size")
        bone, *values = struct.unpack_from("<I4f3f3I", data, start)
        joints.append({"boneCrc": f"{bone:08x}", "parentCrc": f"{values[7]:08x}",
                       "rotation": values[:4], "position": values[4:7]})
    return {"name": name, "version": u32(data, version[0]), "joints": joints}


def _write_bank(blob, stream):
    if len(blob) < 24 or blob[:4] != b"SMNA" or u32(blob, 8) != 8:
        raise ValueError("Unsupported animation bank header")
    clips, curves = zero_anim.parse_bank(blob)
    if sum(len(bones) for _, _, _, bones in clips) != u32(blob, 12) or len(curves) < u32(blob, 16):
        raise ValueError("Animation bank section size mismatch")
    records = []
    for name_crc, flags, frames, bones in clips:
        tracks = zero_anim.decode_clip(bones, frames, curves, {b[0]: b[0] for b in bones})
        records.append({"crc": f"{name_crc:08x}", "flags": flags, "frames": frames,
                        "bones": len(bones), "tracks": [f"{key:08x}" for key, _ in tracks],
                        "offset": stream.tell() // 4})
        for _, samples in tracks:
            floats = array.array("f", samples)
            if sys.byteorder != "little":
                floats.byteswap()
            stream.write(floats.tobytes())
    return {"clipCount": len(records), "clips": records}


def write_animation_banks(sources, out):
    """Model-independent tracks, indexed by bone CRC so each actor can bind its own skeleton."""
    banks, rigs, packages, sets = {}, {}, {}, {}
    with (out / "anim_banks.bin").open("wb") as stream:
        for data, sections in sources:
            owners = {(b, e): f"{h:08x}" for h, b, e in named_subarchives(data)}
            for start, stop in sections:
                for tag, b, e in chunks(data, start, stop):
                    if tag not in (b"zaa_", b"zaf_", b"sanm"):
                        continue
                    nc = child(data, b, e, b"NAME")
                    if not nc:
                        raise ValueError(f"Missing name on {tag!r}")
                    if tag == b"sanm":
                        sets[f"{u32(data, nc[0]):08x}"] = _tree_to_dict(parse_config(data, b, e))
                        continue
                    name = cstr_at(data, *nc)
                    group = packages.setdefault(owners.get((start, stop), "root"), {"banks": [], "skeletons": []})
                    entries = group["banks" if tag == b"zaa_" else "skeletons"]
                    if name not in entries:
                        entries.append(name)
                    if tag == b"zaa_" and name not in banks:
                        bc = child(data, b, e, b"BIN_")
                        if not bc:
                            raise ValueError(f"{name}: missing animation data")
                        banks[name] = _write_bank(data[bc[0]:bc[1]], stream)
                    elif tag == b"zaf_" and name not in rigs:
                        rigs[name] = parse_zaf(data, b, e)
    index = {"fps": zero_anim.FPS, "data": "anim_banks.bin", "banks": banks,
             "skeletons": rigs, "packages": packages, "sets": sets}
    write_text_lf(out / "anim_banks.json", json.dumps(index, allow_nan=False))
    return index


# ---------------------------------------------------------------- plan (2005)


def parse_plnp(data, b, e):
    """Dynamic planning groups, their FNV arc ids and size-delimited zlib blocks."""
    if b >= e or b + 1 + data[b] >= e:
        raise ValueError("Truncated planning groups")
    at = b + 1 + data[b]
    counts = list(data[b + 1:at])
    n = data[at]
    at += 1
    if sum(counts) != n or at + n * 4 > e:
        raise ValueError("Invalid planning connection count")
    hashes = [f"{h:08x}" for (h,) in struct.iter_unpack("<I", data[at:at + n * 4])]
    at += n * 4
    blocks = []
    packed = bytearray()
    while at < e:
        if at + 8 > e:
            raise ValueError("Truncated planning block")
        unpacked, size = struct.unpack_from("<II", data, at)
        at += 8
        if at + size > e:
            raise ValueError("Truncated compressed planning data")
        blocks.append({"offset": len(packed), "size": size, "unpackedSize": unpacked})
        packed.extend(data[at:at + size])
        at += size
    return {"groupCounts": counts, "connectionHashes": hashes, "data": "planning.bin", "blocks": blocks}, packed


def parse_plan_v2(data, body, end):
    """Parse a 2005 `plan` AI planning graph chunk (ported from swbf-unmunge, MIT).

    INFO: u16 hub_count, u16 arc_count, u16 extra (5).
    NODE: hub_count records of 32B (name[16], x, y, z, radius) + 8B arc slots
          + 5B weight_counts + sum(weight_counts) * hub_count bytes.
    ARCS: arc_count records of 26B (name[16], u8 start, u8 end, u32 filter_flags,
          u32 type_flags [bit0=oneWay, bit1=jump, bit2=jetJump]).
    """
    info_c = child(data, body, end, b"INFO")
    node_c = child(data, body, end, b"NODE")
    arcs_c = child(data, body, end, b"ARCS")
    if not (info_c and node_c and arcs_c) or info_c[1] - info_c[0] < 6:
        return None
    hub_count, arc_count, branch_count = struct.unpack_from("<HHH", data, info_c[0])
    hubs = []
    pos, stop = node_c
    for _ in range(hub_count):
        if pos + 45 > stop:
            raise ValueError("Truncated planning hub")
        raw_name = data[pos:pos + 16].split(b"\x00", 1)[0].decode("latin1")
        x, y, z, radius = f32s(data, pos + 16, 4)
        slots = list(data[pos + 32:pos + 40])
        wcounts = list(data[pos + 40:pos + 45])
        pos += 45
        end_weights = pos + sum(wcounts) * hub_count
        if end_weights > stop:
            raise ValueError("Truncated planning weights")
        weights = list(data[pos:end_weights])
        pos = end_weights
        hubs.append({"name": raw_name, "position": [x, y, z], "radius": radius, "slots": slots,
                     "weightCounts": wcounts, "weights": weights})
    arcs = []
    apos, astop = arcs_c
    for _ in range(arc_count):
        if apos + 26 > astop:
            raise ValueError("Truncated planning arc")
        raw_name = data[apos:apos + 16].split(b"\x00", 1)[0].decode("latin1")
        start_i = data[apos + 16]
        end_i = data[apos + 17]
        filt, tflags = struct.unpack_from("<II", data, apos + 18)
        apos += 26
        arcs.append({
            "name": raw_name,
            "start": start_i,
            "end": end_i,
            "filterFlags": filt,
            "typeFlags": tflags,
            "oneWay": bool(tflags & 1),
            "jump": bool(tflags & 2),
            "jetJump": bool(tflags & 4),
        })
    return {"hubCount": hub_count, "arcCount": arc_count, "branchCount": branch_count, "hubs": hubs, "arcs": arcs}


# ---------------------------------------------------------------- world layers


def world_animations(data, worlds):
    """Authored motion, named object bindings and parent-child attachments in wrld."""
    clips, groups, hierarchies = {}, {}, []
    for _, start, stop in worlds:
        for tag, b, e in chunks(data, start, stop):
            if tag not in (b"anim", b"anmg", b"anmh"):
                continue
            info, end = child(data, b, e, b"INFO")
            if tag == b"anmh":
                names = cstrings(data, info + 1, end, data[info])
                if len(names) != data[info] or not names:
                    raise ValueError("Invalid animation hierarchy")
                hierarchies.append({"parent": names[0], "children": names[1:]})
                continue
            name, at = _read_cstr(data, info, end)
            if tag == b"anmg":
                default, controlled = struct.unpack("<BB", data[at:end])
                bindings = []
                for ab, ae in children_of(data, b, e, b"ANIM"):
                    clip, pos = _read_cstr(data, ab, ae)
                    obj, _ = _read_cstr(data, pos, ae)
                    bindings.append({"clip": clip, "object": obj})
                groups[name] = {"defaultOn": bool(default), "stopWhenControlled": bool(controlled),
                                "disableHierarchies": child(data, b, e, b"NOHI") is not None,
                                "bindings": bindings}
            else:
                length, loop, local = struct.unpack("<fBB", data[at:end])
                clip = {"duration": length, "loop": bool(loop), "localTranslation": bool(local),
                        "positions": [], "rotations": []}
                for key, kb, ke in chunks(data, b, e):
                    if key in (b"POSK", b"ROTK"):
                        time, x, y, z, kind, *tangents = struct.unpack("<4fB6f", data[kb:ke])
                        clip["positions" if key == b"POSK" else "rotations"].append(
                            {"time": time, "value": [x, y, z], "type": kind, "tangents": tangents})
                clips[name] = clip
    return {"clips": clips, "groups": groups, "hierarchies": hierarchies}


def boundary_paths(data, body, end):
    # Path values in bnd_ are native FNV words, including bit patterns that
    # cannot round-trip through a floating-point value.
    paths = []
    for scope in children_of(data, body, end, b"SCOP"):
        for b, e in children_of(data, *scope, b"DATA"):
            if e - b >= 9 and u32(data, b) == fnv1a("Path") and data[b + 4] == 1:
                paths.append(u32(data, b + 5))
    return paths


def world_sections(data, map_id, requested_names=(), *, name_parts=((), ()), keep_unnamed=False):
    """World packages indexed by their stored FNV. Each may have a matching source name."""
    root = root_section(data)
    if root is None:
        return {"base": None, "layers": {}}

    def _scan_section(sb, se):
        wrlds = []
        lights = []
        paths_map = {}
        paths_detail = []
        portals = None
        for t, b, e in chunks(data, sb, se):
            if t == b"wrld":
                nc = child(data, b, e, b"NAME")
                wname = cstr_at(data, *nc) if nc else ""
                wrlds.append((wname, b, e))
            elif t == b"lght":
                lights.append(parse_lights(data, b, e))
            elif t == b"path":
                pmap, pdet = parse_paths(data, b, e)
                paths_map.update(pmap)
                paths_detail.extend(pdet)
            elif t == b"port":
                portals = parse_portals(data, b, e)
        return {
            "range": (sb, se),
            "worlds": wrlds,
            "lights": lights,
            "paths": paths_map,
            "pathsDetail": paths_detail,
            "portals": portals,
        }

    base = _scan_section(root[0], root[1])
    layers = {}
    prefix = map_id.lower() + "_"
    suffixes, roles = name_parts
    for h, sb, se in named_subarchives(data):
        sec = _scan_section(sb, se)
        key = f"{h:08x}"
        candidates = {name.lower() for name in requested_names}
        for world, _, _ in sec["worlds"]:
            pieces = world.lower().split("_")
            candidates.update("_".join(pieces[:n]) for n in range(1, len(pieces) + 1))
            if world.lower().startswith(prefix):
                candidates.update(prefix + role + world[len(prefix):].lower() for role in roles)
        if sec["worlds"]:
            candidates.update(prefix + suffix for suffix in suffixes)
        matches = sorted(name for name in candidates if fnv1a(name) == h)
        if len(matches) > 1 or key in layers:
            raise ValueError(f"{map_id}: ambiguous layer hash {key}, candidates {matches}")
        sec.update(hash=key, name=matches[0] if matches else None)
        if not matches:
            message = f"{map_id}: unresolved layer hash {key}, candidates {sorted(candidates)}"
            if not keep_unnamed:
                raise ValueError(message)
            sec["nameError"] = {"hash": key, "candidates": sorted(candidates), "message": message}
        layers[key] = sec
    return {"base": base, "layers": layers}


def requested_layers(sections, names, map_id):
    ordered = []
    for name in names:
        key = f"{fnv1a(name):08x}"
        if key not in sections or sections[key]["name"] != name.lower():
            available = [s["name"] or s["hash"] for s in sections.values()]
            raise ValueError(f"{map_id}: requested layer {name!r} ({key}) is absent, candidates {available}")
        ordered.append(sections[key])
    return ordered


def package_sections(data, names):
    """Root and requested lvl_ packages. An explicit ODF id may live in a package of another name."""
    root = root_section(data)
    if root is None:
        raise ValueError("Expected a ucfb archive")
    if not names:
        from ucfb import iter_all_archives
        return list(iter_all_archives(data))
    packages = list(named_subarchives(data))
    hashes = {h for h, _, _ in packages}
    direct = {fnv1a(n) for n in names}
    other = {n.lower() for n in names if fnv1a(n) not in hashes}
    selected = [root]
    for h, b, e in packages:
        if h in direct or (other and any(
                cstr_at(data, *typ).lower() in other for tag, cb, ce in chunks(data, b, e)
                if tag == b"entc" and (typ := child(data, cb, ce, b"TYPE")))):
            selected.append((b, e))
    return selected


def read_all_classes(data, sections=None):
    """Read every entc/wpnc/ordc/expc class across sections (default: root + all lvl_
    sub-archives), keeping every ODF key/value using the combined 2004+2005 property
    name table. Returns {tag_str: {class_name: {"base": ..., "props": [(k, v), ...], "subarchive": hash_or_None}}}."""
    out = {"entc": {}, "wpnc": {}, "ordc": {}, "expc": {}}
    if sections is None:
        sec_list = []
        root = root_section(data)
        if root is not None:
            sec_list.append((None, root[0], root[1]))
            for h, sb, se in named_subarchives(data):
                sec_list.append((f"{h:08x}", sb, se))
    else:
        sec_list = [(None, sb, se) for sb, se in sections]

    for sub_hash, sb, se in sec_list:
        for t, b, e in chunks(data, sb, se):
            if t not in (b"entc", b"wpnc", b"ordc", b"expc"):
                continue
            typ = base = None
            props = []
            for ct, cb, ce in chunks(data, b, e):
                if ct == b"TYPE":
                    typ = cstr_at(data, cb, ce)
                elif ct == b"BASE":
                    base = cstr_at(data, cb, ce)
                elif ct == b"PROP" and ce - cb >= 4:
                    props.append((property_name(u32(data, cb)), cstr_at(data, cb + 4, ce)))
            if typ:
                bucket = out[t.decode("latin1")]
                if typ not in bucket:
                    bucket[typ] = {"base": base, "props": props, "subarchive": sub_hash}
    return out


# ClassLabel vocabulary (swbf-unmunge, MIT), with the additional 2005 labels.
# A missing ClassParent is not a ClassLabel.
ENGINE_BASES = {
    "entc": frozenset("""
        animatedbuilding animatedprop armedbuilding armedbuildingdynamic asteroid beacon building catapult
        cloth commandarmedanimatedbuilding commandflyer commandhover commandpost commandwalker
        defensegridturret destructablebuilding door droid dusteffect flag flyer godray grasspatch
        hologram hover leafpatch light mine portableturret powerupitem powerupstation prop
        remoteterminal rumbleeffect soldier soundambiencestatic soundambiencestreaming trap vehiclepad
        vehiclespawn walker walkerdroid water
    """.split()),
    "wpnc": frozenset("""
        areaeffectweapon binoculars cannon cloudcluster destruct detonator disguise dispenser grapplinghookweapon
        grenade haywire invisibility launcher melee meleethrow remote repair shield towcableweapon weapon
    """.split()),
    "ordc": frozenset("""
        beam bolt bullet cloudcluster emitterordnance fatray grapplinghook meleethrowordnance mine missile
        shell sticky towcable
    """.split()),
    "expc": frozenset({"explosion"}),
}


def class_catalog(sources):
    """Loaded definitions take precedence. Only ancestor ODFs enter from unloaded packages."""
    from ucfb import iter_all_archives
    loaded = {tag: {} for tag in ENGINE_BASES}
    other = {tag: {} for tag in ENGINE_BASES}
    for data, selected in sources:
        included = set(selected)
        rest = (span for span in iter_all_archives(data) if span not in included)
        for spans, catalog in ((selected, loaded), (rest, other)):
            for tag, classes in read_all_classes(data, spans).items():
                for name, entry in classes.items():
                    catalog[tag].setdefault(name.lower(), {"name": name, **entry})
    for tag, classes in loaded.items():
        pending = list(classes.values())
        while pending:
            parent = (pending.pop()["base"] or "").lower()
            if parent not in classes and parent in other[tag]:
                classes[parent] = other[tag][parent]
                pending.append(classes[parent])
    return loaded


def resolve_class(tag, name, all_classes):
    """Resolve a 2005 ODF class inheritance chain.

    Unlike 2004 (where munged classes flattened their parent ODFs into the
    child chunk and set BASE to the engine class), 2005 classes set BASE to
    their parent ODF class. Follow each parent until an engine class is reached.

    Returns {"name", "base" (immediate BASE), "engineBase" (root engine class),
    "parents" (rootmost..immediate parent ODFs), "props" (merged ordered
    (key, value) pairs, parent first then child), "ownProps" (child's own
    props), "subarchive"}, or None if not found.
    """
    bucket = all_classes.get(tag) or {}
    cur = bucket.get(name.lower())
    if cur is None:
        return None
    chain = []
    seen = set()
    node = cur
    engine_base = None
    problem = None
    while node is not None:
        key = node["name"].lower()
        if key in seen:
            problem = {"parent": node["name"], "reason": "cycle"}
            break
        seen.add(key)
        chain.append(node)
        parent_name = node.get("base") or ""
        parent = bucket.get(parent_name.lower()) if parent_name else None
        if parent is None:
            if parent_name.lower() in ENGINE_BASES[tag]:
                engine_base = parent_name
            else:
                problem = {"parent": parent_name, "reason": "missing"}
            break
        node = parent
    chain.reverse()  # rootmost parent first, target class last
    merged_props = []
    for item in chain:
        merged_props.extend(item.get("props") or ())
    return {
        "name": cur["name"],
        "base": cur.get("base"),
        "engineBase": engine_base,
        **({"parentProblem": problem} if problem else {}),
        "parents": [item["name"] for item in chain[:-1]],
        "props": merged_props,
        "ownProps": list(cur.get("props") or ()),
        "subarchive": cur.get("subarchive"),
    }
