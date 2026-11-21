//! Unit.tick enters the real buffered combat solver over synthetic content.
//! Only outside-world observations are supplied here; no captured methods or
//! game files participate, so CI covers this whole span on native targets.
use bfe_sim::{combat::{combat::{self,CombatDefs,Edges,Scratch},defs::{Js,JsObj},
    weapons::{Admission,Shot,WeaponDraws},world::{self,ArcTarget,CombatWorld,DamageInfo,Droid,LockPick,Presentation}},
    gen::*,store::{DefId,Nullable,Ref,StrId},units::{shapes::{Bounds,HitShape},world::{CastHit,UnitsWorld}}};
use std::rc::Rc;
use three::Vector3;

#[derive(Default)]
struct World {
    defs:CombatDefs,scratch:Scratch,order:u8,updates:usize,aims:usize,
    releases:usize,respawns:usize,draws:usize,
    // A droideka's guns, what its aim line meets (None: nothing), and the
    // rounds fired (from, direction).
    guns:Vec<Vector3>,next:usize,reach:Option<f64>,fired:Vec<(Vector3,Vector3)>,
    // A bot's quarry's aim point, and the trigger edges the next tick carries.
    quarry:Option<Vector3>,edges:Edges,
    // Where the droideka's guns were turned each tick, and the way its gun
    // rig says they point (None: a world without the rig's guns).
    turns:Vec<Option<Vector3>>,along:Option<Vector3>,
    // The firing channel each aim ray was cast for.
    channels:Vec<f64>,
}
impl World {
    fn new() -> Self {
        let mut world=Self::default();
        // PlayerCombat.regenerate must read the prepared class's AddHealth.
        // An empty loadout isolates tick ordering and control consumption.
        world.defs.set_class(DefId(0),Rc::new(JsObj {fields:vec![
            ("AddHealth".into(),Js::Num(12.)),("weapons".into(),Js::Arr(Rc::new(Vec::new())))],def:None}));
        world.scratch.reserve(64);
        world
    }
}
impl WeaponDraws for World {
    fn draw(&mut self,_:u32)->f64 {self.draws+=1;0.5}
    fn legacy(&mut self,index:u32)->f64 {self.draw(index)}
    fn seeded(&mut self,_:f64,_:f64) {}
    fn admit(&mut self,_:u32)->Admission {Admission::Yes}
}
impl CombatWorld for World {
    // There is no scenery or other body in this fixture. Unexpected weapon
    // services have neutral answers, and the assertions require no RNG draws.
    fn cast(&mut self,_:&Vector3,_:&Vector3,_:f64,_:bool)->Option<world::CastHit> {None}
    fn remote_of(&mut self,_:u32)->Option<Droid> {None}
    fn remote_enter(&mut self,_:Droid,_:&mut Unit,_:&mut Combat) {}
    fn remote_kill(&mut self,_:Droid,_:&mut Unit,_:&mut Combat) {}
    fn remote_launch(&mut self,_:u32,_:&Vector3,_:&Vector3,_:f64,_:f64,_:Option<f64>,_:&mut Unit,_:&mut Combat) {}
    fn lock_on(&mut self,_:&Vector3,_:&Vector3,_:u32,_:&Weapon,_:Option<RepairTarget>,_:bool,_:f64)->Option<RepairTarget> {None}
    fn lock_key(&mut self,_:Option<RepairTarget>)->StrId {StrId(0)}
    fn repair(&mut self,_:u32,_:Option<RepairTarget>,_:f64,_:&mut Unit,_:&mut Combat)->bool {false}
    fn lock_pick(&mut self,_:u32,_:&Weapon,_:&Vector3,_:&Vector3)->LockPick {LockPick::Nothing}
    fn fire_round(&mut self,from:&Vector3,dir:&Vector3,_:&Shot,_:bool,_:Option<(f64,f64)>,_:&mut Unit,_:&mut Combat) {self.fired.push((*from,*dir));}
    fn dispense(&mut self,_:u32,_:f64,_:&Vector3,_:&Vector3,_:f64,_:f64,_:&mut Unit,_:&mut Combat)->bool {false}
    fn melee_strike(&mut self,_:&mut Unit,_:&mut Combat) {}
    fn arc_targets(&mut self)->Vec<ArcTarget> {Vec::new()}
    fn hurt_arc_target(&mut self,_:usize,_:f64,_:&Vector3,_:&mut Unit,_:&mut Combat) {}
    fn has_lightning(&mut self,_:&str)->bool {false}
    fn fired(&mut self,_:&Shot,_:&mut Unit,_:&mut Combat) {}
    fn damaged(&mut self,_:f64,_:Attacker,_:&DamageInfo,_:Option<Vector3>,_:&mut Unit,_:&mut Combat) {}
    fn died(&mut self,_:&mut Unit,_:&mut Combat) {}
    fn blown_apart(&mut self,_:f64,_:&mut Unit,_:&mut Combat) {}
    fn respawn(&mut self,_:&mut Unit,_:&mut Combat) {self.respawns+=1;}
    fn presentation(&mut self,_:&Presentation) {}
    fn next_gun(&mut self,u:&mut Unit,_:&mut Combat)->Option<Vector3> {
        if self.guns.is_empty() {return None;}
        let index=self.next%self.guns.len();self.next+=1;
        if let UnitBody::Droideka(d)=&mut u.soldier {d.last_gun=Some(LastGun {index:index as f64,barrel:0.});}
        Some(self.guns[index])
    }
    fn gun_at(&mut self,(index,_):(f64,f64))->Option<Vector3> {self.guns.get(index as usize).copied()}
    fn aim_reach(&mut self,_:&Unit,_:&Vector3,_:&Vector3,far:f64,channel:f64)->f64 {self.channels.push(channel);self.reach.unwrap_or(far)}
    fn bot_target_point(&mut self)->Option<Vector3> {self.quarry}
    fn turn_guns(&mut self,_:&mut Unit,target:Option<&Vector3>,_:f64) {self.turns.push(target.copied());}
    fn gun_along(&mut self,_:(f64,f64))->Option<Vector3> {self.along}
    fn shield_hit(&mut self,_:f64,_:f64,_:Option<Vector3>,_:bool) {}
    fn draw_combat(&mut self,_:bool)->f64 {self.draw(0)}
    fn text(&self,_:StrId)->String {String::new()}
}
impl UnitsWorld for World {
    fn cast(&mut self,_:&Vector3,_:&Vector3,_:f64)->Option<CastHit> {None}
    fn visit_gathered(&mut self, _: u32, _: &Vector3, _: f64, _: usize,
        _: &mut dyn FnMut(bool, f64, &[Vector3; 3])) {}
    fn ground(&mut self,_:f64,_:f64,_:f64)->Option<(f64,Vector3)> {None}
    fn seat_exposed(&mut self,_:u32,_:&Unit)->bool {false}
    fn rider_shapes(&mut self,_:u32,_:&Unit,_:&mut [HitShape])->usize {0}
    fn held_shapes(&mut self,_:u32,_:&Unit,_:&mut [HitShape])->usize {0}
    fn target_alive(&mut self,_:u32)->bool {false}
    fn target_dead(&mut self,_:u32)->bool {false}
    fn target_sphere(&mut self,_:u32)->Bounds {Bounds::default()}
    // Unit.tick supplies consumed movement intent before its aimer. A simple
    // kinematic body makes that observation point exact and easy to inspect.
    fn move_soldier(&mut self,_:u32,u:&mut Unit,h:f64) {
        self.order=1;
        let UnitBody::Soldier(s)=&mut u.soldier else {unreachable!()};
        s.position[2]+=u.input.forward*h;
    }
    fn damage(&mut self,_:u32,u:&mut Unit,_:f64) {u.alive=false;u.dead=true;}
    fn aim(&mut self,_:u32,_:&mut Unit,_:f64) {assert_eq!(self.order,1);self.order=2;self.aims+=1;}
    fn has_remote_step(&mut self,_:u32,_:&Unit)->bool {false}
    fn remote_step(&mut self,_:u32,_:&mut Unit,_:f64,_:bool) {}
    fn combat_reset(&mut self,_:u32,u:&mut Unit) {
        let mut c=std::mem::take(&mut u.combat);combat::reset(&mut c,u,&self.defs);u.combat=c;
    }
    // This is the same value-frame split as the production module. The real
    // buffered solver consumes controls, regenerates health and computes aim.
    fn combat_update(&mut self,id:u32,u:&mut Unit,h:f64,stick:f64) {
        assert!(self.order==1||self.order==2);self.order=3;self.updates+=1;
        let defs=std::mem::take(&mut self.defs);
        let mut scratch=std::mem::take(&mut self.scratch);
        let mut c=std::mem::take(&mut u.combat);
        let edges=std::mem::take(&mut self.edges);
        combat::update_buffered(&mut c,id,u,&defs,h,stick,&edges,self,&mut scratch);
        u.combat=c;self.defs=defs;self.scratch=scratch;
    }
    fn flush_muzzles(&mut self,_:u32,u:&mut Unit) {
        assert_eq!(self.order,3);self.order=4;
        let defs=std::mem::take(&mut self.defs);
        let mut pending=std::mem::take(&mut self.scratch.pending);
        let mut c=std::mem::take(&mut u.combat);
        combat::flush_muzzles_buffered(&mut c,u,&defs,self,&mut pending);
        u.combat=c;self.defs=defs;self.scratch.pending=pending;
    }
    fn clear_charge(&mut self,_:u32,_:&mut Unit) {}
    fn death_blast(&mut self,_:u32,_:&mut Unit) {}
    fn blow_apart(&mut self,_:u32,_:&mut Unit,_:f64) {}
    fn let_go_of_character(&mut self,_:u32,_:&mut Unit) {}
    fn release(&mut self,_:u32,_:&Unit) {self.releases+=1;}
}
fn soldier()->Unit {
    Unit {class_def:DefId(0),alive:true,root_scale:1.,spheres_tick:-1.,
        combat:Combat {health:100.,max_health:300.,..Default::default()},
        controls:Controls {was:[0.;2],pressed:vec![false;2]},
        soldier:UnitBody::Soldier(Soldier {grounded:true,posture:Posture::Stand,
            max_turn:1.,tick_length:1./60.,pitch_limits:[-1.4,1.4],..Default::default()}),
        ..Default::default()}
}
#[test]
fn complete_tick_moves_then_aims_then_updates_real_combat() {
    let mut world=World::new();let mut unit=soldier();
    unit.intent.forward=1.;unit.intent.reload=true;unit.intent.buttons=[1.,0.];
    for _ in 0..60 {unit.tick(0,1./60.,&mut world,None);assert_eq!(world.order,4);}
    assert_eq!(world.updates,60);assert_eq!(world.aims,60);assert_eq!(world.draws,0);
    assert_eq!(unit.tick_count,60.);assert_eq!(unit.spheres_tick,-1.);
    assert!(!unit.intent.reload);assert!(!unit.controls.pressed[0]);
    assert_eq!(unit.combat.health.to_bits(),112.00000000000017_f64.to_bits());
    let UnitBody::Soldier(body)=&unit.soldier else {unreachable!()};
    assert_eq!(body.position[2].to_bits(),1.0000000000000013_f64.to_bits());
    assert_eq!(unit.combat.aim_dir,[0.,0.,1.]);
}
#[test]
fn held_seated_and_removed_bodies_never_enter_combat() {
    let mut world=World::new();
    for gate in 0..3 {
        let mut unit=soldier();
        match gate {0=>unit.seat=Some(Ref::new(0,1)),1=>unit.held=Nullable::Value(Held::default()),_=>unit.removed=true}
        unit.tick(0,1./60.,&mut world,None);
        assert_eq!(unit.tick_count,if gate==2 {0.} else {1.});
    }
    assert_eq!(world.updates,0);assert_eq!(world.aims,0);assert_eq!(world.draws,0);
}
#[test]
fn corpse_updates_real_combat_until_its_ordered_removal() {
    let mut world=World::new();let mut unit=soldier();
    unit.alive=false;unit.dead=true;unit.combat.dead=true;unit.combat.health=0.;
    for _ in 0..242 {unit.tick(0,1./60.,&mut world,None);}
    assert!(unit.removed);assert_eq!(world.releases,1);assert_eq!(world.respawns,1);
    assert_eq!(world.updates,241);assert_eq!(world.aims,0);assert_eq!(world.draws,0);
    assert_eq!(unit.combat.health,0.);
}

