"""Soldier animation banks (zaa_) from common.lvl.

BIN_ holds chunks with reversed tags. SMNA is a 24-byte header whose last
word is the clip count. MINA has one 16-byte record per clip (flags, unused,
name CRC, frame count, bone count). TNJA has 48 bytes per bone per clip
(bone CRC, four rotation curve offsets, three translation curve offsets,
translation bias xyz, translation scale). TADA holds the curves.

A curve is an int16 first sample, then signed bytes: a delta from the last
sample, or -128 (hold for the next unsigned byte's count of frames), or -127
(an absolute int16 follows). Rotation curves are quaternion xyzw over 2047.
Translation is bias + sample * scale, or just the bias when the scale is 0.

Clips are authored at 30 fps and loop on their last frame. Locomotion clips
(flag bit 0) carry their ground travel on DummyRoot, which is how the game
knows each cycle's natural speed.
"""

import array
import json
import math
import struct

from ucfb import child, nodes, node_name
from zero_hash import crc

FPS = 30
ROOT_MOTION = 1

BANKS = (
    "human_animation_0",
    "human_animation_1",
    "human_animation_2",
    "human_animation_3",
    "human_animation_4",
    "human_animation_alert",
)

WEAPONS = ("rifle", "pistol", "bazooka", "tool", "grenade", "sabre", "sbdroid", "bdroid", "lascannon", "gam")
POSTURES = ("stand", "crouch", "prone")
ACTIONS = (
    "idle_emote", "idle_checkweapon", "idle_lookaround",
    "walkforward", "walkbackward", "walkleft", "walkright",
    "runforward", "runbackward", "runleft", "runright", "turnleft", "turnright",
    "shoot", "shoot2", "shoot3", "shoot_secondary", "charge", "reload",
    "hitfront", "hitback", "hitleft", "hitright",
    "death_forward", "death_backward", "death_left", "death_right",
    "getupfront", "getupback",
)
# The engine's action names that take no posture (its own table, in order).
POSTURELESS = (
    "jump", "jump_up", "landsoft", "landhard", "fall", "die",
    "thrown_flail", "thrown_flyingfront", "thrown_flyingback",
    "thrown_flyingleft", "thrown_flyingright", "thrown_tumblefront",
    "thrown_tumbleback", "thrown_bouncefrontsoft", "thrown_bouncebacksoft",
    "thrown_landfrontsoft", "thrown_landbacksoft", "stand_getupfront",
    "stand_getupback", "thrown_restfrontsoft", "thrown_restbacksoft",
    "diveforward", "jetpack_hover", "stand_getdown_prone", "prone_getup_stand",
    "crouch_getdown_prone", "prone_getup_crouch", "stand_death_forward",
    "stand_death_backward", "stand_death_left", "stand_death_right",
)


# The first-person bank (human_animation_fp) drives the arms rig; its clips
# are named in the original game's own table.
FIRST_PERSON_BANK = "human_animation_fp"
FIRST_PERSON = tuple(
    f"_1st_{weapon}_{action}"
    for weapon, actions in (
        ("rifle", ("idle", "run", "jump", "jump_land", "flail", "shoot", "reload")),
        ("bazooka", ("idle", "run", "jump", "jump_land", "flail", "shoot", "reload")),
        ("tool", ("idle", "run", "jump", "jump_land", "flail", "shoot", "reload", "repair")),
        ("lascannon", ("idle", "run", "jump", "flail", "shoot")),
        ("droideka", ("idle", "walk", "shoot", "reload")),
        ("grenade", ("charge", "shoot", "shoot2")),
    )
    for action in actions
) + ("_1st_handsdown",)


# The animator's weapon banks, in the original game's order, with the clip
# prefix each uses and the bank it borrows missing clips from. A weapon's
# AnimationBank names one; an unknown name is the rifle.
CLIP_BANKS = (
    ("rifle", "rifle", 0), ("bazooka", "bazooka", 0), ("pistol", "pistol", 4), ("grenade", "grenade", 4),
    ("tool", "tool", 2), ("lightsabre", "sabre", 4), ("sbdroid", "sbdroid", 4), ("bdroid", "bdroid", 0),
    ("lascannon", "lascannon", 0), ("gam", "gam", 5),
)
LOCO = (
    "idle_emote", "idle_checkweapon", "idle_lookaround", "turnleft", "turnright", "walkforward", "walkbackward",
    "runforward", "runbackward", "walkright", "walkleft", "runright", "runleft",
)
WEAPON_ACTIONS = ("idle", "shoot", "shoot2", "shoot3", "charge", "reload")
SET_POSTURELESS = (
    "jump", "jump_up", "landsoft", "landhard", "fall", "thrown_flail", "thrown_flyingfront", "thrown_flyingback",
    "thrown_flyingleft", "thrown_flyingright", "thrown_tumblefront", "thrown_tumbleback", "thrown_bouncefrontsoft",
    "thrown_bouncebacksoft", "thrown_landfrontsoft", "thrown_landbacksoft", "stand_getupfront", "stand_getupback",
    "thrown_restfrontsoft", "thrown_restbacksoft", "diveforward", "jetpack_hover", "stand_getdown_prone",
    "prone_getup_stand", "crouch_getdown_prone", "prone_getup_crouch", "stand_death_forward",
    "stand_death_backward", "stand_death_left", "stand_death_right",
)


