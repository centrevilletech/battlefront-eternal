//! Narrow adapter over `bfe-script` and scoped access to the active
//! battle [`RuleActions`] port.
//!
//! All interaction between the simulation's script host and `bfe-script` passes
//! through this module so upstream runtime changes stay isolated to one file.

use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

pub use bfe_script::vm::InstructionSite;
pub use bfe_script::{
    ClosureId as ScriptClosureId,
    NativeFnId as ScriptNativeFnId, ScriptError, ScriptRng,
    StringId as ScriptStringId, TableId as ScriptTableId, UserdataId as ScriptUserdataId,
    Value as ScriptValue, ValueRef as ScriptValueRef, Vm as ScriptVm, VmLimits as ScriptLimits,
};

use crate::battle::rules::events::RuleActions;

pub type ScriptCallCtx<'a, H> = bfe_script::CallCtx<'a, H>;
pub type ScriptNativeFn<H> = bfe_script::NativeFn<H>;

/// Maximum positive integer handle that has an exact, distinct IEEE-754 `f32`
/// representation (`2^24`). Handles above this would alias adjacent IDs in Lua.
pub const MAX_EXACT_LUA_HANDLE: u32 = 1 << 24;

// Thread-local scoped pointer to the active `&mut dyn RuleActions` for the
// duration of a single `EventRules::tick` call.
//
// Soundness invariants:
// 1. `enter_actions_scope` asserts the slot is `None`, erases the borrow's
//    lifetime only until its RAII `ScopeGuard` drops (including on unwind),
//    and unconditionally clears the slot on drop.
// 2. `with_actions` removes (`take`) the pointer from the slot before creating
//    `&mut dyn RuleActions` and restores it via an RAII guard on exit/unwind,
//    making nested/aliased `&mut` references impossible.
thread_local! {
    static ACTIVE_ACTIONS: Cell<Option<NonNull<dyn RuleActions>>> = const { Cell::new(None) };
}

struct ScopeGuard;

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        ACTIVE_ACTIONS.with(|slot| slot.set(None));
    }
}

struct BorrowRestoreGuard(Option<NonNull<dyn RuleActions>>);

impl Drop for BorrowRestoreGuard {
    fn drop(&mut self) {
        if let Some(ptr) = self.0.take() {
            ACTIVE_ACTIONS.with(|slot| slot.set(Some(ptr)));
        }
    }
}

