#!/usr/bin/env python3
"""Sound archives: sample banks, streams, and the sound configs.

    python3 tools/zero_sound.py nab2 [--side rep]

The sound archives in Data/_LVL_PC/Sound are ucfb trees whose chunk tags are
mostly FNV hashes of names (zero_hash.fnv1a), so they don't read as text:

- `StreamList` chunks hold one stream each (music, voice-over, ambience):
  `Stream` → `Info` and `Data`, each padded with `_pad` to 2048 bytes.
- `lvl_` sub-levels (`nab2cw`: Theed in the Clone Wars) hold a
  `SoundBankList` and config chunks: `snd_` (sound properties), `mus_`
  (music), and `ffx_` (foley).
- `common.bnk` is one `SoundBankList` → `SampleBank` → `Info` and `Data` with
  every sample the levels use. A level's bank lists the sample ids it needs
  (and no data of its own); the samples come from common.bnk. core.lvl has a
  small bank with data (interface sounds) and the buses.

An `Info` is a run of (name hash, u32) pairs. `SegmentInfo` (streams) or
`SampleInfo` (banks) opens a list of `Sample` ... `SampleEnd` records, each
with an `Id`, `Frequency`, `Size`, a frame count (1d48feef), `Padding`, and
for some samples an `Alias`: another id whose data it plays. Data is the
samples one after another (aliases take no space), each followed by its
padding.

Formats: 2 is 16-bit PCM, little-endian, mono (banks). 5 is Xbox IMA ADPCM
(streams): per channel, 36 bytes encode 64 frames, a 4-byte header (int16
sample, u8 step index, a pad byte; the header sample is the first frame)
and 32 bytes of nibbles, low nibble first, the last one unused; channels are
interleaved every 4 bytes. A stream with two substreams (Theed's ambience)
interleaves two stereo streams in `SubstreamInterleave`-byte chunks.

Config chunks are the same DATA/SCOP trees as sky_ and fx__ (zero_config),
but strings are stored as FNV hashes in the value slots. The tree is written
out as objects whose hash-valued keys are hex strings ("0x1234abcd").

The extractor writes, all gitignored (retail-derived):

    public/audio/<sublevel>/sound.json   configs, the sample index, the
                                         mission's audio setup, class sound
                                         keys, and the map's emitters/regions
    public/audio/<sublevel>/bank.bin     the sub-level's samples, 16-bit PCM
    public/audio/streams/<name>.json     a stream's segments
    public/audio/streams/<name>.bin      its data, as stored (ADPCM)
"""

import argparse
import array
import json
import os
import struct
import sys
from pathlib import Path

from ucfb import chunks, cstr_at, iter_archives
from zero_addon import data_file
from zero_hash import fnv1a
from zero_lua import data_path

try:
    import numpy as np
except ImportError:  # the plain loops decode the same, slower
    np = None

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import GAME, shared_lock  # noqa: E402
from zero_paths import PUBLIC as _PUBLIC  # noqa: E402
PUBLIC = _PUBLIC / "audio"

PCM16 = 2
XBOX_ADPCM = 5
# Bytes per channel per Xbox ADPCM block, and the frames each one holds.
ADPCM_BLOCK = 36
ADPCM_FRAMES = 64

_NAMES = """
StreamList Stream SoundBankList SampleBank Info Data Name Format NumChannels ChannelInterleave
NumSegments Size NumSubstreams SubstreamInterleave SegmentInfo SampleInfo Sample SampleEnd Id
Frequency Padding NumSamples Alias
""".split()
FIELD = {fnv1a(n): n for n in _NAMES}
# The frame count's name isn't known.
FIELD[0x1D48FEEF] = "Frames"
# A bank whose data lives in common.bnk (level banks) sets this to 1.
FIELD[0x694AAA0B] = "External"
H = {name: fnv1a(name) for name in _NAMES}
LVL = struct.unpack("<I", b"lvl_")[0]