def clip_sets(present):
    """Which clip plays for each bank, posture, and action, as the animator
    builds its tables once for every body (`present`: the clip names there
    are). The rifle's sets are the base the legs always play; every bank
    fills its gaps from the bank it borrows from, in bank order, then grenade
    crouch throws take the standing ones, then shoot, shoot2, and shoot3 stand
    in for each other, and whatever is still missing takes the rifle's (a
    weapon action with no rifle clip either takes the action before it).
    Alert clips exist only standing. Returns {base, banks}."""
    def get(name):
        return name if name in present else None

    postures = POSTURES
    base_loco = {p: [get(f"human_rifle_{p}_{a}") for a in LOCO] for p in postures}
    for p in postures:
        row = base_loco[p]
        if row[0] is None:
            row[0] = base_loco["stand"][0]
        for i in (1, 2, 3, 4):
            if row[i] is None:
                row[i] = row[0]
    base_postureless = [get(f"human_rifle_{a}") for a in SET_POSTURELESS]

    loco, weapon, alert, postureless = {}, {}, {}, {}
    for b, (_, prefix, _) in enumerate(CLIP_BANKS):
        for p in postures:
            loco[b, p] = [get(f"human_{prefix}_{p}_{a}") for a in LOCO]
            weapon[b, p] = [get(f"human_{prefix}_{p}_{a}") for a in WEAPON_ACTIONS]
        # The bazooka's alert set is its standing set.
        alert[b] = list(loco[b, "stand"]) if prefix == "bazooka" else [get(f"human_{prefix}_stand_{a}_alert") for a in LOCO]
        postureless[b] = [get(f"human_{prefix}_{a}") for a in SET_POSTURELESS]
    fill = lambda row, other: [x or y for x, y in zip(row, other)]
    for b, (_, _, parent) in enumerate(CLIP_BANKS):
        if parent == b:
            continue
        for p in postures:
            loco[b, p] = fill(loco[b, p], loco[parent, p])
            weapon[b, p] = fill(weapon[b, p], weapon[parent, p])
        alert[b] = fill(alert[b], alert[parent])
        postureless[b] = fill(postureless[b], postureless[parent])
    grenade = 3
    for k in (1, 2, 3):
        if weapon[grenade, "crouch"][k] is None:
            weapon[grenade, "crouch"][k] = weapon[grenade, "stand"][k]
    for b in range(len(CLIP_BANKS)):
        for p in postures:
            row = weapon[b, p]
            shoot = row[1] or row[2] or row[3]
            for k in (1, 2, 3):
                if row[k] is None:
                    row[k] = shoot
            loco[b, p] = fill(loco[b, p], loco[0, p])
            rifle = weapon[0, p]
            if row[0] is None:
                row[0] = rifle[0]
            for k in range(1, len(WEAPON_ACTIONS)):
                if row[k] is None:
                    row[k] = rifle[k] or row[k - 1]
        alert[b] = fill(alert[b], alert[0])
        postureless[b] = fill(postureless[b], base_postureless)
    banks = {}
    for b, (name, _, _) in enumerate(CLIP_BANKS):
        banks[name] = {
            "loco": {p: dict(zip(LOCO, loco[b, p])) for p in postures},
            "weapon": {p: dict(zip(WEAPON_ACTIONS, weapon[b, p])) for p in postures},
            "alert": dict(zip(LOCO, alert[b])),
            "postureless": dict(zip(SET_POSTURELESS, postureless[b])),
        }
    base = {"loco": {p: dict(zip(LOCO, base_loco[p])) for p in postures}, "postureless": dict(zip(SET_POSTURELESS, base_postureless))}
    return {"base": base, "banks": banks}


