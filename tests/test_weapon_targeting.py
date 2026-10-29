"""Native launcher defaults and authored salvo properties survive extraction."""
import sys
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import zero_weapons


class WeaponTargeting(unittest.TestCase):
    def extract(self, base, props):
        def classes(data, tag):
            return {'test': {'base': base, 'props': props}} if tag == b'wpnc' else {}
        with patch.object(zero_weapons, 'classes', classes):
            return zero_weapons.weapon_chain([b'fixture'], 'test')

    def test_launcher_default_is_distinct_from_cannon_range_fields(self):
        launcher = self.extract('launcher', [('LockTime', '1')])
        self.assertEqual(launcher['ClearLockOnFire'], 1)
        self.assertEqual(launcher['LockTime'], 1)
        cannon = self.extract('cannon', [('LockTime', '.4'), ('LockOnRange', '64')])
        self.assertEqual(cannon['base'], 'cannon')
        self.assertNotIn('ClearLockOnFire', cannon)

    def test_persistent_lock_and_two_delayed_salvos_remain_authored(self):
        launcher = self.extract('launcher', [('ClearLockOnFire', '0'),
            ('SalvoCount', '2'), ('ShotsPerSalvo', '1'), ('SalvoDelay', '.25'),
            ('InitialSalvoDelay', '.25'), ('ShotDelay', '0'), ('RoundsPerClip', '2')])
        self.assertEqual(launcher['ClearLockOnFire'], 0)
        self.assertEqual([launcher[k] for k in ['SalvoCount', 'ShotsPerSalvo', 'SalvoDelay', 'InitialSalvoDelay', 'ShotDelay', 'RoundsPerClip']], [2, 1, .25, .25, 0, 2])


    def test_only_a_beam_keeps_its_impact_effect(self):
        def classes(data, tag):
            if tag == b'wpnc':
                return {'laser': {'base': 'cannon', 'props': [('OrdnanceName', 'laser_ord')]},
                        'cannon': {'base': 'cannon', 'props': [('OrdnanceName', 'cannon_ord')]}}
            if tag == b'ordc':
                return {'laser_ord': {'base': 'beam', 'props': [('ImpactEffect', 'spiderdroid_ord_exp')]},
                        'cannon_ord': {'base': 'bolt', 'props': [('ImpactEffect', 'med_cannon_impacts')]}}
            return {}
        with patch.object(zero_weapons, 'classes', classes):
            beam = zero_weapons.weapon_chain([b'fixture'], 'laser')
            bolt = zero_weapons.weapon_chain([b'fixture'], 'cannon')
        self.assertEqual(beam['ordnance']['ImpactEffect'], 'spiderdroid_ord_exp')
        self.assertNotIn('ImpactEffect', bolt['ordnance'])


if __name__ == '__main__':
    unittest.main()
