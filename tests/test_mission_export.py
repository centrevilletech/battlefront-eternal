"""Every battle of a map: the mission scripts an import holds for each map and
era (and the historical versions, marked), the world each is fought in, the
battle a side picks, each battle's vehicle classes and sound, and what the
shell, the spawn screen and the vehicle refresh make of them. Synthetic
inputs; the last class reads the installed game when there is one.

    python3 -m unittest tests/test_mission_export.py
"""
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import extract  # noqa: E402
import zero_shell  # noqa: E402
import zero_spawn_screen  # noqa: E402
import zero_vehicle  # noqa: E402

IDENTITY = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]


def setup(*sides, teams=None):
    return {'teams': teams or {'1': {'classes': [[f'{sides[0]}_inf_trooper', 10]]}, '2': {'classes': [[f'{sides[1]}_inf_trooper', 10]]}},
            'attacker': 1, 'deathRegions': [], 'sides': list(sides), 'spawnDelay': None}


# Theed's scripts as mission.lvl has them, with the worlds they read.
THEED = {'nab2a': (setup('all', 'imp'), 'nab2'), 'nab2c': (setup('rep', 'cis'), 'nab2'), 'nab2c_h': (setup('gar', 'cis'), 'nab2_h'),
         'nab2x9': (setup('rep', 'cis'), 'nab2'), 'nab22c': (setup('rep', 'cis'), 'nab22')}
# The Dune Sea's: its campaign version reads tat1h.lvl, a copy of tat1.lvl.
DUNE_SEA = {'tat1i': (setup('all', 'imp', 'des'), 'tat1'), 'tat1i_h': (setup('all', 'imp', 'des'), 'tat1h'), 'tat1r': (setup('rep', 'cis', 'des'), 'tat1')}


def scripts(*tables):
    """mission.lvl's scripts as the tools see them (zero_addon.missions), each
    chunk standing for its own name, and mission_setup and mission_world
    reading them."""
    found = {name: value for table in tables for name, value in table.items()}
    return (mock.patch.object(extract.zero_addon, 'missions', lambda *args: {name: name for name in found}),
            mock.patch.object(extract, 'mission_setup', lambda chunk: json.loads(json.dumps(found[chunk][0]))),
            mock.patch.object(extract, 'mission_world', lambda chunk: found[chunk][1]))


class MissionTests(unittest.TestCase):
    def missions(self, name, *tables, same=lambda world, name: False):
        patches = scripts(*tables)
        with patches[0], patches[1], patches[2], mock.patch.object(extract, 'same_world', lambda world, map_name: world == map_name or same(world, map_name)):
            return extract.map_missions(name)

    def test_every_era_and_its_historical_version_are_the_maps_battles(self):
        found = self.missions('nab2', THEED)
        self.assertEqual(sorted(found), ['nab2a', 'nab2c', 'nab2c_h'], 'the map and one era letter, and _h; no other map\'s')
        self.assertEqual([(m, s['era'], s['historical']) for m, s in found.items()],
                         [('nab2a', 'gcw', False), ('nab2c', 'cw', False), ('nab2c_h', 'cw', True)])
        self.assertEqual(found['nab2c']['sides'], ['rep', 'cis'], 'each with its own sides and teams')
        self.assertEqual(found['nab2a']['teams']['1']['classes'], [['all_inf_trooper', 10]])

    def test_a_battle_in_another_world_says_which(self):
        found = self.missions('nab2', THEED)
        self.assertEqual(found['nab2c_h']['world'], 'nab2_h', 'the campaign\'s Theed is NAB/nab2_h.lvl')
        self.assertNotIn('world', found['nab2c'])
        found = self.missions('tat1', DUNE_SEA, same=lambda world, name: world == 'tat1h')
        self.assertNotIn('world', found['tat1i_h'], 'TAT/tat1h.lvl is tat1.lvl byte for byte')

    def test_the_world_a_script_reads(self):
        reads = [('ReadDataFile', ['sound\\nab.lvl;nab2cw']), ('ReadDataFile', ['SIDE\\gar.lvl', 'gar_inf_basic']),
                 ('ReadDataFile', ['NAB\\nab2_h.lvl']), ('SetTeamName', [1, 'CIS'])]
        with mock.patch.object(extract, 'lua_calls', lambda chunk: reads), mock.patch.object(extract, 'load_lua', lambda chunk: chunk):
            self.assertEqual(extract.mission_world(b''), 'nab2_h')
        with mock.patch.object(extract, 'lua_calls', lambda chunk: [('ReadDataFile', ['dc:TAT\\tat3.lvl'])]), \
                mock.patch.object(extract, 'load_lua', lambda chunk: chunk):
            self.assertEqual(extract.mission_world(b''), 'tat3', 'an add-on\'s own world')

    def test_same_world_by_name_or_by_bytes(self):
        with tempfile.TemporaryDirectory() as root:
            folder = Path(root) / 'TAT'
            folder.mkdir()
            (folder / 'tat1.lvl').write_bytes(b'world')
            (folder / 'tat1h.lvl').write_bytes(b'world')
            (folder / 'tat9.lvl').write_bytes(b'other')
            with mock.patch.object(extract, 'map_archive', lambda name: folder / f'{name}.lvl'):
                self.assertTrue(extract.same_world('tat1', 'tat1'))
                self.assertTrue(extract.same_world('tat1h', 'tat1'))
                self.assertFalse(extract.same_world('tat9', 'tat1'))
                self.assertFalse(extract.same_world('tat5', 'tat1'), 'an archive that is not there')

    def test_a_side_picks_its_eras_battle_never_a_historical_one(self):
        patches = scripts(THEED, DUNE_SEA)
        with patches[0], patches[1], patches[2], mock.patch.object(extract, 'same_world', lambda world, name: world in (name, name + 'h')):
            self.assertEqual(extract.pick_mission('nab2', 'rep')[0], 'nab2c')
            self.assertEqual(extract.pick_mission('nab2', 'cis')[0], 'nab2c')
            self.assertEqual(extract.pick_mission('nab2', 'imp')[0], 'nab2a')
            self.assertEqual(extract.pick_mission('nab2', 'gar'), (None, None), 'only the campaign fields the Naboo Guard')
            self.assertEqual(extract.pick_mission('tat1', 'des')[0], 'tat1r', 'a local side in both eras: the Clone Wars battle')
            self.assertEqual(extract.pick_mission('tat1', 'all')[0], 'tat1i')

    def test_each_battle_keeps_its_sound_the_eras_own_last(self):
        found = {'nab2a': {'historical': False}, 'nab2c': {'historical': False}, 'nab2c_h': {'historical': True, 'world': 'nab2_h'},
                 'nab2a_h': {'historical': True}}
        written = []
        with mock.patch.object(extract, 'map_missions', lambda name: found), \
                mock.patch.object(extract, 'extract_audio', lambda name, side='rep', mission=None: written.append(mission)):
            extract.extract_map_audio('nab2')
        self.assertEqual(written, ['nab2a_h', 'nab2a', 'nab2c'], 'a shared sub-level keeps its era\'s battle; another world\'s waits for it')


