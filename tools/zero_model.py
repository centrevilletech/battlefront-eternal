"""Model chunks (`modl`, `skel`): segments, vertex formats, and skeletons."""

import math
import struct

from ucfb import child, children_of, chunks, cstr_at, f32s, i16s, iter_archives, u32
from xform import mat4, mul, transform_dir, transform_point

POS = 0b10
BONE_IDX = 0b100
BONE_W = 0b1000
NORMAL = 0b100000
TANGENTS = 0b1000000
COLOR = 0b10000000
STATIC_LIT = 0b100000000
UV = 0b1000000000
POS_C = 0b1000000000000
BONE_C = 0b10000000000000
NORM_C = 0b100000000000000
UV_C = 0b1000000000000000
COMPRESSED = POS_C | BONE_C | NORM_C | UV_C

TRI_LIST = 4
TRI_STRIP = 5
TRI_FAN = 6


def lod_name(name):
    for suffix in ("LOWD", "LOD1", "LOD2", "LOD3", "low1", "low2", "low3"):
        if name.endswith(suffix):
            return name[: -len(suffix)], False
    return name, True


def unpack_unorm(byte):
    return byte / 255.0


def _packed_vector(data, at):
    """A vector packed in four bytes: x, y, z from bytes 2, 1, 0, each 0..255
    mapped to -1..1."""
    packed = u32(data, at)
    u = [unpack_unorm((packed >> (8 * i)) & 255) * 2 - 1 for i in range(3)]
    return (u[2], u[1], u[0])


def _color(data, at):
    """A D3DCOLOR (stored B, G, R, A) as RGBA in 0..1."""
    b, g, r, a = data[at:at + 4]
    return (r / 255.0, g / 255.0, b / 255.0, a / 255.0)


def read_vertex(data, base, flags, box):
    """One vertex as a dict: pos, normal, uv, and when present tangent,
    binormal, color (a tint, COLOR) or lit (baked light, STATIC_LIT)."""
    p = base
    out = {"pos": None, "normal": None, "uv": None}
    if flags & POS:
        if flags & POS_C:
            x, y, z, _w = i16s(data, p, 4)
            p += 8
            low = box[0]
            mul = [box[1][i] - box[0][i] for i in range(3)]
            span = 32767 - (-32768)
            out["pos"] = tuple(low[i] + (c - (-32768)) * mul[i] / span for i, c in enumerate((x, y, z)))
        else:
            out["pos"] = f32s(data, p, 3)
            p += 12
    if flags & BONE_W:
        # A soft skin's first two bone weights (the third is what they
        # leave), as the renderer reads them.
        if not flags & BONE_C:
            out["weights"] = f32s(data, p, 2)
        p += 4 if flags & BONE_C else 8
    if flags & BONE_IDX:
        p += 4
    if flags & NORMAL:
        if flags & NORM_C:
            out["normal"] = _packed_vector(data, p)
            p += 4
        else:
            out["normal"] = f32s(data, p, 3)
            p += 12
    if flags & TANGENTS:
        if flags & NORM_C:
            out["tangent"] = _packed_vector(data, p)
            out["binormal"] = _packed_vector(data, p + 4)
            p += 8
        else:
            out["tangent"] = f32s(data, p, 3)
            out["binormal"] = f32s(data, p + 12, 3)
            p += 24
    if flags & COLOR:
        out["color"] = _color(data, p)
        p += 4
    if flags & STATIC_LIT:
        out["lit"] = _color(data, p)
        p += 4
    if flags & UV:
        if flags & UV_C:
            u, v = i16s(data, p, 2)
            out["uv"] = (u / 2048.0, v / 2048.0)
        else:
            out["uv"] = f32s(data, p, 2)
    return out


