//! JavaScript's `Math` as the production server computes it, bit for bit:
//! V8 13.6, the V8 in Node 24, whose results the game server's world
//! simulation produces. Rust code that must agree with that simulation to
//! the last bit calls these functions and never Rust's own `sin`, `exp` and
//! so on, whose last bits differ.
//!
//! - `sin` to `cbrt`, the transcendental functions: V8's port of fdlibm in
//!   `src/base/ieee754.cc`, translated operation by operation. V8 12.4 (Node
//!   22) computes the same for all of them.
//! - `pow`, for `Math.pow` and `**` alike: V8 13.6 keeps a few special cases
//!   and calls the C library's `pow` for the rest, so the server's results
//!   are glibc's as it runs on x86-64 with FMA; `pow` computes those (see
//!   its documentation). `pow_legacy` is V8's own fdlibm `pow`, which V8
//!   12.4 (Node 22) used.
//! - `round`, `sign`, `min`, `max`, `hypot`, `clz32`, `imul`: V8's builtins.
//! - `to_int32` and `to_uint32`: ECMAScript's ToInt32 and ToUint32, what
//!   `x | 0` and `x >>> 0` compute.
//! - `sqrt`, `floor`, `ceil`, `trunc`, `abs` and `fround` are exact
//!   operations, the same in every implementation.
//!
//! Newer V8 differs: V8 15.2 computes most transcendental
//! functions with LLVM's libc instead of fdlibm and differs from V8 13.6 in
//! the last bit of a small fraction of the results of `sin`, `cos`, `tan`,
//! `atan`, `atan2`, `acos`, `exp`, `log`, `cbrt` and `tanh`. This crate
//! keeps the server's results.
//!
//! NaN: any NaN stands for any other, as in JavaScript (V8 itself returns
//! NaNs with different bit patterns from one tier of its compiler to the
//! next). Where V8's fdlibm returns a constant NaN, so does this crate, with
//! the same bits.
//!
//! The arithmetic is plain IEEE double precision in the order written, with
//! no fused multiply-add (Rust never contracts `a * b + c` by itself),
//! except in `pow`, which fuses exactly the operations glibc's `pow` fuses.

#![forbid(unsafe_code)]

mod explog;
mod inv;
mod pow;
mod pow_data;
mod trig;

pub use explog::{cbrt, exp, expm1, log, log10, log1p, log2, pow_legacy};
pub use pow::{glibc_pow, pow};
pub use inv::{acos, acosh, asin, asinh, atan, atan2, atanh, cosh, sinh, tanh};
pub use trig::{cos, sin, sin_cos, tan};

/// The NaN V8's fdlibm returns where it returns
/// `std::numeric_limits<double>::signaling_NaN()`.
pub(crate) const SIGNALING_NAN: f64 = f64::from_bits(0x7FF4_0000_0000_0000);

