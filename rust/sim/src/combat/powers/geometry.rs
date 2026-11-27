//! Target tests and the soldier's response to a push, as the 2005 game
//! decides them. Pure functions over positions; the world supplies bodies.

use super::def::{Area, Cone, NEAR_CONE_COS, NEAR_CONE_RADIUS};
use crate::ordnance::math::{self as om, V3};

/// Upward bias the 2005 game adds to every push direction before scaling.
pub const PUSH_LIFT: f64 = 0.6;
/// A push origin farther than this from the body center pushes it.
const PUSH_MIN_SPAN: f64 = 1e-4;
/// A standing soldier is thrown when the new velocity rises off the ground
/// faster than this, in metres per second.
pub const THROW_RISE: f64 = 3.0;
/// A standing soldier tumbles when the new speed exceeds this.
pub const TUMBLE_SPEED: f64 = 8.0;
/// Thresholds on the push strength for soldiers already off the ground.
pub const AIRBORNE_THROW: f64 = 5.0;
pub const AIRBORNE_STAGGER: f64 = 1.0;
pub const JET_THROW: f64 = 8.0;
pub const STAGGER: f64 = 2.0;

/// Whether `point` lies inside `cone` from `origin` along unit `axis`, and its distance.
pub fn in_cone(cone: &Cone, origin: V3, axis: V3, point: V3) -> Option<f64> {
    let delta = om::sub(point, origin);
    let span_sq = om::length_sq(delta);
    if !(span_sq < cone.length * cone.length) || span_sq <= 1e-12 {
        return None;
    }
    let span = v8math::sqrt(span_sq);
    let dir = om::divide(delta, span);
    let bound = if span_sq < NEAR_CONE_RADIUS * NEAR_CONE_RADIUS {
        v8math::min(cone.cos_half, NEAR_CONE_COS)
    } else {
        cone.cos_half
    };
    (om::dot(dir, axis) > bound).then_some(span)
}

/// The caster's aim frame for an aimed ray: forward, horizontal right and
/// the normal of the aim plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AimFrame {
    pub forward: V3,
    pub right: V3,
    pub up: V3,
}

impl AimFrame {
    pub fn from_angles(yaw: f64, pitch: f64) -> Self {
        let (sy, cy) = (v8math::sin(yaw), v8math::cos(yaw));
        let (sp, cp) = (v8math::sin(pitch), v8math::cos(pitch));
        let forward = [sy * cp, sp, cy * cp];
        let right = [-cy, 0.0, sy];
        let up = om::cross(right, forward);
        Self { forward, right, up }
    }
}

/// Whether `point` (a body of `radius`) is inside the aimed ray of `area`.
/// Returns its squared distance as a fraction of the reach, the order in
/// which an area weapon prefers its targets.
pub fn in_ray(area: &Area, origin: V3, frame: &AimFrame, point: V3, radius: f64) -> Option<f64> {
    let d = om::sub(point, origin);
    if v8math::abs(om::dot(d, frame.up)) > 0.5 * area.height + radius {
        return None;
    }
    let forward = om::dot(d, frame.forward);
    let side = v8math::abs(om::dot(d, frame.right));
    if side * area.cos_half - radius > forward * area.sin_half {
        return None;
    }
    let reach = area.range + radius;
    let span_sq = side * side + forward * forward;
    if span_sq > reach * reach || area.range <= 0.0 {
        return None;
    }
    Some(span_sq / (area.range * area.range))
}

/// Where a cone's push comes from, how far it reaches and how hard it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PushSource {
    pub origin: V3,
    pub inner: f64,
    pub outer: f64,
    pub strength: f64,
}

/// A push spreads from the caster at full strength to half the cone length
/// and fades to nothing at twice the length. A pull pushes from a point past
/// the target, 1.1 cone lengths from the caster, so it pulls a far target
/// harder than a near one.
pub fn cone_push(cone: &Cone, caster: V3, target: V3) -> Option<PushSource> {
    if cone.push > 0.0 {
        return Some(PushSource { origin: caster, inner: 0.5 * cone.length, outer: 2.0 * cone.length, strength: cone.push });
    }
    if cone.push < 0.0 {
        let delta = om::sub(target, caster);
        let span = om::length(delta);
        let dir = if span > 0.0 { om::divide(delta, span) } else { delta };
        let reach = 1.1 * cone.length;
        return Some(PushSource { origin: om::add_scaled(caster, dir, reach), inner: 0.0, outer: reach, strength: -cone.push });
    }
    None
}

