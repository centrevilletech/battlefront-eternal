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

use crate::explog::{exp, expm1, log, log1p};
use crate::{hi, lo, with_hi, with_lo, SIGNALING_NAN};

const PI: f64 = 3.14159265358979311600e+00;
const PIO2_HI: f64 = 1.57079632679489655800e+00;
const PIO2_LO: f64 = 6.12323399573676603587e-17;
const PIO4_HI: f64 = 7.85398163397448278999e-01;
const PS0: f64 = 1.66666666666666657415e-01;
const PS1: f64 = -3.25565818622400915405e-01;
const PS2: f64 = 2.01212532134862925881e-01;
const PS3: f64 = -4.00555345006794114027e-02;
const PS4: f64 = 7.91534994289814532176e-04;
const PS5: f64 = 3.47933107596021167570e-05;
const QS1: f64 = -2.40339491173441421878e+00;
const QS2: f64 = 2.02094576023350569471e+00;
const QS3: f64 = -6.88283971605453293030e-01;
const QS4: f64 = 7.70381505559019352791e-02;
const HUGE: f64 = 1.0e+300;
const LN2: f64 = 6.93147180559945286227e-01;

fn asin_r(z: f64) -> (f64, f64) {
    let p = z * (PS0 + z * (PS1 + z * (PS2 + z * (PS3 + z * (PS4 + z * PS5)))));
    let q = 1.0 + z * (QS1 + z * (QS2 + z * (QS3 + z * QS4)));
    (p, q)
}

/// `Math.acos`.
pub fn acos(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix >= 0x3FF0_0000 {
        let lx = lo(x);
        if ((ix.wrapping_sub(0x3FF0_0000) as u32) | lx) == 0 {
            if hx > 0 {
                return 0.0;
            } else {
                return PI + 2.0 * PIO2_LO;
            }
        }
        return SIGNALING_NAN; // acos(|x| > 1) is NaN
    }
    if ix < 0x3FE0_0000 {
        if ix <= 0x3C60_0000 {
            return PIO2_HI + PIO2_LO;
        }
        let z = x * x;
        let (p, q) = asin_r(z);
        let r = p / q;
        PIO2_HI - (x - (PIO2_LO - x * r))
    } else if hx < 0 {
        let z = (1.0 + x) * 0.5;
        let (p, q) = asin_r(z);
        let s = z.sqrt();
        let r = p / q;
        let w = r * s - PIO2_LO;
        PI - 2.0 * (s + w)
    } else {
        let z = (1.0 - x) * 0.5;
        let s = z.sqrt();
        let df = with_lo(s, 0);
        let c = (z - df * df) / (s + df);
        let (p, q) = asin_r(z);
        let r = p / q;
        let w = r * s + c;
        2.0 * (df + w)
    }
}

/// `Math.asin`.
pub fn asin(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix >= 0x3FF0_0000 {
        let lx = lo(x);
        if ((ix.wrapping_sub(0x3FF0_0000) as u32) | lx) == 0 {
            return x * PIO2_HI + x * PIO2_LO;
        }
        return SIGNALING_NAN; // asin(|x| > 1) is NaN
    } else if ix < 0x3FE0_0000 {
        let mut t = 0.0f64;
        if ix < 0x3E40_0000 {
            if HUGE + x > 1.0 {
                return x;
            }
        } else {
            t = x * x;
        }
        let (p, q) = asin_r(t);
        let w = p / q;
        return x + x * w;
    }
    let w = 1.0 - x.abs();
    let t = w * 0.5;
    let (p, q) = asin_r(t);
    let s = t.sqrt();
    let t = if ix >= 0x3FEF_3333 {
        let w = p / q;
        PIO2_HI - (2.0 * (s + s * w) - PIO2_LO)
    } else {
        let w = with_lo(s, 0);
        let c = (t - w * w) / (s + w);
        let r = p / q;
        let p = 2.0 * s * r - (PIO2_LO - 2.0 * c);
        let q2 = PIO4_HI - 2.0 * w;
        PIO4_HI - (p - q2)
    };
    if hx > 0 {
        t
    } else {
        -t
    }
}

/// `Math.acosh`.
pub fn acosh(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let lx = lo(x);
    if hx < 0x3FF0_0000 {
        SIGNALING_NAN // x < 1
    } else if hx >= 0x41B0_0000 {
        if hx >= 0x7FF0_0000 {
            x + x
        } else {
            log(x) + LN2
        }
    } else if ((hx.wrapping_sub(0x3FF0_0000) as u32) | lx) == 0 {
        0.0
    } else if hx > 0x4000_0000 {
        let t = x * x;
        log(2.0 * x - 1.0 / (x + (t - 1.0).sqrt()))
    } else {
        let t = x - 1.0;
        log1p(t + (2.0 * t + t * t).sqrt())
    }
}

