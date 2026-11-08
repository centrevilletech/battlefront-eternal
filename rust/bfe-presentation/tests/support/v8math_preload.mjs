// Exact IEEE-754 non-FMA fdlibm `Math.sin` and `Math.cos` matching `rust/v8math/src/trig.rs`
// and V8 13.6 (`src/base/ieee754.cc`) on x86_64.
//
// Why this preload is needed on `linux-arm64` Node:
// Official `linux-arm64` Node 24 binaries are compiled by GCC targeting `armv8-a`
// (where hardware FMA `fmadd`/`fmsub` is baseline) with GCC's default `-ffp-contract=fast`.
// That contracts `a * b + c` into hardware FMA inside V8's C++ `ieee754.cc` (`rem_pio2`,
// `kernel_sin`, and `kernel_cos`), producing a 1-ULP difference from `linux-x64` Node 24
// and Rust's `v8math` on ~1.5% of angles. In JavaScript, ECMAScript forbids FMA contraction
// of separate `*` and `+`/`-` operators, so this direct port of `v8math::trig::{sin, cos}`
// produces identical bits on both `x86_64` and `aarch64`.

const buf = new ArrayBuffer(8);
const f64 = new Float64Array(buf);
const u32 = new Uint32Array(buf);
// Little-endian word indices (both x86_64 and aarch64 Linux are little-endian).
const LO = 0;
const HI = 1;

function hi(x) {
  f64[0] = x;
  return u32[HI] >>> 0;
}

function lo(x) {
  f64[0] = x;
  return u32[LO] >>> 0;
}

function fromWords(h, l) {
  u32[HI] = h >>> 0;
  u32[LO] = l >>> 0;
  return f64[0];
}

const TWO54 = 1.80143985094819840000e+16;
const TWOM54 = 5.55111512312578270212e-17;
const HUGE = 1.0e+300;
const TINY = 1.0e-300;

function copysign(mag, sgn) {
  f64[0] = sgn;
  const signBit = u32[HI] & 0x80000000;
  f64[0] = mag;
  u32[HI] = (u32[HI] & 0x7fffffff) | signBit;
  return f64[0];
}

function scalbn(x, n) {
  f64[0] = x;
  let hx = u32[HI] >>> 0;
  let lx = u32[LO] >>> 0;
  let k = (hx >>> 20) & 0x7ff;
  if (k === 0) {
    if (((hx & 0x000fffff) | lx) === 0) return x;
    x *= TWO54;
    f64[0] = x;
    hx = u32[HI] >>> 0;
    lx = u32[LO] >>> 0;
    k = ((hx >>> 20) & 0x7ff) - 54;
  }
  if (k === 0x7ff) return x + x;
  if (n < -50000) return TINY * copysign(TINY, x);
  if (n > 50000 || k + n > 0x7fe) return HUGE * copysign(HUGE, x);
  k = (k + n) | 0;
  if (k > 0) {
    u32[HI] = (hx & 0x800fffff) | ((k & 0x7ff) << 20);
    u32[LO] = lx;
    return f64[0];
  }
  if (k <= -54) return TINY * copysign(TINY, x);
  k = (k + 54) | 0;
  u32[HI] = (hx & 0x800fffff) | ((k & 0x7ff) << 20);
  u32[LO] = lx;
  return f64[0] * TWOM54;
}

const TWO_OVER_PI = new Int32Array([
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
]);

const NPIO2_HW = new Int32Array([
  0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB,
  0x402921FB, 0x402C463A, 0x402F6A7A, 0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB,
  0x40378FDB, 0x403921FB, 0x403AB41B, 0x403C463A, 0x403DD85A, 0x403F6A7A, 0x40407E4C,
  0x4041475C, 0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB,
  0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
]);

const HALF = 5.00000000000000000000e-01;
const TWO24 = 1.67772160000000000000e+07;
const INVPIO2 = 6.36619772367581382433e-01;
const PIO2_1 = 1.57079632673412561417e+00;
const PIO2_1T = 6.07710050650619224932e-11;
const PIO2_2 = 6.07710050630396597660e-11;
const PIO2_2T = 2.02226624879595063154e-21;
const PIO2_3 = 2.02226624871116645580e-21;
const PIO2_3T = 8.47842766036889956997e-32;

const PIO2 = new Float64Array([
  1.57079625129699707031e+00,
  7.54978941586159635335e-08,
  5.39030252995776476554e-15,
  3.28200341580791294123e-22,
  1.27065575308067607349e-29,
  1.22933308981111328932e-36,
  2.73370053816464559624e-44,
  2.16741683877804819444e-51,
]);
const TWON24 = 5.96046447753906250000e-08;

