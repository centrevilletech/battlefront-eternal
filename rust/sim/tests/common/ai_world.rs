//! The concrete canonical World drives Core, navigation and population.
//! Disabled cross-owner services fail immediately if a capability leaks.
use super::*;
use owned::{arsenals::Mounted,frame::{Hooks,Settings},rows::Rows,weapons::Changes,world::Facts};
use bfe_sim::ai::{phase::RandomState,combat::{binding::CombatAi,world::{Obj,Sphere,QueryFilter,GotoMode as CombatGotoMode}}};
use three::Vector3;
struct Services {now:f64,vo:usize,vehicles:owned::vehicles::Binding}
impl Environment for Services {
 fn time(&self)->f64 {self.now}
 fn remembered(&mut self,_:Actor)->Option<Vec3> {None}
 fn clear_above(&mut self,_:&mut bfe_sim::gen::Store,_:&mut bfe_sim::vehicles::f2_seats::Seats,_:Vec3,_:&AiLifeHandle)->bool {true}
 fn follow_far(&mut self)->f64 {3.}
 fn seat_frame(&mut self,_:&mut bfe_sim::gen::Store,_:&mut bfe_sim::vehicles::f2_seats::Seats,_:bfe_sim::vehicles::f2_seats::HullLife,_:usize)->Option<(Vec3,Vec3)> {panic!("no mounts in this fixture")}
}
#[allow(unused_variables)]
impl<C:Changes+?Sized,M:Mounted> Hooks<C,M> for Services {
fn vehicle_binding(&self)->owned::vehicles::Binding {self.vehicles.clone()}
fn fire_primary(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,controller:Obj) {panic!("unprepared native primary fire")}
fn flight_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent:Obj,points:&[[f64;3]],over:bool,altitude:bool) {panic!("unprepared native flight path")}

