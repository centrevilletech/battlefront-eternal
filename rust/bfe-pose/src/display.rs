//! Display animation from src/animator.js. Clip names and weapon banks are
//! resolved at load; a frame only reads numeric handles and reuses its poses.
//! The same sampler and transform tree serve the world's rider and gun rigs.

//! SoldierAnimator carries the legs relative to facing. At rest they stay
//! planted until a posture's turn limit starts an authored step; in motion
//! they ease toward travel, keeping the forward/backward gait hysteresis.
//! Alert rifle layers turn the torso onto the aim, while relaxed layers let
//! the weapon follow the legs and turn only the neck and head.
//!
//! Lower-body layers set timing in the rifle bank. Each upper-body layer
//! samples its bank's corresponding clip at that same phase, except idles
//! keep their real-time clock. Weapon actions replace the upper body, or the
//! whole body while standing still. Only root and weapon hardpoints accept
//! authored translations; other joints retain their own bind lengths.
//!
use crate::rig::skeleton::Skeleton;
use crate::{js_max as max, js_min as min, Clip, Pose};
use three::{Matrix4, NodeId, Quaternion, TransformNode, TransformTree, Vector3};
use v8math::{atan2, cos, hypot, pow, sin};

pub const NONE: usize = usize::MAX;
pub const IDLE: usize = 0;
pub const WALK: usize = 1;
pub const RUN: usize = 2;
pub const WALK_BACK: usize = 3;
pub const RUN_BACK: usize = 4;
pub const TURN_LEFT: usize = 5;
pub const TURN_RIGHT: usize = 6;
pub const WALK_LEFT: usize = 7;
pub const WALK_RIGHT: usize = 8;
pub const STAND: usize = 0;
pub const PRONE: usize = 2;

/// Clip plus the locomotion metadata prepared by src/anim.js Clip.
pub struct DisplayClip {
    pub sample: Clip,
    pub duration: f64,
    pub speed: f64,
    pub idle: bool,
}

/// SoldierAnimator's named tables, resolved once by the content host.
/// An absent clip is NONE, including absent optional transition clips.
pub struct Tables {
    pub sets: [[usize; 9]; 3],
    pub alert: [usize; 9],
    pub transitions: [[usize; 3]; 3],
    pub jump: usize,
    pub fall: usize,
    pub land_soft: usize,
    pub land_hard: usize,
    pub dive: usize,
    pub jet: usize,
    /// Front/back, then flying/bounce/land/rest.
    pub thrown: [[usize; 4]; 2],
    /// [bank][base clip]. Missing bank clips already borrow the base clip.
    pub upper: Vec<Vec<usize>>,
}

/// The visual soldier state consumed by SoldierAnimator.updatePose.
#[derive(Clone, Copy, Default)]
pub struct SoldierState {
    pub yaw: f64,
    pub pitch: f64,
    pub velocity: Vector3,
    pub posture: usize,
    /// 0 grounded, 1 jump, 2 fall, 3 thrown, 4 jet.
    pub air: u32,
    pub dive: f64,
    pub dive_time: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Loco,
    Dive,
    Jet,
    Thrown,
    ThrownRest,
    ThrownLand,
    Dead,
    Jump,
    Fall,
    LandSoft,
    LandHard,
    Transition,
    Fidget,
}

#[derive(Clone, Copy)]
pub struct Gunning {
    pub clip: usize,
    pub pitch: f64,
    pub turn: f64,
    pub time: Option<f64>,
    pub seat: Option<Quaternion>,
}

#[derive(Clone, Copy)]
struct Layer {
    clip: usize,
    time: f64,
    weight: f64,
    looping: bool,
    rate: f64,
}

#[derive(Clone, Copy)]
struct WeaponLayer {
    clip: usize,
    action: u32,
    time: f64,
    weight: f64,
    blend: f64,
    stopping: bool,
}

/// An actor's complete body hierarchy. Children such as weapon hardpoints
/// can be registered in this tree and receive the same native world walk.
pub struct DisplayRig {
    pub tree: TransformTree,
    pub root: NodeId,
    pub body: NodeId,
    pub bones: Vec<NodeId>,
    parent: NodeId,
    root_bone: usize,
    root_bind: Vector3,
    bind_lengths: Vec<(usize, Vector3)>,
    upper: Vec<usize>,
    head: [Option<usize>; 2],
    spine: [Option<usize>; 3],
}

