"""tools/prepare_check.py, the look at an installation before anything is
converted (tools/prepare.py --check), and the run around it: an add-on's
map that can't be prepared is left out while the game's own content
completes, the Enhanced sound can be left out by choice, and the game's own
failures say what failed. A synthetic installation: the archives' headers,
scripts and text are made here, nothing retail."""
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
import prepare  # noqa: E402
import prepare_check  # noqa: E402
import zero_addon  # noqa: E402
from test_addon_import import addme, calls_chunk, core, lua_chunk, op, scripts, ucfb, write  # noqa: E402

ARCHIVE = ucfb()  # an empty archive: the header the check looks for


def missionlist(*entries):
    """shell.lvl's missionlist script: sp_missionselect_listbox_contents as
    a table of {mapluafile, showstr, side_<letter>} entries."""
    strings = []

    def k(text):
        if text not in strings:
            strings.append(text)
        return strings.index(text)

    code = [op("CREATETABLE", u=len(entries))]
    for map_id, show, letters in entries:
        code += [op("CREATETABLE", u=2 + len(letters)), op("PUSHSTRING", u=k("mapluafile")), op("PUSHSTRING", u=k(map_id)),
                 op("PUSHSTRING", u=k("showstr")), op("PUSHSTRING", u=k(show))]
        for letter in letters:
            code += [op("PUSHSTRING", u=k(f"side_{letter}")), op("PUSHINT", s=1)]
        code.append(op("SETMAP", u=2 + len(letters)))
    code += [op("SETLIST", a=0, b=len(entries)), op("SETGLOBAL", u=k("sp_missionselect_listbox_contents")), op("END")]
    return scripts(missionlist=lua_chunk(strings, code))


def battle(world, sound, *sides):
    """A battle's script: its sound bank, armies and world, read as the
    game's are."""
    return calls_chunk(("ReadDataFile", sound), *(("ReadDataFile", side) for side in sides),
                       ("SetTeamName", 1, "Republic"), ("SetTeamName", 2, "CIS"), ("ReadDataFile", world))


class Install:
    """The game with one map, Geonosis (geo1: the Republic and the
    Geonosians), and what the tools read beside it."""

    def __init__(self, root):
        self.root = Path(root)
        self.data = self.root / "GameData"
        self.game = self.data / "Data" / "_LVL_PC"
        write(self.data / "Battlefront.exe", b"MZ" + bytes(62))
        write(self.game / "mission.lvl", scripts(geo1r=battle("GEO\\geo1.lvl", "sound\\geo.lvl;geo1cw", "SIDE\\rep.lvl", "SIDE\\geo.lvl")))
        write(self.game / "core.lvl", core(planets__geonosis__mapname1="GEONOSIS: SPIRE"))
        write(self.game / "Shell" / "ENG" / "shell.lvl", missionlist(("GEO1", "planets.geonosis.mapname1", "rc")))
        for name in ("Common/ENG/common.lvl", "Load/load.lvl", "Sound/shell.lvl", "Sound/common.bnk", "Movies/shell.mvs",
                     "Sound/geo.lvl", "SIDE/rep.lvl", "SIDE/geo.lvl", "GEO/geo1.lvl"):
            write(self.game / name, ARCHIVE)

    def add_on(self, folder, map_id, show, army, files=("world", "army", "sound")):
        """An add-on with one map fielding the Republic and an army of its
        own, with the files named in files."""
        root = self.data / "AddOn" / folder
        write(root / "addme.script", addme(map_id.upper(), show, "r"))
        lvl = root / "Data" / "_LVL_PC"
        planet = map_id[:3].upper()
        write(lvl / "mission.lvl", scripts(**{f"{map_id}r": battle(f"dc:{planet}\\{map_id}.lvl", f"dc:sound\\{map_id[:3]}.lvl;{map_id}cw",
                                                                  "SIDE\\rep.lvl", f"dc:SIDE\\{army}.lvl")}))
        named = {"world": lvl / planet / f"{map_id}.lvl", "army": lvl / "Side" / f"{army}.lvl", "sound": lvl / "sound" / f"{map_id[:3]}.lvl"}
        for part in files:
            write(named[part], ARCHIVE)
        return root


