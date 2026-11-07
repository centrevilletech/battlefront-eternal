//! Publication and the post-step reads: what the host takes out of the
//! world after a tick (the packets, the typed mirror banks, the events) and
//! the remaining command bodies it still drives one at a time.
use super::super::context::{Context, Error, ErrorKind};
use crate::gen::WirePublicationOutput;
use crate::store::block::Block;
use crate::world::owner::World;

/// One publication: the `WirePublicationOutput` block and the encoder bytes
/// its spans point into. Both are owned copies, valid after the next call
/// and after disposal. The block's `bytes` word is 0: every span's offset is
/// relative to `bytes`, where the WebAssembly export gives a module address.
#[derive(Default)]
pub struct Publication {
    pub block: Block,
    pub bytes: Vec<u8>,
}
impl Publication {
    /// The decoded block, for a host that reads fields rather than rows.
    pub fn output(&self) -> WirePublicationOutput {
        let (row, heap) = self.block.read(0);
        WirePublicationOutput::take(row, heap)
    }
}

/// The retained banks a host reads after a step, numbered for `read_bank`.
/// Each is what the matching WebAssembly descriptor names by address: the
/// descriptors themselves (their addresses zero natively) and the blocks
/// they point to. Reading a bank before its owner is prepared gives no
/// words, as the export gives null.
pub mod bank {
    /// The soldiers' publication rows (`F3Boundary.output`), a block.
    pub const F3_PUBLICATION: u32 = 1;
    /// The hulls' toggled and placed bodies since the last read
    /// (`veh_changes`); reading takes them, as the export does.
    pub const F2_DELTA: u32 = 2;
    /// The battle's phase descriptor (`F4Phase`), publishing first.
    pub const F4_PHASE: u32 = 3;
    /// The AI boundary descriptor (`AiPhaseBoundary`).
    pub const AI_BOUNDARY: u32 = 4;
    /// The last publication's allocation census (`WireCensus`).
    pub const WIRE_CENSUS: u32 = 5;
    /// The phase's events (`sim_events`), until `clear_events`.
    pub const EVENTS: u32 = 6;
    /// The soldiers' descriptor (`F3Boundary`), publishing first.
    pub const F3_DESCRIPTOR: u32 = 7;
    /// The soldiers' random streams (`F3RandomState` rows).
    pub const F3_RANDOM_STATES: u32 = 8;
    /// The random streams' revision (`F3RandomEpoch`).
    pub const F3_RANDOM_EPOCH: u32 = 9;
    /// The shared random globals (`F3RandomGlobals`).
    pub const F3_RANDOM_GLOBALS: u32 = 10;
    /// The aim changes' header (`F3AimHeader`).
    pub const F3_AIM_HEADER: u32 = 11;
    /// The foreign props' header (`F3ForeignHeader`).
    pub const F3_FOREIGN_HEADER: u32 = 12;
    /// The soldiers' bounds cache.
    pub const F3_BOUNDS_CACHE: u32 = 13;
    /// The soldiers' acknowledgement row (`F3Acknowledgement`).
    pub const F3_ACKNOWLEDGEMENT: u32 = 14;
    /// The hulls' phase descriptor (`F2Phase`).
    pub const F2_PHASE: u32 = 15;
    /// The buildings' publication, a block.
    pub const F4_BUILDINGS: u32 = 16;
    /// The animated props' publication, a block.
    pub const F4_ANIMATED: u32 = 17;
    /// The traps' publication, a block.
    pub const F4_TRAPS: u32 = 18;
    /// The battle core's publication, a block.
    pub const F4_BATTLE: u32 = 19;
    /// The battle's counters (`F4Counters`).
    pub const F4_COUNTERS: u32 = 20;
    /// The AI phase's counters (`AiPhaseCounters`).
    pub const AI_COUNTERS: u32 = 21;
    /// The AI's published control views (`AiControlView` rows).
    pub const AI_CONTROLS: u32 = 22;
    /// The hulls' publication, a block.
    pub const F2_HULLS: u32 = 23;
    /// The pads' publication, a block.
    pub const F2_PADS: u32 = 24;
    /// The hulls' lives, a block.
    pub const F2_LIVES: u32 = 25;
    /// The hulls' posed geometry, a block.
    pub const F2_POSES: u32 = 26;
    /// The seats' descriptor (`F2SeatIo`).
    pub const F2_SEATS: u32 = 27;
    /// The firing phase's descriptor (`FfPhase`).
    pub const F2_FIRING: u32 = 28;
    /// Every bank, in numbering order.
    pub const ALL: [u32; 28] = [
        F3_PUBLICATION, F2_DELTA, F4_PHASE, AI_BOUNDARY, WIRE_CENSUS, EVENTS, F3_DESCRIPTOR,
        F3_RANDOM_STATES, F3_RANDOM_EPOCH, F3_RANDOM_GLOBALS, F3_AIM_HEADER, F3_FOREIGN_HEADER,
        F3_BOUNDS_CACHE, F3_ACKNOWLEDGEMENT, F2_PHASE, F4_BUILDINGS, F4_ANIMATED, F4_TRAPS,
        F4_BATTLE, F4_COUNTERS, AI_COUNTERS, AI_CONTROLS, F2_HULLS, F2_PADS, F2_LIVES, F2_POSES,
        F2_SEATS, F2_FIRING,
    ];
}

