//! Number conversions and the algebraic `Math` functions, with
//! JavaScript's rules. Rust's `as` casts from floats saturate, its
//! `round` takes ties away from zero, and its `min`/`max` drop NaNs;
//! none of that may appear in ported sim code, so every `|0`, `>>>0`,
//! `Math.round`, `Math.min`, bitwise operand and `Math.imul` in the
//! JavaScript becomes a call here.

/// ECMAScript ToUint32 (`x >>> 0`, and the operand conversion of
/// unsigned shifts, `Math.clz32`, and friends): truncate toward zero,
/// then reduce modulo 2^32. NaN and the infinities become 0. Values
/// past 2^53 are already the rounded doubles JavaScript holds, and
/// `x % 4294967296` in f64 is exact for every double (the remainder
/// is computed exactly), so this matches V8 for every input.
#[inline(always)]
pub fn to_uint32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    let t = x.trunc();
    let m = t % 4294967296.0;
    let m = if m < 0.0 { m + 4294967296.0 } else { m };
    // m is an integer in [0, 2^32): the cast is exact.
    m as u32
}

/// ECMAScript ToInt32 (`x | 0`, and the operand conversion of every
/// other bitwise operator and `Math.imul`): ToUint32 read as signed.
#[inline(always)]
pub fn to_int32(x: f64) -> i32 {
    to_uint32(x) as i32
}

/// ToUint32's result as the f64 JavaScript goes on holding (`x >>> 0`
/// where the number, not the bits, is what flows on).
#[inline(always)]
pub fn uint32_as_f64(x: f64) -> f64 {
    to_uint32(x) as f64
}

/// ToInt32's result as an f64 (`x | 0` where the number flows on).
#[inline(always)]
pub fn int32_as_f64(x: f64) -> f64 {
    to_int32(x) as f64
}

/// ECMAScript ToIntegerOrInfinity: NaN becomes +0, the infinities stay,
/// everything else truncates toward zero (`Math.trunc`).
#[inline(always)]
pub fn to_integer_or_infinity(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x.trunc()
    }
}

/// ToUint8Clamp, the conversion a `Uint8ClampedArray` store performs:
/// clamp to [0, 255] with ties to even, NaN to 0.
pub fn to_uint8_clamp(x: f64) -> u8 {
    if x.is_nan() {
        return 0;
    }
    if x <= 0.0 {
        return 0;
    }
    if x >= 255.0 {
        return 255;
    }
    let f = x.floor();
    let r = if x - f > 0.5 {
        f + 1.0
    } else if x - f < 0.5 {
        f
    } else if (f as i64) % 2 == 0 {
        f
    } else {
        f + 1.0
    };
    r as u8
}

/// `Math.fround`: the nearest float32, held as a double. Rust's
/// `as f32` rounds the same way, subnormals and all.
#[inline(always)]
pub fn fround(x: f64) -> f64 {
    x as f32 as f64
}

/// `Math.imul(a, b)`: ToUint32 both sides, multiply modulo 2^32, read
/// as a signed 32-bit number held in a double.
#[inline(always)]
pub fn imul(a: f64, b: f64) -> f64 {
    (to_int32(a).wrapping_mul(to_int32(b))) as f64
}

/// `Math.clz32(x)`: leading zero bits of ToUint32(x).
#[inline(always)]
pub fn clz32(x: f64) -> f64 {
    to_uint32(x).leading_zeros() as f64
}

/// `Math.round`: the nearest integer, ties toward +infinity, and a
/// negative input that rounds to zero gives -0 (round(-0.5) is -0,
/// round(0.49999999999999994) is 0). Floors and the surviving
/// fraction are exact for every double, so comparing the fraction
/// against 1/2 decides as V8 does, without the `x + 0.5` pitfall.
#[inline(always)]
pub fn js_round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    let f = x.floor();
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    if r == 0.0 && x < 0.0 {
        -0.0
    } else {
        r
    }
}

