//! A walker or riding animal on the ground (src/vehicles.js walkGround,
//! standOnLegs): it moves at its speeds in the plane, its wall probes push
//! it out of what it walks into, and its probes (an animal's, lying with
//! the ground) or its legs' rays find the floor under it.

use super::hull::{approach, truthy, Hull, Input, DOWN, UP};
use super::VehicleWorld;
use crate::rig::math;
use three::{clamp, Euler, EulerOrder, Quaternion, Vector3};
use v8math::{hypot, max, min};

/// vehicles.js walkGround.
pub fn walk_ground<W: VehicleWorld>(v: &mut Hull, w: &mut W, dt: f64, input: &Input, driving: bool, old_yaw: f64) {
    let forward = if driving { v.forward_input * (if v.forward_input >= 0.0 { v.speed } else { v.reverse_speed }) } else { 0.0 };
    let right = if driving && truthy(v.strafe_speed) { v.right_input * v.strafe_speed } else { 0.0 };
    let norm = max(1.0, hypot(v.forward_input, if truthy(v.strafe_speed) { v.right_input } else { 0.0 }));
    let sin = math::sin(v.yaw);
    let cos = math::cos(v.yaw);
    let accel = if truthy(forward) || truthy(right) { v.acceleration } else { v.deceleration * (if input.brake { 2.0 } else { 1.0 }) };
    // +Z is forward in the assets; screen-right is -X in that frame.
    v.velocity.x = approach(v.velocity.x, (sin * forward - cos * right) / norm, accel * dt);
    v.velocity.z = approach(v.velocity.z, (cos * forward + sin * right) / norm, accel * dt);
    let legs = if v.creature { None } else { v.legs.take() };
    if !v.creature && legs.is_none() {
        v.roll -= v.roll * (1.0 - math::exp(-5.0 * dt));
        v.tilt -= v.tilt * (1.0 - math::exp(-4.0 * dt));
    }
    v.position.x += v.velocity.x * dt;
    v.position.z += v.velocity.z * dt;
    w.enable_body(v.body, false);
    v.quaternion.set_from_axis_angle(&UP, v.yaw);
    let q = v.quaternion;
    v.push_walls(w, old_yaw, &q, false);
    if let Some(legs) = legs {
        stand_on_legs(v, w, dt, &legs);
        v.legs = Some(legs);
        w.enable_body(v.body, v.alive);
        return;
    }
    // Several suspension probes keep the hull from climbing walls or flying
    // across a missing floor. Gravity takes over at ledges and map cuts.
    let mut floor = f64::NEG_INFINITY;
    v.support_normal.set(0.0, 0.0, 0.0);
    let mut supports = 0u32;
    let mut height_sum = 0.0;
    for p in 0..v.support_probes.len() {
        let mut from = v.support_probes[p];
        from.apply_quaternion(&v.quaternion).add(&v.position);
        from.y = v.position.y + v.bounds_min.y + 1.2;
        if let Some(hit) = w.cast(&from, &DOWN, 6.0, None) {
            if hit.normal.y > 0.65 {
                if v.creature {
                    // The contact plane projected back to the model origin.
                    height_sum += hit.point.y + (hit.normal.x * (hit.point.x - v.position.x) + hit.normal.z * (hit.point.z - v.position.z)) / hit.normal.y;
                    v.support_normal.add(&hit.normal);
                    supports += 1;
                } else {
                    floor = max(floor, hit.point.y);
                }
            }
        }
    }
    if supports > 0 {
        floor = height_sum / supports as f64;
        let mut normal = v.support_normal;
        normal.normalize();
        v.support_normal = normal;
        v.align_ground(&normal, 1.0 - math::exp(-14.0 * dt));
    }
    // A walker wades: the floor under the water holds it.
    if floor.is_finite() {
        let target = floor + if v.creature { 0.0 } else { -v.bounds_min.y + v.hover };
        if target > v.position.y + 1.1 {
            v.position.x = v.previous.x;
            v.position.z = v.previous.z;
            v.velocity.x = 0.0;
            v.velocity.z = 0.0;
        } else {
            v.position.y += (target - v.position.y) * (if v.creature { 1.0 } else { 1.0 - math::exp(-14.0 * dt) });
            v.velocity.y = 0.0;
        }
    } else {
        v.velocity.y -= 20.0 * dt;
        v.position.y += v.velocity.y * dt;
    }
    w.enable_body(v.body, v.alive);
}

