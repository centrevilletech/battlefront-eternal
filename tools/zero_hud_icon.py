"""Project small, stationary HUD models onto transparent equipment and marker icons."""
import math
import struct

from portable_math import cos, sin
from zero_hud_chunks2 import ConfigError, checked_chunks
from zero_model import model_box, parse_segment


def parts(data, fields, budget):
    bounds = next(((b, e) for tag, b, e in fields if tag == b"INFO"), None)
    if bounds is None:
        return []
    if bounds[1] - bounds[0] < 28:
        raise ConfigError("short icon bounds")
    box = model_box(data, *bounds)
    output = []
    for tag, start, end in fields:
        if tag != b"segm":
            continue
        if len(output) >= 64:
            raise ConfigError("too many icon parts")
        for prop, b, e in checked_chunks(data, start, end, budget):
            if prop in (b"IBUF", b"VBUF"):
                if e - b < 4:
                    raise ConfigError("short icon buffer")
                count = struct.unpack_from("<I", data, b)[0]
                if prop == b"IBUF":
                    if count > 32768 or e - b - 4 < 2 * count:
                        raise ConfigError("oversized icon index buffer")
                else:
                    if e - b < 12:
                        raise ConfigError("short icon vertex header")
                    stride = struct.unpack_from("<I", data, b + 4)[0]
                    if count > 8192 or not 8 <= stride <= 256 or e - b - 12 < count * stride:
                        raise ConfigError("oversized icon vertex buffer")
        part = parse_segment(data, start, end, box)
        if part:
            output.append(part)
    return output


def extent(parts, rotation=None):
    """The model's extent on the two axes its icon shows, as [left, bottom, right, top] in model
    units (the icon's vertical axis points up), matching raster() or raster_oriented()."""
    if rotation is None:
        points = [p for part in parts for p in part["positions"]]
    else:
        terms = _rotation_terms(rotation)
        points = [_rotate_xyz(p, terms) for part in parts for p in part["positions"]]
    if not points or any(len(p) != 3 or any(not math.isfinite(c) for c in p) for p in points):
        return None
    low = list(map(min, zip(*points)))
    high = list(map(max, zip(*points)))
    if rotation is None:
        spans = [hi - lo for hi, lo in zip(high, low)]
        x, y = sorted(sorted(range(3), key=lambda a: -spans[a])[:2])
    else:
        x, y = 0, 1
    return [low[x], low[y], high[x], high[y]]


def raster(parts, texture, limit=128):
    """Preserve the visible plane, silhouette, diffuse sample and vertex alpha."""
    points = [p for part in parts for p in part["positions"]]
    if not points:
        return None
    if any(len(p) != 3 or any(not math.isfinite(c) for c in p) for p in points):
        raise ConfigError("invalid icon vertex")
    low = list(map(min, zip(*points)))
    spans = [hi - lo for hi, lo in zip(map(max, zip(*points)), low)]
    axes = sorted(sorted(range(3), key=lambda a: -spans[a])[:2])
    x, y = axes
    if min(spans[x], spans[y]) <= 0:
        return None
    scale = (limit - 2) / max(spans[x], spans[y])
    width, height = [max(2, math.ceil(spans[a] * scale) + 2) for a in axes]
    out = bytearray(4 * width * height)
    remaining = 4 << 20
    for part in parts:
        xy = [(1 + (p[x] - low[x]) * scale, height - 1 - (p[y] - low[y]) * scale) for p in part["positions"]]
        uv = [v if all(math.isfinite(n) for n in v) else (0., 0.) for v in part["uvs"]]
        colors = part.get("colors")
        tex = texture(part["texture"].lower())
        ids = part["indices"]
        for a, b, c in zip(ids[::3], ids[1::3], ids[2::3]):
            (ax, ay), (bx, by), (cx, cy) = xy[a], xy[b], xy[c]
            det = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
            if abs(det) < 1e-9:
                continue
            xx = range(max(0, int(min(ax, bx, cx))), min(width, math.ceil(max(ax, bx, cx)) + 1))
            yy = range(max(0, int(min(ay, by, cy))), min(height, math.ceil(max(ay, by, cy)) + 1))
            remaining -= len(xx) * len(yy)
            if remaining < 0:
                raise ConfigError("excessive icon coverage")
            for j in yy:
                for i in xx:
                    u = ((by-cy)*(i+.5-cx)+(cx-bx)*(j+.5-cy))/det
                    v = ((cy-ay)*(i+.5-cx)+(ax-cx)*(j+.5-cy))/det
                    w = 1-u-v
                    if min(u,v,w) < -1e-8:
                        continue
                    rgba = (255,255,255,255)
                    if tex:
                        tw, th, pixels = tex
                        ix = int((uv[a][0]*u+uv[b][0]*v+uv[c][0]*w)*tw) % tw
                        iy = int((uv[a][1]*u+uv[b][1]*v+uv[c][1]*w)*th) % th
                        k = (iy*tw+ix)*4
                        rgba = pixels[k:k+4]
                    if colors:
                        rgba = bytes(max(0,min(255,round(channel*(colors[a][k]*u+colors[b][k]*v+colors[c][k]*w)))) for k,channel in enumerate(rgba))
                    offset = 4*(width*j+i)
                    if rgba[3] >= out[offset+3]:
                        out[offset:offset+4] = bytes(rgba)
    return width,height,out