def triangles(topology, indices):
    out = []
    if topology == TRI_LIST:
        usable = len(indices) - (len(indices) % 3)
        return list(indices[:usable])
    if topology == TRI_STRIP:
        restart = []
        strip = []
        for idx in indices:
            if idx == 0xFFFF:
                restart.append(strip)
                strip = []
            else:
                strip.append(idx)
        restart.append(strip)
        for strip in restart:
            for i in range(len(strip) - 2):
                a, b, c = strip[i], strip[i + 1], strip[i + 2]
                if a == b or b == c or a == c:
                    continue
                if i & 1:
                    out.extend((a, c, b))
                else:
                    out.extend((a, b, c))
        return out
    if topology == TRI_FAN and len(indices) >= 3:
        a = indices[0]
        for i in range(1, len(indices) - 1):
            b, c = indices[i], indices[i + 1]
            if a != b and b != c and a != c and b != 0xFFFF and c != 0xFFFF:
                out.extend((a, b, c))
    return out


def model_box(data, info_body, info_end):
    size = info_end - info_body
    # swbf1 INFO is 68 bytes: 3 ints, then two vec3 bounds.
    if size < 36:
        return (0, 0, 0), (1, 1, 1)
    base = info_body + (16 if size >= 72 else 12)
    low = f32s(data, base, 3)
    high = f32s(data, base + 12, 3)
    return low, high


# Each full-detail model's bounds, by name: the INFO block's second box (min,
# max) and the SPHR bounding sphere (centre, radius), as collect_models finds
# them. The engine aims at and locks onto things by these.
BOUNDS = {}


def model_bounds(data, body, end):
    info_c = child(data, body, end, b"INFO")
    sphere_c = child(data, body, end, b"SPHR")
    out = {}
    if info_c and info_c[1] - info_c[0] >= 60:
        values = f32s(data, info_c[0] + 12, 12)
        out["box"] = [list(values[6:9]), list(values[9:12])]
    if sphere_c and sphere_c[1] - sphere_c[0] >= 16:
        out["sphere"] = list(f32s(data, sphere_c[0], 4))
    return out


def parse_skeleton(data, body, end):
    info = child(data, body, end, b"INFO")
    name_c = child(data, body, end, b"NAME")
    parent_c = child(data, body, end, b"PRNT")
    xfrm = child(data, body, end, b"XFRM")
    if info is None or name_c is None or parent_c is None or xfrm is None:
        return None
    raw = data[info[0]:info[1]]
    zero = raw.find(b"\x00")
    if zero < 0:
        return None
    name = raw[:zero].decode("latin1")
    count = struct.unpack_from("<H", raw, zero + 1)[0]

    def packed_strings(start, stop):
        out = []
        i = start
        while i < stop and len(out) < count:
            z = data.find(b"\x00", i, stop)
            if z < 0:
                break
            out.append(data[i:z].decode("latin1"))
            i = z + 1
        return out

    names = packed_strings(name_c[0], name_c[1])
    parents = packed_strings(parent_c[0], parent_c[1])
    if len(names) != count or len(parents) != count or xfrm[1] - xfrm[0] < count * 48:
        return None
    local = []
    for i in range(count):
        f = f32s(data, xfrm[0] + i * 48, 12)
        # Same glm::mat4x3 layout as world instances: column-major 3x3, then translation.
        local.append(mat4(f))

    index = {bone: i for i, bone in enumerate(names)}
    model = [None] * count

    def world(i, stack=()):
        if model[i] is not None:
            return model[i]
        parent = parents[i]
        if parent and parent in index and parent != names[i] and i not in stack:
            model[i] = mul(world(index[parent], stack + (i,)), local[i])
        else:
            model[i] = local[i]
        return model[i]

    for i in range(count):
        world(i)
    return {
        "name": name,
        "names": names,
        "parents": parents,
        "local": local,
        "model": model,
    }


def apply_bone(m, p):
    return transform_point(m, p)


def apply_bone_n(m, n):
    return transform_dir(m, n)


