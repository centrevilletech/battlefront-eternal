"""Front end export: the movie archive's index, the movie config's fades and
what they inherit, the movies' names, the screens' movies, the Instant Action
catalog, the loading screen's config, the music's order and names, and the
asset lists. Synthetic inputs; the last class audits an existing extraction
against docs/shell-assets.md when there is one."""
import json
from helpers.content import content_path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib
from array import array
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_hash import fnv1a
from zero_scoreboard import SCOREBOARD_RASTER
from zero_shell import (
    EXE_STRINGS, LOAD_PATTERNS, LOAD_TEXTURES, MENU_TEXTURES, SCREEN_MOVIES,
    SHELL_COMMON_TEXTURES, SHELL_PICTURES, SHELL_RASTER, SHELL_TEXTURES, WHOOSHES, bink_info, loading_screen,
    map_catalog, movie_catalog, movie_config, movie_index, movie_names, music_tracks, screen_movies,
    write_movie_index,
    write_picture,
)
import zero_shell
from zero_sound import hexname
from zero_spawn_screen import RASTER_HUD

PUBLIC = content_path()
SHELL = PUBLIC / 'common' / 'shell'
# The game's own maps, in its list's order; add-on maps (GameData/AddOn,
# tools/zero_addon.py) follow them.
STOCK_MAPS = ['bes1', 'bes2', 'end1', 'geo1', 'hot1', 'kam1', 'kas1', 'kas2', 'nab1', 'nab2', 'rhn1', 'rhn2', 'tat1', 'tat2',
              'yav1', 'yav2']


def table(**texts):
    """A localization table: key -> text, keyed by hash as Locl is."""
    return {fnv1a(key.replace('__', '.')): text for key, text in texts.items()}


def chunk(tag, body):
    """A ucfb chunk, padded to 4 bytes."""
    tag = tag if isinstance(tag, bytes) else struct.pack('<I', tag)
    return tag + struct.pack('<I', len(body)) + body + b'\0' * (-len(body) % 4)


def bink(frames=30, width=800, height=600, rate=2997, scale=100, tracks=0, size=200):
    """A Bink file's header, padded to `size` bytes."""
    head = b'BIKi' + struct.pack('<10I', size - 8, frames, 1000, 0, width, height, rate, scale, 0, tracks)
    return head + b'\x55' * (size - len(head))


def mvs(movies):
    """A movie archive: (name hash, bytes, padding) per movie."""
    words = [fnv1a('Name'), fnv1a('shell'), fnv1a('NumSegments'), len(movies), fnv1a('SegmentInfo')]
    for name, data, padding in movies:
        words += [fnv1a('Segment'), fnv1a('Name'), name, fnv1a('Length'), len(data), fnv1a('Padding'), padding,
                  fnv1a('SegmentEnd')]
    info = chunk(fnv1a('Info'), struct.pack(f'<{len(words)}I', *words))
    body = b''.join(data + b'\0' * padding for _, data, padding in movies)
    inner = info + chunk(fnv1a('Data'), body)
    return b'ucfb' + struct.pack('<I', len(inner)) + inner


def data(key, *values):
    """A config DATA chunk: a hash, then each value as a hash (int) or a float."""
    packed = b''.join(struct.pack('<I', v) if isinstance(v, int) else struct.pack('<f', v) for v in values)
    return chunk(b'DATA', struct.pack('<IB', fnv1a(key), len(values)) + packed + struct.pack('<I', 0))


def movie_properties(*entries):
    return data('MovieProperties') + chunk(b'SCOP', b''.join(entries))


def segment_list(*segments):
    return data('SegmentList') + chunk(b'SCOP', b''.join(data('Segment', h, 1.0, float(loops)) for h, loops in segments))


def archive(*nodes):
    inner = b''.join(nodes)
    return b'ucfb' + struct.pack('<I', len(inner)) + inner