function kernelRemPio2(x, y, e0, nx, ipio2) {
  const jk = 4;
  const jp = jk;
  const jx = (nx - 1) | 0;
  let jv = ((e0 - 3) / 24) | 0;
  if (jv < 0) jv = 0;
  let q0 = (e0 - 24 * (jv + 1)) | 0;

  const f = new Float64Array(20);
  const q = new Float64Array(20);
  const fq = new Float64Array(20);
  const iq = new Int32Array(20);

  let j = (jv - jx) | 0;
  const m = (jx + jk) | 0;
  for (let i = 0; i <= m; i++) {
    f[i] = j < 0 ? 0.0 : ipio2[j];
    j = (j + 1) | 0;
  }
  for (let i = 0; i <= jk; i++) {
    let fw = 0.0;
    for (let jj = 0; jj <= jx; jj++) {
      fw += x[jj] * f[jx + i - jj];
    }
    q[i] = fw;
  }

  let jz = jk;
  let n = 0;
  let ih = 0;
  let z = 0.0;
  for (;;) {
    let i = 0;
    let jj = jz;
    z = q[jz];
    while (jj > 0) {
      const fw = ((TWON24 * z) | 0);
      iq[i] = (z - TWO24 * fw) | 0;
      z = q[jj - 1] + fw;
      i = (i + 1) | 0;
      jj = (jj - 1) | 0;
    }
    z = scalbn(z, q0);
    z -= 8.0 * Math.floor(z * 0.125);
    n = z | 0;
    z -= n;
    ih = 0;
    if (q0 > 0) {
      const ii = iq[jz - 1] >> (24 - q0);
      n = (n + ii) | 0;
      iq[jz - 1] -= ii << (24 - q0);
      ih = iq[jz - 1] >> (23 - q0);
    } else if (q0 === 0) {
      ih = iq[jz - 1] >> 23;
    } else if (z >= 0.5) {
      ih = 2;
    }
    if (ih > 0) {
      n = (n + 1) | 0;
      let carry = 0;
      for (let i = 0; i < jz; i++) {
        const jj = iq[i];
        if (carry === 0) {
          if (jj !== 0) {
            carry = 1;
            iq[i] = (0x1000000 - jj) | 0;
          }
        } else {
          iq[i] = (0xffffff - jj) | 0;
        }
      }
      if (q0 > 0) {
        if (q0 === 1) iq[jz - 1] &= 0x7fffff;
        else if (q0 === 2) iq[jz - 1] &= 0x3fffff;
      }
      if (ih === 2) {
        z = 1.0 - z;
        if (carry !== 0) {
          z -= scalbn(1.0, q0);
        }
      }
    }
    if (z === 0.0) {
      let jj = 0;
      for (let i = jz - 1; i >= jk; i--) {
        jj |= iq[i];
      }
      if (jj === 0) {
        let k = 1;
        while (jk >= k && iq[jk - k] === 0) {
          k = (k + 1) | 0;
        }
        for (let i = jz + 1; i <= jz + k; i++) {
          f[jx + i] = ipio2[jv + i];
          let fw = 0.0;
          for (let jj2 = 0; jj2 <= jx; jj2++) {
            fw += x[jj2] * f[jx + i - jj2];
          }
          q[i] = fw;
        }
        jz = (jz + k) | 0;
        continue;
      }
    }
    break;
  }

  if (z === 0.0) {
    jz = (jz - 1) | 0;
    q0 = (q0 - 24) | 0;
    while (iq[jz] === 0) {
      jz = (jz - 1) | 0;
      q0 = (q0 - 24) | 0;
    }
  } else {
    z = scalbn(z, -q0);
    if (z >= TWO24) {
      const fw = (TWON24 * z) | 0;
      iq[jz] = (z - TWO24 * fw) | 0;
      jz = (jz + 1) | 0;
      q0 = (q0 + 24) | 0;
      iq[jz] = fw;
    } else {
      iq[jz] = z | 0;
    }
  }

  let fw = scalbn(1.0, q0);
  for (let i = jz; i >= 0; i--) {
    q[i] = fw * iq[i];
    fw *= TWON24;
  }
  for (let i = jz; i >= 0; i--) {
    let fw2 = 0.0;
    for (let k = 0; k <= jp && k <= jz - i; k++) {
      fw2 += PIO2[k] * q[i + k];
    }
    fq[jz - i] = fw2;
  }

  let fw3 = 0.0;
  for (let i = jz; i >= 0; i--) {
    fw3 += fq[i];
  }
  y[0] = ih === 0 ? fw3 : -fw3;
  fw3 = fq[0] - fw3;
  for (let i = 1; i <= jz; i++) {
    fw3 += fq[i];
  }
  y[1] = ih === 0 ? fw3 : -fw3;
  return n & 7;
}

const remY = new Float64Array(2);
const remTx = new Float64Array(3);

