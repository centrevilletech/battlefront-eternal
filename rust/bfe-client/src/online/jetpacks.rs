//! JetpackEffects, JetpackVolume and JetpackWake (src/jetpack-effects.js,
//! jetpack-volume.js, jetpack-wake.js) for the soldiers drawn here, over the
//! shared Jetpack owner: the attached flame at each nozzle, bent by how the
//! pack moves, and the combustion wisps, smoke and embers it leaves behind.
//! The jetpackEffects setting picks this Enhanced look or the class's stock
//! JetExhaust effect (game.js updateJetFlame), which the combat owner plays.
//! The nozzles sit where the class's effect authors them on the ribcage,
//! not on the pack model's own outlets (JetpackNozzles), and the exhaust's
//! light is not drawn.
use crate::render_bridge::Bridge;
use bfe_content_native::{loader::authored_json, BattleContent};
use bfe_presentation::{
    unit_effects::jetpack::{Definition, Jetpack},
    visual_effects_layout::{jet_frame as j, jet_nozzle as n, visual_limits as lim, volume_frame as v, volume_nozzle as z, wake_frame as w, wake_particle as p},
};
use bfe_render::effects as render;

/// Soldiers jetting, or still trailing smoke, at once.
const PACKS: usize = 8;
// JetpackWake's pools: where each starts among the owner's particles, how
// many it holds, its drag and its render order (smoke under the particles).
const POOLS: [(usize, usize, f32, f64); 3] =
    [(0, lim::WAKE_COMBUSTION, 7., 11.), (lim::WAKE_COMBUSTION, lim::WAKE_SMOKE, 3., 8.), (lim::WAKE_COMBUSTION + lim::WAKE_SMOKE, lim::WAKE_EMBERS, 1.8, 11.)];
// One pack's floats: the flame's nozzles and three values, then each pool's
// four planes and three values.
const FLAME: usize = lim::NOZZLES * 11 + 3;
const PACK: usize = FLAME + (lim::WAKE_COMBUSTION + lim::WAKE_SMOKE + lim::WAKE_EMBERS) * 14 + 9;
pub const FLOATS: usize = PACKS * PACK;

