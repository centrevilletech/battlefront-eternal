//! Deterministic Lua 5.0.3 virtual machine and host interface.
//!
//! Instruction execution, call frames, varargs (`arg`), closures/upvalues, and
//! metatable dispatch are ported from Lua 5.0.3 (`src/lvm.c`, `src/ldo.c`,
//! `src/ltm.c`, MIT License, Copyright (C) 1994-2006 Tecgraf, PUC-Rio), with
//! `MAXSTACK = 128` (`RK(x) >= 128` -> `Kst(x - 128)`), `f32` arithmetic,
//! instruction/stack/memory limits, and deterministic table/RNG/output hooks.

use bfe_wide::Wide;
use crate::units::Units;
use std::io::Write;
use std::rc::Rc;

use crate::chunk::Proto;
use crate::error::{LoadError, ScriptError};
use crate::numfmt;
use crate::opcode::{OpCode, LFIELDS_PER_FLUSH, MAXSTACK};
use crate::value::{
    alloc_vm_id, canonicalize_number, next_generation, ClosureId, CollectorStats, GcHeap, GcPhase,
    LoadedProtoId, NativeFnId, SlotScan, StringId, TableId, TagMethod, UpvalId, UpvalList,
    UpvalState, UserdataId, Value, ValueRef, VALUE_BYTES,
};

const MAX_TAG_LOOP: usize = 100;
const MAX_NATIVE_REENTRY_DEPTH: usize = 64;

/// Bytes of copying, scanning or comparing charged as one work unit.
pub(crate) const BYTES_PER_UNIT: u32 = 16;

/// Most table slots one traversal step looks at before it is charged.
pub(crate) const SCAN_CHUNK: usize = 256;

/// Extra work units charged for each native function call.
const NATIVE_CALL_UNITS: u32 = 3;

/// Capacity of the concatenation buffer kept between operations (a larger buffer is
/// released after use, so what stays allocated is bounded).
const CONCAT_BUF_KEEP: usize = 4096;

/// Collector work units (slots or references examined) that take about as long as one
/// budget unit. A call's collector allowance is its budget in these units, so the
/// collector may spend inside a call at most the time of the call's own allowance.
const GC_UNITS_PER_BUDGET_UNIT: u32 = 2;
/// Smallest share of work a step is given.
const GC_MIN_STEP_WORK: u32 = 256;
/// Largest share of work in an incremental or rescue traversal.
pub(crate) const GC_MAX_STEP_WORK: u32 = 8192;
/// Collector work shared by all operations in one simulation tick.
pub const TICK_COLLECTOR_UNITS: u32 = VmLimits::TICK_BUDGET * 2;

/// Bytes charged per call frame and per call trace entry (the VM reserves both at
/// their configured maximum).
const FRAME_BYTES: usize = 64;
const TRACE_BYTES: usize = 24;
/// Bytes charged per host reference slot.
const REF_SLOT_BYTES: usize = 24;
/// Bytes charged per open upvalue list entry (`(usize, UpvalId)`).
const OPEN_UPVAL_BYTES: usize = 16;

/// Safety limits enforced by a [`Vm`] on every host call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmLimits {
    /// Work units a single top-level host call may use. Every instruction costs one unit
    /// and library functions are charged for the work they do. Collection has its own
    /// allowance (see `docs/scripts.md` for the measured cost of a unit).
    pub instruction_budget: u32,
    /// Maximum combined Lua and native call depth. The default is Lua 5.0.3's
    /// `LUA_MAXCALLS` (4096). Lua-to-Lua calls do not use the native stack.
    pub max_call_depth: usize,
    /// Maximum operand stack size in value slots (16 bytes each, reserved against the
    /// memory limit up front).
    pub max_stack_slots: usize,
    /// Maximum tracked GC heap size in bytes.
    pub memory_limit: usize,
}

impl VmLimits {
    /// Recommended budget for loading a mission: running its chunks and the setup calls
    /// that follow. The game's own mission setups use under 7,000 units; a hostile script
    /// stops within about a second (`docs/scripts.md`).
    pub const LOAD_BUDGET: u32 = 10_000_000;
    /// Recommended budget for the script callbacks of one simulation tick, shared by the
    /// callbacks that tick dispatches. A hostile script stops within about 3 ms, a fifth
    /// of a 60 Hz tick, while an ordinary game callback uses around a hundred units
    /// (`docs/scripts.md`).
    pub const TICK_BUDGET: u32 = 30_000;
}

impl Default for VmLimits {
    fn default() -> Self {
        Self {
            instruction_budget: Self::LOAD_BUDGET,
            max_call_depth: 4096,
            max_stack_slots: 32_768,
            memory_limit: 16 * 1024 * 1024,
        }
    }
}

/// Host-supplied deterministic random number generator for `math.random` / `math.randomseed`.
pub trait ScriptRng {
    /// Seed the generator from a Lua integer seed (`math.randomseed`).
    fn seed(&mut self, seed: i32);
    /// Return a deterministic single-precision float strictly in `[0.0, 1.0)`.
    fn next_unit(&mut self) -> f32;
}

/// The SplitMix increment, which also seeds the generator.
const GOLDEN: Wide = Wide::new(0x9E37_79B9, 0x7F4A_7C15);

/// Deterministic SplitMix64-based default PRNG used when the host does not supply one.
#[derive(Debug, Clone)]
pub struct DefaultRng {
    state: Wide,
}

impl Default for DefaultRng {
    fn default() -> Self {
        Self::from_seed(1)
    }
}

impl DefaultRng {
    pub fn from_seed(seed: i32) -> Self {
        Self {
            state: Wide::from_i32(seed).xor(GOLDEN),
        }
    }
}

impl ScriptRng for DefaultRng {
    fn seed(&mut self, seed: i32) {
        self.state = Wide::from_i32(seed).xor(GOLDEN);
    }

    fn next_unit(&mut self) -> f32 {
        self.state = self.state.wrapping_add(GOLDEN);
        let mut z = self.state;
        z = z.xor(z.shr(30)).wrapping_mul(Wide::new(0xBF58_476D, 0x1CE4_E5B9));
        z = z.xor(z.shr(27)).wrapping_mul(Wide::new(0x94D0_49BB, 0x1331_11EB));
        z = z.xor(z.shr(31));
        // Use top 24 bits so the f32 value is exact and strictly < 1.0.
        (z.shr(40).low() as f32) * (1.0 / 16_777_216.0)
    }
}

/// Callback type for custom [`OutputSink`] destinations.
pub type OutputCallback = Box<dyn FnMut(&[u8])>;

/// Output destination for Lua's `print` function.
pub enum OutputSink {
    /// Write to process stdout.
    Stdout,
    /// Silently discard `print` output.
    Discard,
    /// Append UTF-8 / raw bytes into an internal buffer readable via [`Vm::output_bytes`].
    Buffer(Vec<u8>),
    /// Forward raw chunks to a host callback.
    Custom(OutputCallback),
}

impl OutputSink {
    fn write_bytes(&mut self, data: &[u8]) {
        match self {
            OutputSink::Stdout => {
                let _ = std::io::stdout().write_all(data);
            }
            OutputSink::Discard => {}
            OutputSink::Buffer(buf) => buf.extend_from_slice(data),
            OutputSink::Custom(cb) => cb(data),
        }
    }
}

