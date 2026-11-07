//! Generated exchange for the retained control phase. All frame storage is
//! reserved at construction; resource import alone may grow immutable tables.
use crate::layout::*;
use crate::phase::{Phase, RECORDS};
use crate::resources::Resources;
use crate::{Collision, engine::CAPACITY};

// Runtime.feed ports GameAudio.update's observation boundary. Rows retain
// their last facts until a touched update arrives, including during pause.
// Disappearance is explicit: stale identities cannot silently keep a loop
// alive, and recycling an identity first clears its previous edge history.
// Followed one-shots freeze at the source's last place when it leaves the
// foreign registry. They can finish without retaining the browser object.
// Input, command and diagnostic storage is reserved before gameplay begins.
// Decode completion has its own generation-checked I/O boundary because a
// ready segment starts on the device clock between presented game frames.
// It never advances game time or reruns event selection for that frame.
pub const INPUT_CAPACITY: usize = 1 << 20;
pub struct Runtime {
    pub phase: Phase, resources: Resources, pub input: Vec<f64>, pub info: [f64; info::STRIDE],
    pub events: Vec<[f64; event::STRIDE]>, pub body_flags: Vec<u8>, pub planes: Vec<[f64; plane::STRIDE]>,
    pub candidates: Vec<u32>, pub enabled: Vec<u8>, pub requests: Vec<f64>, pub debug: Vec<f64>,
    pub follows: Vec<u32>,
}
impl Runtime {
    pub fn new(seed: u32) -> Self {
        Self { phase: Phase::new(seed), resources: Resources::default(), input: vec![0.0; INPUT_CAPACITY], info: [0.0; info::STRIDE],
            events: Vec::with_capacity(RECORDS), body_flags: vec![0; 65536], planes: Vec::new(), candidates: vec![0; 65536], enabled: vec![0; 65536],
            requests: vec![0.0; 128 * request::STRIDE], debug: vec![0.0; CAPACITY * debug_voice::STRIDE], follows: vec![0; RECORDS] }
    }
    pub fn import(&mut self, kind: usize, length: usize) {
        assert!(length <= self.input.len());
        if kind == import::PLANE { for row in self.input[..length].chunks_exact(plane::STRIDE) { self.planes.push(row.try_into().unwrap()); } }
        else { self.resources.import(&mut self.phase, kind, &self.input[..length]); }
    }
    pub fn clear_output(&mut self) {
        self.phase.engine.commands.clear(); self.phase.engine.placements.clear(); self.phase.streams.requests.clear(); self.phase.room_pending = 0; self.info[info::ACCEPTED] = 0.0; self.info[info::ERROR] = 0.0;
    }
    /// Missing rows retain their last pose. Removal is an explicit kind or
    /// presence update, so unchanged entities never need another full copy.
    pub fn feed(&mut self, length: usize) {
        assert!(length <= self.input.len()); self.events.clear(); let mut at = 0;
        while at < length {
            assert!(at + packet::STRIDE <= length); let kind = self.input[at + packet::KIND] as usize; let count = self.input[at + packet::COUNT] as usize; at += packet::STRIDE;
            let stride = match kind { input::FRAME => frame::STRIDE, input::EMITTER => emitter::STRIDE, input::WEAPON => weapon::STRIDE, input::POST => post::STRIDE, input::ITEM => item::STRIDE, input::TURRET => turret::STRIDE, input::FOOT => foot::STRIDE, input::EVENT => event::STRIDE, input::BODY => body::STRIDE, input::COUNT => count::STRIDE, _ => panic!("unknown sound input record") };
            assert!(count <= RECORDS || kind == input::BODY); let end = at.checked_add(count.checked_mul(stride).unwrap()).unwrap(); assert!(end <= length);
            for row in self.input[at..end].chunks_exact(stride) {
                macro_rules! copy { ($layout:ident, $field:ident, $count:ident) => {{ let id = row[$layout::ID] as usize; assert!(id < RECORDS); self.phase.$field[id].copy_from_slice(row); self.phase.$count = self.phase.$count.max(id + 1); }}; }
                match kind {
                    input::FRAME => { assert_eq!(count, 1); self.phase.frame.copy_from_slice(row); }
                    input::EMITTER => self.phase.observe_emitter(row),
                    input::WEAPON => self.phase.observe_weapon(row),
                    input::POST => copy!(post, posts, post_count),
                    input::COUNT => { let id = row[count::POST] as usize; let team = row[count::TEAM] as usize; self.phase.post_counts[id][team] = row[count::VALUE]; }
                    input::ITEM => { let id = row[item::ID] as usize; if row[item::PRESENT] != 0.0 && self.phase.items[id][item::PRESENT] == 0.0 { self.phase.item_state[id] = Default::default(); } copy!(item, items, item_count); },
                    input::TURRET => {
                        let id = row[turret::ID] as usize; assert!(id < RECORDS);
                        if row[turret::PRESENT] == 0.0 && self.phase.turrets[id][turret::PRESENT] != 0.0 { let point = [row[turret::X], row[turret::Y], row[turret::Z]]; self.phase.engine.detach_emitter(RECORDS + id, point); self.phase.layers.detach_emitter(RECORDS + id, point); }
                        if row[turret::PRESENT] != 0.0 && self.phase.turrets[id][turret::PRESENT] == 0.0 { self.phase.turret_state[id] = Default::default(); } copy!(turret, turrets, turret_count);
                    }
                    input::FOOT => copy!(foot, feet, foot_count),
                    input::EVENT => { assert!(self.events.len() < self.events.capacity()); self.events.push(row.try_into().unwrap()); }
                    input::BODY => { self.body_flags[row[body::ID] as usize] = (row[body::ENABLED] != 0.0) as u8; }
                    _ => unreachable!(),
                }
            } at = end;
        }
    }
    pub fn step(&mut self, collision: &mut impl Collision) { self.phase.step(collision, &self.events); self.publish(); }
    /// Asset readiness does not advance gameplay. Generation checks prevent
    /// a cancelled decoder from reviving a reused stream row.
    pub fn ready(&mut self) {
        self.clear_output(); let r = &self.input[..ready::STRIDE]; self.phase.engine.now = r[ready::TIME]; self.phase.engine.wall_now = r[ready::WALL_NOW];
        let voice = r[ready::VOICE] as usize; let generation = r[ready::GENERATION] as u64;
        let accepted = if r[ready::FAILED] != 0.0 {
            let accepted = self.phase.streams.failed(&self.phase.engine, voice, generation); if accepted && r[ready::FAILED] == 2.0 { self.info[info::ERROR] = 1.0; } accepted
        } else { self.phase.streams.ready(&mut self.phase.engine, voice, generation, r[ready::PART] as usize, r[ready::SAMPLE] as usize, r[ready::DURATION], r[ready::CHANNELS] as usize) };
        self.info[info::ACCEPTED] = accepted as u8 as f64; self.phase.streams_ended(); self.publish();
    }
    pub fn publish(&mut self) {
        let p = &self.phase; let i = &mut self.info;
        i[info::COMMANDS] = p.engine.commands.len() as f64; i[info::PLACEMENTS] = p.engine.placements.len() as f64; i[info::REQUESTS] = p.streams.requests.len() as f64;
        for n in 0..6 { i[info::REAL + n] = p.engine.stats[n] as f64; } i[info::SEED] = p.engine.random.0 as f64; i[info::TIME] = p.engine.time; i[info::ROOM_CHANGED] = p.room_pending as f64;
        let r = if p.engine.enhanced { p.acoustics.room } else { p.original_room };
        i[info::ROOM] = r.room; i[info::ROOM_HF] = r.room_hf; i[info::DECAY] = r.decay; i[info::HF_RATIO] = r.hf_ratio; i[info::REFLECTIONS] = r.reflections; i[info::REFL_DELAY] = r.refl_delay; i[info::REVERB] = r.reverb; i[info::REVERB_DELAY] = r.reverb_delay; i[info::DIFFUSION] = r.diffusion; i[info::DENSITY] = r.density; i[info::HF_REFERENCE] = r.hf_reference;
        for (n, r) in p.streams.requests.iter().enumerate() {
            let row = &mut self.requests[n * request::STRIDE..(n + 1) * request::STRIDE]; row[request::VOICE] = r.voice as f64; row[request::GENERATION] = r.generation as f64; row[request::SEGMENT] = if r.segment == crate::engine::NONE { -1.0 } else { r.segment as f64 }; row[request::PART] = r.part as f64; row[request::SAMPLE] = r.sample as f64; row[request::OPERATION] = r.operation as f64; row[request::VARIANT] = if r.variant == crate::engine::NONE { -1.0 } else { r.variant as f64 };
        }
        let mut follows = [0u32; RECORDS];
        for v in &p.engine.voices { if !v.done && v.emitter < RECORDS { follows[v.emitter] += 1; } }
        p.layers.follows(&mut follows);
        for id in 0..RECORDS { if self.follows[id] != follows[id] { self.follows[id] = follows[id]; } }
    }
    pub fn diagnostic(&mut self) {
        let mut count = 0;
        for v in &self.phase.engine.voices { if v.done { continue; } let row = &mut self.debug[count * debug_voice::STRIDE..(count + 1) * debug_voice::STRIDE];
            row[debug_voice::SOUND] = v.sound as f64; row[debug_voice::CATEGORY] = v.category as f64; row[debug_voice::MODE3D] = v.mode3d as u8 as f64; row[debug_voice::DISTANCE] = v.distance; row[debug_voice::AUDIBLE] = v.audible; row[debug_voice::SCORE] = v.score; row[debug_voice::REAL] = v.real as u8 as f64; row[debug_voice::OCCLUDED] = v.occluded.unwrap_or(0.0); row[debug_voice::ELAPSED] = v.elapsed; count += 1;
        } self.info[info::DEBUG_COUNT] = count as f64;
    }
}

