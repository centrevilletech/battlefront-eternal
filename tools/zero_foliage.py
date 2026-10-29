"""Painted foliage placement from the PC engine's FOLG/prp_ rules.

Only format/behaviour is implemented here. The placement random table is read
from the player's executable, never distributed with the engine.
"""
import hashlib
import itertools
import math
import struct
from functools import lru_cache
from pathlib import Path

from portable_math import cos, sin
from zero_config import configs
from zero_hash import fnv1a
from zero_terrain import height_lookup
from xform import mat4


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def key(name):
    return '%08x' % fnv1a(name)


def name_hash(value):
    return struct.unpack('<I', struct.pack('<f', value))[0] if isinstance(value, float) else fnv1a(value.lower())


def foliage_layers(data, classes, models=()):
    """Read top-level layers, retaining weighted alternatives and File meshes.

    File in a grass/leaf Mesh is an editor stand-in, not another placement.
    Nested Layer sections (stock geo1) are ignored by the native loader.
    """
    names = {fnv1a(n.lower()): n for n in (*classes, *models)}
    result = {}
    for config in configs(data, b'prp_'):
        for tag, values, children in config:
            if tag != key('Layer') or not values or not 0 <= int(values[0]) < 4:
                continue
            spread = next((v[0] for k, v, _ in children if k == key('SpreadFactor') and v), 1)
            spacing = max(1, int(spread))
            count = int(f32(1 / spread)) if 0 < spread < 1 else 1
            meshes = []
            for tag, _, fields in children:
                if tag != key('Mesh'):
                    continue
                entries = {k: v for k, v, _ in fields}
                kind = next((k for k in ('grasspatch', 'leafpatch') if entries.get(key(k))), 'File')
                ref = entries.get(key(kind))
                if not ref:
                    continue
                name = names.get(name_hash(ref[0]))
                # An unresolved alternative still occupies its frequency interval.
                scalar = lambda k, default: (entries.get(key(k)) or [default])[0]
                meshes.append(dict(name=name, kind=kind,
                    distance=min((ref[1] if len(ref) > 1 else 1000) * (1 if kind == 'File' else 2), 1000),
                    frequency=max(0, scalar('Frequency', 1)), scale=scalar('Scale', 1),
                    stiffness=scalar('Stiffness', 0), variation=scalar('ColorVariation', 0),
                    collision=key('UseCollision') in entries))
            total = sum(m['frequency'] for m in meshes)
            cumulative = 0
            for mesh in meshes:
                cumulative += mesh['frequency'] / total if total else 0
                mesh['cumulative'] = cumulative
            result[int(values[0])] = dict(spacing=spacing, count=count, meshes=meshes)
    return result


# The Steam and GOG builds keep the 256-float foliage random table at different
# places. The importer reads it from the player's own executable and accepts it
# only if the SHA-256 of its 1024 bytes matches.
TABLE_RVA = 0x4231d0
TABLE_SHA256 = '07289bf61525eeedec945b3b2dfce47d181e2d6b7147b15e65683b9d24013ac6'
EXECUTABLE_SECTION = 0x20000000


@lru_cache(maxsize=4)
def placement_table(executable):
    """The PC foliage random table, addressed by RVA through the PE section
    table: at the Steam build's RVA, else at any aligned place in the data
    sections that holds the same bytes. Any other table is refused, since it
    would plant the foliage (and its collision) elsewhere than the game does."""
    data = Path(executable).read_bytes()
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    if data[:2] != b'MZ' or data[pe:pe + 4] != b'PE\0\0':
        raise ValueError('Foliage placement needs a supported Battlefront.exe')
    count = struct.unpack_from('<H', data, pe + 6)[0]
    optional = struct.unpack_from('<H', data, pe + 20)[0]
    sections = []
    for i in range(count):
        at = pe + 24 + optional + i * 40
        _, va, size, offset = struct.unpack_from('<IIII', data, at + 8)
        flags = struct.unpack_from('<I', data, at + 36)[0]
        sections.append((va, size, offset, flags))
    known = [offset + TABLE_RVA - va for va, size, offset, _ in sections if va <= TABLE_RVA and TABLE_RVA + 1024 <= va + size]
    elsewhere = (at for _, size, offset, flags in sections if not flags & EXECUTABLE_SECTION
                 for at in range(offset, offset + size - 1023, 4))
    for at in itertools.chain(known, elsewhere):
        block = data[at:at + 1024]
        if len(block) == 1024 and hashlib.sha256(block).hexdigest() == TABLE_SHA256:
            return struct.unpack('<256f', block)
    raise ValueError('Unsupported Battlefront.exe foliage random table; refusing incorrect placement')


class TableRandom:
    def __init__(self, table, seed):
        self.table, self.index = table, seed

    def next(self):
        self.index += 1
        return self.table[self.index & 255]