# Config key names, from the original game's own lists of them (its snd.view
# and snd.set debug commands) and the names its data uses.
CONFIG_NAMES = """
SoundProperties SoundLayered SoundParameterized SoundStreamProperties soundstream Bus space Music
FoleyFX FoleyFXSoldier Name Inherit Group Pitch PitchDev Gain GainDev ReverbGain Looping Pan RollIn
RollInDistance MinDistance MuteDistance MaxDistance RollOff Mode3D ModeDoppler Bias Priority
PlayProbability PlayInterval PlayIntervalDev SampleList Sample Layer PlaybackDelay PlaybackDelayDev
Stream SegmentList Segment Parent AttenuateBus AttenuationLeadIn AttenuationLeadOut Attenuation
RoomGain RoomHFGain RoomRollOff DecayTime DecayHFRatio ERGain ERDelay ReverbDelay Diffusion Density
HFReference FadeInTime FadeOutTime Class stand prone jump Land roll RandomPlayPos CyclePlayback
ParameterGraph ControlPoint I3DL2ReverbPreset DirectGain DirectHFGain RoomRolloffFactor ObstructionHFGain
ObstructionLFRatio OcclusionHFGain OcclusionLFRatio Mute Solo SoundStream MinPlaybackTime MinInactiveTime
FoleyFXGroup FootstepLeftWalk FootstepRightWalk FootstepLeftRun FootstepRightRun FootstepLeftStop FootstepRightStop
squat bodyfall
""".split()
CONFIG = {fnv1a(n): n for n in CONFIG_NAMES}
# Another name the library accepts for SoundProperties.
CONFIG[fnv1a("Sound")] = "SoundProperties"
# Keys whose values are names (hashes), not numbers, wherever they appear.
HASH_KEYS = {"Name", "Inherit", "Group", "Bus", "Parent", "AttenuateBus", "Stream", "SoundStream", "Class",
             "FoleyFX", "SoundLayered", "I3DL2ReverbPreset"}
# Keys whose first values are names: Sample(id, ...), Segment(id, ...),
# Layer(name, sound, ...) and a parameter layer's ParameterGraph(input,
# graph, output).
HASH_LEADS = {"Sample": 1, "Segment": 1, "Layer": 2, "ParameterGraph": 3}
# Object kinds whose every value is a name: the foley tables.
ALL_HASHES = {"FoleyFXSoldier", "FoleyFXGroup"}
# Keys that repeat inside an object and are kept as lists of their values.
REPEATED = {"Sample", "Segment", "Layer", "FoleyFX", "ControlPoint", "ParameterGraph"}


def hexname(h):
    return "0x%08x" % h


def raw_chunks(data, start, end):
    """Chunks whose tags may be hashes rather than text."""
    pos = start
    while pos + 8 <= end:
        tag, size = struct.unpack_from("<II", data, pos)
        if pos + 8 + size > end:
            break
        yield tag, pos + 8, pos + 8 + size
        pos = (pos + 8 + size + 3) & ~3


def read_info(data, start, end):
    """An Info chunk: the header fields, and the Sample records listed after
    SegmentInfo or SampleInfo."""
    head, items, current = {}, [], None
    pos = start
    while pos + 4 <= end:
        key = struct.unpack_from("<I", data, pos)[0]
        name = FIELD.get(key, "%08x" % key)
        if key == 0:
            break
        if name in ("SegmentInfo", "SampleInfo"):
            pos += 4
        elif name == "Sample":
            current = {}
            pos += 4
        elif name == "SampleEnd":
            items.append(current)
            current = None
            pos += 4
        else:
            (head if current is None else current)[name] = struct.unpack_from("<I", data, pos + 4)[0]
            pos += 8
    return head, items


def _container(data, start, end):
    """The Info and Data ranges of a Stream or SampleBank."""
    found = {}
    for tag, b, e in raw_chunks(data, start, end):
        if tag in (H["Info"], H["Data"]):
            found[FIELD[tag]] = (b, e)
    return found


def _layout(head, items, data_range):
    """Each sample's or segment's byte range in the data."""
    offset = 0
    out = []
    for item in items:
        record = {
            "id": item["Id"],
            "rate": item.get("Frequency", 22050),
            "size": item.get("Size", 0),
            "frames": item.get("Frames", 0),
        }
        if "Alias" in item:
            record["alias"] = item["Alias"]
        else:
            record["offset"] = offset
            offset += record["size"] + item.get("Padding", 0)
        out.append(record)
    return out


def archive_contents(data):
    """A sound archive's streams, banks, and sub-levels: {"streams": [...],
    "banks": [...], "configs": [...], "sublevels": {hash: (start, end)}}."""
    out = {"streams": [], "banks": [], "configs": [], "sublevels": {}}
    root_size = struct.unpack_from("<I", data, 4)[0]

    def scan(start, end, into):
        for tag, b, e in raw_chunks(data, start, end):
            if tag == H["StreamList"]:
                for _, sb, se in raw_chunks(data, b, e):
                    parts = _container(data, sb, se)
                    if "Info" not in parts:
                        continue
                    head, items = read_info(data, *parts["Info"])
                    into["streams"].append({"head": head, "items": _layout(head, items, parts.get("Data")), "data": parts.get("Data")})
            elif tag == H["SoundBankList"]:
                for _, bb, be in raw_chunks(data, b, e):
                    parts = _container(data, bb, be)
                    if "Info" not in parts:
                        continue
                    head, items = read_info(data, *parts["Info"])
                    into["banks"].append({"head": head, "items": _layout(head, items, parts.get("Data")), "data": parts.get("Data")})
            elif tag in (struct.unpack("<I", t)[0] for t in (b"snd_", b"mus_", b"ffx_")):
                into["configs"].append((struct.pack("<I", tag).decode(), b, e))
            elif tag == LVL:
                name, inner = struct.unpack_from("<II", data, b)
                into["sublevels"][name] = (b + 8, b + 8 + inner)

    scan(8, 8 + root_size, out)
    return out