/// Why `run_operation` gave no block.
#[derive(Clone, Debug)]
pub enum Fault {
    /// The context refused the call: disposed, poisoned, busy or unavailable.
    Context(Error),
    /// `op` names no operation in the generated table.
    UnknownOperation(u32),
    /// `args` is not the operation's argument row: `expected` is its scalars
    /// plus its record's row, the least a record with a heap can take.
    Arguments { expected: usize, given: usize },
    /// The body refused its arguments, as the export answers with null.
    Refused,
}
impl From<Error> for Fault {
    fn from(error: Error) -> Self {
        Self::Context(error)
    }
}

/// The words of one bank, borrowed from its owner; None before the owner is
/// prepared. The two descriptors publish first, as their exports do.
fn bank_words(w: &mut World, bank: u32) -> Option<&[f64]> {
    use crate::{ai::owned::entry as ai, battle::{f4_core, f4_live}, units::f3, vehicles::module as fleet};
    match bank {
        bank::F3_PUBLICATION => f3::output_bank_in(w),
        bank::F2_DELTA => fleet::veh_changes_in(w),
        bank::F4_PHASE => { f4_live::counters_bank_in(w)?; f4_live::f4_live_publish_in(w) }
        bank::AI_BOUNDARY => ai::ai_world_boundary_in(w),
        bank::WIRE_CENSUS => w.wire.as_deref().map(|owner| owner.census.result()),
        bank::EVENTS => Some(w.sim.events.words()),
        bank::F3_DESCRIPTOR => f3::descriptor_bank_in(w),
        bank::F3_RANDOM_STATES => f3::random_states_bank_in(w),
        bank::F3_RANDOM_EPOCH => f3::random_epoch_bank_in(w),
        bank::F3_RANDOM_GLOBALS => f3::random_globals_bank_in(w),
        bank::F3_AIM_HEADER => f3::aim_header_bank_in(w),
        bank::F3_FOREIGN_HEADER => f3::foreign_header_bank_in(w),
        bank::F3_BOUNDS_CACHE => f3::bounds_cache_bank_in(w),
        bank::F3_ACKNOWLEDGEMENT => f3::acknowledgement_bank_in(w),
        bank::F2_PHASE => Some(fleet::f2::descriptor_bank_in(w)),
        bank::F4_BUILDINGS => f4_live::buildings_bank_in(w),
        bank::F4_ANIMATED => f4_live::animated_bank_in(w),
        bank::F4_TRAPS => f4_live::traps_bank_in(w),
        bank::F4_BATTLE => f4_core::publication_bank_in(&mut w.battle),
        bank::F4_COUNTERS => f4_live::counters_bank_in(w),
        bank::AI_COUNTERS => ai::ai_counters_bank_in(w),
        bank::AI_CONTROLS => ai::ai_controls_bank_in(w),
        bank::F2_HULLS => fleet::f2::hulls_bank_in(w),
        bank::F2_PADS => fleet::f2::pads_bank_in(w),
        bank::F2_LIVES => fleet::f2::lives_bank_in(w),
        bank::F2_POSES => fleet::f2::poses_bank_in(w),
        bank::F2_SEATS => fleet::f2::seats_bank_in(w),
        bank::F2_FIRING => fleet::f2::firing_bank_in(w),
        _ => None,
    }
}

