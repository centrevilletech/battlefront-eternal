//! Exact nested-state comparison for replay: Object.is for every number.
use super::*;
use super::super::enhanced::Profile;

pub(super) trait Exact: std::fmt::Debug {
    fn exact(&self, other: &Self) -> bool;
    fn difference(&self, other: &Self) -> Option<String> {
        (!self.exact(other)).then(|| format!("capture {other:?}; native {self:?}"))
    }
}
impl Exact for f64 { fn exact(&self, other: &Self) -> bool { sim_replay::same_number(*self, *other) } }
macro_rules! scalar { ($($t:ty),* $(,)?) => { $(impl Exact for $t { fn exact(&self, other: &Self) -> bool { self == other } })* }; }
scalar!(bool, usize, u64, i32, String, str, ArsenalKind);
impl<T: Exact> Exact for Option<T> {
fn exact(&self, other: &Self) -> bool { match (self, other) { (Some(a), Some(b)) => a.exact(b), (None, None) => true, _ => false } }
fn difference(&self, other: &Self) -> Option<String> { match (self,other) { (Some(a),Some(b))=>a.difference(b),(None,None)=>None,_=>Some(format!("capture {other:?}; native {self:?}")) } }
}
impl<T: Exact> Exact for Vec<T> {
fn exact(&self, other: &Self) -> bool { self.len() == other.len() && self.iter().zip(other).all(|(a,b)| a.exact(b)) }
fn difference(&self, other: &Self) -> Option<String> {
if self.len()!=other.len() { return Some(format!("length capture {}; native {}",other.len(),self.len())); }
for (i,(a,b)) in self.iter().zip(other).enumerate() { if let Some(d)=a.difference(b) { return Some(format!("[{i}].{d}")); } } None
}
}
impl<T: Exact, const N: usize> Exact for [T;N] { fn exact(&self, other: &Self) -> bool { self.iter().zip(other).all(|(a,b)| a.exact(b)) } }
impl<T: Exact + ?Sized> Exact for Rc<T> { fn exact(&self, other: &Self) -> bool { (**self).exact(&**other) } }
impl<A: Exact, B: Exact> Exact for (A,B) { fn exact(&self, other: &Self) -> bool { self.0.exact(&other.0) && self.1.exact(&other.1) } }
macro_rules! fields { ($t:ty, $($f:ident),+ $(,)?) => { impl Exact for $t { fn exact(&self, other: &Self) -> bool { true $(&& self.$f.exact(&other.$f))+ }
fn difference(&self, other: &Self) -> Option<String> { $(if let Some(d)=self.$f.difference(&other.$f) { return Some(format!("{}.{}", stringify!($f), d)); })+ None }
} }; }
fields!(Vector3, x, y, z);
fields!(JsObj, fields);
impl Exact for Js { fn exact(&self, other: &Self) -> bool { match (self, other) {
(Js::Undefined, Js::Undefined) | (Js::Null, Js::Null) => true,
(Js::Bool(a), Js::Bool(b)) => a.exact(b), (Js::Num(a), Js::Num(b)) => a.exact(b), (Js::Str(a), Js::Str(b)) => a.exact(b),
(Js::Arr(a), Js::Arr(b)) => a.exact(b), (Js::Obj(a), Js::Obj(b)) => a.exact(b), _ => false,
} } }
impl<T: Exact> Exact for PriorityHeap<T> { fn exact(&self, other: &Self) -> bool {
self.count == other.count && self.dropped.exact(&other.dropped) && self.priority[..=self.count] == other.priority[..=other.count]
&& self.items[1..=self.count].iter().zip(&other.items[1..=other.count]).all(|(a,b)| a.exact(b))
} }
fields!(Ref, obj, generation);
fields!(ClassView, forward_speed, max_speed, max_health, health_type, jet_jump, jet_fuel_min_border, unit_type, role, ai_role);
fields!(ShieldView, on, max, value);
fields!(SoldierView, air, posture, shield, jet, jet_jump, jet_min_border, fuel, max_speed);
fields!(LeaseView, obj, human, released, live_false, controllable_controller, unit_controller);
fields!(WeaponView, def, levels, channel, clip, reserve, rounds, bottomless, heat, overheated, charging, held, wait, limit, reloading, busy, spread_scale, sniper_scale, yaw_spread_override, kick, floor, enhanced_floor);
fields!(ArsenalView, kind, weapons, channels, selected);
fields!(UnitView, obj, id, generation, alive, team, kind, human, hero, trap, targetable_false, collision_category, category, position, velocity, class, combat_max_health, max_health, soldier, arsenal, has_origin, controller, character, pilot, vehicle, tow_on, towed, turret, building_post, seat, has_lease, body_leases, intent, intent_buttons);
fields!(HintView, obj, position, forward, primary_mask, secondary_mask, side_mask, siblings);
fields!(Curves, a, d, b, s);
fields!(ShotAlert, pending, known, point, source);
fields!(DangerAlert, pending, point);
fields!(JetRequest, pending, vector);
fields!(Ctl, obj, unit, generation, character, objective, perception, target, fire_a, fire_b, roll, range_scale, aim_state, aim_point, aim_target, stance, nav_stance, facing, wanted_mode, dodge, alert_anim, shot, danger, acoustic, difficulty, jet);
fields!(AimCurves, a, d, b, s);
fields!(Aimer, direction, velocity, started, curves);
fields!(Profile, reaction, reaction_floor, convergence, sigma25, slew);
fields!(EnhancedAimer, profile, skill, direction, started, error, bias, key, last_key, lost_at, acquired_at, ready_at);
fields!(Perception, heap, candidate, candidate_point, aimer, fire, last_scan_count, enhanced);
fields!(LosRequest, controller, target, generation);
fields!(DeferredLos, active, scratch, overflow, tested);
fields!(TargetTracker, controller, target, fresh, point, sample_time, cost, delay, frozen, counting, deadline);
fields!(DangerState, first_contact, last_notify);
fields!(Response, mode, point, exit, alert);
fields!(Button, count, code);
fields!(Burst, state, heat_limit, last_end, saved_ammo, count);
fields!(FireControl, channel, buttons, last_release, charging, hold, sniper_active, sniper_deadline, burst, weapon_target, protected);
fields!(ClusterState, scan_at, flagged_at, generation);
fields!(Cover, state, pending, hint, posture, deadline, check_at, reselect_at);
fields!(Patterns, state, pending, cover, probability, cover_retry, prior, path_active, crouch, paused, pause_until, toward, side, band_check_at, step_at, facing, stepped, lateral_at, lateral_facing, last_path, retreat_at, cover_hint, jetting, jet_retry, jet_start, jet_hint, hint_jet_retry, searched, skip_fire);
fields!(CombatAgent, obj, controller, state, pending, tracker, patterns, point, hazard, align_at, looks, look_angle, aligned, hazard_until, prone_until, pattern, stationary);
fields!(Reservation, owner, at);
fields!(HelperView, obj, arrived, flying, follow, terminate);
fields!(AgentView, obj, patterns_obj, controller, tracker, helper, combat);
fields!(UnitWrites, buttons, aim, random, weapons);
