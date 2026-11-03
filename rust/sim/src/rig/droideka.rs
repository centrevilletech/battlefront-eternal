//! A droideka's hidden gun rig (src/droideka.js): the skeleton the world
//! fires its blasters from. `Droideka.arm` clones the body's bones into a
//! small rig of their own, hangs a copy of each blaster's barrel markers on
//! it, and every tick `sampleMuzzles` poses it with a DroidekaAnimator of its
//! own (the mode's clip, the legs' lag at DummyRoot and up the spine, the aim
//! pitch on the spine, the folded shell's roll) at the droideka's place and
//! heading; `gunAt` reads a barrel's place off it.
//!
//! Every quaternion here is written as three writes it, so each bone's
//! Euler angles would come out as three's (nothing reads them). One line of
//! the JavaScript does not do what it says: the unfold's blend out of the
//! ball (`bone.quaternion.slerpQuaternions(from.q, bone.quaternion, blend)`)
//! copies `from.q` over the quaternion it then slerps toward, so for the
//! blend's 0.16 s every bone keeps the turn it had as the unfold began
//! while its position blends. It is ported as it is (quirks register).

use super::skeleton::Skeleton;
use three::{smoothstep, Quaternion, Vector3};
use three::{NodeId, TransformNode, TransformTree};
use crate::collision::{js_max, js_min};
use crate::pose::{Clip, Pose};

// Modes (src/droideka.js): the clip each plays, by mode.
pub const CLIPS: [Option<&str>; 14] = [
    Some("idle"), Some("turnl"), Some("turnr"), Some("death01"), None, Some("idle_to_leftfoot"), None,
    Some("walk_leftfoot_rightfoot"), Some("walk_rightfoot_leftfoot"), Some("leftfoot_to_idle"), Some("rightfoot_to_idle"),
    Some("fold"), Some("roll"), Some("unfold"),
];
pub const FOLD: i32 = 11;
pub const BALL: i32 = 12;
pub const UNFOLD: i32 = 13;
pub const DEATH: i32 = 3;
pub const DEAD: i32 = 4;
/// m, the ball's centre above the origin (BallRadius's default).
pub const BALL_CENTER: f64 = 0.65;
const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const AXIS_X: Vector3 = Vector3::new(1.0, 0.0, 0.0);
const AXIS_Z: Vector3 = Vector3::new(0.0, 0.0, 1.0);

/// A droideka's clip bank as its modes reach it: `bank[mode]`, the clip
/// `clips.get(CLIPS[mode])` finds (None where the mode has none or the bank
/// lacks it).
pub struct DroidekaClips<'c> {
    pub by_mode: [Option<&'c Clip>; 14],
}

impl<'c> DroidekaClips<'c> {
    /// `clip(mode)`.
    pub fn clip(&self, mode: i32) -> Option<&'c Clip> {
        if (0..14).contains(&mode) {
            self.by_mode[mode as usize]
        } else {
            None
        }
    }

    /// `length(mode)`: the clip's frames at 30 fps, sped up by
    /// AnimationSpeed; 0 without a clip.
    pub fn length(&self, mode: i32, speed: f64) -> f64 {
        match self.clip(mode) {
            Some(clip) => clip.frames as f64 / 30.0 / speed,
            None => 0.0,
        }
    }
}

/// What the animator reads of the droideka, as it stands when
/// `sampleMuzzles` runs.
#[derive(Clone, Copy, Debug, Default)]
pub struct DroidekaState {
    pub mode: i32,
    /// s into the mode's clip.
    pub time: f64,
    pub dead: bool,
    /// How far the legs trail the body, rad.
    pub legs: f64,
    pub aim_pitch: f64,
    pub aim_yaw: f64,
    /// The ball's turn about its axle.
    pub roll: f64,
    pub lean: f64,
    /// AnimationSpeed.
    pub speed: f64,
    pub position: Vector3,
}

/// A barrel of a blaster, as the body carries it: a marker at a tip found
/// on the arm's mesh (`<gun>_barrel_<i>`, hung on the gun's parent bone), or
/// the gun's own bone where no tip was found.
#[derive(Clone, Copy, Debug)]
pub enum Barrel {
    Marker(Vector3),
    GunBone,
}

