//! VehicleAI reads canonical owners through one native frame. Scoped AI
//! edits and F2 intent writes finish before boarding or damage reenters Core.
use super::{arsenals::Mounted,frame::{Frame as CombatFrame,Hooks as CombatHooks,Settings as CombatSettings},rows::Rows,weapons::Changes,entities::Entity,scratch::List};
use crate::{ai::{vehicles::world::*,combat::binding::CombatAi,phase::RandomState},vehicles::{BodyId,Hit}};
use three::Vector3;
/// Native geometry, F2 commands and active-Core actions. Implementations loan
/// owners only during the callback and retain no row reference afterward.
pub trait Hooks<C:Changes+?Sized,M:Mounted>:CombatHooks<C,M> {
 fn support_holds(&self,rows:&Rows<'_,C,M>, agent: Obj) -> Option<f64>;
 fn cast(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit>;
 fn mount_origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, origin: Origin) -> Option<Vector3>;
 fn eligible_seat(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> f64;
 fn board(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> bool;
 fn man(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, turret: Obj) -> bool;
 fn request_exit_vehicle(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool;
 fn request_exit_turret(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool;
 fn unboard(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool;
 fn damage(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, hull: Obj, amount: f64);
 fn pilot_input(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick;
 fn nav_attach(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>, unit: Obj);
 fn nav_update(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>, dt: f64);
 fn nav_request(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>) -> Option<Request>;
 fn request_objective(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, exclude: bool);
 fn controller_request_exit(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj);
 fn set_reboard_at(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, t: f64);
 fn fire_a(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj);
 fn invalidate_request(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj);
 fn add_vehicle_note(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, note: Ref);
 fn set_objective(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, character: Obj, post: Obj);
 fn set_support_holds(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, holds: f64);
 fn set_patrol_expires(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patrol: Obj, t: f64);
 fn patrol_request_state(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patrol: Obj, state: f64);
 fn face(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patterns: Obj, posture: f64);
 fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, point: &Vector3, direct: bool);
 fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj);
 fn helper_follow_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool);
 fn helper_land(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, on: bool);
}
pub struct Frame<'a,'b,C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> {
 pub rows:&'a mut Rows<'b,C,M>,pub hooks:&'a mut H,pub random:&'a mut RandomState,
 pub combat:&'a mut CombatAi,pub combat_settings:&'a CombatSettings,pub settings:&'a Settings,
 pub time:f64,pub navigation:bool,pub population:bool,
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> Frame<'_,'_,C,M,H> {
 fn post_snapshot(&self,key:Obj)->crate::ai::control::world::PostSnap {
  let post=match self.rows.state.entities.get(key).expect("current native post") {
   Entity::Post(reference)=>crate::gen::PostRef::Post(reference),Entity::MobilePost(reference)=>crate::gen::PostRef::MobilePost(reference),_=>panic!("native post identity"),
  };
  let index=self.rows.state.population.index(post).expect("prepared native post");
  self.rows.state.posts.read(self.rows.store,&self.rows.state.population,index,&self.rows.state.bindings.facts(self.rows.buildings.rows())).expect("current native post")
 }
 pub(super) fn combat<R>(&mut self,run:impl FnOnce(&mut CombatFrame<'_,'_,C,M,H>,&mut CombatAi)->R)->R {
  let mut frame=CombatFrame {rows:self.rows,hooks:self.hooks,random:self.random,settings:self.combat_settings,time:self.time,response:None};run(&mut frame,self.combat)
 }
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> crate::ai::vehicles::random::Draws for Frame<'_,'_,C,M,H> {
 fn next_a(&mut self)->u32 {crate::ai::vehicles::random::Draws::next_a(self.random)}
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> World for Frame<'_,'_,C,M,H> {
 fn object_buffer(&self)->List<Obj> {self.rows.state.buffers.units.take()}
 fn now(&self) -> f64 {self.time}
 fn settings(&self) -> &Settings {self.settings}
 fn unit(&self, u: Obj) -> UnitView {self.rows.scene().vehicle(u).unwrap_or_else(||UnitView {obj:u,..Default::default()})}
 fn hull(&self, h: Obj) -> HullView {let Some(Entity::Hull(reference))=self.rows.state.entities.get(h) else{return HullView {obj:h,..Default::default()}};self.rows.state.mounts.hull(self.rows.store,self.rows.seats,&self.rows.state.entities,reference).unwrap_or_else(||HullView {obj:h,..Default::default()})}
 fn turret(&self, t: Obj) -> TurretView {let Some(Entity::Turret(reference))=self.rows.state.entities.get(t) else{return TurretView {obj:t,..Default::default()}};self.rows.state.turrets.view(self.rows.seats,self.rows.buildings.rows(),&self.rows.state.entities,reference).unwrap_or_else(||TurretView {obj:t,..Default::default()})}
 fn lease(&self, l: Obj) -> LeaseView {let Some(Entity::Lease(reference))=self.rows.state.entities.get(l) else{return LeaseView {obj:l,..Default::default()}};self.rows.state.mounts.lease(self.rows.store,&self.rows.state.entities,reference).unwrap_or_else(||LeaseView {obj:l,..Default::default()})}
 fn controller(&self, c: Obj) -> ControllerView {self.rows.state.vehicle_controller(self.rows.store,c).unwrap_or_else(||ControllerView {obj:c,..Default::default()})}
 fn post(&self, p: Obj) -> PostView {let post=self.post_snapshot(p);PostView {obj:p,position:Vector3::new(post.position.x,post.position.y,post.position.z),team:post.team}}
 fn character_unit(&self, character: Obj) -> Option<Obj> {self.rows.character_unit(character)}
 fn character_controller(&self, character: Obj) -> Option<Obj> {let reference=self.rows.character(character)?;self.rows.store.character.get(reference)?.controller.and_then(|c|self.rows.state.entities.find(Entity::Controller(c)))}
 fn hulls(&self) -> List<Obj> {self.rows.state.mounts.hulls(self.rows.store,self.rows.seats,&self.rows.state.entities)}
 fn turrets(&self) -> List<Obj> {let mut out=self.rows.state.buffers.units.take();for turret in &self.rows.seats.crews.turrets {if let Some(obj)=self.rows.state.entities.find(Entity::Turret(turret.turret)) {out.push_reserved(obj);}}out}
 fn tow_weapon(&self,control:Obj)->Option<(usize,f64)> {self.rows.tow_weapon(control)}
 fn reinforcements(&self, team: f64) -> Option<f64> {self.rows.store.battle.reinforcements.at(team)}
 fn human_units(&mut self) -> List<Obj> {let mut out=self.rows.state.buffers.units.take();for index in 0..self.rows.state.roster.handles().len() {
        let Some(reference)=self.rows.state.roster.reference(self.rows.store,self.rows.characters,index) else{continue};
        let character=self.rows.store.character.get(reference).unwrap();if !character.human {continue;}
        if let Some(unit)=character.unit.filter(|r|self.rows.store.unit.get(*r).is_some_and(|u|u.alive)) {out.push_reserved(self.rows.state.entities.find(Entity::Unit(unit)).expect("prepared human identity"));}
        }out}
 fn has_planner(&self) -> bool {self.navigation}
 fn has_graph(&self) -> bool {self.navigation&&self.rows.state.navigation.nav.graph.is_some()}
 fn nav(&mut self) -> Option<&mut crate::ai::nav::Nav> {self.navigation.then_some(&mut self.rows.state.navigation.nav)}
 fn support_holds(&self, agent: Obj) -> Option<f64> {Hooks::support_holds(self.hooks,self.rows,agent)}
 fn hint_link(&self, hint: Obj) -> Option<Obj> {let Entity::Hint(index)=self.rows.state.entities.get(hint)? else{return None};let next=self.rows.state.navigation.nav.hints.list.get(index as usize)?.link?;self.rows.state.entities.find(Entity::Hint(next as u32))}
 fn edit_steer<R>(&mut self, control: Obj, f: impl FnOnce(&mut Option<Steer>) -> R) -> R {self.rows.vehicle_steer(control,f).expect("current native vehicle steering")}
 fn read_decay(&mut self, hull: Obj) -> Option<crate::ai::vehicles::service::Decay> {let Entity::Hull(reference)=self.rows.state.entities.get(hull)? else{return None};self.rows.state.mounts.decay(reference).flatten()}
 fn write_decay(&mut self, hull: Obj, value: crate::ai::vehicles::service::Decay) {let Some(Entity::Hull(reference))=self.rows.state.entities.get(hull) else{panic!("current native hull")};assert!(self.rows.state.mounts.write_decay(reference,value));}
 fn write_vehicle_intent(&mut self, control: Obj, value: SeatIntent) {self.rows.vehicle_fields(control,value);}
 fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {Hooks::cast(self.hooks,self.rows,self.random,self.combat,origin,direction,far,ignore)}
 fn query(&mut self,center:&Vector3,radius:f64,filter:QueryFilter)->List<Obj> {
  use crate::ai::combat::world::QueryFilter as CombatFilter;
  let native=match filter {
   QueryFilter::EnemiesOf {unit,team}|QueryFilter::LiveEnemiesOf {unit,team}=>CombatFilter::Hostile {except:unit,team},
   QueryFilter::FriendsOf {entry,team}=>CombatFilter::AllyOrNeutral {except:entry,team},
  };
  let mut found=CombatHooks::query(self.hooks,self.rows,self.random,center,radius,native);
  if matches!(filter,QueryFilter::EnemiesOf {..}) {found.retain(|key|self.rows.scene().vehicle(*key).is_some_and(|unit|unit.targetable));}
  found
 }
 fn relation(&mut self, a: f64, b: f64) -> f64 {CombatHooks::relation(self.hooks,self.rows,self.random,a,b)}
 fn los(&mut self, a: &Vector3, b: &Vector3) -> Option<bool> {Some(CombatHooks::los(self.hooks,self.rows,self.random,a,b,None,None))}
 fn sphere(&mut self, x: Obj) -> Sphere {let sphere=CombatHooks::sphere(self.hooks,self.rows,self.random,x);Sphere {center:sphere.center,radius:sphere.radius}}
 fn origin(&mut self, control: Obj) -> Vector3 {CombatHooks::origin(self.hooks,self.rows,self.random,control)}
 fn forward(&mut self, control: Obj) -> Vector3 {CombatHooks::forward(self.hooks,self.rows,self.random,control)}
 fn mount_origin(&mut self, origin: Origin) -> Option<Vector3> {Hooks::mount_origin(self.hooks,self.rows,self.random,self.combat,origin)}
 fn hold_yaw(&mut self, turret: Obj, a: f64) -> f64 {let Some(Entity::Turret(reference))=self.rows.state.entities.get(turret) else{panic!("current native turret")};self.rows.state.turrets.hold_yaw(self.rows.seats,self.rows.buildings.rows(),reference,a).expect("current native turret pivot")}
 fn eligible_seat(&mut self, unit: Obj, hull: Obj) -> f64 {Hooks::eligible_seat(self.hooks,self.rows,self.random,self.combat,unit,hull)}
 fn first_free_seat(&mut self, hull: Obj) -> f64 {let Some(Entity::Hull(reference))=self.rows.state.entities.get(hull) else{return -1.};self.rows.store.vehicle.get(reference).map_or(-1.,|h|crate::seats::boarding::first_free_seat(h) as f64)}
 fn board(&mut self, unit: Obj, hull: Obj) -> bool {Hooks::board(self.hooks,self.rows,self.random,self.combat,unit,hull)}
 fn man(&mut self, unit: Obj, turret: Obj) -> bool {Hooks::man(self.hooks,self.rows,self.random,self.combat,unit,turret)}
 fn request_exit_vehicle(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {Hooks::request_exit_vehicle(self.hooks,self.rows,self.random,self.combat,unit,options)}
 fn request_exit_turret(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {Hooks::request_exit_turret(self.hooks,self.rows,self.random,self.combat,unit,options)}
 fn unboard(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {Hooks::unboard(self.hooks,self.rows,self.random,self.combat,unit,options)}
 fn human_target(&mut self, hull: Obj) -> Option<Obj> {CombatHooks::human_target(self.hooks,self.rows,self.random,hull)}
 fn damage(&mut self, hull: Obj, amount: f64) {Hooks::damage(self.hooks,self.rows,self.random,self.combat,hull,amount)}
 fn pilot_input(&mut self, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick {Hooks::pilot_input(self.hooks,self.rows,self.random,self.combat,control,request,aim,dt,env)}
 fn nav_attach(&mut self, controller: Option<Obj>, unit: Obj) {Hooks::nav_attach(self.hooks,self.rows,self.random,self.combat,controller,unit)}
 fn nav_update(&mut self, controller: Option<Obj>, dt: f64) {Hooks::nav_update(self.hooks,self.rows,self.random,self.combat,controller,dt)}
 fn nav_request(&mut self, controller: Option<Obj>) -> Option<Request> {Hooks::nav_request(self.hooks,self.rows,self.random,self.combat,controller)}
 fn reserve_hint(&mut self, hint: Obj, controller: Obj, now: f64) {let Some(Entity::Hint(index))=self.rows.state.entities.get(hint) else{panic!("prepared native hint")};self.rows.state.navigation.nav.hints.reserve(index as usize,Some(controller),now);}
 fn set_hint_reserved_at(&mut self, hint: Obj, at: f64) {let Some(Entity::Hint(index))=self.rows.state.entities.get(hint) else{panic!("prepared native hint")};self.rows.state.navigation.nav.hints.list[index as usize].reserved_at=at;}
 fn set_intent_analogs(&mut self, control: Obj, stick: &Stick) {self.rows.intent(control,super::intent::Command::Analogs([stick.forward,stick.right,stick.turn,stick.pitch]));}
 fn request_objective(&mut self, controller: Obj, exclude: bool) {Hooks::request_objective(self.hooks,self.rows,self.random,self.combat,controller,exclude)}
 fn controller_request_exit(&mut self, controller: Obj) {Hooks::controller_request_exit(self.hooks,self.rows,self.random,self.combat,controller)}
 fn set_reboard_at(&mut self, controller: Obj, t: f64) {Hooks::set_reboard_at(self.hooks,self.rows,self.random,self.combat,controller,t)}
 fn fire_a(&mut self, controller: Obj) {Hooks::fire_a(self.hooks,self.rows,self.random,self.combat,controller)}
 fn invalidate_request(&mut self, controller: Obj) {Hooks::invalidate_request(self.hooks,self.rows,self.random,self.combat,controller)}
 fn add_vehicle_note(&mut self, controller: Obj, note: Ref) {Hooks::add_vehicle_note(self.hooks,self.rows,self.random,self.combat,controller,note)}
 fn set_objective(&mut self, character: Obj, post: Obj) {Hooks::set_objective(self.hooks,self.rows,self.random,self.combat,character,post)}
 fn set_support_holds(&mut self, agent: Obj, holds: f64) {Hooks::set_support_holds(self.hooks,self.rows,self.random,self.combat,agent,holds)}
 fn set_patrol_expires(&mut self, patrol: Obj, t: f64) {Hooks::set_patrol_expires(self.hooks,self.rows,self.random,self.combat,patrol,t)}
 fn patrol_request_state(&mut self, patrol: Obj, state: f64) {Hooks::patrol_request_state(self.hooks,self.rows,self.random,self.combat,patrol,state)}
 fn cp_center(&mut self, post: Obj) -> Vector3 {let p=crate::ai::control::patrol::cp_center(&self.post_snapshot(post),self.population);Vector3::new(p.x,p.y,p.z)}
 fn capture_region(&mut self, post: Obj) -> Option<(f64, f64)> {crate::ai::control::patrol::capture_region_pop(&self.post_snapshot(post),self.population).map(|(_,x,z)|(x,z))}
 fn helper_traversal(&mut self,agent:Obj,radius:bool)->f64 {
  let row=self.rows.state.agents.get(&self.rows.state.entities,agent).expect("current native helper agent");
  let kind=self.rows.state.controllers.helper(&self.rows.state.entities,row.controller).and_then(|(kind,_)|kind);
  // Only GroundGotoHelper defines mask/radius getters. Fly and stationary
  // helpers deliberately retain undefined, which these comparisons reject.
  if kind!=Some(crate::ai::control::world::HelperKind::Ground) {return f64::NAN;}
  let unit=self.rows.state.controllers.read(&self.rows.state.entities,row.controller).and_then(|c|c.unit).expect("current native helper body");
  let unit=self.rows.scene().policy(unit).expect("current native traversal view").nav_unit();
  if radius {crate::ai::nav::units::traversal_radius(&unit)}else{crate::ai::nav::units::traversal_mask(&unit) as f64}
 }
 fn memory_point(&mut self, agent: Obj) -> Option<Vector3> {self.combat(|frame,combat|combat.memory_point(frame,agent))}
 fn select_weapon(&mut self, controller: Obj, control: Obj, target: Obj, switch_allowed: bool) -> Choice {self.combat(|frame,combat| {let choice=crate::ai::combat::weapons_ai::select_weapon(frame,&mut combat.shared,Some(controller),control,Some(target),switch_allowed);
        Choice {ok:choice.ok,action:choice.action,in_band:choice.in_band,weapon:choice.index.map(|index|index as u64)}})}
 fn switch_weapon(&mut self, controller: Obj, control: Obj, index: usize) -> bool {self.combat(|frame,_|crate::ai::combat::weapons_ai::switch_weapon(frame,controller,control,index))}
 fn face(&mut self, patterns: Obj, posture: f64) {Hooks::face(self.hooks,self.rows,self.random,self.combat,patterns,posture)}
 fn set_delay(&mut self, agent: Obj, seconds: f64, now: f64) {self.rows.state.agents.get_mut(&self.rows.state.entities,agent).expect("current native agent").tracker_mut().set_delay(seconds,now);}
 fn default_delay(&mut self, agent: Obj, now: f64) {self.rows.state.agents.get_mut(&self.rows.state.entities,agent).expect("current native agent").tracker_mut().default_delay(now);}
 fn helper_goto(&mut self, agent: Obj, point: &Vector3, direct: bool) {Hooks::helper_goto(self.hooks,self.rows,self.random,self.combat,agent,point,direct)}
 fn helper_stop(&mut self, agent: Obj) {Hooks::helper_stop(self.hooks,self.rows,self.random,self.combat,agent)}
 fn helper_follow_path(&mut self, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool) {Hooks::helper_follow_path(self.hooks,self.rows,self.random,self.combat,agent,points,over,altitude)}
 fn helper_land(&mut self, agent: Obj, on: bool) {Hooks::helper_land(self.hooks,self.rows,self.random,self.combat,agent,on)}
 fn hull_seat_count(&self,h:Obj)->usize {let Some(Entity::Hull(reference))=self.rows.state.entities.get(h) else{return 0};self.rows.state.mounts.seat_count(self.rows.store,reference).unwrap_or(0)}
 fn hull_seat(&self,h:Obj,index:usize)->Option<SeatView> {let Entity::Hull(reference)=self.rows.state.entities.get(h)? else{return None};self.rows.state.mounts.seat(self.rows.store,&self.rows.state.entities,reference,index)}
}
