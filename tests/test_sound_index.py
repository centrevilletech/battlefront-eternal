"""Readers of the live sound index always see a published complete generation."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import zero_sound_index as index


class IndexTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "POSIX create permissions")
    def test_resuming_an_index_write_uses_the_current_creation_mode(self):
        import stat
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            audio = root / "audio"
            audio.mkdir()
            staged = audio / ".index-rebuild"
            desired = {"maps": {"dune": "reed"}, "missions": {"weave": "reed"}}
            for mask, stale_mode in ((0o022, 0o600), (0o077, 0o644)):
                (audio / "index.json").write_text("{}")
                staged.touch()
                staged.chmod(stale_mode)
                original = os.umask(mask)
                try:
                    with (mock.patch.object(index, "records", return_value=[("dune", root)]),
                          mock.patch.object(index, "expected_index", return_value=desired.copy())):
                        index.restore(root, root)
                finally:
                    os.umask(original)
                self.assertEqual(stat.S_IMODE((audio / "index.json").stat().st_mode), 0o666 & ~mask)

    def test_absent_addon_does_not_block_other_maps_or_replace_its_old_entries(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "audio").mkdir()
            (root / ".prepare.json").write_text(json.dumps({"done": []}))
            for map_id, keys in (("aisle", ["aislea", "aisleb"]), ("yard", ["yardc"])):
                folder = root / "maps" / map_id
                folder.mkdir(parents=True)
                (folder / "map.json").write_text(json.dumps({"missions": {k: {"mission": {}} for k in keys}}))
            live = root / "audio/index.json"
            live.write_text(json.dumps({"maps": {"aisle": "old"}, "missions": {"aislea": "old"}}))
            with (mock.patch.object(index, "missions", return_value={"aislea": "x", "yardc": "y"}),
                  mock.patch.object(index, "load", side_effect=lambda x: x),
                  mock.patch.object(index, "calls", side_effect=lambda x: x),
                  mock.patch.object(index, "read_mission_audio", return_value={"banks": ["custom;new"]}),
                  mock.patch("builtins.print") as log):
                index.restore(root, root)
            self.assertEqual(json.loads(live.read_bytes()), {
                "maps": {"aisle": "old", "yard": "new"}, "missions": {"aislea": "old", "yardc": "new"}})
            self.assertEqual(sum("aisle" in str(line) for line in log.call_args_list), 1)

    def test_an_interrupted_world_record_can_use_the_recorded_install(self):
        import extract
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / "map.json"
            path.write_bytes(b'{"missions":')
            with (mock.patch.object(index, "missions", return_value={"looma": "sample"}),
                  mock.patch.object(extract, "mission_world", return_value=None),
                  mock.patch.object(index, "load", side_effect=lambda x: x),
                  mock.patch.object(index, "calls", side_effect=lambda x: x),
                  mock.patch.object(index, "read_mission_audio", return_value={"banks": ["custom;wind"]})):
                self.assertEqual(index.expected_index([("loom", path)], root),
                                 {"maps": {"loom": "wind"}, "missions": {"looma": "wind"}})

    def test_interrupt_every_map_and_publication_boundary_then_resume(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            audio = root / "audio"
            audio.mkdir()
            live = audio / "index.json"
            stages = ["loom", "spool", "reed"]
            (root / ".prepare.json").write_text(json.dumps({"done": ["map:" + n for n in stages]}))
            for name in stages:
                directory = root / "maps" / name
                directory.mkdir(parents=True)
                (directory / "map.json").write_text(json.dumps({"missions": {name: {"mission": {}}}}))
            mission = {key: key for key in stages}
            audio_setup = lambda key: {"banks": ["custom;" + key]}
            real_replace = Path.replace
            partial = {"maps": {"loom": "loom"}, "missions": {"loom": "loom"}}
            for cut in range(len(stages) + 2):
                with self.subTest(cut=cut):
                    seen = []
                    live.write_text(json.dumps(partial))
                    def before_read(key):
                        self.assertEqual(json.loads(live.read_bytes()), partial)
                        if len(seen) == cut:
                            raise KeyboardInterrupt()
                        seen.append(key)
                        return audio_setup(key)
                    def publish(path, target):
                        if cut == len(stages):
                            raise KeyboardInterrupt()
                        real_replace(path, target)
                        raise KeyboardInterrupt()
                    with (mock.patch.object(index, "missions", return_value=mission),
                          mock.patch.object(index, "load", side_effect=lambda x: x),
                          mock.patch.object(index, "calls", side_effect=lambda x: x),
                          mock.patch.object(index, "read_mission_audio", side_effect=before_read),
                          mock.patch.object(Path, "replace", publish), self.assertRaises(KeyboardInterrupt)):
                        index.restore(root, root)
                    self.assertIn(json.loads(live.read_bytes()), (partial, {"missions": mission, "maps": mission}))
                    with (mock.patch.object(index, "missions", return_value=mission),
                          mock.patch.object(index, "load", side_effect=lambda x: x),
                          mock.patch.object(index, "calls", side_effect=lambda x: x),
                          mock.patch.object(index, "read_mission_audio", side_effect=audio_setup)):
                        index.restore(root, root)
                        stamp = live.stat().st_mtime_ns
                        index.restore(root, root)
                        self.assertEqual(live.stat().st_mtime_ns, stamp)
                    self.assertEqual(json.loads(live.read_bytes()), {"missions": mission, "maps": mission})
                    self.assertFalse((audio / ".index-rebuild").exists())