class MovieArchiveTests(unittest.TestCase):
    def test_movies_follow_each_other_with_their_padding(self):
        first, second = bink(size=200), bink(frames=90, width=720, height=480, tracks=1, size=97)
        data = mvs([(fnv1a('main'), first, 48), (0x1234, second, 0)])
        index = movie_index(data)
        self.assertEqual([m['hash'] for m in index], [fnv1a('main'), 0x1234])
        self.assertEqual([m['length'] for m in index], [200, 97])
        self.assertEqual(index[1]['offset'] - index[0]['offset'], 200 + 48, "a movie's padding follows it")
        self.assertEqual(data[index[1]['offset']:index[1]['offset'] + 97], second)
        info = bink_info(data, index[1]['offset'])
        self.assertEqual((info['width'], info['height'], info['frames'], info['audio']), (720, 480, 90, 1))
        self.assertEqual(info['duration'], round(90 * 100 / 2997, 3))

    def test_the_movie_index_lists_the_movies_and_removes_old_webm_copies(self):
        with tempfile.TemporaryDirectory() as tmp:
            game = Path(tmp) / "Game" / "GameData" / "Data" / "_LVL_PC"
            (game / "Movies").mkdir(parents=True)
            data = mvs([(fnv1a('main'), bink(size=200), 48), (fnv1a('atat_in'), bink(frames=42, tracks=1, size=97), 0)])
            (game / "Movies" / "shell.mvs").write_bytes(data)
            index = movie_index(data)
            catalog = {"main": {**index[0], "file": "main.bik"}, "atat_in": {**index[1], "file": "atat_in.bik"}}
            public = Path(tmp) / "public"
            out = public / "common" / "shell" / "movies"
            out.mkdir(parents=True)
            # What an earlier version of the preparation left: a WebM copy and its manifest entry.
            encoded = {"file": "main.webm", "hash": "0x%08x" % fnv1a('main'), "settings": "x", "bytes": 4, "source": {"offset": 2048}}
            (out / "manifest.json").write_text(json.dumps({"version": 1, "movies": {"main": encoded}}))
            (out / "main.webm").write_bytes(b"webm")
            manifest = write_movie_index(catalog, game=game, public=public)
            self.assertEqual(manifest, json.loads((out / "manifest.json").read_text()))
            self.assertEqual(manifest["version"], 2)
            main, atat = manifest["movies"]["main"], manifest["movies"]["atat_in"]
            self.assertEqual(main["hash"], "0x%08x" % fnv1a('main'))
            self.assertEqual(atat["hash"], "0x%08x" % fnv1a('atat_in'))
            self.assertEqual(atat["source"], {"archive": "GameData/Data/_LVL_PC/Movies/shell.mvs", "offset": index[1]["offset"], "length": 97})
            self.assertEqual((atat["frames"], atat["audio"]), (42, True))
            for entry in (main, atat):
                self.assertFalse({"file", "settings", "bytes", "kind"} & set(entry))
            self.assertEqual(sorted(p.name for p in out.iterdir()), ["manifest.json"], "nothing decoded or encoded; the old copy gone")

    def test_a_bad_offset_is_an_error(self):
        data = bytearray(mvs([(1, bink(size=64), 0)]))
        data[data.index(b'BIKi')] = ord('X')
        with self.assertRaises(ValueError):
            movie_index(bytes(data))


