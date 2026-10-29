"""The sky's models: every DomeModel, in order, with its Offset and
MovementScale (tools/extract.py dome_models); and the far scene's fog
(low_res_terrain)."""
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from extract import dome_models, low_res_terrain
from zero_config import NAMES
from zero_hash import fnv1a
from zero_paths import GAME


def model(*children):
    return ['DomeModel', [], [list(child) for child in children]]


class DomeModels(unittest.TestCase):
    def test_every_model_in_order_with_its_placement(self):
        # Geonosis: the dome, then the arena and the rim standing on its horizon.
        dome_info = [['Texture', ['SKY_Geonosis'], []],
                     model(('Geometry', ['geo_sky_dome'], [])),
                     model(('Geometry', ['geo_sky_arena'], []), ('Offset', [-20.0], []), ('MovementScale', [0.995], [])),
                     ['LowResTerrain', [], []],
                     model(('Geometry', ['geo_sky_dome_rim'], []), ('Offset', [10.0], [])),
                     model(('Geometry', [''], []))]
        self.assertEqual(dome_models(dome_info), [('geo_sky_dome', None), ('geo_sky_arena', [0.995, -20.0]),
                                                  ('geo_sky_dome_rim', [1.0, 10.0]), ('', None)])
        self.assertEqual(dome_models([]), [])

    def test_names_resolve(self):
        for name in ('Offset', 'MovementScale', 'DomeModel', 'Geometry'):
            self.assertEqual(NAMES[fnv1a(name)], name)


class FarSceneFog(unittest.TestCase):
    """LowResTerrain's fog as the original game reads it: FogColor's
    fourth value stretches FogFar by 255 / alpha, except at 0 and 255. The
    range is exported as authored otherwise; whether it fogs at all
    (0 <= FogNear < FogFar) is the renderer's call (src/materials.js fogs)."""

    def fog(self, *entries):
        return low_res_terrain([[name, values, []] for name, values in entries], {})

    def test_alpha_stretches_the_far_end(self):
        # Bespin Platforms: alpha 100.
        low = self.fog(('FogNear', [80.0]), ('FogFar', [430.0]), ('FogColor', [45.0, 45.0, 60.0, 100.0]))
        self.assertEqual(low['fog'], [45.0, 45.0, 60.0])
        self.assertEqual(low['fogRange'], [80.0, 80.0 + 350.0 * 255 / 100])

    def test_no_alpha_or_an_alpha_of_0_or_255_leaves_it(self):
        for color in ([82.0, 45.0, 25.0], [201.0, 200.0, 206.0, 255.0], [70.0, 70.0, 70.0, 0.0]):
            with self.subTest(color=color):
                low = self.fog(('FogNear', [3000.0]), ('FogFar', [6000.0]), ('FogColor', color))
                self.assertEqual(low['fogRange'], [3000.0, 6000.0])

    def test_kashyyyk_islands_range_is_exported_as_authored(self):
        low = self.fog(('Texture', ['kas1']), ('FogNear', [9000.0]), ('FogFar', [8000.0]),
                       ('FogColor', [0.0, 0.0, 0.0, 0.0]), ('MaxDistance', [1500.0]))
        self.assertEqual(low['fog'], [0.0, 0.0, 0.0])
        self.assertEqual(low['fogRange'], [9000.0, 8000.0])
        self.assertIsNone(low_res_terrain([], {}))


@unittest.skipUnless((GAME / 'mission.lvl').exists(), 'requires local retail archives')
class RetailSkies(unittest.TestCase):
    def test_kashyyyk_islands_far_fog(self):
        # The sky sets its fog's start past its end, with a zero alpha.
        import zero_addon
        from zero_config import configs, section, value
        data = zero_addon.map_archive('kas1', GAME).read_bytes()
        low = section((configs(data, b'sky_') or [[]])[0], 'DomeInfo', 'LowResTerrain')
        self.assertEqual(value(low, 'FogNear'), [9000.0])
        self.assertEqual(value(low, 'FogFar'), [8000.0])
        self.assertEqual(value(low, 'FogColor'), [0.0, 0.0, 0.0, 0.0])

    def test_horizon_models(self):
        import zero_addon
        from zero_config import configs, section
        expected = {'hot1': ['hoth_sky_dome', 'hoth_mountain', 'hoth_cannon'],
                    'end1': ['end1_sky_dome', 'end_sky_dome_sat', 'end_sky_dome_trees', 'end_sky_dome_death'],
                    'rhn2': ['RhenVar2_skydome', 'RhenVar2_skydome_clouds', 'RhenVar2_skydome_clouds_2', '', ''],
                    'tat2': ['tat2_sky_dome', '', '', '']}
        for name, models in expected.items():
            with self.subTest(map=name):
                data = zero_addon.map_archive(name, GAME).read_bytes()
                dome_info = section((configs(data, b'sky_') or [[]])[0], 'DomeInfo')
                self.assertEqual([m for m, _ in dome_models(dome_info)], models)


if __name__ == '__main__':
    unittest.main()