/// The high 32 bits of `x`.
#[inline(always)]
pub(crate) fn hi(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

/// The low 32 bits of `x`.
#[inline(always)]
pub(crate) fn lo(x: f64) -> u32 {
    x.to_bits() as u32
}

/// `x` with its high 32 bits replaced by `v`.
#[inline(always)]
pub(crate) fn with_hi(x: f64, v: u32) -> f64 {
    f64::from_bits((x.to_bits() & 0x0000_0000_FFFF_FFFF) | ((v as u64) << 32))
}

/// `x` with its low 32 bits replaced by `v`.
#[inline(always)]
pub(crate) fn with_lo(x: f64, v: u32) -> f64 {
    f64::from_bits((x.to_bits() & 0xFFFF_FFFF_0000_0000) | (v as u64))
}

/// The double whose high and low 32 bits are `hi` and `lo`.
#[inline(always)]
pub(crate) fn from_words(hi: u32, lo: u32) -> f64 {
    f64::from_bits(((hi as u64) << 32) | (lo as u64))
}

/// `x * 2^n`, correctly rounded, as the C library's `scalbn` (fdlibm's,
/// which glibc keeps) computes it. Every correct `scalbn` gives the same
/// result; V8's fdlibm calls the C library's.
pub(crate) fn scalbn(x: f64, n: i32) -> f64 {
    const TWO54: f64 = 1.801_439_850_948_198_4e16;
    const TWOM54: f64 = 5.551_115_123_125_783e-17;
    const HUGE: f64 = 1.0e+300;
    const TINY: f64 = 1.0e-300;
    let mut x = x;
    let mut ix = x.to_bits();
    let mut k = ((ix >> 52) & 0x7FF) as i64;
    if k == 0 {
        // 0 or subnormal x
        if ix & 0x000F_FFFF_FFFF_FFFF == 0 {
            return x;
        }
        x *= TWO54;
        ix = x.to_bits();
        k = ((ix >> 52) & 0x7FF) as i64 - 54;
    }
    if k == 0x7FF {
        return x + x; // NaN or Inf
    }
    let n = n as i64;
    if n < -50000 {
        return TINY * TINY.copysign(x); // underflow
    }
    if n > 50000 || k + n > 0x7FE {
        return HUGE * HUGE.copysign(x); // overflow
    }
    k += n;
    if k > 0 {
        // normal result
        return f64::from_bits((ix & 0x800F_FFFF_FFFF_FFFF) | ((k as u64) << 52));
    }
    if k <= -54 {
        return TINY * TINY.copysign(x); // underflow
    }
    k += 54; // subnormal result
    f64::from_bits((ix & 0x800F_FFFF_FFFF_FFFF) | ((k as u64) << 52)) * TWOM54
}

/// ECMAScript ToUint32, what `x >>> 0` computes: NaN and the infinities give
/// 0; anything else is truncated toward zero and reduced modulo 2^32.
#[inline]
pub fn to_uint32(x: f64) -> u32 {
    if x.abs() < 2147483648.0 {
        // Rust's conversion truncates toward zero, as ToInt32 does, for
        // every number this small (NaN fails the test).
        return x as i32 as u32;
    }
    large_to_uint32(x)
}

/// ToUint32 of a number of magnitude 2^31 or more, or NaN.
fn large_to_uint32(x: f64) -> u32 {
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1023;
    if exp < 0 || exp == 1024 {
        return 0; // |x| < 1, zeros, NaN and the infinities
    }
    let m = (bits & 0x000F_FFFF_FFFF_FFFF) | 0x0010_0000_0000_0000;
    // The low 32 bits of the integer part of |x| = m * 2^(exp - 52).
    let r = if exp >= 52 {
        let shift = (exp - 52) as u32;
        if shift >= 32 {
            0
        } else {
            (m << shift) as u32
        }
    } else {
        (m >> (52 - exp) as u32) as u32
    };
    if bits >> 63 != 0 {
        r.wrapping_neg()
    } else {
        r
    }
}

/// ECMAScript ToInt32, what `x | 0` computes: ToUint32's bits read as a
/// signed integer.
#[inline]
pub fn to_int32(x: f64) -> i32 {
    to_uint32(x) as i32
}

/// `Math.clz32`.
pub fn clz32(x: f64) -> f64 {
    to_uint32(x).leading_zeros() as f64
}

/// `Math.imul`.
pub fn imul(x: f64, y: f64) -> f64 {
    to_int32(x).wrapping_mul(to_int32(y)) as f64
}

/// `Math.fround`: rounded to single precision (to nearest, ties to even).
pub fn fround(x: f64) -> f64 {
    (x as f32) as f64
}

/// `Math.sqrt`, the correctly rounded square root.
pub fn sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// `Math.abs`.
pub fn abs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & 0x7FFF_FFFF_FFFF_FFFF)
}

/// `Math.floor`.
pub fn floor(x: f64) -> f64 {
    x.floor()
}

/// `Math.ceil`.
pub fn ceil(x: f64) -> f64 {
    x.ceil()
}

/// `Math.trunc`.
pub fn trunc(x: f64) -> f64 {
    x.trunc()
}