/// `Math.floor` / `Math.ceil` / `Math.trunc` are Rust's own (exact in
/// both, -0 included) but are spelled through here so ported code
/// never mixes rules by accident.
#[inline(always)]
pub fn js_floor(x: f64) -> f64 {
    x.floor()
}

#[inline(always)]
pub fn js_ceil(x: f64) -> f64 {
    x.ceil()
}

#[inline(always)]
pub fn js_trunc(x: f64) -> f64 {
    x.trunc()
}

/// `Math.sign`: NaN stays NaN, +0 and -0 keep their signs.
#[inline(always)]
pub fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `Math.abs`: -0 becomes +0; NaN stays NaN.
#[inline(always)]
pub fn js_abs(x: f64) -> f64 {
    x.abs()
}

/// `Math.min(a, b)`: NaN if either is, and -0 below +0. As in
/// `rust/rays`; the sim port takes it from here.
#[inline(always)]
pub fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a < b {
        a
    } else if b < a {
        b
    } else if a == 0.0 && a.is_sign_negative() {
        a
    } else {
        b
    }
}

/// `Math.max(a, b)`: NaN if either is, and +0 above -0.
#[inline(always)]
pub fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a > b {
        a
    } else if b > a {
        b
    } else if a == 0.0 && a.is_sign_positive() {
        a
    } else {
        b
    }
}

/// `Math.min` over every argument JavaScript was given, left to right
/// (each step is the two-argument rule; no arguments gives +Infinity).
pub fn js_min_all(xs: &[f64]) -> f64 {
    let mut r = f64::INFINITY;
    for &x in xs {
        r = js_min(r, x);
    }
    r
}

/// `Math.max` over every argument (none gives -Infinity).
pub fn js_max_all(xs: &[f64]) -> f64 {
    let mut r = f64::NEG_INFINITY;
    for &x in xs {
        r = js_max(r, x);
    }
    r
}