/// vehicles.js standOnLegs: a walker on its legs, as the 2004 game stands
/// one (docs/vehicles.md "Standing on the ground").
pub fn stand_on_legs<W: VehicleWorld>(v: &mut Hull, w: &mut W, dt: f64, legs: &super::hull::Legs) {
    let l = legs.ray_length;
    let k = dt * 60.0;
    let mut q = Quaternion::default();
    q.set_from_euler(&Euler::new(-v.tilt, v.yaw, v.roll, EulerOrder::YXZ));
    let mut heights = std::mem::take(&mut v.support_heights); heights.clear();
    let created = v.leg_side.is_none();
    if created {
        v.leg_side = Some(vec![0.0; legs.pairs.len()]);
    }
    w.leg_side(v.index, v.leg_side.as_mut().unwrap(), created);
    for (i, pair) in legs.pairs.iter().enumerate() {
        let mut h = [0.0; 2];
        for (j, at) in pair.iter().enumerate() {
            let mut from = Vector3::new(at[0], at[1], at[2]);
            from.apply_quaternion(&q).add(&v.position);
            from.y += l;
            h[j] = match w.cast(&from, &DOWN, 2.0 * l, None) {
                Some(hit) => l - hit.distance,
                None => -l,
            };
        }
        let lo = min(h[0], h[1]);
        let hi = max(h[0], h[1]);
        heights.push(if legs.orient_roll { (h[0] + h[1]) / 2.0 } else { lo + (hi - lo) * clamp((hi - l / 2.0) / (0.4 * l), 0.0, 1.0) });
        let side = clamp(-0.6 * (h[0] - h[1]), -1.0, 1.0);
        let sides = v.leg_side.as_mut().unwrap();
        w.leg_side(v.index, sides, false);
        // A shared array can be shortened by a query callback. JavaScript
        // then reads undefined, adds to NaN, and extends it at this index.
        if sides.len() <= i { sides.resize(i + 1, f64::NAN); }
        sides[i] += (side - sides[i]) * min(1.0, 0.4 * k);
        w.leg_side(v.index, sides, true);
    }
    // The rays meet terrain and buildings, not water: a walker wades.
    if heights.len() >= 2 {
        // (A centimetre's grace: a foot exactly on flat ground reads 0.)
        if !heights.iter().any(|&h| h > -0.01) {
            v.velocity.y -= 29.4 * dt;
            v.position.y += v.velocity.y * dt;
            v.support_heights = heights; return;
        }
        v.velocity.y = 0.0;
        const MAX: f64 = 0.2955;
        v.tilt = clamp(v.tilt + 0.01 * k * (heights[0] - heights[heights.len() - 1]), -MAX, MAX);
        if legs.width > 1.0 {
            let sides = v.leg_side.as_ref().unwrap();
            let mut sum = 0.0;
            for s in sides {
                sum += s;
            }
            let side = sum / sides.len() as f64;
            v.roll += (clamp(math::atan(side / legs.width), -MAX, MAX) - v.roll) * min(1.0, 0.05 * k);
        }
        let rise = (max(heights[0], -0.1 * k) + max(heights[1], -0.1 * k)) / 2.0;
        v.position.y += rise;
        v.support_heights = heights; return;
    }
    // One pair (the AT-ST); none at all reads undefined, NaN here.
    let h = heights.first().copied().unwrap_or(f64::NAN);
    if h >= 0.0 {
        v.velocity.y = 0.0;
        v.position.y += h - 0.1;
    } else {
        v.velocity.y -= 29.4 * dt;
        v.position.y += max(v.velocity.y * dt, h);
    }
    v.support_heights = heights;
}