def table_found(*_):
    return (0.0,) * 256


class CheckTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.install = Install(Path(self.temp.name) / "Star Wars Battlefront")
        self.game = self.install.game
        patcher = mock.patch("zero_foliage.placement_table", table_found)
        patcher.start()
        self.addCleanup(patcher.stop)

    def tearDown(self):
        self.temp.cleanup()

    def check(self):
        return prepare_check.check(self.game)

    def problems(self):
        return [(entry["code"], entry["path"]) for entry in self.check()["problems"]]

    def test_a_whole_copy_passes_and_names_its_maps(self):
        report = self.check()
        self.assertEqual(report["problems"], [])
        self.assertTrue(report["ok"])
        self.assertEqual(report["maps"], ["geo1"])
        self.assertEqual(report["game"], str(self.install.root))
        self.assertEqual(report["addOns"], [])

    def test_a_missing_army_world_or_sound_is_named_from_GameData_on(self):
        (self.game / "SIDE" / "geo.lvl").unlink()
        (self.game / "Sound" / "geo.lvl").unlink()
        self.assertEqual(self.problems(), [("missing", "GameData/Data/_LVL_PC/SIDE/geo.lvl"), ("missing", "GameData/Data/_LVL_PC/Sound/geo.lvl")])
        self.assertFalse(self.check()["ok"])
        (self.game / "GEO" / "geo1.lvl").unlink()
        self.assertIn(("missing", "GameData/Data/_LVL_PC/GEO/geo1.lvl"), self.problems())

    def test_empty_and_damaged_archives(self):
        write(self.game / "core.lvl", b"")
        write(self.game / "Movies" / "shell.mvs", b"not an archive")
        found = {entry["path"]: entry for entry in self.check()["problems"]}
        self.assertEqual(found["GameData/Data/_LVL_PC/core.lvl"]["code"], "empty")
        self.assertEqual(found["GameData/Data/_LVL_PC/Movies/shell.mvs"]["code"], "damaged")
        self.assertIn("isn't a Star Wars Battlefront archive", found["GameData/Data/_LVL_PC/Movies/shell.mvs"]["message"])

    @unittest.skipIf(os.path.normcase("A") == os.path.normcase("a") or Path(__file__.upper()).exists(),
                     "the file system matches names without regard to case")
    def test_a_folder_spelled_otherwise_is_the_one_problem_on_linux(self):
        (self.game / "SIDE").rename(self.game / "Side")
        found = self.check()["problems"]
        self.assertEqual([(entry["code"], entry["path"]) for entry in found], [("misnamed", "GameData/Data/_LVL_PC/SIDE")],
                         "the armies below it aren't reported again")
        self.assertIn("is spelled Side in this copy, where it must be SIDE", found[0]["message"])
        self.game.rename(self.game.parent / "_lvl_pc")
        self.assertEqual(self.problems(), [("misnamed", "GameData/Data/_LVL_PC")], "a missing folder stops the check")

    def test_a_copy_without_the_english_files(self):
        for folder in ("Common", "Shell"):
            (self.game / folder / "ENG").rename(self.game / folder / "GER")
        found = self.check()["problems"]
        self.assertEqual([entry["code"] for entry in found], ["language"])
        self.assertIn("It has GER only.", found[0]["message"])

    def test_the_program_the_foliage_is_placed_from(self):
        with mock.patch("zero_foliage.placement_table", side_effect=ValueError("Unsupported")):
            found = self.check()["problems"]
        self.assertEqual([(entry["code"], entry["path"]) for entry in found], [("program", "GameData/Battlefront.exe")])
        self.assertIn("modified or replaced", found[0]["message"])
        (self.install.data / "Battlefront.exe").unlink()
        found = self.check()["problems"]
        self.assertEqual([(entry["code"], entry["path"]) for entry in found], [("program", "GameData/Battlefront.exe")])
        self.assertIn("is missing", found[0]["message"])

    def test_add_ons_are_reported_and_never_fail_the_check(self):
        self.install.add_on("Abc", "abc1", "ABC: GOOD", "abc")
        self.install.add_on("Brk", "brk1", "BRK: NO ARMY", "brk", files=("world", "sound"))
        junk = self.install.data / "AddOn" / "Junk"
        write(junk / "addme.script", b"not a script")
        write(junk / "Data" / "_LVL_PC" / "mission.lvl", ARCHIVE)
        report = self.check()
        self.assertTrue(report["ok"], report["problems"])
        found = {entry["name"]: entry for entry in report["addOns"]}
        self.assertEqual(sorted(found), ["abc", "brk", "junk"])
        self.assertEqual(found["abc"]["maps"], [{"id": "abc1", "name": "ABC: GOOD", "problems": []}])
        self.assertEqual(found["abc"]["folder"], "Abc")
        brk = found["brk"]["maps"][0]
        self.assertEqual(found["brk"]["problems"], [])
        self.assertEqual([entry["message"] for entry in brk["problems"]], ["It needs an army, SIDE/brk.lvl, which isn't in the add-on or the game."])
        self.assertEqual([entry["code"] for entry in found["junk"]["problems"]], ["damaged"], "its own script can't be read: all of it")
        self.assertEqual(found["junk"]["maps"], [])

    def test_the_command_reports_one_check_event_and_writes_nothing(self):
        out = Path(self.temp.name) / "out"
        env = {**os.environ, "BFE_GAME_DIR": str(self.install.root), "BFE_CONTENT_DIR": str(out), "PYTHONDONTWRITEBYTECODE": "1"}
        result = subprocess.run([sys.executable, "-u", str(HERE.parent / "tools" / "prepare.py"), "--check", "--jobs", "4"],
                                env=env, capture_output=True, text=True, timeout=120)
        events = [json.loads(line[2:]) for line in result.stdout.splitlines() if line.startswith("@@")]
        self.assertEqual([event["event"] for event in events], ["check"], result.stdout + result.stderr)
        # The stand-in program has no foliage table: the one problem.
        self.assertEqual([entry["code"] for entry in events[0]["problems"]], ["program"])
        self.assertEqual(result.returncode, 1)
        self.assertGreaterEqual(events[0]["workers"], 1)
        self.assertFalse(out.exists(), "a check writes nothing")


