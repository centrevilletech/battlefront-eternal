//! A droideka's hidden gun rig: the skeleton the world
//! fires its blasters from. `Droideka.arm` clones the body's bones into a
//! small rig of their own, hangs a copy of each blaster's barrel markers on
//! it, and every tick `sampleMuzzles` poses it with a DroidekaAnimator of its
//! own (the mode's clip, the legs' lag at DummyRoot and up the spine, the aim
//! pitch on the spine, the folded shell's roll) at the droideka's place and
//! heading; `gunAt` reads a barrel's place off it.
//!
//! Every quaternion here is written as three writes it, so each bone's
//! Euler angles would come out as three's (nothing reads them).
//!
//! As in the 2004 game, a mode's clip spreads its frames over the whole of
//! the mode (its last frame shows as the mode ends) and nothing blends one
//! clip into the next: they are made to meet. Two changes do not meet, and
//! here (looks only; the game snaps) they are eased over `BLEND`: the pose
//! drawn as the fold begins turns into the fold's clip, and as the unfold
//! begins the shell, rolled to any angle in the ball (and the unfold's first
//! frame has it turned a further 138 degrees), turns about its axle the
//! short way round from where it was drawn into the unfold. While it does,
//! `unfold_turn` holds that turn and the shell bone's `from` place the
//! distance it had left to go.

use super::math;
use super::skeleton::Skeleton;
use crate::{js_max, js_min};
use crate::{Clip, Pose};
use three::{smoothstep, Box3, Matrix4, Quaternion, Vector3};
use three::{NodeId, TransformNode, TransformTree};
use v8math::hypot as js_hypot2;

// Modes: the clip each plays, by mode.
pub const CLIPS: [Option<&str>; 14] = [
    Some("idle"),
    Some("turnl"),
    Some("turnr"),
    Some("death01"),
    None,
    Some("idle_to_leftfoot"),
    None,
    Some("walk_leftfoot_rightfoot"),
    Some("walk_rightfoot_leftfoot"),
    Some("leftfoot_to_idle"),
    Some("rightfoot_to_idle"),
    Some("fold"),
    Some("roll"),
    Some("unfold"),
];
pub const FOLD: i32 = 11;
pub const BALL: i32 = 12;
pub const UNFOLD: i32 = 13;
pub const DEATH: i32 = 3;
pub const DEAD: i32 = 4;
/// m, the ball's centre above the origin (BallRadius's default).
pub const BALL_CENTER: f64 = 0.65;
/// s, the drawn pose's turn into the fold's and the unfold's first frame.
pub const BLEND: f64 = 0.25;
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

/// The time on a clip's own clock (what `Pose::add` takes) at `time` into a
/// mode `length` long: the 2004 game spreads the clip's frames over the
/// whole mode, from the first at its start to the last at its end.
pub fn clip_time(clip: &Clip, time: f64, length: f64) -> f64 {
    if length <= 0.0 || clip.fps <= 0.0 {
        return 0.0;
    }
    js_min(js_max(time, 0.0), length) / length * clip.frames.saturating_sub(1) as f64 / clip.fps
}

