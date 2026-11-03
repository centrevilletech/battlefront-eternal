//! What rounds and sight lines meet on a soldier: his hit shapes, as
//! src/units.js builds them each tick from his posture (or a droideka's
//! from its ball and its upright cylinder), the broadphase sphere about
//! them, and src/registry.js's segment tests against each kind of shape.
//!
//! Each function is the JavaScript's operation by operation, in f64, in
//! the same order; Math.min, Math.max and Math.hypot are V8's (`v8math`),
//! and so are the sines and cosines a box's turn takes.

use three::{clamp, Vector3};

pub use crate::gen::{Bounds, Half, HitShape, ShapePart, ShapeType};

/// `[x, y, z]` as a Vector3, and back (the store keeps vectors as arrays).
#[inline(always)]
pub fn v3(a: [f64; 3]) -> Vector3 {
    Vector3 { x: a[0], y: a[1], z: a[2] }
}
#[inline(always)]
pub fn arr(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

impl HitShape {
    /// `half` as a number (a cylinder's half height; a sphere's 0).
    pub fn half_number(&self) -> f64 {
        match self.half {
            Half::F64(x) => x,
            // `-half` of a Vector3 is NaN: no shape of a kind that reads it
            // as a number is ever given a vector.
            Half::Vec3(_) => f64::NAN,
        }
    }
    /// `half` as a Vector3 (a box's or a capsule's half size).
    pub fn half_vector(&self) -> Vector3 {
        match self.half {
            Half::Vec3(v) => v3(v),
            Half::F64(_) => Vector3 { x: f64::NAN, y: f64::NAN, z: f64::NAN },
        }
    }
}

/// One of soldierShapes' parts (units.js:64): where it sits in his frame
/// (x his left, y up, z forward), its kind and size.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub kind: ShapeType,
    pub part: ShapePart,
    pub at: [f64; 3],
    /// `part.radius || 0` as the shape takes it.
    pub radius: f64,
    /// A cylinder's half height (`part.half || 0`).
    pub half: f64,
    /// A box's half sizes (`part.half`, an array).
    pub half_box: [f64; 3],
}

/// What a posture's shapes are: units.js `hitPosture`'s three answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitPosture {
    Stand,
    Crouch,
    Prone,
}

/// soldierShapes(posture, s) (units.js:64): an upright cylinder for the body
/// standing or crouched (a box lying prone) and a sphere for the head. `s`
/// is the class's CollisionRootScale.
pub fn soldier_shapes(posture: HitPosture, s: f64) -> [Part; 2] {
    let big = v8math::max(s, 1.0);
    let lift = 0.17 * (big - 1.0);
    let cylinder = |at: [f64; 3], radius: f64, half: f64| Part { kind: ShapeType::Cylinder, part: ShapePart::Body, at, radius, half, half_box: [0.0; 3] };
    let head = |at: [f64; 3], radius: f64| Part { kind: ShapeType::Sphere, part: ShapePart::Head, at, radius, half: 0.0, half_box: [0.0; 3] };
    match posture {
        HitPosture::Prone => [
            Part { kind: ShapeType::Box, part: ShapePart::Body, at: [0.0, 0.2, -0.25 * s], radius: 0.0, half: 0.0, half_box: [0.25, 0.2, 0.87 * s] },
            head([-0.07, 0.28 * s, 0.75 * s], 0.17 * big),
        ],
        HitPosture::Crouch => [cylinder([0.0, 0.6 * s + lift, 0.0], 0.35, 0.45 * s), head([-0.12, 1.2 * s, 0.25], 0.17 * big)],
        HitPosture::Stand => [cylinder([0.0, 0.8 * s + lift, 0.0], 0.35, 0.775 * s), head([-0.09, 1.65 * s, 0.16], 0.17 * big)],
    }
}

// The droideka's OrdnanceCollision primitives (units.js:62): its ball of
// 0.65 m, and deployed the upright cylinder ahead of its root.
pub const DROIDEKA_BALL: f64 = 0.65;
pub const DROIDEKA_ORD_AT: [f64; 3] = [0.0, 1.0, 0.506];
pub const DROIDEKA_ORD_RADIUS: f64 = 0.8;
pub const DROIDEKA_ORD_HALF: f64 = 1.0;