/// The operation `op` names, once `args` has its shape.
fn shape(op: u32, args: &[f64]) -> Result<&'static crate::gen::Operation, Fault> {
    let operation = crate::gen::operation(op).ok_or(Fault::UnknownOperation(op))?;
    let expected = operation.scalars + operation.record;
    let fits = if operation.heap { args.len() >= expected } else { args.len() == expected };
    if !fits {
        return Err(Fault::Arguments { expected, given: args.len() });
    }
    Ok(operation)
}

impl Context {
    /// The shared `wire_publish_in` body for one publication session (a pool
    /// key from `wire_publish_create`), into the host's retained publication:
    /// `out` keeps its block and byte capacity, so a host publishing every
    /// tick allocates once. `facts` is the `WireWorldFacts` row the host
    /// stamps before publishing (tick, time, wall clock, dropped ticks and
    /// the links' round trips), with its links' heap words.
    pub fn world_publish_into(&self, session: u32, facts: &[f64], out: &mut Publication) -> Result<(), Error> {
        self.owned(|w| {
            crate::wire::publish_module::wire_publish_native_in(w, session as usize, facts).map(|(words, bytes)| {
                out.block.words.clear();
                out.block.words.extend_from_slice(words);
                out.block.heap.clear();
                out.bytes.clear();
                out.bytes.extend_from_slice(bytes);
            })
        })?
        .ok_or_else(|| Error::new(ErrorKind::InvalidKind, "no such publication session"))
    }

    /// `world_publish_into` into a new publication, for a host that keeps
    /// none.
    pub fn world_publish(&self, session: u32, facts: &[f64]) -> Result<Publication, Error> {
        let mut out = Publication::default();
        self.world_publish_into(session, facts, &mut out)?;
        Ok(out)
    }

    /// Copies one post-step bank (`bank::*`) into `out`, replacing its
    /// contents, and answers with the word count: zero before the bank's
    /// owner is prepared. `out` keeps its capacity across ticks, so a host
    /// reading every tick allocates once.
    pub fn read_bank(&self, bank: u32, out: &mut Vec<f64>) -> Result<usize, Error> {
        if !bank::ALL.contains(&bank) {
            return Err(Error::new(ErrorKind::InvalidKind, "no such bank"));
        }
        self.owned(|w| {
            out.clear();
            if let Some(words) = bank_words(w, bank) {
                out.extend_from_slice(words);
            }
            out.len()
        })
    }

    /// One body from the generated operation table (`OPERATION_*`, gen), with
    /// its argument row in `args`: the scalars the export takes, then the
    /// record the host writes into the call block, row then heap. The words
    /// the export would return a pointer to replace `out`'s, which keeps its
    /// capacity; an operation without a result leaves `out` empty. Wire
    /// handles cross as pool keys.
    pub fn run_operation_into(&self, op: u32, args: &[f64], out: &mut Block) -> Result<(), Fault> {
        let operation = shape(op, args)?;
        let answered = self.owned(|w| {
            out.words.clear();
            out.heap.clear();
            match crate::gen::run_operation(w, op, args) {
                Some(words) => {
                    out.words.extend_from_slice(words);
                    true
                }
                None => false,
            }
        })?;
        if operation.answers && !answered {
            return Err(Fault::Refused);
        }
        Ok(())
    }

    /// `run_operation_into` into a new block, for a host that keeps none.
    pub fn run_operation(&self, op: u32, args: &[f64]) -> Result<Block, Fault> {
        let mut out = Block::default();
        self.run_operation_into(op, args, &mut out)?;
        Ok(out)
    }
}
