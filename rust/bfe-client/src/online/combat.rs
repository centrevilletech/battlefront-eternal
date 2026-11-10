//! Network combat admission feeds Ordnance and Presentation directly. The
//! retained wire identity is independent of a trigger: pellets, terminals and
//! automatic rounds keep their own visual lives without an authoritative World.
use super::{
    index::Index,
    models::Model,
    particles::{self, Particles},
    receiver::Receiver,
};
use crate::{render_bridge::Bridge, resources::Resources};
use bfe_content_native::BattleContent;
use bfe_presentation::{
    network_layout::{net_kind as wk, net_record as wire},
    ordnance::Ordnance,
    ordnance_layout::{
        call_kind as ck, net_command as n, net_flag as nf, net_kind as nk, ordnance_call as call, ordnance_flag as flag,
        ordnance_frame as f, ordnance_header as h, ordnance_kind as kind, ordnance_limits as limits, ordnance_record as r,
        ordnance_style as s, VERSION,
    },
};
use serde::Deserialize;
use three::{Matrix4, Quaternion, Vector3};
const BOLTS: usize = 4095;
#[derive(Clone, Copy, Default)]
// NetworkCombatViews retains a visual life separately from a trigger edge.
// Automatic fire, pellets and a terminal all have their own wire identity.
// The local bounded slot supplies an exactly representable numeric draw key;
// combining two u32 wire fields in an f64 would lose low identity bits.
struct Bolt {
    key: u32,
    active: bool,
    id: u32,
    generation: u32,
    tick: f64,
    expiry: f64,
    style: usize,
    position: [f64; 3],
    direction: [f64; 3],
    offset: [f64; 3],
    speed: f64,
    life: f64,
    merge: f64,
    flight: f64,
    execution: f64,
    started: bool,
    terminal: bool,
    impact: Option<(Option<usize>, [f64; 3])>,
}
struct Style {
    model: Option<Model>,
    impact: [Option<usize>; 6],
    explosion: [Option<usize>; 2],
}
#[derive(Clone, Copy, Default)]
// Dynamic projectiles carry samples only. The shared Ordnance net owner
// interpolates, coasts, smooths and expires them; this shell holds the model
// recipe and its published matrix. It never flies a second projectile or
// applies damage, bounces or authoritative collision.
struct Net {
    active: bool,
    id: u32,
    generation: u32,
    expiry: f64,
    style: usize,
    matrix: [f64; 16],
    shown: bool,
}
// Native admission joins network records to the existing visual owners.
// Ordnance owns bolt and dynamic-projectile drawing; Presentation owns effect
// lifetime and particle sampling. Resource handles remain numeric after load.
// Retiring a visual returns only its host slot, not a gameplay entity lease.
pub struct Combat {
    pub ordnance: Ordnance,
    pub particles: Particles,
    pub debris: super::debris::Debris,
    effects: super::effects::Spawner,
    gpu_indices: Vec<Option<usize>>,
    definitions: Vec<Option<usize>>,
    styles: Vec<Style>,
    quads: Vec<usize>,
    bolts: Vec<Bolt>,
    bound: Index,
    free: Vec<usize>,
    net: Vec<Net>,
    net_bound: Index,
    net_free: Vec<usize>,
    scratch: bfe_sim::wire::cosmetics::Compact,
    frame: u64,
    epoch: f64,
    tick: f64,
    pub visible: usize,
    pub refused: u64,
    pub effect_names: Vec<(String, usize)>,
    serial: u32,
    pub total_quads: u64,
    pub total_effects: u64,
    pub received: [u64; 9],
    pending: Vec<(Option<usize>, [f64; 3], u32, f64)>,
}
fn number(v: &bfe_content::Value, key: &str) -> f64 {
    v.get(key).and_then(bfe_content::Value::as_f64).unwrap_or(0.)
}
fn color(v: &bfe_content::Value, key: &str) -> [f64; 3] {
    let a = v.get(key).and_then(bfe_content::Value::as_array);
    let alpha = a.and_then(|a| a.get(3)).and_then(bfe_content::Value::as_f64).unwrap_or(255.) / 255.;
    std::array::from_fn(|i| a.and_then(|a| a.get(i)).and_then(bfe_content::Value::as_f64).unwrap_or(255.) / 255. * alpha)
}
impl Combat {
    pub fn new(
        content: &BattleContent,
        mut prepared: particles::Prepared,
        bridge: &mut Bridge,
        textures: &[u32],
        resources: &mut Resources,
        collision: super::replay::Collision,
        world: u32,
    ) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let library = content.effects.iter().find(|(name, _)| name == "battle").map(|(_, e)| e).ok_or("missing battle effect library")?;
        let effect_names = library.handles.clone();
        let effect = |name: &str| effect_names.iter().find(|(n, _)| n == name).map(|(_, id)| *id);
        let mut ordnance = Ordnance::new();
        let mut definitions = vec![None; defs.registry.combat.len() + 1];
        let mut styles = Vec::new();
        let mut quads = Vec::new();
        // Ordnance's bounded quad handles map to renderer batches. Effects may
        // already own other batches, so renderer ids are never used as slots.
        let mut quad = |name: &str, prepared: &mut particles::Prepared| -> f64 {
            let Some(batch) = prepared.ordnance(name) else {
                return -1.;
            };
            if let Some(id) = quads.iter().position(|b| *b == batch) {
                return id as f64;
            }
            if quads.len() == limits::HANDLES {
                return -1.;
            }
            quads.push(batch);
            (quads.len() - 1) as f64
        };
        // Ordnance.set_style consumes one style-input record at a time.
        // Definition ids are wire ids, whereas styles are densely admitted
        // visual resources. Keep an explicit map so omitted unsupported or
        // model-less definitions cannot shift every later style association.
        for (id, (_, def)) in defs.registry.combat.iter().enumerate() {
            let base = def.get("base").and_then(bfe_content::Value::as_str).unwrap_or("");
            if !["bolt", "laser", "bullet", "fatray", "beam", "shell", "missile", "grenade", "sticky", "explosion"].contains(&base) {
                continue;
            }
            if styles.len() == limits::STYLES {
                continue;
            }
            let index = styles.len();
            definitions[id + 1] = Some(index);
            let row = &mut ordnance.style_input[..s::STRIDE];
            row.fill(0.);
            let laser = def.get("LaserTexture").and_then(bfe_content::Value::as_str).unwrap_or("");
            row[s::CORE_BATCH] = if laser.is_empty() { -1. } else { quad(laser, &mut prepared) };
            row[s::GLOW_BATCH] = quad("lightglow", &mut prepared);
            row[s::BEAM_BATCH] = if base == "beam" { quad(if laser.is_empty() { "lightglow" } else { laser }, &mut prepared) } else { -1. };
            for (at, key) in [
                (s::LASER_WIDTH, "LaserWidth"),
                (s::LASER_LENGTH, "LaserLength"),
                (s::GLOW_RAW, "GlowLength"),
                (s::BLUR_LENGTH, "BlurLength"),
                (s::FADE_OUT_TIME, "FadeOutTime"),
                (s::LIGHT_RADIUS, "LightRadius"),
            ] {
                row[at] = number(def, key);
            }
            row[s::GLOW_LENGTH] = def.get("GlowLength").and_then(bfe_content::Value::as_f64).unwrap_or_else(|| {
                if row[s::LASER_LENGTH] != 0. {
                    row[s::LASER_LENGTH] * 3.
                } else {
                    0.
                }
            });
            row[s::LIGHT_COLOR..s::LIGHT_COLOR + 3].copy_from_slice(&color(def, "LightColor"));
            row[s::GLOW_COLOR..s::GLOW_COLOR + 3].copy_from_slice(&color(def, "LaserGlowColor"));
            row[s::FLIGHT] = if base == "bullet" || base == "fatray" {
                1.
            } else if base == "shell" {
                2.
            } else {
                0.
            };
            row[s::GRAVITY] = def.get("Gravity").and_then(bfe_content::Value::as_f64).unwrap_or(1.);
            ordnance.set_style(index);
            let terrain = def.get("ImpactEffectTerrain").and_then(bfe_content::Value::as_str);
            let static_hit = def.get("ImpactEffectStatic").and_then(bfe_content::Value::as_str).or(terrain);
            let impact = [
                terrain,
                static_hit,
                def.get("ImpactEffectSoft").and_then(bfe_content::Value::as_str).or(static_hit),
                def.get("ImpactEffectRigid").and_then(bfe_content::Value::as_str).or(static_hit),
                Some(def.get("ImpactEffectWater").and_then(bfe_content::Value::as_str).unwrap_or("watersplash_ord")),
                static_hit,
            ]
            .map(|name| name.and_then(effect));
            let explosion = [
                def.get("Effect").and_then(bfe_content::Value::as_str),
                def.get("WaterEffect")
                    .and_then(bfe_content::Value::as_str)
                    .or_else(|| def.get("Effect").and_then(bfe_content::Value::as_str)),
            ]
            .map(|name| name.and_then(effect));
            let name = def.get("GeometryName").and_then(bfe_content::Value::as_str).unwrap_or("");
            styles.push(Style { model: Model::new(content, bridge, textures, name, crate::shadows::COUNT)?, impact, explosion });
        }
        let effects = bfe_presentation::Presentation::new(
            serde_json::from_value(library.descriptor.clone()).map_err(|e| e.to_string())?,
            bfe_presentation::Limits { effects: 1024, emitters: 4096, particles: 16384, commands: 2048, events: 32768 },
        );
        let particles = prepared.admit(content, bridge, textures, resources)?;
        let debris = super::debris::Debris::new(content, bridge, textures, &effect_names, collision, world)?;
        let mut scratch = bfe_sim::wire::cosmetics::Compact::default();
        scratch.offset.reserve(3);
        Ok(Self {
            ordnance,
            particles,
            debris,
            effects: super::effects::Spawner::new(effects),
            gpu_indices: library.gpu_indices.clone(),
            definitions,
            styles,
            quads,
            bolts: vec![Bolt::default(); BOLTS],
            bound: Index::new(BOLTS),
            free: (0..BOLTS).rev().collect(),
            net: vec![Net::default(); limits::NET_SLOTS],
            net_bound: Index::new(limits::NET_SLOTS),
            net_free: (0..limits::NET_SLOTS).rev().collect(),
            scratch,
            frame: 0,
            epoch: 0.,
            tick: 0.,
            visible: 0,
            refused: 0,
            effect_names,
            serial: 0,
            total_quads: 0,
            total_effects: 0,
            received: [0; 9],
            pending: Vec::with_capacity(2048),
        })
    }
    // Effects.play for a one-shot: its handle is never used again. A missing
    // group plays nothing; a full batch or pool is counted and dropped.
    fn play(&mut self, group: Option<usize>, position: [f64; 3], seed: u32) {
        let Some(group) = group else {
            return;
        };
        if self.effects.start(group, position, [0.; 3], seed).is_some() {
            self.total_effects += 1;
        } else {
            self.refused += 1;
        }
    }
    // An effect another owner asks for between two frames (a droid's death
    // blast). It joins the next frame's spawn batch, as any deferred impact.
    pub fn present_effect(&mut self, group: Option<usize>, position: [f64; 3], seed: u32) {
        self.defer(group, position, seed, self.tick);
    }
    fn defer(&mut self, group: Option<usize>, position: [f64; 3], id: u32, tick: f64) {
        if group.is_none() {
            return;
        }
        if self.pending.len() == self.pending.capacity() {
            // Preserve the newest events after a long unpresented window.
            // Capacity exhaustion never interrupts packet or render progress.
            self.pending.remove(0);
            self.refused += 1;
        }
        self.pending.push((group, position, id, tick));
    }
    pub fn frame(&mut self, receiver: &Receiver, tick: f64, dt: f64, bridge: &mut Bridge) {
        use bfe_presentation::layout::frame as ef;
        self.frame += 1;
        self.tick = tick;
        self.visible = 0;
        self.effects.owner.input[ef::VERSION] = bfe_presentation::layout::VERSION as f64;
        self.effects.owner.input[ef::FRAME] = self.frame as f64;
        self.effects.owner.input[ef::EPOCH] = receiver.epoch;
        self.effects.owner.input[ef::TICK] = tick;
        self.effects.owner.input[ef::PLAYOUT_TICK] = tick;
        self.effects.owner.input[ef::INPUT_CLOCK] = tick;
        self.effects.owner.input[ef::DT] = dt;
        self.effects.owner.input[ef::PRESENT] = 1.;
        self.effects.owner.input[ef::COMMANDS] = 0.;
        let mut net_count = 0;
        // Epoch reset releases visual identities without reconstructing their
        // resource banks. Pending effects from another battle cannot cross
        // this boundary, even when the next battle reuses every wire id.
        // Native Ordnance receives its own reset in the same command batch.
        if self.epoch != receiver.epoch {
            self.epoch = receiver.epoch;
            self.pending.clear();
            self.debris.clear();
            self.bound.clear();
            self.free.clear();
            self.free.extend((0..BOLTS).rev());
            self.bolts.fill(Bolt::default());
            self.net_bound.clear();
            self.net_free.clear();
            self.net_free.extend((0..limits::NET_SLOTS).rev());
            self.net.fill(Net::default());
            self.ordnance.net_input[..n::STRIDE].fill(0.);
            self.ordnance.net_input[n::KIND] = nk::RESET as f64;
            net_count = 1;
        }
        for field in receiver.publications.iter() {
            let row = &field.row;
            if row[wire::KIND] != wk::COMBAT as f64 {
                continue;
            }
            // Session's changed-field lease was copied by the receiver before
            // another packet could reuse it. Borrow that journal synchronously;
            // no combat object or payload vector is constructed on this path.
            // Wire schemas below retain the browser's shared decoder rules.
            let bytes = &field.bytes[..field.length];
            let schema = row[wire::SCHEMA] as u32;
            if let Some(count) = self.received.get_mut(schema as usize) {
                *count += 1;
            }
            let id = row[wire::ID] as u32;
            let generation = row[wire::GENERATION] as u32;
            let stamp = row[wire::TICK];
            if schema == 1 || schema == 2 {
                if bfe_sim::wire::cosmetics::decode_bolt_into(schema as f64, stamp, bytes, &mut self.scratch).is_err() {
                    self.refused += 1;
                    continue;
                }
                let source = &self.scratch;
                // Reject unavailable styles before claiming a retained slot.
                let Some(style) = self.definitions.get(source.definition as usize).copied().flatten() else {
                    continue;
                };
                let key = (id, generation);
                let slot = if let Some(&slot) = self.bound.get(&key) {
                    slot
                } else {
                    if schema == 2 {
                        let style = self.definitions.get(source.definition as usize).copied().flatten();
                        let group = style.and_then(|s| self.styles[s].impact.get(source.surface as usize).copied().flatten());
                        let position = source.view.position.unwrap_or_default();
                        self.defer(group, position, id, stamp);
                        continue;
                    }
                    let Some(slot) = self.free.pop() else {
                        self.refused += 1;
                        continue;
                    };
                    self.bound.insert(key, slot);
                    slot
                };
                let old = self.bolts[slot];
                if old.active && (old.terminal || stamp < old.tick) {
                    continue;
                }
                if schema == 1 {
                    self.serial = self.serial.wrapping_add(1).max(1);
                    self.bolts[slot] = Bolt {
                        key: self.serial,
                        active: true,
                        id,
                        generation,
                        tick: stamp,
                        expiry: source.expiry_tick,
                        style,
                        position: source.view.position.unwrap_or_default(),
                        direction: source.view.direction.unwrap_or_default(),
                        offset: std::array::from_fn(|i| source.offset.get(i).copied().unwrap_or(0.)),
                        speed: source.view.speed,
                        life: source.view.life,
                        merge: source.merge,
                        flight: source.launch_tick,
                        execution: source.execution_tick,
                        ..Default::default()
                    };
                } else {
                    let point = source.view.position.unwrap_or_default();
                    let bolt = &mut self.bolts[slot];
                    if bolt.speed > 0. {
                        let distance = (0..3).map(|i| (point[i] - bolt.position[i]) * bolt.direction[i]).sum::<f64>().max(0.);
                        bolt.life = bolt.life.min(distance / bolt.speed);
                    }
                    bolt.terminal = true;
                    bolt.tick = stamp;
                    bolt.expiry = source.expiry_tick;
                    let group = self.styles[style].impact.get(source.surface as usize).copied().flatten();
                    self.bolts[slot].impact = Some((group, point));
                }
            } else if schema == 5 && bytes.len() == 33 {
                let definition = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
                let group = self
                    .definitions
                    .get(definition)
                    .copied()
                    .flatten()
                    .and_then(|style| self.styles[style].explosion.get(bytes[32] as usize).copied().flatten());
                let position = std::array::from_fn(|i| f64::from_le_bytes(bytes[8 + i * 8..16 + i * 8].try_into().unwrap()));
                self.defer(group, position, id, stamp);
            } else if schema == 6 || schema == 7 {
                let valid = if schema == 6 { bytes.len() == 82 } else { bytes.len() == 4 };
                if !valid {
                    self.refused += 1;
                    continue;
                }
                let style = if schema == 6 {
                    let definition = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
                    let Some(style) = self.definitions.get(definition).copied().flatten() else {
                        continue;
                    };
                    Some(style)
                } else {
                    None
                };
                let key = (id, generation);
                let slot = if let Some(&slot) = self.net_bound.get(&key) {
                    slot
                } else {
                    if schema == 7 {
                        continue;
                    }
                    let Some(slot) = self.net_free.pop() else {
                        self.refused += 1;
                        continue;
                    };
                    self.net_bound.insert(key, slot);
                    self.net[slot] = Net { active: true, id, generation, ..Default::default() };
                    slot
                };
                if net_count == limits::NET_COMMANDS {
                    self.ordnance.net_flush(net_count);
                    net_count = 0;
                }
                let x = &mut self.ordnance.net_input[net_count * n::STRIDE..][..n::STRIDE];
                x.fill(0.);
                x[n::KIND] = if schema == 7 { nk::END } else { nk::SAMPLE } as f64;
                x[n::SLOT] = slot as f64;
                x[n::GENERATION] = generation as f64;
                x[n::SCHEMA] = schema as f64;
                x[n::TICK] = stamp;
                self.net[slot].expiry = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64;
                x[n::EXPIRY_TICK] = self.net[slot].expiry;
                if schema == 6 {
                    let style = style.unwrap();
                    self.net[slot].style = style;
                    x[n::STYLE] = style as f64;
                    x[n::TYPE] = bytes[8] as f64;
                    x[n::FLAGS] = if self.styles[style].model.is_some() { (1 << nf::MESH) as f64 } else { 0. };
                    let scalar = |at: usize| f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as f64;
                    for i in 0..3 {
                        x[n::POSITION + i] = scalar(25 + i * 4);
                        x[n::VELOCITY + i] = scalar(37 + i * 4);
                        x[n::END + i] = scalar(65 + i * 4);
                    }
                    for i in 0..4 {
                        x[n::QUATERNION + i] = scalar(49 + i * 4);
                    }
                    x[n::LIFE] = scalar(77);
                }
                net_count += 1;
            } else if schema == 8 {
                #[derive(Deserialize)]
                struct Event<'a> {
                    kind: &'a str,
                    #[serde(default)]
                    name: Option<&'a str>,
                    #[serde(default)]
                    position: Option<[f64; 3]>,
                }
                if let Ok(event) = serde_json::from_slice::<Event>(bytes) {
                    if event.kind == "effect" {
                        let group = event.name.and_then(|name| self.effect_names.iter().find(|(n, _)| n == name).map(|(_, g)| *g));
                        self.play(group, event.position.unwrap_or_default(), id);
                    }
                }
            }
        }
        let mut count = 0;
        // NetworkCombatViews.present resolves a late launch at its first draw,
        // bounded by the launch presentation interval. Thereafter Ordnance
        // sees its retained clock and draws the whole bolt batch once.
        // Frame frequency cannot advance a second projectile flight clock.
        for (slot, bolt) in self.bolts.iter_mut().enumerate() {
            if !bolt.active {
                continue;
            }
            if tick > bolt.expiry + 24. {
                self.bound.remove(&(bolt.id, bolt.generation));
                bolt.active = false;
                self.free.push(slot);
                continue;
            }
            if !bolt.started {
                bolt.started = true;
                if tick > bolt.flight {
                    bolt.flight = tick.min(bolt.flight.max(bolt.execution) + 24.);
                }
            }
            let x = &mut self.ordnance.input[f::STRIDE + count * r::STRIDE..][..r::STRIDE];
            x.fill(0.);
            x[r::KIND] = kind::BOLT as f64;
            x[r::KEY] = bolt.key as f64;
            x[r::STYLE] = bolt.style as f64;
            x[r::SOURCE] = slot as f64;
            x[r::FLAGS] = ((1 << flag::HAS_CLOCK) | (1 << flag::HAS_OFFSET) | (1 << flag::RESOLVE_SHOWN)) as f64;
            x[r::ALPHA] = 1.;
            x[r::CLOCK_TICK] = tick;
            x[r::FADE] = 1.;
            x[r::SPEED] = bolt.speed;
            x[r::LIFE] = bolt.life;
            x[r::MERGE] = bolt.merge;
            x[r::FLIGHT_TICK] = bolt.flight;
            x[r::POSITION..r::POSITION + 3].copy_from_slice(&bolt.position);
            x[r::DIRECTION..r::DIRECTION + 3].copy_from_slice(&bolt.direction);
            x[r::OFFSET..r::OFFSET + 3].copy_from_slice(&bolt.offset);
            count += 1;
        }
        // NetworkCombatViews.settle ties an impact to the visible flight,
        // not packet receipt. A lost launch retains the terminal's own tick.
        for slot in 0..self.bolts.len() {
            let bolt = &mut self.bolts[slot];
            if bolt.active && bolt.started && tick >= bolt.flight + bolt.life * 60. {
                if let Some((group, position)) = bolt.impact.take() {
                    let due = bolt.flight + bolt.life * 60.;
                    let id = bolt.id;
                    if tick - due <= 18. {
                        self.defer(group, position, id, due);
                    }
                }
            }
        }
        let mut pending = 0;
        while pending < self.pending.len() {
            let (group, position, id, due) = self.pending[pending];
            if tick >= due {
                self.pending.remove(pending);
                if tick - due <= 18. {
                    self.play(group, position, id);
                }
            } else {
                pending += 1;
            }
        }

        let x = &mut self.ordnance.input[f::STRIDE + count * r::STRIDE..][..r::STRIDE];
        x.fill(0.);
        x[r::KIND] = kind::NET as f64;
        x[r::CLOCK_TICK] = tick;
        count += 1;
        self.ordnance.input[f::VERSION] = VERSION as f64;
        self.ordnance.input[f::FRAME] = self.frame as f64;
        self.ordnance.input[f::RECORDS] = count as f64;
        self.ordnance.input[f::NET_COMMANDS] = net_count as f64;
        self.ordnance.present();
        for call in self.ordnance.calls().chunks_exact(call::STRIDE).take(self.ordnance.header[h::CALLS] as usize) {
            let slot = call[call::SOURCE] as usize;
            if let Some(net) = self.net.get_mut(slot) {
                if call[call::KIND] == ck::NET_MESH as f64 {
                    let mut matrix = Matrix4::default();
                    matrix.compose(
                        &Vector3::new(call[call::A], call[call::A + 1], call[call::A + 2]),
                        &Quaternion::new(call[call::D], call[call::D + 1], call[call::D + 2], call[call::D + 3]),
                        &Vector3::new(1., 1., 1.),
                    );
                    net.matrix = matrix.elements;
                    net.shown = true;
                } else if call[call::KIND] == ck::NET_REMOVE as f64 {
                    net.shown = false;
                }
            }
        }
        for (slot, net) in self.net.iter_mut().enumerate() {
            if net.active && tick > net.expiry + 24. {
                self.net_bound.remove(&(net.id, net.generation));
                net.active = false;
                net.shown = false;
                self.net_free.push(slot);
            }
        }
        // Debris.update runs before Effects.update: a piece's trail moves
        // and its bounce plays in this frame's batch.
        self.debris.step(dt, &mut self.effects, &mut self.total_effects, &mut self.refused);
        self.effects.frame();
        // The owners publish planar quad attributes and analytic particle
        // records. The retained renderer batches consume those publications;
        // creating one mesh per round would reintroduce resource admission
        // and allocation into the middle of a displayed battle frame.
        self.particles.begin(bridge);
        let arrays = self.ordnance.quad_arrays();
        for (handle, batch) in self.quads.iter().enumerate() {
            for i in 0..self.ordnance.counts()[handle] as usize {
                let index = handle * limits::QUADS_PER_HANDLE + i;
                let mut quad = [0.; 21];
                let mut at = 0;
                for (array, size) in arrays.iter().zip([3, 4, 4, 3, 3, 4]) {
                    quad[at..at + size].copy_from_slice(&array[index * size..index * size + size]);
                    at += size;
                }
                self.particles.quad(bridge, *batch, &quad);
                self.visible += 1;
                self.total_quads += 1;
            }
        }
        self.particles.effects(bridge, &self.effects.owner, &self.gpu_indices);
        self.particles.finish(bridge);
    }
    pub fn record_budget(&self) -> usize {
        self.net.iter().filter(|n| n.active && n.shown).filter_map(|n| self.styles[n.style].model.as_ref()).map(|m| m.rows.len()).sum::<usize>()
            + self.debris.record_budget()
    }
    pub fn append(&self, bridge: &mut Bridge, mut records: usize, mut bones: usize) -> (usize, usize) {
        for net in &self.net {
            if net.active && net.shown {
                if let Some(model) = self.styles.get(net.style).and_then(|s| s.model.as_ref()) {
                    (records, bones) = model.append(bridge, &net.matrix, records, bones);
                }
            }
        }
        self.debris.append(bridge, records, bones)
    }
}
