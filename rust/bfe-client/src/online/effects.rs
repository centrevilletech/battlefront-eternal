//! Effects.play and the handle it returns (src/effects.js) over the shared
//! Presentation owner. A handle moves or stops its effect later (a trail
//! follows a flying piece); a one-shot never looks at its handle again.
//! Slots are a bounded pool: a finished effect returns its slot, and a
//! handle from an earlier life of that slot no longer names anything.
use bfe_presentation::{
    layout::{command as c, command_kind as k, effect as eo, frame as f, header as h},
    Presentation,
};
const SLOTS: usize = 1024;
const COMMANDS: usize = 2048;
const GENERATION_BITS: u32 = 22;
pub struct Spawner {
    pub owner: Presentation,
    // Which slots hold a playing effect, each slot's current life, and the
    // slots free to lend. Slot 0 is never lent.
    active: Vec<bool>,
    generation: Vec<u32>,
    free: Vec<usize>,
    // Commands apply in the order they were written.
    sequence: u32,
    // Where each effect was last put and how fast it moved, which a move
    // without a velocity and a stop both keep.
    placed: Vec<[f64; 6]>,
}
impl Spawner {
    pub fn new(owner: Presentation) -> Self {
        Self { owner, active: vec![false; SLOTS], generation: vec![0; SLOTS], free: (1..SLOTS).rev().collect(), sequence: 0, placed: vec![[0.; 6]; SLOTS] }
    }
    // The slot's number and the low bits of its life, in one word.
    fn handle(&self, id: usize) -> u32 {
        id as u32 | (self.generation[id] & ((1 << GENERATION_BITS) - 1)) << 10
    }
    // The slot a handle still names: none once its effect finished, even if
    // the slot plays another since.
    fn live(&self, handle: u32) -> Option<usize> {
        let id = handle as usize & (SLOTS - 1);
        (self.active[id] && self.handle(id) == handle).then_some(id)
    }
    // One command row of this frame's batch, with an effect's standing
    // fields set as a new Effect has them: unturned, full tint and size.
    fn command(&mut self, operation: u32, id: usize) -> Option<&mut [f64]> {
        let count = self.owner.input[f::COMMANDS] as usize;
        if count == COMMANDS {
            return None;
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.owner.input[f::COMMANDS] = (count + 1) as f64;
        let tick = self.owner.input[f::PLAYOUT_TICK];
        let row = &mut self.owner.input[f::STRIDE + count * c::STRIDE..][..c::STRIDE];
        row.fill(0.);
        row[c::SEQUENCE] = self.sequence as f64;
        row[c::KIND] = operation as f64;
        row[c::ID] = id as f64;
        row[c::GENERATION] = self.generation[id] as f64;
        row[c::TICK] = tick;
        row[c::QUATERNION + 3] = 1.;
        row[c::TINT] = 1.;
        row[c::SCALE] = 1.;
        Some(row)
    }
    /// Effects.play: the effect group at a point, moving at `velocity`. The
    /// seed keeps its particle draws deterministic. None when the frame's
    /// batch or the pool is full.
    pub fn start(&mut self, group: usize, position: [f64; 3], velocity: [f64; 3], seed: u32) -> Option<u32> {
        if self.owner.input[f::COMMANDS] as usize == COMMANDS {
            return None;
        }
        let id = self.free.pop()?;
        // A new life of the slot: handles from its earlier lives go stale.
        self.active[id] = true;
        self.generation[id] = self.generation[id].wrapping_add(1).max(1);
        self.placed[id][..3].copy_from_slice(&position);
        self.placed[id][3..].copy_from_slice(&velocity);
        let row = self.command(k::SPAWN, id)?;
        row[c::CONTENT] = group as f64;
        row[c::SEED] = seed as f64;
        row[c::POSITION..c::POSITION + 3].copy_from_slice(&position);
        row[c::VELOCITY..c::VELOCITY + 3].copy_from_slice(&velocity);
        Some(self.handle(id))
    }
    /// Effect.move: the effect follows its owner. Without a velocity it
    /// keeps the one it had.
    pub fn sync(&mut self, handle: u32, position: [f64; 3], velocity: Option<[f64; 3]>) {
        let Some(id) = self.live(handle) else {
            return;
        };
        self.placed[id][..3].copy_from_slice(&position);
        if let Some(velocity) = velocity {
            self.placed[id][3..].copy_from_slice(&velocity);
        }
        self.place(id, false);
    }
    /// Effect.stop: it emits no more; what is out already lives on.
    pub fn stop(&mut self, handle: u32) {
        if let Some(id) = self.live(handle) {
            self.place(id, true);
        }
    }
    fn place(&mut self, id: usize, stopped: bool) {
        let placed = self.placed[id];
        if let Some(row) = self.command(k::SYNC, id) {
            row[c::POSITION..c::POSITION + 3].copy_from_slice(&placed[..3]);
            row[c::VELOCITY..c::VELOCITY + 3].copy_from_slice(&placed[3..]);
            row[c::STOPPED] = stopped as u8 as f64;
        }
    }
    /// Effects.update: step every effect, then take back the slots of the
    /// ones that finished in this life.
    pub fn frame(&mut self) {
        self.owner.frame();
        for row in self.owner.effect_output.chunks_exact(eo::STRIDE).take(self.owner.header[h::EFFECTS] as usize) {
            let id = row[eo::ID] as usize;
            if row[eo::DONE] != 0. && id < SLOTS && self.active[id] && row[eo::GENERATION] == self.generation[id] as f64 {
                self.active[id] = false;
                self.free.push(id);
            }
        }
    }
}