/// `Math.hypot(...values)` as V8 computes it: the largest magnitude
/// divides every value, the squares are summed with Kahan's
/// compensation in argument order, and the root is scaled back. An
/// infinite magnitude wins over NaN; with none, a NaN anywhere gives
/// NaN. The two-argument form is the port already proven in
/// `rust/rays` (`movement::js_hypot2`, matched on 10^7 pairs); this
/// is V8's general algorithm, of which that is the two-value case.
/// `rust/v8math` will own this when it lands.
pub fn js_hypot(values: &[f64]) -> f64 {
    let mut max = 0.0f64;
    let mut any_nan = false;
    for &v in values {
        if v.is_nan() {
            any_nan = true;
        } else {
            let a = v.abs();
            if a > max {
                max = a;
            }
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if any_nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for &v in values {
        let n = v.abs() / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `Math.hypot(a, b)`.
#[inline(always)]
pub fn js_hypot2(a: f64, b: f64) -> f64 {
    js_hypot(&[a, b])
}

/// `Math.hypot(a, b, c)`.
#[inline(always)]
pub fn js_hypot3(a: f64, b: f64, c: f64) -> f64 {
    js_hypot(&[a, b, c])
}

/// JavaScript's `%` on doubles: the truncated remainder, sign of the
/// dividend. Rust's `f64 %` is the same operation; the name keeps
/// ported code from reaching for `rem_euclid`.
#[inline(always)]
pub fn js_mod(a: f64, b: f64) -> f64 {
    a % b
}

/// The f64 bit pattern of a value, for exact comparisons in tests and
/// hashes. JavaScript has no canonical NaN payload to preserve
/// through its arithmetic, and neither do we: comparisons that must
/// treat NaNs alike go through [`crate::same_value`].
#[inline(always)]
pub fn bits(x: f64) -> u64 {
    x.to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_follow_js() {
        assert_eq!(to_int32(0.0), 0);
        assert_eq!(to_int32(-0.0), 0);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(f64::INFINITY), 0);
        assert_eq!(to_int32(f64::NEG_INFINITY), 0);
        assert_eq!(to_int32(2147483647.0), 2147483647);
        assert_eq!(to_int32(2147483648.0), -2147483648);
        assert_eq!(to_int32(4294967295.0), -1);
        assert_eq!(to_int32(4294967296.0), 0);
        assert_eq!(to_int32(-1.5), -1);
        assert_eq!(to_int32(1.9), 1);
        assert_eq!(to_uint32(-1.0), 4294967295);
        assert_eq!(to_uint32(9007199254740992.0), 0); // 2^53
        assert_eq!(to_uint32(9007199254740994.0), 2); // 2^53 + 2
        // 2^53 + 1 rounds to 2^53 in a double before >>> ever sees it.
        assert_eq!(to_uint32(9007199254740993.0), 0);
        assert_eq!(to_int32(5e-324), 0);
    }

    #[test]
    fn round_follows_js() {
        assert_eq!(js_round(0.49999999999999994), 0.0);
        assert_eq!(js_round(0.5), 1.0);
        assert_eq!(js_round(-0.5).to_bits(), (-0.0f64).to_bits());
        assert_eq!(js_round(-1.5), -1.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-0.4).to_bits(), (-0.0f64).to_bits());
        assert_eq!(js_round(0.4).to_bits(), 0.0f64.to_bits());
        assert!(js_round(f64::NAN).is_nan());
        assert_eq!(js_round(f64::INFINITY), f64::INFINITY);
        assert_eq!(js_round(4503599627370497.0), 4503599627370497.0);
        // Round of a value just below a .5 boundary near 2^51.
        assert_eq!(js_round(2251799813685248.5), 2251799813685249.0);
    }

    #[test]
    fn min_max_sign_follow_js() {
        assert!(js_min(1.0, f64::NAN).is_nan());
        assert!(js_max(f64::NAN, 1.0).is_nan());
        assert_eq!(js_min(0.0, -0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(js_min(-0.0, 0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(js_max(0.0, -0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(js_max(-0.0, 0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(js_sign(-0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(js_sign(0.0).to_bits(), 0.0f64.to_bits());
        assert!(js_sign(f64::NAN).is_nan());
        assert_eq!(js_min_all(&[]), f64::INFINITY);
        assert_eq!(js_max_all(&[]), f64::NEG_INFINITY);
    }

    #[test]
    fn hypot_follows_v8() {
        assert_eq!(js_hypot2(3.0, 4.0), 5.0);
        assert_eq!(js_hypot2(-0.0, 0.0).to_bits(), 0.0f64.to_bits());
        assert!(js_hypot2(f64::NAN, 1.0).is_nan());
        assert_eq!(js_hypot2(f64::NAN, f64::NEG_INFINITY), f64::INFINITY);
        assert_eq!(js_hypot2(0.7071067811865476, 0.7071067811865476), 1.0000000000000002);
        assert_eq!(js_hypot2(1.0, 1e-17), 1.0);
        assert_eq!(js_hypot2(0.1, 0.2), 0.223606797749979);
        assert_eq!(js_hypot(&[]), 0.0);
        assert_eq!(js_hypot3(2.0, 3.0, 6.0), 7.0);
    }

    #[test]
    fn imul_and_clz32_follow_js() {
        assert_eq!(imul(0xffffffff_u32 as f64, 5.0), -5.0);
        assert_eq!(imul(3.9, 4.0), 12.0);
        assert_eq!(clz32(0.0), 32.0);
        assert_eq!(clz32(1.0), 31.0);
        assert_eq!(clz32(-1.0), 0.0);
        assert_eq!(clz32(f64::NAN), 32.0);
    }

    #[test]
    fn uint8_clamp_ties_to_even() {
        assert_eq!(to_uint8_clamp(f64::NAN), 0);
        assert_eq!(to_uint8_clamp(-3.0), 0);
        assert_eq!(to_uint8_clamp(300.0), 255);
        assert_eq!(to_uint8_clamp(0.5), 0);
        assert_eq!(to_uint8_clamp(1.5), 2);
        assert_eq!(to_uint8_clamp(2.5), 2);
        assert_eq!(to_uint8_clamp(254.5), 254);
    }
}