// A droideka player's two guns take turns, and its target point is what its
// reticle is on: in third person the first thing along the chase camera's
// line (a wall 20 m out here), level with its eye; in first person the first
// thing along its eye's line. In a world without the rig's turning guns (this
// one) each bolt goes straight at that point. Before, they met 512 m out
// along the eye's line and passed 2 m from what the reticle was on at 20 m.
#[test]
fn a_droideka_players_bolts_cross_the_point_under_its_reticle() {
    use bfe_sim::combat::{defs::{DefTable,WeaponClass},weapons::arm};
    let object=|fields:Vec<(&str,Js)>| Rc::new(JsObj {fields:fields.into_iter().map(|(k,v)|(k.into(),v)).collect(),def:None});
    let mut world=World::new();
    world.defs.weapons.set(DefId(1),WeaponClass::new(object(vec![("base",Js::Str("cannon".into())),("channel",Js::Num(0.)),
        ("ShotDelay",Js::Num(0.11)),("SalvoCount",Js::Num(1.)),("ShotsPerSalvo",Js::Num(1.)),
        ("ordnance",Js::Obj(object(vec![("base",Js::Str("bolt".into())),("Velocity",Js::Num(80.))])))])));
    world.defs.set_class(DefId(0),object(vec![("base",Js::Str("walkerdroid".into())),("weapons",Js::Arr(Rc::new(vec![
        Js::Obj(Rc::new(JsObj {fields:Vec::new(),def:Some((DefTable::Weapon,DefId(1)))}))])))]));
    world.guns=vec![Vector3::new(0.7,0.8,1.04),Vector3::new(-0.7,0.8,1.04)];
    world.reach=Some(20.);
    let (yaw,pitch)=(0.3,-0.05);
    let droideka=Droideka {position:[3.,1.,-2.],aim_yaw:yaw,aim_pitch:pitch,eye_height:1.8,pitch_limits:[-0.44,0.44],
        track_center:[0.,1.8,0.],track_offset:[0.,0.5,4.5],track_tilt:5f64.to_radians(),grounded:true,..Default::default()};
    for first in [false,true] {
        let mut unit=Unit {class_def:DefId(0),alive:true,human:true,soldier:UnitBody::Droideka(droideka.clone()),
            controls:Controls {was:[0.;2],pressed:vec![true,false]},..Default::default()};
        unit.combat=Combat {health:450.,max_health:450.,player:true,first_person:first,defer_muzzles:true,
            weapons:arm(world.defs.loadout(DefId(0)),&world.defs.weapons,true),seen:vec![Seen::default();1],..Default::default()};
        unit.intent.buttons=[1.,0.];
        world.fired.clear();world.next=0;
        for _ in 0..20 {world.order=1;world.combat_update(0,&mut unit,1./60.,0.);world.flush_muzzles(0,&mut unit);}
        let feet=Vector3::new(3.,1.,-2.);let eye=Vector3::new(3.,1.+1.8,-2.);
        let stand=bfe_presentation::cameras::soldier::Section {eye:[0.;3],center:[0.,1.8,0.],offset:[0.,0.5,4.5],tilt:5f64.to_radians()};
        let (start,direction)=bfe_presentation::aim::camera_line::<bfe_presentation::cameras::math::V8>(feet,yaw,pitch,eye,(!first).then_some(&stand));
        let mut point=start;point.add_scaled_vector(&direction,20.);
        assert!(world.fired.len()>=2,"the held trigger fires both guns: {} rounds",world.fired.len());
        for (n,(from,dir)) in world.fired.iter().enumerate() {
            assert_eq!(*from,world.guns[n%2],"the guns take turns");
            let mut to=point;to.sub(from);
            let along=to.dot(dir);let mut miss=*from;miss.add_scaled_vector(dir,along);
            assert!(miss.distance_to(&point)<1e-9,"bolt {n} misses the reticle's point by {}",miss.distance_to(&point));
        }
        // Third person, the reticle's line runs 0.89 m above the eye, where
        // the 2004 game starts it (0.5 cos 5 + 4.5 sin 5 for this camera).
        let mut rise=start;rise.sub(&eye);
        assert!((rise.length()-if first {0.} else {0.5*5f64.to_radians().cos()+4.5*5f64.to_radians().sin()}).abs()<1e-12);
    }
}

