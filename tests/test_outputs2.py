"""Inventory checks use paths found in the tree and preserve unexpected files."""
import os
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
import zero_outputs2 as products


class InventoryTests(unittest.TestCase):
    def test_a_2004_destination_is_refused_without_opening_its_lock(self):
        import prepare
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            lock = root / ".prepare.lock"
            lock.touch()
            (root / ".prepare.json").write_text(json.dumps({
                "version": prepare.STATE_VERSION, "mapImportVersion": prepare.MAP_IMPORT_VERSION,
                "muzzleVersion": prepare.SIDE_EXPORT_VERSION, "commonVersion": prepare.COMMON_VERSION,
                "complete": True, "failed": [], "skipped": [],
            }))
            original = Path.open
            def open_file(path, *args, **kwargs):
                self.assertNotEqual(path, lock)
                return original(path, *args, **kwargs)
            with (mock.patch.dict(os.environ, BFE_FAMILY="swbf2"),
                  mock.patch.object(prepare, "PUBLIC", root), mock.patch.object(Path, "open", open_file),
                  mock.patch.object(prepare, "hold_lock", side_effect=AssertionError("opened the lock"))):
                for options in ([], ["--maintain-sound"]):
                    with self.subTest(options=options), mock.patch.object(sys, "argv", ["prepare.py", *options]):
                        self.assertEqual(prepare.main(), 2)

    def test_an_owned_tree_with_an_unreadable_product_reports_a_competing_writer(self):
        import prepare
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".prepare-swbf2.json").write_text('{"family":"swbf2","swbf2Version":1}')
            maps = root / "maps"
            maps.mkdir()
            lock = root / ".prepare.lock"
            iterdir = Path.iterdir
            def list_dir(path):
                if path == maps:
                    raise PermissionError("cannot read maps")
                return iterdir(path)
            with (mock.patch.dict(os.environ, BFE_FAMILY="swbf2", BFE_PREPARE_LOCK=""),
                  mock.patch.object(sys, "argv", ["prepare.py"]), mock.patch.object(prepare, "PUBLIC", root),
                  mock.patch.object(Path, "iterdir", list_dir), mock.patch.object(prepare, "emit") as emit):
                self.assertEqual(prepare.main(), 2)
                self.assertFalse(lock.exists())
                with prepare.hold_lock(lock):
                    self.assertEqual(prepare.main(), 3)
                    self.assertEqual(emit.call_args.args, ("busy",))
                self.assertEqual(prepare.main(), 2)
                self.assertIn("cannot read maps", emit.call_args.kwargs["message"])
                # A visible foreign product still vetoes the lock after another folder failed.
                side = root / "sides" / "foreign"
                side.mkdir(parents=True)
                (side / "units.json").write_text('{"family":"swbf1"}')
                with mock.patch("builtins.open", wraps=open) as opened:
                    self.assertEqual(prepare.main(), 2)
                    self.assertFalse(any(call.args[0] == lock for call in opened.call_args_list))

    def test_sound_maintenance_rechecks_the_receipt_before_cleanup(self):
        import prepare
        import zero_sound_health as health
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "audio/enhanced").mkdir(parents=True)
            receipt = root / ".prepare-swbf2.json"
            receipt.write_text('{"family":"swbf2","swbf2Version":1}')
            take_lock = prepare.hold_lock
            def change_origin(path):
                locked = take_lock(path)
                receipt.write_text('{"family":"swbf1"}')
                return locked
            with (mock.patch.dict(os.environ, BFE_FAMILY="swbf2"),
                  mock.patch.object(sys, "argv", ["prepare.py", "--maintain-sound"]),
                  mock.patch.object(prepare, "PUBLIC", root), mock.patch.object(prepare, "hold_lock", change_origin),
                  mock.patch.object(health, "can_record", side_effect=AssertionError("wrote a cache probe")),
                  mock.patch.object(health, "clean_temporaries", side_effect=AssertionError("cleaned the folder"))):
                self.assertEqual(prepare.main(), 2)
                self.assertEqual(json.loads(receipt.read_bytes()), {"family": "swbf1"})

    def test_an_unlocked_input_cannot_initialize_a_marker_only_tree(self):
        import prepare
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".bfe-family.json").write_text('{"family":"swbf2","format":1}')
            with (root / ".prepare.lock").open("a+b") as client:
                stat_fd = os.fstat
                def inherited(fd):
                    return stat_fd(client.fileno() if fd == 0 else fd)
                with (mock.patch.dict(os.environ, BFE_FAMILY="swbf2", BFE_PREPARE_LOCK="stdin"),
                      mock.patch.object(os, "fstat", inherited)):
                    with self.assertRaises(ValueError):
                        products.destination_origin(root, client_handover=prepare.client_holds_initial_lock)
                    if os.name == "nt":
                        import msvcrt
                        msvcrt.locking(client.fileno(), msvcrt.LK_NBLCK, 1)
                    else:
                        import fcntl
                        fcntl.flock(client, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    self.assertEqual(products.destination_origin(root, client_handover=prepare.client_holds_initial_lock),
                                     "empty")
    def test_common_publication_installs_and_receipts_each_first_person_product(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            live, work = root / "live", root / "scratch"
            live.mkdir()
            files = {
                "common/common.json": b"{}",
                "firstperson/manifest.json": json.dumps({"version": 2, "animsData": "anims.bin",
                    "textures": {}, "rigs": {"spindle": {"file": "rigs/spindle.json"}}}).encode(),
                "firstperson/anims.bin": b"acbd",
                "firstperson/rigs/spindle.json": b"{}",
                "firstperson/rigs/spindle.bin": b"xyz",
                "firstperson/rigs/spindle_index.bin": b"ijk",
            }
            for name, contents in files.items():
                path = work / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(contents)
            rows = products.inventory(work)["common"]
            result = products.publish(live, work, "common", rows, [], None)
            self.assertEqual(result, rows)
            self.assertEqual(products.inventory(live)["common"], rows)
            self.assertTrue(products.first_person_complete(live, result))
            (live / "firstperson/rigs/spindle.bin").write_bytes(b"zyx")
            self.assertFalse(products.matches(result, products.inventory(live)["common"]))

    def test_a_reused_stream_is_read_from_its_2005_archive(self):
        import zero_sound
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            archive = root / "weave"
            archive.write_bytes(b"woven")
            (root / "pond.bin").write_bytes(b"")
            (root / "pond.json").write_text('{"segments":{}}')
            stream = {"head": {"Name": zero_sound.fnv1a("pond")}, "data": (0,5), "items": []}
            with (mock.patch("zero_family.is_swbf2", return_value=True),
                  mock.patch.object(zero_sound, "sound_file", return_value=archive),
                  mock.patch.object(zero_sound, "archive_contents", return_value={"streams": [stream]})):
                zero_sound.write_streams([("weave", "pond")], root)
            self.assertEqual((root / "pond.bin").read_bytes(), b"woven")

    def test_required_geometry_and_json_have_content_and_optional_buffers_can_be_empty(self):
        for key, row in (("common", "common/common.json"), ("side:reed", "sides/reed/units.json")):
            for rows in ([], [[row, 0]]):
                self.assertFalse(products.core_present(key, rows))
            rows = [[row, 2], ["common/optional.bin", 0]]
            if key == "common":
                rows.extend([["firstperson/manifest.json", 1], ["firstperson/anims.bin", 4]])
            self.assertTrue(products.core_present(key, rows))
        self.assertFalse(products.core_present("map:loom", [["maps/loom/map.json", 20]]))
        self.assertFalse(products.core_present("map:loom", [["maps/loom/map.json", 20], ["maps/loom/geom.bin", 0]]))

    def test_enumeration_errors_and_invalid_path_components_are_not_empty_inventories(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "common").mkdir()
            scan = os.scandir
            def fail(path):
                if Path(path) == root / "common":
                    raise PermissionError("unreadable directory")
                return scan(path)
            with mock.patch("os.scandir", side_effect=fail), self.assertRaisesRegex(PermissionError, "unreadable"):
                products.inventory(root)
            for name in ("../outside", "/outside", "common//room", "common/../outside", "common/C:outside", "common/CON"):
                self.assertFalse(products.matches([[name, 10]], [[name, 10]]))
    def test_first_person_mesh_dropped_from_tree_and_receipt_needs_repair(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            mesh = root / "firstperson/rigs"
            mesh.mkdir(parents=True)
            manifest = {"version": 2, "animsData": "anims.bin", "rigs": {"reed": {"file": "rigs/reed.json"}}, "textures": {}}
            (mesh.parent / "manifest.json").write_text(json.dumps(manifest))
            for name in ("reed.json", "reed.bin", "reed_index.bin"):
                (mesh / name).write_bytes(b"made up")
            (mesh.parent / "anims.bin").write_bytes(b"keys")
            rows = products.inventory(root)["common"]
            self.assertTrue(products.first_person_complete(root, rows))
            index = mesh / "reed_index.bin"
            index.write_bytes(b"changed")
            self.assertFalse(products.matches(rows, products.inventory(root)["common"]))
            index.unlink()
            self.assertFalse(products.first_person_complete(root, products.inventory(root)["common"]))

    def test_ownership_before_any_write(self):
        from zero_mission2 import PREPARED_VERSION
        modern = {"family": "swbf2", "swbf2Version": PREPARED_VERSION}
        early = {"family": "swbf2", "swbf2Version": 3, "version": 2, "game": "/installed", "complete": True, "done": []}
        current = dict(early, version=2, mapImportVersion=16, muzzleVersion=3, commonVersion=2,
                       complete=True, skipped=[], failed=[])
        old, new, marker = ".prepare.json", ".prepare-swbf2.json", ".bfe-family.json"
        cases = [
            ({}, "empty"), ({new: modern}, "dedicated"), ({old: early}, "early"),
            ({new: modern, old: early}, "early"), ({old: current}, None),
            ({old: dict(current, swbf2Version=None), marker: {"family": "swbf2"}}, None),
            ({old: dict(early, game="relative")}, None), ({old: dict(early, complete=1)}, None),
            ({old: dict(early, done="")}, None), ({old: dict(early, version=0)}, None),
            ({".prepare.lock": b""}, None),
            ({new: modern, old: current}, None), ({new: {"family": "swbf1"}}, None),
            ({new: None}, None), ({new: b"{"}, None), ({new: dict(modern, swbf2Version=True)}, None),
            ({new: dict(modern, swbf2Version=0)}, None), ({new: dict(modern, swbf2Version=PREPARED_VERSION+1)}, None),
            ({marker: {"family": "swbf2"}}, None), ({marker: b"", "other": b"keep"}, None),
            ({new: modern, marker: {"family": "swbf1"}}, None), ({"maps/weave/map.json": {"world": "weave", "missions": {}}}, None),
            ({new: modern, "maps/weave/map.json": {"world": "weave", "missions": {}}}, None),
            ({new: modern, "common/shell": None}, None), ({new: modern, "common/common.json": b""}, "dedicated"),
            ({"player-file": b"keep"}, None),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(products.destination_origin(Path(tmp)/"absent"), "empty")
            for i, (files, expected) in enumerate(cases):
                with self.subTest(files=files):
                    root = Path(tmp) / str(i)
                    root.mkdir()
                    for name, value in files.items():
                        path = root / name
                        path.parent.mkdir(parents=True, exist_ok=True)
                        if value is None:
                            path.mkdir()
                        else:
                            path.write_bytes(value if isinstance(value, bytes) else json.dumps(value).encode())
                    if expected:
                        self.assertEqual(products.destination_origin(root), expected)
                    else:
                        with self.assertRaises(ValueError):
                            products.destination_origin(root)
                    self.assertEqual((root / ".prepare.lock").exists(), ".prepare.lock" in files)

    def test_only_recognized_abandoned_files_are_left_out_of_the_inventory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            temps = [".prepare-swbf2.tmp", "audio/index.json.part", "audio/streams/pond.bin.132.part",
                     "audio/weave/.bfe-bank-abcdefgh.part", "audio/weave/tmpabcdefgh/sound.json",
                     "maps/loom/scripts/tmpabcdefgh/body", "scripts/tmppqrstuvw/body"]
            for name in [*temps, "audio/notes.part", "audio/weave/tmpextra123/notes"]:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.touch()
            remove = []
            actual = products.inventory(root, leftovers=remove)
            self.assertEqual({p.relative_to(root).as_posix() for p in remove if p.is_file()}, set(temps))
            self.assertEqual(actual["*"], [["audio/notes.part", 0], ["audio/weave/tmpextra123/notes", 0]])

    def test_links_are_rejected_before_reading_files_beyond_them(self):
        with tempfile.TemporaryDirectory() as tmp:
            parent = Path(tmp)
            root = parent / "prepared"
            root.mkdir()
            outside = parent / "outside"
            outside.mkdir()
            (outside / "body").write_bytes(b"invented")
            for name, target in (("maps", outside), ("piece.bin", outside / "body"),
                                 (".locks", outside), ("missing", parent / "absent")):
                with self.subTest(name=name):
                    link = root / name
                    try:
                        link.symlink_to(target, target_is_directory=target.is_dir())
                    except OSError as error:
                        self.skipTest(f"Cannot create links on this filesystem: {error}")
                    with self.assertRaisesRegex(ValueError, name):
                        products.inventory(root)
                    link.unlink()
            self.assertEqual((outside / "body").read_bytes(), b"invented")

    def test_interrupted_extractors_cannot_write_into_a_new_runs_workspace(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with products.workspace(root) as working:
                abandoned = Path(working)
                products.clear_workspaces(root)
                with products.workspace(root) as resumed:
                    self.assertNotEqual(Path(resumed), abandoned)
                    abandoned.mkdir(exist_ok=True)
                    (abandoned / "late-output").write_bytes(b"late")
                    self.assertFalse((Path(resumed) / "late-output").exists())

    def test_script_bodies_require_content_addressed_names(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts").mkdir()
            (root / "scripts/body.luac").write_bytes(b"invented")
            with self.assertRaisesRegex(ValueError, "content-addressed"):
                products.inventory(root)

    def test_special_files_cannot_be_certified(self):
        if not hasattr(os, "mkfifo"):
            self.skipTest("requires a FIFO")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            os.mkfifo(root / "pipe")
            with self.assertRaisesRegex(ValueError, "pipe"):
                products.inventory(root)

    def test_absent_and_duplicate_rows_fail_even_with_equal_row_counts(self):
        rows = [["common/a", 0], ["common/b", 1]]
        self.assertTrue(products.matches(rows[::-1], rows))
        for proposed in ([], [rows[0], rows[0]], [rows[0], ["common/b", True]],
                         [rows[0], ["common/b", 1, None]], [["../a", 0], rows[1]]):
            self.assertFalse(products.matches(proposed, rows))

    def test_same_game_reads_spellings_without_a_link(self):
        with tempfile.TemporaryDirectory() as tmp:
            game = Path(tmp) / "game"
            game.mkdir()
            other = Path(tmp) / "other"
            other.mkdir()
            self.assertTrue(products._same_game(str(game), game))
            self.assertTrue(products._same_game(os.path.join(str(game), "."), game))
            self.assertTrue(products._same_game(str(game) + os.sep, game))
            self.assertFalse(products._same_game(str(other), game))
            self.assertFalse(products._same_game(".", game))
            self.assertFalse(products._same_game("game", game))
            self.assertFalse(products._same_game("", game))
            self.assertFalse(products._same_game(None, game))
            self.assertFalse(products._same_game("a\u0000b", game))
            if os.name == "nt":
                swapped = str(game).replace("\\", "/")
                self.assertTrue(products._same_game(swapped, game))
                self.assertTrue(products._same_game(str(game).upper(), game))

    def test_receipt_with_a_damaged_game_spelling_starts_an_empty_plan(self):
        import prepare
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / ".prepare-swbf2.json"
            path.write_text(json.dumps({
                "family": "swbf2", "game": "a\u0000b", "version": prepare.STATE_VERSION,
                "done": ["common"],
            }))
            self.assertEqual(products.Receipt(path, root).data["done"], [])

    def test_receipt_resumes_for_the_same_folder_by_another_spelling(self):
        import prepare
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            game = root / "game"
            game.mkdir()
            alias = root / "alias"
            try:
                alias.symlink_to(game, target_is_directory=True)
                alias.stat()
            except OSError:
                self.skipTest("folder links unavailable")
            path = root / ".prepare-swbf2.json"
            path.write_text(json.dumps({
                "family": "swbf2", "game": str(game), "version": prepare.STATE_VERSION,
                "mapImportVersion": prepare.MAP_IMPORT_VERSION,
                "muzzleVersion": prepare.SIDE_EXPORT_VERSION,
                "commonVersion": prepare.COMMON_VERSION,
                "done": ["common"],
            }))
            self.assertEqual(products.Receipt(path, alias).data["done"], ["common"])
            other = root / "other"
            other.mkdir()
            self.assertEqual(products.Receipt(path, other).data["done"], [])

    def test_receipt_with_unusable_done_list_starts_an_empty_plan(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / ".prepare-swbf2.json"
            for body in (b'[]', b'{}', b'{"done":false}', b'{"done":[{}]}', b'{"outputs"'):
                path.write_bytes(body)
                state = products.Receipt(path, root)
                self.assertEqual(state.data["done"], [])
                self.assertEqual(path.read_bytes(), body)

    def test_receipt_resumes_across_symlink_and_case_spellings_of_same_game_folder(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            game = root / "MixedCaseGame" / "GameData" / "data" / "_lvl_pc"
            game.mkdir(parents=True)
            other = root / "OtherGame" / "GameData" / "data" / "_lvl_pc"
            other.mkdir(parents=True)
            alias = root / "alias"
            try:
                alias.symlink_to(root / "MixedCaseGame", target_is_directory=True)
            except OSError as error:
                self.skipTest(str(error))
            receipt = root / ".prepare-swbf2.json"
            first = products.Receipt(receipt, game.resolve())
            first.mark("map:loom")
            resumed_via_link = products.Receipt(receipt, alias / "GameData" / "data" / "_lvl_pc")
            self.assertEqual(resumed_via_link.data["done"], ["map:loom"])
            alt_case = Path(str(game.resolve()).replace("MixedCaseGame", "mixedcasegame"))
            if alt_case.exists() and os.path.samefile(game, alt_case):
                resumed_via_case = products.Receipt(receipt, alt_case)
                self.assertEqual(resumed_via_case.data["done"], ["map:loom"])
            different = products.Receipt(receipt, other.resolve())
            self.assertEqual(different.data["done"], [])
