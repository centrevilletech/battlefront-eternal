//! Synthetic canonical store fixture. Unbound services fail if the Core
//! crosses their capability gates; navigation and population execute in Rust.
#![allow(unused_variables)]
use bfe_sim::{ai::{control as ctl,nav,owned::{self,navigation::{Actor,Command,Environment,Navigation}}},gen::*,store::{Ref,DefId},battle::commands::Commands};
use ctl::world::*;
use owned::scratch::List;
use nav::math::Vec3;
use std::{alloc::{GlobalAlloc,Layout,System},cell::Cell};
use super::phase::{COUNTING,ALLOCATIONS};
struct Facts;
impl owned::population::Facts for Facts {fn building_alive(&self,_:Ref<Building>)->bool {true}fn vehicle_mask(&self,_:Ref<Vehicle>)->f64 {1.}}
struct Env {now:f64}
impl Environment for Env {
 fn time(&self)->f64 {self.now}
 fn remembered(&mut self,_:Actor)->Option<Vec3> {None}
 fn clear_above(&mut self,_:&mut crate::world::owner::World,_:Vec3,_:&AiLifeHandle)->bool {true}
 fn follow_far(&mut self)->f64 {3.}
 fn seat_frame(&mut self,_:&mut crate::world::owner::World,_:bfe_sim::vehicles::f2_seats::HullLife,_:usize)->Option<(Vec3,Vec3)> {panic!("no seats")}
}
struct Native {
 store:Box<crate::world::owner::World>,commands:Commands,entities:owned::entities::Entities,units:owned::units::Units,
 roster:owned::roster::Roster,population:owned::population::Population,posts:owned::posts::Posts,
 buffers:owned::buffers::Buffers,nav:Navigation,actors:Vec<Actor>,keys:Vec<UKey>,cameras:Vec<CameraView>,
 random:bfe_sim::ai::phase::RandomState,env:Env,vo:usize,crew:Option<CrewSnap>,
 controllers:owned::controllers::Controllers,combat_units:owned::combat::Units,seat_controls:owned::controls::HullControls,mounts:owned::mounts::Mounts,turrets:owned::turrets::Turrets,
 props:owned::buildings::Buildings,seats:bfe_sim::vehicles::f2_seats::Seats,buildings:bfe_sim::battle::live_phase::Live,
}
impl Characters for Native {
 fn character(&self,index:usize)->CharacterSnap {self.roster.read(&self.store,&self.commands,&self.entities,&self.population,index).unwrap()}
 fn character_set_objective(&mut self,index:usize,post:Option<usize>) {self.roster.assign(&mut self.store,&mut self.commands,&self.population,index,post).unwrap();}
}
impl AiWorld for Native {
    fn link_character(&mut self,character:usize,controller:usize,bound:bool) {
      let object=if bound {self.controllers.adopt(controller,&mut self.entities)}else{self.controllers.key(controller).unwrap()};
      owned::controllers::link_character(&mut self.store,&mut self.commands,&self.roster,&self.entities,character,object,bound).unwrap();
      if !bound {assert!(self.controllers.retire(&mut self.entities,object));}
    }
    fn link_controller(&mut self,key:UKey,controller:usize,bound:bool) {
      let object=self.controllers.key(controller).unwrap();
      owned::controllers::link_unit(&mut self.store,&self.entities,key,object,bound).unwrap();
    }

    fn nav_stance_changed(&mut self,controller:usize,stance:f64) {self.nav.control_mut(self.actor(controller)).unwrap().stance=stance;}
    fn nav_jet_requested(&mut self,controller:usize,vector:Vec3) {let control=self.nav.control_mut(self.actor(controller)).unwrap();control.jet.pending=true;control.jet.vector=vector;}

