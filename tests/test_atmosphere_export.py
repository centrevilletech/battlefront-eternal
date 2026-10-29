"""The world's haze: FogCloud from the fx config, dusteffect objects from the
world (tools/zero_atmosphere.py)."""
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_atmosphere import CELL, atmosphere_textures, dust_class, fog_cloud, parse_atmosphere
from zero_config import NAMES
from zero_hash import fnv1a
from zero_odf import property_name
from zero_paths import GAME


def entry(name, *values):
    return [name, list(values), []]


def effect(name, *children):
    return ['Effect', [name], list(children)]


GEONOSIS = effect('FogCloud', entry('Enable', 1.0), entry('Texture', 'brownmist'), entry('Range', 90.0, 120.0),
                  entry('Color', 168.0, 172.0, 180.0, 128.0), entry('Velocity', 5.0, 0.0), entry('Rotation', 0.1),
                  entry('Height', 16.0), entry('ParticleSize', 28.0), entry('ParticleDensity', 60.0))
DUST = {'base': 'dusteffect', 'props': [
    ('MinPos', '-50 4 -50'), ('MaxPos', '-10 12 -10'), ('MinVel', '2 0 2'), ('MaxVel', '10 1 10'),
    ('MinSize', '15'), ('MaxSize', '20'), ('MinLifetime', '3'), ('MaxLifetime', '5'),
    ('RadiusFadeMin', '50'), ('RadiusFadeMax', '60'), ('Alpha', '0.5'), ('NumParticles', '25'),
    ('MaxDistance', '150'), ('Texture', 'browndust')]}


class FogCloudImport(unittest.TestCase):
    def test_enabled_effect_keeps_its_settings(self):
        fog = fog_cloud([[GEONOSIS]])
        self.assertEqual(fog, {'texture': 'brownmist', 'range': [90.0, 120.0], 'color': [168, 172, 180, 128],
                               'velocity': [5.0, 0.0], 'rotation': 0.1, 'height': 16.0, 'size': 28.0, 'density': 60, 'cell': CELL})

    def test_disabled_or_absent_effect_invents_nothing(self):
        self.assertIsNone(fog_cloud([]))
        self.assertIsNone(fog_cloud([[effect('FogCloud', entry('Enable', 0.0), entry('Texture', 'fluffy'))]]))
        self.assertIsNone(fog_cloud([[effect('Blur', entry('Enable', 1.0))]]))

    def test_unset_keys_take_the_engines_defaults(self):
        fog = fog_cloud([[effect('FogCloud', entry('Enable', 1.0), entry('Color', 10.0, 20.0, 30.0))]])
        self.assertEqual(fog['texture'], 'fluffy')
        self.assertEqual(fog['range'], [40.0, 90.0])
        self.assertEqual(fog['density'], 30)
        self.assertEqual((fog['size'], fog['height'], fog['rotation']), (28.0, 16.0, 0.1))
        self.assertEqual(fog['color'], [10, 20, 30, 255], 'three values leave the alpha opaque')

    def test_config_names_resolve(self):
        for name in ('Rotation', 'Height', 'ParticleSize', 'ParticleDensity', 'Range', 'CameraCrossVelocityScale', 'RotationRange'):
            self.assertEqual(NAMES[fnv1a(name)], name)


