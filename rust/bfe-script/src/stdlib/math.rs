//! Lua 5.0.3 math library (`src/lib/lmathlib.c`, MIT License).
//!
//! All transcendental functions (`sin`, `cos`, `tan`, `asin`, `acos`, `atan`,
//! `atan2`, `exp`, `log`, `log10`, `pow`) go through the workspace `v8math`
//! crate (`v8math::glibc_pow` for C99/Lua 5.0 `pow` special cases) rather than
//! the host C `libm`. `mod`, `frexp`, and `ldexp` are computed with pure
//! IEEE-754 bit operations, and `random`/`randomseed` call the host-supplied
//! [`crate::vm::ScriptRng`].

use crate::error::ScriptError;
use crate::value::Value;
use crate::vm::{CallCtx, Vm};

const PI_F64: f64 = core::f64::consts::PI;
const RADIANS_PER_DEGREE: f64 = PI_F64 / 180.0;

pub fn open<H>(vm: &mut Vm<H>) -> Result<(), ScriptError> {
    let tbl = vm.register_table(
        "math",
        &[
            ("abs", math_abs),
            ("acos", math_acos),
            ("asin", math_asin),
            ("atan", math_atan),
            ("atan2", math_atan2),
            ("ceil", math_ceil),
            ("cos", math_cos),
            ("deg", math_deg),
            ("exp", math_exp),
            ("floor", math_floor),
            ("frexp", math_frexp),
            ("ldexp", math_ldexp),
            ("log", math_log),
            ("log10", math_log10),
            ("max", math_max),
            ("min", math_min),
            ("mod", math_mod),
            ("pow", math_pow),
            ("rad", math_rad),
            ("random", math_random),
            ("randomseed", math_randomseed),
            ("sin", math_sin),
            ("sqrt", math_sqrt),
            ("tan", math_tan),
        ],
    )?;
    vm.table_set_str(tbl, "pi", Value::Number(PI_F64 as f32))?;
    // Lua 5.0 `^` operator looks up global `__pow`, installed by `luaopen_math`.
    vm.register_fn("__pow", math_pow)?;
    Ok(())
}

fn math_abs<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(x.abs()))
}

fn math_sin<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::sin(x as f64) as f32))
}

fn math_cos<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::cos(x as f64) as f32))
}

fn math_tan<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::tan(x as f64) as f32))
}

fn math_asin<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::asin(x as f64) as f32))
}

fn math_acos<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::acos(x as f64) as f32))
}

fn math_atan<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::atan(x as f64) as f32))
}

fn math_atan2<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let y = ctx.check_number(0)?;
    let x = ctx.check_number(1)?;
    ctx.push_return(Value::Number(v8math::atan2(y as f64, x as f64) as f32))
}

fn math_ceil<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(x.ceil()))
}

fn math_floor<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(x.floor()))
}

fn math_sqrt<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(x.sqrt()))
}

fn math_pow<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    let y = ctx.check_number(1)?;
    // Use glibc/C99 pow semantics (pow(1, NaN) == 1, pow(+-1, +-inf) == 1) rather than JS Math.pow.
    ctx.push_return(Value::Number(v8math::glibc_pow(x as f64, y as f64) as f32))
}

fn math_exp<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::exp(x as f64) as f32))
}

fn math_log<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::log(x as f64) as f32))
}

fn math_log10<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(v8math::log10(x as f64) as f32))
}

fn math_deg<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(((x as f64) / RADIANS_PER_DEGREE) as f32))
}

fn math_rad<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    ctx.push_return(Value::Number(((x as f64) * RADIANS_PER_DEGREE) as f32))
}

fn math_mod<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    let y = ctx.check_number(1)?;
    ctx.push_return(Value::Number(pure_fmod_f32(x, y)))
}

fn math_frexp<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    let (m, e) = pure_frexp_f32(x);
    ctx.push_return(Value::Number(m))?;
    ctx.push_return(Value::Number(e as f32))
}

fn math_ldexp<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let x = ctx.check_number(0)?;
    let e = ctx.check_int(1)?;
    ctx.push_return(Value::Number(pure_ldexp_f32(x, e)))
}

fn math_min<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let n = ctx.arg_count();
    let mut dmin = ctx.check_number(0)?;
    for i in 1..n {
        let d = ctx.check_number(i)?;
        if d < dmin {
            dmin = d;
        }
    }
    ctx.push_return(Value::Number(dmin))
}

fn math_max<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let n = ctx.arg_count();
    let mut dmax = ctx.check_number(0)?;
    for i in 1..n {
        let d = ctx.check_number(i)?;
        if d > dmax {
            dmax = d;
        }
    }
    ctx.push_return(Value::Number(dmax))
}