    fn time(&self) -> f64 {self.env.now}
    fn paused(&self) -> bool {false}
    fn unit(&self, key: UKey) -> AiUnit {self.scene().policy(key).unwrap()}
    fn intent_command(&mut self,key:UKey,command:bfe_sim::ai::owned::intent::Command) {assert!(self.units.command(&mut self.store,&self.entities,key,command));}
    fn units_len(&self) -> usize {self.keys.len()}
    fn unit_key(&self,index:usize)->UKey {self.keys[index]}
    fn unit_sphere_radius(&mut self,key:UKey)->Option<f64> {self.unit(key).radius}
    fn unit_forward(&mut self,key:UKey)->Option<Vec3> {Some(Vec3::new(0.,0.,1.))}
    fn unit_aim_point(&mut self,key:UKey)->Option<Vec3> {Some(self.unit(key).position)}
    fn unit_has_origin_forward(&self,key:UKey)->bool {true}
    fn post(&self, index: usize) -> PostSnap {self.posts.read(&self.store,&self.population,index,&Facts).unwrap()}
    fn objective_post(&self,index:usize)->ctl::goals::ObjectivePost<'_> {self.population.view(&self.store,index,&Facts).unwrap()}
    fn posts_len(&self) -> usize {self.population.len()}
    fn post_name_id(&self,index:usize)->u32 {self.posts.name_id(index)}
    fn cameras(&self) -> &[CameraView] {&self.cameras}
    fn network_views(&self) -> bool {false}
    fn lod_disabled_world(&self) -> bool {false}
    fn unit_buffer(&self)->List<UKey> {self.buffers.units.take()}
    fn predicate_buffer(&self)->List<(UKey,bool)> {self.buffers.predicates.take()}
    fn index_buffer(&self)->List<usize> {self.buffers.indices.take()}
    fn weight_buffer(&self)->List<f64> {self.buffers.weights.take()}
    fn objective_buffer(&self)->List<Option<usize>> {self.buffers.objectives.take()}
    fn recruit_buffer(&self)->List<(f64,usize)> {self.buffers.recruits.take()}
    fn sort_recruits(&mut self,rows:&mut Vec<(f64,usize)>) {self.buffers.sort_recruits(rows);}
    fn units_query(&mut self, center: Vec3, radius: f64, filter: &mut dyn FnMut(&AiUnit) -> bool) -> List<UKey> {let mut out=self.buffers.units.take();for &key in &self.keys {let u=self.unit(key);if u.alive&&ctl::vec::distance_sq(u.position,center)<=radius*radius&&filter(&u) {out.push_reserved(key);}}out}
    fn service_candidates(&mut self, center: Vec3, radius: f64) -> List<UKey> {self.buffers.units.take()}
    fn items_len(&self)->usize {0}
    fn item(&self,index:usize)->ItemSnap {panic!("unbound native service item")}
    fn crew(&mut self, character: usize) -> Option<CrewSnap> {if character==0 {self.crew.clone()}else{None}}
    fn objective_vehicle_mask(&self,vehicle:UKey)->f64 {assert_eq!(vehicle,self.keys[0]);32.}
    fn crew_occupants(&self,character:usize)->List<usize> {self.buffers.indices.take()}
    fn tickets(&self, team: f64) -> Option<f64> {None}
    fn reinforcements(&self, team: f64) -> Option<f64> {None}
    fn mine_ground_height(&mut self, point: Vec3) -> f64 {panic!("unbound native service mine_ground_height")}
    fn emit_vo(&mut self, unit: UKey, event: &str, property: &str, command: f64, context: Option<f64>, positional: Option<bool>, notice: Option<bool>) {self.vo+=1;}
    fn combat_feed(&mut self, _controller: usize, _input: &CombatInput)  {}
    fn controller_state_changed(&mut self,controller:usize,state:f64)  {self.controllers.state_changed(controller,state);}
    fn combat_create_perception(&mut self, controller: usize) {panic!("unbound native service combat_create_perception")}
    fn combat_attach_controller(&mut self, controller: usize) {panic!("unbound native service combat_attach_controller")}
    fn combat_detach_controller(&mut self, controller: usize) {panic!("unbound native service combat_detach_controller")}
    fn combat_rebuild_protected(&mut self) {panic!("unbound native service combat_rebuild_protected")}
    fn combat_ingest(&mut self) -> List<AlertState> {panic!("unbound native service combat_ingest")}
    fn combat_init_agent(&mut self, controller: usize) {panic!("unbound native service combat_init_agent")}
    fn combat_scan(&mut self, controller: usize) {panic!("unbound native service combat_scan")}
    fn combat_drop_los_requests(&mut self, controller: usize) {panic!("unbound native service combat_drop_los_requests")}
    fn combat_service_los(&mut self, budget: f64) -> bool {panic!("unbound native service combat_service_los")}
    fn combat_broadcast_acoustic(&mut self, unit: UKey) -> List<(usize, EntRef)> {panic!("unbound native service combat_broadcast_acoustic")}
    fn combat_clear_candidate(&mut self, controller: usize) {panic!("unbound native service combat_clear_candidate")}
    fn combat_rank_targets(&mut self, controller: usize) {panic!("unbound native service combat_rank_targets")}
    fn combat_update_target(&mut self, controller: usize) {panic!("unbound native service combat_update_target")}
    fn combat_update_target_selection(&mut self, controller: usize) {panic!("unbound native service combat_update_target_selection")}
    fn combat_gate_fire(&mut self, controller: usize) {panic!("unbound native service combat_gate_fire")}
    fn combat_current_target(&mut self, controller: usize) -> Option<UKey> {panic!("unbound native service combat_current_target")}
    fn combat_memory_point(&mut self, controller: usize) -> Option<Vec3> {panic!("unbound native service combat_memory_point")}
    fn combat_acquired_target(&mut self, controller: usize) -> Option<UKey> {panic!("unbound native service combat_acquired_target")}
    fn combat_can_engage(&mut self, controller: usize, target: UKey) -> bool {panic!("unbound native service combat_can_engage")}
    fn combat_has_weapon(&mut self, controller: usize) -> bool {panic!("unbound native service combat_has_weapon")}
    fn combat_sensor_range(&mut self, controller: usize, target: UKey) -> f64 {panic!("unbound native service combat_sensor_range")}
    fn combat_acoustic_range(&mut self, controller: usize) -> f64 {panic!("unbound native service combat_acoustic_range")}
    fn combat_consume_fire(&mut self, controller: usize, step: f64) {panic!("unbound native service combat_consume_fire")}
    fn combat_aim_point(&mut self, controller: usize, dt: f64) -> Option<Vec3> {panic!("unbound native service combat_aim_point")}
    fn combat_lead_point(&mut self, controller: usize, target: UKey, dt: f64) -> Option<Vec3> {panic!("unbound native service combat_lead_point")}
    fn combat_mark_skip(&mut self, controller: usize, skipped: bool) {panic!("unbound native service combat_mark_skip")}
    fn combat_hazard_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> {panic!("unbound native service combat_hazard_response")}
    fn combat_alert_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> {panic!("unbound native service combat_alert_response")}
    fn combat_acoustic_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> {panic!("unbound native service combat_acoustic_response")}
    fn weapons_selected(&mut self, controller: usize) -> Option<usize> {None}
    fn weapons_switch(&mut self, controller: usize, index: usize) {panic!("unbound native service weapons_switch")}
    fn weapons_select(&mut self, controller: usize, index: usize) {panic!("unbound native service weapons_select")}
    fn weapons_effective_range(&mut self, controller: usize, weapon: usize) -> f64 {panic!("unbound native service weapons_effective_range")}
    fn weapons_firing_cone(&mut self, controller: usize, point: Vec3) -> bool {panic!("unbound native service weapons_firing_cone")}
    fn combat_repair_target(&mut self, unit: UKey) -> Option<UKey> {panic!("unbound native service combat_repair_target")}
    fn classes_needs(&self,unit:UKey,ammo:bool)->bool  {panic!("unbound native service classes_needs")}
    fn classes_dispenser_available(&mut self,unit:UKey,index:usize)->bool  {panic!("unbound native service classes_dispenser_available")}
    fn classes_mine_weapon(&mut self,unit:UKey)->Option<usize>  {panic!("unbound native service classes_mine_weapon")}
    fn classes_service_weapon(&mut self,unit:UKey,health_type:f64,ammo:bool)->Option<usize>  {panic!("unbound native service classes_service_weapon")}
    fn classes_repair_weapon(&self,unit:UKey,index:usize)->bool  {panic!("unbound native service classes_repair_weapon")}
    fn repair_matches(&self,target:UKey,current:UKey)->bool  {panic!("unbound native service repair_matches")}
    fn vehicles_notify(&mut self) -> List<(usize, EntRef)> {panic!("unbound native service vehicles_notify")}
    fn vehicles_request_exit(&mut self,_core:&mut ctl::Core,controller:usize) {panic!("unbound native service vehicles_request_exit")}
    fn vehicles_boarded(&mut self, controller: usize) -> bool {panic!("unbound native service vehicles_boarded")}
    fn vehicles_boarded_vehicle(&mut self, controller: usize) -> Option<UKey> {panic!("unbound native service vehicles_boarded_vehicle")}
    fn vehicles_boarded_vehicle_of(&mut self, unit: UKey) -> Option<UKey> {panic!("unbound native service vehicles_boarded_vehicle_of")}
    fn vehicles_boarded_vehicle_of_character(&mut self, character: usize) -> Option<UKey> {panic!("unbound native service vehicles_boarded_vehicle_of_character")}
    fn vehicles_current_controllable(&mut self, character: usize) -> Option<UKey> {panic!("unbound native service vehicles_current_controllable")}
    fn vehicles_is_mounted_turret(&mut self, unit: UKey) -> bool {panic!("unbound native service vehicles_is_mounted_turret")}
    fn vehicles_empty_seats(&mut self, vehicle: UKey) -> f64 {panic!("unbound native service vehicles_empty_seats")}
    fn vehicles_crew_controllers(&mut self, vehicle: UKey, except: usize) -> List<usize> {panic!("unbound native service vehicles_crew_controllers")}
    fn vehicles_seats_full(&mut self, vehicle: UKey) -> bool {panic!("unbound native service vehicles_seats_full")}
    fn vehicles_pending_reservations(&mut self, vehicle: UKey, team: f64) -> bool {panic!("unbound native service vehicles_pending_reservations")}
    fn vehicles_low_tickets(&mut self, team: f64) -> bool {panic!("unbound native service vehicles_low_tickets")}
    fn vehicles_pilot_keeps_seat(&mut self, controller: usize) -> bool {panic!("unbound native service vehicles_pilot_keeps_seat")}
    fn vehicles_alerts_blocked(&mut self, controller: usize) -> bool {panic!("unbound native service vehicles_alerts_blocked")}
    fn vehicles_mount_reach(&mut self, unit: UKey, point: Vec3) -> bool {panic!("unbound native service vehicles_mount_reach")}
    fn vehicles_refuses_hold(&mut self, controller: usize) -> bool {panic!("unbound native service vehicles_refuses_hold")}
    fn vehicles_can_enter(&mut self, controller: usize, candidate: UKey) -> Option<bool> {panic!("unbound native service vehicles_can_enter")}
    fn vehicles_free_seats(&mut self, candidate: UKey, team: f64, want: f64) -> f64 {panic!("unbound native service vehicles_free_seats")}
    fn vehicles_reserve(&mut self, candidate: UKey, unit: UKey, want: f64) {panic!("unbound native service vehicles_reserve")}
    fn vehicles_reserve_seat(&mut self, controller: usize, vehicle: UKey) -> bool {panic!("unbound native service vehicles_reserve_seat")}
    fn vehicles_target_reserved(&mut self, target: UKey, team: f64) -> bool {panic!("unbound native service vehicles_target_reserved")}
    fn vehicles_reserve_target(&mut self, target: UKey, unit: UKey) {panic!("unbound native service vehicles_reserve_target")}
    fn vehicles_is_at_at(&mut self, unit: UKey) -> bool {panic!("unbound native service vehicles_is_at_at")}
    fn vehicles_support_point(&mut self, unit: UKey, post: usize) -> Option<Vec3> {panic!("unbound native service vehicles_support_point")}
    fn vehicles_board_policy(&mut self, controller: usize, candidate: UKey) -> Option<bool> {panic!("unbound native service vehicles_board_policy")}
    fn vehicles_board_tick(&mut self,core:&mut ctl::Core, controller: usize, dt: f64) {panic!("unbound native service vehicles_board_tick")}
    fn vehicles_patrol_mounted(&mut self,core:&mut ctl::Core, controller: usize, patrol: ctl::patrol::Patrol, shared: &mut ForeignShared, objectives: &[Option<usize>], support_holds: Option<f64>) -> Option<MountedPatrol> {panic!("unbound native service vehicles_patrol_mounted")}
    fn vehicles_policy_of(&mut self, unit: UKey) -> Policy {panic!("unbound native service vehicles_policy_of")}
    fn vehicles_policy_home_facing(&mut self, policy: Policy, unit: UKey) -> Option<bool> {panic!("unbound native service vehicles_policy_home_facing")}
    fn vehicles_policy_agent(&mut self, policy: Policy, state: f64) -> bool {panic!("unbound native service vehicles_policy_agent")}
    fn foreign_agent_create(&mut self,core:&mut ctl::Core, controller: usize, kind: ForeignKind, shared: &ForeignShared) -> (ForeignTick, Option<ControlObs>) {panic!("unbound native service foreign_agent_create")}
    fn foreign_agent_tick(&mut self,core:&mut ctl::Core, controller: usize, dt: f64, phase: f64, shared: &ForeignShared) -> ForeignTick {panic!("unbound native service foreign_agent_tick")}
    fn foreign_agent_exit(&mut self,core:&mut ctl::Core, controller: usize, shared: &ForeignShared) -> (Option<ForeignShared>, Vec<CoreEffect>) {panic!("unbound native service foreign_agent_exit")}
    fn helper_create(&mut self, controller: usize) -> (HelperKind, HelperObs, Option<ControlObs>) {let actor=self.actor(controller);let obs=self.nav.renew_helper(&self.store,actor).unwrap();(HelperKind::Ground,obs,self.nav.observation(&self.store,actor))}
    fn helper_goto(&mut self, controller: usize, point: Vec3, mode: GotoMode) -> HelperObs {let actor=self.actor(controller);let mode=match mode {GotoMode::Route=>nav::goto::GotoMode::Route,GotoMode::Direct=>nav::goto::GotoMode::Direct,GotoMode::Straight=>nav::goto::GotoMode::Straight};self.nav.command(&mut self.store,&mut self.random,&mut self.env,actor,Command::Goto(point,mode)).unwrap()}
    fn helper_stop(&mut self, controller: usize) -> HelperObs {let actor=self.actor(controller);self.nav.command(&mut self.store,&mut self.random,&mut self.env,actor,Command::Stop).unwrap()}
    fn helper_terminate(&mut self, controller: usize) -> HelperObs {let actor=self.actor(controller);self.nav.command(&mut self.store,&mut self.random,&mut self.env,actor,Command::Stop).unwrap()}
    fn helper_tick(&mut self, controller: usize, dt: f64) -> HelperObs {let actor=self.actor(controller);self.nav.command(&mut self.store,&mut self.random,&mut self.env,actor,Command::Tick(dt)).unwrap()}
    fn helper_destination(&mut self, controller: usize) -> Vec3 {self.nav.destination(&self.store,self.actor(controller)).unwrap()}
    fn helper_install_path(&mut self, controller: usize, points: &[Vec3]) -> HelperObs {let actor=self.actor(controller);self.nav.command(&mut self.store,&mut self.random,&mut self.env,actor,Command::Install(points)).unwrap()}
    fn helper_follow(&mut self, controller: usize, target: EntRef) -> HelperObs {panic!("unbound native service helper_follow")}
    fn helper_follow_trail(&mut self, controller: usize, on: bool) -> HelperObs {let actor=self.actor(controller);self.nav.command(&mut self.store,&mut self.random,&mut self.env,actor,Command::Trail(on)).unwrap()}
    fn helper_hold_path(&mut self, controller: usize, on: bool) -> HelperObs {let actor=self.actor(controller);self.nav.command(&mut self.store,&mut self.random,&mut self.env,actor,Command::Hold(on)).unwrap()}
    fn helper_set_leader(&mut self, controller: usize, leader: Option<EntRef>) -> HelperObs {let actor=self.actor(controller);self.nav.command(&mut self.store,&mut self.random,&mut self.env,actor,Command::Leader(None)).unwrap()}
    fn helper_dispose(&mut self, controller: usize) {}
    fn nav_control_ensure(&mut self, controller: usize) -> ControlObs {self.nav.observation(&self.store,self.actor(controller)).unwrap()}
    fn nav_control_reset(&mut self, controller: usize) {let actor=self.actor(controller);assert!(self.nav.reset(actor));}
    fn nav_control_obs(&mut self, controller: usize) -> ControlObs {self.nav.observation(&self.store,self.actor(controller)).unwrap()}
    fn nav_planner_idle(&mut self) -> bool {self.nav.nav.planner_idle()}
    fn nav_service(&mut self, budget: f64) {self.nav.nav.service(budget);}
    fn nav_point_clear(&mut self, point: Vec3, radius: f64, mask: f64) -> bool {self.nav.nav.barriers.point_clear(point,radius,v8math::to_uint32(mask)).0}
    fn nav_clip_segment(&mut self, from: Vec3, to: Vec3, radius: f64, mask: f64) -> (bool, Vec3) {let mut end=to;let ok=self.nav.nav.barriers.obstruct(from,&mut end,radius,v8math::to_uint32(mask),None,None);(ok,end)}
    fn nav_in_danger_region(&mut self, point: Vec3, pad: f64) -> bool {false}
    fn nav_query_hints(&mut self, mask: f64, center: Vec3, half_extent: f64, cap: f64, requester: Option<UKey>) -> List<usize> {self.buffers.indices.take()}
    fn nav_hints_len(&self) -> usize {0}
    fn nav_hint(&self, hint: usize) -> HintSnap {panic!("unbound native service nav_hint")}
    fn nav_hint_available(&mut self, hint: usize) -> bool {false}
    fn nav_mark_hint(&mut self, hint: usize, unit: Option<UKey>, prefer_world: bool) {panic!("unbound native service nav_mark_hint")}
    fn nav_hint_set_reserved(&mut self, hint: usize, at: f64) {panic!("unbound native service nav_hint_set_reserved")}
    fn perception_flags(&mut self, controller: usize) -> (bool, bool) {panic!("unbound native service perception_flags")}
    fn combat_writes(&mut self, controller: usize) -> CombatWrites {panic!("unbound native service combat_writes")}
    fn network_secondary_active(&self) -> bool {false}
    fn nav_navigation_input(&mut self, _core:&mut bfe_sim::ai::control::Core, controller: usize, step: f64, aim: Option<Vec3>,facing:bool) -> (Option<HelperObs>, Vec<CoreEffect>) {let actor=self.actor(controller);self.nav.input(&mut self.store,&mut self.random,&mut self.env,actor,step,aim,facing).unwrap();(Some(self.nav.helper_observation(&self.store,self.actor(controller)).unwrap()),Vec::new())}
    fn nav_analogs(&mut self, _core:&mut bfe_sim::ai::control::Core, controller: usize, aim: Option<Vec3>, step: f64,_facing:bool) -> ([f64; 4], Vec<CoreEffect>) {panic!("unbound native service nav_analogs")}
}

