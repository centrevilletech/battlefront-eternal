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

use crate::{from_words, hi, lo, scalbn, with_lo};

/// 396 hexadecimal digits of 2/pi, 24 bits an entry.
const TWO_OVER_PI: [i32; 66] = [
    0xA2F983, 0x6E4E44, 0x1529FC, 0x2757D1, 0xF534DD, 0xC0DB62, 0x95993C,
    0x439041, 0xFE5163, 0xABDEBB, 0xC561B7, 0x246E3A, 0x424DD2, 0xE00649,
    0x2EEA09, 0xD1921C, 0xFE1DEB, 0x1CB129, 0xA73EE8, 0x8235F5, 0x2EBB44,
    0x84E99C, 0x7026B4, 0x5F7E41, 0x3991D6, 0x398353, 0x39F49C, 0x845F8B,
    0xBDF928, 0x3B1FF8, 0x97FFDE, 0x05980F, 0xEF2F11, 0x8B5A0A, 0x6D1F6D,
    0x367ECF, 0x27CB09, 0xB74F46, 0x3F669E, 0x5FEA2D, 0x7527BA, 0xC7EBE5,
    0xF17B3D, 0x0739F7, 0x8A5292, 0xEA6BFB, 0x5FB11F, 0x8D5D08, 0x560330,
    0x46FC7B, 0x6BABF0, 0xCFBC20, 0x9AF436, 0x1DA9E3, 0x91615E, 0xE61B08,
    0x659985, 0x5F14A0, 0x68408D, 0xFFD880, 0x4D7327, 0x310606, 0x1556CA,
    0x73A8C9, 0x60E27B, 0xC08C6B,
];

/// The high words of n * pi/2 for n = 1 to 32.
const NPIO2_HW: [i32; 32] = [
    0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB,
    0x402921FB, 0x402C463A, 0x402F6A7A, 0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB,
    0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A, 0x403DD85A, 0x403F6A7A, 0x40407E4C,
    0x4041475C, 0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB,
    0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
];

const HALF: f64 = 5.00000000000000000000e-01;
const TWO24: f64 = 1.67772160000000000000e+07;
const INVPIO2: f64 = 6.36619772367581382433e-01;
const PIO2_1: f64 = 1.57079632673412561417e+00;
const PIO2_1T: f64 = 6.07710050650619224932e-11;
const PIO2_2: f64 = 6.07710050630396597660e-11;
const PIO2_2T: f64 = 2.02226624879595063154e-21;
const PIO2_3: f64 = 2.02226624871116645580e-21;
const PIO2_3T: f64 = 8.47842766036889956997e-32;

/// Remainder of `x` modulo pi/2: returns `n` and writes the remainder as
/// `y[0] + y[1]`, as fdlibm's `__ieee754_rem_pio2`.
fn rem_pio2(x: f64, y: &mut [f64; 2]) -> i32 {
    let hx = hi(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        y[0] = x;
        y[1] = 0.0;
        return 0;
    }
    if ix < 0x4002_D97C {
        if hx > 0 {
            let mut z = x - PIO2_1;
            if ix != 0x3FF9_21FB {
                y[0] = z - PIO2_1T;
                y[1] = (z - y[0]) - PIO2_1T;
            } else {
                z -= PIO2_2;
                y[0] = z - PIO2_2T;
                y[1] = (z - y[0]) - PIO2_2T;
            }
            return 1;
        } else {
            let mut z = x + PIO2_1;
            if ix != 0x3FF9_21FB {
                y[0] = z + PIO2_1T;
                y[1] = (z - y[0]) + PIO2_1T;
            } else {
                z += PIO2_2;
                y[0] = z + PIO2_2T;
                y[1] = (z - y[0]) + PIO2_2T;
            }
            return -1;
        }
    }
    if ix <= 0x4139_21FB {
        let t = x.abs();
        let n = (t * INVPIO2 + HALF) as i32;
        let f_n = n as f64;
        let mut r = t - f_n * PIO2_1;
        let mut w = f_n * PIO2_1T;
        if n < 32 && ix != NPIO2_HW[(n - 1) as usize] {
            y[0] = r - w;
        } else {
            let j = ix >> 20;
            y[0] = r - w;
            let high = hi(y[0]);
            let mut i = j - (((high >> 20) & 0x7FF) as i32);
            if i > 16 {
                let t2 = r;
                w = f_n * PIO2_2;
                r = t2 - w;
                w = f_n * PIO2_2T - ((t2 - r) - w);
                y[0] = r - w;
                let high = hi(y[0]);
                i = j - (((high >> 20) & 0x7FF) as i32);
                if i > 49 {
                    let t2 = r;
                    w = f_n * PIO2_3;
                    r = t2 - w;
                    w = f_n * PIO2_3T - ((t2 - r) - w);
                    y[0] = r - w;
                }
            }
        }
        y[1] = (r - y[0]) - w;
        if hx < 0 {
            y[0] = -y[0];
            y[1] = -y[1];
            return -n;
        } else {
            return n;
        }
    }
    if ix >= 0x7FF0_0000 {
        y[0] = x - x;
        y[1] = x - x;
        return 0;
    }
    // set z = scalbn(|x|, ilogb(x) - 23), by words
    let e0 = (ix >> 20) - 1046;
    let mut z = from_words((ix - ((e0 as u32) << 20) as i32) as u32, lo(x));
    let mut tx = [0.0f64; 3];
    for i in 0..2 {
        tx[i] = (z as i32) as f64;
        z = (z - tx[i]) * TWO24;
    }
    tx[2] = z;
    let mut nx: i32 = 3;
    while tx[(nx - 1) as usize] == 0.0 {
        nx -= 1;
    }
    let n = kernel_rem_pio2(&tx, y, e0, nx, &TWO_OVER_PI);
    if hx < 0 {
        y[0] = -y[0];
        y[1] = -y[1];
        -n
    } else {
        n
    }
}

