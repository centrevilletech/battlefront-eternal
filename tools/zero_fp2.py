#!/usr/bin/env python3
"""2005 (Star Wars: Battlefront II) first-person arms, animations, and camera motions.

    BFE_SWBF2_DIR=<install> python3 tools/zero_fp2.py [--out DIR] [--side SIDE ...]

Reads the 2005 game's first-person arm archives (`fpm/<side>/*.lvl`), first-person
animation banks (`humanfp` in `ingame.lvl`, plus optional side banks such as
`droidekafp` and `sbdroidfp`), and first-person camera motions (`globals` in
`common.lvl`), writing a self-contained `firstperson/` tree under the prepared
2005 content root:

    firstperson/manifest.json
    firstperson/anims.bin
    firstperson/rigs/<rig>.json, <rig>.bin, <rig>_index.bin
    firstperson/textures/<name>.png

Nothing here touches 2004 preparation or existing prepared side/map JSON files.
"""

import argparse
import array
import json
import math
import os
from pathlib import Path
import stat
import struct
import sys

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT / "tools") not in sys.path:
    sys.path.insert(0, str(ROOT / "tools"))

from ucfb import child, chunks, cstr_at, root_section, u32  # noqa: E402
from xform import transform_dir, transform_point  # noqa: E402
import zero_anim  # noqa: E402
from zero_hash import crc, fnv1a  # noqa: E402
from zero_lua import mission_scripts  # noqa: E402
from zero_model import collect_models  # noqa: E402
from zero_outputs2 import component, relative_name
from zero_paths import GAME, PUBLIC  # noqa: E402
import zero_swbf2  # noqa: E402
from zero_texture import SWBF2_DECODABLE, collect_textures, write_png  # noqa: E402

MANIFEST_VERSION = 2

MAX_VERTICES = 65_536
MAX_INDICES = 262_144
MAX_BONES = 64
MAX_FRAMES = 4096
MAX_FLOATS = 8 << 20


