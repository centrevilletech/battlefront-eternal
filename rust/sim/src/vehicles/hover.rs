//! A hover hull on the ground (src/vehicle-hover.js): its physics before
//! the collision pass (`hoverPhysics`, `drive`, `hoverWithoutSprings`), its
//! springs' contacts during it (`hoverContacts`, `springContact`: the
//! search over the polygons near each spring, which the JavaScript ran
//! over the gather's triangles) and their forces after it (`hoverSprings`).
//! The JavaScript keeps its scratch at module level; each function here
//! keeps it on its own stack, the springs' compressions passed from the
//! contacts to the forces as the module's Float64Array carried them.

use super::hull::{Hull, HoverClass, Spring, DOWN, UP};
use super::{Polygon, VehicleWorld};
use crate::collision::spheres::closest_point_to_point;
use crate::collision::triangle_normal;
use three::{clamp, lerp, Quaternion, Vector3};
use v8math::{abs, hypot, max, min};

const GRAVITY: f64 = 9.8;
/// A spring meets ground within 60 degrees of world up and of the hull's up.
const SPRING_GROUND: f64 = 0.5;
/// Beyond its travel a contact also loses this share of the speed into it.
const SPRING_SKID: f64 = 0.1;
/// The look-ahead of a hull without springs, over this ground speed (m/s).
const LOOKAHEAD: f64 = 5.0;
/// The y added to its target up while it hovers.
const LEVEL_LIFT: f64 = 0.1;
/// The parser admits six AddSpringBody spheres at most.
pub const SPRINGS: usize = 6;

/// The hull's axes in its frame (`axes`): q, right, up, forward.
struct Axes {
    q: Quaternion,
    right: Vector3,
    up: Vector3,
    forward: Vector3,
}

fn axes(v: &Hull) -> Axes {
    let mut q = Quaternion::default();
    v.hull_quaternion(&mut q);
    let mut right = Vector3::new(1.0, 0.0, 0.0);
    right.apply_quaternion(&q);
    let mut up = Vector3::new(0.0, 1.0, 0.0);
    up.apply_quaternion(&q);
    let mut forward = Vector3::new(0.0, 0.0, 1.0);
    forward.apply_quaternion(&q);
    Axes { q, right, up, forward }
}

/// `drive`: the speed along forward toward the stick's, across toward the
/// strafe stick's, at the hull's rates times its grip.
#[allow(clippy::too_many_arguments)]
fn drive(v: &mut Hull, h: &HoverClass, forward: &Vector3, right: &Vector3, up_y: f64, fwd: f64, side: f64, brake: bool, dt: f64) {
    let throttle = max(abs(fwd), abs(side));
    let grip = max(up_y, 0.0) * v.support * dt;
    let rate = lerp(v.deceleration, v.acceleration, throttle) * (if brake { 2.0 } else { 1.0 }) * grip;
    let traction = h.traction * grip;
    let along = (if fwd >= 0.0 { v.speed } else { v.reverse_speed }) * fwd - v.velocity.dot(forward);
    v.velocity.add_scaled_vector(forward, clamp(along, -rate, rate));
    // +x is the hull's left: strafing right moves it along -x.
    let across = -v.strafe_speed * side - v.velocity.dot(right);
    v.velocity.add_scaled_vector(right, clamp(across, -traction, traction));
}

/// Box3.getCenter.
fn box_center(min: &Vector3, max: &Vector3) -> Vector3 {
    if max.x < min.x || max.y < min.y || max.z < min.z {
        return Vector3::new(0.0, 0.0, 0.0);
    }
    let mut c = Vector3::default();
    c.add_vectors(min, max).multiply_scalar(0.5);
    c
}

