//! src/flight/targeting.js: a craft's assist target and its lock (each
//! tick, `updateFlightTarget`), a flyer missile's proportional navigation
//! (`navigateMissile`, which the rounds ask through their world) and the
//! assisted shot (`assistShot`, in a pilot's fire).
//!
//! A target is an entry of the registry within a cone about the nose, in
//! range and an enemy; the best is the one nearest the nose that the craft
//! can see, the earlier entry of equal ones. The sight ray goes only to the
//! candidates in score order until one passes. As in the JavaScript, the
//! sight test is passed by whatever the ray strikes beyond 2 m short of the
//! target, by the target hull's own body, and, for a target that is no
//! hull's (a soldier, a turret, a building), by the terrain and the map's
//! own collision, whose hits name no body (quirks.md).

use super::config::Tuning;
use super::model::truthy;
use super::state::{LockState, Lock};
use super::{FlightWorld, Member};
use crate::rig::math;
use crate::vehicles::{BodyId, Hull};
use three::{Quaternion, Vector3};
use v8math::{abs, max, min};

const PI: f64 = std::f64::consts::PI;

/// What inCone leaves for clear(): the direction to the target and how far.
#[derive(Clone, Debug)]
struct Sight {
    direction: Vector3,
    distance: f64,
}

#[derive(Clone, Debug, Default)]
pub struct TargetScratch {
    candidates: Vec<usize>, scores: Vec<f64>, sights: Vec<Option<(f64, Sight)>>,
    asked: Vec<usize>, order: Vec<usize>, sort: jsrt::sort::SortScratch<usize>,
}

impl TargetScratch {
    /// bestTarget may first run when a pilot boards long after a battle warmed.
    /// Size its work from the registry before that first acquisition, rather
    /// than allocating candidates as enemies enter the cone.
    pub fn reserve(&mut self, count: usize) {
        if self.candidates.capacity() < count { self.candidates.reserve(count.saturating_sub(self.candidates.len())); }
        if self.scores.capacity() < count { self.scores.reserve(count.saturating_sub(self.scores.len())); }
        if self.sights.capacity() < count { self.sights.reserve(count.saturating_sub(self.sights.len())); }
        if self.asked.capacity() < count { self.asked.reserve(count.saturating_sub(self.asked.len())); }
        if self.order.capacity() < count { self.order.reserve(count.saturating_sub(self.order.len())); }
        self.sort.reserve(count);
    }
}

impl Hull {
    /// inCone(v, t, cone): a living enemy entry within the cone and in
    /// range: its score (the cosine to the nose), else None.
    fn in_cone<W: FlightWorld>(&self, w: &mut W, t: &Tuning, nose: &Vector3, i: usize, cone: f64) -> Option<(f64, Sight)> {
        let target = w.member(i);
        if !target.alive || target.own {
            return None;
        }
        let relation = w.relation(self.flight.team, target.team);
        if relation >= 0.0 {
            return None;
        }
        let point = w.aim_point(i);
        let mut d = Vector3::default();
        d.sub_vectors(&point, &self.position);
        let distance = d.length();
        if distance > t.lock_range || distance < 1.0 {
            return None;
        }
        let dot = d.divide_scalar(distance).dot(nose);
        if dot < math::cos(cone * PI / 180.0) {
            return None;
        }
        Some((dot, Sight { direction: d, distance }))
    }

    /// clear(v, t): nothing but the target itself, or something within 2 m
    /// short of it, stands between the craft and the target.
    fn clear<W: FlightWorld>(&self, w: &mut W, target: &Member, sight: &Sight) -> bool {
        let contact = w.cast(&self.position, &sight.direction, sight.distance, Some(self.body));
        match contact {
            None => true,
            Some(c) => !(owner_differs(c.body, target.body) && c.distance < sight.distance - 2.0),
        }
    }

    /// eligible(v, t, cone).
    fn eligible<W: FlightWorld>(&self, w: &mut W, t: &Tuning, nose: &Vector3, i: usize, cone: f64) -> Option<f64> {
        let (dot, sight) = self.in_cone(w, t, nose, i, cone)?;
        let target = w.member(i);
        if self.clear(w, &target, &sight) {
            Some(dot)
        } else {
            None
        }
    }