function remPio2(x, y) {
  const hx = hi(x) | 0;
  const ix = hx & 0x7fffffff;
  if (ix <= 0x3fe921fb) {
    y[0] = x;
    y[1] = 0.0;
    return 0;
  }
  if (ix < 0x4002d97c) {
    if (hx > 0) {
      let z = x - PIO2_1;
      if (ix !== 0x3ff921fb) {
        y[0] = z - PIO2_1T;
        y[1] = (z - y[0]) - PIO2_1T;
      } else {
        z -= PIO2_2;
        y[0] = z - PIO2_2T;
        y[1] = (z - y[0]) - PIO2_2T;
      }
      return 1;
    } else {
      let z = x + PIO2_1;
      if (ix !== 0x3ff921fb) {
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
  if (ix <= 0x413921fb) {
    const t = Math.abs(x);
    const n = (t * INVPIO2 + HALF) | 0;
    const f_n = n;
    let r = t - f_n * PIO2_1;
    let w = f_n * PIO2_1T;
    if (n < 32 && ix !== NPIO2_HW[n - 1]) {
      y[0] = r - w;
    } else {
      const j = ix >> 20;
      y[0] = r - w;
      let high = hi(y[0]);
      let i = j - ((high >>> 20) & 0x7ff);
      if (i > 16) {
        const t2 = r;
        w = f_n * PIO2_2;
        r = t2 - w;
        w = f_n * PIO2_2T - ((t2 - r) - w);
        y[0] = r - w;
        high = hi(y[0]);
        i = j - ((high >>> 20) & 0x7ff);
        if (i > 49) {
          const t2 = r;
          w = f_n * PIO2_3;
          r = t2 - w;
          w = f_n * PIO2_3T - ((t2 - r) - w);
          y[0] = r - w;
        }
      }
    }
    y[1] = (r - y[0]) - w;
    if (hx < 0) {
      y[0] = -y[0];
      y[1] = -y[1];
      return -n;
    }
    return n;
  }
  if (ix >= 0x7ff00000) {
    y[0] = x - x;
    y[1] = x - x;
    return 0;
  }
  const e0 = (ix >> 20) - 1046;
  let z = fromWords((ix - (e0 << 20)) >>> 0, lo(x));
  for (let i = 0; i < 2; i++) {
    remTx[i] = z | 0;
    z = (z - remTx[i]) * TWO24;
  }
  remTx[2] = z;
  let nx = 3;
  while (remTx[nx - 1] === 0.0) {
    nx--;
  }
  const n = kernelRemPio2(remTx, y, e0, nx, TWO_OVER_PI);
  if (hx < 0) {
    y[0] = -y[0];
    y[1] = -y[1];
    return -n;
  }
  return n;
}

const C1 = 4.16666666666666019037e-02;
const C2 = -1.38888888888741095749e-03;
const C3 = 2.48015872894767294178e-05;
const C4 = -2.75573143513906633035e-07;
const C5 = 2.08757232129817482790e-09;
const C6 = -1.13596475577881948265e-11;

function kernelCos(x, y) {
  const ix = hi(x) & 0x7fffffff;
  if (ix < 0x3e400000) {
    if ((x | 0) === 0) return 1.0;
  }
  const z = x * x;
  const r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
  if (ix < 0x3fd33333) {
    return 1.0 - (0.5 * z - (z * r - x * y));
  } else {
    const qx = ix > 0x3fe90000 ? 0.28125 : fromWords((ix - 0x00200000) >>> 0, 0);
    const iz = 0.5 * z - qx;
    const a = 1.0 - qx;
    return a - (iz - (z * r - x * y));
  }
}

const S1 = -1.66666666666666324348e-01;
const S2 = 8.33333333332248946124e-03;
const S3 = -1.98412698298579493134e-04;
const S4 = 2.75573137070700676789e-06;
const S5 = -2.50507602534068634195e-08;
const S6 = 1.58969099521155010221e-10;

function kernelSin(x, y, iy) {
  const ix = hi(x) & 0x7fffffff;
  if (ix < 0x3e400000) {
    if ((x | 0) === 0) return x;
  }
  const z = x * x;
  const v = z * x;
  const r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
  if (iy === 0) {
    return x + v * (S1 + z * r);
  } else {
    return x - ((z * (HALF * y - v * r) - y) - v * S1);
  }
}

export function v8Sin(x) {
  x = +x;
  const ix = hi(x) & 0x7fffffff;
  if (ix <= 0x3fe921fb) {
    return kernelSin(x, 0.0, 0);
  } else if (ix >= 0x7ff00000) {
    return x - x;
  } else {
    const n = remPio2(x, remY);
    switch (n & 3) {
      case 0: return kernelSin(remY[0], remY[1], 1);
      case 1: return kernelCos(remY[0], remY[1]);
      case 2: return -kernelSin(remY[0], remY[1], 1);
      default: return -kernelCos(remY[0], remY[1]);
    }
  }
}

export function v8Cos(x) {
  x = +x;
  const ix = hi(x) & 0x7fffffff;
  if (ix <= 0x3fe921fb) {
    return kernelCos(x, 0.0);
  } else if (ix >= 0x7ff00000) {
    return x - x;
  } else {
    const n = remPio2(x, remY);
    switch (n & 3) {
      case 0: return kernelCos(remY[0], remY[1]);
      case 1: return -kernelSin(remY[0], remY[1], 1);
      case 2: return -kernelCos(remY[0], remY[1]);
      default: return kernelSin(remY[0], remY[1], 1);
    }
  }
}

Math.sin = v8Sin;
Math.cos = v8Cos;