/// `hoverWithoutSprings`: a hull without springs hovers on a lift spring
/// over what is under its collision centre.
#[allow(clippy::too_many_arguments)]
fn hover_without_springs<W: VehicleWorld>(v: &mut Hull, h: &HoverClass, a: &Axes, w: &mut W, dt: f64, fwd: f64, side: f64, brake: bool) {
    let mut normal = UP;
    let f;
    let target;
    if h.altitude > 0.0 {
        let mut center = box_center(&v.bounds_min, &v.bounds_max);
        center.apply_quaternion(&a.q).add(&v.position);
        let lift = center.y - v.position.y;
        let reach = v.bound_radius + 10.0 * h.altitude;
        let mut height = reach - lift;
        if let Some(below) = w.cast(&center, &DOWN, reach, Some(v.body)) {
            height = below.distance - lift;
            normal = below.normal;
        }
        if w.water_over(center.x, center.z) && center.y - w.water_level() < height + lift {
            height = min(height, center.y - w.water_level() - lift);
            normal = UP;
        }
        if hypot(v.velocity.x, v.velocity.z) > LOOKAHEAD {
            let mut ray = Vector3::new(v.velocity.x, v.velocity.y - reach, v.velocity.z);
            let length = ray.length();
            ray.divide_scalar(length);
            if let Some(ahead) = w.cast(&center, &ray, length, Some(v.body)) {
                height = min(height, -ray.multiply_scalar(ahead.distance).dot(&ahead.normal));
                if ahead.normal.y > 0.0 {
                    normal.lerp(&ahead.normal, (1.0 - ahead.distance / length) * ahead.normal.y).normalize();
                }
            }
        }
        f = clamp(1.0 - (height - h.altitude) / (9.0 * h.altitude), 0.0, 1.0);
        v.velocity.y -= lerp(1.0, h.gravity_scale, f) * GRAVITY * dt;
        let push = (h.altitude - height) * h.lift_spring + GRAVITY * f - v.velocity.dot(&normal) * h.lift_damp;
        if push > 0.0 {
            v.velocity.add_scaled_vector(&normal, push * dt);
        }
        let mut t = Vector3::new(normal.x * f, normal.y + LEVEL_LIFT, normal.z * f);
        t.normalize();
        target = t;
    } else {
        // Resting on its body alone: the last tick's support.
        f = v.support;
        v.velocity.y -= lerp(1.0, h.gravity_scale, f) * GRAVITY * dt;
        target = UP;
    }
    v.support = f;
    let damp = 1.0 - min(1.0, h.level_damp * dt);
    let level = min(1.0, h.level_spring * dt);
    v.tilt_rate = v.tilt_rate * damp - level * a.forward.dot(&target);
    v.roll_rate = v.roll_rate * damp - level * a.right.dot(&target);
    let mut ground_forward = a.forward;
    ground_forward.add_scaled_vector(&normal, -a.forward.dot(&normal)).normalize();
    let mut ground_right = Vector3::default();
    ground_right.cross_vectors(&normal, &ground_forward).normalize();
    drive(v, h, &ground_forward, &ground_right, a.up.y, fwd, side, brake, dt);
}

/// `hoverPhysics`: one tick before the collision pass. fwd, side: the
/// drive and strafe sticks (0 when nobody drives); brake: the brake key.
pub fn hover_physics<W: VehicleWorld>(v: &mut Hull, w: &mut W, dt: f64, fwd: f64, side: f64, brake: bool) {
    let h = v.hover_class.take().expect("a hover's class");
    let a = axes(v);
    let side = if !(v.strafe_speed >= 1e-4) { 0.0 } else { side };
    if !h.springs.is_empty() {
        // Off the ground gravity is GravityScale times 9.8, on it 9.8.
        v.velocity.y -= lerp(h.gravity_scale, 1.0, v.support) * GRAVITY * dt;
        // LevelSpring tips the nose and wings back toward level by the
        // square of how far they point up or down.
        let level = min(1.0, h.level_spring * dt);
        v.tilt_rate -= level * a.forward.y * abs(a.forward.y);
        v.roll_rate -= level * a.right.y * abs(a.right.y);
        drive(v, &h, &a.forward, &a.right, a.up.y, fwd, side, brake, dt);
    } else {
        hover_without_springs(v, &h, &a, w, dt, fwd, side, brake);
    }
    v.tilt += v.tilt_rate * dt;
    v.roll += v.roll_rate * dt;
    let velocity = v.velocity;
    v.position.add_scaled_vector(&velocity, dt);
    // A class with no altitude (the Hailfire) stands only on this tick's contacts.
    if h.altitude <= 0.0 {
        v.support = 0.0;
    }
    v.hover_class = Some(h);
}

/// `springContact`: ground within 60 degrees of world up and of the hull's
/// up presses the spring in, up to its travel; past it any contact holds
/// the hull off and stops it moving in. Returns the spring's compression.
fn spring_contact(v: &mut Hull, spring: &Spring, compression: f64, up: &Vector3, n: &Vector3, depth: f64) -> f64 {
    let mut compression = compression;
    let along = n.dot(up);
    if along > SPRING_GROUND && n.y > SPRING_GROUND {
        compression = max(compression, min(along * depth, spring.length) / spring.length);
    }
    if depth > spring.length {
        v.position.add_scaled_vector(n, depth - spring.length);
        let into = v.velocity.dot(n);
        if into < 0.0 && v.alive {
            v.velocity.add_scaled_vector(n, -into);
            v.velocity.x -= SPRING_SKID * into * n.x;
            v.velocity.z -= SPRING_SKID * into * n.z;
        }
    }
    compression
}

/// A spring's centre in the world: its place in the hull's frame q.
fn spring_center(spring: &Spring, q: &Quaternion, position: &Vector3) -> Vector3 {
    let mut c = spring.at;
    c.apply_quaternion(q).add(position);
    c
}

fn corners(t: &[Vector3; 3]) -> ([f64; 3], [f64; 3], [f64; 3]) {
    ([t[0].x, t[0].y, t[0].z], [t[1].x, t[1].y, t[1].z], [t[2].x, t[2].y, t[2].z])
}

