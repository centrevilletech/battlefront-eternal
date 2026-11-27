//! Runtime values, deterministic tables, and the garbage-collected heap.
//!
//! Table, upvalue, and value semantics are ported from Lua 5.0.3 (`src/lobject.h`,
//! `src/ltable.c`, `src/lfunc.c`, `src/lgc.c`, MIT License, Copyright (C) 1994-2006
//! Tecgraf, PUC-Rio), using safe arena handles and a deterministic insertion-ordered
//! map instead of pointer hashing so behavior is identical on x86-64 and ARM64.
//!
//! # Collector
//!
//! The heap is collected incrementally, a snapshot at the beginning: a cycle marks
//! the roots in slices, then traverses the heap in small steps between script
//! operations, then sweeps the arenas in small steps. While a cycle marks:
//!
//! - every object allocated is marked (it survives the cycle);
//! - every reference the script overwrites inside a heap object (a table slot, a
//!   metatable, a closed upvalue, a function environment, a native closure's upvalue)
//!   is marked first, so nothing reachable at the snapshot is lost;
//! - every value the host hands in (arguments, results of native functions, values it
//!   stores, temporary roots) is marked, since the host may hold handles the heap does
//!   not reach;
//! - ordinary cycles traverse weak tables strongly. Explicit and pressure collections clear weak entries.
//!
//! Every piece of collector work has a cost known before it starts, and a step does
//! no more than the work it was given: a large object is traversed in blocks of slots
//! with a per-object cursor, the sweep frees one slot at a time, and a dead object
//! whose destruction frees many blocks (a large table key index, a large prototype)
//! is set aside and destroyed a block at a time.
//!
//! The simulation grants collector work between dispatches. A failed memory
//! reservation can spend the balance of that tick before it retries. Userdata `__gc` metamethods are never
//! called: no stock mission script uses them and the library offers no `newproxy`.
//!
//! # Memory accounting
//!
//! Every allocation is charged before it is made, at the size the 64-bit build really
//! allocates: the arena slot of each object (slots are reused but never returned, so
//! their bytes stay charged), container capacity rather than length, allocator block
//! rounding, the string intern index and table key index nodes. The sizes are fixed
//! constants, not `size_of`, so the accounting (and with it collection timing and the
//! memory limit) is identical on every target; a test checks that they bound the real
//! layouts.

use bfe_wide::{Fnv, Wide};
use crate::units::Units;
use std::collections::hash_map::RandomState;
use std::collections::{btree_map, BTreeMap};
use std::hash::BuildHasher;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::chunk::{Constant, LocalVarInfo, Proto};
use crate::error::ScriptError;
use crate::numfmt;
use crate::vm::NativeKind;

static NEXT_VM_ID: AtomicU32 = AtomicU32::new(1);

pub(crate) fn alloc_vm_id() -> u32 {
    loop {
        let id = NEXT_VM_ID.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
}

#[inline]
pub(crate) fn next_generation(gen: u32) -> u32 {
    let next = gen.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

/// Handle to an interned Lua byte string inside a VM heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StringId {
    pub(crate) slot: u32,
    pub(crate) gen: u32,
    pub(crate) vm_id: u32,
}

/// Handle to a Lua table inside a VM heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableId {
    pub(crate) slot: u32,
    pub(crate) gen: u32,
    pub(crate) vm_id: u32,
}

/// Handle to a Lua bytecode closure inside a VM heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClosureId {
    pub(crate) slot: u32,
    pub(crate) gen: u32,
    pub(crate) vm_id: u32,
}

/// Handle to a registered native function (with optional captured upvalues).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeFnId {
    pub(crate) slot: u32,
    pub(crate) gen: u32,
    pub(crate) vm_id: u32,
}

/// Handle to a heap-allocated upvalue cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct UpvalId(pub(crate) u32);

/// Handle to host-opaque userdata with an optional metatable and deterministic tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UserdataId {
    pub(crate) slot: u32,
    pub(crate) gen: u32,
    pub(crate) vm_id: u32,
}

/// Pinned host reference to any Lua value, kept alive across GC cycles until released.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueRef {
    pub(crate) slot: u32,
    pub(crate) gen: u32,
    pub(crate) vm_id: u32,
}

/// Canonical quiet NaN bit pattern (`+qNaN`) used everywhere a NaN enters VM state,
/// eliminating x86-64 (`0xFFC0_0000`) vs ARM64 (`0x7FC0_0000`) sign-bit divergence.
pub const CANONICAL_NAN_BITS: u32 = 0x7FC0_0000;

/// Map any NaN bit pattern to [`CANONICAL_NAN_BITS`] and leave every other pattern
/// (including infinities and `-0.0`) unchanged.
///
/// The test and the selection are integer operations on the bits. A floating-point
/// form (`if x.is_nan() { NAN } else { x }`) is not enough: the optimizer treats all
/// NaNs as interchangeable and may drop that selection after inlining, which lets
/// the hardware's default NaN (negative on x86-64, positive on ARM64) through.
#[inline]
pub fn canonical_bits(bits: u32) -> u32 {
    let magnitude = bits & 0x7FFF_FFFF;
    // All ones when `magnitude > 0x7F80_0000` (a NaN), zero otherwise.
    let nan_mask = ((0x7F80_0000u32.wrapping_sub(magnitude) as i32) >> 31) as u32;
    (bits & !nan_mask) | (CANONICAL_NAN_BITS & nan_mask)
}

/// Canonicalize any NaN `f32` payload/sign to [`CANONICAL_NAN_BITS`].
#[inline]
pub fn canonicalize_number(n: f32) -> f32 {
    f32::from_bits(canonical_bits(n.to_bits()))
}

/// A Lua 5.0 runtime value. Small, `Copy`, and free of raw host pointers.
#[derive(Debug, Clone, Copy)]
pub enum Value {
    Nil,
    Bool(bool),
    Number(f32),
    String(StringId),
    Table(TableId),
    Closure(ClosureId),
    NativeFn(NativeFnId),
    Userdata(UserdataId),
}

impl Value {
    /// Return a `Value` with any NaN number canonicalized to [`CANONICAL_NAN_BITS`].
    #[inline]
    pub fn canonicalize(self) -> Self {
        match self {
            Value::Number(n) => Value::Number(canonicalize_number(n)),
            other => other,
        }
    }

    #[inline]
    pub fn is_nil(self) -> bool {
        matches!(self, Value::Nil)
    }

    /// Lua falsiness: only `nil` and `false` are false (`0` and `""` are true).
    #[inline]
    pub fn is_falsy(self) -> bool {
        matches!(self, Value::Nil | Value::Bool(false))
    }

    #[inline]
    pub fn is_truthy(self) -> bool {
        !self.is_falsy()
    }

    #[inline]
    pub fn is_function(self) -> bool {
        matches!(self, Value::Closure(_) | Value::NativeFn(_))
    }

    #[inline]
    pub fn as_number(self) -> Option<f32> {
        match self {
            Value::Number(n) => Some(n),
            _ => None,
        }
    }

    #[inline]
    pub fn as_bool(self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }

    #[inline]
    pub fn as_string(self) -> Option<StringId> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    #[inline]
    pub fn as_table(self) -> Option<TableId> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    /// Lua 5.0 `type(v)` name string.
    #[inline]
    pub fn type_name(self) -> &'static str {
        match self {
            Value::Nil => "nil",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Table(_) => "table",
            Value::Closure(_) | Value::NativeFn(_) => "function",
            Value::Userdata(_) => "userdata",
        }
    }

    /// Raw equality (`rawequal` / `luaO_rawequalObj`), ignoring metatables.
    #[inline]
    pub fn rawequal(self, other: Value) -> bool {
        match (self, other) {
            (Value::Nil, Value::Nil) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Table(a), Value::Table(b)) => a == b,
            (Value::Closure(a), Value::Closure(b)) => a == b,
            (Value::NativeFn(a), Value::NativeFn(b)) => a == b,
            (Value::Userdata(a), Value::Userdata(b)) => a == b,
            _ => false,
        }
    }
}

/// Tag method identifiers matching Lua 5.0.3 `TMS` (`src/ltm.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum TagMethod {
    Index = 0,
    NewIndex = 1,
    Gc = 2,
    Mode = 3,
    Eq = 4,
    Add = 5,
    Sub = 6,
    Mul = 7,
    Div = 8,
    Pow = 9,
    Unm = 10,
    Lt = 11,
    Le = 12,
    Concat = 13,
    Call = 14,
}

/// Pre-interned strings kept alive as permanent GC roots for zero-alloc lookups.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WellKnownStrings {
    pub tm: [StringId; 15],
    pub metatable: StringId,
    pub tostring_tm: StringId,
    pub fenv_tm: StringId,
    pub n_field: StringId,
    pub next_name: StringId,
    pub ipairs_name: StringId,
    pub tostring_name: StringId,
    pub empty: StringId,
}

impl WellKnownStrings {
    fn all(&self) -> impl Iterator<Item = StringId> + '_ {
        self.tm.iter().copied().chain([
            self.metatable,
            self.tostring_tm,
            self.fenv_tm,
            self.n_field,
            self.next_name,
            self.ipairs_name,
            self.tostring_name,
            self.empty,
        ])
    }
}

/// Handle to a GC-managed loaded function prototype inside a VM heap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LoadedProtoId(pub(crate) u32);

/// A `Proto` whose constant pool strings have already been interned in the VM heap.
#[derive(Debug, Clone)]
pub(crate) struct LoadedProto {
    pub proto: Rc<Proto>,
    pub constants: Box<[Value]>,
    pub children: Box<[LoadedProtoId]>,
    pub mem_bytes: usize,
}

/// Upvalue storage state (open on stack vs closed on heap).
#[derive(Debug, Clone, Copy)]
pub(crate) enum UpvalState {
    Open(usize),
    Closed(Value),
}

#[derive(Debug, Clone)]
pub(crate) struct GcUpval {
    pub state: UpvalState,
}

#[derive(Debug, Clone)]
pub(crate) struct LuaClosure {
    pub uid: Wide,
    pub proto: LoadedProtoId,
    pub env: TableId,
    pub upvalues: UpvalList,
}

/// Upvalue cells captured by a Lua closure: up to [`UpvalList::INLINE`] held in the
/// closure's own arena slot (creating such a closure allocates nothing once the arena
/// has a free slot), more in one separate block.
#[derive(Debug, Clone)]
pub(crate) enum UpvalList {
    Inline { len: u8, ids: [UpvalId; UpvalList::INLINE] },
    Heap(Box<[UpvalId]>),
}

impl UpvalList {
    pub(crate) const INLINE: usize = 5;