def sublevel_contents(data, span):
    """A sub-level's banks and configs."""
    out = {"streams": [], "banks": [], "configs": [], "sublevels": {}}
    start, end = span
    for tag, b, e in raw_chunks(data, start, end):
        if tag == H["SoundBankList"]:
            for _, bb, be in raw_chunks(data, b, e):
                parts = _container(data, bb, be)
                if "Info" in parts:
                    head, items = read_info(data, *parts["Info"])
                    out["banks"].append({"head": head, "items": _layout(head, items, parts.get("Data")), "data": parts.get("Data")})
        elif struct.pack("<I", tag) in (b"snd_", b"mus_", b"ffx_"):
            out["configs"].append((struct.pack("<I", tag).decode(), b, e))
    return out


# --- configs ---------------------------------------------------------------

def _config_value(kind, key, index, raw):
    """A config value: a hex name for name-valued slots, else a float."""
    if kind in ALL_HASHES or key in HASH_KEYS or index < HASH_LEADS.get(key, 0):
        return hexname(struct.unpack("<I", raw)[0])
    value = struct.unpack("<f", raw)[0]
    return round(value, 6)


def _entries(data, start, end, kind):
    """A config body as (key, values, children) entries."""
    entries = []
    for tag, b, e in chunks(data, start, end):
        if tag == b"DATA":
            key = struct.unpack_from("<I", data, b)[0]
            name = CONFIG.get(key, "%08x" % key)
            count = data[b + 4]
            values = [_config_value(kind, name, i, data[b + 5 + 4 * i:b + 9 + 4 * i]) for i in range(count)]
            entries.append([name, values, []])
        elif tag == b"SCOP" and entries:
            # A second scope after the same entry adds to the first: 46
            # objects are followed by an empty one (the AT-ST's and AT-AT's
            # head cannon FireSound among them), which used to wipe their
            # Name and SampleList.
            parent = entries[-1]
            parent[2] = parent[2] + _entries(data, b, e, parent[0] if kind in ("snd_", "mus_", "ffx_") else kind)
    return entries


def _object(kind, entries):
    """One config object as a dict. Single values become scalars; lists
    (SampleList, SegmentList) and repeated keys become lists of values."""
    obj = {"type": kind}
    for key, values, kids in entries:
        if kids:
            sub = [_object(k, c) if c else v for k, v, c in kids]
            if all(isinstance(s, list) for s in sub) and len({k for k, _, _ in kids}) <= 1:
                obj[key] = sub
            else:
                obj.setdefault(key + ".children", []).append({k: (_object(k, c) if c else v) for k, v, c in kids})
            continue
        if key in REPEATED:
            obj.setdefault(key, []).append(values)
        elif len(values) == 1:
            obj[key] = values[0]
        else:
            obj[key] = values
    return obj


def config_objects(data, configs):
    """Every object in a list of config chunks, as dicts with a "type"."""
    out = []
    for tag, b, e in configs:
        for kind, _, kids in _entries(data, b, e, tag):
            out.append(_object(kind, kids))
    return out


# --- decoding --------------------------------------------------------------

STEPS = (
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107,
    118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963,
    1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894,
    6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794,
    32767,
)
INDEX_STEP = (-1, -1, -1, -1, 2, 4, 6, 8)


def decode_xbox_adpcm(raw, channels):
    """Xbox IMA ADPCM to one array('h') per channel. Each 36-byte block
    starts from its own header, so with numpy every block decodes at once
    and only the 63 steps within a block run in turn."""
    if np is not None:
        return _decode_xbox_adpcm_np(raw, channels)
    out = [array.array("h") for _ in range(channels)]
    block = ADPCM_BLOCK * channels
    for b in range(0, len(raw) - block + 1, block):
        for c in range(channels):
            sample, index = struct.unpack_from("<hB", raw, b + 4 * c)
            index = min(index, 88)
            dest = out[c]
            dest.append(sample)
            count = 1
            for group in range(8):
                base = b + 4 * channels + (group * channels + c) * 4
                for k in range(4):
                    byte = raw[base + k]
                    for code in (byte & 15, byte >> 4):
                        if count == ADPCM_FRAMES:
                            break
                        step = STEPS[index]
                        diff = step >> 3
                        if code & 1:
                            diff += step >> 2
                        if code & 2:
                            diff += step >> 1
                        if code & 4:
                            diff += step
                        sample = sample - diff if code & 8 else sample + diff
                        sample = -32768 if sample < -32768 else 32767 if sample > 32767 else sample
                        index = min(max(index + INDEX_STEP[code & 7], 0), 88)
                        dest.append(sample)
                        count += 1
    return out