class RunTests(unittest.TestCase):
    """prepare.main() over the synthetic installation, the tools stood in
    for: each writes what the next steps read, or fails as told."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.install = Install(Path(self.temp.name) / "Star Wars Battlefront")
        self.install.add_on("Abc", "abc1", "ABC: GOOD", "abc")
        self.install.add_on("Bad", "bad1", "BAD: BROKEN ARMY", "bad")
        self.out = Path(self.temp.name) / "out"
        self.failing = set()  # steps whose tool fails
        self.ran = []
        self.events = []
        for name, value in (("GAME", self.install.game), ("PUBLIC", self.out), ("run_tool", self.tool), ("emit", self.emit),
                            ("ffmpeg_ok", lambda: True), ("ktx_ok", lambda: True), ("has_modules", lambda *_: True)):
            patcher = mock.patch.object(prepare, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        before = os.environ.get("BFE_SKIP_ADDONS")
        self.addCleanup(lambda: os.environ.pop("BFE_SKIP_ADDONS", None) if before is None else os.environ.update(BFE_SKIP_ADDONS=before))

    def tearDown(self):
        self.temp.cleanup()

    def emit(self, kind, **fields):
        self.events.append({"event": kind, **fields})

    def tool(self, script, *args, prefix=""):
        """What each tool writes that later steps read: the catalog, an
        army's units, a map's world."""
        args = [str(arg) for arg in args]
        key = ("side:" + args[3] if args[:2] == ["--only", "side"] else "common" if args[:2] == ["--only", "common"]
               else "map:" + args[0] if script == "extract.py" else "audio:" + args[0] if script == "enhance_audio.py"
               else "catalog" if args == ["--catalog"] else script)
        self.ran.append(key)
        if key in self.failing:
            return prepare.Ran(False, ["Traceback (most recent call last):", 'File "extract.py", line 1', f"no archive for {key}"])
        if key == "catalog":
            maps = [("geo1", "GEONOSIS: SPIRE"), ("abc1", "ABC: GOOD"), ("bad1", "BAD: BROKEN ARMY")]
            catalog = {"maps": [{"id": map_id, "name": name, "extracted": (self.out / "maps" / map_id / "map.json").exists(),
                                 "eras": {"cw": {"teams": ["rep", "cis"]}}} for map_id, name in maps]}
            write(self.out / "common" / "shell" / "shell.json", json.dumps(catalog).encode())
        elif key.startswith("side:"):
            write(self.out / "sides" / key[5:] / "units.json", json.dumps({"units": {f"{key[5:]}_trooper": {}}}).encode())
        elif key.startswith("map:"):
            write(self.out / "maps" / key[4:] / "map.json", json.dumps({"meshes": [{}], "commandPosts": [{}]}).encode())
        return prepare.Ran(True)

    def run_main(self, *flags):
        self.events.clear()
        self.ran.clear()
        with mock.patch.object(sys, "argv", ["prepare.py", "--jobs", "4", *flags]):
            code = prepare.main()
        return code, json.loads((self.out / ".prepare.json").read_text())

    def statuses(self):
        return {event["key"]: event["status"] for event in self.events if event["event"] == "step" and event["status"] != "start"}

    def test_an_add_on_map_whose_army_fails_is_left_out_and_the_game_completes(self):
        self.failing = {"side:bad"}
        code, state = self.run_main()
        self.assertEqual(code, 0)
        self.assertEqual((state["complete"], state["failed"], state["skipped"]), (True, [], []))
        self.assertEqual(state["addOnFailures"], [{"addOn": "Bad", "map": "bad1", "name": "BAD: BROKEN ARMY", "part": "map",
                                                   "step": "side:bad", "reason": "no archive for side:bad"}])
        self.assertNotIn("map:bad1", state["done"], "it is tried again next time")
        self.assertFalse((self.out / "maps" / "bad1").exists(), "what it wrote is gone, so the menu doesn't offer it")
        self.assertTrue((self.out / "maps" / "abc1" / "map.json").exists())
        self.assertEqual(self.statuses()["side:bad"], "left-out")
        # The catalog was made again after the armies and maps, without it.
        self.assertLess(max(self.ran.index("side:bad"), self.ran.index("map:bad1")), self.ran.index("catalog", 1))
        shown = {entry["id"]: entry["extracted"] for entry in json.loads((self.out / "common/shell/shell.json").read_text())["maps"]}
        self.assertEqual(shown, {"geo1": True, "abc1": True, "bad1": False})
        finished = [event for event in self.events if event["event"] == "finished"][0]
        self.assertEqual(finished["failed"], [])
        self.assertEqual([entry["map"] for entry in finished["addOnFailures"]], ["bad1"])

    def test_an_add_on_map_that_comes_out_empty_is_left_out(self):
        def tool(script, *args, prefix=""):
            result = self.tool(script, *args, prefix=prefix)
            if script == "extract.py" and str(args[0]) == "bad1":
                write(self.out / "maps" / "bad1" / "map.json", json.dumps({"meshes": [], "commandPosts": []}).encode())
            return result
        with mock.patch.object(prepare, "run_tool", tool):
            _, state = self.run_main()
        self.assertEqual([(entry["map"], entry["step"]) for entry in state["addOnFailures"]], [("bad1", "map:bad1")])
        self.assertIn("came out empty", state["addOnFailures"][0]["reason"])
        self.assertEqual(state["failed"], [])

    def test_the_games_own_failure_fails_and_says_why(self):
        self.failing = {"map:geo1"}
        code, state = self.run_main()
        self.assertEqual(state["failed"], ["map:geo1"])
        self.assertEqual(state["failures"], [{"key": "map:geo1", "label": "Geonosis: Spire", "reason": "no archive for map:geo1"}])
        self.assertEqual(state["addOnFailures"], [])
        self.assertEqual(self.statuses()["map:geo1"], "failed")
        self.assertEqual(code, 0, "only the catalog, common and shell stop the tool with an error")

    def test_an_add_ons_sound_that_fails_leaves_only_its_sound_out(self):
        self.failing = {"audio:abc1cw"}
        _, state = self.run_main()
        self.assertEqual([(entry["map"], entry["part"]) for entry in state["addOnFailures"]], [("abc1", "sound")])
        self.assertIn("map:abc1", state["done"])
        self.assertTrue((self.out / "maps" / "abc1" / "map.json").exists())
        self.assertEqual(state["failed"], [])

    def test_enhanced_sound_left_out_by_choice_is_complete_and_can_be_added_later(self):
        _, state = self.run_main("--no-enhanced-sound")
        self.assertEqual((state["complete"], state["failed"], state["skipped"]), (True, [], []))
        self.assertEqual(state["extras"], {"enhancedSound": False, "soundSteps": 3})
        self.assertFalse([key for key in state["done"] if key.startswith("audio:")])
        self.assertEqual({key: status for key, status in self.statuses().items() if key.startswith("audio:")},
                         {"audio:geo1cw": "off", "audio:abc1cw": "off", "audio:bad1cw": "off"})
        _, state = self.run_main()
        self.assertEqual(sorted(self.ran), ["audio:abc1cw", "audio:bad1cw", "audio:geo1cw"], "only the sound runs")
        self.assertEqual(state["extras"], {"enhancedSound": True, "soundSteps": 0})
        self.assertEqual(state["complete"], True)
        # Turned off once the folder has it: it still has it.
        _, state = self.run_main("--no-enhanced-sound")
        self.assertEqual(self.ran, [])
        self.assertEqual(state["extras"], {"enhancedSound": True, "soundSteps": 0})

    def test_the_check_leaves_a_map_out_before_anything_runs_for_it(self):
        (self.install.data / "AddOn" / "Bad" / "Data" / "_LVL_PC" / "Side" / "bad.lvl").unlink()
        _, state = self.run_main()
        self.assertEqual([(entry["map"], entry["step"]) for entry in state["addOnFailures"]], [("bad1", "check")])
        self.assertIn("SIDE/bad.lvl", state["addOnFailures"][0]["reason"])
        self.assertNotIn("map:bad1", self.ran)
        self.assertNotIn("side:bad", self.ran)
        self.assertEqual({key: self.statuses()[key] for key in ("map:bad1", "side:bad", "audio:bad1cw")},
                         {"map:bad1": "left-out", "side:bad": "left-out", "audio:bad1cw": "left-out"})

    def test_an_add_on_whose_files_cant_be_read_is_left_out_whole(self):
        write(self.install.data / "AddOn" / "Bad" / "addme.script", b"not a script")
        _, state = self.run_main()
        self.assertEqual([(entry["addOn"], entry["part"]) for entry in state["addOnFailures"]], [("Bad", "add-on")])
        self.assertEqual(state["skippedAddOns"], ["bad"])
        self.assertEqual(state["addOns"], ["abc", "bad"], "installed is installed: the desktop app compares these")
        self.assertEqual([add_on.name for add_on in zero_addon.add_ons(self.install.game)], ["abc"], "the tools don't see it")
        # Readable again: what lists the add-ons runs again.
        write(self.install.data / "AddOn" / "Bad" / "addme.script", addme("BAD1", "BAD: BROKEN ARMY", "r"))
        _, state = self.run_main()
        self.assertEqual(state["skippedAddOns"], [])
        self.assertIn("catalog", self.ran)

    def test_a_map_prepared_again_makes_the_catalog_and_effects_again(self):
        self.failing = {"map:abc1"}
        _, state = self.run_main()
        self.assertIn("catalog-update", state["done"])
        self.failing = set()
        _, state = self.run_main()
        self.assertIn("map:abc1", self.ran)
        self.assertIn("zero_environment.py", self.ran)
        self.assertEqual(self.ran.count("catalog"), 1, "catalog-update ran again, after the map")
        self.assertTrue(json.loads((self.out / "common/shell/shell.json").read_text())["maps"][1]["extracted"])
        self.assertEqual(state["addOnFailures"], [])


if __name__ == "__main__":
    unittest.main()
