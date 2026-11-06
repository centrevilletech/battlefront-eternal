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
    fn fire_round(&mut self,_:&Vector3,_:&Vector3,_:&Shot,_:bool,_:Option<(f64,f64)>,_:&mut Unit,_:&mut Combat) {}
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
    fn next_gun(&mut self,_:&mut Unit,_:&mut Combat)->Option<Vector3> {None}
    fn gun_at(&mut self,_:(f64,f64))->Option<Vector3> {None}
    fn shield_hit(&mut self,_:f64,_:f64,_:Option<Vector3>,_:bool) {}
    fn draw_combat(&mut self,_:bool)->f64 {self.draw(0)}
    fn text(&self,_:StrId)->String {String::new()}
}
impl UnitsWorld for World {
    fn cast(&mut self,_:&Vector3,_:&Vector3,_:f64)->Option<CastHit> {None}
    fn visit_gathered(&mut self, _: u32, _: &Vector3, _: f64, _: usize,
        _: &mut dyn FnMut(bool, &[Vector3; 3])) {}
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
        combat::update_buffered(&mut c,id,u,&defs,h,stick,&Edges::default(),self,&mut scratch);
        u.combat=c;self.defs=defs;self.scratch=scratch;
    }
    fn flush_muzzles(&mut self,_:u32,u:&mut Unit) {
        assert_eq!(self.order,3);self.order=4;
        let defs=std::mem::take(&mut self.defs);
        let mut pending=std::mem::take(&mut self.scratch.pending);
        let mut c=std::mem::take(&mut u.combat);
        combat::flush_muzzles_buffered(&mut c,u,&defs,&Edges::default(),self,&mut pending);
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
