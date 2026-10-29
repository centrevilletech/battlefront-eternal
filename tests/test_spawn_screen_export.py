"""Spawn screen export: camera shots in three's camera frame, team sides and
objectives in mission order, the vehicles each post gives each team, display
names, and the asset lists. Synthetic inputs; the last class audits an
existing extraction against docs/spawn-screen-assets.md when there is one."""
import json
import math
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_hash import fnv1a
from zero_spawn_screen import (
    RASTER_HUD, SPAWN_MODELS, SPAWN_STRINGS, SPAWN_TEXTURES, camera_shot, display_text, post_vehicles, spawn_screen,
    spawn_strings, unit_name, weapon_name,
)

PUBLIC = Path(__file__).resolve().parents[1] / 'public'


def table(**texts):
    """A localization table: key -> text, keyed by hash as Locl is."""
    return {fnv1a(key.replace('__', '.')): text for key, text in texts.items()}


def spawn(*pairs, cls='com_item_vehicle_spawn', name=''):
    """A placed object with (property name, value) pairs as raw hashes."""
    return {'class': cls, 'name': name, 'xfrm': [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0],
            'props': [(fnv1a(key), value) for key, value in pairs]}


def rotate(q, v):
    """v turned by the unit quaternion q = [x, y, z, w]."""
    x, y, z, w = q
    tx, ty, tz = 2 * (y * v[2] - z * v[1]), 2 * (z * v[0] - x * v[2]), 2 * (x * v[1] - y * v[0])
    return [v[0] + w * tx + y * tz - z * ty, v[1] + w * ty + z * tx - x * tz, v[2] + w * tz + x * ty - y * tx]


class CameraShotTests(unittest.TestCase):
    def test_lua_order_becomes_three_order_in_the_archive_frame(self):
        shot = camera_shot([2.0, 0.0, 0.0, 0.0, -6.5, -59.25, 26.0])
        self.assertEqual(shot, {'position': [-6.5, -59.25, 26.0], 'quaternion': [0.0, 0.0, 0.0, 1.0]},
                         'normalized, w last, position untouched')

    def test_the_rotation_is_a_three_camera_looking_down_its_minus_z(self):
        half = math.radians(90) / 2
        shot = camera_shot([math.cos(half), 0.0, math.sin(half), 0.0, 0.0, 0.0, 0.0])
        forward = rotate(shot['quaternion'], [0, 0, -1])
        for got, want in zip(forward, [-1, 0, 0]):
            self.assertAlmostEqual(got, want)
        # A shot pitched about x by a positive angle looks up, as three's
        # camera does with the same quaternion.
        shot = camera_shot([math.cos(0.1), math.sin(0.1), 0.0, 0.0, 0.0, 0.0, 0.0])
        self.assertGreater(rotate(shot['quaternion'], [0, 0, -1])[1], 0)


class NameTests(unittest.TestCase):
    LOCL = table(entity__rep__inf_clone_trooper='CLONE', weapons__rep__weap__inf_mine_dispenser='MINES\r\n',
                 entity__rep__walk_atte='WALKER')

    def test_keys_and_trailing_line_breaks(self):
        self.assertEqual(unit_name(self.LOCL, 'rep_inf_clone_trooper'), 'CLONE')
        self.assertEqual(weapon_name(self.LOCL, 'rep_weap_inf_mine_dispenser'), 'MINES')
        self.assertEqual(unit_name(self.LOCL, 'REP_Walk_ATTE'), 'WALKER', 'keys hash without regard to case')
        self.assertEqual(display_text(self.LOCL, 'entity.rep.missing'), None)

    def test_a_missing_name_falls_back_to_the_class(self):
        self.assertEqual(unit_name(self.LOCL, 'rep_inf_unknown'), 'rep_inf_unknown')
        self.assertEqual(weapon_name(self.LOCL, 'rep_weap_unknown'), 'rep_weap_unknown')

    def test_screen_strings_are_the_contract_keys(self):
        self.assertEqual(len(SPAWN_STRINGS), 10)
        self.assertEqual(spawn_strings(table(common__none='NONE', common__back='BACK\r\n')),
                         {'common.none': 'NONE', 'common.back': 'BACK'})


