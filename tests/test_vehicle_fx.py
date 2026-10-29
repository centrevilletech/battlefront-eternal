"""tools/zero_vehicle.py vehicle_fx on synthetic class properties (no retail data).

    python3 -m unittest tests/test_vehicle_fx.py
"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_hash import fnv1a  # noqa: E402
from zero_odf import property_name  # noqa: E402
from zero_vehicle import NAMES, dying_span, foot_bottom, vehicle_fx  # noqa: E402


class VehicleFx(unittest.TestCase):
    def test_the_hashes_resolve_to_their_names(self):
        for name in ('ThrustEffect', 'ThrustEffectScaleStart', 'DamageAttachPoint', 'DamageInheritVelocity', 'ExplosionDestruct',
                     'WaterEffect', 'StompEffect', 'ChunkSmokeEffect'):
            self.assertEqual(property_name(fnv1a(name)), name)
            self.assertEqual(NAMES[fnv1a(name)], name)

    def test_thrust_entries_take_the_keys_after_them_and_the_flyer_speeds(self):
        props = [('MinSpeed', '20.0'), ('MaxSpeed', '72.0'), ('CHUNKSECTION', 'CHUNK1'), ('ChunkGeometryName', 'Rep_Fly_Chunk1'),
                 ('ChunkOmega', '2.0 2.0 1.5'), ('ChunkSmokeEffect', 'smokeplume'), ('ChunkSmokeNodeName', 'hp_smoke1')]
        for side in (1, -1):
            props += [('ThrustEffect', 'exhaust'), ('ThrustAttachPoint', 'DummyRoot'), ('ThrustAttachOffset', f'{0.7368 * side} -0.3585 -2.4618'),
                      ('ThrustEffectMinScale', '0.5'), ('ThrustEffectMaxScale', '1.4'), ('ThrustEffectScaleStart', '0.55')]
        props += [('ThrustEffect', 'third'), ('ThrustEffect', 'fourth'), ('ThrustEffect', 'fifth'), ('ThrustEffectMinScale', '9')]
        fx = vehicle_fx(props)
        self.assertEqual([t['effect'] for t in fx['thrust']], ['exhaust', 'exhaust', 'third', 'fourth'])
        self.assertEqual(fx['thrust'][1], {'effect': 'exhaust', 'node': 'dummyroot', 'offset': [-0.7368, -0.3585, -2.4618],
                                           'minScale': 0.5, 'maxScale': 1.4, 'scaleStart': 0.55})
        self.assertEqual(fx['thrust'][3]['minScale'], 1.0, 'a fifth slot does not exist; its keys go nowhere')
        self.assertEqual(fx['speed'], [20.0, 72.0])
        self.assertEqual(vehicle_fx([('ThrustEffect', 'x')])['speed'], [10.0, 30.0])
        self.assertEqual(fx['chunks'], [{'ChunkGeometryName': 'rep_fly_chunk1', 'ChunkOmega': [2.0, 2.0, 1.5],
                                         'ChunkSmokeEffect': 'smokeplume', 'ChunkSmokeNodeName': 'hp_smoke1'}])

    def test_damage_groups_open_at_their_start_and_need_an_effect(self):
        props = [('DamageStartPercent', '60.0'), ('DamageStopPercent', '20.0'), ('DamageEffect', 'vehiclespark'),
                 ('DamageEffectScale', '0.4'), ('DamageInheritVelocity', '0.75'), ('DamageAttachPoint', 'HP_Damage_3'),
                 ('DamageStartPercent', '10.0'), ('DamageEffect', 'vehicleflame'), ('DamageAttachOffset', '0 1 2'),
                 ('DamageStartPercent', '5.0')]
        fx = vehicle_fx(props)
        self.assertEqual(fx['damage'], [
            {'start': 0.6, 'stop': 0.2, 'effect': 'vehiclespark', 'scale': 0.4, 'inherit': 0.75, 'node': 'hp_damage_3', 'offset': [0.0, 0.0, 0.0]},
            {'start': 0.1, 'stop': 0.0, 'effect': 'vehicleflame', 'scale': 1.0, 'inherit': 0.0, 'node': '', 'offset': [0.0, 1.0, 2.0]}])
        many = vehicle_fx([kv for _ in range(12) for kv in (('DamageStartPercent', '50'), ('DamageEffect', 'smoke'))])
        self.assertEqual(len(many['damage']), 10)

    def test_water_wake_stomp_feet_and_death_dust(self):
        shapes = [{'name': 'p_l_foot', 'parent': 'bone_l_foot', 'matrix': [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0.1, -0.2, 0.3, 1]},
                  {'name': 'p_body', 'parent': '', 'matrix': [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 2, 0, 1]}]
        skeleton = {'names': ['DummyRoot', 'bone_l_foot']}
        props = [('WaterEffect', 'waterwake_sm'), ('WakeEffect', 'dustwake'), ('WaterEffect', 'waterwake_lg'),
                 ('TerrainCollision', 'P_L_Foot'), ('TerrainCollision', 'p_missing'), ('StompEffect', 'walkerstomp'),
                 ('StompDetectionType', '1'), ('DeathDustEffect', 'dust'), ('DeathDustOffset', '0 1 0'), ('DeathDustDelay', '2.5')]
        fx = vehicle_fx(props, shapes, skeleton, walker=True)
        self.assertEqual(fx['water'], 'waterwake_lg')
        self.assertEqual(fx['wake'], 'dustwake')
        feet = [{'name': 'p_l_foot', 'bone': 'bone_l_foot', 'at': [0.1, -0.2, 0.3]},
                {'name': 'p_missing', 'bone': '', 'at': [0.0, 0.0, 0.0]}]
        self.assertEqual(fx['feet'], feet, 'in order: FootstepSound<i> belongs to the i-th')
        self.assertEqual(fx['stomp'], {'effect': 'walkerstomp', 'detection': 1.0, 'threshold': 0.15, 'hydraulic': 1.0})
        # A walker with feet and no StompEffect still has the rules its
        # footsteps follow (the tauntaun: StompThreshold 0.5).
        self.assertEqual(vehicle_fx([('TerrainCollision', 'p_l_foot'), ('StompThreshold', '0.5'), ('HydraulicSoundHeight', '.2')],
                                    shapes, skeleton, walker=True)['stomp'], {'detection': 0.0, 'threshold': 0.5, 'hydraulic': 0.2})
        self.assertEqual(fx['deathDust'], {'effect': 'dust', 'offset': [0.0, 1.0, 0.0], 'delay': 2.5})
        # A flyer's or hover's TerrainCollision primitives are spheres, not feet.
        self.assertNotIn('feet', vehicle_fx(props, shapes, skeleton))
        # A hover's wake with no TerrainCollision has nowhere to play.
        self.assertEqual(vehicle_fx([('WakeEffect', 'dustwake')])['wakeNodes'], [])
        self.assertNotIn('stomp', vehicle_fx([('StompEffect', 'walkerstomp')], walker=True))
        self.assertEqual(vehicle_fx([]), {})

    def test_a_foot_meets_the_ground_at_the_bottom_of_its_primitive(self):
        # The AT-AT's p_front_left: a cylinder 2 m across and 3 m tall whose
        # up axis is its foot bone's -X; the AT-ST's p_l_foot, tilted; a
        # sphere's bottom is a radius under its centre.
        atat = {'name': 'p_front_left', 'parent': 'bone_fl_foot', 'kind': 2, 'size': (2.0, 3.0, 0.0),
                'matrix': [0, 0, 1, 0, -1, 0, 0, 0, 0, -1, 0, 0, 0.6084, 0.0029, 0.008, 1]}
        atst = {'name': 'p_l_foot', 'parent': 'bone_l_toe', 'kind': 2, 'size': (1.0, 1.0, 0.0),
                'matrix': [0, 0, 1, 0, -0.3093, -0.951, 0, 0, 0.951, -0.3093, 0, 0, 0.6814, -0.2026, 0.0005, 1]}
        ball = {'name': 'p_l_foot', 'parent': 'bone_l_toe', 'kind': 0, 'size': (0.3, 0.0, 0.0),
                'matrix': [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]}
        bottom, turn = foot_bottom(atat)
        self.assertEqual(bottom, [2.1084, 0.0029, 0.008])
        self.assertAlmostEqual(sum(v * v for v in turn), 1.0, places=3)
        self.assertEqual(foot_bottom(atst)[0], [0.836, 0.2729, 0.0005])
        self.assertEqual(foot_bottom(ball), ([0.0, -0.3, 0.0], [0.0, 0.0, 0.0, 1.0]))
        skeleton = {'names': ['DummyRoot', 'bone_fl_foot']}
        fx = vehicle_fx([('TerrainCollision', 'p_front_left'), ('StompEffect', 'bigwalkerstomp')], [atat], skeleton, walker=True)
        self.assertEqual(fx['feet'][0]['bottom'], [2.1084, 0.0029, 0.008])
        self.assertEqual(fx['feet'][0]['turn'], turn)

    def test_a_walker_dies_over_its_death_animation(self):
        # The walker parser's dying keys (07fe74d0 and 871eb169 before they were named).
        self.assertEqual(fnv1a('DeathAnimationExplosion'), 0x07FE74D0)
        self.assertEqual(fnv1a('DeathAnimationExplosionTime'), 0x871EB169)
        for name in ('DeathAnimationExplosion', 'DeathAnimationExplosionTime', 'FinalExplosion', 'FinalExplosionOffset'):
            self.assertEqual(property_name(fnv1a(name)), name)
        props = [('DeathAnimationExplosion', 'small_explosion'), ('DeathAnimationExplosion', 'med_explosion'),
                 ('DeathAnimationExplosion', 'c'), ('DeathAnimationExplosion', 'd'), ('DeathAnimationExplosion', 'fifth'),
                 ('DeathAnimationExplosionTime', '1.5'), ('FinalExplosion', 'imp_walk_atst_finalexp'),
                 ('FinalExplosionOffset', '5.5808 1.9066 -0.8685')]
        fx = vehicle_fx(props, walker=True)
        self.assertEqual(fx['dying'], {'effects': ['small_explosion', 'med_explosion', 'c', 'd'], 'interval': 1.5,
                                       'finalOffset': [5.5808, 1.9066, -0.8685]}, 'the class keeps four')
        self.assertNotIn('dying', vehicle_fx(props), 'a walker\'s alone')
        self.assertEqual(vehicle_fx([], walker=True)['dying'], {'effects': [], 'interval': 0.4, 'finalOffset': [0.0, 0.0, 0.0]})
        # Its length: the death animation's frames at 30 fps over AnimationSpeed (the AT-AT's: 261 at 1.5).
        clip = {'fps': 30, 'clips': {'death01': {'frames': 261}}}
        self.assertEqual(dying_span(vehicle_fx([], walker=True), clip, {'AnimationSpeed': '1.5'})['dying']['span'], 5.8)
        self.assertEqual(dying_span(vehicle_fx([], walker=True), clip, {})['dying']['span'], 8.7)
        self.assertNotIn('dying', dying_span(vehicle_fx([], walker=True), {'fps': 30, 'clips': {}}, {}), 'none: gone at once')
        self.assertEqual(dying_span({}, clip, {}), {})

    def test_a_turret_takes_a_hulls_damage_effects_and_chunks(self):
        """write_props gives each live object its class's damage effects and
        chunks, and files the pieces' models in the model's space."""
        import json
        import tempfile
        from unittest.mock import patch
        import extract
        piece = {'texture': 'kam_bldg_antenna', 'positions': [[0, 0, 0], [1, 0, 0], [0, 1, 0]], 'local_positions': [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
                 'normals': [[0, 0, 1]] * 3, 'local_normals': [[0, 0, 1]] * 3, 'uvs': [[0, 0]] * 3, 'indices': [0, 1, 2],
                 'additive': False, 'transparent': False, 'material': {'flags': 0}}
        turret = {'base': 'armedbuilding', 'props': [
            ('GeometryName', 'kam_prop_antiaircraft'), ('MaxHealth', '500.0'), ('DamageStartPercent', '50.0'), ('DamageStopPercent', '30.0'),
            ('DamageEffect', 'vehiclespark'), ('DamageStartPercent', '30.0'), ('DamageStopPercent', '0.0'), ('DamageEffect', 'smokeplume'),
            ('DamageAttachPoint', 'hp_smoke_1'), ('CHUNKSECTION', 'CHUNK1'), ('ChunkGeometryName', 'AntiAircraft_Chunk2'),
            ('ChunkSpeed', '8.0'), ('ChunkTrailEffect', 'mediumsmoketrail')]}
        droid = {'base': 'powerupstation', 'props': [('GeometryName', 'com_item_healthrecharge'), ('MaxHealth', '80')]}
        classes = {'bes_bldg_AAA_turret': turret, 'com_item_healthrecharge': droid}
        records = [{'class': name, 'name': name, 'xfrm': [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], 'props': []} for name in classes]
        models = {'kam_prop_antiaircraft': [piece], 'com_item_healthrecharge': [piece], 'antiaircraft_chunk2': [piece]}
        with tempfile.TemporaryDirectory() as tmp, patch('extract.collision_sources', return_value=({}, {})):
            out = Path(tmp)
            extract.write_props(records, classes, set(classes), models, {}, [b''], out)
            props = json.loads((out / 'props.json').read_text())
        aaa, gonk = props['instances']
        self.assertEqual([d['effect'] for d in aaa['fx']['damage']], ['vehiclespark', 'smokeplume'])
        self.assertEqual(aaa['fx']['damage'][1]['node'], 'hp_smoke_1')
        self.assertEqual(aaa['fx']['chunks'], [{'ChunkGeometryName': 'antiaircraft_chunk2', 'ChunkSpeed': 8.0, 'ChunkTrailEffect': 'mediumsmoketrail'}])
        self.assertNotIn('fx', gonk, 'a droid has neither')
        self.assertEqual(list(props['extras']), ['antiaircraft_chunk2'], 'the piece, under its lower-case name')
        self.assertEqual(props['effects'], {}, 'no archive here holds the effects')

    def test_the_blast_goes_off_at_its_offset(self):
        self.assertEqual(property_name(fnv1a('ExplosionOffset')), 'ExplosionOffset')
        fx = vehicle_fx([('ExplosionName', 'imp_walk_atst_exp'), ('ExplosionOffset', '0.0 5.9058 0.1833')], walker=True)
        self.assertEqual(fx['explosionOffset'], [0.0, 5.9058, 0.1833])
        self.assertNotIn('explosionOffset', vehicle_fx([('ExplosionOffset', 'bad')]))


if __name__ == '__main__':
    unittest.main()
