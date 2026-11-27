//! Lua 5.0.3 basic library (`src/lib/lbaselib.c`, MIT License).

use crate::units::Units;
use crate::error::ScriptError;
use crate::numfmt;
use crate::value::Value;
use crate::vm::{CallCtx, Vm};

pub(super) fn functions<H>() -> [(&'static str, crate::vm::NativeFn<H>); 21] {
    [
        ("assert", lua_assert),
        ("collectgarbage", lua_collectgarbage),
        ("error", lua_error),
        ("gcinfo", lua_gcinfo),
        ("getfenv", lua_getfenv),
        ("getmetatable", lua_getmetatable),
        ("ipairs", lua_ipairs),
        ("next", lua_next),
        ("pairs", lua_pairs),
        ("pcall", lua_pcall),
        ("print", lua_print),
        ("rawequal", lua_rawequal),
        ("rawget", lua_rawget),
        ("rawset", lua_rawset),
        ("setfenv", lua_setfenv),
        ("setmetatable", lua_setmetatable),
        ("tonumber", lua_tonumber),
        ("tostring", lua_tostring),
        ("type", lua_type),
        ("unpack", lua_unpack),
        ("xpcall", lua_xpcall),
    ]
}

pub fn open<H>(vm: &mut Vm<H>) -> Result<(), ScriptError> {
    let g = vm.globals();
    vm.set_global("_G", Value::Table(g))?;
    let ver = Value::String(vm.intern_str("Lua 5.0.3")?);
    // Rooted until stored: interning the global's name may allocate, and so collect.
    vm.with_temp_roots(&[ver], |vm| vm.set_global("_VERSION", ver))?;

    for (name, func) in functions() {
        vm.register_fn(name, func)?;
    }
    Ok(())
}

fn lua_assert<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let v = ctx.check_any(0)?;
    if v.is_falsy() {
        // `luaL_error(L, "%s", luaL_optstring(L, 2, "assertion failed!"))`: the message
        // is printed with `%s`, so it ends at its first NUL byte.
        let msg = if ctx.arg(1).is_nil() {
            b"assertion failed!".to_vec()
        } else {
            let sid = ctx.check_string_id(1)?;
            let len = ctx.vm().string_bytes_unchecked(sid).len();
            ctx.charge_work(1 + len.units() / crate::vm::BYTES_PER_UNIT)?;
            numfmt::cstr_prefix(ctx.vm().string_bytes_unchecked(sid)).to_vec()
        };
        return Err(ctx.lua_error_bytes(&msg));
    }
    // Lua 5.0 `assert` returns only its first argument.
    ctx.push_return(v)
}

fn lua_collectgarbage<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    // `lua_setgcthreshold(L, luaL_optint(L, 1, 0))`: a threshold in KB; a collection
    // runs when the count reaches it (always for 0). A negative value, cast to an
    // unsigned size by Lua 5.0, sets the largest threshold.
    let limit_kb = ctx.opt_int(0, 0)?;
    let threshold = if limit_kb < 0 {
        usize::MAX
    } else {
        (limit_kb as usize).saturating_mul(1024)
    };
    ctx.vm_mut().heap.set_threshold(threshold);
    if ctx.vm().memory_bytes() >= threshold {
        ctx.vm_mut().collect_garbage();
    }
    Ok(())
}

fn lua_gcinfo<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let used_kb = (ctx.vm().memory_bytes() / 1024) as f32;
    let thresh_kb = (ctx.vm().heap.gc_threshold / 1024) as f32;
    ctx.push_return(Value::Number(used_kb))?;
    ctx.push_return(Value::Number(thresh_kb))
}

