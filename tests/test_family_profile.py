"""tools/zero_family.py: per-game profiles (swbf1 and swbf2) and how the
preparation tools resolve install layout, shared archives, language folders
and executable checks for each family. Synthetic trees only; nothing retail."""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "tools"))
sys.path.insert(0, str(HERE))
import prepare_check  # noqa: E402
import zero_family  # noqa: E402
import zero_language  # noqa: E402
from test_addon_import import chunk, ucfb, write  # noqa: E402
from test_language import text as locl_archive  # noqa: E402


class FamilyProfileTests(unittest.TestCase):
    def test_client_and_preparer_share_the_battlefront_ii_generation(self):
        from zero_mission2 import PREPARED_VERSION
        versions = json.loads((HERE.parent / "tools/content-version.json").read_bytes())
        self.assertIs(type(versions["swbf2"]), int)
        self.assertEqual(versions["swbf2"], PREPARED_VERSION)

    def test_ambient_family_falls_back_but_explicit_profile_is_strict(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("BFE_FAMILY", None)
            self.assertEqual(zero_family.current(), "swbf1")
            self.assertFalse(zero_family.is_swbf2())
        with mock.patch.dict(os.environ, {"BFE_FAMILY": "SWBF2"}):
            self.assertEqual(zero_family.current(), "swbf2")
            self.assertTrue(zero_family.is_swbf2())
        with mock.patch.dict(os.environ, {"BFE_FAMILY": "swbf3"}):
            self.assertEqual(zero_family.current(), "swbf1")
            self.assertEqual(zero_family.profile().data_subpath, ("Data", "_LVL_PC"))
            with self.assertRaisesRegex(ValueError, "Unknown game family"):
                zero_family.profile("swbf3")

    def test_swbf1_profile_matches_existing_layout(self):
        p = zero_family.profile("swbf1")
        self.assertEqual(p.data_subpath, ("Data", "_LVL_PC"))
        self.assertEqual(p.program_name, "Battlefront.exe")
        self.assertTrue(p.has_foliage_table)
        self.assertTrue(p.language_folders)
        self.assertEqual(p.side_dir, "SIDE")
        self.assertEqual(p.map_relpath("bes2"), "BES/bes2.lvl")
        self.assertEqual(p.shared_archive_keys, ("common", "core"))
        self.assertEqual(p.lua_version, "4.0")

    def test_swbf2_profile_matches_2005_layout(self):
        p = zero_family.profile("swbf2")
        self.assertEqual(p.data_subpath, ("data", "_lvl_pc"))
        self.assertEqual(p.program_name, "BattlefrontII.exe")
        self.assertFalse(p.has_foliage_table)
        self.assertFalse(p.language_folders)
        self.assertEqual(p.side_dir, "side")
        self.assertEqual(p.map_relpath("tat2"), "tat/tat2.lvl")
        self.assertEqual(p.map_relpath("spa6"), "spa/spa6.lvl")
        self.assertEqual(p.shared_archive_keys, ("common", "core", "ingame"))
        self.assertEqual(p.lua_version, "5.0")

    def test_swbf2_language_and_check_without_language_folders(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp) / "SWBF2"
            data = root / "GameData"
            game = data / "data" / "_lvl_pc"
            write(data / "BattlefrontII.exe", b"MZ" + bytes(62))
            write(game / "core.lvl", locl_archive(english={"common.yes": "YES"}))
            for rel in (
                "common.lvl", "shell.lvl", "ingame.lvl", "inshell.lvl",
                "tat/tat2.lvl", "spa/spa6.lvl", "load/common.lvl", "sound/shell.lvl",
                "sound/common.bnk", "sound/global.lvl", "movies/shell.mvs",
            ):
                write(game / rel, ucfb())
            scripts = [chunk(b"scr_", chunk(b"NAME", name + b"\0") + chunk(b"BODY", b"\x1bLua\x50test"))
                       for name in (b"tat2g_eli", b"spa6c_ass")]
            write(game / "mission.lvl", ucfb(*scripts))
            (game / "side").mkdir(parents=True)
            self.assertEqual(zero_language.common_archive(game, family="swbf2"), game / "common.lvl")
            self.assertEqual(zero_language.shell_archive(game, family="swbf2"), game / "shell.lvl")
            self.assertEqual(zero_language.recorded(game / "common.lvl", family="swbf2"), "common.lvl")
            lang = zero_language.found(game, family="swbf2")
            self.assertEqual((lang["common"], lang["shell"], lang["text"]), (None, None, "english"))
            recorder = Path(tmp) / "recorder"
            recorder.write_text("#!/usr/bin/env python3\nimport json, sys\n"
                                "print(json.dumps({'schema': 1, 'script': sys.argv[-1], 'calls': []}))\n")
            recorder.chmod(0o755)
            with mock.patch.dict(os.environ, {"BFE_SCRIPT_TOOL": str(recorder)}):
                report = prepare_check.check(game, family="swbf2")
                self.assertTrue(report["ok"], report["problems"])
                self.assertEqual(report["counts"], {"maps": 2, "sides": 0, "sublevels": 0})
                (data / "BattlefrontII.exe").unlink()
                report = prepare_check.check(game, family="swbf2")
                self.assertEqual([p["code"] for p in report["problems"]], ["program"])
            (game / "spa/spa6.lvl").unlink()
            for mode, expected in (("absent", "BFE_SCRIPT_TOOL"), ("failure", "rc=3"), ("schema", "schema 99")):
                with self.subTest(mode=mode), mock.patch.dict(os.environ, {"BFE_SCRIPT_TOOL": str(recorder)}):
                    if mode == "absent":
                        os.environ.pop("BFE_SCRIPT_TOOL")
                    elif mode == "failure":
                        recorder.write_text("#!/usr/bin/env python3\nraise SystemExit(3)\n")
                    else:
                        recorder.write_text("#!/usr/bin/env python3\nimport json\nprint(json.dumps({'schema': 99}))\n")
                    report = prepare_check.check(game, family="swbf2")
                    self.assertFalse(report["ok"])
                    problems = report["problems"]
                    self.assertTrue(any(p["code"] == "recorder" and expected in p["message"] for p in problems))
                    self.assertTrue(any(p["code"] == "missing" and "spa6.lvl" in p["path"] for p in problems))
                    self.assertTrue(any(p["code"] == "program" for p in problems))

            recorder.write_text("#!/usr/bin/env python3\nimport time\ntime.sleep(300)\n")
            with (mock.patch.dict(os.environ, {"BFE_SCRIPT_TOOL": str(recorder)}),
                  mock.patch.object(zero_family, "RECORDER_TIMEOUT_SECONDS", 0.1)):
                problems = prepare_check.check(game, family="swbf2")["problems"]
                for mission in ("tat2g_eli", "spa6c_ass"):
                    self.assertTrue(any(p["code"] == "recorder" and "timed out" in p["message"]
                                        and mission in p["message"] for p in problems))
                self.assertTrue(any(p["code"] == "missing" and "spa6.lvl" in p["path"] for p in problems))
                from zero_mission2 import prepare_opening
                with (mock.patch("zero_paths.PUBLIC", Path(tmp) / "output"),
                      mock.patch("prepare.emit") as emit):
                    self.assertEqual(prepare_opening(None), 1)
                    self.assertIn("timed out after 0.1 seconds for tat2g_eli", emit.call_args.kwargs["message"])

    def test_zero_paths_respects_bfe_family_subprocess(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = {
                **os.environ,
                "BFE_FAMILY": "swbf2",
                "BFE_GAME_DIR": tmp,
                "BFE_CONTENT_DIR": str(Path(tmp) / "out"),
                "PYTHONDONTWRITEBYTECODE": "1",
            }
            code = "import sys; sys.path.insert(0, 'tools'); import zero_paths; print(zero_paths.FAMILY); print(zero_paths.GAME); print(zero_paths.PUBLIC)"
            run = subprocess.run([sys.executable, "-c", code], cwd=HERE.parent, env=env, capture_output=True, text=True, check=True)
            lines = run.stdout.strip().splitlines()
            self.assertEqual(lines[0], "swbf2")
            self.assertEqual(Path(lines[1]), Path(tmp).resolve() / "GameData" / "data" / "_lvl_pc")
            self.assertEqual(Path(lines[2]), Path(tmp) / "out")

    def test_relative_2005_paths_are_absolute_before_helpers_change_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            cwd = Path(tmp).resolve()
            env = dict(os.environ, BFE_FAMILY="swbf2", BFE_GAME_DIR="copy", BFE_CONTENT_DIR="out",
                       PYTHONPATH=str(HERE.parent / "tools"))
            code = ("import json, os, zero_paths; "
                    "print(json.dumps([os.environ['BFE_GAME_DIR'], os.environ['BFE_CONTENT_DIR']]))")
            result = subprocess.check_output([sys.executable, "-c", code], cwd=cwd, env=env, text=True)
            self.assertEqual(json.loads(result), [str(cwd / "copy"), str(cwd / "out")])

    def test_read_mission_manifest_calls_script_tool(self):
        with tempfile.TemporaryDirectory() as tmp:
            tool = Path(tmp) / "mock_script_tool.py"
            tool.write_text("#!/usr/bin/env python3\nimport json, sys\nprint(json.dumps({'schema': 1, 'script': sys.argv[-1], 'calls': [], 'argv': sys.argv[1:]}))\n")
            tool.chmod(0o755)
            out = zero_family.read_mission_manifest(Path(tmp) / "mission.lvl", "tat2g_eli", tool=tool)
            self.assertEqual(out["argv"], ["manifest", str(Path(tmp) / "mission.lvl"), "tat2g_eli"])
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("BFE_SCRIPT_TOOL", None)
            with self.assertRaises(FileNotFoundError):
                zero_family.read_mission_manifest("mission.lvl", "tat2g_eli")

    def test_manifest_versions_share_asset_requests_and_preserve_lossless_values(self):
        from zero_mission2 import data_files
        call = {"name": "ReadDataFile", "args": ["SIDE/example.lvl", "unit_é"]}
        old = {"schema": 1, "script": "entry", "calls": [call]}
        raw = {"$bfe": "bytes", "hex": "756e69745fe9"}
        typed = {"$bfe": "table", "entries": [[1, "a"], ["1", "b"], ["$bfe", "cycle"]]}
        new = {"schema": 2, "profile": "pc-fresh", "unresolved_queries": [], "script": "entry",
               "calls": [{"name": "ReadDataFile", "args": [call["args"][0], raw]},
                         {"name": "Observe", "args": [{"$bfe": "number", "special": "-0"}, typed]}]}
        result = zero_family.preparation_manifest(new)
        self.assertEqual(list(data_files(result)), list(data_files(zero_family.preparation_manifest(old))))
        self.assertEqual(result["calls"][0]["wireArgs"][1], raw)
        self.assertEqual(json.dumps(result["calls"][1]["args"][0]), "-0.0")
        self.assertEqual(result["calls"][1]["args"][1], typed)
        self.assertEqual(zero_family.preparation_manifest(result), result)
        for version in (3, 4):
            diagnostics = {"probed_globals": ["optional"]}
            if version == 4:
                diagnostics["skipped_scripts"] = [{"script": "example", "error": "invalid chunk"}]
            projected = zero_family.preparation_manifest({**new, "schema": version, **diagnostics})
            self.assertEqual(projected["calls"], result["calls"])
            self.assertEqual({key: projected[key] for key in diagnostics}, diagnostics)
            for key in diagnostics:
                with self.assertRaisesRegex(ValueError, key):
                    zero_family.preparation_manifest({k: v for k, v in projected.items() if k != key})
        for bad in (None, True, 0, 5, "2"):
            with self.subTest(bad=bad), self.assertRaisesRegex(ValueError, "expected 1 through 4"):
                zero_family.preparation_manifest({**new, "schema": bad})


if __name__ == "__main__":
    unittest.main()
