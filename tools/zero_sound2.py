"""Audio requests, graph bindings, room attenuation and class sounds for 2005."""

import struct

from zero_hash import fnv1a
import zero_sound as sound
from zero_swbf2 import class_catalog, resolve_class


def mission_audio(calls):
    mission = sound.read_mission_audio(calls, low_voice=False)
    opens = []
    mission.update(appendedStreams=[], streamRequests=[], scaledParameters=[], lowReinforcementsVo=[])
    for name, args in calls:
        if name == "OpenAudioStream" and len(args) >= 2:
            opens.append(args[1])
            mission["streamRequests"].append(args[:2])
        elif name == "AudioStreamAppendSegments" and len(args) >= 3:
            archive, stream, slot = args[:3]
            mission["streamRequests"].append([archive, stream])
            if type(slot) in (int, float) and 1 <= slot <= len(opens) and int(slot) == slot:
                mission["appendedStreams"].append([opens[int(slot) - 1], stream])
            else:
                mission.setdefault("unresolvedStreamAppends", []).append(list(args))
        elif name == "ScriptCB_SetDopplerFactor" and args:
            mission["dopplerFactor"] = args[0]
        elif name == "ScaleSoundParameter" and len(args) >= 3:
            mission["scaledParameters"].append(args[:3])
        elif name == "SetLowReinforcementsVoiceOver" and len(args) >= 4:
            mission["lowReinforcementsVo"].append(sound.low_voice_rule(args))
        elif name == "ScriptCB_EnableHeroMusic" and args:
            mission["enableHeroMusic"] = bool(args[0])
        elif name == "ScriptCB_EnableHeroVO" and args:
            mission["enableHeroVo"] = bool(args[0])
    return mission


CONFIG = {**sound.CONFIG, fnv1a("MaxPlaybackTime"): "MaxPlaybackTime", 0x148F0D58: "SpaceAttenuations"}


def _value(kind, key, index, raw):
    if kind == "space" and key == "space" and index == 0:
        return sound.hexname(struct.unpack("<I", raw)[0])
    return sound._config_value(kind, key, index, raw)


def config_objects(data, configs):
    objects = []
    for tag, start, end in configs:
        for kind, _, fields in sound._entries(data, start, end, tag, CONFIG, _value):
            obj = sound._object(kind, fields)
            if kind == "SoundParameterized":
                layers = []
                for key, _, children in fields:
                    if key != "Layer" or not children:
                        continue
                    layer = {k: sound._object(k, sub) if sub else values for k, values, sub in children
                             if k != "ParameterGraph"}
                    layer["ParameterGraphs"] = [values for k, values, _ in children if k == "ParameterGraph"]
                    layers.append(layer)
                if layers:
                    obj["Layer.children"] = layers
            objects.append(obj)
    return objects


_SOUND_KEYS = set(sound.CLASS_SOUND_KEYS + sound.CLASS_SOUND_NUMBERS) | set("""
OnSound DeflectSound BoostSound TrickSound FlipSound Cockpit1stPersonSound Cockpit3rdPersonSound
WarmUpSound BarrelSound DamageEffectSound OpenSound CloseSound LockedSound SndHeroSpawned
SndHeroDefeated SndHeroKiller SoldierMusic RepMusic CisMusic VOSound WarmUpTime WarmUpSoundStartPitch
WarmUpSoundFadeInTime WarmUpSoundFadeOutTime BarrelSoundFadeInTime BarrelSoundFadeOutTime VOUnitType
""".split())
_LABELS = {f"{fnv1a(key):08x}": key for key in _SOUND_KEYS}


def class_sounds(_archives, known, sections):
    catalog = class_catalog(sections)
    result = {}
    for tag, classes in catalog.items():
        own = {}
        for ident, node in classes.items():
            groups = {}
            for key, value in node["props"]:
                label = _LABELS.get(key, key)
                if label in _SOUND_KEYS or any(fnv1a(w) in known for w in value.split()[:2]):
                    groups.setdefault(label, []).append(value)
            own[ident] = {key: values[0] if len(values) == 1 else values for key, values in groups.items()}
        for ident, node in classes.items():
            chain = resolve_class(tag, ident, catalog)
            merged = {}
            for parent in chain["parents"]:
                merged.update(own[parent.lower()])
            merged.update(own[ident])
            if merged:
                result.setdefault(node["name"], merged)
    return result