# MTRL flag bits (from the 2004 renderer).
LIT = 0x1  # lit by ambient, sun, and lights; unlit when clear
HARD_EDGED = 0x2  # alpha test at half
TRANSPARENT = 0x4  # alpha blended
PARAMETERS = 0x10  # two 4-byte parameters follow the flags
SPECULAR = 0x20  # the parameters are a specular exponent and colour (else scroll speeds)
STATIC_LIGHTING = 0x40  # baked vertex lighting
ADDITIVE = 0x80
DOUBLE_SIDED = 0x86  # the renderer draws these flags without back-face culling


def parse_material(data, b, e):
    flags = u32(data, b)
    material = {"flags": flags}
    if flags & PARAMETERS and e - b >= 12:
        if flags & SPECULAR:
            exponent, color = struct.unpack_from("<II", data, b + 4)
            material["specular"] = [float(exponent), ((color >> 16) & 255) / 255, ((color >> 8) & 255) / 255, (color & 255) / 255]
        else:
            material["scroll"] = list(struct.unpack_from("<ff", data, b + 4))
    return material


def _fallback_basis(normal):
    """A tangent and binormal perpendicular to a normal, for vertices whose
    stored basis is missing (about 1% of float tangents are NaN)."""
    nx, ny, nz = normal
    ax, ay, az = (0.0, 1.0, 0.0) if abs(ny) < 0.9 else (1.0, 0.0, 0.0)
    tx, ty, tz = ay * nz - az * ny, az * nx - ax * nz, ax * ny - ay * nx
    length = math.sqrt(tx * tx + ty * ty + tz * tz) or 1.0
    tx, ty, tz = tx / length, ty / length, tz / length
    return (tx, ty, tz), (ny * tz - nz * ty, nz * tx - nx * tz, nx * ty - ny * tx)


