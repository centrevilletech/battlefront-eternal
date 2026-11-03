// The following is adapted from fdlibm (http://www.netlib.org/fdlibm),
// as ported by V8 in src/base/ieee754.cc.
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================
//
// The original source code covered by the above license has been
// modified significantly by Google Inc.
// Copyright 2016 the V8 project authors. All rights reserved.
// Use of V8's code is governed by a BSD-style license; see
// THIRD-PARTY-NOTICES.md in the repository root.

use crate::{from_words, hi, lo, with_hi, with_lo, SIGNALING_NAN};

const HUGE: f64 = 1.0e+300;
const LN2_HI: f64 = 6.93147180369123816490e-01;
const LN2_LO: f64 = 1.90821492927058770002e-10;
const TWO54: f64 = 1.80143985094819840000e+16;
const LG1: f64 = 6.666666666666735130e-01;
const LG2: f64 = 3.999999999940941908e-01;
const LG3: f64 = 2.857142874366239149e-01;
const LG4: f64 = 2.222219843214978396e-01;
const LG5: f64 = 1.818357216161805012e-01;
const LG6: f64 = 1.531383769920937332e-01;
const LG7: f64 = 1.479819860511658591e-01;

/// `Math.exp`.
pub fn exp(x_in: f64) -> f64 {
    const HALF: [f64; 2] = [0.5, -0.5];
    const O_THRESHOLD: f64 = 7.09782712893383973096e+02;
    const U_THRESHOLD: f64 = -7.45133219101941108420e+02;
    const LN2HI: [f64; 2] = [6.93147180369123816490e-01, -6.93147180369123816490e-01];
    const LN2LO: [f64; 2] = [1.90821492927058770002e-10, -1.90821492927058770002e-10];
    const INV_LN2: f64 = 1.44269504088896338700e+00;
    const P1: f64 = 1.66666666666666019037e-01;
    const P2: f64 = -2.77777777770155933842e-03;
    const P3: f64 = 6.61375632143793436117e-05;
    const P4: f64 = -1.65339022054652515390e-06;
    const P5: f64 = 4.13813679705723846039e-08;
    const E: f64 = 2.718281828459045;
    const TWOM1000: f64 = 9.33263618503218878990e-302;
    const TWO1023: f64 = 8.988465674311579539e307;

    let mut x = x_in;
    let mut hi_v = 0.0f64;
    let mut lo_v = 0.0f64;
    let mut k = 0i32;
    let hx_word = hi(x);
    let xsb = ((hx_word >> 31) & 1) as usize;
    let hx = hx_word & 0x7FFF_FFFF;

    if hx >= 0x4086_2E42 {
        if hx >= 0x7FF0_0000 {
            let lx = lo(x);
            if ((hx & 0xF_FFFF) | lx) != 0 {
                return x + x;
            } else {
                return if xsb == 0 { x } else { 0.0 };
            }
        }
        if x > O_THRESHOLD {
            return HUGE * HUGE;
        }
        if x < U_THRESHOLD {
            return TWOM1000 * TWOM1000;
        }
    }

    if hx > 0x3FD6_2E42 {
        if hx < 0x3FF0_A2B2 {
            if x == 1.0 {
                return E;
            }
            hi_v = x - LN2HI[xsb];
            lo_v = LN2LO[xsb];
            k = 1 - (xsb as i32) - (xsb as i32);
        } else {
            k = (INV_LN2 * x + HALF[xsb]) as i32;
            let t = k as f64;
            hi_v = x - t * LN2HI[0];
            lo_v = t * LN2LO[0];
        }
        x = hi_v - lo_v;
    } else if hx < 0x3E30_0000 {
        if HUGE + x > 1.0 {
            return 1.0 + x;
        }
    } else {
        k = 0;
    }

    let t = x * x;
    let twopk = if k >= -1021 {
        from_words(
            (0x3FF0_0000i32).wrapping_add(((k as u32) << 20) as i32) as u32,
            0,
        )
    } else {
        from_words(
            (0x3FF0_0000i32).wrapping_add((((k + 1000) as u32) << 20) as i32) as u32,
            0,
        )
    };
    let c = x - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    let y;
    if k == 0 {
        return 1.0 - ((x * c) / (c - 2.0) - x);
    } else {
        y = 1.0 - ((lo_v - (x * c) / (2.0 - c)) - hi_v);
    }
    if k >= -1021 {
        if k == 1024 {
            y * 2.0 * TWO1023
        } else {
            y * twopk
        }
    } else {
        y * twopk * TWOM1000
    }
}

