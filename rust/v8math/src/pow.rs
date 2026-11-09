// Math.pow and `**`: V8 13.6's math::pow (src/numbers/ieee754.cc), which
// keeps a few special cases and hands the rest to the C library's pow, and
// that pow as glibc computes it on x86-64 with FMA.
//
// V8's part: Copyright 2011 the V8 project authors. All rights reserved.
// Use of V8's code is governed by a BSD-style license; see
// THIRD-PARTY-NOTICES.md in the repository root.
//
// The pow is Arm's optimized-routines math/pow.c, which glibc ships as
// sysdeps/ieee754/dbl-64/e_pow.c:
// Copyright (c) 2018-2026, Arm Limited.
// SPDX-License-Identifier: MIT OR Apache-2.0 WITH LLVM-exception
// Used under the MIT License; see THIRD-PARTY-NOTICES.md.

use crate::pow_data::{
    A, C2, C3, C4, C5, EXP_TAB, INV_LN2_N, LN2_HI, LN2_LO, LOG_TAB, NEG_LN2_HI_N, NEG_LN2_LO_N,
    SHIFT,
};

/// `Math.pow(x, y)` and `x ** y`, as Node 24 (V8 13.6) computes them on the
/// game's servers.
///
/// V8 13.6 answers NaN for a NaN `y` and for `(+-1) ** (+-Infinity)`,
/// `x * x` for `y == 2` and `sqrt(x + 0)` for `y == 0.5` (`+Infinity` for
/// `x == -Infinity`); every other case goes to the C library's `pow`, at run
/// time. So the server's results depend on its C library and CPU: this is
/// glibc's `pow` (Arm's code, in glibc since 2.28) as it runs on an x86-64
/// CPU with FMA and AVX2, where glibc picks a variant built with fused
/// multiply-adds; Ubuntu's glibc 2.39 and 2.43 compute the same. It matches
/// Node 24.22 on such a machine for both `Math.pow` and `**`. (glibc's
/// variant without FMA rounds about one random case in 6,000 the other way.)
///
/// The variant's fused multiply-adds are part of what it computes: `fma`
/// below is one rounding, on every platform (Rust's `mul_add`, a hardware
/// instruction where there is one and exact software elsewhere, as in
/// WebAssembly), placed where GCC places them when it builds that variant
/// (13.3 for glibc 2.39, 15.2 for 2.43, the same 21 operations).
pub fn pow(x: f64, y: f64) -> f64 {
    if y.is_nan() {
        return f64::NAN;
    }
    if y.is_infinite() && (x == 1.0 || x == -1.0) {
        return f64::NAN;
    }
    // std::pow tells quiet and signaling NaNs apart; JavaScript doesn't.
    let x = if x.is_nan() { f64::NAN } else { x };
    // The cases V8's optimizing compilers reduce without calling pow.
    if y == 2.0 {
        return x * x;
    }
    if y == 0.5 {
        return if x.is_infinite() { f64::INFINITY } else { (x + 0.0).sqrt() };
    }
    glibc_pow(x, y)
}

/// A multiply-add with one rounding.
#[inline(always)]
fn fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}

const OFF: u64 = 0x3FE6_9555_0000_0000;
const SIGN_BIAS: u64 = 0x800 << 7;
const ONE: u64 = 0x3FF0_0000_0000_0000;
const INF: u64 = 0x7FF0_0000_0000_0000;

/// log(x) as `hi + tail`, the tail carrying about 15 more bits; `ix` is
/// the bit pattern of x, normalized in the subnormal range with the sign bit
/// as part of the exponent.
#[inline(always)]
fn log_inline(ix: u64) -> (f64, f64) {
    // x = 2^k z, with z in [OFF, 2*OFF) exact; the range is split into 128
    // subintervals, the ith of which holds z, with c near its centre.
    let tmp = ix.wrapping_sub(OFF);
    let i = ((tmp >> (52 - 7)) % 128) as usize;
    let k = ((tmp as i64) >> 52) as i32; // arithmetic shift
    let iz = ix.wrapping_sub(tmp & (0xFFF << 52));
    let z = f64::from_bits(iz);
    let kd = k as f64;
    // log(x) = k*Ln2 + log(c) + log1p(z/c - 1)
    let (invc, logc, logctail) = LOG_TAB[i];
    // 1/c makes r = z/c - 1 exact.
    let r = fma(z, invc, -1.0);
    // k*Ln2 + log(c) + r
    let t1 = fma(kd, LN2_HI, logc);
    let t2 = t1 + r;
    let lo1 = fma(kd, LN2_LO, logctail);
    let lo2 = t1 - t2 + r;
    let ar = A[0] * r; // A[0] = -0.5
    let ar2 = r * ar;
    let ar3 = r * ar2;
    // k*Ln2 + log(c) + r + A[0]*r*r
    let hi = t2 + ar2;
    let lo3 = fma(ar, r, -ar2);
    let lo4 = t2 - hi + ar2;
    // p = log1p(r) - r - A[0]*r*r
    //   = ar3 * (A[1] + r*A[2] + ar2*(A[3] + r*A[4] + ar2*(A[5] + r*A[6]))),
    // its additions fused with the products before them, and p fused into
    // the sum of the low parts.
    let q = fma(ar2, fma(ar2, fma(r, A[6], A[5]), fma(r, A[4], A[3])), fma(r, A[2], A[1]));
    let lo = fma(ar3, q, lo1 + lo2 + lo3 + lo4);
    let y = hi + lo;
    let tail = hi - y + lo;
    (y, tail)
}