/// Effect falloff between an inner and an outer radius.
pub fn falloff(distance: f64, inner: f64, outer: f64) -> f64 {
    if distance < inner {
        return 1.0;
    }
    if distance < outer && outer > inner {
        return (outer - distance) / (outer - inner);
    }
    0.0
}

/// The soldier's state as a push sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Footing {
    /// Standing, crouching, sprinting or held by a choke.
    Ground,
    /// In a roll: pushed as on the ground, but a choke cannot take hold.
    Rolling,
    /// Jumping or falling.
    Air,
    /// Flying on a jet or a Force jump.
    Jet,
    /// Already thrown: a second push is ignored until the soldier recovers.
    Thrown,
    /// In a seat or held by something else: never pushed.
    Seated,
}

/// What a push does to the soldier besides replacing its velocity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reaction {
    /// The soldier leaves the ground and cannot act until it lands.
    Thrown,
    /// A fast horizontal push on the ground enters ground tumble directly.
    Tumble,
    /// A brief flinch; control is kept.
    Stagger,
    /// The new velocity alone.
    Moved,
}

/// The velocity a push gives to a body of `radius` centered at `center`.
pub fn push_velocity(source: &PushSource, center: V3, radius: f64) -> Option<(V3, f64)> {
    let delta = om::sub(center, source.origin);
    let span = om::length(delta);
    if !(span > PUSH_MIN_SPAN) {
        return None;
    }
    let strength = falloff(span - 0.5 * radius, source.inner, source.outer) * source.strength;
    if !(strength > 0.0) {
        return None;
    }
    let mut dir = om::divide(delta, span);
    dir[1] += PUSH_LIFT;
    Some((om::scale(dir, strength), strength))
}

/// The reaction of a soldier with `footing` to a push of `strength` that gave
/// it `velocity`, given its ground normal.
pub fn reaction(footing: Footing, velocity: V3, strength: f64, normal: V3) -> Option<Reaction> {
    match footing {
        Footing::Thrown | Footing::Seated => None,
        Footing::Ground | Footing::Rolling => Some(if om::dot(velocity, normal) > THROW_RISE {
            Reaction::Thrown
        } else if om::length_sq(velocity) > TUMBLE_SPEED * TUMBLE_SPEED {
            Reaction::Tumble
        } else if strength > STAGGER {
            Reaction::Stagger
        } else {
            Reaction::Moved
        }),
        Footing::Air => Some(if strength > AIRBORNE_THROW {
            Reaction::Thrown
        } else if strength > AIRBORNE_STAGGER {
            Reaction::Stagger
        } else {
            Reaction::Moved
        }),
        Footing::Jet => Some(if strength > JET_THROW {
            Reaction::Thrown
        } else if strength > STAGGER {
            Reaction::Stagger
        } else {
            Reaction::Moved
        }),
    }
}

/// Fixed-capacity list kept in ascending order of a sort key. Ties keep the
/// order of insertion, so the world's slot order breaks them.
#[derive(Clone, Copy, Debug)]
pub struct Nearest<T: Copy, const N: usize> {
    items: [(f64, Option<T>); N],
    len: usize,
}

impl<T: Copy, const N: usize> Default for Nearest<T, N> {
    fn default() -> Self {
        Self { items: [(0.0, None); N], len: 0 }
    }
}

impl<T: Copy, const N: usize> Nearest<T, N> {
    /// Insert, dropping the farthest entry when full. Returns false when the
    /// item itself was dropped.
    pub fn insert(&mut self, key: f64, item: T) -> bool {
        let mut at = self.len;
        while at > 0 && self.items[at - 1].0 > key {
            at -= 1;
        }
        if at >= N {
            return false;
        }
        let end = if self.len < N { self.len } else { N - 1 };
        let mut i = end;
        while i > at {
            self.items[i] = self.items[i - 1];
            i -= 1;
        }
        self.items[at] = (key, Some(item));
        if self.len < N {
            self.len += 1;
        }
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (f64, T)> + '_ {
        self.items[..self.len].iter().filter_map(|(k, t)| t.map(|t| (*k, t)))
    }
}
