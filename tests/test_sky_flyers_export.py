"""The sky's flyers: the sky config's SkyObjects with the original game's
defaults, and the battles whose scripts load their models
(tools/zero_sky.py; docs/formats.md "Sky flyers")."""
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_config import NAMES
from zero_hash import fnv1a
from zero_paths import GAME
from zero_sky import Loads, battle_flyers, era_flyers, flyer_era, loaded_sections, sky_objects


def entry(name, *values, children=()):
    return [name, list(values), [list(child) for child in children]]


def sky_object(*settings):
    return entry('SkyObject', children=settings)


def chunk(tag, body):
    return tag + struct.pack('<I', len(body)) + body + b'\0' * (-len(body) % 4)


def model(name):
    return chunk(b'modl', chunk(b'NAME', name.encode() + b'\0'))


def sub_level(name, *bodies):
    inner = b''.join(bodies)
    return chunk(b'lvl_', struct.pack('<II', fnv1a(name), len(inner)) + inner)


class SkyObjects(unittest.TestCase):
    def test_unset_keys_take_the_executables_defaults(self):
        [flyer] = sky_objects([entry('SkyInfo'), sky_object(entry('Geometry', 'rep_fly_assault_DOME'))])
        self.assertEqual(flyer, {'model': 'rep_fly_assault_DOME', 'count': 1, 'height': [70.0, 100.0],
                                 'velocity': [[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]], 'acceleration': [0.0, 0.0, 0.0],
                                 'distance': 300.0, 'inDirection': 3.0, 'lifeTime': 1000.0})

    def test_geonosis_core_ships(self):
        [flyer] = sky_objects([sky_object(
            entry('Geometry', 'cis_fly_techounion_DOME'), entry('NumObjects', 8.0), entry('Height', 0.0, 0.0),
            entry('VelocityY', 10.0, 12.0), entry('Acceleration', 0.0, 2.0, 0.0), entry('Distance', 1000.0), entry('LifeTime', 80.0))])
        self.assertEqual((flyer['count'], flyer['height'], flyer['velocity'], flyer['acceleration'], flyer['distance'], flyer['lifeTime']),
                         (8, [0.0, 0.0], [[0.0, 0.0], [10.0, 12.0], [0.0, 0.0]], [0.0, 2.0, 0.0], 1000.0, 80.0))
        self.assertEqual(flyer['inDirection'], 3.0)

    def test_one_value_ranges_last_keys_and_truncated_counts(self):
        [flyer] = sky_objects([sky_object(
            entry('Geometry', 'imp_fly_destroyer_DOME'), entry('Height', 150.0), entry('VelocityZ', 30.0),
            entry('Distance', 900.0), entry('Distance', 2500.0), entry('NumObjects', 2.9), entry('Acceleration', 1.0),
            entry('InDirectionFactor', 0.0))])
        self.assertEqual(flyer['height'], [150.0, 150.0], 'one value: no range')
        self.assertEqual(flyer['velocity'][2], [30.0, 30.0])
        self.assertEqual(flyer['distance'], 2500.0, 'a key given twice keeps its last value')
        self.assertEqual(flyer['count'], 2, 'NumObjects is truncated')
        self.assertEqual(flyer['acceleration'], [1.0, 0.0, 0.0])
        self.assertEqual(flyer['inDirection'], 0.0)
        self.assertEqual(sky_objects([sky_object(entry('NumObjects', 3.0))])[0]['model'], None, 'kept without a model, to keep the order')
        self.assertEqual(sky_objects([]), [])

    def test_config_names_resolve(self):
        for name in ('SkyObject', 'Geometry', 'NumObjects', 'Height', 'VelocityX', 'VelocityY', 'VelocityZ',
                     'Acceleration', 'Distance', 'InDirectionFactor', 'LifeTime'):
            self.assertEqual(NAMES[fnv1a(name)], name)


