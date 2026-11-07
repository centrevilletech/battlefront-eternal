//! The page's animated props and log traps (LiveObjects.presentAnimated of
//! src/props.js, AnimatedProp.step/follow on the page and Trap.present of
//! src/traps.js), stepped in one batched frame. Registration owns every
//! allocation; the frame reads soldiers' feet, bodies and the adopted world
//! state from fixed records and publishes each posed prop's bones, and
//! where a creature hangs whom it holds (AnimatedProp.presentHeld). The
//! world's props are rust/sim battle/animated.rs: both reach the clocks of
//! bfe_pose::prop, so a page draws a prop as the world steps it.
use crate::layout::{
    prop_body as pb, prop_held as hs, prop_bone as bo, prop_event as pe, prop_foot as pf,
    prop_frame as fr, prop_header as ph, prop_input as pi, prop_limits as lim, prop_output as po,
    status, trap_input as ti, trap_output as to, trap_phase as tp,
};
use bfe_pose::prop::{self, max, min, TICK};
use bfe_pose::rig::rider::{rider_points, RiderPoints, RiderSample, RiderScratch, RiderSkeleton};
use bfe_pose::rig::skeleton::Skeleton;
use bfe_pose::{Clip, Pose};
use serde::Deserialize;
use three::{Matrix4, Quaternion, Vector3};

const NONE: u32 = u32::MAX;

