//! The store's exports: what JavaScript calls to move state in and out of
//! the instance (src/sim/runtime.js `SimBridge`), and the memory policy.
//! Plain `extern "C"` functions taking and returning numbers and pointers
//! into the module's memory, like the collision's.
//!
//! JavaScript writes what it hands over (a string's UTF-16 units) into the
//! scratch buffer (`sim_scratch`) and reads a kind's rows from its block.
//! Any call here can grow the memory, so JavaScript makes its views after
//! the call.

use std::ptr::addr_of_mut;

use crate::gen::{Echo, Fill, Store, KINDS, KIND_HASHES};
use crate::store::block::Block;
use crate::store::calls::{Calls, Events};
use crate::store::{KindCode, StrId, Strings};

/// The version of these exports and of the block format (src/sim/runtime.js
/// checks it).
pub const ABI: u32 = 1;

pub struct Sim {
    pub store: Store,
    pub strings: Strings,
    /// A sync block per kind, by code.
    pub blocks: Vec<Block>,
    /// Where a command's arguments and result (and an import's) cross.
    pub calls: Calls,
    /// The phase's events, drained by JavaScript at its end.
    pub events: Events,
    scratch: Vec<u8>,
}

impl Sim {
    fn new() -> Self {
        Sim {
            store: Store::default(),
            strings: Strings::default(),
            blocks: (0..=KINDS).map(|_| Block::default()).collect(),
            calls: Calls::default(),
            events: Events::default(),
            scratch: Vec::new(),
        }
    }
}

static mut SIM: Option<Sim> = None;

/// The instance's store. One thread: JavaScript calls in one at a time.
pub fn sim() -> &'static mut Sim {
    unsafe { (*addr_of_mut!(SIM)).get_or_insert_with(Sim::new) }
}

#[no_mangle]
pub extern "C" fn sim_abi() -> u32 { ABI }

/// How many kinds the module knows, and each one's layout hash: the
/// generated JavaScript has the same (src/sim/gen/index.js), and a bridge
/// refuses a module whose kinds were generated from other schemas.
#[no_mangle]
pub extern "C" fn sim_kinds() -> u32 { KINDS as u32 }

#[no_mangle]
pub extern "C" fn sim_kind_hash(kind: u32) -> u32 { KIND_HASHES.get(kind as usize).copied().unwrap_or(0) }

/// Room for `bytes` bytes of input, where JavaScript writes it.
#[no_mangle]
pub extern "C" fn sim_scratch(bytes: u32) -> *mut u8 {
    let s = &mut sim().scratch;
    s.clear();
    s.resize(bytes as usize, 0);
    s.as_mut_ptr()
}

/// Interns the string whose `units` UTF-16 units are in the scratch buffer.
#[no_mangle]
pub extern "C" fn sim_str_intern(units: u32) -> u32 {
    let s = sim();
    let n = units as usize;
    let list: Vec<u16> = s.scratch[..n * 2].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    s.strings.intern(&list).0
}

#[no_mangle]
pub extern "C" fn sim_str_count() -> u32 { sim().strings.list.len() as u32 }

#[no_mangle]
pub extern "C" fn sim_str_len(id: u32) -> u32 { sim().strings.units(StrId(id)).len() as u32 }

#[no_mangle]
pub extern "C" fn sim_str_ptr(id: u32) -> *const u16 { sim().strings.units(StrId(id)).as_ptr() }

/// Writes a kind's export block and returns where it starts, or null for a
/// kind the store holds no fields of. `mode` 0: a row for each entity with
/// its mirrored fields, after the rows of the entities retired since the
/// last such export (which it hands over: they are not listed again); 1:
/// every field of each live entity; 2: as 0 without the retired (what the
/// mirror write detector compares, leaving them for the next export).
#[no_mangle]
pub extern "C" fn sim_export(kind: u32, mode: u32) -> *const f64 {
    let s = sim();
    let Some(block) = s.blocks.get_mut(kind as usize) else { return std::ptr::null() };
    if s.store.export(kind as KindCode, block, mode) { block.ptr() } else { std::ptr::null() }
}

