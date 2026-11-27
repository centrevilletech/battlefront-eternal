"""Read back a prepared panel through the native client's scenery renderer."""
import json
import os
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest


@unittest.skipUnless(os.environ.get("BFE_TEST_CLIENT"), "set BFE_TEST_CLIENT to a native client with a display")
class PlacedVertexLight(unittest.TestCase):
    def test_authored_light_and_tint_reach_the_panel(self):
        from PIL import Image

        client = Path(os.environ["BFE_TEST_CLIENT"]).resolve()
        tools = Path(__file__).resolve().parents[1] / "tools"
        fixture = runpy.run_path(str(tools / "renderer-smoke-native.py"))
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            root = work / "swbf2" / ".content"
            fixture["write_synthetic_content"](root)
            room = root / "maps" / "room"
            scene_file = room / "map.json"
            scene = json.loads(scene_file.read_text())
            for feature in ("foliage", "water"):
                scene.pop(feature)
            scene_file.write_text(json.dumps(scene))
            material = room / "props.json"
            props = json.loads(material.read_text())
            part = props["models"]["banner_panel"]["parts"][0]
            part["flags"] |= 64
            props["attributesFile"] = "colours.bin"
            count = part["vertexCount"]
            light = [0.75, 0.5, 0.125, 1.0] * count
            tint = [0.25, 1.0, 0.5, 1.0] * count
            (room / "colours.bin").write_bytes(fixture["pack_f32"](light + tint))
            (room / "prop.png").write_bytes(fixture["encode_png_rgba"](1, 1, bytes([255] * 4)))
            camera = work / "view.json"
            camera.write_text(json.dumps({
                "position": [0, 2, 3],
                "projection": fixture["perspective_webgl"](60, 1, 0.1, 100),
                "inverse": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, -2, -3, 1],
            }))
            environment = dict(os.environ, SDL_AUDIODRIVER="dummy",
                               BFE_USER_DATA_DIR=str(work / "user"), BFE_STATE_DIR=str(work / "state"),
                               BFE_BENCH=f"world=swbf2/room&view={camera}&time=0")
            pixels = []
            for i, streams in enumerate([{}, {"lit": 0}, {"lit": 0, "tint": count * 16}]):
                part["vertexStreams"] = streams
                material.write_text(json.dumps(props))
                shots = work / str(i)
                shots.mkdir()
                run = subprocess.run([str(client), "--content-dir", str(root), "--shots-dir", str(shots),
                                      "--width", "480", "--height", "480", "--no-audio", "--skip-update-check"],
                                     env=environment, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                     text=True, timeout=45)
                self.assertEqual(run.returncode, 0, run.stdout)
                with Image.open(shots / "world.png") as frame:
                    pixels.append(frame.convert("RGB").getpixel((frame.width // 2, frame.height // 2)))
            ordinary, baked, tinted = pixels
            self.assertNotEqual(ordinary, baked)
            self.assertGreater(baked[0], baked[1] + 20)
            self.assertGreater(baked[1], baked[2] + 20)
            self.assertLess(tinted[0], baked[0] // 2)
            self.assertAlmostEqual(tinted[1], baked[1], delta=2)
            self.assertLess(tinted[2], baked[2] - 5)


if __name__ == "__main__":
    unittest.main()
