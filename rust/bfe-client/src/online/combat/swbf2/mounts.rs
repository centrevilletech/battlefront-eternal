//! Exhausts, contrails and damage effects follow the pose between consecutive observations.
use super::{
    authored_json, group, BattleContent, Matrix4, Quaternion, Receiver, Spawner, Value, Vector3,
};
use bfe_presentation::network_layout::{net_kind as kind, net_limits, net_record as r};
use serde::Deserialize;
use std::collections::{HashMap, VecDeque};

mod population;
use population::{Demand, Lens, Plays, Ranked};

const MOUNT_LIMIT: usize = crate::online::effects::SLOTS - 1;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MountClass(usize);

#[derive(Clone, Copy)]
pub struct MountObservation {
    pub owner: u64,
    pub generation: u32,
    pub class: MountClass,
    pub tick: f64,
    pub position: [f64; 3],
    pub rotation: [f64; 4],
    pub velocity: [f64; 3],
    pub health: f64,
    pub thrusting: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Damage,
    Thrust,
    Contrail,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Mount {
    #[serde(rename = "kind")]
    kind: Kind,
    #[serde(rename = "effect")]
    effect: String,
    #[serde(rename = "matrix")]
    local: [f64; 16],
    #[serde(skip)]
    group: Option<usize>,
    #[serde(skip)]
    demand: Demand,
    #[serde(skip)]
    needs_motion: bool,
    #[serde(default)]
    start_percent: f64,
    #[serde(default)]
    stop_percent: f64,
    #[serde(default = "one")]
    effect_scale: f64,
    #[serde(default)]
    inherit_velocity: f64,
    #[serde(default)]
    effect_min_scale: f64,
    #[serde(default = "one")]
    effect_max_scale: f64,
    #[serde(default)]
    effect_scale_start: f64,
    /// ContrailEffectMinSpeed; the game's class default is 1.1, and a contrail
    /// whose value is under 0.5 is never drawn.
    #[serde(default = "contrail_min_speed")]
    effect_min_speed: f64,
}
fn contrail_min_speed() -> f64 {
    1.1
}
impl Mount {
    fn for_source(&self, source: Source) -> bool {
        !matches!((source, &self.kind),
            (Source::Engine, Kind::Damage) | (Source::Hull, Kind::Thrust | Kind::Contrail))
    }
}
fn one() -> f64 {
    1.
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Class {
    mounts: Vec<Mount>,
    min_speed: f64,
    max_speed: f64,
    #[serde(default)]
    boost_speed: f64,
    max_health: f64,
}
impl Class {
    /// A damage effect burns while the health fraction is below its start and,
    /// for a positive stop, at or above its stop, each with a ten-thousandth of
    /// slack, and while the body has health left (the 2005 game's comparisons).
    /// Whether the body has been damaged at all is the caller's: see `Body::damaged`.
    fn scale(&self, m: &Mount, health: f64, speed: f64, thrust: bool) -> Option<f64> {
        if matches!(m.kind, Kind::Damage) {
            const SLACK: f64 = 1e-4;
            let fraction = health / self.max_health;
            let stop = m.stop_percent / 100.;
            // A stop that small is read as two ten-thousandths.
            let stop = if stop > 0. && stop <= 2. * SLACK { 2. * SLACK } else { stop };
            return (self.max_health > 0.
                && health > SLACK
                && fraction < m.start_percent / 100. + SLACK
                && (stop <= 0. || fraction >= stop - SLACK))
                .then_some(m.effect_scale);
        }
        if matches!(m.kind, Kind::Thrust) && !thrust {
            return None;
        }
        if matches!(m.kind, Kind::Contrail) {
            // The 2005 game's contrails: only a mount whose ContrailEffectMinSpeed
            // is at least 0.5, only past MaxSpeed and a tenth of the way on to
            // BoostSpeed, scaled from EffectMinScale to EffectMaxScale up to
            // BoostSpeed (one and a half times that start without a boost).
            if m.effect_min_speed < 0.5 {
                return None;
            }
            let start = self.max_speed + (self.boost_speed - self.max_speed) * 0.1;
            let top = if self.boost_speed > start { self.boost_speed } else { start * 1.5 };
            if speed < start || top <= start {
                return None;
            }
            let f = ((speed - start) / (top - start)).clamp(0., 1.);
            let scale = m.effect_min_scale + (m.effect_max_scale - m.effect_min_scale) * f;
            return (scale > 0.).then_some(scale);
        }
        // The ramp starts its share of the way from MinSpeed to MaxSpeed and
        // runs on to the larger of MaxSpeed and BoostSpeed; only its bottom
        // is clamped.
        let max_speed = if self.max_speed == 0. { 1. } else { self.max_speed };
        let top_speed = max_speed.max(self.boost_speed);
        let start = self.min_speed + (max_speed - self.min_speed) * m.effect_scale_start;
        let f = if top_speed > start {
            ((speed - start) / (top_speed - start)).max(0.)
        } else {
            0.
        };
        let scale = m.effect_min_scale + (m.effect_max_scale - m.effect_min_scale) * f;
        (scale > 0.).then_some(scale)
    }
}

/// A guided remote's engine (a steered rocket), read from its own class record:
/// its flame (`ThrustEffect*`) and its contrail (`ContrailEffect*`) are scaled by
/// its airspeed with the same rule as a craft's mounts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::online) struct RemoteEngine {
    min_speed: f64,
    max_speed: f64,
    boost_speed: f64,
    /// EffectMinScale, EffectMaxScale, EffectScaleStart.
    thrust: [f64; 3],
    /// The same, then EffectMinSpeed.
    contrail: [f64; 4],
}

impl RemoteEngine {
    pub(in crate::online) fn read(def: &Value) -> Self {
        let n = |key: &str, default: f64| {
            def.get(key)
                .and_then(|v| v.as_f64().or_else(|| v.as_str()?.split_whitespace().next()?.parse().ok()))
                .filter(|v: &f64| v.is_finite())
                .unwrap_or(default)
        };
        Self {
            min_speed: n("MinSpeed", 0.),
            max_speed: n("MaxSpeed", 0.),
            boost_speed: n("BoostSpeed", 0.),
            thrust: [n("ThrustEffectMinScale", 0.), n("ThrustEffectMaxScale", 1.), n("ThrustEffectScaleStart", 0.)],
            contrail: [
                n("ContrailEffectMinScale", 0.),
                n("ContrailEffectMaxScale", 1.),
                n("ContrailEffectScaleStart", 0.),
                n("ContrailEffectMinSpeed", 1.1),
            ],
        }
    }