/// Retires an entity (`Slab::retire`); 0, or -1 if it is gone or of a kind
/// the store holds no slab of.
#[no_mangle]
pub extern "C" fn sim_retire(kind: u32, slot: u32, gen: u32) -> i32 {
    if sim().store.retire(kind as KindCode, slot, gen) { 0 } else { -1 }
}

/// Room for an import block of a kind: `rows` rows of `stride` words, then
/// `heap` heap words.
#[no_mangle]
pub extern "C" fn sim_import_room(kind: u32, rows: u32, stride: u32, heap: u32) -> *mut f64 {
    sim().blocks[kind as usize].room(rows as usize, stride as usize, heap as usize)
}

/// Takes in the import block JavaScript wrote (its import fields, or
/// every field when `full` is 1); 0, or -1 for a kind the store holds no
/// fields of.
#[no_mangle]
pub extern "C" fn sim_import(kind: u32, full: u32) -> i32 {
    let s = sim();
    let block = std::mem::take(&mut s.blocks[kind as usize]);
    let ok = s.store.import(kind as KindCode, &block, full != 0);
    s.blocks[kind as usize] = block;
    if ok { 0 } else { -1 }
}

/// Room for a call's record (`words` words, then `heap` heap words): a
/// command's arguments, or an import's result.
#[no_mangle]
pub extern "C" fn sim_call_room(words: u32, heap: u32) -> *mut f64 { sim().calls.room(words as usize, heap as usize) }

/// The call block (an import's arguments, as Rust wrote them).
#[no_mangle]
pub extern "C" fn sim_calls() -> *const f64 { sim().calls.block.ptr() }

/// Runs command `code` on the arguments in the call block: where its result
/// is (the call block), or null for no such command.
#[no_mangle]
pub extern "C" fn sim_command(code: u32) -> *const f64 {
    let s = sim();
    if crate::gen::run_command(s, code) { s.calls.block.ptr() } else { std::ptr::null() }
}

/// The events emitted since they were last cleared: their words' count,
/// then each event's code, length and words.
#[no_mangle]
pub extern "C" fn sim_events() -> *const f64 { sim().events.ptr() }

#[no_mangle]
pub extern "C" fn sim_events_clear() { sim().events.clear() }

/// The store's own command (rust/sim/schema/store.mjs), for its tests and
/// benchmarks: its arguments back, `value` one more, after emitting
/// `events` echoed events.
pub fn echo(sim: &mut Sim, args: Echo) -> Echo {
    for i in 0..args.events {
        Echo { value: i as f64, ..args.clone() }.emit(&mut sim.events);
    }
    Echo { value: args.value + 1.0, ..args }
}

/// The store's other command: `count` new entities of a kind, its fields'
/// defaults, at the end of its home list (tests and benchmarks).
pub fn fill(sim: &mut Sim, args: Fill) { sim.store.fill(args.kind as KindCode, args.count as usize); }

/// Makes room for at least `bytes` more bytes of the battle's data now,
/// as it loads, so playing it does not grow the memory (which would detach
/// every view JavaScript holds): blocks of 64 KB are allocated and freed
/// again, and the allocator keeps them for what comes later. Returns the
/// memory's size in 64 KB pages.
#[no_mangle]
pub extern "C" fn sim_reserve(bytes: u32) -> u32 {
    const BLOCK: usize = 64 * 1024 - 64;
    let mut held: Vec<Vec<u8>> = Vec::with_capacity(bytes as usize / BLOCK + 1);
    let mut made = 0usize;
    while made < bytes as usize {
        let v: Vec<u8> = Vec::with_capacity(BLOCK);
        std::hint::black_box(v.as_ptr());
        held.push(v);
        made += BLOCK;
    }
    drop(held);
    memory_pages()
}

/// The memory's size in 64 KB pages.
#[no_mangle]
pub extern "C" fn sim_memory_pages() -> u32 { memory_pages() }

#[cfg(target_arch = "wasm32")]
fn memory_pages() -> u32 { core::arch::wasm32::memory_size(0) as u32 }

#[cfg(not(target_arch = "wasm32"))]
fn memory_pages() -> u32 { 0 }

/// Empties the store (a battle gets an instance of its own; this is for
/// tests and tools that reuse one).
#[no_mangle]
pub extern "C" fn sim_reset() {
    sim().store = Store::default();
}