/// A droideka's clip a moment between two samples of it, `a` the older and
/// `b` the newer, each (mode, time into its clip, played backward), the
/// moment `elapsed` seconds after `a` and `left` before `b`. Within one run
/// of a clip the time goes from the one to the other. Where the mode changed
/// (or its clip started again) the old clip runs on until the new one began
/// (`b`'s time counted back), so a droideka drawn between samples neither
/// stands still nor skips where one clip hands over to the next.
pub fn between(a: (i32, f64, bool), b: (i32, f64, bool), elapsed: f64, left: f64) -> (i32, f64, bool) {
    let ((am, at, ar), (bm, bt, br)) = (a, b);
    let restarted = if br { bt > at } else { bt < at };
    if am == bm && !restarted {
        let span = elapsed + left;
        let f = if span > 0.0 { elapsed / span } else { 1.0 };
        return (bm, at + (bt - at) * f, br);
    }
    if !br {
        if bt - left >= 0.0 {
            return (bm, bt - left, false);
        }
        return (am, if ar { js_max(at - elapsed, 0.0) } else { at + elapsed }, ar);
    }
    if ar && at - elapsed >= 0.0 {
        return (am, at - elapsed, true);
    }
    (bm, bt + left, true)
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
    /// rad, the shell's turn about its axle still to undo as an unfold
    /// begins (kept apart from `roll_origin`, which a droideka killed while
    /// unfolding still needs for its rolled corpse).
    pub unfold_turn: f64,
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
            let up = if parent < 0 {
                root
            } else {
                bones[parent as usize]
            };
            tree.add_child(up, bones[i]);
        }
        // The barrels' copies, each under the bone its original hangs from
        // (a marker on the gun's parent; the gun bone itself from there too).
        let mut barrels = Vec::with_capacity(guns.len());
        for gun in guns {
            let parent = skeleton.parents[gun.bone];
            let up = if parent < 0 {
                root
            } else {
                bones[parent as usize]
            };
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
        let spine = ["bone_spine1", "bone_spine2", "bone_spine3"]
            .iter()
            .map(|n| index(n))
            .filter(|&i| i >= 0)
            .map(|i| i as usize)
            .collect();
        let bind_q: Vec<Quaternion> = (0..n).map(|i| skeleton.bind[i].quaternion).collect();
        let bind_p: Vec<Vector3> = (0..n).map(|i| skeleton.bind[i].position).collect();
        // Without a skinned body (the hidden rig has none) the pivot is the
        // ball bone's bind place; the roll centre is then copied over from
        // the visible animator's.
        let pivot = if ball >= 0 {
            bind_p[ball as usize]
        } else {
            Vector3::new(0.0, BALL_CENTER, 0.0)
        };
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
            unfold_turn: 0.0,
        };
        DroidekaRig {
            tree,
            root,
            bones,
            barrels,
            animator,
        }
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
        update_display(
            &mut self.animator,
            &mut self.tree,
            &self.bones,
            self.root,
            dt,
            d,
            clips,
        );
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

/// A droideka's drawn body as its animator's constructor skins it: each
/// vertex's place (three f32s a vertex), each vertex's bone (the first of
/// its four skin indices; that one's weight is 1, the others' 0), the body's
/// bind matrix and the skeleton's bone inverses.
pub struct SkinnedBody<'a> {
    pub positions: &'a [f32],
    pub skin_index: &'a [u16],
    pub bind_matrix: Matrix4,
    pub bone_inverses: &'a [Matrix4],
}

