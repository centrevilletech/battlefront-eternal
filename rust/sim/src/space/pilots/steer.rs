//! The stick a pilot holds and the bounded look-ahead tests it flies by.

use super::super::bounds::SpaceBounds;
use super::super::flight::{cast_flyer_ray, SpaceFighter, SpaceFlightProfile};
use super::vec::{add, clamp, cross, dot, len, scale, sub, unit};

use crate::collision::CollisionOwner;

/// Stick deflection per radian of the angle between the nose and the wanted
/// direction (full stick from about 23 degrees off).
pub const PROVISIONAL_STICK_GAIN: f64 = 2.5;
/// Seconds of flight a hull look-ahead covers, and its shortest reach.
pub const PROVISIONAL_LOOK_AHEAD_S: f64 = 2.6;
pub const PROVISIONAL_LOOK_AHEAD_MIN: f64 = 70.0;
/// Clearance kept from another craft beyond both hulls' reach, and the time
/// over which a closest approach counts as a coming collision.
pub const PROVISIONAL_CRAFT_CLEARANCE: f64 = 10.0;
pub const PROVISIONAL_CRAFT_HORIZON_S: f64 = 1.6;

/// Direction of flight: the velocity, or the nose when nearly still.
pub fn heading(f: &SpaceFighter) -> [f64; 3] {
    if len(f.velocity) > 1.0 { unit(f.velocity) } else { f.forward() }
}

/// Half the craft's widest extent, the reach of its hull from its centre.
pub fn hull_reach(p: &SpaceFlightProfile) -> f64 {
    let ext = |i: usize| v8math::max(p.contact_bounds_max[i].abs(), p.contact_bounds_min[i].abs());
    v8math::max(ext(0), v8math::max(ext(1), ext(2)))
}

/// A surface the craft would meet within the look-ahead.
#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub distance: f64,
    /// The body met (negative for static geometry).
    pub body: i32,
    /// Surface normal turned to face the craft.
    pub normal: [f64; 3],
}

/// One ray along `dir` reaching `reach_m`, from the hull's edge chosen by
/// `probe` (centre, then right, left, top and bottom in turn), past hangar
/// shield planes a craft flies through. A single cast.
#[allow(clippy::too_many_arguments)]
pub fn look_along(
    f: &SpaceFighter,
    p: &SpaceFlightProfile,
    bounds: &SpaceBounds,
    collision: &mut CollisionOwner,
    world: usize,
    probe: u32,
    dir: [f64; 3],
    reach_m: f64,
) -> Option<Obstacle> {
    let span = v8math::max(p.contact_bounds_max[0].abs(), p.contact_bounds_min[0].abs());
    let height = v8math::max(p.contact_bounds_max[1].abs(), p.contact_bounds_min[1].abs());
    let offset = match probe % 5 {
        1 => scale(f.right(), span),
        2 => scale(f.right(), -span),
        3 => scale(f.up(), height),
        4 => scale(f.up(), -height),
        _ => [0.0; 3],
    };
    let origin = add(f.position, offset);
    let ignore = f.kernel_body.map_or(-1, |b| b as i32);
    let (body, hit, distance) = cast_flyer_ray(origin, dir, reach_m, ignore, bounds, collision, world)?;
    let mut normal = unit([hit.nx, hit.ny, hit.nz]);
    if dot(normal, dir) > 0.0 {
        normal = scale(normal, -1.0);
    }
    Some(Obstacle { distance, body, normal })
}

/// The way out from a surface ahead: along the surface, leaning away from it.
pub fn escape_from(heading: [f64; 3], normal: [f64; 3], up: [f64; 3]) -> [f64; 3] {
    let along = sub(heading, scale(normal, dot(heading, normal)));
    let along = if len(along) < 0.2 { up } else { unit(along) };
    unit(add(along, scale(normal, 0.9)))
}

/// A craft's position, velocity and reach as the others see it this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Traffic {
    pub id: usize,
    pub generation: u32,
    pub team: i32,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub reach: f64,
}

/// The way out from the nearest coming collision with another craft, if any:
/// the closest approach within the horizon closer than both hulls' reach plus
/// the clearance.
pub fn traffic_escape(me: &Traffic, others: &[Traffic], up: [f64; 3]) -> Option<[f64; 3]> {
    let mut best: Option<(f64, [f64; 3])> = None;
    for o in others.iter().filter(|o| o.id != me.id) {
        let rel = sub(o.position, me.position);
        if len(rel) > 150.0 {
            continue;
        }
        let rv = sub(o.velocity, me.velocity);
        // Only a closing craft is a coming collision.
        if dot(rel, rv) >= 0.0 {
            continue;
        }
        let rv2 = dot(rv, rv);
        let t = if rv2 > 1e-6 { clamp(-dot(rel, rv) / rv2, 0.0, PROVISIONAL_CRAFT_HORIZON_S) } else { 0.0 };
        let closest = add(rel, scale(rv, t));
        let miss = len(closest);
        if miss > me.reach + o.reach + PROVISIONAL_CRAFT_CLEARANCE {
            continue;
        }
        if best.is_none_or(|(bt, _)| t < bt) {
            let away = if miss > 0.5 { scale(closest, -1.0 / miss) } else {
                let side = cross(unit(rv), up);
                if len(side) > 0.1 { unit(side) } else { up }
            };
            best = Some((t, away));
        }
    }
    best.map(|(_, away)| away)
}

/// Where to point to meet a target moving at `target_vel` with rounds that
/// fly at `speed` from `from`: one solution of the intercept equation, or the
/// target itself when none exists.
pub fn lead(from: [f64; 3], target: [f64; 3], target_vel: [f64; 3], speed: f64) -> [f64; 3] {
    if speed <= 1.0 {
        return target;
    }
    let r = sub(target, from);
    let a = dot(target_vel, target_vel) - speed * speed;
    let b = 2.0 * dot(r, target_vel);
    let c = dot(r, r);
    let t = if a.abs() < 1e-6 {
        if b.abs() < 1e-9 { 0.0 } else { -c / b }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            0.0
        } else {
            let s = v8math::sqrt(disc);
            let t1 = (-b - s) / (2.0 * a);
            let t2 = (-b + s) / (2.0 * a);
            let t = if t1 > 0.0 && (t2 <= 0.0 || t1 < t2) { t1 } else { t2 };
            if t > 0.0 { t } else { 0.0 }
        }
    };
    add(target, scale(target_vel, clamp(t, 0.0, 6.0)))
}
