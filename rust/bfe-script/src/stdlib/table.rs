//! Lua 5.0.3 table library (`src/lib/ltablib.c`, MIT License).

use crate::units::Units;
use crate::error::ScriptError;
use crate::numfmt;
use crate::value::{TableId, Value};
use crate::vm::{CallCtx, Vm};

pub fn open<H>(vm: &mut Vm<H>) -> Result<(), ScriptError> {
    vm.register_table(
        "table",
        &[
            ("concat", tab_concat),
            ("foreach", tab_foreach),
            ("foreachi", tab_foreachi),
            ("getn", tab_getn),
            ("setn", tab_setn),
            ("insert", tab_insert),
            ("remove", tab_remove),
            ("sort", tab_sort),
        ],
    )?;
    Ok(())
}

fn tab_foreachi<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let func = ctx.check_function(1)?;
    let n = ctx.vm_mut().table_getn_budgeted(tbl)?;
    for i in 1..=n {
        ctx.charge_work(1)?;
        let val = ctx.vm().table_raw_get_int(tbl, i)?;
        let first = ctx.call_one(func, &[Value::Number(i as f32), val])?;
        if !first.is_nil() {
            return ctx.push_return(first);
        }
    }
    Ok(())
}

fn tab_foreach<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let func = ctx.check_function(1)?;
    let mut key = Value::Nil;
    while let Some((k, v)) = ctx.vm_mut().table_next_charged(tbl, key)? {
        ctx.charge_work(1)?;
        key = k;
        // Root `key` across the callback so deleting `key` + GC cannot collect it
        // before the next `table_next(tbl, key)` step.
        let first = ctx.with_temp_roots(&[key], |ctx| ctx.call_one(func, &[k, v]))?;
        if !first.is_nil() {
            return ctx.push_return(first);
        }
    }
    Ok(())
}

fn tab_getn<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let n = ctx.vm_mut().table_getn_budgeted(tbl)?;
    ctx.push_return(Value::Number(n as f32))
}

fn tab_setn<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let n = ctx.check_int(1)?;
    ctx.vm_mut().table_setn(tbl, n)
}

fn tab_insert<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let base_n = ctx.vm_mut().table_getn_budgeted(tbl)?;
    let mut n = base_n
        .checked_add(1)
        .ok_or_else(|| ScriptError::runtime("table index overflow in `insert'"))?;
    let (pos, val) = if ctx.arg_count() == 2 {
        (n, ctx.arg(1))
    } else {
        let p = ctx.check_int(1)?;
        if p > n {
            n = p;
        }
        (p, ctx.arg(2))
    };
    ctx.vm_mut().table_setn(tbl, n)?;
    if let Some(mut i) = n.checked_sub(1) {
        while i >= pos {
            ctx.charge_work(1)?;
            let dst = i
                .checked_add(1)
                .ok_or_else(|| ScriptError::runtime("table index overflow in `insert'"))?;
            let elem = ctx.vm().table_raw_get_int(tbl, i)?;
            ctx.vm_mut()
                .table_raw_set(tbl, Value::Number(dst as f32), elem)?;
            match i.checked_sub(1) {
                Some(prev) => i = prev,
                None => break,
            }
        }
    }
    ctx.vm_mut()
        .table_raw_set(tbl, Value::Number(pos as f32), val)
}

fn tab_remove<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let n = ctx.vm_mut().table_getn_budgeted(tbl)?;
    // `luaL_optint(L, 2, n)`: the default passes through the float number type.
    let pos = ctx.opt_int(1, numfmt::c_int(n as f32))?;
    if n <= 0 {
        return Ok(());
    }
    ctx.vm_mut().table_setn(tbl, n - 1)?;
    let removed = ctx.vm().table_raw_get_int(tbl, pos)?;
    // Root `removed` across shifting writes that may trigger GC.
    ctx.with_temp_roots(&[removed], |ctx| {
        let mut i = pos;
        while i < n {
            ctx.charge_work(1)?;
            let next_i = i
                .checked_add(1)
                .ok_or_else(|| ScriptError::runtime("table index overflow in `remove'"))?;
            let next_v = ctx.vm().table_raw_get_int(tbl, next_i)?;
            ctx.vm_mut()
                .table_raw_set(tbl, Value::Number(i as f32), next_v)?;
            i = next_i;
        }
        ctx.vm_mut()
            .table_raw_set(tbl, Value::Number(n as f32), Value::Nil)?;
        ctx.push_return(removed)
    })
}