// A soldier player's held rounds converge on his crosshair's point as the
// 2004 game converges them, the same point his own screen aims a press at:
// the ray from the camera's line, level with his eye, TrackOffset.z + 2 m
// on, meets a wall 20 m further here. In first person they fly along the aim.
#[test]
fn a_soldier_players_held_rounds_cross_his_crosshairs_point() {
    use bfe_sim::combat::{defs::{DefTable,WeaponClass},weapons::arm};
    use bfe_presentation::cameras::soldier::Section;
    let object=|fields:Vec<(&str,Js)>| Rc::new(JsObj {fields:fields.into_iter().map(|(k,v)|(k.into(),v)).collect(),def:None});
    let stand=Section {eye:[0.,1.8,0.],center:[0.,1.8,0.],offset:[0.,0.,3.5],tilt:7.5f64.to_radians()};
    let crouch=Section {eye:[0.,1.3,0.],center:[0.,1.3,0.],offset:[0.,0.15,3.5],tilt:7.5f64.to_radians()};
    let zoom=Section {eye:[0.,1.8,0.],center:[0.,1.8,0.],offset:[0.4,0.05,2.8],tilt:3.5f64.to_radians()};
    let sections=[stand,crouch,crouch,zoom,zoom,zoom];
    let cameras=sections.iter().flat_map(|s| s.eye.into_iter().chain(s.center).chain(s.offset).chain([s.tilt])).map(Js::Num).collect();
    let mut world=World::new();
    world.defs.weapons.set(DefId(1),WeaponClass::new(object(vec![("base",Js::Str("cannon".into())),("channel",Js::Num(0.)),
        ("ShotDelay",Js::Num(0.1)),("SalvoCount",Js::Num(1.)),("ShotsPerSalvo",Js::Num(1.)),
        ("ordnance",Js::Obj(object(vec![("base",Js::Str("bolt".into())),("Velocity",Js::Num(300.))])))])));
    world.defs.set_class(DefId(0),object(vec![("base",Js::Str("soldier".into())),("weapons",Js::Arr(Rc::new(vec![
        Js::Obj(Rc::new(JsObj {fields:Vec::new(),def:Some((DefTable::Weapon,DefId(1)))}))]))),("cameras",Js::Arr(Rc::new(cameras)))]));
    world.reach=Some(20.);
    let (yaw,pitch)=(-0.6,-0.12);
    for (first,posture) in [(false,Posture::Stand),(false,Posture::Crouch),(true,Posture::Stand)] {
        let mut unit=soldier();unit.human=true;unit.class_def=DefId(0);unit.controls.pressed=vec![true,false];
        if let UnitBody::Soldier(s)=&mut unit.soldier {s.position=[4.,2.,-3.];s.aim_yaw=yaw;s.aim_pitch=pitch;s.posture=posture;}
        unit.combat=Combat {health:300.,max_health:300.,player:true,first_person:first,
            weapons:arm(world.defs.loadout(DefId(0)),&world.defs.weapons,true),seen:vec![Seen::default();1],..Default::default()};
        unit.intent.buttons=[1.,0.];
        world.fired.clear();
        for _ in 0..20 {world.order=1;world.combat_update(0,&mut unit,1./60.,0.);world.flush_muzzles(0,&mut unit);}
        assert!(world.fired.len()>=3,"the held trigger fires: {} rounds",world.fired.len());
        let section=&sections[posture as usize];
        let (start,direction)=bfe_presentation::aim::crosshair_start::<bfe_presentation::cameras::math::V8>(Vector3::new(4.,2.,-3.),yaw,pitch,section);
        let mut point=start;point.add_scaled_vector(&direction,20.);
        for (n,(from,dir)) in world.fired.iter().enumerate() {
            if first {
                assert!((dir.dot(&direction)-1.).abs()<1e-12,"round {n} leaves along the aim in first person");
                continue;
            }
            let mut to=point;to.sub(from);
            let mut miss=*from;miss.add_scaled_vector(dir,to.dot(dir));
            assert!(miss.distance_to(&point)<1e-9,"round {n} misses the crosshair's point by {}",miss.distance_to(&point));
        }
    }
}

