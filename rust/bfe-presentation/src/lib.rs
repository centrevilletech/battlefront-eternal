//! Persistent effect state, independent of the window, GL backend and clock.
//! The host submits ordered commands once; the kernel publishes draw records.
pub mod layout;
pub mod cameras;
pub mod aim;
pub mod feet;
pub mod sniper;
pub mod sniper_layout;
use layout::{
    command as c, command_kind as ck, draw as d, draw_kind as dk, effect as e, event as ev,
    event_kind as ek, frame as f, header as h, status,
};
use serde::Deserialize;
use three::{Quaternion, Vector3};

const NONE: usize = usize::MAX;
const DEG: f64 = std::f64::consts::PI / 180.0;
#[derive(Clone, Copy, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Scalar(f64),
    Vector([f64; 3]),
    Color([f64; 4]),
}
impl Value {
    fn scalar(self) -> f64 {
        match self {
            Self::Scalar(x) => x,
            Self::Vector(x) => x[0],
            Self::Color(x) => x[0],
        }
    }
    fn vector(self) -> [f64; 3] {
        match self {
            Self::Vector(x) => x,
            _ => [self.scalar(); 3],
        }
    }
    fn color(self) -> [f64; 4] {
        match self {
            Self::Color(x) => x,
            _ => [self.scalar(); 4],
        }
    }
}
#[derive(Deserialize)]
pub struct Key {
    pub lifetime: f64,
    pub accelerate: Option<Value>,
    pub reach: Option<Value>,
    pub scale: Option<f64>,
    pub r#move: Option<Value>,
}
#[derive(Deserialize)]
pub struct Color {
    pub channels: [[f64; 2]; 4],
    #[serde(default)]
    pub hsv: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spawner {
    pub shape: String,
    pub r#box: [[f64; 2]; 3],
    pub position_scale: [f64; 2],
    pub velocity_scale: [f64; 2],
    pub offset: [[f64; 2]; 3],
    pub inherit_velocity: [f64; 2],
    pub size: [f64; 2],
    pub color: Color,
    pub fade_in_time: f64,
    pub start_rotation: [f64; 2],
    pub rotation_velocity: [f64; 2],
}
#[derive(Deserialize)]
pub struct Transformer {
    pub lifetime: f64,
    #[serde(default)]
    pub position: Vec<Key>,
    #[serde(default)]
    pub size: Vec<Key>,
    #[serde(default)]
    pub color: Vec<Key>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Geometry {
    pub r#type: String,
    pub blend: String,
    pub spark_length: Option<f64>,
    pub time_per_frame: Option<f64>,
    pub frame_size: Option<f64>,
    pub total_frames: Option<f64>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub start_delay: [f64; 2],
    pub burst_delay: [f64; 2],
    pub burst_count: [f64; 2],
    pub max_particles: [f64; 2],
    pub size: [f64; 2],
    pub color: Color,
    pub register_step: bool,
    pub spawner: Spawner,
    pub transformer: Transformer,
    pub geometry: Geometry,
    pub child: usize,
    pub gpu: bool,
}
#[derive(Deserialize)]
pub struct Content {
    pub definitions: Vec<Definition>,
    pub groups: Vec<Vec<usize>>,
}

// A seed is part of an effect's birth event. No host callback or platform RNG
// is consulted while presenting. The JS replay oracle uses this same stream.
#[derive(Clone, Copy, Default)]
pub struct Random {
    pub seed: u32,
    pub draws: u32,
}
impl Random {
    fn next(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        self.draws += 1;
        self.seed as f64 / 4294967296.0
    }
    fn range(&mut self, r: [f64; 2]) -> f64 {
        r[0] + self.next() * (r[1] - r[0])
    }
}
#[derive(Clone, Copy)]
struct Effect {
    token: u32,
    seed: u32,
    id: u32,
    generation: u32,
    content: usize,
    position: Vector3,
    previous: Vector3,
    quaternion: Quaternion,
    velocity: Vector3,
    time: f64,
    tint: f64,
    scale: f64,
    inherit: f64,
    stopped: bool,
    killed: bool,
    done: bool,
    first: usize,
    random: Random,
}
impl Default for Effect {
    fn default() -> Self {
        Self {
            token: 0,
            seed: 0,
            id: 0,
            generation: 0,
            content: 0,
            position: Vector3::default(),
            previous: Vector3::default(),
            quaternion: Quaternion::default(),
            velocity: Vector3::default(),
            time: 0.0,
            tint: 1.0,
            scale: 1.0,
            inherit: 0.0,
            stopped: false,
            killed: false,
            done: false,
            first: NONE,
            random: Random::default(),
        }
    }
}
#[derive(Clone, Copy)]
struct Emitter {
    definition: usize,
    next: usize,
    delay: f64,
    next_burst: f64,
    limit: f64,
    emitted: u32,
    size: f64,
    scale: [f64; 4],
    first: usize,
    last: usize,
    nozzle: bool,
    turn: Quaternion,
    offset: Vector3,
}
impl Default for Emitter {
    fn default() -> Self {
        Self {
            definition: 0,
            next: NONE,
            delay: 0.0,
            next_burst: 0.0,
            limit: 0.0,
            emitted: 0,
            size: 0.0,
            scale: [0.0; 4],
            first: NONE,
            last: NONE,
            nozzle: false,
            turn: Quaternion::default(),
            offset: Vector3::default(),
        }
    }
}
impl Emitter {
    fn emitting(&self) -> bool {
        self.limit < 0.0 || (self.emitted as f64) < self.limit
    }
}
#[derive(Clone, Copy)]
struct Particle {
    serial: u32,
    next: usize,
    birth: f64,
    life: f64,
    position: Vector3,
    velocity: Vector3,
    size: f64,
    color: [f64; 4],
    rotation: f64,
    spin: f64,
    turn: Quaternion,
    child: usize,
    child_token: u32,
}
impl Default for Particle {
    fn default() -> Self {
        Self {
            serial: 0,
            next: NONE,
            birth: 0.0,
            life: 0.0,
            position: Vector3::default(),
            velocity: Vector3::default(),
            size: 0.0,
            color: [0.0; 4],
            rotation: 0.0,
            spin: 0.0,
            turn: Quaternion::default(),
            child: NONE,
            child_token: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub effects: usize,
    pub emitters: usize,
    pub particles: usize,
    pub commands: usize,
    pub events: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            effects: 4096,
            emitters: 16384,
            particles: 65536,
            commands: 16384,
            events: 131072,
        }
    }
}

pub struct Presentation {
    content: Content,
    effects: Vec<Effect>,
    emitters: Vec<Emitter>,
    particles: Vec<Particle>,
    free_effects: Vec<usize>,
    free_emitters: Vec<usize>,
    free_particles: Vec<usize>,
    order: Vec<usize>,
    lookup: Vec<usize>,
    generations: Vec<u32>,
    /// Page spawns refused this frame (id, generation): published as finished
    /// rows so the page frees their handles, with no birth or death event.
    refused: Vec<(u32, u32)>,
    /// Totals since load, published every frame for a once-only page report.
    refused_effects: f64,
    refused_particles: f64,
    skipped_draws: f64,
    skipped_commands: f64,
    pub input: Vec<f64>,
    pub header: Vec<f64>,
    pub effect_output: Vec<f64>,
    pub draws: Vec<f64>,
    pub events: Vec<f64>,
    serial: u32,
    effect_serial: u32,
    sequence: u32,
    tick: f64,
    frame: f64,
    epoch: f64,
    last_command: f64,
}
impl Presentation {
    pub fn new(content: Content, limits: Limits) -> Self {
        Self {
            content,
            effects: vec![Effect::default(); limits.effects],
            emitters: vec![Emitter::default(); limits.emitters],
            particles: vec![Particle::default(); limits.particles],
            free_effects: (0..limits.effects).rev().collect(),
            free_emitters: (0..limits.emitters).rev().collect(),
            free_particles: (0..limits.particles).rev().collect(),
            order: Vec::with_capacity(limits.effects),
            lookup: vec![NONE; limits.effects],
            generations: vec![0; limits.effects],
            refused: Vec::with_capacity(limits.commands),
            refused_effects: 0.0,
            refused_particles: 0.0,
            skipped_draws: 0.0,
            skipped_commands: 0.0,
            input: vec![0.0; f::STRIDE + limits.commands * c::STRIDE],
            header: vec![0.0; h::STRIDE],
            effect_output: vec![0.0; (limits.effects + limits.commands) * e::STRIDE],
            draws: vec![0.0; limits.particles * d::STRIDE],
            events: vec![0.0; limits.events * ev::STRIDE],
            serial: 0,
            effect_serial: 0,
            sequence: 0,
            tick: 0.0,
            frame: -1.0,
            epoch: 0.0,
            last_command: 0.0,
        }
    }
    /// Effect ids the host may issue, 1 to handles() - 1.
    pub fn handles(&self) -> usize {
        self.lookup.len()
    }
    // Room for one more birth this frame: its own birth and death events,
    // with the death of everything already alive still guaranteed a slot.
    // Every birth reserves its death, so deaths and births always pair.
    fn birth_room(&self) -> bool {
        let live = (self.particles.len() - self.free_particles.len())
            + (self.effects.len() - self.free_effects.len());
        (self.header[h::EVENTS] as usize + live + 2) * ev::STRIDE <= self.events.len()
    }
    fn event(&mut self, kind: u32, effect: Effect, p: Particle) {
        let at = self.header[h::EVENTS] as usize * ev::STRIDE;
        // Unreachable while births keep their reservation; never fail a frame.
        if at + ev::STRIDE > self.events.len() {
            return;
        }
        self.sequence += 1;
        let out = &mut self.events[at..at + ev::STRIDE];
        out[ev::SEQUENCE] = self.sequence as f64;
        out[ev::KIND] = kind as f64;
        out[ev::ID] = effect.id as f64;
        out[ev::GENERATION] = effect.generation as f64;
        out[ev::TICK] = self.tick;
        out[ev::CONTENT] = effect.content as f64;
        out[ev::PARTICLE] = p.serial as f64;
        out[ev::BIRTH] = p.birth;
        out[ev::LIFE] = p.life;
        out[ev::SEED] = effect.seed as f64;
        out[ev::BIRTH_ORDER] = effect.token as f64;
        self.header[h::EVENTS] += 1.0;
    }
    // A full store refuses the whole birth, never part of an effect: no
    // slot, too few emitters for its group, or no event room. The old page
    // pools grew instead; these stores are sized above what content makes.
    fn start(&mut self, mut effect: Effect) -> usize {
        let fits = effect.content < self.content.groups.len()
            && self.content.groups[effect.content].len() <= self.free_emitters.len()
            && !self.free_effects.is_empty()
            && self.birth_room();
        let slot = match self.free_effects.pop() {
            Some(slot) if fits => slot,
            other => {
                if let Some(slot) = other {
                    self.free_effects.push(slot);
                }
                self.refused_effects += 1.0;
                return NONE;
            }
        };
        self.effect_serial += 1;
        effect.token = self.effect_serial;
        effect.seed = effect.random.seed;
        let mut last = NONE;
        for k in 0..self.content.groups[effect.content].len() {
            let Some(em) = self.free_emitters.pop() else {
                break;
            };
            let definition = self.content.groups[effect.content][k];
            let def = &self.content.definitions[definition];
            // effects.js Emitter constructor: keep random draws in authored order.
            let delay = effect.random.range(def.start_delay);
            let limit = v8math::round(effect.random.range(def.max_particles));
            let size = effect.random.range(def.size);
            let mut scale = [0.0; 4];
            for (i, v) in scale.iter_mut().enumerate() {
                *v = effect.random.range(def.color.channels[i]) / 255.0;
            }
            self.emitters[em] = Emitter {
                definition,
                delay,
                limit,
                size,
                scale,
                ..Emitter::default()
            };
            if last == NONE {
                effect.first = em;
            } else {
                self.emitters[last].next = em;
            }
            last = em;
        }
        self.effects[slot] = effect;
        self.order.push(slot);
        if effect.id != 0 {
            self.lookup[effect.id as usize] = slot;
            self.generations[effect.id as usize] = effect.generation;
        }
        self.event(ek::EFFECT_BIRTH, effect, Particle::default());
        slot
    }
    // frame is the sole live phase entry point. Buffers and pools never grow.
    // Nothing the page sends can fail it: a full store refuses births, and a
    // stale, unknown or malformed command is skipped and counted. Only a
    // page built against another layout is reported, every frame.
    pub fn frame(&mut self) {
        self.header.fill(0.0);
        self.header[h::VERSION] = layout::VERSION as f64;
        if self.input[f::VERSION] != layout::VERSION as f64 {
            self.header[h::STATUS] = status::VERSION_MISMATCH as f64;
            return;
        }
        self.refused.clear();
        let commands =
            (self.input[f::COMMANDS] as usize).min((self.input.len() - f::STRIDE) / c::STRIDE);
        self.frame = self.input[f::FRAME];
        self.epoch = self.input[f::EPOCH];
        self.tick = self.input[f::TICK];
        for k in 0..commands {
            let at = f::STRIDE + k * c::STRIDE;
            let id = self.input[at + c::ID] as usize;
            let generation = self.input[at + c::GENERATION] as u32;
            // Generation-qualified ids: a stale handle never reaches a slot
            // its id now names for a later effect.
            if id == 0
                || id >= self.lookup.len()
                || self.input[at + c::SEQUENCE] <= self.last_command
            {
                self.skipped_commands += 1.0;
                continue;
            }
            self.last_command = self.input[at + c::SEQUENCE];
            let kind = self.input[at + c::KIND] as u32;
            let slot = if kind == ck::SPAWN {
                if self.lookup[id] != NONE || generation <= self.generations[id] {
                    self.skipped_commands += 1.0;
                    continue;
                }
                let effect = Effect {
                    id: id as u32,
                    generation,
                    content: self.input[at + c::CONTENT] as usize,
                    random: Random {
                        seed: self.input[at + c::SEED] as u32,
                        draws: 0,
                    },
                    ..Effect::default()
                };
                let slot = self.start(effect);
                if slot == NONE {
                    // The page frees this handle from its finished row.
                    self.generations[id] = generation;
                    self.refused.push((id as u32, generation));
                    continue;
                }
                slot
            } else {
                self.lookup[id]
            };
            if slot == NONE || self.effects[slot].generation != generation {
                self.skipped_commands += 1.0;
                continue;
            }
            if kind == ck::NOZZLE {
                let mut em = self.effects[slot].first;
                let mut index = self.input[at + c::EMITTER] as usize;
                while em != NONE && index > 0 {
                    em = self.emitters[em].next;
                    index -= 1;
                }
                if em == NONE {
                    self.skipped_commands += 1.0;
                    continue;
                }
                self.emitters[em].nozzle = true;
                self.emitters[em].turn = quat(&self.input, at + c::NOZZLE_TURN);
                self.emitters[em].offset = vec3(&self.input, at + c::NOZZLE_OFFSET);
            } else if kind == ck::PREWARM {
                // Effects.step at a map source's first visibility. Its birth
                // events survive, but temporary draws are replaced by the
                // following whole-batch update, just as renderer.begin did.
                let seconds = self.input[at + c::SECONDS];
                if !seconds.is_finite() || seconds < 0.0 {
                    self.skipped_commands += 1.0;
                    continue;
                }
                self.step(slot, seconds, 0);
                self.header[h::DRAWS] = 0.0;
            } else if kind == ck::SPAWN || kind == ck::SYNC {
                let effect = &mut self.effects[slot];
                effect.position = vec3(&self.input, at + c::POSITION);
                effect.quaternion = quat(&self.input, at + c::QUATERNION);
                effect.velocity = vec3(&self.input, at + c::VELOCITY);
                effect.tint = self.input[at + c::TINT];
                effect.scale = self.input[at + c::SCALE];
                effect.inherit = self.input[at + c::INHERIT];
                effect.stopped = self.input[at + c::STOPPED] != 0.0;
                effect.killed = self.input[at + c::KILLED] != 0.0;
                if kind == ck::SPAWN {
                    effect.previous = effect.position;
                }
            } else {
                self.skipped_commands += 1.0;
            }
        }
        if self.input[f::PRESENT] == 0.0 || self.input[f::FROZEN] != 0.0 {
            self.publish_refused(0);
            return;
        }
        let dt = self.input[f::DT];
        let dt = if dt.is_finite() && dt > 0.0 { dt } else { 0.0 };
        let mut cursor = 0;
        let mut gpu_slot = 0;
        // Child births join the tail and are advanced this frame, just as the
        // growing Effects.effects list in the JS oracle.
        while cursor < self.order.len() {
            let slot = self.order[cursor];
            let killed = self.effects[slot].killed;
            self.step(slot, dt, gpu_slot);
            if !killed {
                gpu_slot += 1;
            }
            cursor += 1;
        }
        let mut kept = 0;
        for i in 0..self.order.len() {
            let slot = self.order[i];
            let effect = self.effects[slot];
            self.publish_effect(effect, i);
            if effect.done {
                if effect.id != 0 {
                    self.lookup[effect.id as usize] = NONE;
                }
                let mut em = effect.first;
                while em != NONE {
                    let next = self.emitters[em].next;
                    self.free_emitters.push(em);
                    em = next;
                }
                self.free_effects.push(slot);
            } else {
                self.order[kept] = slot;
                kept += 1;
            }
        }
        let published = self.order.len();
        self.order.truncate(kept);
        self.publish_refused(published);
    }
    // Finished rows for refused page spawns follow the live rows, then the
    // store occupancy and the totals of everything refused or skipped.
    fn publish_refused(&mut self, mut row: usize) {
        for k in 0..self.refused.len() {
            let (id, generation) = self.refused[k];
            let out = &mut self.effect_output[row * e::STRIDE..(row + 1) * e::STRIDE];
            out.fill(0.0);
            out[e::ID] = id as f64;
            out[e::GENERATION] = generation as f64;
            out[e::DONE] = 1.0;
            out[e::KILLED] = 1.0;
            row += 1;
        }
        self.header[h::EFFECTS] = row as f64;
        self.header[h::PARTICLES] = (self.particles.len() - self.free_particles.len()) as f64;
        self.header[h::EMITTERS] = (self.emitters.len() - self.free_emitters.len()) as f64;
        self.header[h::REFUSED_EFFECTS] = self.refused_effects;
        self.header[h::REFUSED_PARTICLES] = self.refused_particles;
        self.header[h::SKIPPED_DRAWS] = self.skipped_draws;
        self.header[h::SKIPPED_COMMANDS] = self.skipped_commands;
    }
    fn step(&mut self, slot: usize, dt: f64, output: usize) {
        let mut effect = self.effects[slot];
        let from = effect.time;
        if !effect.killed {
            effect.time += dt;
        }
        let mut alive = false;
        let mut em_slot = effect.first;
        while em_slot != NONE {
            let mut em = self.emitters[em_slot];
            if !effect.killed {
                while !effect.stopped && em.emitting() && effect.time - em.delay >= em.next_burst {
                    let at = em.delay + em.next_burst;
                    let share = if dt > 0.0 {
                        ((at - from) / dt).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let count = v8math::round(
                        effect
                            .random
                            .range(self.content.definitions[em.definition].burst_count),
                    )
                    .min(128.0) as i32;
                    for _ in 0..count {
                        if !em.emitting() {
                            break;
                        }
                        self.spawn(&mut effect, &mut em, at, share);
                    }
                    let def = &self.content.definitions[em.definition];
                    em.next_burst += effect.random.range(def.burst_delay).max(1e-3);
                    if count <= 0 && em.limit < 0.0 && def.burst_delay[1] <= 0.0 {
                        break;
                    }
                }
            }
            let mut p_slot = em.first;
            let mut last = NONE;
            em.first = NONE;
            em.last = NONE;
            while p_slot != NONE {
                let p = self.particles[p_slot];
                let next = p.next;
                let age = effect.time - p.birth;
                if effect.killed || age >= p.life {
                    if p.child != NONE
                        && self.effects[p.child].token == p.child_token
                        && !self.effects[p.child].done
                    {
                        if effect.killed {
                            self.effects[p.child].killed = true;
                        }
                        self.effects[p.child].stopped = true;
                    }
                    self.event(ek::PARTICLE_DEATH, effect, p);
                    self.free_particles.push(p_slot);
                } else {
                    self.draw(effect, em, p, age, output);
                    if last == NONE {
                        em.first = p_slot;
                    } else {
                        self.particles[last].next = p_slot;
                    }
                    self.particles[p_slot].next = NONE;
                    last = p_slot;
                    em.last = p_slot;
                }
                p_slot = next;
            }
            if em.first != NONE || (!effect.stopped && em.emitting()) {
                alive = true;
            }
            self.emitters[em_slot] = em;
            em_slot = em.next;
        }
        effect.done = effect.killed || !alive;
        effect.previous = effect.position;
        self.header[h::RANDOM_DRAWS] += effect.random.draws as f64;
        effect.random.draws = 0;
        self.effects[slot] = effect;
        if effect.done {
            self.event(ek::EFFECT_DEATH, effect, Particle::default());
        }
    }
    fn spawn(&mut self, effect: &mut Effect, em: &mut Emitter, birth: f64, share: f64) {
        // A full particle store or event reservation refuses this particle;
        // the emitter keeps its count and tries again at its next burst.
        if !self.birth_room() {
            self.refused_particles += 1.0;
            return;
        }
        let Some(slot) = self.free_particles.pop() else {
            self.refused_particles += 1.0;
            return;
        };
        let def = &self.content.definitions[em.definition];
        let sp = &def.spawner;
        let rng = &mut effect.random;
        let mut dir = Vector3::default();
        if sp.shape == "circle" {
            dir.random_direction(&mut || rng.next());
            dir.set(
                sp.r#box[0][0] + ((dir.x + 1.0) / 2.0) * (sp.r#box[0][1] - sp.r#box[0][0]),
                sp.r#box[1][0] + ((dir.y + 1.0) / 2.0) * (sp.r#box[1][1] - sp.r#box[1][0]),
                sp.r#box[2][0] + ((dir.z + 1.0) / 2.0) * (sp.r#box[2][1] - sp.r#box[2][0]),
            );
        } else {
            dir.set(
                rng.range(sp.r#box[0]),
                rng.range(sp.r#box[1]),
                rng.range(sp.r#box[2]),
            );
        }
        let position_scale = rng.range(sp.position_scale);
        let velocity_scale = rng.range(sp.velocity_scale);
        let mut position = Vector3::new(
            rng.range(sp.offset[0]),
            rng.range(sp.offset[1]),
            rng.range(sp.offset[2]),
        );
        position
            .add_scaled_vector(&dir, position_scale)
            .apply_quaternion(&effect.quaternion);
        if em.nozzle {
            position.apply_quaternion(&em.turn).add(&em.offset);
        }
        if def.register_step {
            let mut origin = Vector3::default();
            origin.lerp_vectors(&effect.previous, &effect.position, share);
            position.add(&origin);
        }
        let mut velocity = dir;
        velocity
            .multiply_scalar(velocity_scale)
            .apply_quaternion(&effect.quaternion);
        if em.nozzle {
            velocity.apply_quaternion(&em.turn);
        }
        velocity.add_scaled_vector(
            &effect.velocity,
            rng.range(sp.inherit_velocity) + effect.inherit,
        );
        let mut color = [0.0; 4];
        for (i, v) in color.iter_mut().enumerate() {
            *v = rng.range(sp.color.channels[i]) / 255.0;
        }
        if sp.color.hsv {
            hsv(&mut color);
        }
        for (i, v) in color.iter_mut().enumerate() {
            *v *= em.scale[i];
        }
        if sp.fade_in_time > 0.0 && birth - em.delay < sp.fade_in_time {
            color[3] *= (birth - em.delay) / sp.fade_in_time;
        }
        let size = rng.range(sp.size) * em.size;
        let rotation = rng.range(sp.start_rotation) * DEG;
        let spin = rng.range(sp.rotation_velocity) * DEG;
        let mut turn = Quaternion::default();
        if def.geometry.r#type == "geometry" {
            turn.random(&mut || rng.next());
        }
        self.serial += 1;
        let mut p = Particle {
            serial: self.serial,
            birth,
            life: def.transformer.lifetime,
            position,
            velocity,
            size,
            color,
            rotation,
            spin,
            turn,
            ..Particle::default()
        };
        if def.geometry.r#type == "emitter" && def.child != 0 {
            let child_content = def.child;
            let mut at = position;
            if !def.register_step {
                at.add(&effect.position);
            }
            let seed = (rng.next() * 4294967296.0) as u32;
            let child = Effect {
                content: child_content,
                position: at,
                previous: at,
                quaternion: effect.quaternion,
                velocity,
                scale: effect.scale,
                random: Random { seed, draws: 0 },
                ..Effect::default()
            };
            p.child = self.start(child);
            if p.child != NONE {
                p.child_token = self.effects[p.child].token;
            }
        }
        self.particles[slot] = p;
        if em.last == NONE {
            em.first = slot;
        } else {
            self.particles[em.last].next = slot;
        }
        em.last = slot;
        em.emitted += 1;
        self.event(ek::PARTICLE_BIRTH, *effect, p);
    }
    fn draw(&mut self, effect: Effect, em: Emitter, p: Particle, age: f64, output: usize) {
        let def = &self.content.definitions[em.definition];
        let g = &def.geometry;
        let at = self.header[h::DRAWS] as usize * d::STRIDE;
        if at + d::STRIDE > self.draws.len() {
            self.skipped_draws += 1.0;
            return;
        }
        let analytic = def.gpu && output < 1024;
        // Analytic draw rows retain spawn data. The GPU applies age, tint and
        // scale from the separate effect table, so stable rows need no writes.
        if analytic
            && self.draws[at + d::KIND] == dk::ANALYTIC as f64
            && self.draws[at + d::PARTICLE] == p.serial as f64
            && self.draws[at + d::EFFECT] == output as f64
            && self.draws[at + d::DEFINITION] == em.definition as f64
        {
            self.header[h::DRAWS] += 1.0;
            return;
        }
        let mut pos = p.position;
        let mut vel = p.velocity;
        let mut color = p.color;
        let mut size = p.size;
        let mut rotation = p.rotation;
        let mut turn = p.turn;
        let mut shape = 0.0;
        let mut length = 0.0;
        let mut axis = [0.0, 0.0, 1.0];
        let mut axis2 = [1.0, 0.0, 0.0];
        let mut frame = [0.0, 0.0, 1.0, 1.0];
        if !analytic {
            let disp = travel(&def.transformer.position, age, &mut vel);
            pos.add(&disp);
            if !def.register_step {
                pos.add(&effect.position);
            }
            size = scalar_track(&def.transformer.size, age, size) * effect.scale;
            color_track(&def.transformer.color, age, &mut color);
            if effect.tint != 1.0 {
                for v in &mut color {
                    *v *= effect.tint;
                }
            }
            rotation += p.spin * age;
            if p.child != NONE && self.effects[p.child].token == p.child_token {
                self.effects[p.child].position = pos;
                self.effects[p.child].velocity = vel;
            }
            if g.r#type == "emitter" || g.blend == "blur" || size <= 0.0 {
                return;
            }
            if g.r#type == "spark" || g.r#type == "streak" {
                let speed = vel.length();
                length = speed * g.spark_length.filter(|v| *v != 0.0).unwrap_or(0.1);
                if speed < 1e-4 {
                    return;
                }
                let mut a = vel;
                a.divide_scalar(speed);
                axis = [a.x, a.y, a.z];
                shape = 1.0;
                rotation = 0.0;
            } else if g.r#type == "billboard" {
                let cos = v8math::cos(rotation);
                let sin = v8math::sin(rotation);
                let mut a = Vector3::new(cos, 0.0, sin);
                a.apply_quaternion(&effect.quaternion);
                axis = [a.x, a.y, a.z];
                let mut b = Vector3::new(-sin, 0.0, cos);
                b.apply_quaternion(&effect.quaternion);
                axis2 = [b.x, b.y, b.z];
                shape = 2.0;
                rotation = 0.0;
            } else if g.r#type == "animated" && g.time_per_frame.unwrap_or(0.0) > 0.0 {
                let cells = v8math::round(g.frame_size.unwrap_or(1.0)).max(1.0);
                let total = v8math::round(g.total_frames.unwrap_or(1.0)).max(1.0);
                let index = (age / g.time_per_frame.unwrap()).floor().min(total - 1.0);
                frame = [
                    index % cells / cells,
                    (index / cells).floor() / cells,
                    1.0 / cells,
                    1.0 / cells,
                ];
            }
        }
        if !analytic && g.r#type == "geometry" {
            // effects.js drawModel / Object3D.rotateY, now composed by the
            // state owner. Hosts apply the resulting quaternion directly.
            let mut spin = Quaternion::default();
            spin.set_from_axis_angle(&Vector3::new(0.0, 1.0, 0.0), rotation);
            turn.multiply(&spin);
            rotation = 0.0;
        }
        let out = &mut self.draws[at..at + d::STRIDE];
        out.fill(0.0);
        out[d::KIND] = if analytic {
            dk::ANALYTIC
        } else if g.r#type == "geometry" {
            dk::MODEL
        } else {
            dk::PARTICLE
        } as f64;
        out[d::DEFINITION] = em.definition as f64;
        out[d::EFFECT] = output as f64;
        out[d::PARTICLE] = p.serial as f64;
        out[d::BIRTH] = p.birth;
        out[d::LIFE] = p.life;
        put3(out, d::POSITION, pos);
        put3(out, d::VELOCITY, vel);
        out[d::COLOR..d::COLOR + 4].copy_from_slice(&color);
        out[d::SIZE] = size;
        out[d::ROTATION] = rotation;
        out[d::SPIN] = p.spin;
        out[d::TURN..d::TURN + 4].copy_from_slice(&[turn.x, turn.y, turn.z, turn.w]);
        out[d::SHAPE] = shape;
        out[d::LENGTH] = length;
        out[d::AXIS..d::AXIS + 3].copy_from_slice(&axis);
        out[d::AXIS2..d::AXIS2 + 3].copy_from_slice(&axis2);
        out[d::FRAME..d::FRAME + 4].copy_from_slice(&frame);
        self.header[h::DRAWS] += 1.0;
    }
    fn publish_effect(&mut self, effect: Effect, row: usize) {
        let out = &mut self.effect_output[row * e::STRIDE..(row + 1) * e::STRIDE];
        out[e::ID] = effect.id as f64;
        out[e::GENERATION] = effect.generation as f64;
        out[e::CONTENT] = effect.content as f64;
        out[e::TIME] = effect.time;
        put3(out, e::POSITION, effect.position);
        out[e::QUATERNION..e::QUATERNION + 4].copy_from_slice(&[
            effect.quaternion.x,
            effect.quaternion.y,
            effect.quaternion.z,
            effect.quaternion.w,
        ]);
        out[e::TINT] = effect.tint;
        out[e::SCALE] = effect.scale;
        out[e::DONE] = effect.done as u8 as f64;
        out[e::KILLED] = effect.killed as u8 as f64;
    }
}
fn vec3(data: &[f64], at: usize) -> Vector3 {
    Vector3::new(data[at], data[at + 1], data[at + 2])
}
fn quat(data: &[f64], at: usize) -> Quaternion {
    Quaternion::new(data[at], data[at + 1], data[at + 2], data[at + 3])
}
fn put3(out: &mut [f64], at: usize, v: Vector3) {
    out[at] = v.x;
    out[at + 1] = v.y;
    out[at + 2] = v.z;
}
// effects.js hsv, with a stack table rather than temporary RGB arrays.
fn hsv(c: &mut [f64; 4]) {
    let h = c[0];
    let s = c[1];
    let v = c[2];
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let rgb = match ((i % 6.0 + 6.0) % 6.0) as u32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    };
    c[..3].copy_from_slice(&rgb);
}
// effects.js travel: preserve expression and key order, including zero spans.
fn travel(keys: &[Key], age: f64, v: &mut Vector3) -> Vector3 {
    let mut p = Vector3::default();
    let mut left = age;
    for key in keys {
        if left <= 0.0 {
            break;
        }
        let span = key.lifetime;
        if span <= 0.0 {
            if let Some(a) = key.accelerate {
                let a = a.vector();
                v.x += a[0];
                v.y += a[1];
                v.z += a[2];
            } else if let Some(r) = key.reach {
                let r = r.vector();
                v.set(r[0], r[1], r[2]);
            } else if let Some(s) = key.scale {
                v.multiply_scalar(s);
            }
            continue;
        }
        let t = left.min(span);
        let f = t / span;
        if let Some(a) = key.accelerate {
            let a = a.vector();
            p.x += v.x * t + (a[0] * t * f) / 2.0;
            p.y += v.y * t + (a[1] * t * f) / 2.0;
            p.z += v.z * t + (a[2] * t * f) / 2.0;
            v.x += a[0] * f;
            v.y += a[1] * f;
            v.z += a[2] * f;
        } else if let Some(r) = key.reach {
            let r = r.vector();
            p.x += v.x * t + ((r[0] - v.x) * t * f) / 2.0;
            p.y += v.y * t + ((r[1] - v.y) * t * f) / 2.0;
            p.z += v.z * t + ((r[2] - v.z) * t * f) / 2.0;
            v.x += (r[0] - v.x) * f;
            v.y += (r[1] - v.y) * f;
            v.z += (r[2] - v.z) * f;
        } else if let Some(s) = key.scale {
            p.add_scaled_vector(v, t + ((s - 1.0) * t * f) / 2.0);
            v.multiply_scalar(1.0 + (s - 1.0) * f);
        } else {
            p.add_scaled_vector(v, t);
        }
        left -= t;
    }
    if left > 0.0 {
        p.add_scaled_vector(v, left);
    }
    p
}
fn scalar_track(keys: &[Key], age: f64, mut value: f64) -> f64 {
    let mut left = age;
    for key in keys {
        if left < 0.0 {
            break;
        }
        let span = key.lifetime;
        let f = if span > 0.0 {
            (left / span).min(1.0)
        } else {
            1.0
        };
        if let Some(s) = key.scale {
            value *= 1.0 + (s - 1.0) * f;
        } else if let Some(m) = key.r#move {
            value += m.scalar() * f;
        } else if let Some(r) = key.reach {
            value += (r.scalar() - value) * f;
        }
        left -= span;
        if f < 1.0 {
            break;
        }
    }
    value
}
fn color_track(keys: &[Key], age: f64, c: &mut [f64; 4]) {
    let mut left = age;
    for key in keys {
        let span = key.lifetime;
        let f = if span > 0.0 {
            (left / span).min(1.0)
        } else {
            1.0
        };
        if let Some(r) = key.reach {
            let r = r.color();
            for i in 0..4 {
                c[i] += (r[i] / 255.0 - c[i]) * f;
            }
        } else if let Some(m) = key.r#move {
            let m = m.color();
            for i in 0..4 {
                c[i] = (c[i] + (m[i] / 255.0) * f).clamp(0.0, 1.0);
            }
        }
        left -= span;
        if f < 1.0 {
            break;
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "web"))]
mod web;

pub mod unit_content;

pub mod units;

pub mod props;

pub mod vehicles;

pub mod items;
pub mod items_layout;
pub mod ordnance;
pub mod ordnance_layout;
pub mod scene;
pub mod turrets;

pub mod network;
pub mod network_layout;
pub mod leaving_warning;
pub mod page_layout;
pub mod lights;
pub mod interface_batch;
pub mod interface_batch_layout;

pub mod rider_sources;

pub mod unit_collision;
