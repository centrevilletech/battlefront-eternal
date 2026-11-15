//! The store's exports: what JavaScript calls to move state in and out of
//! the instance (src/sim/runtime.js `SimBridge`), and the memory policy.
//! Plain `extern "C"` functions taking and returning numbers and pointers
//! into the module's memory, like the collision's.
//!
//! JavaScript writes what it hands over (a string's UTF-16 units) into the
//! scratch buffer (`sim_scratch`) and reads a kind's rows from its block.
//! Any call here can grow the memory, so JavaScript makes its views after
//! the call.

pub mod context;
pub mod context_step;

use crate::gen::{Echo, Fill, Store, KINDS, KIND_HASHES};
use crate::store::block::Block;
use crate::store::calls::{Calls, Events};
use crate::store::{KindCode, StrId, Strings};

/// The version of these exports and of the block format (src/sim/runtime.js
/// checks it).
pub const ABI: u32 = 1;

/// A native tick must resolve every gameplay service from its owners.
/// An unserved host callback is a deterministic failure, never a second path.
#[cold]
pub fn no_native_host(name: &str) -> ! { panic!("native host import unavailable: {name}") }

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
    pub const fn cold() -> Self {
        Self { store: Store::cold(), strings: Strings::new(), blocks: Vec::new(),
            calls: Calls::new(), events: Events::cold(), scratch: Vec::new() }
    }
    pub(crate) fn warm(&mut self) {
        self.blocks = (0..=KINDS).map(|_| Block::default()).collect();
        self.events.warm();
    }
    pub fn new() -> Self { let mut sim = Self::cold(); sim.warm(); sim }

}

impl Default for Sim {
    fn default() -> Self { Self::new() }
}

impl Sim {
    pub fn scratch_room(&mut self, bytes: usize) -> &mut [u8] {
        self.scratch.clear();
        self.scratch.resize(bytes, 0);
        &mut self.scratch
    }

    /// Cold page readers may borrow only the currently reserved scratch span.
    /// Bounds are checked before slicing; arbitrary page pointers are refused.
    pub fn scratch_bytes(&self, ptr: *const u8, len: usize) -> Option<&[u8]> {
        let start=(ptr as usize).checked_sub(self.scratch.as_ptr() as usize)?;
        self.scratch.get(start..start.checked_add(len)?)
    }

    pub fn intern_scratch(&mut self, units: usize) -> StrId {
        let list: Vec<u16> = self.scratch[..units * 2].chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        self.strings.intern(&list)
    }

    pub fn export(&mut self, kind: u32, mode: u32) -> Option<&Block> {
        let block = self.blocks.get_mut(kind as usize)?;
        self.store.export(kind as KindCode, block, mode).then_some(block)
    }

    pub fn import(&mut self, kind: u32, flags: u32) -> bool {
        let Some(block) = self.blocks.get_mut(kind as usize) else { return false };
        self.store.import(kind as KindCode, block, flags & 1 != 0, flags & 2 != 0)
    }
}


// Native callers use Context and explicit owners. These raw WASM entries
// cannot acquire a process-global simulation in a native host.
pub(crate) fn cold() -> &'static mut crate::world::owner::World {
    panic!("WASM facade called by native host")
}

pub fn sim_abi() -> u32 { ABI }

/// How many kinds the module knows, and each one's layout hash: the
/// generated JavaScript has the same (src/sim/gen/index.js), and a bridge
/// refuses a module whose kinds were generated from other schemas.
pub fn sim_kinds() -> u32 { KINDS as u32 }

pub fn sim_kind_hash(kind: u32) -> u32 { KIND_HASHES.get(kind as usize).copied().unwrap_or(0) }



pub(crate) fn sim_str_count_in(w: &mut crate::world::owner::World) -> u32 { w.sim.strings.list.len() as u32 }

pub(crate) fn sim_str_len_in(w: &mut crate::world::owner::World, id: u32) -> u32 { w.sim.strings.units(StrId(id)).len() as u32 }

pub(crate) fn sim_str_ptr_in(w: &mut crate::world::owner::World, id: u32) -> &[u16] { w.sim.strings.units(StrId(id)) }

/// Writes a kind's export block and returns where it starts, or null for a
/// kind the store holds no fields of. `mode` 0: a row for each entity with
/// its mirrored fields, after the rows of the entities retired since the
/// last such export (which it hands over: they are not listed again); 1:
/// every field of each live entity; 2: as 0 without the retired (what the
/// mirror write detector compares, leaving them for the next export).
pub(crate) fn sim_export_in(w: &mut crate::world::owner::World, kind: u32, mode: u32) -> Option<&Block> { w.sim.export(kind, mode) }