// A soldier class with one held-fire rifle and the given cameras (STAND,
// CROUCH, PRONE, then their ZOOM sections), and a soldier player of it at
// `feet` aiming (yaw, pitch), holding the rifle's trigger; with `launcher`,
// a second gun on the secondary trigger (no zoom of its own) held instead.
#[allow(clippy::too_many_arguments)]
fn rifleman(world:&mut World,sections:&[bfe_presentation::cameras::soldier::Section;6],scoped:bool,feet:[f64;3],yaw:f64,pitch:f64,posture:Posture,launcher:bool)->Unit {
    use bfe_sim::combat::{defs::{DefTable,WeaponClass},weapons::arm};
    let object=|fields:Vec<(&str,Js)>| Rc::new(JsObj {fields:fields.into_iter().map(|(k,v)|(k.into(),v)).collect(),def:None});
    let cameras=sections.iter().flat_map(|s| s.eye.into_iter().chain(s.center).chain(s.offset).chain([s.tilt])).map(Js::Num).collect();
    let mut keys=vec![("base",Js::Str("cannon".into())),("channel",Js::Num(0.)),
        ("ShotDelay",Js::Num(0.1)),("SalvoCount",Js::Num(1.)),("ShotsPerSalvo",Js::Num(1.)),
        ("ordnance",Js::Obj(object(vec![("base",Js::Str("bolt".into())),("Velocity",Js::Num(300.))])))];
    if scoped {keys.push(("ZoomFirstPerson",Js::Num(1.)));}
    world.defs.weapons.set(DefId(1),WeaponClass::new(object(keys)));
    let weapon=|id:DefId| Js::Obj(Rc::new(JsObj {fields:Vec::new(),def:Some((DefTable::Weapon,id))}));
    let mut weapons=vec![weapon(DefId(1))];
    if launcher {
        world.defs.weapons.set(DefId(2),WeaponClass::new(object(vec![("base",Js::Str("cannon".into())),("channel",Js::Num(1.)),
            ("ShotDelay",Js::Num(0.25)),("SalvoCount",Js::Num(1.)),("ShotsPerSalvo",Js::Num(1.)),
            ("ordnance",Js::Obj(object(vec![("base",Js::Str("bolt".into())),("Velocity",Js::Num(60.))])))])));
        weapons.push(weapon(DefId(2)));
    }
    let count=weapons.len();
    world.defs.set_class(DefId(0),object(vec![("base",Js::Str("soldier".into())),("weapons",Js::Arr(Rc::new(weapons))),
        ("cameras",Js::Arr(Rc::new(cameras)))]));
    let mut unit=soldier();unit.human=true;unit.class_def=DefId(0);unit.controls.pressed=vec![!launcher,launcher];
    if let UnitBody::Soldier(s)=&mut unit.soldier {s.position=feet;s.aim_yaw=yaw;s.aim_pitch=pitch;s.posture=posture;}
    unit.combat=Combat {health:300.,max_health:300.,player:true,
        weapons:arm(world.defs.loadout(DefId(0)),&world.defs.weapons,true),seen:vec![Seen::default();count],..Default::default()};
    unit.intent.buttons=if launcher {[0.,1.]} else {[1.,0.]};
    unit
}

