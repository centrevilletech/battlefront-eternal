"""Readers for the ucfb chunk trees in the PC .lvl archives.

A chunk is a 4-byte tag, a little-endian size, and a body padded to 4 bytes.
"""

import struct


def align4(n):
    return (n + 3) & ~3


def chunks(data, start, end):
    pos = start
    out = []
    while pos + 8 <= end:
        if data[pos:pos + 4] == b"\x00\x00\x00\x00":
            pos += 4
            continue
        typ = data[pos:pos + 4]
        if not all(32 <= b < 127 for b in typ):
            break
        size = struct.unpack_from("<I", data, pos + 4)[0]
        body = pos + 8
        raw_end = body + size
        if size < 0 or raw_end > end:
            break
        out.append((typ, body, raw_end))
        pos = align4(raw_end)
    return out


def cstr_at(data, start, end):
    raw = data[start:end].split(b"\x00", 1)[0]
    return raw.decode("latin1")


def child(data, body, end, typ):
    for t, b, e in chunks(data, body, end):
        if t == typ:
            return b, e
    return None


def children_of(data, body, end, typ):
    return [(b, e) for t, b, e in chunks(data, body, end) if t == typ]


def u32(data, off):
    return struct.unpack_from("<I", data, off)[0]


def i16s(data, off, n):
    return struct.unpack_from("<" + "h" * n, data, off)


def f32s(data, off, n):
    return struct.unpack_from("<" + "f" * n, data, off)


def cstrings(data, start, end, limit):
    out = []
    i = start
    while i < end and len(out) < limit:
        z = data.find(b"\x00", i, end)
        if z < 0:
            break
        text = data[i:z].decode("latin1")
        if text:
            out.append(text)
        i = z + 1
    return out


def iter_archives(data):
    """Yield (body, end) ranges that directly hold tex_/modl/wrld/tern nodes."""
    top = chunks(data, 0, len(data))
    if not top or top[0][0] != b"ucfb":
        return
    root_b, root_e = top[0][1], top[0][2]
    kids = chunks(data, root_b, root_e)
    if kids and kids[0][0] == b"lvl_":
        for t, b, e in kids:
            if t == b"lvl_" and e - b > 16:
                yield b + 8, e
    else:
        yield root_b, root_e


def nodes(data, typ):
    """Every top-level node of one type across the archive's lvl_ sections."""
    for start, end in iter_archives(data):
        for t, b, e in chunks(data, start, end):
            if t == typ:
                yield b, e


def node_name(data, body, end):
    name = child(data, body, end, b"NAME")
    return cstr_at(data, name[0], name[1]) if name else None
