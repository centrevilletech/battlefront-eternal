//! The inputs v8math is checked on and the functions under test, shared by
//! the native comparer (`src/main.rs`, which asks Node for V8's results) and
//! the WebAssembly build that `check-wasm.mjs` runs inside Node. Both make
//! the same inputs from the same seed, chunk by chunk.
//!
//! Inputs are made from bit patterns, not uniform floats: any pattern at
//! all; a sign, an exponent from the function's working range and random
//! mantissa bits; patterns within a few ulps (or up to 2^47 ulps) of the
//! thresholds the implementations branch on; and short mantissas (integers,
//! halves, simple fractions). Two-argument functions also get pairs whose
//! exponents differ by about as much as their algorithms care about.

/// A function under test: its name (the JavaScript the oracle runs is in
/// `oracle.mjs`), how many numbers it takes, and the range of exponents
/// its inputs mostly come from.
#[derive(Clone, Copy)]
pub struct Fun {
    pub name: &'static str,
    pub arity: usize,
    /// Results per input: 2 for pow (`Math.pow(x, y)` and `x ** y`).
    pub outs: usize,
    pub emin: i32,
    pub emax: i32,
    pub positive: bool,
}

const fn f(name: &'static str, arity: usize, emin: i32, emax: i32) -> Fun {
    Fun { name, arity, outs: 1, emin, emax, positive: false }
}

pub const FUNS: &[Fun] = &[
    f("sin", 1, -40, 40),
    f("cos", 1, -40, 40),
    f("tan", 1, -40, 40),
    f("asin", 1, -60, 1),
    f("acos", 1, -60, 1),
    f("atan", 1, -40, 70),
    f("sinh", 1, -40, 11),
    f("cosh", 1, -40, 11),
    f("tanh", 1, -40, 6),
    f("asinh", 1, -40, 60),
    Fun { name: "acosh", arity: 1, outs: 1, emin: 0, emax: 60, positive: true },
    f("atanh", 1, -40, 1),
    f("exp", 1, -60, 11),
    f("expm1", 1, -60, 11),
    Fun { name: "log", arity: 1, outs: 1, emin: -1074, emax: 1023, positive: true },
    f("log1p", 1, -60, 60),
    Fun { name: "log2", arity: 1, outs: 1, emin: -1074, emax: 1023, positive: true },
    Fun { name: "log10", arity: 1, outs: 1, emin: -1074, emax: 1023, positive: true },
    f("cbrt", 1, -1074, 1023),
    f("sqrt", 1, -1074, 1023),
    f("abs", 1, -1074, 1023),
    f("floor", 1, -4, 60),
    f("ceil", 1, -4, 60),
    f("trunc", 1, -4, 60),
    f("round", 1, -4, 60),
    f("fround", 1, -160, 130),
    f("sign", 1, -1074, 1023),
    f("clz32", 1, -4, 90),
    f("to_int32", 1, -4, 90),
    f("to_uint32", 1, -4, 90),
    f("atan2", 2, -80, 80),
    f("hypot", 2, -1074, 1023),
    f("hypot3", 3, -1074, 1023),
    f("min", 2, -1074, 1023),
    f("max", 2, -1074, 1023),
    f("imul", 2, -4, 90),
    Fun { name: "pow", arity: 2, outs: 2, emin: -1074, emax: 1023, positive: false },
    Fun { name: "pow_legacy", arity: 2, outs: 2, emin: -1074, emax: 1023, positive: false },
    f("glibc_pow", 2, -1074, 1023),
    // Rust's mul_add, on which pow's fused multiply-adds rest; compared
    // natively (hardware) with the WebAssembly build (software).
    f("fma", 3, -1074, 1023),
];

pub fn find(name: &str) -> Option<usize> {
    FUNS.iter().position(|f| f.name == name)
}

