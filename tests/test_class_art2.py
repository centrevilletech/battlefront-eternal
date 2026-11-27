"""A class can reference a picture in its package or elsewhere in the installation."""
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import zero_class_art2 as art
import zero_outputs2 as products
from ktx2 import decode_png
from zero_texture import A8R8G8B8, Image


def chunk(tag, data):
    return tag + struct.pack("<I", len(data)) + data + bytes(-len(data) % 4)


def bitmap(name, bgra):
    info = struct.pack("<IHHHHI", A8R8G8B8, 1, 1, 1, 1, 1)
    face = chunk(b"FACE", chunk(b"LVL_", chunk(b"BODY", bgra)))
    return chunk(b"tex_", chunk(b"NAME", name.encode() + b"\0") + chunk(b"FMT_", chunk(b"INFO", info) + face))


class ClassArtwork(unittest.TestCase):
    def test_named_mesh_uses_its_segment_texture_and_shares_a_name_with_a_bitmap(self):
        from test_extract_swbf2 import _make_modl_swbf2
        from ucfb import iter_all_archives

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            hull = _make_modl_swbf2("Clasp", "Chalk", [(4, 0, 1), (4, 0, 5), (4, 3, 2)])
            data = chunk(b"ucfb", chunk(b"lvl_", bytes(8) + hull))
            classes = {"entc": {"handle": {"props": [("MapTexture", "CLASP"), ("HUDModel", "Clasp")]}},
                       "wpnc": {"loop": {"props": [("HUDTag", "Ring"), ("HUDTag", "Quill")]}}}
            chalk = bytes((24, 42, 67, 153))
            textures = {"chalk": Image(1, 1, rgba=chalk), "clasp": Image(1, 1, rgba=bytes(4)),
                        "ring": Image(1, 1, rgba=chalk)}
            art.write_map_art(root / "hud", classes, textures, root, [(data, list(iter_all_archives(data)))])
            catalog = json.loads((root / "hud/catalog.json").read_bytes())
            self.assertEqual(catalog["meshes"], {"clasp": "mesh.clasp", "ring": "ring"})
            self.assertEqual(catalog["bounds"], {"mesh.clasp": [0, 1, 3, 5]})
            self.assertEqual(catalog["missing"]["meshes"], ["quill"])
            self.assertEqual(catalog["missingDiffuse"], {})
            self.assertNotEqual(catalog["textures"]["clasp"], catalog["textures"]["mesh.clasp"])
            w, h, rgba = decode_png((root / "hud" / catalog["textures"]["mesh.clasp"]).read_bytes())
            self.assertEqual(max(w, h), 128)
            self.assertEqual(set(rgba[3::4]), {0, 153})
            self.assertIn(chalk, rgba)

    def test_textures_no_model_draws_join_the_map_art(self):
        from test_extract_swbf2 import _make_modl_swbf2
        from ucfb import iter_all_archives

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            hull = _make_modl_swbf2("Keel", "Plank", [(0, 0, 0), (0, 1, 0), (1, 0, 0)])
            data = chunk(b"ucfb", chunk(b"lvl_", bytes(8) + hull))
            swatch = bytes((10, 20, 30, 40))
            textures = {"plank": Image(1, 1, rgba=swatch), "Banner_Wide": Image(1, 1, rgba=swatch),
                        "turf": Image(1, 1, rgba=swatch)}
            models = {"keel": [{"texture": "plank", "bump": ""}]}
            loose = art.loose_textures(textures, (models,), taken={"turf"})
            self.assertEqual(loose, {"banner_wide"})
            art.write_map_art(root / "hud", {}, textures, root, [(data, list(iter_all_archives(data)))],
                              loose=loose)
            catalog = json.loads((root / "hud/catalog.json").read_bytes())
            self.assertIn("banner_wide", catalog["textures"])
            self.assertNotIn("plank", catalog["textures"])

    def test_named_images_use_loaded_art_and_agree_before_falling_back(self):
        with tempfile.TemporaryDirectory() as tmp:
            game, hud = Path(tmp) / "game", Path(tmp) / "hud"
            game.mkdir()
            bgra = bytes((9, 23, 57, 191))
            for i, tint in enumerate((12, 26)):
                package = (bitmap("Spool", bgra) + bitmap("LOOP", bgra) +
                           bitmap("reed", bytes((tint, 13, 15, 23))))
                (game / f"{i}.lvl").write_bytes(chunk(b"ucfb", chunk(b"lvl_", bytes(8) + package)))
            tables = {"entc": {"base": {"props": [("MapTexture", "Spool")]},
                               "unit": {"props": [("IconTexture", "Loop"), ("IconTexture", "../missing")]}},
                      "wpnc": {"tool": {"props": [("IconTexture", "Reed"), ("HealthTexture", "Twin")]}}}
            loaded = Image(1, 1, rgba=bytes((17, 13, 47, 0)))
            art.write_map_art(hud, tables, {"sPOOL": loaded, "TWIN": loaded}, game)
            record = json.loads((hud / "catalog.json").read_bytes())
            paths = record["textures"]
            self.assertEqual(set(paths), {"spool", "loop", "twin"})
            self.assertEqual(paths["spool"], paths["twin"])
            self.assertEqual(tuple(decode_png((hud / paths["spool"]).read_bytes())), (1, 1, loaded.rgba))
            self.assertEqual(tuple(decode_png((hud / paths["loop"]).read_bytes())), (1, 1, bytes((57, 23, 9, 191))))
            self.assertEqual(record["missing"]["textures"], ["../missing"])
            self.assertEqual(record["ambiguous"]["textures"], ["reed"])

    def test_preparation_and_resume_through_a_linked_library(self):
        import prepare
        from test_addon_import import ucfb, write
        from test_extract_swbf2 import _make_entc, _make_inst, _make_modl_swbf2, _make_tex
        from test_scripts2 import archive, first_person_files, write_effect_pack

        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            disk, library = base / "disk", base / "library"
            disk.mkdir()
            try:
                library.symlink_to(disk, target_is_directory=True)
            except OSError as error:
                self.skipTest(str(error))
            install = base / "install"
            game = install / "GameData/data/_lvl_pc"
            (game / "side").mkdir(parents=True)
            write(game / "core.lvl", ucfb())
            write(game / "mission.lvl", archive())
            world = chunk(b"wrld", chunk(b"NAME", b"loom\0") + _make_inst("spool", "bobbin", (1, 2, 3)))
            write(game / "loo/loom.lvl", ucfb(world, _make_entc("spool", "prop", [("MapTexture", "thread")]),
                                               _make_modl_swbf2("spool", "thread"), _make_tex("thread")))
            manifest = {"schema": 1, "script": "loomc_spin", "calls": []}
            run_tool = prepare.run_tool

            def tool_or_effects(tool, *args, env=None, **kwargs):
                # This invented install has no archives for the steps that read effects and first-person arms.
                if tool == "zero_fp2.py":
                    return first_person_files(Path(env["BFE_CONTENT_DIR"]))
                if tool == "zero_fx2.py":
                    return write_effect_pack(Path(env["BFE_CONTENT_DIR"]))
                return run_tool(tool, *args, env=env, **kwargs)

            def prepare_at(folder, *, refusal=False):
                with (mock.patch("zero_paths.PUBLIC", folder), mock.patch.object(prepare, "PUBLIC", folder),
                      mock.patch.object(prepare, "emit") as emit):
                    status = prepare.main()
                if refusal:
                    self.assertNotEqual(status, 0)
                    self.assertIn("Expected a plain file or folder", emit.call_args.kwargs["message"])
                else:
                    self.assertEqual(status, 0, emit.call_args_list)
                    self.assertTrue(json.loads((folder / ".prepare-swbf2.json").read_bytes())["complete"])
                return {c.kwargs["key"] for c in emit.call_args_list if c.kwargs.get("status") == "start"}

            with (mock.patch.dict(os.environ, BFE_GAME_DIR=str(install), BFE_FAMILY="swbf2", BFE_PREPARE_LOCK=""),
                  mock.patch.object(prepare, "run_tool", side_effect=tool_or_effects),
                  mock.patch.object(prepare, "GAME", game), mock.patch("zero_mission2.GAME", game),
                  mock.patch("zero_mission2.OPENING_MISSIONS", {"loom": manifest["script"]}),
                  mock.patch("zero_mission2.opening_manifest", return_value=manifest),
                  mock.patch.object(sys, "argv", ["prepare.py", "--jobs", "1", "--no-enhanced-sound"])):
                for parent in (library, library / "games"):
                    for existing in (False, True):
                        with self.subTest(parent=parent, existing=existing):
                            dest = parent / ("existing" if existing else "fresh")
                            self.assertEqual(prepare_at(dest.resolve() if existing else dest), {"effects:loom", "map:loom", "common"})
                            receipt = dest / ".prepare-swbf2.json"
                            saved = receipt.read_bytes(), receipt.stat().st_mtime_ns
                            self.assertEqual(prepare_at(dest), set())
                            self.assertEqual((receipt.read_bytes(), receipt.stat().st_mtime_ns), saved)

                            hud = dest / "maps/loom/hud"
                            outside = base / f"outside-{parent.name}-{dest.name}"
                            hud.rename(outside)
                            hud.symlink_to(outside, target_is_directory=True)
                            contents = {p.name: (p.read_bytes(), p.stat().st_mtime_ns) for p in outside.iterdir()}
                            self.assertEqual(prepare_at(dest, refusal=True), set())
                            self.assertEqual({p.name: (p.read_bytes(), p.stat().st_mtime_ns)
                                              for p in outside.iterdir()}, contents)

    def test_linked_map_and_hud_directories_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            target = root / "original"
            target.mkdir()
            hud = root / "map/hud"
            for link in (hud.parent, hud):
                with self.subTest(link=link):
                    link.parent.mkdir(exist_ok=True)
                    try:
                        link.symlink_to(target, target_is_directory=True)
                    except OSError as error:
                        self.skipTest(str(error))
                    with self.assertRaisesRegex(ValueError, "Expected a plain file or folder"):
                        art.publish(hud, {}, {})
                    self.assertFalse(art.catalog_current(hud))
                    self.assertEqual(list(target.iterdir()), [])
                    link.unlink()

    def test_allows_symlinked_ancestor_outside_artwork_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            real = root / "real"
            (real / "out").mkdir(parents=True)
            alias = root / "alias"
            try:
                alias.symlink_to(real, target_is_directory=True)
            except OSError as error:
                self.skipTest(str(error))
            hud = alias / "out" / "hud"
            art.publish(hud, {"ribbon": (1, 1, bytes((8, 2, 9, 4)))}, {"meshes": {}})
            self.assertTrue(art.catalog_current(hud))

    def test_resume_detects_missing_changed_and_external_catalog_entries(self):
        with tempfile.TemporaryDirectory() as tmp:
            hud = Path(tmp) / "hud"
            art.publish(hud, {"ribbon": (1, 1, bytes((8, 2, 9, 4)))}, {"meshes": {}})
            self.assertTrue(art.catalog_current(hud))
            record = hud / "catalog.json"
            catalog = json.loads(record.read_bytes())
            image = hud / catalog["textures"]["ribbon"]
            original = image.read_bytes()
            image.write_bytes(bytes(len(original)))
            self.assertFalse(art.catalog_current(hud))
            image.unlink()
            self.assertFalse(art.catalog_current(hud))
            (hud.parent / image.name).write_bytes(original)
            catalog["textures"]["ribbon"] = "../" + image.name
            record.write_text(json.dumps(catalog))
            self.assertFalse(art.catalog_current(hud))

    def test_rebuild_retires_only_its_own_recorded_picture(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, work = Path(tmp) / "old", Path(tmp) / "new"
            key = "map:bay"
            for target in (root, work):
                directory = target / "maps/bay"
                directory.mkdir(parents=True)
                (directory / "map.json").write_text("{}")
                (directory / "geom.bin").write_bytes(b"x")
                images = {"pin": (1, 1, bytes(4))} if target == root else {}
                art.publish(directory / "hud", images, {"meshes": {}})
            old = products.inventory(root)[key]
            new = products.inventory(work)[key]
            image = next((root / "maps/bay/hud").glob("*.png"))
            with self.assertRaisesRegex(ValueError, "not produced"):
                products.publish(root, work, key, new, old, [])
            self.assertTrue(image.exists())
            products.publish(root, work, key, new, old, old)
            self.assertFalse(image.exists())
