"""Locals: a side's units are the mission classes its archive defines, and
each command post carries how the maps and holograms show its class.

    python3 -m unittest tests/test_locals_export.py
"""
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import extract  # noqa: E402

IDENTITY = [1, 0, 0, 0, 1, 0, 0, 0, 1, 10, 2, -30]


class LocalSideTests(unittest.TestCase):
    def playable(self, side, defined):
        missions = {
            'tat1r': {'teams': {1: {'classes': [['rep_inf_clone_trooper', 11]], 'hero': 'rep_inf_macewindu'},
                                3: {'classes': [['tat_inf_tuskenraider', 5], ['tat_inf_tuskenhunter', 2]]}}},
            'tat2r': {'teams': {3: {'classes': [['tat_inf_jawa', 14]]}}},
        }
        with tempfile.TemporaryDirectory() as root:
            game = Path(root)
            (game / 'mission.lvl').write_bytes(b'')
            archive = game / f'{side}.lvl'
            archive.write_bytes(b'')
            scripts = {name: name for name in missions}
            # Where the missions come from: mission.lvl, or with add-ons
            # (tools/zero_addon.py) theirs too.
            source = (mock.patch.object(extract.zero_addon, 'missions', lambda *args: scripts) if hasattr(extract, 'zero_addon')
                      else mock.patch.object(extract, 'mission_scripts', lambda data: scripts))
            with mock.patch.object(extract, 'GAME', game), source, mock.patch.dict(extract.ARCHIVE_CLASSES, clear=True), \
                    mock.patch.object(extract, 'side_archive', lambda name: archive), \
                    mock.patch.object(extract, 'mission_setup', lambda name: missions[name]), \
                    mock.patch.object(extract, 'entity_classes', lambda data: {name: {} for name in defined}):
                return extract.playable_units(side)

    def test_tatooines_locals_come_from_des_whatever_their_prefix(self):
        # SIDE\des.lvl defines tat_inf_*: before, des extracted no unit at all.
        self.assertEqual(self.playable('des', ['tat_inf_jawa', 'tat_inf_tuskenraider', 'tat_inf_tuskenhunter']),
                         ['tat_inf_tuskenraider', 'tat_inf_tuskenhunter', 'tat_inf_jawa'])

    def test_an_army_keeps_its_own_classes_and_heroes(self):
        self.assertEqual(self.playable('rep', ['rep_inf_clone_trooper', 'rep_inf_macewindu']),
                         ['rep_inf_clone_trooper', 'rep_inf_macewindu'])
        # A class another side's archive defines isn't the army's.
        self.assertNotIn('tat_inf_jawa', self.playable('rep', ['rep_inf_clone_trooper']))


class StaleSideTests(unittest.TestCase):
    def test_a_side_imported_without_units_is_imported_again(self):
        import json
        import prepare
        with tempfile.TemporaryDirectory() as root:
            public = Path(root)
            for side, units in (('des', {}), ('rep', {'rep_inf_clone_trooper': {}})):
                (public / 'sides' / side).mkdir(parents=True)
                (public / 'sides' / side / 'units.json').write_text(json.dumps({'units': units}))
            with mock.patch.object(prepare, 'PUBLIC', public):
                self.assertTrue(prepare.stale_side('side:des'), 'the empty Tatooine side of earlier imports')
                self.assertFalse(prepare.stale_side('side:rep'))
                self.assertTrue(prepare.stale_side('side:gun'), 'never written')
                self.assertFalse(prepare.stale_side('map:tat1'))


class CommandPostPresentationTests(unittest.TestCase):
    def test_campfires_and_drops_are_neither_drawn_nor_holograms(self):
        classes = {
            'com_bldg_controlzone': {'base': 'commandpost', 'props': [
                ('MapTexture', 'hud_flag_icon'), ('MapScale', '2.5'), ('HoloOdf', 'com_holo_controlzone')]},
            'tat_bldg_tuskanspawn': {'base': 'commandpost', 'props': [('Label', 'TuskanSpawn'), ('MapScale', '0')]},
            'com_bldg_untakeable_controlzone': {'base': 'commandpost', 'props': [('MapTexture', 'hud_flag_icon'), ('MapScale', '2.5')]},
        }
        records = [
            {'name': 'cp1', 'class': 'com_bldg_controlzone', 'xfrm': IDENTITY,
             'props': [('Team', '3'), ('Label', 'cp1'), ('CaptureRegion', 'cp1capture'), ('SpawnPath', 'CP1SpawnPath')]},
            {'name': 'ts1', 'class': 'tat_bldg_tuskanspawn', 'xfrm': IDENTITY, 'props': [('Team', '3'), ('Label', 'TS1'), ('ValueBleed', '0')]},
            {'name': 'cp9', 'class': 'com_bldg_untakeable_controlzone', 'xfrm': IDENTITY, 'props': [('Team', '1')]},
        ]
        regions = [{'name': 'CP1Capture', 'type': 'cylinder', 'matrix': extract.mat4(IDENTITY), 'size': [5, 2, 5]}]
        posts, _ = extract.command_posts(records, classes, {}, regions, 'tatooine1', {})
        cp1, ts1, cp9 = posts
        self.assertEqual((cp1['team'], cp1['mapTexture'], cp1['mapScale'], cp1['holoOdf']), (3, 'hud_flag_icon', 2.5, 'com_holo_controlzone'))
        self.assertIsNotNone(cp1['capture'])
        self.assertEqual((ts1['team'], ts1['mapTexture'], ts1['mapScale'], ts1['holoOdf'], ts1['capture'], ts1['valueBleed']),
                         (3, None, 0.0, None, None, 0.0))
        self.assertEqual((cp9['mapTexture'], cp9['holoOdf']), ('hud_flag_icon', None))
        # The hologram's anchor stays where it was.
        self.assertEqual(cp1['hologram'], [10, 2, -30])


if __name__ == '__main__':
    unittest.main()