fn math_random<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let r = ctx.vm_mut().rng_mut().next_unit();
    match ctx.arg_count() {
        0 => ctx.push_return(Value::Number(r)),
        1 => {
            let u = ctx.check_int(0)?;
            if u < 1 {
                return Err(ctx.arg_error(0, "interval is empty"));
            }
            let val = (f64::from(r) * f64::from(u)).floor() + 1.0;
            let clamped = val.clamp(1.0, f64::from(u));
            ctx.push_return(Value::Number(clamped as f32))
        }
        2 => {
            let l = ctx.check_int(0)?;
            let u = ctx.check_int(1)?;
            if l > u {
                return Err(ctx.arg_error(1, "interval is empty"));
            }
            let span = f64::from(u) - f64::from(l) + 1.0;
            let val = (f64::from(r) * span).floor() + f64::from(l);
            let clamped = val.clamp(f64::from(l), f64::from(u));
            ctx.push_return(Value::Number(clamped as f32))
        }
        _ => Err(ctx.lua_error("wrong number of arguments")),
    }
}

fn math_randomseed<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let seed = ctx.check_int(0)?;
    ctx.vm_mut().rng_mut().seed(seed);
    Ok(())
}

/// Exact IEEE-754 `fmodf(x, y)` using pure integer bit operations (zero host libm).
fn pure_fmod_f32(x: f32, y: f32) -> f32 {
    if x.is_nan() || y.is_nan() || x.is_infinite() || y == 0.0 {
        return f32::NAN;
    }
    if y.is_infinite() || x == 0.0 {
        return x;
    }
    let sign = x.to_bits() & 0x8000_0000;
    let (mut ux, mut ex) = unpack_positive_f32(x.abs());
    let (uy, ey) = unpack_positive_f32(y.abs());

    if ex < ey || (ex == ey && ux < uy) {
        return x;
    }
    if ex == ey && ux == uy {
        return f32::from_bits(sign);
    }

    // Binary long division on exact 24-bit significands.
    let mut n = ex - ey;
    while n > 0 {
        if ux >= uy {
            ux -= uy;
            if ux == 0 {
                return f32::from_bits(sign);
            }
        }
        ux <<= 1;
        n -= 1;
    }
    if ux >= uy {
        ux -= uy;
        if ux == 0 {
            return f32::from_bits(sign);
        }
    }
    ex = ey;
    while (ux & 0x0080_0000) == 0 {
        ux <<= 1;
        ex -= 1;
    }
    pack_positive_f32(sign, ux, ex)
}

fn unpack_positive_f32(v: f32) -> (u32, i32) {
    let bits = v.to_bits();
    let raw_exp = ((bits >> 23) & 0xFF) as i32;
    let mant = bits & 0x007F_FFFF;
    if raw_exp == 0 {
        let mut m = mant;
        let mut e = -126;
        while (m & 0x0080_0000) == 0 {
            m <<= 1;
            e -= 1;
        }
        (m, e)
    } else {
        (mant | 0x0080_0000, raw_exp - 127)
    }
}

fn pack_positive_f32(sign: u32, mant: u32, exp: i32) -> f32 {
    if exp >= 128 {
        return f32::from_bits(sign | 0x7F80_0000);
    }
    if exp >= -126 {
        let biased = (exp + 127) as u32;
        return f32::from_bits(sign | (biased << 23) | (mant & 0x007F_FFFF));
    }
    let shift = (-126 - exp) as u32;
    if shift >= 24 {
        return f32::from_bits(sign);
    }
    f32::from_bits(sign | (mant >> shift))
}

fn pure_frexp_f32(x: f32) -> (f32, i32) {
    if x == 0.0 || !x.is_finite() {
        return (x, 0);
    }
    let sign = x.to_bits() & 0x8000_0000;
    let (mant, exp) = unpack_positive_f32(x.abs());
    // frexp returns m in [0.5, 1.0) and e such that x = m * 2^e.
    let m = f32::from_bits(sign | (126u32 << 23) | (mant & 0x007F_FFFF));
    (m, exp + 1)
}

fn pure_ldexp_f32(x: f32, exp: i32) -> f32 {
    if x == 0.0 || !x.is_finite() || exp == 0 {
        return x;
    }
    let sign = x.to_bits() & 0x8000_0000;
    let (mant, cur_exp) = unpack_positive_f32(x.abs());
    let new_exp = cur_exp.saturating_add(exp);
    if new_exp >= 128 {
        return f32::from_bits(sign | 0x7F80_0000);
    }
    if new_exp >= -126 {
        let biased = (new_exp + 127) as u32;
        return f32::from_bits(sign | (biased << 23) | (mant & 0x007F_FFFF));
    }
    // Subnormal rounding to nearest even.
    let shift = (-126 - new_exp) as u32;
    if shift >= 25 {
        return f32::from_bits(sign);
    }
    let low_mask = (1u32 << shift) - 1;
    let half = 1u32 << (shift - 1);
    let rem = mant & low_mask;
    let mut sub = mant >> shift;
    if rem > half || (rem == half && (sub & 1) == 1) {
        sub += 1;
    }
    f32::from_bits(sign | sub)
}
