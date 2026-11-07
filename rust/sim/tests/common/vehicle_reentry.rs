//! Reentrant native vehicle notification must release its state before
//! querying the world. Nested notification advances the same counter in place.
use super::{ALLOCATIONS,COUNTING};
use std::cell::Cell;
use bfe_sim::ai::{owned::{scratch::{List,Pool},vehicles::Authority},vehicles::{VehicleAI,world::*,service::VehicleStorage,notify::VehicleNotify,random::Draws}};
use bfe_sim::vehicles::{BodyId,Hit};
use three::Vector3;
struct Native {state:Authority,settings:Settings,pool:Pool<Obj>,nested:bool,offer:bool,calls:Cell<usize>}
impl Draws for Native {fn next_a(&mut self)->u32 {panic!("notifier consumed RNG")}}
#[allow(unused_variables)]
impl World for Native {
fn now(&self) -> f64 {panic!("unexpected vehicle notification service now")}
fn settings(&self) -> &Settings {&self.settings}
fn unit(&self,u:Obj)->UnitView {
 assert!(self.offer);match u {
  1|4=>UnitView {obj:u,id:0.,alive:true,team:1.,generation:if u==1 {7.}else{8.},turret:Some(if u==1 {10}else{11}),..Default::default()},
  2|3=>UnitView {obj:u,alive:true,team:1.,soldier:true,controller:Some(20+u),..Default::default()},
  _=>panic!("unexpected notification identity"),
 }
}
fn hull(&self, h: Obj) -> HullView {panic!("unexpected vehicle notification service hull")}
fn turret(&self,t:Obj)->TurretView {assert!(self.offer);TurretView {obj:t,alive:true,entry:Some(if t==10 {1}else{4}),..Default::default()}}
fn lease(&self, l: Obj) -> LeaseView {panic!("unexpected vehicle notification service lease")}
fn controller(&self,c:Obj)->ControllerView {assert!(self.offer);ControllerView {obj:c,is_unit_controller:true,..Default::default()}}
fn post(&self, p: Obj) -> PostView {panic!("unexpected vehicle notification service post")}
fn character_unit(&self, character: Obj) -> Option<Obj> {panic!("unexpected vehicle notification service character_unit")}
fn character_controller(&self, character: Obj) -> Option<Obj> {panic!("unexpected vehicle notification service character_controller")}
fn object_buffer(&self)->List<Obj> {self.pool.take()}
fn hulls(&self) -> List<Obj> {if !self.nested&&!self.offer {
    self.calls.set(self.calls.get()+1);
    let mut state=self.state.clone();
    let mut nested=Native {state:self.state.clone(),settings:self.settings.clone(),pool:self.pool.clone(),nested:true,offer:false,calls:Cell::new(0)};
    assert_eq!(state.notify(&mut nested),0.);
   }self.pool.take()}
fn turrets(&self)->List<Obj> {let mut out=self.pool.take();if self.offer {out.push_reserved(10);out.push_reserved(11);}out}
fn tow_weapon(&self,control:Obj)->Option<(usize,f64)> {panic!("unexpected vehicle notification service tow_weapon")}
fn reinforcements(&self,team:f64)->Option<f64> {assert!(self.offer);Some(100.)}
fn human_units(&mut self) -> List<Obj> {panic!("unexpected vehicle notification service human_units")}
fn has_planner(&self) -> bool {panic!("unexpected vehicle notification service has_planner")}
fn has_graph(&self) -> bool {panic!("unexpected vehicle notification service has_graph")}
fn nav(&mut self) -> Option<&mut bfe_sim::ai::nav::Nav> {panic!("unexpected vehicle notification service nav")}
fn support_holds(&self, agent: Obj) -> Option<f64> {panic!("unexpected vehicle notification service support_holds")}
fn hint_link(&self, hint: Obj) -> Option<Obj> {panic!("unexpected vehicle notification service hint_link")}
fn edit_steer<R>(&mut self, control: Obj, f: impl FnOnce(&mut Option<Steer>) -> R) -> R {panic!("unexpected vehicle notification service edit_steer")}
fn read_decay(&mut self, hull: Obj) -> Option<bfe_sim::ai::vehicles::service::Decay> {panic!("unexpected vehicle notification service read_decay")}
fn write_decay(&mut self, hull: Obj, value: bfe_sim::ai::vehicles::service::Decay) {panic!("unexpected vehicle notification service write_decay")}
fn write_vehicle_intent(&mut self, control: Obj, value: SeatIntent) {panic!("unexpected vehicle notification service write_vehicle_intent")}
fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {panic!("unexpected vehicle notification service cast")}
fn query(&mut self,center:&Vector3,radius:f64,filter:QueryFilter)->List<Obj> {assert!(self.offer);let mut out=self.pool.take();out.push_reserved(3);out.push_reserved(2);out}
fn relation(&mut self,a:f64,b:f64)->f64 {assert!(self.offer);assert_eq!(a,b);1.}
fn los(&mut self, a: &Vector3, b: &Vector3) -> Option<bool> {panic!("unexpected vehicle notification service los")}
fn sphere(&mut self, x: Obj) -> Sphere {panic!("unexpected vehicle notification service sphere")}
fn origin(&mut self, control: Obj) -> Vector3 {panic!("unexpected vehicle notification service origin")}
fn forward(&mut self, control: Obj) -> Vector3 {panic!("unexpected vehicle notification service forward")}
fn mount_origin(&mut self, origin: Origin) -> Option<Vector3> {panic!("unexpected vehicle notification service mount_origin")}
fn hold_yaw(&mut self, turret: Obj, a: f64) -> f64 {panic!("unexpected vehicle notification service hold_yaw")}
fn eligible_seat(&mut self, unit: Obj, hull: Obj) -> f64 {panic!("unexpected vehicle notification service eligible_seat")}
fn first_free_seat(&mut self, hull: Obj) -> f64 {panic!("unexpected vehicle notification service first_free_seat")}
fn board(&mut self, unit: Obj, hull: Obj) -> bool {panic!("unexpected vehicle notification service board")}
fn man(&mut self, unit: Obj, turret: Obj) -> bool {panic!("unexpected vehicle notification service man")}
fn request_exit_vehicle(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unexpected vehicle notification service request_exit_vehicle")}
fn request_exit_turret(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unexpected vehicle notification service request_exit_turret")}
fn unboard(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unexpected vehicle notification service unboard")}
fn human_target(&mut self, hull: Obj) -> Option<Obj> {panic!("unexpected vehicle notification service human_target")}
fn damage(&mut self, hull: Obj, amount: f64) {panic!("unexpected vehicle notification service damage")}
fn pilot_input(&mut self, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick {panic!("unexpected vehicle notification service pilot_input")}
fn nav_attach(&mut self, controller: Option<Obj>, unit: Obj) {panic!("unexpected vehicle notification service nav_attach")}
fn nav_update(&mut self, controller: Option<Obj>, dt: f64) {panic!("unexpected vehicle notification service nav_update")}
fn nav_request(&mut self, controller: Option<Obj>) -> Option<Request> {panic!("unexpected vehicle notification service nav_request")}
fn reserve_hint(&mut self, hint: Obj, controller: Obj, now: f64) {panic!("unexpected vehicle notification service reserve_hint")}
fn set_hint_reserved_at(&mut self, hint: Obj, at: f64) {panic!("unexpected vehicle notification service set_hint_reserved_at")}
fn set_intent_analogs(&mut self, control: Obj, stick: &Stick) {panic!("unexpected vehicle notification service set_intent_analogs")}
fn request_objective(&mut self, controller: Obj, exclude: bool) {panic!("unexpected vehicle notification service request_objective")}
fn controller_request_exit(&mut self, controller: Obj) {panic!("unexpected vehicle notification service controller_request_exit")}
fn set_reboard_at(&mut self, controller: Obj, t: f64) {panic!("unexpected vehicle notification service set_reboard_at")}
fn fire_a(&mut self, controller: Obj) {panic!("unexpected vehicle notification service fire_a")}
fn invalidate_request(&mut self, controller: Obj) {panic!("unexpected vehicle notification service invalidate_request")}
fn add_vehicle_note(&mut self, controller: Obj, note: Ref) {panic!("unexpected vehicle notification service add_vehicle_note")}
fn set_objective(&mut self, character: Obj, post: Obj) {panic!("unexpected vehicle notification service set_objective")}
fn set_support_holds(&mut self, agent: Obj, holds: f64) {panic!("unexpected vehicle notification service set_support_holds")}
fn set_patrol_expires(&mut self, patrol: Obj, t: f64) {panic!("unexpected vehicle notification service set_patrol_expires")}
fn patrol_request_state(&mut self, patrol: Obj, state: f64) {panic!("unexpected vehicle notification service patrol_request_state")}
fn cp_center(&mut self, post: Obj) -> Vector3 {panic!("unexpected vehicle notification service cp_center")}
fn capture_region(&mut self, post: Obj) -> Option<(f64, f64)> {panic!("unexpected vehicle notification service capture_region")}
fn helper_traversal(&mut self, agent: Obj, radius: bool) -> f64 {panic!("unexpected vehicle notification service helper_traversal")}
fn memory_point(&mut self, agent: Obj) -> Option<Vector3> {panic!("unexpected vehicle notification service memory_point")}
fn select_weapon(&mut self, controller: Obj, control: Obj, target: Obj, switch_allowed: bool) -> Choice {panic!("unexpected vehicle notification service select_weapon")}
fn switch_weapon(&mut self, controller: Obj, control: Obj, index: usize) -> bool {panic!("unexpected vehicle notification service switch_weapon")}
fn face(&mut self, patterns: Obj, posture: f64) {panic!("unexpected vehicle notification service face")}
fn set_delay(&mut self, agent: Obj, seconds: f64, now: f64) {panic!("unexpected vehicle notification service set_delay")}
fn default_delay(&mut self, agent: Obj, now: f64) {panic!("unexpected vehicle notification service default_delay")}
fn helper_goto(&mut self, agent: Obj, point: &Vector3, direct: bool) {panic!("unexpected vehicle notification service helper_goto")}
fn helper_stop(&mut self, agent: Obj) {panic!("unexpected vehicle notification service helper_stop")}
fn helper_follow_path(&mut self, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool) {panic!("unexpected vehicle notification service helper_follow_path")}
fn helper_land(&mut self, agent: Obj, on: bool) {panic!("unexpected vehicle notification service helper_land")}
}
#[test]
fn nested_vehicle_notification_shares_state_without_retaining_borrows_or_allocating() {
 let mut state=Authority::new(VehicleAI::new());
 let mut world=Native {state:state.clone(),settings:Settings::default(),pool:Pool::new(6,0),nested:false,offer:false,calls:Cell::new(0)};
 ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
 for tick in 0..120 {
  assert_eq!(state.notify(&mut world),0.);
  assert_eq!(state.read(|s|s.notifier.counter),Some(((tick*2+1)%7) as f64));
 }
 COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
 assert_eq!(world.calls.get(),120);drop(world);
 let retained=state.finish();assert_eq!(retained.notifier.counter,Some(1.));
}

#[test]
fn vehicle_notice_recipients_keep_repeated_controllers_and_boardable_order_without_allocations() {
 let mut state=Authority::new(VehicleAI::new());
 let mut world=Native {state:state.clone(),settings:Settings::default(),pool:Pool::new(6,4),nested:false,offer:true,calls:Cell::new(0)};
 let buffers=bfe_sim::ai::owned::buffers::Buffers::new(4,0,2,0,2);
 ALLOCATIONS.with(|n|n.set(0));COUNTING.with(|n|n.set(true));
 for _ in 0..120 {
  state.write(|s|s.notifier.counter=Some(6.));
  let mut out=buffers.vehicle_notes.take();
  assert_eq!(state.notify_with(&mut world,&mut |w,controller,note| {
   // Native selection holds no vehicle-authority borrow across recipients.
   assert_eq!(w.state.read(|s|s.notifier.counter),Some(0.));
   out.push_reserved((controller as usize,bfe_sim::ai::control::world::EntRef {unit:note.obj,generation:note.generation}));
  }),4.);
  assert_eq!(out.len(),4);
  assert_eq!((out[0].0,out[1].0,out[2].0,out[3].0),(23,22,23,22));
  assert_eq!((out[0].1.unit,out[1].1.unit,out[2].1.unit,out[3].1.unit),(1,1,4,4));
  assert_eq!((out[0].1.generation,out[2].1.generation),(7.,8.));
 }
 COUNTING.with(|n|n.set(false));assert_eq!(ALLOCATIONS.with(Cell::get),0);
}