/// Retires an entity (`Slab::retire`); 0, or -1 if it is gone or of a kind
/// the store holds no slab of.
pub(crate) fn sim_retire_in(w: &mut crate::world::owner::World, kind: u32, slot: u32, gen: u32) -> i32 { if w.sim.store.retire(kind as KindCode, slot, gen) { 0 } else { -1 } }

/// Room for an import block of a kind: `rows` rows of `stride` words, then
/// `heap` heap words.
pub(crate) fn sim_import_room_in(w: &mut crate::world::owner::World, kind: u32, rows: u32, stride: u32, heap: u32) -> &mut Block { let b = &mut w.sim.blocks[kind as usize]; b.room(rows as usize, stride as usize, heap as usize); b }

/// Takes in the import block JavaScript wrote: its import fields, or every
/// field when `flags` has 1; with 2, the block holds some of the kind's
/// entities and the store keeps the others as they are (without it, an
/// entity the block does not list leaves the store, as one that left the
/// JavaScript's list). 0, or -1 for a kind the store holds no fields of.
pub(crate) fn sim_import_in(w: &mut crate::world::owner::World, kind: u32, flags: u32) -> i32 { if w.sim.import(kind, flags) { 0 } else { -1 } }


/// The call block (an import's arguments, as Rust wrote them).
pub(crate) fn sim_calls_in(w: &mut crate::world::owner::World) -> &Block { &w.sim.calls.block }

/// Runs command `code` on the arguments in the call block: where its result
/// is (the call block), or null for no such command.
pub(crate) fn sim_command_in(w: &mut crate::world::owner::World, code: u32) -> Option<&Block> { if crate::gen::run_command(&mut w.sim, code) { Some(&w.sim.calls.block) } else { None } }

/// The import block's size in words (src/sim/runtime.js writes a result
/// there itself, up to this).
pub fn sim_import_capacity() -> u32 { crate::store::calls::EXCHANGE as u32 }

/// The events emitted since they were last cleared: their words' count,
/// then each event's code, length and words.
pub(crate) fn sim_events_in(w: &mut crate::world::owner::World) -> &[f64] { w.sim.events.words() }

pub(crate) fn sim_events_clear_in(w: &mut crate::world::owner::World) -> () { w.sim.events.clear() }

/// Legacy raw exports have no native callback owner. Context commands use
/// their explicit dispatcher instead; unsupported raw commands return null.
pub fn command_available(code: u32) -> bool {
    // Relay asks the host back; only Context::command may dispatch a callback.
    { code != crate::gen::COMMAND_RELAY }
}

/// The store's own command (rust/sim/schema/store.mjs), for its tests and
/// benchmarks: its arguments back, `value` one more, after emitting
/// `events` echoed events.
pub fn echo(sim: &mut Sim, args: Echo) -> Echo {
    for i in 0..args.events {
        Echo { value: i as f64, ..args.clone() }.emit(&mut sim.events);
    }
    Echo { value: args.value + 1.0, ..args }
}

/// The store's third command: asks JavaScript (the import `ask`) and gives
/// back its answer, so the module's path for imports is held to what the
/// generator makes.
pub fn relay(_sim: &mut Sim, _args: Echo) -> Echo {
    panic!("native relay requires abi::context::Context");
}

/// The store's other command: `count` new entities of a kind, its fields'
/// defaults, at the end of its home list (tests and benchmarks).
pub fn fill(sim: &mut Sim, args: Fill) { sim.store.fill(args.kind as KindCode, args.count as usize); }

/// Makes room for at least `bytes` more bytes of the battle's data now,
/// as it loads, so playing it does not grow the memory (which would detach
/// every view JavaScript holds): blocks of 64 KB are allocated and freed
/// again, and the allocator keeps them for what comes later. Returns the
/// memory's size in 64 KB pages.
pub fn sim_reserve(bytes: u32) -> u32 {
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
pub fn sim_memory_pages() -> u32 { memory_pages() }


fn memory_pages() -> u32 { 0 }

/// Store-only reset for tool fixtures: finish AI and replace generated rows.
/// Collision, movement, pose, phase owners, wire and page resources retain
/// their state. A new battle needs a new World or WASM instance.
pub(crate) fn sim_reset_in(w: &mut crate::world::owner::World) -> () { w.ai.finish(); w.sim.store = Store::default(); }