class VehicleTests(unittest.TestCase):
    PAD = {'name': 'cp2_vspawn', 'class': 'com_item_vehicle_spawn', 'xfrm': IDENTITY, 'props': [
        ('ControlZone', 'cp2'), ('ClassCISATK', 'cis_hover_aat'), ('ClassRepDEF', 'rep_hover_fightertank'),
        ('ClassAllATK', 'all_hover_combatspeeder'), ('ClassImpDEF', 'imp_hover_fightertank'),
        ('ClassHisATK', 'cis_hover_aat'), ('ClassHisDEF', 'nab_hover_gianspeeder'), ('ClassLocDEF', 'gun_walk_kaadu')]}

    def test_each_battle_takes_its_own_factions_classes(self):
        cw = zero_vehicle.vehicle_spawns([self.PAD], {'1': 'cis', '2': 'rep'}, 1)
        gcw = zero_vehicle.vehicle_spawns([self.PAD], {'1': 'all', '2': 'imp'}, 1)
        self.assertEqual(cw[0]['classes'], {'1': 'cis_hover_aat', '2': 'rep_hover_fightertank'})
        self.assertEqual(gcw[0]['classes'], {'1': 'all_hover_combatspeeder', '2': 'imp_hover_fightertank'})

    def test_a_historical_battle_takes_the_pads_historical_classes(self):
        spawns = zero_vehicle.vehicle_spawns([self.PAD], {'1': 'cis', '2': 'gar', '3': 'gun'}, 1, historical=True)
        self.assertEqual(spawns[0]['classes'], {'1': 'cis_hover_aat', '2': 'nab_hover_gianspeeder', '3': 'gun_walk_kaadu'},
                         'teams 1 and 2 read ClassHis*; a local team its own')

    def test_each_battle_has_a_pack_of_its_own(self):
        self.assertEqual(zero_vehicle.pack_file('nab2a'), 'vehicles/nab2a/vehicles.json')
        with tempfile.TemporaryDirectory() as root:
            public = Path(root)
            folder = public / 'maps' / 'nab2'
            (folder / 'vehicles' / 'textures').mkdir(parents=True)
            for name in ('vehicles.json', 'models.bin'):
                (folder / 'vehicles' / name).write_text('old')
            (folder / 'map.json').write_text(json.dumps({'vehicles': 'vehicles/vehicles.json', 'missions': {
                'nab2a': {'mission': {'name': 'nab2a'}}, 'nab2c': {'mission': {'name': 'nab2c'}},
                'nab2c_h': {'mission': {'name': 'nab2c_h', 'world': 'nab2_h'}}}}))
            made = []
            with mock.patch.object(extract, 'PUBLIC', public), \
                    mock.patch.object(zero_vehicle, 'extract_vehicles', lambda name, mission, shared=None, setup=None: made.append(mission)):
                zero_vehicle.refresh_map('nab2')
            world = json.loads((folder / 'map.json').read_text())
            self.assertEqual(made, ['nab2a', 'nab2c'], 'not the battle fought in another world')
            self.assertEqual({m: b.get('vehicles') for m, b in world['missions'].items()},
                             {'nab2a': 'vehicles/nab2a/vehicles.json', 'nab2c': 'vehicles/nab2c/vehicles.json', 'nab2c_h': None})
            self.assertNotIn('vehicles', world, 'no pack outside a battle')
            self.assertEqual(sorted(p.name for p in (folder / 'vehicles').iterdir()), [], 'the one pack of an older import is gone')