// Where a soldier's crosshair ray meets a wall `reach` m beyond its start,
// from first principles (no shipped aim function): the camera `z` m back
// along the aim tipped down by the tilt `t` puts its line z sin t above the
// eye along the aim's own up, n; the ray starts z + 2 m on.
fn crosshair_point(feet:[f64;3],yaw:f64,pitch:f64,section:&bfe_presentation::cameras::soldier::Section,reach:f64)->Vector3 {
    let (eye,z,t)=(section.eye[1],section.offset[2],section.tilt);
    let f=Vector3::new(yaw.sin()*pitch.cos(),pitch.sin(),yaw.cos()*pitch.cos());
    let n=Vector3::new(-yaw.sin()*pitch.sin(),pitch.cos(),-yaw.cos()*pitch.sin());
    let (rise,on)=(z*t.sin(),z+2.+reach);
    Vector3::new(feet[0]+n.x*rise+f.x*on,feet[1]+eye+n.y*rise+f.y*on,feet[2]+n.z*rise+f.z*on)
}

// Zoomed, the ray starts from the posture's ZOOM camera; lying down, from the
// PRONE one; zoomed with a primary whose zoom is first person, rounds fly
// along the aim. Points checked against first principles, not the shipped
// functions.
#[test]
fn a_zoomed_or_prone_soldiers_held_rounds_take_that_cameras_ray() {
    use bfe_presentation::cameras::soldier::Section;
    let section=|eye:f64,z:f64,tilt:f64| Section {eye:[0.,eye,0.],center:[0.,eye,0.],offset:[0.,0.,z],tilt:tilt.to_radians()};
    let sections=[section(1.8,3.5,7.5),section(1.3,3.2,6.),section(0.5,3.0,5.),section(1.8,2.8,3.5),section(1.3,2.6,3.),section(0.5,2.4,2.5)];
    let (feet,yaw,pitch)=([4.,2.,-3.],-0.6,-0.12);
    for (zoomed,posture,scoped,index) in [(true,Posture::Stand,false,3),(false,Posture::Prone,false,2),(true,Posture::Prone,false,5),(true,Posture::Stand,true,3)] {
        let mut world=World::new();world.reach=Some(20.);
        let mut unit=rifleman(&mut world,&sections,scoped,feet,yaw,pitch,posture,false);
        if zoomed {unit.combat.zoom_step=1.;}
        for _ in 0..20 {world.order=1;world.combat_update(0,&mut unit,1./60.,0.);world.flush_muzzles(0,&mut unit);}
        assert!(world.fired.len()>=3,"the held trigger fires: {} rounds",world.fired.len());
        let point=crosshair_point(feet,yaw,pitch,&sections[index],20.);
        let aim=Vector3::new(yaw.sin()*pitch.cos(),pitch.sin(),yaw.cos()*pitch.cos());
        for (n,(from,dir)) in world.fired.iter().enumerate() {
            if scoped {
                assert!((dir.dot(&aim)-1.).abs()<1e-12,"round {n} leaves along the aim through a first person zoom");
                continue;
            }
            let mut to=point;to.sub(from);
            let mut miss=*from;miss.add_scaled_vector(dir,to.dot(dir));
            assert!(miss.distance_to(&point)<1e-9,"section {index} round {n} misses by {}",miss.distance_to(&point));
        }
        assert!(scoped==world.channels.is_empty()&&world.channels.iter().all(|&c|c==0.),"the rifle's rays: {:?}",world.channels);
    }
}

