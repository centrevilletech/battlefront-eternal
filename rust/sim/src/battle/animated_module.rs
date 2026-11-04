//! Animated prop commands. Clips and bind geometry are configured once;
//! callback arguments publish native prop/body state before foreign work.
use super::animated::{Animated, Body, Grab, Held, Settings, Trigger, Victim, World};
use crate::abi::sim;
use crate::gen::*;
use crate::pose::{Clip, Pose};
use crate::seats::turrets::TurretRig;
use crate::store::StrId;
use std::{ptr::addr_of_mut, rc::Rc};
use three::{Matrix4, Quaternion, Vector3};

struct Configuration { settings: Settings, root: [f64; 16], parents: Vec<f64>, bind: Vec<(Quaternion, Vector3)> }
static mut CONFIG: Vec<Rc<Configuration>> = Vec::new();
fn config() -> &'static mut Vec<Rc<Configuration>> { unsafe { &mut *addr_of_mut!(CONFIG) } }
fn v3(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }
fn a3(v: Vector3) -> [f64; 3] { [v.x, v.y, v.z] }
fn q4(v: [f64; 4]) -> Quaternion { Quaternion::new(v[0], v[1], v[2], v[3]) }
fn a4(v: Quaternion) -> [f64; 4] { [v.x, v.y, v.z, v.w] }
fn mat(v: [f64; 16]) -> Matrix4 { let mut m = Matrix4::default(); m.from_array(&v, 0); m }
fn clip(v: PaClip) -> Clip {
    Clip { frames: v.frames as usize, fps: v.fps, root_motion: v.root_motion,
        tracks: v.tracks.into_iter().map(|n| n as i32).collect(), data: v.data.into_iter().map(|n| n as f32).collect() }
}
fn configuration(v: PaSettings) -> Configuration {
    Configuration { root: v.root, parents: v.parents, bind: v.bind.into_iter().map(|b| (q4(b.q), v3(b.p))).collect(),
        settings: Settings { door: v.door, clips: v.clips.into_iter().map(|v| v.map(clip)).collect(), door_clip: v.door_clip.map(clip),
            door_duration: v.door_duration, idle_delay: v.idle_delay, open_slots: v.open_slots, position: v3(v.position), kill_sound: v.kill_sound,
            triggers: v.triggers.into_iter().map(|t| Trigger { slot: t.slot as i32, point: v3(t.point), radius: t.radius, sound: t.sound, opens: t.opens }).collect(),
            grabs: v.grabs.into_iter().map(|g| Grab { slot: g.slot as i32, window: g.window, radius: g.radius, node: g.node.map(|n| n as usize), clip: g.clip }).collect() } }
}
impl From<PaBody> for Body {
    fn from(v: PaBody) -> Self {
        Self { key: v.key as usize, id: v.id, generation: v.generation, soldier: v.soldier, walker: v.walker, removed: v.removed, alive: v.alive, dead: v.dead,
            seated: v.seated, held: v.held.map(|h| Held { prop: h.prop as usize, grab: h.grab as usize, since: h.since, frame: h.frame.map(mat), time: h.time }),
            combat: v.combat, mounted: v.mounted, position: v3(v.position), previous: v.previous.map(v3), settled: v.settled.map(v3), velocity: v3(v.velocity),
            air: v.air, dive: v.dive, tick_count: v.tick_count, let_go: v.let_go, remove: v.remove, rider_clips: v.rider_clips }
    }
}
impl From<&Body> for PaBody {
    fn from(v: &Body) -> Self {
        Self { key: v.key as f64, id: v.id, generation: v.generation, soldier: v.soldier, walker: v.walker, removed: v.removed, alive: v.alive, dead: v.dead,
            seated: v.seated, held: v.held.as_ref().map(|h| PaHeld { prop: h.prop as f64, grab: h.grab as f64, since: h.since, frame: h.frame.as_ref().map(|m| m.elements), time: h.time }),
            combat: v.combat, mounted: v.mounted, position: a3(v.position), previous: v.previous.map(a3), settled: v.settled.map(a3), velocity: a3(v.velocity),
            air: v.air, dive: v.dive, tick_count: v.tick_count, let_go: v.let_go, remove: v.remove, rider_clips: v.rider_clips.clone() }
    }
}
fn prop(v: PaState, config: &Configuration) -> Animated {
    let bones: Vec<_> = v.bones.iter().enumerate().map(|(i, b)| {
        let parent = config.parents[i]; ((parent >= 0.0).then_some(parent as usize), v3(b.p), q4(b.q), v3(b.s))
    }).collect();
    let worlds: Vec<_> = v.bones.iter().map(|b| b.world).collect();
    let mut rig = TurretRig::new(&config.root, &bones, Some(&worlds));
    rig.tree.node_mut(rig.root).matrix_world = mat(v.root_world);
    let pose = v.pose.map(|v| {
        let mut pose = Pose::new(v.w.len(), v.root as i32);
        pose.q = v.q.into_iter().map(|x| x as f32).collect(); pose.t = v.t.into_iter().map(|x| x as f32).collect(); pose.w = v.w.into_iter().map(|x| x as f32).collect(); pose
    });
    Animated { index: v.index as usize, slot: v.slot as i32, time: v.time, weight: v.weight, looping: v.r#loop, hold: v.hold, started: v.started, start_time: v.start_time,
        victim: v.victim.map(|v| Victim { unit: v.unit as usize, generation: v.generation, grab: v.grab as usize, since: v.since }),
        openness: v.openness, body_enabled: v.body_enabled, stand_q: v.stand_q.into_iter().map(|n| n as f32).collect(), stand_t: v.stand_t.into_iter().map(|n| n as f32).collect(),
        pose, rig, bind: config.bind.clone() }
}
impl From<&Animated> for PaState {
    fn from(p: &Animated) -> Self {
        Self { index: p.index as f64, slot: p.slot as f64, time: p.time, weight: p.weight, r#loop: p.looping, hold: p.hold, started: p.started, start_time: p.start_time,
            victim: p.victim.as_ref().map(|v| PaVictim { unit: v.unit as f64, generation: v.generation, grab: v.grab as f64, since: v.since }),
            openness: p.openness, body_enabled: p.body_enabled, stand_q: p.stand_q.iter().map(|&n| n as f64).collect(), stand_t: p.stand_t.iter().map(|&n| n as f64).collect(),
            pose: p.pose.as_ref().map(|v| PaPose { root: v.root as f64, q: v.q.iter().map(|&n| n as f64).collect(), t: v.t.iter().map(|&n| n as f64).collect(), w: v.w.iter().map(|&n| n as f64).collect() }),
            root_world: p.rig.tree.node(p.rig.root).matrix_world.elements,
            bones: p.rig.bones.iter().map(|&id| { let b = p.rig.tree.node(id); PaBone { p: a3(b.position), q: a4(b.quaternion), s: a3(b.scale), world: b.matrix_world.elements } }).collect() }
    }
}
struct JsWorld;
impl JsWorld {
    fn args(&self, kind: PaActionKind, p: &Animated) -> PaAction { PaAction { kind, index: p.index as f64, state: p.into(), ..PaAction::default() } }
    fn body(&mut self, kind: PaActionKind, p: &Animated, body: &mut Body, value: f64) {
        let result = import_pa_action(&PaAction { body: Some((&*body).into()), value, ..self.args(kind, p) });
        *body = result.body.expect("animated body callback").into();
    }
}
impl World for JsWorld {
    fn sound(&mut self, p: &Animated, name: StrId, position: Vector3) {
        import_pa_action(&PaAction { text: Some(name), point: a3(position), ..self.args(PaActionKind::Sound, p) });
    }
    fn let_go(&mut self, p: &Animated, b: &mut Body) { self.body(PaActionKind::LetGo, p, b, 0.0); }
    fn damage(&mut self, p: &Animated, b: &mut Body, value: f64) { self.body(PaActionKind::Damage, p, b, value); }
    fn remove(&mut self, p: &Animated, b: &mut Body) { self.body(PaActionKind::Remove, p, b, 0.0); }
    fn rider_root(&mut self, p: &Animated, b: &Body, clip: StrId, time: f64) -> Option<(Vector3, Quaternion)> {
        let result = import_pa_action(&PaAction { body: Some(b.into()), text: Some(clip), value: time, ..self.args(PaActionKind::Rider, p) });
        result.found.then(|| (v3(result.root), q4(result.turn)))
    }
}
#[no_mangle]
pub extern "C" fn animated_words(which: u32) -> u32 {
    (match which { 0 => PaConfiguration::WORDS, 1 => PaInput::WORDS, 2 => PaResult::WORDS, _ => 0 }) as u32
}
#[no_mangle]
pub extern "C" fn animated_configure() {
    let (row, heap) = sim().calls.args(); let input = PaConfiguration::take(row, heap);
    *config() = input.props.into_iter().map(|p| Rc::new(configuration(p))).collect();
}
#[no_mangle]
pub extern "C" fn animated_run() -> *const f64 {
    let (row, heap) = sim().calls.args(); let input = PaInput::take(row, heap);
    let config = config()[input.state.index as usize].clone();
    let mut prop = prop(input.state, &config);
    let mut bodies: Vec<Body> = input.bodies.into_iter().map(Into::into).collect();
    if input.op == PaOperation::Step {
        let order: Vec<_> = input.order.into_iter().map(|n| n as usize).collect();
        let positions: Vec<_> = input.positions.into_iter().map(v3).collect();
        prop.step(input.dt, &positions, &mut bodies, &order, input.tick, &config.settings, &mut JsWorld);
    }
    let result = PaResult { state: (&prop).into(), bodies: bodies.iter().map(Into::into).collect(), record: prop.state_record(&bodies).to_vec() };
    let sim = sim(); sim.calls.result(PaResult::WORDS, |row, heap| result.put(row, heap)); sim.calls.block.ptr()
}
