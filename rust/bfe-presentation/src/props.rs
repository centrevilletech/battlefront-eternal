//! The page's animated props and log traps (LiveObjects.presentAnimated of
//! src/props.js, AnimatedProp.step/follow on the page and Trap.present of
//! src/traps.js), stepped in one batched frame. Registration owns every
//! allocation; the frame reads soldiers' feet, bodies and the adopted world
//! state from fixed records and publishes each posed prop's bones. The
//! world's props are rust/sim battle/animated.rs: both reach the clocks of
//! bfe_pose::prop, so a page draws a prop as the world steps it.
use crate::layout::{
    prop_adopted as pa, prop_body as pb, prop_bone as bo, prop_event as pe, prop_foot as pf,
    prop_frame as fr, prop_header as ph, prop_input as pi, prop_limits as lim, prop_output as po,
    status, trap_input as ti, trap_output as to, trap_phase as tp,
};
use bfe_pose::prop::{self, max, min, TICK};
use bfe_pose::{Clip, Pose};
use serde::Deserialize;

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
    /// The constructor's start time into the idle.
    pub time: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrapDesc {
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
    /// A creature: it follows its adopted world state.
    grabs: bool,
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
struct Trap {
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
        self.props.len() - 1
    }
    /// A log trap with a pose (Trap's constructor). Its bones follow every
    /// prop's.
    pub fn add_trap(&mut self, d: TrapDesc) -> usize {
        let bones = d.initial.len() / 7;
        self.bones.extend_from_slice(&d.initial);
        self.traps.push(Trap {
            pose: Pose::new(bones, d.root),
            trigger_clip: handle(d.trigger_clip),
            build_clip: handle(d.build_clip),
            trigger_length: d.trigger_length,
            bones,
        });
        self.trap_input.resize(self.traps.len() * ti::STRIDE, 0.0);
        self.trap_output.resize(self.traps.len() * to::STRIDE, 0.0);
        self.traps.len() - 1
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
            let adopted = self.prop_input[at + pi::ADOPTED];
            // presentAnimated's three cases: a creature following the world,
            // one idling before its state arrives (no bodies: it seizes no
            // one here), and any other prop stepping on the page's soldiers.
            let posed = if p.grabs && adopted != pa::NONE as f64 {
                // A creature that seizes follows the world's adopted state
                // (AnimatedProp.adopt), whom it holds placed by the host.
                p.slot = self.prop_input[at + pi::SLOT] as i32;
                p.looping = p.slot == 0;
                p.started = self.prop_input[at + pi::STARTED];
                p.start_time = self.prop_input[at + pi::START_TIME];
                p.follow(&self.clips, dt, tick, adopted == pa::FIRST as f64, out)
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
        }
        self.header[ph::EVENTS] = events as f64;
    }
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