    /// bestTarget(v, cone): the best entry in the cone the craft can see.
    fn best_target<W: FlightWorld>(&mut self, w: &mut W, t: &Tuning, nose: &Vector3, cone: f64) -> Option<usize> {
        let mut scratch = std::mem::take(&mut self.target_scratch);
        let result = self.best_target_with(w, t, nose, cone, &mut scratch);
        self.target_scratch = scratch; result
    }
    fn best_target_with<W: FlightWorld>(&self, w: &mut W, t: &Tuning, nose: &Vector3, cone: f64, scratch: &mut TargetScratch) -> Option<usize> {
        let TargetScratch { candidates, scores, sights, asked, order, sort } = scratch;
        candidates.clear(); scores.clear(); sights.clear(); asked.clear(); order.clear();
        let n = w.member_count();
        // The entries inCone will ask the aim point of (living, not the
        // craft's own, enemies), in its order.
        for i in 0..n {
            let m = w.member(i);
            if m.alive && !m.own && w.relation(self.flight.team, m.team) < 0.0 {
                asked.push(i);
            }
        }
        w.prefetch_aim_points(asked);
        for i in 0..n {
            if let Some((score, sight)) = self.in_cone(w, t, nose, i, cone) {
                candidates.push(i);
                scores.push(score);
                sights.push(Some((score, sight)));
            }
        }
        // order: by score, highest first, then by place (V8's sort).
        order.extend(0..candidates.len());
        jsrt::sort::js_sort_reuse(order, &mut |&a: &usize, &b: &usize| {
            let d = scores[b] - scores[a];
            if truthy(d) {
                d
            } else {
                a as f64 - b as f64
            }
        }, sort);
        for &k in order.iter() {
            let i = candidates[k];
            // inCone again, for its direction and distance (its calls made
            // again too). Nothing it reads has changed since the first
            // pass, so it passes again; were it not to, clear() would read
            // what the last pass that did left, which the first pass's is.
            let seen = self.in_cone(w, t, nose, i, cone).or_else(|| sights[k].take());
            let target = w.member(i);
            if let Some((_, sight)) = &seen {
                if self.clear(w, &target, sight) {
                    return Some(i);
                }
            }
        }
        None
    }

    /// updateFlightTarget(v, dt, press): the assist target, then the lock
    /// (a press locks the best target in the lock cone, or lets the lock
    /// go), held while the target lives and stays eligible in the wider
    /// retain cone, with a grace for lost sight; and what it leaves in each
    /// weapon.
    pub fn update_flight_target<W: FlightWorld>(&mut self, w: &mut W, dt: f64, press: bool) {
        let t = w.tuning(self.flight.class);
        let mut nose = Vector3::new(0.0, 0.0, 1.0);
        nose.apply_quaternion(&self.quaternion);
        self.flight.assist_id = -1.0;
        if w.environment().assists {
            if let Some(i) = self.best_target(w, &t, &nose, t.assist_cone) {
                self.flight.assist_id = w.member(i).id;
            }
        }
        let mut current = w.member_by_id(self.flight.lock_id);
        if press {
            if current.is_some() {
                self.flight.lock_id = -1.0;
                current = None;
            } else {
                current = self.best_target(w, &t, &nose, t.lock_cone);
                if let Some(i) = current {
                    let m = w.member(i);
                    self.flight.lock_id = m.id;
                    self.flight.lock_generation = m.generation.unwrap_or(0.0);
                }
            }
            self.flight.lock_timer = 0.0;
            self.flight.lost_los = 0.0;
        }
        if let Some(i) = current {
            let m = w.member(i);
            if !m.alive || m.generation.unwrap_or(0.0) != self.flight.lock_generation {
                current = None;
                self.flight.lock_id = -1.0;
            }
        }
        if let Some(i) = current {
            if self.eligible(w, &t, &nose, i, t.retain_cone).is_some() {
                self.flight.lost_los = 0.0;
                self.flight.lock_timer += dt;
            } else {
                self.flight.lost_los += dt;
            }
            if self.flight.lost_los > t.los_grace {
                self.flight.lock_id = -1.0;
                current = None;
            }
        }
        self.flight.lock_state = match current {
            Some(_) if self.flight.lock_timer >= t.lock_dwell => LockState::Locked,
            Some(_) => LockState::Locking,
            None => LockState::None,
        };
        if current.is_none() {
            self.flight.lock_timer = 0.0;
        }
        let held = current.map(|i| w.member(i));
        self.flight.lock = Some(Lock {
            target_id: held.map_or(-1.0, |m| m.id),
            generation: held.and_then(|m| m.generation).unwrap_or(-1.0),
            elapsed: self.flight.lock_timer,
            complete: self.flight.lock_state == LockState::Locked,
        });
    }

