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
use bfe_content_native::{loader::authored_json, BattleContent};
use bfe_presentation::{
    network_layout::{net_kind as wk, net_record as wire},
    ordnance::{style, Ordnance},
    ordnance_layout::{
        call_kind as ck, net_command as n, net_flag as nf, net_kind as nk, net_type as nt, ordnance_call as call, ordnance_flag as flag,
        ordnance_frame as f, ordnance_header as h, ordnance_kind as kind, ordnance_limits as limits, ordnance_record as r,
        ordnance_style as s, VERSION,
    },
};
use bfe_presentation::sniper_layout::{sniper_flare as flare, sniper_limits as sn, sniper_streak as streak};
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
    // The effect a round of it trails as it flies (TrailEffect).
    trail: Option<usize>,
    // A class with a FadeOutTime (the sniper rifles): its tracer's authored
    // colour (`LaserGlowColor || LightColor`, 0 to 255) and how far a round
    // can reach (Velocity times LifeSpan).
    rail: Option<([f64; 3], f64)>,
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
    // Its trail effect's handle, while it flies.
    trail: Option<u32>,
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
    // Effects waiting for their tick: group, place, record id, the tick,
    // and whether a sniper round's impact (which its drawn tracer delays).
    pending: Vec<(Option<usize>, [f64; 3], u32, f64, bool)>,
    /// The boltEffects and sniperEffects settings: Enhanced unless "classic".
    pub enhanced_bolts: bool,
    pub enhanced_sniper: bool,
    // BlasterBatch: the renderer batch of the Enhanced bolts' capsules,
    // where its planes sit in the effect data, and this frame's count.
    blasters: u32,
    blasters_at: usize,
    blaster_count: usize,
    /// Enhanced bolt capsules handed to the renderer so far.
    pub total_blasters: u64,
    // SniperTrace (src/sniper-trace.js): the Enhanced sniper look's owner,
    // its two renderer batches and where their rows sit in the effect data.
    sniper: bfe_presentation::sniper::Sniper,
    streaks: u32,
    flares: u32,
    streaks_at: usize,
    // This frame's sniper rounds as the ordnance owner drew them (bolt key,
    // style, start, head, direction, origin, ended), the impacts waiting for
    // a tracer, and the head shots.
    rail_touches: Vec<[f64; 15]>,
    rail_impacts: Vec<(Option<usize>, [f64; 3], u32)>,
    rail_heads: Vec<[f64; 3]>,
    // Impacts handed to the trace, by the token it calls them back with.
    rail_waiting: Vec<(u32, Option<usize>, [f64; 3], u32)>,
    rail_token: u32,
    // `Math.random()` for the trace's sparks and wisps; decides nothing shared.
    rail_rng: jsrt::rng::ClassicRandom,
    // Seconds of frames presented (lighting.time).
    clock: f64,
    /// Tracer capsules and flares handed to the renderer so far.
    pub total_streaks: u64,
    // The Classic jet exhausts asked for this frame (game.js updateJetFlame
    // with jetpackEffects "classic"): the body, the effect group, where its
    // ribcage is and how it moves and is turned, and the effect once played.
    flames: Vec<Flame>,
    /// Classic jet exhaust effects started so far.
    pub total_flames: u64,
}
struct Flame {
    unit: (u32, u32),
    group: usize,
    position: [f64; 3],
    velocity: [f64; 3],
    turn: [f64; 4],
    handle: Option<u32>,
    seen: bool,
}
const STREAK_FLOATS: usize = sn::STREAKS * streak::STRIDE + 1;
const FLARE_FLOATS: usize = sn::FLARES * flare::STRIDE;
// The Enhanced capsules' floats: head, tail, colour and size for each, then
// the batch's one value (the pixel angle).
const BLASTER_FLOATS: usize = limits::BLASTERS * 14 + 1;
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
            // PageOrdnance.send and blasterLook: the Enhanced capsule's
            // colour and proportions, from the same authored numbers. A
            // heavy gun (no GlowLength) takes its hue from its laser
            // texture's saturated texels when that texture has one
            // (blaster-bolts.js textureHue).
            let authored: style::Content = serde_json::from_value(authored_json(def)).unwrap_or_default();
            let look = style::row(&authored, true);
            row[s::LOOK_COLOR..s::LOOK_COLOR + 3].copy_from_slice(&look[s::LOOK_COLOR..s::LOOK_COLOR + 3]);
            for at in [s::LOOK_GLOW, s::LOOK_CORE, s::LOOK_LENGTH, s::LOOK_BRIGHTNESS, s::LOOK_LIGHT_RADIUS] {
                row[at] = look[at];
            }
            let hue = style::resources(&authored, 0, true)["hue"].as_str().and_then(|name| prepared.textures.get(name)).map(|image| {
                let image = &content.textures.images[*image];
                let texels = (image.width as usize * image.height as usize * 4).min(image.data.len());
                // Only plain RGBA images can be read here; a compressed one
                // keeps the authored colour.
                if matches!(image.format, bfe_render::texture_format::Format::Rgba) { style::texture_hue(&image.data[..texels]) } else { Vec::new() }
            });
            if let Some(hue) = hue.filter(|hue| !hue.is_empty()) {
                row[s::LOOK_COLOR..s::LOOK_COLOR + 3].copy_from_slice(&style::vivid(&hue, 1.6));
            }
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
            let trail = def.get("TrailEffect").and_then(bfe_content::Value::as_str).and_then(effect);
            let rail = (number(def, "FadeOutTime") > 0.).then(|| {
                let rgb = ["LaserGlowColor", "LightColor"].iter().find_map(|key| def.get(key).and_then(bfe_content::Value::as_array));
                let color = std::array::from_fn(|i| rgb.and_then(|c| c.get(i)).and_then(bfe_content::Value::as_f64).unwrap_or([120., 160., 255.][i]));
                (color, number(def, "Velocity") * number(def, "LifeSpan"))
            });
            styles.push(Style { model: Model::new(content, bridge, textures, name, crate::shadows::COUNT)?, impact, explosion, trail, rail });
        }
        let effects = bfe_presentation::Presentation::new(
            serde_json::from_value(library.descriptor.clone()).map_err(|e| e.to_string())?,
            bfe_presentation::Limits { effects: 1024, emitters: 4096, particles: 16384, commands: 2048, events: 32768 },
        );
        let mut particles = prepared.admit(content, bridge, textures, resources)?;
        // BlasterBatch: one instanced draw of every Enhanced bolt, after the
        // particles' own data.
        let blasters = bridge.effect_batch(bfe_render::effect_layout::BLASTER as u32, 0, crate::shadows::COUNT > 0, -1, limits::BLASTERS as u32, -1, &[])?;
        let blasters_at = particles.words;
        particles.words += BLASTER_FLOATS;
        // SniperTrace's two instanced draws: capsules (bolts, tracer lines,
        // vapour, sparks), then flares.
        let streaks = bridge.effect_batch(bfe_render::effect_layout::SNIPER_STREAKS as u32, 0, crate::shadows::COUNT > 0, -1, sn::STREAKS as u32, -1, &[])?;
        let flares = bridge.effect_batch(bfe_render::effect_layout::SNIPER_FLARES as u32, 0, crate::shadows::COUNT > 0, -1, sn::FLARES as u32, -1, &[])?;
        let streaks_at = particles.words;
        particles.words += STREAK_FLOATS + FLARE_FLOATS;
        bridge.fx_reserve(particles.words as u32);
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
            enhanced_bolts: true,
            enhanced_sniper: true,
            blasters,
            blasters_at,
            blaster_count: 0,
            total_blasters: 0,
            sniper: Default::default(),
            streaks,
            flares,
            streaks_at,
            rail_touches: Vec::with_capacity(64),
            rail_impacts: Vec::with_capacity(64),
            rail_heads: Vec::with_capacity(64),
            rail_waiting: Vec::with_capacity(256),
            rail_token: 0,
            rail_rng: jsrt::rng::ClassicRandom::new(
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0., |t| t.subsec_nanos() as f64),
            ),
            clock: 0.,
            total_streaks: 0,
            flames: Vec::with_capacity(16),
            total_flames: 0,
        })
    }
    // Effects.play for a one-shot: its handle is never used again. A missing
    // group plays nothing; a full batch or pool is counted and dropped.
    fn play(&mut self, group: Option<usize>, position: [f64; 3], normal: Option<[f64; 3]>, seed: u32) {
        let Some(group) = group else {
            return;
        };
        if self.effects.start(group, position, [0.; 3], normal, seed).is_some() {
            self.total_effects += 1;
        } else {
            self.refused += 1;
        }
    }
    /// A body jetting with the Classic look this frame: its class's
    /// JetExhaust effect plays at its ribcage and follows it
    /// (`effects.play(name, flameAt, null, velocity, flameTurn)`, then
    /// `jetFlame.move(...)`), from the next frame's batch on. A frame
    /// without this call stops it. The effect's emitters stay where it
    /// authors them; the browser also moves each to the pack's nearest
    /// outlet (attachNozzles).
    pub fn flame(&mut self, unit: (u32, u32), group: usize, position: [f64; 3], velocity: [f64; 3], turn: [f64; 4]) {
        if let Some(flame) = self.flames.iter_mut().find(|flame| flame.unit == unit) {
            (flame.position, flame.velocity, flame.turn, flame.seen) = (position, velocity, turn, true);
        } else if self.flames.len() < self.flames.capacity() {
            self.flames.push(Flame { unit, group, position, velocity, turn, handle: None, seen: true });
        }
    }
    // An effect another owner asks for between two frames (a droid's death
    // blast). It joins the next frame's spawn batch, as any deferred impact.
    pub fn present_effect(&mut self, group: Option<usize>, position: [f64; 3], seed: u32) {
        self.defer(group, position, seed, self.tick, false);
    }
    fn defer(&mut self, group: Option<usize>, position: [f64; 3], id: u32, tick: f64, rail: bool) {
        if group.is_none() {
            return;
        }
        if self.pending.len() == self.pending.capacity() {
            // Preserve the newest events after a long unpresented window.
            // Capacity exhaustion never interrupts packet or render progress.
            self.pending.remove(0);
            self.refused += 1;
        }
        // NetworkCombatViews.present walks its records in the order their
        // ids first arrived, and ids are handed out as events happen: a
        // round's impact (its id from the launch) shows before the explosion
        // it sets off (a later id) when both fall due together. Kept in id
        // order, the effects due in one frame start in that order.
        let at = self.pending.partition_point(|kept| kept.2 <= id);
        self.pending.insert(at, (group, position, id, tick, rail));
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
        // The jet exhausts asked for since the last batch: started, moved,
        // or stopped when their body no longer jets.
        let mut at = 0;
        while at < self.flames.len() {
            let flame = &mut self.flames[at];
            if !flame.seen {
                if let Some(handle) = flame.handle {
                    self.effects.stop(handle);
                }
                self.flames.swap_remove(at);
                continue;
            }
            flame.seen = false;
            match flame.handle {
                Some(handle) => self.effects.sync(handle, flame.position, Some(flame.turn), Some(flame.velocity)),
                None => {
                    flame.handle = self.effects.start_turned(flame.group, flame.position, flame.velocity, flame.turn, flame.unit.0);
                    self.total_flames += flame.handle.is_some() as u64;
                }
            }
            at += 1;
        }
        let mut net_count = 0;
        // Epoch reset releases visual identities without reconstructing their
        // resource banks. Pending effects from another battle cannot cross
        // this boundary, even when the next battle reuses every wire id.
        // Native Ordnance receives its own reset in the same command batch.
        if self.epoch != receiver.epoch {
            self.epoch = receiver.epoch;
            self.pending.clear();
            self.flames.clear();
            self.debris.clear();
            self.bound.clear();
            self.free.clear();
            self.free.extend((0..BOLTS).rev());
            self.bolts.fill(Bolt::default());
            self.net_bound.clear();
            self.net_free.clear();
            self.net_free.extend((0..limits::NET_SLOTS).rev());
            // A trail emits until it is stopped: none outlives its round.
            for trail in self.net.iter().filter_map(|net| net.trail) {
                self.effects.stop(trail);
            }
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
                        let rail = style.is_some_and(|s| self.styles[s].rail.is_some());
                        self.defer(group, position, id, stamp, rail);
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
                self.defer(group, position, id, stamp, false);
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
                let mut fresh = false;
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
                    fresh = true;
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
                    // ProjectilePresentation.receive: a round or grenade new
                    // to this client starts its TrailEffect where its first
                    // sample puts it, moving and turned as it is. The owner
                    // then says where the trail is each frame.
                    if fresh && bytes[8] as u32 != nt::BEAM {
                        if let Some(group) = self.styles[style].trail {
                            let position = std::array::from_fn(|i| x[n::POSITION + i]);
                            let velocity = std::array::from_fn(|i| x[n::VELOCITY + i]);
                            let turn = std::array::from_fn(|i| x[n::QUATERNION + i]);
                            self.net[slot].trail = self.effects.start_turned(group, position, velocity, turn, id);
                        }
                    }
                    let mut flags = 0;
                    if self.styles[style].model.is_some() {
                        flags |= 1 << nf::MESH;
                    }
                    if self.net[slot].trail.is_some() {
                        flags |= 1 << nf::TRAIL;
                    }
                    x[n::FLAGS] = flags as f64;
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
                    #[serde(default)]
                    normal: Option<[f64; 3]>,
                }
                // presentEntityEvent "effect": the named effect at the
                // record's point, its up axis along the record's normal. A
                // record without a name or a point shows nothing.
                if let Ok(event) = serde_json::from_slice::<Event>(bytes) {
                    if let ("effect", Some(position)) = (event.kind, event.position) {
                        let group = event.name.and_then(|name| self.effect_names.iter().find(|(n, _)| n == name).map(|(_, g)| *g));
                        self.play(group, position, event.normal, id);
                        // `if (event.name === "headshot")
                        // ordnance.presentHeadshot(point)`: the Enhanced
                        // sniper look's flourish.
                        if event.name == Some("headshot") && self.enhanced_sniper && self.rail_heads.len() < self.rail_heads.capacity() {
                            self.rail_heads.push(position);
                        }
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
                    let rail = self.styles[bolt.style].rail.is_some();
                    if tick - due <= 18. {
                        self.defer(group, position, id, due, rail);
                    }
                }
            }
        }
        let mut pending = 0;
        while pending < self.pending.len() {
            let (group, position, id, due, rail) = self.pending[pending];
            if tick >= due {
                self.pending.remove(pending);
                if tick - due <= 18. {
                    // NetworkCombatViews.impact: an Enhanced sniper round's
                    // impact waits for its drawn tracer to arrive
                    // (Ordnance.deferSniperImpact); the trace plays it.
                    if rail && self.enhanced_sniper && self.rail_impacts.len() < self.rail_impacts.capacity() {
                        self.rail_impacts.push((group, position, id));
                    } else {
                        self.play(group, position, None, id);
                    }
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
        // graphics.js: `boltLook.enhanced = settings.get("boltEffects") !==
        // "classic"`; the owner draws either look from this one flag.
        self.ordnance.input[f::ENHANCED_BOLTS] = self.enhanced_bolts as u8 as f64;
        self.ordnance.input[f::ENHANCED_SNIPER] = self.enhanced_sniper as u8 as f64;
        self.ordnance.present();
        self.clock += dt;
        self.rail_touches.clear();
        for call in self.ordnance.calls().chunks_exact(call::STRIDE).take(self.ordnance.header[h::CALLS] as usize) {
            let slot = call[call::SOURCE] as usize;
            // Ordnance.drawBolt's `sniperTrace().touch(...)`: kept for the
            // trace's turn, which needs the frame's camera.
            if call[call::KIND] == ck::TOUCH as f64 {
                if let (Some(bolt), true) = (self.bolts.get(slot), self.rail_touches.len() < self.rail_touches.capacity()) {
                    let mut touch = [0.; 15];
                    touch[0] = bolt.key as f64;
                    touch[1] = bolt.style as f64;
                    touch[2..5].copy_from_slice(&call[call::A..call::A + 3]);
                    touch[5..8].copy_from_slice(&call[call::B..call::B + 3]);
                    touch[8..11].copy_from_slice(&call[call::C..call::C + 3]);
                    touch[11..14].copy_from_slice(&call[call::E..call::E + 3]);
                    touch[14] = call[call::S0];
                    self.rail_touches.push(touch);
                }
                continue;
            }
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
                } else if call[call::KIND] == ck::NET_TRAIL as f64 {
                    // drawProjectileView: `trail.move(drawnAt, quaternion, velocity)`.
                    if let Some(trail) = net.trail {
                        self.effects.sync(
                            trail,
                            std::array::from_fn(|i| call[call::A + i]),
                            Some(std::array::from_fn(|i| call[call::D + i])),
                            Some(std::array::from_fn(|i| call[call::E + i])),
                        );
                    }
                } else if call[call::KIND] == ck::NET_REMOVE as f64 {
                    // ProjectilePresentation.remove: `p.trail?.stop()`.
                    net.shown = false;
                    if let Some(trail) = net.trail.take() {
                        self.effects.stop(trail);
                    }
                }
            }
        }
        for (slot, net) in self.net.iter_mut().enumerate() {
            if net.active && tick > net.expiry + 24. {
                if let Some(trail) = net.trail.take() {
                    self.effects.stop(trail);
                }
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
        // BlasterBatch.load: the owner's capsules, plane after plane, then
        // the batch's value, which `project` fills once the lens is known.
        // Drawn after the particles, as the browser's mesh is (render
        // order 11).
        let count = (self.ordnance.header[h::BLASTERS] as usize).min(limits::BLASTERS);
        self.blaster_count = count;
        let mut draws = bridge.r.effects.input[bfe_render::effects::IN_DRAWS] as usize;
        if count > 0 && draws < bfe_render::effects::MAX_DRAWS {
            use bfe_render::effects as render;
            let mut at = self.blasters_at;
            for (plane, size) in self.ordnance.blaster_arrays().into_iter().zip([3, 3, 4, 4]) {
                bridge.r.effects.data[at..at + count * size].copy_from_slice(&plane[..count * size]);
                at += count * size;
            }
            bridge.r.effects.data[at] = 0.002;
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&[render::KIND_BATCH as f64, self.blasters as f64, count as f64, 0., self.blasters_at as f64, count as f64]);
            row[render::DRAW_KEY + 1] = 11.;
            row[render::DRAW_KEY + 5] = self.blasters as f64;
            draws += 1;
            bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
            self.total_blasters += count as u64;
        }
    }
    /// SniperTrace's frame, once the camera is posed: this frame's rounds
    /// and waiting impacts go to the trace as commands (touch, headshot,
    /// defer), it collects them and steps, its callbacks play the impacts
    /// whose tracers arrived, and its capsules and flares are drawn.
    /// `camera` is the camera's world matrix. Its lights are not drawn
    /// (there are none here), and in first person the bolt leaves the
    /// camera's side, not the first-person gun's muzzle.
    pub fn present_sniper(&mut self, camera: &Matrix4, projection: &[f64], height: f64, bridge: &mut Bridge) {
        use bfe_presentation::sniper_layout::{sniper_command as c, sniper_counts as counts, sniper_frame as sf, sniper_kind as k};
        let live = self.sniper.counts[counts::RECORDS] > 0;
        if self.rail_touches.is_empty() && self.rail_impacts.is_empty() && self.rail_heads.is_empty() && !live {
            // "Empty frames still advance native time in end": nothing to
            // draw or to wait for, but the trace's clock keeps up.
            self.sniper.input[sf::TIME] = self.clock;
            self.sniper.input[sf::COMMANDS] = 0.;
            self.sniper.input[sf::HAS_CAMERA] = 0.;
            let rng = &mut self.rail_rng;
            self.sniper.frame(&mut || rng.uniform());
            return;
        }
        let mut inverse = *camera;
        inverse.invert();
        let x = &mut self.sniper.input;
        x[sf::TIME] = self.clock;
        x[sf::HAS_CAMERA] = 1.;
        x[sf::HAS_MUZZLE] = 0.;
        x[sf::EYE..sf::EYE + 3].copy_from_slice(&camera.elements[12..15]);
        x[sf::CAMERA..sf::CAMERA + 16].copy_from_slice(&camera.elements);
        x[sf::INVERSE..sf::INVERSE + 16].copy_from_slice(&inverse.elements);
        x[sf::BEGIN] = 1.;
        let mut count = 0;
        // The trace takes new sources in rising order of their ids: the
        // bolts' own serial numbers, oldest first.
        self.rail_touches.sort_by(|a, b| a[0].total_cmp(&b[0]));
        for touch in &self.rail_touches {
            let Some((color, reach)) = self.styles.get(touch[1] as usize).and_then(|style| style.rail) else {
                continue;
            };
            let row = &mut x[sf::STRIDE + count * c::STRIDE..][..c::STRIDE];
            row.fill(0.);
            row[c::KIND] = k::TOUCH as f64;
            row[c::SOURCE] = touch[0];
            row[c::START..c::START + 3].copy_from_slice(&touch[2..5]);
            row[c::HEAD..c::HEAD + 3].copy_from_slice(&touch[5..8]);
            row[c::DIRECTION..c::DIRECTION + 3].copy_from_slice(&touch[8..11]);
            row[c::ORIGIN..c::ORIGIN + 3].copy_from_slice(&touch[11..14]);
            row[c::COLOR..c::COLOR + 3].copy_from_slice(&color);
            row[c::REACH] = reach;
            row[c::ENDED] = touch[14];
            count += 1;
        }
        for point in self.rail_heads.drain(..) {
            let row = &mut x[sf::STRIDE + count * c::STRIDE..][..c::STRIDE];
            row.fill(0.);
            row[c::KIND] = k::HEADSHOT as f64;
            row[c::POINT..c::POINT + 3].copy_from_slice(&point);
            count += 1;
        }
        // SniperTrace.defer: the impact's effect waits for the tracer whose
        // line it lies on. With no room to remember it, it plays at once.
        for at in 0..self.rail_impacts.len() {
            let (group, position, id) = self.rail_impacts[at];
            if self.rail_waiting.len() == self.rail_waiting.capacity() {
                self.present_effect(group, position, id);
                continue;
            }
            self.rail_token = if self.rail_token == u32::MAX { 1 } else { self.rail_token + 1 };
            self.rail_waiting.push((self.rail_token, group, position, id));
            let row = &mut self.sniper.input[sf::STRIDE + count * c::STRIDE..][..c::STRIDE];
            row.fill(0.);
            row[c::KIND] = k::DEFER as f64;
            row[c::TOKEN] = self.rail_token as f64;
            row[c::RECORD] = -1.;
            row[c::POINT..c::POINT + 3].copy_from_slice(&position);
            count += 1;
        }
        self.rail_impacts.clear();
        self.sniper.input[sf::COMMANDS] = count as f64;
        {
            let rng = &mut self.rail_rng;
            self.sniper.collect(&mut || rng.uniform());
        }
        self.rail_events();
        self.sniper.input[sf::COMMANDS] = 0.;
        self.sniper.input[sf::BEGIN] = 0.;
        {
            let rng = &mut self.rail_rng;
            self.sniper.frame(&mut || rng.uniform());
        }
        self.rail_events();
        // SniperTrace.end: the two publications are prefixes of the trace's
        // retained rows. Capsules draw with the bolts (render order 11),
        // flares over them (12).
        use bfe_render::effects as render;
        let (streaks, flares) = (
            (self.sniper.counts[counts::STREAKS] as usize).min(sn::STREAKS),
            (self.sniper.counts[counts::FLARES] as usize).min(sn::FLARES),
        );
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        if streaks > 0 && draws < render::MAX_DRAWS {
            let at = self.streaks_at;
            let floats = streaks * streak::STRIDE;
            bridge.r.effects.data[at..at + floats].copy_from_slice(&self.sniper.streaks[..floats]);
            let fov = 2. * (1. / projection[5]).atan().to_degrees();
            bridge.r.effects.data[at + floats] = style::bolt_view(fov, 1., height) as f32;
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&[render::KIND_BATCH as f64, self.streaks as f64, streaks as f64, 0., at as f64, streaks as f64]);
            row[render::DRAW_KEY + 1] = 11.;
            row[render::DRAW_KEY + 5] = self.streaks as f64;
            draws += 1;
        }
        if flares > 0 && draws < render::MAX_DRAWS {
            let at = self.streaks_at + STREAK_FLOATS;
            let floats = flares * flare::STRIDE;
            bridge.r.effects.data[at..at + floats].copy_from_slice(&self.sniper.flares[..floats]);
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&[render::KIND_BATCH as f64, self.flares as f64, flares as f64, 0., at as f64, flares as f64]);
            row[render::DRAW_KEY + 1] = 12.;
            row[render::DRAW_KEY + 5] = self.flares as f64;
            draws += 1;
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
        self.total_streaks += (streaks + flares) as u64;
    }
    // SniperTrace.apply: a callback plays the impact it was given, a
    // release forgets one whose tracer is gone. (Its LIGHT and FLASH events
    // ask for lights, which are not drawn here.)
    fn rail_events(&mut self) {
        use bfe_presentation::sniper_layout::{sniper_counts as counts, sniper_event as e, sniper_event_kind as ek};
        for at in 0..self.sniper.counts[counts::EVENTS] as usize {
            let event = &self.sniper.events[at * e::STRIDE..][..e::STRIDE];
            let kind = event[e::KIND] as u32;
            if kind != ek::CALLBACK && kind != ek::RELEASE {
                continue;
            }
            let token = event[e::TOKEN] as u32;
            let Some(held) = self.rail_waiting.iter().position(|waiting| waiting.0 == token) else {
                continue;
            };
            let (_, group, position, id) = self.rail_waiting.remove(held);
            if kind == ek::CALLBACK {
                self.present_effect(group, position, id);
            }
        }
    }
    /// BlasterBatch's onBeforeRender: a thin bolt is never drawn under a
    /// couple of pixels wide, which needs the lens (three's fov from the
    /// projection) and the frame's height.
    pub fn project(&self, bridge: &mut Bridge, projection: &[f64], height: f64) {
        if self.blaster_count > 0 {
            let fov = 2. * (1. / projection[5]).atan().to_degrees();
            bridge.r.effects.data[self.blasters_at + self.blaster_count * 14] = style::bolt_view(fov, 1., height) as f32;
        }
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