class SpawnScreenTests(unittest.TestCase):
    LOCL = table(level__x__objectives__1='TAKE', level__x__objectives__2='HOLD', entity__imp__walk_atst='AT',
                 entity__all__walk_tauntaun='TAUN', entity__all__fly_snowspeeder='SNOW', entity__all__hover_bike='BIKE')
    CALLS = [
        ('SetTeamName', [2.0, 'Alliance']),
        ('SetTeamName', [1.0, 'Empire']),
        ('SetTeamName', [3.0, 'locals']),
        ('AddMissionObjective', [2.0, 'orange', 'level.x.objectives.2']),
        ('AddMissionObjective', [1.0, 'red', 'level.x.objectives.1']),
        ('AddMissionObjective', [1.0, 'red', 'level.x.objectives.missing']),
        ('AddMissionObjective', [3.0, 'red', 'level.x.objectives.1']),
        ('AddMissionObjective', [1.0, 'orange', 'level.x.objectives.2']),
        ('AddCameraShot', [1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0]),
        ('AddCameraShot', [0.0, 0.0, 1.0, 0.0, 4.0, 5.0, 6.0]),
        ('AddCameraShot', [1.0, None, 0.0, 0.0, 4.0, 5.0, 6.0]),
    ]

    def test_teams_objectives_and_shots_in_mission_order(self):
        screen = spawn_screen(self.CALLS, [], ['cp1'], self.LOCL)
        self.assertEqual(screen['teams'], {'1': {'side': 'imp', 'objectives': ['TAKE', 'HOLD']},
                                           '2': {'side': 'all', 'objectives': ['HOLD']}})
        self.assertEqual([s['position'] for s in screen['cameraShots']], [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
                         'a computed argument drops the shot')
        self.assertEqual(screen['cameraShots'][1]['quaternion'], [0.0, 1.0, 0.0, 0.0])
        self.assertEqual(screen['postVehicles'], {})

    def test_each_team_reads_its_faction_and_slot(self):
        records = [
            spawn(('ControlZone', 'CP5'), ('ClassImpATK', 'imp_walk_atst'), ('ClassImpDEF', 'imp_walk_atat'),
                  ('ClassAllATK', 'all_fly_snowspeeder'), ('ClassAllDEF', 'all_walk_tauntaun'), ('ClassRepATK', 'rep_x')),
            # The last of a team's properties wins; classes are listed once.
            spawn(('ClassAllDEF', 'all_hover_bike'), ('ClassAllDEF', 'ALL_Walk_Tauntaun'), ('ControlZone', 'cp5')),
            spawn(('ControlZone', 'shield'), ('ClassAllDEF', 'all_fly_snowspeeder'), ('ClassEmpire', 'imp_walk_atst')),
            spawn(('ControlZone', 'nowhere'), ('ClassAllDEF', 'all_walk_tauntaun')),
            spawn(('ClassAllDEF', 'all_walk_tauntaun')),
            spawn(('ControlZone', 'cp1'), ('ClassAllDEF', 'all_walk_tauntaun'), cls='com_bldg_controlzone'),
        ]
        screen = spawn_screen(self.CALLS, records, ['cp1', 'shield', 'cp5', 'cp7'], self.LOCL)
        self.assertEqual(screen['postVehicles'], {
            'shield': {'1': ['AT'], '2': ['SNOW']},
            'cp5': {'1': ['AT'], '2': ['TAUN']},
        })
        self.assertEqual(list(screen['postVehicles']), ['shield', 'cp5'], 'in map.json post order')

    def test_historical_teams_read_the_historical_slots(self):
        records = [spawn(('ControlZone', 'cp1'), ('ClassImpATK', 'imp_walk_atst'), ('ClassHisATK', 'all_hover_bike'),
                         ('ClassHisDEF', 'all_walk_tauntaun'))]
        factions = {1: 2, 2: 1}
        self.assertEqual(post_vehicles(records, ['cp1'], factions, True, self.LOCL), {'cp1': {'1': ['BIKE'], '2': ['TAUN']}})
        self.assertEqual(post_vehicles(records, ['cp1'], factions, False, self.LOCL), {'cp1': {'1': ['AT'], '2': []}})

    def test_unnamed_teams_have_no_side(self):
        screen = spawn_screen([('SetTeamName', [1.0, 'Republic'])], [], [], {})
        self.assertEqual(screen['teams'], {'1': {'side': 'rep', 'objectives': []}, '2': {'side': None, 'objectives': []}})


class AssetListTests(unittest.TestCase):
    def test_lists(self):
        self.assertEqual(set(SPAWN_TEXTURES), {'border_3_pieces', 'headerbuttonleft', 'headerbuttonmid', 'headerbuttonright',
                                               'cursor', 'opaque_rect', 'mousecursor', 'shell_cube'})
        self.assertEqual(RASTER_HUD, SPAWN_TEXTURES + ('com_icon_controlzone',))
        self.assertEqual(SPAWN_MODELS, ('shell_cube',))


def extracted():
    path = PUBLIC / 'common' / 'common.json'
    return path.exists() and 'rasterHud' in json.loads(path.read_text())


@unittest.skipUnless(extracted(), 'requires an extraction with the spawn screen data (tools/zero_spawn_screen.py)')
class ExtractionAudit(unittest.TestCase):
    """The ignored public/ data against the schema in docs/spawn-screen-assets.md."""

    def test_common(self):
        common = json.loads((PUBLIC / 'common' / 'common.json').read_text())
        for name in SPAWN_TEXTURES:
            self.assertEqual(common['textures'].get(name), f'textures/{name}.png')
            self.assertTrue((PUBLIC / 'common' / common['textures'][name]).exists())
        # The spawn screen's entries come first; the scoreboard's follow (tools/zero_scoreboard.py).
        self.assertEqual(common['rasterHud'][:len(RASTER_HUD)], list(RASTER_HUD))
        room = common['holograms']['shell_cube']
        self.assertEqual([part['texture'] for part in room], ['shell_cube'])
        size = (PUBLIC / 'common' / 'holograms.bin').stat().st_size
        self.assertTrue(all(p['vertexOffset'] + p['vertexCount'] * 32 <= size for parts in common['holograms'].values() for p in parts))
        for key in SPAWN_STRINGS:
            self.assertIsInstance(common['strings'].get(key), str, key)
            self.assertEqual(common['strings'][key], common['strings'][key].rstrip('\r\n'))
        manifest = json.loads((PUBLIC / 'common' / 'raster-hud' / 'manifest.json').read_text())
        pages = [page for font in ('gamefont_tiny', 'gamefont_medium', 'gamefont_large') for page in common['fonts'][font]['pages']]
        for name in list(RASTER_HUD) + pages:
            entry = manifest['textures'].get(common['textures'][name])
            self.assertIsNotNone(entry, name)
            self.assertTrue((PUBLIC / 'common' / 'raster-hud' / entry['file']).exists(), name)

    def test_units(self):
        for path in sorted(PUBLIC.glob('sides/*/units.json')):
            for name, unit in json.loads(path.read_text())['units'].items():
                with self.subTest(unit=name):
                    self.assertIsInstance(unit.get('displayName'), str)
                    self.assertTrue(unit['displayName'] and unit['displayName'] == unit['displayName'].rstrip('\r\n'))
                    for weapon in unit['weapons']:
                        self.assertIsInstance(weapon.get('displayName'), str)
                        self.assertEqual(weapon['displayName'], weapon['displayName'].rstrip('\r\n'))
                        self.assertIn('label', weapon, 'the old label stays')

    def test_maps(self):
        for path in sorted(PUBLIC.glob('maps/*/map.json')):
            world = json.loads(path.read_text())
            # Each battle fought in the map's world (missions), or an older
            # extraction's one.
            battles = world.get('missions', {(world.get('mission') or {}).get('name'): world})
            for mission, battle in battles.items():
                if (battle.get('mission') or {}).get('world'):
                    continue
                with self.subTest(map=path.parent.name, mission=mission):
                    screen = battle['spawnScreen']
                    self.assertGreater(len(screen['cameraShots']), 0)
                    for shot in screen['cameraShots']:
                        self.assertEqual((len(shot['position']), len(shot['quaternion'])), (3, 4))
                        self.assertAlmostEqual(sum(v * v for v in shot['quaternion']), 1.0, places=5)
                    self.assertEqual(sorted(screen['teams']), ['1', '2'])
                    for team in screen['teams'].values():
                        # A historical version's Locals army (the Wookiees of
                        # kas1c_h) is no army's side.
                        if not (battle.get('mission') or {}).get('historical'):
                            self.assertIn(team['side'], ('rep', 'cis', 'imp', 'all'))
                        self.assertTrue(team['objectives'] and all(isinstance(text, str) and text for text in team['objectives']))
                    posts = {post['name'] for post in world['commandPosts']}
                    for post, teams in screen['postVehicles'].items():
                        self.assertIn(post, posts)
                        self.assertEqual(sorted(teams), ['1', '2'])
                        self.assertTrue(all(isinstance(name, str) for names in teams.values() for name in names))


if __name__ == '__main__':
    unittest.main()