/// `Math.log`.
pub fn log(x_in: f64) -> f64 {
    let mut x = x_in;
    let mut hx = hi(x) as i32;
    let lx = lo(x);
    let mut k = 0i32;
    if hx < 0x0010_0000 {
        if (((hx & 0x7FFF_FFFF) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return SIGNALING_NAN; // log(-#) = NaN
        }
        k -= 54;
        x *= TWO54;
        hx = hi(x) as i32;
    }
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000F_FFFF;
    let i = (hx + 0x95F64) & 0x10_0000;
    x = with_hi(x, (hx | (i ^ 0x3FF0_0000)) as u32);
    k += i >> 20;
    let f = x - 1.0;
    if (0x000F_FFFF & (2 + hx)) < 3 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            } else {
                let dk = k as f64;
                return dk * LN2_HI + dk * LN2_LO;
            }
        }
        let r = f * f * (0.5 - 0.33333333333333333 * f);
        if k == 0 {
            return f - r;
        } else {
            let dk = k as f64;
            return dk * LN2_HI - ((r - dk * LN2_LO) - f);
        }
    }
    let s = f / (2.0 + f);
    let dk = k as f64;
    let z = s * s;
    let mut i = hx - 0x6_147A;
    let w = z * z;
    let j = 0x6_B851 - hx;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    i |= j;
    let r = t2 + t1;
    if i > 0 {
        let hfsq = 0.5 * f * f;
        if k == 0 {
            f - (hfsq - s * (hfsq + r))
        } else {
            dk * LN2_HI - ((hfsq - (s * (hfsq + r) + dk * LN2_LO)) - f)
        }
    } else if k == 0 {
        f - s * (f - r)
    } else {
        dk * LN2_HI - ((s * (f - r) - dk * LN2_LO) - f)
    }
}

fn k_log1p(f: f64) -> f64 {
    let s = f / (2.0 + f);
    let z = s * s;
    let w = z * z;
    let t1 = w * (LG2 + w * (LG4 + w * LG6));
    let t2 = z * (LG1 + w * (LG3 + w * (LG5 + w * LG7)));
    let r = t2 + t1;
    let hfsq = 0.5 * f * f;
    s * (hfsq + r)
}

/// `Math.log1p`.
pub fn log1p(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ax = hx & 0x7FFF_FFFF;
    let mut k = 1i32;
    let mut f = 0.0f64;
    let mut c = 0.0f64;
    let mut hu = 0i32;
    if hx < 0x3FDA_827A {
        if ax >= 0x3FF0_0000 {
            if x == -1.0 {
                return f64::NEG_INFINITY;
            } else {
                return SIGNALING_NAN; // log1p(x < -1) = NaN
            }
        }
        if ax < 0x3E20_0000 {
            if TWO54 + x > 0.0 && ax < 0x3C90_0000 {
                return x;
            } else {
                return x - x * x * 0.5;
            }
        }
        if hx > 0 || hx <= (0xBFD2_BEC4u32 as i32) {
            k = 0;
            f = x;
            hu = 1;
        }
    }
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    if k != 0 {
        let mut u;
        if hx < 0x4340_0000 {
            u = 1.0 + x;
            hu = hi(u) as i32;
            k = (hu >> 20) - 1023;
            c = if k > 0 {
                1.0 - (u - x)
            } else {
                x - (u - 1.0)
            };
            c /= u;
        } else {
            u = x;
            hu = hi(u) as i32;
            k = (hu >> 20) - 1023;
            c = 0.0;
        }
        hu &= 0x000F_FFFF;
        if hu < 0x6_A09E {
            u = with_hi(u, (hu | 0x3FF0_0000) as u32);
        } else {
            k += 1;
            u = with_hi(u, (hu | 0x3FE0_0000) as u32);
            hu = (0x0010_0000 - hu) >> 2;
        }
        f = u - 1.0;
    }
    let hfsq = 0.5 * f * f;
    if hu == 0 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            } else {
                c += (k as f64) * LN2_LO;
                return (k as f64) * LN2_HI + c;
            }
        }
        let r = hfsq * (1.0 - 0.66666666666666666 * f);
        if k == 0 {
            return f - r;
        } else {
            return (k as f64) * LN2_HI - ((r - ((k as f64) * LN2_LO + c)) - f);
        }
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let r = z
        * (LG1
            + z * (LG2 + z * (LG3 + z * (LG4 + z * (LG5 + z * (LG6 + z * LG7))))));
    if k == 0 {
        f - (hfsq - s * (hfsq + r))
    } else {
        (k as f64) * LN2_HI - ((hfsq - (s * (hfsq + r) + ((k as f64) * LN2_LO + c))) - f)
    }
}