    #[inline]
    pub(crate) fn as_slice(&self) -> &[UpvalId] {
        match self {
            Self::Inline { len, ids } => &ids[..*len as usize],
            Self::Heap(b) => b,
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    pub(crate) fn get(&self, i: usize) -> Option<UpvalId> {
        self.as_slice().get(i).copied()
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, UpvalId> {
        self.as_slice().iter()
    }

    /// Bytes held outside the closure's slot (0 when inline).
    pub(crate) fn block_bytes(&self) -> usize {
        match self {
            Self::Inline { .. } => 0,
            Self::Heap(b) => block_bytes(b.len() * 4),
        }
    }

    /// The bytes a list of `len` cells holds outside the closure's slot.
    pub(crate) fn block_bytes_for(len: usize) -> usize {
        if len <= Self::INLINE {
            0
        } else {
            block_bytes(len * 4)
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GcString {
    pub bytes: Rc<[u8]>,
    /// Keyed hash of `bytes` for the intern index (never part of any result).
    hash: u32,
    prev: u32,
}

/// Host userdata value: stores a deterministic tag and opaque 64-bit payload.
#[derive(Debug, Clone)]
pub(crate) struct GcUserdata {
    pub uid: Wide,
    pub tag: u32,
    pub payload: Wide,
    pub metatable: Option<TableId>,
}

/// Captured values of a native closure: up to three inline, more in one block.
pub(crate) enum NativeUpvals {
    Inline { len: u8, vals: [Value; 3] },
    Heap(Box<[Value]>),
}

impl NativeUpvals {
    pub(crate) fn from_slice(src: &[Value]) -> Self {
        if src.len() <= 3 {
            let mut vals = [Value::Nil; 3];
            for (i, &v) in src.iter().enumerate() {
                vals[i] = v.canonicalize();
            }
            Self::Inline {
                len: src.len() as u8,
                vals,
            }
        } else {
            let boxed: Box<[Value]> = src.iter().map(|v| v.canonicalize()).collect();
            Self::Heap(boxed)
        }
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[Value] {
        match self {
            Self::Inline { len, vals } => &vals[..*len as usize],
            Self::Heap(b) => b,
        }
    }

    #[inline]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [Value] {
        match self {
            Self::Inline { len, vals } => &mut vals[..*len as usize],
            Self::Heap(b) => b,
        }
    }
}

/// A native function object: the Rust function and its captured values.
pub(crate) struct NativeRecord<H> {
    pub uid: Wide,
    pub kind: NativeKind<H>,
    pub upvalues: NativeUpvals,
    unpublished: bool,
}

// ---------------------------------------------------------------------------
// Memory model (64-bit layouts; see the module documentation)
// ---------------------------------------------------------------------------

/// Bytes an allocator block for an `n`-byte request occupies: the request plus an
/// 8-byte header, rounded up to 16, at least 32 (the glibc malloc chunk rule).
#[inline]
pub(crate) fn block_bytes(n: usize) -> usize {
    if n == 0 {
        0
    } else {
        (n.saturating_add(8 + 15) & !15).max(32)
    }
}

/// `size_of::<Value>()`.
pub(crate) const VALUE_BYTES: usize = 16;
/// `size_of::<MapEntry>()` (key and value).
const MAP_ENTRY_BYTES: usize = 32;
/// One `BTreeMap<TableKey, usize>` leaf node block, the least a non-empty index holds.
const INDEX_NODE_BYTES: usize = 288;
/// Worst-case index bytes per key: leaves at their minimum fill of five keys plus a
/// share of the internal nodes.
const INDEX_ENTRY_BYTES: usize = 72;
/// Reference counts in front of an `Rc` allocation.
pub(crate) const RC_HEADER_BYTES: usize = 16;
/// Estimate for the captured state of a host closure (`Rc<dyn Fn>`).
pub(crate) fn host_closure_bytes<T: ?Sized>(f: &T) -> usize {
    use std::alloc::Layout;
    Layout::new::<[usize; 2]>()
        .extend(Layout::for_value(f))
        .map_or(usize::MAX, |(layout, _)| {
            block_bytes(layout.pad_to_align().size())
        })
}

/// Arena slot sizes (`size_of::<Slot<T>>()`), charged once per slot ever created.
pub(crate) const STRING_SLOT_BYTES: usize = 40;
pub(crate) const TABLE_SLOT_BYTES: usize = 144;
pub(crate) const PROTO_SLOT_BYTES: usize = 64;
pub(crate) const CLOSURE_SLOT_BYTES: usize = 64;
pub(crate) const NATIVE_SLOT_BYTES: usize = 112;
pub(crate) const UPVAL_SLOT_BYTES: usize = 40;
pub(crate) const USERDATA_SLOT_BYTES: usize = 56;

/// Bytes a string of `len` bytes holds outside its slot: the `Rc<[u8]>` block (the
/// intern index is charged by its capacity, see [`InternIndex`]).
#[inline]
pub(crate) fn string_object_bytes(len: usize) -> usize {
    block_bytes(RC_HEADER_BYTES.saturating_add(len))
}

/// Bytes the decoded prototype and its loaded form hold outside their slot. The
/// decoder charges exactly this, part by part, before allocating each part.
pub(crate) fn loaded_proto_mem_bytes(proto: &Proto, charge_source: bool) -> usize {
    use crate::chunk::{
        CHILD_RC_BYTES, CONSTANT_BYTES, LOADED_CHILD_BYTES, LOADED_CONSTANT_BYTES,
        LOCAL_VAR_BYTES, NAME_BOX_BYTES, PROTO_RC_BYTES,
    };
    let mut n = block_bytes(PROTO_RC_BYTES);
    if charge_source {
        n += proto.source.as_ref().map_or(0, |s| block_bytes(RC_HEADER_BYTES + s.len()));
    }
    n += block_bytes(proto.code.len() * 4);
    n += block_bytes(proto.line_info.len() * 4);
    n += block_bytes(proto.local_vars.len() * LOCAL_VAR_BYTES);
    n += proto.local_vars.iter().map(|lv| block_bytes(lv.name.len())).sum::<usize>();
    n += block_bytes(proto.upvalue_names.len() * NAME_BOX_BYTES);
    n += proto.upvalue_names.iter().map(|u| block_bytes(u.len())).sum::<usize>();
    n += block_bytes(proto.constants.len() * CONSTANT_BYTES);
    n += proto
        .constants
        .iter()
        .map(|c| match c {
            crate::chunk::Constant::String(s) => block_bytes(s.len()),
            _ => 0,
        })
        .sum::<usize>();
    n += block_bytes(proto.protos.len() * CHILD_RC_BYTES);
    // The loaded form: interned constants and child ids.
    n += block_bytes(proto.constants.len() * LOADED_CONSTANT_BYTES);
    n += block_bytes(proto.protos.len() * LOADED_CHILD_BYTES);
    n
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/// The error a heap operation returns when its work would exceed the allowance it was
/// given; the VM replaces it with its own budget error (the operation did nothing).
pub(crate) fn work_refused() -> ScriptError {
    ScriptError::InstructionLimit { limit: 0 }
}

/// Maximum positive integer key stored in a table's contiguous 1-based array part.
const MAX_ARRAY_INDEX: usize = 1 << 20;

/// Canonicalized key for deterministic `BTreeMap` lookup in the non-array part of a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TableKey {
    Tombstone,
    Bool(bool),
    /// Canonical `f32` bits (`-0.0` normalized to `+0.0`; `NaN` rejected before insertion).
    Number(u32),
    String(StringId),
    Table(TableId),
    Closure(ClosureId),
    NativeFn(NativeFnId),
    Userdata(UserdataId),
}

impl TableKey {
    fn from_value(v: Value) -> Result<Self, ScriptError> {
        match v {
            Value::Nil => Err(ScriptError::runtime("table index is nil")),
            Value::Bool(b) => Ok(TableKey::Bool(b)),
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(ScriptError::runtime("table index is NaN"));
                }
                let normalized = if n == 0.0 { 0.0f32 } else { n };
                Ok(TableKey::Number(normalized.to_bits()))
            }
            Value::String(s) => Ok(TableKey::String(s)),
            Value::Table(t) => Ok(TableKey::Table(t)),
            Value::Closure(c) => Ok(TableKey::Closure(c)),
            Value::NativeFn(f) => Ok(TableKey::NativeFn(f)),
            Value::Userdata(u) => Ok(TableKey::Userdata(u)),
        }
    }

    fn to_value(self) -> Value {
        match self {
            TableKey::Tombstone => Value::Nil,
            TableKey::Bool(b) => Value::Bool(b),
            TableKey::Number(bits) => Value::Number(f32::from_bits(bits)),
            TableKey::String(s) => Value::String(s),
            TableKey::Table(t) => Value::Table(t),
            TableKey::Closure(c) => Value::Closure(c),
            TableKey::NativeFn(f) => Value::NativeFn(f),
            TableKey::Userdata(u) => Value::Userdata(u),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MapEntry {
    key: TableKey,
    value: Value,
}

/// Deterministic Lua table: 1-based contiguous array part + insertion-ordered map part.
///
/// `next` and `pairs` always traverse the array part in ascending integer order (`1..=len`)
/// followed by non-array entries in exact insertion order. Deleting an entry (`t[k] = nil`)
/// leaves a tombstone that keeps its key, so an in-progress `next(t, k)` traversal
/// continues cleanly; tombstones are dropped only when an insertion compacts the
/// table, never by the collector, so traversal never depends on collection.
#[derive(Debug, Clone)]
pub(crate) struct LuaTable {
    pub uid: Wide,
    array: Vec<Value>,
    entries: Vec<MapEntry>,
    index: BTreeMap<TableKey, usize>,
    tombstones: usize,
    pub metatable: Option<TableId>,
    pub explicit_n: Option<i32>,
    /// Bytes charged for this table's buffers (array, entries and index).
    mem_bytes: usize,
    /// Assigning `__mode` updates [`MODE_WEAK_KEYS`] and [`MODE_WEAK_VALUES`] from
    /// the bytes before a terminating NUL. Tables using this metatable inherit that mode.
    mode: u8,
    weak_next: u32,
}

/// [`LuaTable::mode`] flag: keys are weak (`__mode` contains `k`).
const MODE_WEAK_KEYS: u8 = 1;
/// [`LuaTable::mode`] flag: values are weak (`__mode` contains `v`).
const MODE_WEAK_VALUES: u8 = 2;

/// Weak-mode flags of a `__mode` value.
fn mode_flags(mode: &[u8]) -> u8 {
    let mode = numfmt::cstr_prefix(mode);
    let mut flags = 0;
    if mode.contains(&b'k') {
        flags |= MODE_WEAK_KEYS;
    }
    if mode.contains(&b'v') {
        flags |= MODE_WEAK_VALUES;
    }
    flags
}

/// Result of [`LuaTable::scan_slots`].
pub(crate) enum SlotScan {
    /// The next entry: its position, key and value.
    Found(usize, Value, Value),
    /// No entry within the scanned slots; continue after this position.
    Partial(usize),
    /// No entry after the start position.
    End,
}

#[inline]
fn index_bytes(len: usize) -> usize {
    if len == 0 {
        0
    } else {
        len.saturating_mul(INDEX_ENTRY_BYTES).max(INDEX_NODE_BYTES)
    }
}

#[inline]
fn table_mem_bytes_for(array_cap: usize, entries_cap: usize, index_len: usize) -> usize {
    block_bytes(array_cap.saturating_mul(VALUE_BYTES))
        .saturating_add(block_bytes(entries_cap.saturating_mul(MAP_ENTRY_BYTES)))
        .saturating_add(index_bytes(index_len))
}

/// Capacity a growing buffer moves to: double, at least `needed`, at least four.
#[inline]
fn grown_capacity(cap: usize, needed: usize) -> usize {
    cap.saturating_mul(2).max(needed).max(4)
}

#[inline]
fn as_array_index(v: Value) -> Option<usize> {
    if let Value::Number(n) = v {
        if (1.0..=(MAX_ARRAY_INDEX as f32)).contains(&n) {
            let i = n as usize;
            if (i as f32) == n {
                return Some(i);
            }
        }
    }
    None
}

impl LuaTable {
    fn new(uid: Wide) -> Self {
        Self {
            uid,
            array: Vec::new(),
            entries: Vec::new(),
            index: BTreeMap::new(),
            tombstones: 0,
            metatable: None,
            explicit_n: None,
            mem_bytes: 0,
            mode: 0,
            weak_next: NONE,
        }
    }

    fn compute_mem_bytes(&self) -> usize {
        table_mem_bytes_for(self.array.capacity(), self.entries.capacity(), self.index.len())
    }

    /// Number of value slots a traversal of this table visits (array and entries).
    pub fn slot_count(&self) -> usize {
        self.array.len() + self.entries.len()
    }

    pub fn raw_get(&self, key: Value) -> Value {
        if key.is_nil() {
            return Value::Nil;
        }
        if let Some(i) = as_array_index(key) {
            if i <= self.array.len() {
                return self.array[i - 1];
            }
        }
        let Ok(tkey) = TableKey::from_value(key) else {
            return Value::Nil;
        };
        match self.index.get(&tkey) {
            Some(&idx) => self.entries.get(idx).map_or(Value::Nil, |e| e.value),
            None => Value::Nil,
        }
    }

    pub fn raw_get_int(&self, i: i32) -> Value {
        if i >= 1 {
            let idx = i as usize;
            if idx <= self.array.len() {
                return self.array[idx - 1];
            }
        }
        self.raw_get(Value::Number(i as f32))
    }

    pub fn raw_get_str(&self, s: StringId) -> Value {
        match self.index.get(&TableKey::String(s)) {
            Some(&idx) => self.entries.get(idx).map_or(Value::Nil, |e| e.value),
            None => Value::Nil,
        }
    }

    /// Whether storing a new hash key now compacts the entries first.
    fn compacts_on_insert(&self) -> bool {
        self.tombstones > self.entries.len() / 2 && self.entries.len() >= 16
    }

    /// Transactional raw table assignment. Checks the projected memory growth against
    /// `(heap_bytes, mem_limit)` and the work against `work_left` BEFORE mutating the
    /// table, so a `MemoryLimit` error or a refusal ([`work_refused`]) leaves the table
    /// unchanged. Returns the change of the table's charged bytes and the work done
    /// beyond a plain store (slots filled, moved or reindexed, in units of eight slots,
    /// plus the key index insertion), which is at most `work_left`.
    pub fn raw_set_checked(
        &mut self,
        key: Value,
        val: Value,
        heap_bytes: usize,
        mem_limit: usize,
        work_left: u32,
    ) -> Result<(isize, u32), ScriptError> {
        let old_mem = self.mem_bytes;
        let tkey = TableKey::from_value(key)?;
        let fits = |projected: usize| -> Result<(), ScriptError> {
            let extra = projected.saturating_sub(old_mem);
            if extra > 0 && heap_bytes.saturating_add(extra) > mem_limit {
                Err(ScriptError::MemoryLimit { limit: mem_limit })
            } else {
                Ok(())
            }
        };
        let afford = |units: u32| -> Result<u32, ScriptError> {
            if units > work_left {
                Err(work_refused())
            } else {
                Ok(units)
            }
        };

        if let Some(i) = as_array_index(key) {
            if i <= self.array.len() {
                self.array[i - 1] = val;
                return Ok((0, 0));
            }
            let max_grow = self.array.len().saturating_mul(2).clamp(16, MAX_ARRAY_INDEX);
            if !val.is_nil() && i <= max_grow {
                let new_cap = if i > self.array.capacity() {
                    grown_capacity(self.array.capacity(), i).min(MAX_ARRAY_INDEX)
                } else {
                    self.array.capacity()
                };
                let old_array = if new_cap > self.array.capacity() {
                    block_bytes(self.array.capacity() * VALUE_BYTES)
                } else {
                    0
                };
                fits(
                    table_mem_bytes_for(new_cap, self.entries.capacity(), self.index.len())
                        .saturating_add(old_array),
                )?;
                let old_len = self.array.len();
                // Filling the new slots, copying the old ones if the buffer moves, and
                // looking up each new index among the map keys to migrate it.
                let mut work = (i - old_len).units();
                if new_cap > self.array.capacity() {
                    work += old_len.units();
                }
                if !self.index.is_empty() {
                    work += 4 * (i - old_len).units();
                }
                let units = afford(work / 8)?;
                if new_cap > self.array.capacity() {
                    self.array.reserve_exact(new_cap - old_len);
                }
                self.array.resize(i, Value::Nil);
                // Migrate any existing map entries that now fall inside `array`.
                if !self.index.is_empty() {
                    for j in (old_len + 1)..=i {
                        let jk = TableKey::Number((j as f32).to_bits());
                        if let Some(map_idx) = self.index.remove(&jk) {
                            let moved = self.entries[map_idx].value;
                            self.array[j - 1] = moved;
                            self.entries[map_idx] = MapEntry {
                                key: TableKey::Tombstone,
                                value: Value::Nil,
                            };
                            if !moved.is_nil() {
                                self.tombstones += 1;
                            }
                        }
                    }
                }
                self.array[i - 1] = val;
                self.mem_bytes = self.compute_mem_bytes();
                return Ok(((self.mem_bytes as isize) - (old_mem as isize), units));
            }
        }

        if val.is_nil() {
            if let Some(&idx) = self.index.get(&tkey) {
                if !self.entries[idx].value.is_nil() {
                    self.entries[idx].value = Value::Nil;
                    self.tombstones += 1;
                }
            }
            return Ok((0, 0));
        }

        let existing_idx = self.index.get(&tkey).copied();
        if let Some(idx) = existing_idx {
            if !self.entries[idx].value.is_nil() {
                // Updating an already-live key preserves its insertion position.
                self.entries[idx].value = val;
                return Ok((0, 0));
            }
        }
        // Either a brand-new key or reinsertion of a previously deleted (tombstoned)
        // key: always assign a fresh insertion position at the end of `entries` so
        // iteration order is independent of whether GC swept the deleted key.
        let is_reinsert = existing_idx.is_some();
        let will_compact = self.compacts_on_insert();
        let (live_after_compact, index_after) = if will_compact {
            let live = self.entries.len() - self.tombstones;
            (live, live.saturating_add(1))
        } else {
            (
                self.entries.len(),
                self.index.len() + usize::from(!is_reinsert),
            )
        };
        let new_cap = if live_after_compact + 1 > self.entries.capacity() {
            grown_capacity(self.entries.capacity(), live_after_compact + 1)
        } else {
            self.entries.capacity()
        };
        let old_entries = if new_cap > self.entries.capacity() {
            block_bytes(self.entries.capacity() * MAP_ENTRY_BYTES)
        } else {
            0
        };
        fits(
            table_mem_bytes_for(self.array.capacity(), new_cap, index_after)
                .saturating_add(old_entries),
        )?;

        // Compacting (retain plus a tree insertion per live entry to rebuild the index)
        // and copying the entries if their buffer moves; a key index insertion costs a
        // few units in a large table (cache misses).
        let mut work = 0u32;
        if will_compact {
            work += (self.entries.len() + 4 * live_after_compact).units();
        }
        if new_cap > self.entries.capacity() {
            work += 2 * live_after_compact.units();
        }
        let units = afford(3 + work / 8)?;
        if will_compact {
            self.compact();
        } else if let Some(old_idx) = existing_idx {
            // Keep old slot as an anonymous tombstone (`tombstones` count unchanged).
            self.entries[old_idx].key = TableKey::Tombstone;
        }
        if new_cap > self.entries.capacity() {
            self.entries.reserve_exact(new_cap - self.entries.len());
        }
        let idx = self.entries.len();
        self.entries.push(MapEntry {
            key: tkey,
            value: val,
        });
        self.index.insert(tkey, idx);
        self.mem_bytes = self.compute_mem_bytes();
        Ok(((self.mem_bytes as isize) - (old_mem as isize), units))
    }

    /// Drop tombstones in place (the entries buffer keeps its capacity).
    fn compact(&mut self) {
        self.index.clear();
        self.entries
            .retain(|e| !e.value.is_nil() && e.key != TableKey::Tombstone);
        for (idx, e) in self.entries.iter().enumerate() {
            self.index.insert(e.key, idx);
        }
        self.tombstones = 0;
    }

    /// The position after which `next(t, prev_key)` resumes. `None` starts traversal.
    /// Array keys `1..=len` come in index order, followed by other keys in insertion order.
    pub fn next_after(&self, prev_key: Value) -> Result<Option<usize>, ScriptError> {
        if prev_key.is_nil() {
            return Ok(None);
        }
        match as_array_index(prev_key) {
            Some(i) if i <= self.array.len() => Ok(Some(i - 1)),
            _ => {
                let tkey = TableKey::from_value(prev_key)
                    .map_err(|_| ScriptError::runtime("invalid key for `next'"))?;
                let Some(&idx) = self.index.get(&tkey) else {
                    return Err(ScriptError::runtime("invalid key for `next'"));
                };
                Ok(Some(self.array.len() + idx))
            }
        }
    }

    /// `next(t, prev_key)` for the host (unbounded scan).
    pub fn next(&self, prev_key: Value) -> Result<Option<(Value, Value)>, ScriptError> {
        let after = self.next_after(prev_key)?;
        Ok(self.next_slot(after).map(|(_, k, v)| (k, v)))
    }

    /// Look at no more than `max` slots after position `after` (`None`: from the
    /// beginning) for the next non-nil entry. Positions below the array length are
    /// array slots; the rest index the insertion-ordered entries.
    pub fn scan_slots(&self, after: Option<usize>, max: usize) -> SlotScan {
        let alen = self.array.len();
        let total = alen + self.entries.len();
        let start = after.map_or(0, |a| a + 1);
        let end = start.saturating_add(max).min(total);
        for p in start..end {
            if p < alen {
                let v = self.array[p];
                if !v.is_nil() {
                    return SlotScan::Found(p, Value::Number((p + 1) as f32), v);
                }
            } else {
                let e = self.entries[p - alen];
                if !e.value.is_nil() && e.key != TableKey::Tombstone {
                    return SlotScan::Found(p, e.key.to_value(), e.value);
                }
            }
        }
        if end >= total {
            SlotScan::End
        } else {
            SlotScan::Partial(end - 1)
        }
    }

    /// Allocation-free traversal: the next non-nil entry after position `after`
    /// (`None` starts at the beginning), with its position.
    pub fn next_slot(&self, after: Option<usize>) -> Option<(usize, Value, Value)> {
        match self.scan_slots(after, usize::MAX) {
            SlotScan::Found(p, k, v) => Some((p, k, v)),
            _ => None,
        }
    }

    /// Lua 5.0 `luaL_getn` using the table's `"n"` field, explicit size, or array scan.
    pub fn getn<H>(&self, heap: &GcHeap<H>) -> i32 {
        let mut unlimited = u32::MAX;
        self.getn_budgeted(heap, &mut unlimited, u32::MAX)
            .unwrap_or(0)
    }

    /// Budgeted `luaL_getn`: the array scan is charged one unit per eight elements.
    pub fn getn_budgeted<H>(
        &self,
        heap: &GcHeap<H>,
        budget: &mut u32,
        budget_limit: u32,
    ) -> Result<i32, ScriptError> {
        let n_val = self.raw_get_str(heap.well_known.n_field);
        if let Value::String(sid) = n_val {
            // A numeric string `n` is parsed: charge its length.
            let units = 1 + heap.string_bytes(sid).len().units() / 16;
            if *budget < units {
                *budget = 0;
                return Err(ScriptError::InstructionLimit {
                    limit: budget_limit,
                });
            }
            *budget -= units;
        }
        if let Some(n) = check_table_size_value(n_val, heap) {
            return Ok(n);
        }
        if let Some(n) = self.explicit_n {
            if n >= 0 {
                return Ok(n);
            }
        }
        let mut i: i32 = 1;
        while !self.raw_get_int(i).is_nil() {
            if i % 8 == 0 {
                if *budget == 0 {
                    return Err(ScriptError::InstructionLimit {
                        limit: budget_limit,
                    });
                }
                *budget -= 1;
            }
            if i == i32::MAX {
                return Ok(i);
            }
            i += 1;
        }
        Ok(i - 1)
    }

    /// Return all non-nil `(key, value)` pairs in deterministic iteration order.
    pub fn iter_pairs(&self) -> Vec<(Value, Value)> {
        let mut out = Vec::new();
        let mut pos = None;
        while let Some((p, k, v)) = self.next_slot(pos) {
            out.push((k, v));
            pos = Some(p);
        }
        out
    }
}

/// Lua 5.0.3 `lauxlib.c` `checkint` on a table's `n` field: `(int)` of the number (or
/// numeric string), used when it is not negative.
fn check_table_size_value<H>(v: Value, heap: &GcHeap<H>) -> Option<i32> {
    let num = match v {
        Value::Number(x) => x,
        Value::String(sid) => numfmt::parse_lua_number(heap.string_bytes(sid))?,
        _ => return None,
    };
    let n = numfmt::c_int(num);
    if n >= 0 { Some(n) } else { None }
}

// ---------------------------------------------------------------------------
// Arenas
// ---------------------------------------------------------------------------

/// No slot / end of an intrusive list.
const NONE: u32 = u32::MAX;

/// Bytes a buffer's growth needs (see [`Arena::insert_bytes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Growth {
    /// Free bytes needed beside the heap while the old and new buffers coexist.
    room: usize,
    /// Bytes the heap holds more afterwards.
    added: usize,
    /// Traffic from occupying a slot, including a reused slot.
    slot: usize,
}

/// One arena slot. `link` threads the free list while the slot is empty and the gray
/// list while its object waits to be traversed.
struct Slot<T> {
    gen: u32,
    mark: u32,
    link: u32,
    value: Option<T>,
}

struct Arena<T> {
    slots: Vec<Slot<T>>,
    free_head: u32,
    gray_head: u32,
}

impl<T> Arena<T> {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_head: NONE,
            gray_head: NONE,
        }
    }

    /// Buffer capacity once one more object is stored: unchanged while a slot is free or
    /// the buffer has room, else doubled (at least four slots).
    fn capacity_for_insert(&self) -> usize {
        let cap = self.slots.capacity();
        if self.free_head != NONE || self.slots.len() < cap {
            cap
        } else {
            grown_capacity(cap, self.slots.len() + 1)
        }
    }

    /// Bytes storing one more object needs, for slots of `slot_bytes`: what must be
    /// free beside the heap (the new buffer, while the old one still exists) and what
    /// the heap grows by once the old buffer is gone. Both zero when it fits.
    fn insert_bytes(&self, slot_bytes: usize) -> Growth {
        let cap = self.slots.capacity();
        let new_cap = self.capacity_for_insert();
        if new_cap == cap {
            Growth {
                room: 0,
                added: 0,
                slot: slot_bytes,
            }
        } else {
            let new_block = block_bytes(new_cap.saturating_mul(slot_bytes));
            Growth {
                room: new_block,
                slot: slot_bytes,
                added: new_block - block_bytes(cap * slot_bytes),
            }
        }
    }

    /// Store `value` (marked `mark`) in a free slot or a new one. Returns the slot, its
    /// generation and whether the arena grew by one slot.
    fn insert(&mut self, value: T, mark: u32) -> (u32, u32, bool) {
        let cap = self.capacity_for_insert();
        if cap > self.slots.capacity() {
            self.slots.reserve_exact(cap - self.slots.len());
        }
        if self.free_head != NONE {
            let slot = self.free_head;
            let s = &mut self.slots[slot as usize];
            self.free_head = s.link;
            s.link = NONE;
            s.mark = mark;
            s.value = Some(value);
            (slot, s.gen, false)
        } else {
            let slot = self.slots.len() as u32;
            self.slots.push(Slot {
                gen: 1,
                mark,
                link: NONE,
                value: Some(value),
            });
            (slot, 1, true)
        }
    }

    fn free(&mut self, slot: u32) -> Option<T> {
        let s = &mut self.slots[slot as usize];
        let value = s.value.take();
        s.gen = next_generation(s.gen);
        s.link = self.free_head;
        self.free_head = slot;
        value
    }

    #[inline]
    fn get(&self, slot: u32, gen: u32) -> Option<&T> {
        self.slots
            .get(slot as usize)
            .filter(|s| s.gen == gen)
            .and_then(|s| s.value.as_ref())
    }

    #[inline]
    fn get_mut(&mut self, slot: u32, gen: u32) -> Option<&mut T> {
        self.slots
            .get_mut(slot as usize)
            .filter(|s| s.gen == gen)
            .and_then(|s| s.value.as_mut())
    }

    #[inline]
    fn live(&self, slot: u32) -> Option<&T> {
        self.slots.get(slot as usize).and_then(|s| s.value.as_ref())
    }

    #[inline]
    fn live_mut(&mut self, slot: u32) -> Option<&mut T> {
        self.slots.get_mut(slot as usize).and_then(|s| s.value.as_mut())
    }

    #[inline]
    fn mark_of(&self, slot: u32) -> u32 {
        self.slots.get(slot as usize).map_or(0, |s| s.mark)
    }

    /// Mark slot `slot` with `epoch`; returns true if it was not marked yet.
    #[inline]
    fn set_mark(&mut self, slot: u32, epoch: u32) -> bool {
        match self.slots.get_mut(slot as usize) {
            Some(s) if s.value.is_some() && s.mark != epoch => {
                s.mark = epoch;
                true
            }
            _ => false,
        }
    }

    fn push_gray(&mut self, slot: u32) {
        let head = self.gray_head;
        if let Some(s) = self.slots.get_mut(slot as usize) {
            s.link = head;
            self.gray_head = slot;
        }
    }

    fn pop_gray(&mut self) -> Option<u32> {
        if self.gray_head == NONE {
            return None;
        }
        let slot = self.gray_head;
        let s = &mut self.slots[slot as usize];
        self.gray_head = s.link;
        s.link = NONE;
        Some(slot)
    }

    /// Empty the gray list at once. Links left in the slots are never read: pushing a
    /// slot or freeing it writes its link first.
    fn clear_gray(&mut self) {
        self.gray_head = NONE;
    }
}

// ---------------------------------------------------------------------------
// String intern index
// ---------------------------------------------------------------------------

/// Slot ranges 0..16, 16..32, 32..64, ... have their own hash buckets. Growing the
/// directory never moves an existing string to a different chain. A sweep unlinks
/// its known slot with two pointer updates regardless of collisions.
struct InternIndex {
    buckets: Vec<u32>,
}

impl InternIndex {
    fn new() -> Self {
        Self {
            buckets: Vec::new(),
        }
    }

    fn range(slot: u32) -> (usize, usize) {
        if slot < 16 {
            (0, 16)
        } else {
            let width = 1usize << slot.ilog2();
            (width, width)
        }
    }

    fn capacity_for(&self, slot: u32) -> usize {
        let (first, width) = Self::range(slot);
        self.buckets.len().max(first + width)
    }

    fn bytes_for(buckets: usize) -> usize {
        block_bytes(buckets.saturating_mul(4))
    }

    fn bucket(slot: u32, hash: u32) -> usize {
        let (first, width) = Self::range(slot);
        first + (hash as usize & (width - 1))
    }

    fn find(&self, strings: &Arena<GcString>, hash: u32, bytes: &[u8]) -> Option<u32> {
        let mut end = self.buckets.len();
        while end != 0 {
            let first = if end == 16 { 0 } else { end / 2 };
            let mut slot = self.buckets[first + (hash as usize & (end - first - 1))];
            while let Some(s) = strings.live(slot) {
                if s.hash == hash && &*s.bytes == bytes {
                    return Some(slot);
                }
                slot = strings.slots[slot as usize].link;
            }
            end = first;
        }
        None
    }

    fn insert(&mut self, strings: &mut Arena<GcString>, slot: u32, hash: u32) {
        let bucket = Self::bucket(slot, hash);
        let next = self.buckets[bucket];
        if let Some(s) = strings.live_mut(next) {
            s.prev = slot;
        }
        strings.slots[slot as usize].link = next;
        self.buckets[bucket] = slot;
    }

    fn remove(&mut self, strings: &mut Arena<GcString>, slot: u32) {
        let s = strings.live(slot).expect("interned string");
        let (hash, prev, next) = (s.hash, s.prev, strings.slots[slot as usize].link);
        if prev != NONE {
            strings.slots[prev as usize].link = next;
        } else {
            self.buckets[Self::bucket(slot, hash)] = next;
        }
        if let Some(n) = strings.live_mut(next) {
            n.prev = prev;
        }
    }
}

// ---------------------------------------------------------------------------
// Deferred destruction
// ---------------------------------------------------------------------------

/// Work units of destroying a dead object at once in the sweep, beyond visiting its
/// slot. Freeing a block costs about one unit.
const STRING_FREE_UNITS: u32 = 4;
const TABLE_FREE_UNITS: u32 = 6;
const PROTO_FREE_UNITS: u32 = 8;
const CLOSURE_FREE_UNITS: u32 = 4;
const NATIVE_FREE_UNITS: u32 = 4;
const SMALL_FREE_UNITS: u32 = 2;
/// Blocks a dead object's destruction may free at once in the sweep; a larger index
/// or prototype goes to the graveyard and is destroyed a block at a time.
const GRAVE_MIN: usize = 64;
const GRAVE_BYTES: usize = 96;

/// Part of a dead object that the sweep set aside, destroyed one element (about one
/// freed block) per work unit.
enum Grave {
    /// A table's key index (dropping a B-tree frees each node).
    Index(btree_map::IntoIter<TableKey, usize>),
    /// A prototype's debug local records (each name is a block).
    Locals(std::vec::IntoIter<LocalVarInfo>),
    /// A prototype's upvalue names.
    UpvalueNames(std::vec::IntoIter<Box<[u8]>>),
    /// A prototype's constants (each string is a block).
    Constants(std::vec::IntoIter<Constant>),
    /// A prototype's nested prototypes (each may be the last reference to one).
    Children(std::vec::IntoIter<Rc<Proto>>),
    /// Bytes to release once everything above this entry is destroyed.
    Release(usize),
}

impl Grave {
    /// Elements left (each costs one unit).
    fn len(&self) -> usize {
        match self {
            Grave::Index(it) => it.len(),
            Grave::Locals(it) => it.len(),
            Grave::UpvalueNames(it) => it.len(),
            Grave::Constants(it) => it.len(),
            Grave::Children(it) => it.len(),
            Grave::Release(_) => 0,
        }
    }
}

/// Blocks destroying `proto` frees, if this is its last reference: its debug records,
/// names, constants and children (the last reference to a child is destroyed with it).
fn proto_parts(proto: &Proto) -> usize {
    proto.local_vars.len() + proto.upvalue_names.len() + proto.constants.len() + proto.protos.len()
}

/// Destruction visits the remaining child tree when its last owner releases it.
fn digest_drop_tree(h: &mut Digest, proto: &Rc<Proto>) {
    h.count(Rc::strong_count(proto));
    h.count(proto.local_vars.len());
    h.count(proto.constants.len());
    h.count(proto.upvalue_names.len());
    h.count(proto.protos.len());
    for child in &proto.protos {
        digest_drop_tree(h, child);
    }
}

/// Whether dropping this last reference to `proto` frees at most [`GRAVE_MIN`] blocks
/// and no child prototype with it.
fn proto_drops_small(proto: &Rc<Proto>) -> bool {
    Rc::strong_count(proto) > 1
        || (proto_parts(proto) <= GRAVE_MIN
            && proto.protos.iter().all(|c| Rc::strong_count(c) > 1))
}

// ---------------------------------------------------------------------------
// Heap
// ---------------------------------------------------------------------------

/// Collector phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GcPhase {
    /// No cycle in progress; one starts when the live bytes reach the threshold.
    Pause,
    /// Traversing from the snapshot of the roots.
    Mark,
    /// Freeing what the finished mark did not reach.
    Sweep,
    /// Restore a single color after abandoning a full mark. Every object survives.
    Recolor,
}

/// Bytes allocated between two collector steps.
const GC_STEP_BYTES: usize = 4096;
/// Work units of the fixed part of traversing one object (marking its metatable or
/// environment and prototype, reading its weak mode).
const OBJECT_HEADER_UNITS: u32 = 4;
/// Work units of starting a cycle beyond its roots (marking the well-known strings).
const CYCLE_START_UNITS: u32 = 32;
/// Collector work done per byte allocated, in eighths (work units: one per slot or
/// reference examined).
const GC_WORK_PER_BYTE_EIGHTHS: usize = 4;
/// A cycle starts when the live object bytes reach this many percent of those left
/// by the previous cycle.
const GC_PAUSE_PERCENT: usize = 200;
/// The smallest threshold.
const GC_MIN_THRESHOLD: usize = 64 * 1024;
/// Arena order of the sweep.
const SWEEP_ARENAS: u8 = 7;

/// Between dispatches the VM samples allocation traffic. A slot counts even when
/// it comes from an existing arena block. The memory limit itself uses the ledger.
pub(crate) struct CollectorTick {
    pub number: u32,
    pub left: u32,
    pub allocated: usize,
    pub since_start: usize,
    pub pressure: bool,
    // Map entries move toward lower positions on compaction and into the array.
    // Clean the map backwards, then the array backwards, across mutations.
    weak_cursor: Option<(bool, usize)>,
}

impl CollectorTick {
    pub fn new(number: u32, occupied: usize) -> Self {
        Self {
            number,
            left: 0,
            allocated: 0,
            since_start: occupied,
            pressure: false,
            weak_cursor: None,
        }
    }

    pub fn digest(&self, h: &mut Digest) {
        h.small(self.number);
        h.small(self.left);
        h.count(self.allocated);
        h.count(self.since_start);
        h.bytes(&[u8::from(self.pressure)]);
        if let Some((array, pos)) = self.weak_cursor {
            h.bytes(&[1 + u8::from(array)]);
            h.count(pos);
        } else {
            h.bytes(&[0]);
        }
    }
}

/// Arena-backed garbage-collected heap for a single Lua VM.
pub(crate) struct GcHeap<H> {
    pub tick: Option<CollectorTick>,
    pub vm_id: u32,

    strings: Arena<GcString>,
    /// String slots by content. Only ever looked up, never iterated, so its hashing
    /// (randomly keyed, which keeps crafted strings from colliding) cannot change
    /// any result.
    intern: InternIndex,
    hash_keys: RandomState,
    tables: Arena<LuaTable>,
    protos: Arena<LoadedProto>,
    closures: Arena<LuaClosure>,
    natives: Arena<NativeRecord<H>>,
    upvals: Arena<GcUpval>,
    userdata: Arena<GcUserdata>,

    next_uid: Wide,
    /// All charged bytes: slots, objects and reservations.
    pub memory_bytes: usize,
    /// The part of `memory_bytes` that collecting objects can free.
    object_bytes: usize,
    pub memory_limit: usize,

    phase: GcPhase,
    epoch: u32,
    /// The current mark treats weak tables as weak (full collections only).
    full: bool,
    /// Root writes shade their value during a root scan and throughout a weak mark.
    pub trace_stack: bool,
    /// Weak tables the current full mark traversed, with the weak mode it used.
    weak_head: u32,
    weak_work: u32,
    weak_pos: usize,
    /// The gray object being traversed, if a step stopped inside it.
    cursor: Option<Cursor>,
    sweep_arena: u8,
    sweep_pos: u32,
    /// Parts of dead objects waiting to be destroyed (a stack).
    graveyard: Vec<Grave>,
    grave_work: u32,
    debt: usize,
    /// Live object bytes at which the next cycle starts (Lua's `GCthreshold`).
    pub gc_threshold: usize,
    pub stats: CollectorStats,
    pub well_known: WellKnownStrings,
}

/// Where the traversal of a gray object stopped. A table's map entries are visited
/// from the last down, then its array slots from the last down: an assignment only
/// ever moves an entry toward a lower position (compaction, migration into the array
/// part), never from a position not yet visited to one already visited, and it marks
/// any value it overwrites, so a cursor stays valid across script operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cursor {
    TableEntries {
        slot: u32,
        pos: usize,
        weak_k: bool,
        weak_v: bool,
    },
    TableArray {
        slot: u32,
        pos: usize,
    },
    Closure {
        slot: u32,
        pos: usize,
    },
    Proto {
        slot: u32,
        pos: usize,
    },
    Native {
        slot: u32,
        pos: usize,
    },
}

/// Counters of the collector's work since the VM was created
/// ([`crate::Vm::collector_stats`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollectorStats {
    /// Native handles discarded before the host first stored, pinned or called them.
    pub discarded_host_natives: u32,
    /// Incremental steps that did work.
    pub steps: u32,
    /// Steps that did less than their share, or nothing, because the running call's
    /// collector allowance was used up; their work waits for a later step.
    pub deferred_steps: u32,
    /// The most work units one incremental step did, a cycle's root scan included.
    pub max_step_units: u32,
    /// Cycles completed, incremental and full.
    pub cycles: u32,
    /// Full collections run (on request or for memory).
    pub full_collections: u32,
    /// Full collections run because an allocation would otherwise exceed the memory
    /// limit.
    pub forced_collections: u32,
    /// Requests for a full collection that waited for an allowance covering it.
    pub deferred_requests: u32,
    /// Collector work units in total.
    pub total_units: u32,
}

impl<H> GcHeap<H> {
    pub fn try_new(vm_id: u32, memory_limit: usize) -> Result<Self, ScriptError> {
        let placeholder = StringId {
            slot: 0,
            gen: 1,
            vm_id,
        };
        let mut heap = Self {
            tick: None,
            vm_id,
            strings: Arena::new(),
            intern: InternIndex::new(),
            hash_keys: RandomState::new(),
            tables: Arena::new(),
            protos: Arena::new(),
            closures: Arena::new(),
            natives: Arena::new(),
            upvals: Arena::new(),
            userdata: Arena::new(),
            next_uid: Wide::from_u32(1),
            memory_bytes: 0,
            object_bytes: 0,
            memory_limit,
            phase: GcPhase::Pause,
            epoch: 1,
            full: false,
            trace_stack: false,
            weak_head: NONE,
            weak_work: 0,
            weak_pos: 0,
            cursor: None,
            sweep_arena: 0,
            sweep_pos: 0,
            graveyard: Vec::new(),
            grave_work: 0,
            debt: 0,
            gc_threshold: GC_MIN_THRESHOLD,
            stats: CollectorStats::default(),
            well_known: WellKnownStrings {
                tm: [placeholder; 15],
                metatable: placeholder,
                tostring_tm: placeholder,
                fenv_tm: placeholder,
                n_field: placeholder,
                next_name: placeholder,
                ipairs_name: placeholder,
                tostring_name: placeholder,
                empty: placeholder,
            },
        };

        let tm_names: [&[u8]; 15] = [
            b"__index",
            b"__newindex",
            b"__gc",
            b"__mode",
            b"__eq",
            b"__add",
            b"__sub",
            b"__mul",
            b"__div",
            b"__pow",
            b"__unm",
            b"__lt",
            b"__le",
            b"__concat",
            b"__call",
        ];
        let init_err = |_| ScriptError::InvalidConfig("memory_limit too small for initial VM state");
        let mut tm = [placeholder; 15];
        for (i, name) in tm_names.iter().enumerate() {
            tm[i] = heap.intern_bytes(name).map_err(init_err)?;
        }
        heap.well_known = WellKnownStrings {
            tm,
            metatable: heap.intern_bytes(b"__metatable").map_err(init_err)?,
            tostring_tm: heap.intern_bytes(b"__tostring").map_err(init_err)?,
            fenv_tm: heap.intern_bytes(b"__fenv").map_err(init_err)?,
            n_field: heap.intern_bytes(b"n").map_err(init_err)?,
            next_name: heap.intern_bytes(b"next").map_err(init_err)?,
            ipairs_name: heap.intern_bytes(b"ipairs").map_err(init_err)?,
            tostring_name: heap.intern_bytes(b"tostring").map_err(init_err)?,
            empty: heap.intern_bytes(b"").map_err(init_err)?,
        };
        Ok(heap)
    }

