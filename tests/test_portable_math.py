"""What the tools write must come out the same on every platform: the
content fingerprint (src/runtime/content-hash.js) compares a player's
preparation with the server's to the last bit. tools/portable_math.py
gives sin, cos and atan2 exactly (where glibc and the Windows runtime each
miss the last bit now and then), the tools that write fingerprinted data
use it rather than the platform's library, and tools/zero_addon.py records
an add-on's paths as spelled on disk whatever the platform's case rules.
Nothing retail."""
import math
import re
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

TOOLS = Path(__file__).resolve().parents[1] / "tools"
sys.path.insert(0, str(TOOLS))
import portable_math  # noqa: E402
import zero_addon  # noqa: E402

# Correctly rounded values (mpmath at 200 bits). glibc 2.39 returns the
# double next to each of these; the Windows runtime misses others.
EXACT = [
    (portable_math.sin, (-0.22073799048388842,), -0.21894976661916013),
    (portable_math.sin, (7.685331110508933,), 0.9858121770327489),
    (portable_math.cos, (-2.066390506984397,), -0.4755544273599666),
    (portable_math.cos, (8.20034112631113,), -0.33947574918893525),
    (portable_math.atan2, (0.9192394293994341, 0.8926870009608265), 0.8000513676074187),
    (portable_math.atan2, (0.06591652565492123, -0.2854602056182467), 2.9146571329214273),
    (portable_math.atan2, (0.09494217047896103, 0.8132345565452741), 0.11622024470420371),
]

# Tools whose output the fingerprint never reads: sound, textures, movies,
# the HUD's atlases and vectors, audits and test harnesses.
UNFINGERPRINTED = {
    "enhance_audio.py", "make_binaural.py", "zero_sound.py", "zero_texture.py", "ktx2.py",
    "upscale_hud.py", "vectorize_hud.py",
}
PLATFORM_MATH = re.compile(
    r"math\.(sin|cos|tan|asin|acos|atan|atan2|sinh|cosh|tanh|asinh|acosh|atanh|exp|exp2|expm1|log|log2|log10|log1p|pow|cbrt|erf|erfc|gamma|lgamma)\("
    r"|\*\*\s*\(?\s*(0?\.5|1\s*/|2\.\d)")


class PortableMathTest(unittest.TestCase):
    def test_exact_where_the_platform_libraries_miss(self):
        for function, args, expected in EXACT:
            self.assertEqual(function(*args), expected, (function.__name__, args))

    def test_agrees_with_the_library_where_both_are_exact(self):
        for x in (0.5, 1.0, 2.0, math.pi / 7, -3.0, 100.0, 1e-8):
            self.assertAlmostEqual(portable_math.sin(x), math.sin(x), delta=abs(math.sin(x)) * 4e-16 + 1e-300)
            self.assertAlmostEqual(portable_math.cos(x), math.cos(x), delta=abs(math.cos(x)) * 4e-16 + 1e-300)
        for y, x in ((1.0, 2.0), (-1.0, 2.0), (1.0, -2.0), (-1.0, -2.0), (3.0, 1e-9), (1e-9, -3.0)):
            self.assertAlmostEqual(portable_math.atan2(y, x), math.atan2(y, x), delta=abs(math.atan2(y, x)) * 4e-16)

    def test_special_values_as_c99_defines_them(self):
        for y, x in ((0.0, 1.0), (-0.0, 1.0), (0.0, -1.0), (-0.0, -1.0), (1.0, 0.0), (-1.0, -0.0), (0.0, -0.0), (math.inf, -math.inf)):
            self.assertEqual(math.copysign(1, portable_math.atan2(y, x)), math.copysign(1, math.atan2(y, x)))
            self.assertEqual(portable_math.atan2(y, x), math.atan2(y, x))
        self.assertEqual(math.copysign(1, portable_math.sin(-0.0)), -1.0)
        self.assertEqual(portable_math.cos(-0.0), 1.0)
        self.assertTrue(math.isnan(portable_math.atan2(math.nan, 1.0)))
        # A quadrant's edge: the angle a quarter turn on, the right sign.
        self.assertEqual(portable_math.cos(math.pi / 2), 6.123233995736766e-17)
        self.assertEqual(portable_math.sin(math.pi), 1.2246467991473532e-16)

    def test_fingerprinted_tools_use_no_platform_math(self):
        found = []
        for path in sorted(TOOLS.glob("*.py")):
            if path.name in UNFINGERPRINTED or path.name.startswith(("audit", "multiplayer")) or path.name == "portable_math.py":
                continue
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                if PLATFORM_MATH.search(line.split("#", 1)[0]):
                    found.append(f"{path.name}:{number}: {line.strip()}")
        self.assertEqual(found, [], "use tools/portable_math.py (sin, cos, atan2) or math.sqrt")


class OnDiskSpellingTest(unittest.TestCase):
    def test_an_add_on_is_recorded_as_spelled_on_disk_where_names_ignore_case(self):
        with tempfile.TemporaryDirectory() as temp:
            game_data = Path(temp) / "GameData"
            game = game_data / "Data" / "_LVL_PC"
            (game / "SIDE").mkdir(parents=True)
            root = game_data / "AddOn" / "Tat3" / "Data" / "_lvl_pc"
            for part in ("TAT", "Side"):
                (root / part).mkdir(parents=True)
            (root / "TAT" / "tat3.lvl").write_bytes(b"")
            (root / "Side" / "gam.lvl").write_bytes(b"")
            (game_data / "AddOn" / "Tat3" / "addme.script").write_bytes(b"")
            real_exists = Path.exists

            def windows_exists(path, *args, **kwargs):
                """As on Windows: any spelling of a name that is there."""
                at = Path(path.anchor)
                for part in path.parts[1:]:
                    names = {entry.name.lower(): entry.name for entry in at.iterdir()} if real_exists(at) and at.is_dir() else {}
                    if part.lower() not in names:
                        return False
                    at = at / names[part.lower()]
                return True

            for exists in (real_exists, windows_exists):
                with mock.patch.object(Path, "exists", exists):
                    self.assertEqual(zero_addon.source_name(zero_addon.map_archive("tat3", game), game),
                                     "AddOn/Tat3/Data/_lvl_pc/TAT/tat3.lvl")
                    self.assertEqual(zero_addon.source_name(zero_addon.side_archive("gam", game), game),
                                     "AddOn/Tat3/Data/_lvl_pc/Side/gam.lvl")
                    self.assertEqual(zero_addon.child(root, "side", "GAM.LVL"), root / "Side" / "gam.lvl")
                    self.assertIsNone(zero_addon.child(root, "SIDE", "rep.lvl"))


if __name__ == "__main__":
    unittest.main()