/// The drawn DroidekaAnimator's roll axle (its constructor):
/// `center_box` is an animation control, not the folded shell's axle, so the
/// axle is found from the skinned body in the roll clip's first pose, in the
/// ball bone's parent's frame: the pivot (the middle of the silhouette's
/// extent averaged over 32 axle angles) and the roll centre (as high as the
/// farthest vertex from the axle, at least BallRadius). The hidden gun rig
/// copies the roll centre (`DroidekaRig::new`). Without a ball bone or a
/// roll clip, the pivot is the ball bone's bind place (or 0.65 m up) and the
/// centre BallRadius up.
pub fn roll_axle(
    skeleton: &Skeleton,
    body: &SkinnedBody,
    roll: Option<&Clip>,
    ball_radius: f64,
) -> (Vector3, Vector3) {
    let index = |name: &str| skeleton.find_lower(name).map_or(-1, |i| i as i32);
    let (root_bone, ball) = (index("dummyroot"), index("center_box"));
    let mut pivot = if ball >= 0 {
        skeleton.bind[ball as usize].position
    } else {
        Vector3::new(0.0, BALL_CENTER, 0.0)
    };
    let mut roll_center = Vector3::new(0.0, ball_radius, 0.0);
    let Some(roll) = roll else {
        return (pivot, roll_center);
    };
    if ball < 0 {
        return (pivot, roll_center);
    }
    let n = skeleton.len();
    let mut pose = Pose::new(n, root_bone);
    pose.add(roll, 0.0, 1.0, false);
    // The drawn actor as buildActor hangs it: the root, the body's mesh on
    // it, bone 0 on the mesh and every other bone on its parent (one with
    // none on nothing, never updated).
    let mut tree = TransformTree::new();
    let root = tree.add_node(TransformNode::default());
    let mesh = tree.add_node(TransformNode::default());
    tree.add_child(root, mesh);
    let bones: Vec<NodeId> = skeleton
        .bind
        .iter()
        .map(|b| tree.add_node(b.clone()))
        .collect();
    for i in 0..n {
        let parent = skeleton.parents[i];
        if i == 0 {
            tree.add_child(mesh, bones[0]);
        } else if parent >= 0 {
            tree.add_child(bones[parent as usize], bones[i]);
        }
    }
    // Pose.apply.
    for (i, &b) in bones.iter().enumerate() {
        let w = pose.w[i] as f64;
        if w <= 1e-6 {
            continue;
        }
        let node = tree.node_mut(b);
        node.with_quaternion(|q| {
            q.set(
                pose.q[i * 4] as f64,
                pose.q[i * 4 + 1] as f64,
                pose.q[i * 4 + 2] as f64,
                pose.q[i * 4 + 3] as f64,
            )
            .normalize();
        });
        node.position.set(
            pose.t[i * 3] as f64 / w,
            pose.t[i * 3 + 1] as f64 / w,
            pose.t[i * 3 + 2] as f64 / w,
        );
    }
    tree.update_matrix_world(root, true);
    let ball_parent = tree
        .node(bones[ball as usize])
        .parent
        .expect("the ball bone hangs from something");
    let mut parent_inverse = tree.node(ball_parent).matrix_world;
    parent_inverse.invert();
    let mut to_parent = Matrix4::default();
    to_parent.multiply_matrices(&parent_inverse, &tree.node(mesh).matrix_world);
    let mut bind_inverse = body.bind_matrix;
    bind_inverse.invert();
    let count = body.positions.len() / 3;
    let mut bounds = Box3::default();
    let mut points = Vec::with_capacity(count);
    let mut bone_matrix = Matrix4::default();
    for i in 0..count {
        // getVertexPosition: the mesh's place, then applyBoneTransform.
        let mut base = Vector3::new(
            body.positions[i * 3] as f64,
            body.positions[i * 3 + 1] as f64,
            body.positions[i * 3 + 2] as f64,
        );
        base.apply_matrix4(&body.bind_matrix);
        let mut v = Vector3::new(0.0, 0.0, 0.0);
        let bone = body.skin_index[i * 4] as usize;
        bone_matrix.multiply_matrices(
            &tree.node(bones[bone]).matrix_world,
            &body.bone_inverses[bone],
        );
        let mut skinned = base;
        skinned.apply_matrix4(&bone_matrix);
        v.add_scaled_vector(&skinned, 1.0);
        v.apply_matrix4(&bind_inverse);
        v.apply_matrix4(&to_parent);
        bounds.expand_by_point(&v);
        points.push(v);
    }
    if count > 0 {
        bounds.get_center(&mut pivot);
        pivot.y = 0.0;
        pivot.z = 0.0;
        // Averaged over axle angles: one box favours whichever foot points
        // up in the first frame.
        for k in 0..32 {
            let a = k as f64 * std::f64::consts::PI * 2.0 / 32.0;
            let (y, z) = (math::cos(a), math::sin(a));
            let (mut min, mut max) = (f64::INFINITY, f64::NEG_INFINITY);
            for p in &points {
                let dot = p.y * y + p.z * z;
                min = js_min(min, dot);
                max = js_max(max, dot);
            }
            pivot.y += (min + max) * y / 32.0;
            pivot.z += (min + max) * z / 32.0;
        }
        let mut radius = 0.0;
        for p in &points {
            radius = js_max(radius, js_hypot2(p.y - pivot.y, p.z - pivot.z));
        }
        roll_center.y = js_max(ball_radius, radius);
    }
    (pivot, roll_center)
}