fn lua_error<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let level = ctx.opt_int(1, 1)?;
    let msg_val = ctx.check_any(0)?;
    let is_str_or_num = matches!(msg_val, Value::String(_) | Value::Number(_));
    // The message is copied into the error, and `luaL_where` walks up to `level` frames.
    if let Value::String(sid) = msg_val {
        let len = ctx.vm().string_bytes_unchecked(sid).len();
        ctx.charge_work(1 + len.units() / crate::vm::BYTES_PER_UNIT)?;
    }
    ctx.charge_work(1 + level_walk(ctx, level) / crate::vm::BYTES_PER_UNIT)?;
    // Match Lua 5.0 `luaB_error` (`lbaselib.c:82`): only `level == 0` skips
    // concatenation/coercion; negative levels call `luaL_where(L, level)` (empty)
    // and still coerce numbers to strings while preserving raw byte strings verbatim.
    if !is_str_or_num || level == 0 {
        ctx.vm_mut().set_error_value(msg_val);
        if let Value::String(sid) = msg_val {
            let text = ctx.vm().string_rc_unchecked(sid);
            return Err(ctx.vm_mut().display_error(&text));
        }
        return Err(ScriptError::Runtime(
            ctx.vm().default_tostring_unchecked(msg_val),
        ));
    }
    let prefix = ctx.vm().where_bytes(level);
    let error = match msg_val {
        Value::String(sid) => {
            let text = ctx.vm().string_rc_unchecked(sid);
            ctx.vm_mut().raise_prefixed(prefix.as_bytes(), &text)
        }
        Value::Number(n) => ctx
            .vm_mut()
            .raise_prefixed(prefix.as_bytes(), numfmt::lua_number_text(n).as_bytes()),
        _ => unreachable!(),
    };
    Err(error)
}

fn lua_getmetatable<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let obj = ctx.check_any(0)?;
    let Some(mt) = ctx.vm().get_metatable(obj) else {
        return ctx.push_return(Value::Nil);
    };
    let field = ctx.vm().well_known().metatable;
    let protected = ctx.vm().table_raw_get(mt, Value::String(field))?;
    if !protected.is_nil() {
        ctx.push_return(protected)
    } else {
        ctx.push_return(Value::Table(mt))
    }
}

fn lua_setmetatable<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    // `luaL_checktype(L, 1, LUA_TTABLE)` first, then the type of argument 2 (absent
    // counts as neither nil nor table).
    let tbl = ctx.check_table(0)?;
    let new_mt = match (ctx.arg_count() >= 2, ctx.arg(1)) {
        (true, Value::Nil) => None,
        (true, Value::Table(id)) => Some(id),
        _ => {
            return Err(ctx.arg_error(1, "nil or table expected"));
        }
    };
    let mf = ctx.vm().well_known().metatable;
    if !ctx.vm().get_metafield(Value::Table(tbl), mf).is_nil() {
        return Err(ctx.lua_error("cannot change a protected metatable"));
    }
    ctx.vm_mut().set_table_metatable(tbl, new_mt)?;
    ctx.push_return(Value::Table(tbl))
}

enum FenvTarget {
    Global,
    Closure(crate::value::ClosureId),
    Native,
}

/// Frames a walk to stack level `level` can visit (the walk stops at the bottom).
fn level_walk<H>(ctx: &CallCtx<'_, H>, level: i32) -> u32 {
    level.max(0).unsigned_abs().min(ctx.vm().limits().max_call_depth.units())
}

fn resolve_fenv_target<H>(ctx: &mut CallCtx<'_, H>) -> Result<FenvTarget, ScriptError> {
    let arg = ctx.arg(0);
    if let Value::Closure(cid) = arg {
        return Ok(FenvTarget::Closure(cid));
    }
    if matches!(arg, Value::NativeFn(_)) {
        return Ok(FenvTarget::Native);
    }
    let level = ctx.opt_int(0, 1)?;
    if level < 0 {
        return Err(ctx.arg_error(0, "level must be non-negative"));
    }
    if level == 0 {
        // Level 0 is the running native function itself (`getfenv`/`setfenv`). Lua 5.0.3
        // replaces the thread's globals only when the argument, unrounded, is exactly 0
        // (`lua_tonumber(L, 1) == 0`); `0.5`, `-0.5` or `"0.75"` select the native.
        let exact_zero = ctx.vm_mut().coerce_number(arg)? == Some(0.0);
        return Ok(if exact_zero {
            FenvTarget::Global
        } else {
            FenvTarget::Native
        });
    }
    ctx.charge_work(1 + level_walk(ctx, level) / crate::vm::BYTES_PER_UNIT)?;
    match ctx.vm().resolve_stack_level(level) {
        crate::vm::StackLevel::Lua(cid) => Ok(FenvTarget::Closure(cid)),
        crate::vm::StackLevel::Native => Ok(FenvTarget::Native),
        crate::vm::StackLevel::TailCall => {
            Err(ctx.lua_error(&format!("no function environment for tail call at level {level}")))
        }
        crate::vm::StackLevel::None => Err(ctx.arg_error(0, "invalid level")),
    }
}

