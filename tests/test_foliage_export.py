import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))
from extract import placed_foliage
from zero_odf import property_name
from xform import IDENTITY


class AttachedFoliage(unittest.TestCase):
    def test_original_attachment_hashes(self):
        self.assertEqual(property_name(0xA9D0D48B), "AttachOdf")
        self.assertEqual(property_name(0x3BE7B80A), "AttachToHardPoint")

    def test_crown_uses_tree_hardpoint(self):
        raw = [1, 0, 0, 0, 1, 0, 0, 0, 1, 10, 20, 30]
        records = [{"class": "tree", "xfrm": raw}]
        classes = {
            "tree": {"base": "prop", "props": [("GeometryName", "trunk"), ("AttachOdf", "crown"), ("AttachToHardPoint", "hp_branch_01")]},
            "crown": {"base": "leafpatch", "props": [("NumParticles", "50"), ("Texture", "leaves")]},
        }
        local = list(IDENTITY)
        local[13] = 5
        skeletons = {"trunk": {"names": ["hp_branch_01"], "model": [local]}}
        data = placed_foliage(records, classes, skeletons)
        self.assertEqual(len(data["instances"]), 1)
        self.assertEqual(data["instances"][0]["matrix"][12:15], [10, 25, 30])
        self.assertEqual(data["classes"]["crown"]["NumParticles"], 50)




class AttachmentSequences(unittest.TestCase):
    def test_every_hardpoint_after_an_attachodf_attaches_it(self):
        from extract import attachments
        cls = {"base": "prop", "props": [
            ("GeometryName", "trunk"),
            ("AttachOdf", "leaf"), ("AttachToHardPoint", "hp_1"), ("AttachToHardPoint", "hp_2"),
            ("AttachOdf", "leaf2"), ("AttachToHardPoint", "hp_3"),
            ("AttachOdf", "godray"), ("AttachToHardPoint", "hp_1"),
        ]}
        self.assertEqual(attachments(cls), [("leaf", "hp_1"), ("leaf", "hp_2"), ("leaf2", "hp_3"), ("godray", "hp_1")])

    def test_tree_hangs_every_listed_crown(self):
        raw = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]
        records = [{"class": "tree", "xfrm": raw}]
        classes = {
            "tree": {"base": "prop", "props": [("GeometryName", "trunk"), ("AttachOdf", "leaf"), ("AttachToHardPoint", "hp_1"),
                                               ("AttachToHardPoint", "hp_2"), ("AttachOdf", "leaf2"), ("AttachToHardPoint", "hp_3")]},
            "leaf": {"base": "leafpatch", "props": [("Texture", "leaves")]},
            "leaf2": {"base": "leafpatch", "props": [("Texture", "leaves2")]},
        }
        model = []
        for y in (1, 2, 3):
            local = list(IDENTITY)
            local[13] = y
            model.append(local)
        skeletons = {"trunk": {"names": ["hp_1", "hp_2", "hp_3"], "model": model}}
        data = placed_foliage(records, classes, skeletons)
        self.assertEqual([(i["class"], i["matrix"][13]) for i in data["instances"]], [("leaf", 1), ("leaf", 2), ("leaf2", 3)])
        self.assertEqual(set(data["classes"]), {"leaf", "leaf2"})


