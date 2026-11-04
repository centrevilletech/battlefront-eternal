//! The 2004 game's float arithmetic the AI's combat code shares
//! (src/ai/classic/aim.js:43-155): its interpolating sine/cosine tables,
//! the angle thresholds it takes from them, and its float32 vector helpers
//! in the original's operation order. Every rounding point is the
//! JavaScript's `Math.fround`; every transcendental is V8's (v8math).

use std::sync::OnceLock;

/// `Math.fround` (aim.js:43 `f`).
#[inline]
pub fn f(x: f64) -> f64 {
    v8math::fround(x)
}

/// FLT_MAX (perception.js:507, combat.js:51).
pub const FLT_MAX: f64 = 3.4028234663852886e38;

/// aim.js:50-51: 2pi / 2048 and 2048 / 2pi as float32.
fn table_step() -> f64 {
    f(0.003067961661145091)
}
fn table_scale() -> f64 {
    f(325.9493103027344)
}

/// aim.js:55 `tables()`: 2048 float32 samples of sin and cos at
/// float32(i * step), computed once (V8's sin and cos, stored into a
/// Float32Array).
struct Tables {
    sin: [f64; 2048],
    cos: [f64; 2048],
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut t = Tables { sin: [0.0; 2048], cos: [0.0; 2048] };
        let step = table_step();
        for i in 0..2048 {
            let a = f(i as f64 * step);
            t.sin[i] = f(v8math::sin(a));
            t.cos[i] = f(v8math::cos(a));
        }
        t
    })
}

/// aim.js:66 `lookup`: the index truncated toward zero, both neighbours
/// wrapped with `& 2047` (ToInt32 first, as JavaScript's bitwise and).
fn lookup(table: &[f64; 2048], angle: f64) -> f64 {
    let t = f(f(angle) * table_scale());
    let i = v8math::trunc(t);
    let frac = f(t - i);
    let a = (v8math::to_int32(i) & 2047) as usize;
    let b = (v8math::to_int32(i + 1.0) & 2047) as usize;
    f(f(f(1.0 - frac) * table[a]) + f(table[b] * frac))
}

/// aim.js:73.
pub fn lookup_cos(angle: f64) -> f64 {
    lookup(&tables().cos, angle)
}

/// aim.js:78.
pub fn lookup_sin(angle: f64) -> f64 {
    lookup(&tables().sin, angle)
}

/// aim.js:86 `ANGLE`: the thresholds the original takes from its tables.
pub mod angle {
    use super::f;
    /// 77.5 degrees: the sight cone and the facing cones.
    pub fn cone() -> f64 {
        f(1.3526301383972168)
    }
    /// pi/8: the firing cone.
    pub fn fire() -> f64 {
        f(std::f64::consts::PI / 8.0)
    }
    /// pi/4: the melee firing cone (and the evade's 45 degrees).
    pub fn melee() -> f64 {
        f(std::f64::consts::PI / 4.0)
    }
    /// pi/10 (attack pattern 4).
    pub fn face_tight() -> f64 {
        f(0.3141592741012573)
    }
    /// pi/3 (attack pattern 5).
    pub fn face_wide() -> f64 {
        f(1.0471975803375244)
    }
}

/// aim.js:98 `dot32`: y*y first, then x*x, then z*z, each step float32.
#[inline]
pub fn dot32(ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64) -> f64 {
    f(f(f(ay * by) + f(ax * bx)) + f(az * bz))
}

/// aim.js:103 `lengthSq32`: (x*x + y*y) + z*z.
#[inline]
pub fn length_sq32(x: f64, y: f64, z: f64) -> f64 {
    f(f(f(x * x) + f(y * y)) + f(z * z))
}

/// aim.js:108 `lengthSqXZ32`.
#[inline]
pub fn length_sq_xz32(x: f64, z: f64) -> f64 {
    f(f(x * x) + f(z * z))
}

/// A plain {x, y, z} the helpers write (aim.js's `out` objects).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl P3 {
    pub fn new(x: f64, y: f64, z: f64) -> P3 {
        P3 { x, y, z }
    }
}

/// aim.js:117 `normalizeD3DX`: below 2^-46 squared length the result is
/// zero; otherwise one Newton step on float32(1 / float32(sqrt s)).
pub fn normalize_d3dx(x: f64, y: f64, z: f64) -> P3 {
    let s = length_sq32(x, y, z);
    if !(s >= 1.4210854715202004e-14) {
        return P3::new(0.0, 0.0, 0.0);
    }
    let y0 = f(1.0 / f(v8math::sqrt(s)));
    let t = f(f(y0 * s) * y0);
    let r = f(f(3.0 - t) * f(y0 * 0.5));
    P3::new(f(x * r), f(y * r), f(z * r))
}

/// aim.js:135 `normalizeOrX`: exactly zero (either sign) becomes (1, 0, 0).
pub fn normalize_or_x(x: f64, y: f64, z: f64) -> P3 {
    if x == 0.0 && y == 0.0 && z == 0.0 {
        return P3::new(1.0, 0.0, 0.0);
    }
    normalize_d3dx(x, y, z)
}

/// aim.js:147 `normalizeXZ`: the horizontal direction (y = 0); a zero
/// horizontal vector becomes (1, 0, 0).
pub fn normalize_xz(x: f64, z: f64) -> P3 {
    if x == 0.0 && z == 0.0 {
        return P3::new(1.0, 0.0, 0.0);
    }
    normalize_d3dx(x, 0.0, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_wrap_like_javascript() {
        // A negative angle reads the wrapped samples (ToInt32 & 2047).
        let a = lookup_cos(-0.5);
        assert!(a.is_finite());
        assert_eq!(lookup_cos(0.0), 1.0);
    }

    #[test]
    fn zero_directions() {
        assert_eq!(normalize_or_x(0.0, -0.0, 0.0), P3::new(1.0, 0.0, 0.0));
        assert_eq!(normalize_xz(-0.0, 0.0), P3::new(1.0, 0.0, 0.0));
        assert_eq!(normalize_d3dx(1e-8, 0.0, 0.0), P3::new(0.0, 0.0, 0.0));
    }
}