// The view is the current primary's, as the client's and the 2004 game's
// are, whatever weapon fires: zoomed through a primary whose zoom is first
// person, the secondary trigger's gun (no zoom of its own) fires along the
// aim too; zoomed with a primary that stays in third person, it converges
// on the ZOOM camera's point, its ray cast for the secondary's channel.
#[test]
fn a_secondary_fired_zoomed_takes_the_primarys_view() {
    use bfe_presentation::cameras::soldier::Section;
    let section=|eye:f64,z:f64,tilt:f64| Section {eye:[0.,eye,0.],center:[0.,eye,0.],offset:[0.,0.,z],tilt:tilt.to_radians()};
    let sections=[section(1.8,3.5,7.5),section(1.3,3.2,6.),section(0.5,3.0,5.),section(1.8,2.8,3.5),section(1.3,2.6,3.),section(0.5,2.4,2.5)];
    let (feet,yaw,pitch):([f64;3],f64,f64)=([4.,2.,-3.],-0.6,-0.12);
    let aim=Vector3::new(yaw.sin()*pitch.cos(),pitch.sin(),yaw.cos()*pitch.cos());
    let point=crosshair_point(feet,yaw,pitch,&sections[3],20.);
    for scoped in [true,false] {
        let mut world=World::new();world.reach=Some(20.);
        let mut unit=rifleman(&mut world,&sections,scoped,feet,yaw,pitch,Posture::Stand,true);
        unit.combat.zoom_step=1.;
        for _ in 0..40 {world.order=1;world.combat_update(0,&mut unit,1./60.,0.);world.flush_muzzles(0,&mut unit);}
        assert!(world.fired.len()>=2,"the held secondary fires: {} rounds",world.fired.len());
        for (n,(from,dir)) in world.fired.iter().enumerate() {
            if scoped {
                assert!((dir.dot(&aim)-1.).abs()<1e-12,"round {n} leaves along the aim through the primary's first person zoom");
                continue;
            }
            let mut to=point;to.sub(from);
            let mut miss=*from;miss.add_scaled_vector(dir,to.dot(dir));
            assert!(miss.distance_to(&point)<1e-9,"round {n} misses the ZOOM camera's point by {}",miss.distance_to(&point));
        }
        if scoped {
            assert!(world.channels.is_empty(),"no ray is cast through a first person zoom: {:?}",world.channels);
        } else {
            assert!(!world.channels.is_empty()&&world.channels.iter().all(|&c|c==1.),"the secondary's rays: {:?}",world.channels);
        }
    }
}

