"""Each charge level keeps its own shot pattern, as the original game builds it."""
import sys
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import zero_weapons


# The bowcaster's class as the retail ODF chain lists it (abridged): a charge
# level copies the class without its pattern, ShotPatternCount starts a new
# one, and the entries that follow are that level's.
BOWCASTER = [
    ('ShotDelay', '0.35'), ('MaxPressedTime', '1.75'), ('ShotsPerSalvo', '1'),
    ('NextCharge', '0.15'),
    ('NextCharge', '0.35'), ('ShotsPerSalvo', '3'), ('ShotPatternCount', '3'),
    ('ShotPatternPitchYaw', '0.0 0.0'), ('ShotPatternPitchYaw', '0.0 -0.7'), ('ShotPatternPitchYaw', '0.0 0.7'),
    ('NextCharge', '0.65'), ('ShotsPerSalvo', '5'), ('ShotPatternCount', '5'),
    ('ShotPatternPitchYaw', '0.0 0.0'), ('ShotPatternPitchYaw', '0.0 -0.7'), ('ShotPatternPitchYaw', '0.0 0.7'),
    ('ShotPatternPitchYaw', '0.0 -1.4'), ('ShotPatternPitchYaw', '0.0 1.4'),
]


class WeaponPatterns(unittest.TestCase):
    def extract(self, props):
        def classes(data, tag):
            return {'test': {'base': 'cannon', 'props': props}} if tag == b'wpnc' else {}
        with patch.object(zero_weapons, 'classes', classes):
            return zero_weapons.weapon_chain([b'fixture'], 'test')

    def test_each_charge_level_lists_only_its_own_entries(self):
        bow = self.extract(BOWCASTER)
        self.assertNotIn('ShotPatternPitchYaw', bow)
        low, three, five = bow['charges']
        self.assertNotIn('ShotPatternPitchYaw', low)
        self.assertEqual(three['ShotPatternPitchYaw'], [[0.0, 0.0], [0.0, -0.7], [0.0, 0.7]])
        self.assertEqual(five['ShotPatternPitchYaw'], [[0.0, 0.0], [0.0, -0.7], [0.0, 0.7], [0.0, -1.4], [0.0, 1.4]])
        self.assertEqual(five['ShotPatternCount'], 5)

    def test_a_level_without_a_pattern_has_none(self):
        shrapnel = self.extract([('ShotPatternCount', '2'), ('ShotPatternPitchYaw', '-0.1 -0.8'), ('ShotPatternPitchYaw', '0.8 0.0'),
                                 ('MaxPressedTime', '1'), ('NextCharge', '0.5'), ('ShotsPerSalvo', '2')])
        self.assertEqual(shrapnel['ShotPatternPitchYaw'], [[-0.1, -0.8], [0.8, 0.0]])
        self.assertEqual(shrapnel['charges'][0]['ShotPatternPitchYaw'], [])


if __name__ == '__main__':
    unittest.main()