class MovieConfigTests(unittest.TestCase):
    CONFIG = archive(chunk(b'mcfg', chunk(b'NAME', b'x\0')
        + movie_properties(data('Name', 0xB2573B25), data('FadeInTime', 0.0), data('FadeOutTime', 0.2))
        + movie_properties(data('Name', 0x1271E06C), data('FadeInTime', 0.1), data('FadeOutTime', 0.5))
        + movie_properties(data('Name', fnv1a('ifs_main_intro')), data('Inherit', 0xB2573B25), data('Movie', fnv1a('shell')),
                           data('FadeOutTime', 0.0), segment_list((fnv1a('main_trans'), 0)))
        + movie_properties(data('Name', fnv1a('ifs_main')), data('FadeOutTime', 9.0), data('Inherit', 0x1271E06C),
                           data('FadeInTime', 0.0), segment_list((fnv1a('main'), -1)))
        + movie_properties(data('Name', fnv1a('Rebel_win')), data('Inherit', 0xB2573B25), segment_list((0xF3D18818, 0)))))

    def test_fades_inherit_and_override(self):
        config = movie_config(self.CONFIG)
        self.assertEqual(config[fnv1a('ifs_main_intro')], {'fadeIn': 0.0, 'fadeOut': 0.0, 'segments': [[fnv1a('main_trans'), 0]]})
        # A fade set before Inherit gives way to the parent's; after, it overrides.
        self.assertEqual(config[fnv1a('ifs_main')], {'fadeIn': 0.0, 'fadeOut': 0.5, 'segments': [[fnv1a('main'), -1]]})
        self.assertEqual(config[fnv1a('Rebel_win')]['fadeOut'], 0.2)

    def test_movies_are_named_for_their_segment_or_what_plays_them(self):
        config = movie_config(self.CONFIG)
        index = [{'hash': fnv1a('main_trans'), 'offset': 2048, 'length': 1}, {'hash': fnv1a('main'), 'offset': 4096, 'length': 1},
                 {'hash': 0xF3D18818, 'offset': 8192, 'length': 1}, {'hash': 0xABCDEF01, 'offset': 9000, 'length': 1}]
        catalog = movie_catalog(index, config, movie_names(['main', 'Rebel_win', 'ifs_main']))
        self.assertEqual(list(catalog), ['main_trans', 'main', 'rebel_win', '0xabcdef01'])
        self.assertEqual(catalog['main']['file'], 'main.bik')
        self.assertIn('main_trans', movie_names([]).values(), 'segment names found by hashing candidates')

    def test_screens_get_their_intro_and_loop(self):
        config = movie_config(self.CONFIG)
        catalog = {'main_trans': {'hash': fnv1a('main_trans'), 'file': 'main_trans.bik'},
                   'main': {'hash': fnv1a('main'), 'file': 'main.bik'}}
        movies = screen_movies(catalog, config)
        self.assertEqual(movies, {'ifs_main': {'intro': 'movies/main_trans.bik', 'loop': 'movies/main.bik', 'fadeIn': 0.0,
                                               'fadeOut': 0.5, 'introFadeIn': 0.0, 'introFadeOut': 0.0}},
                         'screens whose movies are missing are left out')

    def test_screen_table(self):
        self.assertEqual(SCREEN_MOVIES['ifs_main'], ('ifs_main_intro', 'ifs_main'))
        self.assertEqual(SCREEN_MOVIES['ifs_opt_contmain'], ('ifs_opt_top_intro', 'ifs_opt_top'))
        self.assertEqual(SCREEN_MOVIES['ifs_start'], (None, 'ifs_start'))


class CatalogTests(unittest.TestCase):
    LOCL = table(planets__naboo__mapname2='NABOO: THEED', planets__geonosis__mapname1='GEONOSIS: SPIRE')
    TABLE = [{'mapluafile': 'NAB2', 'showstr': 'planets.naboo.mapname2', 'side_c': 1.0, 'side_a': 1.0},
             {'mapluafile': 'GEO1', 'showstr': 'planets.geonosis.mapname1', 'side_r': 1.0}]

    def test_eras_teams_and_what_is_extracted(self):
        maps = map_catalog(self.TABLE, {}, self.LOCL, {'nab2': 'nab2c'}, {'nab2fly': {'file': 'nab2fly.bik'}},
                           {'nab2': {'planet': 'NABOO'}})
        self.assertEqual([m['id'] for m in maps], ['nab2', 'geo1'], "the list's order")
        nab2, geo1 = maps
        self.assertEqual(nab2['name'], 'NABOO: THEED')
        self.assertTrue(nab2['extracted'])
        # Without a mission script, team 1 is the side the letter names.
        self.assertEqual(nab2['eras'], {
            'cw': {'teams': ['cis', 'rep'], 'attacker': 1, 'mission': 'nab2c', 'extracted': True},
            'gcw': {'teams': ['all', 'imp'], 'attacker': 1, 'mission': 'nab2a', 'extracted': False},
        })
        self.assertEqual(nab2['fly'], 'movies/nab2fly.bik')
        self.assertEqual(nab2['load'], {'planet': 'NABOO'})
        self.assertFalse(geo1['extracted'])
        self.assertEqual(geo1['eras'], {'cw': {'teams': ['rep', 'cis'], 'attacker': 1, 'mission': 'geo1r', 'extracted': False}})
        self.assertIsNone(geo1['fly'])