/// fdlibm's `__kernel_rem_pio2` for the precision V8 asks for (2: 53 bits
/// and a second term), `jk` = 4.
fn kernel_rem_pio2(x: &[f64; 3], y: &mut [f64; 2], e0: i32, nx: i32, ipio2: &[i32; 66]) -> i32 {
    const PIO2: [f64; 8] = [
        1.57079625129699707031e+00,
        7.54978941586159635335e-08,
        5.39030252995776476554e-15,
        3.28200341580791294123e-22,
        1.27065575308067607349e-29,
        1.22933308981111328932e-36,
        2.73370053816464559624e-44,
        2.16741683877804819444e-51,
    ];
    const TWON24: f64 = 5.96046447753906250000e-08;

    let jk = 4i32;
    let jp = jk;
    let jx = nx - 1;
    let mut jv = (e0 - 3) / 24;
    if jv < 0 {
        jv = 0;
    }
    let mut q0 = e0 - 24 * (jv + 1);

    let mut f = [0.0f64; 20];
    let mut q = [0.0f64; 20];
    let mut fq = [0.0f64; 20];
    let mut iq = [0i32; 20];

    let mut j = jv - jx;
    let m = jx + jk;
    for i in 0..=m {
        f[i as usize] = if j < 0 {
            0.0
        } else {
            ipio2[j as usize] as f64
        };
        j += 1;
    }
    for i in 0..=jk {
        let mut fw = 0.0f64;
        for jj in 0..=jx {
            fw += x[jj as usize] * f[(jx + i - jj) as usize];
        }
        q[i as usize] = fw;
    }

    let mut jz = jk;
    let mut n: i32;
    let mut ih: i32;
    let mut z: f64;
    loop {
        // distill q[] into iq[] reversingly
        let mut i = 0i32;
        let mut jj = jz;
        z = q[jz as usize];
        while jj > 0 {
            let fw = ((TWON24 * z) as i32) as f64;
            iq[i as usize] = (z - TWO24 * fw) as i32;
            z = q[(jj - 1) as usize] + fw;
            i += 1;
            jj -= 1;
        }
        z = scalbn(z, q0);
        z -= 8.0 * (z * 0.125).floor();
        n = z as i32;
        z -= n as f64;
        ih = 0;
        if q0 > 0 {
            let ii = iq[(jz - 1) as usize] >> (24 - q0);
            n += ii;
            iq[(jz - 1) as usize] -= ii << (24 - q0);
            ih = iq[(jz - 1) as usize] >> (23 - q0);
        } else if q0 == 0 {
            ih = iq[(jz - 1) as usize] >> 23;
        } else if z >= 0.5 {
            ih = 2;
        }
        if ih > 0 {
            n += 1;
            let mut carry = 0i32;
            for i in 0..jz {
                let jj = iq[i as usize];
                if carry == 0 {
                    if jj != 0 {
                        carry = 1;
                        iq[i as usize] = 0x100_0000 - jj;
                    }
                } else {
                    iq[i as usize] = 0xFF_FFFF - jj;
                }
            }
            if q0 > 0 {
                match q0 {
                    1 => iq[(jz - 1) as usize] &= 0x7F_FFFF,
                    2 => iq[(jz - 1) as usize] &= 0x3F_FFFF,
                    _ => {}
                }
            }
            if ih == 2 {
                z = 1.0 - z;
                if carry != 0 {
                    z -= scalbn(1.0, q0);
                }
            }
        }
        if z == 0.0 {
            let mut jj = 0i32;
            let mut i = jz - 1;
            while i >= jk {
                jj |= iq[i as usize];
                i -= 1;
            }
            if jj == 0 {
                let mut k = 1i32;
                while jk >= k && iq[(jk - k) as usize] == 0 {
                    k += 1;
                }
                for i in (jz + 1)..=(jz + k) {
                    f[(jx + i) as usize] = ipio2[(jv + i) as usize] as f64;
                    let mut fw = 0.0f64;
                    for jj2 in 0..=jx {
                        fw += x[jj2 as usize] * f[(jx + i - jj2) as usize];
                    }
                    q[i as usize] = fw;
                }
                jz += k;
                continue;
            }
        }
        break;
    }

    if z == 0.0 {
        jz -= 1;
        q0 -= 24;
        while iq[jz as usize] == 0 {
            jz -= 1;
            q0 -= 24;
        }
    } else {
        z = scalbn(z, -q0);
        if z >= TWO24 {
            let fw = ((TWON24 * z) as i32) as f64;
            iq[jz as usize] = (z - TWO24 * fw) as i32;
            jz += 1;
            q0 += 24;
            iq[jz as usize] = fw as i32;
        } else {
            iq[jz as usize] = z as i32;
        }
    }

    let mut fw = scalbn(1.0, q0);
    let mut i = jz;
    while i >= 0 {
        q[i as usize] = fw * (iq[i as usize] as f64);
        fw *= TWON24;
        i -= 1;
    }
    let mut i = jz;
    while i >= 0 {
        let mut fw2 = 0.0f64;
        let mut k = 0i32;
        while k <= jp && k <= jz - i {
            fw2 += PIO2[k as usize] * q[(i + k) as usize];
            k += 1;
        }
        fq[(jz - i) as usize] = fw2;
        i -= 1;
    }

    // compress fq[] into y[] (precision 2)
    let mut fw3 = 0.0f64;
    let mut i = jz;
    while i >= 0 {
        fw3 += fq[i as usize];
        i -= 1;
    }
    y[0] = if ih == 0 { fw3 } else { -fw3 };
    fw3 = fq[0] - fw3;
    for i in 1..=jz {
        fw3 += fq[i as usize];
    }
    y[1] = if ih == 0 { fw3 } else { -fw3 };
    n & 7
}