def placements(painted, layers, table):
    """Replay the per-block, per-layer random stream in native cell order."""
    cells = {(i, j): bits for i, j, bits in painted['cells']}
    blocks = sorted({(j // 2, i // 2) for i, j in cells})
    half = painted['side'] // 2
    for row, col in blocks:
        for layer, spec in sorted(layers.items()):
            rand = TableRandom(table, row * half + col + 2 * row + layer)
            s = spec['spacing']
            for j in (2 * row, 2 * row + 1):
                for i in (2 * col, 2 * col + 1):
                    if not cells.get((i, j), 0) & (1 << layer) or i % s or j % s:
                        continue
                    for _ in range(spec['count']):
                        pick = rand.next()
                        mesh = next((m for m in spec['meshes'] if pick <= m['cumulative']), None)
                        if mesh is None:
                            continue
                        x = ((i - half) + .5 - .45 * s + .9 * s * rand.next()) * 16
                        z = -((j - half) + .5 - .45 * s + .9 * s * rand.next()) * 16
                        yaw = rand.next() * 6.1415
                        if mesh['name']:
                            yield mesh, x, z, yaw


def normalize(v):
    length = math.sqrt(sum(x*x for x in v)) or 1
    return [x / length for x in v]


def ground_frame(height, x, z, stiffness, model=False, yaw=0, scale=1):
    """Cards use the native three-probe support plane, not the cell centre."""
    a, b, c = height(x, z + 4), height(x - 3, z - 3), height(x + 3, z - 3)
    center = height(x, z)
    if center is None or any(y is None for y in (a, b, c)):
        return None
    y = center if model else (a + b + c) / 3
    up = normalize([-(c - b) * 7, 42, -6 * (a - (b + c) / 2)])
    up = normalize([up[0] * (1 - stiffness), up[1] * (1 - stiffness) + stiffness, up[2] * (1 - stiffness)])
    right = normalize([up[1], -up[0], 0])
    forward = [right[1]*up[2], -right[0]*up[2], right[0]*up[1]-right[1]*up[0]]
    if model:
        sn, co = sin(yaw), cos(yaw)
        right, forward = ([scale*(co*r-sn*f) for r, f in zip(right, forward)],
                          [scale*(sn*r+co*f) for r, f in zip(right, forward)])
        up = [scale*v for v in up]
    return [*right, *up, *forward, x, y, z]


def foliage_tint(terrain, x, z, variation):
    grid, data = terrain['grid'], terrain.get('lumi')
    light = 1.0
    if data:
        size, unit = grid['size'], grid['unit']
        ox, oz = grid['origin']
        fx = min(max((x - ox) / unit, 0), size - 1)
        fz = min(max((z - oz) / unit, 0), size - 1)
        i, j = int(fx), int(fz)
        tx, tz = fx - i, fz - j
        at = lambda a, b: data[min(a, size - 1) * size + min(b, size - 1)] / 255
        light = ((1-tx)*at(i, j)+tx*at(i+1, j))*(1-tz) + ((1-tx)*at(i, j+1)+tx*at(i+1, j+1))*tz
    brightness = min(.7 * light + .6, 1)
    seed = struct.unpack('<I', struct.pack('<f', x*z))[0] ^ 0xc71c2830
    def rand():
        nonlocal seed
        first = (seed * 1664525 + 1013904223) & 0xffffffff
        seed = (first * 1664525 + 1013904223) & 0xffffffff
        return (((seed & 0xffff0000) | (first >> 16)) >> 9) / 8388608
    rand()
    return list(reversed([(1 - rand()*variation)*brightness for _ in range(3)]))


def terrain_foliage(data, terrain, classes, water=None, *, models=(), table=None, executable=None):
    # Water does not suppress the native painted layer: banks/reeds can extend
    # below its mean plane. WATR and FOLG deliberately have different grids.
    result = {'foliage': [], 'instances': [], 'collision': []}
    painted = terrain and terrain.get('foliage')
    if not painted or not painted['cells']:
        return result
    layers = foliage_layers(data, classes, models)
    if not layers:
        return result
    if table is None:
        table = placement_table(executable)
    height = height_lookup(terrain)
    extent = terrain['grid']['size'] * terrain['grid']['unit'] / 2
    for mesh, x, z, yaw in placements(painted, layers, table):
        # FOLG has a fixed 8 km extent; many retail maps retain template paint
        # outside their smaller height field. It must not be clamped to its edge.
        if not (-extent <= x < extent and -extent <= z < extent):
            continue
        model = mesh['kind'] == 'File'
        raw = ground_frame(height, x, z, mesh['stiffness'], model, yaw, mesh['scale'])
        if raw is None:
            continue
        if model:
            instance = (mesh['name'], raw)
            result['instances'].append(instance)
            if mesh['collision']:
                result['collision'].append(instance)
        else:
            result['foliage'].append({'class': mesh['name'], 'matrix': mat4(raw),
                'terrain': True, 'distance': mesh['distance'],
                'tint': foliage_tint(terrain, x, z, mesh['variation'])})
    return result
