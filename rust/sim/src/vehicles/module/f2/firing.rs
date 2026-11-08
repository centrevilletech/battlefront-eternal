//! Vehicles.fireCrews, Vehicle.fireSeat and the mounted fire and cable phase.
//! Barrels, weapon clocks and cables persist between calls; only frozen crew
//! input and facts still owned by units cross the phase boundary.
use crate::vehicles::build::TowLeg;
use crate::vehicles::hull::angle;
use three::{Matrix4, Vector3};

/// Vehicle.legAt: a body hit above the pelvis cannot attach a cable. Leg
/// columns stay in the bind frame, even while the walker's pose advances.
fn leg_at(legs: &[TowLeg], top: f64, low: f64, inverse: &Matrix4, point: &Vector3) -> Option<(usize, Vector3)> {
    if legs.is_empty() { return None; }
    let mut local = *point;
    local.apply_matrix4(inverse);
    if local.y > top { return None; }
    let mut best = None;
    let mut span = f64::INFINITY;
    for (i, leg) in legs.iter().enumerate() {
        let c = leg.calf.or(leg.thigh).unwrap();
        let d = v8math::hypot(local.x - c.x, local.z - c.z);
        if d < span { span = d; best = Some(i); }
    }
    let i = best?;
    let leg = &legs[i];
    let c = leg.calf.or(leg.thigh).unwrap();
    Some((i, Vector3::new(c.x, three::clamp(local.y, leg.foot.map_or(low, |p| p.y), leg.thigh.map_or(top, |p| p.y)), c.z)))
}

/// Vehicle.towAim and Vehicles.launchTow choose the first closest leg.
fn nearest_leg(legs: &[TowLeg], matrix: &Matrix4, origin: &Vector3) -> Option<(usize, Vector3)> {
    let mut best = None;
    let mut span = f64::INFINITY;
    for (i, leg) in legs.iter().enumerate() {
        let mut point = leg.calf.or(leg.thigh).unwrap();
        point.apply_matrix4(matrix);
        let d = point.distance_to_squared(origin);
        if d < span { span = d; best = Some((i, point)); }
    }
    best
}

/// Vehicles.updateTethers: opposite turns unwind, and three circuits trip.
fn winding(previous: f64, accumulated: f64, source: &Vector3, target: &Vector3) -> (f64, f64, bool) {
    let around = v8math::atan2(source.x - target.x, source.z - target.z);
    let wound = accumulated + angle(around - previous);
    (around, wound, v8math::abs(wound) >= 6.0 * std::f64::consts::PI)
}

/// lockSpan's largest cosine on a box edge (hud-units.js nearestCosine).
fn nearest_cosine(a: &Vector3, e: &Vector3, d: &Vector3) -> f64 {
    let a0 = a.dot(d); let a1 = e.dot(d);
    let qa = e.length_sq(); let qb = 2.0 * a.dot(e); let qc = a.length_sq();
    let at = |s: f64| (a0 + a1 * s) / v8math::sqrt(v8math::max(qa * s * s + qb * s + qc, 1e-12));
    let mut best = v8math::max(at(0.0), at(1.0));
    let denominator = a1 * qb / 2.0 - a0 * qa;
    if v8math::abs(denominator) > 1e-12 {
        let s = (a0 * qb / 2.0 - a1 * qc) / denominator;
        if s > 0.0 && s < 1.0 { best = v8math::max(best, at(s)); }
    }
    best
}

/// hud-units.js lockSpan: hulls offer their box, units their lazy sphere.
pub(crate) fn box_span(origin: &Vector3, direction: &Vector3, matrix: &Matrix4, lo: &Vector3, hi: &Vector3) -> (f64, f64) {
    let mut inverse = *matrix; inverse.invert();
    let mut o = *origin; o.apply_matrix4(&inverse);
    let mut d = *direction; d.transform_direction(&inverse);
    let near = Vector3::new(three::clamp(o.x, lo.x, hi.x), three::clamp(o.y, lo.y, hi.y), three::clamp(o.z, lo.z, hi.z));
    let distance = near.distance_to(&o);
    let os = [o.x, o.y, o.z]; let ds = [d.x, d.y, d.z];
    let lows = [lo.x, lo.y, lo.z]; let highs = [hi.x, hi.y, hi.z];
    let mut enter = 0.0; let mut leave = f64::INFINITY;
    for k in 0..3 {
        if v8math::abs(ds[k]) < 1e-9 {
            if os[k] < lows[k] || os[k] > highs[k] { leave = -1.0; break; }
            continue;
        }
        let t0 = (lows[k] - os[k]) / ds[k]; let t1 = (highs[k] - os[k]) / ds[k];
        enter = v8math::max(enter, v8math::min(t0, t1));
        leave = v8math::min(leave, v8math::max(t0, t1));
        if enter > leave { break; }
    }
    if enter <= leave { return (0.0, distance); }
    let corner = |i: usize| Vector3::new(if i & 1 != 0 { hi.x } else { lo.x }, if i & 2 != 0 { hi.y } else { lo.y }, if i & 4 != 0 { hi.z } else { lo.z });
    let mut best = -1.0;
    for i in 0..8 { for bit in [1, 2, 4] {
        if i & bit != 0 { continue; }
        let mut a = corner(i); a.sub(&o);
        let mut e = corner(i | bit); e.sub(&o).sub(&a);
        best = v8math::max(best, nearest_cosine(&a, &e, &d));
    }}
    (v8math::acos(v8math::min(1.0, best)), distance)
}