    pub fn alloc_uid(&mut self) -> Wide {
        let id = self.next_uid;
        self.next_uid = self.next_uid.wrapping_add(Wide::from_u32(1));
        id
    }

    #[inline]
    pub fn phase(&self) -> GcPhase {
        self.phase
    }

    #[inline]
    pub fn check_budget(&self, extra: usize) -> Result<(), ScriptError> {
        if extra > self.memory_limit.saturating_sub(self.memory_bytes) {
            Err(ScriptError::MemoryLimit {
                limit: self.memory_limit,
            })
        } else {
            Ok(())
        }
    }

    /// Charge `bytes` that are not an object (reservations such as the VM stack or
    /// buffered output). Fails without charging if the limit would be exceeded.
    pub fn reserve(&mut self, bytes: usize) -> Result<(), ScriptError> {
        self.check_budget(bytes)?;
        self.add_bytes(bytes);
        Ok(())
    }

    #[inline]
    fn note_allocation(&mut self, bytes: usize) {
        if let Some(tick) = &mut self.tick {
            tick.allocated = tick.allocated.saturating_add(bytes);
            tick.since_start = tick.since_start.saturating_add(bytes);
        }
    }

    pub fn add_bytes(&mut self, bytes: usize) {
        self.memory_bytes += bytes;
        self.note_allocation(bytes);
    }

