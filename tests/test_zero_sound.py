"""Checks for tools/zero_sound.py on synthetic sound archives (no retail data).

    python3 -m unittest tests/test_zero_sound.py
"""

import math
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

from zero_hash import fnv1a  # noqa: E402
import zero_sound as zs  # noqa: E402


def chunk(tag, body):
    tag = struct.pack("<I", fnv1a(tag)) if isinstance(tag, str) else tag
    pad = b"\x00" * ((4 - len(body) % 4) % 4)
    return tag + struct.pack("<I", len(body)) + body + pad


def pair(name, value):
    key = fnv1a(name) if isinstance(name, str) else name
    return struct.pack("<II", key, value)


def marker(name):
    return struct.pack("<I", fnv1a(name))


def info(head, items, list_name="SampleInfo"):
    body = b"".join(pair(k, v) for k, v in head)
    body += marker(list_name)
    for item in items:
        body += marker("Sample") + b"".join(pair(k, v) for k, v in item) + marker("SampleEnd")
    return chunk("Info", body)


def ima_block(samples, index=0):
    """One mono Xbox ADPCM block (36 bytes) for the first 64 of `samples`,
    encoded with the reference IMA rules."""
    pred = max(-32768, min(32767, int(samples[0])))
    out = bytearray(struct.pack("<hBB", pred, index, 0))
    codes = []
    for target in samples[1:64]:
        step = zs.STEPS[index]
        diff = target - pred
        code = 0
        if diff < 0:
            code = 8
            diff = -diff
        vp = step >> 3
        if diff >= step:
            code |= 4
            diff -= step
            vp += step
        if diff >= step >> 1:
            code |= 2
            diff -= step >> 1
            vp += step >> 1
        if diff >= step >> 2:
            code |= 1
            vp += step >> 2
        pred = max(-32768, min(32767, pred - vp if code & 8 else pred + vp))
        index = max(0, min(88, index + zs.INDEX_STEP[code & 7]))
        codes.append(code)
    codes.append(0)
    for i in range(0, 64, 2):
        out.append(codes[i] | (codes[i + 1] << 4))
    return bytes(out), index