impl owned::runtime::World for Native {
 fn aim_controller(&mut self,_:usize,_:f64) {panic!("combat is unbound")}
 fn decay(&mut self,_:&mut ctl::Core,_:&mut bfe_sim::ai::phase::RandomState,_:f64) {panic!("vehicles are unbound")}
}
impl owned::lifecycle::Bindings for Native {
 fn character_index(&self,handle:&F4Handle)->Option<usize> {self.roster.index(&self.store,&self.commands,self.commands.resolve(&self.store,handle)?)}
 fn post_index(&self,post:SpawnPost)->Option<usize> {
  let present=match post {SpawnPost::Post(r)=>self.store.post.contains(r),SpawnPost::MobilePost(r)=>self.store.mobile_post.contains(r)};
  present.then(||self.population.index(bfe_sim::battle::posts::post_ref(Some(post)))).flatten()
 }
}
impl Native {
 fn actor(&self,controller:usize)->Actor {
  let key=self.controllers.key(controller).unwrap();
  let owned::entities::Entity::Controller(reference)=self.entities.get(key).unwrap() else{unreachable!()};
  self.actors[reference.slot as usize]
 }
 fn scene(&self)->owned::scene::Scene<'_> {
  owned::scene::Scene {w:&self.store,store:&self.store,seats:&self.seats,buildings:&self.buildings.buildings,traps:Some(&self.buildings.traps),characters:&self.commands,roster:&self.roster,entities:&self.entities,
   units:&self.units,combat_units:&self.combat_units,controls:&self.seat_controls,mounts:&self.mounts,turrets:&self.turrets,props:&self.props}
 }
 fn new(graph:bool,side:f64)->Self {
  let mut store=crate::ai::owned::tests::empty_world();let mut entities=owned::entities::Entities::new(16);
  let mut commands=Commands::new(2);let mut roster=owned::roster::Roster::new(2);let mut units=owned::units::Units::new();
  let mut strings=bfe_sim::store::Strings::default();
  let class_row=AiUnitClass {def:DefId(0),name:strings.intern_str("trooper"),size_type:AiClassScalar::Str(strings.intern_str("SOLDIER")),max_speed:AiClassScalar::F64(7.),forward_speed:AiClassScalar::F64(7.),..Default::default()};
  let class_data=owned::content::UnitClass::read(&class_row,&strings);
  let mut nav=nav::Nav::empty(nav::PlannerOptions::default());
  if graph {
   use nav::graph::{NavGraph,GraphRecord,HubRecord,ArcRecord};
   let hubs=[0.,20.].into_iter().map(|x|HubRecord {name:None,position:[x,0.,0.],radius:5.,arcs:vec![0.,255.,255.,255.,255.,255.,255.,255.]}).collect();
   let arcs=vec![ArcRecord {name:None,hubs:[0.,1.],mask:63.}];let mut words=vec![0;20];
   for from in 0..2 {for table in 0..5 {words[(from*5+table)*2+(1-from)]=31<<3;}}
   nav.graph=Some(NavGraph::new(&GraphRecord {info:[2.,1.],hubs,arcs,tables:Some(5.)},words).unwrap());
  }
  let mut nav=Navigation::new(nav,8);let mut combat_units=owned::combat::Units::new();let class=class_data.install(&mut units,&mut combat_units,&mut nav);
  let mut actors=Vec::with_capacity(2);let mut keys=Vec::with_capacity(2);
  for i in 0..2 {
   let c=store.character.insert(Character {team:side,..Default::default()});
   let u=store.unit.insert(Unit {alive:true,generation:1.,team:side,character:Some(c),soldier:UnitBody::Soldier(Soldier {max_speed:7.,max_strafe:7.,max_turn:2.,max_pitch:1.,position:[0.,0.,i as f64*2.],..Default::default()}),..Default::default()});
   store.character.get_mut(c).unwrap().unit=Some(u);let handle=commands.adopt(&store,c);roster.add(handle);
   entities.register(owned::entities::Entity::Character(c));keys.push(entities.register(owned::entities::Entity::Unit(u)));units.character(c,i);
   actors.push(nav.register(&store,AiLifeHandle {kind:u32::from(KIND_UNIT),slot:u.slot,generation:u.gen,life:1.},class).unwrap());
  }
  let mut controllers=owned::controllers::Controllers::new(2,4);for i in 0..2 {controllers.adopt(i,&mut entities);}
  let mut population=owned::population::Population::new(2,2,false);let mut posts=owned::posts::Posts::new(2);
  for i in 0..2 {
   let p=store.post.insert(Post {active:true,team:if i==0 {side}else{3.-side},position:[i as f64*20.,0.,0.],radius:Some(4.),ai:Some(bfe_sim::gen::PostAi {spawn_path:Some(SpawnPath {present:true,points:vec![]}),..Default::default()}),..Default::default()});
   posts.register(&store,&mut population,AiPostPolicy {post:SpawnPost::Post(p),attack:[100.;6],defend:[100.;6],bans:0.,spawn:true,ally:true,class_spawn_points:1.},owned::posts::Definition {name:Some(format!("post{i}").into()),spawn_path:true,spawn_points:1.,ally_path:true,..Default::default()}).unwrap();
  }
  Self {store,commands,entities,units,roster,population,posts,buffers:owned::buffers::Buffers::new(8,2,2,0,8),nav,actors,keys,cameras:vec![],random:bfe_sim::ai::phase::RandomState::new(81),env:Env {now:0.},vo:0,controllers,combat_units,seat_controls:owned::controls::HullControls::with_capacity(8),mounts:owned::mounts::Mounts::new(8,3),turrets:owned::turrets::Turrets::new(8,8),
   props:owned::buildings::Buildings::new(0),seats:Default::default(),buildings:bfe_sim::battle::live_phase::Live::new(0,0,0),crew:None}
 }
}
#[test]
fn canonical_core_runs_navigation_and_objectives_on_maps_with_and_without_a_graph() {
 for graph in [false,true] {for side in [1.,2.] {for enhanced in [false,true] {
  let mut world=Native::new(graph,side);
  let options=ctl::Options {bind:Bind {enhanced,combat:false,combat_agent:false,vehicles:false,classes:false,vehicle_adapter:false,mine_ground:false,..Default::default()},..Default::default()};
  let difficulty=if enhanced {ctl::difficulty::Difficulty::enhanced(3.,Some(side),false)}else{ctl::difficulty::Difficulty::classic(Default::default())};
  let mut core=ctl::Core::new(options,TeamTable::default(),difficulty,2);core.prepare(2,8,16);
  let mut runtime=owned::runtime::Runtime::new(core,world.random.share());
  for i in 0..2 {let character=world.roster.reference(&world.store,&world.commands,i).unwrap();let handle=world.roster.handles()[i].clone();runtime.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::RequestObjective,character_handle:Some(handle),..Default::default()}).unwrap();runtime.attach(&mut world,i).unwrap();}
  for &key in &world.keys {assert_eq!(world.scene().combat(key).unwrap().controller,world.controllers.key(world.unit(key).character.unwrap()));}
  for tick in 1..=180 {
   world.env.now=tick as f64/60.;runtime.phase(&mut world,1./60.);
  }
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  for tick in 181..=360 {world.env.now=tick as f64/60.;runtime.phase(&mut world,1./60.);}
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  assert_eq!(world.random.draw_counts(),runtime.random.draw_counts());
  assert_eq!(runtime.core.live.len(),2);assert!(runtime.core.scheduler.stats.considered>0.);
  assert!(runtime.core.controllers.iter().any(|c|c.agent.is_some()));
  assert!(world.roster.handles().iter().any(|h|world.commands.resolve(&world.store,h).and_then(|r|world.store.character.get(r)).is_some_and(|c|matches!(c.objective,PostRef::Post(_)))));
  assert!(world.keys.iter().any(|&key|world.unit(key).intent.forward!=0.));
  for &key in &world.keys {assert_eq!(world.scene().vehicle(key).unwrap().class_name.as_deref(),Some("trooper"));assert!(world.scene().combat(key).unwrap().alive);}
  let SpawnPost::Post(home)=world.population.post(0).unwrap() else{unreachable!()};
  world.store.post.get_mut(home).unwrap().team=0.;owned::lifecycle::Callback {core:&mut runtime.core,random:&mut runtime.random}.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::Post,post:Some(PostEvent {post:home,kind:PostChange::Neutralized,from:Some(side),..Default::default()}),..Default::default()}).unwrap();
  assert_eq!(world.character(0).objective,Some(0));assert_eq!(world.character(1).objective,Some(0));
  runtime.core.objectives_assign(&mut world,0,Some(1));
  let SpawnPost::Post(enemy)=world.population.post(1).unwrap() else{unreachable!()};
  world.store.post.get_mut(enemy).unwrap().active=false;runtime.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::Post,post:Some(PostEvent {post:enemy,kind:PostChange::Lost,..Default::default()}),..Default::default()}).unwrap();
  assert_eq!(world.character(0).objective,Some(0));
  let owned::entities::Entity::Unit(body)=world.entities.get(world.keys[0]).unwrap() else{unreachable!()};
  let character=world.roster.reference(&world.store,&world.commands,0).unwrap();let character_handle=world.roster.handles()[0].clone();
  world.store.unit.get_mut(body).unwrap().alive=false;runtime.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::Died,character_handle:Some(character_handle.clone()),..Default::default()}).unwrap();
  assert!(world.store.unit.get(body).unwrap().controller.is_none());assert_eq!(runtime.core.live.len(),1);
  let character=world.roster.reference(&world.store,&world.commands,0).unwrap();let character_handle=world.roster.handles()[0].clone();
  assert!(world.store.character.get(character).unwrap().controller.is_some());
  {let unit=world.store.unit.get_mut(body).unwrap();unit.alive=true;unit.generation=2.;}
  assert!(world.nav.refresh(&world.store,world.actors[0],AiLifeHandle {kind:u32::from(KIND_UNIT),slot:body.slot,generation:body.gen,life:2.},0));
  runtime.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::Spawned,character_handle:Some(character_handle.clone()),..Default::default()}).unwrap();assert_eq!(runtime.core.live.len(),2);
  assert!(world.store.unit.get(body).unwrap().controller.is_some());assert_eq!(runtime.core.controllers[0].generation,2.);
  runtime.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::Detach,character_handle:Some(character_handle.clone()),..Default::default()}).unwrap();
  assert!(world.store.character.get(character).unwrap().controller.is_none());
  assert!(world.store.unit.get(body).unwrap().controller.is_none());
  let retired=runtime.core.controllers.len();
  runtime.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::Attach,character_handle:Some(character_handle.clone()),..Default::default()}).unwrap();
  assert_eq!(runtime.core.by_character[0],Some(retired));
  assert!(world.controllers.key(0).is_none());assert!(world.controllers.key(retired).is_some());
  assert!(world.store.unit.get(body).unwrap().controller.is_some());
  runtime.phase(&mut world,1./60.);

 }}}
}