def _decode_xbox_adpcm_np(raw, channels):
    block = ADPCM_BLOCK * channels
    count = len(raw) // block
    out = []
    if not count:
        return [array.array("h") for _ in range(channels)]
    data = np.frombuffer(raw, dtype=np.uint8, count=count * block).reshape(count, block)
    steps = np.array(STEPS, dtype=np.int32)
    index_step = np.array(INDEX_STEP, dtype=np.int32)
    for c in range(channels):
        head = data[:, 4 * c:4 * c + 4]
        sample = (head[:, 0].astype(np.uint16) | (head[:, 1].astype(np.uint16) << 8)).astype(np.int16).astype(np.int32)
        index = np.minimum(head[:, 2].astype(np.int32), 88)
        at = [4 * channels + (group * channels + c) * 4 + k for group in range(8) for k in range(4)]
        packed = data[:, at]
        codes = np.empty((count, 64), dtype=np.int32)
        codes[:, 0::2] = packed & 15
        codes[:, 1::2] = packed >> 4
        frames = np.empty((count, ADPCM_FRAMES), dtype=np.int16)
        frames[:, 0] = sample
        for i in range(ADPCM_FRAMES - 1):
            code = codes[:, i]
            step = steps[index]
            diff = (step >> 3) + np.where(code & 1, step >> 2, 0) + np.where(code & 2, step >> 1, 0) + np.where(code & 4, step, 0)
            sample = np.clip(np.where(code & 8, sample - diff, sample + diff), -32768, 32767)
            index = np.clip(index + index_step[code & 7], 0, 88)
            frames[:, i + 1] = sample
        channel = array.array("h")
        channel.frombytes(frames.astype("<i2").tobytes())
        out.append(channel)
    return out


def stream_substreams(stream, data):
    """A stream's data split into its substreams (each a bytes object)."""
    head = stream["head"]
    start, end = stream["data"]
    raw = data[start:end]
    count = head.get("NumSubstreams", 1)
    if count <= 1:
        return [raw]
    step = head.get("SubstreamInterleave", len(raw))
    parts = [bytearray() for _ in range(count)]
    for k, pos in enumerate(range(0, len(raw), step)):
        parts[k % count] += raw[pos:pos + step]
    return [bytes(p) for p in parts]


def decode_segment(stream, segment, substreams):
    """One stream segment decoded: a list of channels, each an array('h'),
    for every substream in turn (front pair first)."""
    head = stream["head"]
    channels = head.get("NumChannels", 1)
    count = head.get("NumSubstreams", 1)
    out = []
    for part in substreams:
        size = segment["size"] // count if count > 1 else segment["size"]
        offset = segment["offset"] // count if count > 1 else segment["offset"]
        out.extend(decode_xbox_adpcm(part[offset:offset + size], channels))
    return out


# --- extraction ------------------------------------------------------------

def sound_archive(name):
    return GAME / "Sound" / name


def sound_file(path, root=None):
    """The sound archive a mission names: "sound\\tat.lvl;tat2cw" is the
    game's Sound/tat.lvl, "dc:sound\\tat.lvl;tat3cw" its add-on's (root,
    the add-on's Data/_LVL_PC; tools/zero_addon.py)."""
    return data_file(path, root, GAME) or sound_archive(path.split(";", 1)[0].split("\\")[-1])