/// Every input tuple of `inputs` (`arity` numbers each) through function
/// `id`, into `out`: one loop per function, so each is compiled with the
/// function inlined.
pub fn eval_all(id: usize, inputs: &[f64], out: &mut [f64]) {
    use v8math as m;
    fn one(inputs: &[f64], out: &mut [f64], f: impl Fn(f64) -> f64) {
        for (o, &x) in out.iter_mut().zip(inputs) {
            *o = f(x);
        }
    }
    fn two(inputs: &[f64], out: &mut [f64], f: impl Fn(f64, f64) -> f64) {
        for (o, p) in out.iter_mut().zip(inputs.chunks_exact(2)) {
            *o = f(p[0], p[1]);
        }
    }
    fn three(inputs: &[f64], out: &mut [f64], f: impl Fn(f64, f64, f64) -> f64) {
        for (o, p) in out.iter_mut().zip(inputs.chunks_exact(3)) {
            *o = f(p[0], p[1], p[2]);
        }
    }
    let (i, o) = (inputs, out);
    match FUNS[id].name {
        "sin" => one(i, o, m::sin),
        "cos" => one(i, o, m::cos),
        "tan" => one(i, o, m::tan),
        "asin" => one(i, o, m::asin),
        "acos" => one(i, o, m::acos),
        "atan" => one(i, o, m::atan),
        "sinh" => one(i, o, m::sinh),
        "cosh" => one(i, o, m::cosh),
        "tanh" => one(i, o, m::tanh),
        "asinh" => one(i, o, m::asinh),
        "acosh" => one(i, o, m::acosh),
        "atanh" => one(i, o, m::atanh),
        "exp" => one(i, o, m::exp),
        "expm1" => one(i, o, m::expm1),
        "log" => one(i, o, m::log),
        "log1p" => one(i, o, m::log1p),
        "log2" => one(i, o, m::log2),
        "log10" => one(i, o, m::log10),
        "cbrt" => one(i, o, m::cbrt),
        "sqrt" => one(i, o, m::sqrt),
        "abs" => one(i, o, m::abs),
        "floor" => one(i, o, m::floor),
        "ceil" => one(i, o, m::ceil),
        "trunc" => one(i, o, m::trunc),
        "round" => one(i, o, m::round),
        "fround" => one(i, o, m::fround),
        "sign" => one(i, o, m::sign),
        "clz32" => one(i, o, m::clz32),
        "to_int32" => one(i, o, |x| m::to_int32(x) as f64),
        "to_uint32" => one(i, o, |x| m::to_uint32(x) as f64),
        "atan2" => two(i, o, m::atan2),
        "hypot" => two(i, o, m::hypot),
        "hypot3" => three(i, o, m::hypot3),
        "min" => two(i, o, m::min),
        "max" => two(i, o, m::max),
        "imul" => two(i, o, m::imul),
        "pow" => two(i, o, m::pow),
        "pow_legacy" => two(i, o, m::pow_legacy),
        "glibc_pow" => two(i, o, m::glibc_pow),
        "fma" => three(i, o, |a: f64, b: f64, c: f64| a.mul_add(b, c)),
        name => panic!("no function {name}"),
    }
}