fn kernel_cos(x: f64, y: f64) -> f64 {
    const C1: f64 = 4.16666666666666019037e-02;
    const C2: f64 = -1.38888888888741095749e-03;
    const C3: f64 = 2.48015872894767294178e-05;
    const C4: f64 = -2.75573143513906633035e-07;
    const C5: f64 = 2.08757232129817482790e-09;
    const C6: f64 = -1.13596475577881948265e-11;
    let ix = (hi(x) as i32) & 0x7FFF_FFFF;
    if ix < 0x3E40_0000 {
        if (x as i32) == 0 {
            return 1.0;
        }
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3FD3_3333 {
        1.0 - (0.5 * z - (z * r - x * y))
    } else {
        let qx = if ix > 0x3FE9_0000 {
            0.28125
        } else {
            from_words((ix - 0x0020_0000) as u32, 0)
        };
        let iz = 0.5 * z - qx;
        let a = 1.0 - qx;
        a - (iz - (z * r - x * y))
    }
}

fn kernel_sin(x: f64, y: f64, iy: i32) -> f64 {
    const S1: f64 = -1.66666666666666324348e-01;
    const S2: f64 = 8.33333333332248946124e-03;
    const S3: f64 = -1.98412698298579493134e-04;
    const S4: f64 = 2.75573137070700676789e-06;
    const S5: f64 = -2.50507602534068634195e-08;
    const S6: f64 = 1.58969099521155010221e-10;
    let ix = (hi(x) as i32) & 0x7FFF_FFFF;
    if ix < 0x3E40_0000 {
        if (x as i32) == 0 {
            return x;
        }
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if iy == 0 {
        x + v * (S1 + z * r)
    } else {
        x - ((z * (HALF * y - v * r) - y) - v * S1)
    }
}

fn kernel_tan(x_in: f64, y_in: f64, iy: i32) -> f64 {
    const T: [f64; 13] = [
        3.33333333333334091986e-01,
        1.33333333333201242699e-01,
        5.39682539762260521377e-02,
        2.18694882948595424599e-02,
        8.86323982359930005737e-03,
        3.59207910759131235356e-03,
        1.45620945432529025516e-03,
        5.88041240820264096874e-04,
        2.46463134818469906812e-04,
        7.81794442939557092300e-05,
        7.14072491382608190305e-05,
        -1.85586374855275456654e-05,
        2.59073051863633712884e-05,
    ];
    const PIO4: f64 = 7.85398163397448278999e-01;
    const PIO4LO: f64 = 3.06161699786838301793e-17;
    let mut x = x_in;
    let mut y = y_in;
    let hx = hi(x) as i32;
    let ix = hx & 0x7FFF_FFFF;
    if ix < 0x3E30_0000 {
        if (x as i32) == 0 {
            let low = lo(x);
            if ((ix as u32 | low) | ((iy + 1) as u32)) == 0 {
                return 1.0 / x.abs();
            } else if iy == 1 {
                return x;
            } else {
                let w = x + y;
                let z = with_lo(w, 0);
                let v = y - (z - x);
                let a = -1.0 / w;
                let t = with_lo(a, 0);
                let s = 1.0 + t * z;
                return t + a * (s + t * v);
            }
        }
    }
    if ix >= 0x3FE5_9428 {
        if hx < 0 {
            x = -x;
            y = -y;
        }
        let z = PIO4 - x;
        let w = PIO4LO - y;
        x = z + w;
        y = 0.0;
    }
    let z = x * x;
    let w = z * z;
    let r = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))));
    let v = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))));
    let s = z * x;
    let mut r = y + z * (s * (r + v) + y);
    r += T[0] * s;
    let w = x + r;
    if ix >= 0x3FE5_9428 {
        let v = iy as f64;
        return ((1 - ((hx >> 30) & 2)) as f64) * (v - 2.0 * (x - (w * w / (w + v) - r)));
    }
    if iy == 1 {
        return w;
    }
    let z = with_lo(w, 0);
    let v = r - (z - x);
    let a = -1.0 / w;
    let t = with_lo(a, 0);
    let s = 1.0 + t * z;
    t + a * (s + t * v)
}