#[test]
fn canonical_lifecycle_hooks_republish_refresh_curves_and_reject_retired_handles_in_place() {
 for enhanced in [false,true] {
  let mut world=Native::new(false,1.);
  let mobile=world.store.mobile_post.insert(MobilePost {active:true,team:2.,mobile:true,ai:MobileAi {atk:vec![100.;6],def:vec![100.;6],..Default::default()},..Default::default()});
  world.posts.register(&world.store,&mut world.population,AiPostPolicy {post:SpawnPost::MobilePost(mobile),attack:[100.;6],defend:[100.;6],..Default::default()},Default::default()).unwrap();
  let options=ctl::Options {bind:Bind {enhanced,combat:false,combat_agent:false,vehicles:false,classes:false,vehicle_adapter:false,mine_ground:false,..Default::default()},..Default::default()};
  let difficulty=if enhanced {ctl::difficulty::Difficulty::enhanced(3.,Some(1.),false)}else{ctl::difficulty::Difficulty::classic(ctl::difficulty::DifficultyOptions {player_team:Some(1.),..Default::default()})};
  let mut core=ctl::Core::new(options,TeamTable::default(),difficulty,2);core.prepare(3,8,16);
  let mut runtime=owned::runtime::Runtime::new(core,world.random.share());
  let character=world.roster.reference(&world.store,&world.commands,0).unwrap();let character_handle=world.roster.handles()[0].clone();
  for i in 0..2 {runtime.attach(&mut world,i).unwrap();}
  let request=F4AIHook {op:F4AIHookOp::RequestObjective,character_handle:Some(character_handle.clone()),..Default::default()};
  runtime.battle_hook(&mut world,&request).unwrap();
  let post=world.character(0).objective.unwrap();
  let republish=F4AIHook {op:F4AIHookOp::Republish,character_handle:Some(character_handle.clone()),..Default::default()};
  let retarget=F4AIHook {op:F4AIHookOp::Retarget,number:2.,..Default::default()};
  let before=runtime.random.draw_counts();
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  for _ in 0..120 {
   runtime.core.controllers[0].commands.queue.clear();
   runtime.battle_hook(&mut world,&republish).unwrap();
   assert_eq!(runtime.core.controllers[0].commands.queue.len(),1,"unchanged objective still replaces the controller command");
   assert_eq!(world.character(0).objective,Some(post));
   runtime.battle_hook(&mut world,&retarget).unwrap();
   assert_eq!(runtime.core.difficulty.player_team,Some(2.));
   assert_eq!(runtime.core.controllers[0].difficulty,runtime.core.curves_for(1.));
  }
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  assert_eq!(runtime.random.draw_counts(),before);
  runtime.core.objectives_assign(&mut world,0,Some(2));
  world.store.mobile_post.get_mut(mobile).unwrap().team=0.;
  runtime.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::Mobile,mobile:Some(PsMobileEvent {kind:PsMobileEventKind::Owner,post:mobile,from:2.,to:Some(0.),..Default::default()}),..Default::default()}).unwrap();
  world.store.mobile_post.get_mut(mobile).unwrap().active=false;
  runtime.battle_hook(&mut world,&F4AIHook {op:F4AIHookOp::Mobile,mobile:Some(PsMobileEvent {kind:PsMobileEventKind::Lost,post:mobile,from:0.,..Default::default()}),..Default::default()}).unwrap();
  assert_ne!(world.character(0).objective,Some(2));
  let before=runtime.random.draw_counts();
  let invalid=F4AIHook {op:F4AIHookOp::Post,post:Some(PostEvent {post:Ref::new(99,0),..Default::default()}),..Default::default()};
  assert_eq!(runtime.battle_hook(&mut world,&invalid),Err(owned::lifecycle::Stale::Post));
  for change in 0..4 {
   let mut wrong=character_handle.clone();
   match change {0=>wrong.kind=F4ObjectKind::Building,1=>wrong.slot=99,2=>wrong.generation+=1,_=>wrong.life+=1.}
   let invalid=F4AIHook {op:F4AIHookOp::RequestObjective,character_handle:Some(wrong),..Default::default()};
   assert_eq!(runtime.battle_hook(&mut world,&invalid),Err(owned::lifecycle::Stale::Character));
   assert_eq!(runtime.random.draw_counts(),before);
  }
  world.store.character.remove(character).unwrap();
  let replacement=world.store.character.insert(Character::default());assert_eq!(replacement.slot,character.slot);
  assert_eq!(runtime.battle_hook(&mut world,&request),Err(owned::lifecycle::Stale::Character));
  assert_eq!(runtime.random.draw_counts(),before);
 }
}