class LoadingScreenTests(unittest.TestCase):
    ENTRIES = [
        ['LoadDisplay', [], [
            ['TargetTime', [3.5, 1.0], []], ['TeamModel', ['all', ''], []], ['TeamModel', ['imp', ''], []],
            ['Backdrop', ['load_simple_backdrop'], []], ['RotatingDisk', ['load_rotating_disk'], []],
            ['NoiseTextures', ['load_noise_', 6.0, 30.0, 6.0, 6.0], []], ['ScanlineTexture', ['load_scanline', 1.0, 12.0, 0.5], []],
            ['EdgeTextures', ['load_edge_horz', 'load_edge_vert', 'load_edge_corner'], []],
            ['AnimatedTextures', ['load_side_animation_', 6.0, 6.0], []], ['ZoomSound', ['load_zoom'], []],
            ['BarSoundInterval', [1.0], []],
        ]],
        ['World', ['geo1'], [
            ['PlanetInfo', ['planets.geonosis.name', 'load_planet_geonosis'], []],
            ['PlanetLevel', [0.0, 'load_galacticmap_0', 390.0, 737.0, 37.0, 43.0], []],
            ['PlanetLevel', [3.0, 'load_level_geonosis_3', 0.0, 0.0, 0.0, 0.0], []],
        ]],
    ]

    def test_screen_and_maps(self):
        screen, maps = loading_screen(self.ENTRIES, table(planets__geonosis__name='GEONOSIS'))
        self.assertEqual(screen['backdrop'], 'load/load_simple_backdrop.png')
        self.assertEqual(screen['disk'], '/common/textures/load_rotating_disk.png', "a common texture's URL")
        self.assertEqual(screen['edges'], {'horizontal': '/common/textures/load_edge_horz.png',
                                           'vertical': '/common/textures/load_edge_vert.png',
                                           'corner': '/common/textures/load_edge_corner.png'})
        self.assertEqual(screen['noise'], {'textures': [f'/common/textures/load_noise_{n}.png' for n in range(6)], 'args': [30, 6, 6]})
        self.assertEqual(screen['sideAnimation']['textures'][5], '/common/textures/load_side_animation_5.png')
        self.assertEqual(screen['scanline'], {'texture': '/common/textures/load_scanline.png', 'args': [1, 12, 0.5]})
        self.assertEqual(screen['teams'], ['all', 'imp'])
        self.assertEqual(screen['timing'], [3.5, 1])
        self.assertEqual(screen['sounds']['zoom'], 'load_zoom')
        self.assertIsNone(screen['sounds']['bar'])
        self.assertEqual(screen['barSoundInterval'], 1)
        self.assertEqual(screen['text'], 'common.loading')
        self.assertEqual(maps, {'geo1': {
            'planet': 'GEONOSIS', 'planetKey': 'planets.geonosis.name', 'planetTexture': 'load/load_planet_geonosis.png',
            'stages': [{'image': 'load/load_galacticmap_0.png', 'target': [390, 737, 37, 43]},
                       {'image': 'load/load_level_geonosis_3.png', 'target': None}],
        }})


class MusicTests(unittest.TestCase):
    def test_soundtrack_order_then_the_rest_named_for_their_property(self):
        stream = hexname(fnv1a('shell_music'))
        objects = [
            {'type': 'SoundStreamProperties', 'Name': hexname(fnv1a('shell_soundtrack')), 'Stream': stream,
             'SegmentList': [['0x00000002', 0.5], ['0x00000001', 0.5]]},
            {'type': 'SoundStreamProperties', 'Name': hexname(fnv1a('shell_clash')), 'Stream': stream,
             'SegmentList': [['0x00000001', 1.0]]},
            {'type': 'SoundStreamProperties', 'Name': hexname(fnv1a('shell_droidinvasion')), 'Stream': stream,
             'SegmentList': [['0x00000003', 1.0]]},
            {'type': 'SoundStreamProperties', 'Name': hexname(fnv1a('shell_vo_line')), 'Stream': hexname(1),
             'SegmentList': [['0x00000003', 1.0]]},
        ]
        items = [{'id': n, 'rate': 44100, 'frames': 44100 * n, 'size': 0, 'offset': 0} for n in (1, 2, 3)]
        tracks = music_tracks(objects, {'items': items})
        self.assertEqual([(t['name'], t['segment'], t['properties'], t['duration']) for t in tracks], [
            ('shell_soundtrack_1', '0x00000002', ['shell_soundtrack'], 2.0),
            ('shell_clash', '0x00000001', ['shell_soundtrack', 'shell_clash'], 1.0),
            ('shell_droidinvasion', '0x00000003', ['shell_droidinvasion'], 3.0),
        ])
        self.assertEqual(WHOOSHES[0], 'shell_transition1')
        self.assertEqual(len(WHOOSHES), 10)


