//! Native AI service scopes borrow canonical owners for an entire phase.
//! Observation-only combat frames do not acquire Core; stateful agent and
//! vehicle calls use Active, which carries that Core explicitly. This also
//! makes recursive physical callbacks use the caller's recon/query loans.
use super::rows::{row_buildings,row_nav,row_seats_mut,row_store,row_store_mut};
use super::{arsenals::{self,Live},frame::{Hooks,Settings},rows::Rows,weapons::F3,entities::Entity,world::Facts,scratch::List};
use crate::{ai::{combat::{binding::CombatAi,world::{Obj,Sphere,QueryFilter,GotoMode}},control::{self as ctl,world::{UKey,CoreEffect}},nav::math::Vec3,phase::RandomState},battle::live::Queries,recon::Authority,vehicles::{BodyId,Hit}};
use three::Vector3;
/// Voice publication and first-use item ownership are battle-scoped leaves.
/// Neither can reenter an AI decision or observe a copied body snapshot.
pub trait Voice {
    fn emit(&self,time:f64,rows:&mut Rows<'_,F3,Live>,unit:Obj,event:&str,property:&str,command:f64,context:Option<f64>,positional:Option<bool>,notice:Option<bool>);
}
pub trait OwnedItems {fn count(&self,rows:&mut Rows<'_,F3,Live>,unit:Obj,weapon:(crate::store::DefId,u32))->usize;}
pub struct Context<V:Voice,I:OwnedItems> {
    pub time:f64,pub follow_far:f64,pub stream_a:u32,
    pub vehicles:super::vehicles::Binding,pub voice:V,pub items:I,
}
pub struct Scope<'a,V:Voice,I:OwnedItems> {
    pub context:&'a Context<V,I>,pub recon:&'a mut Authority,pub queries:&'a mut Queries,
}
impl<V:Voice,I:OwnedItems> super::callbacks::Factory for Context<V,I> {
    type Services<'a>=Scope<'a,V,I> where Self:'a;
    fn services<'a>(&'a self,recon:&'a mut Authority,queries:&'a mut Queries)->Self::Services<'a> {Scope {context:self,recon,queries}}
}
impl<V:Voice,I:OwnedItems> super::navigation::Environment for Scope<'_,V,I> {
    fn time(&self)->f64 {self.context.time}
    fn building_target(&mut self,w:&crate::world::owner::World,handle:&crate::gen::F4Handle)->Option<super::navigation::FollowView> {
        let rows=&crate::battle::f4::live_ref(&w.battle).buildings;let current=rows.current(handle)?;let building=rows.get(&current)?;
        let p=super::buildings::position(building);let state=&building.state.building;
        Some(super::navigation::FollowView {position:Vec3::new(p.x,p.y,p.z),velocity:Vec3::default(),generation:state.target_generation.unwrap_or(1.),alive:state.alive()})
    }
    // A log trap's immutable pad is its repair/follow point. Health and life
    // still resolve the same canonical trap as F4 and the cutter lock.
    fn trap_target(&mut self,w:&crate::world::owner::World,handle:&crate::gen::F4Handle,position:Vec3)->Option<super::navigation::FollowView> {
        let rows=&crate::battle::f4::live_ref(&w.battle).traps;let current=rows.current(handle)?;let trap=rows.get(&current)?;
        Some(super::navigation::FollowView {position,velocity:Vec3::default(),generation:1.,alive:trap.state.built})
    }
    // Game supplies no remembered-position option to its navigation helpers.
    fn remembered(&mut self,_:super::navigation::Actor)->Option<Vec3> {None}
    fn follow_far(&mut self)->f64 {self.context.follow_far}
    fn clear_above(&mut self,w:&mut crate::world::owner::World,from:Vec3,unit:&crate::gen::AiLifeHandle)->bool {super::live::clear_above(w,from,unit)}
    fn seat_frame(&mut self,w:&mut crate::world::owner::World,hull:crate::vehicles::f2_seats::HullLife,index:usize)->Option<(Vec3,Vec3)> {super::live::seat_frame(w,hull,index)}
}
// The shared algorithm traits also describe agent actions. A passive query
// frame cannot perform those actions: World creates Active before invoking an
// agent, mounted pattern, recovery or decay. Reject a scope violation at that
// call instead of acquiring a second Core or invoking a foreign AI method.
fn active()->! {unreachable!("native AI action requires its active Core scope")}
fn helper(rows:&Rows<'_,F3,Live>,agent:Obj)->Option<ctl::agents::Helper> {
    let row=rows.state.agents.get(&rows.state.entities,agent)?;
    let mut helper=row.helper;
    if let Some(key)=row.navigation {if let Some(obs)=rows.state.navigation.helper_observation_for(row_store!(rows),key) {helper.obs=obs;}}
    Some(helper)
}
// VehicleAI.mountAngles refuses a mount whose body or lease has retired.
// Keep absence through the shared traits so no origin at zero can be targeted.
fn mount_origin(rows:&mut Rows<'_,F3,Live>,origin:crate::ai::vehicles::world::Origin)->Option<Vector3> {
    use crate::ai::vehicles::world::Origin;
    match origin {
        Origin::Lease(key)=>{let Entity::Lease(lease)=rows.state.entities.get(key)? else{return None};rows.lease_origin(lease,&mut super::geometry::F2)},
        Origin::TurretPivot(key)=>{let Entity::Turret(turret)=rows.state.entities.get(key)? else{return None};rows.state.turrets.origin(row_seats_mut!(rows),row_buildings!(rows),turret)},
        Origin::SeatOrigin(key,index)=>{let Entity::Hull(hull)=rows.state.entities.get(key)? else{return None};row_store_mut!(rows).vehicle.get(hull)?;super::observe::Hulls::origin(&mut super::geometry::F2,rows.w,hull,index)},
    }
}