class DustImport(unittest.TestCase):
    def test_class_settings_and_radius(self):
        kind = dust_class(DUST)
        self.assertEqual(kind['count'], 25)
        self.assertEqual((kind['minPos'], kind['maxVel']), ([-50.0, 4.0, -50.0], [10.0, 1.0, 10.0]))
        self.assertEqual(kind['radiusFade'], [50.0, 60.0])
        self.assertEqual(kind['heightScale'], 0.2, 'the engine flattens dust unless told otherwise')
        self.assertEqual(kind['radius'], 70.0, 'half the largest size plus the radius fade end')
        open_box = dust_class({'base': 'dusteffect', 'props': [p for p in DUST['props'] if not p[0].startswith('RadiusFade')]})
        self.assertAlmostEqual(open_box['radius'], 10 + (10 ** 2 * 2 + 1) ** 0.5 * 5 + (50 ** 2 * 2 + 4 ** 2) ** 0.5)
        self.assertEqual(dust_class({'base': 'dusteffect', 'props': [('NumParticles', '8'), ('NumParticles', '12')]})['count'], 12,
                         'the last value set wins')

    def test_placed_copies_keep_only_their_position(self):
        records = [
            {'class': 'geo_prop_dust', 'name': '', 'xfrm': [-1, 0, 0, 0, 1, 0, 0, 0, -1, 10.0, -62.5, 20.0], 'props': []},
            {'class': 'geo_prop_dust', 'name': '', 'xfrm': [1, 0, 0, 0, 1, 0, 0, 0, 1, 30.0, -60.0, 40.0], 'props': []},
            {'class': 'geo_bldg_hull', 'name': '', 'xfrm': [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], 'props': []},
        ]
        classes = {'geo_prop_dust': DUST, 'geo_bldg_hull': {'base': 'prop', 'props': []}}
        haze = parse_atmosphere([[GEONOSIS]], records, classes)
        self.assertEqual(list(haze['dust']['classes']), ['geo_prop_dust'])
        self.assertEqual([i['position'] for i in haze['dust']['instances']], [[10.0, -62.5, 20.0], [30.0, -60.0, 40.0]])
        self.assertEqual(atmosphere_textures(haze), {'brownmist', 'browndust'})
        self.assertIsNone(parse_atmosphere([], records[2:], classes))
        self.assertEqual(atmosphere_textures(None), set())

    def test_property_names_resolve(self):
        for name in ('MinPos', 'MaxPos', 'MinVel', 'MaxVel', 'MinLifetime', 'MaxLifetime', 'HeightScale', 'RadiusFadeMin'):
            self.assertEqual(property_name(fnv1a(name)), name)


# Each map's haze as the retail archives hold it: (FogCloud texture, range,
# density) or None, and the dusteffect copies per class.
RETAIL = {
    'geo1': (('brownmist', [90.0, 120.0], 60), {'geo_prop_dust': 23}),
    'end1': (('fluffy', [85.0, 110.0], 90), {}),
    'rhn2': (('fluffy', [90.0, 120.0], 60), {'Rhn2_prop_cloud': 13}),
    'yav1': (('fluffy', [90.0, 120.0], 60), {'yav_prop_mist': 23}),
    'yav2': (('fluffy', [60.0, 100.0], 60), {'yav_prop_mist': 9}),
    'hot1': (None, {'hoth_prop_dust': 52, 'hoth_prop_mist': 15}),
    'kam1': (None, {'kam1_prop_mist': 22}),
    'tat1': (None, {'tat_prop_dust': 66}),
    'bes1': None, 'bes2': None, 'kas1': None, 'kas2': None, 'nab1': None, 'nab2': None, 'rhn1': None, 'tat2': None,
}


@unittest.skipUnless((GAME / 'mission.lvl').exists(), 'requires local retail archives')
class RetailHaze(unittest.TestCase):
    def test_every_map(self):
        import zero_addon
        from ucfb import nodes
        from zero_config import configs
        from zero_odf import entity_classes
        from zero_world import instance_records
        shared = [(GAME / 'Common' / 'ENG' / 'common.lvl').read_bytes(), (GAME / 'core.lvl').read_bytes()]
        for name, expected in RETAIL.items():
            with self.subTest(map=name):
                data = zero_addon.map_archive(name, GAME).read_bytes()
                classes = {}
                for archive in (data, *shared):
                    for cname, cls in entity_classes(archive).items():
                        classes.setdefault(cname, cls)
                canonical = {c.lower(): c for c in classes}
                records = [r for b, e in nodes(data, b'wrld') for r in instance_records(data, b, e, property_name)]
                for record in records:
                    record['class'] = canonical.get(record['class'].lower(), record['class'])
                haze = parse_atmosphere(configs(data, b'fx__'), records, classes)
                if expected is None:
                    self.assertIsNone(haze)
                    continue
                fog, dust = expected
                found = haze['fogCloud'] and (haze['fogCloud']['texture'], haze['fogCloud']['range'], haze['fogCloud']['density'])
                self.assertEqual(found or None, fog)
                counts = {}
                for i in (haze['dust'] or {}).get('instances', []):
                    counts[i['class']] = counts.get(i['class'], 0) + 1
                self.assertEqual(counts, dust)


if __name__ == '__main__':
    unittest.main()