// Native facts use the same admission edge as Runtime.feed. Reusing a slot
// must retire its loops before resetting the edge history for the new body.
impl Phase {
    pub fn observe_emitter(&mut self, row: &[f64]) {
        if row.len() != emitter::STRIDE { return; }
        let id = row[emitter::ID] as usize;
        if id >= RECORDS { return; }
        if row[emitter::KIND] == 0.0 && self.emitters[id][emitter::KIND] != 0.0 {
            let point = [row[emitter::X], row[emitter::Y], row[emitter::Z]];
            self.engine.detach_emitter(id, point);
            self.layers.detach_emitter(id, point);
        }
        if row[emitter::KIND] != 0.0 && self.emitters[id][emitter::KIND] == 0.0 {
            self.entity_state[id] = Default::default();
        }
        self.emitters[id].copy_from_slice(row);
        self.entity_count = self.entity_count.max(id + 1);
    }
    pub fn observe_weapon(&mut self, row: &[f64]) {
        if row.len() != weapon::STRIDE { return; }
        let id = row[weapon::ID] as usize;
        if id >= RECORDS { return; }
        if row[weapon::OWNER] >= 0.0 && self.weapons[id][weapon::OWNER] < 0.0 {
            self.weapon_state[id] = Default::default();
            self.player_weapon_state[id] = Default::default();
        }
        self.weapons[id].copy_from_slice(row);
        self.weapon_count = self.weapon_count.max(id + 1);
    }
}
