//! What a hull shows beyond its model (vehicle-effects.js): its thrust
//! while a flyer is in the air, its damage smoke and fire by health band, a
//! dying walker's explosions, and the dust a wreck raises. The hulls owner
//! says each frame what should burn where (`want`) and what goes off once
//! (`shot`); the combat frame then starts, moves or stops the effects in
//! its next batch, as it does a jet exhaust.
use super::effects::Spawner;
use bfe_content::Value;

// s, DeathAnimationExplosionTime unless set.
const DYING_INTERVAL: f64 = 0.4;

pub struct Entry {
    group: usize,
    node: usize,
    offset: [f64; 3],
    min_scale: f64,
    max_scale: f64,
    scale_start: f64,
    start: f64,
    stop: f64,
    scale: f64,
    inherit: f64,
}

/// One hull class's effects, with its attach points bound to node indices
/// (a missing one binds the root, node 0, as the original game's do).
pub struct ClassFx {
    thrust: Vec<Entry>,
    damage: Vec<Entry>,
    // A hover's WaterEffect, raised on the water under it.
    water: Option<usize>,
    speed: [f64; 2],
    max_health: f64,
    // DeathDustEffect: group, offset from the root, delay.
    dust: Option<(usize, [f64; 3], f64)>,
    // A death animation: its span, the interval between explosions, the
    // explosions to pick from, and the effects at hardpoints as it ends.
    dying: Option<(f64, f64, Vec<usize>, Vec<Entry>)>,
    // Its registry id and its nodes' names (lower case), for its chunks.
    class: usize,
    names: Vec<String>,
    chunks: bool,
}

impl ClassFx {
    pub fn parse(class: usize, record: &Value, names: Vec<String>, effect: impl Fn(&str) -> Option<usize>) -> Option<Self> {
        let fx = record.get("fx")?;
        let node = |name: &str| names.iter().position(|n| !n.is_empty() && n.eq_ignore_ascii_case(name));
        let number = |v: &Value, key: &str, fallback: f64| v.get(key).and_then(Value::as_f64).unwrap_or(fallback);
        let entry = |v: &Value| -> Option<Entry> {
            let group = effect(v.get("effect").and_then(Value::as_str)?)?;
            let offset = v.get("offset").and_then(Value::as_array);
            Some(Entry {
                group,
                node: v.get("node").and_then(Value::as_str).and_then(&node).unwrap_or(0),
                offset: std::array::from_fn(|i| offset.and_then(|o| o.get(i)).and_then(Value::as_f64).unwrap_or(0.)),
                min_scale: number(v, "minScale", 1.),
                max_scale: number(v, "maxScale", 1.),
                scale_start: number(v, "scaleStart", 0.),
                start: number(v, "start", 1.),
                stop: number(v, "stop", 0.),
                scale: number(v, "scale", 1.),
                inherit: number(v, "inherit", 0.),
            })
        };
        let list = |key: &str| fx.get(key).and_then(Value::as_array).map_or(vec![], |a| a.iter().filter_map(entry).collect());
        let speed = fx.get("speed").and_then(Value::as_array);
        let dust = fx.get("deathDust").and_then(|d| {
            let e = entry(d)?;
            Some((e.group, e.offset, number(d, "delay", 0.)))
        });
        let dying = fx.get("dying").map(|d| {
            let effects = d.get("effects").and_then(Value::as_array).map_or(vec![], |a| a.iter().filter_map(|n| n.as_str().and_then(&effect)).collect());
            let last = d.get("final").and_then(Value::as_array).map_or(vec![], |a| a.iter().filter_map(entry).collect());
            (number(d, "span", 0.), d.get("interval").and_then(Value::as_f64).unwrap_or(DYING_INTERVAL), effects, last)
        });
        let max_health = bfe_content::odf::num(record.get("settings").unwrap_or(record).get("MaxHealth"), 1000.);
        let fx = Self {
            thrust: list("thrust"),
            water: fx.get("water").and_then(Value::as_str).and_then(&effect),
            damage: list("damage"),
            speed: [speed.and_then(|s| s.first()).and_then(Value::as_f64).unwrap_or(10.), speed.and_then(|s| s.get(1)).and_then(Value::as_f64).unwrap_or(30.)],
            max_health,
            dust,
            dying,
            class,
            chunks: fx.get("chunks").and_then(Value::as_array).is_some_and(|c| !c.is_empty()),
            names: names.iter().map(|n| n.to_lowercase()).collect(),
        };
        (!fx.thrust.is_empty() || !fx.damage.is_empty() || fx.water.is_some() || fx.dust.is_some() || fx.dying.is_some() || fx.chunks).then_some(fx)
    }
}

