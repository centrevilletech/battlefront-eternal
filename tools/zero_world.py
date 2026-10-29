"""Collision, spawn paths, and regions from a world archive.

coll: NAME, NODE, INFO (vertex count, three counts, bounds), POSI (float3 per
vertex), TREE (an AABB tree written depth first; each LEAF is one polygon: a
vertex count, a 6-byte quantized box, then u16 vertex indices).

prim: INFO (model name, shape count), then NAME, PRNT, XFRM, DATA per shape.
DATA is the shape type and three floats: 4 is a box (half extents), 2 is a
cylinder along Y (radius, full height), 0 or 1 is a sphere (radius).

The engine resolves a soldier's contacts one polygon at a time, so collision
is written as polygons: triangles plus the polygon each one came from.
"""

import math
import struct

from portable_math import atan2, cos, sin
from ucfb import child, children_of, chunks, cstr_at, f32s, nodes, u32, u32
from xform import mat4, transform_point

BOX = 4
CYLINDER = 2
SPHERES = (0, 1)


def coll_meshes(data):
    out = {}
    for body, end in nodes(data, b"coll"):
        name_c = child(data, body, end, b"NAME")
        info = child(data, body, end, b"INFO")
        posi = child(data, body, end, b"POSI")
        tree = child(data, body, end, b"TREE")
        if not (name_c and info and posi and tree):
            continue
        count = u32(data, info[0])
        if posi[1] - posi[0] < count * 12:
            continue
        positions = [f32s(data, posi[0] + i * 12, 3) for i in range(count)]
        polys = []
        for t, b, e in chunks(data, tree[0], tree[1]):
            if t != b"LEAF" or e - b < 7:
                continue
            n = data[b]
            if n < 3 or e - b < 7 + n * 2:
                continue
            idx = struct.unpack_from("<%dH" % n, data, b + 7)
            if max(idx) >= count:
                continue
            polys.append(idx)
        out.setdefault(cstr_at(data, *name_c), []).append((positions, polys))
    return out


def prim_shapes(data):
    out = {}
    for body, end in nodes(data, b"prim"):
        info = child(data, body, end, b"INFO")
        if info is None:
            continue
        shapes = []
        cur = {}
        for t, b, e in chunks(data, body, end):
            if t == b"NAME":
                cur = {"name": cstr_at(data, b, e)}
            elif t == b"PRNT":
                cur["parent"] = cstr_at(data, b, e)
            elif t == b"XFRM" and e - b >= 48:
                cur["matrix"] = mat4(f32s(data, b, 12))
            elif t == b"DATA" and e - b >= 16 and "matrix" in cur:
                kind = u32(data, b)
                cur["kind"] = kind
                cur["size"] = f32s(data, b + 4, 3)
                shapes.append(cur)
                cur = {}
        out[cstr_at(data, *info)] = shapes
    return out


def shape_polygons(kind, size, segments=16):
    """Local-space polygons (lists of points) for one primitive."""
    polys = []
    if kind == BOX:
        hx, hy, hz = size
        c = [(sx * hx, sy * hy, sz * hz) for sx in (-1, 1) for sy in (-1, 1) for sz in (-1, 1)]
        for face in ((0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)):
            polys.append([c[i] for i in face])
    elif kind == CYLINDER:
        r, h = size[0], size[1] / 2
        ring = [(r * cos(2 * math.pi * i / segments), r * sin(2 * math.pi * i / segments)) for i in range(segments)]
        for i in range(segments):
            (x0, z0), (x1, z1) = ring[i], ring[(i + 1) % segments]
            polys.append([(x0, -h, z0), (x1, -h, z1), (x1, h, z1), (x0, h, z0)])
        polys.append([(x, h, z) for x, z in ring])
        polys.append([(x, -h, z) for x, z in reversed(ring)])
    elif kind in SPHERES:
        r = size[0]
        rows, cols = 6, 12
        pts = [[tuple(round(v, 6) + 0.0 for v in (
                    r * sin(math.pi * i / rows) * cos(2 * math.pi * j / cols),
                    r * cos(math.pi * i / rows),
                    r * sin(math.pi * i / rows) * sin(2 * math.pi * j / cols))) for j in range(cols)]
               for i in range(rows + 1)]
        for i in range(rows):
            for j in range(cols):
                a, b = pts[i][j], pts[i][(j + 1) % cols]
                c, d = pts[i + 1][j], pts[i + 1][(j + 1) % cols]
                # The pole rows collapse to triangles.
                quad = [a, c, d, b]
                polys.append([q for k, q in enumerate(quad) if q != quad[k - 1]])
    return polys


