"""The fixtures/mission-scripts sources use Lua 5.0.3 with 32-bit floats and sizes.
Rebuild each fixture in that directory with `luac -s -o name.luac name.lua`.
"""

import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "tools"))
from test_addon_import import chunk, ucfb
from zero_family import read_mission_manifest
from zero_scripts2 import script_chain, script_index


def body(name):
    return (HERE / "fixtures" / "mission-scripts" / (name + ".luac")).read_bytes()


def script(name, data):
    return chunk(b"scr_", chunk(b"NAME", name.encode() + b"\0") + chunk(b"BODY", data))


def archive():
    woven = script("Weft", body("weft"))
    package = chunk(b"lvl_", bytes(8) + ucfb(woven, script("Bobbin", body("bobbin"))))
    return ucfb(script("loomc_spin", body("loom")), package, script("WEFT", body("loom")),
                script("Spare", body("bobbin")))


def recording():
    calls = ["WeFt", "Bobbin", "AbsentThread", "WEFT", "Bobbin", "Spare"]
    return {"script": "loomc_spin", "calls": [{"name": "ScriptCB_DoFile", "args": [n]} for n in calls],
            "loaded_scripts": ["loomc_spin", "Weft", "Bobbin", "Weft", "Bobbin", "Spare"]}


def write_effect_pack(directory):
    from zero_fx2 import PACK_VERSION
    pack = directory / "maps/loom/effects/loomc_spin/effects.json"
    pack.parent.mkdir(parents=True, exist_ok=True)
    tables = {k: {} for k in ("effects", "models", "textures", "bindings", "mounts", "links", "rounds", "beams")}
    pack.write_text(json.dumps({"version": PACK_VERSION, **tables, "hdr": None}))
    return True


def empty_map_art(directory):
    from zero_class_art2 import write_map_art
    write_map_art(directory / "hud", {}, {}, directory)
def first_person_files(out):
    root = out / "firstperson"
    root.mkdir()
    (root / "manifest.json").write_text(json.dumps({"version": 2, "animsData": "anims.bin", "rigs": {}, "textures": {}}))
    (root / "anims.bin").write_bytes(b"reed")
    return True


