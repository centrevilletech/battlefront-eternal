//! Identity tokens and lossless number words for the live vehicle boundary.
//! Optional values carry a tag so NaN, null and absent fields remain distinct.
use super::{world::*, service::{Decay, AnalogOptions, PatrolView}, patterns::{PatternState, PatternsView, AttackRun}, agents::{Fsm, AgentCx, BoardAgent, FlyAgent}, reservations::Reservation, support::SupportHub};
use crate::{abi::sim, store::{StrId, word_u32}, vehicles::Hit};
use three::{Vector3, Quaternion, Matrix4};
pub struct Reader { pub values: Vec<f64>, at: usize }
impl Reader {
    pub fn new(values: Vec<f64>) -> Self { Self { values, at: 0 } }
    pub fn read<T: Wire>(&mut self) -> T { T::read(self) }
    pub fn done(&self) { assert_eq!(self.at, self.values.len(), "vehicle boundary word count"); }
}
pub trait Wire: Sized { fn read(r: &mut Reader) -> Self; fn put(&self, out: &mut Vec<f64>); }
impl Wire for f64 {
    fn read(r: &mut Reader) -> Self { let v=r.values[r.at]; r.at+=1; v }
    fn put(&self, out: &mut Vec<f64>) { out.push(*self); }
}
macro_rules! integer { ($($t:ty),*) => { $(impl Wire for $t {
    fn read(r: &mut Reader) -> Self { word_u32(r.read()) as $t }
    fn put(&self, out: &mut Vec<f64>) { out.push(*self as f64); }
})* }; }
integer!(u32, u64, usize);
impl Wire for bool { fn read(r:&mut Reader)->Self { r.read::<f64>()!=0.0 } fn put(&self,o:&mut Vec<f64>){o.push(*self as u8 as f64);} }
impl Wire for String {
    fn read(r:&mut Reader)->Self { let id=r.read(); String::from_utf16_lossy(sim().strings.units(StrId(id))) }
    fn put(&self,o:&mut Vec<f64>){ let id=sim().strings.intern(&self.encode_utf16().collect::<Vec<_>>()).0; id.put(o); }
}
impl<T: Wire> Wire for Option<T> {
    fn read(r:&mut Reader)->Self { if r.read::<bool>() {Some(r.read())} else {None} }
    fn put(&self,o:&mut Vec<f64>){ self.is_some().put(o);if let Some(v)=self {v.put(o);} }
}
impl<T: Wire> Wire for Vec<T> {
    fn read(r:&mut Reader)->Self { let n=r.read::<usize>(); (0..n).map(|_|r.read()).collect() }
    fn put(&self,o:&mut Vec<f64>){ self.len().put(o); for v in self {v.put(o);} }
}
impl<T: Wire, const N:usize> Wire for [T;N] {
    fn read(r:&mut Reader)->Self { std::array::from_fn(|_|r.read()) }
    fn put(&self,o:&mut Vec<f64>){ for v in self {v.put(o);} }
}
impl Wire for Vector3 {
    fn read(r:&mut Reader)->Self { Self::new(r.read(),r.read(),r.read()) }
    fn put(&self,o:&mut Vec<f64>){o.extend([self.x,self.y,self.z]);}
}
impl Wire for Quaternion {
    fn read(r:&mut Reader)->Self { Self::new(r.read(),r.read(),r.read(),r.read()) }
    fn put(&self,o:&mut Vec<f64>){o.extend([self.x,self.y,self.z,self.w]);}
}
impl Wire for Matrix4 {
    fn read(r:&mut Reader)->Self { Self {elements:r.read()} }
    fn put(&self,o:&mut Vec<f64>){self.elements.put(o);}
}
impl Wire for Ref {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), generation:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.generation.put(o); }
}
impl Wire for ClassView {
    fn read(r:&mut Reader)->Self { Self { ai_size_type:r.read(), forward_speed:r.read(), strafe_speed:r.read(), turn_rate:r.read(), pitch_rate:r.read(), moving_turn_only:r.read(), max_speed:r.read(), min_speed:r.read(), max_turn_speed:r.read(), max_pitch_speed:r.read(), max_yaw_speed:r.read(), vehicle_type:r.read(), landing_time:r.read(), landing_speed:r.read(), landed_height:r.read(), bounds:r.read(), no_enter_vehicles:r.read(), role:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.ai_size_type.put(o); self.forward_speed.put(o); self.strafe_speed.put(o); self.turn_rate.put(o); self.pitch_rate.put(o); self.moving_turn_only.put(o); self.max_speed.put(o); self.min_speed.put(o); self.max_turn_speed.put(o); self.max_pitch_speed.put(o); self.max_yaw_speed.put(o); self.vehicle_type.put(o); self.landing_time.put(o); self.landing_speed.put(o); self.landed_height.put(o); self.bounds.put(o); self.no_enter_vehicles.put(o); self.role.put(o); }
}
impl Wire for UnitView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), kind:r.read(), id:r.read(), generation:r.read(), alive:r.read(), team:r.read(), position:r.read(), velocity:r.read(), body_yaw:r.read(), class:r.read(), class_name:r.read(), soldier:r.read(), soldier_max_speed:r.read(), walker:r.read(), human:r.read(), network_human:r.read(), targetable:r.read(), seat:r.read(), lease:r.read(), vehicle:r.read(), turret:r.read(), controller:r.read(), character:r.read(), up_y:r.read(), own_label:r.read(), own_name:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.kind.put(o); self.id.put(o); self.generation.put(o); self.alive.put(o); self.team.put(o); self.position.put(o); self.velocity.put(o); self.body_yaw.put(o); self.class.put(o); self.class_name.put(o); self.soldier.put(o); self.soldier_max_speed.put(o); self.walker.put(o); self.human.put(o); self.network_human.put(o); self.targetable.put(o); self.seat.put(o); self.lease.put(o); self.vehicle.put(o); self.turret.put(o); self.controller.put(o); self.character.put(o); self.up_y.put(o); self.own_label.put(o); self.own_name.put(o); }
}
impl Wire for SeatView {
    fn read(r:&mut Reader)->Self { Self { lease:r.read(), weapons:r.read(), yaw_limits:r.read(), pitch_limits:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.lease.put(o); self.weapons.put(o); self.yaw_limits.put(o); self.pitch_limits.put(o); }
}
impl Wire for HullView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), entry:r.read(), alive:r.read(), name:r.read(), flying:r.read(), walking:r.read(), grounded:r.read(), flight_state:r.read(), gear:r.read(), speed:r.read(), max_speed_setting:r.read(), forward_speed_setting:r.read(), label:r.read(), position:r.read(), velocity:r.read(), yaw:r.read(), quaternion:r.read(), matrix:r.read(), bounds_min:r.read(), bounds_max:r.read(), bound_radius:r.read(), size:r.read(), body:r.read(), seats:r.read(), occupants:r.read(), driver:r.read(), free_seats:r.read(), occupied:r.read(), controlling_team:r.read(), tow_target:r.read(), slot_post:r.read(), decay_time:r.read(), max_health:r.read(), own_system:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.entry.put(o); self.alive.put(o); self.name.put(o); self.flying.put(o); self.walking.put(o); self.grounded.put(o); self.flight_state.put(o); self.gear.put(o); self.speed.put(o); self.max_speed_setting.put(o); self.forward_speed_setting.put(o); self.label.put(o); self.position.put(o); self.velocity.put(o); self.yaw.put(o); self.quaternion.put(o); self.matrix.put(o); self.bounds_min.put(o); self.bounds_max.put(o); self.bound_radius.put(o); self.size.put(o); self.body.put(o); self.seats.put(o); self.occupants.put(o); self.driver.put(o); self.free_seats.put(o); self.occupied.put(o); self.controlling_team.put(o); self.tow_target.put(o); self.slot_post.put(o); self.decay_time.put(o); self.max_health.put(o); self.own_system.put(o); }
}
impl Wire for TurretView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), entry:r.read(), alive:r.read(), gunner:r.read(), lease:r.read(), facing:r.read(), yaw_limits_setting:r.read(), pitch_limits_setting:r.read(), pitch_limits:r.read(), yaw_limits:r.read(), has_hold_yaw:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.entry.put(o); self.alive.put(o); self.gunner.put(o); self.lease.put(o); self.facing.put(o); self.yaw_limits_setting.put(o); self.pitch_limits_setting.put(o); self.pitch_limits.put(o); self.yaw_limits.put(o); self.has_hold_yaw.put(o); }
}
impl Wire for LeaseView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), released:r.read(), human:r.read(), live:r.read(), unit:r.read(), controllable:r.read(), vehicle:r.read(), turret:r.read(), index:r.read(), team:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.released.put(o); self.human.put(o); self.live.put(o); self.unit.put(o); self.controllable.put(o); self.vehicle.put(o); self.turret.put(o); self.index.put(o); self.team.put(o); }
}
impl Wire for ControllerView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), unit:r.read(), nav_control:r.read(), executor:r.read(), agent:r.read(), executor_status:r.read(), target:r.read(), helper:r.read(), helper_has_path:r.read(), helper_state:r.read(), helper_destination:r.read(), is_unit_controller:r.read(), fire_channel:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.unit.put(o); self.nav_control.put(o); self.executor.put(o); self.agent.put(o); self.executor_status.put(o); self.target.put(o); self.helper.put(o); self.helper_has_path.put(o); self.helper_state.put(o); self.helper_destination.put(o); self.is_unit_controller.put(o); self.fire_channel.put(o); }
}
impl Wire for Settings {
    fn read(r:&mut Reader)->Self { Self { min_fly_height:r.read(), max_fly_height:r.read(), notify_radius:r.read(), dense:r.read(), stay_in_turrets:r.read(), cycle:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.min_fly_height.put(o); self.max_fly_height.put(o); self.notify_radius.put(o); self.dense.put(o); self.stay_in_turrets.put(o); self.cycle.put(o); }
}
impl Wire for Request {
    fn read(r:&mut Reader)->Self { Self { valid:r.read(), steering_target:r.read(), waypoint:r.read(), segment_direction:r.read(), mode:r.read(), lower:r.read(), upper:r.read(), blend:r.read(), throttle:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.valid.put(o); self.steering_target.put(o); self.waypoint.put(o); self.segment_direction.put(o); self.mode.put(o); self.lower.put(o); self.upper.put(o); self.blend.put(o); self.throttle.put(o); }
}
impl Wire for Orbit {
    fn read(r:&mut Reader)->Self { Self { target:r.read(), offset:r.read(), radius:r.read(), factor:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.target.put(o); self.offset.put(o); self.radius.put(o); self.factor.put(o); }
}
impl Wire for StuckAt {
    fn read(r:&mut Reader)->Self { Self { x:r.read(), z:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.x.put(o); self.z.put(o); }
}
impl Wire for Steer {
    fn read(r:&mut Reader)->Self { Self { moving_turn_at:r.read(), loop_:r.read(), loop_at:r.read(), min_speed:r.read(), min_speed_at:r.read(), orbit:r.read(), land:r.read(), path_override:r.read(), path_altitude:r.read(), throttle:r.read(), recovery:r.read(), recovery_since:r.read(), last_recovery:r.read(), saved_forward:r.read(), hold_since:r.read(), holding:r.read(), turn_yaw:r.read(), turn_at:r.read(), walk_since:r.read(), stuck_at:r.read(), stuck_count:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.moving_turn_at.put(o); self.loop_.put(o); self.loop_at.put(o); self.min_speed.put(o); self.min_speed_at.put(o); self.orbit.put(o); self.land.put(o); self.path_override.put(o); self.path_altitude.put(o); self.throttle.put(o); self.recovery.put(o); self.recovery_since.put(o); self.last_recovery.put(o); self.saved_forward.put(o); self.hold_since.put(o); self.holding.put(o); self.turn_yaw.put(o); self.turn_at.put(o); self.walk_since.put(o); self.stuck_at.put(o); self.stuck_count.put(o); }
}
impl Wire for SeatIntent {
    fn read(r:&mut Reader)->Self { Self { up:r.read(), down:r.read(), brake:r.read(), trick:r.read(), immelmann:r.read(), boost:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.up.put(o); self.down.put(o); self.brake.put(o); self.trick.put(o); self.immelmann.put(o); self.boost.put(o); }
}
impl Wire for Stick {
    fn read(r:&mut Reader)->Self { Self { forward:r.read(), right:r.read(), turn:r.read(), pitch:r.read(), up:r.read(), down:r.read(), brake:r.read(), trick:r.read(), immelmann:r.read(), boost:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.forward.put(o); self.right.put(o); self.turn.put(o); self.pitch.put(o); self.up.put(o); self.down.put(o); self.brake.put(o); self.trick.put(o); self.immelmann.put(o); self.boost.put(o); }
}
impl Wire for PilotEnv {
    fn read(r:&mut Reader)->Self { Self { has_path:r.read(), min_fly_height:r.read(), max_fly_height:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.has_path.put(o); self.min_fly_height.put(o); self.max_fly_height.put(o); }
}
impl Wire for Sphere {
    fn read(r:&mut Reader)->Self { Self { center:r.read(), radius:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.center.put(o); self.radius.put(o); }
}
impl Wire for Choice {
    fn read(r:&mut Reader)->Self { Self { ok:r.read(), action:r.read(), in_band:r.read(), weapon:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.ok.put(o); self.action.put(o); self.in_band.put(o); self.weapon.put(o); }
}
impl Wire for PostView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), position:r.read(), team:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.position.put(o); self.team.put(o); }
}
impl Wire for HintView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), position:r.read(), forward:r.read(), up:r.read(), command_post:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.position.put(o); self.forward.put(o); self.up.put(o); self.command_post.put(o); }
}
impl Wire for WeaponView {
    fn read(r:&mut Reader)->Self { Self { base:r.read(), channel:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.base.put(o); self.channel.put(o); }
}
impl Wire for ExitOptions {
    fn read(r:&mut Reader)->Self { Self { post:r.read(), promote:r.read(), placement:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.post.put(o); self.promote.put(o); self.placement.put(o); }
}
impl Wire for Decay {
    fn read(r:&mut Reader)->Self { Self { idle:r.read(), sample:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.idle.put(o); self.sample.put(o); }
}
impl Wire for AnalogOptions {
    fn read(r:&mut Reader)->Self { Self { controller:r.read(), stepped:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.controller.put(o); self.stepped.put(o); }
}
impl Wire for PatrolView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), agent:r.read(), controller:r.read(), post:r.read(), support:r.read(), helper:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.agent.put(o); self.controller.put(o); self.post.put(o); self.support.put(o); self.helper.put(o); }
}
impl Wire for PatternsView {
    fn read(r:&mut Reader)->Self { Self { obj:r.read(), agent:r.read(), controller:r.read(), control:r.read(), tracker_point:r.read(), fresh_target:r.read(), target:r.read(), helper:r.read(), helper_arrived:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.obj.put(o); self.agent.put(o); self.controller.put(o); self.control.put(o); self.tracker_point.put(o); self.fresh_target.put(o); self.target.put(o); self.helper.put(o); self.helper_arrived.put(o); }
}
impl Wire for PatternState {
    fn read(r:&mut Reader)->Self { Self { toggle:r.read(), entry:r.read(), destination:r.read(), best:r.read(), progress_at:r.read(), fallback:r.read(), loop_:r.read(), inverted:r.read(), flip_a:r.read(), flip_b:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.toggle.put(o); self.entry.put(o); self.destination.put(o); self.best.put(o); self.progress_at.put(o); self.fallback.put(o); self.loop_.put(o); self.inverted.put(o); self.flip_a.put(o); self.flip_b.put(o); }
}
impl Wire for AttackRun {
    fn read(r:&mut Reader)->Self { Self { a:r.read(), b:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.a.put(o); self.b.put(o); }
}
impl Wire for Fsm {
    fn read(r:&mut Reader)->Self { Self { state:r.read(), pending:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.state.put(o); self.pending.put(o); }
}
impl Wire for AgentCx {
    fn read(r:&mut Reader)->Self { Self { agent:r.read(), controller:r.read(), unit:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.agent.put(o); self.controller.put(o); self.unit.put(o); }
}
impl Wire for BoardAgent {
    fn read(r:&mut Reader)->Self { Self { vehicle:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.vehicle.put(o); }
}
impl Wire for FlyAgent {
    fn read(r:&mut Reader)->Self { Self { hint:r.read(), search_at:r.read(), flag:r.read(), deadline:r.read(), post:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.hint.put(o); self.search_at.put(o); self.flag.put(o); self.deadline.put(o); self.post.put(o); }
}
impl Wire for Reservation {
    fn read(r:&mut Reader)->Self { Self { candidate:r.read(), owner:r.read(), time:r.read(), kind:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.candidate.put(o); self.owner.put(o); self.time.put(o); self.kind.put(o); }
}
impl Wire for SupportHub {
    fn read(r:&mut Reader)->Self { Self { hub:r.read(), distance:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.hub.put(o); self.distance.put(o); }
}
impl Wire for Hit {
    fn read(r:&mut Reader)->Self { Self { distance:r.read(), point:r.read(), normal:r.read(), terrain:r.read(), body:r.read() } }
    fn put(&self,o:&mut Vec<f64>){ self.distance.put(o); self.point.put(o); self.normal.put(o); self.terrain.put(o); self.body.put(o); }
}
impl<T: Wire> Wire for &T {
    fn read(_: &mut Reader)->Self { unreachable!("boundary reads own their values") }
    fn put(&self, out:&mut Vec<f64>) { (*self).put(out); }
}
impl Wire for super::patterns::Dispatch {
    fn read(r:&mut Reader)->Self { Self {pattern:r.read(),probability:r.read(),aux:r.read(),action:r.read(),in_band:r.read(),weapon:r.read()} }
    fn put(&self,o:&mut Vec<f64>) {self.pattern.put(o);self.probability.put(o);self.aux.put(o);self.action.put(o);self.in_band.put(o);self.weapon.put(o);}
}