    /// Release bytes charged by [`GcHeap::reserve`].
    pub fn unreserve(&mut self, bytes: usize) {
        self.memory_bytes = self.memory_bytes.saturating_sub(bytes);
    }

    /// Charge a new object of `object` bytes and its arena's growth, if any, checking
    /// the room both need first.
    fn charge_object(&mut self, object: usize, growth: Growth) -> Result<(), ScriptError> {
        self.check_budget(object.saturating_add(growth.room))?;
        let held = object + growth.added;
        self.memory_bytes += held;
        self.note_allocation(held.saturating_add(growth.slot));
        self.object_bytes += object;
        self.add_debt(object + growth.added);
        Ok(())
    }

    /// Add allocated bytes to the collector's debt. The debt is capped at the memory
    /// limit: the work it asks for is then at least a whole cycle over a full heap.
    #[inline]
    #[cfg(test)]
    pub fn owe_max_step(&mut self) {
        self.add_debt(crate::vm::GC_MAX_STEP_WORK as usize * 8);
    }

    pub fn weak_mark(&self) -> bool {
        self.full && self.phase == GcPhase::Mark
    }

    pub fn full_cycle(&self) -> bool {
        self.full
    }

    fn add_debt(&mut self, bytes: usize) {
        self.debt = self.debt.saturating_add(bytes).min(self.memory_limit);
    }

    fn release_object(&mut self, object: usize) {
        self.memory_bytes = self.memory_bytes.saturating_sub(object);
        self.object_bytes = self.object_bytes.saturating_sub(object);
    }

    /// Apply a change of an object's charged bytes that was already checked.
    fn apply_object_delta(&mut self, delta: isize) {
        if delta > 0 {
            let d = delta as usize;
            self.add_bytes(d);
            self.object_bytes = self.object_bytes.saturating_add(d);
            self.add_debt(d);
        } else if delta < 0 {
            self.release_object(delta.unsigned_abs());
        }
    }

    // -- strings -------------------------------------------------------------

    /// Bytes interning a new string of `len` bytes may add: its object, a new slot and
    /// the intern index's growth (the old buckets are freed after the new ones exist).
    pub fn new_string_bytes(&self, len: usize) -> usize {
        let slot = self.strings.insert_bytes(STRING_SLOT_BYTES).room;
        let next = if self.strings.free_head == NONE {
            self.strings.slots.len() as u32
        } else {
            self.strings.free_head
        };
        let buckets = self.intern.capacity_for(next);
        let payload = string_object_bytes(len).saturating_add(slot);
        if buckets == self.intern.buckets.len() {
            return payload;
        }
        let new = InternIndex::bytes_for(buckets);
        let old = InternIndex::bytes_for(self.intern.buckets.len());
        new.max(payload.saturating_add(new - old))
    }

    pub fn intern_bytes(&mut self, bytes: &[u8]) -> Result<StringId, ScriptError> {
        self.intern_with_budget(bytes, u32::MAX).map(|(id, _)| id)
    }

    pub fn intern_with_budget(
        &mut self,
        bytes: &[u8],
        work_left: u32,
    ) -> Result<(StringId, bool), ScriptError> {
        let hash = self.hash_keys.hash_one(bytes) as u32;
        if let Some(slot) = self.intern.find(&self.strings, hash, bytes) {
            // An unreachable string found again is reachable once more: mark it
            // during a cycle so neither the mark nor the sweep loses it (a string has
            // no references, so marking alone is enough).
            if self.phase != GcPhase::Pause {
                self.strings.set_mark(slot, self.epoch);
            }
            let gen = self.strings.slots[slot as usize].gen;
            return Ok((StringId {
                slot,
                gen,
                vm_id: self.vm_id,
            }, false));
        }
        if work_left < 2 {
            return Err(work_refused());
        }
        self.check_budget(self.new_string_bytes(bytes.len()))?;
        let old_buckets = self.intern.buckets.len();
        let next = if self.strings.free_head == NONE {
            self.strings.slots.len() as u32
        } else {
            self.strings.free_head
        };
        let buckets = self.intern.capacity_for(next);
        if buckets != old_buckets {
            self.add_bytes(InternIndex::bytes_for(buckets));
            self.intern.buckets.reserve_exact(buckets - old_buckets);
            self.intern.buckets.resize(buckets, NONE);
            self.unreserve(InternIndex::bytes_for(old_buckets));
        }
        let object = string_object_bytes(bytes.len());
        self.charge_object(object, self.strings.insert_bytes(STRING_SLOT_BYTES))?;
        let (slot, gen, _) = self.strings.insert(
            GcString {
                bytes: Rc::from(bytes),
                hash,
                prev: NONE,
            },
            self.epoch,
        );
        self.intern.insert(&mut self.strings, slot, hash);
        Ok((StringId {
            slot,
            gen,
            vm_id: self.vm_id,
        }, true))
    }

    /// Whether an object with mark `mark` is already dead: during a sweep, everything
    /// the finished mark did not reach is, even before its slot is freed. Every getter
    /// of a handle from outside the VM checks this, like [`GcHeap::is_live_value`].
    #[inline]
    fn condemned(&self, mark: u32) -> bool {
        self.phase == GcPhase::Sweep && mark != self.epoch
    }

    #[inline]
    pub fn get_string_bytes(&self, id: StringId) -> Option<&[u8]> {
        if id.vm_id != self.vm_id || self.condemned(self.strings.mark_of(id.slot)) {
            return None;
        }
        self.strings.get(id.slot, id.gen).map(|s| s.bytes.as_ref())
    }

    pub fn string_bytes(&self, id: StringId) -> &[u8] {
        self.strings
            .live(id.slot)
            .map_or(&[][..], |s| s.bytes.as_ref())
    }

    /// Cheap O(1) refcount clone of the immutable interned byte buffer (zero heap allocation).
    #[inline]
    pub fn string_rc(&self, id: StringId) -> Rc<[u8]> {
        match self.strings.live(id.slot) {
            Some(s) => Rc::clone(&s.bytes),
            None => Rc::from(&[][..]),
        }
    }

    // -- tables --------------------------------------------------------------

    pub fn alloc_table(&mut self) -> Result<TableId, ScriptError> {
        self.charge_object(0, self.tables.insert_bytes(TABLE_SLOT_BYTES))?;
        let uid = self.alloc_uid();
        let (slot, gen, _) = self.tables.insert(LuaTable::new(uid), self.epoch);
        Ok(TableId {
            slot,
            gen,
            vm_id: self.vm_id,
        })
    }

    #[inline]
    pub fn get_table(&self, id: TableId) -> Option<&LuaTable> {
        if id.vm_id != self.vm_id || self.condemned(self.tables.mark_of(id.slot)) {
            return None;
        }
        self.tables.get(id.slot, id.gen)
    }