#[derive(Deserialize)]
pub struct TriggerInput {
    /// The slot its action plays in (-1: none).
    pub slot: i32,
    /// Its node's world point at load.
    pub point: [f64; 3],
    /// How far round the point it reaches.
    pub radius: f64,
    /// Whether it plays a sound (the host plays it).
    pub sound: bool,
}
/// An AnimatedProp's load-time description (the JS constructor's fields):
/// clip handles by slot, its triggers' world points, and its bones'
/// current and bind transforms (quaternion then position, 7 per bone).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropInput {
    /// Complete resource tree, including matrix-authored placement and meshes.
    #[serde(default)]
    pub scene: crate::scene::Input,
    /// The pose's DummyRoot (`dummy_?root`, else 0).
    pub root: i32,
    /// Each bone's quaternion then position as it stands.
    pub initial: Vec<f64>,
    /// Each bone's bind (`rig.bind`), the same layout.
    pub bind: Vec<f64>,
    /// `desc.base === "door"`.
    pub door: bool,
    /// The open clip, else the idle (`doorClip`).
    pub door_clip: Option<u32>,
    /// That clip's `duration`.
    pub door_duration: f64,
    /// Each slot's clip (the constructor's `slots`).
    pub slots: Vec<Option<u32>>,
    /// Whether each slot's name is "open".
    pub open_slots: Vec<bool>,
    /// Its triggers, in order.
    pub triggers: Vec<TriggerInput>,
    /// Whether a trigger's action is "open" (its body follows play).
    pub opens: bool,
    /// Whether it has a collision body.
    pub body: bool,
    /// IdleDelay, s.
    pub idle_delay: f64,
    /// Whether it seizes (a creature).
    pub grabs: bool,
    /// Each grab's node, as an index into its scene's nodes (-1: none).
    #[serde(default)]
    pub grab_nodes: Vec<i32>,
    /// The constructor's start time into the idle.
    pub time: f64,
}
/// A held soldier's skeleton as riderPoints reads it: his bones' names,
/// parents and transforms (position, quaternion, scale) when first seized,
/// and his animator's bind lengths and DummyRoot bind (bindsOf, rootBind).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiderInput {
    pub names: Vec<String>,
    pub parents: Vec<i32>,
    pub poses: Vec<[f64; 10]>,
    /// Each bind length: its bone and that bone's bind position.
    pub binds: Vec<(i32, [f64; 3])>,
    pub root_bind: Option<[f64; 3]>,
}
/// A registered held skeleton, with what riderPoints samples into.
struct Rider {
    skeleton: RiderSkeleton,
    /// The joints a clip leaves alone keep these (his bones as registered).
    q: Vec<Quaternion>,
    p: Vec<Vector3>,
    scratch: RiderScratch,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrapDesc {
    #[serde(default)]
    pub scene: crate::scene::Input,
    /// The pose's DummyRoot.
    pub root: i32,
    /// Each bone's quaternion then position as it stands.
    pub initial: Vec<f64>,
    /// The swing.
    pub trigger_clip: Option<u32>,
    /// The haul up, if any.
    pub build_clip: Option<u32>,
    /// The swing's length at 30 frames a second (traps.js length).
    pub trigger_length: f64,
}

struct Prop {
    scene: crate::scene::Scene,
    /// The sampler AnimatedProp keeps.
    pose: Pose,
    /// The stand pose, Float32Array storage as the JS keeps it.
    stand_q: Vec<f32>,
    stand_t: Vec<f32>,
    /// Each bone's bind (a door starts from it).
    bind: Vec<f64>,
    door: bool,
    /// Clip handles (NONE: no clip).
    door_clip: u32,
    door_duration: f64,
    slots: Vec<u32>,
    open_slots: Vec<bool>,
    triggers: Vec<TriggerInput>,
    opens: bool,
    idle_delay: f64,
    /// A creature: it follows its adopted world state, and hangs whom it
    /// holds from its grabs' nodes (scene node indexes).
    grabs: bool,
    grab_nodes: Vec<i32>,
    /// AnimatedProp.adopted: none yet, the first state (stood at once), or
    /// following the world's.
    adopted: Adopted,
    /// How many bones it has (its bone records).
    bones: usize,
    // AnimatedProp's clock (the constructor's defaults).
    slot: i32,
    time: f64,
    weight: f64,
    looping: bool,
    hold: f64,
    started: f64,
    start_time: f64,
    openness: f64,
    /// Its collision body's `enabled`, None without a body.
    body: Option<bool>,
}
#[derive(Clone, Copy, PartialEq)]
enum Adopted {
    None,
    First,
    Following,
}
struct Trap {
    scene: crate::scene::Scene,
    /// The sampler Trap keeps.
    pose: Pose,
    /// Clip handles (NONE: no clip).
    trigger_clip: u32,
    build_clip: u32,
    /// The swing's length (s).
    trigger_length: f64,
    /// How many bones it has.
    bones: usize,
}

pub struct Props {
    clips: Vec<Clip>,
    props: Vec<Prop>,
    traps: Vec<Trap>,
    /// PropFrame, then PropLimits.FEET feet and PropLimits.BODIES bodies.
    pub input: Vec<f64>,
    /// A PropInput record per prop (the world state a page adopted).
    pub prop_input: Vec<f64>,
    pub prop_output: Vec<f64>,
    pub trap_input: Vec<f64>,
    pub trap_output: Vec<f64>,
    /// PropBone records: every prop's bones, then every trap's.
    pub bones: Vec<f64>,
    pub events: Vec<f64>,
    pub header: Vec<f64>,
    turrets: Vec<crate::turrets::Turret>,
    pub turret_input: Vec<f64>,
    pub nodes: Vec<f64>,
    riders: Vec<Rider>,
    points: RiderPoints,
}

/// A handle's clip, or None (NONE, or past the loaded ones).
fn clip(clips: &[Clip], handle: u32) -> Option<&Clip> {
    clips.get(handle as usize)
}
fn handle(x: Option<u32>) -> u32 {
    x.unwrap_or(NONE)
}

impl Default for Props {
    fn default() -> Self {
        Self::new()
    }
}
impl Props {
    pub fn new() -> Props {
        Props {
            clips: Vec::new(),
            props: Vec::new(),
            traps: Vec::new(),
            input: vec![0.0; fr::STRIDE + lim::FEET * pf::STRIDE + lim::BODIES * pb::STRIDE],
            prop_input: Vec::new(),
            prop_output: Vec::new(),
            trap_input: Vec::new(),
            trap_output: Vec::new(),
            bones: Vec::new(),
            events: Vec::new(),
            header: vec![0.0; ph::STRIDE],
            turrets: Vec::new(),
            turret_input: Vec::new(),
            nodes: Vec::new(),
            riders: Vec::new(),
            points: RiderPoints::default(),
        }
    }
    pub fn feet_at() -> usize {
        fr::STRIDE
    }
    pub fn bodies_at() -> usize {
        fr::STRIDE + lim::FEET * pf::STRIDE
    }

