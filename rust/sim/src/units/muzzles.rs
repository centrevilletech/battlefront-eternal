//! Live hidden gun rigs. The host supplies the already-built hierarchy and
//! bind pose once; posing and physical gun selection use the same rig as
//! the native unit replay. The drawn skeleton remains independent.

use super::droideka::{self, DroidekaParts};
use crate::gen::{Unit, UnitBody};
use crate::pose::Pose;
use crate::rig::droideka::{DroidekaAnimator, DroidekaRig};
use crate::store::word_u32;
use std::ptr::addr_of_mut;
use three::{EulerOrder, Quaternion, TransformNode, TransformTree, Vector3};

const ORDERS: [EulerOrder; 6] = [EulerOrder::XYZ, EulerOrder::YXZ, EulerOrder::ZXY, EulerOrder::ZYX, EulerOrder::YZX, EulerOrder::XZY];

pub(crate) struct Muzzles {
    parts: DroidekaParts,
}

static mut RIGS: Vec<Option<Muzzles>> = Vec::new();

fn rigs() -> &'static mut Vec<Option<Muzzles>> {
    // One module thread. No caller holds this borrow across an import.
    unsafe { &mut *addr_of_mut!(RIGS) }
}

struct Read<'a> { words: &'a [f64], at: usize }
impl Read<'_> {
    fn number(&mut self) -> f64 { let v = self.words[self.at]; self.at += 1; v }
    fn index(&mut self) -> usize { word_u32(self.number()) as usize }
    fn vector(&mut self) -> Vector3 { Vector3::new(self.number(), self.number(), self.number()) }
    fn quaternion(&mut self) -> Quaternion { Quaternion::new(self.number(), self.number(), self.number(), self.number()) }
}

/// The actor's prepared indices and transforms, including its current pose:
/// attaching in the middle of a rolling pose must keep an unfold's source.
pub fn attach(id: u32, mut parts: DroidekaParts, words: &[f64]) {
    let mut r = Read { words, at: 0 };
    let n = r.index();
    let root = r.number() as i32;
    let ball = r.number() as i32;
    let spine_len = r.index();
    let spine = (0..spine_len).map(|_| r.index()).collect();
    let guns = r.index();
    let mut next = n as u32 + 1;
    let barrels = (0..guns).map(|_| {
        let count = r.index() as u32;
        let list = (next..next + count).collect();
        next += count;
        list
    }).collect();
    let mut bind_p = Vec::with_capacity(n);
    let mut bind_q = Vec::with_capacity(n);
    for _ in 0..n { bind_p.push(r.vector()); bind_q.push(r.quaternion()); }
    let mut tree = TransformTree::new();
    let mut parents = Vec::new();
    for _ in 0..next {
        parents.push(r.number() as i32);
        let mut node = TransformNode::placed(r.vector(), Vector3::default());
        let q = r.quaternion();
        node.scale = r.vector();
        let (x, y, z) = (r.number(), r.number(), r.number());
        let order = ORDERS[r.index()];
        node.set_rotation(x, y, z, Some(order));
        // Preserve the supplied Euler without running a quaternion callback
        // at attachment. Later native quaternion writes sync it normally.
        node.quaternion = q;
        for v in &mut node.matrix.elements { *v = r.number(); }
        for v in &mut node.matrix_world.elements { *v = r.number(); }
        node.matrix_auto_update = r.number() != 0.0;
        node.matrix_world_auto_update = r.number() != 0.0;
        node.matrix_world_needs_update = r.number() != 0.0;
        tree.add_node(node);
    }
    for (i, &parent) in parents.iter().enumerate() {
        if parent >= 0 { tree.add_child(parent as u32, i as u32); }
    }
    let mut animator = DroidekaAnimator {
        root, ball, spine, pose: Pose::new(n, root),
        from_q: bind_q.clone(), from_p: bind_p.clone(), bind_q, bind_p,
        facing: 0.0, legs: 0.0, mode: -1, went_up_rolled: false,
        roll_origin: 0.0, transition: 0.0, pivot: Vector3::default(), roll_center: Vector3::default(),
    };
    for v in &mut animator.pose.q { *v = r.number() as f32; }
    for v in &mut animator.pose.t { *v = r.number() as f32; }
    for v in &mut animator.pose.w { *v = r.number() as f32; }
    assert_eq!(r.at, words.len());
    parts.rig = Some(Box::new(DroidekaRig { tree, root: 0, bones: (1..=n as u32).collect(), barrels, animator }));
    let list = rigs();
    if list.len() <= id as usize { list.resize_with(id as usize + 1, || None); }
    list[id as usize] = Some(Muzzles { parts });
}

pub fn sample(id: u32, unit: &mut Unit, dt: f64) -> bool {
    let Some(m) = rigs().get_mut(id as usize).and_then(Option::as_mut) else { return false };
    droideka::sample_muzzles(unit, dt, &mut m.parts);
    true
}

/// The outer result distinguishes a missing prepared rig from a valid
/// selection with no barrel. The native combat owner requires the rig.
pub fn next_gun(id: u32, unit: &mut Unit) -> Option<Option<Vector3>> {
    let m = rigs().get_mut(id as usize)?.as_mut()?;
    let UnitBody::Droideka(d) = &mut unit.soldier else { return None };
    let point = d.next_gun(m.parts.rig.as_mut().unwrap());
    Some(point)
}

pub fn gun_at(id: u32, selected: (f64, f64)) -> Option<Option<Vector3>> {
    let m = rigs().get_mut(id as usize)?.as_mut()?;
    let index = |x: f64| (x >= 0.0 && x.fract() == 0.0).then_some(x as usize);
    let point = match (index(selected.0), index(selected.1)) {
        (Some(i), Some(b)) => m.parts.rig.as_mut().unwrap().gun_at(i, b),
        _ => None,
    };
    Some(point)
}


// UnitFactory cold construction transfers its private rig at activation.
// No drawn skeleton or barrel query participates in the native birth.
pub(crate) fn take(id: u32) -> Option<Muzzles> {
    rigs().get_mut(id as usize).and_then(Option::take)
}
pub(crate) fn install(id: u32, rig: Option<Muzzles>) {
    let rows = rigs();
    assert!((id as usize) < rows.len(), "reserved native muzzle slots");
    rows[id as usize] = rig;
}
pub(crate) fn reserve(count: usize) {
    let rows = rigs();
    rows.resize_with(count.max(rows.len()), || None);
}
