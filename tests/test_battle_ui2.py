import json
from pathlib import Path
import tempfile
import unittest

from test_interface_2005_export import archive, chunk, config, data, fake_install, name_chunk, scope, texture
import zero_battle_ui2 as ui
import zero_hud2


class BattleInterfaceTests(unittest.TestCase):
    def test_publish_locales_and_art_before_the_catalog(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            out = Path(tmp) / "prepared"
            (root / "common.lvl").write_bytes(archive(texture("linen")))
            viewport = config(b"hud_", name_chunk("1playerhud"), data("Bitmap", "badge")
                              + scope(data("Bitmap", "linen")))
            transforms = config(b"hud_", name_chunk("hudtransforms"))
            mesh = chunk(b"modl", chunk(b"NAME", b"hud_coral\0") + chunk(b"segm",
                         chunk(b"TNAM", bytes(5)) + chunk(b"TNAM", bytes(4) + b"linen\0")))
            (root / "ingame.lvl").write_bytes(archive(viewport[8:], transforms[8:], mesh))
            zero_hud2.write_hud_files(root, out)
            self.assertTrue((out / ui.HUD / "1playerhud.json").is_file())
            catalog = json.loads((out / ui.HUD / "presentation.json").read_text())
            for section in ("trees", "textures", "languages", "meshes"):
                for entry in catalog[section].values():
                    self.assertNotIn("\\", entry)
            folder = out / ui.HUD
            self.assertEqual(catalog["meshes"]["hud_coral"], "linen")
            self.assertEqual(set(catalog["languages"]), {"english", "spanish"})
            self.assertTrue((folder / catalog["textures"]["linen"]).read_bytes().startswith(b"\x89PNG"))
            self.assertEqual(json.loads((folder / catalog["trees"]["1playerhud"]).read_text())["file"], "1playerhud")
            published = (folder / "presentation.json").read_bytes()
            (root / "ingame.lvl").write_bytes(archive(chunk(b"tex_", b"invalid")))
            with self.assertRaises(ui.ConfigError):
                ui.write_battle_interface(root, out)
            self.assertEqual((folder / "presentation.json").read_bytes(), published)

class IconProjectionTests(unittest.TestCase):
    def test_triangle_keeps_its_silhouette_and_texture(self):
        part = dict(positions=[(0, 1, 1), (0, 4, 1), (0, 1, 3)],
                    uvs=[(0.3, 0.2)] * 3, indices=[0, 1, 2], texture="felt")
        w, h, pixels = ui.zero_hud_icon.raster([part], lambda _: (1, 1, b"\x60\x11\xee\xff"), 26)
        self.assertGreater(w, h)
        self.assertEqual(pixels[:4], bytes(4))
        self.assertIn(b"\x60\x11\xee\xff", pixels)
        self.assertLess(sum(a > 0 for a in pixels[3::4]), w * h // 2)

    def test_icon_extent_follows_the_axes_the_icon_shows(self):
        flat = dict(positions=[(-2, 1, 0.5), (3, -1, 0.5), (0, 2, 0.5)])
        self.assertEqual(ui.zero_hud_icon.extent([flat]), [-2, -1, 3, 2])
        lying = dict(positions=[(1, 0, -4), (5, 0.1, 2), (2, 0, 0)])
        self.assertEqual(ui.zero_hud_icon.extent([lying]), [1, -4, 5, 2])
        barrel = dict(positions=[(0, 0, 0), (0, 0, 4), (0, 1, 0)])
        left, bottom, right, top = ui.zero_hud_icon.extent([barrel], (0.0, 90.0, 0.0))
        self.assertAlmostEqual(right - left, 4)
        self.assertAlmostEqual(top - bottom, 1)

    def test_bitmap_and_mesh_icons_share_the_catalog_limit(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            out = Path(tmp) / "result"
            (root / "common.lvl").write_bytes(archive(texture("linen")))
            view = config(b"hud_", name_chunk("1playerhud"))
            transforms = config(b"hud_", name_chunk("hudtransforms"))
            mesh = chunk(b"modl", chunk(b"NAME", b"hud_chalk\0") + chunk(b"segm",
                         chunk(b"TNAM", bytes(4) + b"linen\0")))
            (root / "ingame.lvl").write_bytes(archive(view[8:], transforms[8:], mesh))
            with patch.object(ui, "MAX_TEXTURES", 1, create=True), patch.object(
                    ui.zero_hud_icon, "raster", return_value=(1, 1, bytes(4))):
                with self.assertRaises(ui.ConfigError):
                    ui.write_battle_interface(root, out)
            self.assertFalse((out / ui.HUD / "presentation.json").exists())

    def test_oriented_projection_rotates_3d_weapon_to_horizontal_opaque_silhouette(self):
        # Gun barrel along Z (len 4), height along Y (len 1), thin in X: rotating +90 deg around Y
        # lays the barrel horizontally along X with opaque alpha=255 even when diffuse alpha is 0.
        part = dict(positions=[(0, 0, 0), (0, 0, 4), (0, 1, 0)],
                    uvs=[(0.5, 0.5)] * 3, indices=[0, 1, 2], texture="steel")
        w, h, pixels = ui.zero_hud_icon.raster_oriented(
            [part], lambda _: (1, 1, b"\x80\x80\x80\x00"), (0.0, 90.0, 0.0), 40
        )
        self.assertGreater(w, h)
        self.assertIn(255, set(pixels[3::4]))

    def test_side_weapon_mesh_info_is_rasterized_into_presentation_catalog(self):
        from test_extract_swbf2 import _make_modl_swbf2
        with tempfile.TemporaryDirectory() as tmp:
            root = fake_install(tmp)
            out = Path(tmp) / "prepared"
            (root / "common.lvl").write_bytes(archive(texture("linen")))
            mesh_info = (data("Model3D", "player1weapon1mesh")
                         + scope(data("MeshInfo", "imp_weap_inf_rifle")
                                 + scope(data("rotation", 0.0, 90.0, 0.0))))
            viewport = config(b"hud_", name_chunk("1playerhud"), mesh_info)
            transforms = config(b"hud_", name_chunk("hudtransforms"))
            (root / "ingame.lvl").write_bytes(archive(viewport[8:], transforms[8:]))
            side_dir = root / "side"
            side_dir.mkdir()
            rifle = _make_modl_swbf2("imp_weap_inf_rifle", "linen", [(0, 0, 0), (0, 0, 4), (0, 1, 0)])
            (side_dir / "imp.lvl").write_bytes(archive(rifle))
            catalog = ui.write_battle_interface(root, out)
            self.assertEqual(catalog["meshes"]["imp_weap_inf_rifle"], "mesh.imp_weap_inf_rifle")
            left, bottom, right, top = catalog["bounds"]["mesh.imp_weap_inf_rifle"]
            self.assertAlmostEqual(right - left, 4)
            self.assertAlmostEqual(top - bottom, 1)
            self.assertTrue((out / ui.HUD / catalog["textures"]["mesh.imp_weap_inf_rifle"]).is_file())
