#!/usr/bin/env python3
"""Writes pattern.bik: a synthetic Bink 1 (revision i) movie, 32 by 24 pixels, five frames at 10 fps,
every frame a flat colour made of fill blocks (Y = 20 + 40 n, U = 60 + 10 n, V = 200 - 20 n for frame n).
No game data. Run from this folder: python3 make_pattern_bik.py"""

import struct

WIDTH, HEIGHT, FRAMES = 32, 24, 5
FILL = 6


class Bits:
    """Bits the way Bink reads them: first bit lowest."""

    def __init__(self):
        self.bytes = bytearray()
        self.n = 0

    def put(self, count, value):
        for i in range(count):
            if self.n % 8 == 0:
                self.bytes.append(0)
            if (value >> i) & 1:
                self.bytes[-1] |= 1 << (self.n % 8)
            self.n += 1

    def align32(self):
        while self.n % 32:
            self.put(1, 0)


def plane(bw, bh, value):
    # Every bundle count is 10 bits wide at these sizes.
    b = Bits()
    for _ in range(23):  # trees: block types, sub types, 16 colour high nibbles, colours, pattern, x, y, run
        b.put(4, 0)
    for row in range(bh):
        b.put(10, bw)  # block types: all fill
        b.put(1, 1)
        b.put(4, FILL)
        if row == 0:
            b.put(10, 0)  # no 16x16 sub types
        b.put(10, bw)  # colours: all the same
        b.put(1, 1)
        b.put(4, value >> 4)
        b.put(4, value & 15)
        if row == 0:
            for _ in range(6):  # no patterns, motion values, DCs or runs
                b.put(10, 0)
    b.align32()
    return bytes(b.bytes)


def frame(n):
    y, u, v = 20 + 40 * n, 60 + 10 * n, 200 - 20 * n
    luma = plane((WIDTH + 7) >> 3, (HEIGHT + 7) >> 3, y)
    chroma = ((WIDTH + 15) >> 4, (HEIGHT + 15) >> 4)
    # Revision i: 32 bits, then Y, V, U.
    return bytes(4) + luma + plane(*chroma, v) + plane(*chroma, u)


frames = [frame(n) for n in range(FRAMES)]
header_len = 44 + 4 * FRAMES
offsets, at = [], header_len
for n, f in enumerate(frames):
    offsets.append(at | (n == 0))
    at += len(f)
out = b"BIKi" + struct.pack("<10I", at - 8, FRAMES, max(map(len, frames)), 0, WIDTH, HEIGHT, 10, 1, 0, 0)
out += struct.pack(f"<{FRAMES}I", *offsets) + b"".join(frames)
open("pattern.bik", "wb").write(out)
print(f"pattern.bik: {len(out)} bytes")