#[allow(unused_variables)]
impl<V:Voice,I:OwnedItems> Hooks<F3,Live> for Scope<'_,V,I> {
fn vehicle_binding(&self)->super::vehicles::Binding { self.context.vehicles.clone() }
fn fire_primary(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,controller:Obj) { active() }
fn flight_path(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent:Obj,points:&[[f64;3]],over:bool,altitude:bool) { active() }
fn mount_origin(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,origin:crate::ai::vehicles::world::Origin)->Option<Vector3> { mount_origin(rows,origin) }
fn origin(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,u: Obj)-> Vector3 { super::live::origin(rows,u) }
fn forward(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,u: Obj)-> Vector3 { super::live::forward(rows,u) }
fn aim_point(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,u: Obj)-> Vector3 { super::live::sphere(rows,u).center }
fn sphere(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,u: Obj)-> Sphere { super::live::sphere(rows,u) }
fn use_classic_random(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,u: Obj) { let Some(owner)=arsenals::Owner::resolve(row_store_mut!(rows),&rows.state.entities,&rows.state.births,u) else{return};let stream=random.weapon_stream();
        owner.bind_classic(rows.w,self.context.stream_a,stream); }
fn query(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,center: &Vector3,radius: f64,filter: QueryFilter)-> List<Obj> { super::live::query(rows,center,radius,filter) }
fn relation(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,a: f64,b: f64)-> f64 { crate::units::f3::relation_in(&rows.w.units,a,b) }
fn living(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState)-> List<Obj> { super::live::living(rows) }
fn los(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,from: &Vector3,to: &Vector3,a: Option<Obj>,b: Option<Obj>)-> bool { super::live::los(rows,from,to,a,b) }
fn set_stance(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,c:Obj,stance:f64) { let Some(controller)=rows.state.controllers.write(&rows.state.entities,c) else{return};controller.set_stance(stance);
        if let Some(index)=rows.state.controllers.core(&rows.state.entities,c) {if let Some(actor)=rows.state.actor(index) {if let Some(control)=rows.state.navigation.control_mut(actor) {control.stance=stance;}}} }
fn human_target(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,hull:Obj)->Option<Obj> { rows.human_reticle_target(hull) }
fn interrupts_tick(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent: Obj,combat:&mut crate::ai::combat::binding::CombatAi)-> bool { active() }
fn has_helper(&self,rows:&Rows<'_,F3,Live>,agent: Obj)-> bool { helper(rows,agent).is_some_and(|helper|helper.kind.is_some()) }
fn helper_arrived(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent: Obj)-> bool { let Some(helper)=helper(rows,agent) else{return false};match helper.kind {None=>false,Some(ctl::world::HelperKind::Direct)=>helper.direct.arrived,_=>helper.obs.arrived} }
fn helper_flying(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent: Obj)-> bool { false }
fn helper_has_follow(&self,rows:&Rows<'_,F3,Live>,agent: Obj)-> bool { if helper(rows,agent).is_some_and(|helper|helper.kind==Some(ctl::world::HelperKind::Stationary)) {return true;}
        rows.state.agents.get(&rows.state.entities,agent).and_then(|row|row.navigation).and_then(|key|rows.state.navigation.helper_has_follow_for(key)).unwrap_or(false) }
fn helper_goto(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent: Obj,point: &Vector3,mode: Option<GotoMode>) { active() }
fn helper_stop(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent: Obj) { active() }
fn helper_tick(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent: Obj,dt: f64) { active() }
fn helper_follow(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent: Obj,target: Obj) { active() }
fn helper_terminate(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,agent: Obj) { active() }
fn request_jet(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,controller: Obj,vector: &Vector3) { active() }
fn emit_vo(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,unit: Obj,event: &str,property: &str) { self.context.voice.emit(self.context.time,rows,unit,event,property,-1.,None,None,None) }
}

