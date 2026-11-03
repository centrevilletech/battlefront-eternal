//! `MathUtils.js` (r170), function by function. The UUID table and
//! the `Math.random` helpers take their draws from the caller (see
//! the crate notes).

use jsrt::conv::{imul, js_max, js_min, js_round, to_int32};
use jsrt::v8math;

use crate::euler::ProperEulerOrder;
use crate::quaternion::Quaternion;

/// `MathUtils.DEG2RAD`: `Math.PI / 180`, as a double.
pub const DEG2RAD: f64 = core::f64::consts::PI / 180.0;
/// `MathUtils.RAD2DEG`: `180 / Math.PI`.
pub const RAD2DEG: f64 = 180.0 / core::f64::consts::PI;

/// Which typed array `normalize`/`denormalize` scale for
/// (`array.constructor` in three).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentType {
    Float32,
    Uint32,
    Uint16,
    Uint8,
    Int32,
    Int16,
    Int8,
}

/// `generateUUID()`: four draws of `Math.random() * 0xffffffff | 0`
/// spelled through three's byte table. `random` is the draw source
/// (a `Math.random` in three).
pub fn generate_uuid(random: &mut impl FnMut() -> f64) -> String {
    fn lut(i: i32) -> String {
        format!("{:02x}", i as u8)
    }
    let d0 = to_int32(random() * 4294967295.0);
    let d1 = to_int32(random() * 4294967295.0);
    let d2 = to_int32(random() * 4294967295.0);
    let d3 = to_int32(random() * 4294967295.0);
    let uuid = format!(
        "{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}",
        lut(d0 & 0xff),
        lut((d0 >> 8) & 0xff),
        lut((d0 >> 16) & 0xff),
        lut((d0 >> 24) & 0xff),
        lut(d1 & 0xff),
        lut((d1 >> 8) & 0xff),
        lut(((d1 >> 16) & 0x0f) | 0x40),
        lut((d1 >> 24) & 0xff),
        lut((d2 & 0x3f) | 0x80),
        lut((d2 >> 8) & 0xff),
        lut((d2 >> 16) & 0xff),
        lut((d2 >> 24) & 0xff),
        lut(d3 & 0xff),
        lut((d3 >> 8) & 0xff),
        lut((d3 >> 16) & 0xff),
        lut((d3 >> 24) & 0xff),
    );
    uuid.to_lowercase()
}

/// `clamp(value, min, max)`: `Math.max(min, Math.min(max, value))`,
/// with JavaScript's NaN and signed-zero rules.
#[inline(always)]
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    js_max(min, js_min(max, value))
}

/// `euclideanModulo(n, m)`: `((n % m) + m) % m`, the `%` being
/// JavaScript's truncated remainder.
pub fn euclidean_modulo(n: f64, m: f64) -> f64 {
    ((n % m) + m) % m
}

/// `mapLinear(x, a1, a2, b1, b2)`.
pub fn map_linear(x: f64, a1: f64, a2: f64, b1: f64, b2: f64) -> f64 {
    b1 + (x - a1) * (b2 - b1) / (a2 - a1)
}

/// `inverseLerp(x, y, value)`.
pub fn inverse_lerp(x: f64, y: f64, value: f64) -> f64 {
    if x != y {
        (value - x) / (y - x)
    } else {
        0.0
    }
}

/// `lerp(x, y, t)`: `(1 - t) * x + t * y`, spelled that way (it is
/// not `x + (y - x) * t`).
pub fn lerp(x: f64, y: f64, t: f64) -> f64 {
    (1.0 - t) * x + t * y
}

/// `damp(x, y, lambda, dt)`.
pub fn damp(x: f64, y: f64, lambda: f64, dt: f64) -> f64 {
    lerp(x, y, 1.0 - v8math::exp(-lambda * dt))
}

/// `pingpong(x, length)`.
pub fn pingpong(x: f64, length: f64) -> f64 {
    length - (euclidean_modulo(x, length * 2.0) - length).abs()
}

/// `smoothstep(x, min, max)`.
pub fn smoothstep(x: f64, min: f64, max: f64) -> f64 {
    if x <= min {
        return 0.0;
    }
    if x >= max {
        return 1.0;
    }
    let x = (x - min) / (max - min);
    x * x * (3.0 - 2.0 * x)
}

