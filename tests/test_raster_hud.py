import json
from pathlib import Path
import sys
import tempfile
import unittest

from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from upscale_hud import convert, enlarge


class RasterHudTests(unittest.TestCase):
    def test_transparent_rgb_does_not_fringe(self):
        image = Image.new("RGBA", (8, 8), (255, 0, 0, 0))
        for y in range(2, 6):
            for x in range(2, 6):
                image.putpixel((x, y), (255, 255, 255, 128))
        result = enlarge(image, 4)
        pixels = list(result.get_flattened_data() if hasattr(result, "get_flattened_data") else result.getdata())
        self.assertTrue(any(0 < p[3] < 128 for p in pixels))
        self.assertTrue(all(max(p[:3]) - min(p[:3]) <= 1 for p in pixels if p[3]))

    def test_lossless_art_and_source_preservation(self):
        with tempfile.TemporaryDirectory() as folder:
            common = Path(folder)
            image = Image.new("RGBA", (8, 8), (72, 132, 201, 127))
            image.save(common / "hud.png")
            original = (common / "hud.png").read_bytes()
            metadata = {"textures": {"hud1": "hud.png", "wall": "missing.png"}, "fonts": {}}
            (common / "common.json").write_text(json.dumps(metadata))
            manifest = convert(common)
            self.assertEqual((common / "hud.png").read_bytes(), original)
            self.assertEqual(json.loads((common / "common.json").read_text()), metadata)
            entry = manifest["textures"]["hud.png"]
            with Image.open(common / "raster-hud" / entry["file"]) as result:
                self.assertEqual(result.size, (32, 32))
                self.assertEqual(result.convert("RGBA").tobytes(), enlarge(image, 4).tobytes())
            self.assertEqual(len(manifest["textures"]), 1)

    def test_listed_art_is_enlarged_beside_the_name_rule(self):
        with tempfile.TemporaryDirectory() as folder:
            common = Path(folder)
            for name in ("hud1", "border_3_pieces", "com_icon_controlzone", "wall", "gamefont_tiny_tex0"):
                Image.new("RGBA", (4, 2), (10, 20, 30, 40)).save(common / f"{name}.png")
            metadata = {
                "textures": {name: f"{name}.png" for name in ("hud1", "border_3_pieces", "com_icon_controlzone", "wall", "gamefont_tiny_tex0")},
                "fonts": {"gamefont_tiny": {"pages": ["gamefont_tiny_tex0"]}},
                "rasterHud": ["border_3_pieces", "com_icon_controlzone", "absent"],
            }
            (common / "common.json").write_text(json.dumps(metadata))
            manifest = convert(common)
            self.assertEqual(sorted(manifest["textures"]),
                             ["border_3_pieces.png", "com_icon_controlzone.png", "gamefont_tiny_tex0.png", "hud1.png"])
            entry = manifest["textures"]["border_3_pieces.png"]
            self.assertEqual((entry["width"], entry["height"]), (4, 2))
            with Image.open(common / "raster-hud" / entry["file"]) as result:
                self.assertEqual(result.size, (16, 8))


if __name__ == "__main__":
    unittest.main()