def parse_segment(data, body, end, box):
    topology = TRI_LIST
    textures = {}
    vbufs = []
    indices = []
    material = {"flags": LIT}
    render = "Normal"
    skin = None
    soft = False
    blend = None
    bmap = None
    bone = None
    for t, b, e in chunks(data, body, end):
        if t == b"INFO" and e - b >= 12:
            topology = u32(data, b)
        elif t == b"BNAM":
            bone = cstr_at(data, b, e)
        elif t == b"MTRL" and e - b >= 4:
            material = parse_material(data, b, e)
        elif t == b"RTYP":
            render = cstr_at(data, b, e)
        elif t == b"TNAM":
            if e - b >= 5:
                index = u32(data, b)
                textures[index] = cstr_at(data, b + 4, e)
        elif t == b"IBUF" and e - b >= 4:
            count = u32(data, b)
            indices = list(struct.unpack_from("<" + "H" * count, data, b + 4))
        elif t == b"VBUF" and e - b >= 12:
            count, stride, flags = struct.unpack_from("<III", data, b)
            vbufs.append((count, stride, flags, b + 12))
        elif t == b"SKIN" and e - b >= 8:
            count, kind = struct.unpack_from("<II", data, b)
            if kind == 3 and e - b >= 8 + 5 * count:
                # A soft skin (Jabba's Palace's Gamorrean guard, the capes,
                # the sarlacc tentacle): per vertex two weights out of 255,
                # the third what they leave, then three BMAP slots. `skin`
                # keeps the heaviest slot, one bone a vertex as a hard skin
                # has; `blend` keeps all three slots and weights.
                skin = []
                blend = []
                for at in range(b + 8, b + 8 + 5 * count, 5):
                    w0, w1, *slots = data[at:at + 5]
                    weights = (w0, w1, 255 - w0 - w1)
                    skin.append(slots[weights.index(max(weights))])
                    blend.append((tuple(slots), tuple(w / 255 for w in weights)))
                soft = True
            else:
                skin = list(data[b + 8:b + 8 + count])
        elif t == b"BMAP" and e - b >= 4:
            count = u32(data, b)
            bmap = list(data[b + 4:b + 4 + count])
    if not vbufs or not indices:
        return None
    # Every VBUF of a segment holds the same vertices; the uncompressed one with
    # the most fields keeps the lighting and the tangents.
    plain = [v for v in vbufs if (v[2] & COMPRESSED) == 0]
    pool = plain or vbufs
    count, stride, flags, vert_at = max(pool, key=lambda v: v[2])
    vertices = [read_vertex(data, vert_at + i * stride, flags, box) for i in range(count)]
    if any(v["pos"] is None for v in vertices):
        return None
    tris = triangles(topology, indices)
    if len(tris) < 3:
        return None
    # Drop indices that point past the vertex buffer.
    tris = [i for i in tris if i < count]
    if len(tris) < 3 or len(tris) % 3:
        tris = tris[: len(tris) - (len(tris) % 3)]
    if len(tris) < 3:
        return None
    mflags = material["flags"]
    bump = textures.get(1) if render.lower() == "bump" and flags & TANGENTS else None
    part = {
        "texture": textures.get(0, ""),
        "bump": bump,
        "render": render,
        "material": material,
        "positions": [v["pos"] for v in vertices],
        "normals": [v["normal"] or (0.0, 1.0, 0.0) for v in vertices],
        "uvs": [v["uv"] or (0.0, 0.0) for v in vertices],
        "indices": tris,
        "additive": bool(mflags & ADDITIVE),
        "transparent": bool(mflags & TRANSPARENT),
        "skin": skin,
        "softSkin": soft,
        "bmap": bmap,
        # The skeleton node a rigid segment hangs from (BNAM).
        "bone": bone,
    }
    if blend is not None and len(blend) == count:
        # The weights the renderer blends by are the vertex buffer's floats;
        # SKIN's bytes are the same weights rounded to 1/255.
        exact = [v.get("weights") for v in vertices]
        if all(w is not None for w in exact):
            blend = [(slots, (w[0], w[1], max(0.0, 1.0 - w[0] - w[1]))) for (slots, _), w in zip(blend, exact)]
        part["blend"] = blend
    # Vertex colour: a tint (COLOR) or baked light (STATIC_LIT); never both.
    if flags & STATIC_LIT:
        part["colors"] = [v["lit"] for v in vertices]
        part["colorKind"] = "lit"
    elif flags & COLOR:
        part["colors"] = [v["color"] for v in vertices]
        part["colorKind"] = "tint"
    if bump:
        tangents = []
        binormals = []
        for v in vertices:
            t, b = v.get("tangent"), v.get("binormal")
            if t is None or b is None or any(x != x for x in t + b):
                t, b = _fallback_basis(v["normal"] or (0.0, 1.0, 0.0))
            tangents.append(t)
            binormals.append(b)
        part["tangents"] = tangents
        part["binormals"] = binormals
    return part


def pose_part(part, skeleton):
    skin = part.get("skin")
    bmap = part.get("bmap")
    mats = skeleton["model"] if skeleton else None
    if not mats or not skin or not bmap:
        return
    positions = []
    normals = []
    joints = []
    for i, (pos, nrm) in enumerate(zip(part["positions"], part["normals"])):
        bone_i = bmap[skin[i]] if i < len(skin) and skin[i] < len(bmap) else 0
        if bone_i >= len(mats):
            bone_i = 0
        joints.append(bone_i)
        positions.append(apply_bone(mats[bone_i], pos))
        normals.append(apply_bone_n(mats[bone_i], nrm))
    part["positions"] = positions
    part["normals"] = normals
    part["joints"] = joints
    for key in ("tangents", "binormals"):
        if part.get(key):
            part[key] = [apply_bone_n(mats[j], v) for j, v in zip(joints, part[key])]