/// `smootherstep(x, min, max)`.
pub fn smootherstep(x: f64, min: f64, max: f64) -> f64 {
    if x <= min {
        return 0.0;
    }
    if x >= max {
        return 1.0;
    }
    let x = (x - min) / (max - min);
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

/// `randInt(low, high)` with the draw handed in.
pub fn rand_int(random: &mut impl FnMut() -> f64, low: f64, high: f64) -> f64 {
    low + (random() * (high - low + 1.0)).floor()
}

/// `randFloat(low, high)` with the draw handed in.
pub fn rand_float(random: &mut impl FnMut() -> f64, low: f64, high: f64) -> f64 {
    low + random() * (high - low)
}

/// `randFloatSpread(range)` with the draw handed in.
pub fn rand_float_spread(random: &mut impl FnMut() -> f64, range: f64) -> f64 {
    range * (0.5 - random())
}

/// three's `seededRandom` (MathUtils.js): a mulberry32 over a
/// module-level `_seed` that starts at 1234567. The seed is a
/// JavaScript number and the `+= 0x6D2B79F5` is double arithmetic,
/// so past 2^53 of accumulated seed the low bits wash out exactly
/// as they do there; this keeps the state as an f64 to reproduce
/// that. (Not the game's `seededRandom` in weapons.js: that one
/// keeps its state in 32 bits. This is three's.)
#[derive(Clone, Copy, Debug)]
pub struct ThreeSeededRandom {
    seed: f64,
}

impl Default for ThreeSeededRandom {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreeSeededRandom {
    /// A fresh module state: `_seed = 1234567`.
    pub fn new() -> Self {
        ThreeSeededRandom { seed: 1234567.0 }
    }

    /// `seededRandom(s)`: with `s` given it reseeds first
    /// (`if (s !== undefined) _seed = s`), then draws.
    pub fn draw(&mut self, s: Option<f64>) -> f64 {
        if let Some(s) = s {
            self.seed = s;
        }
        self.seed += 1831565813.0; // 0x6D2B79F5, in double arithmetic
        let seed = self.seed;
        // t = Math.imul(t ^ t >>> 15, t | 1) with t = _seed (a
        // double): every bitwise step goes through ToInt32.
        let a = to_int32(seed);
        let t = imul((a ^ ((a as u32) >> 15) as i32) as f64, (1 | a) as f64);
        let ti = to_int32(t);
        // t ^= t + Math.imul(t ^ t >>> 7, t | 61)
        let add = imul((ti ^ ((ti as u32) >> 7) as i32) as f64, (61 | ti) as f64);
        let t2 = ti ^ to_int32(t + add);
        ((t2 ^ ((t2 as u32) >> 14) as i32) as u32) as f64 / 4294967296.0
    }
}

/// `degToRad(degrees)`.
pub fn deg_to_rad(degrees: f64) -> f64 {
    degrees * DEG2RAD
}

/// `radToDeg(radians)`.
pub fn rad_to_deg(radians: f64) -> f64 {
    radians * RAD2DEG
}

/// `isPowerOfTwo(value)`: `(value & (value - 1)) === 0 && value !== 0`
/// — the `&` is ToInt32 on both sides, in JavaScript's arithmetic
/// (`value - 1` in doubles first).
pub fn is_power_of_two(value: f64) -> bool {
    (to_int32(value) & to_int32(value - 1.0)) == 0 && value != 0.0
}

/// `ceilPowerOfTwo(value)`: `Math.pow(2, Math.ceil(Math.log(value) / Math.LN2))`.
pub fn ceil_power_of_two(value: f64) -> f64 {
    v8math::pow(2.0, (v8math::log(value) / core::f64::consts::LN_2).ceil())
}

/// `floorPowerOfTwo(value)`.
pub fn floor_power_of_two(value: f64) -> f64 {
    v8math::pow(2.0, (v8math::log(value) / core::f64::consts::LN_2).floor())
}

/// `setQuaternionFromProperEuler(q, a, b, c, order)`.
pub fn set_quaternion_from_proper_euler(
    q: &mut Quaternion,
    a: f64,
    b: f64,
    c: f64,
    order: ProperEulerOrder,
) {
    let c2 = v8math::cos(b / 2.0);
    let s2 = v8math::sin(b / 2.0);
    let c13 = v8math::cos((a + c) / 2.0);
    let s13 = v8math::sin((a + c) / 2.0);
    let c1_3 = v8math::cos((a - c) / 2.0);
    let s1_3 = v8math::sin((a - c) / 2.0);
    let c3_1 = v8math::cos((c - a) / 2.0);
    let s3_1 = v8math::sin((c - a) / 2.0);
    match order {
        ProperEulerOrder::XYX => q.set(c2 * s13, s2 * c1_3, s2 * s1_3, c2 * c13),
        ProperEulerOrder::YZY => q.set(s2 * s1_3, c2 * s13, s2 * c1_3, c2 * c13),
        ProperEulerOrder::ZXZ => q.set(s2 * c1_3, s2 * s1_3, c2 * s13, c2 * c13),
        ProperEulerOrder::XZX => q.set(c2 * s13, s2 * s3_1, s2 * c3_1, c2 * c13),
        ProperEulerOrder::YXY => q.set(s2 * c3_1, c2 * s13, s2 * s3_1, c2 * c13),
        ProperEulerOrder::ZYZ => q.set(s2 * s3_1, s2 * c3_1, c2 * s13, c2 * c13),
    };
}

/// `denormalize(value, array)`.
pub fn denormalize(value: f64, kind: ComponentType) -> f64 {
    match kind {
        ComponentType::Float32 => value,
        ComponentType::Uint32 => value / 4294967295.0,
        ComponentType::Uint16 => value / 65535.0,
        ComponentType::Uint8 => value / 255.0,
        ComponentType::Int32 => js_max(value / 2147483647.0, -1.0),
        ComponentType::Int16 => js_max(value / 32767.0, -1.0),
        ComponentType::Int8 => js_max(value / 127.0, -1.0),
    }
}

/// `normalize(value, array)`.
pub fn normalize(value: f64, kind: ComponentType) -> f64 {
    match kind {
        ComponentType::Float32 => value,
        ComponentType::Uint32 => js_round(value * 4294967295.0),
        ComponentType::Uint16 => js_round(value * 65535.0),
        ComponentType::Uint8 => js_round(value * 255.0),
        ComponentType::Int32 => js_round(value * 2147483647.0),
        ComponentType::Int16 => js_round(value * 32767.0),
        ComponentType::Int8 => js_round(value * 127.0),
    }
}