def read_mission_audio(calls):
    """The audio calls a mission script makes, as the runtime needs them."""
    mission = {"banks": [], "streams": [], "music": {}, "victory": {}, "defeat": {}, "bleeding": [], "outOfBounds": {},
               "bonus": [], "effects": {}}
    for name, args in calls:
        if name == "ReadDataFile" and args and isinstance(args[0], str) and data_path(args[0]).lower().startswith("sound\\"):
            mission["banks"].append(args[0])
        elif name == "OpenAudioStream" and len(args) >= 2:
            mission["streams"].append([args[0], args[1]])
        elif name == "SetAmbientMusic" and len(args) >= 3 and args[0] is not None:
            mission["music"].setdefault(str(int(args[0])), []).append(list(args[1:]))
        elif name == "SetVictoryMusic" and len(args) >= 2 and args[0] is not None:
            mission["victory"][str(int(args[0]))] = args[1]
        elif name == "SetDefeatMusic" and len(args) >= 2 and args[0] is not None:
            mission["defeat"][str(int(args[0]))] = args[1]
        elif name == "SetBleedingVoiceOver" and len(args) >= 3:
            mission["bleeding"].append(list(args))
        elif name == "SetOutOfBoundsVoiceOver" and len(args) >= 2 and args[0] is not None:
            mission["outOfBounds"][str(int(args[0]))] = args[1]
        elif name == "SetPlanetaryBonusVoiceOver" and len(args) >= 4:
            mission["bonus"].append(list(args))
        elif name == "SetSoundEffect" and len(args) >= 2:
            mission["effects"][args[0]] = args[1]
    return mission


def class_sounds(archives, known):
    """Every class property (weapons, ordnance, explosions, entities) whose
    value names a sound object, or that tunes one: class -> {key: value}.
    Keys whose names are known come out by name, the rest as hex. A key set
    more than once (a command post's AmbientSound per side, "rep
    com_blg_commandpost_goodie defer") keeps every value, as a list."""
    from zero_odf import property_name

    extra = {fnv1a(n): n for n in CLASS_SOUND_KEYS + CLASS_SOUND_NUMBERS}
    out = {}
    for data in archives:
        for start, end in iter_archives(data):
            for tag, b, e in chunks(data, start, end):
                if tag not in (b"entc", b"wpnc", b"ordc", b"expc"):
                    continue
                name = None
                found = {}
                repeated = set()
                for t2, b2, e2 in chunks(data, b, e):
                    if t2 == b"TYPE":
                        name = cstr_at(data, b2, e2)
                    elif t2 == b"PROP" and e2 - b2 > 4:
                        key = struct.unpack_from("<I", data, b2)[0]
                        value = cstr_at(data, b2 + 4, e2)
                        words = value.split()
                        label = extra.get(key) or property_name(key)
                        names_sound = any(fnv1a(w) in known for w in words[:2])
                        if not names_sound and label not in CLASS_SOUND_NUMBERS:
                            continue
                        if label in found:
                            if label not in repeated:
                                found[label] = [found[label]]
                                repeated.add(label)
                            found[label].append(value)
                        else:
                            found[label] = value
                if name and found and name not in out:
                    out[name] = found
    return out


# Class property names that hold sounds, as far as known.
CLASS_SOUND_KEYS = (
    "FireSound", "ReloadSound", "FireEmptySound", "WeaponChangeSound", "ChangeModeSound", "OverheatSound",
    "OverheatStopSound", "ChargeSound", "FireLoopSound", "JumpSound", "LandSound", "RollSound",
    "SquatSound", "ProneSound", "StandSound", "OrdnanceSound", "CollisionSound", "SoundProperty", "DeathSound",
    "HurtSound", "FleeSound", "AmbientSound", "Ambient2Sound", "EngineSound", "TurnOnSound", "TurnOffSound",
    "Footstep0Sound", "Footstep1Sound", "Footstep2Sound", "PowerupSound", "DischargeSound", "LostSound",
    "DisputeSound", "BuildingSound", "BuiltSound", "Sound", "HitSound", "TakeoffSound", "VehicleCollisionSound",
    "CapturedSound", "CollisionOtherSound", "CollisionWaterSound", "CollisionFoliageSound", "LowHealthSound",
    "TickSound", "TurretYawSound", "TurretPitchSound", "TurretActivateSound", "TurretDeactivateSound",
    "TurretAmbientSound", "ShieldSound", "ShieldOffSound", "ClothingRustleSound", "AcquiredTargetSound",
    "HidingSound", "ApproachingTargetSound", "PreparingForDamageSound", "HeardEnemySound", "LockedOnSound",
    # A vehicle's: the turn-off's start (R2's), the takeoff's and landing's
    # other names, a walker's legs (docs/vehicles.md, "Effects and sounds").
    "TurningOffSound", "WindUpSound", "WindDownSound", "HydraulicSound", "FootstepSound0", "FootstepSound1",
    "FootstepSound2", "FootstepSound3", "FootstepSound4", "FootstepSound5",
)
# Keys that tune sounds or pick them (numbers, foley classes), kept even
# though they name no sound.
CLASS_SOUND_NUMBERS = (
    "ChargeSoundPitch", "OverheatSoundPitch", "FireSoundStop", "FoleyFXClass", "FoleyFXGroup", "RunSpeed",
    "FootstepStride", "SoundPitchDev", "TurnOffTime", "TurnOnTime", "TurretYawSoundPitch", "TurretPitchSoundPitch",
    "LowHealthThreshold", "TickSoundPitch",
)


