//! A soldier's body as an exposed seat poses it, for the simulation
//! (src/rider-pose.js): what a round meets of a rider, and where his body
//! falls from when he dies there.
//!
//! The rider's points come from his seat's clip at the seat's point in it,
//! carried down his own skeleton without touching the bones that are drawn:
//! joint rotations from the clip (a joint the clip leaves alone keeps its
//! bone's own), every bone's bind length but the root's (DummyRoot stays at
//! its bind place on a seat), bone_root where the clip puts it. Nothing
//! here reads a transcendental function: the pose is sampled with sqrt
//! only, and the rest is quaternion and matrix arithmetic, each line the
//! JavaScript's in its order.

use super::skeleton::Skeleton;
use three::{clamp, Matrix4, Quaternion, Vector3};
use crate::pose::{Clip, Pose};

// The head's middle in the head bone's frame (x runs up the neck): the
// average of the head vertices of the troopers' bodies.
const HEAD_MIDDLE: Vector3 = Vector3::new(0.05, 0.04, 0.0);
const HEAD_RADIUS: f64 = 0.17; // m, by max(CollisionRootScale, 1), as on foot (src/units.js)
const TORSO_RADIUS: f64 = 0.25; // m, pelvis to neck
const THIGH_RADIUS: f64 = 0.13; // m, hip to knee
const SHIN_RADIUS: f64 = 0.11; // m, knee to ankle
const UPPER_ARM_RADIUS: f64 = 0.09; // m, shoulder to elbow
const FOREARM_RADIUS: f64 = 0.07; // m, elbow to the middle of the hand
const HAND_MIDDLE: Vector3 = Vector3::new(0.1, -0.02, 0.0);
// m: a standing soldier's ankles above his soles.
const ANKLE: f64 = 0.13;

// The joints a rider's points hang from; the arms' last, which a skeleton
// may lack (its rider is then struck without them).
const NAMES: [&str; 11] = [
    "DummyRoot", "bone_root", "bone_pelvis", "bone_neck", "bone_head",
    "bone_l_thigh", "bone_l_calf", "bone_l_foot", "bone_r_thigh", "bone_r_calf", "bone_r_foot",
];
const ARMS: [&str; 6] = ["bone_l_upperarm", "bone_l_forearm", "bone_l_hand", "bone_r_upperarm", "bone_r_forearm", "bone_r_hand"];

/// The joints the animator gives back their bind translation after every
/// pose (src/animator.js BIND_LENGTH), in its order.
pub const BIND_LENGTH: [&str; 22] = [
    "bone_pelvis", "bone_a_spine", "bone_b_spine", "bone_ribcage", "bone_neck", "bone_head",
    "bone_l_clavicle", "bone_l_upperarm", "bone_l_forearm", "bone_l_hand", "bone_l_thigh", "bone_l_calf", "bone_l_foot", "bone_l_toe",
    "bone_r_clavicle", "bone_r_upperarm", "bone_r_forearm", "bone_r_hand", "bone_r_thigh", "bone_r_calf", "bone_r_foot", "bone_r_toe",
];

/// What riderPoints keeps per skeleton (src/rider-pose.js skeletonOf), and
/// what it reads of the animator: the bind lengths it restores and its
/// root's bind place.
#[derive(Clone, Debug)]
pub struct RiderSkeleton {
    /// Each bone's parent's index, -1 for none.
    pub parent: Vec<i32>,
    /// The bones the points hang from, each after its parent.
    pub order: Vec<usize>,
    /// DummyRoot's index (0 without one).
    pub root: usize,
    /// Whether the skeleton has both arms' joints.
    pub arms: bool,
    /// Whether a joint the points need is missing (riderPoints answers null).
    pub incomplete: bool,
    /// `byName[name]` for the joints read (the last bone of the name).
    pelvis: usize,
    neck: usize,
    head: usize,
    hips: [usize; 2],
    knees: [usize; 2],
    ankles: [usize; 2],
    shoulders: [usize; 2],
    elbows: [usize; 2],
    hands: [usize; 2],
    bone_root: Option<usize>,
    /// The animator's bind lengths by bone index (bindsOf): the bone's bind
    /// position where it is one of BIND_LENGTH's.
    pub binds: Vec<Option<Vector3>>,
    /// The animator's `rootBind`: DummyRoot's bind position.
    pub root_bind: Option<Vector3>,
}

