//! The player's rolling droideka's dust (droideka-trail.js) over the shared Trail
//! state: with droidekaEffects Enhanced, a ball rolling outdoors on dry
//! ground faster than 2.2 m/s kicks up dust behind it, more as it speeds up
//! and turns, tinted for the map. Classic draws none.
use super::replay::Collision;
use crate::render_bridge::Bridge;
use bfe_presentation::{unit_effects::trail::Trail, visual_effects_layout::trail_frame as f};
use bfe_render::{effect_layout as layout, effects as render};

/// Droidekas trailing dust at once.
const TRAILS: usize = 6;
const CAPACITY: usize = 160;
const FLOATS: usize = CAPACITY * 12 + 4;

/// What a droideka is doing this frame.
pub struct Roller {
    pub unit: (u32, u32),
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub rolling: bool,
    pub dead: bool,
}

pub struct Trails {
    pub enhanced: bool,
    name: String,
    trails: Vec<(Option<(u32, u32)>, Trail, bool)>,
    batch: u32,
    at: usize,
    collision: Collision,
    world: u32,
    water: Option<bfe_sim::units::water::WaterState>,
    rng: jsrt::rng::ClassicRandom,
    /// Dust particles drawn, summed over frames.
    pub drawn: u64,
}

impl Trails {
    pub fn new(content: &bfe_content_native::BattleContent, bridge: &mut Bridge, collision: Collision, world: u32, at: usize) -> Result<Self, String> {
        let batch = bridge.effect_batch(layout::TRAIL as u32, 0, bridge.shadow_cascades() > 0, -1, CAPACITY as u32, -1, &[])?;
        bridge.fx_batch_interleaved(batch);
        bridge.fx_reserve((at + TRAILS * FLOATS) as u32);
        let name = content.map["name"].as_str().or_else(|| content.map["map"].as_str()).unwrap_or("").to_string();
        Ok(Self {
            enhanced: true,
            trails: (0..TRAILS).map(|_| (None, Trail::new(&name), false)).collect(),
            name,
            batch,
            at,
            collision,
            world,
            water: super::water::new(content),
            rng: jsrt::rng::ClassicRandom::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0., |t| t.subsec_nanos() as f64)),
            drawn: 0,
        })
    }

    pub fn end(&self) -> usize {
        self.at + TRAILS * FLOATS
    }

    fn cast(&self, origin: [f64; 3], up: bool, far: f64) -> Option<bfe_sim::collision::Hit> {
        let ray = bfe_sim::collision::Ray { ox: origin[0], oy: origin[1], oz: origin[2], dx: 0., dy: if up { 1. } else { -1. }, dz: 0. };
        self.collision.borrow_mut().ray_hit(self.world, ray, far, -1).map(|(_, h)| h)
    }

    /// DroidekaTrail.update for every droideka drawn, then the draws.
    pub fn frame(&mut self, dt: f64, rollers: &[Roller], bridge: &mut Bridge) {
        for t in &mut self.trails {
            t.2 = false;
        }
        for roller in rollers {
            let at = match self.trails.iter().position(|t| t.0 == Some(roller.unit)) {
                Some(at) => at,
                None if self.enhanced && roller.rolling => match self.trails.iter().position(|t| t.0.is_none()) {
                    Some(at) => {
                        self.trails[at] = (Some(roller.unit), Trail::new(&self.name), false);
                        at
                    }
                    None => continue,
                },
                None => continue,
            };
            self.trails[at].2 = true;
            let water = self.water.as_ref().filter(|w| w.over(roller.position[0], roller.position[2])).map(|w| w.level());
            let enhanced = self.enhanced;
            let trail = &mut self.trails[at].1;
            let x = &mut trail.input;
            x[f::DT] = dt;
            x[f::ENHANCED] = enhanced as u8 as f64;
            x[f::POSITION..f::POSITION + 3].copy_from_slice(&roller.position);
            x[f::VELOCITY..f::VELOCITY + 3].copy_from_slice(&roller.velocity);
            x[f::ROLLING] = roller.rolling as u8 as f64;
            x[f::DEAD] = roller.dead as u8 as f64;
            x[f::AIR] = 0.;
            x[f::GROUNDED] = 1.;
            x[f::WET] = water.is_some() as u8 as f64;
            x[f::WATER_LEVEL] = water.unwrap_or(0.);
            trail.probe();
            if trail.input[f::PROBE] != 0. {
                // DroidekaTrail.probe: the floor 0.6 m up from the body, then
                // the open sky 80 m up from just over that floor.
                let origin = [trail.rays[0], trail.rays[1], trail.rays[2]];
                let floor = self.collision.borrow_mut().ray_hit(self.world, bfe_sim::collision::Ray { ox: origin[0], oy: origin[1], oz: origin[2], dx: 0., dy: -1., dz: 0. }, 1.3, -1);
                let trail = &mut self.trails[at].1;
                trail.input[f::FLOOR_HIT] = floor.is_some() as u8 as f64;
                if let Some((code, h)) = floor {
                    trail.input[f::FLOOR_NORMAL] = h.ny;
                    trail.input[f::FLOOR] = h.py;
                    trail.input[f::TERRAIN] = (code == bfe_sim::collision::TERRAIN) as u8 as f64;
                }
                trail.roof();
                if trail.input[f::ROOF_PROBE] != 0. {
                    let origin = [trail.rays[0], trail.rays[1], trail.rays[2]];
                    let roof = self.cast(origin, true, 80.).is_some();
                    self.trails[at].1.input[f::ROOF] = roof as u8 as f64;
                }
            }
            let rng = &mut self.rng;
            self.trails[at].1.frame(&mut || rng.uniform());
        }
        let mut draws = bridge.r.effects.input[render::IN_DRAWS] as usize;
        for (k, t) in self.trails.iter_mut().enumerate() {
            if t.0.is_none() {
                continue;
            }
            let count = t.1.input[f::COUNT] as usize;
            if !t.2 || (count == 0 && !self.enhanced) {
                t.0 = None;
                continue;
            }
            if count == 0 || draws == render::MAX_DRAWS {
                continue;
            }
            let at = self.at + k * FLOATS;
            let data = &mut bridge.r.effects.data;
            data[at..at + count * 12].copy_from_slice(&t.1.records[..count * 12]);
            let v = at + count * 12;
            data[v] = t.1.input[f::TIME] as f32;
            for c in 0..3 {
                data[v + 1 + c] = t.1.input[f::TINT + c] as f32;
            }
            let row = &mut bridge.r.effects.input[render::DRAW_AT + draws * render::DRAW_STRIDE..][..render::DRAW_STRIDE];
            row.fill(0.);
            row[..6].copy_from_slice(&[render::KIND_BATCH as f64, self.batch as f64, count as f64, 0., at as f64, CAPACITY as f64]);
            row[render::DRAW_KEY + 1] = 9.;
            row[render::DRAW_KEY + 2..render::DRAW_KEY + 5].copy_from_slice(&[t.1.records[0] as f64, t.1.records[1] as f64, t.1.records[2] as f64]);
            row[render::DRAW_KEY + 5] = self.batch as f64 + k as f64 * 1e-3;
            draws += 1;
            self.drawn += count as u64;
        }
        bridge.r.effects.input[render::IN_DRAWS] = draws as f64;
    }
}