    /// A clip, loaded once (LiveObjects.clipsFor). Returns its handle.
    pub fn load_clip(&mut self, frames: usize, fps: f64, root_motion: bool, tracks: Vec<i32>, data: Vec<f32>) -> u32 {
        self.clips.push(Clip { frames, fps, root_motion, tracks, data });
        (self.clips.len() - 1) as u32
    }

    /// An animated prop with a pose (the JS constructor's). Returns its
    /// index; its bones start at `bone_start(index)`.
    pub fn add_prop(&mut self, d: PropInput) -> usize {
        let bones = d.initial.len() / 7;
        // The stand pose starts at the bones as they stand (from bind, the
        // JS reads), rounded to its Float32Array storage.
        let mut stand_q = vec![0f32; bones * 4];
        let mut stand_t = vec![0f32; bones * 3];
        // The stand pose's Float32Arrays, from the bones as they stand.
        for i in 0..bones {
            for k in 0..4 {
                stand_q[i * 4 + k] = d.initial[i * 7 + k] as f32;
            }
            for k in 0..3 {
                stand_t[i * 3 + k] = d.initial[i * 7 + 4 + k] as f32;
            }
        }
        self.bones.extend_from_slice(&d.initial);
        let mut bind = d.bind;
        bind.resize(bones * 7, 0.0);
        self.props.push(Prop {
            scene: crate::scene::Scene::new(d.scene, &mut self.nodes),
            pose: Pose::new(bones, d.root),
            stand_q,
            stand_t,
            bind,
            door: d.door,
            door_clip: handle(d.door_clip),
            door_duration: d.door_duration,
            slots: d.slots.into_iter().map(handle).collect(),
            open_slots: d.open_slots,
            triggers: d.triggers,
            opens: d.opens,
            idle_delay: d.idle_delay,
            grabs: d.grabs,
            grab_nodes: d.grab_nodes,
            adopted: Adopted::None,
            bones,
            slot: -1,
            time: d.time,
            weight: 1.0,
            looping: false,
            hold: 0.0,
            started: 0.0,
            start_time: d.time,
            openness: 0.0,
            body: if d.body { Some(true) } else { None },
        });
        // One record of each kind per prop, and room for one sound each: a
        // frame never grows them.
        self.prop_input.resize(self.props.len() * pi::STRIDE, 0.0);
        self.prop_output.resize(self.props.len() * po::STRIDE, 0.0);
        self.events.resize(self.props.len() * pe::STRIDE, 0.0);
        let row = (self.props.len()-1)*po::STRIDE;
        let scene = &self.props.last().unwrap().scene;
        self.prop_output[row+po::NODE_START] = scene.start as f64;
        self.prop_output[row+po::NODE_COUNT] = scene.nodes.len() as f64;
        self.props.len() - 1
    }
    /// A held soldier's skeleton (his class's, registered at his first
    /// seizure). Returns its handle for PropInput.RIDER.
    pub fn add_rider(&mut self, d: RiderInput) -> usize {
        let n = d.names.len().min(d.parents.len()).min(d.poses.len());
        let bind: Vec<_> = d.poses[..n]
            .iter()
            .map(|p| (Vector3::new(p[0], p[1], p[2]), Quaternion::new(p[3], p[4], p[5], p[6]), Vector3::new(p[7], p[8], p[9])))
            .collect();
        let mut names = d.names;
        names.truncate(n);
        let mut parents = d.parents;
        parents.truncate(n);
        // Parents outside the skeleton hang from nothing.
        for up in parents.iter_mut() {
            if *up >= n as i32 {
                *up = -1;
            }
        }
        let mut skeleton = RiderSkeleton::new(&Skeleton::from_bind(names, parents, &bind, false));
        // The animator's own bind lengths and root bind, as riderPoints reads
        // them (the bones it was built with may stand posed by now).
        skeleton.binds.iter_mut().for_each(|b| *b = None);
        for &(i, at) in &d.binds {
            if i >= 0 && (i as usize) < n {
                skeleton.binds[i as usize] = Some(Vector3::new(at[0], at[1], at[2]));
            }
        }
        skeleton.root_bind = d.root_bind.map(|at| Vector3::new(at[0], at[1], at[2]));
        let scratch = RiderScratch::new(&skeleton);
        self.riders.push(Rider { skeleton, q: bind.iter().map(|b| b.1).collect(), p: bind.iter().map(|b| b.0).collect(), scratch });
        self.riders.len() - 1
    }
    /// A log trap with a pose (Trap's constructor). Its bones follow every
    /// prop's.
    pub fn add_trap(&mut self, d: TrapDesc) -> usize {
        let bones = d.initial.len() / 7;
        self.bones.extend_from_slice(&d.initial);
        self.traps.push(Trap {
            scene: crate::scene::Scene::new(d.scene, &mut self.nodes),
            pose: Pose::new(bones, d.root),
            trigger_clip: handle(d.trigger_clip),
            build_clip: handle(d.build_clip),
            trigger_length: d.trigger_length,
            bones,
        });
        self.trap_input.resize(self.traps.len() * ti::STRIDE, 0.0);
        self.trap_output.resize(self.traps.len() * to::STRIDE, 0.0);
        let row = (self.traps.len()-1)*to::STRIDE;
        let scene = &self.traps.last().unwrap().scene;
        self.trap_output[row+to::NODE_START] = scene.start as f64;
        self.trap_output[row+to::NODE_COUNT] = scene.nodes.len() as f64;
        self.traps.len() - 1
    }
    /// TurretSeats.present and poseTurret join this phase; registration
    /// reserves every matrix row before the page can present a frame.
    pub fn add_turret(&mut self, d: crate::turrets::Descriptor) -> usize {
        let turret = crate::turrets::Turret::new(d, &mut self.nodes);
        let start = turret.scene.start;
        self.turrets.push(turret);
        self.turret_input.resize(self.turrets.len() * crate::layout::turret_presentation_input::STRIDE, 0.0);
        start
    }
    /// The first PropBone of a prop's (`trap` false) or a trap's bones.
    pub fn bone_start(&self, trap: bool, index: usize) -> usize {
        let props: usize = self.props.iter().map(|p| p.bones).sum();
        if !trap {
            return self.props[..index.min(self.props.len())].iter().map(|p| p.bones).sum();
        }
        props + self.traps[..index.min(self.traps.len())].iter().map(|t| t.bones).sum::<usize>()
    }