#[test]
fn canonical_core_uses_the_vehicle_navigation_mask_for_driver_objective_bans() {
 for enhanced in [false,true] {for side in [1.,2.] {
  let mut world=Native::new(false,side);
  let mut population=owned::population::Population::new(2,2,enhanced);
  let SpawnPost::Post(neutral)=world.population.post(0).unwrap() else{unreachable!()};
  world.store.post.get_mut(neutral).unwrap().team=0.;
  for index in 0..2 {
   let post=world.population.post(index).unwrap();
   population.add(&world.store,AiPostPolicy {post,attack:[100.;6],defend:[100.;6],bans:if index==1 {32.}else{0.},spawn:true,ally:true,class_spawn_points:1.}).unwrap();
  }
  world.population=population;
  world.crew=Some(CrewSnap {vehicle:world.keys[0],is_driver:true,driver:Some(0),occupants:Vec::new()});
  let options=ctl::Options {bind:Bind {enhanced,combat:false,combat_agent:false,vehicles:false,classes:false,vehicle_adapter:false,mine_ground:false,..Default::default()},..Default::default()};
  let difficulty=if enhanced {ctl::difficulty::Difficulty::enhanced(3.,Some(side),false)}else{ctl::difficulty::Difficulty::classic(Default::default())};
  let mut core=ctl::Core::new(options,TeamTable::default(),difficulty,2);core.prepare(2,8,16);
  let mut random=world.random.share();let controller=core.attach(&mut world,&mut random,0).unwrap();
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  for _ in 0..120 {
   assert_eq!(core.request_objective(&mut world,&mut random,controller,false),Some(0));
  }
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
 }}
}

