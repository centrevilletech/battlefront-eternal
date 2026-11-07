//! Vehicle services on the battle's existing simulation module.
//! The live accessor never lends State or VehicleAI to a world callback.
use super::{module::state, vehicles::{world::*, wire::{Wire, Reader}, service::{VehicleAI, VehicleStorage, VehicleServices, Decay}, notify::VehicleNotify, random::Draws}};
use crate::{abi::sim, gen::{import_ai_vehicle_world, AiVehicleWorldArgs, AiVehicleWorldOp as Op}, vehicles::{BodyId, Hit}};
use three::Vector3;

struct Live;
impl VehicleStorage for Live {
    fn read<R>(&self, f: impl FnOnce(&VehicleAI)->R)->R { f(&state().vehicles) }
    fn write<R>(&mut self, f: impl FnOnce(&mut VehicleAI)->R)->R { f(&mut state().vehicles) }
}
fn call(op: Op, values: Vec<f64>) -> Reader {
    Reader::new(import_ai_vehicle_world(&AiVehicleWorldArgs {op, values}).values)
}
struct LiveWorld { settings: Settings }
impl LiveWorld { fn new()->Self { Self {settings:call(Op::Settings,vec![]).read()} } }
impl Draws for LiveWorld { fn next_a(&mut self)->u32 {call(Op::NextA,vec![]).read()} }
impl World for LiveWorld {
    fn settings(&self)->&Settings { &self.settings }
    fn nav(&mut self)->Option<&mut super::nav::Nav> { Some(&mut state().nav) }
    // Live edits use the scoped forms below; references never cross the ABI.
    fn steer(&mut self,_:Obj)->&mut Option<Steer> { unreachable!("use scoped steering access") }
    fn intent(&mut self,_:Obj)->Option<&mut SeatIntent> { unreachable!("use intent publication") }
    fn decay(&mut self,_:Obj)->&mut Option<Decay> { unreachable!("use decay publication") }
    fn edit_steer<R>(&mut self,id:Obj,f:impl FnOnce(&mut Option<Steer>)->R)->R {
        let mut value=call(Op::ReadSteer,vec![id as f64]).read();
        let answer=f(&mut value);
        let mut args=vec![id as f64];value.put(&mut args);call(Op::WriteSteer,args);
        answer
    }
    fn read_decay(&mut self,id:Obj)->Option<Decay> { call(Op::ReadDecay,vec![id as f64]).read() }
    fn write_decay(&mut self,id:Obj,value:Decay) { let mut args=vec![id as f64];value.put(&mut args);call(Op::WriteDecay,args); }
    fn write_vehicle_intent(&mut self,id:Obj,value:SeatIntent) { let mut args=vec![id as f64];value.put(&mut args);call(Op::WriteVehicleIntent,args); }
    fn query(&mut self,center:&Vector3,radius:f64,filter:QueryFilter)->Vec<Obj> {
        let (kind,id,team)=match filter { QueryFilter::EnemiesOf{unit,team}=>(0.0,unit,team), QueryFilter::LiveEnemiesOf{unit,team}=>(1.0,unit,team), QueryFilter::FriendsOf{entry,team}=>(2.0,entry,team) };
        call(Op::Query,vec![center.x,center.y,center.z,radius,kind,id as f64,team]).read()
    }
    fn mount_origin(&mut self,origin:Origin)->Vector3 {
        let (kind,id,index)=match origin {Origin::Lease(id)=>(0.0,id,0),Origin::TurretPivot(id)=>(1.0,id,0),Origin::SeatOrigin(id,index)=>(2.0,id,index)};
        call(Op::MountOrigin,vec![kind,id as f64,index as f64]).read()
    }
    fn capture_region(&mut self,id:Obj)->Option<(f64,f64)> { call(Op::CaptureRegion,vec![id as f64]).read::<Option<[f64;2]>>().map(|v|(v[0],v[1])) }
    fn now(&self) -> f64 {
        let args=Vec::new();
        call(Op::Now,args).read()
    }
    fn unit(&self, u: Obj) -> UnitView {
        let mut args=Vec::new(); u.put(&mut args);
        call(Op::Unit,args).read()
    }
    fn hull(&self, h: Obj) -> HullView {
        let mut args=Vec::new(); h.put(&mut args);
        call(Op::Hull,args).read()
    }
    fn turret(&self, t: Obj) -> TurretView {
        let mut args=Vec::new(); t.put(&mut args);
        call(Op::Turret,args).read()
    }
    fn lease(&self, l: Obj) -> LeaseView {
        let mut args=Vec::new(); l.put(&mut args);
        call(Op::Lease,args).read()
    }
    fn controller(&self, c: Obj) -> ControllerView {
        let mut args=Vec::new(); c.put(&mut args);
        call(Op::Controller,args).read()
    }
    fn post(&self, p: Obj) -> PostView {
        let mut args=Vec::new(); p.put(&mut args);
        call(Op::Post,args).read()
    }
    fn character_unit(&self, character: Obj) -> Option<Obj> {
        let mut args=Vec::new(); character.put(&mut args);
        call(Op::CharacterUnit,args).read()
    }
    fn character_controller(&self, character: Obj) -> Option<Obj> {
        let mut args=Vec::new(); character.put(&mut args);
        call(Op::CharacterController,args).read()
    }
    fn hulls(&self) -> Vec<Obj> {
        let args=Vec::new();
        call(Op::Hulls,args).read()
    }
    fn turrets(&self) -> Vec<Obj> {
        let args=Vec::new();
        call(Op::Turrets,args).read()
    }
    fn weapons(&self, control: Obj) -> Option<Vec<WeaponView>> {
        let mut args=Vec::new(); control.put(&mut args);
        call(Op::Weapons,args).read()
    }
    fn reinforcements(&self, team: f64) -> Option<f64> {
        let mut args=Vec::new(); team.put(&mut args);
        call(Op::Reinforcements,args).read()
    }
    fn human_units(&mut self) -> Vec<Obj> {
        let args=Vec::new();
        call(Op::HumanUnits,args).read()
    }
    fn has_planner(&self) -> bool {
        let args=Vec::new();
        call(Op::HasPlanner,args).read()
    }
    fn has_graph(&self) -> bool {
        let args=Vec::new();
        call(Op::HasGraph,args).read()
    }
    fn support_holds(&self, agent: Obj) -> Option<f64> {
        let mut args=Vec::new(); agent.put(&mut args);
        call(Op::SupportHolds,args).read()
    }
    fn hint_link(&self, hint: Obj) -> Option<Obj> {
        let mut args=Vec::new(); hint.put(&mut args);
        call(Op::HintLink,args).read()
    }
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {
        let mut args=Vec::new(); origin.put(&mut args); direction.put(&mut args); far.put(&mut args); ignore.put(&mut args);
        call(Op::Cast,args).read()
    }
    fn relation(&mut self, a: f64, b: f64) -> f64 {
        let mut args=Vec::new(); a.put(&mut args); b.put(&mut args);
        call(Op::Relation,args).read()
    }
    fn notify_relation(&mut self, a: f64, b: f64) -> f64 {
        call(Op::NotifyRelation,vec![a,b]).read()
    }
    fn los(&mut self, a: &Vector3, b: &Vector3) -> Option<bool> {
        let mut args=Vec::new(); a.put(&mut args); b.put(&mut args);
        call(Op::Los,args).read()
    }
    fn sphere(&mut self, x: Obj) -> Sphere {
        let mut args=Vec::new(); x.put(&mut args);
        call(Op::Sphere,args).read()
    }
    fn origin(&mut self, control: Obj) -> Vector3 {
        let mut args=Vec::new(); control.put(&mut args);
        call(Op::Origin,args).read()
    }
    fn forward(&mut self, control: Obj) -> Vector3 {
        let mut args=Vec::new(); control.put(&mut args);
        call(Op::Forward,args).read()
    }
    fn hold_yaw(&mut self, turret: Obj, a: f64) -> f64 {
        let mut args=Vec::new(); turret.put(&mut args); a.put(&mut args);
        call(Op::HoldYaw,args).read()
    }
    fn eligible_seat(&mut self, unit: Obj, hull: Obj) -> f64 {
        let mut args=Vec::new(); unit.put(&mut args); hull.put(&mut args);
        call(Op::EligibleSeat,args).read()
    }
    fn first_free_seat(&mut self, hull: Obj) -> f64 {
        let mut args=Vec::new(); hull.put(&mut args);
        call(Op::FirstFreeSeat,args).read()
    }
    fn board(&mut self, unit: Obj, hull: Obj) -> bool {
        let mut args=Vec::new(); unit.put(&mut args); hull.put(&mut args);
        call(Op::Board,args).read()
    }
    fn man(&mut self, unit: Obj, turret: Obj) -> bool {
        let mut args=Vec::new(); unit.put(&mut args); turret.put(&mut args);
        call(Op::Man,args).read()
    }
    fn request_exit_vehicle(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {
        let mut args=Vec::new(); unit.put(&mut args); options.put(&mut args);
        call(Op::RequestExitVehicle,args).read()
    }
    fn request_exit_turret(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {
        let mut args=Vec::new(); unit.put(&mut args); options.put(&mut args);
        call(Op::RequestExitTurret,args).read()
    }
    fn unboard(&mut self, unit: Option<Obj>, options: ExitOptions) -> bool {
        let mut args=Vec::new(); unit.put(&mut args); options.put(&mut args);
        call(Op::Unboard,args).read()
    }
    fn human_target(&mut self, hull: Obj) -> Option<Obj> {
        let mut args=Vec::new(); hull.put(&mut args);
        call(Op::HumanTarget,args).read()
    }
    fn damage(&mut self, hull: Obj, amount: f64) {
        let mut args=Vec::new(); hull.put(&mut args); amount.put(&mut args);
        call(Op::Damage,args);
    }
    fn pilot_input(&mut self, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick {
        let mut args=Vec::new(); control.put(&mut args); request.put(&mut args); aim.put(&mut args); dt.put(&mut args); env.put(&mut args);
        call(Op::PilotInput,args).read()
    }
    fn nav_attach(&mut self, controller: Option<Obj>, unit: Obj) {
        let mut args=Vec::new(); controller.put(&mut args); unit.put(&mut args);
        call(Op::NavAttach,args);
    }
    fn nav_update(&mut self, controller: Option<Obj>, dt: f64) {
        let mut args=Vec::new(); controller.put(&mut args); dt.put(&mut args);
        call(Op::NavUpdate,args);
    }
    fn nav_request(&mut self, controller: Option<Obj>) -> Option<Request> {
        let mut args=Vec::new(); controller.put(&mut args);
        call(Op::NavRequest,args).read()
    }
    fn reserve_hint(&mut self, hint: Obj, controller: Obj, now: f64) {
        let mut args=Vec::new(); hint.put(&mut args); controller.put(&mut args); now.put(&mut args);
        call(Op::ReserveHint,args);
    }
    fn set_hint_reserved_at(&mut self, hint: Obj, at: f64) {
        let mut args=Vec::new(); hint.put(&mut args); at.put(&mut args);
        call(Op::SetHintReservedAt,args);
    }
    fn set_intent_analogs(&mut self, control: Obj, stick: &Stick) {
        let mut args=Vec::new(); control.put(&mut args); stick.put(&mut args);
        call(Op::SetIntentAnalogs,args);
    }
    fn request_objective(&mut self, controller: Obj, exclude: bool) {
        let mut args=Vec::new(); controller.put(&mut args); exclude.put(&mut args);
        call(Op::RequestObjective,args);
    }
    fn controller_request_exit(&mut self, controller: Obj) {
        let mut args=Vec::new(); controller.put(&mut args);
        call(Op::ControllerRequestExit,args);
    }
    fn set_reboard_at(&mut self, controller: Obj, t: f64) {
        let mut args=Vec::new(); controller.put(&mut args); t.put(&mut args);
        call(Op::SetReboardAt,args);
    }
    fn fire_a(&mut self, controller: Obj) {
        let mut args=Vec::new(); controller.put(&mut args);
        call(Op::FireA,args);
    }
    fn invalidate_request(&mut self, controller: Obj) {
        let mut args=Vec::new(); controller.put(&mut args);
        call(Op::InvalidateRequest,args);
    }
    fn add_vehicle_note(&mut self, controller: Obj, note: Ref) {
        let mut args=Vec::new(); controller.put(&mut args); note.put(&mut args);
        call(Op::AddVehicleNote,args);
    }
    fn set_objective(&mut self, character: Obj, post: Obj) {
        let mut args=Vec::new(); character.put(&mut args); post.put(&mut args);
        call(Op::SetObjective,args);
    }
    fn set_support_holds(&mut self, agent: Obj, holds: f64) {
        let mut args=Vec::new(); agent.put(&mut args); holds.put(&mut args);
        call(Op::SetSupportHolds,args);
    }
    fn set_patrol_expires(&mut self, patrol: Obj, t: f64) {
        let mut args=Vec::new(); patrol.put(&mut args); t.put(&mut args);
        call(Op::SetPatrolExpires,args);
    }
    fn patrol_request_state(&mut self, patrol: Obj, state: f64) {
        let mut args=Vec::new(); patrol.put(&mut args); state.put(&mut args);
        call(Op::PatrolRequestState,args);
    }
    fn cp_center(&mut self, post: Obj) -> Vector3 {
        let mut args=Vec::new(); post.put(&mut args);
        call(Op::CpCenter,args).read()
    }
    fn helper_traversal(&mut self, agent: Obj, radius: bool) -> f64 {
        let mut args=Vec::new(); agent.put(&mut args); radius.put(&mut args);
        call(Op::HelperTraversal,args).read()
    }
    fn memory_point(&mut self, agent: Obj) -> Option<Vector3> {
        let mut args=Vec::new(); agent.put(&mut args);
        call(Op::MemoryPoint,args).read()
    }
    fn select_weapon(&mut self, controller: Obj, control: Obj, target: Obj, switch_allowed: bool) -> Choice {
        let mut args=Vec::new(); controller.put(&mut args); control.put(&mut args); target.put(&mut args); switch_allowed.put(&mut args);
        call(Op::SelectWeapon,args).read()
    }
    fn switch_weapon(&mut self, controller: Obj, control: Obj, index: usize) -> bool {
        let mut args=Vec::new(); controller.put(&mut args); control.put(&mut args); index.put(&mut args);
        call(Op::SwitchWeapon,args).read()
    }
    fn face(&mut self, patterns: Obj, posture: f64) {
        let mut args=Vec::new(); patterns.put(&mut args); posture.put(&mut args);
        call(Op::Face,args);
    }
    fn set_delay(&mut self, agent: Obj, seconds: f64, now: f64) {
        let mut args=Vec::new(); agent.put(&mut args); seconds.put(&mut args); now.put(&mut args);
        call(Op::SetDelay,args);
    }
    fn default_delay(&mut self, agent: Obj, now: f64) {
        let mut args=Vec::new(); agent.put(&mut args); now.put(&mut args);
        call(Op::DefaultDelay,args);
    }
    fn helper_goto(&mut self, agent: Obj, point: &Vector3, direct: bool) {
        let mut args=Vec::new(); agent.put(&mut args); point.put(&mut args); direct.put(&mut args);
        call(Op::HelperGoto,args);
    }
    fn helper_stop(&mut self, agent: Obj) {
        let mut args=Vec::new(); agent.put(&mut args);
        call(Op::HelperStop,args);
    }
    fn helper_follow_path(&mut self, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool) {
        let mut args=Vec::new(); agent.put(&mut args); points.to_vec().put(&mut args); over.put(&mut args); altitude.put(&mut args);
        call(Op::HelperFollowPath,args);
    }
    fn helper_land(&mut self, agent: Obj, on: bool) {
        let mut args=Vec::new(); agent.put(&mut args); on.put(&mut args);
        call(Op::HelperLand,args);
    }
}

/// All command arguments are owned before the first callback. Nested
/// commands can resize both shared blocks without changing this frame.
#[no_mangle]
pub extern "C" fn ai_vehicle_service(op:u32) {
    use crate::gen::AiVehicleServiceOp as Service;
    use super::vehicles::{service::{AnalogOptions, PatrolView}, controllers::controller_policy, reservations::ReservationService};
    let mut input=Reader::new(sim().calls.args().0.to_vec());
    let mut out=Vec::new();
    let op=Service::from_u8(op as u8).expect("vehicle service command");
    // Settings are read after owning the command's input.
    let mut w=LiveWorld::new();let mut live=Live;
    match op {
        Service::CurrentControllable => { let a0:Option<Obj>=input.read(); input.done(); live.current_controllable(&mut w,a0).put(&mut out); }
        Service::SeatCount => { let a0:Obj=input.read(); input.done(); live.seat_count(&mut w,a0).put(&mut out); }
        Service::OccupiedSeats => { let a0:Obj=input.read(); input.done(); live.occupied_seats(&mut w,a0).put(&mut out); }
        Service::HumanOccupants => { let a0:Obj=input.read(); input.done(); live.human_occupants(&mut w,a0).put(&mut out); }
        Service::HumanAboard => { let a0:Obj=input.read(); input.done(); live.human_aboard(&mut w,a0).put(&mut out); }
        Service::Boardables => { input.done(); live.boardables(&mut w).put(&mut out); }
        Service::CyclesPerSecond => { input.done(); live.cycles_per_second(&mut w).put(&mut out); }
        Service::Boarded => { let a0:Option<Obj>=input.read(); input.done(); live.boarded(&mut w,a0).put(&mut out); }
        Service::BoardedVehicle => { let a0:Option<Obj>=input.read(); input.done(); live.boarded_vehicle(&mut w,a0).put(&mut out); }
        Service::BoardedVehicleOf => { let a0:Option<Obj>=input.read(); input.done(); live.boarded_vehicle_of(&mut w,a0).put(&mut out); }
        Service::BoardedVehicleOfCharacter => { let a0:Option<Obj>=input.read(); input.done(); live.boarded_vehicle_of_character(&mut w,a0).put(&mut out); }
        Service::IsMountedTurret => { let a0:Option<Obj>=input.read(); input.done(); live.is_mounted_turret(&mut w,a0).put(&mut out); }
        Service::EmptySeats => { let a0:Option<Obj>=input.read(); input.done(); live.empty_seats(&mut w,a0).put(&mut out); }
        Service::CrewControllers => { let a0:Option<Obj>=input.read(); let a1:Option<Obj>=input.read(); input.done(); live.crew_controllers(&mut w,a0,a1).put(&mut out); }
        Service::SeatsFull => { let a0:Obj=input.read(); input.done(); live.seats_full(&mut w,a0).put(&mut out); }
        Service::PendingReservations => { let a0:Obj=input.read(); let a1:f64=input.read(); input.done(); live.pending_reservations(&mut w,a0,a1).put(&mut out); }
        Service::RequestExit => { let a0:Option<Obj>=input.read(); let a1:Option<Obj>=input.read(); input.done(); live.request_exit(&mut w,a0,a1).put(&mut out); }
        Service::LowTickets => { let a0:f64=input.read(); input.done(); live.low_tickets(&mut w,a0).put(&mut out); }
        Service::PilotKeepsSeat => { input.done(); live.pilot_keeps_seat().put(&mut out); }
        Service::AlertsBlocked => { input.done(); live.alerts_blocked().put(&mut out); }
        Service::RefusesHold => { input.done(); live.refuses_hold().put(&mut out); }
        Service::CanEnter => { let a0:Option<Obj>=input.read(); let a1:Obj=input.read(); input.done(); live.can_enter(&mut w,a0,a1).put(&mut out); }
        Service::FreeSeats => { let a0:Obj=input.read(); let a1:f64=input.read(); let a2:f64=input.read(); input.done(); live.free_seats(&mut w,a0,a1,a2).put(&mut out); }
        Service::Reserve => { let a0:Obj=input.read(); let a1:Obj=input.read(); let a2:f64=input.read(); input.done(); live.reserve(&mut w,a0,a1,a2); }
        Service::ReserveSeat => { let a0:Option<Obj>=input.read(); let a1:Option<Obj>=input.read(); input.done(); live.reserve_seat(&mut w,a0,a1).put(&mut out); }
        Service::HoldReservation => { let a0:Option<Obj>=input.read(); let a1:Obj=input.read(); input.done(); live.hold_reservation(&mut w,a0,a1).put(&mut out); }
        Service::BoardNow => { let a0:Option<Obj>=input.read(); let a1:Obj=input.read(); input.done(); live.board_now(&mut w,a0,a1).put(&mut out); }
        Service::BoardPathClear => { let a0:Option<Obj>=input.read(); let a1:Obj=input.read(); input.done(); live.board_path_clear(&mut w,a0,a1).put(&mut out); }
        Service::Relation => { let a0:f64=input.read(); let a1:f64=input.read(); input.done(); live.relation(&mut w,a0,a1).put(&mut out); }
        Service::TargetReserved => { let a0:Obj=input.read(); let a1:f64=input.read(); input.done(); live.target_reserved(&mut w,a0,a1).put(&mut out); }
        Service::ReserveTarget => { let a0:Obj=input.read(); let a1:Obj=input.read(); input.done(); live.reserve_target(&mut w,a0,a1); }
        Service::MountReach => { let a0:Option<Obj>=input.read(); let a1:Vector3=input.read(); input.done(); live.mount_reach(&mut w,a0,&a1).put(&mut out); }
        Service::UnreachableFromMount => { let a0:Option<Obj>=input.read(); let a1:Vector3=input.read(); input.done(); live.unreachable_from_mount(&mut w,a0,&a1).put(&mut out); }
        Service::SharedTarget => { let a0:Option<Obj>=input.read(); input.done(); live.shared_target(&mut w,a0).put(&mut out); }
        Service::IsAtAt => { let a0:Obj=input.read(); input.done(); live.is_at_at(&mut w,a0).put(&mut out); }
        Service::HoldHint => { let a0:Obj=input.read(); let a1:Obj=input.read(); input.done(); live.hold_hint(&mut w,a0,a1); }
        Service::CloseHint => { let a0:Obj=input.read(); let a1:f64=input.read(); input.done(); live.close_hint(&mut w,a0,a1); }
        Service::DisembarkPassengers => { let a0:Option<Obj>=input.read(); let a1:Option<Obj>=input.read(); input.done(); live.disembark_passengers(&mut w,a0,a1).put(&mut out); }
        Service::DestroyStuck => { let a0:Option<Obj>=input.read(); input.done(); live.destroy_stuck(&mut w,a0); }
        Service::AbandonPad => { let a0:Obj=input.read(); input.done(); live.abandon_pad(&mut w,a0).put(&mut out); }
        Service::UpdateDecay => { let a0:f64=input.read(); input.done(); live.update_decay(&mut w,a0); }
        Service::SupportPoint => { let a0:Option<Obj>=input.read(); let a1:Option<Obj>=input.read(); let a2:Option<Obj>=input.read(); input.done(); live.support_point(&mut w,a0,a1,a2).put(&mut out); }
        Service::Notify => { input.done(); live.notify(&mut w).put(&mut out); }
        Service::BoardPolicy => { let a0:Obj=input.read(); let a1:Obj=input.read(); let a2:Obj=input.read(); input.done(); live.board_policy(&mut w,a0,a1,a2).put(&mut out); }
        Service::PatrolMounted => { let a0:PatrolView=input.read(); input.done(); live.patrol_mounted(&mut w,&a0).put(&mut out); }
        Service::Dispatch => { let a0:Obj=input.read(); let a1:Obj=input.read(); let a2:Obj=input.read(); let a3:bool=input.read(); input.done(); live.dispatch(&mut w,a0,a1,a2,a3).put(&mut out); }
        Service::Analogs => { let a0:Obj=input.read(); let a1:Option<Request>=input.read(); let a2:Option<Vector3>=input.read(); let a3:f64=input.read(); let a4:AnalogOptions=input.read(); input.done(); live.analogs(&mut w,a0,a1.as_ref(),a2.as_ref(),a3,a4).put(&mut out); }
        Service::SeatInput => { let a0:Obj=input.read(); let a1:f64=input.read(); let a2:Option<Vector3>=input.read(); input.done(); live.seat_input(&mut w,a0,a1,a2.as_ref()).put(&mut out); }
        Service::ControllerPolicy => { let id:Option<Obj>=input.read();let p=controller_policy(id.map(|u|w.unit(u)).as_ref());policy_word(p).put(&mut out); }
        Service::MountedState => { let p=input.read::<u32>();let st=input.read();VehicleAI::mounted_state(policy(p),st).put(&mut out); }
        Service::AgentFor => { let p=input.read::<u32>();let st=input.read();VehicleAI::agent_for(policy(p),st).is_some().put(&mut out); }
        Service::CreateHelper => { let c=input.read();let k=live.create_helper(&w,c);(match k {HelperKind::None=>0u32,HelperKind::Stationary=>1,HelperKind::Fly=>2}).put(&mut out); }
        Service::ReservationRead => { live.read(|s|s.reservations.records.clone()).put(&mut out); }
        Service::ReservationClear => { live.write(|s|s.reservations.clear()); }
        Service::ReservationRelease => { let c=input.read();let u=input.read();let k=input.read();live.write(|s|s.reservations.release(c,u,k)); }
        Service::ReservationHas => { let c=input.read();let u=input.read();let k=input.read();live.reservation_has(&w,c,u,k).put(&mut out); }
        Service::ReservationCount => { let c=input.read();let t=input.read();let k=input.read();live.reservation_count(&w,c,t,k).put(&mut out); }
    }
    state().out=out;
}
fn policy(word:u32)->Option<super::vehicles::controllers::Policy> {
    use super::vehicles::controllers::Policy;
    match word {1=>Some(Policy::Mobile),2=>Some(Policy::Stationary),3=>Some(Policy::Fly),_=>None}
}
fn policy_word(p:Option<super::vehicles::controllers::Policy>)->u32 {
    use super::vehicles::controllers::Policy;
    match p {Some(Policy::Mobile)=>1,Some(Policy::Stationary)=>2,Some(Policy::Fly)=>3,None=>0}
}

use super::vehicles::{agents::{AgentHost, AgentMachine, AgentCx, BoardAgent, UnBoardAgent, FlyAgent, FlyAccess, FlyBehavior}, fly::{self,FlyHost}, patterns::{PatternAccess,PatternState,PatternsView}};
impl AgentHost for LiveWorld {
    fn interrupts_tick(&mut self, agent: Obj) -> bool {let mut args=Vec::new(); agent.put(&mut args); call(Op::InterruptsTick,args).read()}
    fn update_combat_target(&mut self, agent: Obj) {let mut args=Vec::new(); agent.put(&mut args); call(Op::UpdateCombatTarget,args);}
    fn aim(&mut self, agent: Obj, stance: f64) {let mut args=Vec::new(); agent.put(&mut args); stance.put(&mut args); call(Op::Aim,args);}
    fn tick_helper(&mut self, agent: Obj, dt: f64) {let mut args=Vec::new(); agent.put(&mut args); dt.put(&mut args); call(Op::TickHelper,args);}
    fn can_engage(&mut self, agent: Obj, target: Obj) -> bool {let mut args=Vec::new(); agent.put(&mut args); target.put(&mut args); call(Op::CanEngage,args).read()}
    fn has_helper(&self, agent: Obj) -> bool {let mut args=Vec::new(); agent.put(&mut args); call(Op::HasHelper,args).read()}
    fn helper_arrived(&mut self, agent: Obj) -> bool {let mut args=Vec::new(); agent.put(&mut args); call(Op::HelperArrived,args).read()}
    fn helper_failed(&mut self, agent: Obj) -> bool {let mut args=Vec::new(); agent.put(&mut args); call(Op::HelperFailed,args).read()}
    fn posts(&self) -> Vec<Obj> {let args=Vec::new(); call(Op::Posts,args).read()}
    fn query_hints(&mut self, center: &Vector3, requester: Obj, now: f64) -> Option<Vec<HintView>> {let mut args=Vec::new(); center.put(&mut args); requester.put(&mut args); now.put(&mut args); call(Op::QueryHints,args).read()}
}
impl FlyHost for LiveWorld {
    fn fly_unit(&mut self, helper: Obj) -> Option<Obj> {let mut args=Vec::new(); helper.put(&mut args); call(Op::FlyUnit,args).read()}
    fn fly_super_goto(&mut self, helper: Obj, point: &Vector3, direct: bool) {let mut args=Vec::new(); helper.put(&mut args); point.put(&mut args); direct.put(&mut args); call(Op::FlySuperGoto,args);}
    fn fly_super_stop(&mut self, helper: Obj) {let mut args=Vec::new(); helper.put(&mut args); call(Op::FlySuperStop,args);}
    fn fly_has_planner(&self, helper: Obj) -> bool {let mut args=Vec::new(); helper.put(&mut args); call(Op::FlyHasPlanner,args).read()}
    fn fly_planner_cancel(&mut self, helper: Obj) {let mut args=Vec::new(); helper.put(&mut args); call(Op::FlyPlannerCancel,args);}
    fn fly_path_new(&mut self, helper: Obj) -> u64 {let mut args=Vec::new(); helper.put(&mut args); call(Op::FlyPathNew,args).read()}
    fn fly_path_prepend(&mut self, path: u64, x: f64, y: f64, z: f64) -> bool {let mut args=Vec::new(); path.put(&mut args); x.put(&mut args); y.put(&mut args); z.put(&mut args); call(Op::FlyPathPrepend,args).read()}
    fn fly_path_release(&mut self, path: u64) {let mut args=Vec::new(); path.put(&mut args); call(Op::FlyPathRelease,args);}
    fn fly_install(&mut self, helper: Obj, path: u64, over: bool, altitude: bool) {let mut args=Vec::new(); helper.put(&mut args); path.put(&mut args); over.put(&mut args); altitude.put(&mut args); call(Op::FlyInstall,args);}
    fn fly_set_destination(&mut self, helper: Obj, destination: &Vector3) {let mut args=Vec::new(); helper.put(&mut args); destination.put(&mut args); call(Op::FlySetDestination,args);}
    fn fly_set_followed(&mut self, helper: Obj, failed: bool, state: f64, pending: f64) {let mut args=Vec::new(); helper.put(&mut args); failed.put(&mut args); state.put(&mut args); pending.put(&mut args); call(Op::FlySetFollowed,args);}
}
struct LiveMachine(Obj);
impl AgentMachine for LiveMachine {
    fn state(&self)->f64 {call(Op::AgentState,vec![self.0 as f64]).read()}
    fn request_state(&mut self,value:f64) {call(Op::AgentRequestState,vec![self.0 as f64,value]);}
}
struct LiveFly(Obj);
impl FlyAccess for LiveFly {
    fn fly(&self)->FlyAgent {call(Op::FlyFields,vec![self.0 as f64]).read()}
    fn edit_fly(&mut self,f:impl FnOnce(&mut FlyAgent)) {
        let mut fields=self.fly();f(&mut fields);let mut args=vec![self.0 as f64];fields.put(&mut args);call(Op::WriteFlyFields,args);
    }
}
struct LivePattern(Obj);
impl PatternAccess for LivePattern {
    fn pattern(&self)->PatternState {call(Op::PatternFields,vec![self.0 as f64]).read::<Option<PatternState>>().unwrap_or_default()}
    fn edit(&mut self,f:impl FnOnce(&mut PatternState)) {
        let mut fields=self.pattern();f(&mut fields);let mut args=vec![self.0 as f64];fields.put(&mut args);call(Op::WritePatternFields,args);
    }
}
#[no_mangle]
pub extern "C" fn ai_vehicle_agent(kind:u32,op:u32) {
    let mut input=Reader::new(sim().calls.args().0.to_vec());
    let cx:AgentCx=input.read();let dt:f64=input.read();let target:Obj=input.read();let board:BoardAgent=input.read();input.done();
    let mut w=LiveWorld::new();let mut ai=Live;let mut fsm=LiveMachine(cx.agent);let mut out=Vec::new();
    match kind {
        0=>match op {
            0=>board.on_enter(&mut w,&mut ai,&mut fsm,&cx),
            1=>board.behave(&mut w,&mut ai,&mut fsm,&cx,dt),
            3=>board.contested(&mut w,&ai,&cx).put(&mut out),
            4=>board.accepts_target(&mut w,&ai,&cx,target).put(&mut out),
            _=>panic!("invalid board agent command"),
        },
        1=>match op {
            0=>UnBoardAgent.on_enter(&mut w,&fsm,&cx),
            1=>UnBoardAgent.behave(&mut w,&ai,&mut fsm,&cx,dt),
            _=>panic!("invalid leaving agent command"),
        },
        2=>{let mut agent=LiveFly(cx.agent);match op {
            0=>agent.on_enter(&mut w,&mut ai,&mut fsm,&cx),
            1=>agent.behave(&mut w,&mut ai,&mut fsm,&cx,dt),
            2=>agent.on_exit(&mut w,&fsm,&cx),
            4=>agent.accepts_target(&mut w,&fsm,&cx,target).put(&mut out),
            5=>agent.clear_above(&mut w,&cx).put(&mut out),
            6=>agent.pick_post(&mut w,&cx),
            7=>agent.find_hint(&mut w,&cx).put(&mut out),
            8=>agent.approach(&mut w,&mut ai,&mut fsm,&cx),
            9=>agent.landed(&mut w,&mut ai,&mut fsm,&cx),
            _=>panic!("invalid flying agent command"),
        }},
        _=>panic!("invalid vehicle agent"),
    }
    state().out=out;
}
#[no_mangle]
pub extern "C" fn ai_vehicle_pattern(op:u32) {
    let mut input=Reader::new(sim().calls.args().0.to_vec());let p:PatternsView=input.read();let pattern:f64=input.read();input.done();
    let mut w=LiveWorld::new();let mut live=Live;let mut fields=LivePattern(p.obj);let mut out=Vec::new();
    match op {
        0=>live.enter_pattern(&mut w,&p,&mut fields,pattern),
        1=>live.tick_pattern(&mut w,&p,&mut fields,pattern),
        2=>live.exit_pattern(&mut w,&p,pattern),
        3=>live.sidestep(&mut w,&p).put(&mut out),
        _=>panic!("invalid vehicle pattern command"),
    }
    state().out=out;
}
#[no_mangle]
pub extern "C" fn ai_vehicle_helper(op:u32) {
    let mut input=Reader::new(sim().calls.args().0.to_vec());let id:Obj=input.read();let point:Vector3=input.read();
    let points:Vec<[f64;3]>=input.read();let over:bool=input.read();let altitude:bool=input.read();input.done();
    let mut w=LiveWorld::new();let mut out=Vec::new();
    match op {
        0=>fly::goto(&mut w,id,&point,over),
        1=>fly::follow_path(&mut w,id,&points,over,altitude).put(&mut out),
        2=>fly::stop(&mut w,id),
        3=>fly::land(&mut w,id,over),
        4=>fly::set_flags(&mut w,id,over,altitude),
        _=>panic!("invalid flyer helper command"),
    }
    state().out=out;
}

#[no_mangle]
pub extern "C" fn ai_vehicle_aux(op:u32) {
    use crate::gen::AiVehicleAuxOp as Aux;
    use super::vehicles::{service::{Body,StationaryHelper}, reservations::ReservationService, steering};
    let mut input=Reader::new(sim().calls.args().0.to_vec());let mut out=Vec::new();let mut live=Live;
    match Aux::from_u8(op as u8).expect("vehicle auxiliary command") {
        Aux::PadGet=>{let id:u64=input.read();live.read(|s|s.pad_left.iter().find(|(h,_)|*h==id).map(|(_,t)|*t)).put(&mut out);}
        Aux::PadSet=>{let id=input.read();let time=input.read();live.write(|s|match s.pad_left.iter_mut().find(|(h,_)|*h==id){Some((_,t))=>*t=time,None=>s.pad_left.push((id,time))});}
        Aux::CounterGet=>live.read(|s|s.notifier.counter).put(&mut out),
        Aux::CounterSet=>{let value=input.read();live.write(|s|s.notifier.counter=value);}
        Aux::SupportRead=>{
            let posts=live.read(|s|s.support_cache.clone());posts.len().put(&mut out);
            for (post,keys) in posts {post.put(&mut out);keys.len().put(&mut out);for (key,hubs) in keys {key.put(&mut out);hubs.put(&mut out);}}
        }
        op=>{
            let mut w=LiveWorld::new();
            match op {
                Aux::BodyOf=>{let id:Option<Obj>=input.read();let b=id.and_then(|id|VehicleAI::body_of_unit(&w.unit(id)));b.map(|b|match b{Body::Hull(id)|Body::Turret(id)=>id}).put(&mut out);}
                Aux::EntryOf=>{let id:Option<Obj>=input.read();let b=id.and_then(|id|VehicleAI::body_of_unit(&w.unit(id)));VehicleAI::entry_of(&w,b).put(&mut out);}
                Aux::LeaseOf=>{let id=input.read();VehicleAI::lease_of(&w,id).map(|l|l.obj).put(&mut out);}
                Aux::ControllerLease=>{let id=input.read();VehicleAI::controller_lease(&w,id).map(|l|l.obj).put(&mut out);}
                Aux::NominalSpeed=>{let id=input.read();live.nominal_speed(&w,&w.unit(id)).put(&mut out);}
                Aux::BodyReach=>{let id:Option<Obj>=input.read();let point=input.read();let body=id.and_then(|id|VehicleAI::body_of_unit(&w.unit(id)));live.body_reach(&mut w,body,&point).put(&mut out);}
                Aux::WalkerContacts=>{let id=input.read();let unit=w.unit(id);live.walker_contacts(&mut w,&unit);}
                Aux::WalkerContact=>{let id=input.read();let other:UnitView=input.read();let unit=w.unit(id);let now=w.now();steering::walker_contact(&mut w,&unit,other.kind,&other.position,&other.velocity,other.class.and_then(|c|c.bounds),now);}
                Aux::Reachable=>{let id=input.read();live.reachable(&mut w,id).put(&mut out);}
                Aux::PlayerNear=>{let id=input.read();let hull=w.hull(id);live.player_near(&mut w,&hull).put(&mut out);}
                Aux::Stationary=>{let id=input.read();StationaryHelper::default().stop(&mut w,id);}
                Aux::BoardTick=>{
                    let cx:AgentCx=input.read();let target:Option<Obj>=input.read();let mut fsm=LiveMachine(cx.agent);
                    if let Some(target)=target {
                        if live.can_enter(&mut w,Some(cx.controller),target) && live.hold_reservation(&w,Some(cx.controller),target) {
                            if let Some(unit)=w.controller(cx.controller).unit {
                                let a=w.sphere(unit);let b=w.sphere(target);let reach=a.radius+b.radius+0.5;
                                if a.center.distance_to_squared(&b.center)<reach*reach {live.board_now(&mut w,Some(cx.controller),target);}
                            }
                        } else {fsm.request_state(super::vehicles::controllers::DONE);}
                    } else {fsm.request_state(super::vehicles::controllers::DONE);}
                }
                Aux::Consider=>{let id=input.read();live.consider(&mut w,id).put(&mut out);}
                Aux::ReservationLive=>{live.reservation_live(&w);}
                Aux::AdaptController=>{
                    let id=input.read();let (policy,home)=live.adapt_controller(&w,id);
                    [policy_word(policy) as f64,home.map_or(f64::NAN,|v|if v{1.0}else{0.0})].put(&mut out);
                }
                Aux::SteerState=>{let id=input.read();steering::steer_state(&mut w,id,|_|());}
                _=>unreachable!(),
            }
        }
    }
    state().out=out;
}
