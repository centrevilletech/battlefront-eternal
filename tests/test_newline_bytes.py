"""Prepared JSON keeps LF bytes on every platform (no CRLF translation)."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import zero_hud2
import zero_sound_index as sound_index
from zero_paths import write_text_lf

TOOLS = Path(__file__).resolve().parents[1] / "tools"
# Modules whose writes land in a prepared content tree. Every text write in
# them goes through zero_paths.write_text_lf (zero_sound_index stages its
# own exclusive file, with newline set there).
PREPARED_WRITERS = [
    "extract", "extract_swbf2", "enhance_audio", "ktx2", "prepare",
    "zero_ai_classes", "zero_ai_mission", "zero_anim", "zero_class_art2",
    "zero_environment", "zero_hud2", "zero_load2", "zero_scoreboard",
    "zero_shell", "zero_sky", "zero_sound", "zero_spawn_screen",
    "zero_swbf2", "zero_terrain", "zero_vehicle",
]


class NewlineBytesTests(unittest.TestCase):
    def test_the_shared_writer_keeps_lf_bytes(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "zz.json"
            write_text_lf(path, json.dumps({"made": ["up"]}, indent=1) + "\n")
            written = path.read_bytes()
        self.assertEqual(written, b'{\n "made": [\n  "up"\n ]\n}\n')
        self.assertNotIn(b"\r", written)

    def test_hud_json_is_written_with_lf_bytes(self):
        payload = {"made": ["up", "names"], "count": 2}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            zero_hud2.write_json(root, Path("common/hud/zz.json"), payload)
            written = (root / "common/hud/zz.json").read_bytes()
        self.assertEqual(written, (json.dumps(payload, indent=1) + "\n").encode("utf-8"))
        self.assertNotIn(b"\r", written)

    def test_sound_index_is_written_with_lf_bytes(self):
        desired = {"maps": {"dune": "reed"}, "missions": {"weave": "reed"}}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "audio").mkdir()
            (root / "audio/index.json").write_text("{}")
            with (mock.patch.object(sound_index, "records", return_value=[("dune", root)]),
                  mock.patch.object(sound_index, "expected_index", return_value=dict(desired))):
                sound_index.restore(root, root)
            written = (root / "audio/index.json").read_bytes()
        self.assertEqual(written, json.dumps(desired, indent=1, sort_keys=True).encode("utf-8"))
        self.assertNotIn(b"\r", written)

    def test_every_prepared_writer_goes_through_the_shared_rule(self):
        for name in PREPARED_WRITERS:
            source = (TOOLS / f"{name}.py").read_text(encoding="utf-8")
            self.assertNotIn(".write_text(", source, name)
            self.assertTrue(
                any("from zero_paths import" in line and "write_text_lf" in line
                    for line in source.splitlines()),
                name,
            )
        enhance = (TOOLS / "enhance_audio.py").read_text(encoding="utf-8")
        self.assertIn('newline="\\n"', enhance)


if __name__ == "__main__":
    unittest.main()
