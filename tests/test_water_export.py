import struct
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_water import parse_water, water_textures


def chunk(tag, body):
    return tag + struct.pack('<I', len(body)) + body + b'\0' * (-len(body) % 4)


class WaterImport(unittest.TestCase):
    def fixture(self):
        info = struct.pack('<IIfffI', 2, 2, 32, 32, 32, 4)
        layer = b'water\0' + struct.pack('<If', 10, -72.5)
        return chunk(b'WATR', chunk(b'INFO', info) + chunk(b'LAYR', layer) + chunk(b'WMAP', b'\x10\x01'))

    def test_water_grid_and_aliases_preserve_authored_order(self):
        data = self.fixture()
        for keys, expected in [
            ([['RefractionColor', [1,2,3,4], []], ['DiffuseColor', [5,6,7,8], []]], [5,6,7,8]),
            ([['DiffuseColor', [5,6,7,8], []], ['RefractionColor', [1,2,3,4], []]], [1,2,3,4]),
        ]:
            out = parse_water(data, 0, len(data), [[['Effect', ['Water'], keys]]])
            self.assertEqual(out['cells'], [[0,0,0],[1,1,0]])
            self.assertEqual(out['height'], -72.5)
            self.assertEqual(out['scale'], 4)
            self.assertEqual(out['look']['RefractionColor'], expected)
        self.assertIsNone(parse_water(data, 0, len(data), []))

    def test_low_quality_texture_dependencies_include_every_mask_frame(self):
        names = water_textures({'look': {'MainTexture':['yav_water'], 'SpecularMaskTextures':['mask_',3,2]}})
        self.assertTrue({'yav_water','mask_0','mask_1','mask_2'} <= names)


if __name__ == '__main__':
    unittest.main()