class SubLevels(unittest.TestCase):
    ARCHIVE = chunk(b'ucfb', sub_level('rep_fly_assault_dome', model('rep_fly_assault_dome'))
                    + sub_level('rep_inf_basic', model('rep_inf_trooper'), model('rep_weap_rifle')))

    def names(self, sections, data):
        from zero_sky import _model_names
        return [sorted(_model_names(data, b, e)) for b, e in sections]

    def test_a_named_sub_level_loads_its_models_only(self):
        data = self.ARCHIVE
        self.assertEqual(self.names(loaded_sections(data, ['rep_fly_assault_dome']), data), [['rep_fly_assault_dome']])
        self.assertEqual(self.names(loaded_sections(data, ['REP_INF_BASIC']), data), [['rep_inf_trooper', 'rep_weap_rifle']],
                         'sub-level names hash without regard to case')
        self.assertEqual(loaded_sections(data, ['rep_fly_gunship_dome']), [])

    def test_no_sub_level_named_loads_the_top_level(self):
        data = chunk(b'ucfb', model('geo_bldg_hangar') + sub_level('geo1_extra', model('hidden')))
        self.assertEqual(self.names(loaded_sections(data, []), data), [['geo_bldg_hangar']])
        self.assertEqual(loaded_sections(b'RIFF' + b'\0' * 12, []), [])


class Battles(unittest.TestCase):
    def test_a_battle_draws_the_flyers_whose_models_it_loads(self):
        flyers = sky_objects([sky_object(entry('Geometry', 'rep_fly_assault_DOME'), entry('NumObjects', 2.0)),
                              sky_object(entry('Geometry', 'cis_fly_fedlander_DOME')),
                              sky_object(entry('Geometry', 'imp_fly_destroyer_DOME'), entry('NumObjects', 0.0)),
                              sky_object(entry('NumObjects', 4.0))])

        class Fake(Loads):
            def models(self, chunk, root=None):
                return {name: (chunk, 0, 1, 'rep') for name in {'cw': ['rep_fly_assault_dome', 'rep_inf_trooper'],
                                                                 'gcw': ['imp_fly_destroyer_dome'], 'none': []}[chunk]}

        drawn, sources = battle_flyers(flyers, {'bes2r': ('cw', None), 'bes2a': ('gcw', None), 'bes2x': ('none', None)}, Fake())
        self.assertEqual(drawn, {'bes2a': [], 'bes2r': [0], 'bes2x': []}, 'no ships without a model, or with none to fly')
        self.assertEqual(sources, {'rep_fly_assault_dome': ('cw', 0, 1, 'rep')})

    def test_a_class_takes_its_sides_era(self):
        self.assertEqual(flyer_era('rep_fly_assault_dome', 'rep'), 'cw')
        self.assertEqual(flyer_era('cis_fly_fedlander_dome', 'CIS'), 'cw')
        self.assertEqual(flyer_era('all_fly_moncalamari_dome', 'all'), 'gcw')
        self.assertEqual(flyer_era('imp_fly_destroyer_dome', None), 'gcw', 'without its archive, by its name')
        self.assertEqual(flyer_era('rep_fly_odd_dome', 'imp'), 'gcw', 'the archive holding it decides')
        self.assertIsNone(flyer_era('tat_fly_skiff_dome', 'des'))
        self.assertIsNone(flyer_era(None))

    def test_the_enhanced_sky_draws_the_battles_era(self):
        flyers = sky_objects([sky_object(entry('Geometry', 'rep_fly_assault_DOME'), entry('NumObjects', 2.0)),
                              sky_object(entry('Geometry', 'cis_fly_fedlander_DOME')),
                              sky_object(entry('Geometry', 'imp_fly_destroyer_DOME')),
                              sky_object(entry('Geometry', 'des_fly_skiff_DOME')),
                              sky_object(entry('Geometry', 'all_fly_moncalamari_DOME')),
                              sky_object(entry('Geometry', 'imp_fly_missing_DOME'))])
        sources = {'rep_fly_assault_dome': (b'', 0, 1, 'rep'), 'cis_fly_fedlander_dome': (b'', 0, 1, 'cis'),
                   'imp_fly_destroyer_dome': (b'', 0, 1, 'imp'), 'des_fly_skiff_dome': (b'', 0, 1, 'des'),
                   'all_fly_moncalamari_dome': (b'', 0, 1, 'all')}
        # bes2r loads the assault ship and the skiff; bes2a the Republic's ship.
        drawn = {'bes2r': [0, 3], 'bes2a': [0], 'bes2x': []}
        enhanced, era = era_flyers(flyers, drawn, {'bes2r': 'cw', 'bes2a': 'gcw', 'bes2x': None}, sources)
        self.assertEqual(era, ['cw', 'cw', 'gcw', None, 'gcw', 'gcw'])
        self.assertEqual(enhanced['bes2r'], [0, 1, 3], "the era's ships, and its own loaded one with no era")
        self.assertEqual(enhanced['bes2a'], [2, 4], "never another era's, loaded or not; none without a model")
        self.assertEqual(enhanced['bes2x'], [], 'a battle of no era: only its own with no era')