/// `Math.log2`.
pub fn log2(x_in: f64) -> f64 {
    const IVLN2HI: f64 = 1.44269504072144627571e+00;
    const IVLN2LO: f64 = 1.67517131648865118353e-10;
    let mut x = x_in;
    let mut hx = hi(x) as i32;
    let lx = lo(x);
    let mut k = 0i32;
    if hx < 0x0010_0000 {
        if (((hx & 0x7FFF_FFFF) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return SIGNALING_NAN; // log(-#) = NaN
        }
        k -= 54;
        x *= TWO54;
        hx = hi(x) as i32;
    }
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    if hx == 0x3FF0_0000 && lx == 0 {
        return 0.0;
    }
    k += (hx >> 20) - 1023;
    hx &= 0x000F_FFFF;
    let i = (hx + 0x95F64) & 0x10_0000;
    x = with_hi(x, (hx | (i ^ 0x3FF0_0000)) as u32);
    k += i >> 20;
    let y = k as f64;
    let f = x - 1.0;
    let hfsq = 0.5 * f * f;
    let r = k_log1p(f);
    let hi_v = with_lo(f - hfsq, 0);
    let lo_v = (f - hi_v) - hfsq + r;
    let mut val_hi = hi_v * IVLN2HI;
    let mut val_lo = (lo_v + hi_v) * IVLN2LO + lo_v * IVLN2HI;
    let w = y + val_hi;
    val_lo += (y - w) + val_hi;
    val_hi = w;
    val_lo + val_hi
}

/// `Math.log10`.
pub fn log10(x_in: f64) -> f64 {
    const IVLN10: f64 = 4.34294481903251816668e-01;
    const LOG10_2HI: f64 = 3.01029995663611771306e-01;
    const LOG10_2LO: f64 = 3.69423907715893078616e-13;
    let mut x = x_in;
    let mut hx = hi(x) as i32;
    let mut lx = lo(x);
    let mut k = 0i32;
    if hx < 0x0010_0000 {
        if (((hx & 0x7FFF_FFFF) as u32) | lx) == 0 {
            return f64::NEG_INFINITY;
        }
        if hx < 0 {
            return f64::NAN;
        }
        k -= 54;
        x *= TWO54;
        hx = hi(x) as i32;
        lx = lo(x);
    }
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    if hx == 0x3FF0_0000 && lx == 0 {
        return 0.0;
    }
    k += (hx >> 20) - 1023;
    let i = ((k as u32 & 0x8000_0000) >> 31) as i32;
    hx = (hx & 0x000F_FFFF) | ((0x3FF - i) << 20);
    let y = (k + i) as f64;
    x = from_words(hx as u32, lx);
    let z = y * LOG10_2LO + IVLN10 * log(x);
    z + y * LOG10_2HI
}

/// `Math.expm1`.
pub fn expm1(x_in: f64) -> f64 {
    const TINY: f64 = 1.0e-300;
    const O_THRESHOLD: f64 = 7.09782712893383973096e+02;
    const INV_LN2: f64 = 1.44269504088896338700e+00;
    const Q1: f64 = -3.33333333333331316428e-02;
    const Q2: f64 = 1.58730158725481460165e-03;
    const Q3: f64 = -7.93650757867487942473e-05;
    const Q4: f64 = 4.00821782732936239552e-06;
    const Q5: f64 = -2.01099218183624371326e-07;

    let mut x = x_in;
    let hx_word = hi(x);
    let xsb = hx_word & 0x8000_0000;
    let hx = hx_word & 0x7FFF_FFFF;
    let k: i32;
    let hi_v: f64;
    let lo_v: f64;
    let mut c = 0.0f64;

    if hx >= 0x4043_687A {
        if hx >= 0x4086_2E42 {
            if hx >= 0x7FF0_0000 {
                let low = lo(x);
                if ((hx & 0xF_FFFF) | low) != 0 {
                    return x + x;
                } else {
                    return if xsb == 0 { x } else { -1.0 };
                }
            }
            if x > O_THRESHOLD {
                return HUGE * HUGE;
            }
        }
        if xsb != 0 {
            if x + TINY < 0.0 {
                return TINY - 1.0;
            }
        }
    }

    if hx > 0x3FD6_2E42 {
        if hx < 0x3FF0_A2B2 {
            if xsb == 0 {
                hi_v = x - LN2_HI;
                lo_v = LN2_LO;
                k = 1;
            } else {
                hi_v = x + LN2_HI;
                lo_v = -LN2_LO;
                k = -1;
            }
        } else {
            k = (INV_LN2 * x + if xsb == 0 { 0.5 } else { -0.5 }) as i32;
            let t = k as f64;
            hi_v = x - t * LN2_HI;
            lo_v = t * LN2_LO;
        }
        x = hi_v - lo_v;
        c = (hi_v - x) - lo_v;
    } else if hx < 0x3C90_0000 {
        let t = HUGE + x;
        return x - (t - (HUGE + x));
    } else {
        k = 0;
    }

    let hfx = 0.5 * x;
    let hxs = x * hfx;
    let r1 = 1.0 + hxs * (Q1 + hxs * (Q2 + hxs * (Q3 + hxs * (Q4 + hxs * Q5))));
    let t = 3.0 - r1 * hfx;
    let mut e = hxs * ((r1 - t) / (6.0 - x * t));
    if k == 0 {
        return x - (x * e - hxs);
    }
    let twopk = from_words(
        (0x3FF0_0000i32).wrapping_add(((k as u32) << 20) as i32) as u32,
        0,
    );
    e = x * (e - c) - c;
    e -= hxs;
    if k == -1 {
        return 0.5 * (x - e) - 0.5;
    }
    if k == 1 {
        if x < -0.25 {
            return -2.0 * (e - (x + 0.5));
        } else {
            return 1.0 + 2.0 * (x - e);
        }
    }
    if k <= -2 || k > 56 {
        let mut y = 1.0 - (e - x);
        if k == 1024 {
            y = y * 2.0 * 8.98846567431158e+307;
        } else {
            y = y * twopk;
        }
        return y - 1.0;
    }
    let y;
    if k < 20 {
        let t2 = with_hi(1.0, (0x3FF0_0000i32 - (0x20_0000 >> k)) as u32);
        y = (t2 - (e - x)) * twopk;
    } else {
        let t2 = with_hi(1.0, ((0x3FF - k) << 20) as u32);
        let mut yy = x - (e + t2);
        yy += 1.0;
        y = yy * twopk;
    }
    y
}

/// `Math.cbrt`.
pub fn cbrt(x: f64) -> f64 {
    const B1: u32 = 715094163;
    const B2: u32 = 696219795;
    const P0: f64 = 1.87595182427177009643;
    const P1: f64 = -1.88497979543377169875;
    const P2: f64 = 1.621429720105354466140;
    const P3: f64 = -0.758397934778766047437;
    const P4: f64 = 0.145996192886612446982;

    let low = lo(x);
    let mut hx = (hi(x) as i32) as u32;
    let sign = hx & 0x8000_0000;
    hx ^= sign;
    if hx >= 0x7FF0_0000 {
        return x + x;
    }
    let mut t;
    if hx < 0x0010_0000 {
        if (hx | low) == 0 {
            return x;
        }
        t = from_words(0x4350_0000, 0);
        t *= x;
        let high = hi(t);
        t = from_words(sign | ((high & 0x7FFF_FFFF) / 3 + B2), 0);
    } else {
        t = from_words(sign | (hx / 3 + B1), 0);
    }
    let r = (t * t) * (t / x);
    t = t * ((P0 + r * (P1 + r * P2)) + ((r * r) * r) * (P3 + r * P4));
    let bits = t.to_bits();
    let bits = (bits.wrapping_add(0x8000_0000)) & 0xFFFF_FFFF_C000_0000;
    t = f64::from_bits(bits);
    let s = t * t;
    let mut r = x / s;
    let w = t + t;
    r = (r - t) / (w + r);
    t = t + t * r;
    t
}