class ZeroSoundTest(unittest.TestCase):
    def test_info_records_and_aliases(self):
        data = info(
            [("Name", fnv1a("common")), ("Format", 2), ("NumSamples", 3)],
            [
                [("Id", 1), ("Frequency", 22050), ("Size", 100), (0x1D48FEEF, 50), ("Padding", 0)],
                [("Id", 2), ("Frequency", 22050), ("Size", 40), (0x1D48FEEF, 20), ("Padding", 0), ("Alias", 1)],
                [("Id", 3), ("Frequency", 44100), ("Size", 60), (0x1D48FEEF, 30), ("Padding", 4)],
            ],
        )
        head, items = zs.read_info(data, 8, len(data))
        self.assertEqual(head["Format"], zs.PCM16)
        self.assertEqual([i["Id"] for i in items], [1, 2, 3])
        self.assertEqual(items[0]["Frames"], 50)
        layout = zs._layout(head, items, None)
        # Aliases take no space: the third sample follows the first.
        self.assertEqual(layout[0]["offset"], 0)
        self.assertNotIn("offset", layout[1])
        self.assertEqual(layout[1]["alias"], 1)
        self.assertEqual(layout[2]["offset"], 100)

    def test_adpcm_block_holds_64_frames_header_first(self):
        tone = [9000 * math.sin(2 * math.pi * 440 * i / 22050) for i in range(64)]
        # Start the step size where a stream in progress would have it.
        block, _ = ima_block(tone, index=50)
        self.assertEqual(len(block), zs.ADPCM_BLOCK)
        (decoded,) = zs.decode_xbox_adpcm(block, 1)
        self.assertEqual(len(decoded), zs.ADPCM_FRAMES)
        self.assertEqual(decoded[0], int(tone[0]))
        err = sum((d - t) ** 2 for d, t in zip(decoded, tone)) / 64
        self.assertLess(math.sqrt(err), 300)

    def test_stereo_interleaves_every_four_bytes(self):
        left = [8000 * math.sin(i / 7) for i in range(64)]
        right = [-6000 * math.sin(i / 5) for i in range(64)]
        bl, _ = ima_block(left)
        br, _ = ima_block(right)
        stereo = bl[:4] + br[:4]
        for g in range(8):
            stereo += bl[4 + 4 * g:8 + 4 * g] + br[4 + 4 * g:8 + 4 * g]
        dl, dr = zs.decode_xbox_adpcm(stereo, 2)
        (ml,) = zs.decode_xbox_adpcm(bl, 1)
        (mr,) = zs.decode_xbox_adpcm(br, 1)
        self.assertEqual(list(dl), list(ml))
        self.assertEqual(list(dr), list(mr))

    def test_archive_streams_banks_and_sublevels(self):
        block, _ = ima_block([1000 * math.sin(i / 3) for i in range(64)])
        stream_info = info(
            [("Name", fnv1a("nab2_emt")), ("Format", zs.XBOX_ADPCM), ("NumChannels", 1), ("NumSegments", 1), ("NumSubstreams", 1)],
            [[("Id", fnv1a("seg")), ("Frequency", 44100), ("Size", len(block)), (0x1D48FEEF, 64), ("Padding", 0)]],
            "SegmentInfo",
        )
        stream = chunk("StreamList", chunk("Stream", stream_info + chunk("Data", block)))
        bank = chunk(
            "SoundBankList",
            chunk("SampleBank", info([("Name", fnv1a("nab2cw")), ("Format", 2), (0x694AAA0B, 1)], [[("Id", 7), ("Frequency", 22050), ("Size", 8), (0x1D48FEEF, 4)]])),
        )
        # A config: SoundProperties { Name(hash) Gain(0.5) }.
        def data_entry(name, values):
            body = struct.pack("<IB", fnv1a(name), len(values)) + b"".join(values) + struct.pack("<I", 0)
            return b"DATA" + struct.pack("<I", len(body)) + body + b"\x00" * ((4 - len(body) % 4) % 4)

        inner = data_entry("Name", [struct.pack("<I", fnv1a("canal"))]) + data_entry("Gain", [struct.pack("<f", 0.5)])
        scop = b"SCOP" + struct.pack("<I", len(inner)) + inner
        snd = b"snd_" + struct.pack("<I", 0)
        top = data_entry("SoundProperties", [])
        cfg_body = top + scop
        snd = b"snd_" + struct.pack("<I", len(cfg_body)) + cfg_body
        sub_body = bank + snd
        lvl = b"lvl_" + struct.pack("<I", len(sub_body) + 8) + struct.pack("<II", fnv1a("nab2cw"), len(sub_body)) + sub_body
        root_body = stream + lvl
        data = b"ucfb" + struct.pack("<I", len(root_body)) + root_body
        contents = zs.archive_contents(data)
        self.assertEqual(len(contents["streams"]), 1)
        s = contents["streams"][0]
        self.assertEqual(s["head"]["Name"], fnv1a("nab2_emt"))
        self.assertEqual(s["items"][0]["frames"], 64)
        self.assertIn(fnv1a("nab2cw"), contents["sublevels"])
        level = zs.sublevel_contents(data, contents["sublevels"][fnv1a("nab2cw")])
        self.assertEqual(level["banks"][0]["head"]["External"], 1)
        objects = zs.config_objects(data, level["configs"])
        self.assertEqual(objects, [{"type": "SoundProperties", "Name": "0x%08x" % fnv1a("canal"), "Gain": 0.5}])

    def test_mission_audio_calls(self):
        calls = [
            ("ReadDataFile", ["sound\\nab.lvl;nab2cw"]),
            ("OpenAudioStream", ["sound\\nab.lvl", "nabcw_music"]),
            ("SetAmbientMusic", [2.0, 1.0, "rep_nab_amb_start", 0.0, 1.0]),
            ("SetVictoryMusic", [2.0, "rep_nab_amb_victory"]),
            ("SetSoundEffect", ["ScopeDisplayZoomIn", "binocularzoomin"]),
        ]
        m = zs.read_mission_audio(calls)
        self.assertEqual(m["banks"], ["sound\\nab.lvl;nab2cw"])
        self.assertEqual(m["streams"], [["sound\\nab.lvl", "nabcw_music"]])
        self.assertEqual(m["music"]["2"], [[1.0, "rep_nab_amb_start", 0.0, 1.0]])
        self.assertEqual(m["victory"]["2"], "rep_nab_amb_victory")
        self.assertEqual(m["effects"]["ScopeDisplayZoomIn"], "binocularzoomin")


