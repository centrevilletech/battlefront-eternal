"""Texture chunks (`tex_`): decoding to RGBA and writing PNGs.

An archive's textures are collected without decoding them (collect_textures
gives Image objects that decode when first read), since an extraction
writes a fraction of what its archives hold. The decoders use numpy where
it is installed (the desktop app's toolchain has it) and plain Python
loops otherwise, to the same bytes.
"""

import struct
import zlib

from ucfb import child, children_of, chunks, cstr_at, iter_archives

try:
    import numpy as np
except ImportError:  # the plain loops below do the same, slower
    np = None

DXT1 = 0x31545844
DXT3 = 0x33545844
DXT5 = 0x35545844
A8R8G8B8 = 21
X8R8G8B8 = 22
R5G6B5 = 23
A1R5G5B5 = 25
A4R4G4B4 = 26
V8U8 = 60  # signed du/dv bump offsets (the water's distortion maps)
DECODABLE = (A8R8G8B8, X8R8G8B8, DXT5, DXT3, DXT1, A4R4G4B4, A1R5G5B5, R5G6B5, V8U8)


def safe_name(name):
    """A texture name made safe for a file name."""
    return "".join(c if c.isalnum() or c in "-_" else "_" for c in name)


def write_png(path, width, height, rgba):
    def chunk(tag, payload):
        return (
            struct.pack(">I", len(payload))
            + tag
            + payload
            + struct.pack(">I", zlib.crc32(tag + payload) & 0xFFFFFFFF)
        )

    stride = width * 4
    if np is not None and len(rgba) >= stride * height:
        rows = np.frombuffer(rgba, dtype=np.uint8, count=stride * height).reshape(height, stride)
        raw = np.concatenate([np.zeros((height, 1), dtype=np.uint8), rows], axis=1).tobytes()
    else:
        raw = bytearray()
        for y in range(height):
            raw.append(0)
            raw += rgba[y * stride:(y + 1) * stride]
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 1))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


def _rgb565(c):
    return (
        ((c >> 11) & 31) * 255 // 31,
        ((c >> 5) & 63) * 255 // 63,
        (c & 31) * 255 // 31,
    )