fn mount_origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,origin:bfe_sim::ai::vehicles::world::Origin)->Option<Vector3> {panic!("no mounts in this fixture")}
fn origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 {let owned::entities::Entity::Unit(reference)=rows.state.entities.get(u).unwrap() else{panic!("body geometry")};rows.store.unit.get(reference).unwrap().origin_on_foot()}
fn forward(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 {let owned::entities::Entity::Unit(reference)=rows.state.entities.get(u).unwrap() else{panic!("body geometry")};rows.store.unit.get(reference).unwrap().forward_on_foot()}
fn aim_point(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 {let owned::entities::Entity::Unit(reference)=rows.state.entities.get(u).unwrap() else{panic!("body geometry")};rows.store.unit.get(reference).unwrap().position()}
fn sphere(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Sphere {let owned::entities::Entity::Unit(reference)=rows.state.entities.get(u).unwrap() else{panic!("body geometry")};Sphere {center:rows.store.unit.get(reference).unwrap().position(),radius:0.}}
fn use_classic_random(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj) {panic!("disabled canonical fixture service use_classic_random")}
fn query(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,center: &Vector3,radius: f64,filter: QueryFilter)-> List<Obj> {
 assert!(matches!(filter,QueryFilter::Except {except:None}));
 let mut out=rows.state.buffers.units.take();
 for &member in &rows.store.units.list {
  let Member::Unit(reference)=member else{continue};
  let key=rows.state.entities.find(owned::entities::Entity::Unit(reference)).unwrap();
  let unit=rows.scene().policy(key).unwrap();
  if unit.alive&&Vector3::new(unit.position.x,unit.position.y,unit.position.z).distance_to_squared(center)<=radius*radius {out.push_reserved(key);}
 }
 out
}
fn relation(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,a: f64,b: f64)-> f64 {if a==b&&a!=0. {1.}else if a!=0.&&b!=0. {-1.}else{0.}}
fn living(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState)-> List<Obj> {panic!("disabled canonical fixture service living")}
fn los(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,from: &Vector3,to: &Vector3,a: Option<Obj>,b: Option<Obj>)-> bool {panic!("disabled canonical fixture service los")}
fn set_stance(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,c:Obj,stance:f64) {panic!("disabled canonical fixture service set_stance")}
fn human_target(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,hull:Obj)->Option<Obj> {panic!("unprepared native human reticle target")} 
fn interrupts_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,combat:&mut bfe_sim::ai::combat::binding::CombatAi)-> bool {panic!("disabled canonical fixture service interrupts_tick")}
fn has_helper(&self,rows:&Rows<'_,C,M>,agent: Obj)-> bool {panic!("disabled canonical fixture service has_helper")}
fn helper_arrived(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj)-> bool {panic!("disabled canonical fixture service helper_arrived")}
fn helper_flying(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj)-> bool {panic!("disabled canonical fixture service helper_flying")}
fn helper_has_follow(&self,rows:&Rows<'_,C,M>,agent: Obj)-> bool {panic!("disabled canonical fixture service helper_has_follow")}
fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,point: &Vector3,mode: Option<CombatGotoMode>) {panic!("disabled canonical fixture service helper_goto")}
fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj) {panic!("disabled canonical fixture service helper_stop")}
fn helper_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,dt: f64) {panic!("disabled canonical fixture service helper_tick")}
fn helper_follow(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,target: Obj) {panic!("disabled canonical fixture service helper_follow")}
fn helper_terminate(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj) {panic!("disabled canonical fixture service helper_terminate")}
fn request_jet(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,controller: Obj,vector: &Vector3) {panic!("disabled canonical fixture service request_jet")}
fn emit_vo(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,unit: Obj,event: &str,property: &str) {assert_eq!(event,"flee");self.vo+=1;}
}
#[allow(unused_variables)]
impl<C:Changes+?Sized,M:Mounted> owned::world::Services<C,M> for Services {
fn owned_item_count(&self,rows:&Rows<'_,C,M>,unit:UKey,weapon:&bfe_sim::gen::Weapon)->usize {panic!("no dispensers in this fixture")}
fn bind_actor(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,controller:usize,unit:UKey) {panic!("disabled canonical fixture service bind_actor")}

fn mine_ground_height(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,point: Vec3) -> f64 {panic!("disabled canonical fixture service mine_ground_height")}
fn emit_vo(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,unit: UKey,event: &str,property: &str,command: f64,context: Option<f64>,positional: Option<bool>,notice: Option<bool>) {self.vo+=1;}
fn nav_analogs(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,controller: usize,aim: Option<Vec3>,step: f64,_facing:bool) -> ([f64; 4], Vec<CoreEffect>) {panic!("disabled canonical fixture service nav_analogs")}
}

struct NoMount;
impl Mounted for NoMount {
 fn count(&self,_:&bfe_sim::gen::Store,_:owned::seats::Source,_:usize)->Option<usize> {panic!("no mounted arsenal")}
 fn read<R>(&self,_:&bfe_sim::gen::Store,_:owned::seats::Source,_:usize,_:usize,_:impl FnOnce(&Weapon)->R)->Option<R> {panic!("no mounted arsenal")}
 fn edit<R>(&mut self,_:&bfe_sim::gen::Store,_:owned::seats::Source,_:usize,_:usize,_:impl FnOnce(&mut bfe_sim::ai::combat::world::WeaponSettings)->R)->Option<R> {panic!("no mounted arsenal")}
}
#[test]
fn concrete_world_runs_core_over_canonical_rows_without_tick_allocations() {
 for graph in [false,true] {for side in [1.,2.] {for enhanced in [false,true] {
  let source=Native::new(graph,side);
  let Native {mut store,mut commands,entities,units,roster,population,posts,nav,controllers,combat_units,seat_controls,mounts,turrets,props,mut seats,buildings,..}=source;
  let mut state=owned::state::State::new(nav::Nav::empty(Default::default()),enhanced,owned::state::Capacity {registry:8,characters:2,posts:2,hulls:0,buildings:0,controls:0,scopes:8,cameras:0});
  state.entities=entities;state.units=units;state.roster=roster;state.population=population;state.posts=posts;
  state.navigation=nav;state.controllers=controllers;state.combat_units=combat_units;
  state.controls=seat_controls;state.mounts=mounts;state.turrets=turrets;state.buildings=props;
  let mut strings=bfe_sim::store::Strings::default();
  let class=AiUnitClass {def:DefId(0),name:strings.intern_str("trooper"),size_type:AiClassScalar::Str(strings.intern_str("SOLDIER")),max_speed:AiClassScalar::F64(7.),forward_speed:AiClassScalar::F64(7.),..Default::default()};
  state.unit_class(&class,&strings);state.prepare_hints(|_|None);
  for index in 0..2 {let character=state.roster.reference(&store,&commands,index).unwrap();store.units.list.push(Member::Unit(store.character.get(character).unwrap().unit.unwrap()));}
  let difficulty=if enhanced {ctl::difficulty::Difficulty::enhanced(3.,Some(side),false)}else{ctl::difficulty::Difficulty::classic(Default::default())};
  let options=ctl::Options {bind:Bind {enhanced,combat:false,combat_agent:false,vehicles:false,classes:false,vehicle_adapter:false,mine_ground:false,..Default::default()},..Default::default()};
  let mut core=ctl::Core::new(options,TeamTable::default(),difficulty.clone(),2);core.prepare(2,8,16);
  let mut runtime=owned::runtime::Runtime::new(core,RandomState::new(81));let mut random=runtime.random.share();
  let settings=Settings {difficulty:std::cell::RefCell::new(difficulty),mission_stay_in_turrets:false,mission_no_jet_obstacle_test:false,ai_stay_in_turrets:false,vehicles:false,dense:false,navigation:true};
  let mut facts=Facts {time:0.,paused:false,network_views:false,lod_disabled:false,network_secondary:false};
  let mut combat=CombatAi::default();let mut services=Services {now:0.,vo:0,vehicles:owned::vehicles::Binding {authority:owned::vehicles::Authority::new(bfe_sim::ai::vehicles::VehicleAI::new()),settings:Default::default(),navigation:true,population:true}};
  let defs=bfe_sim::combat::defs::WeaponDefs::default();let mut changes=bfe_sim::units::ownership::Ownership::default();let mut mounted=NoMount;
  // The publication owner adopts the same canonical bodies before AI can
  // mark a changed controller link. Its generation stamps stay authoritative.
  for &member in &store.units.list {if let Member::Unit(reference)=member {changes.adopt(&store,reference).unwrap();}}
  changes.clear_changed();
  let mut rows=Rows {store:&mut store,seats:&mut seats,buildings:&buildings.buildings,characters:&mut commands,state:&mut state,changes:&mut changes,mounted:&mut mounted,definitions:&defs};
  for index in 0..2 {
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   runtime.attach(&mut world,index).unwrap();runtime.request_objective(&mut world,index);
  }
  for tick in 1..=360 {
   if tick==181 {ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));}
   facts.time=tick as f64/60.;services.now=facts.time;
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   runtime.phase(&mut world,1./60.);
   assert_eq!(runtime.core.controllers[0].agent.is_some(),world.rows.state.agents.key(0).is_some());
   // Native service views use the same owners while these bodies are on foot.
   // No mounted callback or random draw is needed for these observations.
   let body=runtime.core.controllers[0].unit.unwrap();
   let life=runtime.core.controllers[0].generation;
   assert!(world.unit_live(body,life));assert!(!world.unit_live(body,life+1.));
   assert!(!world.unit_live(u64::MAX,life));
   assert!(!world.vehicles_boarded(0));assert_eq!(world.vehicles_boarded_vehicle(0),None);
   assert_eq!(world.vehicles_boarded_vehicle_of(body),None);
   assert_eq!(world.vehicles_boarded_vehicle_of_character(0),None);
   assert_eq!(world.vehicles_current_controllable(0),None);
   assert!(!world.vehicles_is_mounted_turret(body));assert_eq!(world.vehicles_empty_seats(body),0.);
   assert!(world.vehicles_crew_controllers(body,0).is_empty());assert!(world.vehicles_notify().is_empty());
   let all=world.units_query(Vec3::default(),f64::INFINITY,&mut |_|true);
   let repair=world.service_candidates(Vec3::default(),f64::INFINITY);
   assert_eq!(all.len(),2);assert_eq!(&all[..],&repair[..]);
   assert_eq!(world.vehicles_policy_of(body),ctl::world::Policy::None);
   assert!(!world.vehicles_policy_agent(ctl::world::Policy::Mobile,7.));assert!(world.vehicles_policy_agent(ctl::world::Policy::Fly,7.));

  }
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  assert_eq!(runtime.core.live.len(),2);assert!(runtime.random.draw_counts()[0]>0);
  // A running combat callback retains its helper while an interrupt
  // disposes that agent and constructs its successor on the same controller.
  let old=rows.state.agents.key(0).unwrap();
  assert!(rows.state.agents.enter(&rows.state.entities,old));
  let mut old_core=runtime.core.controllers[0].agent.take().unwrap();
  {
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   world.agent_disposed(0,old_core.helper);
   let (kind,obs,control)=world.helper_create(0);
   old_core.helper=ctl::agents::Helper {kind:Some(kind),obs,direct:Default::default()};
   if let Some(control)=control {runtime.core.controllers[0].request=control.request;runtime.core.controllers[0].nav_control=Some(control);}
   world.agent_created(0,old_core.helper);
  }
  runtime.core.controllers[0].agent=Some(old_core);
  let next=rows.state.agents.key(0).unwrap();assert_ne!(old,next);
  let actor=rows.state.actor(0).unwrap();let old_helper=rows.state.agents.get(&rows.state.entities,old).unwrap().navigation.unwrap();
  let next_helper=rows.state.navigation.helper_key(actor).unwrap();assert_ne!(old_helper,next_helper);
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  {
   let mut active=owned::active::Active {core:&mut runtime.core,services:&mut services,facts:&facts,settings:&settings};
   Hooks::helper_goto(&mut active,&mut rows,&mut random,old,&Vector3::new(24.,0.,0.),Some(CombatGotoMode::Direct));
   Hooks::helper_stop(&mut active,&mut rows,&mut random,old);
   assert_eq!(rows.state.navigation.destination_for(rows.store,old_helper),Some(Vec3::new(24.,0.,0.)));
   assert_eq!(rows.state.navigation.destination_for(rows.store,next_helper),Some(Vec3::default()));
   assert!(Hooks::helper_arrived(&mut active,&mut rows,&mut random,old));
   assert_eq!(rows.state.agents.key(0),Some(next));
  }
  rows.state.agents.leave(&mut rows.state.entities,old);rows.state.reap_helpers();
  assert!(rows.state.navigation.helper_observation_for(rows.store,old_helper).is_none());
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  // Mounted callbacks update the active Core immediately. A support
  // hold returns the same patrol and native helper without a service bag.
  let saved_kind=runtime.core.controllers[0].agent.as_ref().unwrap().kind.clone();
  let mut objective=ctl::agents::ObjectiveAgent::new(ctl::agents::ObjectiveFlavor::Trooper,enhanced,None,false);
  objective.support=true;runtime.core.controllers[0].agent.as_mut().unwrap().kind=ctl::agents::AgentKind::Objective(objective);
  let draws=runtime.random.draw_counts();
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  for _ in 0..120 {
   let objective=runtime.core.objective_mut(0).unwrap();objective.support_holds=None;
   let patrol=objective.patrol.clone();let mut shared=ForeignShared::read(&runtime.core.controllers[0]);
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   let result=world.vehicles_patrol_mounted(&mut runtime.core,0,patrol,&mut shared,&[],None).unwrap();
   assert_eq!(result.support_holds,Some(1.));assert_eq!(result.patrol.fsm.pending,6.);
   assert!(result.patrol.expires>=facts.time+20.&&result.patrol.expires<=facts.time+30.);
   assert!(result.publications.is_empty());
   owned::runtime::World::decay(&mut world,&mut runtime.core,&mut runtime.random,1./60.);
  }
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  assert_eq!(runtime.random.draw_counts()[0]-draws[0],120);
  runtime.core.controllers[0].agent.as_mut().unwrap().kind=saved_kind;
  let character=rows.state.roster.reference(rows.store,rows.characters,0).unwrap();
  let body=rows.store.character.get(character).unwrap().unit.unwrap();
  let old_actor=rows.state.actor(0).unwrap();
  rows.store.unit.get_mut(body).unwrap().generation=2.;
  assert!(rows.state.navigation.observation(rows.store,old_actor).is_none());
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  {
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   runtime.phase(&mut world,1./60.);
  }
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  assert_eq!(runtime.core.controllers[0].generation,2.);
  assert_eq!(rows.state.actor(0),Some(old_actor));
  let draws=runtime.random.draw_counts();
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  {
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   runtime.present_human_team(&mut world,3.-side);
  }
  let mut combat_frame=owned::frame::Frame {rows:&mut rows,hooks:&mut services,random:&mut random,settings:&settings,time:facts.time,response:None};
  let observed=bfe_sim::ai::combat::world::World::curves_for(&mut combat_frame,side).unwrap();
  let expected=runtime.core.curves_for(side).unwrap();
  assert_eq!(observed.a,expected.a);assert_eq!(observed.d,expected.d);
  assert_eq!(observed.b,Some(expected.b));assert_eq!(observed.s,Some(expected.s));
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  assert_eq!(runtime.random.draw_counts(),draws);
  let controller=rows.state.controllers.key(0).unwrap();
  let agent=rows.state.agents.key(0).unwrap_or_else(||rows.state.agents.begin(&mut rows.state.entities,0,controller));
  {
   let mut active=owned::active::Active {core:&mut runtime.core,services:&mut services,facts:&facts,settings:&settings};
   ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
   let _=Hooks::interrupts_tick(&mut active,&mut rows,&mut random,agent,&mut combat);
   COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  }
  assert!(rows.state.agents.retire(&mut rows.state.entities,agent));
  // The actual native foreign driver constructs after the helper, ranks
  // perception and exits through Core's helper operations in this same scope.
  let saved_goal=runtime.core.controllers[0].commands.current;
  runtime.core.controllers[0].commands.current=ctl::controller::SourceGoal::combat(0.,Some(Vec3::new(20.,0.,20.)),None);
  {
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   world.combat_create_perception(0);
  }
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  for _ in 0..120 {
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   let shared=ForeignShared::read(&runtime.core.controllers[0]);
   world.combat_feed(0,&CombatInput::read(&runtime.core.controllers[0],None));
   let (created,_)=world.foreign_agent_create(&mut runtime.core,0,ForeignKind::Combat,&shared);
   runtime.core.controllers[0].agent=Some(ctl::agents::Agent {
    kind:ctl::agents::AgentKind::Foreign(ForeignKind::Combat),helper:created.helper,interrupts:created.interrupts,
    fsm:ctl::fsm::Fsm {state:created.state,pending:created.pending},unit:runtime.core.controllers[0].unit,goal:Some(runtime.core.controllers[0].commands.current),
   });
   let tick=world.foreign_agent_tick(&mut runtime.core,0,1./60.,1.,&shared);
   assert!(!tick.replaced);assert_ne!(tick.state,100.);
   world.foreign_agent_exit(&mut runtime.core,0,&shared);world.agent_disposed(0,runtime.core.controllers[0].agent.as_ref().unwrap().helper);
  }
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  // Vehicle subclasses use their real native driver. Missing boarding
  // candidates finish on the rank pass; unboarding requests exit first,
  // then observes the already unmounted body on its behavior pass.
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  for _ in 0..120 {for kind in [ForeignKind::Board,ForeignKind::Unboard] {
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   let shared=ForeignShared::read(&runtime.core.controllers[0]);
   let (created,_)=world.foreign_agent_create(&mut runtime.core,0,kind,&shared);
   runtime.core.controllers[0].agent=Some(ctl::agents::Agent {kind:ctl::agents::AgentKind::Foreign(kind),helper:created.helper,interrupts:created.interrupts,fsm:ctl::fsm::Fsm {state:created.state,pending:created.pending},unit:runtime.core.controllers[0].unit,goal:Some(runtime.core.controllers[0].commands.current)});
   let tick=world.foreign_agent_tick(&mut runtime.core,0,1./60.,1.,&shared);
   assert!(!tick.replaced);
   if kind==ForeignKind::Board {assert_eq!(tick.state,100.);}else{
    assert_eq!(tick.state,1.);assert!(runtime.core.controllers[0].exit_request);
    let tick=world.foreign_agent_tick(&mut runtime.core,0,1./60.,0.,&shared);
    assert_eq!(tick.pending,100.);
   }
   world.foreign_agent_exit(&mut runtime.core,0,&shared);
   world.agent_disposed(0,runtime.core.controllers[0].agent.as_ref().unwrap().helper);
  }}
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  runtime.core.controllers[0].commands.current=saved_goal;
  for index in 0..2 {let character=rows.state.roster.reference(rows.store,rows.characters,index).unwrap();assert!(!matches!(rows.store.character.get(character).unwrap().objective,PostRef::Null|PostRef::Undefined));}
  // A post-battle warning survives the next control feed. Its recipient
  // list comes from canonical registry rows and consumes only one A draw.
  let unit=runtime.core.controllers[0].unit.unwrap();
  let p=rows.scene().policy(unit).unwrap().position;
  let grenade=rows.store.grenade.insert(Grenade {position:[p.x,p.y,p.z],..Default::default()});
  rows.store.ordnance.grenades.push(grenade);rows.state.dangers.reserve(grenade.slot as usize+1);
  rows.state.dangers.contact(grenade,0.);
  let before=runtime.random.draw_counts()[0];
  {
   let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
   assert_eq!(world.grenade_dangers(&mut runtime.core,||None),1);
   assert!(runtime.core.controllers[0].danger_pending);
   assert_eq!(runtime.core.controllers[0].danger_point,p);
   world.combat_feed(0,&CombatInput::read(&runtime.core.controllers[0],None));
   let alerts=world.rows.combat_alerts();
   let warned=alerts.iter().find(|a|a.controller==0).unwrap();
   assert!(warned.danger_pending);assert_eq!(warned.danger_point,p);
  }
  assert_eq!(runtime.random.draw_counts()[0]-before,1);
 }}}
}