# The survey (docs/formats.md, "Sky flyers"): every stock map's SkyObjects,
# and what each battle fought in its world draws, as the original and as
# the Enhanced sky.
GEONOSIS = ['rep_fly_assault_DOME', 'rep_fly_gunship_DOME', 'cis_fly_droidfighter_DOME', 'cis_fly_techounion_DOME']
CW = ['rep_fly_assault_DOME', 'cis_fly_fedlander_DOME']
GCW = ['all_fly_moncalamari_DOME', 'imp_fly_destroyer_DOME']
SURVEY = {
    'bes1': (4, {'bes1a': [], 'bes1a_h': [], 'bes1r': []}, {'bes1a': GCW, 'bes1a_h': GCW, 'bes1r': CW}),
    'bes2': (4, {'bes2a': [], 'bes2a_h': [], 'bes2r': ['rep_fly_assault_DOME']}, {'bes2a': GCW, 'bes2a_h': GCW, 'bes2r': CW}),
    'geo1': (4, {'geo1r': GEONOSIS, 'geo1r_h': GEONOSIS}, {'geo1r': GEONOSIS, 'geo1r_h': GEONOSIS}),
    'nab1': (2, {'nab1c': ['cis_fly_fedlander_DOME'], 'nab1i': ['imp_fly_destroyer_DOME']},
             {'nab1c': ['cis_fly_fedlander_DOME'], 'nab1i': ['imp_fly_destroyer_DOME']}),
    'tat1': (4, {'tat1r': ['rep_fly_assault_DOME'], 'tat1i': ['imp_fly_destroyer_DOME'], 'tat1i_h': ['imp_fly_destroyer_DOME']},
             {'tat1r': CW, 'tat1i': GCW, 'tat1i_h': GCW}),
    'tat2': (4, {'tat2r': ['rep_fly_assault_DOME'], 'tat2i': ['imp_fly_destroyer_DOME'], 'tat2i_h': ['all_fly_moncalamari_DOME']},
             {'tat2r': CW, 'tat2i': GCW, 'tat2i_h': GCW}),
    **{name: (0, None, None) for name in ('end1', 'hot1', 'kam1', 'kas1', 'kas2', 'nab2', 'rhn1', 'rhn2', 'tat3', 'yav1', 'yav2')},
}