fn lua_getfenv<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let env = match resolve_fenv_target(ctx)? {
        FenvTarget::Global | FenvTarget::Native => ctx.vm().globals(),
        FenvTarget::Closure(cid) => ctx.vm().closure_env(cid),
    };
    let fenv_key = ctx.vm().well_known().fenv_tm;
    let protected = ctx.vm().table_raw_get(env, Value::String(fenv_key))?;
    if !protected.is_nil() {
        ctx.push_return(protected)
    } else {
        ctx.push_return(Value::Table(env))
    }
}

fn lua_setfenv<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let new_env = ctx.check_table(1)?;
    let target = resolve_fenv_target(ctx)?;
    let cur_env = match target {
        FenvTarget::Global | FenvTarget::Native => ctx.vm().globals(),
        FenvTarget::Closure(cid) => ctx.vm().closure_env(cid),
    };
    let fenv_key = ctx.vm().well_known().fenv_tm;
    if !ctx
        .vm()
        .table_raw_get(cur_env, Value::String(fenv_key))?
        .is_nil()
    {
        return Err(ctx.lua_error("`setfenv' cannot change a protected environment"));
    }
    match target {
        FenvTarget::Global => ctx.vm_mut().set_globals(new_env),
        FenvTarget::Closure(cid) => ctx.vm_mut().set_closure_env(cid, new_env),
        FenvTarget::Native => {
            return Err(ctx.lua_error("`setfenv' cannot change environment of given function"));
        }
    }
    Ok(())
}

fn lua_next<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let key = ctx.arg(1);
    // An invalid key raises `luaG_runerror` from a C function: no location prefix.
    match ctx.vm_mut().table_next_charged(tbl, key)? {
        Some((k, v)) => {
            ctx.push_return(k)?;
            ctx.push_return(v)
        }
        None => ctx.push_return(Value::Nil),
    }
}

fn lua_pairs<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let next_name = ctx.vm().well_known().next_name;
    let g = ctx.vm().globals();
    let next_fn = ctx.vm().table_raw_get(g, Value::String(next_name))?;
    ctx.push_return(next_fn)?;
    ctx.push_return(Value::Table(tbl))?;
    ctx.push_return(Value::Nil)
}

fn lua_ipairs<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    // Lua 5.0 `luaB_ipairs`: acts as both factory (1 arg) and iterator step (2 args).
    if ctx.arg_count() < 2 {
        let ipairs_name = ctx.vm().well_known().ipairs_name;
        let g = ctx.vm().globals();
        let ipairs_fn = ctx.vm().table_raw_get(g, Value::String(ipairs_name))?;
        ctx.push_return(ipairs_fn)?;
        ctx.push_return(Value::Table(tbl))?;
        ctx.push_return(Value::Number(0.0))
    } else {
        // `lua_tonumber` (0 for a non-number) plus one, then `(int)i` for the index.
        let arg = ctx.arg(1);
        let i = ctx.vm_mut().coerce_number(arg)?.unwrap_or(0.0) + 1.0;
        let idx = numfmt::c_int(i);
        let val = ctx.vm().table_raw_get_int(tbl, idx)?;
        if val.is_nil() {
            Ok(())
        } else {
            ctx.push_return(Value::Number(i))?;
            ctx.push_return(val)
        }
    }
}

fn lua_pcall<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let _ = ctx.check_any(0)?;
    ctx.pcall_args_and_return(1, None)
}

fn lua_xpcall<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    // `luaL_checkany(L, 2)`; Lua 5.0's `xpcall` passes no arguments to the function.
    let err_fn = ctx.check_any(1)?;
    let arg_count = ctx.arg_count();
    ctx.pcall_args_and_return(arg_count, Some(err_fn))
}