if __name__ == "__main__":
    unittest.main()


@unittest.skipUnless(zs.np is not None, "requires numpy")
class NumpyAdpcmMatchesLoops(unittest.TestCase):
    def test_every_block_and_channel_decodes_as_the_loops_do(self):
        import random
        random.seed(7)
        blocks = []
        for b in range(40):
            samples = [int(20000 * math.sin(i * 0.05 * (b + 1)) + random.randint(-3000, 3000)) for i in range(64)]
            blocks.append(ima_block(samples, index=random.randint(0, 88))[0])
        mono = b"".join(blocks)
        # Stereo interleaves the headers, then each 4-byte code group, per channel.
        stereo = b"".join(blocks[2 * i][:4] + blocks[2 * i + 1][:4] + b"".join(
            blocks[2 * i][4 + 4 * g:8 + 4 * g] + blocks[2 * i + 1][4 + 4 * g:8 + 4 * g] for g in range(8)) for i in range(20))
        fast = zs.decode_xbox_adpcm
        saved = zs.np
        zs.np = None
        try:
            slow_mono = zs.decode_xbox_adpcm(mono, 1)
            slow_stereo = zs.decode_xbox_adpcm(stereo, 2)
        finally:
            zs.np = saved
        self.assertEqual(fast(mono, 1), slow_mono)
        self.assertEqual(fast(stereo, 2), slow_stereo)
        self.assertEqual(fast(mono + b"\x00" * 10, 1), slow_mono)  # a trailing partial block is ignored
        self.assertEqual(list(fast(b"", 1)[0]), [])


class ConfigScopes(unittest.TestCase):
    def test_a_second_scope_adds_to_the_entry_it_follows(self):
        # Retail sub-levels follow some objects with an empty scope (the
        # AT-ST's head cannon FireSound on every GCW map); it must not wipe
        # the object's Name and samples.
        def data(name, values=(), raw=False):
            body = struct.pack("<IB", fnv1a(name), len(values))
            body += b"".join(struct.pack("<I", v) if raw else struct.pack("<f", v) for v in values)
            return chunk(b"DATA", body)
        sample = data("Sample", (0x55702388, 0x3F800000), raw=True)
        body = (data("SoundProperties") + chunk(b"SCOP", data("Name", (fnv1a("head_fire"),), raw=True) + data("SampleList")
                                                + chunk(b"SCOP", sample))
                + chunk(b"SCOP", chunk(b"SCOP", b""))
                + data("SoundProperties") + chunk(b"SCOP", data("Name", (fnv1a("next"),), raw=True) + data("Gain", (0.5,))))
        objects = zs.config_objects(body, [(b"snd_", 0, len(body))])
        self.assertEqual([o.get("Name") for o in objects], [zs.hexname(fnv1a("head_fire")), zs.hexname(fnv1a("next"))])
        self.assertEqual(objects[0]["SampleList"], [[zs.hexname(0x55702388), 1.0]])
        self.assertEqual(objects[1]["Gain"], 0.5)