mod vehicle_effects {
use super::Services;
use bfe_sim::{ai::{owned::{vehicle_frame::Hooks,rows::Rows,weapons::Changes,arsenals::Mounted},vehicles::world::*,combat::binding::CombatAi,phase::RandomState},vehicles::{BodyId,Hit}};
use three::Vector3;
#[allow(unused_variables)]
impl<C:Changes+?Sized,M:Mounted> Hooks<C,M> for Services {
fn support_holds(&self,rows:&Rows<'_,C,M>, agent: Obj) -> Option<f64> {panic!("unprepared native vehicle frame effect support_holds") }
fn cast(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {panic!("unprepared native vehicle frame effect cast") }
fn mount_origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, origin: Origin) -> Option<Vector3> {panic!("unprepared native vehicle frame effect mount_origin") }
fn eligible_seat(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> f64 {panic!("unprepared native vehicle frame effect eligible_seat") }
fn board(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> bool {panic!("unprepared native vehicle frame effect board") }
fn man(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, turret: Obj) -> bool {panic!("unprepared native vehicle frame effect man") }
fn request_exit_vehicle(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unprepared native vehicle frame effect request_exit_vehicle") }
fn request_exit_turret(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unprepared native vehicle frame effect request_exit_turret") }
fn unboard(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unprepared native vehicle frame effect unboard") }
fn damage(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, hull: Obj, amount: f64) {panic!("unprepared native vehicle frame effect damage") }
fn pilot_input(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick {panic!("unprepared native vehicle frame effect pilot_input") }
fn nav_attach(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>, unit: Obj) {panic!("unprepared native vehicle frame effect nav_attach") }
fn nav_update(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>, dt: f64) {panic!("unprepared native vehicle frame effect nav_update") }
fn nav_request(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>) -> Option<Request> {panic!("unprepared native vehicle frame effect nav_request") }
fn request_objective(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, exclude: bool) {panic!("unprepared native vehicle frame effect request_objective") }
fn controller_request_exit(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) {panic!("unprepared native vehicle frame effect controller_request_exit") }
fn set_reboard_at(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, t: f64) {panic!("unprepared native vehicle frame effect set_reboard_at") }
fn fire_a(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) {panic!("unprepared native vehicle frame effect fire_a") }
fn invalidate_request(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) {panic!("unprepared native vehicle frame effect invalidate_request") }
fn add_vehicle_note(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, note: Ref) {panic!("unprepared native vehicle frame effect add_vehicle_note") }
fn set_objective(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, character: Obj, post: Obj) {panic!("unprepared native vehicle frame effect set_objective") }
fn set_support_holds(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, holds: f64) {panic!("unprepared native vehicle frame effect set_support_holds") }
fn set_patrol_expires(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patrol: Obj, t: f64) {panic!("unprepared native vehicle frame effect set_patrol_expires") }
fn patrol_request_state(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patrol: Obj, state: f64) {panic!("unprepared native vehicle frame effect patrol_request_state") }
fn face(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patterns: Obj, posture: f64) {panic!("unprepared native vehicle frame effect face") }
fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, point: &Vector3, direct: bool) {panic!("unprepared native vehicle frame effect helper_goto") }
fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj) {panic!("unprepared native vehicle frame effect helper_stop") }
fn helper_follow_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool) {panic!("unprepared native vehicle frame effect helper_follow_path") }
fn helper_land(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, on: bool) {panic!("unprepared native vehicle frame effect helper_land") }
}

}

#[test]
fn instance_owner_runs_native_phases_and_keeps_only_process_caches_between_battles() {
 use owned::owner::{Owner,Owners};
 use bfe_sim::ai::{combat::weapons_ai::Registered,vehicles::service::VehicleStorage};
 use bfe_sim::combat::defs::{Js,JsObj,WeaponClass};
 use std::rc::Rc;
 let mut owner=Owner::default();let mut strings=bfe_sim::store::Strings::default();
 let class=AiUnitClass {def:DefId(0),name:strings.intern_str("trooper"),max_speed:AiClassScalar::F64(7.),forward_speed:AiClassScalar::F64(7.),..Default::default()};
 let weapon=WeaponClass::new(Rc::new(JsObj {fields:vec![("class".into(),Js::Str("test-cannon".into()))],def:None}));
 let caps=AiCapabilities {navigation:true,population:true,classes:true,random_b:true,..Default::default()};
 let capacity=AiCapacity {registry:8,characters:4,posts:2,scopes:8,repair_jobs:16,support_keys:4,..Default::default()};
 for enhanced in [false,true] {
  let source=Native::new(false,1.);
  let Native {mut store,mut commands,entities,units,roster,population,posts,nav,controllers,combat_units,seat_controls,mounts,turrets,props,mut seats,buildings,..}=source;
  let input=AiColdSettings {seed:81,enhanced,tier:3.,profile_tier:3.,player_skill:4.,enemy_skill:14.,sensor_range:75.,mission_stay_in_turrets:true,hard:Some(true),capabilities:caps.clone(),capacity:capacity.clone(),..Default::default()};
  let battle=owner.begin(nav::Nav::empty(Default::default()),&input,&mut strings,TeamTable::default());
  assert_eq!(battle.runtime.random.draw_counts(),[0,0,0]);assert_eq!(battle.runtime.core.options.bind.enhanced,enhanced);
  assert!(battle.settings.mission_stay_in_turrets);assert!(!battle.settings.ai_stay_in_turrets);
  assert_eq!(battle.vehicles.settings.cycle,3.);
  if !enhanced {
   battle.weapon_records(&[("test-cannon".into(),Registered {ai:Js::Obj(Rc::new(JsObj {fields:vec![("maxRange".into(),Js::Num(49.))],def:None})),..Default::default()})]);
   battle.vehicles.authority.write(|v|{v.run.a=Some(71.);v.run.b=Some(112.);});
  }else{
   assert_eq!(battle.vehicles.authority.read(|v|(v.run.a,v.run.b)),(Some(71.),Some(112.)));
   assert!(battle.combat.shared.clusters.borrow().states.is_empty());
   assert_eq!(battle.counters.phases,0);
  }
  battle.weapon_class(DefId(0),&weapon);
  assert_eq!(battle.combat.shared.weapons.weapon_ai(&weapon.levels[0].obj).max_range,49.);
  battle.state.entities=entities;battle.state.units=units;battle.state.roster=roster;
  battle.state.population=population;battle.state.posts=posts;battle.state.navigation=nav;
  battle.state.controllers=controllers;battle.state.combat_units=combat_units;battle.state.controls=seat_controls;
  battle.state.mounts=mounts;battle.state.turrets=turrets;battle.state.buildings=props;
  battle.state.unit_class(&class,&strings);battle.state.prepare_hints(|_|None);
  for index in 0..2 {let character=battle.state.roster.reference(&store,&commands,index).unwrap();store.units.list.push(Member::Unit(store.character.get(character).unwrap().unit.unwrap()));}
  let mut changes=bfe_sim::units::ownership::Ownership::default();
  for &member in &store.units.list {if let Member::Unit(reference)=member {changes.adopt(&store,reference).unwrap();}}
  let defs=bfe_sim::combat::defs::WeaponDefs::default();let mut mounted=NoMount;
  battle.activate();assert!(battle.ready());assert_eq!(battle.runtime.core.character_count,2);assert_eq!(battle.runtime.random.draw_counts(),[0,0,0]);
  let mut services=Services {now:0.,vo:0,vehicles:battle.vehicles.clone()};
  let mut facts=Facts {time:0.,paused:false,network_views:false,lod_disabled:false,network_secondary:false};
  battle.with_world(Owners {store:&mut store,seats:&mut seats,buildings:&buildings.buildings,characters:&mut commands,changes:&mut changes,mounted:&mut mounted,definitions:&defs},&mut services,&facts,|runtime,world|{
   for index in 0..2 {runtime.attach(world,index).unwrap();runtime.request_objective(world,index);}
  });
  for tick in 1..=360 {
   facts.time=tick as f64/60.;services.now=facts.time;
   if tick==181 {ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));}
   battle.phase(Owners {store:&mut store,seats:&mut seats,buildings:&buildings.buildings,characters:&mut commands,changes:&mut changes,mounted:&mut mounted,definitions:&defs},&mut services,&facts,
    &AiPhaseFacts {dt:1./60.,tick,..Default::default()},&[]);
  }
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
  assert_eq!(battle.counters.phases,360);assert_eq!(battle.runtime.core.live.len(),2);
  assert!(battle.counters.draws_a>0);assert_eq!(battle.counters.draws_c,0);
  drop(services);owner.finish();assert!(owner.battle().is_none());
 }
}

#[test]
fn immediate_seat_actions_resolve_physical_lives_and_reject_replaced_occupancy() {
 use owned::physical::{SeatAction,seat_command};
 use bfe_sim::gen::{F2SeatOp,SeatLease,Vehicle};
 let Native {mut store,mut commands,entities,units,roster,population,posts,nav,controllers,combat_units,seat_controls,mounts,turrets,props,mut seats,buildings,..}=Native::new(false,1.);
 let mut state=owned::state::State::new(nav::Nav::empty(Default::default()),false,owned::state::Capacity {registry:8,characters:2,posts:2,hulls:1,buildings:0,controls:2,scopes:2,cameras:0});
 state.entities=entities;state.units=units;state.roster=roster;state.population=population;state.posts=posts;state.navigation=nav;state.controllers=controllers;state.combat_units=combat_units;state.controls=seat_controls;state.mounts=mounts;state.turrets=turrets;state.buildings=props;
 let character=state.roster.reference(&store,&commands,0).unwrap();let unit=store.character.get(character).unwrap().unit.unwrap();
 let key=state.entities.find(owned::entities::Entity::Unit(unit)).unwrap();
 let hull=store.vehicle.insert(Vehicle {target_generation:3.,alive:true,..Default::default()});
 let hull_key=state.entities.register(owned::entities::Entity::Hull(hull));
 let life=store.unit.get(unit).unwrap().generation;
 let lease=store.seat_lease.insert(SeatLease {unit:Some(unit),generation:life,vehicle:Some(hull),..Default::default()});
 store.unit.get_mut(unit).unwrap().seat=Some(lease);
 let defs=bfe_sim::combat::defs::WeaponDefs::default();let mut changes=bfe_sim::units::ownership::Ownership::default();let mut mounted=NoMount;
 let rows=Rows {store:&mut store,seats:&mut seats,buildings:&buildings.buildings,characters:&mut commands,state:&mut state,changes:&mut changes,mounted:&mut mounted,definitions:&defs};
 let options=bfe_sim::ai::vehicles::world::ExitOptions {placement:Some(false),promote:Some(false),post:Some(None)};
 ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
 for _ in 0..120 {
  let board=seat_command(&rows,SeatAction::Board {unit:key,hull:hull_key}).unwrap();
  assert_eq!(board.unit,Some(unit));assert_eq!(board.unit_life,life);assert_eq!(board.hull,Some(hull));assert_eq!(board.hull_life,3.);assert_eq!(board.index,-1.);
  let exit=seat_command(&rows,SeatAction::Unboard {unit:Some(key),options}).unwrap();
  assert_eq!(exit.op,F2SeatOp::Unboard);assert_eq!(exit.lease,Some(lease));assert!(!exit.placement);assert_eq!(exit.promote,Some(false));
  rows.store.seat_lease.get_mut(lease).unwrap().generation=life+1.;
  assert!(seat_command(&rows,SeatAction::Unboard {unit:Some(key),options}).is_none());
  rows.store.seat_lease.get_mut(lease).unwrap().generation=life;
 }
 COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
 rows.store.vehicle.remove(hull).unwrap();
 let next=rows.store.vehicle.insert(Vehicle {target_generation:3.,..Default::default()});assert_eq!(next.slot,hull.slot);
 assert!(seat_command(&rows,SeatAction::Board {unit:key,hull:hull_key}).is_none());
}

#[test]
fn mounted_scheduler_creates_stationary_control_at_input_without_rig_queries() {
 use owned::{entities::Entity,state::{State,Capacity}};
 use bfe_sim::{gen::*,ai::{nav::Nav,control::{controller::Controller,difficulty::Difficulty}},battle::{commands::Commands,live_phase::{Live,Building},props},seats::{hulls::HullSide,turrets::TurretSide},store::{Ref,Strings},vehicles::f2_seats::Seats};
 for turret in [false,true] {
  let mut store=Store::default();let mut seats=Seats::default();let mut strings=Strings::default();
  let mut buildings=Live::new(1,0,0);let mut characters=Commands::new(0);
  let mut state=State::new(Nav::empty(Default::default()),false,Capacity {registry:8,characters:1,posts:0,hulls:1,buildings:1,controls:4,scopes:4,cameras:0});
  let class=state.control_class(&AiUnitClass {name:strings.intern_str("gunner"),forward_speed:AiClassScalar::F64(0.),..Default::default()},&strings,7.);
  let body=store.unit.insert(Unit {alive:true,generation:2.,..Default::default()});
  let lease=store.seat_lease.insert(SeatLease {unit:Some(body),generation:2.,..Default::default()});
  store.unit.get_mut(body).unwrap().seat=Some(lease);
  if turret {
   let reference=Ref::new(0,1);store.seat_lease.get_mut(lease).unwrap().turret=Some(reference);
   seats.add_turret(TurretSide {turret:reference,alive:true,lease:Some(lease),gunner:Some(body.slot),..Default::default()});
   let settings=props::Settings::from(PbSettings {models:vec![None;3],max_health:100.,..Default::default()});
   let owner=buildings.adopt(3.,Building {state:props::Station {building:props::Building {state:props::State::Built,health:100.,add_health:0.,last_attacker:props::Identity::Null,target_generation:Some(3.)},active_mode:false,active:0.,pulse:0.,served_player:false,served_health:false,served_ammo:false},settings,supply:None,attacker:Attacker::Null});
   state.turrets.register(&seats,&buildings.buildings,&mut state.entities,reference,owner,None,None);
   state.turrets.target(reference,owned::turrets::Target {member:Member::TurretTarget(Ref::new(0,1)),team:1.,name:"turret".into(),policy:std::rc::Rc::new(Default::default()),combat:Default::default()}).unwrap();
  }else{
   let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:3.,seats:vec![Default::default(),Default::default()],..Default::default()});
   store.seat_lease.get_mut(lease).unwrap().vehicle=Some(hull);store.seat_lease.get_mut(lease).unwrap().index=1.;
   store.vehicle.get_mut(hull).unwrap().seats[1].lease=Some(lease);
   seats.add_hull(&store,HullSide {vehicle:hull,..Default::default()});state.prepare_hull_controls(hull,vec![class,class]);
  }
  let control=state.construct_control(&mut store,&mut seats,&buildings.buildings,lease).unwrap();
  let object=state.entities.find(Entity::Control(control)).unwrap();
  let controller=state.controllers.adopt(0,&mut state.entities);
  {let row=state.controllers.write(&state.entities,controller).unwrap();row.unit=Some(object);row.generation=1.;}
  let difficulty=Difficulty::classic(Default::default());
  let settings=Settings {difficulty:std::cell::RefCell::new(difficulty.clone()),mission_stay_in_turrets:false,mission_no_jet_obstacle_test:false,ai_stay_in_turrets:false,vehicles:true,dense:false,navigation:true};
  let mut core=ctl::Core::new(ctl::Options {lod_disabled:true,bind:Bind {nav:true,vehicles:true,combat:false,combat_agent:false,population:false,classes:false,..Default::default()},..Default::default()},TeamTable::default(),difficulty,1);
  let mut value=Controller::new(0);value.unit=Some(object);value.generation=1.;core.controllers.push(value);core.live.push(0);
  let mut services=Services {now:0.,vo:0,vehicles:owned::vehicles::Binding {authority:owned::vehicles::Authority::new(bfe_sim::ai::vehicles::VehicleAI::new()),settings:Default::default(),navigation:true,population:false}};
  let mut changes=bfe_sim::units::ownership::Ownership::default();let mut mounted=NoMount;let defs=bfe_sim::combat::defs::WeaponDefs::default();
  let mut random=RandomState::new(17);let mut combat=CombatAi::default();let facts=Facts {time:0.,paused:false,network_views:false,lod_disabled:true,network_secondary:false};
  let mut rows=Rows {store:&mut store,seats:&mut seats,buildings:&buildings.buildings,characters:&mut characters,state:&mut state,changes:&mut changes,mounted:&mut mounted,definitions:&defs};
  assert!(rows.state.actor(0).is_none());
  let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
  core.execute(&mut world,1./60.);
  let actor=world.rows.state.actor(0).unwrap();assert!(core.controllers[0].nav_control.is_some());
  let serial=world.rows.state.navigation.control(actor).unwrap().serial;assert_eq!(serial,1.);
  ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
  for _ in 0..120 {core.execute(&mut world,1./60.);}
  COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);assert_eq!(world.random.draw_counts(),[0,0,0]);
  assert_eq!(world.rows.state.navigation.control(actor).unwrap().serial,serial);
  let output=if turret {&world.rows.seats.crews.controls}else{&world.rows.seats.hulls.controls};
  let intent=&output.get(&bfe_sim::seats::lease::control_key(control)).unwrap().intent;
  assert_eq!([intent.forward,intent.right,intent.turn,intent.pitch],[0.;4]);
 }
}