/// `Math.round`, as V8 computes it: `ceil(x)`, less one when `ceil(x) - 0.5`
/// is above `x`. Halves round up, and `-0.5 <= x < 0` gives `-0`.
pub fn round(x: f64) -> f64 {
    let c = x.ceil();
    if c - 0.5 <= x {
        c
    } else {
        c - 1.0
    }
}

/// `Math.sign`: 1 or -1, or `x` itself for zeros and NaN.
pub fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `Math.min` of two numbers: NaN if either is, and `-0` below `+0`.
/// `Math.min(a, b, c)` is `min(min(a, b), c)`.
pub fn min(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        f64::NAN
    } else if x < y {
        x
    } else if y < x {
        y
    } else {
        // Equal: the same number, or two zeros, of which -0 is the smaller.
        f64::from_bits(x.to_bits() | y.to_bits())
    }
}

/// `Math.max` of two numbers: NaN if either is, and `+0` above `-0`.
/// `Math.max(a, b, c)` is `max(max(a, b), c)`.
pub fn max(x: f64, y: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        f64::NAN
    } else if x > y {
        x
    } else if y > x {
        y
    } else {
        // Equal: the same number, or two zeros, of which +0 is the larger.
        f64::from_bits(x.to_bits() & y.to_bits())
    }
}

/// `Math.hypot(x, y)`.
pub fn hypot(x: f64, y: f64) -> f64 {
    hypot_n(&[x, y])
}

/// `Math.hypot(x, y, z)`.
pub fn hypot3(x: f64, y: f64, z: f64) -> f64 {
    hypot_n(&[x, y, z])
}