/// DroidekaAnimator.update on either the visible actor or the hidden world
/// rig. Both hosts share the same pose and fold/unfold state.
pub fn update_display(
    a: &mut DroidekaAnimator,
    tree: &mut TransformTree,
    bones: &[NodeId],
    root: NodeId,
    dt: f64,
    d: &DroidekaState,
    clips: &DroidekaClips,
) {
    let mut unfolding = false;
    if d.mode != a.mode {
        unfolding = d.mode == UNFOLD && a.mode == BALL;
        if d.mode == BALL {
            a.roll_origin = d.roll;
        }
        if d.mode == DEAD {
            a.went_up_rolled = a.mode >= FOLD;
        }
        // The fold starts from its own standing frame: the last drawn pose
        // turns into it. The unfold starts from the shell's authored angle.
        let folding = d.mode == FOLD && a.mode >= 0 && a.mode < FOLD && a.mode != DEATH && a.mode != DEAD;
        a.transition = if folding || unfolding { BLEND } else { 0.0 };
        if folding || unfolding {
            for (i, &b) in bones.iter().enumerate() {
                a.from_q[i].copy(&tree.node(b).quaternion);
                a.from_p[i].copy(&tree.node(b).position);
            }
        }
        a.mode = d.mode;
    }
    // Gone up (dead, which has no clip): the pose it went up in.
    let mode = if d.mode == DEAD {
        if a.went_up_rolled {
            BALL
        } else {
            DEATH
        }
    } else {
        d.mode
    };
    let clip = clips.clip(mode);
    let length = clips.length(d.mode, d.speed);
    let pose = &mut a.pose;
    pose.clear();
    if let Some(clip) = clip {
        let t = if d.mode == DEAD {
            clips.length(DEATH, d.speed) * d.speed
        } else {
            clip_time(clip, d.time, length)
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
            q.set(
                pose.q[qi] as f64,
                pose.q[qi + 1] as f64,
                pose.q[qi + 2] as f64,
                pose.q[qi + 3] as f64,
            )
            .normalize();
        });
        node.position.set(
            pose.t[ti] as f64 / w,
            pose.t[ti + 1] as f64 / w,
            pose.t[ti + 2] as f64 / w,
        );
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
    // 0 as a change that does not meet happens, 1 once it has been eased.
    a.transition = js_max(0.0, a.transition - dt);
    let blend = smoothstep(1.0 - a.transition / BLEND, 0.0, 1.0);
    if a.ball >= 0 && mode >= FOLD {
        let ball = tree.node_mut(bones[a.ball as usize]);
        // The folded shell brought above the contact point gradually,
        // so the fold's end and the first rolling frame agree.
        let mut offset = Vector3::default();
        offset
            .sub_vectors(&a.roll_center, &a.pivot)
            .multiply_scalar(folded);
        let k = a.ball as usize;
        if unfolding {
            // The turn about the axle from the unfold's first pose to the
            // shell as drawn (its lean is about another axis), the short way.
            let mut turn = ball.quaternion;
            turn.invert().premultiply(&a.from_q[k]);
            let tau = std::f64::consts::TAU;
            let angle = 2.0 * v8math::atan2(turn.x, turn.w);
            a.unfold_turn = angle - tau * ((angle + tau / 2.0) / tau).floor();
        }
        // Rolled, the shell turns and leans about its axle; unfolding, it
        // turns from where it was drawn into the clip.
        let turn = if mode == BALL {
            Some((d.lean, d.roll - a.roll_origin))
        } else if mode == UNFOLD && a.transition > 0.0 {
            Some((d.lean * (1.0 - blend), a.unfold_turn * (1.0 - blend)))
        } else {
            None
        };
        if let Some((lean, roll)) = turn {
            let mut spin = Quaternion::default();
            spin.set_from_axis_angle(&AXIS_Z, lean);
            q.set_from_axis_angle(&AXIS_X, roll);
            spin.multiply(&q);
            ball.position
                .sub(&a.pivot)
                .apply_quaternion(&spin)
                .add(&a.pivot);
            ball.with_quaternion(|bq| {
                bq.premultiply(&spin);
            });
        }
        ball.position.add(&offset);
        if mode == UNFOLD && a.transition > 0.0 {
            if unfolding {
                let mut left = a.from_p[k];
                left.sub(&ball.position);
                a.from_p[k] = left;
            }
            ball.position.add_scaled_vector(&a.from_p[k], 1.0 - blend);
        }
    }
    if mode == FOLD && a.transition > 0.0 {
        for (i, &b) in bones.iter().enumerate() {
            let node = tree.node_mut(b);
            // From the pose drawn as the mode changed to this frame's.
            let target = node.quaternion;
            node.slerp_quaternions(&a.from_q[i], &target, blend);
            let p = node.position;
            node.position.lerp_vectors(&a.from_p[i], &p, blend);
        }
    }
    a.facing = d.aim_yaw;
    a.legs = d.legs;
    tree.node_mut(root).set_rotation(0.0, d.aim_yaw, 0.0, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    // A walking step hands over to the next one tick after the newer
    // sample's clip began: the clip time runs on through the change.
    #[test]
    fn the_clip_runs_on_across_a_change_of_mode() {
        let step = 17.0 / 60.0;
        let (a, b) = ((7, step - 0.03, false), (8, 0.02, false));
        let tick = 1.0 / 60.0;
        let mut last = None;
        for k in 0..=3 {
            let elapsed = k as f64 * tick;
            let (mode, time, _) = between(a, b, elapsed, 0.05 - elapsed);
            if elapsed < 0.03 {
                assert_eq!((mode, time), (7, a.1 + elapsed));
            } else {
                assert_eq!(mode, 8);
                assert!((time - (elapsed - 0.03)).abs() < 1e-12, "{time}");
            }
            last = Some((mode, time));
        }
        assert_eq!(last, Some((8, 0.02)));
        // Within one clip the time goes straight from one sample to the next;
        // played backward too.
        let close = |got: (i32, f64, bool), want: (i32, f64, bool)| {
            assert!(got.0 == want.0 && got.2 == want.2 && (got.1 - want.1).abs() < 1e-12, "{got:?} against {want:?}");
        };
        close(between((7, 0.1, false), (7, 0.2, false), 0.025, 0.025), (7, 0.15, false));
        close(between((9, 0.2, true), (9, 0.1, true), 0.025, 0.025), (9, 0.15, true));
        // Out of a backward step into the next one, also played backward.
        close(between((9, 0.01, true), (7, 0.25, true), 0.005, 0.045), (9, 0.005, true));
        close(between((9, 0.01, true), (7, 0.25, true), 0.02, 0.03), (7, 0.28, true));
    }

    // The corner cases: backward into forward, one clip starting again
    // (either way round), dying, dead and the ball.
    #[test]
    fn the_clip_runs_on_in_the_corner_cases() {
        let close = |got: (i32, f64, bool), want: (i32, f64, bool)| {
            assert!(got.0 == want.0 && got.2 == want.2 && (got.1 - want.1).abs() < 1e-12, "{got:?} against {want:?}");
        };
        // A backward step ends (time 0) and idle begins 0.01 s before the newer sample.
        close(between((9, 0.02, true), (0, 0.01, false), 0.005, 0.025), (9, 0.015, true));
        close(between((9, 0.02, true), (0, 0.01, false), 0.04, 0.01), (0, 0.0, false));
        // Idle starting again: the old run runs on, then the new run from its start.
        close(between((0, 0.7, false), (0, 0.01, false), 0.01, 0.03), (0, 0.71, false));
        close(between((0, 0.7, false), (0, 0.01, false), 0.03, 0.01), (0, 0.0, false));
        // A backward step starting again (its time jumps back up to the clip's end).
        close(between((9, 0.01, true), (9, 0.39, true), 0.005, 0.045), (9, 0.005, true));
        close(between((9, 0.01, true), (9, 0.39, true), 0.03, 0.02), (9, 0.41, true));
        // Death begins at the newer sample (time 0): the old clip runs on to it; dead the same.
        close(between((7, 0.1, false), (3, 0.0, false), 0.02, 0.03), (7, 0.12, false));
        close(between((7, 0.1, false), (3, 0.0, false), 0.05, 0.0), (3, 0.0, false));
        close(between((3, 0.5, false), (4, 0.0, false), 0.02, 0.01), (3, 0.52, false));
        // Into the ball and out of it: the change shows when it happened.
        close(between((11, 0.5, false), (12, 0.02, false), 0.01, 0.04), (11, 0.51, false));
        close(between((11, 0.5, false), (12, 0.02, false), 0.04, 0.01), (12, 0.01, false));
        close(between((12, 0.0, false), (13, 0.01, false), 0.04, 0.01), (13, 0.0, false));
        // The ball's time does not run: it stays where the samples have it.
        close(between((12, 0.0, false), (12, 0.0, false), 0.02, 0.03), (12, 0.0, false));
    }

    // The clip's last frame shows as its mode ends, its first as it begins.
    #[test]
    fn a_clip_spreads_its_frames_over_its_mode() {
        let clip = Clip { frames: 31, fps: 30.0, root_motion: false, tracks: vec![], data: vec![] };
        let length = 31.0 / 30.0 / 2.0;
        assert_eq!(clip_time(&clip, 0.0, length), 0.0);
        assert!((clip_time(&clip, length, length) * clip.fps - 30.0).abs() < 1e-12);
        assert!((clip_time(&clip, length / 2.0, length) * clip.fps - 15.0).abs() < 1e-12);
        assert_eq!(clip_time(&clip, 2.0 * length, length), clip_time(&clip, length, length));
    }
}
