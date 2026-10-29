"""AI mission export: calls kept in order, team summaries, and command post
AI properties parsed the way the original game parses them. Synthetic inputs."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_ai_mission import (
    ai_calls, atoi, bonus_mask, class_records, faction_of, map_scripts, native_base, pick_mission, post_record,
    scan_d, scan_i, script_record, team_summary,
)
from zero_hash import fnv1a


def props(*pairs):
    """(name, value) pairs as raw (hash, value) PROP entries."""
    return [(fnv1a(name), value) for name, value in pairs]


IDENTITY = [1, 0, 0, 0, 1, 0, 0, 0, 1, 10, 0, 20]


def record(name, *pairs, cls='cp_class'):
    return {'class': cls, 'name': name, 'xfrm': IDENTITY, 'props': props(*pairs)}


class ParsingTests(unittest.TestCase):
    def test_c_integer_parsers(self):
        self.assertEqual([atoi(v) for v in ['1', ' 2', '-3', '1.0', '0.5', 'true', '', '7x']], [1, 2, -3, 1, 0, 0, 0, 7])
        self.assertEqual([scan_d(v) for v in ['30', ' 12.5', '-4', 'x', '']], [30, 12, -4, None, None])
        self.assertEqual([scan_i(v) for v in ['65536', '0x10', '010', '0', 'y']], [65536, 16, 8, 0, None])

    def test_team_names_parse_to_factions(self):
        names = ['Alliance', 'all', 'Empire', 'IMP', 'Republic', 'rep', 'CIS', 'neutral', 'locals', 'Local', 'Naboo Guard', '']
        self.assertEqual([faction_of(n) for n in names], [1, 1, 2, 2, 3, 3, 4, 0, 5, 5, 5, 5])

    def test_bonus_names(self):
        self.assertEqual([bonus_mask(n) for n in ['HERO', 'Hero', 'training', 'Sabotage', 'nothing']], [8, 8, 0x80, 0x20, 0])


class ScriptTests(unittest.TestCase):
    CALLS = [
        ('ReadDataFile', ['SIDE\\rep.lvl', 'rep_inf_basic']),
        ('SetPlanetaryBonusVoiceOver', [1.0, 'x']),
        ('SetPlayerTeamDifficultyEasy', [2.0]),
        ('SetEnemyTeamDifficultyEasy', [6.0]),
        ('SetTeamName', [1.0, 'CIS']),
        ('AddUnitClass', [1.0, 'droid', 11.0]),
        ('AddUnitClass', [1.0, 'sniper']),
        ('SetHeroClass', [1.0, 'dooku']),
        ('SetUnitCount', [1.0, 22.0]),
        ('SetReinforcementCount', [1.0, 200.0]),
        ('AddBleedThreshold', [1.0, 31.0, 0.0]),
        ('AddBleedThreshold', [1.0, 1.0, 3.0]),
        ('SetTeamAsFriend', [3.0, 1.0]),
        ('SetTeamName', [3.0, 'locals']),
        ('SetTeamAggressiveness', [1.0, 0.949999988079071]),
        ('ActivateBonus', [1.0, 'HERO']),
        ('SetSpawnDelay', [10.0, 0.25]),
        ('SetMaxFlyHeight', [-20.0]),
        ('SetSpawnDelay', [1.0, 0.5]),
    ]

    def test_ai_calls_keep_source_order_and_positions(self):
        calls = ai_calls(self.CALLS)
        self.assertEqual(calls[0], {'at': 2, 'name': 'SetPlayerTeamDifficultyEasy', 'args': [2]})
        self.assertNotIn('ReadDataFile', [c['name'] for c in calls])
        self.assertNotIn('SetPlanetaryBonusVoiceOver', [c['name'] for c in calls])
        self.assertEqual([c['at'] for c in calls], sorted(c['at'] for c in calls))
        self.assertEqual([c['args'] for c in calls if c['name'] == 'SetSpawnDelay'], [[10, 0.25], [1, 0.5]])
        self.assertEqual([c['faction'] for c in calls if c['name'] == 'SetTeamName'], [4, 5])
        self.assertEqual([c['mask'] for c in calls if c['name'] == 'ActivateBonus'], [8])

    def test_team_summary_applies_calls_in_order(self):
        teams = team_summary(ai_calls(self.CALLS))
        cis = teams['1']
        self.assertEqual(cis['faction'], 4)
        self.assertEqual(cis['classes'], [['droid', 11], ['sniper', 0]], 'count defaults to 0')
        self.assertEqual((cis['units'], cis['reinforcements'], cis['hero'], cis['bonus']), (22, 200, 'dooku', 8))
        self.assertEqual(cis['bleed'], [[1, 3.0], [31, 0.0]], 'sorted lowest first')
        self.assertAlmostEqual(cis['aggressiveness'], 0.95, places=6)
        locals_ = teams['3']
        self.assertEqual((locals_['faction'], locals_['reinforcements'], locals_['relations']), (5, 0x7FFFFFFF, {'1': 1}))

    def test_twelve_classes_at_most(self):
        calls = ai_calls([('AddUnitClass', [2.0, f'c{i}', 1.0]) for i in range(14)])
        self.assertEqual(len(team_summary(calls)['2']['classes']), 12)

    def test_script_record(self):
        rec = script_record('xyz1c_h', self.CALLS)
        self.assertEqual(rec['variant'], 'historical')
        self.assertEqual(rec['spawnDelay'], [1, 0.5], 'the last setter wins')
        self.assertEqual([c['name'] for c in rec['difficulty']], ['SetPlayerTeamDifficultyEasy', 'SetEnemyTeamDifficultyEasy'])
        self.assertEqual(rec['sides'], ['rep'])
        self.assertEqual(script_record('xyz1c', [])['spawnDelay'], [15, 0.5])

    def test_map_scripts_and_mission_choice(self):
        scripts = {'nab2a': 0, 'nab2c': 0, 'nab2c_h': 0, 'nab1c': 0, 'nab22': 0}
        self.assertEqual(map_scripts(scripts, 'nab2'), ['nab2a', 'nab2c', 'nab2c_h'])
        records = {'nab2a': {'variant': 'regular', 'sides': ['all', 'imp']},
                   'nab2c': {'variant': 'regular', 'sides': ['rep', 'cis']},
                   'nab2c_h': {'variant': 'historical', 'sides': ['gar', 'cis']}}
        self.assertEqual(pick_mission(records, 'rep'), 'nab2c')
        self.assertEqual(pick_mission(records, 'imp'), 'nab2a')
        self.assertEqual(pick_mission(records, 'rep', preferred='nab2c_h'), 'nab2c_h')


class PostTests(unittest.TestCase):
    def classes(self, *class_props):
        return {'cp_class': {'base': 'commandpost', 'props': props(*class_props)}, 'derived': {'base': 'cp_class', 'props': []}}

    def test_weights_default_100_then_class_then_instance_in_order(self):
        classes = self.classes(('Value_ATK_CIS', '20'), ('Value_DEF_Locals', '5'))
        rec = record('cp1', ('Value_ATK_CIS', '30'), ('Value_ATK_CIS', 'x'), ('Value_ATK_Republic', '12.5'),
                     ('Value_DEF_Empire', '50'))
        out = post_record(0, rec, 'cp_class', classes, {}, [])
        self.assertEqual(out['values']['atk'], [100, 100, 100, 12, 30, 100], 'x leaves 30; 12.5 reads 12')
        self.assertEqual(out['values']['def'], [100, 100, 50, 100, 100, 5])
        self.assertEqual([o[0] for o in out['values']['overrides']], ['class', 'class', 'instance', 'instance', 'instance', 'instance'])
        self.assertEqual(out['valueBleed'], 100)

    def test_bans_only_ever_or_in(self):
        rec = record('cp1', ('HugeBan', '1'), ('HugeBan', '0'), ('SoldierBan', '1.0'), ('FlyerBan', 'true'), ('HoverBan', '-1'))
        out = post_record(0, rec, 'cp_class', self.classes(), {}, [])
        self.assertEqual(out['bans'], 16 | 1, 'a later 0 does not clear; "true" and -1 set nothing')
        self.assertEqual(len(out['banProps']), 5)

    def test_paths_and_regions_resolve_by_name_case_insensitively(self):
        paths = {'CP1Spawn': [[1, 2, 3, 0.5]], 'GeoSpawn1': [[4, 5, 6, 0.0], [7, 8, 9, 1.0]]}
        regions = [{'type': 'box', 'name': 'CP1Capture', 'matrix': [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 7, 1], 'size': [3, 1, 2]}]
        rec = record('cp1', ('Team', '2'), ('SpawnPath', 'cp1spawn'), ('AllyPath', 'GEOSPAWN1'), ('AllyCount', '6'),
                     ('CaptureRegion', 'cp1capture'), ('ControlRegion', 'Missing'), ('Radius', '0.0'))
        out = post_record(3, rec, 'cp_class', self.classes(), paths, regions)
        self.assertEqual((out['order'], out['team'], out['allyCount']), (3, 2, 6))
        self.assertEqual(out['spawnPath'], {'name': 'cp1spawn', 'present': True, 'path': 'CP1Spawn', 'points': [[1, 2, 3, 0.5]]})
        self.assertEqual(len(out['allyPath']['points']), 2)
        self.assertTrue(out['captureRegion']['present'])
        self.assertEqual(out['captureRegion']['center'], [5, 6, 7])
        self.assertFalse(out['controlRegion']['present'])
        self.assertEqual(out['position'], [10, 0, 20])
        missing = post_record(0, record('cp2', ('SpawnPath', 'nowhere')), 'cp_class', self.classes(), paths, regions)
        self.assertEqual(missing['spawnPath']['present'], False)
        self.assertIsNone(missing['allyPath'])

    def test_class_spawn_points(self):
        classes = self.classes(('SpawnPointCount', '2'), ('SpawnPointLocation', '8.0 0.0 0.0 5'),
                               ('SpawnPointLocation', '-8.0 0.0 4.0 355'))
        out = post_record(0, record('walker'), 'cp_class', classes, {}, [])
        self.assertEqual(out['classSpawnPoints'], 2)
        self.assertEqual(out['classSpawnWorld'][0][:3], [18.0, 0.0, 20.0])
        none = self.classes(('SpawnPointLocation', '8.0 0.0 0.0 5'))
        self.assertEqual(post_record(0, record('x'), 'cp_class', none, {}, [])['classSpawnPoints'], 0, 'no count, no array')

    def test_native_base_follows_the_class_chain(self):
        classes = self.classes()
        self.assertEqual(native_base(classes, 'derived'), 'commandpost')
        self.assertEqual(native_base(classes, 'cp_class'), 'commandpost')

    def test_class_records_keep_raw_hashes(self):
        import struct

        def chunk(tag, body):
            body = body + b'\0' * (-len(body) % 4)
            return tag + struct.pack('<I', len(body)) + body

        entc = chunk(b'TYPE', b'cp_class\0') + chunk(b'BASE', b'commandpost\0') + chunk(b'PROP', struct.pack('<I', fnv1a('Value_ATK_CIS')) + b'30\0')
        data = chunk(b'ucfb', chunk(b'entc', entc))
        found = class_records([data])
        self.assertEqual(found['cp_class']['base'], 'commandpost')
        self.assertEqual(found['cp_class']['props'], [(fnv1a('Value_ATK_CIS'), '30')])


if __name__ == '__main__':
    unittest.main()