/// The cases of `exp_inline` that may overflow or underflow, computing
/// scale * (1 + tmp) without an intermediate rounding. `sbits` is the bit
/// pattern of scale, whose exponent may have overflowed into the sign bit;
/// `ki` is the k of the reduction: positive may overflow, negative may
/// underflow.
#[inline(never)]
fn specialcase(tmp: f64, sbits: u64, ki: u64) -> f64 {
    const P1009: f64 = f64::from_bits(0x7F00_0000_0000_0000); // 2^1009
    const PM1022: f64 = f64::from_bits(0x0010_0000_0000_0000); // 2^-1022
    if ki & 0x8000_0000 == 0 {
        // k > 0: the exponent of scale may have overflowed by up to 460.
        let scale = f64::from_bits(sbits.wrapping_sub(1009 << 52));
        return fma(scale, tmp, scale) * P1009;
    }
    // k < 0: careful in the subnormal range. (Not fused here: the product
    // scale * tmp is used twice.)
    let sbits = sbits.wrapping_add(1022 << 52);
    let scale = f64::from_bits(sbits); // signed
    let st = scale * tmp;
    let mut y = scale + st;
    if y.abs() < 1.0 {
        // Round y to the right precision before scaling it into the
        // subnormal range, avoiding a double rounding.
        let one = if y < 0.0 { -1.0 } else { 1.0 };
        let mut lo = scale - y + st;
        let hi = one + y;
        lo += one - hi + y;
        y = (hi + lo) - one;
        if y == 0.0 {
            y = f64::from_bits(sbits & 0x8000_0000_0000_0000); // the sign of 0
        }
    }
    PM1022 * y
}

/// sign * exp(x + xtail), for |xtail| < 2^-15 and |xtail| <= |x|; the sign
/// is negative when `sign_bias` is `SIGN_BIAS`.
#[inline(always)]
fn exp_inline(x: f64, xtail: f64, sign_bias: u64) -> f64 {
    let mut abstop = ((x.to_bits() >> 52) & 0x7FF) as u32;
    // 0x3C9 is the exponent of 2^-54, 0x408 of 512, 0x409 of 1024.
    if abstop.wrapping_sub(0x3C9) >= 0x408 - 0x3C9 {
        if abstop.wrapping_sub(0x3C9) >= 0x8000_0000 {
            // Tiny x (0 included).
            let one = 1.0 + x;
            return if sign_bias != 0 { -one } else { one };
        }
        if abstop >= 0x409 {
            // inf and NaN were dealt with before
            return if x.to_bits() >> 63 != 0 { underflow(sign_bias) } else { overflow(sign_bias) };
        }
        // Large x: special-cased below.
        abstop = 0;
    }
    // exp(x) = 2^(k/N) * exp(r), with x = ln2/N*k + r, N = 128.
    let kd = fma(INV_LN2_N, x, SHIFT);
    let ki = kd.to_bits();
    let kd = kd - SHIFT;
    let r = fma(kd, NEG_LN2_LO_N, fma(kd, NEG_LN2_HI_N, x));
    let r = r + xtail;
    // 2^(k/N) ~= scale * (1 + tail)
    let idx = (2 * (ki % 128)) as usize;
    let top = ki.wrapping_add(sign_bias) << (52 - 7);
    let tail = f64::from_bits(EXP_TAB[idx]);
    // valid as a scale only for -1023*N < k < 1024*N
    let sbits = EXP_TAB[idx + 1].wrapping_add(top);
    // exp(x) = 2^(k/N) * exp(r) ~= scale + scale * (tail + exp(r) - 1)
    let r2 = r * r;
    // tail + r + r2*(C2 + r*C3) + r2*r2*(C4 + r*C5), the additions fused
    let tmp = fma(r2 * r2, fma(r, C5, C4), fma(r2, fma(r, C3, C2), tail + r));
    if abstop == 0 {
        return specialcase(tmp, sbits, ki);
    }
    let scale = f64::from_bits(sbits);
    fma(scale, tmp, scale)
}