// A network player's press flies along the aim his screen converged, as
// the press carries it: the world does not converge it a second time. The
// rounds he holds after it converge in the world.
#[test]
fn a_press_keeps_the_aim_its_screen_converged() {
    use bfe_presentation::cameras::soldier::Section;
    use bfe_sim::combat::combat::Edge;
    let stand=Section {eye:[0.,1.8,0.],center:[0.,1.8,0.],offset:[0.,0.,3.5],tilt:7.5f64.to_radians()};
    let sections=[stand;6];
    let (feet,yaw,pitch)=([4.,2.,-3.],-0.6,-0.12);
    let mut world=World::new();world.reach=Some(20.);
    let mut unit=rifleman(&mut world,&sections,false,feet,yaw,pitch,Posture::Stand,false);
    unit.network_human=Some(true);
    let (press_yaw,press_pitch)=(-0.55,-0.08);
    world.edges=Edges {fixed:Some([Some(Edge {button:0.,id:Some(1.),late:None,aim_yaw:Some(press_yaw),aim_pitch:Some(press_pitch)}),None]),..Default::default()};
    for _ in 0..20 {world.order=1;world.combat_update(0,&mut unit,1./60.,0.);world.flush_muzzles(0,&mut unit);}
    assert!(world.fired.len()>=3,"{} rounds",world.fired.len());
    let pressed=Vector3::new(press_yaw.sin()*press_pitch.cos(),press_pitch.sin(),press_yaw.cos()*press_pitch.cos());
    assert!(world.fired[0].1.distance_to(&pressed)<1e-12,"the press flies along its own aim: {:?}",world.fired[0].1);
    let point=crosshair_point(feet,yaw,pitch,&stand,20.);
    for (n,(from,dir)) in world.fired.iter().enumerate().skip(1) {
        let mut to=point;to.sub(from);
        let mut miss=*from;miss.add_scaled_vector(dir,to.dot(dir));
        assert!(miss.distance_to(&point)<1e-9,"held round {n} misses by {}",miss.distance_to(&point));
    }
}

// A bot droideka's guns converge along its aim as far as its quarry's aim
// point (the 2004 game's rule), not on any reticle.
#[test]
fn a_bot_droidekas_guns_converge_at_its_quarrys_distance() {
    use bfe_sim::combat::{defs::{DefTable,WeaponClass},weapons::arm};
    let object=|fields:Vec<(&str,Js)>| Rc::new(JsObj {fields:fields.into_iter().map(|(k,v)|(k.into(),v)).collect(),def:None});
    let mut world=World::new();
    world.defs.weapons.set(DefId(1),WeaponClass::new(object(vec![("base",Js::Str("cannon".into())),("channel",Js::Num(0.)),
        ("ShotDelay",Js::Num(0.11)),("SalvoCount",Js::Num(1.)),("ShotsPerSalvo",Js::Num(1.)),
        ("ordnance",Js::Obj(object(vec![("base",Js::Str("bolt".into())),("Velocity",Js::Num(80.))])))])));
    world.defs.set_class(DefId(0),object(vec![("base",Js::Str("walkerdroid".into())),("weapons",Js::Arr(Rc::new(vec![
        Js::Obj(Rc::new(JsObj {fields:Vec::new(),def:Some((DefTable::Weapon,DefId(1)))}))])))]));
    world.guns=vec![Vector3::new(3.7,1.8,-0.96),Vector3::new(2.3,1.8,-0.96)];
    // Its quarry stands 15 m off, beside its aim; a reticle would see a wall 6 m out.
    world.quarry=Some(Vector3::new(9.,2.3,11.));world.reach=Some(6.);
    let (yaw,pitch)=(0.3,-0.05);
    let droideka=Droideka {position:[3.,1.,-2.],aim_yaw:yaw,aim_pitch:pitch,eye_height:1.8,pitch_limits:[-0.44,0.44],
        track_center:[0.,1.8,0.],track_offset:[0.,0.5,4.5],track_tilt:5f64.to_radians(),grounded:true,..Default::default()};
    let mut unit=Unit {class_def:DefId(0),alive:true,human:false,soldier:UnitBody::Droideka(droideka),
        controls:Controls {was:[0.;2],pressed:vec![true,false]},..Default::default()};
    unit.combat=Combat {health:450.,max_health:450.,defer_muzzles:true,
        weapons:arm(world.defs.loadout(DefId(0)),&world.defs.weapons,true),seen:vec![Seen::default();1],..Default::default()};
    unit.intent.buttons=[1.,0.];
    for _ in 0..20 {world.order=1;world.combat_update(0,&mut unit,1./60.,0.);world.flush_muzzles(0,&mut unit);}
    assert!(world.fired.len()>=2,"{} rounds",world.fired.len());
    let eye=Vector3::new(3.,2.8,-2.);
    let aim=Vector3::new(yaw.sin()*pitch.cos(),pitch.sin(),yaw.cos()*pitch.cos());
    let mut point=eye;point.add_scaled_vector(&aim,eye.distance_to(&Vector3::new(9.,2.3,11.)));
    for (n,(from,dir)) in world.fired.iter().enumerate() {
        let mut to=point;to.sub(from);
        let mut miss=*from;miss.add_scaled_vector(dir,to.dot(dir));
        assert!(miss.distance_to(&point)<1e-9,"bolt {n} misses its quarry's distance by {}",miss.distance_to(&point));
    }
}

