"""Optional cross-map intake audit against the user's ignored retail archives."""
import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from extract import GAME, pick_mission, instance_records, shared_archives
from zero_vehicle import vehicle_spawns, property_name, sections
from zero_odf import entity_classes
from zero_weapons import classes as weapon_classes
from ucfb import nodes, node_name


def index(data):
    return {
        'classes': {k.lower(): v for k, v in entity_classes(data).items()},
        'models': {node_name(data, b, e).lower() for b, e in nodes(data, b'modl')},
        'weapons': {k.lower(): v for k, v in weapon_classes(data, b'wpnc').items()},
        'ordnance': {k.lower(): v for k, v in weapon_classes(data, b'ordc').items()},
    }


@unittest.skipUnless((GAME / 'mission.lvl').exists(), 'requires local retail archives')
class RetailVehicleIntake(unittest.TestCase):
    def test_every_standard_map_and_era_resolves_native_vehicle_assets(self):
        sides = {p.stem: index(p.read_bytes()) for p in sorted((GAME / 'SIDE').glob('*.lvl'))}
        shared = [index(data) for data in shared_archives()]
        checked = 0
        for path in sorted(GAME.glob('*/*.lvl')):
            if path.stem[:3].upper() != path.parent.name or '_' in path.stem or path.stem.endswith('h'):
                continue
            data = path.read_bytes()
            local = index(data)
            records = [r for b, e in nodes(data, b'wrld') for r in instance_records(data, b, e, property_name)]
            for side in ('rep', 'imp'):
                name, mission = pick_mission(path.stem, side)
                if not mission:
                    continue
                with self.subTest(mission=name):
                    factions = {t: i['classes'][0][0].split('_')[0] for t, i in mission['teams'].items() if i.get('classes')}
                    pads = vehicle_spawns(records, factions, mission.get('attacker') or 1)
                    wanted = {n for pad in pads for n in pad['classes'].values()}
                    sources = [sides[n] for n in sorted({n.split('_')[0] for n in wanted}) if n in sides] + [local, *shared]
                    def find(kind, key):
                        return next((source[kind][key] for source in sources if key in source[kind]), None)
                    for vehicle in sorted(wanted):
                        cls = find('classes', vehicle)
                        self.assertIsNotNone(cls, vehicle)
                        self.assertIn(cls['base'], ('hover', 'commandhover', 'walker', 'commandwalker', 'flyer'), vehicle)
                        seats = sections(cls['props'])
                        model = seats[0]['settings'].get('GeometryName', vehicle).lower()
                        self.assertTrue(any(model in source['models'] for source in sources), model)
                        for seat in seats:
                            for weapon in seat['weapons']:
                                definition = find('weapons', weapon['class'].lower())
                                self.assertIsNotNone(definition, weapon['class'])
                                ordnance = dict(definition['props']).get('OrdnanceName', '').lower()
                                self.assertIsNotNone(find('ordnance', ordnance), ordnance)
                    if path.stem in ('bes2', 'kam1', 'rhn2'):
                        self.assertEqual(pads, [], 'infantry maps must remain infantry maps')
                    if path.stem == 'nab2':
                        self.assertEqual(len(pads), 4, 'do not synthesize extra Theed scout pads')
                    if path.stem == 'bes1':
                        self.assertIn('bes_fly_cloudcar', wanted)
                    if path.stem == 'tat1':
                        self.assertIn('tat_hover_skiff', wanted)
                    if path.stem == 'end1':
                        self.assertTrue(any(pad['classes'].get('1') == 'imp_hover_speederbike' for pad in pads))
                    checked += 1
        self.assertEqual(checked, 28)


# The stock flyers on the 17 maps and whether their bank's takeoff clip moves
# landing gear the model has (docs/vehicles.md "Landing gear"). The cloud
# car's clip names bones its model lacks; the snowspeeder's bank has no
# takeoff; the rest name no bank.
FLYERS = {'rep_fly_jedifighter': True, 'all_fly_xwing': True, 'all_fly_ywing': True, 'cis_fly_maf': True,
          'bes_fly_cloudcar': False, 'all_fly_snowspeeder': False, 'imp_fly_tiefighter': False, 'imp_fly_tiebomber': False,
          'cis_fly_droidfighter': False, 'cis_fly_geofighter': False, 'rep_fly_gunship': False}


def _lowest(skeleton, meta, floats, frame):
    """The lowest landing gear joint (a bone named for its gear) at a frame
    of the takeoff clip, its DummyRoot track left out as the port leaves it."""
    import array as _array
    import math as _math
    names, parents = skeleton['names'], skeleton['parents']
    clip = meta['clips']['takeoff']
    samples = _array.array('f'); samples.frombytes(floats)
    pose = {}
    for k, joint in enumerate(clip['tracks']):
        if names[joint].lower() != 'dummyroot':
            at = clip['offset'] + (k * clip['frames'] + frame) * 7
            pose[joint] = samples[at:at + 7]
    def local(i):
        if i in pose:
            x, y, z, w, tx, ty, tz = pose[i]
            r = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)], [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                 [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
            return r, [tx, ty, tz]
        m = skeleton['local'][i]
        return [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]], [m[12], m[13], m[14]]
    world = {}
    def place(i):
        if i not in world:
            r, t = local(i)
            p = names.index(parents[i]) if parents[i] in names and parents[i] != names[i] else -1
            if p < 0:
                world[i] = (r, t)
            else:
                pr, pt = place(p)
                world[i] = ([[sum(pr[a][c] * r[c][b] for c in range(3)) for b in range(3)] for a in range(3)],
                            [sum(pr[a][c] * t[c] for c in range(3)) + pt[a] for a in range(3)])
        return world[i]
    return min(place(i)[1][1] for i in range(len(names)) if 'gear' in names[i].lower())


@unittest.skipUnless((GAME / 'mission.lvl').exists(), 'requires local retail archives')
class RetailFlyerGear(unittest.TestCase):
    def test_takeoff_clips_hold_the_gear_down_at_their_first_frame(self):
        from zero_model import collect_models
        from zero_vehicle import bank_clips, flyer_clips, FLYER_ACTIONS
        archives = [(GAME / 'SIDE' / f'{side}.lvl').read_bytes() for side in ('rep', 'cis', 'all', 'imp')]
        archives += [(GAME / 'BES' / 'bes1.lvl').read_bytes(), *shared_archives()]
        classes, models, skeletons = {}, {}, {}
        for data in archives:
            for key, value in entity_classes(data).items():
                classes.setdefault(key.lower(), value)
            collect_models(data, models, skeletons)
        skeletons = {key.lower(): value for key, value in skeletons.items()}
        for name, gear in FLYERS.items():
            with self.subTest(flyer=name):
                settings = sections(classes[name]['props'])[0]['settings']
                skeleton = skeletons.get(settings.get('GeometryName', name).lower())
                bank = settings.get('AnimationName') and skeleton and bank_clips(archives, settings['AnimationName'].lower(), skeleton['names'], FLYER_ACTIONS)
                meta = bank and flyer_clips(bank[0], skeleton['names'])
                self.assertEqual(bool(meta), gear)
                if meta:
                    frames = meta['clips']['takeoff']['frames']
                    down, up = _lowest(skeleton, meta, bank[1], 0), _lowest(skeleton, meta, bank[1], frames - 1)
                    self.assertLess(down, up - 0.05, f'gear down at the first frame ({down:.2f} m) and up at the last ({up:.2f} m)')


if __name__ == '__main__':
    unittest.main()