#[test]
fn canonical_combat_ingest_uses_sparse_journal_order_overflow_and_qualified_lives() {
 use bfe_sim::gen::*;
 let Native {mut store,mut commands,entities,units,roster,population,posts,nav,controllers,combat_units,seat_controls,mounts,turrets,props,mut seats,buildings,keys,..}=Native::new(false,1.);
 let mut state=owned::state::State::new(nav::Nav::empty(Default::default()),false,owned::state::Capacity {registry:8,characters:2,posts:2,hulls:0,buildings:0,controls:0,scopes:8,cameras:0});
 state.entities=entities;state.units=units;state.roster=roster;state.population=population;state.posts=posts;state.navigation=nav;state.controllers=controllers;state.combat_units=combat_units;state.controls=seat_controls;state.mounts=mounts;state.turrets=turrets;state.buildings=props;
 let units:[bfe_sim::store::Ref<Unit>;2]=std::array::from_fn(|index|match state.entities.get(keys[index]).unwrap() {owned::entities::Entity::Unit(unit)=>unit,_=>unreachable!()});
 for index in 0..2 {
  let controller=state.controllers.key(index).unwrap();let owned::entities::Entity::Controller(reference)=state.entities.get(controller).unwrap() else{unreachable!()};
  let character=store.unit.get(units[index]).unwrap().character.unwrap();
  store.unit.get_mut(units[index]).unwrap().controller=Some(reference);store.unit.get_mut(units[index]).unwrap().team=1.+index as f64;store.character.get_mut(character).unwrap().team=1.+index as f64;
  let row=state.controllers.write(&state.entities,controller).unwrap();row.unit=Some(keys[index]);row.generation=1.;row.character=state.entities.find(owned::entities::Entity::Character(character));
 }
 let mut input=AiJournalInput {sequence:1.,records:vec![AiJournalEvent {sequence:0.,kind:AiJournalKind::Shot,
  source:AiJournalRef {member:Some(Member::Unit(units[1])),generation:Some(1.),direct:false},
  target:AiJournalRef {member:Some(Member::Unit(units[0])),generation:Some(1.),direct:false},..Default::default()}]};
 let mut words=vec![0.;AiJournalInput::WORDS];let mut heap=Vec::new();input.put(&mut words,&mut heap);input=AiJournalInput::take(&words,&heap);
 let mut changes=bfe_sim::units::ownership::Ownership::default();let mut mounted=NoMount;let defs=bfe_sim::combat::defs::WeaponDefs::default();
 let difficulty=ctl::difficulty::Difficulty::classic(Default::default());let settings=Settings {difficulty:std::cell::RefCell::new(difficulty),mission_stay_in_turrets:false,mission_no_jet_obstacle_test:false,ai_stay_in_turrets:false,vehicles:false,dense:false,navigation:false};
 let facts=Facts {time:0.,paused:false,network_views:false,lod_disabled:true,network_secondary:false};let mut random=RandomState::new(17);let mut combat=CombatAi::default();
 let mut services=Services {now:0.,vo:0,vehicles:owned::vehicles::Binding {authority:owned::vehicles::Authority::new(bfe_sim::ai::vehicles::VehicleAI::new()),settings:Default::default(),navigation:false,population:false}};
 let mut rows=Rows {store:&mut store,seats:&mut seats,buildings:&buildings.buildings,characters:&mut commands,state:&mut state,changes:&mut changes,mounted:&mut mounted,definitions:&defs};
 let mut world=owned::world::World {rows:&mut rows,random:&mut random,combat:&mut combat,services:&mut services,settings:&settings,facts:&facts};
 let victim=world.rows.state.controllers.key(0).unwrap();
 world.rows.state.import_journal(&input).unwrap();
 {let alerts=world.combat_ingest();assert!(alerts[0].shot_pending);assert!(alerts[0].shot_known);}
 world.rows.state.controllers.write(&world.rows.state.entities,victim).unwrap().shot.pending=false;
 assert!(!world.combat_ingest()[0].shot_pending);
 // One AI-relevant record survives after unrelated events have overrun the
 // ring. Its original position is not compacted into a shorter AI history.
 input.sequence=4500.;input.records[0].sequence=4498.;
 ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
 world.rows.state.import_journal(&input).unwrap();assert!(world.combat_ingest()[0].shot_pending);
 COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
 assert_eq!(world.rows.state.reader.cursor,4500);assert_eq!(world.rows.state.reader.dropped,403);
 world.rows.state.controllers.write(&world.rows.state.entities,victim).unwrap().shot.pending=false;
 input.sequence=4501.;input.records[0].sequence=4500.;input.records[0].source.generation=Some(2.);
 world.rows.state.import_journal(&input).unwrap();assert!(!world.combat_ingest()[0].shot_pending);
 input.records[0].sequence=4502.;assert_eq!(world.rows.state.import_journal(&input),Err(owned::events::Invalid::Window));
 assert_eq!(world.rows.state.journal.sequence(),4501);
 // Reset retains the existing reader's cursor, matching SimEvents.reset.
 input.sequence=1.;input.records[0].sequence=0.;input.records[0].source.generation=Some(1.);
 world.rows.state.import_journal(&input).unwrap();assert!(!world.combat_ingest()[0].shot_pending);assert_eq!(world.rows.state.reader.cursor,4501);
}

