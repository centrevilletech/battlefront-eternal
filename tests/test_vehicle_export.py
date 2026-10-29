import sys
import unittest
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from zero_vehicle import sections, vehicle_collision, vehicle_spawns, vehicle_parts, hover_springs, flyer_clips
from zero_odf import property_name
from xform import IDENTITY

class VehicleExport(unittest.TestCase):
    def test_hover_springs_follow_the_parser(self):
        # A fighter tank's: the pitch factor by where the sphere sits (-1 in
        # front, 1 behind), the roll factor by its side, the travel half the
        # radius until BodySpringLength sets the spring added last, and the
        # Body...Factor keys likewise.
        props = [('GeometryName', 'tank'), ('AddSpringBody', '-1.8 1.4 1.7 2.5'), ('AddSpringBody', '1.8 1.4 1.7 2.5'),
                 ('BodySpringLength', '1.0'), ('AddSpringBody', '0.0 2.4 -2.0 3.5'), ('BodyOmegaXSpringFactor', '2'),
                 ('BodyOmegaZDampFactor', '0.5//low')]
        springs = hover_springs(props)
        self.assertEqual([s['at'] for s in springs], [[-1.8, 1.4, 1.7], [1.8, 1.4, 1.7], [0.0, 2.4, -2.0]])
        self.assertEqual([s['length'] for s in springs], [1.25, 1.0, 1.75])
        self.assertEqual(springs[0]['factors'], [1.0, 1.0, -1.0, 1.0, -1.0, 1.0])
        self.assertEqual(springs[1]['factors'], [1.0, 1.0, -1.0, 1.0, 1.0, 1.0])
        self.assertEqual(springs[2]['factors'], [1.0, 1.0, 2.0, 1.0, 0.0, 0.5])
        # Six at most; the unnamed 77d6d149 sets the travel's share of the
        # radius for the springs after it, and GeometryName puts it back.
        many = [('77d6d149', '0.25')] + [('AddSpringBody', f'{i} 0 0 2') for i in range(8)]
        self.assertEqual(len(hover_springs(many)), 6)
        self.assertEqual(hover_springs(many)[0]['length'], 0.5)
        self.assertEqual(hover_springs([('77d6d149', '0.25'), ('GeometryName', 'x'), ('AddSpringBody', '0 0 0 2')])[0]['length'], 1.0)
        self.assertEqual(hover_springs([('AddSpringBody', '0 1 2')]), [])

    def test_creature_triangles_keep_each_vertex_joint_and_bind_space(self):
        part={'positions':[[0,0,0],[1,0,0],[1,1,0]], 'normals':[[0,0,1]]*3,
              'local_positions':[[9,9,9]]*3, 'local_normals':[[0,1,0]]*3,
              'indices':[0,1,2], 'joints':[0,1,1], 'bone':'hip'}
        result=vehicle_parts([part], {'names':['hip','leg']})
        self.assertEqual(len(result),1)
        self.assertEqual(result[0]['indices'],[0,1,2])
        self.assertEqual(result[0]['joints'],[0,1,1])
        self.assertEqual(result[0]['local_positions'],part['positions'])
        self.assertIsNone(result[0]['bone'])

    def test_spawn_roster_uses_native_faction_and_role_fields_without_adding_pads(self):
        record={'class':'com_item_vehicle_spawn', 'name':'pad',
                'xfrm':[1,0,0,0,1,0,0,0,1,12,3,24],
                'props':[('Team','1'),('ControlZone','CP1'),('SpawnTime','60'),
                         ('ClassAllATK','imp_hover_speederbike'),
                         ('ClassImpDEF','bes_fly_cloudcar'),
                         ('ClassImpATK','imp_walk_atst'),
                         ('ClassAllDEF','tat_hover_skiff'),
                         ('ClassHisATK','nab_hover_gianspeeder')]}
        pads=vehicle_spawns([record],{'1':'all','2':'imp'})
        self.assertEqual(len(pads),1)
        self.assertEqual(pads[0]['classes'],{'1':'imp_hover_speederbike','2':'bes_fly_cloudcar'})
        self.assertEqual(pads[0]['matrix'][12:15],[12,3,24])
        self.assertEqual(pads[0]['respawnTime'],60)
        self.assertEqual(pads[0]['post'],'cp1')
        swapped=vehicle_spawns([record],{'1':'all','2':'imp'},attacker=2)
        self.assertEqual(swapped[0]['classes'],{'1':'tat_hover_skiff','2':'imp_walk_atst'})
        self.assertEqual(vehicle_spawns([],{'1':'all','2':'imp'}),[])
        self.assertEqual(vehicle_spawns([record],{'1':'rep','2':'cis'}),[])
        self.assertEqual(vehicle_spawns([{**record,'class':'commandpost'}],{'1':'all','2':'imp'}),[])

    def test_seats_keep_weapon_sections_and_early_turret_aimers(self):
        props = [('WakeEffect', 'water'), ('ControlSection','BODY'), ('GeometryName','tank'),
                 ('WeaponSection','1'), ('WeaponName','cannon'), ('WeaponAmmo','0'),
                 ('AimerNodeName','left'), ('FirePointName','hp_left'),
                 ('NextAimer','-'), ('AimerNodeName','right'), ('FirePointName','hp_right'),
                 ('WeaponSection','2'), ('WeaponName','missile'),
                 ('ControlSection','TURRET1'), ('AimerNodeName','turret'), ('FirePointName','hp_turret'),
                 ('WeaponName','turretgun'), ('CHUNKSECTION','chunk'), ('GeometryName','debris')]
        seats = sections(props)
        self.assertEqual(len(seats),2)
        self.assertEqual(seats[0]['settings']['GeometryName'],'tank')
        self.assertEqual([w['class'] for w in seats[0]['weapons']],['cannon','missile'])
        self.assertEqual([a['FirePointName'] for a in seats[0]['weapons'][0]['aimers']],['hp_left','hp_right'])
        self.assertEqual(seats[1]['aimers'][0]['FirePointName'],'hp_turret')
        self.assertEqual(seats[1]['weapons'][0]['class'],'turretgun')

    def test_a_second_weapon_name_without_a_section_stays_in_its_slot(self):
        # rep_fly_gunship TURRET1: one wing laser with an aimer per wing.
        props = [('ControlSection','BODY'), ('WeaponSection','1'), ('WeaponName','cannon'),
                 ('WeaponSection','2'), ('WeaponName','missile'), ('WeaponAmmo','6'),
                 ('ControlSection','TURRET1'), ('WeaponSection','1'), ('WeaponName','wing'), ('WeaponAmmo','0'),
                 ('AimerNodeName','wing_turret_2'), ('NextAimer','-'),
                 ('WeaponName','wing'), ('WeaponAmmo','0'), ('AimerNodeName','wing_turret_1'),
                 ('ControlSection','TURRET2'), ('WeaponName','ball')]
        seats = sections(props)
        self.assertEqual([[w['class'] for w in s['weapons']] for s in seats], [['cannon','missile'], ['wing'], ['ball']])
        self.assertEqual([a['node'] for a in seats[1]['weapons'][0]['aimers']], ['wing_turret_2','wing_turret_1'])
        self.assertEqual(seats[0]['weapons'][1]['ammo'], 6)

    def test_hover_and_walker_section_hashes_share_parser(self):
        self.assertEqual(property_name(0x4cbe283c),'ControlSection')
        self.assertEqual(property_name(0x1cd9f762),'ControlSection')
        self.assertEqual(property_name(0xd42834bd),'ForwardSpeed')
        self.assertEqual(property_name(0x447c6db0),'ControlZone')

    def test_walker_collision_includes_bone_attached_shapes(self):
        parent=IDENTITY.copy(); parent[13]=5
        skeleton={'names':['bone_body'],'model':[parent]}
        shapes={'walker':[{'name':'p_main','parent':'bone_body','matrix':IDENTITY,'kind':4,'size':[1,1,1]}]}
        c=vehicle_collision('walker',{'props':[('OrdnanceCollision','p_main')]},{},shapes,skeleton)
        self.assertEqual(len(c['indices']),36)
        self.assertEqual(min(c['positions'][1::3]),4)
        self.assertEqual(max(c['positions'][1::3]),6)

    def test_a_flyer_keeps_the_clips_that_move_more_than_its_root(self):
        # A takeoff whose only tracks the model has are DummyRoot's constant
        # lift (the cloud car's) poses nothing the port draws.
        names = ['DummyRoot', 'bone_gear', 'hp_fire']
        clip = lambda *tracks: {'frames': 2, 'flags': 0, 'offset': 0, 'tracks': list(tracks), 'travel': [0, 0, 0]}
        meta = {'fps': 30, 'bones': names, 'clips': {'takeoff': clip(0, 0, 1)}}
        self.assertEqual(flyer_clips(meta, names), meta)
        self.assertIsNone(flyer_clips({**meta, 'clips': {'takeoff': clip(0, 0)}}, names))
        kept = flyer_clips({**meta, 'clips': {'takeoff': clip(0, 1), 'fins': clip(0)}}, names)
        self.assertEqual(list(kept['clips']), ['takeoff'])
        self.assertEqual(kept['bones'], names)

if __name__ == '__main__': unittest.main()