/// thrustScale: rests at its least scale and grows with speed past the
/// point ScaleStart of the way from the least to the most speed.
fn thrust_scale(entry: &Entry, speed: f64, [min, max]: [f64; 2]) -> f64 {
    let top = if max != 0. { max } else { 1. };
    let start = (top - min) * entry.scale_start + min;
    let span = top - start;
    let t = if span > 1e-6 { ((speed - start) / span).clamp(0., 1.) } else if speed > start { 1. } else { 0. };
    entry.min_scale + (entry.max_scale - entry.min_scale) * t
}

/// damageBurns: above its stop share of full health, at most its start.
fn damage_burns(entry: &Entry, health: f64, max: f64) -> bool {
    health > entry.stop * max && health <= entry.start * max
}

/// One hull's life as its effects see it.
#[derive(Default)]
pub struct Body {
    generation: Option<u32>,
    alive: bool,
    // While dying: explosions fired so far, and whether its dust rose.
    dying: Option<(f64, bool)>,
    dust_left: Option<f64>,
    // Where its root was last drawn, for the dust of a wreck no longer drawn.
    root: [f64; 16],
    // Each node's world matrix as last drawn, where its chunks start.
    nodes: Vec<f64>,
    velocity: [f64; 3],
}

/// What a hull is doing this frame.
pub struct Frame<'a> {
    pub key: u64,
    pub generation: u32,
    /// 0 alive, 1 dying (a death animation playing), 2 destroyed.
    pub state: u8,
    /// Seconds into its death animation.
    pub dying: f64,
    pub health: f64,
    /// A flyer taking off, flying or landing.
    pub thrusting: bool,
    pub velocity: [f64; 3],
    /// Each node's world matrix, `stride` values apart from `at`; none once
    /// the hull is no longer drawn.
    pub nodes: Option<(&'a [f64], usize, usize)>,
    /// The water's level under the hull's root, when there is water there.
    pub water: Option<f64>,
}

struct Want {
    key: u64,
    group: usize,
    position: [f64; 3],
    turn: [f64; 4],
    velocity: [f64; 3],
    scale: f64,
    inherit: f64,
}

/// A wrecked hull's chunks: its class, its nodes' world matrices and
/// names, where it stood and how it moved.
pub struct Wreck {
    pub class: usize,
    pub nodes: Vec<f64>,
    pub names: Vec<String>,
    pub origin: [f64; 3],
    pub velocity: [f64; 3],
}

#[derive(Default)]
pub struct HullEffects {
    /// Wrecks whose chunks fly this frame.
    pub wrecks: Vec<Wreck>,
    wanted: Vec<Want>,
    shots: Vec<(usize, [f64; 3], [f64; 4])>,
    live: Vec<(u64, u32, bool)>,
    random: u32,
    /// Hull effects started so far; hull frames seen, and the lowest health
    /// share among them, since the last report.
    pub started: u64,
    pub seen: u64,
    pub lowest: f64,
    /// Walker footfalls so far.
    pub stomps: u64,
}

fn place(nodes: &[f64], stride: usize, at: usize, node: usize, offset: &[f64; 3]) -> Option<([f64; 3], [f64; 4])> {
    let m = nodes.get(node * stride + at..node * stride + at + 16)?;
    let mut matrix = three::Matrix4::default();
    matrix.elements.copy_from_slice(m);
    if offset.iter().any(|v| *v != 0.) {
        let mut shift = three::Matrix4::default();
        shift.make_translation(offset[0], offset[1], offset[2]);
        let base = matrix;
        matrix.multiply_matrices(&base, &shift);
    }
    let (mut p, mut q, mut s) = (three::Vector3::default(), three::Quaternion::default(), three::Vector3::default());
    matrix.decompose(&mut p, &mut q, &mut s);
    Some(([p.x, p.y, p.z], [q.x, q.y, q.z, q.w]))
}

