"""tools/zero_texture.py: the numpy decoders give the bytes the plain loops
do, for every format, odd sizes and truncated bodies; collected textures
decode only when read."""
import os
import random
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import zero_texture as zt  # noqa: E402


@unittest.skipUnless(zt.np is not None, "requires numpy")
class NumpyDecodersMatchLoops(unittest.TestCase):
    def body(self, n, seed):
        random.seed(seed)
        return bytes(random.getrandbits(8) for _ in range(n))

    def test_dxt(self):
        for width, height in ((4, 4), (8, 8), (13, 7), (1, 1), (64, 32)):
            bw, bh = max(1, (width + 3) // 4), max(1, (height + 3) // 4)
            for size, loop, fast in ((8, lambda b: zt.decode_dxt1(b, width, height, True), lambda b: zt._np_dxt1(b, width, height, True)),
                                     (16, lambda b: zt.decode_dxt3(b, width, height), lambda b: zt._np_dxt3(b, width, height)),
                                     (16, lambda b: zt.decode_dxt5(b, width, height), lambda b: zt._np_dxt5(b, width, height))):
                for length in (bw * bh * size, bw * bh * size - size, 3):  # whole, a block short, nothing whole
                    body = self.body(length, length * size + width)
                    self.assertEqual(bytes(fast(body)), bytes(loop(body)), (size, width, height, length))

    def test_raw(self):
        for fmt in (zt.A8R8G8B8, zt.X8R8G8B8, zt.R5G6B5, zt.A1R5G5B5, zt.A4R4G4B4, zt.V8U8):
            width, height = 7, 5
            per = 4 if fmt in (zt.A8R8G8B8, zt.X8R8G8B8) else 2
            body = self.body(width * height * per, fmt)
            self.assertEqual(bytes(zt._np_raw(fmt, body, width, height)), bytes(zt.decode_raw(fmt, body, width, height)), fmt)
        self.assertIsNone(zt._np_raw(99, b"", 1, 1))

    def test_dxt1_two_colour_blocks_punch_through(self):
        # c0 <= c1: three colours and a transparent black.
        body = bytes([0x00, 0x00, 0xff, 0xff]) + bytes([0b11100100] * 4)
        self.assertEqual(bytes(zt._np_dxt1(body, 4, 4, True)), bytes(zt.decode_dxt1(body, 4, 4, True)))
        self.assertEqual(bytes(zt._np_dxt1(body, 4, 1, True))[12:16], b"\x00\x00\x00\x00")


class LazyImage(unittest.TestCase):
    def test_unpacks_like_a_tuple_and_decodes_once(self):
        calls = []
        original = zt.decode_pixels
        zt.decode_pixels = lambda fmt, pixels, w, h: calls.append(fmt) or bytearray(w * h * 4)
        try:
            data = b"BODY" + (8).to_bytes(4, "little") + bytes(8)
            image = zt.Image(2, 1, source=(zt.A8R8G8B8, data, 0, len(data)))
            width, height, rgba = image
            self.assertEqual((width, height, len(rgba)), (2, 1, 8))
            self.assertEqual(image[2], rgba)
            self.assertEqual(len(image), 3)
            self.assertEqual(calls, [zt.A8R8G8B8])
        finally:
            zt.decode_pixels = original


if __name__ == "__main__":
    unittest.main()
