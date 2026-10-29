"""tools/zero_vehicle.py aimer, barrel and leg-pair records, on synthetic
class properties shaped like the retail walkers' (no retail data).

    python3 -m unittest tests/test_vehicle_aimers.py
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_hash import fnv1a  # noqa: E402
from zero_odf import property_name  # noqa: E402
from zero_vehicle import leg_pairs, sections  # noqa: E402


class Aimers(unittest.TestCase):
    def test_the_keys_resolve_to_their_names(self):
        for name in ('BarrelNodeName', 'BarrelRecoil', 'BarrelLength', 'NextBarrel', 'FireNodeName', 'HierarchyLevel',
                     'AimerYawRate', 'AimerPitchRate', 'FireOutsideLimits', 'LegRayHitLength', 'TerrainLeft', 'WalkerWidth'):
            self.assertEqual(property_name(fnv1a(name)), name)

    def test_an_aimer_takes_the_keys_up_to_next_aimer_whatever_their_order(self):
        # The AT-ST gunner's: each gun's limits before its AimerNodeName.
        seats = sections([
            ('ControlSection', 'BODY'), ('ControlSection', 'TURRET1'), ('YawLimits', '-50 50'), ('WeaponSection', '1'),
            ('WeaponName', 'gun'), ('AimerYawLimits', '-90 25'), ('AimerPitchLimits', '-25 25'), ('AimerNodeName', 'aimer_Reargun'),
            ('FirePointName', 'hp_gun_2'), ('NextAimer', '-'), ('AimerYawLimits', '-25 90'), ('AimerPitchLimits', '-25 25'),
            ('AimerNodeName', 'aimer_Leargun'), ('FirePointName', 'hp_gun_1'),
        ])
        aimers = seats[1]['weapons'][0]['aimers']
        self.assertEqual(aimers, [
            {'AimerYawLimits': '-90 25', 'AimerPitchLimits': '-25 25', 'node': 'aimer_Reargun', 'FirePointName': 'hp_gun_2'},
            {'AimerYawLimits': '-25 90', 'AimerPitchLimits': '-25 25', 'node': 'aimer_Leargun', 'FirePointName': 'hp_gun_1'},
        ])
        self.assertNotIn('AimerYawLimits', seats[1]['settings'])

    def test_barrels_take_turns_under_one_aimer(self):
        # The AT-AT pilot's: the head is the aimer, its two chin cannons the
        # barrels; the tips' "FirePoint" key is read by nothing.
        seats = sections([
            ('ControlSection', 'BODY'), ('AimerNodeName', 'aimer_head'), ('WeaponName', 'chin'), ('AimerPitchLimits', '-30 15'),
            ('AimerYawLimits', '-70 70'), ('FireOutsideLimits', '1'), ('BarrelNodeName', 'barrel_Lcannon'), ('0df83e75', 'hp_fire_Lcannon'),
            ('BarrelRecoil', '0.4'), ('NextBarrel', '-'), ('AimerPitchLimits', '-30 15'), ('AimerYawLimits', '-70 70'),
            ('FireOutsideLimits', '1'), ('BarrelNodeName', 'barrel_Rcannon'), ('0df83e75', 'hp_fire_Rcannon'), ('BarrelRecoil', '0.4'),
        ])
        head = seats[0]['aimers'][0]
        self.assertEqual(head['node'], 'aimer_head')
        self.assertEqual(head['FireOutsideLimits'], '1')
        self.assertEqual(head['barrels'], [{'BarrelNodeName': 'barrel_Lcannon', 'BarrelRecoil': '0.4'},
                                           {'BarrelNodeName': 'barrel_Rcannon', 'BarrelRecoil': '0.4'}])
        # The AT-ST pilot's: a parent yaw aimer, then the mouth with two
        # barrels, each with its own fire point.
        seats = sections([
            ('ControlSection', 'BODY'), ('WeaponName', 'chin'), ('HierarchyLevel', '1'), ('AimerNodeName', 'cube5'),
            ('NextAimer', '-'), ('AimerNodeName', 'aimer_mouthpivot'), ('BarrelNodeName', 'barrel_lgun'), ('FirePointName', 'hp_cannon_1'),
            ('NextBarrel', '-'), ('BarrelNodeName', 'barrel_rgun'), ('FirePointName', 'hp_cannon_2'),
        ])
        cube, mouth = seats[0]['weapons'][0]['aimers']
        self.assertEqual(cube, {'HierarchyLevel': '1', 'node': 'cube5'})
        self.assertEqual([b['FirePointName'] for b in mouth['barrels']], ['hp_cannon_1', 'hp_cannon_2'])

    def test_a_record_that_aims_nothing_leaves_its_fire_point_to_the_seat(self):
        seats = sections([('ControlSection', 'BODY'), ('WeaponName', 'laser'), ('FirePointName', 'hp_fire')])
        self.assertEqual(seats[0]['weapons'][0]['aimers'], [])
        self.assertEqual(seats[0]['settings']['FirePointName'], 'hp_fire')

    def test_leg_pairs_stand_on_their_terrain_primitives(self):
        props = [('LegRayHitLength', '16'), ('LegPairCount', '2'),
                 ('LegPair', 'front_legs'), ('TerrainLeft', 'p_front_left'), ('TerrainRight', 'p_front_right'),
                 ('LegPair', 'back_legs'), ('TerrainLeft', 'p_back_left'), ('TerrainRight', 'p_back_right')]
        # Spheres (kind 0) of radius 0.5 at the feet, in the hull's frame.
        shape = lambda name, x, z: {'name': name, 'kind': 0, 'size': (0.5, 0, 0),
                                    'matrix': [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, 0.5, z, 1]}
        shapes = [shape('p_front_left', 2, 5), shape('p_front_right', -2, 5), shape('p_back_left', 2, -5), shape('p_back_right', -2, -5)]
        legs = leg_pairs(props, shapes)
        self.assertEqual(legs['rayLength'], 16.0)
        self.assertEqual([p['name'] for p in legs['pairs']], ['front_legs', 'back_legs'])
        self.assertEqual(legs['pairs'][0]['left']['at'], [2.0, 0.0, 5.0])
        self.assertIsNone(leg_pairs([('LegRayHitLength', '5')], shapes))


if __name__ == '__main__':
    unittest.main()