// What a class's JetEffect gives: its emitters for the Enhanced nozzles, as
// authored, and its effect group for the Classic exhaust.
struct Class {
    definitions: String,
    classic: Option<usize>,
}
struct Pack {
    owner: Jetpack,
    unit: Option<(u32, u32)>,
    class: usize,
    seen: bool,
}
pub struct Jetpacks {
    pub enhanced: bool,
    classes: Vec<Option<Class>>,
    packs: Vec<Pack>,
    flame: u32,
    wakes: [u32; 3],
    data: usize,
    // `Math.random()` for the wake's births; it decides nothing shared.
    rng: jsrt::rng::ClassicRandom,
    /// Flame and wake draws handed to the renderer so far.
    pub draws: u64,
}
impl Jetpacks {
    /// `effects`: the battle effect library's groups by name. `data`: where
    /// this owner's floats start in the effect data.
    pub fn new(content: &BattleContent, bridge: &mut Bridge, resources: &mut crate::resources::Resources, effects: &[(String, usize)], data: usize) -> Result<Self, String> {
        let defs = bfe_content::defs::Defs::build(&content.content)?;
        let battle = &content.content.battle;
        let mut classes = Vec::new();
        classes.resize_with(defs.registry.classes.len() + 1, || None);
        for class in &defs.classes {
            let (Some(id), Some(name)) = (class.registry_id, class.record.get("JetEffect").and_then(bfe_content::Value::as_str)) else {
                continue;
            };
            // game.js jetEffect: the side's own effect of that name, else
            // the shared one, else none.
            let side = defs.names.text(class.side);
            let own = battle.sides.iter().find(|s| s.name == side).and_then(|s| s.fx.as_ref()).and_then(|fx| fx.get("effects")).and_then(|e| e.get(name));
            let definitions = own.or_else(|| battle.effects.get("effects").and_then(|e| e.get(name))).map_or_else(|| "[]".to_string(), |d| authored_json(d).to_string());
            classes[id as usize] = Some(Class { definitions, classic: effects.iter().find(|(known, _)| known == name).map(|(_, group)| *group) });
        }
        let shadows = crate::shadows::COUNT > 0;
        // JetpackVolume's density field: 32 cubed bytes, repeating, linear.
        let source = resources.source(
            &serde_json::json!({"width":32,"height":32,"depth":32,"target":glow::TEXTURE_3D,"format":1028,"dataType":1009,"srgb":false}).to_string(),
        )?;
        resources.source_pixels(source, &bfe_render::effect_resources::flame_noise())?;
        let sample = resources.sample(
            source,
            r#"{"wrapS":1000,"wrapT":1000,"wrapR":1000,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}"#,
        )?;
        resources.upload(sample)?;
        let noise = bridge.r.core.add_texture(resources.texture(sample)?);
        let flame = bridge.effect_batch(bfe_render::effect_layout::FLAME as u32, 0, shadows, -1, lim::NOZZLES as u32, -1, &[noise as i32])?;
        let mut wakes = [0; 3];
        for (kind, (_, capacity, _, _)) in POOLS.iter().enumerate() {
            wakes[kind] = bridge.effect_batch(bfe_render::effect_layout::WAKE as u32, kind as u32, shadows, -1, *capacity as u32, -1, &[])?;
        }
        bridge.fx_reserve((data + FLOATS) as u32);
        Ok(Self {
            enhanced: true,
            classes,
            packs: (0..PACKS).map(|_| Pack { owner: Jetpack::default(), unit: None, class: usize::MAX, seen: false }).collect(),
            flame,
            wakes,
            data,
            rng: jsrt::rng::ClassicRandom::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0., |t| t.subsec_nanos() as f64)),
            draws: 0,
        })
    }
    /// The Classic exhaust's effect group for a class with a JetEffect.
    pub fn classic(&self, class: usize) -> Option<usize> {
        self.classes.get(class)?.as_ref()?.classic
    }
    pub fn begin(&mut self) {
        for pack in &mut self.packs {
            pack.seen = false;
        }
    }
    /// game.js updateJetFlame for one body: `active` while he is jetting
    /// and alive; `position` and `quaternion` are his ribcage's as posed
    /// this frame. A body takes a pack when it lights, and keeps it while
    /// its throttle runs down and its wake lives.
    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, unit: (u32, u32), class: usize, active: bool, position: [f64; 3], quaternion: [f64; 4], velocity: [f64; 3], dt: f64) {
        let Some(known) = self.classes.get(class).and_then(Option::as_ref) else {
            return;
        };
        let held = self.packs.iter().position(|pack| pack.unit == Some(unit));
        let at = match held {
            Some(at) => at,
            None if active && self.enhanced => {
                let Some(at) = self.packs.iter().position(|pack| pack.unit.is_none()) else {
                    return;
                };
                // A pack last worn by another class is rebuilt for this
                // one's nozzles; lighting a pack is not a per-frame path.
                if self.packs[at].class != class {
                    self.packs[at].owner = Jetpack::new(serde_json::from_str::<Vec<Definition>>(&known.definitions).unwrap_or_default());
                    self.packs[at].class = class;
                }
                self.packs[at].unit = Some(unit);
                at
            }
            None => return,
        };
        let pack = &mut self.packs[at];
        pack.seen = true;
        let x = &mut pack.owner.input;
        x[j::DT] = dt;
        x[j::ACTIVE] = active as u8 as f64;
        x[j::ENHANCED] = self.enhanced as u8 as f64;
        x[j::POSITION..j::POSITION + 3].copy_from_slice(&position);
        x[j::QUATERNION..j::QUATERNION + 4].copy_from_slice(&quaternion);
        x[j::VELOCITY..j::VELOCITY + 3].copy_from_slice(&velocity);
        // No outlet frames from the model: the owner places each nozzle
        // from its authored offset on the ribcage.
        for nozzle in 0..lim::NOZZLES {
            pack.owner.nozzles[nozzle * n::STRIDE + n::HAS_PREVIOUS] = 0.;
            pack.owner.nozzles[nozzle * n::STRIDE + n::RADIUS] = 0.034;
        }
        let rng = &mut self.rng;
        pack.owner.frame(&mut || rng.uniform());
    }
    /// The frame's draws: each pack's flame, then its wake pools. A pack
    /// whose body is gone, or that has burnt out, goes back to the pool.
    pub fn finish(&mut self, depth: bool, bridge: &mut Bridge) {
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        for at in 0..self.packs.len() {
            let pack = &mut self.packs[at];
            if pack.unit.is_none() {
                continue;
            }
            let wake = &pack.owner.wake;
            let live = (0..3).any(|kind| wake.input[w::COUNTS + kind] > 0.) || pack.owner.input[j::THROTTLE] > 0.;
            if !pack.seen || !live {
                pack.owner.input[j::CLEAR] = 1.;
                let rng = &mut self.rng;
                pack.owner.frame(&mut || rng.uniform());
                pack.unit = None;
                continue;
            }
            let base = self.data + at * PACK;
            // JetpackVolume.publish: the nozzles as the owner bent them.
            let volume = &pack.owner.volume;
            let count = (volume.input[v::COUNT] as usize).min(lim::NOZZLES);
            if volume.input[v::SHOWN] != 0. && count > 0 && draws < render::MAX_DRAWS {
                let x = &mut bridge.r.effects.data[base..base + FLAME];
                for i in 0..count {
                    let row = &volume.arrays[i * z::STRIDE..][..z::STRIDE];
                    x[i * 3..i * 3 + 3].copy_from_slice(&row[z::POSITION..z::POSITION + 3]);
                    x[count * 3 + i * 3..count * 3 + i * 3 + 3].copy_from_slice(&row[z::DIRECTION..z::DIRECTION + 3]);
                    x[count * 6 + i * 3..count * 6 + i * 3 + 3].copy_from_slice(&row[z::BEND..z::BEND + 3]);
                    x[count * 9 + i] = row[z::RADIUS];
                    x[count * 10 + i] = row[z::PHASE];
                }
                x[count * 11..count * 11 + 3].copy_from_slice(&[volume.input[v::TIME] as f32, volume.input[v::POWER] as f32, depth as u8 as f32]);
                let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
                row.fill(0.);
                row[..6].copy_from_slice(&[render::KIND_BATCH as f64, self.flame as f64, count as f64, 0., base as f64, count as f64]);
                row[render::DRAW_KEY + 1] = 10.;
                row[render::DRAW_KEY + 5] = self.flame as f64;
                draws += 1;
                self.draws += 1;
            }
            // WakePool.publish: birth, flow, axis and style for each particle.
            let mut pool_at = base + FLAME;
            for (kind, (first, capacity, drag, order)) in POOLS.iter().enumerate() {
                let count = (wake.input[w::COUNTS + kind] as usize).min(*capacity);
                if count > 0 && draws < render::MAX_DRAWS {
                    let x = &mut bridge.r.effects.data[pool_at..pool_at + capacity * 14 + 3];
                    for i in 0..count {
                        let row = &wake.particles[(first + i) * p::STRIDE..][..p::STRIDE];
                        x[i * 4..i * 4 + 4].copy_from_slice(&row[p::POSITION..p::POSITION + 4]);
                        x[count * 4 + i * 4..count * 4 + i * 4 + 4].copy_from_slice(&row[p::VELOCITY..p::VELOCITY + 4]);
                        x[count * 8 + i * 4..count * 8 + i * 4 + 4].copy_from_slice(&row[p::DIRECTION..p::DIRECTION + 4]);
                        x[count * 12 + i * 2] = row[p::SEED];
                        x[count * 12 + i * 2 + 1] = row[p::OPACITY];
                    }
                    x[count * 14..count * 14 + 3].copy_from_slice(&[wake.input[w::TIME] as f32, kind as f32, *drag]);
                    let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
                    row.fill(0.);
                    row[..6].copy_from_slice(&[render::KIND_BATCH as f64, self.wakes[kind] as f64, count as f64, 0., pool_at as f64, count as f64]);
                    row[render::DRAW_KEY + 1] = *order;
                    row[render::DRAW_KEY + 5] = self.wakes[kind] as f64;
                    draws += 1;
                    self.draws += 1;
                }
                pool_at += capacity * 14 + 3;
            }
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
    }
}
