//! Small numeric helpers the navigation code shares (nav/math.js), and
//! the plain {x, y, z} point the JavaScript's `vec()` makes. Every
//! transcendental and rounding comes from `v8math`, JavaScript's Math as
//! the production server's V8 computes it.

/// `Math.fround` (math.js:9).
pub fn f32(x: f64) -> f64 {
    v8math::fround(x)
}

/// math.js:12: "zero length" for points and segments.
pub fn tiny() -> f64 {
    f32(0.0001)
}

/// math.js:13: direction / goal-equality threshold.
pub fn tiny_dir() -> f64 {
    f32(9.999999e-9)
}

/// math.js:14: 0.2 m squared.
pub fn near_waypoint() -> f64 {
    f32(0.040000003)
}

/// math.js:15.
pub const FLT_MAX: f64 = 3.4028234663852886e38;

/// A point or vector as the navigation code keeps it (math.js:70 `vec`,
/// and the {x, y, z} fields of three.js Vector3s it reads).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Vec3 { x, y, z }
    }
}

/// Horizontal squared length of (ax - bx, az - bz) (math.js:18).
pub fn h2(ax: f64, az: f64, bx: f64, bz: f64) -> f64 {
    let dx = ax - bx;
    let dz = az - bz;
    dx * dx + dz * dz
}

/// Full 3D squared distance (math.js:25).
pub fn d2(a: Vec3, b: Vec3) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    dx * dx + dy * dy + dz * dz
}

/// Componentwise exact equality (math.js:33). `-0 === 0` is true and
/// NaN is unequal to itself, as with f64 `==`.
pub fn same(a: Vec3, b: Vec3) -> bool {
    a.x == b.x && a.y == b.y && a.z == b.z
}

/// Normalize in place; a zero vector stays zero (math.js:38).
pub fn normalize(v: &mut Vec3) {
    let l = v8math::sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
    if l > 0.0 {
        v.x /= l;
        v.y /= l;
        v.z /= l;
    }
}

/// The horizontal unit direction; a zero horizontal vector becomes
/// (1, 0, 0) (math.js:50).
pub fn flat_dir(x: f64, z: f64) -> Vec3 {
    let l = v8math::sqrt(x * x + z * z);
    if l > 0.0 {
        Vec3::new(x / l, 0.0, z / l)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    }
}

/// Lowest set bit's index; 0 for 0 (math.js:75). The argument is a
/// JavaScript number first reduced by `>>>= 0`.
pub fn lowest_bit(mask: f64) -> f64 {
    let mask = v8math::to_uint32(mask);
    if mask == 0 {
        return 0.0;
    }
    // 31 - Math.clz32(mask & -mask): the negation is int32's.
    31.0 - v8math::clz32((mask & mask.wrapping_neg()) as i32 as f64)
}