class ListTests(unittest.TestCase):
    def test_lists(self):
        for name in ('buttonleft', 'buttonmid', 'buttonright', 'border_3a_pieces', 'slider_bg', 'slider_fg', 'white'):
            self.assertIn(name, MENU_TEXTURES, 'the contract names it')
        self.assertIn('his_brief_bg', SHELL_TEXTURES)
        self.assertIn('iface_bgmeta_space', SHELL_PICTURES)
        self.assertEqual(SHELL_RASTER, MENU_TEXTURES + SHELL_TEXTURES + LOAD_TEXTURES)
        self.assertEqual(len(set(SHELL_COMMON_TEXTURES)), len(SHELL_COMMON_TEXTURES))
        self.assertFalse(set(SHELL_RASTER) & (set(SHELL_PICTURES) | set(LOAD_PATTERNS)), 'pictures and patterns stay native')
        self.assertFalse(set(SHELL_RASTER) & (set(RASTER_HUD) | set(SCOREBOARD_RASTER)), 'nothing twice in rasterHud')
        self.assertEqual(EXE_STRINGS, ('common.loading',))


class PictureTests(unittest.TestCase):
    def read(self, path):
        data = path.read_bytes()
        width, height, depth, kind = struct.unpack('>IIBB', data[16:26])
        at = data.index(b'IDAT')
        size = struct.unpack('>I', data[at - 4:at])[0]
        return width, height, kind, zlib.decompress(data[at + 4:at + 4 + size])

    def test_opaque_pictures_drop_alpha(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / 'p.png'
            write_picture(path, 2, 1, bytes([1, 2, 3, 255, 4, 5, 6, 255]))
            self.assertEqual(self.read(path), (2, 1, 2, bytes([0, 1, 2, 3, 4, 5, 6])))
            write_picture(path, 1, 2, bytes([1, 2, 3, 0, 4, 5, 6, 255]))
            self.assertEqual(self.read(path), (1, 2, 6, bytes([0, 1, 2, 3, 0, 0, 4, 5, 6, 255])))


def extracted():
    return (SHELL / 'shell.json').exists()


def probe(path):
    """ffprobe's first stream and the format, as dicts."""
    run = subprocess.run(['ffprobe', '-v', 'error', '-show_entries', 'stream=codec_name,width,height,pix_fmt,channels,sample_rate',
                          '-show_entries', 'format=duration', '-of', 'json', str(path)], capture_output=True, text=True, check=True)
    found = json.loads(run.stdout)
    return found.get('streams', []), found.get('format', {})


def has_soxr():
    try:
        import numpy  # noqa: F401
        import soxr  # noqa: F401
    except ImportError:
        return False
    return True


class ResampleTests(unittest.TestCase):
    """The whooshes' resampler: ffmpeg's soxr, and the soxr module only where
    ffmpeg says it was built without that resampler."""

    PCM = array('h', [0, 1000, 0, -1000] * 64)

    def setUp(self):
        zero_shell.FFMPEG_SOXR = True
        self.addCleanup(setattr, zero_shell, 'FFMPEG_SOXR', True)

    def ffmpeg(self, code, stdout=b'', stderr=b''):
        """ffmpeg ending with this exit code and output."""
        done = subprocess.CompletedProcess(['ffmpeg'], code, stdout, stderr)
        return mock.patch.object(zero_shell.subprocess, 'run', return_value=done)

    def test_ffmpegs_soxr_output_is_taken_as_it_is(self):
        with self.ffmpeg(0, array('h', [1, -2, 3]).tobytes()) as run:
            self.assertEqual(list(zero_shell.resample(self.PCM, 22050, 1.0, 44100)), [1, -2, 3])
        command = run.call_args.args[0]
        self.assertIn('aresample=44100:resampler=soxr:precision=28', command)
        self.assertEqual(command[command.index('-ar') + 1], '22050')

    def test_an_ffmpeg_failure_that_is_not_a_missing_soxr_is_an_error(self):
        with self.ffmpeg(1, stderr=b'pipe:0: Invalid data found when processing input\n'):
            with self.assertRaises(subprocess.CalledProcessError):
                zero_shell.resample(self.PCM, 22050, 1.0, 44100)
        self.assertTrue(zero_shell.FFMPEG_SOXR)

    @unittest.skipUnless(has_soxr(), 'requires the numpy and soxr modules')
    def test_an_ffmpeg_without_soxr_hands_over_to_the_soxr_module(self):
        refusal = b'[SWR @ 0x600000] Requested resampling engine is unavailable\n'
        with self.ffmpeg(234, stderr=refusal) as run:
            first = zero_shell.resample(self.PCM, 22050, 1.0, 44100)
            second = zero_shell.resample(self.PCM, 22050, 1.0, 44100)
        # ffmpeg is asked once; twice the rate is twice the samples.
        self.assertEqual(run.call_count, 1)
        self.assertEqual(len(first), 2 * len(self.PCM))
        self.assertEqual(len(first), len(second))
        # libsoxr dithers integer output. Independent conversions can differ
        # by up to two 16-bit sample units with the same input and settings.
        self.assertLessEqual(max(abs(a - b) for a, b in zip(first, second)), 2)
        self.assertGreater(max(first), 500)


@unittest.skipUnless(extracted(), 'requires an extraction with the front end data (tools/zero_shell.py)')
class ExtractionAudit(unittest.TestCase):
    """The ignored public/ data against docs/shell-assets.md."""

    @classmethod
    def setUpClass(cls):
        cls.common = json.loads((PUBLIC / 'common' / 'common.json').read_text())
        cls.shell = json.loads((SHELL / 'shell.json').read_text())
        manifest = SHELL / 'movies' / 'manifest.json'
        cls.movies = json.loads(manifest.read_text())['movies'] if manifest.exists() else {}

    def test_common(self):
        common = self.common
        for name in SHELL_COMMON_TEXTURES:
            self.assertEqual(common['textures'].get(name), f'textures/{name}.png', name)
            self.assertTrue((PUBLIC / 'common' / common['textures'][name]).exists(), name)
        # The spawn screen's entries, then the scoreboard's, then the front end's.
        raster = common['rasterHud']
        before = len(RASTER_HUD) + len(SCOREBOARD_RASTER)
        self.assertEqual(raster[:before], list(RASTER_HUD + SCOREBOARD_RASTER))
        self.assertEqual(raster[before:], list(SHELL_RASTER))
        manifest = json.loads((PUBLIC / 'common' / 'raster-hud' / 'manifest.json').read_text())
        self.assertEqual(manifest['scale'], 4)
        pages = [page for font in ('gamefont_tiny', 'gamefont_small', 'gamefont_medium', 'gamefont_large')
                 for page in common['fonts'][font]['pages']]
        for name in raster + pages:
            entry = manifest['textures'].get(common['textures'][name])
            self.assertIsNotNone(entry, name)
            self.assertTrue((PUBLIC / 'common' / 'raster-hud' / entry['file']).exists(), name)
        for name in SHELL_PICTURES + LOAD_PATTERNS:
            self.assertNotIn(common['textures'][name], manifest['textures'], f'{name} stays native')

    def test_strings(self):
        strings = self.common['strings']
        expected = {
            'ifs.main.title': 'MAIN MENU', 'ifs.main.sp': 'SINGLEPLAYER', 'common.quit2windows': 'EXIT TO WINDOWS',
            'game.pause.title': 'GAME PAUSED', 'common.resume': 'RESUME GAME', 'common.restart': 'RESTART MISSION',
            'ifs.missionselect.available': 'AVAILABLE MAPS:', 'ifs.missionselect.inorder': 'PLAY IN ORDER',
            'ifs.onlinelobby.launch': 'LAUNCH', 'common.era.gcw': 'GALACTIC CIVIL WAR', 'common.loading': 'LOADING',
            'planets.naboo.mapname2': 'NABOO: THEED', 'ifs.Controls.Soldier.title': 'INFANTRY CONTROLS',
        }
        for key, text in expected.items():
            self.assertEqual(strings.get(key), text, key)
        self.assertGreaterEqual(len(strings), 638)
        for key, text in strings.items():
            self.assertEqual(text, text.rstrip('\r\n'), key)

    def test_shell_json(self):
        shell = self.shell
        self.assertEqual(list(shell), ['movies', 'maps', 'music', 'whooshes', 'load'])
        for screen, entry in shell['movies'].items():
            with self.subTest(screen=screen):
                self.assertTrue(entry['loop'].startswith('movies/') and entry['loop'].endswith('.bik'))
                self.assertTrue(entry['intro'] is None or entry['intro'].startswith('movies/'))
                self.assertIsInstance(entry['fadeIn'], float)
                self.assertIsInstance(entry['fadeOut'], float)
                self.assertEqual('introFadeIn' in entry, entry['intro'] is not None)
        self.assertEqual(shell['movies']['ifs_main'], {'intro': 'movies/main_trans.bik', 'loop': 'movies/main.bik',
                                                       'fadeIn': 0.0, 'fadeOut': 0.5, 'introFadeIn': 0.0, 'introFadeOut': 0.0})
        self.assertEqual(shell['movies']['ifs_sp']['loop'], 'movies/atat_screen.bik')

    def test_maps(self):
        maps = self.shell['maps']
        self.assertEqual([m['id'] for m in maps][:len(STOCK_MAPS)], STOCK_MAPS)
        sides = {'cw': {'rep', 'cis'}, 'gcw': {'all', 'imp'}}
        letters = {'rep': 'r', 'cis': 'c', 'all': 'a', 'imp': 'i'}
        for entry in maps:
            with self.subTest(map=entry['id']):
                self.assertTrue(entry['name'] and ':' in entry['name'])
                world = PUBLIC / 'maps' / entry['id'] / 'map.json'
                self.assertEqual(entry['extracted'], world.exists())
                # Every battle fought in the map's world (map.json missions),
                # or an older extraction's one.
                manifest = json.loads(world.read_text()) if world.exists() else {}
                battles = manifest.get('missions', {(manifest.get('mission') or {}).get('name'): {}})
                held = {m for m, b in battles.items() if m and not (b.get('mission') or {}).get('world')}
                self.assertTrue(entry['eras'] and set(entry['eras']) <= {'cw', 'gcw'})
                for era, value in entry['eras'].items():
                    self.assertEqual(set(value['teams']), sides[era])
                    self.assertIn(value['attacker'], (1, 2))
                    attacker = value['teams'][value['attacker'] - 1]
                    if entry['id'] in STOCK_MAPS:  # Jabba's Palace's tat3c has the Republic attack
                        self.assertEqual(value['mission'], entry['id'] + letters[attacker], 'the attacking side names the mission')
                    self.assertEqual(value['extracted'], value['mission'] in held)
                self.assertEqual(entry['extracted'], any(v['extracted'] for v in entry['eras'].values()),
                                 'each extracted map holds one of its eras')
                self.assertEqual(entry['fly'], f"movies/{entry['id']}fly.bik")
                if entry['load'] is None:
                    # An add-on's map: load.lvl has no World for it (Jabba's Palace).
                    self.assertNotIn(entry['id'], STOCK_MAPS)
                    continue
                stages = entry['load']['stages']
                self.assertEqual(len(stages), 4)
                self.assertIsNone(stages[-1]['target'])
                self.assertTrue(all(len(stage['target']) == 4 for stage in stages[:-1]))
                for path in [stage['image'] for stage in stages] + [entry['load']['planetTexture']]:
                    self.assertTrue((SHELL / path).exists(), path)
                self.assertEqual(entry['load']['planet'], self.common['strings'][entry['load']['planetKey']])
        geo1 = next(m for m in maps if m['id'] == 'geo1')
        self.assertEqual(geo1['eras'], {'cw': {'teams': ['rep', 'cis'], 'attacker': 1, 'mission': 'geo1r', 'extracted': geo1['extracted']}})

    def test_load(self):
        load = self.shell['load']
        self.assertTrue((SHELL / load['backdrop']).exists())
        urls = [load['disk'], *load['leds'], *load['edges'].values(), *load['titleBar'], *load['planetBackdrops'],
                *load['sideAnimation']['textures'], *load['noise']['textures'], load['scanline']['texture']]
        textures = set(self.common['textures'].values())
        manifest = json.loads((PUBLIC / 'common' / 'raster-hud' / 'manifest.json').read_text())['textures']
        for url in urls:
            self.assertTrue(url.startswith('/common/textures/'), url)
            self.assertIn(url[len('/common/'):], textures, url)
            self.assertTrue((PUBLIC / url.lstrip('/')).exists(), url)
        for url in [load['disk'], *load['leds'], *load['edges'].values()]:
            self.assertIn(url[len('/common/'):], manifest, f'{url} has its 4x atlas')
        self.assertEqual(self.common['strings'][load['text']], 'LOADING')
        self.assertEqual(load['teams'], ['all', 'imp', 'rep', 'cis'])
        self.assertEqual(load['timing'], [3.5, 1])

    def test_audio(self):
        music, whooshes = self.shell['music'], self.shell['whooshes']
        self.assertEqual(len(music), 6)
        self.assertEqual([m['properties'][0] for m in music[:5]], ['shell_soundtrack'] * 5, "the soundtrack's cycle first")
        self.assertNotIn('shell_soundtrack', music[5]['properties'])
        self.assertEqual(len(whooshes), 10)
        self.assertEqual(whooshes[0], '/audio/shell/shell_transition1.flac')
        for path in [m['file'] for m in music] + whooshes:
            self.assertTrue((PUBLIC / path.lstrip('/')).exists(), path)
        sound = json.loads((PUBLIC / 'audio' / 'shell' / 'sound.json').read_text())
        names = {o.get('Name') for o in sound['objects']}
        for name in ('shell_select_change', 'shell_menu_accept', 'shell_menu_exit', 'shell_transition1', 'shellfx', 'shellmusic'):
            self.assertIn(hexname(fnv1a(name)), names, name)
        size = (PUBLIC / 'audio' / 'shell' / 'bank.bin').stat().st_size
        self.assertTrue(all(offset + 2 * frames <= size for offset, frames, _ in sound['samples'].values()))

    @unittest.skipUnless(shutil.which('ffprobe'), 'needs ffprobe')
    def test_audio_files_probe(self):
        for entry in self.shell['music']:
            streams, form = probe(PUBLIC / entry['file'].lstrip('/'))
            self.assertEqual((streams[0]['codec_name'], streams[0]['channels']), ('opus', 2), entry['name'])
            self.assertAlmostEqual(float(form['duration']), entry['duration'], delta=0.05)
        for path in self.shell['whooshes']:
            streams, _ = probe(PUBLIC / path.lstrip('/'))
            self.assertEqual((streams[0]['codec_name'], streams[0]['channels'], streams[0]['sample_rate']), ('flac', 2, '44100'))

    def test_movies(self):
        referenced = {e[k] for e in self.shell['movies'].values() for k in ('intro', 'loop') if e[k]}
        referenced |= {m['fly'] for m in self.shell['maps'] if m['fly']}
        added = [m for m in self.shell['maps'] if m['id'] not in STOCK_MAPS and m['fly']]
        self.assertEqual(len(self.movies), 55 + len(added), "every movie in Movies/shell.mvs, and the add-ons' flyovers")
        self.assertLessEqual({Path(url).stem for url in referenced}, set(self.movies))
        for name, movie in self.movies.items():
            with self.subTest(movie=name):
                self.assertRegex(movie['hash'], r'^0x[0-9a-f]{8}$')
                self.assertTrue(movie['source']['archive'].lower().endswith('.mvs'))
                self.assertGreater(movie['frames'], 0)
        self.assertEqual(list((SHELL / 'movies').glob('*.webm')), [], 'the movies are not converted')


if __name__ == '__main__':
    unittest.main()