#[path="common/ai_world.rs"]
mod canonical_world;

// BarrierSearch routes around named records repeatedly. This includes first
// use and path identity churn, not just an obstacle-free warmed movement loop.
#[test]
fn named_barrier_routes_reuse_native_search_and_path_storage() {
    use nav::barriers::{BarrierRecord,BarrierSet,barrier_from_record,Work};
    let record=BarrierRecord {name:Some("authored_route_barrier".into()),
        xfrm:[1.,0.,0.,0.,1.,0.,0.,0.,-1.,0.,0.,0.],size:[2.,0.,2.],flags:63.};
    let mut raw=nav::Nav::empty(nav::PlannerOptions::default());
    raw.barriers=BarrierSet::new(vec![barrier_from_record(&record,0).unwrap()]);
    let mut owner=Navigation::new(raw,1);
    let s=&mut owner.nav;let mut valid=true;
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|on|on.set(true));
    for _ in 0..4096 {
        valid &= s.search.begin(&mut s.barriers,&s.store,Vec3::new(-10.,0.,0.),Vec3::new(10.,0.,0.),0.5,63);
        let mut work=Work {count:0.,cap:100000.};
        s.search.step(&mut s.barriers,&mut work);
        valid &= s.search.finished&&s.search.success&&work.count==7.;
        let Some(path)=s.search.reconstruct(&mut s.store) else {valid=false;break};
        let nodes=&s.store.paths[path].nodes;
        valid &= nodes.len()==4 && nodes[1].x == -3. && nodes[1].z == 3. && nodes[2].x == 3. && nodes[2].z == 3.;
        s.store.release(path);
    }
    COUNTING.with(|on|on.set(false));
    assert!(valid,"the same detour and first-hit work order survive reuse");
    assert_eq!(ALLOCATIONS.with(Cell::get),0,"named routes allocate neither barrier text nor search storage");
}