/// `Math.sin`.
pub fn sin(x: f64) -> f64 {
    let ix = (hi(x) as i32) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        kernel_sin(x, 0.0, 0)
    } else if ix >= 0x7FF0_0000 {
        x - x
    } else {
        let mut y = [0.0f64; 2];
        let n = rem_pio2(x, &mut y);
        match n & 3 {
            0 => kernel_sin(y[0], y[1], 1),
            1 => kernel_cos(y[0], y[1]),
            2 => -kernel_sin(y[0], y[1], 1),
            _ => -kernel_cos(y[0], y[1]),
        }
    }
}

/// `Math.cos`.
pub fn cos(x: f64) -> f64 {
    let ix = (hi(x) as i32) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        kernel_cos(x, 0.0)
    } else if ix >= 0x7FF0_0000 {
        x - x
    } else {
        let mut y = [0.0f64; 2];
        let n = rem_pio2(x, &mut y);
        match n & 3 {
            0 => kernel_cos(y[0], y[1]),
            1 => -kernel_sin(y[0], y[1], 1),
            2 => -kernel_cos(y[0], y[1]),
            _ => kernel_sin(y[0], y[1], 1),
        }
    }
}

/// Paired `Math.sin` and `Math.cos`, with their original kernel arithmetic.
/// Body facing and aim bases need both values. Reduce the same angle once,
/// then apply exactly the scalar functions' quadrant and sign decisions.
/// This shares no rounded result between their independent polynomials.
pub fn sin_cos(x: f64) -> (f64, f64) {
    let ix = (hi(x) as i32) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        (kernel_sin(x, 0.0, 0), kernel_cos(x, 0.0))
    } else if ix >= 0x7FF0_0000 {
        (x - x, x - x)
    } else {
        let mut y = [0.0f64; 2];
        let n = rem_pio2(x, &mut y);
        let (s, c) = (kernel_sin(y[0], y[1], 1), kernel_cos(y[0], y[1]));
        match n & 3 {
            0 => (s, c),
            1 => (c, -s),
            2 => (-s, -c),
            _ => (-c, s),
        }
    }
}

/// `Math.tan`.
pub fn tan(x: f64) -> f64 {
    let ix = (hi(x) as i32) & 0x7FFF_FFFF;
    if ix <= 0x3FE9_21FB {
        kernel_tan(x, 0.0, 1)
    } else if ix >= 0x7FF0_0000 {
        x - x
    } else {
        let mut y = [0.0f64; 2];
        let n = rem_pio2(x, &mut y);
        kernel_tan(y[0], y[1], 1 - ((n & 1) << 1))
    }
}