/// `Math.hypot` of any number of numbers, as V8's builtin computes it: the
/// largest magnitude divides them all, their squares are summed in argument
/// order with Kahan's compensation, and the root of the sum is scaled back.
/// An infinity wins over NaN. (V8's shortcuts for one, two and three
/// arguments compute the same.)
pub fn hypot_n(values: &[f64]) -> f64 {
    let mut max = 0.0f64;
    let mut one_is_nan = false;
    for &v in values {
        if v.is_nan() {
            one_is_nan = true;
        } else if abs(v) > max {
            max = abs(v);
        }
    }
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if one_is_nan {
        return f64::NAN;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for &v in values {
        let n = abs(v) / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, a: &[f64]) -> f64 {
        match name {
            "sin" => sin(a[0]),
            "cos" => cos(a[0]),
            "tan" => tan(a[0]),
            "asin" => asin(a[0]),
            "acos" => acos(a[0]),
            "atan" => atan(a[0]),
            "sinh" => sinh(a[0]),
            "cosh" => cosh(a[0]),
            "tanh" => tanh(a[0]),
            "asinh" => asinh(a[0]),
            "acosh" => acosh(a[0]),
            "atanh" => atanh(a[0]),
            "exp" => exp(a[0]),
            "expm1" => expm1(a[0]),
            "log" => log(a[0]),
            "log1p" => log1p(a[0]),
            "log2" => log2(a[0]),
            "log10" => log10(a[0]),
            "cbrt" => cbrt(a[0]),
            "atan2" => atan2(a[0], a[1]),
            "hypot" => hypot(a[0], a[1]),
            _ => unreachable!(),
        }
    }

    /// Results of Node 24.22 (V8 13.6), as bit patterns.
    #[test]
    fn matches_node_24() {
        let cases: &[(&str, &[u64], u64)] = &[
            ("sin", &[0x4480f0cf064dd592], 0xbfeb453ab76bf397),
            ("sin", &[0x7506ac5b262ca1ff], 0x3ff0000000000000),
            ("cos", &[0x3ff0000000000000], 0x3fe14a280fb5068c),
            ("tan", &[0x3ff921fb54442d18], 0x434d02967c31cdb5),
            ("sin", &[0x400921fb54442d18], 0x3ca1a62633145c07),
            ("asin", &[0x3fe0000000000000], 0x3fe0c152382d7366),
            ("acos", &[0xbfe8000000000000], 0x400359d26f93b6c3),
            ("atan", &[0x4024000000000000], 0x3ff789bd2c160054),
            ("sinh", &[0x3ff0000000000000], 0x3ff2cd9fc44eb982),
            ("cosh", &[0x403e000000000000], 0x429370470aec28ed),
            ("tanh", &[0x3fe0000000000000], 0x3fdd9353d7568af3),
            ("asinh", &[0xc008000000000000], 0xbffd185b507edc0e),
            ("acosh", &[0x3ff8000000000000], 0x3feecc2caec5160a),
            ("atanh", &[0x3fd0000000000000], 0x3fd058aefa811452),
            ("exp", &[0x3ff0000000000000], 0x4005bf0a8b145769),
            ("exp", &[0xc087480000000000], 0x0000000000000001),
            ("exp", &[0x40862e3d70a3d70a], 0x7fefe9ce5c4c52b4),
            ("expm1", &[0x3ddb7cdfd9d7bdbb], 0x3ddb7cdfd9dda4e3),
            ("log", &[0x4024000000000000], 0x40026bb1bbb55516),
            ("log1p", &[0xbfe0000000000000], 0xbfe62e42fefa39ef),
            ("log2", &[0x4008000000000000], 0x3ff95c01a39fbd68),
            ("log10", &[0x4000000000000000], 0x3fd34413509f79ff),
            ("cbrt", &[0x403b000000000000], 0x4008000000000000),
            ("cbrt", &[0x800012688b70e62b], 0xaa7a9d1b0b5d7427),
            ("atan2", &[0x3ff0000000000000, 0xbff0000000000000], 0x4002d97c7f3321d2),
            ("atan2", &[0x8000000000000000, 0xbff0000000000000], 0xc00921fb54442d18),
            ("hypot", &[0x4008000000000000, 0x4010000000000000], 0x4014000000000000),
            ("hypot", &[0x7fe1ccf385ebc8a0, 0x7fe1ccf385ebc8a0], 0x7fe92c80954c51f5),
            // Inputs where V8 15.2 gives another last bit.
            ("sin", &[0xd8d9bb0afc003ba2], 0x3fe6bde4c86e2a0e),
            ("cos", &[0xc033a28c59de50d5], 0x3fe6a09e65b26276),
            ("exp", &[0x3fd2bec48000f933], 0x3ff571de7f0febcc),
            ("log", &[0x3ff6a09d00000882], 0x3fd62e3f08fea33e),
            ("atan2", &[0xbfa2a00000000000, 0xbf78c53ddf863fc4], 0xbffbc4bfd638461e),
            ("cbrt", &[0xb72e4b98115eb203], 0xbd03cb51e401f01f),
            ("tanh", &[0x3dd4dae12487119d], 0x3dd4dae12487119d),
        ];
        for &(name, args, want) in cases {
            let a: Vec<f64> = args.iter().map(|&b| f64::from_bits(b)).collect();
            let got = call(name, &a).to_bits();
            assert_eq!(got, want, "{name}({args:x?}) = {got:#018x}, V8 gives {want:#018x}");
        }
    }

    /// Math.pow(x, y) and x ** y in Node 24.22 on x86-64 with FMA (glibc
    /// 2.39 and 2.43 give the same), as bit patterns.
    #[test]
    fn pow_matches_node_24() {
        let cases: &[(u64, u64, u64)] = &[
            (0x3fe0000000000000, 0xbfe0000000000000, 0x3ff6a09e667f3bcd),
            (0x4000000000000000, 0x3fe0000000000000, 0x3ff6a09e667f3bcd),
            (0xfff0000000000000, 0x3fe0000000000000, 0x7ff0000000000000),
            (0x8000000000000000, 0x3fe0000000000000, 0x0000000000000000),
            (0x7ff8000000000000, 0x0000000000000000, 0x3ff0000000000000),
            (0x3ff0000000000000, 0x7ff0000000000000, 0x7ff8000000000000),
            (0xbff0000000000000, 0xfff0000000000000, 0x7ff8000000000000),
            (0xc020000000000000, 0x3fd5555555555555, 0xfff8000000000000),
            (0xc000000000000000, 0x4008000000000000, 0xc020000000000000),
            (0xc000000000000000, 0xc008000000000000, 0xbfc0000000000000),
            (0x4000000000000000, 0xc090c80000000000, 0x0000000000000001),
            (0x4000000000000000, 0xc090cc0000000000, 0x0000000000000000),
            (0x4000000000000000, 0x4090000000000000, 0x7ff0000000000000),
            (0x4000000000000000, 0x408fffffffffffff, 0x7feffffffffffd3a),
            (0x4024000000000000, 0x4073440000000000, 0x7fefa788589d81d3),
            (0x4024000000000000, 0xc074380000000000, 0x0000000000000001),
            (0x3fe0000000000000, 0x4090ca0000000000, 0x0000000000000001),
            (0xbfe0000000000000, 0x4090cc0000000000, 0x8000000000000000),
            (0x3ff0000000000001, 0x4330000000000000, 0x4005bf0a8b145769),
            (0x3fefffffffffffff, 0xc340000000000000, 0x4005bf0a8b14576a),
            (0x8000000000000000, 0xc008000000000000, 0xfff0000000000000),
            (0x8000000000000000, 0xc000000000000000, 0x7ff0000000000000),
            (0xfff0000000000000, 0x4008000000000000, 0xfff0000000000000),
            (0xfff0000000000000, 0xc008000000000000, 0x8000000000000000),
            // Where glibc's variant without FMA rounds the other way.
            (0x662c2cf3da4355cd, 0x3fe654dcdeed6b80, 0x5a9f2963ed9e012d),
            (0x7c29bfd8a8d47802, 0xbfe329eaa34cda0c, 0x1bdd733aa2b485d7),
            (0x4008000000000000, 0x3e7ad7f29abcaf48, 0x3ff000001d7d9b49),
            (0x000012688b70e62b, 0x3fe8000000000000, 0x0fa922f895935598),
            (0x401c000000000000, 0x4000000000000000, 0x4048800000000000),
            (0xc008000000000000, 0x7e37e43c8800759c, 0x7ff0000000000000),
            (0x3ff8000000000000, 0xbbc79ca10c924223, 0x3ff0000000000000),
            (0x4004000000000000, 0x4012000000000000, 0x404ee1b1b3d78c7a),
            (0x4005bf0a8b145769, 0x400921fb54442d18, 0x403724046eb09338),
        ];
        for &(x, y, want) in cases {
            let got = pow(f64::from_bits(x), f64::from_bits(y));
            assert!(
                got.to_bits() == want || (got.is_nan() && f64::from_bits(want).is_nan()),
                "pow({x:#018x}, {y:#018x}) = {:#018x}, V8 gives {want:#018x}",
                got.to_bits()
            );
        }
        // V8's own fdlibm pow (Node 22) rounds this one down.
        assert_eq!(pow_legacy(0.5, -0.5).to_bits(), 0x3ff6a09e667f3bcc);
        assert_eq!(pow(0.5, -0.5).to_bits(), 0x3ff6a09e667f3bcd);
    }

    #[test]
    fn domain_errors_are_v8s_nans() {
        for v in [acos(2.0), asin(-2.0), acosh(0.5), atanh(2.0), log(-1.0), log1p(-2.0), log2(-1.0)] {
            assert_eq!(v.to_bits(), 0x7FF4_0000_0000_0000);
        }
        assert!(log10(-1.0).is_nan());
        assert!(sin(f64::INFINITY).is_nan());
    }

    #[test]
    fn min_max_keep_javascripts_rules() {
        assert_eq!(min(-5.0, -5.0), -5.0);
        assert_eq!(max(-5.0, -5.0), -5.0);
        assert_eq!(min(0.0, -0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(min(-0.0, 0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(max(0.0, -0.0).to_bits(), 0);
        assert_eq!(max(-0.0, 0.0).to_bits(), 0);
        assert!(min(f64::NAN, 1.0).is_nan() && max(1.0, f64::NAN).is_nan());
        assert_eq!(min(1.0, 2.0), 1.0);
        assert_eq!(max(1.0, 2.0), 2.0);
    }

    #[test]
    fn round_and_friends() {
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(-0.5).to_bits(), (-0.0f64).to_bits());
        assert_eq!(round(-0.2).to_bits(), (-0.0f64).to_bits());
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(-2.5000000000000004), -3.0);
        assert_eq!(round(4503599627370495.5), 4503599627370496.0);
        assert!(round(f64::NAN).is_nan());
        assert_eq!(sign(-0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(sign(-3.0), -1.0);
        assert_eq!(trunc(-0.5).to_bits(), (-0.0f64).to_bits());
        assert_eq!(fround(0.1), 0.10000000149011612);
    }

    #[test]
    fn int32_conversions() {
        assert_eq!(to_int32(2147483648.0), -2147483648);
        assert_eq!(to_int32(-1.9), -1);
        assert_eq!(to_int32(4294967296.5), 0);
        assert_eq!(to_int32(4294967297.0), 1);
        assert_eq!(to_int32(-4294967297.0), -1);
        assert_eq!(to_int32(1e20), 1661992960);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(f64::INFINITY), 0);
        assert_eq!(to_uint32(-1.0), 4294967295);
        assert_eq!(to_uint32(1.8446744073709552e19), 0);
        assert_eq!(clz32(0.0), 32.0);
        assert_eq!(clz32(-1.0), 0.0);
        assert_eq!(imul(0xFFFFFFFFu32 as f64, 5.0), -5.0);
        assert_eq!(imul(1e10, 3.0), (1e10f64 as i64 * 3) as i32 as f64);
    }

    #[test]
    fn hypot_follows_v8() {
        assert_eq!(hypot(3.0, 4.0), 5.0);
        assert_eq!(hypot(-0.0, 0.0).to_bits(), 0);
        assert_eq!(hypot(f64::INFINITY, f64::NAN), f64::INFINITY);
        assert!(hypot(f64::NAN, 1.0).is_nan());
        assert_eq!(hypot3(1.0, 2.0, 2.0), 3.0);
        assert_eq!(hypot_n(&[]), 0.0);
        assert_eq!(hypot_n(&[-7.0]), 7.0);
    }

    #[test]
    fn scalbn_rounds_once() {
        assert_eq!(scalbn(1.0, -1074).to_bits(), 1);
        assert_eq!(scalbn(1.0, -1075).to_bits(), 0); // a tie, to even
        assert_eq!(scalbn(1.5, -1075).to_bits(), 1);
        assert_eq!(scalbn(3.0, -1075).to_bits(), 2); // 1.5 ulps, a tie, to even
        assert_eq!(scalbn(f64::from_bits(1), 1074), 1.0);
        assert_eq!(scalbn(1.0, 1024), f64::INFINITY);
        assert_eq!(scalbn(-1.0, 1023), -8.98846567431158e307);
    }
    #[test]
    fn paired_trig_preserves_scalar_bits() {
        // Include quadrant edges and signed zero, then span ordinary and
        // large finite inputs with deterministic raw floating-point values.
        let check = |x: f64| {
            let (s,c)=sin_cos(x);
            if x.is_finite() {
                assert_eq!(s.to_bits(),sin(x).to_bits(),"sin({x})");
                assert_eq!(c.to_bits(),cos(x).to_bits(),"cos({x})");
            } else {assert!(s.is_nan() && c.is_nan());}
        };
        for x in [0.,-0.,1.,-1.,std::f64::consts::PI,std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_4,1e20,-1e20,f64::INFINITY,f64::NAN] {check(x);}
        let mut state=19u64;
        for _ in 0..65536 {
            state=state.wrapping_mul(6364136223846793005).wrapping_add(1);
            check(f64::from_bits(state));
        }
    }

}