/// 2^e for e in [-1074, 1023], exactly.
pub fn pow2(e: i32) -> f64 {
    if e >= -1022 {
        f64::from_bits(((e + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (e + 1074))
    }
}

/// splitmix64.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, n).
    pub fn below(&mut self, n: u64) -> u64 {
        ((self.next() as u128 * n as u128) >> 64) as u64
    }
}

fn fnv(name: &str) -> u64 {
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    for b in name.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// The generator for one chunk of one function's random inputs.
pub fn chunk_rng(id: usize, seed: u64, chunk: u64) -> Rng {
    let mut r = Rng::new(seed ^ fnv(FUNS[id].name) ^ chunk.wrapping_mul(0xD1B5_4A32_D192_ED03));
    r.next();
    r
}

const MANT: u64 = 0x000F_FFFF_FFFF_FFFF;
const SIGN: u64 = 0x8000_0000_0000_0000;

/// High words the fdlibm code branches on.
const THRESHOLD_HIGH_WORDS: [u32; 46] = [
    0x00100000, 0x00200000, 0x3C600000, 0x3C800000, 0x3C900000, 0x3E200000, 0x3E300000,
    0x3E400000, 0x3FD33333, 0x3FD62E42, 0x3FD62E43, 0x3FDA827A, 0x3FDC0000, 0x3FE00000,
    0x3FE59428, 0x3FE60000, 0x3FE90000, 0x3FE921FB, 0x3FEF3333, 0x3FF00000, 0x3FF0A2B2,
    0x3FF30000, 0x3FF921FB, 0x40000000, 0x4002D97C, 0x40038000, 0x40360000, 0x4043687A,
    0x40862E42, 0x413921FB, 0x41B00000, 0x43400000, 0x43500000, 0x44100000, 0x7FF00000,
    0x3FD2BEC4, 0x41E00000, 0x43F00000, 0x3FEFFFFF, 0x40900000, 0x4090CC00,
    // log's and log1p's split points in the mantissa (around sqrt(2))
    0x3FF6147A, 0x3FF6B851, 0x3FF6A09E, 0x3FF6A09C, 0x3FE6A09E,
];

/// Values the functions treat specially or that are hard to get right.
pub fn anchors() -> Vec<f64> {
    let mut v = vec![
        0.0, 1.0, 0.5, 2.0, 1.5, 0.25, 3.0, 0.75, 10.0, 100.0, 1e-300, 1e300,
        f64::MIN_POSITIVE, f64::MAX, f64::EPSILON, 5e-324,
        709.782712893383973096, 745.13321910194110842, 710.4758600739439, 709.7822265625,
        22.0, 56.0 * core::f64::consts::LN_2, 0.5 * core::f64::consts::LN_2,
        1.5 * core::f64::consts::LN_2, core::f64::consts::LN_2, core::f64::consts::E,
        core::f64::consts::PI, core::f64::consts::FRAC_PI_2, core::f64::consts::FRAC_PI_4,
        0.6744, 0.4375, 1.1875, 2.4375, 0.975, 0.78125, 0.3, 0.28125,
        // the worst case for reducing modulo pi/2, and sin(1e22)
        6381956970095103.0 * pow2(797), 1e22,
        2147483648.0, 4294967296.0, 4294967295.0, 2147483647.0, 9007199254740992.0,
        4503599627370496.0, 16777216.0, 3.4028234663852886e38, 1.1754943508222875e-38,
        1.401298464324817e-45,
    ];
    for &hw in THRESHOLD_HIGH_WORDS.iter() {
        for lo in [0u32, 1, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF] {
            for h in [hw.wrapping_sub(1), hw, hw + 1] {
                v.push(f64::from_bits(((h as u64) << 32) | lo as u64));
            }
        }
    }
    // multiples of pi/2, near and far
    let pio2 = core::f64::consts::FRAC_PI_2;
    for k in 1..=200 {
        v.push(k as f64 * pio2);
        v.push(k as f64 * core::f64::consts::FRAC_PI_4);
    }
    let mut k = 256.0f64;
    while k < 1e300 {
        v.push(k * pio2);
        v.push((k + 1.0) * pio2);
        k *= 7.3;
    }
    // powers of two
    for e in -1074..=1023 {
        v.push(pow2(e));
    }
    v
}

/// Special values for one-argument functions: the anchors and their
/// neighbours, both signs, the infinities, NaNs, and small integers and
/// halves.
pub fn special_singles() -> Vec<f64> {
    let mut v = Vec::new();
    for a in anchors() {
        let b = a.to_bits();
        for d in 0..=3u64 {
            for s in [0, SIGN] {
                v.push(f64::from_bits((b + d) ^ s));
                if b >= d {
                    v.push(f64::from_bits((b - d) ^ s));
                }
            }
        }
    }
    for i in -300..=300 {
        v.push(i as f64);
        v.push(i as f64 + 0.5);
        v.push(i as f64 / 8.0);
    }
    v.extend([
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        -f64::NAN,
        f64::from_bits(0x7FF4_0000_0000_0000),
        f64::from_bits(0x7FF0_0000_0000_0001),
        f64::from_bits(0xFFFF_FFFF_FFFF_FFFF),
        f64::from_bits(0x000F_FFFF_FFFF_FFFF),
        f64::from_bits(0x800F_FFFF_FFFF_FFFF),
        f64::from_bits(0x0000_0000_0000_0001),
        f64::from_bits(0x8000_0000_0000_0001),
        -0.0,
    ]);
    v
}

/// A smaller set for pairs and triples, crossed with itself.
pub fn special_small() -> Vec<f64> {
    let mut v = vec![
        0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 3.0, -3.0, 1.5, -1.5, 0.25, 10.0, -10.0,
        f64::INFINITY, f64::NEG_INFINITY, f64::NAN, f64::MAX, -f64::MAX, f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE, 5e-324, -5e-324, f64::from_bits(0x000F_FFFF_FFFF_FFFF),
        1e-300, 1e300, 2147483648.0, -2147483649.0, 4294967296.0, 4294967297.0, 1e10,
        core::f64::consts::PI, -core::f64::consts::PI, core::f64::consts::FRAC_PI_2,
        1.0 + f64::EPSILON, 1.0 - f64::EPSILON / 2.0, 0.1, 0.7, 1e-10, 1e20, pow2(60),
        pow2(-60), pow2(61), pow2(-61), 7.0, 0.3, 1e-310, 3e-320,
    ];
    let mut r = Rng::new(0x5EED);
    for _ in 0..60 {
        v.push(focused(&mut r, -30, 30));
    }
    v
}

/// Exponents and bases pow treats specially, added to `special_small`.
fn pow_specials() -> Vec<f64> {
    let mut v = special_small();
    v.extend([
        4.0, -4.0, 5.0, -5.0, 0.75, -0.75, 2.5, -2.5, 1.0 / 3.0, 4503599627370497.0,
        9007199254740991.0, -9007199254740991.0, 9007199254740992.0, 1.8446744073709552e19,
        pow2(63), -pow2(63), pow2(64), pow2(-65), -pow2(-65), pow2(-66), 1023.0, 1024.0, 1074.0,
        1075.0, -1074.0, -1075.0, 1074.5, -1075.5, 709.0, 710.0, 0.9999999999999999,
        1.0000000000000002, -0.9999999999999999, -1.0000000000000002, core::f64::consts::E,
        core::f64::consts::LN_2, 0.1, 1e-5, 1e5, 1e15, 1e-15, 1e-320, -1e-320,
        f64::from_bits(0x7FF4_0000_0000_0000), -f64::NAN,
    ]);
    v
}

/// The special input tuples of a function, flattened.
pub fn special(id: usize) -> Vec<f64> {
    let fun = FUNS[id];
    if fun.name.contains("pow") {
        let s = pow_specials();
        let mut v = Vec::with_capacity(2 * s.len() * s.len());
        for &a in &s {
            for &b in &s {
                v.push(a);
                v.push(b);
            }
        }
        return v;
    }
    match fun.arity {
        1 => special_singles(),
        2 => {
            let s = special_small();
            let mut v = Vec::with_capacity(2 * s.len() * s.len());
            for &a in &s {
                for &b in &s {
                    v.push(a);
                    v.push(b);
                }
            }
            v
        }
        _ => {
            let s = special_small();
            let mut v = Vec::new();
            for &a in s.iter().step_by(2) {
                for &b in s.iter().step_by(2) {
                    for &c in s.iter().step_by(3) {
                        v.extend([a, b, c]);
                    }
                }
            }
            v
        }
    }
}

/// A random sign, an exponent in [emin, emax] (subnormal below -1022) and
/// random mantissa bits.
fn focused(r: &mut Rng, emin: i32, emax: i32) -> f64 {
    let e = emin + r.below((emax - emin + 1) as u64) as i32;
    let sign = r.next() & SIGN;
    if e < -1022 {
        // subnormal: keep the top bits of the mantissa below 2^(e+1074)
        let bits = (e + 1074).clamp(0, 52) as u32;
        let m = if bits == 0 { 1 } else { r.next() & ((1u64 << bits) - 1) | (1u64 << (bits - 1)) };
        return f64::from_bits(sign | m);
    }
    f64::from_bits(sign | (((e + 1023) as u64) << 52) | (r.next() & MANT))
}

/// A number with few significant bits (an integer, half, or simple fraction)
/// around 2^e.
fn short(r: &mut Rng, emin: i32, emax: i32) -> f64 {
    let bits = 1 + r.below(16) as u32;
    let m = (r.next() >> (64 - bits)) | 1;
    let e = emin + r.below((emax - emin + 1) as u64) as i32;
    let v = m as f64 * pow2((e - bits as i32).clamp(-1074, 1023));
    if r.next() & 1 == 0 {
        v
    } else {
        -v
    }
}

/// One random input, for a function with exponent range [emin, emax].
fn single(r: &mut Rng, fun: &Fun, anchors: &[f64]) -> f64 {
    let x = match r.below(100) {
        0..=24 => f64::from_bits(r.next()),
        25..=69 => focused(r, fun.emin, fun.emax),
        70..=89 => {
            let a = anchors[r.below(anchors.len() as u64) as usize].to_bits();
            let k = r.below(48);
            let d = r.next() & ((1u64 << k) - 1);
            let b = if r.next() & 1 == 0 { a.wrapping_add(d) } else { a.wrapping_sub(d) };
            f64::from_bits(b ^ (r.next() & SIGN))
        }
        _ => short(r, fun.emin.max(-1000), fun.emax.min(1000)),
    };
    if fun.positive && r.below(10) != 0 {
        f64::from_bits(x.to_bits() & !SIGN)
    } else {
        x
    }
}

/// A random (x, y) for pow, from a mixture of the cases its algorithm
/// distinguishes.
fn pow_pair(r: &mut Rng) -> (f64, f64) {
    match r.below(100) {
        0..=14 => (f64::from_bits(r.next()), f64::from_bits(r.next())),
        15..=34 => (focused(r, -1074, 1023), focused(r, -70, 70)),
        35..=54 => {
            // Results near 2^t for t up to past overflow and through the
            // subnormal range: y = t / log2(x), a little off.
            let x = abs(focused(r, -1074, 1023));
            let t = r.below(2200) as f64 - 1100.0 + (r.next() >> 11) as f64 * pow2(-53);
            let l = v8math::log2(x);
            let y = if l == 0.0 { focused(r, -10, 70) } else { t / l };
            let y = f64::from_bits(y.to_bits() ^ (r.next() & ((1u64 << r.below(30)) - 1)));
            let x = if r.below(4) == 0 { -x } else { x };
            let y = if x < 0.0 && r.below(2) == 0 { y.round() } else { y };
            (x, y)
        }
        55..=69 => {
            // x near +-1, y large
            let d = r.next() & ((1u64 << r.below(53)) - 1);
            let one = 0x3FF0_0000_0000_0000u64;
            let x = f64::from_bits(if r.below(2) == 0 { one + d } else { one - d.min(one - 1) });
            let x = if r.below(4) == 0 { -x } else { x };
            (x, focused(r, -10, 64))
        }
        70..=84 => {
            // integer and half-integer y, either sign of x
            let x = focused(r, -80, 80);
            let k = r.below(62) as u32;
            let i = (r.next() >> (63 - k)) as f64;
            let y = if r.below(2) == 0 { i } else { -i };
            let y = if r.below(5) == 0 { y + 0.5 } else { y };
            (x, y)
        }
        _ => (short(r, -40, 40), short(r, -12, 12)),
    }
}

fn abs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & !SIGN)
}