def instance_box(record, material):
    """An object's world box (axis-aligned) from its model's local box."""
    group, lo, hi = material
    x = record["xfrm"]
    # XFRM: three rows of the rotation, then the position.
    rot = [x[0:3], x[3:6], x[6:9]]
    pos = x[9:12]
    corners = [(a, b, c) for a in (lo[0], hi[0]) for b in (lo[1], hi[1]) for c in (lo[2], hi[2])]
    world = [[pos[i] + sum(p[j] * rot[j][i] for j in range(3)) for i in range(3)] for p in corners]
    return {
        "group": group,
        "min": [round(min(w[i] for w in world), 3) for i in range(3)],
        "max": [round(max(w[i] for w in world), 3) for i in range(3)],
    }


def material_classes(archives, classes_by_name):
    """Classes with a FoleyFXGroup, with their model's collision box:
    class -> (group, lo, hi)."""
    from zero_world import coll_meshes

    boxes = {}
    for data in archives:
        for model, found in coll_meshes(data).items():
            pts = [p for positions, _ in found for p in positions]
            if pts and model.lower() not in boxes:
                boxes[model.lower()] = ([min(p[i] for p in pts) for i in range(3)], [max(p[i] for p in pts) for i in range(3)])
    out = {}
    for name, props in classes_by_name.items():
        group = props.get("FoleyFXGroup")
        if not group:
            continue
        model = (props.get("GeometryName") or name).lower()
        box = boxes.get(model) or boxes.get(name.lower())
        if box:
            out[name] = (group, box[0], box[1])
    return out


# A command post's voice-over keys: VO_<listener side>_<actor side><event>.
SIDES = ("All", "Imp", "Rep", "Cis")
CP_EVENTS = ("Capture", "Lost", "InDispute", "Saved", "Info")
VO_KEYS = {fnv1a(f"VO_{l}_{a}{e}"): f"VO_{l}_{a}{e}" for l in SIDES for a in SIDES for e in CP_EVENTS}


def class_geometry(archives):
    """Each entity class's GeometryName."""
    from zero_odf import entity_classes

    out = {}
    for data in archives:
        for name, cls in entity_classes(data).items():
            for key, value in cls["props"]:
                if key == "GeometryName" and name not in out:
                    out[name] = value
    return out


def map_sound_world(map_data, known, materials=None):
    """The map's sound regions (soundstream, soundspace, soundstatic, ...),
    every placed object with a Sound property (the static emitters), each
    command post's voice-over keys, and the world boxes of the objects whose
    class names a foley material (FoleyFXGroup), for footsteps.
    `materials` is class -> (group, local box)."""
    from ucfb import nodes
    from zero_odf import property_name
    from zero_world import instance_records, regions

    materials = materials or {}

    names = {fnv1a("Sound"): "Sound", fnv1a("SoundStream"): "SoundStream", fnv1a("MinDistance"): "MinDistance", **VO_KEYS}
    out = {"regions": [], "emitters": [], "posts": {}, "materials": []}
    for body, end in nodes(map_data, b"wrld"):
        for region in regions(map_data, body, end):
            word = region["name"].split()[0].lower() if region["name"].split() else ""
            if word.startswith("sound") or word.startswith("foley"):
                out["regions"].append(region)
        for record in instance_records(map_data, body, end, lambda key: names.get(key) or property_name(key)):
            props = dict(record["props"])
            x = record["xfrm"]
            if record["class"] in materials:
                box = instance_box(record, materials[record["class"]])
                if box:
                    out["materials"].append(box)
            vo = {k: v for k, v in props.items() if k.startswith("VO_")}
            if vo:
                out["posts"][record["name"]] = {"class": record["class"], "position": [x[9], x[10], x[11]], "vo": vo}
            if "Sound" not in props and "SoundStream" not in props:
                continue
            out["emitters"].append({
                "class": record["class"],
                "name": record["name"],
                "position": [x[9], x[10], x[11]],
                "props": props,
            })
    return out