    /// Table of a handle the VM holds (reachable, hence live).
    pub fn table(&self, id: TableId) -> &LuaTable {
        self.tables
            .live(id.slot)
            .unwrap_or_else(|| unreachable_object("table"))
    }

    pub fn table_mut(&mut self, id: TableId) -> &mut LuaTable {
        self.tables
            .live_mut(id.slot)
            .unwrap_or_else(|| unreachable_object("table"))
    }

    /// Raw assignment with the collector's deletion barrier. Returns the work done
    /// beyond a plain store, in budget units; refuses ([`work_refused`]) without
    /// changing anything if that work would exceed `work_left`.
    ///
    /// Only an overwritten value needs the barrier: compaction drops nothing but the
    /// keys of deleted entries, which no script can read again (`next` skips them), so
    /// they cannot move to a place the mark has already visited.
    pub fn table_raw_set(
        &mut self,
        id: TableId,
        key: Value,
        val: Value,
        work_left: u32,
    ) -> Result<u32, ScriptError> {
        let mode_key = self.well_known.tm[TagMethod::Mode as usize];
        let is_mode = matches!(key, Value::String(k) if k == mode_key);
        // Reading the new weak mode scans the string up to its first NUL.
        let mode_units = match val {
            Value::String(s) if is_mode => {
                self.string_bytes(s).len().units() / crate::vm::BYTES_PER_UNIT
            }
            _ => 0,
        };
        if mode_units > work_left {
            return Err(work_refused());
        }
        if self.phase == GcPhase::Mark {
            let old = self.table(id).raw_get(key);
            self.barrier(old);
        }
        if self.graveyard.capacity() < 2 && self.table(id).index.len() >= GRAVE_MIN {
            self.reserve_graves(2)?;
        }
        self.barrier(key);
        self.barrier(val);
        let cur_bytes = self.memory_bytes;
        let limit = self.memory_limit;
        let (delta, work) = self.table_mut(id).raw_set_checked(
            key,
            val,
            cur_bytes,
            limit,
            work_left - mode_units,
        )?;
        self.apply_object_delta(delta);
        if is_mode {
            let flags = match val {
                Value::String(s) => mode_flags(self.string_bytes(s)),
                _ => 0,
            };
            if self.table(id).mode & !flags != 0 {
                self.abandon_full_mark();
            }
            self.table_mut(id).mode = flags;
        }
        Ok(work + mode_units)
    }

    pub fn table_setn(&mut self, id: TableId, n: i32, work_left: u32) -> Result<u32, ScriptError> {
        let n_field = self.well_known.n_field;
        let n_val = self.table(id).raw_get_str(n_field);
        // A string `n` is parsed as a number: charged first.
        let parse_units = match n_val {
            Value::String(s) => self.string_bytes(s).len().units() / crate::vm::BYTES_PER_UNIT,
            _ => 0,
        };
        if parse_units > work_left {
            return Err(work_refused());
        }
        let has_n = check_table_size_value(n_val, self).is_some();
        if has_n {
            let set = self.table_raw_set(
                id,
                Value::String(n_field),
                Value::Number(n as f32),
                work_left - parse_units,
            )?;
            Ok(set + parse_units)
        } else {
            self.table_mut(id).explicit_n = Some(n);
            Ok(parse_units)
        }
    }

    /// Replace a table's metatable (deletion barrier on the old one).
    pub fn set_table_metatable(&mut self, id: TableId, mt: Option<TableId>) {
        let old = self.table(id).metatable;
        if let Some(old) = old {
            self.barrier(Value::Table(old));
        }
        if let Some(new) = mt {
            self.barrier(Value::Table(new));
        }
        if self.weak_mark() {
            let (old_keys, old_values) = self.weak_flags(old);
            let (keys, values) = self.weak_flags(mt);
            if (old_keys && !keys) || (old_values && !values) {
                self.abandon_full_mark();
            }
        }
        self.table_mut(id).metatable = mt;
    }

    // -- prototypes ----------------------------------------------------------

    /// Bytes [`GcHeap::alloc_loaded_proto`] charges for a prototype of `mem` bytes.
    pub fn proto_cost(&self, mem: usize) -> usize {
        mem + self.protos.insert_bytes(PROTO_SLOT_BYTES).room
    }

    pub fn alloc_loaded_proto(
        &mut self,
        proto: Rc<Proto>,
        constants: Box<[Value]>,
        children: Box<[LoadedProtoId]>,
        charge_source: bool,
    ) -> Result<LoadedProtoId, ScriptError> {
        let mem_bytes = loaded_proto_mem_bytes(&proto, charge_source);
        self.charge_object(mem_bytes, self.protos.insert_bytes(PROTO_SLOT_BYTES))?;
        let (slot, _, _) = self.protos.insert(
            LoadedProto {
                proto,
                constants,
                children,
                mem_bytes,
            },
            self.epoch,
        );
        Ok(LoadedProtoId(slot))
    }

    pub(crate) fn proto_key(&self, id: LoadedProtoId) -> (u32, u32) {
        (id.0, self.protos.slots[id.0 as usize].gen)
    }

    #[inline]
    pub fn loaded_proto(&self, id: LoadedProtoId) -> &LoadedProto {
        self.protos
            .live(id.0)
            .unwrap_or_else(|| unreachable_object("prototype"))
    }

    // -- closures ------------------------------------------------------------

    /// Bytes [`GcHeap::alloc_closure`] charges for a closure with `n_up` upvalues.
    pub fn closure_cost(&self, n_up: usize) -> usize {
        UpvalList::block_bytes_for(n_up) + self.closures.insert_bytes(CLOSURE_SLOT_BYTES).room
    }

    pub fn alloc_closure(
        &mut self,
        proto: LoadedProtoId,
        env: TableId,
        upvalues: UpvalList,
    ) -> Result<ClosureId, ScriptError> {
        let object = upvalues.block_bytes();
        self.charge_object(object, self.closures.insert_bytes(CLOSURE_SLOT_BYTES))?;
        let uid = self.alloc_uid();
        let (slot, gen, _) = self.closures.insert(
            LuaClosure {
                uid,
                proto,
                env,
                upvalues,
            },
            self.epoch,
        );
        Ok(ClosureId {
            slot,
            gen,
            vm_id: self.vm_id,
        })
    }

    pub fn closure(&self, id: ClosureId) -> &LuaClosure {
        self.closures
            .live(id.slot)
            .unwrap_or_else(|| unreachable_object("closure"))
    }

    /// Replace a closure's environment (deletion barrier on the old one).
    pub fn set_closure_env(&mut self, id: ClosureId, env: TableId) {
        let old = self.closure(id).env;
        self.barrier(Value::Table(old));
        if let Some(cl) = self.closures.live_mut(id.slot) {
            cl.env = env;
        }
    }

    // -- native closures -----------------------------------------------------

    /// Bytes of a native function object (outside its slot).
    fn native_object_bytes(kind: &NativeKind<H>, n_up: usize) -> usize {
        let mut object = if n_up > 3 {
            block_bytes(n_up * VALUE_BYTES)
        } else {
            0
        };
        if let NativeKind::Closure(f) = kind {
            object += host_closure_bytes(f.as_ref());
        }
        object
    }

    pub fn native_closure_cost(&self, payload: usize) -> usize {
        payload.saturating_add(self.natives.insert_bytes(NATIVE_SLOT_BYTES).room)
    }

    /// Bytes [`GcHeap::alloc_native`] charges.
    pub fn native_cost(&self, kind: &NativeKind<H>, n_up: usize) -> usize {
        Self::native_object_bytes(kind, n_up) + self.natives.insert_bytes(NATIVE_SLOT_BYTES).room
    }

    pub fn alloc_native(
        &mut self,
        kind: NativeKind<H>,
        upvalues: &[Value],
    ) -> Result<NativeFnId, ScriptError> {
        let object = Self::native_object_bytes(&kind, upvalues.len());
        self.charge_object(object, self.natives.insert_bytes(NATIVE_SLOT_BYTES))?;
        let uid = self.alloc_uid();
        let (slot, gen, _) = self.natives.insert(
            NativeRecord {
                uid,
                kind,
                upvalues: NativeUpvals::from_slice(upvalues),
                unpublished: false,
            },
            self.epoch,
        );
        Ok(NativeFnId {
            slot,
            gen,
            vm_id: self.vm_id,
        })
    }

    #[inline]
    pub fn get_native(&self, id: NativeFnId) -> Option<&NativeRecord<H>> {
        if id.vm_id != self.vm_id || self.condemned(self.natives.mark_of(id.slot)) {
            return None;
        }
        self.natives.get(id.slot, id.gen)
    }

    /// Write a native closure's upvalue (deletion barrier on the old value).
    pub fn set_native_upvalue(&mut self, id: NativeFnId, idx: usize, val: Value) {
        let old = match self.natives.get(id.slot, id.gen) {
            Some(rec) => match rec.upvalues.as_slice().get(idx) {
                Some(&old) => old,
                None => return,
            },
            None => return,
        };
        self.barrier(old);
        if let Some(rec) = self.natives.get_mut(id.slot, id.gen) {
            if let Some(dst) = rec.upvalues.as_mut_slice().get_mut(idx) {
                *dst = val;
            }
        }
    }

    // -- upvalues ------------------------------------------------------------

    pub fn alloc_upval(&mut self, state: UpvalState) -> Result<UpvalId, ScriptError> {
        self.charge_object(0, self.upvals.insert_bytes(UPVAL_SLOT_BYTES))?;
        let (slot, _, _) = self.upvals.insert(GcUpval { state }, self.epoch);
        Ok(UpvalId(slot))
    }

    pub fn upval(&self, id: UpvalId) -> &GcUpval {
        self.upvals
            .live(id.0)
            .unwrap_or_else(|| unreachable_object("upvalue"))
    }

    /// Close an open upvalue with `val` (the value moves from the stack, a root).
    pub fn close_upval(&mut self, id: UpvalId, val: Value) {
        self.barrier(val);
        if let Some(uv) = self.upvals.live_mut(id.0) {
            uv.state = UpvalState::Closed(val);
        }
    }

    /// Write a closed upvalue (deletion barrier on the old value).
    pub fn write_closed_upval(&mut self, id: UpvalId, val: Value) {
        if let Some(UpvalState::Closed(old)) = self.upvals.live(id.0).map(|u| u.state) {
            self.barrier(old);
        }
        if let Some(uv) = self.upvals.live_mut(id.0) {
            uv.state = UpvalState::Closed(val);
        }
    }

    // -- userdata ------------------------------------------------------------

    pub fn alloc_userdata(
        &mut self,
        tag: u32,
        payload: Wide,
        metatable: Option<TableId>,
    ) -> Result<UserdataId, ScriptError> {
        self.charge_object(0, self.userdata.insert_bytes(USERDATA_SLOT_BYTES))?;
        let uid = self.alloc_uid();
        let (slot, gen, _) = self.userdata.insert(
            GcUserdata {
                uid,
                tag,
                payload,
                metatable,
            },
            self.epoch,
        );
        Ok(UserdataId {
            slot,
            gen,
            vm_id: self.vm_id,
        })
    }

    #[inline]
    pub fn get_userdata(&self, id: UserdataId) -> Option<&GcUserdata> {
        if id.vm_id != self.vm_id || self.condemned(self.userdata.mark_of(id.slot)) {
            return None;
        }
        self.userdata.get(id.slot, id.gen)
    }

    pub fn userdata(&self, id: UserdataId) -> &GcUserdata {
        self.userdata
            .live(id.slot)
            .unwrap_or_else(|| unreachable_object("userdata"))
    }

    /// Look up a tag method (`__index`, `__add`, etc.) on `val`'s metatable (if any).
    pub fn get_metafield(&self, val: Value, field: StringId) -> Value {
        let mt = match val {
            Value::Table(tid) => self.tables.live(tid.slot).and_then(|t| t.metatable),
            Value::Userdata(uid) => self.userdata.live(uid.slot).and_then(|u| u.metatable),
            _ => None,
        };
        match mt.and_then(|mt_id| self.tables.live(mt_id.slot)) {
            Some(mt) => mt.raw_get_str(field),
            None => Value::Nil,
        }
    }

    pub fn get_tm(&self, val: Value, tm: TagMethod) -> Value {
        self.get_metafield(val, self.well_known.tm[tm as usize])
    }

    // -- liveness of host handles ----------------------------------------------

    /// Whether a handle names a live object of this heap. During a sweep, an object
    /// the finished mark did not reach is already dead even before its slot is freed.
    pub fn is_live_value(&self, val: Value) -> bool {
        let (ok, mark) = match val {
            Value::Nil | Value::Bool(_) | Value::Number(_) => return true,
            Value::String(id) => (
                id.vm_id == self.vm_id && self.strings.get(id.slot, id.gen).is_some(),
                self.strings.mark_of(id.slot),
            ),
            Value::Table(id) => (
                id.vm_id == self.vm_id && self.tables.get(id.slot, id.gen).is_some(),
                self.tables.mark_of(id.slot),
            ),
            Value::Closure(id) => (
                id.vm_id == self.vm_id && self.closures.get(id.slot, id.gen).is_some(),
                self.closures.mark_of(id.slot),
            ),
            Value::NativeFn(id) => (
                id.vm_id == self.vm_id && self.natives.get(id.slot, id.gen).is_some(),
                self.natives.mark_of(id.slot),
            ),
            Value::Userdata(id) => (
                id.vm_id == self.vm_id && self.userdata.get(id.slot, id.gen).is_some(),
                self.userdata.mark_of(id.slot),
            ),
        };
        ok && !self.condemned(mark)
    }

    /// Mark a value the host hands in during a mark (it may hold handles the heap does
    /// not reach). The value must be live.
    pub fn admit(&mut self, val: Value) {
        if let Value::NativeFn(id) = val {
            if let Some(n) = self.natives.get_mut(id.slot, id.gen) {
                n.unpublished = false;
            }
        }
        if self.phase == GcPhase::Mark {
            self.mark_value(val);
        }
    }

    pub fn track_host_native(&mut self, id: NativeFnId) {
        if let Some(n) = self.natives.get_mut(id.slot, id.gen) {
            n.unpublished = true;
        }
    }

    /// Deletion barrier: a reference about to be overwritten or dropped is marked if a
    /// mark is in progress, so the snapshot stays complete.
    #[inline]
    pub fn barrier(&mut self, old: Value) {
        if self.phase == GcPhase::Mark {
            self.mark_value(old);
        }
    }

    // -- collector -------------------------------------------------------------

    /// Whether a step is due: during a cycle, when [`GC_STEP_BYTES`] of debt have
    /// accumulated; in the pause, when the live object bytes reach the threshold (or
    /// set-aside objects wait to be destroyed).
    #[inline]
    pub fn step_due(&self) -> bool {
        match self.phase {
            GcPhase::Pause => {
                self.object_bytes >= self.gc_threshold
                    || (!self.graveyard.is_empty() && self.debt >= GC_STEP_BYTES)
            }
            _ => self.debt >= GC_STEP_BYTES,
        }
    }

    /// Whether the pause is over: the live object bytes reached the threshold.
    #[inline]
    pub fn cycle_due(&self) -> bool {
        self.phase == GcPhase::Pause && self.object_bytes >= self.gc_threshold
    }

    /// Slots in all arenas (each costs the sweep at least one unit).
    fn total_slots(&self) -> u32 {
        (self.strings.slots.len()
            + self.tables.slots.len()
            + self.protos.slots.len()
            + self.closures.slots.len()
            + self.natives.slots.len()
            + self.upvals.slots.len()
            + self.userdata.slots.len()) as u32
    }

    /// Work units [`GcHeap::begin_cycle`] will do (the roots are the caller's).
    pub fn cycle_start_units(&self) -> u32 {
        CYCLE_START_UNITS
    }

