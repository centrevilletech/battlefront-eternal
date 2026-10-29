"""Generated-asset checks; run with tools/requirements-vector-hud.txt installed."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET
from PIL import Image, ImageDraw
from fontTools.ttLib import TTFont

SPEC = importlib.util.spec_from_file_location("vectorize_hud", Path(__file__).parents[1] / "tools/vectorize_hud.py")
converter = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(converter)


class VectorHudTests(unittest.TestCase):
    def test_art_keeps_alpha_and_real_paths(self):
        image = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
        ImageDraw.Draw(image).rectangle((3, 3, 12, 12), fill=(60, 160, 240, 128))
        root = ET.fromstring(converter.vector_image(image))
        self.assertEqual(root.get("viewBox"), "0 0 16 16")
        ns = {"svg": converter.NS}
        self.assertTrue(root.findall(".//svg:mask/svg:path", ns))
        self.assertTrue(root.findall(".//svg:g/svg:path", ns))
        self.assertFalse(root.findall(".//svg:image", ns))

    def test_font_metrics_counters_and_source_preservation(self):
        with tempfile.TemporaryDirectory() as directory:
            common = Path(directory)
            image = Image.new("RGBA", (16, 16), (255, 255, 255, 0))
            draw = ImageDraw.Draw(image)
            draw.rectangle((2, 2, 13, 13), fill="white")
            draw.rectangle((5, 5, 10, 10), fill=(255, 255, 255, 0))
            image.save(common / "page.png")
            original = (common / "page.png").read_bytes()
            font = {"lineHeight": 16, "baseline": 0, "pages": ["page"], "glyphs": [[79, 0, 15, 0, 16, 0, 16, 0, 0, 1, 1], [32, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0]]}
            (common / "common.json").write_text(json.dumps({"textures": {"page": "page.png"}, "fonts": {"test": font}}))
            output = common / "vector-hud"
            manifest = converter.convert(common, output, art=[])
            generated = TTFont(output / "test.ttf")
            self.assertEqual(set(generated.getBestCmap()), {32, 79})
            self.assertEqual(generated["hmtx"]["uni004F"][0], 960)
            self.assertEqual(generated["hmtx"]["uni0020"][0], 384)
            self.assertEqual(generated["glyf"]["uni004F"].numberOfContours, 2)
            self.assertEqual(manifest["fonts"]["test"]["glyphs"], font["glyphs"])
            self.assertEqual(manifest["fonts"]["test"]["outlineBaseline"], 16)
            self.assertEqual((common / "page.png").read_bytes(), original)
            atlas = ET.parse(output / "page.svg").getroot()
            self.assertEqual(atlas.get("viewBox"), "0 0 16 16")


if __name__ == "__main__":
    unittest.main()