/// A blaster's gun: its bone (an aimer's hardpoint) and its barrels.
#[derive(Clone, Debug)]
pub struct Gun {
    pub bone: usize,
    pub barrels: Vec<Barrel>,
}

/// The animator's state (DroidekaAnimator's fields the update reads and
/// writes).
pub struct DroidekaAnimator {
    pub root: i32,
    pub ball: i32,
    pub spine: Vec<usize>,
    pub pose: Pose,
    /// Each bone's bind quaternion and position (`bind`).
    pub bind_q: Vec<Quaternion>,
    pub bind_p: Vec<Vector3>,
    pub facing: f64,
    pub legs: f64,
    pub mode: i32,
    pub went_up_rolled: bool,
    pub roll_origin: f64,
    pub transition: f64,
    /// The pose the unfold's blend starts from (`from`).
    pub from_q: Vec<Quaternion>,
    pub from_p: Vec<Vector3>,
    pub pivot: Vector3,
    pub roll_center: Vector3,
}

/// The hidden rig: the tree (its root, a clone of each bone, a clone of each
/// barrel) and the animator that poses it.
pub struct DroidekaRig {
    pub tree: TransformTree,
    pub root: NodeId,
    /// The bone clones, by bone index.
    pub bones: Vec<NodeId>,
    /// The barrel clones (`physicalBarrels`), by gun and barrel.
    pub barrels: Vec<Vec<NodeId>>,
    pub animator: DroidekaAnimator,
}

impl DroidekaRig {
    /// `Droideka.arm`'s rig, before its first `sampleMuzzles(0)`: the body's
    /// skeleton (its bones' bind pose, `Skeleton::actor`), the guns with
    /// their barrels, and the visible animator's roll centre (found from the
    /// body's mesh in the roll pose).
    pub fn new(skeleton: &Skeleton, guns: &[Gun], roll_center: Vector3) -> DroidekaRig {
        let mut tree = TransformTree::new();
        let root = tree.add_node(TransformNode::default());
        // Each bone cloned (bone.clone(false)): its place, turn and scale,
        // the copy's quaternion callback firing, as a new Bone's is on. The
        // matrices a clone copies are not kept: arm's sampleMuzzles(0) makes
        // every one afresh before anything reads them.
        let mut bones = Vec::with_capacity(skeleton.len());
        for b in &skeleton.bind {
            let mut copy = TransformNode::placed(b.position, b.scale);
            copy.copy_quaternion(&b.quaternion);
            bones.push(tree.add_node(copy));
        }
        for i in 0..bones.len() {
            let parent = skeleton.parents[i];
            let up = if parent < 0 { root } else { bones[parent as usize] };
            tree.add_child(up, bones[i]);
        }
        // The barrels' copies, each under the bone its original hangs from
        // (a marker on the gun's parent; the gun bone itself from there too).
        let mut barrels = Vec::with_capacity(guns.len());
        for gun in guns {
            let parent = skeleton.parents[gun.bone];
            let up = if parent < 0 { root } else { bones[parent as usize] };
            let mut list = Vec::with_capacity(gun.barrels.len());
            for barrel in &gun.barrels {
                let node = match barrel {
                    // A marker: a new Object3D at the tip, cloned.
                    Barrel::Marker(tip) => {
                        let mut copy = TransformNode::placed(*tip, Vector3::new(1.0, 1.0, 1.0));
                        copy.copy_quaternion(&Quaternion::default());
                        copy
                    }
                    Barrel::GunBone => {
                        let node = &skeleton.bind[gun.bone];
                        let mut copy = TransformNode::placed(node.position, node.scale);
                        copy.copy_quaternion(&node.quaternion);
                        copy
                    }
                };
                let id = tree.add_node(node);
                tree.add_child(up, id);
                list.push(id);
            }
            barrels.push(list);
        }
        let n = skeleton.len();
        let index = |name: &str| skeleton.find_lower(name).map_or(-1, |i| i as i32);
        let root_bone = index("dummyroot");
        let ball = index("center_box");
        let spine = ["bone_spine1", "bone_spine2", "bone_spine3"].iter().map(|n| index(n)).filter(|&i| i >= 0).map(|i| i as usize).collect();
        let bind_q: Vec<Quaternion> = (0..n).map(|i| skeleton.bind[i].quaternion).collect();
        let bind_p: Vec<Vector3> = (0..n).map(|i| skeleton.bind[i].position).collect();
        // Without a skinned body (the hidden rig has none) the pivot is the
        // ball bone's bind place; the roll centre is then copied over from
        // the visible animator's.
        let pivot = if ball >= 0 { bind_p[ball as usize] } else { Vector3::new(0.0, BALL_CENTER, 0.0) };
        let animator = DroidekaAnimator {
            root: root_bone,
            ball,
            spine,
            pose: Pose::new(n, root_bone),
            from_q: bind_q.clone(),
            from_p: bind_p.clone(),
            bind_q,
            bind_p,
            facing: 0.0,
            legs: 0.0,
            mode: -1,
            went_up_rolled: false,
            roll_origin: 0.0,
            transition: 0.0,
            pivot,
            roll_center,
        };
        DroidekaRig { tree, root, bones, barrels, animator }
    }

