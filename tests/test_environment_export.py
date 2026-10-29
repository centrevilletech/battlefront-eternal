"""The repeated ODF properties are ordered data, not a key/value dictionary."""
import sys
from pathlib import Path
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_environment import attachments


class EnvironmentAttachments(unittest.TestCase):
    def test_combined_fountain_attachment(self):
        self.assertEqual(list(attachments([('576b09cd', 'waterfountain hp_water')])),
                         [('waterfountain', 'hp_water')])

    def test_repeated_steam_attachments_preserve_pairs(self):
        properties = [('GeometryName', 'dais')]
        for i in range(1, 10):
            properties += [('6a6c7e0d', 'prop_steamgrate'), ('AttachToHardPoint', f'hp_steam_{i}')]
        self.assertEqual(list(attachments(properties)),
                         [('prop_steamgrate', f'hp_steam_{i}') for i in range(1, 10)])

    def test_an_effect_attaches_at_every_hardpoint_until_the_next_attach(self):
        # The original game keeps the named effect pending: Geonosis's Techno
        # Union hull names vehicleflame once for hp_fire and hp_fire1.
        properties = [('AttachEffect', 'smoke_billow'), ('AttachToHardPoint', 'hp_smoke'),
                      ('AttachEffect', 'vehicleflame'), ('AttachToHardPoint', 'hp_fire'), ('AttachToHardPoint', 'hp_fire1'),
                      ('AttachOdf', 'geo_bldg_godray'), ('AttachToHardPoint', 'hp_god 20_0')]
        self.assertEqual(list(attachments(properties)),
                         [('smoke_billow', 'hp_smoke'), ('vehicleflame', 'hp_fire'), ('vehicleflame', 'hp_fire1')])

    def test_at_most_thirty_attachments_a_class(self):
        properties = [('AttachEffect', 'torchflame')] + [('AttachToHardPoint', f'hp_{i}') for i in range(40)]
        self.assertEqual(len(list(attachments(properties))), 30)


if __name__ == '__main__':
    unittest.main()