def pose_rigid(part, skeleton):
    """A rigid segment's vertices are in the space of the node it hangs from
    (BNAM); carry them into the model's space through that node."""
    name = part.get("bone")
    if not skeleton or not name or name not in skeleton["names"]:
        return
    m = skeleton["model"][skeleton["names"].index(name)]
    if all(abs(m[i] - (1.0 if i in (0, 5, 10, 15) else 0.0)) < 1e-6 for i in range(16)):
        return
    part["positions"] = [apply_bone(m, p) for p in part["positions"]]
    part["normals"] = [apply_bone_n(m, n) for n in part["normals"]]
    for key in ("tangents", "binormals"):
        if part.get(key):
            part[key] = [apply_bone_n(m, v) for v in part[key]]


def _inside(points, box, slack=0.05):
    """How many points lie within a box (min, max), give or take a little."""
    (lx, ly, lz), (hx, hy, hz) = box
    return sum(
        lx - slack <= x <= hx + slack and ly - slack <= y <= hy + slack and lz - slack <= z <= hz + slack
        for x, y, z in points
    )


def parse_model(data, body, end, skeletons, far=False):
    """A model's parts, named without any LOD suffix. Only full-detail models
    are read, or with far set, only the low-detail (LOWD) ones the far scene
    draws."""
    name_c = child(data, body, end, b"NAME")
    info_c = child(data, body, end, b"INFO")
    if name_c is None or info_c is None:
        return None
    raw_name = cstr_at(data, name_c[0], name_c[1])
    name, full = lod_name(raw_name)
    if not (raw_name.endswith("LOWD") if far else full):
        return None
    box = model_box(data, info_c[0], info_c[1])
    bounds = model_bounds(data, body, end)
    if full and not far:
        BOUNDS.setdefault(name, bounds)
    bones = skeletons.get(name) or skeletons.get(raw_name)
    parts = []
    for b, e in children_of(data, body, end, b"segm"):
        part = parse_segment(data, b, e, box)
        if part and part["texture"] and not part["texture"].startswith("p_"):
            part["local_positions"] = list(part["positions"])
            part["local_normals"] = list(part["normals"])
            if part.get("softSkin"):
                # A soft skin's vertices are in the model's space: each
                # follows its bone where it is (joints, the heaviest), or
                # its three bones by their weights (skinJoints and
                # skinWeights, three a vertex).
                if bones and part.get("bmap"):
                    bmap = part["bmap"]
                    joint = lambda k: bmap[k] if k < len(bmap) and bmap[k] < len(bones["model"]) else 0
                    part["joints"] = [joint(k) for k in part["skin"]]
                    if part.get("blend"):
                        part["skinJoints"] = [joint(k) for slots, _ in part["blend"] for k in slots]
                        part["skinWeights"] = [w for _, weights in part["blend"] for w in weights]
            elif part.get("skin"):
                # Skinned vertices are usually in their bones' spaces, but
                # some models store them already placed: keep whichever lies
                # in the model's placed box (INFO's second box).
                trial = dict(part)
                pose_part(trial, bones)
                placed = bounds.get("box")
                if trial.get("joints") and (not placed or _inside(trial["positions"], placed) >= _inside(part["positions"], placed)):
                    part.update(trial)
            else:
                pose_rigid(part, bones)
            parts.append(part)
    if not parts:
        return None
    return name, parts


def collect_models(data, models, skeletons_out=None, far=None):
    """Every model in an archive by name; the first one found wins. With far,
    the low-detail (LOWD) models go there, under their full model's name."""
    for start, end in iter_archives(data):
        skeletons = {}
        nodes = chunks(data, start, end)
        for t, b, e in nodes:
            if t != b"skel":
                continue
            parsed = parse_skeleton(data, b, e)
            if parsed is not None:
                skeletons[parsed["name"]] = parsed
                if skeletons_out is not None:
                    skeletons_out.setdefault(parsed["name"], parsed)
        for t, b, e in nodes:
            if t != b"modl":
                continue
            parsed = parse_model(data, b, e, skeletons)
            if parsed is not None:
                name, parts = parsed
                models.setdefault(name, parts)
            elif far is not None:
                parsed = parse_model(data, b, e, skeletons, far=True)
                if parsed is not None:
                    name, parts = parsed
                    far.setdefault(name, parts)