    /// The flame's and the contrail's scale at `speed`; None where the rule shows none.
    pub(in crate::online) fn scales(&self, speed: f64) -> (Option<f64>, Option<f64>) {
        let class = Class { mounts: Vec::new(), min_speed: self.min_speed, max_speed: self.max_speed, boost_speed: self.boost_speed, max_health: 1. };
        let mount = |kind, [min, max, start, min_speed]: [f64; 4]| Mount {
            kind,
            effect: String::new(),
            local: [0.; 16],
            group: None,
            demand: Demand::default(),
            needs_motion: false,
            start_percent: 0.,
            stop_percent: 0.,
            effect_scale: 1.,
            inherit_velocity: 0.,
            effect_min_scale: min,
            effect_max_scale: max,
            effect_scale_start: start,
            effect_min_speed: min_speed,
        };
        let [min, max, start] = self.thrust;
        (
            class.scale(&mount(Kind::Thrust, [min, max, start, 0.]), 1., speed, true),
            class.scale(&mount(Kind::Contrail, self.contrail), 1., speed, true),
        )
    }
}

fn parse_speed(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) if n.is_finite() && *n > 0. => Some(*n),
        Value::String(s) => s
            .split_whitespace()
            .next()
            .and_then(|tok| tok.parse::<f64>().ok())
            .filter(|n| n.is_finite() && *n > 0.),
        _ => None,
    }
}

fn boost_speed_for(battle: &BattleContent, class_name: &str) -> Option<f64> {
    let mut found = None;
    for side in &battle.content.battle.sides {
        let Some(units) = side.units.as_object() else {
            continue;
        };
        for (name, def) in units.entries() {
            if !name.eq_ignore_ascii_case(class_name) {
                continue;
            }
            if let Some(props) = def.get("properties") {
                if let Some(obj) = props.as_object() {
                    for (k, v) in obj.entries() {
                        if k.eq_ignore_ascii_case("BoostSpeed") {
                            if let Some(s) = parse_speed(v) {
                                found = Some(s);
                            }
                        }
                    }
                } else if let Some(arr) = props.as_array() {
                    for pair in arr {
                        let (Some(k), Some(v)) = (pair.at(0).and_then(Value::as_str), pair.at(1))
                        else {
                            continue;
                        };
                        if k.eq_ignore_ascii_case("BoostSpeed") {
                            if let Some(s) = parse_speed(v) {
                                found = Some(s);
                            }
                        }
                    }
                }
            }
            if let Some(sections) = def.get("vehicleSections").and_then(Value::as_array) {
                for sec in sections {
                    if !sec
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(|k| k.eq_ignore_ascii_case("chassis"))
                    {
                        continue;
                    }
                    if let Some(arr) = sec.get("properties").and_then(Value::as_array) {
                        for pair in arr {
                            let (Some(k), Some(v)) =
                                (pair.at(0).and_then(Value::as_str), pair.at(1))
                            else {
                                continue;
                            };
                            if k.eq_ignore_ascii_case("BoostSpeed") {
                                if let Some(s) = parse_speed(v) {
                                    found = Some(s);
                                }
                            }
                        }
                    }
                }
            }
            if found.is_some() {
                return found;
            }
        }
    }
    found
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    Vehicle,
    Building,
    Recorded,
    Engine,
    Hull,
}
const OBSERVED: [Source; 3] = [Source::Recorded, Source::Engine, Source::Hull];

#[derive(Clone, Copy, PartialEq, Eq)]
struct Life {
    generation: u32,
    retired: bool,
}

fn newer(generation: u32, previous: u32) -> bool {
    let distance = generation.wrapping_sub(previous);
    distance != 0 && distance < (1 << 31)
}