class PaintedFoliage(unittest.TestCase):
    def test_folg_uses_fixed_grid_and_negative_z_rows(self):
        import struct
        from zero_terrain import painted_foliage
        rows = bytearray(8)
        rows[0] = 0x10
        rows[1] = 0x03
        rows[7] = 0x08
        data = struct.pack("<I", len(rows)) + bytes(rows)
        for grid, unit in [(2, 8.0), (256, 8.0), (128, 4.0)]:
            painted = painted_foliage(data, (0, len(data)), grid, unit)
            self.assertEqual(painted["cell"], 16.0)
            self.assertEqual(painted["origin"], [-32.0, 32.0])
            self.assertEqual(painted["rowDirection"], -1)
            self.assertEqual(painted["cells"], [[0, 0, 1], [3, 0, 3], [3, 3, 8]])

    def test_layers_preserve_density_alternatives_and_file_meshes(self):
        import struct
        from unittest.mock import patch
        from zero_foliage import foliage_layers, key
        from zero_hash import fnv1a
        hashed = lambda name: struct.unpack('<f', struct.pack('<I', fnv1a(name)))[0]
        field = lambda name, *v: [key(name), list(v), []]
        grass = [key('Mesh'), [], [field('grasspatch', hashed('grass'), 60),
                 field('File', hashed('editor'), 40), field('Frequency', 3)]]
        fern = [key('Mesh'), [], [field('File', hashed('fern'), 35), field('Frequency', 1)]]
        nested = [key('Layer'), [1], [fern]]
        config = [[key('Layer'), [0], [field('SpreadFactor', .2), grass, fern, nested]]]
        with patch('zero_foliage.configs', return_value=[config]):
            layers = foliage_layers(b'', {'grass': {}}, {'fern': [], 'editor': []})
        self.assertEqual(set(layers), {0})
        self.assertEqual((layers[0]['spacing'], layers[0]['count']), (1, 5))
        self.assertEqual([(m['name'], m['kind'], m['distance'], m['cumulative'])
                          for m in layers[0]['meshes']], [('grass', 'grasspatch', 120, .75), ('fern', 'File', 35, 1)])
        self.assertFalse(layers[0]['meshes'][1]['collision'])

    def test_scatter_is_seeded_per_block_and_layer_not_cell_center(self):
        from zero_foliage import placements
        painted = {'side': 512, 'cells': [[256, 256, 1], [257, 256, 1], [256, 257, 1]]}
        mesh = dict(name='grass', cumulative=1)
        layers = {0: dict(spacing=1, count=2, meshes=[mesh])}
        table = [i / 256 for i in range(256)]
        out = list(placements(painted, layers, table))
        self.assertEqual(len(out), 6)
        # Block row/col 128: seed = 128*256+128+2*128 = 33152.
        # Select at table[129], position from [130] and [131].
        self.assertAlmostEqual(out[0][1], (.05 + .9*130/256)*16)
        self.assertAlmostEqual(out[0][2], -(.05 + .9*131/256)*16)
        self.assertEqual(out, list(placements(painted, layers, table)))
        self.assertGreater(out[2][1], 16)
        self.assertLess(out[4][2], -16)

    def test_slope_support_and_decorative_collision(self):
        from unittest.mock import patch
        from zero_foliage import terrain_foliage
        terrain = {'grid': {'size': 8, 'unit': 16},
            'positions': [(x, 10 + x/8 + z/16, z) for x in range(-64, 65, 16) for z in range(-64, 65, 16)],
            'foliage': {'side': 512, 'cells': [[256, 256, 1]]}}
        base = dict(name='grass', kind='grasspatch', cumulative=1, distance=70, stiffness=0, scale=1, variation=0, collision=False)
        layer = lambda mesh: {0: dict(spacing=1, count=1, meshes=[mesh])}
        with patch('zero_foliage.foliage_layers', return_value=layer(base)):
            out = terrain_foliage(b'', terrain, {}, table=[.5]*256,
                water={'height': 100, 'cells': [[0, 0, 0]]})
        inst = out['foliage'][0]
        self.assertEqual(inst['matrix'][12], 8)
        self.assertEqual(inst['matrix'][14], -8)
        self.assertLess(inst['matrix'][4], 0)
        self.assertLess(inst['matrix'][6], 0)
        self.assertAlmostEqual(inst['matrix'][13], 10.458333333333334)
        self.assertEqual(inst['tint'], [1, 1, 1])
        self.assertTrue(inst['terrain'])
        for collide in (False, True):
            mesh = dict(base, name='fern', kind='File', collision=collide)
            with patch('zero_foliage.foliage_layers', return_value=layer(mesh)):
                out = terrain_foliage(b'', terrain, {}, table=[.5]*256)
            self.assertEqual(len(out['instances']), 1)
            self.assertEqual(len(out['collision']), int(collide))

    def test_template_paint_outside_terrain_is_not_clamped(self):
        from unittest.mock import patch
        from zero_foliage import terrain_foliage
        terrain = {'grid': {'size': 2, 'unit': 8}, 'positions': [(0, 0, 0)],
                   'foliage': {'side': 512, 'cells': [[0, 0, 1]]}}
        mesh = dict(name='grass', cumulative=1)
        with patch('zero_foliage.foliage_layers', return_value={0: dict(spacing=1, count=1, meshes=[mesh])}):
            self.assertFalse(terrain_foliage(b'', terrain, {}, table=[.5]*256)['foliage'])