pub(crate) fn sphere_span(origin: &Vector3, direction: &Vector3, center: &Vector3, radius: f64) -> (f64, f64) {
    let mut d = *center; d.sub(origin);
    let span = d.length(); let distance = v8math::max(0.0, span - radius);
    if span <= radius { return (0.0, distance); }
    (v8math::max(0.0, v8math::acos(three::clamp(d.dot(direction) / span, -1.0, 1.0)) - v8math::asin(radius / span)), distance)
}

/// Vehicle.fireSeat and TurretSeats.drive use the same mounted Weapon.update
/// admission and standing aim. Each caller keeps its own ordered draw stream.
#[allow(clippy::too_many_arguments)]
pub fn update_mounted(weapon: &mut crate::gen::Weapon, index: u32, class: &crate::combat::defs::WeaponClass, dt: f64,
    held: bool, pressed: bool, reload: bool, draws: &mut impl crate::combat::weapons::WeaponDraws, shots: &mut Vec<crate::combat::weapons::Shot>) {
    use crate::combat::weapons::{Aim, Trigger};
    shots.clear();
    weapon.update(index, class, dt, true, &Trigger { held, pressed, seed: None, late: 0.0 }, reload,
        &Aim { stick: 0.0, posture: Some(crate::gen::Posture::Stand), zoomed: false, repair_target: false }, draws, shots);
}

mod targeting;
pub use targeting::*;
pub(crate) mod live;
pub use live::*;
pub(crate) use live::State;

#[cfg(test)]
mod tests {
    use super::*;
    fn leg(x: f64) -> TowLeg { TowLeg { name: String::new(), calf: Some(Vector3::new(x, 3.0, 0.0)), thigh: Some(Vector3::new(x, 6.0, 0.0)), foot: Some(Vector3::new(x, 0.0, 0.0)) } }
    #[test]
    fn cable_hits_only_legs_and_keeps_first_equal_column() {
        let legs = [leg(-2.0), leg(2.0)]; let frame = Matrix4::default();
        assert!(leg_at(&legs, 7.0, -1.0, &frame, &Vector3::new(0.0, 8.0, 0.0)).is_none());
        assert_eq!(leg_at(&legs, 7.0, -1.0, &frame, &Vector3::new(0.0, -2.0, 0.0)), Some((0, Vector3::new(-2.0, 0.0, 0.0))));
        assert_eq!(nearest_leg(&legs, &frame, &Vector3::default()).unwrap().0, 0);
    }
    #[test]
    fn winding_wraps_and_unwinds_before_three_circuits() {
        let target = Vector3::default(); let source = Vector3::new(0.0, 0.0, -10.0);
        let (_, wound, trip) = winding(-3.0, 0.0, &source, &target);
        assert!(wound < 0.0 && !trip);
        let pi = std::f64::consts::PI;
        assert!(winding(pi, 6.0 * pi, &source, &target).2);
        assert!(!winding(pi, 6.0 * pi - 0.01, &source, &target).2);
    }
    #[test]
    fn lock_uses_box_edges_and_sphere_surface() {
        let o = Vector3::default(); let forward = Vector3::new(0.0, 0.0, 1.0);
        let lo = Vector3::new(-1.0, -1.0, 9.0); let hi = Vector3::new(1.0, 1.0, 11.0);
        assert_eq!(box_span(&o, &forward, &Matrix4::default(), &lo, &hi), (0.0, 9.0));
        assert_eq!(sphere_span(&o, &forward, &Vector3::new(0.0, 0.0, 10.0), 1.0), (0.0, 9.0));
        let (angle, _) = box_span(&o, &Vector3::new(1.0, 0.0, 0.0), &Matrix4::default(), &lo, &hi);
        assert!(angle > 1.4 && angle < 1.5);
    }
}