@unittest.skipUnless((GAME / 'mission.lvl').exists(), 'requires local retail archives')
class RetailSkies(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import extract
        import zero_addon
        from zero_config import configs
        from zero_sky import mission_roots
        cls.extract = extract
        cls.roots = mission_roots()
        cls.loads = Loads()
        cls.skies = {name: (configs(zero_addon.map_archive(name, GAME).read_bytes(), b'sky_') or [[]])[0] for name in SURVEY}

    def battles(self, name):
        scripts = self.extract.map_scripts(name)
        return {m: (scripts[m], self.roots.get(m)) for m, setup in self.extract.map_missions(name).items()
                if m in scripts and not setup.get('world')}

    def eras(self, name):
        return {m: setup.get('era') for m, setup in self.extract.map_missions(name).items()}

    def test_every_map_and_battle(self):
        from zero_sky import dome_sources
        for name, (count, classic, enhanced) in SURVEY.items():
            with self.subTest(map=name):
                flyers = sky_objects(self.skies[name])
                self.assertEqual(len(flyers), count)
                if classic is None:
                    continue
                drawn, sources = battle_flyers(flyers, self.battles(name), self.loads)
                self.assertEqual({m: [flyers[i]['model'] for i in indices] for m, indices in drawn.items()}, classic)
                missing = {f['model'].lower() for f in flyers} - set(sources)
                sources.update(dome_sources(missing, self.loads))
                lists, _ = era_flyers(flyers, drawn, self.eras(name), sources)
                self.assertEqual({m: [flyers[i]['model'] for i in indices] for m, indices in lists.items()}, enhanced)

    def test_geonosis_settings(self):
        gunships = next(f for f in sky_objects(self.skies['geo1']) if f['model'] == 'rep_fly_gunship_DOME')
        self.assertEqual(gunships, {'model': 'rep_fly_gunship_DOME', 'count': 20, 'height': [80.0, 140.0],
                                    'velocity': [[0.0, 0.0], [-10.0, 10.0], [80.0, 120.0]], 'acceleration': [0.0, 0.0, 0.0],
                                    'distance': 600.0, 'inDirection': 0.5, 'lifeTime': 1000.0})

    def test_written_for_the_battles_that_draw_them(self):
        from zero_sky import write_sky_flyers
        with tempfile.TemporaryDirectory() as folder:
            out = Path(folder) / 'geo1' / 'sky'
            content = write_sky_flyers(out, self.skies['geo1'], self.battles('geo1'), loads=self.loads, eras=self.eras('geo1'))
            self.assertEqual([f['count'] for f in content['flyers']], [2, 20, 20, 8])
            self.assertEqual([f['era'] for f in content['flyers']], ['cw'] * 4)
            self.assertEqual(content['missions'], {'geo1r': [0, 1, 2, 3], 'geo1r_h': [0, 1, 2, 3]})
            self.assertEqual(content['enhanced'], content['missions'])
            self.assertEqual(json.loads((out / 'flyers.json').read_text()), content)
            for f in content['flyers']:
                parts = content['models'][f['model']]
                self.assertTrue(parts and all(p['indexCount'] % 3 == 0 for p in parts))
                for part in parts:
                    self.assertTrue((out / content['textures'][part['texture']]).exists())
            size = (out / 'flyers.bin').stat().st_size
            self.assertEqual(size, 32 * sum(p['vertexCount'] for parts in content['models'].values() for p in parts))
            # Bespin: Platforms, which loads no dome: only the Enhanced sky's
            # ships, from the side archives.
            platforms = write_sky_flyers(Path(folder) / 'bes1' / 'sky', self.skies['bes1'], self.battles('bes1'), loads=self.loads,
                                         eras=self.eras('bes1'))
            models = [f['model'] for f in platforms['flyers']]
            self.assertEqual(models, ['rep_fly_assault_dome', 'cis_fly_fedlander_dome', 'all_fly_moncalamari_dome', 'imp_fly_destroyer_dome'])
            self.assertEqual(platforms['missions'], {'bes1a': [], 'bes1a_h': [], 'bes1r': []})
            self.assertEqual(platforms['enhanced'], {'bes1a': [2, 3], 'bes1a_h': [2, 3], 'bes1r': [0, 1]})
            self.assertEqual(sorted(platforms['models']), sorted(models))
            # A map whose battles draw none: no folder, and an old one goes.
            empty = Path(folder) / 'kam1' / 'sky'
            empty.mkdir(parents=True)
            (empty / 'flyers.json').write_text('{}')
            self.assertIsNone(write_sky_flyers(empty, self.skies['kam1'], self.battles('kam1'), loads=self.loads, eras=self.eras('kam1')))
            self.assertFalse(empty.exists())


if __name__ == '__main__':
    unittest.main()