    /// One frame (presentAnimated): the traps at the tick drawn, then each
    /// prop following the world's state or stepping on the page's soldiers.
    pub fn frame(&mut self) {
        if self.input[fr::VERSION] != crate::layout::VERSION as f64 {
            self.header[ph::STATUS] = status::VERSION_MISMATCH as f64;
            self.header[ph::EVENTS] = 0.0;
            return;
        }
        self.header[ph::STATUS] = status::OK as f64;
        for (i, turret) in self.turrets.iter_mut().enumerate() {
            let start = i * crate::layout::turret_presentation_input::STRIDE;
            turret.present(&self.turret_input[start..], &mut self.nodes);
        }
        let dt = self.input[fr::DT];
        let tick = self.input[fr::TICK];
        // Counts clamp to the records the host can have written.
        // NaN and negative counts read as none.
        let feet = (self.input[fr::FEET].max(0.0) as usize).min(lim::FEET);
        let bodies = (self.input[fr::BODIES].max(0.0) as usize).min(lim::BODIES);
        let (head, rest) = self.input.split_at(Self::bodies_at());
        let feet = &head[fr::STRIDE..fr::STRIDE + feet * pf::STRIDE];
        let bodies = &rest[..bodies * pb::STRIDE];
        // Bone records: every prop's, then every trap's, in order.
        let mut bone = self.props.iter().map(|p| p.bones).sum::<usize>();
        // Trap.present per trap.
        for (i, trap) in self.traps.iter_mut().enumerate() {
            let at = i * ti::STRIDE;
            let posed = trap.present(&self.clips, tick, self.trap_input[at + ti::PHASE], self.trap_input[at + ti::STARTED]);
            if posed {
                write_posed(&trap.pose, &mut self.bones[bone * bo::STRIDE..(bone + trap.bones) * bo::STRIDE]);
            }
            trap.scene.pose(&self.bones[bone * bo::STRIDE..(bone + trap.bones) * bo::STRIDE]);
            trap.scene.publish(&mut self.nodes);
            self.trap_output[i * to::STRIDE + to::POSED] = posed as u8 as f64;
            bone += trap.bones;
        }
        // presentAnimated stepped the props at the whole tick drawn.
        let mut events = 0;
        let floor = tick.floor();
        bone = 0;
        for (i, p) in self.props.iter_mut().enumerate() {
            let at = i * pi::STRIDE;
            let out = &mut self.bones[bone * bo::STRIDE..(bone + p.bones) * bo::STRIDE];
            bone += p.bones;
            let x = &self.prop_input[at..at + pi::STRIDE];
            // AnimatedProp.adopt: the world's latest state of a creature,
            // taken when its clip or start differs from the one shown. The
            // first a page has stands at once; later ones ease in.
            if p.grabs && x[pi::WORLD] != 0.0 && (x[pi::SLOT] != p.slot as f64 || x[pi::STARTED] != p.started) {
                p.adopted = if p.adopted == Adopted::None { Adopted::First } else { Adopted::Following };
                p.slot = clamp_slot(x[pi::SLOT]);
                p.looping = p.slot == 0;
                p.started = x[pi::STARTED];
                p.start_time = x[pi::START_TIME];
            }
            // presentAnimated's three cases: a creature following the world,
            // one idling before its state arrives (no bodies: it seizes no
            // one here), and any other prop stepping on the page's soldiers.
            let posed = if p.grabs && p.adopted != Adopted::None {
                let posed = p.follow(&self.clips, dt, tick, p.adopted == Adopted::First, out);
                // follow() keeps easing once the world's clip is known.
                if p.slot >= 0 {
                    p.adopted = Adopted::Following;
                }
                posed
            } else {
                // Until then it idles here, setting nothing off.
                let bodies = if p.grabs { &bodies[..0] } else { bodies };
                let fired = p.step(&self.clips, dt, feet, bodies, floor, out);
                if fired >= 0 && events < self.events.len() / pe::STRIDE {
                    self.events[events * pe::STRIDE + pe::PROP] = i as f64;
                    self.events[events * pe::STRIDE + pe::TRIGGER] = fired as f64;
                    events += 1;
                }
                true
            };
            p.scene.pose(out);
            p.scene.publish(&mut self.nodes);
            // What the host writes back: the bones (if posed), its body, and
            // the clip a creature idles in before it adopts the world's.
            let o = i * po::STRIDE;
            self.prop_output[o + po::POSED] = posed as u8 as f64;
            self.prop_output[o + po::SLOT] = p.slot as f64;
            self.prop_output[o + po::STARTED] = p.started;
            self.prop_output[o + po::START_TIME] = p.start_time;
            self.prop_output[o + po::BODY_ENABLED] = match p.body {
                None => -1.0,
                Some(on) => on as u8 as f64,
            };
            // Whom an adopted creature holds, hung from its posed node.
            let held = &mut self.prop_output[o + po::HELD..o + po::HELD_FRAME + 16];
            held[0] = hs::NONE as f64;
            if p.grabs && p.adopted != Adopted::None {
                present_held(p, &self.prop_input[at..at + pi::STRIDE], tick, &self.clips, &mut self.riders, &mut self.points, held);
            }
        }
        self.header[ph::EVENTS] = events as f64;
    }
}

/// A slot from the host's record (clamped; NaN reads as none).
fn clamp_slot(x: f64) -> i32 {
    if x >= 0.0 { x.min(i32::MAX as f64) as i32 } else { -1 }
}

/// AnimatedProp.presentHeld and heldFrame: where a page draws whom a
/// creature holds, `time` seconds into his hold. Before the tick it took
/// him (the page draws a little behind the world) he is drawn where his own
/// record has him (WAITING). Then his body in his grab clip at that time,
/// his root bone (bone_root, as the clip poses it) on the grab's node,
/// turned with it: the 2004 game's inverse root times the node. Not placed
/// without his clip, his skeleton's joints or the node. The host says
/// whether he is still held by this creature on its page (VICTIM).
fn present_held(p: &Prop, x: &[f64], tick: f64, clips: &[Clip], riders: &mut [Rider], points: &mut RiderPoints, out: &mut [f64]) {
    // `out` starts at the HELD word.
    let o = po::HELD;
    if x[pi::VICTIM] == 0.0 {
        return;
    }
    let since = x[pi::SINCE];
    if tick < since {
        out[0] = hs::WAITING as f64;
        return;
    }
    let time = (tick - since) * TICK;
    out[0] = hs::TIMED as f64;
    out[po::HELD_TIME - o] = time;
    out[po::HELD_PLACED - o] = 0.0;
    let grab = x[pi::GRAB];
    let node = if grab >= 0.0 { p.grab_nodes.get(grab as usize).copied().unwrap_or(-1) } else { -1 };
    let node = if node >= 0 { p.scene.nodes.get(node as usize).copied() } else { None };
    let rider = if x[pi::RIDER] >= 0.0 { riders.get_mut(x[pi::RIDER] as usize) } else { None };
    let clip = if x[pi::CLIP] >= 0.0 { clips.get(x[pi::CLIP] as usize) } else { None };
    let (Some(node), Some(r), Some(clip)) = (node, rider, clip) else { return };
    let sample = RiderSample { clip, pitch: 0.0, turn: 0.0, time: Some(max(time, 0.0)) };
    if !rider_points(&r.skeleton, &r.q, &r.p, &sample, &mut r.scratch, points, true) {
        return;
    }
    let mut root = Matrix4::default();
    root.compose(&points.root, &points.root_turn, &Vector3::new(1.0, 1.0, 1.0)).invert();
    let mut frame = Matrix4::default();
    frame.multiply_matrices(&p.scene.tree.node(node).matrix_world, &root);
    out[po::HELD_FRAME - o..po::HELD_FRAME - o + 16].copy_from_slice(&frame.elements);
    out[po::HELD_PLACED - o] = 1.0;
}

/// Pose.apply's finals into bone records, for bones a clip weighted;
/// the rest keep what they held (the bind, for a door, or the last pose).
fn write_posed(pose: &Pose, out: &mut [f64]) {
    for i in 0..pose.w.len() {
        if pose.w[i] as f64 <= 1e-6 {
            continue;
        }
        out[i * bo::STRIDE..(i + 1) * bo::STRIDE].copy_from_slice(&pose.out[i * 7..(i + 1) * 7]);
    }
}

impl Trap {
    /// Trap.present: the clip of its phase at the world's time for `tick`;
    /// unbuilt it rests at the swing's end, cocked at its start. `started`
    /// NaN is the JS's null (`?? tick`).
    fn present(&mut self, clips: &[Clip], tick: f64, phase: f64, started: f64) -> bool {
        let mut handle = self.trigger_clip;
        let mut time = 0.0;
        let since = if started.is_nan() { tick } else { started };
        let elapsed = max(0.0, (tick - since + 1.0) * TICK);
        let phase = phase as u32;
        if phase == tp::DOWN {
            time = self.trigger_length;
        } else if phase == tp::RESET {
            if self.build_clip != NONE {
                handle = self.build_clip;
                time = elapsed;
            } else {
                time = self.trigger_length;
            }
        } else if phase == tp::SWING {
            time = elapsed;
        }
        // Trap.posed: nothing without the clip.
        let Some(c) = clip(clips, handle) else { return false };
        self.pose.clear();
        self.pose.add(c, time, 1.0, false);
        self.pose.apply_out();
        true
    }
}

impl Prop {
    /// AnimatedProp.play (the same slot again changes nothing).
    fn play(&mut self, slot: i32, looping: bool, tick: f64) {
        if slot == self.slot {
            return;
        }
        self.slot = slot;
        self.looping = looping;
        self.started = tick;
        self.start_time = self.time;
        if self.body.is_some() && !self.door && self.opens {
            let open = slot >= 0 && self.open_slots.get(slot as usize) == Some(&true);
            self.body = Some(!(!looping && open));
        }
    }
    fn slot_clip<'a>(&self, clips: &'a [Clip], slot: i32) -> Option<&'a Clip> {
        if slot < 0 {
            return None;
        }
        self.slots.get(slot as usize).and_then(|&h| clip(clips, h))
    }
    /// AnimatedProp.near: anyone's feet within the first trigger's reach.
    fn near(&self, feet: &[f64]) -> bool {
        let Some(t) = self.triggers.first() else { return false };
        let r = t.radius * t.radius;
        feet.chunks_exact(pf::STRIDE).any(|f| {
            let (dx, dy, dz) = (f[0] - t.point[0], f[1] - t.point[1], f[2] - t.point[2]);
            dx * dx + dy * dy + dz * dz <= r
        })
    }
    /// AnimatedProp.step as a page runs it: no one is seized here, so no
    /// grab or swallow. Returns the trigger whose sound plays, or -1.
    fn step(&mut self, clips: &[Clip], dt: f64, feet: &[f64], bodies: &[f64], tick: f64, out: &mut [f64]) -> i32 {
        if self.door {
            let near = self.near(feet);
            let door = clip(clips, self.door_clip);
            self.openness = prop::door_openness(self.openness, near, dt, door.map(|_| self.door_duration));
            self.pose.clear();
            if let Some(c) = door {
                self.pose.add(c, self.openness * self.door_duration, 1.0, false);
            }
            if self.body.is_some() {
                self.body = Some(self.openness < 0.5);
            }
            // The bind, then the clip's bones over it.
            out.copy_from_slice(&self.bind);
            self.pose.apply_out();
            write_posed(&self.pose, out);
            return -1;
        }
        // The idle from the first step; an action, once played, back to it.
        if self.slot < 0 {
            self.play(0, true, tick);
        }
        if !self.looping && self.time > prop::length(self.slot_clip(clips, self.slot)) {
            self.time = 0.0;
            self.weight = 0.0;
            self.play(0, true, tick);
        }
        // Idling and half blended in: the first trigger with a soldier at
        // its node plays its action.
        let mut fired = -1;
        if self.looping && self.weight >= 0.5 {
            for (k, t) in self.triggers.iter().enumerate() {
                if t.slot < 0 || t.slot == self.slot {
                    continue;
                }
                let hit = bodies
                    .chunks_exact(pb::STRIDE)
                    .any(|b| b[pb::ON_FOOT] != 0.0 && prop::within(t.point, t.radius, [b[0], b[1], b[2]]));
                if !hit {
                    continue;
                }
                let (slot, sound) = (t.slot, t.sound);
                self.time = 0.0;
                self.weight = 0.0;
                self.play(slot, false, tick);
                if sound {
                    fired = k as i32;
                }
                break;
            }
        }
        // AnimatedProp.advance: the time, the pose eased, then the weight.
        let span = prop::length(self.slot_clip(clips, 0));
        let step = prop::advance(&mut self.time, &mut self.hold, self.looping, dt, span, self.idle_delay);
        self.stand(clips, self.weight, step, out);
        self.weight += (if self.slot == 0 { 0.2 } else { 1.0 }) * step;
        fired
    }
    /// AnimatedProp.follow: the clip at the world's time for `tick`, eased
    /// as the world eases it. The first state a page adopts stands at once.
    fn follow(&mut self, clips: &[Clip], dt: f64, tick: f64, first: bool, out: &mut [f64]) -> bool {
        if self.slot < 0 {
            return false;
        }
        // The world's step on the tick it started stepped the clock once.
        let elapsed = max(0.0, (tick - self.started + 1.0) * TICK);
        let wanted = if self.looping { self.start_time + elapsed } else { elapsed };
        let span = prop::length(self.slot_clip(clips, 0));
        self.time = if self.looping && span > 0.0 { wanted % span } else { wanted };
        let weight = if first {
            1.0
        } else {
            min(1.0, (if self.slot == 0 { 0.2 } else { 1.0 }) * max(0.0, tick - self.started) * TICK)
        };
        if dt > 0.0 || weight >= 1.0 {
            self.stand(clips, weight, dt, out);
            return true;
        }
        false
    }
    /// AnimatedProp.stand, every bone from the stand pose.
    fn stand(&mut self, clips: &[Clip], weight: f64, dt: f64, out: &mut [f64]) {
        let c = self.slot_clip(clips, self.slot);
        prop::stand(&mut self.stand_q, &mut self.stand_t, &mut self.pose, c, self.time, self.looping, weight, dt);
        for i in 0..self.bones {
            let o = &mut out[i * bo::STRIDE..];
            for k in 0..4 {
                o[bo::QUATERNION + k] = self.stand_q[i * 4 + k] as f64;
            }
            for k in 0..3 {
                o[bo::POSITION + k] = self.stand_t[i * 3 + k] as f64;
            }
        }
    }
}