impl RiderSkeleton {
    /// A soldier's rider skeleton, from his body's skeleton as its animator
    /// found it (SoldierAnimator's constructor reads the bind pose: its
    /// bindLengths and rootBind are the bones' positions then).
    pub fn new(skeleton: &Skeleton) -> RiderSkeleton {
        let n = skeleton.len();
        let by_name = |name: &str| skeleton.by_name(name);
        let parent = skeleton.parents.clone();
        let mut order = Vec::new();
        let mut seen = vec![false; n];
        fn visit(i: i32, parent: &[i32], seen: &mut [bool], order: &mut Vec<usize>) {
            if i < 0 || seen[i as usize] {
                return;
            }
            visit(parent[i as usize], parent, seen, order);
            seen[i as usize] = true;
            order.push(i as usize);
        }
        let arms = ARMS.iter().all(|name| by_name(name).is_some());
        let names: Vec<&str> = if arms { NAMES.iter().chain(ARMS.iter()).copied().collect() } else { NAMES.to_vec() };
        for name in names {
            if let Some(i) = by_name(name) {
                visit(i as i32, &parent, &mut seen, &mut order);
            }
        }
        let incomplete = NAMES[2..].iter().any(|name| by_name(name).is_none());
        let at = |name: &str| by_name(name).unwrap_or(0);
        // The animator's bind lengths: [bone, its position] for each of
        // BIND_LENGTH's names it has (byName, the last of the name), keyed
        // by the bone's index (bones.indexOf: the first bone that is that
        // bone, which is itself).
        let mut binds = vec![None; n];
        for name in BIND_LENGTH {
            if let Some(i) = by_name(name) {
                binds[i] = Some(skeleton.bind_position(i));
            }
        }
        let root_bind = by_name("DummyRoot").map(|i| skeleton.bind_position(i));
        RiderSkeleton {
            parent,
            order,
            root: by_name("DummyRoot").unwrap_or(0),
            arms,
            incomplete,
            pelvis: at("bone_pelvis"),
            neck: at("bone_neck"),
            head: at("bone_head"),
            hips: [at("bone_l_thigh"), at("bone_r_thigh")],
            knees: [at("bone_l_calf"), at("bone_r_calf")],
            ankles: [at("bone_l_foot"), at("bone_r_foot")],
            shoulders: [at("bone_l_upperarm"), at("bone_r_upperarm")],
            elbows: [at("bone_l_forearm"), at("bone_r_forearm")],
            hands: [at("bone_l_hand"), at("bone_r_hand")],
            bone_root: by_name("bone_root"),
            binds,
            root_bind,
        }
    }
}

/// A seat's sample of the rider's clip (src/vehicles.js riderSample,
/// src/props.js Turret.riderSample, AnimatedProp.heldFrame): a time in it,
/// or the 9-pose's pitch and turn.
#[derive(Clone, Copy)]
pub struct RiderSample<'c> {
    pub clip: &'c Clip,
    pub pitch: f64,
    pub turn: f64,
    /// `time`, None for null or undefined.
    pub time: Option<f64>,
}

/// The rider's points in his body's frame (feet at the origin, y up).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RiderPoints {
    pub pelvis: Vector3,
    pub neck: Vector3,
    pub head: Vector3,
    pub hips: [Vector3; 2],
    pub knees: [Vector3; 2],
    pub ankles: [Vector3; 2],
    pub shoulders: [Vector3; 2],
    pub elbows: [Vector3; 2],
    pub hands: [Vector3; 2],
    pub arms: bool,
    /// bone_root's place and turn, written where asked (`out.root`,
    /// `out.rootTurn`: a creature hangs him by it).
    pub root: Vector3,
    pub root_turn: Quaternion,
}

/// riderPoints' scratch: its pose (one per skeleton in the JavaScript) and
/// the local and world turns and places it works through.
pub struct RiderScratch {
    pub pose: Pose,
    local_q: Vec<Quaternion>,
    local_p: Vec<Vector3>,
    world_q: Vec<Quaternion>,
    world_p: Vec<Vector3>,
}

impl RiderScratch {
    pub fn new(skeleton: &RiderSkeleton) -> RiderScratch {
        let n = skeleton.parent.len();
        RiderScratch {
            pose: Pose::new(n, skeleton.root as i32),
            local_q: vec![Quaternion::default(); n],
            local_p: vec![Vector3::default(); n],
            world_q: vec![Quaternion::default(); n],
            world_p: vec![Vector3::default(); n],
        }
    }
}

/// Pose.add of src/anim.js with its own guard: no clip, or a weight at or
/// under 1e-4, adds nothing.
pub fn pose_add(pose: &mut Pose, clip: &Clip, time: f64, weight: f64, looping: bool) {
    if weight <= 1e-4 {
        return;
    }
    pose.add(clip, time, weight, looping);
}

/// `x || 0` for a number.
#[inline(always)]
fn or_zero(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        0.0
    } else {
        x
    }
}