fn tab_concat<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    // Argument order of Lua 5.0.3 `str_concat`: 2, 3, 4, then the table.
    let sep_id = if ctx.arg(1).is_nil() {
        ctx.vm().well_known().empty
    } else {
        ctx.check_string_id(1)?
    };
    let sep = ctx.vm().string_rc_unchecked(sep_id);
    let start = ctx.opt_int(2, 1)?;
    let mut end = ctx.opt_int(3, 0)?;
    let tbl = ctx.check_table(0)?;
    if end == 0 {
        end = ctx.vm_mut().table_getn_budgeted(tbl)?;
    }
    let mem_limit = ctx.vm().limits().memory_limit;
    let mut out = Vec::new();
    let mut i = start;
    while i <= end {
        ctx.charge_work(1)?;
        let v = ctx.vm().table_raw_get_int(tbl, i)?;
        match v {
            Value::String(sid) => {
                let len = ctx.vm().string_bytes_unchecked(sid).len();
                let new_len = out
                    .len()
                    .checked_add(len)
                    .ok_or(ScriptError::MemoryLimit { limit: mem_limit })?;
                ctx.charge_work(len.units())?;
                ctx.reserve_output(&mut out, new_len)?;
                out.extend_from_slice(ctx.vm().string_bytes_unchecked(sid));
            }
            Value::Number(num) => {
                let text = numfmt::lua_number_text(num);
                let new_len = out
                    .len()
                    .checked_add(text.as_bytes().len())
                    .ok_or(ScriptError::MemoryLimit { limit: mem_limit })?;
                ctx.charge_work(1 + text.as_bytes().len().units() / crate::vm::BYTES_PER_UNIT)?;
                ctx.reserve_output(&mut out, new_len)?;
                out.extend_from_slice(text.as_bytes());
            }
            _ => {
                return Err(ctx.arg_error(0, "table contains non-strings"));
            }
        }
        if i != end {
            let new_len = out
                .len()
                .checked_add(sep.len())
                .ok_or(ScriptError::MemoryLimit { limit: mem_limit })?;
            ctx.charge_work(sep.len().units())?;
            ctx.reserve_output(&mut out, new_len)?;
            out.extend_from_slice(&sep);
        }
        match i.checked_add(1) {
            Some(next) => i = next,
            None => break,
        }
    }
    let sid = ctx.vm_mut().intern_bytes(&out)?;
    ctx.push_return(Value::String(sid))
}

fn tab_sort<H>(ctx: &mut CallCtx<'_, H>) -> Result<(), ScriptError> {
    let tbl = ctx.check_table(0)?;
    let n = ctx.vm_mut().table_getn_budgeted(tbl)?;
    let comp = if ctx.arg(1).is_nil() {
        None
    } else {
        Some(ctx.check_function(1)?)
    };
    if n > 1 {
        aux_sort(ctx, tbl, comp, 1, n)?;
    }
    Ok(())
}

fn sort_comp<H>(
    ctx: &mut CallCtx<'_, H>,
    comp: Option<Value>,
    a: Value,
    b: Value,
) -> Result<bool, ScriptError> {
    ctx.charge_work(1)?;
    if let Some(func) = comp {
        let res = ctx.call_one(func, &[a, b])?;
        Ok(res.is_truthy())
    } else {
        let (vm, host) = ctx.split();
        vm.lua_lessthan(host, a, b)
    }
}

fn write_pair<H>(
    ctx: &mut CallCtx<'_, H>,
    tbl: TableId,
    i: i32,
    vi: Value,
    j: i32,
    vj: Value,
) -> Result<(), ScriptError> {
    ctx.with_temp_roots(&[vi, vj], |ctx| {
        ctx.vm_mut()
            .table_raw_set(tbl, Value::Number(i as f32), vi)?;
        ctx.vm_mut()
            .table_raw_set(tbl, Value::Number(j as f32), vj)
    })
}