/// `Math.asinh`.
pub fn asinh(x: f64) -> f64 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix >= 0x7FF0_0000 {
        return x + x;
    }
    if ix < 0x3E30_0000 {
        if HUGE + x > 1.0 {
            return x;
        }
    }
    let w = if ix > 0x41B0_0000 {
        log(x.abs()) + LN2
    } else if ix > 0x4000_0000 {
        let t = x.abs();
        log(2.0 * t + 1.0 / ((x * x + 1.0).sqrt() + t))
    } else {
        let t = x * x;
        log1p(x.abs() + t / (1.0 + (1.0 + t).sqrt()))
    };
    if hx > 0 {
        w
    } else {
        -w
    }
}

/// `Math.atanh`.
pub fn atanh(x_in: f64) -> f64 {
    let hx = hi(x_in) as i32;
    let lx = lo(x_in);
    let ix = hx & 0x7FFF_FFFF;
    let lxn = (lx | 0u32.wrapping_sub(lx)) >> 31;
    if ((ix as u32) | lxn) > 0x3FF0_0000 {
        return SIGNALING_NAN; // |x| > 1
    }
    if ix == 0x3FF0_0000 {
        return if x_in > 0.0 {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        };
    }
    if ix < 0x3E30_0000 && (HUGE + x_in) > 0.0 {
        return x_in;
    }
    let x = with_hi(x_in, ix as u32);
    let t = if ix < 0x3FE0_0000 {
        let t = x + x;
        0.5 * log1p(t + t * x / (1.0 - x))
    } else {
        0.5 * log1p((x + x) / (1.0 - x))
    };
    if hx >= 0 {
        t
    } else {
        -t
    }
}