impl HullEffects {
    pub fn new() -> Self {
        Self { lowest: 1., ..Default::default() }
    }
    // Math.random() for the dying explosions' picks; decides nothing shared.
    fn random(&mut self) -> f64 {
        self.random = self.random.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.random >> 8) as f64 / (1u32 << 24) as f64
    }

    /// VehicleEffects.updateVehicle for one hull.
    pub fn hull(&mut self, fx: &ClassFx, body: &mut Body, hull: Frame, dt: f64) {
        if body.generation != Some(hull.generation) {
            *body = Body { generation: Some(hull.generation), alive: hull.state == 0, ..Default::default() };
        }
        if let Some((nodes, stride, at)) = hull.nodes {
            if let Some(root) = nodes.get(at..at + 16) {
                body.root.copy_from_slice(root);
            }
            if fx.chunks {
                body.nodes.clear();
                for node in nodes.chunks_exact(stride) {
                    body.nodes.extend_from_slice(&node[at..at + 16]);
                }
            }
            body.velocity = hull.velocity;
        }
        if hull.state != 0 {
            // Destroyed: its effects out (nothing is wanted any more); a
            // dying walker's dust and explosions follow its animation.
            if body.alive {
                body.dying = (hull.state == 1 && fx.dying.is_some()).then_some((0., fx.dust.is_none()));
                body.dust_left = fx.dust.filter(|_| body.dying.is_none()).map(|(_, _, delay)| delay);
                if body.dying.is_none() {
                    self.wreck(fx, body);
                }
            }
            body.alive = false;
            if let (Some((fired, dusted)), Some((span, interval, effects, last))) = (body.dying.as_mut(), fx.dying.as_ref()) {
                if hull.state == 2 {
                    // The animation is over: its final effects at their
                    // hardpoints (an animated building's).
                    if let Some((nodes, stride, at)) = hull.nodes {
                        for e in last {
                            if let Some((p, q)) = place(nodes, stride, at, e.node, &[0.; 3]) {
                                self.shots.push((e.group, p, q));
                            }
                        }
                    }
                    body.dying = None;
                    self.wreck(fx, body);
                } else {
                    let elapsed = hull.dying.min(*span).max(0.);
                    if !*dusted {
                        if let Some((group, offset, delay)) = fx.dust {
                            if elapsed > delay {
                                *dusted = true;
                                if let Some((p, _)) = place(&body.root, 0, 0, 0, &offset) {
                                    self.shots.push((group, p, [0., 0., 0., 1.]));
                                }
                            }
                        }
                    }
                    let due = (elapsed / interval.max(1e-3)).floor();
                    if !effects.is_empty() && due > *fired {
                        *fired = due;
                        let group = effects[(self.random() * effects.len() as f64) as usize % effects.len()];
                        if let Some((nodes, stride, at)) = hull.nodes {
                            let count = nodes.len() / stride;
                            let node = (self.random() * count as f64) as usize % count.max(1);
                            if let Some((p, q)) = place(nodes, stride, at, node, &[0.; 3]) {
                                self.shots.push((group, p, q));
                            }
                        }
                    }
                }
            }
            if let (Some(left), Some((group, offset, _))) = (body.dust_left.as_mut(), fx.dust) {
                if dt > 0. {
                    *left -= dt;
                    if *left <= 0. {
                        if let Some((p, _)) = place(&body.root, 0, 0, 0, &offset) {
                            self.shots.push((group, p, [0., 0., 0., 1.]));
                        }
                        body.dust_left = None;
                    }
                }
            }
            return;
        }
        body.alive = true;
        self.seen += 1;
        self.lowest = self.lowest.min(hull.health / fx.max_health);
        let Some((nodes, stride, at)) = hull.nodes else {
            return;
        };
        let speed = hull.velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
        // updateWater: within WATER_HEIGHT (4 m) of the surface, its spray
        // on the water under the root, moving with the hull.
        if let (Some(group), Some(level), Some(root)) = (fx.water, hull.water, nodes.get(at..at + 16)) {
            if (root[13] - level).abs() < 4. {
                self.wanted.push(Want { key: hull.key << 8 | 64, group, position: [root[12], level, root[14]], turn: [0., 0., 0., 1.], velocity: hull.velocity, scale: 1., inherit: 0. });
            }
        }
        if hull.thrusting {
            for (i, e) in fx.thrust.iter().enumerate() {
                if let Some((position, turn)) = place(nodes, stride, at, e.node, &e.offset) {
                    let scale = thrust_scale(e, speed, fx.speed);
                    self.wanted.push(Want { key: hull.key << 8 | i as u64, group: e.group, position, turn, velocity: hull.velocity, scale, inherit: 0. });
                }
            }
        }
        for (i, e) in fx.damage.iter().enumerate() {
            if !damage_burns(e, hull.health, fx.max_health) {
                continue;
            }
            if let Some((position, turn)) = place(nodes, stride, at, e.node, &e.offset) {
                self.wanted.push(Want { key: hull.key << 8 | 128 | i as u64, group: e.group, position, turn, velocity: hull.velocity, scale: e.scale, inherit: e.inherit });
            }
        }
    }

    /// An effect that goes off once where it is placed (a walker's stomp).
    pub fn shot(&mut self, group: usize, position: [f64; 3], turn: [f64; 4]) {
        self.stomps += 1;
        self.shots.push((group, position, turn));
    }
        fn wreck(&mut self, fx: &ClassFx, body: &Body) {
        if fx.chunks && !body.nodes.is_empty() {
            self.wrecks.push(Wreck {
                class: fx.class,
                nodes: body.nodes.clone(),
                names: fx.names.clone(),
                origin: [body.root[12], body.root[13], body.root[14]],
                velocity: body.velocity,
            });
        }
    }

    /// The combat frame's batch: wanted effects start or follow their
    /// place, the rest stop; one-shots start.
    pub fn apply(&mut self, effects: &mut Spawner) {
        for live in &mut self.live {
            live.2 = false;
        }
        for want in self.wanted.drain(..) {
            // `if (!handle || handle.done) handle = play(...)`: a burnt-out
            // effect (a spark's few bursts) starts again.
            if let Some(at) = self.live.iter().position(|(key, handle, _)| *key == want.key && !effects.playing(*handle)) {
                self.live.swap_remove(at);
            }
            match self.live.iter_mut().find(|(key, _, _)| *key == want.key) {
                Some(live) => {
                    live.2 = true;
                    effects.scale(live.1, want.scale, want.inherit);
                    effects.sync(live.1, want.position, Some(want.turn), Some(want.velocity));
                }
                None => {
                    if let Some(handle) = effects.start_turned(want.group, want.position, want.velocity, want.turn, want.key as u32) {
                        // Its scale and inherit reach the player with the
                        // command that follows its start.
                        effects.scale(handle, want.scale, want.inherit);
                        effects.sync(handle, want.position, Some(want.turn), Some(want.velocity));
                        self.live.push((want.key, handle, true));
                        self.started += 1;
                    }
                }
            }
        }
        self.live.retain(|&(_, handle, seen)| {
            if !seen {
                effects.stop(handle);
            }
            seen
        });
        for (group, position, turn) in self.shots.drain(..) {
            if effects.start_turned(group, position, [0.; 3], turn, self.random).is_some() {
                self.started += 1;
            }
        }
    }

    /// Effects burning now; restarts the report's frame count and lowest
    /// health share.
    pub fn live_count(&mut self) -> usize {
        self.seen = 0;
        self.lowest = 1.;
        self.live.len()
    }
    /// Another battle: nothing carries over.
    pub fn clear(&mut self, effects: &mut Spawner) {
        for (_, handle, _) in self.live.drain(..) {
            effects.stop(handle);
        }
        self.wanted.clear();
        self.shots.clear();
        self.wrecks.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry() -> Entry {
        Entry { group: 0, node: 0, offset: [0.; 3], min_scale: 0.5, max_scale: 2., scale_start: 0.5, start: 0.5, stop: 0.2, scale: 1., inherit: 0. }
    }
    #[test]
    fn thrust_grows_with_speed_past_its_start() {
        let e = entry();
        assert_eq!(thrust_scale(&e, 0., [10., 30.]), 0.5);
        assert_eq!(thrust_scale(&e, 20., [10., 30.]), 0.5);
        assert_eq!(thrust_scale(&e, 30., [10., 30.]), 2.);
        assert!((thrust_scale(&e, 25., [10., 30.]) - 1.25).abs() < 1e-9);
    }
    #[test]
    fn damage_burns_inside_its_band() {
        let e = entry();
        assert!(!damage_burns(&e, 60., 100.));
        assert!(damage_burns(&e, 50., 100.));
        assert!(damage_burns(&e, 21., 100.));
        assert!(!damage_burns(&e, 20., 100.));
    }
}
