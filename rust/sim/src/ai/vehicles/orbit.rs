//! Pattern 15's orbit, the AT-AT tow-cable run (src/ai/classic/vehicles/
//! orbit.js): from the orbit request a pattern leaves on the pilot's
//! control, the point on an ellipse round the walker the craft steers at,
//! the request's height share and movement scale, and whether the craft is
//! on the run's tow pass.

use three::{clamp, Matrix4, Vector3};
use v8math::{abs, acos, atan2, cos, hypot, sin};

use super::world::Orbit;

/// The literal (0, 7, 0) over the target's position (orbit.js:39).
const RISE: f64 = 7.0;

/// orbitResult() (orbit.js:103).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OrbitResult {
    pub point: Vector3,
    pub centre: Vector3,
    pub blend: f64,
    pub throttle: f64,
    pub tow_pass: bool,
}

/// squash(v, origin, axis, k, out) (orbit.js:53): v with its part along
/// `axis` (from `origin`) scaled by k.
fn squash(v: &Vector3, origin: &Vector3, axis: &Vector3, k: f64) -> Vector3 {
    let d = axis.x * (v.x - origin.x) + axis.y * (v.y - origin.y) + axis.z * (v.z - origin.z);
    let mut out = *v;
    out.add_scaled_vector(axis, d * (k - 1.0));
    out
}

/// `k || 1` for a number.
fn or_one(k: f64) -> f64 {
    if k == 0.0 || k.is_nan() {
        1.0
    } else {
        k
    }
}

/// orbitSteer(position, forward, hull, orbit, out) (orbit.js:61): the
/// craft's position and forward, the target hull's position and matrix.
pub fn orbit_steer(position: &Vector3, forward: &Vector3, hull_position: &Vector3, hull_matrix: &Matrix4, orbit: &Orbit, out: &mut OrbitResult) {
    let offset = orbit.offset;
    let rho = orbit.radius;
    let r = abs(rho);
    let k = or_one(orbit.factor);
    let side = rho > 0.0;
    let (mut x, mut y, mut z) = (Vector3::default(), Vector3::default(), Vector3::default());
    hull_matrix.extract_basis(&mut x, &mut y, &mut z);
    let c = &mut out.centre;
    c.copy(hull_position);
    c.y += RISE;
    c.add_scaled_vector(&x, offset.x).add_scaled_vector(&y, offset.y).add_scaled_vector(&z, offset.z);
    let c = *c;
    let mut a = z;
    a.normalize();
    let mut f = Vector3::new(forward.x, 0.0, forward.z);
    if f.length_sq() > 1e-8 {
        f.normalize();
    }
    // The tangent, in the squashed (circular) frame.
    let c2 = Vector3::new(c.x, 0.0, c.z);
    let p2 = squash(&Vector3::new(position.x, 0.0, position.z), &c2, &a, 1.0 / k);
    let dx = p2.x - c2.x;
    let dz = p2.z - c2.z;
    let l = hypot(dx, dz);
    let mut u = Vector3::default();
    if !(r > l) {
        let phi = atan2(dz, dx) + (if side { -1.0 } else { 1.0 }) * acos(if l > 0.0 { r / l } else { 1.0 });
        u.set(cos(phi), 0.0, sin(phi));
    } else if l > 1e-9 {
        u.set(dx / l, 0.0, dz / l);
    } else {
        u.set(1.0, 0.0, 0.0);
    }
    let mut t = c2;
    t.add_scaled_vector(&u, r);
    let t = squash(&t, &c2, &a, k);
    // The ellipse's normal there, and the point |radius| on across it.
    let mut n = squash(&u, &Vector3::default(), &a, 1.0 / k);
    n.y = 0.0;
    if n.length_sq() > 1e-12 {
        n.normalize();
    }
    let s = if side { 1.0 } else { -1.0 };
    out.point.set(t.x - s * r * -n.z, c.y, t.z - s * r * n.x);
    // The request's height share and movement scale.
    let t = if rho != 0.0 && !rho.is_nan() { hypot(c.x - position.x, c.z - position.z) / (k * rho) - 1.0 } else { 0.0 };
    out.blend = clamp(t / 3.0, 0.0, 1.0);
    out.throttle = clamp((t - 1.0) / 5.0, 0.0, 1.0);
    // The run's tow pass.
    let along = f.dot(&a) > 0.0;
    let ahead = a.x * (c.x - position.x) + a.y * (c.y - position.y) + a.z * (c.z - position.z) > 0.0;
    out.tow_pass = ((along != (offset.x > 0.0)) != side) && (ahead != side);
}