/// riderPoints(animator, sample, out): the rider's points for a seat
/// sample, written into `out`; false where the JavaScript answers null (a
/// skeleton the points can't be found on). `bone_q` and `bone_p` are his
/// bones' quaternions and positions as they stand (a joint the clip leaves
/// alone keeps its bone's: in a world, which never poses a soldier's drawn
/// bones, the bind pose). `root`: whether `out` asks for bone_root's place
/// and turn (the JavaScript's `out.root` and `out.rootTurn`).
pub fn rider_points(
    skeleton: &RiderSkeleton,
    bone_q: &[Quaternion],
    bone_p: &[Vector3],
    sample: &RiderSample,
    scratch: &mut RiderScratch,
    out: &mut RiderPoints,
    root: bool,
) -> bool {
    if skeleton.incomplete {
        return false;
    }
    let clip = sample.clip;
    let pose = &mut scratch.pose;
    // As SoldierAnimator.update samples a seat's clip.
    pose.clear();
    if let Some(time) = sample.time {
        pose_add(pose, clip, time, 1.0, true);
    } else {
        let pitch = clamp(or_zero(sample.pitch), -1.0, 1.0);
        let turn = clamp(or_zero(sample.turn), -1.0, 1.0);
        pose_add(pose, clip, (4.0 + turn) / clip.fps, 1.0 - pitch.abs(), false);
        pose_add(pose, clip, ((if pitch > 0.0 { 7.0 } else { 1.0 }) + turn) / clip.fps, pitch.abs(), false);
    }
    for &i in &skeleton.order {
        let w = pose.w[i] as f64;
        let q = &mut scratch.local_q[i];
        let p = &mut scratch.local_p[i];
        if w > 1e-6 {
            q.set(pose.q[i * 4] as f64, pose.q[i * 4 + 1] as f64, pose.q[i * 4 + 2] as f64, pose.q[i * 4 + 3] as f64).normalize();
        } else {
            q.copy(&bone_q[i]);
        }
        match (i == skeleton.root, skeleton.root_bind, skeleton.binds[i]) {
            (true, Some(root_bind), _) => {
                p.copy(&root_bind);
            }
            (_, _, Some(bind)) => {
                p.copy(&bind);
            }
            _ if w > 1e-6 => {
                p.set(pose.t[i * 3] as f64 / w, pose.t[i * 3 + 1] as f64 / w, pose.t[i * 3 + 2] as f64 / w);
            }
            _ => {
                p.copy(&bone_p[i]);
            }
        }
        let (q, p) = (*q, *p);
        let up = skeleton.parent[i];
        if up < 0 {
            scratch.world_q[i] = q;
            scratch.world_p[i] = p;
        } else {
            let up = up as usize;
            let (wq_up, wp_up) = (scratch.world_q[up], scratch.world_p[up]);
            scratch.world_q[i].multiply_quaternions(&wq_up, &q);
            let wp = &mut scratch.world_p[i];
            wp.copy(&p).apply_quaternion(&wq_up).add(&wp_up);
        }
    }
    let wp = &scratch.world_p;
    let wq = &scratch.world_q;
    out.pelvis.copy(&wp[skeleton.pelvis]);
    out.neck.copy(&wp[skeleton.neck]);
    let head = skeleton.head;
    out.head.copy(&HEAD_MIDDLE).apply_quaternion(&wq[head]).add(&wp[head]);
    out.hips[0].copy(&wp[skeleton.hips[0]]);
    out.hips[1].copy(&wp[skeleton.hips[1]]);
    out.knees[0].copy(&wp[skeleton.knees[0]]);
    out.knees[1].copy(&wp[skeleton.knees[1]]);
    out.ankles[0].copy(&wp[skeleton.ankles[0]]);
    out.ankles[1].copy(&wp[skeleton.ankles[1]]);
    out.arms = skeleton.arms;
    if out.arms {
        for side in 0..2 {
            out.shoulders[side].copy(&wp[skeleton.shoulders[side]]);
            out.elbows[side].copy(&wp[skeleton.elbows[side]]);
            let hand = skeleton.hands[side];
            out.hands[side].copy(&HAND_MIDDLE).apply_quaternion(&wq[hand]).add(&wp[hand]);
        }
    }
    if root {
        // The JavaScript reads world.p[byName.bone_root]: a skeleton without
        // one throws there. Every soldier's has it.
        let r = skeleton.bone_root.expect("riderPoints: a held body's skeleton has no bone_root");
        out.root.copy(&wp[r]);
        out.root_turn.copy(&wq[r]);
    }
    true
}

/// What a hit shape is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Sphere,
    Capsule,
}

/// Which part of the body a shape is (a head's hits do three times the
/// damage).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapePart {
    Head,
    Body,
}