impl DisplayRig {
    /// buildActor's root, body, and bones. Bone zero is reparented to the
    /// body even when the source model supplied another parent for it.
    pub fn new(skeleton: &Skeleton) -> Self {
        let mut tree = TransformTree::new();
        let root = tree.add_node(TransformNode::default());
        let body = tree.add_node(TransformNode::default());
        // buildActor's body node never changes its local bind transform.
        // Its world matrix still follows the animated root every frame.
        tree.node_mut(body).update_matrix();
        tree.node_mut(body).matrix_auto_update = false;
        tree.add_child(root, body);
        let bones: Vec<_> = skeleton
            .bind
            .iter()
            .map(|b| tree.add_node(b.clone()))
            .collect();
        for i in 0..bones.len() {
            let p = skeleton.parents[i];
            if p >= 0 {
                tree.add_child(bones[p as usize], bones[i]);
            } else if i == 0 {
                tree.add_child(body, bones[i]);
            }
            // buildActor only hangs bone zero from the body. Other roots
            // stay detached, with their own world chain when queried.
        }
        let index = |name: &str| skeleton.by_name(name);
        let upper = [
            "bone_a_spine",
            "bone_b_spine",
            "bone_ribcage",
            "bone_neck",
            "bone_head",
            "bone_l_clavicle",
            "bone_l_upperarm",
            "bone_l_forearm",
            "bone_l_hand",
            "bone_r_clavicle",
            "bone_r_upperarm",
            "bone_r_forearm",
            "bone_r_hand",
        ]
        .iter()
        .filter_map(|n| index(n))
        .collect();
        let mut bind_lengths = Vec::new();
        for name in [
            "bone_pelvis",
            "bone_a_spine",
            "bone_b_spine",
            "bone_ribcage",
            "bone_neck",
            "bone_head",
        ] {
            if let Some(i) = index(name) {
                bind_lengths.push((i, skeleton.bind_position(i)));
            }
        }
        for side in ["l", "r"] {
            for part in [
                "clavicle", "upperarm", "forearm", "hand", "thigh", "calf", "foot", "toe",
            ] {
                if let Some(i) = index(&format!("bone_{side}_{part}")) {
                    bind_lengths.push((i, skeleton.bind_position(i)));
                }
            }
        }
        let root_bone = index("DummyRoot")
            .or_else(|| skeleton.find_lower("dummyroot"))
            .expect("actor has root bone");
        let mut external = TransformNode::default();
        external.matrix_auto_update = false;
        external.children.reserve(1);
        let parent = tree.add_node(external);
        Self {
            tree,
            parent,
            root,
            body,
            bones,
            root_bone,
            root_bind: skeleton.bind_position(root_bone),
            bind_lengths,
            upper,
            head: [index("bone_neck"), index("bone_head")],
            spine: [
                index("bone_a_spine"),
                index("bone_b_spine"),
                index("bone_ribcage"),
            ],
        }
    }

    /// A scene or interface parent is a foreign input, never a callback.
    /// Keep its node after the bones so asset bone indices remain stable.
    pub fn set_parent(&mut self, matrix: Option<Matrix4>) -> bool {
        let attached = self.tree.node(self.root).parent.is_some();
        let changed = match matrix {
            Some(m) => {
                let different = !attached
                    || self
                        .tree
                        .node(self.parent)
                        .matrix_world
                        .elements
                        .iter()
                        .zip(m.elements)
                        .any(|(a, b)| a.to_bits() != b.to_bits());
                if !attached {
                    self.tree.add_child(self.parent, self.root);
                }
                let parent = self.tree.node_mut(self.parent);
                parent.matrix = m;
                parent.matrix_world = m;
                different
            }
            None => {
                if attached {
                    self.tree.remove_from_parent(self.root);
                }
                attached
            }
        };
        changed
    }

    fn apply(&mut self, pose: &mut Pose) {
        // Pose.apply's untouched bones retain their previous transform.
        pose.apply_out();
        for i in 0..self.bones.len() {
            if pose.w[i] <= 1e-6 {
                continue;
            }
            let o = i * 7;
            let b = self.tree.node_mut(self.bones[i]);
            b.set_quaternion(
                pose.out[o],
                pose.out[o + 1],
                pose.out[o + 2],
                pose.out[o + 3],
            );
            b.position
                .set(pose.out[o + 4], pose.out[o + 5], pose.out[o + 6]);
        }
    }
}

/// Persistent SoldierAnimator state. Layers reserve both looping and once
/// forms of every clip, so even a zero-dt burst of gait changes cannot grow it.
pub struct Animator {
    pub time: f64,
    pub facing: f64,
    pub legs: f64,
    pub action: Action,
    pub action_time: f64,
    pub gait: usize,
    pub posture: usize,
    pub alert: f64,
    pub aim_yaw: f64,
    pub aim_pitch: f64,
    pub bank: usize,
    pub gunning: Option<Gunning>,
    action_clip: usize,
    turn: Option<usize>,
    turn_time: f64,
    turn_span: f64,
    airborne: bool,
    died: bool,
    jetted: bool,
    seated: bool,
    air_time: f64,
    dive_legs: f64,
    thrown_way: usize,
    upper_bank: usize,
    upper_blend: f64,
    upper_last: Vec<Quaternion>,
    upper_from: Vec<Quaternion>,
    layers: Vec<Layer>,
    weapon: Option<WeaponLayer>,
    pose: Pose,
    upper_pose: Pose,
    weapon_pose: Pose,
}