/// Function pointer type for zero-allocation registered native functions.
pub type NativeFn<H> = fn(&mut CallCtx<'_, H>) -> Result<(), ScriptError>;

/// Reference-counted closure type for native functions that capture Rust state.
pub type NativeClosure<H> = Rc<dyn Fn(&mut CallCtx<'_, H>) -> Result<(), ScriptError>>;

/// Optional fallback invoked when a script calls an undefined global function.
///
/// Used by the recording host (`bfe-script-tool`) without altering normal Lua
/// global lookups (`if __OBJECTIVE_LUA__` or `if myScriptInit` still see `nil`).
pub type UnknownGlobalCallHandler<H> =
    Rc<dyn Fn(&mut CallCtx<'_, H>, &str) -> Result<(), ScriptError>>;

pub(crate) enum NativeKind<H> {
    Fn(NativeFn<H>),
    Closure(NativeClosure<H>),
}

impl<H> Clone for NativeKind<H> {
    fn clone(&self) -> Self {
        match self {
            NativeKind::Fn(f) => NativeKind::Fn(*f),
            NativeKind::Closure(c) => NativeKind::Closure(Rc::clone(c)),
        }
    }
}

struct RefSlot {
    gen: u32,
    value: Option<Value>,
}

#[derive(Debug, Clone)]
struct CallFrame {
    closure: ClosureId,
    proto: LoadedProtoId,
    base: usize,
    result_slot: usize,
    expected_results: Option<usize>,
    frame_top: usize,
    pc: usize,
}

/// Position of a failed Lua instruction, including for a chunk stripped of debug data.
/// The function key matches the one observed when its prototype was loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstructionSite {
    pub function: (u32, u32),
    pub instruction: usize,
}

/// Unified call-stack entry (both Lua and native/C frames) for exact Lua 5.0.3
/// `lua_getstack`, `luaL_where`, `getfuncname`, `getfenv`/`setfenv`, and `xpcall`.
#[derive(Debug, Clone, Copy)]
enum TraceFrame {
    Lua {
        frame_idx: usize,
        tailcalls: u32,
    },
    Native {
        func: Value,
    },
}

/// Resolved frame kind returned by `resolve_stack_level` (`lua_getstack`).
pub(crate) enum StackLevel {
    None,
    TailCall,
    Lua(ClosureId),
    Native,
}

/// Execution context passed to every native Rust function called from Lua.
///
/// Arguments are read directly from the VM stack and return values are pushed
/// directly onto the VM stack, avoiding per-call heap allocations in steady state.
pub struct CallCtx<'a, H> {
    vm: &'a mut Vm<H>,
    host: &'a mut H,
    fn_id: Option<NativeFnId>,
    arg_base: usize,
    arg_count: usize,
    ret_count: usize,
    scratch_bytes: usize,
    expected_results: Option<usize>,
}

impl<'a, H> CallCtx<'a, H> {
    /// Immutable access to the VM.
    #[inline]
    pub fn vm(&self) -> &Vm<H> {
        self.vm
    }

    /// Mutable access to the VM.
    #[inline]
    pub fn vm_mut(&mut self) -> &mut Vm<H> {
        self.vm
    }

    /// Immutable access to the host state.
    #[inline]
    pub fn host(&self) -> &H {
        self.host
    }

    /// Mutable access to the host state.
    #[inline]
    pub fn host_mut(&mut self) -> &mut H {
        self.host
    }

    /// Borrow both `&mut Vm<H>` and `&mut H` simultaneously.
    #[inline]
    pub fn split(&mut self) -> (&mut Vm<H>, &mut H) {
        (self.vm, self.host)
    }

    /// Number of arguments passed by the Lua caller.
    #[inline]
    pub fn arg_count(&self) -> usize {
        self.arg_count
    }

    /// Number of return values requested by the caller (`Some(0)` when the
    /// caller discards all results, `None` for open `LUA_MULTRET`).
    #[inline]
    pub(crate) fn expected_results(&self) -> Option<usize> {
        self.expected_results
    }

    /// Return the 0-based argument `idx`, or `Value::Nil` if omitted.
    #[inline]
    pub fn arg(&self, idx: usize) -> Value {
        if idx < self.arg_count {
            self.vm.stack[self.arg_base + idx]
        } else {
            Value::Nil
        }
    }

    /// Slice of all arguments passed to this call.
    #[inline]
    pub fn args(&self) -> &[Value] {
        &self.vm.stack[self.arg_base..self.arg_base + self.arg_count]
    }

    /// Push one return value to return to the Lua caller. A handle that is stale or
    /// belongs to another VM is refused with [`ScriptError::InvalidHandle`].
    #[inline]
    pub fn push_return(&mut self, val: Value) -> Result<(), ScriptError> {
        let val = self.vm.admit_value(val)?;
        if self.vm.stack.len() >= self.vm.limits.max_stack_slots {
            return Err(ScriptError::StackOverflow {
                limit: self.vm.limits.max_call_depth,
            });
        }
        self.vm.push_stack(val);
        self.ret_count += 1;
        Ok(())
    }

    /// Read a captured upvalue of this native closure (`0`-based).
    pub fn upvalue(&self, idx: usize) -> Value {
        self.fn_id
            .and_then(|id| self.vm.heap.get_native(id))
            .and_then(|rec| rec.upvalues.as_slice().get(idx).copied())
            .unwrap_or(Value::Nil)
    }

    /// Update a captured upvalue of this native closure (`0`-based). A handle that is
    /// stale or belongs to another VM is refused with [`ScriptError::InvalidHandle`].
    pub fn set_upvalue(&mut self, idx: usize, val: Value) -> Result<(), ScriptError> {
        let val = self.vm.admit_value(val)?;
        if let Some(id) = self.fn_id {
            self.vm.heap.set_native_upvalue(id, idx, val);
        }
        Ok(())
    }

    /// Deduct `units` from the active instruction/work budget. Native functions that
    /// do work in proportion to their input charge it here (see `docs/scripts.md` for
    /// the cost of a unit).
    #[inline]
    pub fn charge_work(&mut self, units: u32) -> Result<(), ScriptError> {
        self.vm.charge_work(units)
    }

    /// Hold a temporary allocation in this native frame until its caller regains control.
    pub(crate) fn ensure_temp_bytes(&mut self, bytes: usize) -> Result<(), ScriptError> {
        let debit = crate::value::block_bytes(bytes);
        if self.vm.heap.reserve(debit).is_err() {
            self.vm.collect_for_memory();
            self.vm.heap.reserve(debit)?;
        }
        self.scratch_bytes += debit;
        Ok(())
    }

    /// Grow the native frame's output buffer, including both allocations while it moves.
    pub(crate) fn reserve_output(
        &mut self,
        out: &mut Vec<u8>,
        needed: usize,
    ) -> Result<(), ScriptError> {
        if needed <= out.capacity() {
            return Ok(());
        }
        let preferred = needed.max(out.capacity().saturating_mul(2)).max(8);
        let size = if self
            .vm
            .heap
            .check_budget(crate::value::block_bytes(preferred))
            .is_ok()
        {
            preferred
        } else {
            needed
        };
        let old = crate::value::block_bytes(out.capacity());
        self.ensure_temp_bytes(size)?;
        out.try_reserve_exact(size - out.len())
            .map_err(|_| ScriptError::ResourceLimit("string buffer"))?;
        self.scratch_bytes -= old;
        self.vm.heap.unreserve(old);
        Ok(())
    }

    /// Run `f` while temporarily rooting `roots` against GC, restoring the
    /// temporary root stack on both `Ok` and `Err`.
    pub(crate) fn with_temp_roots<T>(
        &mut self,
        roots: &[Value],
        f: impl FnOnce(&mut Self) -> Result<T, ScriptError>,
    ) -> Result<T, ScriptError> {
        let mark = self.vm.temp_roots.len();
        self.vm.extend_roots(roots, &[])?;
        let res = f(self);
        self.vm.temp_roots.truncate(mark);
        res
    }

    /// Raise a Lua library error prefixed with `luaL_where(1)` (`luaL_error`).
    pub(crate) fn lua_error(&mut self, msg: &str) -> ScriptError {
        self.lua_error_bytes(msg.as_bytes())
    }

    /// [`CallCtx::lua_error`] for a message made of raw bytes.
    pub(crate) fn lua_error_bytes(&mut self, msg: &[u8]) -> ScriptError {
        let prefix = self.vm.where_bytes(1);
        self.vm.raise_prefixed(prefix.as_bytes(), msg)
    }

    /// Lua 5.0.3 `luaL_argerror(L, idx + 1, extramsg)` with caller location and resolved callee name.
    pub(crate) fn arg_error(&mut self, idx: usize, extramsg: &str) -> ScriptError {
        let mut narg = (idx + 1) as i32;
        let (namewhat, name) = match self.vm.current_native_func_name() {
            Ok(found) => found,
            Err(e) => return e,
        };
        let mut msg = Vec::with_capacity(48 + name.len() + extramsg.len());
        if namewhat == "method" {
            narg -= 1;
            if narg == 0 {
                msg.extend_from_slice(b"calling `");
                msg.extend_from_slice(&name);
                msg.extend_from_slice(b"' on bad self (");
                msg.extend_from_slice(extramsg.as_bytes());
                msg.push(b')');
                return self.lua_error_bytes(&msg);
            }
        }
        msg.extend_from_slice(format!("bad argument #{narg} to `").as_bytes());
        msg.extend_from_slice(&name);
        msg.extend_from_slice(b"' (");
        msg.extend_from_slice(extramsg.as_bytes());
        msg.push(b')');
        self.lua_error_bytes(&msg)
    }

    /// Lua 5.0.3 `luaL_typerror(L, idx + 1, expected)`.
    pub(crate) fn type_error(&mut self, idx: usize, expected: &str) -> ScriptError {
        let got = if idx >= self.arg_count {
            "no value"
        } else {
            self.arg(idx).type_name()
        };
        self.arg_error(idx, &format!("{expected} expected, got {got}"))
    }

    /// Require that argument `idx` (0-based) was supplied (even if `nil`).
    pub fn check_any(&mut self, idx: usize) -> Result<Value, ScriptError> {
        if idx >= self.arg_count {
            return Err(self.arg_error(idx, "value expected"));
        }
        Ok(self.vm.stack[self.arg_base + idx])
    }

    /// Require argument `idx` to be a number (or string coercible to number).
    pub fn check_number(&mut self, idx: usize) -> Result<f32, ScriptError> {
        let v = self.arg(idx);
        match self.vm.coerce_number(v)? {
            Some(n) => Ok(n),
            None => Err(self.type_error(idx, "number")),
        }
    }

    /// Optional numeric argument `idx` (defaults to `default` if omitted or `nil`).
    pub fn opt_number(&mut self, idx: usize, default: f32) -> Result<f32, ScriptError> {
        let v = self.arg(idx);
        if v.is_nil() {
            return Ok(default);
        }
        self.check_number(idx)
    }

    /// Require argument `idx` to be an integer, converted as Lua 5.0.3's
    /// `luaL_checkint` converts it on the reference build: `(int)` of the number,
    /// which is `-2147483648` for NaN and for values outside the `int` range.
    pub fn check_int(&mut self, idx: usize) -> Result<i32, ScriptError> {
        let n = self.check_number(idx)?;
        Ok(numfmt::c_int(n))
    }

    /// Optional integer argument `idx` (`luaL_optint`).
    pub fn opt_int(&mut self, idx: usize, default: i32) -> Result<i32, ScriptError> {
        let v = self.arg(idx);
        if v.is_nil() {
            return Ok(default);
        }
        self.check_int(idx)
    }

    /// Integer position argument converted as `luaL_checklong`. The string library keeps
    /// positions in its `sint32` type, which is a C `long` (64 bits on the reference build),
    /// so nothing is truncated to 32 bits.
    pub(crate) fn check_pos(&mut self, idx: usize) -> Result<i64, ScriptError> {
        let n = self.check_number(idx)?;
        Ok(numfmt::c_long(n))
    }

    /// Optional position argument (`luaL_optlong`).
    pub(crate) fn opt_pos(&mut self, idx: usize, default: i64) -> Result<i64, ScriptError> {
        let v = self.arg(idx);
        if v.is_nil() {
            return Ok(default);
        }
        self.check_pos(idx)
    }

    /// Require argument `idx` to be a string (or number coerced to string, matching `luaL_checklstring`).
    ///
    /// When a numeric argument is coerced to a string, the stack slot is updated
    /// in place so the interned string stays GC-rooted for the rest of the call.
    pub fn check_string_id(&mut self, idx: usize) -> Result<StringId, ScriptError> {
        let v = self.arg(idx);
        match v {
            Value::String(s) => Ok(s),
            Value::Number(n) => {
                let text = numfmt::lua_number_text(n);
                let sid = self.vm.intern_bytes(text.as_bytes())?;
                if idx < self.arg_count {
                    self.vm.write_stack(self.arg_base + idx, Value::String(sid));
                }
                Ok(sid)
            }
            _ => Err(self.type_error(idx, "string")),
        }
    }

    /// Require argument `idx` to be a table.
    pub fn check_table(&mut self, idx: usize) -> Result<TableId, ScriptError> {
        let v = self.arg(idx);
        match v {
            Value::Table(t) => Ok(t),
            _ => Err(self.type_error(idx, "table")),
        }
    }

    /// Require argument `idx` to be a Lua or native function.
    pub fn check_function(&mut self, idx: usize) -> Result<Value, ScriptError> {
        let v = self.arg(idx);
        if v.is_function() {
            Ok(v)
        } else {
            Err(self.type_error(idx, "function"))
        }
    }

    /// Call a Lua or native function re-entrantly from inside a native callback,
    /// returning its first result (or `Value::Nil`) with zero host heap allocations.
    pub(crate) fn call_one(
        &mut self,
        func: Value,
        args: &[Value],
    ) -> Result<Value, ScriptError> {
        self.vm.admit_call(func, args)?;
        self.vm.call_nested_one(self.host, func, args)
    }

    /// Call a Lua or native function re-entrantly from inside a native callback,
    /// appending its return values into `out`.
    pub fn call_into(
        &mut self,
        func: Value,
        args: &[Value],
        out: &mut Vec<Value>,
    ) -> Result<(), ScriptError> {
        self.vm.admit_call(func, args)?;
        self.vm.call_nested_into(self.host, func, args, out)
    }

    /// Protected call (`pcall`) from inside a native callback.
    ///
    /// Returns `Ok(Ok(()))` with results appended to `out` on success,
    /// `Ok(Err(lua_err_val))` on catchable Lua runtime errors, or
    /// `Err(limit_err)` if a non-catchable host limit was exceeded.
    pub fn pcall_into(
        &mut self,
        func: Value,
        args: &[Value],
        out: &mut Vec<Value>,
    ) -> Result<Result<(), Value>, ScriptError> {
        self.vm.failure_site = None;
        self.vm.admit_call(func, args)?;
        self.vm
            .pcall_with_handler_into(self.host, func, args, None, out)
    }

    /// Zero-allocation protected call directly on the VM stack for `pcall` / `xpcall`:
    /// invokes `self.arg(0)` with `self.args()[arg_start..]` and writes `(true, results...)`
    /// or `(false, err_val)` directly into this call's return slots.
    pub(crate) fn pcall_args_and_return(
        &mut self,
        arg_start: usize,
        err_handler: Option<Value>,
    ) -> Result<(), ScriptError> {
        let func = self.vm.stack[self.arg_base];
        let nargs = self.arg_count.saturating_sub(arg_start);
        let ret_start = self.arg_base + self.arg_count;
        debug_assert_eq!(self.ret_count, 0);
        debug_assert_eq!(self.vm.stack.len(), ret_start);
        let needed = ret_start.saturating_add(2).saturating_add(nargs);
        if needed > self.vm.limits.max_stack_slots {
            return Err(ScriptError::StackOverflow {
                limit: self.vm.limits.max_call_depth,
            });
        }
        // Reserve status boolean slot at `ret_start`, then place `func` + args at `ret_start + 1`.
        self.vm.push_stack(Value::Bool(true));
        let call_slot = ret_start + 1;
        self.vm.push_stack(func.canonicalize());
        for i in 0..nargs {
            let v = self.vm.stack[self.arg_base + arg_start + i];
            self.vm.push_stack(v.canonicalize());
        }
        match self
            .vm
            .pcall_at_stack_slot(self.host, call_slot, nargs, err_handler)?
        {
            Ok(()) => {
                // Status `true` is at `ret_start`; results are already in place at `call_slot..`.
                self.ret_count = self.vm.stack.len() - ret_start;
                Ok(())
            }
            Err(err_val) => {
                self.vm.write_stack(ret_start, Value::Bool(false));
                self.vm.stack.truncate(ret_start + 1);
                self.vm.push_stack(err_val.canonicalize());
                self.ret_count = 2;
                Ok(())
            }
        }
    }
}

/// Deterministic Lua 5.0 virtual machine.
///
/// `H` is the host state passed by `&mut H` into every call; it defaults to `()`
/// for standalone scripts and tests.
pub struct Vm<H = ()> {
    vm_id: u32,
    pub(crate) heap: GcHeap<H>,
    globals: TableId,
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    call_trace: Vec<TraceFrame>,
    /// Open upvalues as `(stack slot, upvalue)`, sorted by slot (Lua's `openupval`).
    open_upvals: Vec<(usize, UpvalId)>,
    refs: Vec<RefSlot>,
    free_refs: Vec<u32>,
    temp_roots: Vec<Value>,
    temp_proto_roots: Vec<LoadedProtoId>,
    limits: VmLimits,
    instructions_left: u32,
    /// The budget the current (or last) top-level call started with.
    call_budget: u32,
    /// Collector work units the current top-level call may still use.
    gc_left: u32,
    /// A full collection was requested but no allowance covered it yet.
    gc_full_pending: bool,
    root_cursor: Option<(u8, usize)>,
    in_top_call: bool,
    error_value: Option<Value>,
    failure_site: Option<InstructionSite>,
    err_handler: Option<Value>,
    err_handler_ran: bool,
    err_handler_depth: usize,
    native_reentry_depth: usize,
    rng: Box<dyn ScriptRng>,
    output: OutputSink,
    unknown_global_handler: Option<UnknownGlobalCallHandler<H>>,
    nil_global_tags: Vec<Option<StringId>>,
    /// Bytes reserved for the stack, frames and call trace at their maximum sizes.
    fixed_bytes: usize,
    /// Bytes of `print` output held by an [`OutputSink::Buffer`] (charged to memory).
    buffered_bytes: usize,
    /// Reused buffer of `..` (at most [`CONCAT_BUF_KEEP`] bytes kept).
    concat_buf: Vec<u8>,
    #[cfg(test)]
    stress_gc: bool,
}

/// Bytes the VM reserves for its stack, frames and call trace under `limits`: the
/// three buffers are allocated at these capacities.
fn fixed_blocks(slots: usize, frames: usize, traces: usize) -> Result<[usize; 3], ScriptError> {
    let block = |n: usize, size: usize| {
        n.checked_mul(size)
            .filter(|&n| n <= isize::MAX as usize)
            .map(crate::value::block_bytes)
            .ok_or(ScriptError::InvalidConfig("VM buffer size overflow"))
    };
    Ok([
        block(slots, VALUE_BYTES)?,
        block(frames, FRAME_BYTES)?,
        block(traces, TRACE_BYTES)?,
    ])
}

fn total_blocks(blocks: [usize; 3]) -> Result<usize, ScriptError> {
    blocks
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .ok_or(ScriptError::InvalidConfig("VM buffer size overflow"))
}

/// Replace the allocation only after its replacement is available. On failure the
/// old buffer and all of its contents remain usable.
fn resize_buffer<T>(buf: &mut Vec<T>, capacity: usize) -> Result<(), ScriptError> {
    let mut replacement = Vec::new();
    replacement
        .try_reserve_exact(capacity)
        .map_err(|_| ScriptError::ResourceLimit("VM buffer reservation"))?;
    replacement.append(buf);
    *buf = replacement;
    Ok(())
}

/// Grow `vec` (elements of `elem_bytes`) so one more element fits, charging the new
/// buffer before it is allocated, beside the old one, and the difference after.
fn grow_charged<T>(
    heap: &mut GcHeap<impl Sized>,
    vec: &mut Vec<T>,
    elem_bytes: usize,
) -> Result<(), ScriptError> {
    use crate::value::block_bytes;
    let cap = vec.capacity();
    if vec.len() < cap {
        return Ok(());
    }
    let new_cap = cap.saturating_mul(2).max(4);
    let new_block = block_bytes(new_cap.saturating_mul(elem_bytes));
    heap.check_budget(new_block)?;
    vec.reserve_exact(new_cap - vec.len());
    heap.add_bytes(new_block - block_bytes(cap * elem_bytes));
    Ok(())
}

impl Vm<()> {
    /// Create a new VM with default limits and no custom host state (`H = ()`).
    pub fn new() -> Self {
        Self::with_limits(VmLimits::default()).expect("default VmLimits are valid")
    }
}

impl Default for Vm<()> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H> Vm<H> {
    /// Create a new VM with explicit safety limits and empty globals table.
    /// Call [`Vm::open_stdlib`] to populate the standard Lua 5.0 libraries.
    ///
    /// Returns `Err(ScriptError::InvalidConfig(_))` if `limits` cannot support
    /// the base VM state (for example, zero call depth, stack smaller than
    /// `MAXSTACK`, or a memory limit below the initial VM footprint).
    pub fn with_limits(limits: VmLimits) -> Result<Self, ScriptError> {
        if limits.max_call_depth == 0 {
            return Err(ScriptError::InvalidConfig("max_call_depth must be non-zero"));
        }
        if limits.max_stack_slots < MAXSTACK {
            return Err(ScriptError::InvalidConfig(
                "max_stack_slots must be at least MAXSTACK (128)",
            ));
        }
        let vm_id = alloc_vm_id();
        let mut heap = GcHeap::try_new(vm_id, limits.memory_limit)?;
        let too_small =
            |_| ScriptError::InvalidConfig("memory_limit too small for initial VM state");
        let fixed_bytes = total_blocks(fixed_blocks(
            limits.max_stack_slots,
            limits.max_call_depth,
            limits.max_call_depth,
        )?)?;
        heap.reserve(fixed_bytes).map_err(too_small)?;
        let globals = heap.alloc_table().map_err(too_small)?;
        Ok(Self {
            vm_id,
            heap,
            globals,
            stack: Vec::with_capacity(limits.max_stack_slots),
            frames: Vec::with_capacity(limits.max_call_depth),
            call_trace: Vec::with_capacity(limits.max_call_depth),
            open_upvals: Vec::new(),
            refs: Vec::new(),
            free_refs: Vec::new(),
            temp_roots: Vec::new(),
            temp_proto_roots: Vec::new(),
            limits,
            instructions_left: limits.instruction_budget,
            call_budget: limits.instruction_budget,
            gc_left: 0,
            gc_full_pending: false,
            root_cursor: None,
            in_top_call: false,
            error_value: None,
            failure_site: None,
            err_handler: None,
            err_handler_ran: false,
            err_handler_depth: 0,
            native_reentry_depth: 0,
            rng: Box::new(DefaultRng::default()),
            output: OutputSink::Stdout,
            unknown_global_handler: None,
            nil_global_tags: Vec::new(),
            fixed_bytes,
            buffered_bytes: 0,
            concat_buf: Vec::new(),
            #[cfg(test)]
            stress_gc: false,
        })
    }

    /// Create a new VM with default limits and standard Lua 5.0 libraries loaded.
    pub fn new_with_stdlib() -> Self {
        let mut vm = Self::with_limits(VmLimits::default()).expect("default VmLimits are valid");
        vm.open_stdlib()
            .expect("stdlib registration fits in default memory limit");
        vm
    }

    /// Open the standard Lua 5.0 libraries (`base`, `table`, `string`, `math`).
    pub fn open_stdlib(&mut self) -> Result<(), ScriptError> {
        crate::stdlib::open_all(self)
    }

    /// Budget units the current (or last) top-level call has used, measured from the
    /// budget that call started with (a later [`Vm::set_instruction_budget`] does not
    /// change it).
    pub fn work_used(&self) -> u32 {
        self.call_budget.saturating_sub(self.instructions_left)
    }

    /// Deduct `units` from the active call's instruction/work budget (nothing between
    /// calls: budgets apply to calls).
    #[inline]
    pub(crate) fn charge_work(&mut self, units: u32) -> Result<(), ScriptError> {
        if !self.in_top_call {
            return Ok(());
        }
        if self.instructions_left < units {
            self.instructions_left = 0;
            return Err(ScriptError::InstructionLimit {
                limit: self.call_budget,
            });
        }
        self.instructions_left -= units;
        Ok(())
    }

    /// Mutable access to `(instructions_left, instruction_budget)` for tight native loops.
    #[inline]
    pub(crate) fn work_budget_mut(&mut self) -> (&mut u32, u32) {
        (&mut self.instructions_left, self.call_budget)
    }

    /// Check that a native function may build a temporary buffer of `temp_bytes`
    /// whose content becomes a string: the buffer (up to three times its length while
    /// it grows by doubling) and the string must fit beside the heap. Runs a full
    /// collection once if they do not.
    pub(crate) fn ensure_temp_bytes(&mut self, temp_bytes: usize) -> Result<(), ScriptError> {
        let need = |heap: &GcHeap<H>| {
            crate::value::block_bytes(temp_bytes)
                .saturating_mul(3)
                .saturating_add(heap.new_string_bytes(temp_bytes))
        };
        if self.heap.check_budget(need(&self.heap)).is_ok() {
            return Ok(());
        }
        self.collect_for_memory();
        self.heap.check_budget(need(&self.heap))
    }

    /// Lua 5.0 `table.getn(t)` with work-budget accounting for native library calls.
    pub(crate) fn table_getn_budgeted(&mut self, table: TableId) -> Result<i32, ScriptError> {
        let tbl = self
            .heap
            .get_table(table)
            .ok_or(ScriptError::InvalidHandle("table"))?;
        tbl.getn_budgeted(
            &self.heap,
            &mut self.instructions_left,
            self.call_budget,
        )
    }

    /// Return the current VM safety limits.
    pub fn limits(&self) -> VmLimits {
        self.limits
    }

    /// Update the VM safety limits.
    ///
    /// Returns `Err(ScriptError::InvalidConfig(_))` without modifying limits if
    /// the requested limits are unusable or below the currently live heap bytes
    /// or active stack size.
    pub fn set_limits(&mut self, limits: VmLimits) -> Result<(), ScriptError> {
        if limits.max_call_depth == 0 {
            return Err(ScriptError::InvalidConfig("max_call_depth must be non-zero"));
        }
        if limits.max_stack_slots < MAXSTACK.max(self.stack.len()) {
            return Err(ScriptError::InvalidConfig(
                "max_stack_slots below active stack or MAXSTACK",
            ));
        }
        if limits.max_call_depth < self.frames.len().max(self.call_trace.len()) {
            return Err(ScriptError::InvalidConfig(
                "max_call_depth below active call depth",
            ));
        }
        let old = fixed_blocks(
            self.stack.capacity(),
            self.frames.capacity(),
            self.call_trace.capacity(),
        )?;
        let new = fixed_blocks(
            limits.max_stack_slots,
            limits.max_call_depth,
            limits.max_call_depth,
        )?;
        let retained = self.heap.memory_bytes - self.fixed_bytes;
        let invalid = ScriptError::InvalidConfig("memory_limit below VM buffer reservation");
        if total_blocks(new)?
            .checked_add(retained)
            .is_none_or(|n| n > limits.memory_limit)
        {
            return Err(invalid);
        }
        // Finish every shrink before growing. The previous bound remains in effect
        // while releasing an existing larger configuration.
        let mut order = [0, 1, 2];
        order.sort_unstable_by_key(|&i| (new[i] >= old[i], new[i]));
        let mut live = self.heap.memory_bytes;
        for i in order {
            if old[i] == new[i] {
                continue;
            }
            let bound = if new[i] < old[i] {
                limits.memory_limit.max(self.heap.memory_limit)
            } else {
                limits.memory_limit
            };
            if live.checked_add(new[i]).is_none_or(|n| n > bound) {
                return Err(invalid);
            }
            live = live - old[i] + new[i];
        }
        for i in order {
            if old[i] == new[i] {
                continue;
            }
            let result = match i {
                0 => resize_buffer(&mut self.stack, limits.max_stack_slots),
                1 => resize_buffer(&mut self.frames, limits.max_call_depth),
                _ => resize_buffer(&mut self.call_trace, limits.max_call_depth),
            };
            if let Err(err) = result {
                // An allocator failure after another buffer moved still leaves limits
                // backed by real capacity and a ledger describing those allocations.
                self.limits.max_stack_slots =
                    self.limits.max_stack_slots.min(self.stack.capacity());
                self.limits.max_call_depth = self
                    .limits
                    .max_call_depth
                    .min(self.frames.capacity())
                    .min(self.call_trace.capacity());
                self.heap.memory_limit = self.heap.memory_limit.max(self.heap.memory_bytes);
                self.limits.memory_limit = self.heap.memory_limit;
                return Err(err);
            }
            self.fixed_bytes = self.fixed_bytes - old[i] + new[i];
            self.heap.unreserve(old[i]);
            self.heap.add_bytes(new[i]);
        }
        self.limits = limits;
        self.heap.memory_limit = limits.memory_limit;
        Ok(())
    }

    /// Set the work budget of the following top-level calls (for example
    /// [`VmLimits::TICK_BUDGET`] for callbacks during play and [`VmLimits::LOAD_BUDGET`]
    /// while a mission loads). Takes effect when the next top-level call starts.
    pub fn set_instruction_budget(&mut self, units: u32) {
        self.limits.instruction_budget = units;
    }

    /// Reserve room for `count` more host userdata, so that creating them later (for
    /// example in repeated object callbacks) does not grow the heap's arena. The room is
    /// charged at its allocated capacity. Unreachable values may be reclaimed to make room.
    pub fn reserve_userdata(&mut self, count: usize) -> Result<(), ScriptError> {
        self.with_memory(|heap| heap.reserve_userdata(count))
    }

    /// Digest of the VM's deterministic state, for comparing two machines or two runs:
    /// every heap object and arena slot (including the debug names and ranges that error
    /// messages use), the collector's position, accounting, globals, host references,
    /// the operand stack, the limits, and the call state (frames, call trace, open
    /// upvalues, temporary roots, remaining and starting budget, error handling).
    /// Sample after an outer call has returned and before dispatching the next one.
    /// The embedding supplies the same native functions, RNG, host state and inputs.
    /// A native callback can inspect the digest, but its own stack is outside it.
    /// Sampling allocates no memory.
    pub fn state_digest(&self) -> Wide {
        let mut h = crate::value::Digest::new();
        self.heap.digest(&mut h);
        h.value(Value::Table(self.globals));
        h.count(self.frames.len());
        for f in &self.frames {
            h.value(Value::Closure(f.closure));
            h.small(f.proto.0);
            h.count(f.base);
            h.count(f.result_slot);
            h.option_count(f.expected_results);
            h.count(f.frame_top);
            h.count(f.pc);
        }
        h.count(self.call_trace.len());
        for t in &self.call_trace {
            match *t {
                TraceFrame::Lua {
                    frame_idx,
                    tailcalls,
                } => {
                    h.bytes(&[1]);
                    h.count(frame_idx);
                    h.small(tailcalls);
                }
                TraceFrame::Native { func } => {
                    h.bytes(&[2]);
                    h.value(func);
                }
            }
        }
        h.count(self.open_upvals.len());
        for &(slot, uv) in &self.open_upvals {
            h.count(slot);
            h.small(uv.0);
        }
        h.count(self.temp_roots.len());
        for &v in &self.temp_roots {
            h.value(v);
        }
        h.count(self.temp_proto_roots.len());
        for p in &self.temp_proto_roots {
            h.small(p.0);
        }
        h.small(self.instructions_left);
        h.small(self.call_budget);
        h.bytes(&[u8::from(self.gc_full_pending)]);
        if let Some((part, pos)) = self.root_cursor {
            h.bytes(&[part]);
            h.count(pos);
        } else {
            h.bytes(&[u8::MAX]);
        }
        if self.in_top_call {
            h.small(self.gc_left);
        }
        if let Some(tick) = &self.heap.tick {
            h.bytes(&[1]);
            tick.digest(&mut h);
        } else {
            h.bytes(&[0]);
        }
        if let Some(site) = self.failure_site {
            h.bytes(&[1]);
            h.pair(site.function.0, site.function.1);
            h.count(site.instruction);
        } else {
            h.bytes(&[0]);
        }
        h.bytes(&[
            u8::from(self.in_top_call),
            u8::from(self.err_handler_ran),
            u8::from(self.error_value.is_some()),
            u8::from(self.err_handler.is_some()),
        ]);
        h.value(self.error_value.unwrap_or(Value::Nil));
        h.value(self.err_handler.unwrap_or(Value::Nil));
        h.count(self.err_handler_depth);
        h.count(self.native_reentry_depth);
        h.count(self.nil_global_tags.len());
        for tag in &self.nil_global_tags {
            h.value(tag.map_or(Value::Nil, Value::String));
        }
        h.count(self.refs.len());
        for r in &self.refs {
            h.small(r.gen);
            h.value(r.value.unwrap_or(Value::Nil));
            h.bytes(&[u8::from(r.value.is_some())]);
        }
        h.words(&self.free_refs);
        h.count(self.stack.len());
        for &v in &self.stack {
            h.value(v);
        }
        h.small(self.limits.instruction_budget);
        h.count(self.limits.max_call_depth);
        h.count(self.limits.max_stack_slots);
        h.count(self.limits.memory_limit);
        for capacity in [
            self.refs.capacity(),
            self.free_refs.capacity(),
            self.open_upvals.capacity(),
            self.temp_roots.capacity(),
            self.temp_proto_roots.capacity(),
            self.nil_global_tags.capacity(),
            self.concat_buf.capacity(),
        ] {
            h.count(capacity);
        }
        h.count(self.fixed_bytes);
        h.count(self.buffered_bytes);
        h.finish()
    }

    /// Replace the deterministic RNG used by `math.random` / `math.randomseed`.
    pub fn set_rng(&mut self, rng: Box<dyn ScriptRng>) {
        self.rng = rng;
    }

    /// Access the RNG mutably (used by `math.random` / `math.randomseed`).
    pub(crate) fn rng_mut(&mut self) -> &mut dyn ScriptRng {
        self.rng.as_mut()
    }

    /// Configure where Lua's `print` sends output.
    pub fn set_output_sink(&mut self, sink: OutputSink) {
        self.heap.unreserve(self.buffered_bytes);
        self.buffered_bytes = 0;
        self.output = sink;
    }

    /// Convenience helper to capture `print` output in an internal buffer. The buffered
    /// bytes count against the memory limit until [`Vm::take_output`] takes them.
    pub fn capture_output(&mut self) {
        self.set_output_sink(OutputSink::Buffer(Vec::new()));
    }

    /// Read captured `print` output if the sink is [`OutputSink::Buffer`].
    pub fn output_bytes(&self) -> &[u8] {
        match &self.output {
            OutputSink::Buffer(buf) => buf.as_slice(),
            _ => &[],
        }
    }

    /// Take and clear captured `print` output if the sink is [`OutputSink::Buffer`].
    pub fn take_output(&mut self) -> Vec<u8> {
        self.heap.unreserve(self.buffered_bytes);
        self.buffered_bytes = 0;
        match &mut self.output {
            OutputSink::Buffer(buf) => std::mem::take(buf),
            _ => Vec::new(),
        }
    }

    /// Write `print` output, charging the bytes to the work budget; a buffer sink also
    /// charges them to the memory limit (with the buffer's growth by doubling).
    pub(crate) fn write_output(&mut self, data: &[u8]) -> Result<(), ScriptError> {
        self.charge_work(1 + data.len().units() / BYTES_PER_UNIT)?;
        if let OutputSink::Buffer(buf) = &self.output {
            let needed = buf.len().saturating_add(data.len());
            let cap = buf.capacity();
            let old = self.buffered_bytes;
            if needed > cap {
                let preferred = cap.saturating_mul(2).max(needed);
                let target = if self
                    .heap
                    .check_budget(crate::value::block_bytes(preferred))
                    .is_ok()
                {
                    preferred
                } else {
                    needed
                };
                let block = crate::value::block_bytes(target);
                if self.heap.check_budget(block).is_err() {
                    self.collect_for_memory();
                }
                self.heap.check_budget(block)?;
                if let OutputSink::Buffer(buf) = &mut self.output {
                    buf.try_reserve_exact(target - buf.len())
                        .map_err(|_| ScriptError::ResourceLimit("captured output"))?;
                }
                self.heap.reserve(block - old)?;
                self.buffered_bytes = block;
            } else if old == 0 && cap != 0 {
                let block = crate::value::block_bytes(cap);
                if self.heap.reserve(block).is_err() {
                    self.collect_for_memory();
                    self.heap.reserve(block)?;
                }
                self.buffered_bytes = block;
            }
        }
        self.output.write_bytes(data);
        Ok(())
    }

    /// `luaV_gettable`: `t[k]` with `__index` metamethods, as a script indexes a value.
    pub(crate) fn index_value(&mut self, host: &mut H, t: Value, k: Value) -> Result<Value, ScriptError> {
        if let Value::Table(tid) = t {
            let raw = self.heap.table(tid).raw_get(k);
            if !raw.is_nil() {
                return Ok(raw);
            }
        }
        self.get_table_val(host, t, k, None)
    }

    /// Install or clear a handler for calls to undefined global functions.
    pub fn set_unknown_global_handler(&mut self, handler: Option<UnknownGlobalCallHandler<H>>) {
        self.unknown_global_handler = handler;
        if self.unknown_global_handler.is_none() {
            self.nil_global_tags.clear();
        }
    }

    /// Current tracked GC heap bytes.
    pub fn memory_bytes(&self) -> usize {
        self.heap.memory_bytes
    }

    /// The global environment table `_G`.
    pub fn globals(&self) -> TableId {
        self.globals
    }

    /// Request a collection that clears weak entries. During simulation any
    /// unfinished work carries into later ticks. During preparation a request
    /// between calls completes synchronously.
    pub fn collect_garbage(&mut self) {
        if self.heap.tick.is_some() {
            self.gc_full_pending = true;
            self.paced_collection(TICK_COLLECTOR_UNITS, false, &[], &[]);
        } else if self.full_collection_bound() <= self.gc_allowance() {
            self.collect_with_roots(false, &[], &[]);
        } else {
            if !self.gc_full_pending {
                self.heap.stats.deferred_requests = self.heap.stats.deferred_requests.saturating_add(1);
            }
            self.gc_full_pending = true;
        }
    }

    /// Counters of the collector's work since this VM was created.
    pub fn collector_stats(&self) -> CollectorStats {
        self.heap.stats
    }

    /// Begin a simulation tick: call it once per tick, outside any call and before
    /// dispatching callbacks, including on quiet ticks. It does this tick's paced
    /// collector work, and every VM operation until the next tick shares the rest
    /// of the tick's [`TICK_COLLECTOR_UNITS`]. Once called, the VM stays in
    /// simulation (see `docs/scripts.md`). Repeating a tick does no work.
    /// A lower tick number or a call from a native callback is rejected without
    /// changing the balance.
    pub fn begin_tick(&mut self, number: u32) -> Result<(), ScriptError> {
        if self.in_top_call {
            return Err(ScriptError::InvalidConfig("collector tick inside a call"));
        }
        let occupied = self.heap.memory_bytes;
        let room = self.heap.memory_limit.saturating_sub(occupied);
        let tick = match &mut self.heap.tick {
            Some(tick) if number < tick.number => {
                return Err(ScriptError::InvalidConfig("collector tick moved backwards"));
            }
            Some(tick) if number == tick.number => return Ok(()),
            Some(tick) => tick,
            None => self
                .heap
                .tick
                .insert(crate::value::CollectorTick::new(number, occupied)),
        };
        tick.number = number;
        tick.left = TICK_COLLECTOR_UNITS;
        let rate = std::mem::take(&mut tick.allocated).units();
        // Start at a quarter of the remaining room or 256 KiB of allocation
        // traffic, whichever comes sooner. Keep the balance available for bursts.
        let interval = (room / 4).clamp(1, 256 * 1024);
        let due = tick.since_start >= interval;
        let urgent = tick.pressure || self.gc_full_pending || self.heap.full_cycle();
        if !due && !urgent && self.heap.phase() == GcPhase::Pause && !self.heap.has_deferred_drops()
        {
            return Ok(());
        }
        let grant = if urgent {
            TICK_COLLECTOR_UNITS
        } else {
            // Cover a cycle before another quarter of the headroom is consumed.
            let ticks = (room.units() / 4)
                .checked_div(rate)
                .unwrap_or(u32::MAX)
                .max(1);
            self.full_collection_bound()
                .div_ceil(ticks)
                .max(rate / 4)
                .clamp(256, TICK_COLLECTOR_UNITS)
        };
        self.paced_collection(grant, due, &[], &[]);
        Ok(())
    }

    #[inline]
    fn gc_allowance(&self) -> u32 {
        if self.in_top_call {
            self.gc_left
        } else {
            u32::MAX
        }
    }

    fn spend_gc(&mut self, work: u32) {
        if let Some(tick) = &mut self.heap.tick {
            tick.left = tick
                .left
                .checked_sub(work)
                .expect("collector work admitted");
        }
        if self.in_top_call {
            self.gc_left = self.gc_left.saturating_sub(work);
        }
        self.heap.stats.total_units = self.heap.stats.total_units.saturating_add(work);
    }

    /// Roots a complete scan visits, at one unit each.
    fn root_units(&self) -> u32 {
        (1 + self.stack.len()
            + self.temp_roots.len()
            + self.temp_proto_roots.len()
            + self.frames.len()
            + self.call_trace.len()
            + self.refs.len()
            + self.open_upvals.len()
            + self.nil_global_tags.len()
            + 2) as u32
    }

    /// Each vector keeps its indices when truncated. Appends, moves into an earlier
    /// slot and writes to traced objects shade the incoming value during a root scan.
    fn scan_roots(&mut self, budget: u32) -> u32 {
        let Some((mut part, mut pos)) = self.root_cursor else {
            return 0;
        };
        let mut work = 0;
        while work < budget {
            let heap = &mut self.heap;
            match part {
                0 if pos == 0 => heap.mark_table(self.globals),
                0 if pos == 1 => heap.mark_value(self.error_value.unwrap_or(Value::Nil)),
                0 if pos == 2 => heap.mark_value(self.err_handler.unwrap_or(Value::Nil)),
                1 if pos < self.stack.len() => heap.mark_value(self.stack[pos]),
                2 if pos < self.temp_roots.len() => heap.mark_value(self.temp_roots[pos]),
                3 if pos < self.temp_proto_roots.len() => {
                    heap.mark_proto(self.temp_proto_roots[pos])
                }
                4 if pos < self.frames.len() => heap.mark_closure(self.frames[pos].closure),
                5 if pos < self.call_trace.len() => {
                    if let TraceFrame::Native { func } = self.call_trace[pos] {
                        heap.mark_value(func);
                    }
                }
                6 if pos < self.refs.len() => {
                    heap.mark_value(self.refs[pos].value.unwrap_or(Value::Nil))
                }
                7 if pos < self.open_upvals.len() => heap.mark_upval(self.open_upvals[pos].1),
                8 if pos < self.nil_global_tags.len() => {
                    if let Some(id) = self.nil_global_tags[pos] {
                        heap.mark_string(id);
                    }
                }
                8 => {
                    self.root_cursor = None;
                    self.heap.trace_stack = self.heap.weak_mark();
                    return work;
                }
                _ => {
                    part += 1;
                    pos = 0;
                    continue;
                }
            }
            work += 1;
            pos += 1;
        }
        self.root_cursor = Some((part, pos));
        work
    }

    fn mark_stack_write(&mut self, val: Value) {
        if self.heap.trace_stack {
            self.heap.mark_value(val);
        }
    }

    fn write_stack(&mut self, slot: usize, val: Value) {
        self.mark_stack_write(val);
        self.stack[slot] = val;
    }

    fn push_stack(&mut self, val: Value) {
        self.mark_stack_write(val);
        self.stack.push(val);
    }

    /// One incremental collector step, sized by the allocation debt and limited by the
    /// running call's collector allowance: its pieces stop exactly where the allowance
    /// or the step's share ends, and whatever is left waits for a later step. It never
    /// refuses anything. Runs before an allocation, never inside one, so the heap is
    /// consistent around it.
    fn gc_step(&mut self) {
        let allowance = self.gc_allowance();
        let share = self
            .heap
            .step_work()
            .clamp(GC_MIN_STEP_WORK, GC_MAX_STEP_WORK);
        let budget = share.min(allowance);
        let start = self.heap.cycle_due().then_some(false);
        self.collection_step(budget, start);
        if allowance < share {
            self.heap.stats.deferred_steps = self.heap.stats.deferred_steps.saturating_add(1);
        }
    }

    fn collection_step(&mut self, budget: u32, start: Option<bool>) -> u32 {
        let mut work = self.heap.drain_graveyard(budget);
        if let Some(full) = start.filter(|_| self.heap.cycle_start_units() <= budget - work) {
            work += self.heap.begin_cycle(full);
            self.root_cursor = Some((0, 0));
        }
        if self.heap.phase() == GcPhase::Mark {
            work += self.scan_roots(budget - work);
        } else {
            self.root_cursor = None;
        }
        while work < budget {
            match self.heap.phase() {
                GcPhase::Mark => {
                    let (w, done) = self.heap.propagate(budget - work);
                    work += w;
                    if !done {
                        break;
                    }
                    if self.heap.tick.is_some() && self.heap.full_cycle() {
                        work += self.heap.finish_paced_mark(budget - work);
                        if self.heap.phase() == GcPhase::Mark {
                            break;
                        }
                        continue;
                    }
                    if work + self.heap.finish_mark_units() > budget {
                        break;
                    }
                    work += self.heap.finish_mark_slice(budget - work).0;
                }
                GcPhase::Sweep | GcPhase::Recolor => {
                    let full = self.heap.full_cycle();
                    let (w, done) = self.heap.sweep(budget - work);
                    if done && full {
                        if let Some(tick) = &mut self.heap.tick {
                            self.heap.stats.full_collections = self.heap.stats.full_collections.saturating_add(1);
                            if !self.gc_full_pending {
                                tick.pressure = false;
                            }
                        }
                    }
                    work += w;
                    if done || w == 0 {
                        break;
                    }
                }
                GcPhase::Pause => break,
            }
        }
        let stats = &mut self.heap.stats;
        if work > 0 {
            stats.steps = stats.steps.saturating_add(1);
            stats.max_step_units = stats.max_step_units.max(work);
        }
        self.heap.pay_debt(work);
        self.spend_gc(work);
        work
    }

    /// Advance the current cycle before starting another. Extra roots belong to
    /// an allocation whose temporary root buffer could itself require collection.
    fn paced_collection(
        &mut self,
        grant: u32,
        mut start_due: bool,
        values: &[Value],
        protos: &[LoadedProtoId],
    ) {
        let mut left = grant.min(self.heap.tick.as_ref().expect("simulation tick").left);
        let roots = values.len().saturating_add(protos.len()).units();
        let mut shade = true;
        while left > 0 {
            let phase = self.heap.phase();
            if phase == GcPhase::Recolor {
                self.gc_full_pending = true;
            }
            if phase == GcPhase::Pause && (start_due || self.gc_full_pending) {
                if left < self.heap.cycle_start_units().saturating_add(roots) {
                    break;
                }
                // Start separately so all incoming handles are shaded before sweep.
                let full = self.gc_full_pending;
                let w = self.heap.begin_cycle(full);
                self.root_cursor = Some((0, 0));
                self.spend_gc_piece(w);
                left -= w;
                shade = true;
                start_due = false;
                if full {
                    self.gc_full_pending = false;
                    let tick = self.heap.tick.as_mut().expect("simulation tick");
                    if tick.pressure {
                        self.heap.stats.forced_collections = self.heap.stats.forced_collections.saturating_add(1);
                    }
                }
            } else if phase == GcPhase::Pause && !self.heap.has_deferred_drops() {
                break;
            }
            if shade && self.heap.phase() == GcPhase::Mark {
                if roots > left {
                    break;
                }
                for chunk in values.chunks(GC_MAX_STEP_WORK as usize) {
                    for &v in chunk {
                        self.heap.mark_value(v);
                    }
                    self.spend_gc_piece(chunk.len().units());
                }
                for chunk in protos.chunks(GC_MAX_STEP_WORK as usize) {
                    for &p in chunk {
                        self.heap.mark_proto(p);
                    }
                    self.spend_gc_piece(chunk.len().units());
                }
                left -= roots;
                shade = false;
            }
            let before = self.heap.phase();
            let w = self.collection_step(left.min(GC_MAX_STEP_WORK), None);
            left -= w;
            if w == 0 && before == self.heap.phase() {
                break;
            }
        }
    }

    /// Run collector work owed by recent allocation; called before allocating.
    #[inline]
    fn gc_check(&mut self) {
        #[cfg(test)]
        if self.stress_gc {
            self.collect_for_memory();
            return;
        }
        if self.heap.tick.is_some() {
            return;
        }
        if self.gc_full_pending && self.full_collection_bound() <= self.gc_allowance() {
            self.collect_with_roots(false, &[], &[]);
        } else if self.heap.step_due() {
            self.gc_step();
        }
    }

    /// An upper bound of a full collection's work units, roots included.
    fn full_collection_bound(&self) -> u32 {
        self.heap.full_collection_bound() + self.root_units()
    }

    /// During simulation, rescue shares the tick's balance with routine work.
    /// During preparation, finish sweeping and trace a fresh snapshot. Supplied
    /// roots stay live even if the reservation for storing them has failed.
    fn collect_with_roots(
        &mut self,
        forced: bool,
        values: &[Value],
        protos: &[LoadedProtoId],
    ) {
        if let Some(tick) = &mut self.heap.tick {
            tick.pressure |= forced;
            self.gc_full_pending = true;
            self.paced_collection(TICK_COLLECTOR_UNITS, false, values, protos);
            return;
        }
        let extra = (values.len() + protos.len()).units();
        let bound = self.full_collection_bound().saturating_add(extra);
        let start = self.heap.stats.total_units;
        while matches!(self.heap.phase(), GcPhase::Sweep | GcPhase::Recolor) {
            let (w, _) = self.heap.sweep(GC_MAX_STEP_WORK);
            self.spend_gc_piece(w);
        }
        let w = self.heap.begin_cycle(true);
        self.spend_gc_piece(w);
        self.root_cursor = Some((0, 0));
        while self.root_cursor.is_some() {
            let w = self.scan_roots(GC_MAX_STEP_WORK);
            self.spend_gc_piece(w);
        }
        let mut added = 0;
        while added < extra as usize {
            let share = GC_MAX_STEP_WORK as usize;
            let stop = (added + share).min(extra as usize);
            for i in added..stop {
                if i < values.len() {
                    self.heap.mark_value(values[i]);
                } else {
                    self.heap.mark_proto(protos[i - values.len()]);
                }
            }
            self.spend_gc_piece((stop - added).units());
            added = stop;
        }
        loop {
            let (w, done) = self.heap.propagate(GC_MAX_STEP_WORK);
            self.spend_gc_piece(w);
            if done {
                break;
            }
        }
        loop {
            let (w, done) = self.heap.finish_mark_slice(GC_MAX_STEP_WORK);
            self.spend_gc_piece(w);
            if done {
                break;
            }
        }
        while self.heap.phase() == GcPhase::Sweep {
            let (w, _) = self.heap.sweep(GC_MAX_STEP_WORK);
            self.spend_gc_piece(w);
        }
        while self.heap.has_deferred_drops() {
            let w = self.heap.drain_graveyard(GC_MAX_STEP_WORK);
            self.spend_gc_piece(w);
        }
        let work = self.heap.stats.total_units.saturating_sub(start);
        debug_assert!(
            work <= bound,
            "full collection did {work} units, bound {bound}"
        );
        self.gc_full_pending = false;
        let stats = &mut self.heap.stats;
        stats.full_collections = stats.full_collections.saturating_add(1);
        if forced {
            stats.forced_collections = stats.forced_collections.saturating_add(1);
        }
    }

    fn spend_gc_piece(&mut self, work: u32) {
        self.heap.stats.max_step_units = self.heap.stats.max_step_units.max(work);
        self.spend_gc(work);
    }

    /// Retry a refused reservation after spending the available collector balance.
    fn collect_for_memory(&mut self) {
        self.collect_with_roots(true, &[], &[])
    }

    /// Run `f` against the heap after any collector step it owes; if it fails for
    /// lack of memory, run a full collection and try once more.
    fn with_memory<T>(
        &mut self,
        mut f: impl FnMut(&mut GcHeap<H>) -> Result<T, ScriptError>,
    ) -> Result<T, ScriptError> {
        self.gc_check();
        match f(&mut self.heap) {
            Err(ScriptError::MemoryLimit { .. }) => {
                self.collect_for_memory();
                f(&mut self.heap)
            }
            other => other,
        }
    }

    // -----------------------------------------------------------------------
    // Handle validation and scoped temporary roots
    // -----------------------------------------------------------------------

    /// Return `true` if `val` is primitive or references a live object owned by this VM.
    pub fn is_valid_value(&self, val: Value) -> bool {
        self.heap.is_live_value(val)
    }

    /// Validate a value handed in by the host (or a native function) and let the
    /// collector see it: a handle the host held may be one the heap no longer reaches.
    pub(crate) fn admit_value(&mut self, val: Value) -> Result<Value, ScriptError> {
        let val = val.canonicalize();
        self.validate_value(val)?;
        self.heap.admit(val);
        Ok(val)
    }

    /// [`Vm::admit_value`] for a function and its arguments before a nested call.
    fn admit_call(&mut self, func: Value, args: &[Value]) -> Result<(), ScriptError> {
        self.admit_value(func)?;
        for &arg in args {
            self.admit_value(arg)?;
        }
        Ok(())
    }

    fn validate_table(&self, id: TableId) -> Result<(), ScriptError> {
        if self.heap.is_live_value(Value::Table(id)) {
            Ok(())
        } else {
            Err(ScriptError::InvalidHandle("table"))
        }
    }

    fn admit_table(&mut self, id: TableId) -> Result<(), ScriptError> {
        self.validate_table(id)?;
        self.heap.admit(Value::Table(id));
        Ok(())
    }

    fn validate_value(&self, val: Value) -> Result<(), ScriptError> {
        if self.is_valid_value(val) {
            Ok(())
        } else {
            let kind = match val {
                Value::String(_) => "string",
                Value::Table(_) => "table",
                Value::Closure(_) => "closure",
                Value::NativeFn(_) => "native function",
                Value::Userdata(_) => "userdata",
                _ => "value",
            };
            Err(ScriptError::InvalidHandle(kind))
        }
    }

    pub(crate) fn with_temp_roots<T>(
        &mut self,
        roots: &[Value],
        f: impl FnOnce(&mut Self) -> Result<T, ScriptError>,
    ) -> Result<T, ScriptError> {
        let mark = self.temp_roots.len();
        self.extend_roots(roots, &[])?;
        let res = f(self);
        self.temp_roots.truncate(mark);
        res
    }

    pub(crate) fn temp_root_mark(&self) -> usize {
        self.temp_roots.len()
    }

    fn extend_roots(
        &mut self,
        values: &[Value],
        protos: &[LoadedProtoId],
    ) -> Result<(), ScriptError> {
        use crate::value::block_bytes;
        fn projection<T>(vec: &Vec<T>, extra: usize, bytes: usize) -> (usize, usize, usize) {
            let needed = vec.len().saturating_add(extra);
            if needed <= vec.capacity() {
                return (0, 0, 0);
            }
            let cap = needed.max(vec.capacity().saturating_mul(2)).max(4);
            let block = block_bytes(cap.saturating_mul(bytes));
            (cap, block, block - block_bytes(vec.capacity() * bytes))
        }
        let (vc, vb, vd) = projection(&self.temp_roots, values.len(), VALUE_BYTES);
        let (pc, pb, pd) = projection(&self.temp_proto_roots, protos.len(), 4);
        let peak = vb.max(vd.saturating_add(pb));
        if self.heap.check_budget(peak).is_err() {
            self.collect_with_roots(true, values, protos);
            self.heap.check_budget(peak)?;
        }
        if vc > 0 {
            self.temp_roots
                .try_reserve_exact(vc - self.temp_roots.len())
                .map_err(|_| ScriptError::ResourceLimit("temporary roots"))?;
            self.heap.reserve(vd)?;
        }
        if pc > 0 {
            self.temp_proto_roots
                .try_reserve_exact(pc - self.temp_proto_roots.len())
                .map_err(|_| ScriptError::ResourceLimit("prototype roots"))?;
            self.heap.reserve(pd)?;
        }
        for &v in values {
            self.heap.admit(v);
        }
        for &p in protos {
            self.heap.admit_proto(p);
        }
        self.temp_roots.extend_from_slice(values);
        self.temp_proto_roots.extend_from_slice(protos);
        Ok(())
    }

    pub(crate) fn push_temp_root(&mut self, val: Value) -> Result<(), ScriptError> {
        self.extend_roots(&[val], &[])
    }

    pub(crate) fn pop_temp_roots(&mut self, mark: usize) {
        self.temp_roots.truncate(mark);
    }

    // -----------------------------------------------------------------------
    // Host API: values, strings, tables, refs, native functions, chunk loading
    // -----------------------------------------------------------------------

    /// Intern a byte slice as a Lua string in this VM. Inside a host call, the bytes
    /// are charged to the work budget (they are compared while looking the string up).
    pub fn intern_bytes(&mut self, bytes: &[u8]) -> Result<StringId, ScriptError> {
        if self.in_top_call && bytes.len().units() >= BYTES_PER_UNIT {
            self.charge_work(bytes.len().units() / BYTES_PER_UNIT)?;
        }
        let left = self.work_left();
        let (id, inserted) = self
            .with_memory(|heap| heap.intern_with_budget(bytes, left))
            .map_err(|e| self.budget_error(e))?;
        if inserted {
            self.charge_work(2)?;
        }
        Ok(id)
    }

    /// Intern a UTF-8 `&str` as a Lua string in this VM.
    pub fn intern_str(&mut self, s: &str) -> Result<StringId, ScriptError> {
        self.intern_bytes(s.as_bytes())
    }

    /// Read the raw bytes of an interned Lua string, or `None` if the handle is stale or foreign.
    pub fn string_bytes(&self, id: StringId) -> Option<&[u8]> {
        self.heap.get_string_bytes(id)
    }

    pub(crate) fn string_bytes_unchecked(&self, id: StringId) -> &[u8] {
        self.heap.string_bytes(id)
    }

    #[inline]
    pub(crate) fn string_rc_unchecked(&self, id: StringId) -> Rc<[u8]> {
        self.heap.string_rc(id)
    }

    /// Read an interned Lua string as UTF-8 if valid and the handle is live.
    pub fn string_str(&self, id: StringId) -> Option<&str> {
        std::str::from_utf8(self.heap.get_string_bytes(id)?).ok()
    }

    /// Read an interned Lua string decoding arbitrary bytes as Latin-1, or `None` if stale/foreign.
    pub fn string_latin1(&self, id: StringId) -> Option<String> {
        let bytes = self.heap.get_string_bytes(id)?;
        Some(match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        })
    }

    pub(crate) fn string_latin1_unchecked(&self, id: StringId) -> String {
        let bytes = self.heap.string_bytes(id);
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => bytes.iter().map(|&b| b as char).collect(),
        }
    }

    /// Allocate a new empty Lua table.
    pub fn create_table(&mut self) -> Result<TableId, ScriptError> {
        self.with_memory(|heap| heap.alloc_table())
    }

    /// Allocate a host userdata value with a host-defined `tag`, opaque `payload`, and optional metatable.
    pub fn create_userdata(
        &mut self,
        tag: u32,
        payload: Wide,
        metatable: Option<TableId>,
    ) -> Result<UserdataId, ScriptError> {
        if let Some(mt) = metatable {
            self.admit_table(mt)?;
        }
        let mt_root = metatable.map(Value::Table).unwrap_or(Value::Nil);
        self.with_temp_roots(&[mt_root], |vm| {
            vm.with_memory(|heap| heap.alloc_userdata(tag, payload, metatable))
        })
    }

    /// Read `(tag, payload)` of a host userdata value, or `None` if the handle is stale or foreign.
    pub fn userdata_info(&self, id: UserdataId) -> Option<(u32, Wide)> {
        let ud = self.heap.get_userdata(id)?;
        Some((ud.tag, ud.payload))
    }

    /// Raw table lookup (`rawget`).
    pub fn table_raw_get(&self, table: TableId, key: Value) -> Result<Value, ScriptError> {
        self.validate_value(key)?;
        let tbl = self
            .heap
            .get_table(table)
            .ok_or(ScriptError::InvalidHandle("table"))?;
        Ok(tbl.raw_get(key))
    }

    /// Raw table lookup by string key name.
    pub fn table_get_str(&mut self, table: TableId, key: &str) -> Result<Value, ScriptError> {
        self.validate_table(table)?;
        self.with_temp_roots(&[Value::Table(table)], |vm| {
            let sid = vm.intern_str(key)?;
            Ok(vm.heap.table(table).raw_get_str(sid))
        })
    }

    /// Raw table lookup by 1-based integer key.
    pub fn table_raw_get_int(&self, table: TableId, index: i32) -> Result<Value, ScriptError> {
        let tbl = self
            .heap
            .get_table(table)
            .ok_or(ScriptError::InvalidHandle("table"))?;
        Ok(tbl.raw_get_int(index))
    }

    /// Raw table assignment (`rawset`).
    pub fn table_raw_set(
        &mut self,
        table: TableId,
        key: Value,
        val: Value,
    ) -> Result<(), ScriptError> {
        self.admit_table(table)?;
        let key = self.admit_value(key)?;
        let val = self.admit_value(val)?;
        self.raw_set_rooted(table, key, val)
    }

    /// Raw assignment by the VM itself: table, key and value are on the stack or
    /// otherwise reachable, so the collection that may run first sees them.
    #[inline]
    fn raw_set_vm(&mut self, table: TableId, key: Value, val: Value) -> Result<(), ScriptError> {
        let left = self.work_left();
        let work = self
            .with_memory(|heap| heap.table_raw_set(table, key, val, left))
            .map_err(|e| self.budget_error(e))?;
        if work > 0 {
            self.charge_work(work)?;
        }
        Ok(())
    }

    /// Work the running call may still do before an operation that checks its own cost
    /// first (unlimited between calls).
    #[inline]
    pub(crate) fn work_left(&self) -> u32 {
        if self.in_top_call {
            self.instructions_left
        } else {
            u32::MAX
        }
    }

    /// Turn a heap operation's refusal for work into this call's budget error.
    pub(crate) fn budget_error(&mut self, e: ScriptError) -> ScriptError {
        match e {
            ScriptError::InstructionLimit { .. } => {
                self.instructions_left = 0;
                ScriptError::InstructionLimit {
                    limit: self.call_budget,
                }
            }
            other => other,
        }
    }

    /// Raw assignment of values the VM already holds, rooted across the collection
    /// that may run first.
    pub(crate) fn raw_set_rooted(
        &mut self,
        table: TableId,
        key: Value,
        val: Value,
    ) -> Result<(), ScriptError> {
        self.with_temp_roots(&[Value::Table(table), key, val], |vm| {
            vm.raw_set_vm(table, key, val)
        })
    }

    /// Raw table assignment with a string key.
    pub fn table_set_str(
        &mut self,
        table: TableId,
        key: &str,
        val: Value,
    ) -> Result<(), ScriptError> {
        self.admit_table(table)?;
        let val = self.admit_value(val)?;
        self.with_temp_roots(&[Value::Table(table), val], |vm| {
            let sid = vm.intern_str(key)?;
            vm.raw_set_rooted(table, Value::String(sid), val)
        })
    }

    /// Deterministic table traversal step (`next(t, prev_key)`).
    pub fn table_next(
        &self,
        table: TableId,
        prev_key: Value,
    ) -> Result<Option<(Value, Value)>, ScriptError> {
        self.validate_value(prev_key)?;
        let tbl = self
            .heap
            .get_table(table)
            .ok_or(ScriptError::InvalidHandle("table"))?;
        tbl.next(prev_key)
    }

    /// `next(t, prev_key)` as a script calls it: the scan over holes and tombstones is
    /// charged to the work budget in pieces of at most [`SCAN_CHUNK`] slots.
    pub(crate) fn table_next_charged(
        &mut self,
        table: TableId,
        prev_key: Value,
    ) -> Result<Option<(Value, Value)>, ScriptError> {
        let mut after = self.heap.table(table).next_after(prev_key)?;
        loop {
            // One unit admits seven slots. Each additional unit admits eight more.
            // Debit only the slots examined below.
            let left = self.work_left();
            if left == 0 {
                self.charge_work(1)?;
            }
            let count = left
                .saturating_mul(8)
                .saturating_sub(1)
                .min(SCAN_CHUNK.units()) as usize;
            let scan = self.heap.table(table).scan_slots(after, count);
            let (found, next_after) = match scan {
                SlotScan::Found(pos, k, v) => (Some((k, v)), Some(pos)),
                SlotScan::Partial(pos) => (None, Some(pos)),
                SlotScan::End => {
                    let total = self.heap.table(table).slot_count();
                    let scanned = total.saturating_sub(after.map_or(0, |a| a + 1));
                    self.charge_work(1 + scanned.units() / 8)?;
                    return Ok(None);
                }
            };
            let scanned = next_after.map_or(0, |p| p + 1) - after.map_or(0, |a| a + 1);
            self.charge_work(1 + scanned.units() / 8)?;
            if found.is_some() {
                return Ok(found);
            }
            after = next_after;
        }
    }

    /// Collect all non-nil `(key, value)` pairs of a table in deterministic iteration order.
    pub fn table_entries(&self, table: TableId) -> Result<Vec<(Value, Value)>, ScriptError> {
        let tbl = self
            .heap
            .get_table(table)
            .ok_or(ScriptError::InvalidHandle("table"))?;
        Ok(tbl.iter_pairs())
    }

    /// Lua 5.0 `table.getn(t)`.
    pub fn table_getn(&self, table: TableId) -> Result<i32, ScriptError> {
        let tbl = self
            .heap
            .get_table(table)
            .ok_or(ScriptError::InvalidHandle("table"))?;
        Ok(tbl.getn(&self.heap))
    }

    /// Lua 5.0 `table.setn(t, n)`.
    pub fn table_setn(&mut self, table: TableId, n: i32) -> Result<(), ScriptError> {
        self.admit_table(table)?;
        self.with_temp_roots(&[Value::Table(table)], |vm| {
            let left = vm.work_left();
            let work = vm
                .with_memory(|heap| heap.table_setn(table, n, left))
                .map_err(|e| vm.budget_error(e))?;
            vm.charge_work(work)
        })
    }

    /// Get a table's metatable.
    pub fn table_metatable(&self, table: TableId) -> Result<Option<TableId>, ScriptError> {
        let tbl = self
            .heap
            .get_table(table)
            .ok_or(ScriptError::InvalidHandle("table"))?;
        Ok(tbl.metatable)
    }

    /// Set a table's metatable.
    pub fn set_table_metatable(
        &mut self,
        table: TableId,
        mt: Option<TableId>,
    ) -> Result<(), ScriptError> {
        self.admit_table(table)?;
        if let Some(mt_id) = mt {
            self.admit_table(mt_id)?;
        }
        self.heap.set_table_metatable(table, mt);
        Ok(())
    }

    /// Read a global variable from `_G` (raw lookup).
    pub fn get_global(&mut self, name: &str) -> Result<Value, ScriptError> {
        let g = self.globals;
        self.table_get_str(g, name)
    }

    /// Set a global variable in `_G` (raw assignment).
    pub fn set_global(&mut self, name: &str, val: Value) -> Result<(), ScriptError> {
        let g = self.globals;
        self.table_set_str(g, name, val)
    }

    fn alloc_native_entry(
        &mut self,
        kind: NativeKind<H>,
        upvalues: &[Value],
    ) -> Result<NativeFnId, ScriptError> {
        for &v in upvalues {
            self.admit_value(v)?;
        }
        let mark = self.temp_roots.len();
        self.extend_roots(upvalues, &[])?;
        let n_up = upvalues.len();
        let res = self
            .ensure_room(|h| h.native_cost(&kind, n_up))
            .and_then(|()| self.heap.alloc_native(kind, upvalues));
        self.temp_roots.truncate(mark);
        res
    }

    /// Record a newly returned handle until the host hands it back to this VM.
    fn host_native(&mut self, id: NativeFnId) -> NativeFnId {
        self.heap.track_host_native(id);
        id
    }

    /// Create a GC-managed native function handle from a plain Rust function pointer.
    ///
    /// Retain the function in a VM root while using the handle across allocations.
    /// For a persistent entry point use [`Vm::create_rooted_native_fn`].
    pub fn create_native_fn(&mut self, func: NativeFn<H>) -> Result<NativeFnId, ScriptError> {
        let id = self.alloc_native_entry(NativeKind::Fn(func), &[])?;
        Ok(self.host_native(id))
    }

    /// Create a native entry point and pin it before returning its handle.
    /// Release the reference when the entry point is retired.
    pub fn create_rooted_native_fn(
        &mut self,
        func: NativeFn<H>,
    ) -> Result<(NativeFnId, ValueRef), ScriptError> {
        let id = self.create_native_fn(func)?;
        let root = self.create_ref(Value::NativeFn(id))?;
        Ok((id, root))
    }

    /// Create a GC-managed native function handle with captured Lua upvalues (not a
    /// root: see [`Vm::create_native_fn`]).
    pub fn create_native_fn_with_upvals(
        &mut self,
        func: NativeFn<H>,
        upvalues: Vec<Value>,
    ) -> Result<NativeFnId, ScriptError> {
        let id = self.alloc_native_entry(NativeKind::Fn(func), &upvalues)?;
        Ok(self.host_native(id))
    }

    /// Internal zero-allocation native closure constructor for `<= 3` upvalues.
    pub(crate) fn create_native_fn_with_upval_slice(
        &mut self,
        func: NativeFn<H>,
        upvalues: &[Value],
    ) -> Result<NativeFnId, ScriptError> {
        self.alloc_native_entry(NativeKind::Fn(func), upvalues)
    }

    /// Create a GC-managed native function handle from a Rust closure (not a root: see
    /// [`Vm::create_native_fn`]).
    pub fn create_native_closure(
        &mut self,
        func: impl Fn(&mut CallCtx<'_, H>) -> Result<(), ScriptError> + 'static,
    ) -> Result<NativeFnId, ScriptError> {
        let payload = crate::value::host_closure_bytes(&func);
        self.ensure_room(|heap| heap.native_closure_cost(payload))?;
        let id = self
            .heap
            .alloc_native(NativeKind::Closure(Rc::new(func)), &[])?;
        Ok(self.host_native(id))
    }

    /// Register a native function pointer directly as a global in `_G`.
    pub fn register_fn(&mut self, name: &str, func: NativeFn<H>) -> Result<NativeFnId, ScriptError> {
        let id = self.create_native_fn(func)?;
        self.with_temp_roots(&[Value::NativeFn(id)], |vm| {
            vm.set_global(name, Value::NativeFn(id))
        })?;
        Ok(id)
    }

    /// Register a Rust closure directly as a global in `_G`.
    pub fn register_closure(
        &mut self,
        name: &str,
        func: impl Fn(&mut CallCtx<'_, H>) -> Result<(), ScriptError> + 'static,
    ) -> Result<NativeFnId, ScriptError> {
        let id = self.create_native_closure(func)?;
        self.with_temp_roots(&[Value::NativeFn(id)], |vm| {
            vm.set_global(name, Value::NativeFn(id))
        })?;
        Ok(id)
    }

    /// Create a global table populated with named native functions (such as `string`, `table`, `math`,
    /// or a host API namespace).
    pub fn register_table(
        &mut self,
        name: &str,
        entries: &[(&str, NativeFn<H>)],
    ) -> Result<TableId, ScriptError> {
        let tbl = self.create_table()?;
        self.with_temp_roots(&[Value::Table(tbl)], |vm| {
            for &(fn_name, func) in entries {
                let fid = vm.create_native_fn(func)?;
                vm.with_temp_roots(&[Value::NativeFn(fid)], |vm| {
                    vm.table_set_str(tbl, fn_name, Value::NativeFn(fid))
                })?;
            }
            vm.set_global(name, Value::Table(tbl))?;
            Ok(tbl)
        })
    }

    /// Pin any Lua value (such as a callback closure) as a GC root and return a [`ValueRef`].
    pub fn create_ref(&mut self, val: Value) -> Result<ValueRef, ScriptError> {
        let val = self.admit_value(val)?;
        self.with_temp_roots(&[val], |vm| vm.pin_rooted_value(val))
    }

    fn pin_rooted_value(&mut self, val: Value) -> Result<ValueRef, ScriptError> {
        if self.free_refs.is_empty() && self.refs.len() == self.refs.capacity() {
            // The reference slots and the free list grow together (the free list never
            // holds more than there are slots), each charged before it grows; short of
            // memory, collect once and try again.
            if self.grow_refs().is_err() {
                self.collect_for_memory();
                self.grow_refs()?;
            }
        }
        if let Some(slot) = self.free_refs.pop() {
            let entry = &mut self.refs[slot as usize];
            entry.value = Some(val);
            Ok(ValueRef {
                slot,
                gen: entry.gen,
                vm_id: self.vm_id,
            })
        } else {
            let slot = self.refs.len() as u32;
            self.refs.push(RefSlot {
                gen: 1,
                value: Some(val),
            });
            Ok(ValueRef {
                slot,
                gen: 1,
                vm_id: self.vm_id,
            })
        }
    }

    /// Grow the reference slots and their free list by one doubling, charged first.
    fn grow_refs(&mut self) -> Result<(), ScriptError> {
        use crate::value::block_bytes;
        let new_cap = self.refs.capacity().saturating_mul(2).max(4);
        let refs_block = block_bytes(new_cap * REF_SLOT_BYTES);
        let free_block = block_bytes(new_cap * 4);
        let old_refs = block_bytes(self.refs.capacity() * REF_SLOT_BYTES);
        self.heap
            .check_budget(refs_block.max(refs_block - old_refs + free_block))?;
        grow_charged(&mut self.heap, &mut self.refs, REF_SLOT_BYTES)?;
        let old_free = block_bytes(self.free_refs.capacity() * 4);
        let cap = self.refs.capacity();
        self.free_refs.reserve_exact(cap - self.free_refs.len());
        self.heap.add_bytes(block_bytes(cap * 4) - old_free);
        Ok(())
    }

    /// Read the value held by a pinned [`ValueRef`], or `None` if it was released or belongs to another VM.
    pub fn get_ref(&self, r: ValueRef) -> Option<Value> {
        if r.vm_id != self.vm_id {
            return None;
        }
        self.refs
            .get(r.slot as usize)
            .filter(|s| s.gen == r.gen)
            .and_then(|s| s.value)
    }

    /// Unpin a [`ValueRef`] so the referenced object can be garbage-collected.
    /// Returns `true` if the reference was active and released, `false` if stale or foreign.
    pub fn release_ref(&mut self, r: ValueRef) -> bool {
        if r.vm_id != self.vm_id {
            return false;
        }
        if let Some(slot) = self.refs.get_mut(r.slot as usize) {
            if slot.gen == r.gen && slot.value.is_some() {
                slot.value = None;
                slot.gen = next_generation(slot.gen);
                self.free_refs.push(r.slot);
                return true;
            }
        }
        false
    }

    /// Decode and load a compiled SWBF2 Lua 5.0 chunk, returning its top-level closure.
    ///
    /// Decoding is limited by the default [`crate::chunk::ChunkLimits`] and by the
    /// memory this VM has left: the decoded prototypes and the verifier's scratch are
    /// charged at their real sizes before they are allocated, and stay charged (as the
    /// loaded prototypes) once loaded. Inside a call, decoding and verification are the
    /// call's own work, limited by its remaining budget.
    pub fn load_chunk_bytes(&mut self, bytes: &[u8]) -> Result<ClosureId, ScriptError> {
        let (proto, decoded) = self.decode_chunk(bytes)?;
        self.instantiate_reserved(proto, decoded)
    }

    fn instantiate_reserved(
        &mut self,
        proto: Rc<Proto>,
        size: usize,
    ) -> Result<ClosureId, ScriptError> {
        self.heap.reserve(size)?;
        let mut reserved = size;
        let slots = proto.tree_depth + 4;
        let res = self
            .with_memory(|h| h.reserve_graves(slots))
            .and_then(|()| self.load_top_level(proto, &mut reserved));
        self.heap.unreserve(reserved);
        res
    }

    /// Load a verified chunk, then visit the function keys in preorder. The visitor
    /// is invoked only after successful admission. Keys are local to this VM.
    pub fn load_chunk_with_functions(
        &mut self,
        bytes: &[u8],
        visit: &mut impl FnMut((u32, u32), u32),
    ) -> Result<ClosureId, ScriptError> {
        let (proto, size) = self.decode_chunk(bytes)?;
        self.charge_work(proto.tree_nodes)?;
        let closure = self.instantiate_reserved(proto, size)?;
        fn walk<H>(
            heap: &GcHeap<H>,
            id: LoadedProtoId,
            ordinal: &mut u32,
            visit: &mut impl FnMut((u32, u32), u32),
        ) {
            visit(heap.proto_key(id), *ordinal);
            *ordinal += 1;
            for &child in &heap.loaded_proto(id).children {
                walk(heap, child, ordinal, visit);
            }
        }
        walk(&self.heap, self.heap.closure(closure).proto, &mut 0, visit);
        Ok(closure)
    }

    /// Decode and verify a chunk within the memory left and the call's budget; on a
    /// shortage of memory, collect once and try again.
    fn decode_chunk(&mut self, bytes: &[u8]) -> Result<(Rc<Proto>, usize), ScriptError> {
        use crate::chunk::{load_chunk_metered, ChunkLimits};
        let defaults = ChunkLimits::default();
        let mut limits = defaults;
        limits.max_input_bytes = limits.max_input_bytes.min(self.limits.memory_limit);
        let room = self.limits.memory_limit - self.heap.memory_bytes;
        limits.max_decoded_bytes = limits.max_decoded_bytes.min(room);
        let left = self.work_left();
        let mut spent = 0;
        let mut reclaim = |held| {
            // The reader has already admitted these bytes within the remaining room.
            self.heap.add_bytes(held);
            self.collect_for_memory();
            self.heap.unreserve(held);
            defaults
                .max_decoded_bytes
                .min(self.limits.memory_limit - self.heap.memory_bytes)
        };
        let res = load_chunk_metered(bytes, limits, left, &mut spent, Some(&mut reclaim));
        self.charge_work(spent)?;
        match res {
            Ok((proto, cost)) => Ok((proto, cost.decoded_bytes)),
            Err(LoadError::WorkLimit) => {
                Err(self.budget_error(ScriptError::InstructionLimit { limit: 0 }))
            }
            Err(LoadError::DecodedSizeLimit { limit }) if limit < defaults.max_decoded_bytes => {
                Err(ScriptError::MemoryLimit {
                    limit: self.limits.memory_limit,
                })
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Instantiate a top-level closure from an already-validated [`Proto`].
    pub fn load_proto(&mut self, proto: Rc<Proto>) -> Result<ClosureId, ScriptError> {
        let size = proto.tree_bytes;
        self.charge_work((size.units()).div_ceil(64))?;
        self.ensure_room(|_| size)?;
        self.instantiate_reserved(proto, size)
    }

    /// [`Vm::load_proto`] drawing each prototype's charge from `reserved` first.
    fn load_top_level(
        &mut self,
        proto: Rc<Proto>,
        reserved: &mut usize,
    ) -> Result<ClosureId, ScriptError> {
        if proto.nups != 0 {
            return Err(ScriptError::Load(
                crate::error::LoadError::InvalidBytecode("top-level chunk cannot have upvalues"),
            ));
        }
        let val_mark = self.temp_roots.len();
        let proto_mark = self.temp_proto_roots.len();
        let res = (|| {
            let loaded_id = self.load_proto_rooted(proto, true, reserved)?;
            let env = self.globals;
            self.alloc_closure_gc(
                loaded_id,
                env,
                UpvalList::Inline {
                    len: 0,
                    ids: [UpvalId(0); UpvalList::INLINE],
                },
            )
        })();
        self.temp_roots.truncate(val_mark);
        self.temp_proto_roots.truncate(proto_mark);
        res
    }

    fn load_proto_rooted(
        &mut self,
        proto: Rc<Proto>,
        charge_source: bool,
        reserved: &mut usize,
    ) -> Result<LoadedProtoId, ScriptError> {
        let mut constants = Vec::with_capacity(proto.constants.len());
        for c in proto.constants.iter() {
            match c {
                crate::chunk::Constant::Nil => constants.push(Value::Nil),
                crate::chunk::Constant::Number(n) => {
                    constants.push(Value::Number(canonicalize_number(*n)));
                }
                crate::chunk::Constant::String(bytes) => {
                    let sid = self.intern_bytes(bytes)?;
                    let v = Value::String(sid);
                    self.push_temp_root(v)?;
                    constants.push(v);
                }
            }
        }
        let mut children = Vec::with_capacity(proto.protos.len());
        for child in proto.protos.iter() {
            // A child inherits its parent's source name by sharing it.
            let shares_source = match (&child.source, &proto.source) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, _) => true,
                _ => false,
            };
            let child_id = self.load_proto_rooted(Rc::clone(child), !shares_source, reserved)?;
            children.push(child_id);
        }
        let const_box = constants.into_boxed_slice();
        let child_box = children.into_boxed_slice();
        // The decoder's reservation already holds this prototype's bytes: hand them
        // over, then make room once so the boxes move into the heap without a retry.
        let mem = crate::value::loaded_proto_mem_bytes(&proto, charge_source);
        let handed = mem.min(*reserved);
        self.heap.unreserve(handed);
        *reserved -= handed;
        self.ensure_room(|h| h.proto_cost(mem))?;
        let id = self
            .heap
            .alloc_loaded_proto(proto, const_box, child_box, charge_source)?;
        self.extend_roots(&[], &[id])?;
        Ok(id)
    }

    /// The upvalue cell an `OP_CLOSURE` descriptor at `desc_pc` of `proto_id` captures:
    /// one of the running closure's cells (`GETUPVAL`) or an open cell over a register
    /// (`MOVE`).
    fn capture_upval(
        &mut self,
        proto_id: LoadedProtoId,
        cur_cid: ClosureId,
        base: usize,
        desc_pc: usize,
    ) -> Result<UpvalId, ScriptError> {
        let desc = *self
            .heap
            .loaded_proto(proto_id)
            .proto
            .code
            .get(desc_pc)
            .ok_or_else(|| ScriptError::runtime("truncated OP_CLOSURE upvalue descriptor"))?;
        match desc.op() {
            Some(OpCode::GetUpval) => self
                .heap
                .closure(cur_cid)
                .upvalues
                .get(desc.b())
                .ok_or_else(|| ScriptError::runtime("closure upvalue capture out of range")),
            Some(OpCode::Move) => self.find_or_create_open_upval(base + desc.b()),
            _ => Err(ScriptError::runtime("invalid OP_CLOSURE upvalue descriptor")),
        }
    }

    /// Make room for `bytes` more of the heap (one full collection if needed) before an
    /// allocation that then cannot fail for memory, so its inputs move in once.
    /// The cost is computed again after a collection: freed slots are reused, so an
    /// arena may no longer need to grow.
    fn ensure_room(&mut self, bytes: impl Fn(&GcHeap<H>) -> usize) -> Result<(), ScriptError> {
        self.gc_check();
        if self.heap.check_budget(bytes(&self.heap)).is_ok() {
            return Ok(());
        }
        self.collect_for_memory();
        self.heap.check_budget(bytes(&self.heap))
    }

    fn alloc_closure_gc(
        &mut self,
        proto: LoadedProtoId,
        env: TableId,
        upvalues: UpvalList,
    ) -> Result<ClosureId, ScriptError> {
        let proto_mark = self.temp_proto_roots.len();
        let val_mark = self.temp_roots.len();
        self.extend_roots(&[Value::Table(env)], &[proto])?;
        let n_up = upvalues.len();
        let res = self
            .ensure_room(|h| h.closure_cost(n_up))
            .and_then(|()| self.heap.alloc_closure(proto, env, upvalues));
        self.temp_roots.truncate(val_mark);
        self.temp_proto_roots.truncate(proto_mark);
        res
    }

    /// Execute a top-level call to a Lua or native function, returning all results.
    pub fn call(
        &mut self,
        host: &mut H,
        func: Value,
        args: &[Value],
    ) -> Result<Vec<Value>, ScriptError> {
        let mut out = Vec::new();
        self.call_into(host, func, args, &mut out)?;
        Ok(out)
    }

    /// The innermost instruction of the last failed host call, before unwinding.
    pub fn failure_site(&self) -> Option<InstructionSite> {
        self.failure_site
    }

    /// Execute a top-level call into a caller-owned reusable `Vec<Value>`: `out` allocates
    /// only when it lacks capacity for the results.
    pub fn call_into(
        &mut self,
        host: &mut H,
        func: Value,
        args: &[Value],
        out: &mut Vec<Value>,
    ) -> Result<(), ScriptError> {
        self.failure_site = None;
        self.admit_call(func, args)?;
        out.clear();
        let is_outer = !self.in_top_call;
        if is_outer {
            self.instructions_left = self.limits.instruction_budget;
            self.call_budget = self.limits.instruction_budget;
            self.gc_left = self
                .limits
                .instruction_budget
                .saturating_mul(GC_UNITS_PER_BUDGET_UNIT);
            self.in_top_call = true;
            self.error_value = None;
        }
        let res = self.call_nested_into(host, func, args, out);
        if is_outer {
            if res.is_ok() {
                self.failure_site = None;
            }
            self.in_top_call = false;
            self.error_value = None;
        }
        res
    }

    /// Convenience helper: load a chunk from bytes and execute its main function.
    pub fn run_chunk(
        &mut self,
        host: &mut H,
        bytes: &[u8],
    ) -> Result<Vec<Value>, ScriptError> {
        let cl = self.load_chunk_bytes(bytes)?;
        self.call(host, Value::Closure(cl), &[])
    }

    /// Call a global function by name if it is defined as a function, returning `Ok(None)` if
    /// the global is `nil`.
    pub fn call_global_if_defined(
        &mut self,
        host: &mut H,
        name: &str,
        args: &[Value],
        out: &mut Vec<Value>,
    ) -> Result<bool, ScriptError> {
        let func = self.get_global(name)?;
        if func.is_nil() {
            out.clear();
            return Ok(false);
        }
        self.call_into(host, func, args, out)?;
        Ok(true)
    }

    // -----------------------------------------------------------------------
    // Internal helpers for stdlib and VM execution
    // -----------------------------------------------------------------------

    /// Lua 5.0 `luaV_tonumber`: returns `Some(f32)` for numbers and numeric strings.
    pub fn to_number(&self, val: Value) -> Option<f32> {
        match val {
            Value::Number(n) => Some(canonicalize_number(n)),
            Value::String(sid) => {
                numfmt::parse_lua_number(self.heap.get_string_bytes(sid)?).map(canonicalize_number)
            }
            _ => None,
        }
    }

    /// [`Vm::to_number`] for coercions a script performs: parsing a string is charged
    /// to the work budget in proportion to its length.
    pub(crate) fn coerce_number(&mut self, val: Value) -> Result<Option<f32>, ScriptError> {
        match val {
            Value::Number(n) => Ok(Some(canonicalize_number(n))),
            Value::String(sid) => {
                let len = self.heap.get_string_bytes(sid).map_or(0, <[u8]>::len);
                self.charge_work(1 + (len.units()) / BYTES_PER_UNIT)?;
                Ok(self.to_number(val))
            }
            _ => Ok(None),
        }
    }

    /// Deterministic default `tostring` representation (without invoking `__tostring`).
    pub fn default_tostring(&self, val: Value) -> Result<String, ScriptError> {
        self.validate_value(val)?;
        Ok(self.default_tostring_unchecked(val))
    }

    pub(crate) fn default_tostring_unchecked(&self, val: Value) -> String {
        match val {
            Value::Nil => "nil".to_string(),
            Value::Bool(true) => "true".to_string(),
            Value::Bool(false) => "false".to_string(),
            Value::Number(n) => numfmt::format_lua_number(n),
            Value::String(sid) => self.string_latin1_unchecked(sid),
            Value::Table(tid) => format!("table: 0x{:08x}", self.heap.table(tid).uid),
            Value::Closure(cid) => format!("function: 0x{:08x}", self.heap.closure(cid).uid),
            Value::NativeFn(fid) => {
                let uid = self.heap.get_native(fid).map_or(Wide::ZERO, |rec| rec.uid);
                format!("function: 0x{uid:08x}")
            }
            Value::Userdata(uid) => format!("userdata: 0x{:08x}", self.heap.userdata(uid).uid),
        }
    }

    /// Set the Lua error value (for `error(val)` / `pcall` fidelity).
    pub(crate) fn set_error_value(&mut self, val: Value) {
        self.mark_stack_write(val);
        self.error_value = Some(val);
    }

    /// Resolve a Lua 5.0.3 call stack level (`lua_getstack` from `ldebug.c:94-114`):
    /// level 0 = currently running function, level 1 = its caller, skipping lost tail calls.
    pub(crate) fn resolve_stack_level(&self, mut level: i32) -> StackLevel {
        if level < 0 {
            return StackLevel::None;
        }
        let mut i = self.call_trace.len();
        while i > 0 {
            i -= 1;
            if level == 0 {
                return match self.call_trace[i] {
                    TraceFrame::Lua { frame_idx, .. } => {
                        StackLevel::Lua(self.frames[frame_idx].closure)
                    }
                    TraceFrame::Native { .. } => StackLevel::Native,
                };
            }
            level -= 1;
            if let TraceFrame::Lua { tailcalls, .. } = self.call_trace[i] {
                if tailcalls > 0 {
                    if level < i32::try_from(tailcalls).unwrap_or(i32::MAX) {
                        return StackLevel::TailCall;
                    }
                    level -= tailcalls as i32;
                }
            }
        }
        StackLevel::None
    }

    /// `"chunk:line: "` for the Lua frame `frame_idx`. With `only_positive` (the
    /// `luaL_where` rule) nothing is written unless the current line is positive;
    /// otherwise (the `luaG_runerror` rule) the line is written as recorded, 0 when the
    /// function carries no line information.
    fn frame_where(&self, frame_idx: usize, only_positive: bool) -> numfmt::NumText {
        let frame = &self.frames[frame_idx];
        let pc = frame.pc.saturating_sub(1);
        let proto = &self.heap.loaded_proto(frame.proto).proto;
        let line = proto.line_at(pc);
        if only_positive && line <= 0 {
            return numfmt::NumText::new();
        }
        let mut out = proto.chunk_id_text();
        use std::fmt::Write;
        let _ = write!(out, ":{line}: ");
        out
    }

    /// Lua 5.0.3 `luaL_where(L, level)` as raw bytes (empty when no line is known).
    pub(crate) fn where_bytes(&self, mut level: i32) -> numfmt::NumText {
        let out = numfmt::NumText::new();
        if level < 0 {
            return out;
        }
        let mut i = self.call_trace.len();
        while i > 0 {
            i -= 1;
            if level == 0 {
                if let TraceFrame::Lua { frame_idx, .. } = self.call_trace[i] {
                    return self.frame_where(frame_idx, true);
                }
                return out;
            }
            level -= 1;
            if let TraceFrame::Lua { tailcalls, .. } = self.call_trace[i] {
                if tailcalls > 0 {
                    if level < i32::try_from(tailcalls).unwrap_or(i32::MAX) {
                        return out;
                    }
                    level -= tailcalls as i32;
                }
            }
        }
        out
    }

    pub(crate) fn raise_prefixed(&mut self, prefix: &[u8], msg: &[u8]) -> ScriptError {
        if prefix.is_empty() {
            return self.raise_bytes(msg);
        }
        let size = prefix.len().saturating_add(msg.len());
        let block = crate::value::block_bytes(size);
        if self.heap.reserve(block).is_err() {
            self.collect_for_memory();
            if let Err(e) = self.heap.reserve(block) {
                return e;
            }
        }
        let mut raw = Vec::with_capacity(size);
        raw.extend_from_slice(prefix);
        raw.extend_from_slice(msg);
        let error = self.raise_bytes(&raw);
        drop(raw);
        self.heap.unreserve(block);
        error
    }

    pub(crate) fn display_error(&mut self, bytes: &[u8]) -> ScriptError {
        let len = bytes.utf8_chunks().fold(0usize, |len, c| {
            len.saturating_add(c.valid().len())
                .saturating_add(3 * usize::from(!c.invalid().is_empty()))
        });
        let block = crate::value::block_bytes(len);
        if self.heap.check_budget(block).is_err() {
            self.collect_for_memory();
            if let Err(e) = self.heap.check_budget(block) {
                return e;
            }
        }
        let mut text = String::with_capacity(len);
        for chunk in bytes.utf8_chunks() {
            text.push_str(chunk.valid());
            if !chunk.invalid().is_empty() {
                text.push('�');
            }
        }
        ScriptError::Runtime(text)
    }

    /// Raise a catchable error whose Lua value is exactly the input byte string.
    pub(crate) fn raise_bytes(&mut self, msg: &[u8]) -> ScriptError {
        match self.intern_bytes(msg) {
            Ok(sid) => {
                self.set_error_value(Value::String(sid));
                self.display_error(msg)
            }
            Err(e) => e,
        }
    }

    /// Lua 5.0.3 `luaG_runerror`: prepend `chunk:line: ` when the running function is a
    /// Lua function (`addinfo`) and raise the result.
    fn lua_runerror(&mut self, msg: &[u8]) -> ScriptError {
        // The message is copied with its position prefix (a chunk id of at most
        // `LUA_IDSIZE` bytes and a line number), then interned and copied for the
        // error: charged and given room before any copy.
        let len = msg.len() + 96;
        if let Err(e) = self.charge_work(1 + 2 * len.units() / BYTES_PER_UNIT) {
            return e;
        }
        let prefix = match self.call_trace.last() {
            Some(&TraceFrame::Lua { frame_idx, .. }) => self.frame_where(frame_idx, false),
            _ => numfmt::NumText::new(),
        };
        self.raise_prefixed(prefix.as_bytes(), msg)
    }

    /// Lua 5.0.3 `getobjname` for register `reg` of the Lua frame `frame_idx` at its
    /// current instruction, charging the scans to the work budget.
    fn obj_name(
        &mut self,
        frame_idx: usize,
        reg: usize,
    ) -> Result<Option<(&'static str, Vec<u8>)>, ScriptError> {
        let frame = &self.frames[frame_idx];
        let pc = frame.pc.saturating_sub(1);
        let proto = Rc::clone(&self.heap.loaded_proto(frame.proto).proto);
        let mut charge = |units: u32| self.charge_work(units);
        proto.get_obj_name(pc, reg, &mut charge)
    }

    /// Lua 5.0.3 `luaG_typeerror` (`ldebug.c:515-525`).
    fn lua_type_error(&mut self, reg: Option<usize>, val: Value, op: &str) -> ScriptError {
        let obj_info = if let (Some(r), Some(&TraceFrame::Lua { frame_idx, .. })) =
            (reg, self.call_trace.last())
        {
            match self.obj_name(frame_idx, r) {
                Ok(info) => info,
                Err(e) => return e,
            }
        } else {
            None
        };
        let t = val.type_name();
        let mut msg = Vec::with_capacity(48);
        msg.extend_from_slice(b"attempt to ");
        msg.extend_from_slice(op.as_bytes());
        if let Some((kind, name)) = obj_info {
            msg.push(b' ');
            msg.extend_from_slice(kind.as_bytes());
            msg.extend_from_slice(b" `");
            msg.extend_from_slice(&name);
            msg.extend_from_slice(b"' (a ");
            msg.extend_from_slice(t.as_bytes());
            msg.extend_from_slice(b" value)");
        } else {
            msg.extend_from_slice(b" a ");
            msg.extend_from_slice(t.as_bytes());
            msg.extend_from_slice(b" value");
        }
        self.lua_runerror(&msg)
    }

    /// Lua 5.0.3 `luaG_ordererror` (`ldebug.c:543-551`).
    fn lua_order_error(&mut self, l: Value, r: Value) -> ScriptError {
        let t1 = l.type_name();
        let t2 = r.type_name();
        if t1 == t2 {
            self.lua_runerror(format!("attempt to compare two {t1} values").as_bytes())
        } else {
            self.lua_runerror(format!("attempt to compare {t1} with {t2}").as_bytes())
        }
    }

    /// Lua 5.0.3 `getfuncname` + `travglobals` (`ldebug.c:218-224, 493-503`) for the
    /// currently executing native function at the top of `self.call_trace`: the kind
    /// (`"global"`, `"local"`, `"method"`, `"field"` or `""`) and the name bytes.
    pub(crate) fn current_native_func_name(
        &mut self,
    ) -> Result<(&'static str, Vec<u8>), ScriptError> {
        let len = self.call_trace.len();
        if len >= 2 {
            if let TraceFrame::Lua { frame_idx, .. } = self.call_trace[len - 2] {
                let frame = &self.frames[frame_idx];
                let pc = frame.pc.saturating_sub(1);
                let proto = &self.heap.loaded_proto(frame.proto).proto;
                if let Some(ins) = proto.code.get(pc).copied() {
                    if matches!(ins.op(), Some(OpCode::Call | OpCode::TailCall)) {
                        if let Some(found) = self.obj_name(frame_idx, ins.a())? {
                            return Ok(found);
                        }
                    }
                }
            }
        }
        // Fallback: search `_G` (`travglobals`), charging every slot looked at.
        if let Some(&TraceFrame::Native { func }) = self.call_trace.last() {
            if !func.is_nil() {
                let globals = self.globals;
                let mut after = None;
                loop {
                    let scan = self.heap.table(globals).scan_slots(after, SCAN_CHUNK);
                    let (found, next_after) = match scan {
                        SlotScan::Found(pos, k, v) => (Some((k, v)), Some(pos)),
                        SlotScan::Partial(pos) => (None, Some(pos)),
                        SlotScan::End => break,
                    };
                    let scanned = next_after.map_or(0, |p| p + 1) - after.map_or(0, |a| a + 1);
                    self.charge_work(1 + scanned.units() / 8)?;
                    if let Some((Value::String(sid), v)) = found {
                        if v.rawequal(func) {
                            let len = self.heap.string_bytes(sid).len();
                            self.charge_work(1 + len.units() / BYTES_PER_UNIT)?;
                            let bytes = numfmt::cstr_prefix(self.heap.string_bytes(sid)).to_vec();
                            return Ok(("global", bytes));
                        }
                    }
                    after = next_after;
                }
            }
        }
        Ok(("", b"?".to_vec()))
    }

    pub(crate) fn closure_env(&self, cid: ClosureId) -> TableId {
        self.heap.closure(cid).env
    }

    pub(crate) fn set_closure_env(&mut self, cid: ClosureId, env: TableId) {
        self.heap.set_closure_env(cid, env);
    }

    pub(crate) fn set_globals(&mut self, env: TableId) {
        self.globals = env;
    }

    pub(crate) fn get_metatable(&self, val: Value) -> Option<TableId> {
        match val {
            Value::Table(tid) => self.heap.get_table(tid).and_then(|t| t.metatable),
            Value::Userdata(uid) => self.heap.get_userdata(uid).and_then(|u| u.metatable),
            _ => None,
        }
    }

    pub(crate) fn get_metafield(&self, val: Value, field: StringId) -> Value {
        self.heap.get_metafield(val, field)
    }

    pub(crate) fn well_known(&self) -> crate::value::WellKnownStrings {
        self.heap.well_known
    }

    // -----------------------------------------------------------------------
    // Nil-global side tracking (active only when unknown_global_handler is set)
    // -----------------------------------------------------------------------

    #[inline]
    fn set_nil_tag(&mut self, slot: usize, tag: Option<StringId>) -> Result<(), ScriptError> {
        if self.unknown_global_handler.is_none()
            || (tag.is_none() && slot >= self.nil_global_tags.len())
        {
            return Ok(());
        }
        if slot >= self.nil_global_tags.capacity() {
            let cap = (slot + 1)
                .max(self.nil_global_tags.capacity().saturating_mul(2))
                .min(self.limits.max_stack_slots);
            let new = crate::value::block_bytes(cap * VALUE_BYTES);
            if self.heap.check_budget(new).is_err() {
                self.collect_with_roots(true, &[tag.map_or(Value::Nil, Value::String)], &[]);
                self.heap.check_budget(new)?;
            }
            let old = crate::value::block_bytes(self.nil_global_tags.capacity() * VALUE_BYTES);
            self.nil_global_tags
                .try_reserve_exact(cap - self.nil_global_tags.len())
                .map_err(|_| ScriptError::ResourceLimit("global name tags"))?;
            self.heap.reserve(new - old)?;
        }
        if slot >= self.nil_global_tags.len() {
            self.nil_global_tags.resize(slot + 1, None);
        }
        if let Some(s) = tag {
            self.mark_stack_write(Value::String(s));
        }
        self.nil_global_tags[slot] = tag;
        Ok(())
    }

    #[inline]
    fn get_nil_tag(&self, slot: usize) -> Option<StringId> {
        self.nil_global_tags.get(slot).copied().flatten()
    }

    #[inline]
    fn clear_nil_tags_from(&mut self, start: usize) {
        if self.nil_global_tags.len() > start {
            self.nil_global_tags.truncate(start);
        }
    }

    // -----------------------------------------------------------------------
    // Open upvalues management
    // -----------------------------------------------------------------------

    fn find_or_create_open_upval(&mut self, slot: usize) -> Result<UpvalId, ScriptError> {
        let pos = match self.open_upvals.binary_search_by_key(&slot, |&(s, _)| s) {
            Ok(i) => return Ok(self.open_upvals[i].1),
            Err(i) => i,
        };
        // Inserting keeps the list sorted by moving the cells above it (at most the
        // frame's registers): charged first, with the cell's later closing.
        self.charge_work(1 + (self.open_upvals.len() - pos).units() / 8)?;
        if grow_charged(&mut self.heap, &mut self.open_upvals, OPEN_UPVAL_BYTES).is_err() {
            self.collect_for_memory();
            grow_charged(&mut self.heap, &mut self.open_upvals, OPEN_UPVAL_BYTES)?;
        }
        let uid = self.with_memory(|heap| heap.alloc_upval(UpvalState::Open(slot)))?;
        // The collection that may have run cannot change the open list.
        let pos = match self.open_upvals.binary_search_by_key(&slot, |&(s, _)| s) {
            Ok(i) | Err(i) => i,
        };
        self.open_upvals.insert(pos, (slot, uid));
        Ok(uid)
    }

    /// [`Vm::close_upvals_from`] on the normal path (`CLOSE`, `RETURN`, a tail call):
    /// the cells closed are charged first. Unwinding after an error closes without a
    /// charge, since cleanup must finish whatever is left of the budget.
    fn close_upvals_charged(&mut self, level: usize) -> Result<(), ScriptError> {
        if self.open_upvals.last().is_some_and(|&(slot, _)| slot >= level) {
            let first = self.open_upvals.partition_point(|&(s, _)| s < level);
            self.charge_work((self.open_upvals.len() - first).units() / 4)?;
            self.close_upvals_from(level);
        }
        Ok(())
    }

    /// Close every open upvalue at or above stack slot `level` (`luaF_close`).
    fn close_upvals_from(&mut self, level: usize) {
        while let Some(&(slot, uid)) = self.open_upvals.last() {
            if slot < level {
                break;
            }
            let val = self.stack.get(slot).copied().unwrap_or(Value::Nil);
            self.heap.close_upval(uid, val);
            self.open_upvals.pop();
        }
    }

    /// Read an upvalue. An open upvalue whose slot lies above the current stack top
    /// (only reachable by bytecode the verifier should have refused) reads `nil`.
    fn read_upval(&self, uid: UpvalId) -> Value {
        match self.heap.upval(uid).state {
            UpvalState::Open(slot) => self.stack.get(slot).copied().unwrap_or(Value::Nil),
            UpvalState::Closed(v) => v,
        }
    }

    /// Write an upvalue. An open upvalue whose slot lies above the current stack top is
    /// closed with the written value instead of growing the stack.
    fn write_upval(&mut self, uid: UpvalId, val: Value) {
        match self.heap.upval(uid).state {
            UpvalState::Open(slot) if slot < self.stack.len() => self.write_stack(slot, val),
            UpvalState::Open(slot) => {
                if let Ok(i) = self.open_upvals.binary_search_by_key(&slot, |&(s, _)| s) {
                    self.open_upvals.remove(i);
                }
                self.heap.close_upval(uid, val);
            }
            UpvalState::Closed(_) => self.heap.write_closed_upval(uid, val),
        }
    }

    // -----------------------------------------------------------------------
    // Nested & protected calls
    // -----------------------------------------------------------------------

    /// Execute a nested call expecting a single return value, with zero host heap allocations.
    pub(crate) fn call_nested_one(
        &mut self,
        host: &mut H,
        func: Value,
        args: &[Value],
    ) -> Result<Value, ScriptError> {
        let saved_stack = self.stack.len();
        let saved_frames = self.frames.len();
        let saved_trace = self.call_trace.len();
        let func_slot = saved_stack;

        let needed = saved_stack.saturating_add(1 + args.len());
        if needed > self.limits.max_stack_slots {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        self.push_stack(func.canonicalize());
        for &a in args {
            self.push_stack(a.canonicalize());
        }

        let res = self.call_at_slot(host, func_slot, args.len(), Some(1), saved_frames);
        match res {
            Ok(()) => {
                let val = self.stack.get(func_slot).copied().unwrap_or(Value::Nil);
                self.stack.truncate(saved_stack);
                self.clear_nil_tags_from(saved_stack);
                Ok(val)
            }
            Err(e) => {
                self.close_upvals_from(saved_stack);
                self.frames.truncate(saved_frames);
                self.call_trace.truncate(saved_trace);
                self.stack.truncate(saved_stack);
                self.clear_nil_tags_from(saved_stack);
                Err(e)
            }
        }
    }

    pub(crate) fn call_nested_into(
        &mut self,
        host: &mut H,
        func: Value,
        args: &[Value],
        out: &mut Vec<Value>,
    ) -> Result<(), ScriptError> {
        let saved_stack = self.stack.len();
        let saved_frames = self.frames.len();
        let saved_trace = self.call_trace.len();
        let func_slot = saved_stack;

        let needed = saved_stack.saturating_add(1 + args.len());
        if needed > self.limits.max_stack_slots {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        self.push_stack(func.canonicalize());
        for &a in args {
            self.push_stack(a.canonicalize());
        }

        let res = self.call_at_slot(host, func_slot, args.len(), None, saved_frames);
        match res {
            Ok(()) => {
                out.extend_from_slice(&self.stack[func_slot..]);
                self.stack.truncate(saved_stack);
                self.clear_nil_tags_from(saved_stack);
                Ok(())
            }
            Err(e) => {
                self.close_upvals_from(saved_stack);
                self.frames.truncate(saved_frames);
                self.call_trace.truncate(saved_trace);
                self.stack.truncate(saved_stack);
                self.clear_nil_tags_from(saved_stack);
                Err(e)
            }
        }
    }

    /// Invoke `self.stack[call_slot]` with `nargs` args directly on the VM stack in
    /// protected mode (zero host heap allocations). On `Ok(Ok(()))`, results remain
    /// in place at `self.stack[call_slot..]`.
    pub(crate) fn pcall_at_stack_slot(
        &mut self,
        host: &mut H,
        call_slot: usize,
        nargs: usize,
        err_handler: Option<Value>,
    ) -> Result<Result<(), Value>, ScriptError> {
        let saved_frames = self.frames.len();
        let saved_trace = self.call_trace.len();
        let prev_err = self.error_value.take();
        let prev_handler = self.err_handler.take();
        let prev_ran = self.err_handler_ran;
        let saved_temp = self.temp_roots.len();
        let saved_proto_temp = self.temp_proto_roots.len();
        self.err_handler = err_handler;
        self.err_handler_ran = false;
        let res = self.call_at_slot(host, call_slot, nargs, None, saved_frames);
        self.err_handler = prev_handler;
        self.err_handler_ran = prev_ran;
        match res {
            Ok(()) => {
                self.error_value = prev_err;
                self.temp_roots.truncate(saved_temp);
                self.temp_proto_roots.truncate(saved_proto_temp);
                Ok(Ok(()))
            }
            Err(err) => {
                self.close_upvals_from(call_slot);
                self.frames.truncate(saved_frames);
                self.call_trace.truncate(saved_trace);
                self.stack.truncate(call_slot);
                self.clear_nil_tags_from(call_slot);
                self.temp_roots.truncate(saved_temp);
                self.temp_proto_roots.truncate(saved_proto_temp);
                if !err.is_catchable() {
                    self.error_value = prev_err;
                    return Err(err);
                }
                let err_val = match self.error_value.take() {
                    Some(v) => v,
                    None => {
                        let msg = err.to_string();
                        Value::String(self.intern_bytes(msg.as_bytes())?)
                    }
                };
                self.error_value = prev_err;
                self.failure_site = None;
                Ok(Err(err_val))
            }
        }
    }

    /// Invoke the active `xpcall` error handler BEFORE unwinding frames, matching
    /// Lua 5.0.3 `luaG_errormsg` (`ldebug.c:565-575`).
    fn run_err_handler_if_needed(&mut self, host: &mut H, err: ScriptError) -> ScriptError {
        if !err.is_catchable() || self.err_handler_ran {
            return err;
        }
        let Some(handler) = self.err_handler else {
            return err;
        };
        self.err_handler_ran = true;
        if self.err_handler_depth >= 200 || !handler.is_function() {
            if let Ok(sid) = self.intern_bytes(b"error in error handling") {
                self.error_value = Some(Value::String(sid));
            }
            return ScriptError::runtime("error in error handling");
        }
        let err_val = match self.error_value.take() {
            Some(v) => v,
            None => {
                let msg = err.to_string();
                match self.intern_bytes(msg.as_bytes()) {
                    Ok(sid) => Value::String(sid),
                    Err(mem_err) => return mem_err,
                }
            }
        };
        self.err_handler_depth += 1;
        let prev_handler = self.err_handler.take();
        let prev_ran = self.err_handler_ran;
        let mark = self.temp_roots.len();
        let h_res = self
            .extend_roots(&[handler, err_val], &[])
            .and_then(|()| self.call_nested_one(host, handler, &[err_val]));
        self.temp_roots.truncate(mark);
        self.err_handler = prev_handler;
        self.err_handler_ran = prev_ran;
        self.err_handler_depth -= 1;

        match h_res {
            Ok(result_val) => {
                // The handler's result is copied for the error's display: charged first.
                if let Value::String(sid) = result_val {
                    let len = self.heap.string_bytes(sid).len();
                    if let Err(e) = self
                        .charge_work(1 + len.units() / BYTES_PER_UNIT)
                        .and_then(|()| self.ensure_temp_bytes(len))
                    {
                        return e;
                    }
                }
                let display = self.default_tostring_unchecked(result_val);
                self.error_value = Some(result_val);
                ScriptError::Runtime(display)
            }
            Err(h_err) if !h_err.is_catchable() => h_err,
            Err(_) => {
                if let Ok(sid) = self.intern_bytes(b"error in error handling") {
                    self.error_value = Some(Value::String(sid));
                }
                ScriptError::runtime("error in error handling")
            }
        }
    }

    pub(crate) fn pcall_with_handler_into(
        &mut self,
        host: &mut H,
        func: Value,
        args: &[Value],
        err_handler: Option<Value>,
        out: &mut Vec<Value>,
    ) -> Result<Result<(), Value>, ScriptError> {
        let prev_err = self.error_value.take();
        let prev_handler = self.err_handler.take();
        let prev_ran = self.err_handler_ran;
        let saved_temp = self.temp_roots.len();
        let saved_proto_temp = self.temp_proto_roots.len();
        self.err_handler = err_handler;
        self.err_handler_ran = false;
        let res = self.call_nested_into(host, func, args, out);
        self.err_handler = prev_handler;
        self.err_handler_ran = prev_ran;
        match res {
            Ok(()) => {
                self.error_value = prev_err;
                self.temp_roots.truncate(saved_temp);
                self.temp_proto_roots.truncate(saved_proto_temp);
                Ok(Ok(()))
            }
            Err(err) => {
                self.temp_roots.truncate(saved_temp);
                self.temp_proto_roots.truncate(saved_proto_temp);
                if !err.is_catchable() {
                    self.error_value = prev_err;
                    return Err(err);
                }
                let err_val = match self.error_value.take() {
                    Some(v) => v,
                    None => {
                        let msg = err.to_string();
                        Value::String(self.intern_bytes(msg.as_bytes())?)
                    }
                };
                self.error_value = prev_err;
                Ok(Err(err_val))
            }
        }
    }

    /// Call the function placed at `stack[func_slot]` with `nargs` arguments at
    /// `stack[func_slot + 1 .. func_slot + 1 + nargs]`.
    /// When `expected_results` is `None`, results remain at `stack[func_slot..]`.
    /// When `expected_results` is `Some(n)`, exactly `n` values are written at `stack[func_slot..func_slot+n]`.
    /// Nested call of the function at `func_slot` (from a native function, a metamethod
    /// or a `for` iterator). Every nested call recurses on the native stack, whether it
    /// runs Lua or a native function that calls further, so each one counts toward the
    /// re-entry limit.
    fn call_at_slot(
        &mut self,
        host: &mut H,
        func_slot: usize,
        nargs: usize,
        expected_results: Option<usize>,
        stop_depth: usize,
    ) -> Result<(), ScriptError> {
        if self.native_reentry_depth >= MAX_NATIVE_REENTRY_DEPTH {
            return Err(ScriptError::StackOverflow {
                limit: MAX_NATIVE_REENTRY_DEPTH,
            });
        }
        self.native_reentry_depth += 1;
        self.failure_site = None;
        let res = self.call_at_slot_inner(host, func_slot, nargs, expected_results, stop_depth);
        if res.is_err() && self.failure_site.is_none() {
            if let Some(frame) = self.frames.last() {
                self.failure_site = Some(InstructionSite {
                    function: self.heap.proto_key(frame.proto),
                    instruction: frame.pc.saturating_sub(1),
                });
            }
        }
        self.native_reentry_depth -= 1;
        res
    }

    fn call_at_slot_inner(
        &mut self,
        host: &mut H,
        func_slot: usize,
        mut nargs: usize,
        expected_results: Option<usize>,
        stop_depth: usize,
    ) -> Result<(), ScriptError> {
        let mut func = self.stack[func_slot];

        // Unknown global call hook (recording host).
        if func.is_nil() {
            if let (Some(handler), Some(name_id)) = (
                self.unknown_global_handler.clone(),
                self.get_nil_tag(func_slot),
            ) {
                return self.invoke_unknown_global(
                    host,
                    handler,
                    name_id,
                    func_slot,
                    nargs,
                    expected_results,
                );
            }
        }

        // `__call` metamethod fallback (Lua 5.0 unwraps one level).
        if !func.is_function() {
            let tm = self.heap.get_tm(func, TagMethod::Call);
            if !tm.is_function() {
                let err = self.lua_type_error(None, func, "call");
                return Err(self.run_err_handler_if_needed(host, err));
            }
            if self.stack.len() + 1 > self.limits.max_stack_slots {
                return Err(ScriptError::StackOverflow {
                    limit: self.limits.max_call_depth,
                });
            }
            // Inserting the metamethod moves the function and its arguments up.
            self.charge_work(1 + (self.stack.len() - func_slot).units() / 8)?;
            self.mark_stack_write(tm);
            self.stack.insert(func_slot, tm);
            nargs += 1;
            func = tm;
        }

        match func {
            Value::NativeFn(fid) => {
                self.invoke_native(host, fid, func_slot, nargs, expected_results)
            }
            Value::Closure(cid) => {
                self.push_lua_frame(cid, func_slot, nargs, expected_results)?;
                self.execute_until(host, stop_depth)
            }
            _ => unreachable!(),
        }
    }

    fn invoke_native(
        &mut self,
        host: &mut H,
        fid: NativeFnId,
        func_slot: usize,
        nargs: usize,
        expected_results: Option<usize>,
    ) -> Result<(), ScriptError> {
        if self.call_trace.len() >= self.limits.max_call_depth {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        let Some(kind) = self.heap.get_native(fid).map(|rec| rec.kind.clone()) else {
            return Err(ScriptError::InvalidHandle("native function"));
        };
        // Entering and leaving a native function costs about as much as a few
        // instructions (argument checks, result delivery).
        self.charge_work(NATIVE_CALL_UNITS)?;
        let arg_base = func_slot + 1;
        let ret_start = arg_base + nargs;
        self.stack.truncate(ret_start);
        self.call_trace.push(TraceFrame::Native {
            func: Value::NativeFn(fid),
        });
        let mut ctx = CallCtx {
            vm: self,
            host,
            fn_id: Some(fid),
            arg_base,
            arg_count: nargs,
            ret_count: 0,
            scratch_bytes: 0,
            expected_results,
        };
        let call_res = match kind {
            NativeKind::Fn(f) => f(&mut ctx),
            NativeKind::Closure(c) => c(&mut ctx),
        };
        let ret_count = ctx.ret_count;
        ctx.vm.heap.unreserve(ctx.scratch_bytes);
        if let Err(err) = call_res {
            // Run `xpcall` error handler BEFORE popping this native frame so
            // `getfenv`/`lua_getstack` sees the exact Lua 5.0 call chain.
            let handled = self.run_err_handler_if_needed(host, err);
            self.call_trace.pop();
            return Err(handled);
        }
        // A native that handled an error itself leaves no error value behind.
        self.error_value = None;
        self.call_trace.pop();
        self.deliver_results(func_slot, ret_start, ret_count, expected_results)?;
        Ok(())
    }

    fn invoke_unknown_global(
        &mut self,
        host: &mut H,
        handler: UnknownGlobalCallHandler<H>,
        name_id: StringId,
        func_slot: usize,
        nargs: usize,
        expected_results: Option<usize>,
    ) -> Result<(), ScriptError> {
        if self.call_trace.len() >= self.limits.max_call_depth {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        // The host handler receives the name as text.
        self.charge_work(self.string_bytes_unchecked(name_id).len().div_ceil(32).units())?;
        let name = self.string_latin1_unchecked(name_id);
        let arg_base = func_slot + 1;
        let ret_start = arg_base + nargs;
        self.stack.truncate(ret_start);
        self.call_trace
            .push(TraceFrame::Native { func: Value::Nil });
        let mut ctx = CallCtx {
            vm: self,
            host,
            fn_id: None,
            arg_base,
            arg_count: nargs,
            ret_count: 0,
            scratch_bytes: 0,
            expected_results,
        };
        let call_res = handler(&mut ctx, &name);
        let ret_count = ctx.ret_count;
        ctx.vm.heap.unreserve(ctx.scratch_bytes);
        if let Err(err) = call_res {
            let handled = self.run_err_handler_if_needed(host, err);
            self.call_trace.pop();
            return Err(handled);
        }
        self.error_value = None;
        self.call_trace.pop();
        self.set_nil_tag(func_slot, None)?;
        self.deliver_results(func_slot, ret_start, ret_count, expected_results)?;
        Ok(())
    }

    fn push_lua_frame(
        &mut self,
        cid: ClosureId,
        func_slot: usize,
        nargs: usize,
        expected_results: Option<usize>,
    ) -> Result<(), ScriptError> {
        if self.call_trace.len() >= self.limits.max_call_depth {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        let proto = self.heap.closure(cid).proto;
        let base = func_slot + 1;
        let max_stack = self.heap.loaded_proto(proto).proto.max_stack_size as usize;
        let frame_top = base + max_stack;
        if frame_top > self.limits.max_stack_slots {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        // Setting up the frame clears up to `max_stack` registers.
        self.charge_work(1 + max_stack.units() / 32)?;
        self.prepare_lua_args(proto, base, nargs, frame_top)?;
        let frame_idx = self.frames.len();
        self.mark_stack_write(Value::Closure(cid));
        self.frames.push(CallFrame {
            closure: cid,
            proto,
            base,
            result_slot: func_slot,
            expected_results,
            frame_top,
            pc: 0,
        });
        self.call_trace.push(TraceFrame::Lua {
            frame_idx,
            tailcalls: 0,
        });
        Ok(())
    }

    fn prepare_lua_args(
        &mut self,
        proto: LoadedProtoId,
        base: usize,
        nargs: usize,
        frame_top: usize,
    ) -> Result<(), ScriptError> {
        let (num_params, is_vararg) = {
            let p = &self.heap.loaded_proto(proto).proto;
            (p.num_params as usize, p.is_vararg != 0)
        };
        self.stack.truncate(base + nargs);
        if is_vararg {
            // Pad missing fixed parameters with nil.
            if nargs < num_params {
                self.stack.resize(base + num_params, Value::Nil);
            }
            let extra = self.stack.len() - (base + num_params);
            self.charge_work(1 + extra.units() / 2)?;
            let arg_tbl = self.create_table()?;
            let mark = self.temp_roots.len();
            self.push_temp_root(Value::Table(arg_tbl))?;
            let res: Result<(), ScriptError> = (|| {
                for i in 0..extra {
                    let v = self.stack[base + num_params + i];
                    self.raw_set_vm(arg_tbl, Value::Number((i + 1) as f32), v)?;
                }
                let n_key = Value::String(self.heap.well_known.n_field);
                self.raw_set_vm(arg_tbl, n_key, Value::Number(extra as f32))?;
                Ok(())
            })();
            self.temp_roots.truncate(mark);
            res?;
            self.stack.truncate(base + num_params);
            self.push_stack(Value::Table(arg_tbl));
            self.stack.resize(frame_top, Value::Nil);
        } else {
            if nargs > num_params {
                self.stack.truncate(base + num_params);
            }
            self.stack.resize(frame_top, Value::Nil);
        }
        self.clear_nil_tags_from(base);
        Ok(())
    }

    fn deliver_results(
        &mut self,
        result_slot: usize,
        first_res: usize,
        nret: usize,
        expected_results: Option<usize>,
    ) -> Result<(), ScriptError> {
        let final_len = match expected_results {
            Some(want) => result_slot.checked_add(want),
            None => result_slot.checked_add(nret),
        }
        .ok_or(ScriptError::StackOverflow {
            limit: self.limits.max_call_depth,
        })?;
        if final_len > self.limits.max_stack_slots {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        let copied = expected_results.unwrap_or(nret);
        if copied >= 16 {
            self.charge_work(copied.units() / 16)?;
        }
        // Grow destination before writing so short callees (native or small Lua
        // frames) never index past the end of `self.stack`.
        if self.stack.len() < final_len {
            self.stack.resize(final_len, Value::Nil);
        }
        match expected_results {
            Some(want) => {
                let copy = nret.min(want);
                for i in 0..copy {
                    let val = self.stack.get(first_res + i).copied().unwrap_or(Value::Nil);
                    self.write_stack(result_slot + i, val);
                    self.set_nil_tag(result_slot + i, None)?;
                }
                for i in copy..want {
                    self.write_stack(result_slot + i, Value::Nil);
                    self.set_nil_tag(result_slot + i, None)?;
                }
            }
            None => {
                for i in 0..nret {
                    let val = self.stack.get(first_res + i).copied().unwrap_or(Value::Nil);
                    self.write_stack(result_slot + i, val);
                    self.set_nil_tag(result_slot + i, None)?;
                }
            }
        }
        self.stack.truncate(final_len);
        self.clear_nil_tags_from(final_len);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Metamethod & comparison helpers
    // -----------------------------------------------------------------------

    fn call_tm_res(
        &mut self,
        host: &mut H,
        tm: Value,
        p1: Value,
        p2: Value,
    ) -> Result<Value, ScriptError> {
        let saved = self.stack.len();
        let stop = self.frames.len();
        let saved_trace = self.call_trace.len();
        if saved + 3 > self.limits.max_stack_slots {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        self.push_stack(tm);
        self.push_stack(p1);
        self.push_stack(p2);
        let res = self.call_at_slot(host, saved, 2, Some(1), stop);
        match res {
            Ok(()) => {
                let v = self.stack[saved];
                self.stack.truncate(saved);
                self.clear_nil_tags_from(saved);
                Ok(v)
            }
            Err(e) => {
                self.close_upvals_from(saved);
                self.frames.truncate(stop);
                self.call_trace.truncate(saved_trace);
                self.stack.truncate(saved);
                self.clear_nil_tags_from(saved);
                Err(e)
            }
        }
    }

    fn call_tm_void(
        &mut self,
        host: &mut H,
        tm: Value,
        p1: Value,
        p2: Value,
        p3: Value,
    ) -> Result<(), ScriptError> {
        let saved = self.stack.len();
        let stop = self.frames.len();
        let saved_trace = self.call_trace.len();
        if saved + 4 > self.limits.max_stack_slots {
            return Err(ScriptError::StackOverflow {
                limit: self.limits.max_call_depth,
            });
        }
        self.push_stack(tm);
        self.push_stack(p1);
        self.push_stack(p2);
        self.push_stack(p3);
        let res = self.call_at_slot(host, saved, 3, Some(0), stop);
        self.close_upvals_from(saved);
        if res.is_err() {
            self.frames.truncate(stop);
            self.call_trace.truncate(saved_trace);
        }
        self.stack.truncate(saved);
        self.clear_nil_tags_from(saved);
        res
    }

    fn get_table_val(
        &mut self,
        host: &mut H,
        mut t: Value,
        key: Value,
        mut reg_t: Option<usize>,
    ) -> Result<Value, ScriptError> {
        for _ in 0..=MAX_TAG_LOOP {
            let tm = if let Value::Table(tid) = t {
                let v = self.heap.table(tid).raw_get(key);
                if !v.is_nil() {
                    return Ok(v);
                }
                let tm = self.heap.get_tm(t, TagMethod::Index);
                if tm.is_nil() {
                    return Ok(Value::Nil);
                }
                tm
            } else {
                let tm = self.heap.get_tm(t, TagMethod::Index);
                if tm.is_nil() {
                    return Err(self.lua_type_error(reg_t, t, "index"));
                }
                tm
            };

            if tm.is_function() {
                return self.call_tm_res(host, tm, t, key);
            }
            self.charge_work(1)?;
            t = tm;
            reg_t = None;
        }
        Err(self.lua_runerror(b"loop in gettable"))
    }

    fn set_table_val(
        &mut self,
        host: &mut H,
        mut t: Value,
        key: Value,
        val: Value,
        mut reg_t: Option<usize>,
    ) -> Result<(), ScriptError> {
        for _ in 0..=MAX_TAG_LOOP {
            let tm = if let Value::Table(tid) = t {
                if key.is_nil() {
                    return Err(self.lua_runerror(b"table index is nil"));
                }
                if matches!(key, Value::Number(n) if n.is_nan()) {
                    return Err(self.lua_runerror(b"table index is NaN"));
                }
                let old = self.heap.table(tid).raw_get(key);
                if !old.is_nil() {
                    return self.raw_set_vm(tid, key, val);
                }
                let tm = self.heap.get_tm(t, TagMethod::NewIndex);
                if tm.is_nil() {
                    return self.raw_set_vm(tid, key, val);
                }
                tm
            } else {
                let tm = self.heap.get_tm(t, TagMethod::NewIndex);
                if tm.is_nil() {
                    return Err(self.lua_type_error(reg_t, t, "index"));
                }
                tm
            };

            if tm.is_function() {
                return self.call_tm_void(host, tm, t, key, val);
            }
            self.charge_work(1)?;
            t = tm;
            reg_t = None;
        }
        Err(self.lua_runerror(b"loop in settable"))
    }

    fn call_bin_tm(
        &mut self,
        host: &mut H,
        p1: Value,
        p2: Value,
        event: TagMethod,
    ) -> Result<Option<Value>, ScriptError> {
        let mut tm = self.heap.get_tm(p1, event);
        if tm.is_nil() {
            tm = self.heap.get_tm(p2, event);
        }
        if !tm.is_function() {
            return Ok(None);
        }
        Ok(Some(self.call_tm_res(host, tm, p1, p2)?))
    }

    #[allow(clippy::too_many_arguments)]
    fn arith_op(
        &mut self,
        host: &mut H,
        rb: Value,
        rc: Value,
        reg_b: Option<usize>,
        reg_c: Option<usize>,
        event: TagMethod,
    ) -> Result<Value, ScriptError> {
        let nb = self.coerce_number(rb)?;
        let nc = match nb {
            Some(_) => self.coerce_number(rc)?,
            None => None,
        };
        if let (Some(b), Some(c)) = (nb, nc) {
            return match event {
                TagMethod::Add => Ok(Value::Number(canonicalize_number(b + c))),
                TagMethod::Sub => Ok(Value::Number(canonicalize_number(b - c))),
                TagMethod::Mul => Ok(Value::Number(canonicalize_number(b * c))),
                TagMethod::Div => Ok(Value::Number(canonicalize_number(b / c))),
                TagMethod::Pow => {
                    let pow_fn = self
                        .heap
                        .table(self.globals)
                        .raw_get_str(self.heap.well_known.tm[TagMethod::Pow as usize]);
                    if !pow_fn.is_function() {
                        return Err(self.lua_runerror(b"`__pow' (`^' operator) is not a function"));
                    }
                    self.call_tm_res(host, pow_fn, Value::Number(b), Value::Number(c))
                }
                _ => unreachable!(),
            };
        }
        if let Some(res) = self.call_bin_tm(host, rb, rc, event)? {
            return Ok(res);
        }
        let (bad_reg, bad) = if nb.is_none() { (reg_b, rb) } else { (reg_c, rc) };
        Err(self.lua_type_error(bad_reg, bad, "perform arithmetic on"))
    }

    pub(crate) fn lua_equal(&mut self, host: &mut H, a: Value, b: Value) -> Result<bool, ScriptError> {
        if core::mem::discriminant(&a) != core::mem::discriminant(&b) {
            return Ok(false);
        }
        match (a, b) {
            (Value::Table(t1), Value::Table(t2)) => {
                if t1 == t2 {
                    return Ok(true);
                }
                let tm1 = self.heap.get_tm(a, TagMethod::Eq);
                if tm1.is_nil() {
                    return Ok(false);
                }
                let tm2 = self.heap.get_tm(b, TagMethod::Eq);
                if !tm1.rawequal(tm2) {
                    return Ok(false);
                }
                let res = self.call_tm_res(host, tm1, a, b)?;
                Ok(res.is_truthy())
            }
            (Value::Userdata(u1), Value::Userdata(u2)) => {
                if u1 == u2 {
                    return Ok(true);
                }
                let tm1 = self.heap.get_tm(a, TagMethod::Eq);
                if tm1.is_nil() {
                    return Ok(false);
                }
                let tm2 = self.heap.get_tm(b, TagMethod::Eq);
                if !tm1.rawequal(tm2) {
                    return Ok(false);
                }
                let res = self.call_tm_res(host, tm1, a, b)?;
                Ok(res.is_truthy())
            }
            _ => Ok(a.rawequal(b)),
        }
    }

    fn call_order_tm(
        &mut self,
        host: &mut H,
        p1: Value,
        p2: Value,
        event: TagMethod,
    ) -> Result<Option<bool>, ScriptError> {
        let tm1 = self.heap.get_tm(p1, event);
        if tm1.is_nil() {
            return Ok(None);
        }
        let tm2 = self.heap.get_tm(p2, event);
        if !tm1.rawequal(tm2) {
            return Ok(None);
        }
        let res = self.call_tm_res(host, tm1, p1, p2)?;
        Ok(Some(res.is_truthy()))
    }

    /// Charge an ordering comparison of two strings (bytes up to the shorter length).
    #[inline]
    fn charge_compare(&mut self, a: StringId, b: StringId) -> Result<(), ScriptError> {
        let n = self.heap.string_bytes(a).len().min(self.heap.string_bytes(b).len());
        if n.units() >= BYTES_PER_UNIT {
            self.charge_work(n.units() / BYTES_PER_UNIT)?;
        }
        Ok(())
    }

    pub(crate) fn lua_lessthan(
        &mut self,
        host: &mut H,
        l: Value,
        r: Value,
    ) -> Result<bool, ScriptError> {
        match (l, r) {
            (Value::Number(a), Value::Number(b)) => Ok(a < b),
            (Value::String(a), Value::String(b)) => {
                self.charge_compare(a, b)?;
                Ok(self.heap.string_bytes(a) < self.heap.string_bytes(b))
            }
            _ => {
                if core::mem::discriminant(&l) == core::mem::discriminant(&r) {
                    if let Some(res) = self.call_order_tm(host, l, r, TagMethod::Lt)? {
                        return Ok(res);
                    }
                }
                Err(self.lua_order_error(l, r))
            }
        }
    }

    fn lua_lessequal(&mut self, host: &mut H, l: Value, r: Value) -> Result<bool, ScriptError> {
        match (l, r) {
            (Value::Number(a), Value::Number(b)) => Ok(a <= b),
            (Value::String(a), Value::String(b)) => {
                self.charge_compare(a, b)?;
                Ok(self.heap.string_bytes(a) <= self.heap.string_bytes(b))
            }
            _ => {
                if core::mem::discriminant(&l) == core::mem::discriminant(&r) {
                    if let Some(res) = self.call_order_tm(host, l, r, TagMethod::Le)? {
                        return Ok(res);
                    }
                    if let Some(res) = self.call_order_tm(host, r, l, TagMethod::Lt)? {
                        return Ok(!res);
                    }
                }
                Err(self.lua_order_error(l, r))
            }
        }
    }

    /// Convert a number operand of `..` to a string in place (as `luaV_concat` does
    /// before calling `__concat`).
    fn coerce_concat_str(&mut self, slot: usize) -> Result<(), ScriptError> {
        if let Value::Number(n) = self.stack[slot] {
            let text = numfmt::lua_number_text(n);
            let sid = self.intern_bytes(text.as_bytes())?;
            self.write_stack(slot, Value::String(sid));
        }
        Ok(())
    }

    #[inline]
    fn is_concat_piece(v: Value) -> bool {
        matches!(v, Value::String(_) | Value::Number(_))
    }

    /// Lua 5.0.3 `luaV_concat` over registers `b..=c`. Runs of strings and numbers are
    /// joined in a reused buffer, so joining allocates nothing when the result string
    /// already exists.
    fn do_concat(
        &mut self,
        host: &mut H,
        base: usize,
        b: usize,
        c: usize,
    ) -> Result<(), ScriptError> {
        let mut total = c - b + 1;
        let mut last = c;
        while total > 1 {
            let right_slot = base + last;
            let left_slot = right_slot - 1;
            let lhs = self.stack[left_slot];
            let rhs = self.stack[right_slot];
            if !Self::is_concat_piece(lhs) || !Self::is_concat_piece(rhs) {
                // `!tostring(top-2) || !tostring(top-1)`: a number on the left is
                // converted before the metamethod sees it.
                if Self::is_concat_piece(lhs) {
                    self.coerce_concat_str(left_slot)?;
                }
                let lhs = self.stack[left_slot];
                if let Some(res) = self.call_bin_tm(host, lhs, rhs, TagMethod::Concat)? {
                    self.write_stack(left_slot, res);
                } else {
                    let (bad_slot, bad) = if Self::is_concat_piece(lhs) {
                        (right_slot, rhs)
                    } else {
                        (left_slot, lhs)
                    };
                    return Err(self.lua_type_error(
                        Some(bad_slot - base),
                        bad,
                        "concatenate",
                    ));
                }
                total -= 1;
                last -= 1;
                continue;
            }
            // Coalesce as many adjacent strings/numbers from the right as possible.
            let mut n = 2usize;
            while n < total && Self::is_concat_piece(self.stack[right_slot - n]) {
                n += 1;
            }
            let start_slot = right_slot + 1 - n;
            let mut total_len = 0usize;
            for slot in start_slot..=right_slot {
                total_len = total_len.saturating_add(match self.stack[slot] {
                    Value::String(sid) => self.heap.string_bytes(sid).len(),
                    Value::Number(x) => numfmt::lua_number_text(x).as_bytes().len(),
                    _ => 0,
                });
            }
            if total_len > self.limits.memory_limit {
                return Err(ScriptError::MemoryLimit {
                    limit: self.limits.memory_limit,
                });
            }
            self.charge_work(n.units() + total_len.units() / BYTES_PER_UNIT)?;
            if total_len > self.concat_buf.capacity() {
                let old = crate::value::block_bytes(self.concat_buf.capacity());
                let new = crate::value::block_bytes(total_len);
                if self.heap.check_budget(new).is_err() {
                    self.collect_for_memory();
                }
                self.heap.check_budget(new)?;
                self.concat_buf
                    .try_reserve_exact(total_len - self.concat_buf.len())
                    .map_err(|_| ScriptError::ResourceLimit("concatenation buffer"))?;
                self.heap.reserve(new - old)?;
            }
            let mut buf = std::mem::take(&mut self.concat_buf);
            buf.clear();
            for slot in start_slot..=right_slot {
                match self.stack[slot] {
                    Value::String(sid) => buf.extend_from_slice(self.heap.string_bytes(sid)),
                    Value::Number(x) => buf.extend_from_slice(numfmt::lua_number_text(x).as_bytes()),
                    _ => {}
                }
            }
            let res = self.intern_bytes(&buf);
            if buf.capacity() <= CONCAT_BUF_KEEP {
                self.concat_buf = buf;
            } else {
                self.heap
                    .unreserve(crate::value::block_bytes(buf.capacity()));
            }
            let sid = res?;
            self.write_stack(start_slot, Value::String(sid));
            total -= n - 1;
            last -= n - 1;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Main bytecode interpreter loop (all 35 Lua 5.0.3 opcodes)
    // -----------------------------------------------------------------------

    fn execute_until(&mut self, host: &mut H, stop_depth: usize) -> Result<(), ScriptError> {
        match self.execute_until_inner(host, stop_depth) {
            Ok(()) => Ok(()),
            Err(e) => Err(self.run_err_handler_if_needed(host, e)),
        }
    }

    fn execute_until_inner(&mut self, host: &mut H, stop_depth: usize) -> Result<(), ScriptError> {
        loop {
            if self.frames.len() <= stop_depth {
                return Ok(());
            }
            if self.instructions_left == 0 {
                return Err(ScriptError::InstructionLimit {
                    limit: self.call_budget,
                });
            }
            self.instructions_left -= 1;

            let frame_idx = self.frames.len() - 1;
            let (base, proto_id, pc, ins) = {
                let f = &mut self.frames[frame_idx];
                let pc = f.pc;
                let proto_id = f.proto;
                let ins = self.heap.loaded_proto(proto_id).proto.code[pc];
                f.pc = pc + 1;
                (f.base, proto_id, pc + 1, ins)
            };

            let op = ins
                .op()
                .ok_or_else(|| ScriptError::runtime("invalid opcode"))?;
            let a = ins.a();
            let ra = base + a;

            match op {
                OpCode::Move => {
                    let rb = base + ins.b();
                    self.write_stack(ra, self.stack[rb]);
                    if self.unknown_global_handler.is_some() {
                        let tag = self.get_nil_tag(rb);
                        self.set_nil_tag(ra, tag)?;
                    }
                }
                OpCode::LoadK => {
                    let k = self.heap.loaded_proto(proto_id).constants[ins.bx()];
                    self.write_stack(ra, k);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::LoadBool => {
                    self.write_stack(ra, Value::Bool(ins.b() != 0));
                    self.set_nil_tag(ra, None)?;
                    if ins.c() != 0 {
                        self.frames[frame_idx].pc = pc + 1;
                    }
                }
                OpCode::LoadNil => {
                    let b = ins.b();
                    if b >= a {
                        self.charge_work((b - a).units())?;
                    }
                    for r in a..=b {
                        self.write_stack(base + r, Value::Nil);
                        self.set_nil_tag(base + r, None)?;
                    }
                }
                OpCode::GetUpval => {
                    let cid = self.frames[frame_idx].closure;
                    let uid = self
                        .heap
                        .closure(cid)
                        .upvalues
                        .get(ins.b())
                        .ok_or_else(|| ScriptError::runtime("upvalue index out of range"))?;
                    self.write_stack(ra, self.read_upval(uid));
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::GetGlobal => {
                    let key = self.heap.loaded_proto(proto_id).constants[ins.bx()];
                    let env = self.heap.closure(self.frames[frame_idx].closure).env;
                    let raw = if let Value::String(sid) = key {
                        self.heap.table(env).raw_get_str(sid)
                    } else {
                        self.heap.table(env).raw_get(key)
                    };
                    let val = if !raw.is_nil() {
                        raw
                    } else {
                        self.get_table_val(host, Value::Table(env), key, None)?
                    };
                    self.write_stack(ra, val);
                    if val.is_nil() && self.unknown_global_handler.is_some() {
                        if let Value::String(sid) = key {
                            self.set_nil_tag(ra, Some(sid))?;
                        } else {
                            self.set_nil_tag(ra, None)?;
                        }
                    } else {
                        self.set_nil_tag(ra, None)?;
                    }
                }
                OpCode::GetTable => {
                    let b = ins.b();
                    let tbl = self.stack[base + b];
                    let key = self.rk(frame_idx, base, ins.c());
                    let val = if let Value::Table(tid) = tbl {
                        let raw = self.heap.table(tid).raw_get(key);
                        if !raw.is_nil() {
                            raw
                        } else {
                            self.get_table_val(host, tbl, key, Some(b))?
                        }
                    } else {
                        self.get_table_val(host, tbl, key, Some(b))?
                    };
                    self.write_stack(ra, val);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::SetGlobal => {
                    let key = self.heap.loaded_proto(proto_id).constants[ins.bx()];
                    let env = self.heap.closure(self.frames[frame_idx].closure).env;
                    let val = self.stack[ra];
                    self.set_table_val(host, Value::Table(env), key, val, None)?;
                }
                OpCode::SetUpval => {
                    let cid = self.frames[frame_idx].closure;
                    let uid = self
                        .heap
                        .closure(cid)
                        .upvalues
                        .get(ins.b())
                        .ok_or_else(|| ScriptError::runtime("upvalue index out of range"))?;
                    let val = self.stack[ra];
                    self.write_upval(uid, val);
                }
                OpCode::SetTable => {
                    let tbl = self.stack[ra];
                    let key = self.rk(frame_idx, base, ins.b());
                    let val = self.rk(frame_idx, base, ins.c());
                    self.set_table_val(host, tbl, key, val, Some(a))?;
                }
                OpCode::NewTable => {
                    // The size hints only preallocate in Lua 5.0; tables here grow on
                    // demand so nothing is reserved that is not charged.
                    let tid = self.create_table()?;
                    self.write_stack(ra, Value::Table(tid));
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::SelfOp => {
                    let b = ins.b();
                    let obj = self.stack[base + b];
                    let key = self.rk(frame_idx, base, ins.c());
                    self.write_stack(ra + 1, obj);
                    self.set_nil_tag(ra + 1, None)?;
                    let method = self.get_table_val(host, obj, key, Some(b))?;
                    self.write_stack(ra, method);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Add => {
                    let b = ins.b();
                    let c = ins.c();
                    let rb = self.rk(frame_idx, base, b);
                    let rc = self.rk(frame_idx, base, c);
                    let res = if let (Value::Number(x), Value::Number(y)) = (rb, rc) {
                        Value::Number(canonicalize_number(x + y))
                    } else {
                        let reg_b = (b < MAXSTACK).then_some(b);
                        let reg_c = (c < MAXSTACK).then_some(c);
                        self.arith_op(host, rb, rc, reg_b, reg_c, TagMethod::Add)?
                    };
                    self.write_stack(ra, res);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Sub => {
                    let b = ins.b();
                    let c = ins.c();
                    let rb = self.rk(frame_idx, base, b);
                    let rc = self.rk(frame_idx, base, c);
                    let res = if let (Value::Number(x), Value::Number(y)) = (rb, rc) {
                        Value::Number(canonicalize_number(x - y))
                    } else {
                        let reg_b = (b < MAXSTACK).then_some(b);
                        let reg_c = (c < MAXSTACK).then_some(c);
                        self.arith_op(host, rb, rc, reg_b, reg_c, TagMethod::Sub)?
                    };
                    self.write_stack(ra, res);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Mul => {
                    let b = ins.b();
                    let c = ins.c();
                    let rb = self.rk(frame_idx, base, b);
                    let rc = self.rk(frame_idx, base, c);
                    let res = if let (Value::Number(x), Value::Number(y)) = (rb, rc) {
                        Value::Number(canonicalize_number(x * y))
                    } else {
                        let reg_b = (b < MAXSTACK).then_some(b);
                        let reg_c = (c < MAXSTACK).then_some(c);
                        self.arith_op(host, rb, rc, reg_b, reg_c, TagMethod::Mul)?
                    };
                    self.write_stack(ra, res);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Div => {
                    let b = ins.b();
                    let c = ins.c();
                    let rb = self.rk(frame_idx, base, b);
                    let rc = self.rk(frame_idx, base, c);
                    let res = if let (Value::Number(x), Value::Number(y)) = (rb, rc) {
                        Value::Number(canonicalize_number(x / y))
                    } else {
                        let reg_b = (b < MAXSTACK).then_some(b);
                        let reg_c = (c < MAXSTACK).then_some(c);
                        self.arith_op(host, rb, rc, reg_b, reg_c, TagMethod::Div)?
                    };
                    self.write_stack(ra, res);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Pow => {
                    let b = ins.b();
                    let c = ins.c();
                    let rb = self.rk(frame_idx, base, b);
                    let rc = self.rk(frame_idx, base, c);
                    let reg_b = (b < MAXSTACK).then_some(b);
                    let reg_c = (c < MAXSTACK).then_some(c);
                    let res = self.arith_op(host, rb, rc, reg_b, reg_c, TagMethod::Pow)?;
                    self.write_stack(ra, res);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Unm => {
                    let b = ins.b();
                    let rb = self.stack[base + b];
                    let res = if let Some(n) = self.coerce_number(rb)? {
                        Value::Number(canonicalize_number(-n))
                    } else if let Some(v) = self.call_bin_tm(host, rb, Value::Nil, TagMethod::Unm)? {
                        v
                    } else {
                        return Err(self.lua_type_error(Some(b), rb, "perform arithmetic on"));
                    };
                    self.write_stack(ra, res);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Not => {
                    let rb = self.stack[base + ins.b()];
                    self.write_stack(ra, Value::Bool(rb.is_falsy()));
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Concat => {
                    let b = ins.b();
                    let c = ins.c();
                    self.do_concat(host, base, b, c)?;
                    self.write_stack(ra, self.stack[base + b]);
                    self.set_nil_tag(ra, None)?;
                }
                OpCode::Jmp => {
                    let dest = (pc as isize + ins.sbx() as isize) as usize;
                    self.frames[frame_idx].pc = dest;
                }
                OpCode::Eq => {
                    let rb = self.rk(frame_idx, base, ins.b());
                    let rc = self.rk(frame_idx, base, ins.c());
                    let eq = self.lua_equal(host, rb, rc)?;
                    let cond = (eq as usize) == a;
                    self.finish_cond_jump(frame_idx, pc, cond);
                }
                OpCode::Lt => {
                    let rb = self.rk(frame_idx, base, ins.b());
                    let rc = self.rk(frame_idx, base, ins.c());
                    let lt = self.lua_lessthan(host, rb, rc)?;
                    let cond = (lt as usize) == a;
                    self.finish_cond_jump(frame_idx, pc, cond);
                }
                OpCode::Le => {
                    let rb = self.rk(frame_idx, base, ins.b());
                    let rc = self.rk(frame_idx, base, ins.c());
                    let le = self.lua_lessequal(host, rb, rc)?;
                    let cond = (le as usize) == a;
                    self.finish_cond_jump(frame_idx, pc, cond);
                }
                OpCode::Test => {
                    // Lua 5.0 OP_TEST: if l_isfalse(R(B)) == C then pc++ else { R(A) = R(B); dojump }
                    let rb = self.stack[base + ins.b()];
                    let c_bool = ins.c() != 0;
                    if rb.is_falsy() == c_bool {
                        self.frames[frame_idx].pc = pc + 1;
                    } else {
                        self.write_stack(ra, rb);
                        self.set_nil_tag(ra, None)?;
                        self.finish_cond_jump(frame_idx, pc, true);
                    }
                }
                OpCode::Call => {
                    let b = ins.b();
                    let c = ins.c();
                    let nargs = if b == 0 {
                        self.stack.len().saturating_sub(ra + 1)
                    } else {
                        b - 1
                    };
                    let expected = if c == 0 { None } else { Some(c - 1) };
                    let caller_top = self.frames[frame_idx].frame_top;

                    let mut func = self.stack[ra];
                    if func.is_nil() {
                        if let (Some(handler), Some(name_id)) = (
                            self.unknown_global_handler.clone(),
                            self.get_nil_tag(ra),
                        ) {
                            self.invoke_unknown_global(host, handler, name_id, ra, nargs, expected)?;
                            if expected.is_some() {
                                self.stack.resize(caller_top, Value::Nil);
                            }
                            continue;
                        }
                    }

                    let mut eff_nargs = nargs;
                    if !func.is_function() {
                        let tm = self.heap.get_tm(func, TagMethod::Call);
                        if !tm.is_function() {
                            return Err(self.lua_type_error(Some(a), func, "call"));
                        }
                        self.stack.truncate(ra + 1 + eff_nargs);
                        if self.stack.len() + 1 > self.limits.max_stack_slots {
                            return Err(ScriptError::StackOverflow {
                                limit: self.limits.max_call_depth,
                            });
                        }
                        // Inserting the metamethod moves the function and arguments up.
                        self.charge_work(1 + (eff_nargs.units() + 1) / 8)?;
                        self.mark_stack_write(tm);
                        self.stack.insert(ra, tm);
                        self.clear_nil_tags_from(ra);
                        eff_nargs += 1;
                        func = tm;
                    }

                    match func {
                        Value::NativeFn(fid) => {
                            self.invoke_native(host, fid, ra, eff_nargs, expected)?;
                            if expected.is_some() {
                                self.stack.resize(caller_top, Value::Nil);
                            }
                        }
                        Value::Closure(cid) => {
                            self.push_lua_frame(cid, ra, eff_nargs, expected)?;
                        }
                        _ => unreachable!(),
                    }
                }
                OpCode::TailCall => {
                    let b = ins.b();
                    let nargs = if b == 0 {
                        self.stack.len().saturating_sub(ra + 1)
                    } else {
                        b - 1
                    };

                    let mut func = self.stack[ra];
                    if func.is_nil() {
                        if let (Some(handler), Some(name_id)) = (
                            self.unknown_global_handler.clone(),
                            self.get_nil_tag(ra),
                        ) {
                            // Match `lvm.c:628-638`: leave calling Lua frame active during
                            // native/C tailcall; the following `OP_RETURN A B=0` pops and returns.
                            self.invoke_unknown_global(host, handler, name_id, ra, nargs, None)?;
                            continue;
                        }
                    }

                    let mut eff_nargs = nargs;
                    if !func.is_function() {
                        let tm = self.heap.get_tm(func, TagMethod::Call);
                        if !tm.is_function() {
                            return Err(self.lua_type_error(Some(a), func, "call"));
                        }
                        self.stack.truncate(ra + 1 + eff_nargs);
                        if self.stack.len() + 1 > self.limits.max_stack_slots {
                            return Err(ScriptError::StackOverflow {
                                limit: self.limits.max_call_depth,
                            });
                        }
                        // Inserting the metamethod moves the function and arguments up.
                        self.charge_work(1 + (eff_nargs.units() + 1) / 8)?;
                        self.mark_stack_write(tm);
                        self.stack.insert(ra, tm);
                        self.clear_nil_tags_from(ra);
                        eff_nargs += 1;
                        func = tm;
                    }

                    match func {
                        Value::NativeFn(fid) => {
                            // Match `lvm.c:628-638`: keep the calling Lua frame active while the
                            // C/native function executes (preserving line info for `error()`),
                            // then fall through to the following `OP_RETURN A B=0`.
                            self.invoke_native(host, fid, ra, eff_nargs, None)?;
                        }
                        Value::Closure(cid) => {
                            // Replace current frame in place (proper Lua tail call).
                            self.close_upvals_charged(base)?;
                            let target_func_slot = self.frames[frame_idx].result_slot;
                            let target_expected = self.frames[frame_idx].expected_results;
                            let proto = self.heap.closure(cid).proto;
                            let new_base = target_func_slot + 1;
                            let max_stack =
                                self.heap.loaded_proto(proto).proto.max_stack_size as usize;
                            let new_top = new_base + max_stack;
                            if new_top > self.limits.max_stack_slots {
                                return Err(ScriptError::StackOverflow {
                                    limit: self.limits.max_call_depth,
                                });
                            }
                            // The new frame's setup (as for any call) and moving the
                            // function and arguments down, charged before either.
                            self.charge_work(
                                1 + max_stack.units() / 32 + (eff_nargs.units() + 1) / 8,
                            )?;
                            // Shift callee + args down to target_func_slot.
                            for i in 0..=eff_nargs {
                                self.write_stack(target_func_slot + i, self.stack[ra + i]);
                            }
                            self.prepare_lua_args(proto, new_base, eff_nargs, new_top)?;
                            self.frames[frame_idx] = CallFrame {
                                closure: cid,
                                proto,
                                base: new_base,
                                result_slot: target_func_slot,
                                expected_results: target_expected,
                                frame_top: new_top,
                                pc: 0,
                            };
                            if let Some(TraceFrame::Lua { tailcalls, .. }) =
                                self.call_trace.last_mut()
                            {
                                *tailcalls = tailcalls.saturating_add(1);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                OpCode::Return => {
                    let b = ins.b();
                    let nret = if b == 0 {
                        self.stack.len().saturating_sub(ra)
                    } else {
                        b - 1
                    };
                    self.close_upvals_charged(base)?;
                    let cur = self.frames.pop().unwrap();
                    self.call_trace.pop();
                    self.deliver_results(cur.result_slot, ra, nret, cur.expected_results)?;
                    if cur.expected_results.is_some() && self.frames.len() > stop_depth {
                        let parent_top = self.frames.last().unwrap().frame_top;
                        self.stack.resize(parent_top, Value::Nil);
                    }
                }
                OpCode::ForLoop => {
                    let idx_v = self.stack[ra];
                    let Value::Number(init) = idx_v else {
                        return Err(self.lua_runerror(b"`for' initial value must be a number"));
                    };
                    let Some(limit) = self.coerce_number(self.stack[ra + 1])? else {
                        return Err(self.lua_runerror(b"`for' limit must be a number"));
                    };
                    let Some(step) = self.coerce_number(self.stack[ra + 2])? else {
                        return Err(self.lua_runerror(b"`for' step must be a number"));
                    };
                    self.write_stack(ra + 1, Value::Number(limit));
                    self.write_stack(ra + 2, Value::Number(step));
                    let idx = canonicalize_number(init + step);
                    let keep_going = if step > 0.0 { idx <= limit } else { idx >= limit };
                    if keep_going {
                        self.write_stack(ra, Value::Number(idx));
                        let dest = (pc as isize + ins.sbx() as isize) as usize;
                        self.frames[frame_idx].pc = dest;
                    }
                }
                OpCode::TForLoop => {
                    let nvar = ins.c() + 1;
                    let cb = ra + nvar + 2;
                    let frame_top = self.frames[frame_idx].frame_top;
                    self.write_stack(cb, self.stack[ra]);
                    self.write_stack(cb + 1, self.stack[ra + 1]);
                    self.write_stack(cb + 2, self.stack[ra + 2]);
                    let stop = self.frames.len();
                    self.call_at_slot(host, cb, 2, Some(nvar), stop)?;
                    // Copy results while `cb..cb+nvar` is still live above `frame_top`,
                    // then restore the caller's frame top last.
                    for j in (0..nvar).rev() {
                        self.write_stack(ra + 2 + j, self.stack[cb + j]);
                        self.set_nil_tag(ra + 2 + j, None)?;
                    }
                    self.stack.resize(frame_top, Value::Nil);
                    self.clear_nil_tags_from(frame_top);
                    if self.stack[ra + 2].is_nil() {
                        self.frames[frame_idx].pc = pc + 1;
                    } else {
                        self.finish_cond_jump(frame_idx, pc, true);
                    }
                }
                OpCode::TForPrep => {
                    // Lua 5.0 backward compatibility for `for k, v in tbl do`.
                    if matches!(self.stack[ra], Value::Table(_)) {
                        self.write_stack(ra + 1, self.stack[ra]);
                        let next_fn = self
                            .heap
                            .table(self.globals)
                            .raw_get_str(self.heap.well_known.next_name);
                        self.write_stack(ra, next_fn);
                    }
                    let dest = (pc as isize + ins.sbx() as isize) as usize;
                    self.frames[frame_idx].pc = dest;
                }
                OpCode::SetList | OpCode::SetListO => {
                    let Value::Table(tid) = self.stack[ra] else {
                        return Err(ScriptError::runtime("SETLIST target is not a table"));
                    };
                    let bx = ins.bx();
                    let n = if op == OpCode::SetList {
                        (bx & (LFIELDS_PER_FLUSH - 1)) + 1
                    } else {
                        self.stack.len().saturating_sub(ra + 1)
                    };
                    let base_idx = bx & !(LFIELDS_PER_FLUSH - 1);
                    self.charge_work(n.units())?;
                    // Keep open results live on `self.stack` until copied into the table
                    // (both to prevent OOB reads and to keep them GC-rooted during table growth).
                    for j in 1..=n {
                        let val = self.stack[ra + j];
                        self.raw_set_vm(tid, Value::Number((base_idx + j) as f32), val)?;
                    }
                    if op == OpCode::SetListO {
                        let frame_top = self.frames[frame_idx].frame_top;
                        self.stack.resize(frame_top, Value::Nil);
                        self.clear_nil_tags_from(frame_top);
                    }
                }
                OpCode::Close => {
                    self.close_upvals_charged(ra)?;
                }
                OpCode::Closure => {
                    let child_proto = self
                        .heap
                        .loaded_proto(proto_id)
                        .children
                        .get(ins.bx())
                        .copied()
                        .ok_or_else(|| ScriptError::runtime("closure proto index out of range"))?;
                    let nup = self.heap.loaded_proto(child_proto).proto.nups as usize;
                    self.charge_work(nup.units() / 4)?;
                    let cur_cid = self.frames[frame_idx].closure;
                    let env = self.heap.closure(cur_cid).env;
                    // Up to `UpvalList::INLINE` captures are built in place: creating
                    // such a closure allocates nothing once the arena has a free slot.
                    let credit = UpvalList::block_bytes_for(nup);
                    if credit != 0 {
                        self.ensure_room(|_| credit)?;
                        self.heap.reserve(credit)?;
                    }
                    let captures = (|| {
                        let upvals = if nup <= UpvalList::INLINE {
                            let mut ids = [UpvalId(0); UpvalList::INLINE];
                            for (i, id) in ids.iter_mut().take(nup).enumerate() {
                                *id = self.capture_upval(proto_id, cur_cid, base, pc + i)?;
                            }
                            UpvalList::Inline {
                                len: nup as u8,
                                ids,
                            }
                        } else {
                            let mut ids = Vec::with_capacity(nup);
                            for i in 0..nup {
                                ids.push(self.capture_upval(proto_id, cur_cid, base, pc + i)?);
                            }
                            UpvalList::Heap(ids.into_boxed_slice())
                        };
                        // The executing frame reaches the child prototype, environment
                        // and captured cells while space for the closure is reclaimed.
                        self.ensure_room(|h| h.closure_cost(nup) - credit)?;
                        Ok::<_, ScriptError>(upvals)
                    })();
                    self.heap.unreserve(credit);
                    let new_cid = self.heap.alloc_closure(child_proto, env, captures?)?;
                    self.frames[frame_idx].pc = pc + nup;
                    self.write_stack(ra, Value::Closure(new_cid));
                    self.set_nil_tag(ra, None)?;
                }
            }
        }
    }

    #[inline]
    fn rk(&self, frame_idx: usize, base: usize, operand: usize) -> Value {
        if operand < MAXSTACK {
            self.stack[base + operand]
        } else {
            let pid = self.frames[frame_idx].proto;
            self.heap.loaded_proto(pid).constants[operand - MAXSTACK]
        }
    }

    #[inline]
    fn finish_cond_jump(&mut self, frame_idx: usize, pc: usize, take_jump: bool) {
        if take_jump {
            let pid = self.frames[frame_idx].proto;
            let jmp = self.heap.loaded_proto(pid).proto.code[pc];
            let dest = ((pc + 1) as isize + jmp.sbx() as isize) as usize;
            self.frames[frame_idx].pc = dest;
        } else {
            self.frames[frame_idx].pc = pc + 1;
        }
    }
}

#[cfg(test)]
mod gc_rooting_tests {
    use super::*;

    #[test]
    fn forced_gc_between_every_allocation_preserves_coerced_args_and_captures() {
        let mut vm = Vm::new_with_stdlib();
        let str_tbl = vm.get_global("string").unwrap().as_table().unwrap();
        let find_fn = vm.table_get_str(str_tbl, "find").unwrap();
        let find_ref = vm.create_ref(find_fn).unwrap();

        // Enable stress GC: collect on every single heap allocation!
        vm.stress_gc = true;

        // Both arguments are numbers coerced to strings; interning arg 2 runs GC
        // after arg 1 was coerced and written back onto the VM stack.
        let res = vm
            .call(
                &mut (),
                vm.get_ref(find_ref).unwrap(),
                &[Value::Number(123456.0), Value::Number(345.0)],
            )
            .unwrap();
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].as_number(), Some(3.0));
        assert_eq!(res[1].as_number(), Some(5.0));

        // Multiple captures where every capture interning triggers a full GC.
        let src = vm.intern_str("aaaa-bbbb-cccc-dddd").unwrap();
        let src_ref = vm.create_ref(Value::String(src)).unwrap();
        let pat = vm.intern_str("(%a+)%-(%a+)%-(%a+)%-(%a+)").unwrap();
        let pat_ref = vm.create_ref(Value::String(pat)).unwrap();
        let res = vm
            .call(
                &mut (),
                vm.get_ref(find_ref).unwrap(),
                &[vm.get_ref(src_ref).unwrap(), vm.get_ref(pat_ref).unwrap()],
            )
            .unwrap();
        assert_eq!(res.len(), 6);
        assert_eq!(vm.string_str(res[2].as_string().unwrap()), Some("aaaa"));
        assert_eq!(vm.string_str(res[3].as_string().unwrap()), Some("bbbb"));
        assert_eq!(vm.string_str(res[4].as_string().unwrap()), Some("cccc"));
        assert_eq!(vm.string_str(res[5].as_string().unwrap()), Some("dddd"));

        // Run the entire 11_gc_rooting fixture with GC forced on every single allocation!
        let path = format!(
            "{}/tests/lua/11_gc_rooting.luac",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytecode = std::fs::read(&path).expect("read 11_gc_rooting.luac");
        let expected_path = format!(
            "{}/tests/lua/11_gc_rooting.expected.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        let expected = std::fs::read_to_string(&expected_path).expect("read expected");
        vm.capture_output();
        vm.run_chunk(&mut (), &bytecode).unwrap();
        let actual = String::from_utf8(vm.take_output()).unwrap();
        assert_eq!(actual, expected);
    }
}

#[cfg(test)]
mod root_capacity_tests {
    use super::*;

    #[test]
    fn temporary_root_capacity_is_reserved_and_reused() {
        let mut vm = Vm::new();
        let roots = [Value::Nil; 2048];
        let before = vm.memory_bytes();
        vm.with_temp_roots(&roots, |vm| {
            assert!(vm.memory_bytes() >= before + roots.len() * VALUE_BYTES);
            Ok(())
        })
        .unwrap();
        let retained = vm.memory_bytes();
        vm.with_temp_roots(&roots, |_| Ok(())).unwrap();
        assert_eq!(vm.memory_bytes(), retained);
        assert!(vm.temp_roots.is_empty());
    }
}

#[cfg(test)]
mod collector_timing_tests {
    use super::*;
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "run independently on an idle processor"]
    fn calibrated_collector_step_time() {
        let mut vm = Vm::new();
        let t = vm.create_table().unwrap();
        vm.create_ref(Value::Table(t)).unwrap();
        for n in 1..=131_072 {
            vm.table_raw_set(t, Value::Number(n as f32), Value::Number(n as f32))
                .unwrap();
        }
        for n in 0..12_000 {
            vm.create_ref(Value::Number(n as f32)).unwrap();
        }
        let mut calibration = Vec::new();
        let samples = vec![Value::Number(17.); GC_MAX_STEP_WORK as usize];
        for _ in 0..64 {
            let start = Instant::now();
            for &v in black_box(&samples) {
                black_box(v.as_number());
            }
            calibration.push(start.elapsed());
        }
        calibration.sort_unstable();
        let reference = calibration[calibration.len() / 2];
        let mut times = Vec::new();
        for _ in 0..4 {
            vm.heap.gc_threshold = 0;
            let cycle = vm.collector_stats().cycles;
            while vm.collector_stats().cycles == cycle {
                vm.heap.owe_max_step();
                let start = Instant::now();
                vm.gc_step();
                times.push(start.elapsed());
            }
        }
        times.sort_unstable();
        let p95 = times[(times.len() - 1) * 95 / 100];
        let limit = (reference * 64).max(Duration::from_micros(500));
        println!("131072 entries, 12000 roots, {} steps, reference {reference:?}, p95 {p95:?}, max {:?}, limit {limit:?}", times.len(), times.last().unwrap());
        assert!(p95 <= limit, "p95 {p95:?}, calibrated limit {limit:?}");
        assert!(vm.collector_stats().max_step_units <= GC_MAX_STEP_WORK);
    }
}

#[cfg(test)]
mod output_capacity_tests {
    use super::*;

    #[test]
    fn output_growth_admits_both_live_buffers() {
        let mut vm = Vm::new();
        vm.capture_output();
        vm.write_output(&[3; 2048]).unwrap();
        vm.collect_garbage();
        vm.set_limits(VmLimits {
            memory_limit: vm.memory_bytes() + 2500,
            ..vm.limits()
        })
        .unwrap();
        assert!(matches!(
            vm.write_output(&[4; 1024]),
            Err(ScriptError::MemoryLimit { .. })
        ));
        assert_eq!(vm.output_bytes(), &[3; 2048]);
    }
}