struct Body {
    source: Source,
    prev: MountObservation,
    next: MountObservation,
    alive: bool,
    view: ViewState,
    /// The tick of this life's earliest observation whose health differs
    /// from the one before it (minus infinity for a life first seen damaged).
    changed: Option<f64>,
}
impl Body {
    /// The 2005 game looks at a body's damage effects only when its health
    /// changes, so a life never damaged shows none, even an effect whose band
    /// reaches full health; after any change the bands decide, also back
    /// at full health.
    fn damaged(&self, tick: f64) -> bool {
        self.changed.is_some_and(|at| tick >= at)
    }
    fn first_seen(s: &MountObservation, full: f64) -> Option<f64> {
        (s.health < full).then_some(f64::NEG_INFINITY)
    }
    fn moving(&self, tick: f64) -> bool {
        let (a, b) = (&self.prev, &self.next);
        let interval = b.tick - a.tick;
        // A repeated display frame may precede the next fixed-step publication.
        // Keep the last moving pose through up to a quarter second of jitter.
        // A newer stationary pose stops the strip immediately.
        interval > 0. && tick <= b.tick + interval.max(15.)
            && (a.position != b.position
                || (a.rotation != b.rotation && a.rotation != b.rotation.map(|n| -n)))
    }

    fn at(&self, tick: f64) -> (Matrix4, [f64; 3], f64, bool) {
        let (a, b) = (&self.prev, &self.next);
        let t = if b.tick > a.tick {
            ((tick - a.tick) / (b.tick - a.tick)).clamp(0., 1.)
        } else {
            1.
        };
        let lerp = |a: f64, b: f64| a + (b - a) * t;
        let pos = std::array::from_fn::<_, 3, _>(|i| lerp(a.position[i], b.position[i]));
        let vel = std::array::from_fn(|i| lerp(a.velocity[i], b.velocity[i]));
        let mut rot = [0.; 4];
        Quaternion::slerp_flat(&mut rot, 0, &a.rotation, 0, &b.rotation, 0, t);
        let mut matrix = Matrix4::default();
        matrix.compose(
            &Vector3::new(pos[0], pos[1], pos[2]),
            &Quaternion::new(rot[0], rot[1], rot[2], rot[3]),
            &Vector3::new(1., 1., 1.),
        );
        let state = if t < 1. { a } else { b };
        (matrix, vel, state.health, state.thrusting)
    }
}

struct ViewState {
    inside: bool,
    sockets: usize,
    rate: Option<f64>,
}

struct Live {
    handle: u32,
    seen: bool,
    plays: Plays,
}