def write_streams(stream_calls, out_dir, root=None):
    """Each opened stream's data as stored, and its segment index. root is
    the add-on folder a mission's "dc:" streams are in."""
    out_dir.mkdir(parents=True, exist_ok=True)
    written = {}
    cache = {}
    for file, name in stream_calls:
        path = sound_file(file, root)
        if name in written or not path.exists():
            continue
        # Written by another map's extraction already (the era's voice-over
        # is every battle's): the same bytes, so keep them. Maps extract side
        # by side (tools/prepare.py), so two may write one stream at once:
        # each through files of its own, renamed into place.
        existing = out_dir / f"{name}.json"
        if existing.exists() and (out_dir / f"{name}.bin").exists():
            try:
                written[name] = len(json.loads(existing.read_text())["segments"])
                continue
            except (OSError, ValueError, KeyError):
                pass
        if path not in cache:
            cache[path] = (path.read_bytes(),)
        (data,) = cache[path]
        want = fnv1a(name)
        for stream in archive_contents(data)["streams"]:
            head = stream["head"]
            if head.get("Name") != want or not stream["data"]:
                continue
            start, end = stream["data"]
            temporary = out_dir / f"{name}.bin.{os.getpid()}.part"
            temporary.write_bytes(data[start:end])
            temporary.replace(out_dir / f"{name}.bin")
            index = {
                "name": name,
                "format": head.get("Format"),
                "channels": head.get("NumChannels", 1),
                "substreams": head.get("NumSubstreams", 1),
                "substreamInterleave": head.get("SubstreamInterleave", 0),
                "segments": {hexname(s["id"]): [s["offset"], s["size"], s["frames"], s["rate"]] for s in stream["items"] if "offset" in s},
            }
            temporary = out_dir / f"{name}.json.{os.getpid()}.part"
            temporary.write_text(json.dumps(index))
            temporary.replace(out_dir / f"{name}.json")
            written[name] = len(index["segments"])
    return written


def write_audio(map_name, mission_name, mission_calls, sides, map_data, side_archives, common_data, out_root=PUBLIC, root=None):
    """The audio a battle loads: its sound sub-level's configs and samples,
    the streams it opens, its class sound keys, and the map's emitters.
    public/audio/index.json maps each mission (and map) to its sub-level.
    root is the add-on folder the mission's "dc:" files are in, if any."""
    mission = read_mission_audio(mission_calls)
    if not mission["banks"]:
        print("  audio: the mission reads no sound archive", flush=True)
        return None
    file, _, sub = mission["banks"][0].split("\\")[-1].partition(";")
    data = sound_file(mission["banks"][0], root).read_bytes()
    contents = archive_contents(data)
    span = contents["sublevels"].get(fnv1a(sub))
    if span is None:
        print(f"  audio: no sub-level {sub} in {file}", flush=True)
        return None
    level = sublevel_contents(data, span)

    # core.lvl's buses, interface sounds, and reverb presets come first; the
    # sub-level's objects follow.
    core_data = (GAME / "core.lvl").read_bytes()
    core_configs = []
    for start, end in iter_archives(core_data):
        for tag, b, e in chunks(core_data, start, end):
            if tag in (b"snd_", b"mus_", b"ffx_"):
                core_configs.append((tag.decode(), b, e))
    core_banks = archive_contents_flat(core_data)
    objects = config_objects(core_data, core_configs) + config_objects(data, level["configs"])

    # Samples: the sub-level bank's ids, read from common.bnk; then core.lvl's.
    common_bank = archive_contents(sound_archive("common.bnk").read_bytes())
    bnk_data = sound_archive("common.bnk").read_bytes()
    by_id = {}
    for bank in common_bank["banks"]:
        for item in bank["items"]:
            by_id[item["id"]] = (bank, item, bnk_data)
    # A sub-level bank with data of its own: an add-on's sounds, which
    # common.bnk can't hold (Jabba's Palace's 18 in each era).
    for bank in level["banks"]:
        for item in bank["items"] if bank["data"] else []:
            by_id[item["id"]] = (bank, item, data)
    for bank in core_banks:
        for item in bank["items"]:
            by_id.setdefault(item["id"], (bank, item, core_data))
    wanted = [item["id"] for bank in level["banks"] for item in bank["items"]]
    wanted += [item["id"] for bank in core_banks for item in bank["items"]]
    pcm = bytearray()
    samples = {}
    missing = 0
    for sid in wanted:
        if hexname(sid) in samples:
            continue
        found = by_id.get(sid)
        if not found:
            missing += 1
            continue
        bank, item, source = found
        if "alias" in item:
            target = by_id.get(item["alias"])
            if not target:
                missing += 1
                continue
            bank, item_data, source = target
        else:
            item_data = item
        start = bank["data"][0] + item_data["offset"]
        raw = source[start:start + item_data["size"]]
        samples[hexname(sid)] = [len(pcm), len(raw) // 2, item["rate"]]
        pcm += raw
        if len(pcm) % 4:
            pcm += b"\x00" * (4 - len(pcm) % 4)

    # Names for the hashes, where any text the battle holds spells them.
    known = {int(o["Name"], 16) for o in objects if isinstance(o.get("Name"), str)}
    sounds_of_classes = class_sounds([map_data, *side_archives, common_data], known)
    geometry = class_geometry([map_data, common_data])
    with_geometry = {c: {**props, "GeometryName": geometry.get(c)} for c, props in sounds_of_classes.items()}
    world = map_sound_world(map_data, known, material_classes([map_data, common_data], with_geometry))
    words = set()
    for keys in sounds_of_classes.values():
        for v in keys.values():
            for value in v if isinstance(v, list) else [v]:
                words.update(value.split())
    for e in world["emitters"]:
        words.add(e["props"].get("Sound", ""))
    for post in world["posts"].values():
        words.update(post["vo"].values())
    for r in world["regions"]:
        words.update(r["name"].split())
    for name, args in mission_calls:
        words.update(a for a in args if isinstance(a, str))
    # Sorted: set order varies per process, and some hashes have two
    # spellings ("CIS", "cis"); the output must not.
    names = {hexname(fnv1a(w)): w for w in sorted(words) if w}

    out_dir = out_root / sub
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "bank.bin").write_bytes(bytes(pcm))
    streams = write_streams(mission["streams"], out_root / "streams", root)
    manifest = {
        "sublevel": sub,
        "map": map_name,
        "sides": sides,
        "objects": objects,
        "samples": samples,
        "streams": sorted(streams),
        "mission": mission,
        "classes": sounds_of_classes,
        "world": world,
        "names": names,
    }
    (out_dir / "sound.json").write_text(json.dumps(manifest))
    # The index is every map's: a map's one shared read-modify-write, under
    # a lock held only for it (tools/prepare.py runs maps side by side).
    index_path = out_root / "index.json"
    with shared_lock("audio-index"):
        index = json.loads(index_path.read_text()) if index_path.exists() else {}
        index.setdefault("missions", {})[mission_name] = sub
        index.setdefault("maps", {})[map_name] = sub
        temporary = index_path.with_suffix(".json.part")
        temporary.write_text(json.dumps(index, indent=1, sort_keys=True))
        temporary.replace(index_path)
    kinds = {}
    for o in objects:
        kinds[o["type"]] = kinds.get(o["type"], 0) + 1
    print(f"  audio {sub}: {kinds}", flush=True)
    print(f"  samples {len(samples)} ({len(pcm) / 1e6:.1f} MB, {missing} missing) streams {streams}", flush=True)
    print(f"  class sounds {len(sounds_of_classes)} emitters {len(world['emitters'])} regions {len(world['regions'])} posts with voice-over {len(world['posts'])}", flush=True)
    return manifest