mod vehicle {
use super::*;
use crate::ai::vehicles::world::{Origin,ExitOptions,Request,PilotEnv,Stick,Ref};
#[allow(unused_variables)]
impl<V:Voice,I:OwnedItems> super::super::vehicle_frame::Hooks<F3,Live> for Scope<'_,V,I> {
fn support_holds(&self,rows:&Rows<'_,F3,Live>, agent: Obj) -> Option<f64> { active() }
fn cast(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> { crate::vehicles::module::collision_owner(&mut rows.w.fleet).expect("native AI collision world").cast_in(&mut rows.w.collision.state,&*rows.w.movement.flags_mut(),origin,direction,far,ignore)? }
fn mount_origin(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, origin: Origin) -> Option<Vector3> { mount_origin(rows,origin) }
fn eligible_seat(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> f64 { super::super::physical::eligible(rows,unit,hull,&mut |a,b|crate::units::f3::relation_in(&rows.w.units,a,b)) }
fn board(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> bool { active() }
fn man(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, turret: Obj) -> bool { active() }
fn request_exit_vehicle(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool { active() }
fn request_exit_turret(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool { active() }
fn unboard(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool { active() }
fn damage(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, hull: Obj, amount: f64) { active() }
fn pilot_input(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick { super::super::pilot::input(rows,control,request,aim,env) }
fn nav_attach(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>, unit: Obj) { active() }
fn nav_update(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>, dt: f64) { active() }
fn nav_request(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>) -> Option<Request> { active() }
fn request_objective(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, exclude: bool) { active() }
fn controller_request_exit(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) { active() }
fn set_reboard_at(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, t: f64) { active() }
fn fire_a(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) { active() }
fn invalidate_request(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) { active() }
fn add_vehicle_note(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, note: Ref) { active() }
fn set_objective(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, character: Obj, post: Obj) { active() }
fn set_support_holds(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, holds: f64) { active() }
fn set_patrol_expires(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, patrol: Obj, t: f64) { active() }
fn patrol_request_state(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, patrol: Obj, state: f64) { active() }
fn face(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, patterns: Obj, posture: f64) { active() }
fn helper_goto(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, point: &Vector3, direct: bool) { active() }
fn helper_stop(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj) { active() }
fn helper_follow_path(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool) { active() }
fn helper_land(&mut self,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, on: bool) { active() }
}
}

impl<V:Voice,I:OwnedItems> super::world::Services<F3,Live> for Scope<'_,V,I> {
    fn owned_item_count(&self,rows:&mut Rows<'_,F3,Live>,unit:UKey,weapon:(crate::store::DefId,u32))->usize {self.context.items.count(rows,unit,weapon)}
    fn mine_ground_height(&mut self,rows:&mut Rows<'_,F3,Live>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,point:Vec3)->f64 {
        crate::vehicles::module::collision_owner(&mut rows.w.fleet).expect("native AI ground world").ground(&mut rows.w.collision.state,point.x,point.z,point.y+100.).map_or(point.y,|hit|hit.height)
    }
    fn emit_vo(&mut self,rows:&mut Rows<'_,F3,Live>,_random:&mut RandomState,_combat:&mut CombatAi,facts:&Facts,_settings:&Settings,unit:UKey,event:&str,property:&str,command:f64,context:Option<f64>,positional:Option<bool>,notice:Option<bool>) {self.context.voice.emit(facts.time,rows,unit,event,property,command,context,positional,notice)}
    fn nav_analogs(&mut self,rows:&mut Rows<'_,F3,Live>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller:usize,aim:Option<Vec3>,step:f64,facing:bool)->([f64;4],Vec<CoreEffect>) {
        let Some(actor)=rows.state.actor(controller) else{return ([0.;4],Vec::new())};
        (row_nav!(rows,analogs,self,actor,aim,step,facing).unwrap_or([0.;4]),Vec::new())
    }
    fn seat_action(&mut self,core:&mut ctl::Core,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,action:super::physical::SeatAction)->bool {super::physical::seat(self.context,core,rows,random,combat,settings,facts,self.recon,self.queries,action)}
    fn hull_damage(&mut self,core:&mut ctl::Core,rows:&mut Rows<'_,F3,Live>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,hull:UKey,amount:f64) {super::physical::damage(self.context,core,rows,random,combat,settings,facts,self.recon,self.queries,hull,amount)}
}