/// `hoverContacts`: each spring against the terrain, then against the
/// polygons near it (the nearest it overlaps first, each once, three at
/// most; other hulls meet the wall spheres instead), then the water.
/// Returns the springs' compressions (the module's `s.compression`).
pub fn hover_contacts<W: VehicleWorld>(v: &mut Hull, w: &mut W) -> [f64; SPRINGS] {
    let h = v.hover_class.take().expect("a hover's class");
    let mut compressions = [0.0; SPRINGS];
    let mut polygons: Vec<Polygon> = Vec::new();
    // The module's scratch normal: set before every read below.
    let mut normal = Vector3::default();
    for (i, spring) in h.springs.iter().enumerate() {
        let r = spring.radius;
        let mut q = Quaternion::default();
        v.hull_quaternion(&mut q);
        let mut up = Vector3::new(0.0, 1.0, 0.0);
        up.apply_quaternion(&q);
        let mut compression = 0.0;
        let mut center = spring_center(spring, &q, &v.position);
        if let Some(ground) = w.ground(center.x, center.z, center.y + r) {
            let drop = ground.height - (center.y - r);
            if drop > 0.0 && drop <= 2.0 * r {
                normal = ground.normal;
                compression = spring_contact(v, spring, compression, &up, &normal, drop * normal.y);
                center = spring_center(spring, &q, &v.position);
            }
        }
        // The polygons near it, searched again from where each contact
        // leaves it (the gather's list holds until its next call).
        w.spring_polygons(v.index, i, &center, r, &mut polygons);
        let mut used: [usize; 3] = [usize::MAX; 3];
        let mut used_count = 0;
        for _pass in 0..3 {
            let mut best = r;
            let mut polygon: Option<usize> = None;
            let mut nearest = Vector3::default();
            for (k, p) in polygons.iter().enumerate() {
                if p.moving || used[..used_count].contains(&k) {
                    continue;
                }
                for tri in &p.triangles {
                    let (a, b, c) = corners(tri);
                    let ray = closest_point_to_point(a, b, c, [center.x, center.y, center.z]);
                    let ray = Vector3::new(ray[0], ray[1], ray[2]);
                    let d = ray.distance_to(&center);
                    if d >= best {
                        continue;
                    }
                    best = d;
                    polygon = Some(k);
                    nearest = ray;
                    if d < 1e-6 {
                        let n = triangle_normal(a, b, c);
                        normal = Vector3::new(n[0], n[1], n[2]);
                        let mut to = Vector3::default();
                        to.sub_vectors(&v.position, &nearest);
                        if normal.dot(&to) < 0.0 {
                            normal.negate();
                        }
                    }
                }
            }
            let Some(k) = polygon else { break };
            used[used_count] = k;
            used_count += 1;
            if best >= 1e-6 {
                normal.sub_vectors(&center, &nearest).divide_scalar(best);
            }
            compression = spring_contact(v, spring, compression, &up, &normal, r - best);
            center = spring_center(spring, &q, &v.position);
        }
        if w.water_over(center.x, center.z) {
            let wet = (w.water_level() - (center.y - r)) / spring.length;
            if wet > 0.0 {
                compression = max(compression, min(wet, 1.0));
                if wet > 1.0 && v.velocity.y < 0.0 {
                    v.velocity.y *= 0.5;
                    v.velocity.x *= 0.9;
                    v.velocity.z *= 0.9;
                    v.tilt_rate *= 0.9;
                    v.roll_rate *= 0.9;
                    v.yaw_rate *= 0.9;
                }
            }
        }
        compressions[i] = compression;
    }
    v.hover_class = Some(h);
    compressions
}

/// `hoverSprings`: each pressed spring lifts the hull and turns it about
/// its pitch and roll axes, and puts it on the ground; then the ground
/// factor loses a second's worth.
pub fn hover_springs(v: &mut Hull, dt: f64, compressions: &[f64; SPRINGS]) {
    let h = v.hover_class.take().expect("a hover's class");
    let mut q = Quaternion::default();
    v.hull_quaternion(&mut q);
    let mut up = Vector3::new(0.0, 1.0, 0.0);
    up.apply_quaternion(&q);
    for (i, spring) in h.springs.iter().enumerate() {
        let compression = compressions[i];
        if !(compression > 0.0) {
            continue;
        }
        let k = &spring.factors;
        let slack = 1.0 - compression;
        v.support = 1.0;
        let lift = k[0] * compression * h.velocity_spring - k[1] * slack * h.velocity_damp * min(v.velocity.dot(&up), 0.0);
        v.velocity.add_scaled_vector(&up, lift * dt);
        // The original game's pitch axis points the nose down; tilt is nose up.
        v.tilt_rate += (-k[2] * compression * h.omega_x_spring - k[3] * slack * h.omega_x_damp * v.tilt_rate) * dt;
        v.roll_rate += (k[4] * compression * h.omega_z_spring - k[5] * slack * h.omega_z_damp * v.roll_rate) * dt;
    }
    v.support = max(0.0, v.support - dt);
    v.hover_class = Some(h);
}