def read_resource(path, limit=512 << 20):
    if not stat.S_ISREG(path.stat().st_mode):
        raise ValueError(f"{path.name}: expected a regular file")
    flags = os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_BINARY", 0)
    with os.fdopen(os.open(path, flags), "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            raise ValueError(f"{path.name}: file exceeds {limit} bytes or is not a regular file")
        data = stream.read(info.st_size + 1)
        if len(data) != info.st_size:
            raise ValueError(f"{path.name}: file changed during preparation")
        return data


def little_endian(values):
    if values.itemsize != 4:
        raise ValueError("first-person products require four-byte numeric elements")
    if sys.byteorder != "little":
        values.byteswap()
    return values.tobytes()


CAMERA_MOTIONS = (
    "RollLeft",
    "RollRight",
    "StandToProne",
    "CrouchToProne",
    "ProneToStand",
    "CrouchToStand",
    "Land",
)
MOTION_KEYS = ("Vec", "Slope", "Ore", "RSlope", "Time")

# Structured first-person clip naming scheme for CRC matching.
_HUMAN_SETS = ("rifle", "bazooka", "tool", "grenade")
_HUMAN_ACTIONS = (
    "idle",
    "run",
    "shoot",
    "shoot2",
    "charge",
    "reload",
    "repair",
    "jump",
    "flail",
    "jump_land",
)
_FP_BANK_NAMES = ("humanfp", "droidekafp", "sbdroidfp")

# FNV-1a hashes for class properties that may be absent from the shared name table.
_FORCE_MODE_HASH = f"{fnv1a('ForceMode'):08x}"
_AIM_TENSION_HASH = f"{fnv1a('AimTension'):08x}"


def known_clip_names(bank_name):
    """Map CRC-32 hashes to canonical clip names for a first-person animation bank."""
    bank = bank_name.lower()
    names = [f"{bank}_handsdown"]
    if bank == "humanfp":
        for group in _HUMAN_SETS:
            for action in _HUMAN_ACTIONS:
                names.append(f"{bank}_{group}_{action}")
    elif bank == "droidekafp":
        for action in ("idle", "walk", "run", "shoot", "reload"):
            names.append(f"{bank}_rifle_{action}")
    elif bank == "sbdroidfp":
        for action in ("idle", "run", "shoot", "reload", "jump", "flail", "jump_land"):
            names.append(f"{bank}_rifle_{action}")
    return {crc(n): n for n in names}


def _find_case_insensitive(base, parts):
    """Resolve a relative path under `base` without regard to case."""
    cur = Path(base)
    for part in parts:
        direct = cur / component(part)
        if direct.exists():
            cur = direct
            continue
        if not cur.is_dir():
            return None
        target = part.lower()
        match = next((entry for entry in cur.iterdir() if entry.name.lower() == target), None)
        if match is None:
            return None
        cur = match
    return cur


def resolve_fpm_archive(game_dir, source):
    """Resolve a class's `FirstPerson` property (`<dir>\\<archive>;<model>`) to (Path, model_name)."""
    if not source or ";" not in source:
        return None, None
    path_part, _, model_name = source.partition(";")
    model_name = component(model_name.strip().lower())
    rel = path_part.strip().replace("\\", "/")
    if not rel or not model_name:
        return None, None
    segs = relative_name(rel).split("/")
    if not segs:
        return None, None
    segs[-1] = segs[-1] if segs[-1].lower().endswith(".lvl") else f"{segs[-1]}.lvl"
    resolved = _find_case_insensitive(game_dir, ["fpm", *segs])
    return resolved, model_name


def _topological_skeleton(skeleton):
    """Order skeleton bones so every parent appears before its children.

    Accepts parent entries either as bone name strings (from `zero_model.collect_models`,
    where empty string / None / missing name marks a root) or as integer indices (`-1` root).
    Returns (bones_list, old_to_new, model_matrices_by_old_index).
    Raises ValueError on cycles or out-of-range parent indices.
    """
    names = skeleton["names"]
    raw_parents = skeleton["parents"]
    local = skeleton["local"]
    model = skeleton["model"]
    count = len(names)
    if not 1 <= count <= MAX_BONES:
        raise ValueError("first-person skeleton bone count outside 1..64")
    if len(raw_parents) != count or len(local) != count or len(model) != count:
        raise ValueError("first-person skeleton arrays have mismatched lengths")

    name_to_idx = {n.lower(): idx for idx, n in enumerate(names)}
    parents = []
    for i, p in enumerate(raw_parents):
        if p is None or p == "":
            parents.append(-1)
        elif isinstance(p, str):
            if p.lower() not in name_to_idx:
                raise ValueError(f"unknown first-person parent {p}")
            parents.append(name_to_idx[p.lower()])
        elif isinstance(p, int):
            if p >= count or p < -1:
                raise ValueError(f"first-person bone {names[i]} has invalid parent index {p}")
            parents.append(p)
        else:
            raise ValueError(f"first-person bone {names[i]} has invalid parent {p!r}")

    placed = [False] * count
    order = []
    while len(order) < count:
        before = len(order)
        for i in range(count):
            if placed[i]:
                continue
            p = parents[i]
            if p == -1 or placed[p]:
                placed[i] = True
                order.append(i)
        if len(order) == before:
            raise ValueError("cycle detected in first-person skeleton")

    old_to_new = {old: new for new, old in enumerate(order)}
    bones = []
    for old in order:
        p = parents[old]
        mat = [round(float(x), 7) for x in local[old]]
        if not all(math.isfinite(x) for x in mat):
            raise ValueError(f"non-finite bind matrix on bone {names[old]}")
        bones.append({
            "name": names[old].lower(),
            "crc": f"{crc(names[old]):08x}",
            "parent": old_to_new[p] if p >= 0 else -1,
            "matrix": mat,
        })
    return bones, old_to_new, model


def _invert_rigid(m):
    """Inverse of a rigid column-major 4x4 transform (R^T and -R^T * t)."""
    r00, r10, r20 = m[0], m[1], m[2]
    r01, r11, r21 = m[4], m[5], m[6]
    r02, r12, r22 = m[8], m[9], m[10]
    tx, ty, tz = m[12], m[13], m[14]
    return [
        r00, r01, r02, 0.0,
        r10, r11, r12, 0.0,
        r20, r21, r22, 0.0,
        -(r00 * tx + r10 * ty + r20 * tz),
        -(r01 * tx + r11 * ty + r21 * tz),
        -(r02 * tx + r12 * ty + r22 * tz),
        1.0,
    ]


def bone_local_parts(parts, skeleton, order):
    """Convert each vertex to its own joint space and keep the triangle topology."""
    names = skeleton["names"]
    inverse = [_invert_rigid(m) for m in skeleton["model"]]
    out = []
    for part in parts:
        slots = part.get("skin")
        if slots is None or part.get("bone"):
            out.append(part)
            continue
        if part.get("softSkin"):
            raise ValueError("first-person rig uses unsupported weighted skin")
        positions, normals = part["positions"], part["normals"]
        if not len(positions) == len(normals) == len(slots):
            raise ValueError("first-person skin vertex count differs from its geometry")
        palette = part.get("bmap") or list(range(len(names)))
        if any(not 0 <= joint < len(names) for joint in palette):
            raise ValueError("first-person skin palette references an unknown joint")
        if any(not 0 <= slot < len(palette) for slot in slots):
            raise ValueError("first-person skin vertex references an unknown palette slot")
        joints = [palette[slot] for slot in slots]
        out.append(dict(part, joints=[order[joint] for joint in joints],
                        local_positions=[transform_point(inverse[j], v) for j, v in zip(joints, positions)],
                        local_normals=[transform_dir(inverse[j], v) for j, v in zip(joints, normals)]))
    return out


def _write_rig(model_name, parts, skeleton, out_dir):
    """Write one prepared first-person rig and its material-indexed parts."""
    component(model_name)
    if len(parts) > 64:
        raise ValueError("first-person rig exceeds 64 parts")
    bones, order, _ = _topological_skeleton(skeleton)
    bone_names = {b["name"] for b in bones}
    rigid_parts = bone_local_parts(parts, skeleton, order)
    was_skinned = any(p.get("skin") is not None for p in rigid_parts)

    verts = array.array("f")
    indices = array.array("I")
    part_records = []
    used_textures = set()

    for part in rigid_parts:
        bone = (part.get("bone") or "").lower()
        if bone and bone not in bone_names:
            # Rigid part attached to a non-skeleton mesh node falls back to root bone.
            bone = bones[0]["name"] if bones else ""
        positions = part.get("local_positions") or part.get("positions") or []
        normals = part.get("local_normals") or part.get("normals") or []
        uvs = part.get("uvs") or []
        part_indices = part.get("indices") or []
        if not positions or not part_indices:
            continue
        if len(positions) > MAX_VERTICES or len(part_indices) > MAX_INDICES:
            raise ValueError(f"{model_name}: geometry count exceeds limit")
        if len(normals) != len(positions) or len(uvs) != len(positions):
            raise ValueError(f"{model_name}: mismatched vertex attribute counts")

        base = len(verts) // 8
        for p, n, uv in zip(positions, normals, uvs):
            row = (p[0], p[1], p[2], n[0], n[1], n[2], uv[0], uv[1])
            if not all(math.isfinite(x) for x in row):
                raise ValueError(f"{model_name}: non-finite vertex data")
            verts.extend(row)

        index_at = len(indices)
        vertex_count = len(positions)
        for idx in part_indices:
            if idx < 0 or idx >= vertex_count:
                raise ValueError(f"{model_name}: index {idx} out of range {vertex_count}")
            indices.append(base + idx)

        tex = (part.get("texture") or "").lower()
        bump = (part.get("bump") or "").lower() or None
        if tex:
            used_textures.add(component(tex))
        if bump:
            used_textures.add(component(bump))

        mat = dict(part.get("material") or {})
        rec = {
            "bone": bone or bones[0]["name"],
            "texture": tex,
            "bump": bump,
            "material": mat,
            "vertexOffset": base * 32,
            "vertexCount": vertex_count,
            "indexOffset": index_at * 4,
            "indexCount": len(part_indices),
            "additive": bool(part.get("additive", False)),
            "transparent": bool(part.get("transparent", False)),
            "flags": int(mat.get("flags", 1)),
        }
        if part.get("skin") is not None and not bone:
            rec["joints"] = part["joints"]
        if mat.get("scroll"):
            rec["scroll"] = mat["scroll"]
        part_records.append(rec)

    if not part_records:
        raise ValueError(f"{model_name}: first-person rig has no drawable geometry")

    out_dir.mkdir(parents=True, exist_ok=True)
    rig_doc = {
        "version": MANIFEST_VERSION,
        "model": model_name,
        "stride": 32,
        "skinnedSource": was_skinned,
        "bones": bones,
        "parts": part_records,
    }
    (out_dir / f"{model_name}.json").write_text(json.dumps(rig_doc, allow_nan=False))
    (out_dir / f"{model_name}.bin").write_bytes(little_endian(verts))
    (out_dir / f"{model_name}_index.bin").write_bytes(little_endian(indices))
    return {
        "file": f"rigs/{model_name}.json",
        "model": model_name,
        "skinned": was_skinned,
        "bones": len(bones),
        "parts": len(part_records),
    }, used_textures


def eval_lua50_globals(buf):
    """Evaluate straight-line table construction in a Lua 5.0 bytecode chunk.

    Supports the constant and table-building opcodes used by Battlefront II's
    `globals` script to define first-person camera motion curves. Returns a dict
    mapping global names to Python lists/dicts/numbers/strings.
    """
    if len(buf) < 18 or buf[:5] != b"\x1bLuaP":
        raise ValueError("not a Lua 5.0 chunk")
    endian, _sz_int, _sz_sizet, sz_inst, sz_op, sz_a, sz_b, sz_c, sz_num = buf[5:14]
    if endian != 1 or sz_inst != 4 or sz_op != 6 or sz_a != 8 or sz_b != 9 or sz_c != 9 or sz_num != 4:
        raise ValueError("unsupported Lua 5.0 header parameters")
    pos = 14 + sz_num

    def u32_read():
        nonlocal pos
        if pos + 4 > len(buf):
            raise ValueError("truncated Lua 5.0 chunk")
        val = struct.unpack_from("<I", buf, pos)[0]
        pos += 4
        return val

    def i32_read():
        nonlocal pos
        if pos + 4 > len(buf):
            raise ValueError("truncated Lua 5.0 chunk")
        val = struct.unpack_from("<i", buf, pos)[0]
        pos += 4
        return val

    def f32_read():
        nonlocal pos
        if pos + 4 > len(buf):
            raise ValueError("truncated Lua 5.0 chunk")
        val = struct.unpack_from("<f", buf, pos)[0]
        pos += 4
        return val

    def str_read():
        nonlocal pos
        n = u32_read()
        if n == 0:
            return None
        if pos + n > len(buf):
            raise ValueError("truncated Lua 5.0 string")
        text = buf[pos:pos + n - 1].decode("latin1")
        pos += n
        return text

    str_read()  # source
    i32_read()  # lineDefined
    if pos + 4 > len(buf):
        raise ValueError("truncated Lua 5.0 function header")
    pos += 4  # nups, numparams, is_vararg, maxstack

    nlines = i32_read()
    if nlines < 0 or pos + nlines * 4 > len(buf):
        raise ValueError("invalid Lua 5.0 lineinfo")
    pos += nlines * 4

    nlocs = i32_read()
    if not 0 <= nlocs <= 4096:
        raise ValueError("invalid Lua local count")
    for _ in range(nlocs):
        str_read()
        i32_read()
        i32_read()

    nups = i32_read()
    if not 0 <= nups <= 256:
        raise ValueError("invalid Lua upvalue count")
    for _ in range(nups):
        str_read()

    nk = i32_read()
    if not 0 <= nk <= 8192:
        raise ValueError("invalid Lua constant count")
    consts = []
    for _ in range(nk):
        if pos >= len(buf):
            raise ValueError("truncated Lua 5.0 constants")
        tag = buf[pos]
        pos += 1
        if tag == 0:
            consts.append(None)
        elif tag == 3:
            consts.append(float(f32_read()))
        elif tag == 4:
            consts.append(str_read())
        else:
            raise ValueError(f"unsupported Lua 5.0 constant type {tag}")

    np = i32_read()
    if np != 0:
        raise ValueError("nested Lua 5.0 prototypes unsupported in globals reader")

    ncode = i32_read()
    if not 0 <= ncode <= 65536 or pos + ncode * 4 > len(buf):
        raise ValueError("invalid Lua 5.0 code block")
    code = [u32_read() for _ in range(ncode)]

    # Standard Lua 5.0 uses MAXSTACK=250 (RK threshold 250); some builds use 128.
    bitrk = 128 if any(
        128 <= ((ins >> 15) & 0x1FF) < 250 for ins in code if (ins & 0x3F) == 9
    ) else 250

    regs = [None] * 256
    env = {}

    def rk(val):
        if val >= bitrk:
            idx = val - bitrk
            return consts[idx] if idx < len(consts) else None
        return regs[val] if val < len(regs) else None

    for ins in code:
        op = ins & 0x3F
        c = (ins >> 6) & 0x1FF
        b = (ins >> 15) & 0x1FF
        a = (ins >> 24) & 0xFF
        bx = (ins >> 6) & 0x3FFFF
        if op == 0:  # MOVE
            regs[a] = regs[b]
        elif op == 1:  # LOADK
            regs[a] = consts[bx] if bx < len(consts) else None
        elif op == 2:  # LOADBOOL
            regs[a] = bool(b)
        elif op == 3:  # LOADNIL
            for i in range(a, min(b + 1, len(regs))):
                regs[i] = None
        elif op == 5:  # GETGLOBAL
            regs[a] = env.get(consts[bx]) if bx < len(consts) else None
        elif op == 6:  # GETTABLE
            tbl = regs[b]
            regs[a] = tbl.get(rk(c)) if isinstance(tbl, dict) else None
        elif op == 7:  # SETGLOBAL
            if bx < len(consts):
                env[consts[bx]] = regs[a]
        elif op == 9:  # SETTABLE
            if isinstance(regs[a], dict):
                regs[a][rk(b)] = rk(c)
        elif op == 10:  # NEWTABLE
            regs[a] = {}
        elif op == 17:  # UNM
            regs[a] = -regs[b] if isinstance(regs[b], (int, float)) else None
        elif op in (26, 27):  # RETURN
            break
        elif op == 31:  # SETLIST (FPF=32)
            if isinstance(regs[a], dict):
                count = (bx % 32) + 1
                base = bx - (bx % 32)
                for i in range(1, count + 1):
                    if a + i < len(regs):
                        regs[a][float(base + i)] = regs[a + i]
        else:
            break

    visits = 0
    def _to_lists(val, depth=0):
        nonlocal visits
        visits += 1
        if depth > 16 or visits > 32768:
            raise ValueError("camera table nesting or entry count exceeds limit")
        if not isinstance(val, dict):
            return val
        out = {k: _to_lists(v, depth + 1) for k, v in val.items()}
        if out and all(isinstance(k, float) for k in out):
            expected = [float(i) for i in range(1, len(out) + 1)]
            if sorted(out) == expected:
                return [out[k] for k in expected]
        return {
            (str(int(k)) if isinstance(k, float) and k.is_integer() else str(k)): v
            for k, v in out.items()
        }

    return {str(k): _to_lists(v) for k, v in env.items()}


def extract_camera_motions(common_data):
    """Extract first-person camera motion curves from Battlefront II `common.lvl`."""
    scripts = mission_scripts(common_data)
    body = scripts.get("globals")
    if not body:
        return {}
    found = eval_lua50_globals(body)
    motions = {}
    for name in CAMERA_MOTIONS:
        entry = found.get(name)
        if not isinstance(entry, dict) or not all(k in entry for k in MOTION_KEYS):
            continue
        cleaned = {}
        valid = True
        for key in MOTION_KEYS:
            vals = entry[key]
            if not isinstance(vals, list):
                valid = False
                break
            if key == "Time":
                cleaned[key] = [round(float(v), 6) for v in vals]
            else:
                cleaned[key] = [[round(float(x), 6) for x in row] for row in vals]
        if valid:
            count = len(cleaned["Time"])
            if not 1 <= count <= 64 or not all(math.isfinite(t) and 0 < t <= 60 for t in cleaned["Time"]):
                raise ValueError(f"{name}: invalid camera segment duration")
            for key in MOTION_KEYS[:-1]:
                if len(cleaned[key]) != count or any(len(v) != 3 or not all(map(math.isfinite, v)) for v in cleaned[key]):
                    raise ValueError(f"{name}: invalid camera key array")
            motions[name] = cleaned
    return motions


def checked_clips(blob):
    if len(blob) < 24 or blob[:4] != b"SMNA" or not 1 <= u32(blob, 20) <= 64:
        raise ValueError("invalid first-person animation header or clip count")
    spans = {}
    at = 24
    while at < len(blob):
        if at + 8 > len(blob):
            if len(blob) - at <= 3 and not any(blob[at:]):
                break
            raise ValueError("truncated first-person animation section")
        tag, length = struct.unpack_from("<4sI", blob, at)
        start = at + 8
        at = start + length
        if at > len(blob) or tag in spans:
            raise ValueError("invalid first-person animation section bounds")
        spans[tag] = (start, length)
    start, length = spans[b"MINA"]
    count = u32(blob, 20)
    if length != count * 16:
        raise ValueError("invalid first-person clip table length")
    tracks = 0
    for i in range(count):
        frames, bones = struct.unpack_from("<HH", blob, start + i * 16 + 12)
        if not 1 <= frames <= MAX_FRAMES or not 1 <= bones <= MAX_BONES + 1:
            raise ValueError("invalid first-person clip frame or track count")
        tracks += bones
    if spans[b"TNJA"][1] != tracks * 48:
        raise ValueError("invalid first-person track table length")
    return zero_anim.parse_bank(blob)


def check_mesh_buffers(data):
    for start, stop in _all_sections(data):
        for tag, body, end in chunks(data, start, stop):
            if tag == b"skel":
                info = child(data, body, end, b"INFO")
                if info:
                    at = data.find(b"\0", *info)
                    if at < 0 or at + 3 > info[1] or not 1 <= struct.unpack_from("<H", data, at+1)[0] <= MAX_BONES:
                        raise ValueError("invalid first-person source skeleton size")
            if tag != b"modl":
                continue
            for name, b, e in chunks(data, body, end):
                if name != b"segm":
                    continue
                for kind, a, z in chunks(data, b, e):
                    if kind not in (b"IBUF", b"VBUF", b"SKIN", b"BMAP"):
                        continue
                    count = u32(data, a)
                    header, stride, limit = 4, 1, MAX_BONES
                    if kind == b"IBUF":
                        stride, limit = 2, MAX_INDICES
                    elif kind == b"VBUF":
                        header, stride, limit = 12, u32(data, a+4), MAX_VERTICES
                    elif kind == b"SKIN":
                        header, stride, limit = 8, 5 if u32(data, a+4) == 3 else 1, MAX_VERTICES
                    if count > limit or not 0 < stride <= 256 or a + header + count*stride > z:
                        raise ValueError("invalid first-person source geometry buffer")


def _decode_fp_banks(archive_blobs, anims_out):
    """Extract first-person `zaa_` clip banks and `zaf_` skeletons from archive bytes."""
    bone_map = {crc(n): n for n in ("hp_camera", "hp_weapons")}
    banks = {}
    skeletons = {}
    floats = array.array("f")

    for data in archive_blobs:
        for start, stop in _all_sections(data):
            for tag, body, end in chunks(data, start, stop):
                if tag not in (b"zaa_", b"zaf_"):
                    continue
                nc = child(data, body, end, b"NAME")
                if not nc:
                    continue
                name = cstr_at(data, *nc).lower()
                if name not in _FP_BANK_NAMES:
                    continue
                if tag == b"zaf_" and name not in skeletons:
                    zaf = zero_swbf2.parse_zaf(data, body, end)
                    joints = []
                    for j in zaf.get("joints", []):
                        hcrc = int(j["boneCrc"], 16)
                        joints.append({
                            "name": bone_map.get(hcrc, j["boneCrc"]),
                            "crc": j["boneCrc"],
                            "parentCrc": j["parentCrc"],
                            "position": [round(float(x), 7) for x in j["position"]],
                            "rotation": [round(float(x), 7) for x in j["rotation"]],
                        })
                    hp_cam = next((j["position"] for j in joints if j["name"] == "hp_camera"), None)
                    skeletons[name] = {
                        "joints": joints,
                        "cameraOffset": hp_cam,
                    }
                elif tag == b"zaa_" and name not in banks:
                    bc = child(data, body, end, b"BIN_")
                    if not bc:
                        raise ValueError(f"{name}: missing BIN_ animation data")
                    blob = data[bc[0]:bc[1]]
                    raw_clips, curves = checked_clips(blob)
                    clip_names = known_clip_names(name)
                    clips_meta = {}
                    for clip_crc, flags, frames, bones in raw_clips:
                        cname = clip_names.get(clip_crc, f"{clip_crc:08x}")
                        # Deduplicate repeated bone tracks (e.g. duplicate dummyroot in humanfp).
                        seen_bones = set()
                        unique_bones = []
                        for entry in bones:
                            bcrc = entry[0]
                            if bcrc in seen_bones:
                                continue
                            seen_bones.add(bcrc)
                            unique_bones.append(entry)
                        if len(floats) + frames * len(unique_bones) * 7 > MAX_FLOATS:
                            raise ValueError("first-person animation samples exceed budget")
                        if any(not all(math.isfinite(x) for x in (*entry[3], entry[4])) for entry in bones):
                            raise ValueError("non-finite first-person track scale or bias")
                        decoded = zero_anim.decode_clip(
                            unique_bones,
                            frames,
                            curves,
                            {entry[0]: entry[0] for entry in unique_bones},
                        )
                        clip_offset = len(floats)
                        tracks = []
                        for bcrc, samples in decoded:
                            tracks.append({
                                "name": bone_map.get(bcrc, f"{bcrc:08x}"),
                                "crc": f"{bcrc:08x}",
                            })
                            if not all(math.isfinite(x) for x in samples):
                                raise ValueError("non-finite first-person animation sample")
                            floats.extend(samples)
                        clips_meta[cname] = {
                            "crc": f"{clip_crc:08x}",
                            "frames": frames,
                            "fps": zero_anim.FPS,
                            "flags": flags,
                            "offset": clip_offset,
                            "tracks": tracks,
                        }
                    banks[name] = {
                        "fps": zero_anim.FPS,
                        "clips": clips_meta,
                    }

    for name, bank in banks.items():
        if name in skeletons:
            bank["skeleton"] = skeletons[name]

    anims_out.parent.mkdir(parents=True, exist_ok=True)
    anims_out.write_bytes(little_endian(floats))
    return banks


def _all_sections(data):
    """Yield (start, stop) for root and nested `lvl_` sub-archives."""
    from ucfb import iter_all_archives
    return list(iter_all_archives(data))


def _prop_from_pairs(pairs, key_lower, hash_hex=None):
    """Read last matching value from an ordered sequence of (k, v) property pairs."""
    found = None
    for k, v in pairs:
        kl = k.lower()
        if kl == key_lower or (hash_hex and kl == hash_hex):
            found = v
    return found


def _collect_classes_from_prepared(public_dir, side_filter=None):
    """Read unit class records from already-prepared `sides/<side>/units.json` files."""
    sides_dir = Path(public_dir) / "sides"
    records = {}
    if not sides_dir.is_dir():
        return records
    for path in sorted(sides_dir.glob("*/units.json")):
        side = path.parent.name.lower()
        if side_filter and side not in side_filter:
            continue
        doc = json.loads(path.read_text())
        units = doc.get("units", {})
        if not isinstance(units, dict):
            continue
        for uname, udata in units.items():
            if not isinstance(udata, dict):
                continue
            props = udata.get("properties") or []
            if isinstance(props, dict):
                pairs = list(props.items())
            else:
                pairs = [(str(p[0]), str(p[1])) for p in props if isinstance(p, (list, tuple)) and len(p) >= 2]
            records[f"{side}/{uname.lower()}"] = {
                "side": side,
                "name": uname.lower(),
                "base": (udata.get("base") or "").lower(),
                "parents": [str(p).lower() for p in (udata.get("parents") or [])],
                "props": pairs,
            }
    return records


def _collect_classes_from_archives(game_dir, sides_units):
    """Read class records directly from `ingame.lvl` and `side/<side>.lvl` archives."""
    ingame_path = _find_case_insensitive(game_dir, ["ingame.lvl"])
    shared = []
    if ingame_path and ingame_path.is_file():
        data = read_resource(ingame_path)
        shared.append((data, _all_sections(data)))

    records = {}
    for side, wanted in sides_units.items():
        side_l = side.lower()
        side_path = _find_case_insensitive(game_dir, ["side", f"{side_l}.lvl"])
        if not side_path or not side_path.is_file():
            raise ValueError(f"missing requested first-person side archive {side_l}")
        sdata = read_resource(side_path)
        sections = _all_sections(sdata)
        catalog = zero_swbf2.class_catalog([(sdata, sections), *shared])
        entc = catalog.get("entc") or {}
        names = [n.lower() for n in wanted] if wanted else sorted(entc)
        models = None
        for uname in names:
            if uname not in entc:
                # A mission can load scenery or ship geometry from the side file alongside soldiers.
                if models is None:
                    models = {cstr_at(sdata, *name).lower() for begin, end in sections
                              for tag, start, stop in chunks(sdata, begin, end)
                              if tag == b"modl" and (name := child(sdata, start, stop, b"NAME"))}
                if uname not in models:
                    raise ValueError(f"{side_l}/{uname}: requested class or model missing from side archive")
                continue
            resolved = zero_swbf2.resolve_class("entc", uname, catalog)
            if not resolved:
                raise ValueError(f"{side_l}/{uname}: cannot resolve requested class")
            base = (resolved.get("engineBase") or resolved.get("base") or "").lower()
            if base not in ("soldier", "walkerdroid"):
                continue
            records[f"{side_l}/{uname}"] = {
                "side": side_l,
                "name": uname,
                "base": base,
                "parents": [p.lower() for p in resolved.get("parents", ())],
                "props": list(resolved.get("props", ())),
            }
    return records


def _bank_for_class(base, rig_model, properties=()):
    """Choose a first-person animation bank for a unit class."""
    for name in ("firstpersonanimationbank", "firstpersonanimation"):
        explicit = _prop_from_pairs(properties, name, f"{fnv1a(name):08x}")
        if explicit:
            return component(str(explicit).lower())
    model = (rig_model or "").lower()
    if base == "walkerdroid" or "droideka" in model:
        return "droidekafp"
    if "sbdroid" in model:
        return "sbdroidfp"
    return "humanfp"


def _classify_unit(rec):
    """Classify a unit for first-person view support.

    Returns (fp_spec_or_none, third_person_reason_or_none).
    """
    parents = rec.get("parents") or ()
    props = rec.get("props") or ()
    if "com_jedi_default" in parents:
        return None, "jedi"
    force_mode = _prop_from_pairs(props, "forcemode", _FORCE_MODE_HASH)
    if force_mode is not None:
        try:
            if float(force_mode) == 1.0:
                return None, "force_mode"
        except ValueError:
            pass
    fp_source = _prop_from_pairs(props, "firstperson")
    if not fp_source or ";" not in fp_source:
        return None, "no_first_person"
    fov_str = _prop_from_pairs(props, "firstpersonfov")
    try:
        fov = float(fov_str) if fov_str is not None else 70.0
    except ValueError:
        fov = 70.0
    if not math.isfinite(fov) or fov <= 1.0 or fov >= 179.0:
        fov = 70.0
    tension_str = _prop_from_pairs(props, "aimtension", _AIM_TENSION_HASH)
    try:
        aim_tension = float(tension_str) if tension_str is not None else 0.0
    except ValueError:
        aim_tension = 0.0
    if not math.isfinite(aim_tension) or aim_tension < 0.0:
        aim_tension = 0.0
    return {
        "source": fp_source.strip(),
        "fov": fov,
        "aimTension": aim_tension,
    }, None


def write_first_person_files(game_dir, public_dir, sides_units=None):
    """Prepare all 2005 first-person rigs, animation banks, textures, and camera motions."""
    game_dir = Path(game_dir)
    public_dir = Path(public_dir)
    out_root = public_dir / "firstperson"
    rigs_dir = out_root / "rigs"
    tex_dir = out_root / "textures"

    # Discover classes: prefer direct game archive lookup when sides_units is supplied,
    # otherwise inspect prepared sides/*/units.json (falling back to archives if absent).
    if sides_units:
        class_records = _collect_classes_from_archives(game_dir, sides_units)
    else:
        class_records = _collect_classes_from_prepared(public_dir)
        if not class_records:
            side_folder = _find_case_insensitive(game_dir, ["side"])
            if side_folder and side_folder.is_dir():
                all_sides = {p.stem.lower(): [] for p in sorted(side_folder.glob("*.lvl"))}
                class_records = _collect_classes_from_archives(game_dir, all_sides)

    # Read shared archives for animation banks, fallback textures, and camera motions.
    ingame_path = _find_case_insensitive(game_dir, ["ingame.lvl"])
    common_path = _find_case_insensitive(game_dir, ["common.lvl"])
    if not ingame_path or not ingame_path.is_file():
        raise FileNotFoundError(f"missing ingame.lvl under {game_dir}")

    ingame_bytes = read_resource(ingame_path)
    common_bytes = read_resource(common_path) if (common_path and common_path.is_file()) else b""

    bank_archives = [ingame_bytes]
    cis_path = _find_case_insensitive(game_dir, ["side", "cis.lvl"])
    if cis_path and cis_path.is_file():
        bank_archives.append(read_resource(cis_path))

    banks = _decode_fp_banks(bank_archives, out_root / "anims.bin")
    camera_motions = extract_camera_motions(common_bytes) if common_bytes else {}

    if any(not banks.get(name, {}).get("skeleton", {}).get("cameraOffset") for name in _FP_BANK_NAMES):
        raise ValueError("missing required first-person animation bank or camera joint")
    if set(camera_motions) != set(CAMERA_MOTIONS):
        raise ValueError("missing required first-person camera curves")
    if len(class_records) > 1024:
        raise ValueError("first-person class count exceeds 1024")

    # Lazy fallback texture pool from side archives and common.lvl.
    fallback_textures = {}
    loaded_fallback_sides = set()

    def ensure_fallback_textures(side_name):
        if side_name in loaded_fallback_sides:
            return
        loaded_fallback_sides.add(side_name)
        sp = _find_case_insensitive(game_dir, ["side", f"{side_name}.lvl"])
        if sp and sp.is_file():
            collect_textures(read_resource(sp), fallback_textures, decodable=SWBF2_DECODABLE)
        if common_bytes and "common" not in loaded_fallback_sides:
            loaded_fallback_sides.add("common")
            collect_textures(common_bytes, fallback_textures, decodable=SWBF2_DECODABLE)

    rigs_manifest = {}
    classes_manifest = {}
    third_person_only = {}
    written_textures = {}
    fpm_cache = {}
    bone_names = {}

    for class_key in sorted(class_records):
        rec = class_records[class_key]
        spec, reason = _classify_unit(rec)
        if reason is not None:
            third_person_only[class_key] = reason
            continue

        archive_path, model_name = resolve_fpm_archive(game_dir, spec["source"])
        if archive_path is None or not archive_path.is_file():
            raise ValueError(f"{class_key}: missing archive for {spec['source']}")

        cache_key = str(archive_path)
        if cache_key not in fpm_cache:
            data = read_resource(archive_path, 64 << 20)
            check_mesh_buffers(data)
            models, skeletons, textures = {}, {}, {}
            if len(data) > 16:
                collect_models(data, models, skeletons, swbf2=True)
                collect_textures(data, textures, decodable=SWBF2_DECODABLE)
            if len(fpm_cache) >= 2:
                fpm_cache.pop(next(iter(fpm_cache)))
            fpm_cache[cache_key] = (models, skeletons, textures)
        models, skeletons, fpm_textures = fpm_cache[cache_key]

        # Match model name without regard to case.
        matched_model = next((k for k in models if k.lower() == model_name), None)
        if matched_model is None:
            raise ValueError(f"{class_key}: missing model for {spec['source']}")
        matched_skel = next((skeletons[k] for k in skeletons if k.lower() == model_name), None)
        if matched_skel is None:
            raise ValueError(f"{class_key}: missing skeleton for {spec['source']}")

        bone_names.update((f"{crc(n):08x}", n.lower()) for n in matched_skel["names"])
        rig_id = model_name
        bank_name = _bank_for_class(rec["base"], model_name, rec.get("props", ()))
        if bank_name not in banks:
            raise ValueError(f"{class_key}: unknown first-person animation bank {bank_name}")
        if rig_id in rigs_manifest and rigs_manifest[rig_id]["bank"] != bank_name:
            rig_id = f"{model_name}-{bank_name}"
        if rig_id not in rigs_manifest:
            if len(rigs_manifest) >= 64:
                raise ValueError("first-person rig count exceeds 64")
            rig_entry, used_tex = _write_rig(rig_id, models[matched_model], matched_skel, rigs_dir)
            rig_entry["source"] = spec["source"]
            rig_entry["bank"] = bank_name
            rigs_manifest[rig_id] = rig_entry

            # Decode and write textures used by this rig.
            fpm_by_lower = {k.lower(): v for k, v in fpm_textures.items()}
            tex_dir.mkdir(parents=True, exist_ok=True)
            for tname in sorted(used_tex):
                if tname in written_textures:
                    continue
                img = fpm_by_lower.get(tname)
                if img is None:
                    ensure_fallback_textures(rec["side"])
                    fb_by_lower = {k.lower(): v for k, v in fallback_textures.items()}
                    img = fb_by_lower.get(tname)
                if img is not None:
                    if not 1 <= img.width <= 4096 or not 1 <= img.height <= 4096:
                        raise ValueError(f"{tname}: invalid first-person texture dimensions")
                    w, h, rgba = img
                    write_png(tex_dir / f"{tname}.png", w, h, rgba)
                    written_textures[tname] = f"textures/{tname}.png"

        classes_manifest[class_key] = {
            "rig": rig_id,
            "bank": bank_name,
            "fov": spec["fov"],
            "aimTension": spec["aimTension"],
            "source": spec["source"],
        }

    for bank in banks.values():
        for clip in bank["clips"].values():
            for track in clip["tracks"]:
                track["name"] = bone_names.get(track["crc"], track["name"])
        for joint in bank.get("skeleton", {}).get("joints", []):
            joint["name"] = bone_names.get(joint["crc"], joint["name"])

    manifest = {
        "version": MANIFEST_VERSION,
        "family": "swbf2",
        "animsData": "anims.bin",
        "banks": banks,
        "rigs": rigs_manifest,
        "classes": classes_manifest,
        "thirdPersonOnly": third_person_only,
        "cameraMotions": camera_motions,
        "textures": written_textures,
    }
    out_root.mkdir(parents=True, exist_ok=True)
    (out_root / "manifest.json").write_text(json.dumps(manifest, indent=2, allow_nan=False))
    print(
        f"firstperson: rigs={len(rigs_manifest)} classes={len(classes_manifest)} "
        f"third_person_only={len(third_person_only)} banks={sorted(banks)} "
        f"textures={len(written_textures)} camera_motions={len(camera_motions)}",
        flush=True,
    )
    return manifest


def _resolve_game_dir(arg_game):
    if arg_game:
        base = Path(arg_game)
    elif os.environ.get("BFE_SWBF2_DIR"):
        base = Path(os.environ["BFE_SWBF2_DIR"])
    else:
        return GAME
    for rel in (
        Path("GameData/data/_lvl_pc"),
        Path("GameData/Data/_LVL_PC"),
        Path("install/GameData/data/_lvl_pc"),
        Path("install/GameData/Data/_LVL_PC"),
        Path("data/_lvl_pc"),
        Path("Data/_LVL_PC"),
        Path("."),
    ):
        cand = base / rel
        if _find_case_insensitive(cand, ["ingame.lvl"]):
            return cand
    return base


def _parse_side_unit_args(argv):
    """Parse repeatable `--side S` followed by zero or more `--unit U` into {side: [units]}."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--game", type=Path, help="installed 2005 game folder or _lvl_pc directory")
    parser.add_argument("--out", type=Path, help="prepared content root (default: BFE_PUBLIC_DIR)")
    parser.add_argument("--side", action="append", default=[], help="side archive stem to scan")
    parser.add_argument("--unit", action="append", default=[], help="unit class name to include")
    args = parser.parse_args(argv)

    # Preserve --side / --unit grouping when interleaved on the command line.
    sides_units = {}
    current_side = None
    it = iter(argv)
    for tok in it:
        if tok == "--side":
            current_side = next(it, "").lower()
            if current_side:
                sides_units.setdefault(current_side, [])
        elif tok == "--unit" and current_side is not None:
            unit = next(it, "").lower()
            if unit and unit not in sides_units[current_side]:
                sides_units[current_side].append(unit)
    return args, (sides_units or None)


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    args, sides_units = _parse_side_unit_args(argv)
    game_dir = _resolve_game_dir(args.game)
    public_dir = args.out or PUBLIC
    try:
        from zero_outputs2 import destination_origin
        if destination_origin(public_dir) != "empty":
            raise ValueError("Use the common preparation step to update an existing content inventory.")
        write_first_person_files(game_dir, public_dir, sides_units=sides_units)
    except (OSError, ValueError, KeyError, IndexError, TypeError, ArithmeticError, RecursionError, struct.error) as error:
        print(f"First-person preparation failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