/// A rider's hit shape, as src/registry.js segmentShape reads one: `half`
/// is a number for a sphere (0) and, for a capsule, the half-segment vector
/// (`half_size`, which `half` then names).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitShape {
    pub kind: ShapeKind,
    pub part: ShapePart,
    pub center: Vector3,
    pub radius: f64,
    /// The number `half` holds when it is not the vector.
    pub half: f64,
    /// Whether `half` is `half_size` (a capsule's).
    pub half_is_size: bool,
    pub half_size: Vector3,
    pub yaw: f64,
}

impl Default for HitShape {
    fn default() -> Self {
        HitShape {
            kind: ShapeKind::Sphere,
            part: ShapePart::Body,
            center: Vector3::default(),
            radius: 0.0,
            half: 0.0,
            half_is_size: false,
            half_size: Vector3::default(),
            yaw: 0.0,
        }
    }
}

/// The most shapes a rider has.
pub const RIDER_SHAPES: usize = 10;

fn sphere(shape: &mut HitShape, part: ShapePart, center: &Vector3, radius: f64) {
    shape.kind = ShapeKind::Sphere;
    shape.part = part;
    shape.center.copy(center);
    shape.radius = radius;
    shape.half = 0.0;
    shape.half_is_size = false;
    shape.yaw = 0.0;
}

fn capsule(shape: &mut HitShape, part: ShapePart, from: &Vector3, to: &Vector3, radius: f64) {
    shape.kind = ShapeKind::Capsule;
    shape.part = part;
    shape.center.add_vectors(from, to).multiply_scalar(0.5);
    shape.half_size.sub_vectors(to, from).multiply_scalar(0.5);
    shape.half_is_size = true;
    shape.radius = radius;
    shape.yaw = 0.0;
}

/// riderHitShapes(points, frame, scale, shapes): the head (a sphere), the
/// torso from the pelvis up to the neck, each leg's thigh and shin and,
/// where the skeleton has them, each arm's upper arm and forearm to the
/// middle of the hand (capsules), carried into the world by `frame`.
/// `scale`: the class's CollisionRootScale (None for undefined or null).
/// Returns the count: 10, or 6 without arms.
pub fn rider_hit_shapes(points: &RiderPoints, frame: &Matrix4, scale: Option<f64>, shapes: &mut [HitShape]) -> usize {
    let s = crate::collision::js_max(scale.unwrap_or(1.0), 1.0);
    let mut a = Vector3::default();
    let mut b = Vector3::default();
    let mut up = Vector3::default();
    a.copy(&points.head).apply_matrix4(frame);
    sphere(&mut shapes[0], ShapePart::Head, &a, HEAD_RADIUS * s);
    a.copy(&points.pelvis).apply_matrix4(frame);
    b.copy(&points.neck).apply_matrix4(frame);
    let spine = up.sub_vectors(&b, &a).length();
    if spine > TORSO_RADIUS {
        up.divide_scalar(spine);
        b.add_scaled_vector(&up, -TORSO_RADIUS);
    }
    capsule(&mut shapes[1], ShapePart::Body, &a, &b, TORSO_RADIUS);
    for side in 0..2 {
        a.copy(&points.hips[side]).apply_matrix4(frame);
        b.copy(&points.knees[side]).apply_matrix4(frame);
        capsule(&mut shapes[2 + side * 2], ShapePart::Body, &a, &b, THIGH_RADIUS);
        a.copy(&points.knees[side]).apply_matrix4(frame);
        b.copy(&points.ankles[side]).apply_matrix4(frame);
        capsule(&mut shapes[3 + side * 2], ShapePart::Body, &a, &b, SHIN_RADIUS);
    }
    if !points.arms {
        return 6;
    }
    for side in 0..2 {
        a.copy(&points.shoulders[side]).apply_matrix4(frame);
        b.copy(&points.elbows[side]).apply_matrix4(frame);
        capsule(&mut shapes[6 + side * 2], ShapePart::Body, &a, &b, UPPER_ARM_RADIUS);
        a.copy(&points.elbows[side]).apply_matrix4(frame);
        b.copy(&points.hands[side]).apply_matrix4(frame);
        capsule(&mut shapes[7 + side * 2], ShapePart::Body, &a, &b, FOREARM_RADIUS);
    }
    10
}

/// riderDropPoint(points, out): under his pelvis, his soles where the lower
/// of his feet was, in the body's frame.
pub fn rider_drop_point(points: &RiderPoints, out: &mut Vector3) -> Vector3 {
    out.set(points.pelvis.x, crate::collision::js_min(points.ankles[0].y, points.ankles[1].y) - ANKLE, points.pelvis.z);
    *out
}