/// How far a shape reaches from its centre (boundShapes' `reach`).
fn reach(shape: &HitShape) -> f64 {
    match shape.r#type {
        ShapeType::Box => v3(shape.half_size).length(),
        ShapeType::Cylinder => v8math::hypot(shape.radius, shape.half_number()),
        ShapeType::Capsule => v3(shape.half_size).length() + shape.radius,
        ShapeType::Sphere => shape.radius,
    }
}

/// boundShapes(shapes, count, bounds) (units.js:95): about the middle of
/// the first `count` shapes' centres, reaching past the farthest.
pub fn bound_shapes(shapes: &[HitShape], count: usize, bounds: &mut Bounds) {
    let mut c = Vector3::default();
    for shape in &shapes[..count] {
        c.add(&v3(shape.center));
    }
    c.divide_scalar(count as f64);
    bounds.center = arr(&c);
    let mut r = 0.0;
    for shape in &shapes[..count] {
        r = v8math::max(r, c.distance_to(&v3(shape.center)) + reach(shape));
    }
    bounds.radius = r;
}

/// segmentShape(origin, dir, far, shape) (registry.js): the entry distance
/// along a ray into a shape, or None.
pub fn segment_shape(origin: &Vector3, dir: &Vector3, far: f64, shape: &HitShape) -> Option<f64> {
    let center = v3(shape.center);
    match shape.r#type {
        ShapeType::Sphere => segment_sphere(origin, dir, far, &center, shape.radius),
        // A cylinder's `half` is its half height; a box's and a capsule's,
        // the vector halfSize is.
        ShapeType::Cylinder => segment_cylinder(origin, dir, far, &center, shape.radius, shape.half_number()),
        ShapeType::Box => segment_box(origin, dir, far, &center, &shape.half_vector(), shape.yaw),
        ShapeType::Capsule => segment_capsule(origin, dir, far, &center, &shape.half_vector(), shape.radius),
    }
}

/// segmentSphere (registry.js): the first distance in [0, far] at which a
/// ray from `origin` along unit `dir` is inside the sphere, or None. A ray
/// starting inside hits at 0.
pub fn segment_sphere(origin: &Vector3, dir: &Vector3, far: f64, center: &Vector3, radius: f64) -> Option<f64> {
    let mut to_center = Vector3::default();
    to_center.sub_vectors(center, origin);
    let c = to_center.length_sq() - radius * radius;
    if c <= 0.0 {
        return Some(0.0);
    }
    let b = to_center.dot(dir);
    if b < 0.0 {
        return None;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = b - disc.sqrt();
    if t <= far {
        Some(t)
    } else {
        None
    }
}

/// segmentCylinder (registry.js): the entry distance into an upright
/// cylinder, or None; 0 from inside.
pub fn segment_cylinder(origin: &Vector3, dir: &Vector3, far: f64, center: &Vector3, radius: f64, half: f64) -> Option<f64> {
    let mut near = 0.0;
    let mut far_t = far;
    let oy = origin.y - center.y;
    if dir.y.abs() < 1e-12 {
        if oy.abs() > half {
            return None;
        }
    } else {
        let mut t1 = (-half - oy) / dir.y;
        let mut t2 = (half - oy) / dir.y;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        near = v8math::max(near, t1);
        far_t = v8math::min(far_t, t2);
        if near > far_t {
            return None;
        }
    }
    let ox = origin.x - center.x;
    let oz = origin.z - center.z;
    let a = dir.x * dir.x + dir.z * dir.z;
    let c = ox * ox + oz * oz - radius * radius;
    if a < 1e-12 {
        return if c > 0.0 { None } else { Some(near) };
    }
    let b = ox * dir.x + oz * dir.z;
    let disc = b * b - a * c;
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    near = v8math::max(near, (-b - root) / a);
    far_t = v8math::min(far_t, (-b + root) / a);
    if near <= far_t {
        Some(near)
    } else {
        None
    }
}

/// segmentBox (registry.js): the entry distance into a box turned by `yaw`
/// about the vertical, or None.
pub fn segment_box(origin: &Vector3, dir: &Vector3, far: f64, center: &Vector3, half: &Vector3, yaw: f64) -> Option<f64> {
    let sin = v8math::sin(yaw);
    let cos = v8math::cos(yaw);
    // Into the box's frame: its z along (sin yaw, 0, cos yaw), x along (cos yaw, 0, -sin yaw).
    let mut local = Vector3::default();
    local.sub_vectors(origin, center);
    let lx = local.x * cos - local.z * sin;
    let lz = local.x * sin + local.z * cos;
    let local_dir = Vector3 { x: dir.x * cos - dir.z * sin, y: dir.y, z: dir.x * sin + dir.z * cos };
    let o = [lx, local.y, lz];
    let d = [local_dir.x, local_dir.y, local_dir.z];
    let h = [half.x, half.y, half.z];
    let mut near = 0.0;
    let mut far_t = far;
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if o[i].abs() > h[i] {
                return None;
            }
            continue;
        }
        let mut t1 = (-h[i] - o[i]) / d[i];
        let mut t2 = (h[i] - o[i]) / d[i];
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        near = v8math::max(near, t1);
        far_t = v8math::min(far_t, t2);
        if near > far_t {
            return None;
        }
    }
    Some(near)
}