fn lua_print<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let n = ctx.arg_count();
    // `lua_getglobal(L, "tostring")`: an ordinary global lookup (honours `_G`'s
    // `__index`), done once before the loop as Lua 5.0 does.
    let tostring_name = ctx.vm().well_known().tostring_name;
    let g = ctx.vm().globals();
    let tostring_fn = {
        let (vm, host) = ctx.split();
        vm.index_value(host, Value::Table(g), Value::String(tostring_name))?
    };
    // Root `tostring_fn` across the loop so a `__tostring` metamethod that replaces
    // `_G.tostring` and triggers GC cannot collect the original function while in use.
    // Match Lua 5.0 `luaB_print`: write each argument (up to first NUL, like `fputs`)
    // immediately after its `tostring` call, then emit `\n` at the end.
    ctx.with_temp_roots(&[tostring_fn], |ctx| {
        for i in 0..n {
            let arg = ctx.arg(i);
            let sv = ctx.call_one(tostring_fn, &[arg])?;
            match sv {
                Value::String(sid) => {
                    let rc = ctx.vm().string_rc_unchecked(sid);
                    // `fputs` stops at a NUL: the scan is charged before it runs.
                    ctx.charge_work(rc.len().units() / crate::vm::BYTES_PER_UNIT)?;
                    let bytes = numfmt::cstr_prefix(&rc);
                    if i > 0 {
                        ctx.vm_mut().write_output(b"\t")?;
                    }
                    ctx.vm_mut().write_output(bytes)?;
                }
                Value::Number(num) => {
                    let text = numfmt::lua_number_text(num);
                    if i > 0 {
                        ctx.vm_mut().write_output(b"\t")?;
                    }
                    ctx.vm_mut().write_output(text.as_bytes())?;
                }
                _ => {
                    return Err(ctx.lua_error("`tostring' must return a string to `print'"));
                }
            }
        }
        ctx.vm_mut().write_output(b"\n")
    })
}

fn lua_rawequal<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let a = ctx.check_any(0)?;
    let b = ctx.check_any(1)?;
    ctx.push_return(Value::Bool(a.rawequal(b)))
}

fn lua_rawget<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let key = ctx.check_any(1)?;
    let val = ctx.vm().table_raw_get(tbl, key)?;
    ctx.push_return(val)
}

fn lua_rawset<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let key = ctx.check_any(1)?;
    let val = ctx.check_any(2)?;
    ctx.vm_mut().table_raw_set(tbl, key, val)?;
    ctx.push_return(Value::Table(tbl))
}

fn lua_tonumber<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let base = ctx.opt_int(1, 10)?;
    if base == 10 {
        let v = ctx.check_any(0)?;
        if let Some(n) = ctx.vm_mut().coerce_number(v)? {
            return ctx.push_return(Value::Number(n));
        }
        return ctx.push_return(Value::Nil);
    }
    // `luaL_checkstring(L, 1)` comes before the base range check.
    let sid = ctx.check_string_id(0)?;
    if !(2..=36).contains(&base) {
        return Err(ctx.arg_error(1, "base out of range"));
    }
    let len = ctx.vm().string_bytes_unchecked(sid).len();
    ctx.charge_work(1 + len.units() / crate::vm::BYTES_PER_UNIT)?;
    let parsed = numfmt::parse_lua_int_radix(ctx.vm().string_bytes_unchecked(sid), base as u32);
    match parsed {
        Some(n) => ctx.push_return(Value::Number(n)),
        None => ctx.push_return(Value::Nil),
    }
}

fn lua_tostring<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let v = ctx.check_any(0)?;
    let tm_key = ctx.vm().well_known().tostring_tm;
    let tm = ctx.vm().get_metafield(v, tm_key);
    if !tm.is_nil() {
        let res = ctx.call_one(tm, &[v])?;
        return ctx.push_return(res);
    }
    match v {
        Value::String(_) => ctx.push_return(v),
        Value::Number(n) => {
            let text = numfmt::lua_number_text(n);
            let sid = ctx.vm_mut().intern_bytes(text.as_bytes())?;
            ctx.push_return(Value::String(sid))
        }
        _ => {
            let text = ctx.vm().default_tostring_unchecked(v);
            let sid = ctx.vm_mut().intern_str(&text)?;
            ctx.push_return(Value::String(sid))
        }
    }
}

fn lua_type<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let v = ctx.check_any(0)?;
    let sid = ctx.vm_mut().intern_str(v.type_name())?;
    ctx.push_return(Value::String(sid))
}

/// `LUA_MAXCSTACK` of Lua 5.0.3: the most values a native function's frame may hold.
const LUA_MAXCSTACK: i32 = 2048;

fn lua_unpack<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let n = ctx.vm_mut().table_getn_budgeted(tbl)?;
    // `luaL_checkstack(L, n, "table too big to unpack")`.
    if i32::try_from(ctx.arg_count()).unwrap_or(i32::MAX).saturating_add(n) > LUA_MAXCSTACK {
        return Err(ctx.lua_error("stack overflow (table too big to unpack)"));
    }
    if n < 0 {
        return Ok(());
    }
    for i in 1..=n {
        ctx.charge_work(1)?;
        let v = ctx.vm().table_raw_get_int(tbl, i)?;
        ctx.push_return(v)?;
    }
    Ok(())
}