/// `Math.atan`.
pub fn atan(x_in: f64) -> f64 {
    const ATANHI: [f64; 4] = [
        4.63647609000806093515e-01,
        7.85398163397448278999e-01,
        9.82793723247329054082e-01,
        1.57079632679489655800e+00,
    ];
    const ATANLO: [f64; 4] = [
        2.26987774529616870924e-17,
        3.06161699786838301793e-17,
        1.39033110312309984516e-17,
        6.12323399573676603587e-17,
    ];
    const AT: [f64; 11] = [
        3.33333333333329318027e-01,
        -1.99999999998764832476e-01,
        1.42857142725034663711e-01,
        -1.11111104054623557880e-01,
        9.09088713343650656196e-02,
        -7.69187620504482999495e-02,
        6.66107313738753120669e-02,
        -5.83357013379057348645e-02,
        4.97687799461593236017e-02,
        -3.65315727442169155270e-02,
        1.62858201153657823623e-02,
    ];
    let mut x = x_in;
    let hx = hi(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix >= 0x4410_0000 {
        let low = lo(x);
        if ix > 0x7FF0_0000 || (ix == 0x7FF0_0000 && low != 0) {
            return x + x;
        }
        if hx > 0 {
            return ATANHI[3] + ATANLO[3];
        } else {
            return -ATANHI[3] - ATANLO[3];
        }
    }
    let id;
    if ix < 0x3FDC_0000 {
        if ix < 0x3E40_0000 {
            if HUGE + x > 1.0 {
                return x;
            }
        }
        id = -1;
    } else {
        x = x.abs();
        if ix < 0x3FF3_0000 {
            if ix < 0x3FE6_0000 {
                id = 0;
                x = (2.0 * x - 1.0) / (2.0 + x);
            } else {
                id = 1;
                x = (x - 1.0) / (x + 1.0);
            }
        } else if ix < 0x4003_8000 {
            id = 2;
            x = (x - 1.5) / (1.0 + 1.5 * x);
        } else {
            id = 3;
            x = -1.0 / x;
        }
    }
    let z = x * x;
    let w = z * z;
    let s1 = z * (AT[0] + w * (AT[2] + w * (AT[4] + w * (AT[6] + w * (AT[8] + w * AT[10])))));
    let s2 = w * (AT[1] + w * (AT[3] + w * (AT[5] + w * (AT[7] + w * AT[9]))));
    if id < 0 {
        x - x * (s1 + s2)
    } else {
        let z = ATANHI[id as usize] - ((x * (s1 + s2) - ATANLO[id as usize]) - x);
        if hx < 0 {
            -z
        } else {
            z
        }
    }
}

/// `Math.atan2`.
pub fn atan2(y: f64, x: f64) -> f64 {
    const TINY: f64 = 1.0e-300;
    const PI_O_4: f64 = 7.8539816339744827900E-01;
    const PI_O_2: f64 = 1.5707963267948965580E+00;
    const PI_LO: f64 = 1.2246467991473531772E-16;
    let hx = hi(x) as i32;
    let lx = lo(x);
    let ix = hx & 0x7FFF_FFFF;
    let hy = hi(y) as i32;
    let ly = lo(y);
    let iy = hy & 0x7FFF_FFFF;
    let lxn = (lx | 0u32.wrapping_sub(lx)) >> 31;
    let lyn = (ly | 0u32.wrapping_sub(ly)) >> 31;
    if ((ix as u32) | lxn) > 0x7FF0_0000 || ((iy as u32) | lyn) > 0x7FF0_0000 {
        return x + y;
    }
    if hx == 0x3FF0_0000 && lx == 0 {
        return atan(y);
    }
    let mut m = ((hy >> 31) & 1) | ((hx >> 30) & 2);
    if ((iy as u32) | ly) == 0 {
        match m {
            0 | 1 => return y,
            2 => return PI + TINY,
            _ => return -PI - TINY,
        }
    }
    if ((ix as u32) | lx) == 0 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    if ix == 0x7FF0_0000 {
        if iy == 0x7FF0_0000 {
            match m {
                0 => return PI_O_4 + TINY,
                1 => return -PI_O_4 - TINY,
                2 => return 3.0 * PI_O_4 + TINY,
                _ => return -3.0 * PI_O_4 - TINY,
            }
        } else {
            match m {
                0 => return 0.0,
                1 => return -0.0,
                2 => return PI + TINY,
                _ => return -PI - TINY,
            }
        }
    }
    if iy == 0x7FF0_0000 {
        return if hy < 0 {
            -PI_O_2 - TINY
        } else {
            PI_O_2 + TINY
        };
    }
    let k = (iy - ix) >> 20;
    let z = if k > 60 {
        m &= 1;
        PI_O_2 + 0.5 * PI_LO
    } else if hx < 0 && k < -60 {
        0.0
    } else {
        atan((y / x).abs())
    };
    match m {
        0 => z,
        1 => -z,
        2 => PI - (z - PI_LO),
        _ => (z - PI_LO) - PI,
    }
}

/// `Math.cosh`.
pub fn cosh(x: f64) -> f64 {
    const KCOSH_OVERFLOW: f64 = 710.4758600739439;
    let ix = (hi(x) as i32) & 0x7FFF_FFFF;
    if ix < 0x3FD6_2E43 {
        let t = expm1(x.abs());
        let w = 1.0 + t;
        if ix < 0x3C80_0000 {
            return w;
        }
        return 1.0 + (t * t) / (w + w);
    }
    if ix < 0x4036_0000 {
        let t = exp(x.abs());
        return 0.5 * t + 0.5 / t;
    }
    if ix < 0x4086_2E42 {
        return 0.5 * exp(x.abs());
    }
    if x.abs() <= KCOSH_OVERFLOW {
        let w = exp(0.5 * x.abs());
        let t = 0.5 * w;
        return t * w;
    }
    if ix >= 0x7FF0_0000 {
        return x * x;
    }
    HUGE * HUGE
}

/// `Math.sinh`.
pub fn sinh(x: f64) -> f64 {
    const KSINH_OVERFLOW: f64 = 710.4758600739439;
    const TWO_M28: f64 = 3.725290298461914e-9;
    const LOG_MAXD: f64 = 709.7822265625;
    const SHUGE: f64 = 1.0e307;
    let h = if x < 0.0 { -0.5 } else { 0.5 };
    let ax = x.abs();
    if ax < 22.0 {
        if ax < TWO_M28 {
            return x;
        }
        let t = expm1(ax);
        if ax < 1.0 {
            return h * (2.0 * t - t * t / (t + 1.0));
        }
        return h * (t + t / (t + 1.0));
    }
    if ax < LOG_MAXD {
        return h * exp(ax);
    }
    if ax <= KSINH_OVERFLOW {
        let w = exp(0.5 * ax);
        let t = h * w;
        return t * w;
    }
    x * SHUGE
}

/// `Math.tanh`.
pub fn tanh(x: f64) -> f64 {
    const TINY: f64 = 1.0e-300;
    let jx = hi(x) as i32;
    let ix = jx & 0x7FFF_FFFF;
    if ix >= 0x7FF0_0000 {
        if jx >= 0 {
            return 1.0 / x + 1.0;
        } else {
            return 1.0 / x - 1.0;
        }
    }
    let z;
    if ix < 0x4036_0000 {
        if ix < 0x3E30_0000 {
            if HUGE + x > 1.0 {
                return x;
            }
        }
        if ix >= 0x3FF0_0000 {
            let t = expm1(2.0 * x.abs());
            z = 1.0 - 2.0 / (t + 2.0);
        } else {
            let t = expm1(-2.0 * x.abs());
            z = -t / (t + 2.0);
        }
    } else {
        z = 1.0 - TINY;
    }
    if jx >= 0 {
        z
    } else {
        -z
    }
}