def archive_contents_flat(data):
    """The SoundBankLists anywhere in a regular (text-tagged) archive."""
    banks = []
    for start, end in iter_archives(data):
        pos = start
        while pos + 8 <= end:
            tag, size = struct.unpack_from("<II", data, pos)
            if pos + 8 + size > end:
                break
            if tag == H["SoundBankList"]:
                for _, bb, be in raw_chunks(data, pos + 8, pos + 8 + size):
                    parts = _container(data, bb, be)
                    if "Info" in parts:
                        head, items = read_info(data, *parts["Info"])
                        banks.append({"head": head, "items": _layout(head, items, parts.get("Data")), "data": parts.get("Data")})
            pos = (pos + 8 + size + 3) & ~3
    return banks


def extract_audio(map_name, side="rep", mission=None):
    """Write the sound of one of a map's battles: the mission named, else
    the one tools/extract.py picks for the side (its mission's sound
    sub-level, streams, and class keys)."""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from extract import GAME as GAME_DIR, common_archive, map_archive, pick_mission, side_archive
    from zero_addon import mission_root, missions
    from zero_lua import calls, load, mission_setup

    if mission:
        setup = mission_setup(missions(GAME_DIR)[mission]) if mission in missions(GAME_DIR) else None
    else:
        mission, setup = pick_mission(map_name, side)
    if not setup:
        print(f"  audio: no mission for {map_name} with {mission or side}", flush=True)
        return None
    mission_calls = calls(load(missions(GAME_DIR)[mission]))
    sides = setup["sides"]
    return write_audio(map_name, mission, mission_calls, sides, map_archive(map_name).read_bytes(),
                       [side_archive(s).read_bytes() for s in sides if side_archive(s).exists()],
                       common_archive().read_bytes(), root=mission_root(mission, GAME_DIR))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("map", nargs="?", default="nab2")
    parser.add_argument("--side", default="rep", help="a side the battle has (picks the era's mission)")
    parser.add_argument("--mission", help="the battle by its mission's name (nab2a), in place of --side")
    args = parser.parse_args()
    return 0 if extract_audio(args.map, args.side, args.mission) is not None else 1


if __name__ == "__main__":
    sys.exit(main())
