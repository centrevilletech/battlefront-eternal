//! Mounted callbacks borrow the active Core at their original call point.
//! Geometry and F2 commands loan their owners only for each operation; AI
//! requests and helper state never cross a host callback or mirrored row.
use super::rows::{row_context,row_nav,row_store_mut};
use super::{active::Active,arsenals::Mounted,rows::Rows,weapons::Changes,world::{Services,World},navigation::{Actor,Command}};
use crate::{ai::{combat::binding::CombatAi,control::{self as ctl,world::{Characters,EntRef}},phase::RandomState,vehicles::world::*},vehicles::{BodyId,Hit}};
use three::Vector3;
fn core_index<C:Changes+?Sized,M:Mounted>(rows:&Rows<'_,C,M>,key:Obj)->usize {rows.state.controllers.core(&rows.state.entities,key).expect("current native vehicle controller")}
fn agent_controller<C:Changes+?Sized,M:Mounted>(rows:&Rows<'_,C,M>,key:Obj)->usize {rows.state.agents.get(&rows.state.entities,key).expect("running native vehicle agent").core}
fn vector(p:crate::ai::nav::math::Vec3)->Vector3 {Vector3::new(p.x,p.y,p.z)}
fn request(value:ctl::controller::MoveRequest)->Request {Request {valid:value.valid,steering_target:vector(value.steering_target),waypoint:vector(value.waypoint),segment_direction:vector(value.segment_direction),mode:value.mode,lower:value.lower,upper:value.upper,..Default::default()}}
impl<S> Active<'_,S> {
    fn vehicle_actor<C:Changes+?Sized,M:Mounted>(rows:&Rows<'_,C,M>,key:Option<Obj>)->Option<(usize,Actor)> {
        let index=rows.state.controllers.core(&rows.state.entities,key?)?;Some((index,rows.state.actor(index)?))
    }
    pub(super) fn flight_flags<C:Changes+?Sized,M:Mounted>(&self,rows:&mut Rows<'_,C,M>,agent:Obj,over:bool,altitude:bool,land:Option<bool>) {
        let index=agent_controller(rows,agent);let Some(unit)=self.core.controllers[index].unit else{return};
        rows.vehicle_steer(unit,|state|{let state=state.get_or_insert_with(Steer::full);if let Some(land)=land {state.land=Some(land);}else{state.path_override=Some(over);state.path_altitude=Some(altitude);}});
    }
}
impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> super::vehicle_frame::Hooks<C,M> for Active<'_,S> {
    fn support_holds(&self,rows:&Rows<'_,C,M>, agent: Obj) -> Option<f64> { let index=agent_controller(rows,agent);self.core.objective(index).and_then(|o|o.support_holds) }
    fn cast(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> { super::vehicle_frame::Hooks::cast(self.services,rows,random,combat,origin,direction,far,ignore) }
    fn mount_origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, origin: Origin) -> Option<Vector3> { super::vehicle_frame::Hooks::mount_origin(self.services,rows,random,combat,origin) }
    fn eligible_seat(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> f64 { super::vehicle_frame::Hooks::eligible_seat(self.services,rows,random,combat,unit,hull) }
    fn board(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> bool { Services::seat_action(self.services,self.core,rows,random,combat,self.facts,self.settings,super::physical::SeatAction::Board {unit,hull}) }
    fn man(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, turret: Obj) -> bool { Services::seat_action(self.services,self.core,rows,random,combat,self.facts,self.settings,super::physical::SeatAction::Man {unit,turret}) }
    fn request_exit_vehicle(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool { Services::seat_action(self.services,self.core,rows,random,combat,self.facts,self.settings,super::physical::SeatAction::RequestVehicle {unit,options}) }
    fn request_exit_turret(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool { Services::seat_action(self.services,self.core,rows,random,combat,self.facts,self.settings,super::physical::SeatAction::RequestTurret {unit,options}) }
    fn unboard(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool { Services::seat_action(self.services,self.core,rows,random,combat,self.facts,self.settings,super::physical::SeatAction::Unboard {unit,options}) }
    fn damage(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, hull: Obj, amount: f64) { Services::hull_damage(self.services,self.core,rows,random,combat,self.facts,self.settings,hull,amount) }
    fn pilot_input(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick { super::vehicle_frame::Hooks::pilot_input(self.services,rows,random,combat,control,request,aim,dt,env) }
    fn nav_attach(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi, controller: Option<Obj>, unit: Obj) { let Some((index,actor))=Self::vehicle_actor(rows,controller) else{return};row_nav!(rows,command,random,self.services,actor,Command::Attach);if let Some(obs)=rows.state.navigation.observation(row_store_mut!(rows),actor) {self.core.controllers[index].request=obs.request;self.core.controllers[index].nav_control=Some(obs);}let _=unit; }
    fn nav_update(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi, controller: Option<Obj>, dt: f64) { let Some((index,actor))=Self::vehicle_actor(rows,controller) else{return};row_nav!(rows,command,random,self.services,actor,Command::Update(dt));if let Some(obs)=rows.state.navigation.observation(row_store_mut!(rows),actor) {self.core.controllers[index].request=obs.request;self.core.controllers[index].nav_control=Some(obs);} }
    fn nav_request(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, controller: Option<Obj>) -> Option<Request> { let Some((index,actor))=Self::vehicle_actor(rows,controller) else{return None};rows.state.navigation.observation(row_store_mut!(rows),actor).map(|obs|request(obs.request)).or_else(||Some(request(self.core.controllers[index].request))) }
    fn request_objective(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, exclude: bool) { let index=core_index(rows,controller);let mut draws=random.share();let mut world=World {rows,random,combat,services:self.services,settings:self.settings,facts:self.facts};self.core.request_objective(&mut world,&mut draws,index,exclude); }
    fn controller_request_exit(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) { let index=core_index(rows,controller);let mut world=World {rows,random,combat,services:self.services,settings:self.settings,facts:self.facts};self.core.request_exit(&mut world,index); }
    fn set_reboard_at(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, controller: Obj, t: f64) { let index=core_index(rows,controller);self.core.controllers[index].reboard_at=t; }
    fn fire_a(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, controller: Obj) { let index=core_index(rows,controller);self.core.controllers[index].fire_a=true;rows.state.controllers.write(&rows.state.entities,controller).unwrap().fire_a=true; }
    fn invalidate_request(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, controller: Obj) { let index=core_index(rows,controller);self.core.controllers[index].request.valid=false;if let Some(actor)=rows.state.actor(index) {if let Some(control)=rows.state.navigation.control_mut(actor) {control.request.valid=false;}} }
    fn add_vehicle_note(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, controller: Obj, note: Ref) { let index=core_index(rows,controller);self.core.controllers[index].add_vehicle_note(EntRef {unit:note.obj,generation:note.generation}); }
    fn set_objective(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, character: Obj, post: Obj) { let character=rows.character(character).expect("current native vehicle Character");let index=rows.state.roster.index(row_context!(rows),rows.characters,character).expect("prepared vehicle Character");let post=match rows.state.entities.get(post).expect("current native vehicle post") {super::entities::Entity::Post(r)=>crate::gen::PostRef::Post(r),super::entities::Entity::MobilePost(r)=>crate::gen::PostRef::MobilePost(r),_=>panic!("native vehicle post kind")};let post=rows.state.population.index(post).expect("prepared vehicle post");let mut world=World {rows,random,combat,services:self.services,settings:self.settings,facts:self.facts};world.character_set_objective(index,Some(post)); }
    fn set_support_holds(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, agent: Obj, holds: f64) { let index=agent_controller(rows,agent);if let Some(objective)=self.core.objective_mut(index) {objective.support_holds=Some(holds);} }
    fn set_patrol_expires(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, patrol: Obj, t: f64) { let index=agent_controller(rows,patrol);if let Some(objective)=self.core.objective_mut(index) {objective.patrol.expires=t;} }
    fn patrol_request_state(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, patrol: Obj, state: f64) { let index=agent_controller(rows,patrol);self.core.request_state(ctl::Owner::Patrol(index),state); }
    fn face(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi, patterns: Obj, posture: f64) {
        let settings=self.settings;let time=self.facts.time;
        let mut frame=super::frame::Frame {rows,hooks:self,random,settings,time,response:None};
        let mut machine=frame.rows.state.agents.combat(&frame.rows.state.entities,patterns).expect("native mounted face agent");
        machine.face(&mut frame,posture);
    }
    fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi, agent: Obj, point: &Vector3, direct: bool) { super::frame::Hooks::helper_goto(self,rows,random,agent,point,if direct {Some(crate::ai::combat::world::GotoMode::Direct)}else{None}); }
    fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi, agent: Obj) { super::frame::Hooks::helper_stop(self,rows,random,agent); }
    fn helper_follow_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool) { super::frame::Hooks::flight_path(self,rows,random,agent,points,over,altitude); }
    fn helper_land(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi, agent: Obj, on: bool) { self.flight_flags(rows,agent,false,false,Some(on)); }
}