/// A random (a, b, c) for a fused multiply-add: c of a size near a * b's
/// or cancelling it, results near overflow and in the subnormal range, and
/// short mantissas that make ties.
fn fma_triple(r: &mut Rng) -> [f64; 3] {
    let ab = |r: &mut Rng, emin: i32, emax: i32| (focused(r, emin, emax), focused(r, emin, emax));
    match r.below(100) {
        0..=14 => [f64::from_bits(r.next()), f64::from_bits(r.next()), f64::from_bits(r.next())],
        15..=44 => {
            let (a, b) = ab(r, -520, 520);
            let e = (((a * b).to_bits() >> 52) & 0x7FF) as i32 - 1023;
            let d = r.below(240) as i32 - 120;
            [a, b, focused(r, (e + d).clamp(-1074, 1023), (e + d).clamp(-1074, 1023))]
        }
        45..=69 => {
            // c cancels a * b to within a few ulps
            let (a, b) = ab(r, -520, 520);
            let p = (a * b).to_bits() ^ (r.next() & ((1u64 << r.below(12)) - 1));
            [a, b, -f64::from_bits(p)]
        }
        70..=84 => {
            // products near overflow or underflow
            let ea = r.below(2200) as i32 - 1100;
            let a = focused(r, ea.clamp(-1074, 1023), ea.clamp(-1074, 1023));
            let eb = if r.below(2) == 0 { 1023 - ea } else { -1074 - ea + r.below(60) as i32 };
            let b = focused(r, eb.clamp(-1074, 1023), eb.clamp(-1074, 1023));
            [a, b, focused(r, -1074, 1023)]
        }
        _ => [short(r, -60, 60), short(r, -60, 60), short(r, -60, 60)],
    }
}