// New acquisitions receive idle controls while retired generations keep their
// own payload until publication. Exhaustion is a failed acquisition, not growth.
#[test]
fn lease_channel_storage_is_recycled_after_publication() {
    let mut pool=bfe_sim::seats::lease::InputPool::default();pool.reserve(2);
    let first=pool.acquire().unwrap();let mut second=pool.acquire().unwrap();
    assert!(pool.acquire().is_none());
    second.held[0]=true;second.pressed[1]=true;second.boost=Some(true);
    assert!(!first.held[0]);pool.recycle(first);pool.recycle(second);
    ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|on|on.set(true));
    let mut valid=true;
    for _ in 0..4096 {
        let Some(mut input)=pool.acquire() else {valid=false;break};
        valid &= input.held==[false,false]&&input.pressed==[false,false]&&input.boost.is_none()&&input.aim==[0.,0.,1.];
        input.held[0]=true;input.pressed[1]=true;input.boost=Some(true);pool.recycle(input);
    }
    COUNTING.with(|on|on.set(false));assert!(valid);
    assert_eq!(ALLOCATIONS.with(Cell::get),0);
}

// Reverse-index presence is independent of the qualified source handle:
// slot zero and source generation zero are valid, as is the largest generation.
#[test]
fn identities_keep_zero_sources_and_full_source_generations() {
    use owned::entities::{Entities, Entity};
    let mut entities = Entities::new(1);
    let zero = Entity::Unit(Ref::new(0, 0));
    let first = entities.register(zero);
    assert_eq!(first as u32, 0);
    assert_eq!(entities.find(zero), Some(first));
    let replacement = Entity::Unit(Ref::new(0, u32::MAX));
    let next = entities.register(replacement);
    assert_eq!(entities.get(first), None);
    assert_eq!(entities.find(zero), None);
    assert_eq!(entities.find(replacement), Some(next));
    assert!(!entities.retire(first));
    assert_eq!(entities.find(replacement), Some(next));
    assert!(entities.retire(next));
    assert_eq!(entities.find(replacement), None);
    let other = Entity::Hull(Ref::new(0, u32::MAX));
    let hull = entities.register(other);
    assert_eq!(entities.find(other), Some(hull));
    assert_eq!(entities.find(replacement), None);
}