fn overflow(sign: u64) -> f64 {
    if sign != 0 {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    }
}

fn underflow(sign: u64) -> f64 {
    if sign != 0 {
        -0.0
    } else {
        0.0
    }
}

/// 0 if `iy` (a finite, non-zero number) is not an integer, 1 if it is an
/// odd one, 2 if an even one.
#[inline(always)]
fn checkint(iy: u64) -> i32 {
    let e = ((iy >> 52) & 0x7FF) as i32;
    if e < 0x3FF {
        return 0;
    }
    if e > 0x3FF + 52 {
        return 2;
    }
    if iy & ((1u64 << (0x3FF + 52 - e)) - 1) != 0 {
        return 0;
    }
    if iy & (1u64 << (0x3FF + 52 - e)) != 0 {
        return 1;
    }
    2
}

/// Whether `i` is the bit pattern of 0, an infinity or NaN.
#[inline(always)]
fn zeroinfnan(i: u64) -> bool {
    i.wrapping_mul(2).wrapping_sub(1) >= (2 * INF).wrapping_sub(1)
}

#[inline(always)]
fn issignaling(x: f64) -> bool {
    (x.to_bits() ^ 0x0008_0000_0000_0000).wrapping_mul(2) > 2 * 0x7FF8_0000_0000_0000
}

/// glibc's `pow` as it runs on x86-64 with FMA and AVX2, alone,
/// without V8's special cases: what `pow` hands its general case to.
pub fn glibc_pow(x: f64, y: f64) -> f64 {
    let mut sign_bias = 0u64;
    let mut ix = x.to_bits();
    let iy = y.to_bits();
    let mut topx = (ix >> 52) as u32;
    let topy = (iy >> 52) as u32;
    if topx.wrapping_sub(0x001) >= 0x7FF - 0x001 || (topy & 0x7FF).wrapping_sub(0x3BE) >= 0x43E - 0x3BE {
        // Special cases: x zero, subnormal, infinite, NaN or negative, or
        // |y| < 2^-65, |y| >= 2^63, or y NaN. If |y| > 1075 * ln2 * 2^53
        // the result is inf or 0, and if |y| < 2^-54 / 1075, +-1.
        if zeroinfnan(iy) {
            if iy.wrapping_mul(2) == 0 {
                return if issignaling(x) { x + y } else { 1.0 };
            }
            if ix == ONE {
                return if issignaling(y) { x + y } else { 1.0 };
            }
            if ix.wrapping_mul(2) > 2 * INF || iy.wrapping_mul(2) > 2 * INF {
                return x + y;
            }
            if ix.wrapping_mul(2) == 2 * ONE {
                return 1.0;
            }
            if (ix.wrapping_mul(2) < 2 * ONE) == (iy >> 63 == 0) {
                return 0.0; // |x| < 1 and y == inf, or |x| > 1 and y == -inf
            }
            return y * y;
        }
        if zeroinfnan(ix) {
            let mut x2 = x * x;
            if ix >> 63 != 0 && checkint(iy) == 1 {
                x2 = -x2;
            }
            // (glibc returns a zero x's 1 / x2 as +-1 / 0, the same value.)
            return if iy >> 63 != 0 { 1.0 / x2 } else { x2 };
        }
        // x and y are finite and not zero.
        if ix >> 63 != 0 {
            // x < 0
            let yint = checkint(iy);
            if yint == 0 {
                // The NaN x86-64 makes of (x - x) / (x - x).
                return f64::from_bits(0xFFF8_0000_0000_0000);
            }
            if yint == 1 {
                sign_bias = SIGN_BIAS;
            }
            ix &= 0x7FFF_FFFF_FFFF_FFFF;
            topx &= 0x7FF;
        }
        if (topy & 0x7FF).wrapping_sub(0x3BE) >= 0x43E - 0x3BE {
            // sign_bias is 0 here: such a y is not odd.
            if ix == ONE {
                return 1.0;
            }
            if (topy & 0x7FF) < 0x3BE {
                // |y| < 2^-65: x^y ~= 1 + y*log(x)
                return if ix > ONE { 1.0 + y } else { 1.0 - y };
            }
            return if (ix > ONE) == (topy < 0x800) { overflow(0) } else { underflow(0) };
        }
        if topx == 0 {
            // Normalize a subnormal x so its exponent becomes negative.
            ix = (x * f64::from_bits(0x4330_0000_0000_0000)).to_bits(); // x * 2^52
            ix &= 0x7FFF_FFFF_FFFF_FFFF;
            ix = ix.wrapping_sub(52 << 52);
        }
    }
    let (hi, lo) = log_inline(ix);
    let ehi = y * hi;
    let elo = fma(y, lo, fma(y, hi, -ehi));
    exp_inline(ehi, elo, sign_bias)
}