def _dxt_colors(c0, c1, four):
    cols = [_rgb565(c0), _rgb565(c1)]
    if four:
        cols.append(tuple((2 * a + b) // 3 for a, b in zip(cols[0], cols[1])))
        cols.append(tuple((a + 2 * b) // 3 for a, b in zip(cols[0], cols[1])))
    else:
        cols.append(tuple((a + b) // 2 for a, b in zip(cols[0], cols[1])))
        cols.append((0, 0, 0))
    return cols


def decode_dxt1(body, width, height, punch):
    rgba = bytearray(width * height * 4)
    bw = max(1, (width + 3) // 4)
    bh = max(1, (height + 3) // 4)
    for by in range(bh):
        for bx in range(bw):
            off = (by * bw + bx) * 8
            if off + 8 > len(body):
                return rgba
            c0, c1, bits = struct.unpack_from("<HHI", body, off)
            four = (not punch) or c0 > c1
            cols = _dxt_colors(c0, c1, four)
            alphas = (255, 255, 255, 255) if four else (255, 255, 255, 0)
            for i in range(16):
                code = (bits >> (2 * i)) & 3
                px, py = bx * 4 + (i & 3), by * 4 + (i >> 2)
                if px >= width or py >= height:
                    continue
                r, g, b = cols[code]
                o = (py * width + px) * 4
                rgba[o:o + 4] = bytes((r, g, b, alphas[code]))
    return rgba


def decode_dxt5(body, width, height):
    rgba = bytearray(width * height * 4)
    bw = max(1, (width + 3) // 4)
    bh = max(1, (height + 3) // 4)
    for by in range(bh):
        for bx in range(bw):
            off = (by * bw + bx) * 16
            if off + 16 > len(body):
                return rgba
            a0 = body[off]
            a1 = body[off + 1]
            abits = int.from_bytes(body[off + 2:off + 8], "little")
            alphas = [a0, a1]
            if a0 > a1:
                for i in range(1, 7):
                    alphas.append(((7 - i) * a0 + i * a1) // 7)
            else:
                for i in range(1, 5):
                    alphas.append(((5 - i) * a0 + i * a1) // 5)
                alphas.extend((0, 255))
            c0, c1, bits = struct.unpack_from("<HHI", body, off + 8)
            cols = _dxt_colors(c0, c1, True)
            for i in range(16):
                ac = (abits >> (3 * i)) & 7
                cc = (bits >> (2 * i)) & 3
                px, py = bx * 4 + (i & 3), by * 4 + (i >> 2)
                if px >= width or py >= height:
                    continue
                r, g, b = cols[cc]
                o = (py * width + px) * 4
                rgba[o:o + 4] = bytes((r, g, b, alphas[ac]))
    return rgba


def decode_dxt3(body, width, height):
    rgba = bytearray(width * height * 4)
    bw = max(1, (width + 3) // 4)
    bh = max(1, (height + 3) // 4)
    for by in range(bh):
        for bx in range(bw):
            off = (by * bw + bx) * 16
            if off + 16 > len(body):
                return rgba
            explicit = int.from_bytes(body[off:off + 8], "little")
            c0, c1, bits = struct.unpack_from("<HHI", body, off + 8)
            cols = _dxt_colors(c0, c1, True)
            for i in range(16):
                a = ((explicit >> (4 * i)) & 15) * 17
                cc = (bits >> (2 * i)) & 3
                px, py = bx * 4 + (i & 3), by * 4 + (i >> 2)
                if px >= width or py >= height:
                    continue
                r, g, b = cols[cc]
                o = (py * width + px) * 4
                rgba[o:o + 4] = bytes((r, g, b, a))
    return rgba


def decode_raw(fmt, body, width, height):
    rgba = bytearray(width * height * 4)
    if fmt in (A8R8G8B8, X8R8G8B8):
        for i in range(width * height):
            b, g, r, a = body[i * 4:i * 4 + 4]
            if fmt == X8R8G8B8:
                a = 255
            o = i * 4
            rgba[o:o + 4] = bytes((r, g, b, a))
        return rgba
    if fmt == R5G6B5:
        for i in range(width * height):
            c = struct.unpack_from("<H", body, i * 2)[0]
            r, g, b = _rgb565(c)
            o = i * 4
            rgba[o:o + 4] = bytes((r, g, b, 255))
        return rgba
    if fmt == A1R5G5B5:
        for i in range(width * height):
            c = struct.unpack_from("<H", body, i * 2)[0]
            r = ((c >> 10) & 31) * 255 // 31
            g = ((c >> 5) & 31) * 255 // 31
            b = (c & 31) * 255 // 31
            a = 255 if c & 0x8000 else 0
            o = i * 4
            rgba[o:o + 4] = bytes((r, g, b, a))
        return rgba
    if fmt == V8U8:
        # Stored as r = du + 128, g = dv + 128.
        for i in range(width * height):
            du, dv = struct.unpack_from("<bb", body, i * 2)
            o = i * 4
            rgba[o:o + 4] = bytes((du + 128, dv + 128, 128, 255))
        return rgba
    if fmt == A4R4G4B4:
        for i in range(width * height):
            c = struct.unpack_from("<H", body, i * 2)[0]
            a = ((c >> 12) & 15) * 17
            r = ((c >> 8) & 15) * 17
            g = ((c >> 4) & 15) * 17
            b = (c & 15) * 17
            o = i * 4
            rgba[o:o + 4] = bytes((r, g, b, a))
        return rgba
    return None


def _np_rgb565(c):
    c = c.astype(np.int32)
    return np.stack([((c >> 11) & 31) * 255 // 31, ((c >> 5) & 63) * 255 // 63, (c & 31) * 255 // 31], axis=-1)


def _np_dxt_colors(c0, c1, four):
    """Each block's four colours (n, 4, 3): the two stored and their mixes,
    thirds in four-colour mode, else a half and black."""
    a, b = _np_rgb565(c0), _np_rgb565(c1)
    four = four[:, None]
    col2 = np.where(four, (2 * a + b) // 3, (a + b) // 2)
    col3 = np.where(four, (a + 2 * b) // 3, 0)
    return np.stack([a, b, col2, col3], axis=1)


def _np_blocks(body, width, height, size):
    """The block records of a DXT body as a zero-padded (bh * bw, size)
    uint8 array, with the grid's shape; blocks past the body's end stay
    zero, as the loops leave their pixels."""
    bw, bh = max(1, (width + 3) // 4), max(1, (height + 3) // 4)
    count = min(bw * bh, len(body) // size)
    blocks = np.zeros((bw * bh, size), dtype=np.uint8)
    if count:
        blocks[:count] = np.frombuffer(body, dtype=np.uint8, count=count * size).reshape(count, size)
    return blocks, bw, bh, count


def _np_assemble(pixels, bw, bh, width, height, count):
    """(blocks, 16, 4) pixel values to the image's RGBA bytes, blocks laid
    4 by 4, cropped to the image, blocks past the body's end left zero."""
    pixels = pixels.astype(np.uint8)
    pixels[count:] = 0
    image = pixels.reshape(bh, bw, 4, 4, 4).transpose(0, 2, 1, 3, 4).reshape(bh * 4, bw * 4, 4)
    return bytearray(np.ascontiguousarray(image[:height, :width]).tobytes())


def _np_codes(words, bits, count):
    """count codes of bits each from the low end of each word (n,) -> (n, count)."""
    shifts = np.arange(count, dtype=np.uint64) * bits
    return ((words[:, None] >> shifts) & ((1 << bits) - 1)).astype(np.int64)


def _np_u16(blocks, at):
    return blocks[:, at].astype(np.uint32) | (blocks[:, at + 1].astype(np.uint32) << 8)


def _np_u64(blocks, at, size):
    out = np.zeros(len(blocks), dtype=np.uint64)
    for i in range(size):
        out |= blocks[:, at + i].astype(np.uint64) << np.uint64(8 * i)
    return out


def _np_dxt1(body, width, height, punch):
    blocks, bw, bh, count = _np_blocks(body, width, height, 8)
    c0, c1 = _np_u16(blocks, 0), _np_u16(blocks, 2)
    four = np.full(len(blocks), True) if not punch else c0 > c1
    cols = _np_dxt_colors(c0, c1, four)
    codes = _np_codes(_np_u64(blocks, 4, 4), 2, 16)
    rgb = cols[np.arange(len(blocks))[:, None], codes]
    alpha = np.where(four[:, None] | (codes != 3), 255, 0)
    return _np_assemble(np.concatenate([rgb, alpha[:, :, None]], axis=2), bw, bh, width, height, count)


def _np_dxt_rgb(blocks, at):
    c0, c1 = _np_u16(blocks, at), _np_u16(blocks, at + 2)
    cols = _np_dxt_colors(c0, c1, np.full(len(blocks), True))
    codes = _np_codes(_np_u64(blocks, at + 4, 4), 2, 16)
    return cols[np.arange(len(blocks))[:, None], codes]


def _np_dxt3(body, width, height):
    blocks, bw, bh, count = _np_blocks(body, width, height, 16)
    alpha = _np_codes(_np_u64(blocks, 0, 8), 4, 16) * 17
    return _np_assemble(np.concatenate([_np_dxt_rgb(blocks, 8), alpha[:, :, None]], axis=2), bw, bh, width, height, count)


def _np_dxt5(body, width, height):
    blocks, bw, bh, count = _np_blocks(body, width, height, 16)
    a0, a1 = blocks[:, 0].astype(np.int64), blocks[:, 1].astype(np.int64)
    seven = a0 > a1
    i = np.arange(1, 7)[None, :]
    table7 = ((7 - i) * a0[:, None] + i * a1[:, None]) // 7
    i = np.arange(1, 5)[None, :]
    table5 = np.concatenate([((5 - i) * a0[:, None] + i * a1[:, None]) // 5, np.zeros((len(blocks), 1), dtype=np.int64),
                             np.full((len(blocks), 1), 255, dtype=np.int64)], axis=1)
    table = np.concatenate([a0[:, None], a1[:, None], np.where(seven[:, None], table7, table5)], axis=1)
    alpha = table[np.arange(len(blocks))[:, None], _np_codes(_np_u64(blocks, 2, 6), 3, 16)]
    return _np_assemble(np.concatenate([_np_dxt_rgb(blocks, 8), alpha[:, :, None]], axis=2), bw, bh, width, height, count)


def _np_raw(fmt, body, width, height):
    n = width * height
    out = np.zeros((n, 4), dtype=np.uint8)
    if fmt in (A8R8G8B8, X8R8G8B8):
        count = min(n, len(body) // 4)
        px = np.frombuffer(body, dtype=np.uint8, count=count * 4).reshape(count, 4)
        out[:count, 0], out[:count, 1], out[:count, 2] = px[:, 2], px[:, 1], px[:, 0]
        out[:count, 3] = 255 if fmt == X8R8G8B8 else px[:, 3]
        return bytearray(out.tobytes())
    count = min(n, len(body) // 2)
    if fmt == V8U8:
        du = np.frombuffer(body, dtype=np.int8, count=count * 2).reshape(count, 2).astype(np.int16) + 128
        out[:count, 0], out[:count, 1] = du[:, 0], du[:, 1]
        out[:count, 2], out[:count, 3] = 128, 255
        return bytearray(out.tobytes())
    c = np.frombuffer(body, dtype="<u2", count=count).astype(np.int32)
    if fmt == R5G6B5:
        out[:count, :3] = _np_rgb565(c)
        out[:count, 3] = 255
    elif fmt == A1R5G5B5:
        out[:count, 0] = ((c >> 10) & 31) * 255 // 31
        out[:count, 1] = ((c >> 5) & 31) * 255 // 31
        out[:count, 2] = (c & 31) * 255 // 31
        out[:count, 3] = np.where(c & 0x8000, 255, 0)
    elif fmt == A4R4G4B4:
        out[:count, 0] = ((c >> 8) & 15) * 17
        out[:count, 1] = ((c >> 4) & 15) * 17
        out[:count, 2] = (c & 15) * 17
        out[:count, 3] = ((c >> 12) & 15) * 17
    else:
        return None
    return bytearray(out.tobytes())


def decode_pixels(fmt, pixels, width, height):
    """A mip's pixels in fmt to RGBA bytes (width * height * 4), or None
    for a format not decoded."""
    if np is not None:
        if fmt == DXT1:
            return _np_dxt1(pixels, width, height, True)
        if fmt == DXT3:
            return _np_dxt3(pixels, width, height)
        if fmt == DXT5:
            return _np_dxt5(pixels, width, height)
        return _np_raw(fmt, pixels, width, height)
    if fmt == DXT1:
        return decode_dxt1(pixels, width, height, True)
    if fmt == DXT3:
        return decode_dxt3(pixels, width, height)
    if fmt == DXT5:
        return decode_dxt5(pixels, width, height)
    return decode_raw(fmt, pixels, width, height)


class Image:
    """A texture's (width, height, rgba), the pixels decoded when first
    read. Unpacks like the tuple it stands in for."""

    __slots__ = ("width", "height", "_source", "_rgba")

    def __init__(self, width, height, rgba=None, source=None):
        self.width, self.height = width, height
        self._rgba = rgba
        self._source = source  # (fmt, data, face_body, face_end)

    @property
    def rgba(self):
        if self._rgba is None:
            fmt, data, face_body, face_end = self._source
            self._rgba = decode_pixels(fmt, mip0(data, face_body, face_end), self.width, self.height)
            self._source = None
        return self._rgba

    def __iter__(self):
        return iter((self.width, self.height, self.rgba))

    def __getitem__(self, index):
        return (self.width, self.height, self.rgba)[index]

    def __len__(self):
        return 3


def mip0(data, face_body, face_end):
    levels = children_of(data, face_body, face_end, b"LVL_")
    if not levels:
        return data[face_body:face_end]
    body, end = levels[0]
    info = child(data, body, end, b"INFO")
    raw = child(data, body, end, b"BODY")
    if raw is None:
        return b""
    return data[raw[0]:raw[1]] if info else data[raw[0]:raw[1]]


def decode_texture(data, body, end, lazy=False):
    """(name, image, formats) for a tex_ chunk: the image (width, height,
    rgba) from the best decodable format, None when there is none; lazy
    gives an Image that decodes when read."""
    name_c = child(data, body, end, b"NAME")
    if name_c is None:
        return None
    name = cstr_at(data, name_c[0], name_c[1])
    formats = []
    best = None
    best_rank = 99
    for t, b, e in chunks(data, body, end):
        if t != b"FMT_":
            continue
        info = child(data, b, e, b"INFO")
        face = child(data, b, e, b"FACE")
        if info is None or face is None or info[1] - info[0] < 16:
            continue
        fmt, width, height, depth, mips, type_bias = struct.unpack_from("<IHHHHI", data, info[0])
        formats.append(fmt)
        if (type_bias & 0xFF) != 1:
            continue
        if fmt not in DECODABLE:
            continue
        rank = DECODABLE.index(fmt)
        if rank < best_rank:
            best_rank = rank
            best = (fmt, width, height, None, face[0], face[1])
    if best is None:
        return name, None, formats
    fmt, width, height, _unused, face_body, face_end = best
    if lazy:
        return name, Image(width, height, source=(fmt, data, face_body, face_end)), formats
    return name, (width, height, decode_pixels(fmt, mip0(data, face_body, face_end), width, height)), formats


def collect_textures(data, textures):
    unknown = {}
    for start, end in iter_archives(data):
        for t, b, e in chunks(data, start, end):
            if t != b"tex_":
                continue
            parsed = decode_texture(data, b, e, lazy=True)
            if parsed is None:
                continue
            name, image, formats = parsed
            if image is None:
                unknown[name] = formats
                continue
            if name not in textures:
                textures[name] = image
    return unknown