#[test]
fn sparse_ai_voice_and_owner_notices_preserve_body_and_proxy_identity() {
 use owned::{publication::{Voices,ItemOwners},entities::Entity};
 use bfe_sim::{gen::*,store::Strings,seats::hulls::HullSide};
 let Native {mut store,mut commands,entities,units,roster,population,posts,nav,controllers,combat_units,seat_controls,mounts,turrets,props,mut seats,buildings,keys,..}=Native::new(false,1.);
 let mut state=owned::state::State::new(nav::Nav::empty(Default::default()),false,owned::state::Capacity {registry:8,characters:2,posts:2,hulls:1,buildings:0,controls:2,scopes:4,cameras:0});
 state.entities=entities;state.units=units;state.roster=roster;state.population=population;state.posts=posts;state.navigation=nav;state.controllers=controllers;state.combat_units=combat_units;state.controls=seat_controls;state.mounts=mounts;state.turrets=turrets;state.buildings=props;
 let Entity::Unit(body)=state.entities.get(keys[0]).unwrap() else{unreachable!()};
 let mut strings=Strings::default();let voices=Voices::new(&mut strings);let owners=ItemOwners::new(64);
 let class=state.control_class(&AiUnitClass {name:strings.intern_str("gunner"),forward_speed:AiClassScalar::F64(0.),..Default::default()},&strings,7.);
 let hull=store.vehicle.insert(Vehicle {alive:true,target_generation:3.,seats:vec![Default::default()],..Default::default()});
 seats.add_hull(&store,HullSide {vehicle:hull,..Default::default()});state.prepare_hull_controls(hull,vec![class]);
 let lease=store.seat_lease.insert(SeatLease {unit:Some(body),generation:1.,vehicle:Some(hull),team:1.,..Default::default()});
 store.unit.get_mut(body).unwrap().seat=Some(lease);store.vehicle.get_mut(hull).unwrap().seats[0].lease=Some(lease);
 let control=state.construct_control(&mut store,&mut seats,&buildings.buildings,lease).unwrap();let proxy=state.entities.find(Entity::Control(control)).unwrap();
 let mut changes=bfe_sim::units::ownership::Ownership::default();let mut mounted=NoMount;let defs=bfe_sim::combat::defs::WeaponDefs::default();
 // Binding a mounted proxy chooses its canonical retained intent. Clearing
 // that exact controller restores the physical rider as latchCrews' source.
 assert!(state.link_controller(&mut store,&mut changes,proxy,0,true));state.seat_driven(&mut seats,proxy);
 assert!(seats.hulls.controls.get(&bfe_sim::seats::lease::control_key(control)).unwrap().driven);
 let controller=state.controllers.key(0).unwrap();
 let mut perception=bfe_sim::ai::combat::perception::Perception::default();
 perception.fire.weapon_target=Some(bfe_sim::ai::combat::world::Ref {obj:keys[1],generation:1.});
 state.controllers.write(&state.entities,controller).unwrap().perception=Some(perception);
 let Entity::Unit(target)=state.entities.get(keys[1]).unwrap() else{unreachable!()};
 let before=state.weapon_hint(&store,body,Some(control),|_|panic!("no lazy target observation"));
 assert!(before.bot);assert_eq!(before.target,Some(Member::Unit(target)));
 store.unit.get_mut(target).unwrap().generation=2.;
 assert_eq!(state.weapon_hint(&store,body,Some(control),|_|panic!("no shapes")).target,None);
 assert_eq!(before.target,Some(Member::Unit(target)),"a previously frozen hull hint stays frozen");
 store.unit.get_mut(body).unwrap().network_human=Some(true);
 assert!(!state.weapon_hint(&store,body,Some(control),|_|panic!("no shapes")).bot);
 store.unit.get_mut(body).unwrap().network_human=None;
 assert!(state.link_controller(&mut store,&mut changes,proxy,0,false));state.seat_driven(&mut seats,proxy);
 assert!(!seats.hulls.controls.get(&bfe_sim::seats::lease::control_key(control)).unwrap().driven);
 let rows=Rows {store:&mut store,seats:&mut seats,buildings:&buildings.buildings,characters:&mut commands,state:&mut state,changes:&mut changes,mounted:&mut mounted,definitions:&defs};
 assert!(matches!(owners.observe(&rows,keys[0]).unwrap().actor,AiActor::Unit(reference) if reference==body));
 assert!(matches!(owners.observe(&rows,proxy).unwrap().actor,AiActor::SeatUnit(reference) if reference==control));
 let notice=voices.record(7.,&rows,proxy,"charge","ChargeSound",0.,Some(0.),Some(false),Some(false)).unwrap();
 assert_eq!(notice.generation,1.);assert!(!notice.human);assert_eq!(notice.command,Some(0.));assert_eq!(notice.context,Some(0.));assert_eq!(notice.positional,Some(false));assert_eq!(notice.notice,Some(false));
 let mut words=vec![0.;AiVoiceNotice::WORDS];let mut heap=Vec::new();notice.put(&mut words,&mut heap);assert_eq!(AiVoiceNotice::take(&words,&heap),notice);
 ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
 for _ in 0..120 {
  assert!(owners.observe(&rows,keys[0]).is_none());assert!(owners.observe(&rows,proxy).is_none());
  let record=voices.record(8.,&rows,proxy,"heardEnemy","HeardEnemySound",-1.,None,None,None).unwrap();assert_eq!(record.command,None);assert_eq!(record.positional,None);
  for context in 0..4 {for kind in 0..6 {for human in [false,true] {let _=bfe_sim::ai::control::squads::vo_entry(kind as f64,context as f64,human);}}}
 }
 COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
 // The same pooled body's next life keeps its lazy owner number. Storage
 // replacement and a fresh SeatUnit object each receive a new observation.
 rows.store.unit.get_mut(body).unwrap().generation=2.;assert!(owners.observe(&rows,keys[0]).is_none());
 assert!(rows.state.controls.retire(control));assert!(rows.state.births.retire(rows.seats,&mut rows.state.entities,control));
 rows.store.seat_lease.get_mut(lease).unwrap().generation=2.;
 let next=rows.state.construct_control(rows.store,rows.seats,rows.buildings.rows(),lease).unwrap();assert_eq!(next.slot,control.slot);assert_ne!(next,control);
 let proxy=rows.state.entities.find(Entity::Control(next)).unwrap();assert!(owners.observe(&rows,proxy).is_some());assert!(owners.observe(&rows,proxy).is_none());
}