fn clamp(x: f64, a: f64, b: f64) -> f64 {
    max(a, min(b, x))
}
fn wrap(a: f64) -> f64 {
    atan2(sin(a), cos(a))
}
fn ease(share: f64, dt: f64) -> f64 {
    1.0 - pow(1.0 - share, dt * 30.0)
}
fn smoothstep(x: f64, lo: f64, hi: f64) -> f64 {
    if x <= lo {
        return 0.0;
    }
    if x >= hi {
        return 1.0;
    }
    let x = (x - lo) / (hi - lo);
    x * x * (3.0 - 2.0 * x)
}
fn scaled_rate(clips: &[DisplayClip], clip: usize, speed: f64) -> f64 {
    let authored = if clip == NONE { 0.0 } else { clips[clip].speed };
    clamp(
        ((1.0 - 0.05) * speed) / max(authored.abs(), 0.1) + 0.05,
        0.1,
        2.5,
    )
}
fn add(pose: &mut Pose, clips: &[DisplayClip], clip: usize, time: f64, weight: f64, looping: bool) {
    if clip != NONE && weight > 1e-4 {
        pose.add(&clips[clip].sample, time, weight, looping);
    }
}
fn first(a: usize, b: usize) -> usize {
    if a == NONE {
        b
    } else {
        a
    }
}

impl Animator {
    pub fn new(rig: &DisplayRig, clip_count: usize) -> Self {
        let n = rig.bones.len();
        let mut this = Self {
            time: 0.0,
            facing: 0.0,
            legs: 0.0,
            action: Action::Loco,
            action_time: 0.0,
            gait: IDLE,
            posture: STAND,
            alert: -1.0,
            aim_yaw: 0.0,
            aim_pitch: 0.0,
            bank: 0,
            gunning: None,
            action_clip: NONE,
            turn: None,
            turn_time: 0.0,
            turn_span: 0.0,
            airborne: false,
            died: false,
            jetted: false,
            seated: false,
            air_time: 0.0,
            dive_legs: 0.0,
            thrown_way: 1,
            upper_bank: 0,
            upper_blend: 1.0,
            upper_last: rig
                .upper
                .iter()
                .map(|&i| rig.tree.node(rig.bones[i]).quaternion)
                .collect(),
            upper_from: Vec::with_capacity(rig.upper.len()),
            layers: Vec::with_capacity(clip_count * 2),
            weapon: None,
            pose: Pose::new(n, rig.root_bone as i32),
            upper_pose: Pose::new(n, rig.root_bone as i32),
            weapon_pose: Pose::new(n, rig.root_bone as i32),
        };
        this.upper_from.extend_from_slice(&this.upper_last);
        this
    }