fn swap_slots<H>(ctx: &mut CallCtx<'_, H>, tbl: TableId, i: i32, j: i32) -> Result<(), ScriptError> {
    let vi = ctx.vm().table_raw_get_int(tbl, i)?;
    let vj = ctx.vm().table_raw_get_int(tbl, j)?;
    write_pair(ctx, tbl, i, vj, j, vi)
}

/// Lua 5.0 Sedgewick quicksort (`auxsort` from `src/lib/ltablib.c`).
///
/// Preserves and roots the compared values through each swap (just as C Lua's
/// `set2` writes the two values already pushed on the Lua stack) rather than
/// rereading from the table after a comparator callback may have mutated it.
fn aux_sort<H>(
    ctx: &mut CallCtx<'_, H>,
    tbl: TableId,
    comp: Option<Value>,
    mut l: i32,
    mut u: i32,
) -> Result<(), ScriptError> {
    while l < u {
        let vl = ctx.vm().table_raw_get_int(tbl, l)?;
        let vu = ctx.vm().table_raw_get_int(tbl, u)?;
        ctx.with_temp_roots(&[vl, vu], |ctx| {
            if sort_comp(ctx, comp, vu, vl)? {
                write_pair(ctx, tbl, l, vu, u, vl)?;
            }
            Ok(())
        })?;
        if u - l == 1 {
            break;
        }
        let mid = l + (u - l) / 2;
        let vmid = ctx.vm().table_raw_get_int(tbl, mid)?;
        let vl = ctx.vm().table_raw_get_int(tbl, l)?;
        ctx.with_temp_roots(&[vmid, vl], |ctx| {
            if sort_comp(ctx, comp, vmid, vl)? {
                write_pair(ctx, tbl, mid, vl, l, vmid)
            } else {
                let vu = ctx.vm().table_raw_get_int(tbl, u)?;
                ctx.with_temp_roots(&[vu], |ctx| {
                    if sort_comp(ctx, comp, vu, vmid)? {
                        write_pair(ctx, tbl, mid, vu, u, vmid)?;
                    }
                    Ok(())
                })
            }
        })?;
        if u - l == 2 {
            break;
        }
        let pivot = ctx.vm().table_raw_get_int(tbl, mid)?;
        let v_u1 = ctx.vm().table_raw_get_int(tbl, u - 1)?;
        let (rl, ru) = ctx.with_temp_roots(&[pivot], |ctx| {
            write_pair(ctx, tbl, mid, v_u1, u - 1, pivot)?;
            let mut i = l;
            let mut j = u - 1;
            loop {
                let vi = loop {
                    i += 1;
                    let candidate = ctx.vm().table_raw_get_int(tbl, i)?;
                    let less = ctx.with_temp_roots(&[candidate], |ctx| {
                        sort_comp(ctx, comp, candidate, pivot)
                    })?;
                    if !less {
                        break candidate;
                    }
                    if i > u {
                        return Err(ctx.lua_error("invalid order function for sorting"));
                    }
                };
                let should_break = ctx.with_temp_roots(&[vi], |ctx| {
                    let vj = loop {
                        j -= 1;
                        let candidate = ctx.vm().table_raw_get_int(tbl, j)?;
                        let less = ctx.with_temp_roots(&[candidate], |ctx| {
                            sort_comp(ctx, comp, pivot, candidate)
                        })?;
                        if !less {
                            break candidate;
                        }
                        if j < l {
                            return Err(ctx.lua_error("invalid order function for sorting"));
                        }
                    };
                    if j < i {
                        return Ok(true);
                    }
                    write_pair(ctx, tbl, i, vj, j, vi)?;
                    Ok(false)
                })?;
                if should_break {
                    break;
                }
            }
            swap_slots(ctx, tbl, u - 1, i)?;
            let sub = if i - l < u - i {
                let left = (l, i - 1);
                l = i + 1;
                left
            } else {
                let right = (i + 1, u);
                u = i - 1;
                right
            };
            Ok(sub)
        })?;
        aux_sort(ctx, tbl, comp, rl, ru)?;
    }
    Ok(())
}
