//! The API surface P5's rigs (rust/sim/src/rig, first built against
//! their own rig/three.rs and rig/tree.rs) switch onto this crate
//! by changing their imports: the same type, field and method names
//! through the public API only, plus the two behaviours P5 found
//! that a plain port misses — the per-node Euler callback switch
//! (actor bones run with it off, src/actor.js NO_EULER) and
//! slerp's callback points. The slerp cases are the ones measured
//! against three r170 in Node: with cos >= 1 slerp copies qb in
//! (firing), then writes the original back without firing, so the
//! Euler is left synced from qb while the quaternion is restored;
//! in the negation branch nothing fired at all.

use three::mathutils;
use three::{
    clamp, deg_to_rad, lerp, smoothstep, Axis, Box3, Euler, EulerOrder, Matrix4, Quaternion,
    TransformNode, TransformTree, Vector3,
};

#[test]
fn free_functions_and_axis() {
    assert_eq!(clamp(5.0, 0.0, 3.0), 3.0);
    assert_eq!(lerp(2.0, 4.0, 0.25), 2.5);
    assert_eq!(smoothstep(0.5, 0.0, 1.0), 0.5);
    assert_eq!(deg_to_rad(180.0), core::f64::consts::PI);
    assert_eq!(mathutils::clamp(-1.0, 0.0, 3.0), 0.0);

    let mut v = Vector3::new(1.0, 2.0, 3.0);
    assert_eq!(v.get(Axis::Y), 2.0);
    v.set_axis(Axis::Z, 9.0);
    assert_eq!(v.z, 9.0);
}

#[test]
fn euler_sync_off_keeps_rotation() {
    // NO_EULER bones: quaternion writes never touch the Euler.
    let mut node = TransformNode::default();
    node.set_rotation(0.5, -0.25, 1.0, Some(EulerOrder::YXZ));
    let held = node.rotation();
    node.euler_sync = false;
    node.set_quaternion(1.0, 0.0, 0.0, 0.0);
    node.with_quaternion(|q| {
        q.normalize();
    });
    node.slerp_quaternion(&Quaternion::default(), 0.5);
    assert_eq!(node.rotation(), held);
    assert_eq!(node.rotation_order(), EulerOrder::YXZ);
}

#[test]
fn slerp_callback_points() {
    // cos >= 1 through the copy branch (dot > 1, non-unit): the
    // quaternion is restored, the Euler is qb's.
    let original = Quaternion::new(2.0, 0.0, 0.0, 2.0);
    let qb = Quaternion::new(1.0, 0.0, 0.0, 0.4);
    let mut node = TransformNode::default();
    node.copy_quaternion(&original);
    let euler_original = node.rotation();
    node.slerp_quaternion(&qb, 0.5);
    assert_eq!(node.quaternion, original, "slerp put the original back");
    let mut euler_qb = Euler::default();
    euler_qb.set_from_quaternion(&qb, None);
    assert_eq!(node.rotation(), euler_qb, "the callback last saw qb");
    assert_ne!(node.rotation(), euler_original);

    // cos >= 1 through the negation branch: nothing fired, the
    // Euler keeps what it held.
    let neg_qb = Quaternion::new(-1.0, 0.0, 0.0, -0.4);
    let mut node = TransformNode::default();
    node.copy_quaternion(&original);
    let held = node.rotation();
    node.slerp_quaternion(&neg_qb, 0.5);
    assert_eq!(node.quaternion, original);
    assert_eq!(node.rotation(), held);

    // t = 0 fires nothing; t = 1 lands on qb entirely.
    let mut node = TransformNode::default();
    node.copy_quaternion(&original);
    let held = node.rotation();
    node.slerp_quaternion(&qb, 0.0);
    assert_eq!(node.rotation(), held);
    node.slerp_quaternion(&qb, 1.0);
    assert_eq!(node.quaternion, qb);
    assert_eq!(node.rotation(), euler_qb);

    // The tracked return itself.
    let mut q = original;
    assert_eq!(q.slerp_tracked(&qb, 0.0), None);
    let mut q = original;
    assert_eq!(q.slerp_tracked(&qb, 0.5), Some(qb));
    assert_eq!(q, original);
    let mut q = original;
    assert_eq!(q.slerp_tracked(&neg_qb, 0.5), None);
    assert_eq!(q, original);
}

#[test]
fn node_surface_p5_uses() {
    let mut tree = TransformTree::new();
    let root = tree.add_node(TransformNode::placed(
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, 1.0, 1.0),
    ));
    let child = tree.add_node(TransformNode::default());
    tree.add_child(root, child);

    // decompose_from fires the quaternion callback (deferred).
    let mut m = Matrix4::default();
    let q = Quaternion::new(0.0, 0.0, 0.0, 1.0);
    m.compose(&Vector3::new(3.0, 4.0, 5.0), &q, &Vector3::new(1.0, 1.0, 1.0));
    tree.node_mut(child).decompose_from(&m);
    assert_eq!(tree.node(child).position, Vector3::new(3.0, 4.0, 5.0));

    // clone_node, traverse, hangs_from.
    let clone = tree.clone_node(child);
    assert_eq!(tree.node(clone).position, Vector3::new(3.0, 4.0, 5.0));
    tree.add_child(child, clone);
    let mut seen = Vec::new();
    tree.traverse(root, &mut |id| seen.push(id));
    assert_eq!(seen, vec![root, child, clone]);
    assert!(tree.hangs_from(clone, root));
    assert!(!tree.hangs_from(root, clone));

    // Getters return by value and write the target.
    tree.update_matrix_world(root, true);
    let mut target = Vector3::default();
    let got = tree.get_world_position(child, &mut target);
    assert_eq!(got, target);
    assert_eq!(got, Vector3::new(4.0, 4.0, 5.0));
    let mut qtarget = Quaternion::new(9.0, 9.0, 9.0, 9.0);
    let qgot = tree.get_world_quaternion(child, &mut qtarget);
    assert_eq!(qgot, qtarget);

    // Box3's get_center the way the muzzle finders read it.
    let mut b = Box3::default();
    b.set_from_points(&[Vector3::new(0.0, 0.0, 0.0), Vector3::new(2.0, 4.0, 6.0)]);
    let mut tip = Vector3::default();
    assert_eq!(b.get_center(&mut tip), Vector3::new(1.0, 2.0, 3.0));
    assert_eq!(tip, Vector3::new(1.0, 2.0, 3.0));
}