/// segmentCapsule (registry.js): the entry distance into a capsule (the
/// segment from center - half to center + half, swept by the radius), or
/// None; 0 from inside.
pub fn segment_capsule(origin: &Vector3, dir: &Vector3, far: f64, center: &Vector3, half: &Vector3, radius: f64) -> Option<f64> {
    let mut cap_end = Vector3::default();
    let mut cap_axis = Vector3::default();
    let mut cap_from = Vector3::default();
    cap_end.sub_vectors(center, half);
    cap_axis.copy(half).multiply_scalar(2.0);
    cap_from.sub_vectors(origin, &cap_end);
    let axis2 = cap_axis.length_sq();
    let axis_dir = cap_axis.dot(dir);
    let axis_from = cap_axis.dot(&cap_from);
    let from2 = cap_from.length_sq();
    // Inside: the origin within the radius of the nearest point of the axis.
    let s = if axis2 > 1e-12 { clamp(axis_from / axis2, 0.0, 1.0) } else { 0.0 };
    if from2 - 2.0 * s * axis_from + s * s * axis2 <= radius * radius {
        return Some(0.0);
    }
    let mut best: Option<f64> = None;
    let a = axis2 - axis_dir * axis_dir;
    if a > 1e-12 {
        // The side: the ray against the infinite cylinder, kept between the ends.
        let b = axis2 * dir.dot(&cap_from) - axis_from * axis_dir;
        let c = axis2 * from2 - axis_from * axis_from - radius * radius * axis2;
        let h = b * b - a * c;
        // Clear of the infinite cylinder, it is clear of the capsule inside it.
        if h < 0.0 {
            return None;
        }
        let t = (-b - h.sqrt()) / a;
        let y = axis_from + t * axis_dir;
        if y > 0.0 && y < axis2 && t >= 0.0 && t <= far {
            best = Some(t);
        }
    }
    // The rounded ends.
    let mut end = -1.0;
    while end <= 1.0 {
        cap_end.copy(center).add_scaled_vector(half, end);
        let t = segment_sphere(origin, dir, best.unwrap_or(far), &cap_end, radius);
        if let Some(t) = t {
            if best.is_none_or(|b| t < b) {
                best = Some(t);
            }
        }
        end += 2.0;
    }
    best
}

/// shapeNormal(point, shape, direction, out) (registry.js): the outward
/// normal of a shape at a point on it (the way the round came back, for a
/// point at its middle).
pub fn shape_normal(point: &Vector3, shape: &HitShape, direction: &Vector3) -> Vector3 {
    let mut out = Vector3::default();
    let center = v3(shape.center);
    if shape.r#type == ShapeType::Capsule {
        // From the nearest point of its axis.
        let half = shape.half_vector();
        let mut axis_point = Vector3::default();
        axis_point.sub_vectors(point, &center);
        let length = half.length_sq();
        let along = if length > 1e-12 { clamp(axis_point.dot(&half) / length, -1.0, 1.0) } else { 0.0 };
        axis_point.copy(&center).add_scaled_vector(&half, along);
        out.sub_vectors(point, &axis_point);
    } else {
        out.sub_vectors(point, &center);
    }
    if out.length_sq() < 1e-12 {
        out.copy(direction).negate();
    }
    out.normalize();
    out
}