def spawn_paths(data):
    """Path points are a position and a quaternion stored w, x, y, z."""
    out = {}
    for body, end in nodes(data, b"PATH"):
        for b, e in children_of(data, body, end, b"path"):
            name_c = child(data, b, e, b"NAME")
            pnts = child(data, b, e, b"PNTS")
            if name_c is None or pnts is None:
                continue
            points = []
            for i in range((pnts[1] - pnts[0]) // 28):
                x, y, z, qw, _qx, qy, _qz = f32s(data, pnts[0] + i * 28, 7)
                points.append([x, y, z, 2 * atan2(qy, qw)])
            out[cstr_at(data, *name_c)] = points
    return out


def regions(data, body, end):
    out = []
    for b, e in children_of(data, body, end, b"regn"):
        info = child(data, b, e, b"INFO")
        if info is None:
            continue
        sub = {t: (x, y) for t, x, y in chunks(data, info[0], info[1])}
        if not all(k in sub for k in (b"TYPE", b"NAME", b"XFRM", b"SIZE")):
            continue
        out.append({
            "type": cstr_at(data, *sub[b"TYPE"]),
            "name": cstr_at(data, *sub[b"NAME"]),
            "matrix": mat4(f32s(data, sub[b"XFRM"][0], 12)),
            "size": list(f32s(data, sub[b"SIZE"][0], 3)),
        })
    return out


def triangulate_polygon(positions, poly):
    """Ear-clip concave collision faces without filling doorways/notches.

    Convex faces keep their original fan order. Project along the dominant
    Newell normal, preserving the source winding and original vertex indices.
    """
    if len(poly) <= 3:
        return [tuple(poly)] if len(poly) == 3 else []
    points = [positions[i] for i in poly]
    normal = [sum((a[(d + 1) % 3] - b[(d + 1) % 3]) *
                  (a[(d + 2) % 3] + b[(d + 2) % 3])
                  for a, b in zip(points, points[1:] + points[:1])) for d in range(3)]
    axis = max(range(3), key=lambda d: abs(normal[d]))
    xy = [tuple(p[d] for d in range(3) if d != axis) for p in points]
    def cross(a, b, c):
        return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    area = sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(xy, xy[1:] + xy[:1]))
    sign = 1 if area >= 0 else -1
    if all(sign * cross(xy[i - 1], xy[i], xy[(i + 1) % len(xy)]) >= -1e-8 for i in range(len(xy))):
        return [(poly[0], poly[i], poly[i + 1]) for i in range(1, len(poly) - 1)]
    remaining = list(range(len(poly)))
    triangles = []
    while len(remaining) > 3:
        for j, b in enumerate(remaining):
            a, c = remaining[j - 1], remaining[(j + 1) % len(remaining)]
            if sign * cross(xy[a], xy[b], xy[c]) <= 1e-8:
                continue
            if any(all(sign * cross(xy[u], xy[v], xy[p]) >= -1e-8
                       for u, v in ((a, b), (b, c), (c, a)))
                   for p in remaining if p not in (a, b, c)):
                continue
            triangles.append((poly[a], poly[b], poly[c]))
            remaining.pop(j)
            break
        else:
            raise ValueError("Collision polygon cannot be triangulated (degenerate or self-intersecting)")
    triangles.append(tuple(poly[i] for i in remaining))
    return triangles


def place_mesh(positions, polys, matrix, out):
    """Append polygons (index lists into positions) as triangles. `out` holds
    the arrays pos, idx, and poly (each triangle's polygon number)."""
    base = len(out["pos"]) // 3
    for p in positions:
        out["pos"].extend(transform_point(matrix, p))
    for poly in polys:
        number = out["count"]
        out["count"] += 1
        for tri in triangulate_polygon(positions, poly):
            out["idx"].extend(base + i for i in tri)
            out["poly"].append(number)


def place_shape(kind, size, matrix, out):
    polys = shape_polygons(kind, size)
    points = [p for poly in polys for p in poly]
    numbered = []
    at = 0
    for poly in polys:
        numbered.append(list(range(at, at + len(poly))))
        at += len(poly)
    place_mesh(points, numbered, matrix, out)


def parse_instances(data, body, end):
    found = []
    for b, e in children_of(data, body, end, b"inst"):
        info = child(data, b, e, b"INFO")
        if info is None:
            continue
        type_c = child(data, info[0], info[1], b"TYPE")
        xfrm = child(data, info[0], info[1], b"XFRM")
        if type_c is None or xfrm is None or xfrm[1] - xfrm[0] < 48:
            continue
        kind = cstr_at(data, type_c[0], type_c[1])
        m = f32s(data, xfrm[0], 12)
        found.append((kind, m))
    return found


def instance_records(data, body, end, name_of=lambda key: "%08x" % key):
    """Placed objects with their instance properties: class, instance name,
    the raw XFRM floats, and (property name, value) pairs in file order."""
    found = []
    for b, e in children_of(data, body, end, b"inst"):
        info = child(data, b, e, b"INFO")
        if info is None:
            continue
        type_c = child(data, info[0], info[1], b"TYPE")
        name_c = child(data, info[0], info[1], b"NAME")
        xfrm = child(data, info[0], info[1], b"XFRM")
        if type_c is None or xfrm is None or xfrm[1] - xfrm[0] < 48:
            continue
        props = []
        for tag, pb, pe in chunks(data, b, e):
            if tag == b"PROP" and pe - pb >= 4:
                props.append((name_of(u32(data, pb)), cstr_at(data, pb + 4, pe)))
        found.append({
            "class": cstr_at(data, *type_c),
            "name": cstr_at(data, *name_c) if name_c else "",
            "xfrm": f32s(data, xfrm[0], 12),
            "props": props,
        })
    return found