pub(super) struct Mounts {
    classes: Vec<(String, Class)>,
    class_ids: HashMap<u32, MountClass>,
    vehicles: Vec<Option<MountClass>>,
    placed: Vec<Option<MountObservation>>,
    bodies: Vec<Body>,
    body_index: HashMap<(Source, u64), usize>,
    // External pose, throttle and hull publications share the same life for an owner.
    lives: HashMap<u64, Life>,
    retired: VecDeque<(u64, u32)>,
    // Only sockets with live particles occupy an entry, irrespective of the class size.
    live: HashMap<(Source, u64, u32, MountClass, usize), Live>,
    ranked: Vec<Ranked>,
    scratch: Vec<Ranked>,
    budget: usize,
    lens: Lens,
    /// Seconds of display time, for how long a burnt-out effect rests.
    clock: f64,
    pub(super) refused: u32,
}
impl Mounts {
    pub fn new(
        pack: &Value,
        registry: &[String],
        groups: &[(String, usize)],
        battle: &BattleContent,
        content: &bfe_presentation::Content,
    ) -> Result<Self, String> {
        let mut classes = Vec::new();
        if let Some(map) = pack.get("mounts").and_then(Value::as_object) {
            for (name, data) in map.entries() {
                let mut class: Class = match serde_json::from_value(authored_json(data)) {
                    Ok(class) => class,
                    Err(e) => {
                        eprintln!("Skipping mission effect mounts for {name}: {e}");
                        continue;
                    }
                };
                if class.boost_speed <= 0. {
                    class.boost_speed = boost_speed_for(battle, name).unwrap_or(0.);
                }
                for m in &mut class.mounts {
                    m.group = group(groups, &m.effect);
                    if let Some(group) = m.group {
                        m.demand = Demand::of(content, group);
                        // A stationary socket cannot span a strip between identical births.
                        m.needs_motion = matches!(m.kind, Kind::Contrail)
                            && m.inherit_velocity == 0.
                            && content.groups[group].iter().all(|&i| {
                                let d = &content.definitions[i];
                                let sp = &d.spawner;
                                d.geometry.r#type == "trail" && d.register_step
                                    && sp.position_scale == [0.; 2] && sp.velocity_scale == [0.; 2]
                                    && sp.inherit_velocity == [0.; 2]
                                    && sp.offset.iter().all(|v| v[0] == v[1])
                                    && d.transformer.position.iter().all(|k| k.r#move.is_none()
                                        && k.reach.is_none() && k.accelerate.is_none())
                            });
                    }
                }
                classes.push((name.to_string(), class));
            }
        }
        let lookup = |name: &str| {
            classes
                .iter()
                .position(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(MountClass)
        };
        let vehicles = std::iter::once(None)
            .chain(registry.iter().map(|n| lookup(n)))
            .collect();
        let instances = battle
            .content
            .battle
            .props
            .as_ref()
            .and_then(|p| p.get("instances"));
        let placed = bfe_sim::world::admit::rows::props::live_sources(&battle.content.battle)
            .into_iter()
            .enumerate()
            .map(|(id, index)| {
                let data = instances?.at(index)?;
                let class = lookup(data.get("class")?.as_str()?)?;
                let vals = data.get("matrix")?.as_array()?;
                if vals.len() != 16 {
                    return None;
                }
                let mut m = Matrix4::default();
                for (d, v) in m.elements.iter_mut().zip(vals) {
                    *d = v.as_f64()?;
                }
                let (mut pos, mut rot, mut scale) = (
                    Vector3::default(),
                    Quaternion::default(),
                    Vector3::default(),
                );
                m.decompose(&mut pos, &mut rot, &mut scale);
                Some(MountObservation {
                    owner: id as u64,
                    generation: 0,
                    class,
                    tick: 0.,
                    position: [pos.x, pos.y, pos.z],
                    rotation: [rot.x, rot.y, rot.z, rot.w],
                    velocity: [0.; 3],
                    health: classes[class.0].1.max_health,
                    thrusting: false,
                })
            })
            .collect();
        Ok(Self {
            refused: 0,
            class_ids: classes
                .iter()
                .enumerate()
                .map(|(i, (name, _))| (bfe_content::zero_hash::fnv1a(name), MountClass(i)))
                .collect(),
            classes,
            vehicles,
            placed,
            bodies: Vec::with_capacity(net_limits::RECORDS),
            body_index: HashMap::with_capacity(net_limits::RECORDS * 2),
            lives: HashMap::with_capacity(net_limits::RECORDS * 2),
            retired: VecDeque::with_capacity(net_limits::RECORDS),
            live: HashMap::with_capacity(MOUNT_LIMIT * 2),
            ranked: Vec::with_capacity(net_limits::RECORDS),
            scratch: Vec::with_capacity(net_limits::RECORDS),
            budget: super::particle_limit(pack) * 3 / 4,
            lens: Lens::default(),
            clock: 0.,
        })
    }
    pub fn camera(&mut self, values: &[f64]) {
        self.lens.update(values);
    }
    pub fn class(&self, name: &str) -> Option<MountClass> {
        self.classes
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(MountClass)
    }
    pub fn observe(&mut self, s: MountObservation) {
        self.push(Source::Recorded, s);
    }
    pub fn retire(&mut self, owner: u64, generation: u32) {
        if let Some(life) = self.lives.get(&owner) {
            if life.generation == generation && life.retired
                || life.generation != generation && !newer(generation, life.generation)
            {
                return;
            }
        }
        self.remove_observed(owner);
        // Keep a bounded replay window after despawn. Active owners never age out.
        if self.retired.len() == self.retired.capacity() {
            if let Some((id, old)) = self.retired.pop_front() {
                if self.lives.get(&id)
                    == Some(&Life {
                        generation: old,
                        retired: true,
                    })
                {
                    self.lives.remove(&id);
                }
            }
        }
        self.retired.push_back((owner, generation));
        self.lives.insert(
            owner,
            Life {
                generation,
                retired: true,
            },
        );
    }
    fn remove_observed(&mut self, owner: u64) {
        for source in OBSERVED {
            if let Some(&i) = self.body_index.get(&(source, owner)) {
                self.remove(i);
            }
        }
    }
    pub(super) fn engine(&mut self, body: super::EffectBody, thrusting: bool) -> bool {
        let Some(&class) = self.class_ids.get(&body.class) else {
            return false;
        };
        self.push(Source::Engine, body.sample(class, 0., thrusting))
    }
    pub(super) fn damage(&mut self, body: super::EffectBody, fraction: f64) -> bool {
        self.hull(Source::Hull, body, false, fraction)
    }
    pub(super) fn ship(&mut self, body: super::EffectBody, thrusting: bool, fraction: f64) -> bool {
        self.hull(Source::Recorded, body, thrusting, fraction)
    }
    fn hull(
        &mut self,
        source: Source,
        body: super::EffectBody,
        thrusting: bool,
        fraction: f64,
    ) -> bool {
        if !fraction.is_finite() {
            return false;
        }
        let Some(&class) = self.class_ids.get(&body.class) else {
            return false;
        };
        let health = fraction.clamp(0., 1.) * self.classes[class.0].1.max_health;
        self.push(source, body.sample(class, health, thrusting))
    }
    fn remove(&mut self, i: usize) {
        let body = self.bodies.swap_remove(i);
        self.body_index.remove(&(body.source, body.next.owner));
        if let Some(moved) = self.bodies.get(i) {
            self.body_index.insert((moved.source, moved.next.owner), i);
        }
    }
    fn push(&mut self, source: Source, s: MountObservation) -> bool {
        if s.class.0 >= self.classes.len()
            || !s.tick.is_finite()
            || !s.health.is_finite()
            || !s
                .position
                .iter()
                .chain(&s.rotation)
                .chain(&s.velocity)
                .all(|x| x.is_finite())
        {
            return false;
        }
        let full = self.classes[s.class.0].1.max_health;
        let observed = OBSERVED.contains(&source);
        let next = Life {
            generation: s.generation,
            retired: false,
        };
        if observed {
            if let Some(&current) = self.lives.get(&s.owner) {
                if current != next {
                    if !newer(s.generation, current.generation) {
                        return false;
                    }
                    self.remove_observed(s.owner);
                }
            }
        }
        if let Some(&i) = self.body_index.get(&(source, s.owner)) {
            let body = &mut self.bodies[i];
            body.alive = true;
            if body.next.generation != s.generation && !newer(s.generation, body.next.generation)
                || body.next.generation == s.generation && s.tick < body.next.tick
            {
                return false;
            }
            if body.next.generation != s.generation || body.next.class != s.class {
                body.view = ViewState { inside: false, sockets: 0, rate: None };
                body.prev = s;
                body.next = s;
                body.changed = Body::first_seen(&s, full);
            } else if s.tick >= body.next.tick {
                if body.changed.is_none() && s.health != body.next.health {
                    body.changed = Some(s.tick);
                }
                if s.tick > body.next.tick {
                    body.prev = body.next;
                }
                body.next = s;
            }
        } else if self.bodies.len() < self.bodies.capacity() {
            self.body_index.insert((source, s.owner), self.bodies.len());
            self.bodies.push(Body {
                source,
                prev: s,
                next: s,
                alive: true,
                view: ViewState { inside: false, sockets: 0, rate: None },
                changed: Body::first_seen(&s, full),
            });
        } else {
            self.refused = self.refused.saturating_add(1);
            return false;
        }
        if observed {
            self.lives.insert(s.owner, next);
        }
        true
    }
    pub fn frame(&mut self, receiver: &Receiver, tick: f64, effects: &mut Spawner) {
        let dt = effects.owner.input[bfe_presentation::layout::frame::DT].max(0.);
        self.clock += dt;
        for b in &mut self.bodies {
            b.alive = b.source != Source::Vehicle;
        }
        for row in receiver
            .active()
            .filter(|r| r[r::KIND] == kind::VEHICLE as f64 && r[r::DEAD] == 0.)
        {
            let Some(class) = self
                .vehicles
                .get(row[r::CLASS_ID] as usize)
                .copied()
                .flatten()
            else {
                continue;
            };
            self.push(
                Source::Vehicle,
                MountObservation {
                    class,
                    owner: row[r::ID] as u64,
                    generation: row[r::GENERATION] as u32,
                    tick: row[r::TICK],
                    position: std::array::from_fn(|i| row[r::POSITION + i]),
                    rotation: std::array::from_fn(|i| row[r::QUATERNION + i]),
                    velocity: std::array::from_fn(|i| row[r::VELOCITY + i]),
                    health: row[r::HEALTH],
                    thrusting: (1. ..=3.).contains(&row[r::STATE]),
                },
            );
        }
        for field in receiver
            .publications
            .iter()
            .filter(|f| f.row[r::KIND] == kind::SUMMARY as f64 && f.row[r::SCHEMA] == 7.)
        {
            type State<'a> = (
                usize,
                &'a str,
                Option<f64>,
                serde::de::IgnoredAny,
                serde::de::IgnoredAny,
                serde::de::IgnoredAny,
                serde::de::IgnoredAny,
                serde::de::IgnoredAny,
                serde::de::IgnoredAny,
                serde::de::IgnoredAny,
                serde::de::IgnoredAny,
            );
            let Ok(state) = serde_json::from_slice::<State>(&field.bytes[..field.length]) else {
                continue;
            };
            let Some(mut sample) = self.placed.get(state.0).copied().flatten() else {
                continue;
            };
            let Some(health) = state.2 else {
                continue;
            };
            sample.health = if state.1 == "built" { health } else { 0. };
            sample.tick = field.row[r::TICK];
            self.push(Source::Building, sample);
        }
        for i in (0..self.bodies.len()).rev() {
            if !self.bodies[i].alive {
                self.remove(i);
            }
        }
        for v in self.live.values_mut() {
            v.seen = false;
        }
        self.ranked.clear();
        for (index, body) in self.bodies.iter_mut().enumerate() {
            let (root, velocity, health, thrusting) = body.at(tick);
            let speed = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
            let at = [root.elements[12], root.elements[13], root.elements[14]];
            let (inside, distance) = self.lens.rank(at, body.view.inside);
            body.view.inside = inside;
            let mut row = Ranked::new(index, body.source, &body.next, inside, distance);
            row.previous = body.view.sockets;
            body.view.sockets = 0;
            let class = &self.classes[body.next.class.0].1;
            let moving = body.moving(tick);
            let damaged = body.damaged(tick);
            for m in &class.mounts {
                if (!m.needs_motion || moving) && m.for_source(body.source) && m.group.is_some()
                    && (damaged || !matches!(m.kind, Kind::Damage))
                    && class.scale(m, health, speed, thrusting).is_some() {
                    row.demand += m.demand.0;
                    row.sockets += 1;
                }
            }
            if row.sockets > 0 { self.ranked.push(row); }
        }
        population::distribute(&mut self.ranked, &mut self.scratch, self.budget, MOUNT_LIMIT);
        let blend = -(-dt / 0.18).exp_m1();
        for row in &mut self.ranked {
            let body = &mut self.bodies[row.index];
            body.view.sockets = row.sockets;
            let rate = body.view.rate.get_or_insert(row.rate);
            *rate += (row.rate - *rate) * blend;
            row.rate = *rate;
        }
        // A new fleet or a change of throttle can raise demand immediately.
        let charged: f64 = self.ranked.iter().map(|r| r.rate * r.demand).sum();
        let normalize = (self.budget as f64 / charged).min(1.);
        for row in &mut self.ranked {
            if row.sockets == 0 { continue; }
            row.rate *= normalize;
            let body = &self.bodies[row.index];
            let s = &body.next;
            let class = &self.classes[s.class.0].1;
            let (root, velocity, health, thrusting) = body.at(tick);
            let speed = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
            let mut sockets = row.sockets;
            let moving = body.moving(tick);
            let damaged = body.damaged(tick);
            for (i, m) in class.mounts.iter().enumerate() {
                if sockets == 0 { break; }
                if (m.needs_motion && !moving) || !m.for_source(body.source) { continue; }
                if !damaged && matches!(m.kind, Kind::Damage) { continue; }
                let Some(group) = m.group else { continue };
                let Some(scale) = class.scale(m, health, speed, thrusting) else { continue };
                sockets -= 1;
                let key = (body.source, s.owner, s.generation, s.class, i);
                let mut local = Matrix4::default();
                local.elements = m.local;
                let mut matrix = root;
                matrix.multiply(&local);
                let (mut pos, mut rot, mut scl) = (
                    Vector3::default(),
                    Quaternion::default(),
                    Vector3::default(),
                );
                matrix.decompose(&mut pos, &mut rot, &mut scl);
                let at = [pos.x, pos.y, pos.z];
                let turn = [rot.x, rot.y, rot.z, rot.w];
                let mut current = None;
                if let Some(v) = self.live.get_mut(&key) {
                    if effects.playing(v.handle) {
                        v.seen = true;
                        current = Some(v.handle);
                    } else if !v.plays.may_restart(self.clock, row.rate) {
                        // A burnt-out effect (a spark's few bursts) plays again after a
                        // rest that keeps its mean population at its rate.
                        v.seen = true;
                        continue;
                    }
                }
                let handle = if let Some(handle) = current {
                    handle
                } else {
                    let Some(handle) = effects.start_turned(
                        group,
                        at,
                        velocity,
                        turn,
                        (s.owner as u32).wrapping_add(i as u32),
                    ) else {
                        self.refused = self.refused.saturating_add(1);
                        continue;
                    };
                    self.live.insert(key, Live { handle, seen: true, plays: Plays::started(self.clock) });
                    handle
                };
                effects.mount_rate(handle, row.rate, dt);
                effects.scale(handle, scale, m.inherit_velocity);
                effects.sync(handle, at, Some(turn), Some(velocity));
            }
        }
        self.live
            .retain(|_, v| v.seen || !effects.try_stop(v.handle));
    }
    pub(super) fn active_emitters(&self) -> [usize; 4] {
        let mut counts = [0; 4];
        for &(source, _, _, class, index) in self.live.keys() {
            let mount = &self.classes[class.0].1.mounts[index];
            let category = match mount.kind {
                Kind::Thrust => 0,
                Kind::Contrail => 1,
                Kind::Damage if source == Source::Building => 3,
                Kind::Damage => 2,
            };
            counts[category] += 1;
        }
        counts
    }