    /// An upper bound of the work units a full collection does, roots excepted: every
    /// object traversed and, if weak, cleared, every slot swept and freed, and what the
    /// graveyard holds. A unit of traversal or clearing covers at least two charged
    /// bytes (the densest case is a closure's list of upvalue cells), and a slot costs
    /// the sweep and its object's header at most sixteen units.
    pub fn full_collection_bound(&self) -> u32 {
        self.cycle_start_units()
            + (self.object_bytes / 2).units()
            + self.total_slots() * 16
            + self.grave_work
    }

    /// Begin a cycle: advance the epoch (everything becomes unmarked) and mark the
    /// well-known strings. The caller then marks the roots before the next mutation.
    /// A mark in progress is abandoned (nothing has been freed by it). Returns the work
    /// done, [`GcHeap::cycle_start_units`].
    pub fn begin_cycle(&mut self, full: bool) -> u32 {
        debug_assert!(matches!(self.phase, GcPhase::Pause | GcPhase::Mark));
        let units = self.cycle_start_units();
        self.clear_all_gray();
        self.cursor = None;
        // After a sweep or recoloring, every live slot has one color. The next
        // incremental mark and a full mark that supersedes it each get a fresh color.
        self.epoch = self.epoch % 3 + 1;
        self.phase = GcPhase::Mark;
        self.full = full;
        self.trace_stack = true;
        self.weak_head = NONE;
        self.weak_work = 0;
        self.weak_pos = 0;
        if let Some(tick) = &mut self.tick {
            tick.weak_cursor = None;
            tick.since_start = 0;
        }
        self.sweep_arena = 0;
        self.sweep_pos = 0;
        self.debt = 0;
        let wk = self.well_known;
        for s in wk.all() {
            self.mark_string(s);
        }
        units
    }

    /// A stronger metatable can expose edges skipped by weak traversal. Preserve
    /// all remaining slots and recolor them before taking a new snapshot.
    fn abandon_full_mark(&mut self) {
        if self.phase == GcPhase::Mark && self.full {
            self.clear_all_gray();
            self.cursor = None;
            self.weak_head = NONE;
            self.weak_work = 0;
            self.weak_pos = 0;
            self.full = false;
            self.phase = GcPhase::Recolor;
            self.trace_stack = false;
            if let Some(tick) = &mut self.tick {
                tick.weak_cursor = None;
            }
        }
    }

    fn clear_all_gray(&mut self) {
        self.tables.clear_gray();
        self.protos.clear_gray();
        self.closures.clear_gray();
        self.natives.clear_gray();
    }

    fn has_gray(&self) -> bool {
        self.tables.gray_head != NONE
            || self.closures.gray_head != NONE
            || self.protos.gray_head != NONE
            || self.natives.gray_head != NONE
    }

    #[inline]
    pub fn mark_string(&mut self, id: StringId) {
        self.strings.set_mark(id.slot, self.epoch);
    }

    #[inline]
    pub fn mark_table(&mut self, id: TableId) {
        if self.tables.set_mark(id.slot, self.epoch) {
            self.tables.push_gray(id.slot);
        }
    }

    #[inline]
    pub fn mark_closure(&mut self, id: ClosureId) {
        if self.closures.set_mark(id.slot, self.epoch) {
            self.closures.push_gray(id.slot);
        }
    }

    #[inline]
    pub fn mark_proto(&mut self, id: LoadedProtoId) {
        if self.protos.set_mark(id.0, self.epoch) {
            self.protos.push_gray(id.0);
        }
    }

    /// Mark a prototype the VM holds outside the heap during a mark (see
    /// [`GcHeap::admit`]).
    pub fn admit_proto(&mut self, id: LoadedProtoId) {
        if self.phase == GcPhase::Mark {
            self.mark_proto(id);
        }
    }

    #[inline]
    fn mark_native(&mut self, id: NativeFnId) {
        if self.natives.set_mark(id.slot, self.epoch) {
            self.natives.push_gray(id.slot);
        }
    }

    /// Mark an upvalue and the value it holds when closed (one level, no gray list).
    #[inline]
    pub fn mark_upval(&mut self, id: UpvalId) {
        if self.upvals.set_mark(id.0, self.epoch) {
            if let Some(UpvalState::Closed(v)) = self.upvals.live(id.0).map(|u| u.state) {
                self.mark_value(v);
            }
        }
    }

    #[inline]
    fn mark_userdata(&mut self, id: UserdataId) {
        if self.userdata.set_mark(id.slot, self.epoch) {
            if let Some(mt) = self.userdata.live(id.slot).and_then(|u| u.metatable) {
                self.mark_table(mt);
            }
        }
    }

