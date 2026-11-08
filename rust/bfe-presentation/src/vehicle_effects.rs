//! VehicleEffects' displayed thrust, damage, spray and destruction phase.
//! Stable body ids retain lifecycle clocks. Effect handles and debris meshes
//! remain host resources; generated records tell the host exactly what to do.
//!
//! What a hull shows beyond its model, at Classic (docs/vehicles.md,
//! "Effects and sounds"), from its class's fx (tools/zero_vehicle.py
//! vehicle_fx):
//!
//! - Thrust: each ThrustEffect burns at its attach point and offset from the
//!   moment a flyer starts to take off until it has landed. Its particles
//!   are drawn at a scale that rests at ThrustEffectMinScale and, once the
//!   hull's speed passes the point ThrustEffectScaleStart of the way from
//!   MinSpeed to MaxSpeed, grows in step with it, reaching
//!   ThrustEffectMaxScale at MaxSpeed. The original game doesn't stop it there,
//!   but its flyers never go much faster; the port's boost does, so here it
//!   stops at ThrustEffectMaxScale.
//! - Damage: each damage effect burns at its attach point while the hull's
//!   health is at most DamageStartPercent and above DamageStopPercent of its
//!   MaxHealth, at DamageEffectScale, its particles carrying
//!   DamageInheritVelocity of the hull's velocity.
//! - Water: a hover's WaterEffect, on the water's surface under it while it
//!   is within WATER_HEIGHT of it.
//! - Stomp: a walker's StompEffect where a foot comes down (the native foot phase),
//!   at the foot level with the walker's root, turned as the foot is.
//! - Death: the hull's effects stop and its chunks fly off (Debris), with
//!   their trails and each one's ChunkSmokeEffect. A walker with a death
//!   animation is dying first (Vehicle.dying, the simulation's): every
//!   DeathAnimationExplosionTime one of its DeathAnimationExplosion effects
//!   (picked at random) goes off at a node of its model picked at random,
//!   its DeathDustEffect rises at DeathDustOffset DeathDustDelay after it
//!   was destroyed if it is still dying then (the original game runs the
//!   dust's clock only while the animation plays), and its chunks fly when
//!   it is gone, with its FinalExplosion (the simulation's blast).
//!
//! Placed turrets and destructible posts (src/props.js) go through the same
//! rules (updateBuildings): their damage effects while built, at the built
//! model's nodes, a post's death animation as a walker's (with its
//! DeathEffects at their hardpoints as it ends), and their chunks when
//! destroyed (tools/extract.py write_props
//! gives them a hull's fx; the building reader takes those keys as a hull's).
//!
//! It only draws: it reads what every host gives a hull (alive, dying,
//! health, flight state, velocity and the pose drawn this frame) or a
//! building (its state and health) and runs on the page, so a battle
//! simulated here, in a worker or on a server looks the same. Missing
//! attach points fall back to the hull's root, as the original game's do.
//!
//! Where a dead walker's pieces start. They are modelled in its standing
//! pose, in the hull's frame; its death animation has since brought its body
//! down (the AT-TE's by 2.5 m, onto its belly). Carried by its body bone
//! (bone_root) from where that pose has it to where the animation left it,
//! they start where the body lies instead of jumping back up to where it
//! stood; a piece that would start below the ground starts on it (Debris,
//! floor). Null for a hull without that bone (the root carries its pieces).
//!
//! The displayed body order is supplied separately from stable body ids.
//! A hull may leave a frame without moving any other hull's state. Returning
//! records by id avoids associating a building's fire with the next hull.
//! Admission binds each authored attach name to a numeric node. An absent
//! attach point binds the root, including chunks that name the whole model.
//!
//! Persistent rows precede one-shot rows. Their action says stop, create or
//! move; the host keeps only the corresponding browser resource handle.
//! A stop bit can accompany a create action when the body starts a new life.
//! That ordering prevents an old plume from surviving a rebuilt emplacement.
//! Handle presence and completion are observations, not native effect clocks.
//! The separate particle owner still decides when an existing plume is done.
//!
//! Health bands deliberately use an exclusive lower and inclusive upper end.
//! Exactly zero health cannot keep a damage plume alive. Thrust has a separate
//! flight-state gate, so boost speed changes its scale without reviving it on
//! a landing pad. Surface spray uses the root's drawn local position, as the
//! original reader did, and never substitutes a sampled bone's height.
//!
//! Destruction must preserve resource event order as well as event contents.
//! An ordinary wreck throws its chunks before immediate death dust is played.
//! A completed death animation plays its final hardpoint effects first.
//! Dust during a dying clip precedes that frame's random explosion picks.
//! Resource playback can consume an effect seed from the same cosmetic stream.
//! For that reason a random request suspends this frame after its ready prefix.
//! The host applies that prefix, supplies two raw samples, and resumes the same
//! native body cursor. It never computes an explosion index or a node index.
//!
//! Only admission grows banks. Full body, node or output stores refuse a new
//! resource before modifying any bank. Frame counts and body ids are bounded
//! again when consumed, so excess facts cannot index another body or panic.
//! The bind carrier is a cold inverse chain followed by a current world matrix.
//! A missing or incomplete chain leaves chunks in the root frame.