class PlacementTable(unittest.TestCase):
    """The random table is the player's executable's, found by its bytes."""

    TABLE = [((i * 97) % 256 + .5) / 256 for i in range(256)]

    @staticmethod
    def executable(folder, name, sections):
        """A minimal PE image: (name, rva, flags, payload) per section."""
        import struct
        pe, optional = 0x80, 0xe0
        header = bytearray(0x400)
        header[:2] = b'MZ'
        struct.pack_into('<I', header, 0x3c, pe)
        header[pe:pe + 4] = b'PE\0\0'
        struct.pack_into('<HH', header, pe + 4, 0x14c, len(sections))
        struct.pack_into('<H', header, pe + 20, optional)
        body = bytearray()
        for i, (label, rva, flags, payload) in enumerate(sections):
            struct.pack_into('<8sIIIIIIHHI', header, pe + 24 + optional + i * 40,
                             label, len(payload), rva, len(payload), len(header) + len(body), 0, 0, 0, 0, flags)
            body += payload
        path = Path(folder) / name
        path.write_bytes(bytes(header + body))
        return path

    def test_found_where_the_steam_build_keeps_it_or_wherever_another_build_does(self):
        import hashlib
        import struct
        import tempfile
        from unittest.mock import patch
        import zero_foliage
        table = struct.pack('<256f', *self.TABLE)
        decoy = struct.pack('<256f', *reversed(self.TABLE))
        code, data = 0x60000020, 0x40000040
        expected = tuple(struct.unpack('<256f', table))
        with tempfile.TemporaryDirectory() as folder, \
                patch.multiple(zero_foliage, TABLE_RVA=0x2040, TABLE_SHA256=hashlib.sha256(table).hexdigest()):
            zero_foliage.placement_table.cache_clear()
            steam = self.executable(folder, 'steam.exe', [(b'.text', 0x1000, code, bytes(0x200)),
                                                          (b'.rdata', 0x2000, data, bytes(0x40) + table)])
            self.assertEqual(zero_foliage.placement_table(steam), expected)
            # Another build: something else at the Steam build's address,
            # the table further on.
            moved = self.executable(folder, 'moved.exe', [(b'.text', 0x1000, code, bytes(0x200)),
                                                          (b'.rdata', 0x2000, data, bytes(0x40) + decoy + bytes(0x24) + table)])
            self.assertEqual(zero_foliage.placement_table(moved), expected)
            # Only code holds it, or another table stands in its place: refused.
            in_code = self.executable(folder, 'code.exe', [(b'.text', 0x1000, code, table), (b'.rdata', 0x2000, data, bytes(0x40) + decoy)])
            other = self.executable(folder, 'other.exe', [(b'.rdata', 0x2000, data, bytes(0x40) + decoy)])
            for refused in (in_code, other):
                with self.assertRaisesRegex(ValueError, 'Unsupported Battlefront.exe'):
                    zero_foliage.placement_table(refused)
        zero_foliage.placement_table.cache_clear()


class TreeLineOrientation(unittest.TestCase):
    def test_border_faces_across_path_without_mirroring(self):
        from unittest.mock import patch
        from zero_foliage import key
        from extract import tree_lines
        for flip in (False, True):
            fields = [['Distance', [10], []], [key('BorderOdf'), ['border'], []]]
            if flip:
                fields.append([key('Flip'), [], []])
            config = [[key('TreeLine'), [], [[key('Path'), ['jungle'], fields]]]]
            with patch('extract.configs', return_value=[config]):
                out = tree_lines(b'', {'border': {}}, {'jungle': [[0,0,0], [30,0,0]]})
            self.assertEqual(len(out), 2)
            sign = -1 if flip else 1
            self.assertEqual(out[0][1], [sign,0,0, 0,1,0, 0,0,sign, 5,0,0])
            self.assertEqual(out[1][1][9:], [15,0,0])
            m = out[0][1]
            self.assertEqual(m[0]*m[8]-m[2]*m[6], 1, 'rotation must not reflect border artwork')


if __name__ == "__main__":
    unittest.main()