    /// Borrow the base blend's clip, seconds, weight and playback rate without
    /// advancing a clock or allocating an inspection copy.
    pub fn base_layers(&self) -> impl Iterator<Item = (usize, f64, f64, f64)> + '_ {
        self.layers
            .iter()
            .map(|layer| (layer.clip, layer.time, layer.weight, layer.rate))
    }

    /// SoldierAnimator.reset deliberately retains gunner and seated state.
    pub fn reset(&mut self, yaw: f64) {
        self.time = 0.0;
        self.facing = yaw;
        self.legs = 0.0;
        self.action = Action::Loco;
        self.action_time = 0.0;
        self.gait = IDLE;
        self.turn = None;
        self.turn_time = 0.0;
        self.turn_span = 0.0;
        self.posture = STAND;
        self.airborne = false;
        self.died = false;
        self.alert = -1.0;
        self.aim_yaw = 0.0;
        self.aim_pitch = 0.0;
        self.layers.clear();
        self.weapon = None;
        self.upper_bank = self.bank;
        self.upper_blend = 1.0;
    }

    /// SoldierAnimator.gunner. An explicit time loops; nine-pose samples clamp.
    /// SoldierAnimator.gunner. Explicit held time is distinct from a live
    /// nine-pose seat; retaining that distinction prevents a held body from
    /// advancing its clip while a creature carries it.
    pub fn gunner(&mut self, pose: Option<Gunning>) {
        self.gunning = pose.map(|mut p| {
            p.pitch = clamp(p.pitch, -1.0, 1.0);
            p.turn = clamp(p.turn, -1.0, 1.0);
            p
        });
    }
    /// SoldierAnimator.die; the host resolves direction and fallback once.
    /// SoldierAnimator.die marks death even when a class lacks the clip.
    /// A later thrown state can still take priority over that held death.
    pub fn die(&mut self, clip: usize) {
        if clip != NONE {
            self.start(Action::Dead, clip);
        }
        self.died = true;
        self.weapon = None;
    }
    /// SoldierAnimator.setBank; unknown names are resolved to rifle by the host.
    /// SoldierAnimator.setBank drops an active weapon layer on a bank
    /// change. Re-selecting the same bank must leave that layer alone.
    pub fn set_bank(&mut self, bank: usize) {
        if bank != self.bank {
            self.bank = bank;
            self.weapon = None;
        }
    }
    /// SoldierAnimator.weaponAction, with the selected clip already resolved.
    pub fn weapon_action(&mut self, action: u32, clip: usize, shooting: bool) {
        if clip == NONE {
            return;
        }
        let keep = self.weapon.map_or(0.0, |w| w.weight);
        self.weapon = Some(WeaponLayer {
            clip,
            action,
            time: 0.0,
            weight: keep,
            blend: if shooting { 0.02 } else { 0.5 },
            stopping: false,
        });
    }
    /// SoldierAnimator.endWeaponAction accepts any current action with None.
    pub fn end_weapon_action(&mut self, action: Option<u32>) {
        if let Some(w) = self.weapon.as_mut() {
            if action.is_none() || action == Some(w.action) {
                w.stopping = true;
            }
        }
    }
    /// SoldierAnimator.fire starts the onset only from a relaxed stance.
    pub fn fire(&mut self) {
        self.alert = if self.alert <= 0.0 { 2.5 } else { 2.0 };
    }
    /// SoldierAnimator.holdAlert sustains a charge without cutting short
    /// the initial alert blend, including a recovered network firing edge.
    pub fn hold_alert(&mut self) {
        self.alert = if self.alert <= 0.0 {
            2.5
        } else {
            max(self.alert, 2.0)
        };
    }

    /// SoldierAnimator.updatePose, including the world-space torso aim. A
    /// caller may accumulate dt for distant actors without advancing twice.
    pub fn update(
        &mut self,
        dt: f64,
        s: &SoldierState,
        rig: &mut DisplayRig,
        t: &Tables,
        clips: &[DisplayClip],
    ) {
        self.time += dt;
        self.action_time += dt;
        self.alert -= dt;
        let airborne = s.air != 0;
        let turned = wrap(s.yaw - self.facing);
        self.facing = s.yaw;
        self.legs = wrap(self.legs - turned);
        self.update_action(dt, s, airborne, t, clips);
        let sn = sin(self.facing);
        let cs = cos(self.facing);
        let left = s.velocity.x * cs - s.velocity.z * sn;
        let forward = s.velocity.x * sn + s.velocity.z * cs;
        match self.action {
            Action::Dive => {
                let entry = smoothstep(self.action_time, 0.0, 0.1);
                self.legs = self.dive_legs
                    + wrap(-s.dive * (core::f64::consts::PI / 2.0) - self.dive_legs) * entry;
                let phase = min(self.action_time / max(s.dive_time, 1e-3), 1.0);
                self.play(
                    self.action_clip,
                    dt,
                    Some(phase * clips[self.action_clip].duration),
                    1.0,
                    clips,
                );
            }
            Action::Jet | Action::Thrown => {
                self.legs += wrap(-self.legs) * ease(0.25, dt);
                self.play(
                    self.action_clip,
                    dt,
                    if self.action == Action::Thrown {
                        Some(self.action_time)
                    } else {
                        None
                    },
                    1.0,
                    clips,
                );
            }
            Action::Loco => {
                let (gait, legs, rate) = self.pick(left, forward, s.posture, t, clips);
                if gait == IDLE {
                    self.turn_in_place(dt, s.posture, t, clips);
                } else {
                    self.turn = None;
                    let step = wrap(legs - self.legs) * ease(0.25, dt);
                    self.legs += clamp(step, -8.0 * dt, 8.0 * dt);
                }
                self.gait = gait;
                if let Some(turn) = self.turn {
                    self.play(
                        self.clip_for(s.posture, turn, t),
                        dt,
                        Some(self.turn_time),
                        1.0,
                        clips,
                    );
                } else {
                    self.play(self.clip_for(s.posture, gait, t), dt, None, rate, clips);
                }
            }
            _ => self.play(self.action_clip, dt, Some(self.action_time), 1.0, clips),
        }
        self.pose.clear();
        if let Some(g) = self.gunning {
            if let Some(time) = g.time {
                add(&mut self.pose, clips, g.clip, time, 1.0, true);
            } else {
                let fps = clips[g.clip].sample.fps;
                add(
                    &mut self.pose,
                    clips,
                    g.clip,
                    (4.0 + g.turn) / fps,
                    1.0 - g.pitch.abs(),
                    false,
                );
                add(
                    &mut self.pose,
                    clips,
                    g.clip,
                    ((if g.pitch > 0.0 { 7.0 } else { 1.0 }) + g.turn) / fps,
                    g.pitch.abs(),
                    false,
                );
            }
            self.legs = 0.0;
        } else {
            for l in &self.layers {
                add(&mut self.pose, clips, l.clip, l.time, l.weight, l.looping);
            }
        }
        rig.apply(&mut self.pose);
        if self.gunning.is_some() {
            rig.tree.node_mut(rig.bones[rig.root_bone]).position = rig.root_bind;
        }
        let seat = self.gunning.and_then(|g| g.seat);
        if let Some(q) = seat {
            rig.tree.node_mut(rig.root).copy_quaternion(&q);
        } else if self.seated {
            rig.tree
                .node_mut(rig.root)
                .set_rotation(0.0, self.facing + self.legs, 0.0, None);
        } else {
            rig.tree
                .node_mut(rig.root)
                .set_rotation_y(self.facing + self.legs);
        }
        self.seated = seat.is_some();
        if self.gunning.is_none() {
            self.overlay_upper(dt, rig, t, clips);
            if self.action != Action::Dive {
                self.overlay_weapon(dt, rig, clips);
            }
        }
        for &(i, p) in &rig.bind_lengths {
            rig.tree.node_mut(rig.bones[i]).position = p;
        }
        if self.gunning.is_some() || self.action == Action::Dive {
            for (joint, &i) in rig.upper.iter().enumerate() {
                self.upper_last[joint] = rig.tree.node(rig.bones[i]).quaternion;
            }
            return;
        }
        self.aim_upper_body(dt, s, rig);
    }

    /// SoldierAnimator.overlayUpper retargets every fading leg layer. The
    /// upper fade restarts only for a bank change, never for a gait change.
    fn overlay_upper(&mut self, dt: f64, rig: &mut DisplayRig, t: &Tables, clips: &[DisplayClip]) {
        if self.upper_bank != self.bank {
            self.upper_bank = self.bank;
            self.upper_blend = 0.0;
            self.upper_from.copy_from_slice(&self.upper_last);
        }
        self.upper_blend = min(self.upper_blend + dt / 0.15, 1.0);
        self.upper_pose.clear();
        for l in &self.layers {
            let id = t.upper[self.bank][l.clip];
            let c = &clips[id];
            let phase = if clips[l.clip].duration > 0.0 {
                l.time / clips[l.clip].duration
            } else {
                0.0
            };
            add(
                &mut self.upper_pose,
                clips,
                id,
                if c.idle {
                    self.time
                } else {
                    phase * c.duration
                },
                l.weight,
                c.idle || l.looping,
            );
        }
        for (joint, &i) in rig.upper.iter().enumerate() {
            if self.upper_pose.w[i] <= 1e-6 {
                continue;
            }
            let p = &self.upper_pose.q[i * 4..];
            let mut q = Quaternion::new(p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64);
            q.normalize();
            let b = rig.tree.node_mut(rig.bones[i]);
            b.copy_quaternion(&self.upper_from[joint]);
            b.slerp_quaternion(&q, self.upper_blend);
            self.upper_last[joint] = b.quaternion;
        }
    }

    /// SoldierAnimator.overlayWeapon; standing idle replaces every bone but
    /// DummyRoot. Translation is blended before bind lengths are restored.
    fn overlay_weapon(&mut self, dt: f64, rig: &mut DisplayRig, clips: &[DisplayClip]) {
        let Some(mut w) = self.weapon else {
            return;
        };
        w.time += dt;
        let duration = clips[w.clip].duration;
        let ending = w.stopping || w.time >= duration;
        w.weight += (if ending { -dt } else { dt }) / max(if ending { 0.5 } else { w.blend }, 1e-3);
        w.weight = clamp(w.weight, 0.0, 1.0);
        if ending && w.weight <= 0.0 {
            self.weapon = None;
            return;
        }
        self.weapon = Some(w);
        self.weapon_pose.clear();
        add(
            &mut self.weapon_pose,
            clips,
            w.clip,
            min(w.time, duration),
            1.0,
            false,
        );
        let whole = self.action == Action::Loco
            && self.gait == IDLE
            && self.turn.is_none()
            && self.posture == STAND;
        let count = if whole {
            rig.bones.len()
        } else {
            rig.upper.len()
        };
        for at in 0..count {
            let i = if whole { at } else { rig.upper[at] };
            if whole && i == rig.root_bone {
                continue;
            }
            let weight = self.weapon_pose.w[i] as f64;
            if weight <= 1e-6 {
                continue;
            }
            let p = &self.weapon_pose.q[i * 4..];
            let mut q = Quaternion::new(p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64);
            q.normalize();
            let p = &self.weapon_pose.t[i * 3..];
            let p = Vector3::new(
                p[0] as f64 / weight,
                p[1] as f64 / weight,
                p[2] as f64 / weight,
            );
            let b = rig.tree.node_mut(rig.bones[i]);
            b.slerp_quaternion(&q, w.weight);
            b.position.lerp(&p, w.weight);
        }
    }

    /// SoldierAnimator.pick preserves both speed and heading hysteresis.
    /// SoldierAnimator.pick keeps separate start/stop and walk/run limits.
    /// Contact-speed noise must not repeatedly restart the authored cycle.
    fn pick(
        &self,
        left: f64,
        forward: f64,
        posture: usize,
        t: &Tables,
        clips: &[DisplayClip],
    ) -> (usize, f64, f64) {
        let speed = hypot(left, forward);
        let idle = if self.gait == IDLE { 0.3 } else { 0.18 };
        if speed < idle {
            return (IDLE, 0.0, 1.0);
        }
        let set = &t.sets[posture];
        let angle = atan2(left, forward);
        let alert = self.alert > 0.0;
        if set[WALK_LEFT] != NONE && set[WALK_RIGHT] != NONE {
            if left.abs() > forward.abs() {
                return if left > 0.0 {
                    (
                        WALK_LEFT,
                        wrap(angle - core::f64::consts::PI / 2.0),
                        scaled_rate(clips, set[WALK_LEFT], left),
                    )
                } else {
                    (
                        WALK_RIGHT,
                        wrap(angle + core::f64::consts::PI / 2.0),
                        scaled_rate(clips, set[WALK_RIGHT], -left),
                    )
                };
            }
            if forward > 0.0 || forward.abs() < left.abs() * 0.1 {
                return (WALK, angle, scaled_rate(clips, set[WALK], speed));
            }
            return (
                WALK_BACK,
                wrap(angle + core::f64::consts::PI),
                scaled_rate(clips, set[WALK_BACK], speed),
            );
        }
        let ft = wrap(angle - self.legs).abs();
        let bt = wrap(angle + core::f64::consts::PI - self.legs).abs();
        let going = self.gait == WALK || self.gait == RUN || (self.gait == IDLE && ft <= bt + 1e-5);
        let lo = if going {
            -1.8849558
        } else if alert {
            -1.8064158
        } else {
            -1.2566371
        };
        let hi = if going {
            1.8849558
        } else if alert {
            1.1780972
        } else {
            1.2566371
        };
        // Rounded angle threshold ported verbatim from SoldierAnimator.gaitFor.
        #[allow(clippy::approx_constant)]
        let ll = if alert { -0.7853982 } else { -1.2566371 };
        let lh = if alert { 0.5497787 } else { 1.2566371 };
        if angle > lo && angle < hi {
            let legs = clamp(angle, ll, lh);
            let run_at = if self.gait == RUN { 5.0 } else { 5.5 };
            if set[RUN] != NONE && speed > run_at {
                return (RUN, legs, scaled_rate(clips, set[RUN], speed));
            }
            return (WALK, legs, scaled_rate(clips, set[WALK], speed));
        }
        let legs = clamp(wrap(angle + core::f64::consts::PI), ll, lh);
        if set[RUN_BACK] != NONE {
            return (RUN_BACK, legs, scaled_rate(clips, set[RUN_BACK], speed));
        }
        (WALK_BACK, legs, scaled_rate(clips, set[WALK_BACK], speed))
    }

    /// SoldierAnimator.clipFor chooses alert rifle clips only where the
    /// standing bank supplies them. Other postures keep their normal cycle.
    fn clip_for(&self, posture: usize, gait: usize, t: &Tables) -> usize {
        let set = if self.alert > 0.0 && posture == STAND {
            &t.alert
        } else {
            &t.sets[posture]
        };
        first(set[gait], set[IDLE])
    }

    /// SoldierAnimator.turnInPlace: the turn's total angle is captured once,
    /// but its clip follows the current posture and alert table each frame.
    /// SoldierAnimator.turnInPlace overshoots the aim a little when a foot
    /// step ends. Snapping straight to zero would change the next turn edge.
    fn turn_in_place(&mut self, dt: f64, posture: usize, t: &Tables, clips: &[DisplayClip]) {
        if self.turn.is_none() {
            // Rounded posture turn thresholds ported verbatim from SoldierAnimator.turnInPlace.
            #[allow(clippy::approx_constant)]
            let (lo, hi) = match posture {
                0 => (-0.7068, 0.3927),
                1 => (-1.1781, 0.3927),
                _ => (-0.4712, 0.4712),
            };
            if self.legs > hi {
                self.turn = Some(TURN_RIGHT);
                self.turn_span = -self.legs - 0.2;
            } else if self.legs < lo {
                self.turn = Some(TURN_LEFT);
                self.turn_span = 0.2 - self.legs;
            } else {
                return;
            }
            self.turn_time = 0.0;
        }
        let c = self.clip_for(posture, self.turn.unwrap(), t);
        // SoldierAnimator.turnInPlace leaves a missing authored turn alone.
        // A turret can rotate a body whose clip bank contains only a 9-pose.
        let Some(clip) = clips.get(c) else {
            self.turn = None;
            return;
        };
        let duration = clip.duration;
        let step = min(dt, duration - self.turn_time);
        self.legs += (self.turn_span / duration) * max(step, 0.0);
        self.turn_time += dt;
        if self.turn_time >= duration {
            self.turn = None;
        }
    }

    /// SoldierAnimator.updateAction. Blast flight takes priority over death;
    /// a jet landing remains soft regardless of its total airborne time.
    fn update_action(
        &mut self,
        dt: f64,
        s: &SoldierState,
        airborne: bool,
        t: &Tables,
        clips: &[DisplayClip],
    ) {
        if s.air == 3 {
            if self.action != Action::Thrown {
                self.thrown_way = if s.velocity.x * sin(s.yaw) + s.velocity.z * cos(s.yaw) < 0.0 {
                    1
                } else {
                    0
                };
                self.start(Action::Thrown, first(t.thrown[self.thrown_way][0], t.fall));
                self.turn = None;
                self.weapon = None;
                self.jetted = false;
            }
            self.airborne = true;
            self.posture = s.posture;
            return;
        }
        if self.action == Action::Thrown {
            let c = t.thrown[self.thrown_way];
            if self.died {
                self.start(Action::ThrownRest, first(c[2], first(c[3], t.land_soft)));
            } else {
                self.start(Action::ThrownLand, first(c[1], t.land_soft));
            }
            self.airborne = false;
        }
        if self.action == Action::ThrownRest {
            let rest = t.thrown[self.thrown_way][3];
            if rest != NONE && self.action_clip != rest && self.action_time >= 0.25 {
                self.set_action(Action::ThrownRest, rest);
            }
            return;
        }
        if self.action == Action::Dead {
            return;
        }
        if s.dive != 0.0 && self.action != Action::Dive && t.dive != NONE {
            self.dive_legs = self.legs;
            self.start(Action::Dive, t.dive);
            self.turn = None;
            self.weapon = None;
        } else if s.dive == 0.0 && self.action == Action::Dive {
            self.action = Action::Loco;
            self.gait = WALK;
            self.turn = None;
        }
        if self.action == Action::Dive {
            self.posture = s.posture;
            self.airborne = airborne;
            return;
        }
        if airborne && !self.airborne {
            self.start(
                if s.air == 1 {
                    Action::Jump
                } else {
                    Action::Fall
                },
                if s.air == 1 { t.jump } else { t.fall },
            );
            self.air_time = 0.0;
            self.jetted = false;
        }
        if airborne {
            self.air_time += dt;
            if s.air == 4 && t.jet != NONE {
                if self.action != Action::Jet {
                    self.start(Action::Jet, t.jet);
                }
                self.jetted = true;
            } else if self.action == Action::Jet {
                self.start(Action::Fall, t.fall);
            }
            if self.action == Action::Jump && (self.action_time > 0.3 || s.air != 1) {
                self.set_action(Action::Fall, t.fall);
            }
        } else if self.airborne {
            if self.air_time > 2.0 && !self.jetted {
                self.start(Action::LandHard, t.land_hard);
            } else {
                self.start(Action::LandSoft, t.land_soft);
            }
        }
        self.airborne = airborne;
        if !airborne && s.posture != self.posture {
            let clip = t.transitions[self.posture][s.posture];
            if clip != NONE {
                self.start(Action::Transition, clip);
            }
            self.posture = s.posture;
        }
        if matches!(
            self.action,
            Action::LandSoft | Action::LandHard | Action::Transition | Action::ThrownLand
        ) {
            let moving = hypot(s.velocity.x, s.velocity.z) > 1.0;
            let bounce = self.action == Action::ThrownLand;
            let duration = if self.action_clip == NONE {
                0.0
            } else {
                clips[self.action_clip].duration
            };
            let done = self.action_time >= if bounce { 0.5 } else { duration };
            if done
                || ((self.action == Action::LandSoft || bounce)
                    && self.action_time > 0.15
                    && moving)
            {
                self.action = Action::Loco;
            }
        }
    }
    /// SoldierAnimator.start restarts the action clock. setAction below
    /// deliberately preserves it for jump/fall and thrown/rest continuation.
    pub fn start(&mut self, action: Action, clip: usize) {
        self.action = action;
        self.action_clip = clip;
        self.action_time = 0.0;
    }
    fn set_action(&mut self, action: Action, clip: usize) {
        self.action = action;
        self.action_clip = clip;
    }

    /// SoldierAnimator.play keeps fading layers in birth order and moves the
    /// selected one last, including when it reuses a fading reverse cycle.
    fn play(&mut self, clip: usize, dt: f64, at: Option<f64>, rate: f64, clips: &[DisplayClip]) {
        if clip == NONE {
            return;
        }
        let looping = at.is_none();
        let k = min(dt / 0.15, 1.0);
        let from = self.layers.last().copied();
        let from_index = self.layers.len().checked_sub(1);
        let current = self
            .layers
            .iter()
            .position(|l| l.clip == clip && l.looping == looping)
            .unwrap_or_else(|| {
                let at = self.layers.len();
                assert!(at < self.layers.capacity(), "animation layer capacity");
                self.layers.push(Layer {
                    clip,
                    time: 0.0,
                    weight: if at > 0 { 0.0 } else { 1.0 },
                    looping,
                    rate,
                });
                at
            });
        if let Some(f) = from {
            if Some(current) != from_index
                && looping
                && f.looping
                && clips[f.clip].sample.root_motion
                && clips[clip].sample.root_motion
                && clips[f.clip].duration > 0.0
            {
                self.layers[current].time =
                    (f.time / clips[f.clip].duration % 1.0) * clips[clip].duration;
            }
        }
        let mut selected = self.layers[current];
        selected.rate = rate;
        selected.time = if let Some(at) = at {
            at
        } else if clips[clip].idle {
            self.time
        } else {
            selected.time + dt * rate
        };
        let next = min(selected.weight + k, 1.0);
        let remaining = if selected.weight < 1.0 {
            (1.0 - next) / (1.0 - selected.weight)
        } else {
            0.0
        };
        selected.weight = next;
        let mut kept = 0;
        let mut weight = 0.0;
        for i in 0..self.layers.len() {
            if i == current {
                continue;
            }
            let mut l = self.layers[i];
            l.weight *= remaining;
            if l.looping {
                l.time += dt * l.rate;
            }
            if l.weight > 0.001 {
                self.layers[kept] = l;
                kept += 1;
                weight += l.weight;
            }
        }
        self.layers.truncate(kept);
        self.layers.push(selected);
        weight += selected.weight;
        for l in &mut self.layers {
            l.weight /= weight;
        }
    }

    /// SoldierAnimator.aimUpperBody. Parent quaternions come from the native
    /// hierarchy, including the root transform used for this displayed frame.
    fn aim_upper_body(&mut self, dt: f64, s: &SoldierState, rig: &mut DisplayRig) {
        let k = ease(0.15, dt);
        self.aim_yaw += (clamp(0.0, -0.6283185, 0.6283185) - self.aim_yaw) * k;
        let mut pitch = clamp(s.pitch, -1.0995574, 1.0995574);
        if s.posture == PRONE && pitch < 0.0 {
            pitch = 0.0;
        }
        self.aim_pitch += (pitch - self.aim_pitch) * k;
        let yaw = (self.aim_yaw - self.legs) / 8.0;
        let tilt = self.aim_pitch / 8.0;
        let t = self.alert;
        let mut mix = if t > -0.5 {
            if t > 2.0 {
                1.0 - 2.0 * (t - 2.0)
            } else if t > 0.0 {
                1.0
            } else {
                1.0 + 2.0 * t
            }
        } else {
            0.0
        };
        let prone = s.posture == PRONE;
        if prone {
            mix = 1.0;
        }
        if s.air == 4 {
            mix = 0.0;
        }
        let right = Vector3::new(-cos(self.facing), 0.0, sin(self.facing));
        if !prone && mix <= 0.5 {
            let w = 1.0 - 2.0 * mix;
            let bones = rig.head;
            let mut done = 0;
            for (i, share) in [2, 5].into_iter().enumerate() {
                self.rotate_world(
                    rig,
                    bones[i],
                    yaw * w * (share - done) as f64,
                    tilt * w * (share - done) as f64,
                    &right,
                );
                done = share;
            }
        } else {
            let w = if prone { 1.0 } else { 2.0 * mix - 1.0 };
            let bones = rig.spine;
            let mut done = 0;
            for (i, share) in [1, 4, 8].into_iter().enumerate() {
                self.rotate_world(
                    rig,
                    bones[i],
                    yaw * w * (share - done) as f64,
                    tilt * w * (share - done) as f64,
                    &right,
                );
                done = share;
            }
        }
    }

    /// SoldierAnimator.rotateWorld: parent^-1 * R * parent * local, yaw
    /// before tilt, with exactly one parent quaternion read for both turns.
    /// SoldierAnimator.rotateWorld resolves the current parent orientation
    /// before applying an aim delta, including a foreign scene parent.
    fn rotate_world(
        &self,
        rig: &mut DisplayRig,
        bone: Option<usize>,
        yaw: f64,
        tilt: f64,
        right: &Vector3,
    ) {
        let turn_yaw = yaw.abs() >= 1e-5;
        let turn_tilt = tilt.abs() >= 1e-5;
        let Some(i) = bone else {
            return;
        };
        if !(turn_yaw || turn_tilt) {
            return;
        }
        let node = rig.bones[i];
        let parent = rig.tree.node(node).parent.expect("bone parent");
        let mut pq = Quaternion::default();
        rig.tree.get_world_quaternion(parent, &mut pq);
        let mut inv = pq;
        inv.invert();
        for (active, axis, angle) in [
            (turn_yaw, Vector3::new(0.0, 1.0, 0.0), yaw),
            (turn_tilt, *right, tilt),
        ] {
            if !active {
                continue;
            }
            let mut twist = Quaternion::default();
            twist.set_from_axis_angle(&axis, angle);
            let mut delta = inv;
            delta.multiply(&twist).multiply(&pq);
            rig.tree.node_mut(node).with_quaternion(|q| {
                q.premultiply(&delta);
            });
        }
    }
}