class ShellAndScreenTests(unittest.TestCase):
    def test_the_shell_offers_every_era_the_import_holds(self):
        with tempfile.TemporaryDirectory() as root:
            public = Path(root)
            for name, world in {
                'nab2': {'missions': {'nab2a': {'mission': {}}, 'nab2c': {'mission': {}}, 'nab2c_h': {'mission': {'world': 'nab2_h'}}}},
                'geo1': {'mission': {'name': 'geo1r'}},  # an older import: its one battle
            }.items():
                (public / 'maps' / name).mkdir(parents=True)
                (public / 'maps' / name / 'map.json').write_text(json.dumps(world))
            held = zero_shell.extracted_missions(public)
        self.assertEqual(held, {'geo1': ['geo1r'], 'nab2': ['nab2a', 'nab2c']})
        table = [{'mapluafile': 'nab2', 'showstr': 'NABOO: THEED', 'side_c': 1, 'side_a': 1}, {'mapluafile': 'geo1', 'showstr': 'GEO', 'side_r': 1}]
        maps = zero_shell.map_catalog(table, {}, {}, held, {}, {})
        self.assertEqual({m['id']: {era: v['extracted'] for era, v in m['eras'].items()} for m in maps},
                         {'nab2': {'cw': True, 'gcw': True}, 'geo1': {'cw': True}})

    def test_each_battle_has_its_spawn_screen(self):
        with tempfile.TemporaryDirectory() as root:
            public = Path(root)
            folder = public / 'maps' / 'nab2'
            folder.mkdir(parents=True)
            (folder / 'map.json').write_text(json.dumps({'commandPosts': [{'name': 'cp1'}], 'missions': {
                'nab2a': {'mission': {'name': 'nab2a'}}, 'nab2c': {'mission': {'name': 'nab2c'}},
                'nab2c_h': {'mission': {'name': 'nab2c_h', 'world': 'nab2_h'}}}}))
            screen = lambda mission: {'cameraShots': [], 'teams': {'1': {'side': mission, 'objectives': []}, '2': {'side': mission, 'objectives': []}}, 'postVehicles': {}}
            with mock.patch.object(extract, 'map_archive', lambda name: folder / 'map.json'), \
                    mock.patch.object(zero_spawn_screen, 'map_spawn_screen', lambda data, mission, posts, locl, game=None: screen(mission)):
                zero_spawn_screen.refresh_map('nab2', {}, public=public)
            world = json.loads((folder / 'map.json').read_text())
        self.assertEqual({m: (b.get('spawnScreen') or {}).get('teams', {}).get('1', {}).get('side') for m, b in world['missions'].items()},
                         {'nab2a': 'nab2a', 'nab2c': 'nab2c', 'nab2c_h': None})
        self.assertNotIn('spawnScreen', world)


STOCK = ['bes1', 'bes2', 'end1', 'geo1', 'hot1', 'kam1', 'kas1', 'kas2', 'nab1', 'nab2', 'rhn1', 'rhn2', 'tat1', 'tat2', 'yav1', 'yav2']


@unittest.skipUnless((extract.GAME / 'mission.lvl').exists(), 'requires the installed game (BFE_GAME_DIR)')
class RetailMissions(unittest.TestCase):
    def test_every_map_holds_each_of_its_eras_and_marks_its_historical_versions(self):
        eras = {}
        for name in STOCK + (['tat3'] if extract.map_archive('tat3').exists() else []):
            found = extract.map_missions(name)
            with self.subTest(map=name):
                regular = {m: s for m, s in found.items() if not s['historical']}
                self.assertTrue(regular, 'a regular battle')
                self.assertEqual(len({s['era'] for s in regular.values()}), len(regular), 'one regular battle per era')
                for mission, s in found.items():
                    self.assertEqual(s['historical'], mission.endswith('_h'))
                    self.assertIn(s['era'], ('cw', 'gcw'))
                    army = {'cw': {'rep', 'cis'}, 'gcw': {'all', 'imp'}}[s['era']]
                    self.assertTrue(army & set(s['sides']), f'{mission} fields an army of its era')
                    self.assertTrue(s['world'] if mission in ('nab1c_h', 'nab2c_h') else 'world' not in s, mission)
                eras[name] = sorted(s['era'] for s in regular.values())
                for side, era in (('rep', 'cw'), ('imp', 'gcw')):
                    mission, picked = extract.pick_mission(name, side)
                    if era in eras[name]:
                        self.assertEqual((picked['era'], picked['historical']), (era, False), f'{side} picks {mission}')
        self.assertEqual([m for m, e in eras.items() if e == ['cw', 'gcw']],
                         [m for m in eras if m not in ('end1', 'geo1', 'hot1', 'kam1')], 'both eras where the shell lists both')


if __name__ == '__main__':
    unittest.main()
