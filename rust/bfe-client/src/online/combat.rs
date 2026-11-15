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
// Replicated bolt slots: with every predicted bolt and the closing NET record
// they fill the ordnance input exactly.
const BOLTS: usize = limits::RECORDS - COSMETICS - 1;
// m over which a bolt drawn from the muzzle joins its path (ordnance.js).
const MERGE: f64 = 8.;
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
    // Who fired it and with which class of round (the wire's ids), for the
    // gun it is drawn from; `aimed` once that gun has been looked up.
    owner: (u32, u32),
    definition: usize,
    aimed: bool,
    // The player's own round already drawn from his trigger press: its
    // cosmetic's key. The world's copy is never drawn a second time.
    predicted: Option<u32>,
}
// PredictedShots: the player's own bolt, drawn from the tick his weapon
// fires in the client's replay until the world's copy would have ended. It
// flies the world's path (same origin, aim, spread and first surface) on the
// frame clock; the world's round carries the hit and its impact.
#[derive(Clone, Copy)]
struct Cosmetic {
    key: u32,
    style: usize,
    definition: usize,
    tick: f64,
    channel: u32,
    origin: [f64; 3],
    direction: [f64; 3],
    offset: [f64; 3],
    speed: f64,
    life: f64,
    born: f64,
    until: f64,
    aimed: bool,
    claimed: bool,
    // How far it had flown when last drawn: the next frame sweeps on from
    // there for a soldier in its way.
    flown: f64,
    fade: f64,
}
// What a weapon's bolt needs to be drawn ahead of the world: its round's
// wire class and style, speed, life and FadeOutTime, and its ShotElevate.
#[derive(Clone, Copy)]
struct Barrel {
    definition: usize,
    style: usize,
    speed: f64,
    span: f64,
    fade: f64,
    elevate: f64,
}
impl Cosmetic {
    // Its flight is over: nothing more to sweep.
    fn claimed_end(&self) -> bool {
        self.flown >= self.speed * self.life
    }
}
const COSMETICS: usize = 32;
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
    // Network commands written for the owner since its last presentation.
    net_count: usize,
    // End records whose bolt this client has not been sent yet (id,
    // generation, the point, the tick), newest last: a launch that arrives
    // after its end still stops there.
    orphans: Vec<(u32, u32, [f64; 3], f64)>,
    cosmetics: Vec<Cosmetic>,
    // By weapon (its place in the battle's weapon list), then by charge
    // level: a charge can fire another class of round (the bowcaster's).
    barrels: Vec<Vec<Option<Barrel>>>,
    // BFE_HUD_TRACE: every round's records and the player's own bolts as
    // drawn, on stderr.
    trace: bool,
    names: Vec<String>,
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
    /// The droids' Enhanced deaths (destruction.js).
    pub destruction: super::destruction::Destructions,
    droid_explosions: Vec<usize>,
    /// The map's placed effects (torches, smoke, steam).
    pub map_effects: super::map_effects::MapEffects,
    /// The hulls' thrust, damage and death effects.
    pub hull_fx: super::hull_effects::HullEffects,
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
// Unix milliseconds, for the trace: when a record came in, when a bolt was drawn.
fn wall() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0., |t| t.as_secs_f64() * 1000.)
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
    pub async fn new(
        content: &BattleContent,
        mut prepared: particles::Prepared,
        bridge: &mut Bridge,
        textures: &[u32],
        resources: &mut Resources,
        collision: super::replay::Collision,
        world: u32,
    ) -> Result<Self, String> {
        let shadow_cascades = bridge.shadow_cascades();
        let defs = content.defs()?;
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
        let mut budget = crate::loading_work::Budget::new();
        for (id, (_, def)) in defs.registry.combat.iter().enumerate() {
            if budget.tick().await {
                bridge.r.core.state.reset();
            }
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
            let hue=style::resources(&authored,0,true)["hue"].as_str().and_then(|name|prepared.textures.get(name))
                .and_then(|image|content.textures.cpu_pixels(*image).ok()).map(|pixels|style::texture_hue(&pixels));
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
                // Surface 5: the round ran out in the air and struck nothing
                // (CombatPresentation.receive shows no impact for it).
                None,
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
            styles.push(Style { model: Model::new(content, bridge, textures, name, shadow_cascades)?, impact, explosion, trail, rail });
        }
        // PredictedShots.fire: only a bolt flies straight enough to draw
        // ahead of the world.
        let barrels = defs
            .weapons
            .iter()
            .map(|weapon| {
                // weapons.js levels: the class itself, then each of its
                // charges laid over it.
                let charges = weapon.record.get("charges").and_then(bfe_content::Value::as_array).map_or(&[][..], |list| &list[..]);
                std::iter::once(None)
                    .chain(charges.iter().map(Some))
                    .map(|charge| {
                        let field = |key: &str| charge.and_then(|charge| charge.get(key)).or_else(|| weapon.record.get(key));
                        let round = field("ordnance")?;
                        if round.get("base").and_then(bfe_content::Value::as_str) != Some("bolt") {
                            return None;
                        }
                        let definition = defs.combat_id(round)? as usize;
                        let or = |n: f64, default: f64| if n != 0. && !n.is_nan() { n } else { default };
                        Some(Barrel {
                            definition,
                            style: definitions.get(definition).copied().flatten()?,
                            speed: or(number(round, "Velocity"), 100.).clamp(1., 2000.),
                            span: or(number(round, "LifeSpan"), 1.).min(1.),
                            fade: number(round, "FadeOutTime"),
                            elevate: field("ShotElevate").and_then(bfe_content::Value::as_f64).unwrap_or(0.),
                        })
                    })
                    .collect()
            })
            .collect();
        let effects = bfe_presentation::Presentation::new(
            serde_json::from_value(library.descriptor.clone()).map_err(|e| e.to_string())?,
            bfe_presentation::Limits { effects: 1024, emitters: 4096, particles: 16384, commands: 2048, events: 32768 },
        );
        let mut particles = prepared.admit(content, bridge, textures, resources)?;
        // BlasterBatch: one instanced draw of every Enhanced bolt, after the
        // particles' own data.
        let blasters = bridge.effect_batch(bfe_render::effect_layout::BLASTER as u32, 0, shadow_cascades > 0, -1, limits::BLASTERS as u32, -1, &[])?;
        let blasters_at = particles.words;
        particles.words += BLASTER_FLOATS;
        // SniperTrace's two instanced draws: capsules (bolts, tracer lines,
        // vapour, sparks), then flares.
        let streaks = bridge.effect_batch(bfe_render::effect_layout::SNIPER_STREAKS as u32, 0, shadow_cascades > 0, -1, sn::STREAKS as u32, -1, &[])?;
        let flares = bridge.effect_batch(bfe_render::effect_layout::SNIPER_FLARES as u32, 0, shadow_cascades > 0, -1, sn::FLARES as u32, -1, &[])?;
        let streaks_at = particles.words;
        particles.words += STREAK_FLOATS + FLARE_FLOATS;
        bridge.fx_reserve(particles.words as u32);
        let destruction = super::destruction::Destructions::new(content, bridge, textures, collision.clone(), world, particles.words)?;
        particles.words = destruction.end();
        let debris = super::debris::Debris::new(content, bridge, textures, &effect_names, collision, world).await?;
        // The explosions droids that come apart die in: Enhanced draws its own
        // blast for them and leaves the stock effect out (Destruction.explosion).
        let droid_explosions: Vec<usize> = defs
            .classes
            .iter()
            .filter(|c| c.record.get("chunks").and_then(bfe_content::Value::as_array).is_some_and(|l| !l.is_empty()))
            .filter_map(|c| super::destruction::explosion_of(&c.record))
            .flat_map(|name| super::destruction::explosion_ids(&defs, name).collect::<Vec<_>>())
            .collect();
        // environment-effects.js warmup: each group's longest particle life,
        // at most 4 s.
        let lifetime = |d: &serde_json::Value| d["transformer"]["lifetime"].as_f64().unwrap_or(0.);
        let definitions_json = library.descriptor["definitions"].as_array().map_or(&[][..], |v| v.as_slice());
        let warmups: Vec<f64> = library.descriptor["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|group| {
                let longest = group.as_array().into_iter().flatten().filter_map(|d| d.as_u64()).filter_map(|d| definitions_json.get(d as usize)).map(lifetime).fold(0., f64::max);
                longest.min(4.)
            })
            .collect();
        let map_effects = super::map_effects::MapEffects::new(content, &effect_names, &warmups);
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
            net_count: 0,
            orphans: Vec::with_capacity(64),
            cosmetics: Vec::with_capacity(COSMETICS),
            barrels,
            trace: std::env::var_os("BFE_HUD_TRACE").is_some(),
            names: std::iter::once(String::new()).chain(defs.registry.combat.iter().map(|(name, _)| name.clone())).collect(),
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
            map_effects,
            destruction,
            droid_explosions,
            hull_fx: super::hull_effects::HullEffects::new(),
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
    /// PredictedShots.fire for a round the client's replay just fired:
    /// `weapon` (its place in the battle's weapon list) at charge `level` on trigger `channel`
    /// at `tick`, from the gun origin along the aim, turned by the press's
    /// spread. `cast` gives the distance to the first surface on a ray. A
    /// weapon that fires no bolt draws nothing ahead of the world.
    pub fn predict(
        &mut self,
        weapon: usize,
        level: usize,
        tick: f64,
        channel: u32,
        origin: [f64; 3],
        aim: [f64; 3],
        spread: [f64; 2],
        cast: &mut dyn FnMut([f64; 3], [f64; 3], f64) -> Option<f64>,
    ) {
        let Some(barrel) = self.barrels.get(weapon).and_then(|levels| levels.get(level)).copied().flatten() else {
            return;
        };
        if self.cosmetics.len() == COSMETICS {
            return;
        }
        // The spread: a turn about the vertical, then a tip about the
        // horizontal; then the weapon's ShotElevate.
        let mut direction = Vector3::new(aim[0], aim[1], aim[2]);
        let mut turn = Quaternion::default();
        turn.set_from_axis_angle(&Vector3::new(0., 1., 0.), spread[0]);
        direction.apply_quaternion(&turn);
        let mut side = Vector3::new(direction.z, 0., -direction.x);
        side.normalize();
        turn.set_from_axis_angle(&side, -spread[1]);
        direction.apply_quaternion(&turn);
        direction.y += barrel.elevate;
        direction.normalize();
        let direction = [direction.x, direction.y, direction.z];
        if direction.iter().any(|n| !n.is_finite()) {
            return;
        }
        // As far as the world's round would fly: to the first surface, or
        // its life (at most a second: the world answers well within it).
        let reach = cast(origin, direction, barrel.speed * barrel.span).unwrap_or(f64::INFINITY);
        let life = barrel.span.min(reach / barrel.speed);
        self.serial = self.serial.wrapping_add(1).max(1);
        // Drawn first later in this frame, about half a tick after it fired.
        let born = self.clock - 1. / 120.;
        self.cosmetics.push(Cosmetic {
            key: self.serial,
            style: barrel.style,
            definition: barrel.definition,
            tick,
            channel,
            origin,
            direction,
            offset: [0.; 3],
            speed: barrel.speed,
            life,
            born,
            until: born + (life + barrel.fade).max(1.),
            aimed: false,
            claimed: false,
            flown: 0.,
            fade: barrel.fade,
        });
        if self.trace {
            eprintln!(
                "ord: predict key={} def={} tick={tick} wall={:.1} speed={} life={life:.4} at={origin:.3?} dir={direction:.4?}",
                self.serial,
                self.names[barrel.definition],
                wall(),
                barrel.speed
            );
        }
    }
    /// PlayerCombat.presentCharge's glow: a weapon's ChargeUpEffect at its
    /// fire point, turned as the gun is, its colour and alpha scaled by the
    /// charge. `handle` is the one this gave last frame for the same gun
    /// (None starts it); call before `draw`.
    pub fn glow(&mut self, handle: Option<u32>, group: usize, position: [f64; 3], turn: [f64; 4], tint: f64) -> Option<u32> {
        let handle = match handle {
            Some(handle) => {
                self.effects.sync(handle, position, Some(turn), None);
                Some(handle)
            }
            None => self.effects.start_turned(group, position, [0.; 3], turn, 0),
        };
        if let Some(handle) = handle {
            self.effects.tint(handle, tint);
            // The tint rides on the effect's next command.
            self.effects.sync(handle, position, Some(turn), None);
        }
        handle
    }
    /// `glow.kill()`: the charge is over, or the gun is no longer drawn.
    pub fn end_glow(&mut self, handle: u32) {
        self.effects.kill(handle);
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
    /// The frame's records: rounds, impacts and explosions are taken in and
    /// the effects that fall due are started. Nothing is drawn here: `draw`
    /// follows once the frame's bodies are posed.
    pub fn frame(&mut self, receiver: &Receiver, tick: f64, dt: f64) {
        use bfe_presentation::layout::frame as ef;
        let own = (receiver.own.id as u32, receiver.own.generation as u32);
        // How far behind the newest tick this frame is drawn, once a second.
        if self.trace && self.frame % 60 == 0 {
            eprintln!("ord: clock behind={:.2} latest={} shown={tick:.2} dt={dt:.4}", receiver.latest - tick, receiver.latest);
        }
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
            self.orphans.clear();
            self.cosmetics.clear();
            self.flames.clear();
            self.map_effects.reset(&mut self.effects);
            self.hull_fx.clear(&mut self.effects);
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
                        // ProjectilePresentation.end: a rocket, shell or
                        // grenade that struck something is gone when the
                        // clock reaches this record's tick, its last stretch
                        // flown into the point it names. Without this it
                        // flew on past what it hit for the life it had left.
                        let flying = self.net_bound.get(&key).copied();
                        if let Some(net) = flying {
                            if net_count == limits::NET_COMMANDS {
                                self.ordnance.net_flush(net_count);
                                net_count = 0;
                            }
                            let x = &mut self.ordnance.net_input[net_count * n::STRIDE..][..n::STRIDE];
                            x.fill(0.);
                            x[n::KIND] = nk::END as f64;
                            x[n::SLOT] = net as f64;
                            x[n::GENERATION] = generation as f64;
                            x[n::SCHEMA] = 2.;
                            x[n::TICK] = stamp;
                            x[n::POSITION..n::POSITION + 3].copy_from_slice(&position);
                            net_count += 1;
                        }
                        if flying.is_none() {
                            if self.orphans.len() == self.orphans.capacity() {
                                self.orphans.remove(0);
                            }
                            self.orphans.push((id, generation, position, stamp));
                        }
                        if self.trace {
                            eprintln!(
                                "ord: end id={id} tick={stamp} shown={tick:.3} surface={} at={position:.3?} {}",
                                source.surface,
                                if flying.is_some() { "projectile" } else { "unbound" }
                            );
                        }
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
                    let owner = (u32::from_le_bytes(bytes[8..12].try_into().unwrap()), source.owner_generation as u32);
                    // The player's own round, predicted from the trigger
                    // press, flies as its cosmetic drew it: the one of this
                    // trigger and class of round fired nearest its tick.
                    let fired = source.execution_tick;
                    let predicted = (owner == own)
                        .then(|| {
                            self.cosmetics
                                .iter_mut()
                                .filter(|c| !c.claimed && c.channel == source.channel as u32 && c.definition == source.definition as usize)
                                .filter(|c| (c.tick - fired).abs() <= 30.)
                                .min_by(|a, b| (a.tick - fired).abs().total_cmp(&(b.tick - fired).abs()))
                        })
                        .flatten()
                        .map(|cosmetic| {
                            cosmetic.claimed = true;
                            cosmetic.key
                        });
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
                        owner,
                        definition: source.definition as usize,
                        predicted,
                        ..Default::default()
                    };
                    // A launch that comes after its own end record (its first
                    // copy lost, or sent late) still flies only to that point.
                    if let Some(at) = self.orphans.iter().position(|orphan| orphan.0 == id && orphan.1 == generation) {
                        let (_, _, point, _) = self.orphans.remove(at);
                        let bolt = &mut self.bolts[slot];
                        if bolt.speed > 0. {
                            let distance = (0..3).map(|i| (point[i] - bolt.position[i]) * bolt.direction[i]).sum::<f64>().max(0.);
                            bolt.life = bolt.life.min(distance / bolt.speed);
                        }
                        bolt.terminal = true;
                    }
                    if self.trace {
                        let b = &self.bolts[slot];
                        eprintln!(
                            "ord: launch id={id} def={} owner={} tick={stamp} flight={:.3} shown={tick:.3} wall={:.1} speed={} life={:.4} at={:.3?} dir={:.4?}{}",
                            self.names[b.definition],
                            b.owner.0,
                            b.flight,
                            wall(),
                            b.speed,
                            b.life,
                            b.position,
                            b.direction,
                            b.predicted.map_or(String::new(), |key| format!(" predicted={key}"))
                        );
                    }
                } else {
                    let point = source.view.position.unwrap_or_default();
                    if self.trace {
                        eprintln!("ord: end id={id} tick={stamp} shown={tick:.3} surface={} at={point:.3?}", source.surface);
                    }
                    let bolt = &mut self.bolts[slot];
                    if bolt.speed > 0. {
                        let distance = (0..3).map(|i| (point[i] - bolt.position[i]) * bolt.direction[i]).sum::<f64>().max(0.);
                        bolt.life = bolt.life.min(distance / bolt.speed);
                    }
                    bolt.terminal = true;
                    bolt.tick = stamp;
                    bolt.expiry = source.expiry_tick;
                    let group = self.styles[style].impact.get(source.surface as usize).copied().flatten();
                    if let Some(key) = bolt.predicted {
                        // Its cosmetic has flown already: the impact shows
                        // now. A class drawn on where it stops (the sniper
                        // rifles) has its streak cut there (PredictedShots.stop).
                        let rail = self.styles[style].rail.is_some();
                        let now = self.clock;
                        if let Some(cosmetic) = self.cosmetics.iter_mut().find(|c| c.key == key) {
                            // The world's answer: where its round stopped.
                            // A cosmetic still flying takes that as its own
                            // end, nearer or further than the client's own
                            // ray found (a round that struck nothing keeps
                            // the life it was launched with). One whose
                            // flight is over is only ever cut (the sniper
                            // rifles' streak, PredictedShots.stop).
                            let along = (0..3).map(|i| (point[i] - cosmetic.origin[i]) * cosmetic.direction[i]).sum::<f64>().max(0.);
                            let answer = along / cosmetic.speed;
                            if now - cosmetic.born < cosmetic.life {
                                cosmetic.life = answer.max(now - cosmetic.born);
                                cosmetic.until = cosmetic.born + (cosmetic.life + cosmetic.fade).max(1.);
                            } else {
                                cosmetic.life = cosmetic.life.min(answer);
                            }
                        }
                        self.defer(group, point, id, stamp.min(tick), rail);
                    } else {
                        self.bolts[slot].impact = Some((group, point));
                    }
                }
            } else if schema == 5 && bytes.len() == 33 {
                let definition = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
                let group = self
                    .definitions
                    .get(definition)
                    .copied()
                    .flatten()
                    .and_then(|style| self.styles[style].explosion.get(bytes[32] as usize).copied().flatten());
                let position: [f64; 3] = std::array::from_fn(|i| f64::from_le_bytes(bytes[8 + i * 8..16 + i * 8].try_into().unwrap()));
                if self.trace {
                    eprintln!(
                        "ord: blast id={id} def={} tick={stamp} shown={tick:.3} wet={} at={position:.3?}",
                        self.names.get(definition).map_or("", String::as_str),
                        bytes[32]
                    );
                }
                let quiet = self.destruction.enhanced && self.droid_explosions.contains(&definition);
                self.defer(if quiet { None } else { group }, position, id, stamp, false);
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
                if self.trace && schema == 7 {
                    eprintln!("ord: gone id={id} tick={stamp} shown={tick:.3}");
                }
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
                    if self.trace {
                        let definition = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
                        eprintln!(
                            "ord: sample id={id} def={} type={} owner={} tick={stamp} shown={tick:.3} life={:.3} at={:.3?} vel={:.3?}{}",
                            self.names.get(definition).map_or("", String::as_str),
                            bytes[8],
                            u32::from_le_bytes(bytes[9..13].try_into().unwrap()),
                            x[n::LIFE],
                            &x[n::POSITION..n::POSITION + 3],
                            &x[n::VELOCITY..n::VELOCITY + 3],
                            if fresh { " new" } else { "" }
                        );
                    }
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
        // NetworkCombatViews.present resolves a late launch at its first draw,
        // bounded by the launch presentation interval. Thereafter the bolt
        // flies on the displayed clock from that tick: frame frequency
        // cannot advance a second projectile flight clock.
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
                    if self.trace && group.is_some() {
                        eprintln!("ord: effect id={id} due={due:.3} shown={tick:.3} wall={:.1} at={position:.3?}", wall());
                    }
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
        self.net_count = net_count;
    }
    /// The frame's rounds as drawn, once its bodies are posed. `muzzle`
    /// says where the gun of a unit (id, generation) that fires a class of
    /// round (the wire's id) is drawn now, if it holds one: a bolt leaves
    /// that gun and joins its true path over MERGE metres
    /// (CombatPresentation.present, `bolt.visualMuzzle`). `own` is the
    /// player's unit, whose bolts the trace follows. `strike` gives the
    /// distance to the first drawn body other than the player's on a
    /// segment (from, direction, length): the player's own bolt, drawn
    /// ahead of the world, stops at the first soldier it crosses as this
    /// client shows him (PredictedShots.update), until the world's end
    /// record says where its round really stopped.
    pub fn draw(
        &mut self,
        dt: f64,
        own: (u32, u32),
        muzzle: &mut dyn FnMut((u32, u32), usize) -> Option<[f64; 3]>,
        strike: &dyn Fn([f64; 3], [f64; 3], f64) -> Option<f64>,
        drop_shadows: (&[[f64; 3]], [f64; 3]),
        lights: &mut super::lights::SceneLights,
        bridge: &mut Bridge,
    ) {
        let tick = self.tick;
        let net_count = std::mem::take(&mut self.net_count);
        let mut count = 0;
        for (slot, bolt) in self.bolts.iter_mut().enumerate() {
            if !bolt.active || bolt.predicted.is_some() {
                continue;
            }
            if !bolt.aimed {
                // Resolved at its first draw, against the shooter's body as
                // drawn now; a muzzle far from the launch (a respawn, a
                // seat) is ignored.
                bolt.aimed = true;
                if let Some(muzzle) = muzzle(bolt.owner, bolt.definition) {
                    let offset: [f64; 3] = std::array::from_fn(|i| muzzle[i] - bolt.position[i]);
                    bolt.offset = if offset.iter().map(|o| o * o).sum::<f64>() > 400. { [0.; 3] } else { offset };
                }
            }
            let x = &mut self.ordnance.input[f::STRIDE + count * r::STRIDE..][..r::STRIDE];
            x.fill(0.);
            x[r::KIND] = kind::BOLT as f64;
            x[r::KEY] = bolt.key as f64;
            x[r::STYLE] = bolt.style as f64;
            x[r::SOURCE] = slot as f64;
            // linearBoltFrame: a replicated bolt flies from its flight tick
            // at its speed for its life, on the displayed clock. Without
            // LINEAR the owner reads it as a stepped bolt that has flown
            // nothing, and draws it at the muzzle until its record expires.
            x[r::FLAGS] = ((1 << flag::LINEAR) | (1 << flag::HAS_CLOCK) | (1 << flag::HAS_OFFSET)) as f64;
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
            if self.trace && bolt.owner == own {
                let elapsed = (tick - bolt.flight) / 60.;
                if elapsed >= 0. && elapsed < bolt.life {
                    let flown = bolt.speed * elapsed;
                    let left = (1. - flown / bolt.merge).max(0.);
                    let head: [f64; 3] = std::array::from_fn(|i| bolt.position[i] + bolt.direction[i] * flown + bolt.offset[i] * left);
                    eprintln!("ord: draw id={} shown={tick:.3} wall={:.1} flown={flown:.3} head={head:.3?} offset={:.3?}", bolt.id, wall(), bolt.offset);
                }
            }
        }
        // PredictedShots.update and draw: the player's own bolts on the
        // frame clock, each until its flight (and fade) is over.
        let now = self.clock;
        self.cosmetics.retain(|cosmetic| now < cosmetic.until);
        for (at, cosmetic) in self.cosmetics.iter_mut().enumerate() {
            let reach = cosmetic.speed * (now - cosmetic.born).min(cosmetic.life);
            if reach > cosmetic.flown && !cosmetic.claimed_end() {
                let from: [f64; 3] = std::array::from_fn(|i| cosmetic.origin[i] + cosmetic.direction[i] * cosmetic.flown);
                if let Some(distance) = strike(from, cosmetic.direction, reach - cosmetic.flown) {
                    cosmetic.life = cosmetic.life.min((cosmetic.flown + distance) / cosmetic.speed);
                    if self.trace {
                        eprintln!("ord: stop key={} flown={:.3} wall={:.1}", cosmetic.key, cosmetic.flown + distance, wall());
                    }
                }
            }
            cosmetic.flown = cosmetic.speed * (now - cosmetic.born).min(cosmetic.life);
            if !cosmetic.aimed {
                cosmetic.aimed = true;
                if let Some(muzzle) = muzzle(own, cosmetic.definition) {
                    let offset: [f64; 3] = std::array::from_fn(|i| muzzle[i] - cosmetic.origin[i]);
                    cosmetic.offset = if offset.iter().map(|o| o * o).sum::<f64>() > 400. { [0.; 3] } else { offset };
                }
            }
            let x = &mut self.ordnance.input[f::STRIDE + count * r::STRIDE..][..r::STRIDE];
            x.fill(0.);
            x[r::KIND] = kind::BOLT as f64;
            x[r::KEY] = cosmetic.key as f64;
            x[r::STYLE] = cosmetic.style as f64;
            x[r::SOURCE] = (BOLTS + at) as f64;
            x[r::FLAGS] = ((1 << flag::LINEAR) | (1 << flag::HAS_CLOCK) | (1 << flag::HAS_OFFSET)) as f64;
            x[r::ALPHA] = 1.;
            x[r::CLOCK_TICK] = now * 60.;
            x[r::FADE] = 1.;
            x[r::SPEED] = cosmetic.speed;
            x[r::LIFE] = cosmetic.life;
            x[r::MERGE] = MERGE.min(cosmetic.speed * cosmetic.life).max(1e-3);
            x[r::FLIGHT_TICK] = cosmetic.born * 60.;
            x[r::POSITION..r::POSITION + 3].copy_from_slice(&cosmetic.origin);
            x[r::DIRECTION..r::DIRECTION + 3].copy_from_slice(&cosmetic.direction);
            x[r::OFFSET..r::OFFSET + 3].copy_from_slice(&cosmetic.offset);
            count += 1;
            if self.trace && now - cosmetic.born < cosmetic.life {
                let flown = cosmetic.speed * (now - cosmetic.born);
                let left = (1. - flown / x[r::MERGE]).max(0.);
                let head: [f64; 3] = std::array::from_fn(|i| cosmetic.origin[i] + cosmetic.direction[i] * flown + cosmetic.offset[i] * left);
                eprintln!("ord: draw key={} wall={:.1} flown={flown:.3} head={head:.3?} offset={:.3?}", cosmetic.key, wall(), cosmetic.offset);
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
            // ordnance.js lights.add: a bolt, round or beam asks for one of
            // the pool's lights where it is drawn this frame.
            if call[call::KIND] == ck::LIGHT as f64 {
                lights.add(
                    [call[call::A], call[call::A + 1], call[call::A + 2]],
                    [call[call::B], call[call::B + 1], call[call::B + 2]],
                    call[call::S0],
                    call[call::S1],
                );
                continue;
            }
            let slot = call[call::SOURCE] as usize;
            // Ordnance.drawBolt's `sniperTrace().touch(...)`: kept for the
            // trace's turn, which needs the frame's camera.
            if call[call::KIND] == ck::TOUCH as f64 {
                // The round it is of: a replicated bolt's slot, or past them a
                // cosmetic's place.
                let round = match slot.checked_sub(BOLTS) {
                    Some(at) => self.cosmetics.get(at).map(|cosmetic| (cosmetic.key, cosmetic.style)),
                    None => self.bolts.get(slot).map(|bolt| (bolt.key, bolt.style)),
                };
                if let (Some((key, style)), true) = (round, self.rail_touches.len() < self.rail_touches.capacity()) {
                    let mut touch = [0.; 15];
                    touch[0] = key as f64;
                    touch[1] = style as f64;
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
                    if self.trace {
                        eprintln!("ord: show id={} shown={tick:.3} wall={:.1} at={:.3?}", net.id, wall(), &call[call::A..call::A + 3]);
                    }
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
        self.map_effects.update(&mut self.effects, dt);
        for at in std::mem::take(&mut self.map_effects.blasts) {
            self.destruction.blast(at);
        }
        self.hull_fx.apply(&mut self.effects);
        // A wrecked hull's chunks, each from its node (else the root).
        for wreck in std::mem::take(&mut self.hull_fx.wrecks) {
            if self.trace {
                eprintln!("hull-wreck: {}", serde_json::json!({
                    "class": wreck.class, "origin": wreck.origin,
                    "body": wreck.matrix("bone_root").elements,
                    "carrier": wreck.matrix("").elements,
                }));
            }
            let starts: Vec<three::Matrix4> = self
                .debris
                .parts(wreck.class)
                .iter()
                .map(|part| wreck.matrix(&part.node))
                .collect();
            for (part, matrix) in starts.into_iter().enumerate() {
                self.debris.blow_apart_hull(wreck.class, part, matrix, wreck.origin, wreck.velocity);
            }
        }
        if let Some(clock) = self.map_effects.report.as_mut() {
            *clock += dt;
            if *clock >= 10. {
                *clock = 0.;
                let particles = self.effects.owner.header[bfe_presentation::layout::header::PARTICLES];
                let (seen, lowest) = (self.hull_fx.seen, self.hull_fx.lowest);
                let live = self.hull_fx.live_count();
                eprintln!(
                    "fx-count map={} in-view={} hull={} stomps={} hull-frames={seen} lowest={lowest:.2} live={live} particles={particles} effects={}",
                    self.map_effects.started, self.map_effects.visible, self.hull_fx.started, self.hull_fx.stomps, self.total_effects
                );
                let (pieces, hot, smoke, arcs) = self.destruction.counts();
                eprintln!("fx-destruction enhanced={} deaths={} pieces={pieces} hot={hot} smoke={smoke} arcs={arcs} peak-heat={:.2}", self.destruction.enhanced, self.destruction.deaths, std::mem::take(&mut self.destruction.peak_heat));
            }
        }
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
        self.particles.drop_shadows(bridge, drop_shadows.0, drop_shadows.1);
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
            + self.destruction.record_budget()
    }
    pub fn append(&self, bridge: &mut Bridge, mut records: usize, mut bones: usize) -> (usize, usize) {
        for net in &self.net {
            if net.active && net.shown {
                if let Some(model) = self.styles.get(net.style).and_then(|s| s.model.as_ref()) {
                    (records, bones) = model.append(bridge, &net.matrix, records, bones);
                }
            }
        }
        let (records, bones) = self.debris.append(bridge, records, bones);
        self.destruction.append(bridge, records, bones)
    }
}

#[cfg(test)]
mod record_tests {
    // Combat::draw writes every replicated bolt, every predicted one and the
    // closing NET record into the ordnance input in one frame.
    #[test]
    fn every_bolt_and_the_net_record_fit_the_ordnance_input() {
        assert!(super::BOLTS + super::COSMETICS + 1 <= super::limits::RECORDS);
    }
}