//!
//! The random request is also a matrix request boundary. Persistent hardpoints
//! are retained numeric resources; only the selected random bone needs a fresh
//! world observation for that explosion. The host copies that one matrix after
//! selection, then completes the existing frame. It never walks or publishes the
//! complete death skeleton merely to choose one node. Clip-end carrier matrices
//! still use the bound body bone, independently of the random explosion node.

use crate::{
    unit_effects::{put, vector},
    visual_effects_layout::{
        vehicle_fx_action as a, vehicle_fx_flight as flight, vehicle_fx_frame as fr,
        vehicle_fx_input as i, vehicle_fx_inspect as t, vehicle_fx_node as n,
        vehicle_fx_query as query, vehicle_fx_record as o, vehicle_fx_status as s,
        visual_limits as limits,
    },
};
use serde::Deserialize;
use three::{Matrix4, Quaternion, Vector3};
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    effect: usize,
    node: usize,
    offset: Option<[f64; 3]>,
    min_scale: f64,
    max_scale: f64,
    scale_start: f64,
    start: f64,
    stop: f64,
    scale: f64,
    inherit: f64,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Dust {
    entry: Entry,
    delay: f64,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Dying {
    span: f64,
    interval: Option<f64>,
    effects: Vec<usize>,
    r#final: Vec<Entry>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Descriptor {
    thrust: Vec<Entry>,
    damage: Vec<Entry>,
    water: Option<usize>,
    dust: Option<Dust>,
    dying: Option<Dying>,
    speed: Option<[f64; 2]>,
    nodes: usize,
    bones: Vec<usize>,
    body: Option<usize>,
    bind: Vec<Bind>,
}
#[derive(Deserialize)]
pub struct Bind {
    position: [f64; 3],
    quaternion: [f64; 4],
    scale: [f64; 3],
}
struct Body {
    def: Descriptor,
    node_start: usize,
    output_start: usize,
    life: Option<f64>,
    alive: bool,
    dying: bool,
    dust_done: bool,
    fired: f64,
    dust_left: Option<f64>,
    bind_inverse: Option<Matrix4>,
}
pub struct Vehicles {
    pub frame: Vec<f64>,
    pub order: Vec<u32>,
    cursor: usize,
    pending: Option<usize>,
    selection: Option<Entry>,
    pub input: Vec<f64>,
    pub nodes: Vec<f64>,
    pub output: Vec<f64>,
    pub status: Vec<f64>,
    pub inspection: Vec<f64>,
    bodies: Vec<Body>,
}
impl Default for Vehicles {
    fn default() -> Self {
        Self {
            frame: vec![0.; fr::STRIDE],
            order: vec![0; limits::VEHICLES],
            cursor: 0,
            pending: None,
            selection: None,
            input: Vec::new(),
            nodes: Vec::new(),
            output: Vec::new(),
            status: Vec::new(),
            inspection: vec![0.; t::STRIDE],
            bodies: Vec::with_capacity(limits::VEHICLES),
        }
    }
}
fn thrust(entry: &Entry, speed: f64, speeds: [f64; 2]) -> f64 {
    let min = speeds[0];
    let top = if speeds[1] == 0. { 1. } else { speeds[1] };
    let start = (top - min) * entry.scale_start + min;
    let span = top - start;
    let f = if span > 1e-6 {
        ((speed - start) / span).clamp(0., 1.)
    } else if speed > start {
        1.
    } else {
        0.
    };
    entry.min_scale + (entry.max_scale - entry.min_scale) * f
}
fn burns(entry: &Entry, health: f64, max: f64) -> bool {
    health > entry.stop * max && health <= entry.start * max
}
impl Vehicles {
    // Admission sizes every bank before the displayed phase. Oversized content
    // refuses a resource, rather than panicking or growing a live output queue.
    pub fn add(&mut self, def: Descriptor) -> usize {
        let entries = def.thrust.len()
            + def.damage.len()
            + 1
            + 2
            + def.dying.as_ref().map_or(0, |d| d.r#final.len());
        if self.bodies.len() >= limits::VEHICLES
            || def.nodes == 0
            || self.nodes.len() / n::STRIDE + def.nodes > limits::VEHICLE_NODES
            || self.output.len() / o::STRIDE + entries > limits::VEHICLE_ENTRIES
        {
            return usize::MAX;
        }
        let mut bind = Matrix4::default();
        for b in &def.bind {
            let mut local = Matrix4::default();
            local.compose(
                &Vector3::new(b.position[0], b.position[1], b.position[2]),
                &Quaternion::new(
                    b.quaternion[0],
                    b.quaternion[1],
                    b.quaternion[2],
                    b.quaternion[3],
                ),
                &Vector3::new(b.scale[0], b.scale[1], b.scale[2]),
            );
            bind.premultiply(&local);
        }
        let bind_inverse = def.body.filter(|body| *body < def.nodes).map(|_| {
            bind.invert();
            bind
        });
        let id = self.bodies.len();
        let node_start = self.nodes.len() / n::STRIDE;
        let output_start = self.output.len() / o::STRIDE;
        self.nodes
            .resize(self.nodes.len() + def.nodes * n::STRIDE, 0.);
        self.input.resize((id + 1) * i::STRIDE, 0.);
        self.output
            .resize(self.output.len() + entries * o::STRIDE, 0.);
        self.status
            .resize(self.status.len() + entries * s::STRIDE, 0.);
        self.bodies.push(Body {
            def,
            node_start,
            output_start,
            life: None,
            alive: false,
            dying: false,
            dust_done: false,
            fired: 0.,
            dust_left: None,
            bind_inverse,
        });
        id
    }
    pub fn node_start(&self, id: usize) -> usize {
        self.bodies.get(id).map_or(usize::MAX, |b| b.node_start)
    }
    pub fn output_start(&self, id: usize) -> usize {
        self.bodies.get(id).map_or(usize::MAX, |b| b.output_start)
    }
    // bodyCarrier composes current bone world with inverse authored chain.
    // The chain is resolved during admission; no frame walks bone identities.
    pub fn carrier(&mut self, id: usize) {
        let Some(b) = self.bodies.get(id) else {
            return;
        };
        let x = &mut self.input[id * i::STRIDE..][..i::STRIDE];
        x[i::CARRIER_VALID] = 0.;
        if let (Some(inverse), Some(node)) = (b.bind_inverse, b.def.body) {
            if node >= b.def.nodes {
                return;
            }
            let at = (b.node_start + node) * n::STRIDE;
            let mut world = Matrix4::default();
            world.elements.copy_from_slice(&self.nodes[at..at + 16]);
            world.multiply(&inverse);
            x[i::CARRIER..i::CARRIER + 16].copy_from_slice(&world.elements);
            x[i::CARRIER_VALID] = 1.;
        }
    }
    pub fn inspect(&mut self) {
        let x = &mut self.inspection;
        let entry = Entry {
            min_scale: x[t::MIN_SCALE],
            max_scale: x[t::MAX_SCALE],
            scale_start: x[t::SCALE_START],
            start: x[t::START],
            stop: x[t::STOP],
            ..Default::default()
        };
        x[t::SCALE] = thrust(&entry, x[t::SPEED], [x[t::MIN_SPEED], x[t::MAX_SPEED]]);
        x[t::BURNS] = burns(&entry, x[t::HEALTH], x[t::MAX_HEALTH]) as u8 as f64;
    }
    // VehicleEffects.update is a continuation over one ordered body block.
    // Cosmetic picks occur after earlier resource events have been applied:
    // dust, debris and effect seeds share the page's random service.
    pub fn frame(&mut self) {
        self.output.fill(0.);
        self.cursor = 0;
        self.pending = None;
        self.selection = None;
        self.advance();
    }
    pub fn resume(&mut self) {
        if let Some(id) = self.pending {
            if let Some(body) = self.bodies.get(id) {
                if let Some(dying) = &body.def.dying {
                    if !dying.effects.is_empty() {
                        let pick = (self.frame[fr::RANDOM] * dying.effects.len() as f64).floor()
                            as usize
                            % dying.effects.len();
                        let count = body.def.bones.len().max(1);
                        let bone =
                            (self.frame[fr::RANDOM + 1] * count as f64).floor() as usize % count;
                        let node = body
                            .def
                            .bones
                            .get(bone)
                            .copied()
                            .filter(|node| *node < body.def.nodes)
                            .unwrap_or(0);
                        self.selection = Some(Entry {
                            effect: dying.effects[pick],
                            node,
                            ..Default::default()
                        });
                        self.frame[fr::NODE] = node as f64;
                        self.frame[fr::PENDING] = query::MATRIX as f64;
                        return;
                    }
                }
            }
        }
        self.complete();
    }
    // Only the chosen bone needs a current matrix. Query completion leaves
    // unrelated bind nodes untouched, even on a large animated walker.
    pub fn complete(&mut self) {
        if let Some(id) = self.pending.take() {
            if let (Some(body), Some(entry)) = (self.bodies.get(id), self.selection.take()) {
                let persistent = body.def.thrust.len() + body.def.damage.len() + 1;
                let at = (body.output_start + persistent + 1) * o::STRIDE;
                event(
                    &mut self.output[at..at + o::STRIDE],
                    &entry,
                    body.node_start,
                    body.def.nodes,
                    &self.nodes,
                    false,
                );
            }
            self.cursor += 1;
        }
        self.advance();
    }
    fn advance(&mut self) {
        self.frame[fr::PENDING] = 0.;
        let count = (self.frame[fr::COUNT] as usize).min(self.order.len());
        for order_index in self.cursor..count {
            self.cursor = order_index + 1;
            let id = self.order[order_index] as usize;
            if id >= self.bodies.len() {
                continue;
            }
            self.carrier(id);
            let x = &mut self.input[id * i::STRIDE..][..i::STRIDE];
            x[i::BLOW_APART] = 0.;
            x[i::FALLEN] = 0.;
            if x[i::PRESENT] == 0. {
                continue;
            }
            let b = &mut self.bodies[id];
            let def = &b.def;
            let persistent = def.thrust.len() + def.damage.len() + 1;
            let alive = x[i::ALIVE] != 0.;
            let reset =
                b.life != Some(x[i::GENERATION]) || (x[i::BUILDING] != 0. && alive && !b.alive);
            if reset {
                b.life = Some(x[i::GENERATION]);
                b.alive = alive;
                b.dying = false;
                b.dust_done = false;
                b.fired = 0.;
                b.dust_left = None;
            }
            let dead_now = !alive && b.alive;
            if dead_now {
                b.dying = x[i::DYING] > 0. && def.dying.is_some();
                b.dust_done = def.dust.is_none();
                b.fired = 0.;
                b.dust_left = if !b.dying {
                    def.dust.as_ref().map(|d| d.delay)
                } else {
                    None
                };
                if !b.dying {
                    x[i::BLOW_APART] = 1.;
                }
            }
            b.alive = alive;
            // Persistent rows preserve source ordering: thrust, then damage, then
            // spray. Stop and play can coexist when an effect gets a new life.
            let speed = vector(x, i::VELOCITY).length();
            for entry_index in 0..persistent {
                let at = (b.output_start + entry_index) * o::STRIDE;
                let row = &mut self.output[at..at + o::STRIDE];
                let status_at = (b.output_start + entry_index) * s::STRIDE;
                row[o::STOP] = (reset || dead_now) as u8 as f64;
                let (entry, on, scale, inherit, normal) = if entry_index < def.thrust.len() {
                    let e = &def.thrust[entry_index];
                    (
                        Some(e),
                        alive
                            && x[i::FLYING] != 0.
                            && [
                                flight::TAKING_OFF as f64,
                                flight::FLYING as f64,
                                flight::LANDING as f64,
                            ]
                            .contains(&x[i::FLIGHT_STATE]),
                        thrust(e, speed, def.speed.unwrap_or([10., 30.])),
                        0.,
                        false,
                    )
                } else if entry_index < def.thrust.len() + def.damage.len() {
                    let e = &def.damage[entry_index - def.thrust.len()];
                    (
                        Some(e),
                        alive && burns(e, x[i::HEALTH], x[i::MAX_HEALTH]),
                        e.scale,
                        e.inherit,
                        false,
                    )
                } else {
                    (
                        None,
                        alive
                            && def.water.is_some()
                            && x[i::WATER_LEVEL].is_finite()
                            && x[i::WATER_OVER] != 0.
                            && x[i::POSITION + 1] - x[i::WATER_LEVEL] < 4.
                            && x[i::POSITION + 1] > x[i::WATER_LEVEL] - 4.,
                        1.,
                        0.,
                        true,
                    )
                };
                if !on {
                    row[o::ACTION] = a::STOP as f64;
                    continue;
                }
                row[o::ACTION] = if reset
                    || self.status[status_at + s::PRESENT] == 0.
                    || self.status[status_at + s::DONE] != 0.
                {
                    a::PLAY
                } else {
                    a::MOVE
                } as f64;
                row[o::SCALE] = scale;
                row[o::INHERIT] = inherit;
                row[o::NORMAL] = normal as u8 as f64;
                put(row, o::VELOCITY, vector(x, i::VELOCITY));
                if let Some(e) = entry {
                    place(row, e, b.node_start, def.nodes, &self.nodes);
                    row[o::EFFECT] = e.effect as f64;
                } else {
                    row[o::EFFECT] = def.water.unwrap_or(0) as f64;
                    put(
                        row,
                        o::POSITION,
                        Vector3::new(x[i::POSITION], x[i::WATER_LEVEL], x[i::POSITION + 2]),
                    );
                }
            }
            if alive {
                continue;
            }
            let dust_row = (b.output_start + persistent) * o::STRIDE;
            let explosion_row = dust_row + o::STRIDE;
            if b.dying {
                let dying = def.dying.as_ref().unwrap();
                if x[i::DYING] <= 0. {
                    b.dying = false;
                    x[i::BLOW_APART] = 1.;
                    x[i::FALLEN] = 1.;
                    for (k, e) in dying.r#final.iter().enumerate() {
                        let at = explosion_row + (k + 1) * o::STRIDE;
                        event(
                            &mut self.output[at..at + o::STRIDE],
                            e,
                            b.node_start,
                            def.nodes,
                            &self.nodes,
                            false,
                        );
                    }
                } else {
                    let elapsed = (dying.span - x[i::DYING]).max(0.);
                    if !b.dust_done {
                        if let Some(dust) = &def.dust {
                            if elapsed > dust.delay {
                                b.dust_done = true;
                                event(
                                    &mut self.output[dust_row..dust_row + o::STRIDE],
                                    &dust.entry,
                                    b.node_start,
                                    def.nodes,
                                    &self.nodes,
                                    true,
                                );
                            }
                        }
                    }
                    let due = (elapsed / dying.interval.unwrap_or(0.4).max(1e-3)).floor();
                    if !dying.effects.is_empty() && due > b.fired {
                        b.fired = due;
                        // Publish the prefix before asking for either pick.
                        // The host answers raw random values, never indices.
                        self.pending = Some(id);
                        self.cursor = order_index;
                        self.frame[fr::PENDING] = query::RANDOM as f64;
                        self.frame[fr::CURSOR] = order_index as f64;
                        return;
                    }
                }
            }
            if let Some(left) = &mut b.dust_left {
                if x[i::DT] > 0. {
                    *left -= x[i::DT];
                    if *left <= 0. {
                        if let Some(dust) = &def.dust {
                            event(
                                &mut self.output[dust_row..dust_row + o::STRIDE],
                                &dust.entry,
                                b.node_start,
                                def.nodes,
                                &self.nodes,
                                true,
                            );
                        }
                        b.dust_left = None;
                    }
                }
            }
        }
        self.frame[fr::CURSOR] = count as f64;
    }
}
// VehicleEffects.place: matrix times local offset, then the exact Three
// decomposition. Hosts publish final numbers without recomposing a transform.
fn place(row: &mut [f64], entry: &Entry, start: usize, count: usize, nodes: &[f64]) {
    let node = if entry.node < count { entry.node } else { 0 };
    let at = (start + node) * n::STRIDE;
    let mut matrix = Matrix4::default();
    matrix.elements.copy_from_slice(&nodes[at..at + 16]);
    if let Some(offset) = entry.offset {
        let mut translation = Matrix4::default();
        translation.make_translation(offset[0], offset[1], offset[2]);
        matrix.multiply(&translation);
    }
    let mut p = Vector3::default();
    let mut q = Quaternion::default();
    let mut scale = Vector3::default();
    matrix.decompose(&mut p, &mut q, &mut scale);
    put(row, o::POSITION, p);
    row[o::QUATERNION..o::QUATERNION + 4].copy_from_slice(&[q.x, q.y, q.z, q.w]);
}
fn event(row: &mut [f64], entry: &Entry, start: usize, count: usize, nodes: &[f64], normal: bool) {
    place(row, entry, start, count, nodes);
    row[o::ACTION] = a::PLAY as f64;
    row[o::EFFECT] = entry.effect as f64;
    row[o::NORMAL] = normal as u8 as f64;
    row[o::SCALE] = 1.;
}