def _rotation_terms(degrees):
    rx, ry, rz = (math.radians(a) for a in degrees)
    return (cos(rx), sin(rx), cos(ry), sin(ry), cos(rz), sin(rz))


def _rotate_xyz(point, terms):
    x, y, z = point
    cx, sx, cy, sy, cz, sz = terms
    y, z = y * cx - z * sx, y * sx + z * cx
    x, z = x * cy + z * sy, -x * sy + z * cy
    x, y = x * cz - y * sz, x * sz + y * cz
    return (x, y, z)


def raster_oriented(parts, texture, rotation, limit=128):
    """Project an opaque 3D weapon model at its authored 1playerhud rotation with Z-buffering."""
    terms = _rotation_terms(rotation)
    transformed = []
    points = []
    for part in parts:
        raw = part["positions"]
        if any(len(p) != 3 or any(not math.isfinite(c) for c in p) for p in raw):
            raise ConfigError("invalid icon vertex")
        rot = [_rotate_xyz(p, terms) for p in raw]
        transformed.append((part, rot))
        points.extend(rot)
    if not points:
        return None
    low = [min(p[i] for p in points) for i in range(3)]
    spans = [max(p[i] for p in points) - low[i] for i in range(3)]
    if min(spans[0], spans[1]) <= 0:
        return None
    scale = (limit - 2) / max(spans[0], spans[1])
    width = max(2, math.ceil(spans[0] * scale) + 2)
    height = max(2, math.ceil(spans[1] * scale) + 2)
    out = bytearray(4 * width * height)
    zbuf = [-math.inf] * (width * height)
    remaining = 4 << 20
    for part, rot in transformed:
        xy = [(1 + (p[0] - low[0]) * scale, height - 1 - (p[1] - low[1]) * scale, p[2]) for p in rot]
        uv = [v if all(math.isfinite(n) for n in v) else (0., 0.) for v in part["uvs"]]
        tex = texture(part["texture"].lower())
        ids = part["indices"]
        for a, b, c in zip(ids[::3], ids[1::3], ids[2::3]):
            (ax, ay, az), (bx, by, bz), (cx, cy, cz) = xy[a], xy[b], xy[c]
            det = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
            if abs(det) < 1e-9:
                continue
            p0, p1, p2 = rot[a], rot[b], rot[c]
            ux, uy, uz = p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]
            vx, vy, vz = p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]
            nx, ny, nz = uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx
            nl = math.sqrt(nx * nx + ny * ny + nz * nz) or 1.0
            shade = 0.88 + 0.52 * abs((-0.25 * nx + 0.45 * ny + 0.85 * nz) / nl)
            xx = range(max(0, int(min(ax, bx, cx))), min(width, math.ceil(max(ax, bx, cx)) + 1))
            yy = range(max(0, int(min(ay, by, cy))), min(height, math.ceil(max(ay, by, cy)) + 1))
            remaining -= len(xx) * len(yy)
            if remaining < 0:
                raise ConfigError("excessive icon coverage")
            for j in yy:
                for i in xx:
                    u = ((by - cy) * (i + .5 - cx) + (cx - bx) * (j + .5 - cy)) / det
                    v = ((cy - ay) * (i + .5 - cx) + (ax - cx) * (j + .5 - cy)) / det
                    w = 1.0 - u - v
                    if min(u, v, w) < -1e-8:
                        continue
                    z = az * u + bz * v + cz * w
                    idx = width * j + i
                    if z <= zbuf[idx]:
                        continue
                    zbuf[idx] = z
                    rgb = (220, 220, 220)
                    if tex:
                        tw, th, pixels = tex
                        ix = int((uv[a][0] * u + uv[b][0] * v + uv[c][0] * w) * tw) % tw
                        iy = int((uv[a][1] * u + uv[b][1] * v + uv[c][1] * w) * th) % th
                        k = (iy * tw + ix) * 4
                        rgb = (pixels[k], pixels[k + 1], pixels[k + 2])
                    offset = 4 * idx
                    out[offset] = min(255, int(rgb[0] * shade))
                    out[offset + 1] = min(255, int(rgb[1] * shade))
                    out[offset + 2] = min(255, int(rgb[2] * shade))
                    out[offset + 3] = 255
    return width, height, out