/// Bind `actions` for the synchronous duration of `f` so host native callbacks
/// invoked by `bfe-script` can read and mutate authoritative simulation state.
pub(crate) fn enter_actions_scope<R>(actions: &mut dyn RuleActions, f: impl FnOnce() -> R) -> R {
    let raw: *mut (dyn RuleActions + '_) = actions;
    // SAFETY: `raw` is a live exclusive reference for the entire call to `f`.
    // `ScopeGuard` clears `ACTIVE_ACTIONS` before `enter_actions_scope` returns
    // or unwinds, so the `'static` trait-object pointer never outlives `actions`.
    let erased: *mut (dyn RuleActions + 'static) = unsafe { std::mem::transmute(raw) };
    let ptr = NonNull::new(erased).expect("RuleActions reference is non-null");
    ACTIVE_ACTIONS.with(|slot| {
        assert!(
            slot.get().is_none(),
            "nested enter_actions_scope is not allowed"
        );
        slot.set(Some(ptr));
    });
    let _guard = ScopeGuard;
    f()
}

/// Borrow the currently active `RuleActions` port, returning `None` outside an
/// active tick/post-load scope (e.g., during cold load/init).
pub(crate) fn with_actions<R>(f: impl FnOnce(&mut dyn RuleActions) -> R) -> Option<R> {
    let ptr = ACTIVE_ACTIONS.with(|slot| slot.replace(None))?;
    let mut restore = BorrowRestoreGuard(Some(ptr));
    // SAFETY: `enter_actions_scope` holds exclusive ownership of the underlying
    // `RuleActions` for our lifetime, and replacing the slot with `None` guarantees
    // no second `&mut dyn RuleActions` can be constructed while `f` executes.
    let res = f(unsafe { restore.0.as_mut().unwrap().as_mut() });
    drop(restore);
    Some(res)
}

/// Construct a deterministic VM with standard libraries, host RNG, and
/// non-stdout output handling.
pub fn new_vm<H>(
    limits: ScriptLimits,
    rng: Box<dyn ScriptRng>,
) -> Result<ScriptVm<H>, ScriptError> {
    let mut vm = ScriptVm::with_limits(limits)?;
    vm.set_rng(rng);
    vm.set_output_sink(bfe_script::OutputSink::Discard);
    vm.open_stdlib()?;
    Ok(vm)
}

/// Install an unknown-global call handler on the VM.
pub fn set_unknown_global_handler<H>(
    vm: &mut ScriptVm<H>,
    handler: impl Fn(&mut ScriptCallCtx<'_, H>, &str) -> Result<(), ScriptError> + 'static,
) {
    vm.set_unknown_global_handler(Some(Rc::new(handler)));
}

/// Convert a simulation `f64` to a deterministic finite Lua `f32`.
///
/// Non-finite inputs map to `0.0`. Finite inputs whose magnitude exceeds
/// `f32::MAX` clamp to `f32::MAX`/`f32::MIN` instead of narrowing to infinity.
/// Negative zero normalizes to `+0.0`.
#[inline]
pub fn f64_to_lua(value: f64) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let narrowed = value as f32;
    if !narrowed.is_finite() {
        if value.is_sign_positive() {
            f32::MAX
        } else {
            f32::MIN
        }
    } else if narrowed == 0.0 {
        0.0
    } else {
        narrowed
    }
}

/// Widen a finite Lua `f32` to `f64` (exact); returns `None` for NaN/Inf.
#[inline]
pub fn lua_to_finite_f64(value: f32) -> Option<f64> {
    value.is_finite().then(|| f64::from(value))
}

/// Deterministically truncate a Lua `f32` to `i32` without platform-dependent
/// float-to-int overflow behavior, preserving exact representable `f32`
/// integers up to `2_147_483_520`.
#[inline]
pub fn lua_to_i32(value: f32) -> i32 {
    if !value.is_finite() {
        0
    } else if value >= 2_147_483_648.0 {
        i32::MAX
    } else if value <= -2_147_483_648.0 {
        i32::MIN
    } else {
        value as i32
    }
}

/// Convert a Lua `f32` into an exact positive handle in `1..=MAX_EXACT_LUA_HANDLE`.
#[inline]
pub fn lua_to_exact_handle(value: f32) -> Option<u32> {
    if !value.is_finite()
        || value < 1.0
        || value > (MAX_EXACT_LUA_HANDLE as f32)
        || value.fract() != 0.0
    {
        return None;
    }
    Some(value as u32)
}

/// Compare two raw byte slices using ASCII case folding (zero allocation,
/// never conflating raw Latin-1 bytes with multi-byte UTF-8 sequences).
#[inline]
pub fn ascii_bytes_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Read required integer argument `idx` (0-based).
pub fn check_int<H>(ctx: &mut ScriptCallCtx<'_, H>, idx: usize) -> Result<i32, ScriptError> {
    let n = ctx.check_number(idx)?;
    Ok(lua_to_i32(n))
}

/// Read optional integer argument `idx` (0-based), defaulting if omitted or `nil`.
pub fn opt_int<H>(
    ctx: &mut ScriptCallCtx<'_, H>,
    idx: usize,
    default: i32,
) -> Result<i32, ScriptError> {
    if ctx.arg(idx).is_nil() {
        return Ok(default);
    }
    check_int(ctx, idx)
}

/// Read required string argument `idx` (0-based) as an interned `StringId`.
pub fn check_string_id<H>(
    ctx: &mut ScriptCallCtx<'_, H>,
    idx: usize,
) -> Result<ScriptStringId, ScriptError> {
    ctx.check_string_id(idx)
}

/// Read required string argument `idx` (0-based) as exact raw Lua bytes.
pub fn check_bytes<H>(ctx: &mut ScriptCallCtx<'_, H>, idx: usize) -> Result<Vec<u8>, ScriptError> {
    let sid = ctx.check_string_id(idx)?;
    Ok(ctx.vm().string_bytes(sid).ok_or(ScriptError::InvalidHandle("string"))?.to_vec())
}

/// Read required string argument `idx` (0-based) decoded losslessly as Latin-1.
pub fn check_string<H>(ctx: &mut ScriptCallCtx<'_, H>, idx: usize) -> Result<String, ScriptError> {
    let sid = ctx.check_string_id(idx)?;
    identifier(ctx.vm(), sid)
}

/// Read optional string argument `idx` (0-based); returns `None` if omitted or `nil`.
pub fn opt_string<H>(
    ctx: &mut ScriptCallCtx<'_, H>,
    idx: usize,
) -> Result<Option<String>, ScriptError> {
    if ctx.arg(idx).is_nil() {
        return Ok(None);
    }
    check_string(ctx, idx).map(Some)
}

/// Interpret a Lua argument as a boolean flag: handles `bool`, `number` (`!= 0`),
/// and `"true"` / `"false"` strings (matching 2005 mission script conventions).
pub fn arg_flag<H>(ctx: &ScriptCallCtx<'_, H>, idx: usize, default: bool) -> bool {
    match ctx.arg(idx) {
        ScriptValue::Nil => default,
        ScriptValue::Bool(b) => b,
        ScriptValue::Number(n) => n.is_finite() && n != 0.0,
        ScriptValue::String(sid) => {
            let Some(raw) = ctx.vm().string_bytes(sid) else { return default };
            let trimmed = trim_ascii_bytes(raw);
            if ascii_bytes_eq_ignore_case(trimmed, b"false") || trimmed == b"0" {
                false
            } else {
                !trimmed.is_empty()
            }
        }
        _ => true,
    }
}

/// Trim leading and trailing ASCII whitespace bytes without allocation.
#[inline]
pub fn trim_ascii_bytes(mut bytes: &[u8]) -> &[u8] {
    while let Some((&first, rest)) = bytes.split_first() {
        if first.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    while let Some((&last, rest)) = bytes.split_last() {
        if last.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// Intern and push raw bytes onto the VM stack without UTF-8/Latin-1 alteration.
pub fn push_bytes<H>(ctx: &mut ScriptCallCtx<'_, H>, bytes: &[u8]) -> Result<(), ScriptError> {
    let sid = ctx.vm_mut().intern_bytes(bytes)?;
    ctx.push_return(ScriptValue::String(sid))
}

/// Intern and push a UTF-8 string return value onto the VM stack.
pub fn push_str<H>(ctx: &mut ScriptCallCtx<'_, H>, s: &str) -> Result<(), ScriptError> {
    push_bytes(ctx, s.as_bytes())
}

/// Keep one Unicode scalar per byte in host identifiers. This encoding is reversible.
pub(crate) fn identifier<H>(vm: &ScriptVm<H>, id: ScriptStringId) -> Result<String, ScriptError> {
    Ok(vm.string_bytes(id).ok_or(ScriptError::InvalidHandle("string"))?.iter().copied().map(char::from).collect())
}

pub fn table_strings(ctx: &mut ScriptCallCtx<'_,super::state::HostState>, table: ScriptTableId) -> Result<Vec<String>, ScriptError> {
    let mut rows = Vec::new();
    for i in 1..=33 {
        let value = ctx.vm().table_raw_get_int(table,i)?;
        match value {
            ScriptValue::Nil => return Ok(rows),
            ScriptValue::String(id) if i <= 32 && ctx.vm().string_bytes(id).is_some_and(|s| s.len() <= 256) => {
                let len = ctx.vm().string_bytes(id).unwrap().len() as u32;
                ctx.charge_work(len.saturating_add(1))?;
                ctx.host_mut().charge(1+len.div_ceil(32))?;
                rows.push(identifier(ctx.vm(),id)?);
            }
            _ => return Err(ScriptError::ResourceLimit("mission object name list")),
        }
    }
    Ok(rows)
}

pub fn table_ints(ctx: &mut ScriptCallCtx<'_,super::state::HostState>, table: ScriptTableId) -> Result<Vec<i32>, ScriptError> {
    let mut rows=Vec::new();
    for i in 1..=9 {
        ctx.charge_work(1)?;
        match ctx.vm().table_raw_get_int(table,i)? {
            ScriptValue::Nil => break,
            ScriptValue::Number(n) if i <= 8 => rows.push(lua_to_i32(n)),
            _ => return Err(ScriptError::ResourceLimit("mission team list")),
        }
    }
    Ok(rows)
}