    pub fn reset(&mut self, effects: &mut Spawner) {
        for (_, v) in self.live.drain() {
            effects.stop(v.handle);
        }
        self.lives.clear();
        self.retired.clear();
        self.body_index.clear();
        self.bodies.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Mounts {
        let class: Class =
            serde_json::from_str(r#"{"minSpeed":3,"maxSpeed":21,"maxHealth":81,"mounts":[]}"#)
                .unwrap();
        Mounts {
            classes: vec![("loom".into(), class)],
            class_ids: [(7, MountClass(0))].into(),
            vehicles: vec![],
            placed: vec![],
            bodies: Vec::with_capacity(8),
            body_index: HashMap::with_capacity(16),
            lives: HashMap::with_capacity(16),
            retired: VecDeque::with_capacity(8),
            live: HashMap::new(),
            ranked: Vec::with_capacity(8),
            scratch: Vec::with_capacity(8),
            budget: 24,
            lens: Lens::default(),
            clock: 0.,
            refused: 0,
        }
    }
    #[test]
    fn independent_thrust_and_damage_samples_survive_slot_reuse() {
        let mut mounts = fixture();
        let a = super::super::EffectBody {
            class: 7,
            owner: 24,
            generation: 5,
            tick: 7,
            position: [0.; 3],
            rotation: [0., 0., 0., 1.],
            velocity: [0.; 3],
        };
        let b = super::super::EffectBody { owner: 9, ..a };
        assert!(mounts.engine(a, true) && mounts.damage(a, 0.75) && mounts.engine(b, false));
        assert_eq!(
            mounts.bodies[mounts.body_index[&(Source::Hull, a.owner)]]
                .next
                .health,
            60.75
        );
        mounts.retire(a.owner, 4);
        assert_eq!(mounts.bodies.len(), 3);
        mounts.retire(a.owner, 5);
        assert_eq!(mounts.bodies.len(), 1);
        assert_eq!(
            mounts.bodies[mounts.body_index[&(Source::Engine, b.owner)]]
                .next
                .owner,
            9
        );
        assert!(mounts.engine(super::super::EffectBody { generation: 6, ..b }, true));
        assert_eq!(mounts.bodies[0].prev.generation, 6);
        assert!(!mounts.damage(b, f64::NAN));
    }
    #[test]
    fn late_lives_cannot_reposition_or_orphan_the_current_mounts() {
        let mut mounts = fixture();
        for old in [5, u32::MAX] {
            let current = super::super::EffectBody {
                class: 7,
                owner: 24,
                generation: old.wrapping_add(1),
                tick: 100,
                position: [3., 2., 1.],
                rotation: [0., 0., 0., 1.],
                velocity: [0.; 3],
            };
            let late = super::super::EffectBody {
                generation: old,
                tick: 90,
                position: [-900., 0., 0.],
                ..current
            };
            assert!(mounts.engine(late, true));
            assert!(mounts.damage(late, 0.2));
            assert!(mounts.engine(current, true));
            assert!(!mounts.damage(late, 0.2)); // Hull has not published the new life yet.
            assert!(mounts.damage(current, 0.8));
            assert!(!mounts.engine(late, false));
            assert!(!mounts.damage(super::super::EffectBody { tick: 101, ..late }, 0.2));
            let ambiguous = super::super::EffectBody {
                generation: current.generation.wrapping_add(1 << 31),
                ..current
            };
            assert!(!mounts.engine(ambiguous, true));
            mounts.retire(current.owner, old);
            assert_eq!(mounts.bodies.len(), 2);
            assert!(mounts
                .bodies
                .iter()
                .all(|b| b.next.position == current.position));
            assert_eq!(
                mounts.bodies[mounts.body_index[&(Source::Hull, 24)]]
                    .next
                    .health,
                81. * 0.8
            );
            mounts.retire(current.owner, current.generation);
            assert!(mounts.bodies.is_empty());
            assert!(!mounts.engine(current, true));
            assert!(!mounts.damage(late, 0.2));
            mounts = fixture();
        }
    }
    fn band(start: f64, stop: f64) -> Mount {
        serde_json::from_value(serde_json::json!({
            "kind":"damage","effect":"crackle","matrix":vec![0.; 16],
            "StartPercent":start, "StopPercent":stop, "EffectScale":1.3
        }))
        .unwrap()
    }
    #[test]
    fn an_effect_bound_to_a_health_band_is_off_above_it() {
        let class: Class =
            serde_json::from_str(r#"{"minSpeed":0,"maxSpeed":0,"maxHealth":250,"mounts":[]}"#)
                .unwrap();
        let m = band(40., 16.);
        for (health, shown) in [
            (250., false),
            (112.5, false),
            (100.03, false),
            (100., true),
            (70., true),
            (40., true),
            (39.9, false),
            (0., false),
        ] {
            assert_eq!(class.scale(&m, health, 0., false).is_some(), shown, "{health}");
        }
        // Without a stop the effect lasts while any health is left.
        let open = band(30., 0.);
        assert!(class.scale(&open, 0.5, 0., false).is_some());
        assert!(class.scale(&open, 0., 0., false).is_none());
    }
    #[test]
    fn a_band_reaching_full_health_waits_for_the_first_change_of_health() {
        let class: Class = serde_json::from_value(serde_json::json!({
            "minSpeed":0,"maxSpeed":0,"maxHealth":90,
            "mounts":[{"kind":"damage","effect":"crackle","matrix":vec![0.; 16],
                "StartPercent":100,"StopPercent":70,"EffectScale":1.3}]
        }))
        .unwrap();
        let mut mounts = fixture();
        mounts.classes[0].1 = class;
        let at = |tick, generation| super::super::EffectBody {
            class: 7,
            owner: 3,
            generation,
            tick,
            position: [0.; 3],
            rotation: [0., 0., 0., 1.],
            velocity: [0.; 3],
        };
        let shown = |mounts: &Mounts, tick: f64| {
            let body = &mounts.bodies[mounts.body_index[&(Source::Hull, 3)]];
            let class = &mounts.classes[0].1;
            body.damaged(tick) && class.scale(&class.mounts[0], body.at(tick).2, 0., false).is_some()
        };
        assert!(mounts.damage(at(10, 1), 1.));
        assert!(mounts.damage(at(11, 1), 1.));
        // The band covers full health, but nothing has changed yet.
        assert!(!shown(&mounts, 11.));
        assert!(mounts.damage(at(20, 1), 0.8));
        assert!(!shown(&mounts, 19.5));
        assert!(shown(&mounts, 20.));
        assert!(mounts.damage(at(30, 1), 0.5));
        assert!(!shown(&mounts, 30.));
        // Mended back to full: the game looks again and the band holds.
        assert!(mounts.damage(at(40, 1), 1.));
        assert!(shown(&mounts, 40.));
        // A new life starts undamaged, and one first seen damaged shows at once.
        assert!(mounts.damage(at(50, 2), 1.));
        assert!(!shown(&mounts, 50.));
        assert!(mounts.damage(at(60, 3), 0.9));
        assert!(shown(&mounts, 60.));
    }
    #[test]
    fn a_guided_remotes_flame_and_contrail_scale_with_its_airspeed() {
        let rocket = RemoteEngine::read(&bfe_content::json::parse(
            r#"{"MinSpeed": "20", "MaxSpeed": "26", "BoostSpeed": "60", "ThrustEffectMinScale": "0.3",
                "ThrustEffectMaxScale": "0.9", "ContrailEffectMinScale": "0.0", "ContrailEffectMaxScale": "0.4"}"#,
        ).unwrap());
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-9);
        let (flame, trail) = rocket.scales(20.);
        assert!(close(flame, 0.3), "the flame's smallest at its slowest");
        assert_eq!(trail, None, "no contrail at its slowest");
        let (flame, trail) = rocket.scales(40.);
        // The flame ramps from MinSpeed; the contrail follows the game's rule (only past MaxSpeed
        // and a tenth of the way on to BoostSpeed, here 29.4, whole at BoostSpeed).
        assert!(close(flame, 0.6) && close(trail, 0.4 * (40. - 29.4) / (60. - 29.4)), "half way to its boost speed");
        let (flame, trail) = rocket.scales(60.);
        assert!(close(flame, 0.9) && close(trail, 0.4), "the largest at its boost speed");
    }

    #[test]
    fn contrails_follow_speed_while_a_ship_coasts() {
        let class: Class =
            serde_json::from_str(r#"{"minSpeed":1,"maxSpeed":11,"maxHealth":9,"mounts":[]}"#)
                .unwrap();
        let m: Mount = serde_json::from_value(serde_json::json!({
            "kind":"contrail","effect":"ribbon","matrix":vec![0.; 16],
            "EffectMinSpeed":4., "EffectMinScale":0.2, "EffectMaxScale":1.2
        }))
        .unwrap();
        // No boost: the contrail starts at MaxSpeed less a tenth of itself
        // (9.9) and is whole at one and a half times that.
        assert_eq!(class.scale(&m, 8., 3., false), None);
        assert_eq!(class.scale(&m, 8., 9.8, false), None);
        assert!((class.scale(&m, 8., 12.375, false).unwrap() - 0.7).abs() < 1e-9);
        assert!((class.scale(&m, 8., 20., false).unwrap() - 1.2).abs() < 1e-9);
        // A contrail whose ContrailEffectMinSpeed is under 0.5 is never drawn.
        let off: Mount = serde_json::from_value(serde_json::json!({
            "kind":"contrail","effect":"ribbon","matrix":vec![0.; 16],
            "EffectMinSpeed":0., "EffectMinScale":1.2, "EffectMaxScale":1.8
        }))
        .unwrap();
        assert_eq!(class.scale(&off, 8., 20., false), None);
    }
    #[test]
    fn thrust_and_contrail_scale_across_boost_speed_without_early_saturation() {
        let class: Class = serde_json::from_str(
            r#"{"minSpeed":20,"maxSpeed":80,"boostSpeed":140,"maxHealth":50,"mounts":[]}"#,
        )
        .unwrap();
        let thrust: Mount = serde_json::from_value(serde_json::json!({
            "kind":"thrust","effect":"plume","matrix":vec![0.; 16],
            "EffectMinScale":0.4, "EffectMaxScale":1.6, "EffectScaleStart":0.5
        }))
        .unwrap();
        let contrail: Mount = serde_json::from_value(serde_json::json!({
            "kind":"contrail","effect":"ribbon","matrix":vec![0.; 16],
            "EffectMinScale":0.2, "EffectMaxScale":1.4
        }))
        .unwrap();
        // Scale start = 20 + (80 - 20) * 0.5 = 50, from MaxSpeed; the ramp runs
        // on to BoostSpeed (140) and is not clamped above it.
        assert!((class.scale(&thrust, 50., 40., true).unwrap() - 0.4).abs() < 1e-9);
        assert!((class.scale(&thrust, 50., 50., true).unwrap() - 0.4).abs() < 1e-9);
        assert!((class.scale(&thrust, 50., 80., true).unwrap() - 0.8).abs() < 1e-9);
        assert!((class.scale(&thrust, 50., 110., true).unwrap() - 1.2).abs() < 1e-9);
        assert!((class.scale(&thrust, 50., 140., true).unwrap() - 1.6).abs() < 1e-9);
        assert!((class.scale(&thrust, 50., 170., true).unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(class.scale(&thrust, 50., 110., false), None);
        // The contrail (no ContrailEffectMinSpeed: the class default 1.1) shows
        // only past MaxSpeed and a tenth of the way to BoostSpeed (86), growing
        // to its full scale at BoostSpeed.
        assert_eq!(class.scale(&contrail, 50., 80., true), None);
        assert_eq!(class.scale(&contrail, 50., 85., true), None);
        assert!((class.scale(&contrail, 50., 113., true).unwrap() - 0.8).abs() < 1e-9);
        assert!((class.scale(&contrail, 50., 140., true).unwrap() - 1.4).abs() < 1e-9);
    }
    #[test]
    fn samples_follow_fractional_display_time_and_gate_damage_at_the_observed_tick() {
        let a = MountObservation {
            owner: 7,
            generation: 4,
            class: MountClass(0),
            tick: 17.,
            position: [4., 0., 0.],
            velocity: [1., 2., 3.],
            rotation: [0., 0., 0., 1.],
            health: 32.,
            thrusting: true,
        };
        let b = MountObservation {
            tick: 19.,
            position: [12., 0., 0.],
            health: 13.,
            ..a
        };
        let body = Body {
            source: Source::Recorded,
            prev: a,
            next: b,
            alive: true,
            view: ViewState { inside: false, sockets: 0, rate: None },
            changed: None,
        };
        assert_eq!(body.at(17.75).0.elements[12], 7.);
        assert_eq!(body.at(18.99).2, 32.);
        assert_eq!(body.at(19.).2, 13.);
        assert!(body.moving(17.75));
        for tick in [19., 19.42, 20.83, 25., 34.] {
            assert!(body.moving(tick));
        }
        assert!(!body.moving(34.1));
        let mut parked = Body { prev: body.next, ..body };
        parked.next.tick += 1.;
        parked.next.velocity = [12., 0., 0.];
        parked.next.rotation = parked.next.rotation.map(|n| -n);
        assert!(!parked.moving(parked.next.tick));
    }
}
