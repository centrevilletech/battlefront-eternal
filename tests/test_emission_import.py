import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from extract import fire_point, weapon_fire_points


class EmissionImportTests(unittest.TestCase):
    def test_stale_and_case_variant_hardpoints_use_model_marker(self):
        matrix = [0] * 16
        matrix[12:15] = [.1, .2, .3]
        skeleton = {'names': ['HP_Fire'], 'model': [matrix]}
        self.assertEqual(fire_point(skeleton, 'hp_fire'), [.1, .2, .3])
        self.assertEqual(fire_point(skeleton, 'hp_fire_ALL_INF_BlasterRifle'), [.1, .2, .3])

    def test_shared_model_cannot_lose_muzzle_to_another_weapon(self):
        vertices = [[-.02, .03, .8], [.02, .07, .8], [0, 0, -.1]]
        weapons = [{'GeometryName': 'rifle', 'ordnance': {'base': 'bolt'}},
                   {'GeometryName': 'rifle', 'FirePointName': 'missing'}]
        points = weapon_fire_points(weapons, {}, {'rifle': [{'positions': vertices}]})
        self.assertEqual(points['rifle'], [0, .05, .8])

    def test_first_person_uses_its_own_barrel(self):
        weapon = {'GeometryName': 'low', 'HighResGeometry': 'high', 'ordnance': {'base': 'bolt'}}
        models = {name: [{'positions': [[-.02, 0, 0], [.02, .1, length], [-.02, .1, length]]}]
                  for name, length in [('low', .4), ('high', .8)]}
        points = weapon_fire_points([weapon], {}, models)
        self.assertEqual(points['low'], [0, .1, .4])
        self.assertEqual(points['high'], [0, .1, .8])

    def test_barrel_axis_ignores_a_longer_rail(self):
        matrix = [0] * 16
        matrix[12:15] = [0, .06, .35]
        skeletons = {'gun': {'names': ['hp_fire'], 'model': [matrix]}}
        cap = {'positions': [[-.03, .03, .4], [.03, .03, .4], [0, .09, .4], [0, 0, 0]], 'indices': [0, 1, 2]}
        rail = {'positions': [[-.03, -.01, .6], [.03, -.01, .6], [0, .01, .6]], 'indices': [0, 1, 2]}
        point = weapon_fire_points([{'GeometryName': 'gun', 'ordnance': {'base': 'bolt'}}], skeletons, {'gun': [cap, rail]})['gun']
        self.assertEqual(point, [0, .06, .4])


if __name__ == '__main__':
    unittest.main()