    #[inline]
    pub fn mark_value(&mut self, v: Value) {
        match v {
            Value::String(s) => self.mark_string(s),
            Value::Table(t) => self.mark_table(t),
            Value::Closure(c) => self.mark_closure(c),
            Value::NativeFn(f) => self.mark_native(f),
            Value::Userdata(u) => self.mark_userdata(u),
            Value::Nil | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// Traverse gray objects for at most `budget` work units: [`OBJECT_HEADER_UNITS`]
    /// to start an object, then one unit per array slot, constant, child or native
    /// upvalue and two per map entry or upvalue cell, in blocks that stop exactly at the
    /// budget (the object's cursor keeps its place). Returns the work done, never more
    /// than `budget`, and whether no gray object is left.
    pub fn propagate(&mut self, budget: u32) -> (u32, bool) {
        let mut work = 0u32;
        loop {
            let cursor = match self.cursor.take() {
                Some(c) => c,
                None => {
                    if !self.has_gray() {
                        return (work, true);
                    }
                    if work + OBJECT_HEADER_UNITS > budget {
                        return (work, false);
                    }
                    work += OBJECT_HEADER_UNITS;
                    match self.start_gray() {
                        Some(c) => c,
                        None => return (work, true),
                    }
                }
            };
            let (w, rest) = self.advance(cursor, budget - work);
            work += w;
            if let Some(c) = rest {
                self.cursor = Some(c);
                return (work, false);
            }
        }
    }

    /// Weak mode a metatable gives (full collections only).
    fn weak_flags(&self, mt: Option<TableId>) -> (bool, bool) {
        match mt.and_then(|m| self.tables.live(m.slot)) {
            Some(t) => (t.mode & MODE_WEAK_KEYS != 0, t.mode & MODE_WEAK_VALUES != 0),
            None => (false, false),
        }
    }

    /// Take the next gray object and do the fixed part of its traversal.
    fn start_gray(&mut self) -> Option<Cursor> {
        if let Some(slot) = self.tables.pop_gray() {
            let (mt, entries) = match self.tables.live(slot) {
                Some(t) => (t.metatable, t.entries.len()),
                None => (None, 0),
            };
            if let Some(m) = mt {
                self.mark_table(m);
            }
            let (weak_k, weak_v) = if self.full {
                self.weak_flags(mt)
            } else {
                (false, false)
            };
            if weak_k || weak_v {
                if let Some(table) = self.tables.live_mut(slot) {
                    table.weak_next = self.weak_head;
                    self.weak_head = slot;
                    if let Some(tick) = &mut self.tick {
                        tick.weak_cursor = None;
                    } else {
                        self.weak_work = self.weak_work.saturating_add((table.entries.len() + table.array.len()).units()).saturating_add(1);
                    }
                }
            }
            return Some(Cursor::TableEntries {
                slot,
                pos: entries,
                weak_k,
                weak_v,
            });
        }
        if let Some(slot) = self.closures.pop_gray() {
            let Some((env, proto, n)) = self
                .closures
                .live(slot)
                .map(|c| (c.env, c.proto, c.upvalues.len()))
            else {
                return Some(Cursor::Closure { slot, pos: 0 });
            };
            self.mark_table(env);
            self.mark_proto(proto);
            return Some(Cursor::Closure { slot, pos: n });
        }
        if let Some(slot) = self.protos.pop_gray() {
            return Some(Cursor::Proto { slot, pos: 0 });
        }
        if let Some(slot) = self.natives.pop_gray() {
            return Some(Cursor::Native { slot, pos: 0 });
        }
        None
    }

    /// Continue a traversal with at most `left` work units; returns the work done and
    /// the cursor if the object is not finished.
    fn advance(&mut self, cursor: Cursor, left: u32) -> (u32, Option<Cursor>) {
        const BLOCK: usize = 32;
        let mut work = 0u32;
        match cursor {
            Cursor::TableEntries {
                slot,
                pos,
                weak_k,
                weak_v,
            } => {
                let mut pos = pos.min(self.tables.live(slot).map_or(0, |t| t.entries.len()));
                let mut block = [MapEntry {
                    key: TableKey::Tombstone,
                    value: Value::Nil,
                }; BLOCK];
                while pos > 0 {
                    let n = pos.min(BLOCK).min(((left - work) / 2) as usize);
                    if n == 0 {
                        let rest = Cursor::TableEntries {
                            slot,
                            pos,
                            weak_k,
                            weak_v,
                        };
                        return (work, Some(rest));
                    }
                    let start = pos - n;
                    if let Some(t) = self.tables.live(slot) {
                        block[..n].copy_from_slice(&t.entries[start..pos]);
                    }
                    pos = start;
                    work += 2 * n.units();
                    for e in &block[..n] {
                        if e.key == TableKey::Tombstone || e.value.is_nil() {
                            continue;
                        }
                        if !weak_k {
                            self.mark_value(e.key.to_value());
                        }
                        if !weak_v {
                            self.mark_value(e.value);
                        }
                    }
                }
                if weak_v {
                    return (work, None);
                }
                let alen = self.tables.live(slot).map_or(0, |t| t.array.len());
                let (w, rest) = self.advance(Cursor::TableArray { slot, pos: alen }, left - work);
                (work + w, rest)
            }
            Cursor::TableArray { slot, pos } => {
                let mut pos = pos.min(self.tables.live(slot).map_or(0, |t| t.array.len()));
                let mut block = [Value::Nil; BLOCK];
                while pos > 0 {
                    let n = pos.min(BLOCK).min((left - work) as usize);
                    if n == 0 {
                        return (work, Some(Cursor::TableArray { slot, pos }));
                    }
                    let start = pos - n;
                    if let Some(t) = self.tables.live(slot) {
                        block[..n].copy_from_slice(&t.array[start..pos]);
                    }
                    pos = start;
                    work += n.units();
                    for &v in &block[..n] {
                        self.mark_value(v);
                    }
                }
                (work, None)
            }
            Cursor::Closure { slot, mut pos } => {
                while pos > 0 {
                    if work + 2 > left {
                        return (work, Some(Cursor::Closure { slot, pos }));
                    }
                    pos -= 1;
                    work += 2;
                    if let Some(uid) = self.closures.live(slot).and_then(|c| c.upvalues.get(pos)) {
                        self.mark_upval(uid);
                    }
                }
                (work, None)
            }
            Cursor::Proto { slot, mut pos } => {
                let (nc, nk) = self
                    .protos
                    .live(slot)
                    .map_or((0, 0), |p| (p.constants.len(), p.children.len()));
                while pos < nc + nk {
                    if work + 1 > left {
                        return (work, Some(Cursor::Proto { slot, pos }));
                    }
                    work += 1;
                    if pos < nc {
                        let v = self.protos.live(slot).map_or(Value::Nil, |p| p.constants[pos]);
                        self.mark_value(v);
                    } else if let Some(c) = self.protos.live(slot).map(|p| p.children[pos - nc]) {
                        self.mark_proto(c);
                    }
                    pos += 1;
                }
                (work, None)
            }
            Cursor::Native { slot, mut pos } => {
                let n = self
                    .natives
                    .live(slot)
                    .map_or(0, |rec| rec.upvalues.as_slice().len());
                while pos < n {
                    if work + 1 > left {
                        return (work, Some(Cursor::Native { slot, pos }));
                    }
                    work += 1;
                    let v = self
                        .natives
                        .live(slot)
                        .and_then(|rec| rec.upvalues.as_slice().get(pos).copied())
                        .unwrap_or(Value::Nil);
                    self.mark_value(v);
                    pos += 1;
                }
                (work, None)
            }
        }
    }

    /// Work still needed to finish an uninterrupted mark, including its weak tables.
    pub fn finish_mark_units(&self) -> u32 {
        1 + self.weak_work
    }

    /// Clear at most the admitted number of weak slots, then enter sweep.
    pub fn finish_mark_slice(&mut self, budget: u32) -> (u32, bool) {
        let mut work = 0;
        while self.weak_head != NONE && work < budget {
            let slot = self.weak_head;
            let entry = &self.tables.slots[slot as usize];
            let table = entry.value.as_ref().expect("marked weak table");
            let tid = TableId {
                slot,
                gen: entry.gen,
                vm_id: self.vm_id,
            };
            let (keys, values) = self.weak_flags(table.metatable);
            let (array, entries, next) = (table.array.len(), table.entries.len(), table.weak_next);
            let end = array + entries;
            let take = (end - self.weak_pos).min((budget - work) as usize);
            for i in self.weak_pos..self.weak_pos + take {
                if i < array {
                    if values {
                        let v = self.table(tid).array[i];
                        if !v.is_nil() && !self.weakly_alive(v) {
                            self.table_mut(tid).array[i] = Value::Nil;
                        }
                    }
                } else {
                    self.clear_weak_entry(tid, i - array, keys, values);
                }
            }
            self.weak_pos += take;
            work += take.units();
            self.weak_work -= take.units();
            // Account index nodes released by this slice.
            self.account_weak_removals(tid);
            if self.weak_pos == end && work < budget {
                self.weak_head = next;
                self.weak_pos = 0;
                self.weak_work -= 1;
                work += 1;
            }
        }
        if work == budget {
            return (work, false);
        }
        self.phase = GcPhase::Sweep;
        self.trace_stack = false;
        self.sweep_arena = 0;
        self.sweep_pos = 0;
        (work + 1, true)
    }

    /// Weak entries can move during a paced mark. Positions already visited
    /// can be visited again on compaction. Writes shade both the key and value.
    pub fn finish_paced_mark(&mut self, budget: u32) -> u32 {
        let mut work = 0;
        while self.weak_head != NONE && work < budget {
            let slot = self.weak_head;
            let entry = &self.tables.slots[slot as usize];
            let table = entry.value.as_ref().expect("marked weak table");
            let id = TableId {
                slot,
                gen: entry.gen,
                vm_id: self.vm_id,
            };
            let (keys, values) = self.weak_flags(table.metatable);
            let (array, map, next) = (table.array.len(), table.entries.len(), table.weak_next);
            let tick = self.tick.as_mut().expect("simulation tick");
            let (in_array, pos) = tick.weak_cursor.unwrap_or((false, map));
            let end = pos.min(if in_array { array } else { map });
            let start = end.saturating_sub((budget - work) as usize);
            for i in (start..end).rev() {
                if in_array {
                    let v = self.table(id).array[i];
                    if values && !v.is_nil() && !self.weakly_alive(v) {
                        self.table_mut(id).array[i] = Value::Nil;
                    }
                } else {
                    self.clear_weak_entry(id, i, keys, values);
                }
            }
            self.account_weak_removals(id);
            work += (end - start).units();
            let tick = self.tick.as_mut().expect("simulation tick");
            if start != 0 || work == budget {
                tick.weak_cursor = Some((in_array, start));
            } else if !in_array {
                tick.weak_cursor = Some((true, array));
            } else {
                work += 1;
                self.weak_head = next;
                tick.weak_cursor = None;
            }
        }
        if work < budget {
            self.phase = GcPhase::Sweep;
            self.trace_stack = false;
            self.sweep_arena = 0;
            self.sweep_pos = 0;
            work += 1;
        }
        work
    }

    fn account_weak_removals(&mut self, id: TableId) {
        let table = self.table_mut(id);
        let old = table.mem_bytes;
        table.mem_bytes = table.compute_mem_bytes();
        let delta = table.mem_bytes as isize - old as isize;
        self.apply_object_delta(delta);
    }

    /// Lua 5.0.3 `valismarked`: values that cannot be collected and strings (never
    /// weak) count as marked.
    fn weakly_alive(&mut self, v: Value) -> bool {
        match v {
            Value::Nil | Value::Bool(_) | Value::Number(_) => true,
            Value::String(s) => {
                self.mark_string(s);
                true
            }
            Value::Table(t) => self.tables.mark_of(t.slot) == self.epoch,
            Value::Closure(c) => self.closures.mark_of(c.slot) == self.epoch,
            Value::NativeFn(f) => self.natives.mark_of(f.slot) == self.epoch,
            Value::Userdata(u) => self.userdata.mark_of(u.slot) == self.epoch,
        }
    }

    /// Remove the entries of a weak table whose weak key or value was not marked, with
    /// the weak mode its traversal used. A removed entry's key is forgotten as well (it
    /// may be collected).
    fn clear_weak_entry(&mut self, tid: TableId, i: usize, weak_k: bool, weak_v: bool) {
        let e = self.table(tid).entries[i];
        if e.key == TableKey::Tombstone {
            return;
        }
        let key_dead = weak_k && !self.weakly_alive(e.key.to_value());
        let val_dead = weak_v && !e.value.is_nil() && !self.weakly_alive(e.value);
        if key_dead || (val_dead && !self.weakly_alive(e.key.to_value())) {
            // The key is not reachable: drop it from the index too.
            let tbl = self.table_mut(tid);
            tbl.index.remove(&e.key);
            if !e.value.is_nil() {
                tbl.tombstones += 1;
            }
            tbl.entries[i] = MapEntry {
                key: TableKey::Tombstone,
                value: Value::Nil,
            };
        } else if val_dead {
            let tbl = self.table_mut(tid);
            tbl.entries[i].value = Value::Nil;
            tbl.tombstones += 1;
        }
    }

    fn arena_len(&self, arena: u8) -> u32 {
        (match arena {
            0 => self.strings.slots.len(),
            1 => self.tables.slots.len(),
            2 => self.protos.slots.len(),
            3 => self.closures.slots.len(),
            4 => self.natives.slots.len(),
            5 => self.upvals.slots.len(),
            _ => self.userdata.slots.len(),
        }) as u32
    }

    /// Free unmarked objects for at most `budget` work units: one per slot visited plus
    /// the destruction of a dead object, known before it is freed (a large index or
    /// prototype goes to the graveyard instead). Returns the work done, never more than
    /// `budget`, and whether the sweep is complete (the heap is then in the pause phase
    /// with a new threshold). After an interrupted full mark the same cursor
    /// restores one color at one unit per slot and leaves every object alive.
    pub fn sweep(&mut self, budget: u32) -> (u32, bool) {
        let mut work = 0u32;
        let epoch = self.epoch;
        while self.sweep_arena < SWEEP_ARENAS {
            let len = self.arena_len(self.sweep_arena);
            if self.phase == GcPhase::Recolor {
                let count = (budget - work).min(len - self.sweep_pos);
                let end = self.sweep_pos + count;
                let range = self.sweep_pos as usize..end as usize;
                fn recolor<T>(slots: &mut [Slot<T>], epoch: u32) {
                    for slot in slots {
                        slot.mark = epoch;
                    }
                }
                match self.sweep_arena {
                    0 => recolor(&mut self.strings.slots[range], epoch),
                    1 => recolor(&mut self.tables.slots[range], epoch),
                    2 => recolor(&mut self.protos.slots[range], epoch),
                    3 => recolor(&mut self.closures.slots[range], epoch),
                    4 => recolor(&mut self.natives.slots[range], epoch),
                    5 => recolor(&mut self.upvals.slots[range], epoch),
                    _ => recolor(&mut self.userdata.slots[range], epoch),
                }
                work += count;
                self.sweep_pos = end;
                if end < len {
                    return (work, false);
                }
            }
            while self.sweep_pos < len {
                if !self.graveyard.is_empty() {
                    work += self.drain_graveyard(budget - work);
                    if work == budget {
                        return (work, false);
                    }
                }
                let slot = self.sweep_pos;
                let cost = 1 + self.free_units(self.sweep_arena, slot, epoch);
                if work + cost > budget {
                    return (work, false);
                }
                self.sweep_pos += 1;
                work += cost;
                self.sweep_slot(self.sweep_arena, slot, epoch);
            }
            self.sweep_arena += 1;
            self.sweep_pos = 0;
        }
        if self.phase == GcPhase::Recolor {
            self.phase = GcPhase::Pause;
            return (work, true);
        }
        self.phase = GcPhase::Pause;
        self.full = false;
        self.debt = 0;
        // The next cycle starts at twice the live bytes, or sooner near the memory
        // limit: once half of the remaining room is used, so the incremental cycle has
        // the other half to finish in before an allocation would need a full one.
        let headroom = self.memory_limit.saturating_sub(self.memory_bytes);
        self.gc_threshold = (self.object_bytes / 100)
            .saturating_mul(GC_PAUSE_PERCENT)
            .min(self.object_bytes.saturating_add(headroom / 2))
            .max(GC_MIN_THRESHOLD);
        self.stats.cycles = self.stats.cycles.saturating_add(1);
        (work, true)
    }

    /// Work units of freeing the object in `slot` if this cycle did not mark it (zero
    /// for a live object or an empty slot).
    fn free_units(&self, arena: u8, slot: u32, epoch: u32) -> u32 {
        fn dead<T>(a: &Arena<T>, slot: u32, epoch: u32) -> Option<&T> {
            let s = &a.slots[slot as usize];
            s.value.as_ref().filter(|_| s.mark != epoch)
        }
        match arena {
            0 => dead(&self.strings, slot, epoch).map_or(0, |_| STRING_FREE_UNITS),
            1 => dead(&self.tables, slot, epoch).map_or(0, |t| {
                let n = t.index.len();
                TABLE_FREE_UNITS + if n <= GRAVE_MIN { n.units() / 2 } else { 2 }
            }),
            2 => dead(&self.protos, slot, epoch).map_or(0, |p| {
                if Rc::strong_count(&p.proto) > 1 {
                    PROTO_FREE_UNITS
                } else if proto_drops_small(&p.proto) {
                    PROTO_FREE_UNITS + proto_parts(&p.proto).units() / 2
                } else {
                    PROTO_FREE_UNITS + 2
                }
            }),
            3 => dead(&self.closures, slot, epoch).map_or(0, |_| CLOSURE_FREE_UNITS),
            4 => dead(&self.natives, slot, epoch).map_or(0, |_| NATIVE_FREE_UNITS),
            5 => dead(&self.upvals, slot, epoch).map_or(0, |_| SMALL_FREE_UNITS),
            _ => dead(&self.userdata, slot, epoch).map_or(0, |_| SMALL_FREE_UNITS),
        }
    }

    /// Free one slot if it holds an object this cycle did not mark.
    fn sweep_slot(&mut self, arena: u8, slot: u32, epoch: u32) {
        macro_rules! dead {
            ($arena:expr) => {{
                let s = &$arena.slots[slot as usize];
                s.value.is_some() && s.mark != epoch
            }};
        }
        match arena {
            0 => {
                if !dead!(self.strings) {
                    return;
                }
                self.intern.remove(&mut self.strings, slot);
                if let Some(s) = self.strings.free(slot) {
                    self.release_object(string_object_bytes(s.bytes.len()));
                }
            }
            1 => {
                if !dead!(self.tables) {
                    return;
                }
                if let Some(t) = self.tables.free(slot) {
                    let LuaTable {
                        array,
                        entries,
                        index,
                        mem_bytes,
                        ..
                    } = t;
                    drop(array);
                    drop(entries);
                    if index.len() <= GRAVE_MIN {
                        drop(index);
                        self.release_object(mem_bytes);
                    } else {
                        let index_part = index_bytes(index.len()).min(mem_bytes);
                        self.release_object(mem_bytes - index_part);
                        self.queue_grave(Grave::Release(index_part));
                        self.queue_grave(Grave::Index(index.into_iter()));
                    }
                }
            }
            2 => {
                if !dead!(self.protos) {
                    return;
                }
                if let Some(p) = self.protos.free(slot) {
                    let LoadedProto {
                        proto,
                        constants,
                        children,
                        mem_bytes,
                    } = p;
                    drop(constants);
                    drop(children);
                    if proto_drops_small(&proto) {
                        drop(proto);
                        self.release_object(mem_bytes);
                    } else {
                        self.queue_grave(Grave::Release(mem_bytes));
                        self.bury_proto(proto);
                    }
                }
            }
            3 => {
                if dead!(self.closures) {
                    if let Some(c) = self.closures.free(slot) {
                        self.release_object(c.upvalues.block_bytes());
                    }
                }
            }
            4 => {
                if dead!(self.natives) {
                    if let Some(n) = self.natives.free(slot) {
                        self.stats.discarded_host_natives =
                            self.stats.discarded_host_natives.saturating_add(u32::from(n.unpublished));
                        self.release_object(Self::native_object_bytes(
                            &n.kind,
                            n.upvalues.as_slice().len(),
                        ));
                    }
                }
            }
            5 => {
                if dead!(self.upvals) {
                    self.upvals.free(slot);
                }
            }
            _ => {
                if dead!(self.userdata) {
                    self.userdata.free(slot);
                }
            }
        }
    }

    /// Set a prototype aside for destruction in pieces if this is its last reference
    /// (its code and line buffers are single blocks and go at once).
    fn bury_proto(&mut self, proto: Rc<Proto>) {
        let Ok(p) = Rc::try_unwrap(proto) else {
            return;
        };
        let Proto {
            local_vars,
            upvalue_names,
            constants,
            protos,
            ..
        } = p;
        if !protos.is_empty() {
            self.queue_grave(Grave::Children(protos.into_vec().into_iter()));
        }
        if !constants.is_empty() {
            self.queue_grave(Grave::Constants(constants.into_vec().into_iter()));
        }
        if !upvalue_names.is_empty() {
            self.queue_grave(Grave::UpvalueNames(upvalue_names.into_vec().into_iter()));
        }
        if !local_vars.is_empty() {
            self.queue_grave(Grave::Locals(local_vars.into_vec().into_iter()));
        }
    }

    /// A sweep destroys one object before queuing another. A prototype needs one
    /// child iterator per ancestor, three leaf iterators and its final release.
    pub fn reserve_graves(&mut self, slots: usize) -> Result<(), ScriptError> {
        let old = self.graveyard.capacity();
        if slots <= old {
            return Ok(());
        }
        let block = block_bytes(slots.saturating_mul(GRAVE_BYTES));
        self.check_budget(block)?;
        self.graveyard
            .try_reserve_exact(slots - self.graveyard.len())
            .map_err(|_| ScriptError::ResourceLimit("collector work list"))?;
        self.reserve(block - block_bytes(old * GRAVE_BYTES))
    }

    fn queue_grave(&mut self, grave: Grave) {
        debug_assert!(self.graveyard.len() < self.graveyard.capacity());
        self.grave_work = self.grave_work.saturating_add(grave.len().units()).saturating_add(1);
        self.graveyard.push(grave);
    }

    /// Destroy set-aside parts for at most `budget` work units (one per element);
    /// returns the work done.
    pub fn drain_graveyard(&mut self, budget: u32) -> u32 {
        let mut work = 0u32;
        while work < budget {
            let Some(top) = self.graveyard.last_mut() else {
                break;
            };
            let mut child = None;
            let exhausted = match top {
                Grave::Index(it) => it.next().is_none(),
                Grave::Locals(it) => it.next().is_none(),
                Grave::UpvalueNames(it) => it.next().is_none(),
                Grave::Constants(it) => it.next().is_none(),
                Grave::Children(it) => match it.next() {
                    Some(c) => {
                        child = Some(c);
                        false
                    }
                    None => true,
                },
                Grave::Release(_) => true,
            };
            work += 1;
            self.grave_work -= 1;
            if exhausted {
                if let Some(Grave::Release(bytes)) = self.graveyard.pop() {
                    self.release_object(bytes);
                }
            }
            if let Some(c) = child {
                self.bury_proto(c);
            }
        }
        work
    }

    pub fn has_deferred_drops(&self) -> bool {
        !self.graveyard.is_empty()
    }

    /// Reduce the allocation debt by what `work` units pay for.
    #[inline]
    pub fn pay_debt(&mut self, work: u32) {
        let paid = (work as usize).saturating_mul(8) / GC_WORK_PER_BYTE_EIGHTHS;
        self.debt = self.debt.saturating_sub(paid);
    }

    /// Work units a step should do for the current debt.
    #[inline]
    pub fn step_work(&self) -> u32 {
        (self.debt.saturating_mul(GC_WORK_PER_BYTE_EIGHTHS) / 8).units()
    }

    /// Change the threshold (`collectgarbage(n)` / `lua_setgcthreshold`).
    pub fn set_threshold(&mut self, bytes: usize) {
        self.gc_threshold = bytes;
    }
}

#[cold]
#[inline(never)]
fn unreachable_object(kind: &str) -> ! {
    // A handle the VM itself holds always names a live object: the collector frees
    // only what is unreachable from the VM. Reaching this is a bug in the crate.
    panic!("internal error: the VM holds a freed {kind}")
}

// ---------------------------------------------------------------------------
// State digest (cross-machine determinism checks)
// ---------------------------------------------------------------------------

impl StringId {
    /// `(slot, generation)` of this handle, stable for the life of the string within
    /// one VM (for host-side maps keyed by script strings).
    pub fn local_key(self) -> (u32, u32) {
        (self.slot, self.gen)
    }
}

impl ValueRef {
    /// `(slot, generation)` of this reference, stable until it is released.
    pub fn local_key(self) -> (u32, u32) {
        (self.slot, self.gen)
    }
}

/// FNV-1a over the deterministic VM state. Handles hash as `(slot, generation)`; no
/// address, process-local VM number or host function pointer is hashed.
pub(crate) struct Digest(Fnv);

impl Digest {
    pub(crate) fn new() -> Self {
        Self(Fnv::new())
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.0.bytes(b);
    }

    pub(crate) fn word(&mut self, n: Wide) {
        self.0.word(n);
    }

    /// A small number (a flag, a slot, a generation) as the eight little-endian bytes `word` gives it.
    pub(crate) fn small(&mut self, n: impl Into<u32>) {
        self.bytes(&n.into().to_le_bytes());
        self.bytes(&[0; 4]);
    }

    /// A length or an index, as `small` (saturating above `u32::MAX`, far beyond any VM memory).
    pub(crate) fn count(&mut self, n: usize) {
        self.small(u32::try_from(n).unwrap_or(u32::MAX));
    }

    /// `(high << 32) | low` as the eight little-endian bytes `word` gives it.
    pub(crate) fn pair(&mut self, high: u32, low: u32) {
        self.bytes(&low.to_le_bytes());
        self.bytes(&high.to_le_bytes());
    }

    /// An optional count: the eight bytes of an all-ones word for none.
    pub(crate) fn option_count(&mut self, n: Option<usize>) {
        match n {
            Some(n) => self.count(n),
            None => self.bytes(&[0xff; 8]),
        }
    }

    pub(crate) fn value(&mut self, v: Value) {
        match v {
            Value::Nil => (self.bytes(&[0]), self.small(0u32)),
            Value::Bool(b) => (self.bytes(&[1]), self.small(b)),
            Value::Number(n) => (self.bytes(&[2]), self.small(n.to_bits())),
            Value::String(h) => (self.bytes(&[3]), self.pair(h.gen, h.slot)),
            Value::Table(h) => (self.bytes(&[4]), self.pair(h.gen, h.slot)),
            Value::Closure(h) => (self.bytes(&[5]), self.pair(h.gen, h.slot)),
            Value::NativeFn(h) => (self.bytes(&[6]), self.pair(h.gen, h.slot)),
            Value::Userdata(h) => (self.bytes(&[7]), self.pair(h.gen, h.slot)),
        };
    }

    pub(crate) fn words(&mut self, list: &[u32]) {
        self.count(list.len());
        for &n in list {
            self.small(n);
        }
    }

    pub(crate) fn finish(self) -> Wide {
        self.0.finish()
    }
}

impl<T> Arena<T> {
    /// The slot bookkeeping (generations, marks, list links, occupancy) and each live
    /// object through `each`.
    fn digest(
        &self,
        h: &mut Digest,
        trace_live_links: bool,
        mut each: impl FnMut(&mut Digest, &T),
    ) {
        h.count(self.slots.len());
        h.count(self.slots.capacity());
        h.small(self.free_head);
        h.small(self.gray_head);
        for s in &self.slots {
            h.pair(s.gen, s.mark);
            if trace_live_links || s.value.is_none() {
                h.small(s.link);
            }
            match &s.value {
                Some(v) => {
                    h.bytes(&[1]);
                    each(h, v);
                }
                None => h.bytes(&[0]),
            }
        }
    }
}

impl<H> GcHeap<H> {
    /// Reserve arena room for `count` more userdata, so that creating them later (for
    /// example one per game object in repeated callbacks) does not grow the arena.
    pub(crate) fn reserve_userdata(&mut self, count: usize) -> Result<(), ScriptError> {
        if count > self.memory_limit / USERDATA_SLOT_BYTES {
            return Err(ScriptError::ResourceLimit("userdata reservation"));
        }
        let slots = &self.userdata.slots;
        let cap = slots.capacity();
        let want = slots.len().saturating_add(count);
        if want <= cap {
            return Ok(());
        }
        // The new buffer is charged before it exists, beside the old one.
        let new_block = block_bytes(want.saturating_mul(USERDATA_SLOT_BYTES));
        self.check_budget(new_block)?;
        self.userdata
            .slots
            .try_reserve_exact(want - self.userdata.slots.len())
            .map_err(|_| ScriptError::ResourceLimit("userdata reservation"))?;
        self.add_bytes(new_block - block_bytes(cap * USERDATA_SLOT_BYTES));
        Ok(())
    }

    pub(crate) fn digest(&self, h: &mut Digest) {
        self.strings.digest(h, false, |h, s| {
            h.count(s.bytes.len());
            h.bytes(&s.bytes);
        });
        self.tables.digest(h, true, |h, t| {
            h.word(t.uid);
            h.value(t.metatable.map_or(Value::Nil, Value::Table));
            match t.explicit_n {
                Some(n) => h.small(n as u32),
                None => h.option_count(None),
            }
            h.count(t.array.len());
            for &v in &t.array {
                h.value(v);
            }
            h.count(t.entries.len());
            for e in &t.entries {
                h.bytes(&[u8::from(e.key == TableKey::Tombstone)]);
                h.value(e.key.to_value());
                h.value(e.value);
            }
            h.count(t.tombstones);
            h.count(t.mem_bytes);
        });
        self.protos.digest(h, true, |h, p| {
            let pr = &p.proto;
            h.count(Rc::strong_count(pr));
            h.bytes(&[pr.nups, pr.num_params, pr.is_vararg, pr.max_stack_size]);
            h.small(pr.line_defined as u32);
            match &pr.source {
                Some(s) => {
                    h.count(s.len());
                    h.bytes(s);
                }
                None => h.option_count(None),
            }
            h.count(pr.code.len());
            for ins in pr.code.iter() {
                h.bytes(&ins.0.to_le_bytes());
            }
            h.count(pr.line_info.len());
            for &line in pr.line_info.iter() {
                h.small(line as u32);
            }
            // Debug names and ranges appear in error messages, so they are state too.
            h.count(pr.local_vars.len());
            for lv in pr.local_vars.iter() {
                h.count(lv.name.len());
                h.bytes(&lv.name);
                h.pair(lv.start_pc as u32, lv.end_pc as u32);
            }
            h.count(pr.upvalue_names.len());
            for name in pr.upvalue_names.iter() {
                h.count(name.len());
                h.bytes(name);
            }
            h.count(p.constants.len());
            for &c in p.constants.iter() {
                h.value(c);
            }
            h.count(p.children.len());
            for c in p.children.iter() {
                h.small(c.0);
            }
            h.count(p.mem_bytes);
        });
        self.closures.digest(h, true, |h, c| {
            h.word(c.uid);
            h.small(c.proto.0);
            h.value(Value::Table(c.env));
            h.count(c.upvalues.len());
            for u in c.upvalues.iter() {
                h.small(u.0);
            }
        });
        self.natives.digest(h, true, |h, n| {
            h.word(n.uid);
            h.bytes(&[u8::from(n.unpublished)]);
            let ups = n.upvalues.as_slice();
            h.count(ups.len());
            for &v in ups {
                h.value(v);
            }
            h.count(Self::native_object_bytes(&n.kind, ups.len()));
        });
        self.upvals.digest(h, true, |h, u| match u.state {
            UpvalState::Open(i) => {
                h.bytes(&[1]);
                h.count(i);
            }
            UpvalState::Closed(v) => {
                h.bytes(&[2]);
                h.value(v);
            }
        });
        self.userdata.digest(h, true, |h, u| {
            h.word(u.uid);
            h.small(u.tag);
            h.word(u.payload);
            h.value(u.metatable.map_or(Value::Nil, Value::Table));
        });
        h.word(self.next_uid);
        h.count(self.memory_bytes);
        h.count(self.object_bytes);
        h.count(self.memory_limit);
        h.bytes(&[self.phase as u8, u8::from(self.full), self.sweep_arena]);
        h.small(self.epoch);
        h.small(self.sweep_pos);
        h.count(self.debt);
        h.count(self.gc_threshold);
        h.small(self.weak_work);
        h.count(self.weak_pos);
        let mut weak = self.weak_head;
        while let Some(t) = self.tables.live(weak) {
            h.small(weak);
            weak = t.weak_next;
        }
        let (kind, slot, pos) = match self.cursor {
            None => (0u8, 0u32, 0usize),
            Some(Cursor::TableEntries {
                slot,
                pos,
                weak_k,
                weak_v,
            }) => (1 + u8::from(weak_k) + 2 * u8::from(weak_v), slot, pos),
            Some(Cursor::TableArray { slot, pos }) => (5, slot, pos),
            Some(Cursor::Closure { slot, pos }) => (6, slot, pos),
            Some(Cursor::Proto { slot, pos }) => (7, slot, pos),
            Some(Cursor::Native { slot, pos }) => (8, slot, pos),
        };
        h.bytes(&[kind]);
        h.pair(slot, u32::try_from(pos).unwrap_or(u32::MAX));
        h.count(self.graveyard.len());
        for g in &self.graveyard {
            let kind = match g {
                Grave::Index(_) => 0,
                Grave::Locals(_) => 1,
                Grave::UpvalueNames(_) => 2,
                Grave::Constants(_) => 3,
                Grave::Children(children) => {
                    for child in children.as_slice() {
                        digest_drop_tree(h, child);
                    }
                    4
                }
                Grave::Release(bytes) => {
                    h.count(*bytes);
                    5
                }
            };
            h.bytes(&[kind]);
            h.count(g.len());
        }
        h.count(self.intern.buckets.len());
        h.count(self.graveyard.capacity());
        let stats = self.stats;
        for count in [
            stats.steps,
            stats.cycles,
            stats.discarded_host_natives,
            stats.max_step_units,
            stats.total_units,
            stats.full_collections,
            stats.forced_collections,
            stats.deferred_requests,
            stats.deferred_steps,
        ] {
            h.small(count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed accounting sizes must bound the real 64-bit layouts.
    #[test]
    fn accounting_sizes_bound_real_layouts() {
        use std::mem::size_of;
        assert!(size_of::<Value>() <= VALUE_BYTES);
        assert!(size_of::<Grave>() <= GRAVE_BYTES);
        assert!(size_of::<MapEntry>() <= MAP_ENTRY_BYTES);
        assert!(size_of::<Slot<GcString>>() <= STRING_SLOT_BYTES);
        assert!(size_of::<Slot<LuaTable>>() <= TABLE_SLOT_BYTES);
        assert!(size_of::<Slot<LoadedProto>>() <= PROTO_SLOT_BYTES);
        assert!(size_of::<Slot<LuaClosure>>() <= CLOSURE_SLOT_BYTES);
        assert!(size_of::<Slot<NativeRecord<()>>>() <= NATIVE_SLOT_BYTES);
        assert!(size_of::<Slot<GcUpval>>() <= UPVAL_SLOT_BYTES);
        assert!(size_of::<Slot<GcUserdata>>() <= USERDATA_SLOT_BYTES);
    }

    /// During a sweep an object the mark did not reach is dead before its slot is
    /// freed: every getter of a handle refuses it, as `is_live_value` does, and an
    /// object the mark reached stays readable.
    #[test]
    fn getters_refuse_condemned_objects() {
        let mut heap: GcHeap<()> = GcHeap::try_new(alloc_vm_id(), 1 << 20).unwrap();
        let dead_t = heap.alloc_table().unwrap();
        let live_t = heap.alloc_table().unwrap();
        let dead_s = heap.intern_bytes(b"condemned string").unwrap();
        let dead_u = heap.alloc_userdata(1, Wide::from_u32(2), None).unwrap();
        let dead_f = heap.alloc_native(NativeKind::Fn(|_| Ok(())), &[]).unwrap();
        heap.begin_cycle(false);
        heap.mark_table(live_t);
        heap.propagate(u32::MAX);
        heap.finish_mark_slice(u32::MAX);
        assert_eq!(heap.phase(), GcPhase::Sweep);
        assert!(heap.get_table(dead_t).is_none());
        assert!(heap.get_string_bytes(dead_s).is_none());
        assert!(heap.get_userdata(dead_u).is_none());
        assert!(heap.get_native(dead_f).is_none());
        assert!(!heap.is_live_value(Value::Table(dead_t)));
        assert!(heap.get_table(live_t).is_some());
        heap.sweep(u32::MAX);
        assert!(heap.get_table(live_t).is_some());
        assert!(heap.get_table(dead_t).is_none());
    }

    #[test]
    fn strengthening_a_weak_table_during_mark_preserves_its_remaining_values() {
        let mut heap: GcHeap<()> = GcHeap::try_new(alloc_vm_id(), 1 << 20).unwrap();
        let table = heap.alloc_table().unwrap();
        let meta = heap.alloc_table().unwrap();
        let child = heap.alloc_table().unwrap();
        heap.tick = Some(CollectorTick::new(1, heap.memory_bytes));
        heap.table_mut(meta).mode = MODE_WEAK_VALUES;
        for _ in 0..4 {
            heap.set_table_metatable(table, Some(meta));
            for index in 1..=3 {
                heap.table_raw_set(table, Value::Number(index as f32), Value::Table(child), 20)
                    .unwrap();
            }
            heap.begin_cycle(true);
            heap.mark_table(table);
            heap.propagate(u32::MAX);
            assert_eq!(heap.finish_paced_mark(1), 1);
            assert!(heap.table(table).raw_get(Value::Number(3.)).is_nil());
            heap.set_table_metatable(table, None);
            assert_eq!(heap.phase(), GcPhase::Recolor);
            while heap.phase() == GcPhase::Recolor {
                assert!(heap.sweep(3).0 <= 3);
            }
        }
        heap.begin_cycle(true);
        heap.mark_table(table);
        heap.propagate(u32::MAX);
        heap.finish_mark_slice(u32::MAX);
        heap.sweep(u32::MAX);
        assert!(heap.get_table(child).is_some());
        assert!(heap.get_table(meta).is_none());
    }

    #[test]
    fn paced_weak_cleanup_survives_compaction_and_array_growth() {
        let mut heap = GcHeap::<()>::try_new(1, 1 << 20).unwrap();
        let table = heap.alloc_table().unwrap();
        let meta = heap.alloc_table().unwrap();
        heap.table_mut(meta).mode = MODE_WEAK_VALUES;
        heap.set_table_metatable(table, Some(meta));
        for i in (2..=120).rev() {
            let child = heap.alloc_table().unwrap();
            heap.table_raw_set(
                table,
                Value::Number(i as f32),
                Value::Table(child),
                u32::MAX,
            )
            .unwrap();
        }
        heap.tick = Some(CollectorTick::new(1, heap.memory_bytes));
        heap.begin_cycle(true);
        heap.mark_table(table);
        heap.propagate(u32::MAX);
        assert_eq!(heap.finish_paced_mark(48), 48);
        // Appending index one migrates numeric map entries into the array.
        let kept = heap.alloc_table().unwrap();
        heap.table_raw_set(table, Value::Number(1.), Value::Table(kept), u32::MAX)
            .unwrap();
        while heap.phase() == GcPhase::Mark {
            let (_, done) = heap.propagate(8);
            if done {
                assert!(heap.finish_paced_mark(8) <= 8);
            }
        }
        heap.sweep(u32::MAX);
        assert_eq!(
            heap.table(table).raw_get(Value::Number(1.)).as_table(),
            Some(kept)
        );
        for i in 2..=120 {
            assert!(heap.table(table).raw_get(Value::Number(i as f32)).is_nil());
        }
    }

    #[test]
    fn canonical_bits_maps_every_nan_and_nothing_else() {
        for bits in [0x7FC0_0000u32, 0xFFC0_0000, 0x7F80_0001, 0xFFFF_FFFF, 0x7FFF_FFFF] {
            assert_eq!(canonical_bits(bits), CANONICAL_NAN_BITS);
        }
        for bits in [0u32, 0x8000_0000, 0x7F80_0000, 0xFF80_0000, 0x3F80_0000, 0x7F7F_FFFF] {
            assert_eq!(canonical_bits(bits), bits);
        }
    }
}

#[cfg(test)]
mod collection_accounting_tests {
    use super::*;

    #[test]
    fn outstanding_destruction_work_is_a_running_total() {
        let mut heap = GcHeap::<()>::try_new(1, 1 << 20).unwrap();
        heap.reserve_graves(1).unwrap();
        let base = heap.full_collection_bound();
        let names = vec![Box::<[u8]>::default(); 255];
        heap.queue_grave(Grave::UpvalueNames(names.into_iter()));
        assert_eq!(heap.full_collection_bound() - base, 256);
        for left in (0..4).rev() {
            assert_eq!(heap.drain_graveyard(64), 64);
            assert_eq!(heap.full_collection_bound() - base, left * 64);
        }
    }
}

#[cfg(test)]
mod intern_index_tests {
    use super::*;

    #[test]
    fn unlinking_a_known_string_preserves_other_members_of_its_chain() {
        let mut strings = Arena::new();
        let mut index = InternIndex::new();
        let hash = 17;
        for n in 0..33u8 {
            let slot = strings.slots.len() as u32;
            index.buckets.resize(index.capacity_for(slot), NONE);
            strings.insert(
                GcString {
                    bytes: Rc::from([n]),
                    hash,
                    prev: NONE,
                },
                1,
            );
            index.insert(&mut strings, slot, hash);
        }
        for n in [0u8, 9, 15, 16, 28, 31, 32] {
            index.remove(&mut strings, n.into());
            strings.free(n.into());
            assert_eq!(index.find(&strings, hash, &[n]), None);
        }
        for n in [1u8, 8, 10, 17, 27, 30] {
            assert_eq!(index.find(&strings, hash, &[n]), Some(n.into()));
        }
    }
}
