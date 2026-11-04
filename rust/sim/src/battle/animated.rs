//! Animated prop clocks, doors, triggers and creature holds (src/props.js).
//! Prop poses and grab-node transforms are native. Soldier rider sampling,
//! deaths and removal remain calls into their owning systems.
use crate::pose::{Clip, Pose};
use crate::seats::turrets::TurretRig;
use crate::store::StrId;
use three::{Matrix4, Quaternion, Vector3};
use super::props::{max, min};

const TICK: f64 = 1.0 / 60.0;
#[derive(Clone, Debug)]
pub struct Trigger { pub slot: i32, pub point: Vector3, pub radius: f64, pub sound: Option<StrId>, pub opens: bool }
#[derive(Clone, Debug)]
pub struct Grab { pub slot: i32, pub window: f64, pub radius: f64, pub node: Option<usize>, pub clip: StrId }
pub struct Settings {
    pub door: bool,
    pub clips: Vec<Option<Clip>>,
    pub door_clip: Option<Clip>,
    pub door_duration: f64,
    pub idle_delay: f64,
    pub open_slots: Vec<bool>,
    pub triggers: Vec<Trigger>,
    pub grabs: Vec<Grab>,
    pub position: Vector3,
    pub kill_sound: Option<StrId>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Victim { pub unit: usize, pub generation: f64, pub grab: usize, pub since: f64 }
#[derive(Clone, Debug, PartialEq)]
pub struct Held { pub prop: usize, pub grab: usize, pub since: f64, pub frame: Option<Matrix4>, pub time: Option<f64> }
#[derive(Clone, Debug)]
pub struct Body {
    /// Its identity in the caller's body table, separate from a public id.
    pub key: usize,
    pub id: f64,
    pub generation: f64,
    pub soldier: bool,
    pub walker: bool,
    pub removed: bool,
    pub alive: bool,
    pub dead: bool,
    pub seated: bool,
    pub held: Option<Held>,
    pub combat: bool,
    pub mounted: bool,
    pub position: Vector3,
    pub previous: Option<Vector3>,
    pub settled: Option<Vector3>,
    pub velocity: Vector3,
    pub air: bool,
    pub dive: f64,
    pub tick_count: Option<f64>,
    pub let_go: bool,
    pub remove: bool,
    pub rider_clips: Vec<bool>,
}
impl Body {
    fn on_foot(&self) -> bool { self.soldier && !self.walker && !self.removed && (self.alive || self.dead) && !self.seated && self.held.is_none() }
}
pub struct Animated {
    pub index: usize,
    pub slot: i32,
    pub time: f64,
    pub weight: f64,
    pub looping: bool,
    pub hold: f64,
    pub started: f64,
    pub start_time: f64,
    pub victim: Option<Victim>,
    pub openness: f64,
    pub body_enabled: Option<bool>,
    pub stand_q: Vec<f32>,
    pub stand_t: Vec<f32>,
    pub pose: Option<Pose>,
    pub rig: TurretRig,
    pub bind: Vec<(Quaternion, Vector3)>,
}
pub trait World {
    fn sound(&mut self, name: StrId, position: Vector3);
    fn let_go(&mut self, prop: &Animated, unit: &mut Body);
    fn damage(&mut self, prop: &Animated, unit: &mut Body, amount: f64);
    fn remove(&mut self, prop: &Animated, unit: &mut Body);
    /// The soldier's root from its rider clip, before the prop places it.
    fn rider_root(&mut self, unit: &Body, clip: StrId, time: f64) -> Option<(Vector3, Quaternion)>;
}
fn length(clip: Option<&Clip>) -> f64 { clip.map_or(0.0, |c| c.frames as f64 / c.fps) }
fn within(point: Vector3, reach: f64, feet: Vector3) -> bool {
    let r = reach + 1.278;
    (feet.x - point.x).abs() <= r && (feet.y + 1.048 - point.y).abs() <= r && (feet.z - point.z).abs() <= r
}
impl Animated {
    pub fn near(&self, settings: &Settings, positions: &[Vector3]) -> bool {
        settings.triggers.first().is_some_and(|t| positions.iter().any(|p| p.distance_to_squared(&t.point) <= t.radius * t.radius))
    }
    pub fn play(&mut self, slot: i32, looping: bool, tick: f64, settings: &Settings) {
        if slot == self.slot { return; }
        self.slot = slot; self.looping = looping; self.started = tick; self.start_time = self.time;
        if self.body_enabled.is_some() && !settings.door && settings.triggers.iter().any(|t| t.opens) {
            self.body_enabled = Some(!(!looping && settings.open_slots.get(slot as usize) == Some(&true)));
        }
    }
    pub fn step(&mut self, dt: f64, positions: &[Vector3], bodies: &mut [Body], order: &[usize], tick: f64, settings: &Settings, world: &mut impl World) {
        if self.pose.is_none() { return; }
        if settings.door {
            let near = self.near(settings, positions);
            let span = if settings.door_clip.is_some() { max(settings.door_duration, 1e-3) } else { 1.0 };
            self.openness = max(0.0, min(1.0, self.openness + (if near { dt } else { -dt }) / span));
            let pose = self.pose.as_mut().unwrap(); pose.clear();
            if let Some(clip) = &settings.door_clip { pose.add(clip, self.openness * settings.door_duration, 1.0, false); }
            if self.body_enabled.is_some() { self.body_enabled = Some(self.openness < 0.5); }
            for (i, &(q, p)) in self.bind.iter().enumerate() {
                let node = self.rig.tree.node_mut(self.rig.bones[i]); node.copy_quaternion(&q); node.position = p;
            }
            pose.apply_out();
            for i in 0..pose.w.len() {
                if pose.w[i] as f64 <= 1e-6 { continue; }
                let out = &pose.out[i * 7..]; let node = self.rig.tree.node_mut(self.rig.bones[i]);
                node.set_quaternion(out[0], out[1], out[2], out[3]); node.position.set(out[4], out[5], out[6]);
            }
            return;
        }
        if self.slot < 0 { self.play(0, true, tick, settings); }
        if !self.looping {
            if self.time > length(settings.clips.get(self.slot as usize).and_then(Option::as_ref)) {
                self.time = 0.0; self.weight = 0.0; self.swallow(bodies, settings, world); self.play(0, true, tick, settings);
            }
        } else if self.victim.is_some() { self.swallow(bodies, settings, world); }
        if self.looping && self.victim.is_none() && self.weight >= 0.5 {
            for trigger in &settings.triggers {
                if trigger.slot < 0 || trigger.slot == self.slot { continue; }
                if !order.iter().any(|&i| bodies[i].on_foot() && within(trigger.point, trigger.radius, bodies[i].position)) { continue; }
                self.time = 0.0; self.weight = 0.0; self.play(trigger.slot, false, tick, settings);
                if let Some(sound) = trigger.sound { world.sound(sound, trigger.point); }
                break;
            }
        }
        if self.victim.is_none() {
            for (g, grab) in settings.grabs.iter().enumerate() {
                if grab.slot != self.slot || self.time > grab.window || grab.node.is_none() { continue; }
                self.rig.tree.update_matrix_world(self.rig.root, true);
                let mut point = Vector3::default();
                self.rig.tree.get_world_position(self.rig.bones[grab.node.unwrap()], &mut point);
                // Array.find stops at the first body, even a dead body;
                // the alive check happens after that selection.
                let unit = order.iter().copied().find(|&i| bodies[i].on_foot() && within(point, grab.radius, bodies[i].position));
                if let Some(i) = unit.filter(|&i| bodies[i].alive) { self.seize(i, &mut bodies[i], g, tick); break; }
            }
        }
        self.advance(dt, settings);
        if self.victim.is_some() { self.carry(tick, bodies, settings, world); }
    }
    pub fn advance(&mut self, dt: f64, settings: &Settings) {
        let mut step = dt;
        if !self.looping { self.time += dt; }
        else {
            let span = length(settings.clips.first().and_then(Option::as_ref));
            if self.time + dt < span { self.time += dt; }
            else {
                self.hold += dt;
                if self.hold > settings.idle_delay { self.hold = 0.0; self.time += dt; } else { step = 0.0; }
            }
            if span > 0.0 && self.time > span { self.time -= span; }
        }
        self.stand(self.weight, step, settings);
        self.weight += (if self.slot == 0 { 0.2 } else { 1.0 }) * step;
    }
    pub fn stand(&mut self, weight: f64, dt: f64, settings: &Settings) {
        let pose = self.pose.as_mut().expect("animated pose"); pose.clear();
        if let Some(clip) = settings.clips.get(self.slot as usize).and_then(Option::as_ref) { pose.add(clip, self.time, 1.0, self.looping); }
        let share = if weight >= 1.0 { 1.0 } else if dt > 0.0 { 1.0 - v8math::pow(1.0 - max(weight, 0.0), dt / TICK) } else { weight };
        let (q, t) = (&mut self.stand_q, &mut self.stand_t);
        for i in 0..pose.w.len() {
            let w = pose.w[i] as f64; if w <= 1e-6 { continue; }
            let qi = i * 4; let ti = i * 3;
            let mut target = [pose.q[qi] as f64, pose.q[qi + 1] as f64, pose.q[qi + 2] as f64, pose.q[qi + 3] as f64];
            if q[qi] as f64 * target[0] + q[qi + 1] as f64 * target[1] + q[qi + 2] as f64 * target[2] + q[qi + 3] as f64 * target[3] < 0.0 { for x in &mut target { *x = -*x; } }
            for k in 0..4 { q[qi + k] = (q[qi + k] as f64 + (target[k] - q[qi + k] as f64) * share) as f32; }
            let len = (q[qi] as f64 * q[qi] as f64 + q[qi + 1] as f64 * q[qi + 1] as f64 + q[qi + 2] as f64 * q[qi + 2] as f64 + q[qi + 3] as f64 * q[qi + 3] as f64).sqrt();
            let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
            for k in 0..4 { q[qi + k] = (q[qi + k] as f64 / len) as f32; }
            for k in 0..3 { t[ti + k] = (t[ti + k] as f64 + (pose.t[ti + k] as f64 / w - t[ti + k] as f64) * share) as f32; }
        }
        for (i, &bone) in self.rig.bones.iter().enumerate() {
            let node = self.rig.tree.node_mut(bone); let qi = i * 4; let ti = i * 3;
            node.set_quaternion(q[qi] as f64, q[qi + 1] as f64, q[qi + 2] as f64, q[qi + 3] as f64);
            node.position.set(t[ti] as f64, t[ti + 1] as f64, t[ti + 2] as f64);
        }
    }
    pub fn seize(&mut self, index: usize, unit: &mut Body, grab: usize, tick: f64) {
        self.victim = Some(Victim { unit: index, generation: unit.generation, grab, since: tick });
        unit.held = Some(Held { prop: self.index, grab, since: tick, frame: None, time: None });
        if unit.combat { unit.mounted = true; }
        unit.velocity.set(0.0, 0.0, 0.0); if unit.air { unit.air = false; } unit.dive = 0.0;
    }
    pub fn swallow(&mut self, bodies: &mut [Body], settings: &Settings, world: &mut impl World) {
        let Some(victim) = self.victim.take() else { return };
        let unit = &mut bodies[victim.unit];
        if unit.generation != victim.generation || unit.removed { return; }
        if unit.held.is_some() { if unit.let_go { world.let_go(self, unit); } else { unit.held = None; } }
        if unit.alive && unit.combat { world.damage(self, unit, f64::INFINITY); }
        if unit.remove { world.remove(self, unit); }
        if let Some(sound) = settings.kill_sound { world.sound(sound, settings.position); }
    }
    pub fn carry(&mut self, tick: f64, bodies: &mut [Body], settings: &Settings, world: &mut impl World) {
        let victim = self.victim.as_ref().expect("carry victim");
        let unit = &mut bodies[victim.unit];
        if unit.generation != victim.generation || unit.removed || !unit.held.as_ref().is_some_and(|h| h.prop == self.index) { self.victim = None; return; }
        let time = (tick - victim.since) * TICK;
        let grab = &settings.grabs[unit.held.as_ref().unwrap().grab];
        let Some(node) = grab.node else { return };
        if !unit.rider_clips.get(unit.held.as_ref().unwrap().grab).copied().unwrap_or(false) { return; }
        let Some((point, turn)) = world.rider_root(unit, grab.clip, max(time, 0.0)) else { return };
        self.rig.tree.update_matrix_world(self.rig.root, true);
        let mut root = Matrix4::default(); root.compose(&point, &turn, &Vector3::new(1.0, 1.0, 1.0)); root.invert();
        let mut frame = Matrix4::default(); frame.multiply_matrices(&self.rig.tree.node(self.rig.bones[node]).matrix_world, &root);
        unit.position.set_from_matrix_position(&frame);
        if let Some(previous) = &mut unit.previous { *previous = unit.position; }
        if let Some(settled) = &mut unit.settled { *settled = unit.position; }
        unit.velocity.set(0.0, 0.0, 0.0);
        let held = unit.held.as_mut().unwrap(); held.frame = Some(frame); held.time = Some(time);
        let count = unit.tick_count.unwrap_or(0.0); unit.tick_count = Some(if count.is_nan() { 1.0 } else { count + 1.0 });
    }
    pub fn state_record(&self, bodies: &[Body]) -> [f64; 7] {
        let time = jsrt::numfmt::to_fixed(self.start_time, 4.0).unwrap().parse::<f64>().unwrap();
        let (id, generation, grab, since) = self.victim.as_ref().map_or((0.0, 0.0, -1.0, 0.0), |v| (bodies[v.unit].id, v.generation, v.grab as f64, v.since));
        [self.slot as f64, self.started, time, id, generation, grab, since]
    }
}
