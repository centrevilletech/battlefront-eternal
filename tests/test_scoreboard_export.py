"""Scoreboard export: the list textures, the strings of the table, DETAILS and
AWARDS, and the rasterHud entries that get the stat icons and list art 4x
atlases. Synthetic inputs; the last class audits an existing extraction
against docs/scoreboard-assets.md when there is one."""
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_hash import fnv1a
from zero_scoreboard import (
    AWARDS_STRINGS, DETAILS_STRINGS, SCOREBOARD_RASTER, SCOREBOARD_STRINGS, SCOREBOARD_TEXTURES, STAT_ICONS,
    TABLE_STRINGS, scoreboard_strings,
)
from zero_spawn_screen import RASTER_HUD

PUBLIC = Path(__file__).resolve().parents[1] / 'public'


def table(**texts):
    """A localization table: key -> text, keyed by hash as Locl is."""
    return {fnv1a(key.replace('__', '.')): text for key, text in texts.items()}


class ListTests(unittest.TestCase):
    def test_textures(self):
        self.assertEqual(SCOREBOARD_TEXTURES, ('opaque_black', 'listbox_cursor', 'listbox_hilight', 'small_arrow',
                                               'backbutton_left', 'backbutton_mid', 'backbutton_right'))
        self.assertEqual(STAT_ICONS, ('stats_kills', 'stats_deaths', 'stats_cpp'))
        self.assertEqual(SCOREBOARD_RASTER, SCOREBOARD_TEXTURES + STAT_ICONS)
        self.assertFalse(set(SCOREBOARD_RASTER) & set(RASTER_HUD), 'nothing twice in rasterHud')

    def test_strings(self):
        self.assertEqual(len(TABLE_STRINGS), 8)
        self.assertEqual(DETAILS_STRINGS[0], 'ifs.stats.personalstatstitle')
        self.assertEqual(len(DETAILS_STRINGS), 10, 'the title and nine rows')
        self.assertEqual(AWARDS_STRINGS[0], 'ifs.stats.awardstatstitle')
        self.assertEqual(len(AWARDS_STRINGS), 9, 'the title and eight awards')
        self.assertIn('ifs.stats.meat', AWARDS_STRINGS, "BANTHA FODDER's key")
        self.assertEqual(SCOREBOARD_STRINGS, TABLE_STRINGS + DETAILS_STRINGS + AWARDS_STRINGS)

    def test_strings_trimmed_and_only_present_ones(self):
        got = scoreboard_strings(table(ifs__stats__done='DONE\r\n', ifs__stats__meat='BANTHA FODDER'))
        self.assertEqual(got, {'ifs.stats.done': 'DONE', 'ifs.stats.meat': 'BANTHA FODDER'})


def extracted():
    path = PUBLIC / 'common' / 'common.json'
    return path.exists() and 'listbox_cursor' in json.loads(path.read_text()).get('textures', {})


@unittest.skipUnless(extracted(), 'requires an extraction with the scoreboard data (tools/zero_scoreboard.py)')
class ExtractionAudit(unittest.TestCase):
    """The ignored public/ data against docs/scoreboard-assets.md."""

    def test_common(self):
        common = json.loads((PUBLIC / 'common' / 'common.json').read_text())
        for name in SCOREBOARD_TEXTURES + STAT_ICONS:
            self.assertEqual(common['textures'].get(name), f'textures/{name}.png')
            self.assertTrue((PUBLIC / 'common' / common['textures'][name]).exists(), name)
        raster = common['rasterHud']
        self.assertEqual(raster[:len(RASTER_HUD)], list(RASTER_HUD), "the spawn screen's first")
        # Then the scoreboard's; the front end's follow (tools/zero_shell.py).
        self.assertEqual(raster[len(RASTER_HUD):len(RASTER_HUD) + len(SCOREBOARD_RASTER)], list(SCOREBOARD_RASTER))
        for key in SCOREBOARD_STRINGS:
            self.assertIsInstance(common['strings'].get(key), str, key)
            self.assertEqual(common['strings'][key], common['strings'][key].rstrip('\r\n'))
        self.assertEqual(common['strings']['ifs.stats.meat'], 'BANTHA FODDER')
        # Every texture the table, DETAILS, AWARDS and the popup draw, and
        # every page of the fonts they use, has its 4x atlas.
        manifest = json.loads((PUBLIC / 'common' / 'raster-hud' / 'manifest.json').read_text())
        self.assertEqual(manifest['scale'], 4)
        drawn = list(SCOREBOARD_RASTER) + ['border_3_pieces', 'cursor', 'mousecursor', 'com_icon_controlzone']
        pages = [page for font in ('gamefont_small', 'gamefont_medium') for page in common['fonts'][font]['pages']]
        for name in drawn + pages:
            entry = manifest['textures'].get(common['textures'][name])
            self.assertIsNotNone(entry, name)
            self.assertTrue((PUBLIC / 'common' / 'raster-hud' / entry['file']).exists(), name)
        # The logos the table, DETAILS and AWARDS spin.
        for logo in ('com_icon_republic', 'com_icon_cis', 'com_icon_imperial', 'com_icon_alliance'):
            self.assertIn(logo, common['holograms'])


if __name__ == '__main__':
    unittest.main()
