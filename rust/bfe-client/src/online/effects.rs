//! Effects.play and the handle it returns over the shared
//! Presentation owner. A handle moves or stops its effect later (a trail
//! follows a flying piece); a one-shot never looks at its handle again.
//! Slots are a bounded pool: a finished effect returns its slot, and a
//! handle from an earlier life of that slot no longer names anything.
use bfe_presentation::{
    layout::{command as c, command_kind as k, effect as eo, frame as f, header as h},
    Presentation,
};
pub(super) const SLOTS: usize = 1024;
const COMMANDS: usize = 2048;
const GENERATION_BITS: u32 = 22;
pub struct Spawner {
    pub owner: Presentation,
    decoration: Option<Box<MountedEffects>>,
    // Which slots hold a playing effect, each slot's current life, and the
    // slots free to lend. Slot 0 is never lent.
    active: Vec<bool>,
    generation: Vec<u32>,
    free: Vec<usize>,
    // Commands apply in the order they were written.
    sequence: u32,
    // Where each effect was last put, how fast it moved and how it is
    // turned, which a move without a velocity and a stop all keep.
    placed: Vec<[f64; 10]>,
    // Each effect's drawn size, extra inherited velocity and tint
    // (Effect.scale, Effect.inherit, Effect.tint), which every later
    // command repeats.
    scaled: Vec<[f64; 3]>,
}
struct MountedEffects {
    player: Spawner,
    budget: bfe_presentation::decoration::DecorationBudget,
}
impl Spawner {
    pub fn new(owner: Presentation) -> Self {
        Self { owner, decoration: None, active: vec![false; SLOTS], generation: vec![0; SLOTS], free: (1..SLOTS).rev().collect(), sequence: 0, placed: vec![[0.; 10]; SLOTS], scaled: vec![[1., 0., 1.]; SLOTS] }
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
        row[c::TINT] = self.scaled[id][2];
        row[c::SCALE] = self.scaled[id][0];
        row[c::INHERIT] = self.scaled[id][1];
        Some(row)
    }
    /// Effects.play: the effect group at a point, moving at `velocity`, its
    /// up axis along `normal` (straight up without one). The seed keeps its
    /// particle draws deterministic. None when the frame's batch or the pool
    /// is full.
    pub fn start(&mut self, group: usize, position: [f64; 3], velocity: [f64; 3], normal: Option<[f64; 3]>, seed: u32) -> Option<u32> {
        // `new THREE.Quaternion().setFromUnitVectors(UP, normal || UP)`.
        let mut turn = three::Quaternion::default();
        if let Some(normal) = normal {
            turn.set_from_unit_vectors(&three::Vector3::new(0., 1., 0.), &three::Vector3::new(normal[0], normal[1], normal[2]));
        }
        self.start_turned(group, position, velocity, [turn.x, turn.y, turn.z, turn.w], seed)
    }
    /// Effects.play with a `quaternion`: the effect turned as its owner is
    /// (a round's trail).
    pub fn start_turned(&mut self, group: usize, position: [f64; 3], velocity: [f64; 3], turn: [f64; 4], seed: u32) -> Option<u32> {
        if self.owner.input[f::COMMANDS] as usize == COMMANDS {
            return None;
        }
        let id = self.free.pop()?;
        // A new life of the slot: handles from its earlier lives go stale.
        self.active[id] = true;
        self.generation[id] = self.generation[id].wrapping_add(1).max(1);
        self.placed[id][..3].copy_from_slice(&position);
        self.placed[id][3..6].copy_from_slice(&velocity);
        self.placed[id][6..].copy_from_slice(&turn);
        self.scaled[id] = [1., 0., 1.];
        let row = self.command(k::SPAWN, id)?;
        row[c::CONTENT] = group as f64;
        row[c::SEED] = seed as f64;
        row[c::POSITION..c::POSITION + 3].copy_from_slice(&position);
        row[c::QUATERNION..c::QUATERNION + 4].copy_from_slice(&turn);
        row[c::VELOCITY..c::VELOCITY + 3].copy_from_slice(&velocity);
        Some(self.handle(id))
    }
    /// False once the effect a handle named has finished (`effect.done`).
    pub fn playing(&self, handle: u32) -> bool {
        self.live(handle).is_some()
    }
    /// Effect.move: the effect follows its owner. Without a turn or a
    /// velocity it keeps the one it had.
    pub fn sync(&mut self, handle: u32, position: [f64; 3], turn: Option<[f64; 4]>, velocity: Option<[f64; 3]>) {
        let Some(id) = self.live(handle) else {
            return;
        };
        self.placed[id][..3].copy_from_slice(&position);
        if let Some(turn) = turn {
            self.placed[id][6..].copy_from_slice(&turn);
        }
        if let Some(velocity) = velocity {
            self.placed[id][3..6].copy_from_slice(&velocity);
        }
        self.place(id, false);
    }
    /// A persistent source retries its stop if transient events filled this frame's command batch.
    pub(super) fn try_stop(&mut self, handle: u32) -> bool {
        if self.playing(handle) && self.owner.input[f::COMMANDS] as usize == COMMANDS {
            return false;
        }
        self.stop(handle);
        true
    }
    /// Effect.stop: it emits no more; what is out already lives on.
    pub fn stop(&mut self, handle: u32) {
        if let Some(id) = self.live(handle) {
            self.place(id, true);
        }
    }
    /// `effect.scale` and `effect.inherit`: every particle drawn this much
    /// larger, and taking this share of the effect's velocity on top of its
    /// own, from the next command on.
    pub fn scale(&mut self, handle: u32, scale: f64, inherit: f64) {
        if let Some(id) = self.live(handle) {
            self.scaled[id][..2].copy_from_slice(&[scale, inherit]);
        }
    }
    /// `effect.tint`: every particle's colour and alpha times this, from
    /// the next command on.
    pub fn tint(&mut self, handle: u32, tint: f64) {
        if let Some(id) = self.live(handle) {
            self.scaled[id][2] = tint;
        }
    }
    /// Effect.kill: gone at once, particles and all.
    pub fn kill(&mut self, handle: u32) {
        if let Some(id) = self.live(handle) {
            let placed = self.placed[id];
            if let Some(row) = self.command(k::SYNC, id) {
                row[c::POSITION..c::POSITION + 3].copy_from_slice(&placed[..3]);
                row[c::QUATERNION..c::QUATERNION + 4].copy_from_slice(&placed[6..]);
                row[c::STOPPED] = 1.;
                row[c::KILLED] = 1.;
            }
        }
    }
    /// Effects.step(effect, seconds) right after it starts: it is already
    /// that far into its life when first drawn.
    pub fn prewarm(&mut self, handle: u32, seconds: f64) {
        if seconds <= 0. {
            return;
        }
        if let Some(id) = self.live(handle) {
            if let Some(row) = self.command(k::PREWARM, id) {
                row[c::SECONDS] = seconds;
            }
        }
    }
    fn place(&mut self, id: usize, stopped: bool) {
        let placed = self.placed[id];
        if let Some(row) = self.command(k::SYNC, id) {
            row[c::POSITION..c::POSITION + 3].copy_from_slice(&placed[..3]);
            row[c::QUATERNION..c::QUATERNION + 4].copy_from_slice(&placed[6..]);
            row[c::VELOCITY..c::VELOCITY + 3].copy_from_slice(&placed[3..6]);
            row[c::STOPPED] = stopped as u8 as f64;
        }
    }
    /// Effects.update: step every effect, then take back the slots of the
    /// ones that finished in this life.
    pub fn frame(&mut self) {
        self.frame_shared();
        if let Some(mounted) = &mut self.decoration {
            let room = self.owner.particle_capacity() - self.owner.header[h::PARTICLES] as usize;
            mounted.budget.reserve(&mut mounted.player.owner, room);
            mounted.player.frame_shared();
            mounted.budget.release(&mut mounted.player.owner);
            self.owner.append_decoration(&mounted.player.owner);
        }
    }
    pub(super) fn enable_mounts(&mut self) {
        let particles = self.owner.particle_capacity() * 3 / 4;
        let limits = bfe_presentation::Limits { effects: SLOTS, emitters: 4096, particles, commands: COMMANDS, events: 32768 };
        self.decoration = Some(Box::new(MountedEffects {
            player: Self::new(Presentation::new(self.owner.content().clone(), limits)),
            budget: bfe_presentation::decoration::DecorationBudget::new(particles),
        }));
    }
    pub(super) fn begin_mount_frame(&mut self) {
        if let Some(mounted) = &mut self.decoration {
            mounted.player.owner.copy_clock(&self.owner);
        }
    }
    pub(super) fn mount_player(&mut self) -> Option<&mut Self> {
        self.decoration.as_mut().map(|mounted| &mut mounted.player)
    }
    pub(super) fn mount_rate(&mut self, handle: u32, rate: f64, dt: f64) {
        if let Some(id) = self.live(handle) {
            self.owner.mount_rate(id, self.generation[id], rate, dt);
        }
    }
    fn frame_shared(&mut self) {
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