    /// `muzzleAnimator.reset(yaw)` (Droideka.spawn): `roll` is the
    /// droideka's then.
    pub fn reset(&mut self, yaw: f64, roll: f64) {
        let a = &mut self.animator;
        a.facing = yaw;
        a.mode = -1;
        a.went_up_rolled = false;
        a.transition = 0.0;
        a.roll_origin = roll;
    }

    /// `sampleMuzzles(dt)`: the rig at the droideka's place, posed, its
    /// world matrices made.
    pub fn sample_muzzles(&mut self, dt: f64, d: &DroidekaState, clips: &DroidekaClips) {
        self.tree.node_mut(self.root).position.copy(&d.position);
        self.update(dt, d, clips);
        self.tree.update_matrix_world(self.root, true);
    }

    /// DroidekaAnimator.update(dt, droideka) on the hidden rig.
    pub fn update(&mut self, dt: f64, d: &DroidekaState, clips: &DroidekaClips) {
        let a = &mut self.animator;
        let tree = &mut self.tree;
        let bones = &self.bones;
        if d.mode != a.mode {
            if d.mode == BALL {
                a.roll_origin = d.roll;
            }
            if d.mode == DEAD {
                a.went_up_rolled = a.mode >= FOLD;
            }
            // Unfold starts from a fixed authored orientation; the last
            // displayed pose is kept instead of snapping out of a live spin.
            a.transition = if d.mode == UNFOLD && a.mode == BALL { 0.16 } else { 0.0 };
            if a.transition != 0.0 {
                for (i, &b) in bones.iter().enumerate() {
                    a.from_q[i].copy(&tree.node(b).quaternion);
                    a.from_p[i].copy(&tree.node(b).position);
                }
            }
            a.mode = d.mode;
        }
        // Gone up (dead, which has no clip): the pose it went up in.
        let mode = if d.mode == DEAD { if a.went_up_rolled { BALL } else { DEATH } } else { d.mode };
        let clip = clips.clip(mode);
        let length = clips.length(d.mode, d.speed);
        let pose = &mut a.pose;
        pose.clear();
        if let Some(clip) = clip {
            let t = if d.mode == DEAD {
                clips.length(DEATH, d.speed) * d.speed
            } else {
                js_min(js_max(d.time, 0.0), length) * d.speed
            };
            // Pose.add's own guard passes: the weight is 1.
            pose.add(clip, if mode == BALL { 0.0 } else { t }, 1.0, false);
        }
        for (i, &b) in bones.iter().enumerate() {
            if pose.w[i] as f64 > 1e-6 {
                continue;
            }
            let node = tree.node_mut(b);
            node.copy_quaternion(&a.bind_q[i]);
            node.position.copy(&a.bind_p[i]);
        }
        // Pose.apply.
        for (i, &b) in bones.iter().enumerate() {
            let w = pose.w[i] as f64;
            if w <= 1e-6 {
                continue;
            }
            let (qi, ti) = (i * 4, i * 3);
            let node = tree.node_mut(b);
            node.with_quaternion(|q| {
                q.set(pose.q[qi] as f64, pose.q[qi + 1] as f64, pose.q[qi + 2] as f64, pose.q[qi + 3] as f64).normalize();
            });
            node.position.set(pose.t[ti] as f64 / w, pose.t[ti + 1] as f64 / w, pose.t[ti + 2] as f64 / w);
        }
        // The legs trail the body: DummyRoot turned by the lag, the spine
        // twisted back a third of it at each of its three bones.
        let share = |from: f64, to: f64| smoothstep(d.time / js_max(length, 1e-3), from, to);
        let folded = if mode == BALL {
            1.0
        } else if mode == FOLD {
            share(0.35, 1.0)
        } else if mode == UNFOLD {
            1.0 - share(0.0, 0.3)
        } else {
            0.0
        };
        let lag = d.legs * (1.0 - folded);
        let mut q = Quaternion::default();
        if a.root >= 0 {
            q.set_from_axis_angle(&UP, lag);
            tree.node_mut(bones[a.root as usize]).with_quaternion(|bq| {
                bq.premultiply(&q);
            });
        }
        for &i in &a.spine {
            q.set_from_axis_angle(&AXIS_X, -lag / 3.0);
            tree.node_mut(bones[i]).with_quaternion(|bq| {
                bq.multiply(&q);
            });
        }
        // The blasters and head follow the aim's pitch together; the feet
        // keep the walking pose. The correction fades at the folding clips'
        // ends.
        let aiming = if d.dead || mode == BALL {
            0.0
        } else if mode == FOLD {
            1.0 - share(0.0, 0.25)
        } else if mode == UNFOLD {
            share(0.7, 1.0)
        } else {
            1.0
        };
        if !a.spine.is_empty() && aiming != 0.0 && !aiming.is_nan() {
            q.set_from_axis_angle(&AXIS_X, -d.aim_pitch * aiming);
            tree.node_mut(bones[a.spine[0]]).with_quaternion(|bq| {
                bq.premultiply(&q);
            });
        }
        if a.ball >= 0 && mode >= FOLD {
            let ball = tree.node_mut(bones[a.ball as usize]);
            // The folded shell brought above the contact point gradually,
            // so the fold's end and the first rolling frame agree.
            let mut offset = Vector3::default();
            offset.sub_vectors(&a.roll_center, &a.pivot).multiply_scalar(folded);
            if mode == BALL {
                let mut spin = Quaternion::default();
                spin.set_from_axis_angle(&AXIS_Z, d.lean);
                q.set_from_axis_angle(&AXIS_X, d.roll - a.roll_origin);
                spin.multiply(&q);
                ball.position.sub(&a.pivot).apply_quaternion(&spin).add(&a.pivot);
                ball.with_quaternion(|bq| {
                    bq.premultiply(&spin);
                });
            }
            ball.position.add(&offset);
        }
        if a.transition > 0.0 {
            a.transition = js_max(0.0, a.transition - dt);
            let blend = smoothstep(1.0 - a.transition / 0.16, 0.0, 1.0);
            for (i, &b) in bones.iter().enumerate() {
                let node = tree.node_mut(b);
                // slerpQuaternions(from.q, bone.quaternion, blend): its copy
                // of from.q is also the slerp's target (the same object).
                node.copy_quaternion(&a.from_q[i]);
                let target = node.quaternion;
                node.slerp_quaternion(&target, blend);
                // lerpVectors(from.p, bone.position, blend) reads each part
                // of the position before it writes it.
                let p = node.position;
                node.position.lerp_vectors(&a.from_p[i], &p, blend);
            }
        }
        a.facing = d.aim_yaw;
        a.legs = d.legs;
        tree.node_mut(self.root).set_rotation(0.0, d.aim_yaw, 0.0, None);
    }

    /// `gunAt(out, { index, barrel })`: the barrel's place in the world
    /// (the world matrices refreshed from the rig's root down to it), or
    /// None where the droideka has no such barrel.
    pub fn gun_at(&mut self, index: usize, barrel: usize) -> Option<Vector3> {
        let node = *self.barrels.get(index)?.get(barrel)?;
        self.tree.update_world_matrix(node, true, false);
        let mut out = Vector3::default();
        out.set_from_matrix_position(&self.tree.node(node).matrix_world);
        Some(out)
    }
}