def clip_names():
    """Clip names are only stored as CRCs, so match them against the naming scheme."""
    names = {"human_choking", "human_ride_shoot", "human_minigun_9pose", "human_dishcannon_9pose", "human_speederbike_9pose", "human_stap_ride", "human_drive", "human_ride"}
    for weapon in WEAPONS:
        for posture in POSTURES:
            for action in ACTIONS:
                names.add(f"human_{weapon}_{posture}_{action}")
                names.add(f"human_{weapon}_{posture}_{action}_alert")
        for action in POSTURELESS:
            names.add(f"human_{weapon}_{action}")
    return {crc(name): name for name in names}


def parse_bank(blob):
    clip_count = struct.unpack_from("<I", blob, 20)[0]
    sections = {}
    pos = 24
    while pos + 8 <= len(blob):
        tag = blob[pos:pos + 4]
        size = struct.unpack_from("<I", blob, pos + 4)[0]
        sections[tag] = (pos + 8, pos + 8 + size)
        pos += 8 + size
    mina, tnja, tada = sections[b"MINA"], sections[b"TNJA"], sections[b"TADA"]
    clips = []
    at = tnja[0]
    for i in range(clip_count):
        flags, _unused, name_crc, frames, bone_count = struct.unpack_from(
            "<IIIHH", blob, mina[0] + i * 16
        )
        bones = []
        for _ in range(bone_count):
            rec = struct.unpack_from("<I4I3I4f", blob, at)
            at += 48
            bones.append((rec[0], rec[1:5], rec[5:8], rec[8:11], rec[11]))
        clips.append((name_crc, flags, frames, bones))
    return clips, blob[tada[0]:tada[1]]


def curve(data, at, frames):
    value = struct.unpack_from("<h", data, at)[0]
    at += 2
    out = [value]
    while len(out) < frames:
        code = struct.unpack_from("<b", data, at)[0]
        at += 1
        if code == -128:
            out.extend([value] * data[at])
            at += 1
        elif code == -127:
            value = struct.unpack_from("<h", data, at)[0]
            at += 2
            out.append(value)
        else:
            value += code
            out.append(value)
    return out[:frames]


def decode_clip(bones, frames, data, bone_index):
    """Per skeleton bone: frames of [qx, qy, qz, qw, tx, ty, tz]."""
    tracks = []
    for bone_crc, rot, loc, bias, scale in bones:
        index = bone_index.get(bone_crc)
        if index is None:
            continue
        comps = [curve(data, off, frames) for off in rot]
        if scale:
            moves = [[bias[k] + scale * v for v in curve(data, loc[k], frames)] for k in range(3)]
        else:
            moves = [[bias[k]] * frames for k in range(3)]
        samples = []
        prev = (0.0, 0.0, 0.0, 1.0)
        for f in range(frames):
            q = [comps[k][f] for k in range(4)]
            # A few near-180 degree keys were clamped by the encoder. Renormalize.
            n = math.sqrt(sum(c * c for c in q))
            q = [c / n for c in q] if n > 1e-6 else list(prev)
            if sum(a * b for a, b in zip(q, prev)) < 0:
                q = [-c for c in q]
            prev = q
            samples.extend(q)
            samples.extend(moves[k][f] for k in range(3))
        tracks.append((index, samples))
    return tracks


def write_anims(data, skeleton, out_dir, banks=BANKS, names=None, file="anims"):
    """Decode banks for one skeleton into <file>.json and <file>.bin: by
    default the soldier banks, matched against the soldier clip names."""
    bone_index = {crc(name): i for i, name in enumerate(skeleton)}
    root = bone_index.get(crc("DummyRoot"))
    names = names or clip_names()
    clips = {}
    floats = array.array("f")
    for body, end in nodes(data, b"zaa_"):
        if node_name(data, body, end) not in banks:
            continue
        bin_c = child(data, body, end, b"BIN_")
        parsed, curves = parse_bank(data[bin_c[0]:bin_c[1]])
        for name_crc, flags, frames, bones in parsed:
            name = names.get(name_crc)
            if name is None or name in clips:
                continue
            tracks = decode_clip(bones, frames, curves, bone_index)
            travel = [0.0, 0.0, 0.0]
            for index, samples in tracks:
                if index == root and flags & ROOT_MOTION:
                    last = (frames - 1) * 7
                    travel = [samples[last + 4 + k] - samples[4 + k] for k in range(3)]
            clips[name] = {
                "frames": frames,
                "flags": flags,
                "offset": len(floats),
                "tracks": [index for index, _ in tracks],
                "travel": travel,
            }
            for _, samples in tracks:
                floats.extend(samples)
    meta = {"fps": FPS, "bones": skeleton, "clips": clips}
    if file == "anims":
        meta["sets"] = clip_sets(set(clips))
    (out_dir / f"{file}.json").write_text(json.dumps(meta))
    (out_dir / f"{file}.bin").write_bytes(floats.tobytes())
    return clips