    /// assistShot(v, direction, origin): a pilot's shot bent toward his
    /// assist target when it is within the assist cone of the shot.
    pub fn assist_shot<W: FlightWorld>(&self, w: &mut W, direction: &mut Vector3, origin: &Vector3) {
        if !w.environment().assists || self.flight.assist_id < 0.0 {
            return;
        }
        let Some(i) = w.member_by_id(self.flight.assist_id) else { return };
        let target = w.member(i);
        if !target.alive {
            return;
        }
        let point = w.aim_point(i);
        let t = w.tuning(self.flight.class);
        let mut d = Vector3::default();
        d.sub_vectors(&point, origin).normalize();
        if direction.dot(&d) > math::cos(t.assist_cone * PI / 180.0) {
            direction.lerp(&d, t.assist_strength).normalize();
        }
    }
}

/// `contact.body?.owner !== t.vehicle`: a hit with no body (the terrain, the
/// map's own collision) names an undefined owner, as a target that is no
/// hull's has an undefined hull; a body's owner is its hull, a prop, or
/// null, never undefined.
fn owner_differs(contact: Option<BodyId>, target: Option<BodyId>) -> bool {
    match (contact, target) {
        (None, None) => false,
        (Some(b), Some(t)) => b != t,
        _ => true,
    }
}

/// What a flyer missile's seeker reads of its target (the entry the
/// rounds' world found alive and of the life fired at): where it is, how
/// it moves, and how fast the hull behind it rolls (its `flightRollRate`,
/// 0 for none).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MissileTarget {
    pub position: Vector3,
    /// `target.velocity` (None: undefined, a zero vector is used).
    pub velocity: Option<Vector3>,
    /// `hull.flightRollRate || 0`.
    pub roll_rate: f64,
}

/// navigateMissile(r, target, dt): proportional navigation, the
/// acceleration following the line of sight's angular velocity, the turn
/// capped at the class's missile turn rate. Turns `velocity` in place;
/// returns false where the seeker loses the target (a hull rolling fast
/// within the evade range, or a target outside the seeker's cone), true
/// where it keeps it.
pub fn navigate_missile(position: &Vector3, velocity: &mut Vector3, tuning: &Tuning, target: &MissileTarget, dt: f64) -> bool {
    let mut los = target.position;
    los.sub(position);
    let distance = los.length();
    let speed = velocity.length();
    if distance < 1.0 || speed < 1.0 {
        return true;
    }
    // A powered roll changes aspect faster than the seeker can track.
    if distance < tuning.missile_evade_range && abs(target.roll_rate) > tuning.missile_roll_break_rate {
        return false;
    }
    let mut heading = *velocity;
    heading.normalize();
    let mut sight = los;
    sight.normalize();
    if heading.dot(&sight) < math::cos(tuning.missile_seeker * PI / 180.0) {
        return false;
    }
    let mut rel = target.velocity.unwrap_or_default();
    rel.sub(velocity);
    let mut omega = los;
    omega.cross(&rel).divide_scalar(distance * distance);
    let closing = max(0.0, -rel.dot(&los) / distance);
    let mut acceleration = omega;
    let mut unit = *velocity;
    unit.normalize();
    acceleration.cross(&unit).multiply_scalar(tuning.missile_navigation * closing);
    let mut wanted = *velocity;
    wanted.add_scaled_vector(&acceleration, dt).normalize();
    let mut from = *velocity;
    from.normalize();
    let angle = from.angle_to(&wanted);
    let cap = tuning.missile_turn_rate * dt;
    if angle > 1e-7 {
        let mut q = Quaternion::default();
        q.set_from_unit_vectors(&from, &wanted);
        let full = q;
        q.slerp_quaternions(&Quaternion::default(), &full, min(1.0, cap / angle));
        velocity.apply_quaternion(&q);
    }
    true
}