/// `n` random input tuples for chunk `chunk` of function `id`, flattened.
pub fn random(id: usize, seed: u64, chunk: u64, n: usize, out: &mut Vec<f64>) {
    let fun = FUNS[id];
    let anchors = anchors();
    let mut r = chunk_rng(id, seed, chunk);
    out.clear();
    out.reserve(n * fun.arity);
    if fun.name == "fma" {
        for _ in 0..n {
            out.extend(fma_triple(&mut r));
        }
        return;
    }
    if fun.name.contains("pow") {
        for _ in 0..n {
            let (x, y) = pow_pair(&mut r);
            out.push(x);
            out.push(y);
        }
        return;
    }
    for _ in 0..n {
        match fun.arity {
            1 => out.push(single(&mut r, &fun, &anchors)),
            _ => {
                let a = single(&mut r, &fun, &anchors);
                out.push(a);
                for _ in 1..fun.arity {
                    // Half the time, a partner whose exponent is near a's:
                    // within the range where the algorithms switch.
                    let b = if r.below(2) == 0 && a.is_finite() && a != 0.0 {
                        let ea = ((a.to_bits() >> 52) & 0x7FF) as i32 - 1023;
                        let d = r.below(141) as i32 - 70;
                        let e = (ea + d).clamp(-1074, 1023);
                        focused(&mut r, e, e)
                    } else {
                        single(&mut r, &fun, &anchors)
                    };
                    out.push(b);
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    //! Exports for check-wasm.mjs: it asks for a chunk of inputs, reads them
    //! from memory, computes V8's results, has this module compute its own,
    //! and compares.
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static INPUTS: RefCell<Vec<f64>> = RefCell::new(Vec::new());
        static OUTPUTS: RefCell<Vec<f64>> = RefCell::new(Vec::new());
        static NAMES: String = FUNS.iter().map(|f| f.name).collect::<Vec<_>>().join(",");
    }

    #[no_mangle]
    pub extern "C" fn names_ptr() -> usize {
        NAMES.with(|s| s.as_ptr() as usize)
    }

    #[no_mangle]
    pub extern "C" fn names_len() -> usize {
        NAMES.with(|s| s.len())
    }

    /// Fills the input buffer (special tuples when `n` is 0, else `n`
    /// random tuples of chunk `chunk`); returns how many tuples.
    #[no_mangle]
    pub extern "C" fn prepare(id: usize, seed: u64, chunk: u64, n: usize) -> usize {
        INPUTS.with(|v| {
            let mut v = v.borrow_mut();
            if n == 0 {
                *v = special(id);
            } else {
                random(id, seed, chunk, n, &mut v);
            }
            let tuples = v.len() / FUNS[id].arity;
            OUTPUTS.with(|o| o.borrow_mut().resize(tuples, 0.0));
            tuples
        })
    }

    /// Makes room for `tuples` input tuples of function `id`, to be written
    /// by the caller; returns where.
    #[no_mangle]
    pub extern "C" fn load(id: usize, tuples: usize) -> usize {
        OUTPUTS.with(|o| o.borrow_mut().resize(tuples, 0.0));
        INPUTS.with(|v| {
            let mut v = v.borrow_mut();
            v.resize(tuples * FUNS[id].arity, 0.0);
            v.as_ptr() as usize
        })
    }

    #[no_mangle]
    pub extern "C" fn inputs_ptr() -> usize {
        INPUTS.with(|v| v.borrow().as_ptr() as usize)
    }

    #[no_mangle]
    pub extern "C" fn outputs_ptr() -> usize {
        OUTPUTS.with(|v| v.borrow().as_ptr() as usize)
    }

    /// Computes every output from the inputs.
    #[no_mangle]
    pub extern "C" fn run(id: usize) {
        INPUTS.with(|i| OUTPUTS.with(|o| eval_all(id, &i.borrow(), &mut o.borrow_mut())));
    }
}