class ScriptChainTests(unittest.TestCase):
    def test_order_case_shared_bytes_and_missing_request(self):
        chain = script_chain("loom", recording(), script_index(archive()))
        records = chain.record()["scripts"]
        self.assertEqual([r["name"] for r in records],
                         ["loomc_spin", "WeFt", "Bobbin", "AbsentThread", "WEFT", "Bobbin", "Spare"])
        self.assertEqual(chain.manifest["chunks"], ["loomc_spin", "weft", "bobbin", "weft", "bobbin", "spare"])
        self.assertEqual(chain.manifest["entry"], "loomc_spin")
        self.assertEqual((chain.manifest["era"], chain.manifest["mode"]), ("cw", "spin"))
        self.assertEqual(chain.path, "maps/loom/scripts/cw-spin.json")
        self.assertIn("AbsentThread", records[3]["error"])
        self.assertEqual([records[3][key] for key in ("file", "length", "sha256")], [None] * 3)
        expected = {name: body(name) for name in ("loom", "weft", "bobbin")}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            chain.write(root)
            self.assertEqual(json.loads((root / chain.path).read_bytes()), chain.manifest)
            self.assertEqual(len(list((root / "scripts").iterdir())), 3)
            for index, fixture in ((0, "loom"), (1, "weft"), (2, "bobbin"), (4, "weft"), (5, "bobbin"), (6, "bobbin")):
                row, data = records[index], expected[fixture]
                digest = hashlib.sha256(data).hexdigest()
                self.assertEqual((row["archive"], row["length"], row["sha256"]), ("mission.lvl", len(data), digest))
                self.assertEqual(row["file"], f"scripts/{digest}.luac")
                self.assertEqual((root / row["file"]).read_bytes(), data)

    def test_a_recorded_load_missing_from_the_archive_remains_in_the_record(self):
        chain = script_chain("loom", {"script": "loomc_spin", "loaded_scripts": ["loomc_spin", "Weft"],
                                      "calls": []}, {})
        self.assertEqual(chain.manifest["chunks"], [])
        self.assertEqual([r["name"] for r in chain.record()["scripts"]], ["loomc_spin", "Weft"])
        self.assertTrue(all("error" in row for row in chain.record()["scripts"]))
        with self.assertRaisesRegex(ValueError, "absent from the ordered requests.*bobbin"):
            script_chain("loom", {**recording(), "loaded_scripts": ["loomc_spin", "bobbin", "bobbin", "bobbin"]},
                         script_index(archive()))

    def test_preparation_resumes_and_repairs_changed_bodies_and_records(self):
        from zero_mission2 import PREPARED_VERSION, prepare_opening
        from zero_vehicle2 import write_pack
        with tempfile.TemporaryDirectory() as directory:
            game = Path(directory)
            root = game / "content"
            source = game / "mission.lvl"
            source.write_bytes(archive())
            world = root / "maps/loom/map.json"
            ran = []

            def extract(tool, *args, env=None):
                if tool == "enhance_audio.py":
                    ran.append(("audio", args[0]))
                    enhanced = root / "audio" / "enhanced" / args[0]
                    enhanced.mkdir(parents=True, exist_ok=True)
                    (enhanced / "manifest.json").write_text("{}")
                    return True
                out = Path(env["BFE_CONTENT_DIR"])
                if tool == "zero_fp2.py":
                    return first_person_files(out)
                if tool == "zero_fx2.py":
                    self.assertNotEqual(out, root)
                    return write_effect_pack(out)
                map_file = out / "maps/loom/map.json"
                stage = args[args.index("--only") + 1]
                ran.append(stage)
                if stage == "map":
                    chain = script_chain("loom", recording(), script_index(source.read_bytes()))
                    chain.write(out)
                    (map_file.parent / "geom.bin").write_bytes(b"triangles")
                    (out / "audio").mkdir(exist_ok=True)
                    (out / "audio/index.json").write_text("{}")
                    bank = out / "audio/fabric"
                    bank.mkdir(exist_ok=True)
                    (bank / "sound.json").write_text('{"streams": [], "samples": {}}')
                    (bank / "bank.bin").touch()
                    vehicle_files = write_pack(map_file.parent, {"classes": {}, "models": {}})
                    empty_map_art(map_file.parent)
                    map_file.write_text(json.dumps({"missions": {"loomc_spin": chain.record()}, **vehicle_files,
                                                 "barriers": [], "hints": [], "layers": {"reed": {"barriers": [], "hints": []}}}))
                elif stage == "common":
                    (out / "common").mkdir(parents=True, exist_ok=True)
                    (out / "common/common.json").write_text("{}")
                return True

            with (mock.patch("zero_mission2.GAME", game), mock.patch("zero_paths.PUBLIC", root),
                  mock.patch("zero_mission2.OPENING_MISSIONS", {"loom": "loomc_spin"}),
                  mock.patch("zero_mission2.opening_manifest", return_value=recording()) as recorder,
                  mock.patch("prepare.run_tool", side_effect=extract), mock.patch("prepare.maintain_sound"),
                  mock.patch("prepare.emit")):
                def run(**options):
                    ran.clear()
                    args = {"jobs": 1, "core_only": False, "no_enhanced_sound": False, **options}
                    self.assertEqual(prepare_opening(SimpleNamespace(**args)), 0)
                    return list(ran)

                self.assertEqual(run(), ["map", "common"])
                receipt = root / ".prepare-swbf2.json"
                data = json.loads(receipt.read_bytes())
                self.assertEqual([data[k] for k in ("family", "swbf2Version", "complete", "failed", "skipped", "addOns")],
                                 ["swbf2", PREPARED_VERSION, True, [], [], []])
                old = root / ".prepare.json"
                self.assertFalse(old.exists())
                # The early tools used the released name and omitted skipped.
                # A current released receipt with added 2005 fields is foreign.
                data.pop("skipped")
                data["swbf2Version"] = 3
                old.write_text(json.dumps(data))
                receipt.unlink()
                self.assertEqual(run(), ["map", "common"])
                self.assertFalse(old.exists())
                self.assertEqual(run(), [])
                with (mock.patch("prepare.has_modules", return_value=True), mock.patch("prepare.ffmpeg_ok", return_value=True)):
                    recorder.return_value["calls"].append({"name": "ReadDataFile", "args": ["sound/loom.lvl;fabric"]})
                    self.assertEqual(run(no_enhanced_sound=True), ["map", "common"])
                    self.assertEqual(json.loads(receipt.read_bytes())["extras"], {"enhancedSound": False, "soundSteps": 1})
                    self.assertEqual(run(core_only=True), [])
                    self.assertFalse(json.loads(receipt.read_bytes())["complete"])
                    self.assertEqual(run(), [("audio", "fabric")])
                    self.assertEqual(json.loads(receipt.read_bytes())["extras"], {"enhancedSound": True, "soundSteps": 0})
                    self.assertEqual(run(), [])
                    for version in (6, 7):
                        saved = json.loads(receipt.read_bytes())
                        saved["swbf2Version"] = version
                        receipt.write_text(json.dumps(saved))
                        self.assertEqual(run(), ["map"])
                        self.assertEqual(run(), [])
                    saved = json.loads(world.read_bytes())
                    del saved["layers"]["reed"]["hints"]
                    world.write_text(json.dumps(saved))
                    self.assertEqual(run(), ["map"])
                    (root / "audio/enhanced/fabric/manifest.json").unlink()
                    self.assertEqual(run(), [("audio", "fabric")])
                    recorder.return_value["calls"].pop()
                    self.assertEqual(run(), ["map", "common"])
                entry = json.loads(world.read_bytes())["missions"]["loomc_spin"]
                path = root / entry["scripts"][0]["file"]
                path.write_bytes(bytes(path.stat().st_size))
                self.assertEqual(run(), ["map"])
                self.assertEqual(path.read_bytes(), body("loom"))
                (root / entry["scriptManifest"]).unlink()
                self.assertEqual(run(), ["map"])
                world.write_text("{}")
                self.assertEqual(run(), ["map"])
                source.write_bytes(ucfb(script("loomc_spin", body("weft")), archive()))
                self.assertEqual(run(), ["map"])
                self.assertNotEqual(json.loads(world.read_bytes())["missions"]["loomc_spin"]["scripts"][0], entry["scripts"][0])
                self.assertEqual(run(), [])
                recorder.side_effect = RuntimeError("loomc_spin could not load LooseEnd")
                self.assertEqual(prepare_opening(SimpleNamespace(jobs=1)), 1)
                state = json.loads((root / ".prepare-swbf2.json").read_bytes())
                self.assertFalse(state["complete"])
                self.assertEqual(state["failures"], [{"id": "map:loom", "error": str(recorder.side_effect)}])
                old.write_text('{"version":2}')
                self.assertEqual(prepare_opening(SimpleNamespace(jobs=1)), 1)
                self.assertEqual(old.read_text(), '{"version":2}')

    def test_the_receipt_lists_what_each_step_wrote_and_a_resume_rebuilds_what_is_missing(self):
        from zero_mission2 import PREPARED_VERSION, prepare_opening
        from zero_vehicle2 import write_pack
        with tempfile.TemporaryDirectory() as directory:
            game = Path(directory)
            root = game / "content"
            source = game / "mission.lvl"
            source.write_bytes(archive())
            world = root / "maps/loom/map.json"
            ran = []
            writes_record = [True]

            def put(out, rel, data):
                path = out / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)

            def extract(tool, *args, env=None):
                if tool == "enhance_audio.py":
                    ran.append(("audio", args[0]))
                    put(root, f"audio/enhanced/{args[0]}/manifest.json", b"{}")
                    return True
                out = Path(env["BFE_CONTENT_DIR"])
                if tool == "zero_fp2.py":
                    return first_person_files(out)
                if tool == "zero_fx2.py":
                    self.assertNotEqual(out, root)
                    return write_effect_pack(out)
                map_file = out / "maps/loom/map.json"
                stage = args[args.index("--only") + 1]
                ran.append(stage)
                if stage == "map":
                    chain = script_chain("loom", recording(), script_index(source.read_bytes()))
                    chain.write(out)
                    vehicle_files = write_pack(map_file.parent, {"classes": {}, "models": {}})
                    empty_map_art(map_file.parent)
                    put(out, "maps/loom/geom.bin", b"invented ground")
                    put(out, "maps/loom/anim_banks.bin", b"")
                    put(out, "audio/index.json", b'{"maps":{}}')
                    put(out, "audio/fabric/sound.json", b'{"streams": ["weft"], "samples": {}}')
                    put(out, "audio/fabric/bank.bin", b"")
                    put(out, "audio/streams/weft.json", b'{"segments":{"reed":[0,4,2,8000]}}')
                    put(out, "audio/streams/weft.bin", b"reed")
                    if writes_record[0]:
                        map_file.write_text(json.dumps({"missions": {"loomc_spin": chain.record()}, **vehicle_files,
                                                     "barriers": [], "hints": [],
                                                     "layers": {"reed": {"barriers": [], "hints": []}}}))
                elif stage == "common":
                    put(out, "common/common.json", b"{}")
                    put(out, "common/anim_banks.json", b'{"banks":[]}')
                return True

            with (mock.patch("zero_mission2.GAME", game), mock.patch("zero_paths.PUBLIC", root),
                  mock.patch("zero_mission2.OPENING_MISSIONS", {"loom": "loomc_spin"}),
                  mock.patch("zero_mission2.opening_manifest", return_value=recording()) as recorder,
                  mock.patch("prepare.run_tool", side_effect=extract), mock.patch("prepare.maintain_sound"),
                  mock.patch("prepare.emit"), mock.patch("prepare.has_modules", return_value=True),
                  mock.patch("prepare.ffmpeg_ok", return_value=True)):
                recorder.return_value["calls"].append({"name": "ReadDataFile", "args": ["sound/loom.lvl;fabric"]})

                def run(expect=0, **options):
                    ran.clear()
                    args = {"jobs": 1, "core_only": False, "no_enhanced_sound": False, **options}
                    self.assertEqual(prepare_opening(SimpleNamespace(**args)), expect)
                    return list(ran)

                receipt = root / ".prepare-swbf2.json"
                self.assertEqual(run(), ["map", "common", ("audio", "fabric")])
                data = json.loads(receipt.read_bytes())
                self.assertEqual((data["swbf2Version"], data["complete"]), (PREPARED_VERSION, True))
                self.assertEqual(data["plan"], ["effects:loom", "map:loom", "common", "audio:fabric"])
                outputs = data["outputs"]
                self.assertEqual(set(outputs), {"effects:loom", "map:loom", "common", "audio:fabric", "*"})
                listed = {entry[0]: entry[1:] for files in outputs.values() for entry in files}
                self.assertEqual(listed["maps/loom/anim_banks.bin"], [0])
                self.assertEqual(listed["common/common.json"], [2])
                self.assertEqual(listed["firstperson/anims.bin"], [4, hashlib.sha256(b"reed").hexdigest()])
                self.assertIn("maps/loom/map.json", [entry[0] for entry in outputs["map:loom"]])
                pack = root / "maps/loom/effects/loomc_spin/effects.json"
                self.assertEqual(outputs["effects:loom"], [[pack.relative_to(root).as_posix(), pack.stat().st_size]])
                pack.unlink()
                self.assertEqual(run(), [])
                self.assertTrue(pack.is_file())
                self.assertEqual(json.loads(receipt.read_bytes())["outputs"], outputs)
                self.assertEqual(outputs["audio:fabric"], [])
                bodies = [entry for entry in outputs["*"] if entry[0].startswith("scripts/")]
                self.assertTrue(bodies)
                for rel, size, digest in bodies:
                    self.assertEqual(hashlib.sha256((root / rel).read_bytes()).hexdigest(), digest)
                    self.assertEqual(rel, f"scripts/{digest}.luac")
                self.assertEqual([entry[0] for entry in outputs["*"] if not entry[0].startswith("scripts/")],
                                 ["audio/fabric/bank.bin", "audio/fabric/sound.json", "audio/index.json",
                                  "audio/streams/weft.bin", "audio/streams/weft.json"])
                self.assertFalse(any(rel.startswith(("audio/enhanced/", ".")) for rel in listed))
                self.assertEqual(run(), [])

                # A cut stream cannot become the new size while a recovery is interrupted.
                stream = root / "audio/streams/weft.bin"
                stream.write_bytes(b"")
                with mock.patch("prepare.run_tool", return_value=False):
                    self.assertEqual(run(expect=1), [])
                saved = json.loads(receipt.read_bytes())
                self.assertFalse(saved["complete"])
                self.assertIn(["audio/streams/weft.bin", 4], saved["previousOutputs"]["*"])
                self.assertEqual(run(), ["map"])
                self.assertEqual(stream.read_bytes(), b"reed")
                self.assertNotIn("previousOutputs", json.loads(receipt.read_bytes()))

                # A real file left out of a receipt must be regenerated and listed again.
                # Receipt paths are data. Only names enumerated under the content root are opened.
                outside = game / "keep.txt"
                outside.write_bytes(b"outside")
                for replacement in (None, ["../keep.txt", 7], [outside.as_posix(), 7],
                                    ["C:/keep.txt", 7], ["maps/loom/geom.bin", True]):
                    with self.subTest(replacement=replacement):
                        saved = json.loads(receipt.read_bytes())
                        files = saved["outputs"]["map:loom"]
                        files.remove(["maps/loom/geom.bin", len(b"invented ground")])
                        if replacement is not None:
                            files.append(replacement)
                        receipt.write_text(json.dumps(saved))
                        self.assertEqual(run(), ["map"])
                        saved = json.loads(receipt.read_bytes())
                        self.assertEqual(saved["outputs"], outputs)
                        self.assertEqual(outside.read_bytes(), b"outside")

                # An unknown file inside a step survives, but cannot be certified by a rescan.
                extra = root / "maps/loom/additional.bin"
                extra.write_bytes(b"kept")
                self.assertEqual(run(expect=1), ["map"])
                saved = json.loads(receipt.read_bytes())
                self.assertIn("additional.bin", saved["failures"][0]["error"])
                self.assertFalse(saved["complete"])
                self.assertEqual(extra.read_bytes(), b"kept")
                extra.unlink()
                self.assertEqual(run(), ["map"])
                # A helper that forgets a required record cannot reuse the old copy.
                saved = json.loads(receipt.read_bytes())
                del saved["outputs"]["map:loom"]
                receipt.write_text(json.dumps(saved))
                writes_record[0] = False
                self.assertEqual(run(expect=1), ["map"])
                self.assertTrue(world.is_file())
                self.assertFalse(json.loads(receipt.read_bytes())["complete"])
                writes_record[0] = True
                self.assertEqual(run(), ["map"])
                for change in ("missing", "wrong"):
                    saved = json.loads(receipt.read_bytes())
                    row = next(r for r in saved["outputs"]["*"] if len(r) == 3)
                    if change == "missing":
                        row.pop()
                    else:
                        row[2] = "0" * 64
                    receipt.write_text(json.dumps(saved))
                    self.assertEqual(run(), ["map"])
                    self.assertEqual(json.loads(receipt.read_bytes())["outputs"], outputs)

                # A file only the inventory knows of, gone, cut short or emptied: its step runs again.
                (root / "maps/loom/geom.bin").unlink()
                self.assertEqual(run(), ["map"])
                (root / "common/anim_banks.json").write_bytes(b"{")
                self.assertEqual(run(), ["common"])
                (root / "maps/loom/anim_banks.bin").write_bytes(b"x")
                self.assertEqual(run(), ["map"])
                # A shared file gone: the map steps run again.
                (root / "audio/index.json").unlink()
                self.assertEqual(run(), ["map"])
                # A receipt from before the inventory: every core step runs again, the Enhanced
                # Sound is kept.
                saved = json.loads(receipt.read_bytes())
                del saved["outputs"], saved["plan"]
                receipt.write_text(json.dumps(saved))
                self.assertEqual(run(), ["map", "common"])
                self.assertEqual(json.loads(receipt.read_bytes())["outputs"]["audio:fabric"], [])
                self.assertEqual(run(), [])
                # Left out by choice, the Enhanced Sound is not planned.
                run(no_enhanced_sound=True)
                self.assertEqual(json.loads(receipt.read_bytes())["plan"], ["effects:loom", "map:loom", "common"])
                # A map step that writes no map.json has not finished.
                world.unlink()
                writes_record[0] = False
                self.assertEqual(run(expect=1), ["map"])
                data = json.loads(receipt.read_bytes())
                self.assertFalse(data["complete"])
                self.assertNotIn("map:loom", data["done"])
                self.assertNotIn("map:loom", data["outputs"])
                self.assertEqual(data["failures"][0]["error"], "It finished without writing nonempty required files for map:loom.")
                writes_record[0] = True
                self.assertEqual(run(), ["map"])
                self.assertTrue(json.loads(receipt.read_bytes())["complete"])

    @unittest.skipUnless(os.environ.get("BFE_SCRIPT_TOOL"), "requires the 2005 mission recorder")
    def test_recorded_nested_repeated_and_protected_missing_includes(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "mission.lvl"
            source.write_bytes(archive())
            manifest = read_mission_manifest(source, "loomc_spin")
        index = script_index(archive())
        self.assertEqual(script_chain("loom", manifest, index).manifest,
                         script_chain("loom", recording(), index).manifest)


if __name__ == "__main__":
    unittest.main()