// A droideka's guns are turned every tick toward the point its reticle is
// on (back to rest while it is rolled up), and its rounds leave along the
// gun as its rig turned it, not straight at the point.
#[test]
fn a_droidekas_guns_turn_every_tick_and_its_rounds_leave_along_them() {
    use bfe_sim::combat::{defs::{DefTable,WeaponClass},weapons::arm};
    let object=|fields:Vec<(&str,Js)>| Rc::new(JsObj {fields:fields.into_iter().map(|(k,v)|(k.into(),v)).collect(),def:None});
    let mut world=World::new();
    world.defs.weapons.set(DefId(1),WeaponClass::new(object(vec![("base",Js::Str("cannon".into())),("channel",Js::Num(0.)),
        ("ShotDelay",Js::Num(0.11)),("SalvoCount",Js::Num(1.)),("ShotsPerSalvo",Js::Num(1.)),
        ("ordnance",Js::Obj(object(vec![("base",Js::Str("bolt".into())),("Velocity",Js::Num(80.))])))])));
    world.defs.set_class(DefId(0),object(vec![("base",Js::Str("walkerdroid".into())),("weapons",Js::Arr(Rc::new(vec![
        Js::Obj(Rc::new(JsObj {fields:Vec::new(),def:Some((DefTable::Weapon,DefId(1)))}))])))]));
    world.guns=vec![Vector3::new(3.7,1.8,-0.96),Vector3::new(2.3,1.8,-0.96)];
    world.reach=Some(20.);
    let mut along=Vector3::new(0.31,0.05,0.95);along.normalize();
    world.along=Some(along);
    let (yaw,pitch)=(0.3,-0.05);
    let droideka=Droideka {position:[3.,1.,-2.],aim_yaw:yaw,aim_pitch:pitch,eye_height:1.8,pitch_limits:[-0.44,0.44],
        track_center:[0.,1.8,0.],track_offset:[0.,0.5,4.5],track_tilt:5f64.to_radians(),grounded:true,..Default::default()};
    let mut unit=Unit {class_def:DefId(0),alive:true,human:true,soldier:UnitBody::Droideka(droideka),
        controls:Controls {was:[0.;2],pressed:vec![true,false]},..Default::default()};
    unit.combat=Combat {health:450.,max_health:450.,player:true,defer_muzzles:true,
        weapons:arm(world.defs.loadout(DefId(0)),&world.defs.weapons,true),seen:vec![Seen::default();1],..Default::default()};
    unit.intent.buttons=[1.,0.];
    for _ in 0..20 {world.order=1;world.combat_update(0,&mut unit,1./60.,0.);world.flush_muzzles(0,&mut unit);}
    let feet=Vector3::new(3.,1.,-2.);let eye=Vector3::new(3.,2.8,-2.);
    let stand=bfe_presentation::cameras::soldier::Section {eye:[0.;3],center:[0.,1.8,0.],offset:[0.,0.5,4.5],tilt:5f64.to_radians()};
    let (start,direction)=bfe_presentation::aim::camera_line::<bfe_presentation::cameras::math::V8>(feet,yaw,pitch,eye,Some(&stand));
    let mut point=start;point.add_scaled_vector(&direction,20.);
    assert_eq!(world.turns.len(),20,"one turn a tick");
    assert!(world.turns.iter().all(|t| t.is_some_and(|t| t.distance_to(&point)<1e-9)),"toward the reticle's point");
    assert!(world.fired.len()>=2,"{} rounds",world.fired.len());
    assert!(world.fired.iter().all(|(_,dir)| dir.distance_to(&along)<1e-12),"every round along its gun");
    // Rolled up, the guns go back to rest.
    if let UnitBody::Droideka(d)=&mut unit.soldier {d.mode=12.;}
    world.turns.clear();
    for _ in 0..3 {world.order=1;world.combat_update(0,&mut unit,1./60.,0.);world.flush_muzzles(0,&mut unit);}
    assert_eq!(world.turns,vec![None;3]);
}
