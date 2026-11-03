//! `Object3D`'s transform semantics (r170 `src/core/Object3D.js`),
//! as a tree of nodes by index: the rigs, hulls, seats and bones
//! the sim poses are Object3D hierarchies, and their world matrices
//! are behaviour. Only the transform half of Object3D is here —
//! no events, layers, visibility or rendering state.
//!
//! The Euler/quaternion sync is the subtle part. In three, an
//! Object3D's `rotation` (an Euler) and `quaternion` keep each
//! other current through change callbacks: any write through an
//! Euler setter or method runs `quaternion.setFromEuler(rotation,
//! false)`, and any write through a Quaternion setter or method
//! runs `rotation.setFromQuaternion(quaternion, undefined, false)`
//! (keeping the Euler's current order) — an asin and two atan2s on
//! every write. Code reads whichever it likes, including reading a
//! flyer root's XYZ-Euler `.y` as a heading after the quaternion
//! was written, which gives three's synced (and past 90 degrees,
//! surprising) value.
//!
//! Nearly every quaternion write in the simulation is to a bone
//! whose Euler nothing reads, so here a quaternion write only notes
//! the value the callback saw (`euler_from`), and the Euler is
//! worked out from it when something reads the rotation or writes
//! one of its parts. That gives three's values exactly: the
//! conversion reads only that quaternion and the Euler's order,
//! and the order changes only through a rotation write, which
//! works the Euler out first. A node whose quaternion callback
//! three has switched off (`bone.quaternion._onChange(NO_EULER)`,
//! src/actor.js) has `euler_sync` false: its rotation keeps what it
//! last held.
//!
//! Writes go through the node's methods, which note the callback
//! where three's fires (last value seen): `set_quaternion`,
//! `copy_quaternion`, `with_quaternion` (any of three's quaternion
//! methods but slerp, each of which fires once at its end with the
//! final value), `slerp_quaternion` and `decompose_from`. Reading
//! `quaternion`, `position` and `scale` is plain; a position or
//! scale write is plain in three too. The tree-level methods
//! (`apply_matrix4`, `look_at`, the rotates, ...) go through the
//! same node methods three's Object3D methods go through, so their
//! callback points match too.
//!
//! This layout is the one P5's rigs were first built against
//! (rust/sim/src/rig/tree.rs, superseded by this crate): same type,
//! field and method names, so the rigs move by their imports.

use crate::euler::{Euler, EulerOrder};
use crate::matrix::Matrix4;
use crate::quaternion::Quaternion;
use crate::vector::Vector3;

/// A node's index in a [`TransformTree`].
pub type NodeId = u32;

/// One Object3D's transform state.
#[derive(Clone, Debug)]
pub struct TransformNode {
    pub position: Vector3,
    pub quaternion: Quaternion,
    pub scale: Vector3,
    /// `up`, default (0, 1, 0).
    pub up: Vector3,
    /// The Euler as three last left it, unless `euler_from` is pending.
    rotation: Euler,
    /// The quaternion three's change callback last synced the Euler
    /// from, not yet worked out.
    euler_from: Option<Quaternion>,
    /// Whether a quaternion write syncs the Euler (three's default).
    /// Actor bones run with it off (`src/actor.js` NO_EULER).
    pub euler_sync: bool,
    pub matrix: Matrix4,
    pub matrix_world: Matrix4,
    /// `matrixAutoUpdate`; default true.
    pub matrix_auto_update: bool,
    /// `matrixWorldAutoUpdate`; default true.
    pub matrix_world_auto_update: bool,
    /// `matrixWorldNeedsUpdate`; default false.
    pub matrix_world_needs_update: bool,
    /// Whether `look_at` treats this node as a camera or light
    /// (`this.isCamera || this.isLight`): three aims cameras down
    /// -z, everything else down +z.
    pub is_camera_or_light: bool,
    pub parent: Option<NodeId>,
    /// In `add` order.
    pub children: Vec<NodeId>,
}

impl Default for TransformNode {
    fn default() -> Self {
        TransformNode {
            position: Vector3::default(),
            quaternion: Quaternion::default(),
            scale: Vector3::new(1.0, 1.0, 1.0),
            up: Vector3::new(0.0, 1.0, 0.0),
            rotation: Euler::default(),
            euler_from: None,
            euler_sync: true,
            matrix: Matrix4::default(),
            matrix_world: Matrix4::default(),
            matrix_auto_update: true,
            matrix_world_auto_update: true,
            matrix_world_needs_update: false,
            is_camera_or_light: false,
            parent: None,
            children: Vec::new(),
        }
    }
}

impl TransformNode {
    /// A new Object3D at `position` with `scale`, its quaternion the
    /// identity (no callback has fired).
    pub fn placed(position: Vector3, scale: Vector3) -> TransformNode {
        TransformNode { position, scale, ..TransformNode::default() }
    }

    /// three's `onQuaternionChange`, deferred (see the module's notes).
    #[inline(always)]
    fn quaternion_changed(&mut self) {
        if self.euler_sync {
            self.euler_from = Some(self.quaternion);
        }
    }

    /// The pending sync, worked out now.
    fn settle_euler(&mut self) {
        if let Some(q) = self.euler_from.take() {
            self.rotation.set_from_quaternion(&q, None);
        }
    }

    /// `rotation`, as three holds it now.
    pub fn rotation(&mut self) -> Euler {
        self.settle_euler();
        self.rotation
    }

    /// `rotation.order`: known without working the Euler out (only a
    /// rotation write changes it).
    pub fn rotation_order(&self) -> EulerOrder {
        self.rotation.order
    }

    /// three's `onRotationChange`: `quaternion.setFromEuler(rotation,
    /// false)`, which does not fire the quaternion's callback.
    fn rotation_changed(&mut self) {
        let euler = self.rotation;
        self.quaternion.set_from_euler(&euler);
    }

    /// `rotation.set(x, y, z, order?)`, with the sync.
    pub fn set_rotation(&mut self, x: f64, y: f64, z: f64, order: Option<EulerOrder>) {
        // Every part is written: what a pending sync would have left
        // is overwritten, the order too unless it is kept, which the
        // pending sync did not change.
        self.euler_from = None;
        self.rotation.set(x, y, z, order);
        self.rotation_changed();
    }

    /// `rotation.x = value`, with the sync.
    pub fn set_rotation_x(&mut self, x: f64) {
        self.settle_euler();
        self.rotation.x = x;
        self.rotation_changed();
    }

    /// `rotation.y = value`, with the sync.
    pub fn set_rotation_y(&mut self, y: f64) {
        self.settle_euler();
        self.rotation.y = y;
        self.rotation_changed();
    }

    /// `rotation.z = value`, with the sync.
    pub fn set_rotation_z(&mut self, z: f64) {
        self.settle_euler();
        self.rotation.z = z;
        self.rotation_changed();
    }

    /// `rotation.order = order`, with the sync.
    pub fn set_rotation_order(&mut self, order: EulerOrder) {
        self.settle_euler();
        self.rotation.order = order;
        self.rotation_changed();
    }

    /// `rotation.copy(euler)`, with the sync.
    pub fn copy_rotation(&mut self, euler: &Euler) {
        self.euler_from = None;
        self.rotation.copy(euler);
        self.rotation_changed();
    }

    /// Mutate the Euler as three code does (any mix of field writes
    /// and method calls), then sync the quaternion once, as the last
    /// of three's per-write callbacks does (see `set_rotation`).
    pub fn with_rotation(&mut self, f: impl FnOnce(&mut Euler)) {
        self.settle_euler();
        f(&mut self.rotation);
        self.rotation_changed();
    }

    /// `quaternion.set(x, y, z, w)`.
    pub fn set_quaternion(&mut self, x: f64, y: f64, z: f64, w: f64) {
        self.quaternion.set(x, y, z, w);
        self.quaternion_changed();
    }

    /// `quaternion.copy(q)`.
    pub fn copy_quaternion(&mut self, q: &Quaternion) {
        self.quaternion.copy(q);
        self.quaternion_changed();
    }

    /// Any of three's quaternion methods that fire the callback
    /// once, at their end (set, copy, setFromAxisAngle,
    /// setFromEuler(e, true), setFromRotationMatrix, invert,
    /// normalize, multiply, premultiply, multiplyQuaternions,
    /// fromArray): run on the quaternion, then the callback. A
    /// chain of them fires after each, and the last one wins, so one
    /// callback at the end of the chain is the same.
    pub fn with_quaternion(&mut self, f: impl FnOnce(&mut Quaternion)) {
        f(&mut self.quaternion);
        self.quaternion_changed();
    }

    /// `quaternion.slerp(qb, t)`, with the callback where three
    /// fires it (see `Quaternion::slerp_tracked`).
    pub fn slerp_quaternion(&mut self, qb: &Quaternion, t: f64) {
        if let Some(seen) = self.quaternion.slerp_tracked(qb, t) {
            if self.euler_sync {
                self.euler_from = Some(seen);
            }
        }
    }

    /// `quaternion.slerpQuaternions(qa, qb, t)`: `copy(qa)` (which
    /// fires) then `slerp(qb, t)`.
    pub fn slerp_quaternions(&mut self, qa: &Quaternion, qb: &Quaternion, t: f64) {
        self.copy_quaternion(qa);
        self.slerp_quaternion(qb, t);
    }

    /// `matrix.decompose(position, quaternion, scale)` into this
    /// node's own transform: the quaternion's setFromRotationMatrix
    /// fires.
    pub fn decompose_from(&mut self, m: &Matrix4) {
        let mut q = self.quaternion;
        m.decompose(&mut self.position, &mut q, &mut self.scale);
        self.quaternion = q;
        self.quaternion_changed();
    }

    /// `updateMatrix()`.
    pub fn update_matrix(&mut self) {
        let (p, q, s) = (self.position, self.quaternion, self.scale);
        self.matrix.compose(&p, &q, &s);
        self.matrix_world_needs_update = true;
    }
}

/// A hierarchy of [`TransformNode`]s: Object3D parent/children as
/// indices, children in `add` order.
#[derive(Clone, Debug, Default)]
pub struct TransformTree {
    pub nodes: Vec<TransformNode>,
}

impl TransformTree {
    pub fn new() -> Self {
        TransformTree { nodes: Vec::new() }
    }

    /// A fresh node, unparented.
    pub fn add_node(&mut self, node: TransformNode) -> NodeId {
        let id = self.nodes.len() as NodeId;
        self.nodes.push(node);
        id
    }

    pub fn node(&self, id: NodeId) -> &TransformNode {
        &self.nodes[id as usize]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut TransformNode {
        &mut self.nodes[id as usize]
    }

    /// `parent.add(child)`: removes the child from its old parent
    /// first, as three does. Adding a node to itself is ignored
    /// (three logs an error and returns).
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) {
        if parent == child {
            return;
        }
        self.remove_from_parent(child);
        self.nodes[child as usize].parent = Some(parent);
        self.nodes[parent as usize].children.push(child);
    }

    /// `object.removeFromParent()`.
    pub fn remove_from_parent(&mut self, child: NodeId) {
        if let Some(parent) = self.nodes[child as usize].parent {
            let siblings = &mut self.nodes[parent as usize].children;
            if let Some(i) = siblings.iter().position(|&c| c == child) {
                siblings.remove(i);
            }
            self.nodes[child as usize].parent = None;
        }
    }

    /// `source.clone(false)`: a new node of the same kind, `copy(source,
    /// false)`. three's copy sets the rotation first (which syncs the
    /// new node's quaternion from the copied Euler, a value the next
    /// line overwrites) and then copies the quaternion, which fires
    /// the new node's callback; matrices and flags are copied as
    /// they are, as is `up`. (Object3D.copy also takes name/layers/
    /// visibility: presentation state this type does not hold.)
    pub fn clone_node(&mut self, source: NodeId) -> NodeId {
        let s = self.node(source);
        let mut node = TransformNode {
            position: s.position,
            scale: s.scale,
            up: s.up,
            matrix: s.matrix,
            matrix_world: s.matrix_world,
            matrix_auto_update: s.matrix_auto_update,
            matrix_world_auto_update: s.matrix_world_auto_update,
            matrix_world_needs_update: s.matrix_world_needs_update,
            is_camera_or_light: s.is_camera_or_light,
            ..TransformNode::default()
        };
        let source_quaternion = s.quaternion;
        node.rotation.order = s.rotation.order;
        node.copy_quaternion(&source_quaternion);
        self.add_node(node)
    }

    /// `applyMatrix4(matrix)` on a node.
    pub fn apply_matrix4(&mut self, id: NodeId, matrix: &Matrix4) {
        let node = self.node_mut(id);
        if node.matrix_auto_update {
            node.update_matrix();
        }
        node.matrix.premultiply(matrix);
        let m = node.matrix;
        node.decompose_from(&m);
    }

    /// `applyQuaternion(q)`.
    pub fn apply_quaternion(&mut self, id: NodeId, q: &Quaternion) {
        let q = *q;
        self.node_mut(id).with_quaternion(|quat| {
            quat.premultiply(&q);
        });
    }

    /// `setRotationFromAxisAngle(axis, angle)`.
    pub fn set_rotation_from_axis_angle(&mut self, id: NodeId, axis: &Vector3, angle: f64) {
        let axis = *axis;
        self.node_mut(id).with_quaternion(|q| {
            q.set_from_axis_angle(&axis, angle);
        });
    }

    /// `setRotationFromEuler(euler)`.
    pub fn set_rotation_from_euler(&mut self, id: NodeId, euler: &Euler) {
        let euler = *euler;
        self.node_mut(id).with_quaternion(|q| {
            q.set_from_euler(&euler);
        });
    }

    /// `setRotationFromMatrix(m)`.
    pub fn set_rotation_from_matrix(&mut self, id: NodeId, m: &Matrix4) {
        let m = *m;
        self.node_mut(id).with_quaternion(|q| {
            q.set_from_rotation_matrix(&m);
        });
    }

    /// `setRotationFromQuaternion(q)`.
    pub fn set_rotation_from_quaternion(&mut self, id: NodeId, q: &Quaternion) {
        self.node_mut(id).copy_quaternion(q);
    }

    /// `rotateOnAxis(axis, angle)`: object space; axis assumed
    /// normalized.
    pub fn rotate_on_axis(&mut self, id: NodeId, axis: &Vector3, angle: f64) {
        let mut q1 = Quaternion::default();
        q1.set_from_axis_angle(axis, angle);
        self.node_mut(id).with_quaternion(|q| {
            q.multiply(&q1);
        });
    }

    /// `rotateOnWorldAxis(axis, angle)`.
    pub fn rotate_on_world_axis(&mut self, id: NodeId, axis: &Vector3, angle: f64) {
        let mut q1 = Quaternion::default();
        q1.set_from_axis_angle(axis, angle);
        self.node_mut(id).with_quaternion(|q| {
            q.premultiply(&q1);
        });
    }

    pub fn rotate_x(&mut self, id: NodeId, angle: f64) {
        self.rotate_on_axis(id, &Vector3::new(1.0, 0.0, 0.0), angle);
    }

    pub fn rotate_y(&mut self, id: NodeId, angle: f64) {
        self.rotate_on_axis(id, &Vector3::new(0.0, 1.0, 0.0), angle);
    }

    pub fn rotate_z(&mut self, id: NodeId, angle: f64) {
        self.rotate_on_axis(id, &Vector3::new(0.0, 0.0, 1.0), angle);
    }

    /// `translateOnAxis(axis, distance)`.
    pub fn translate_on_axis(&mut self, id: NodeId, axis: &Vector3, distance: f64) {
        let node = self.node_mut(id);
        let mut v = *axis;
        let q = node.quaternion;
        v.apply_quaternion(&q);
        v.multiply_scalar(distance);
        node.position.add(&v);
    }

    pub fn translate_x(&mut self, id: NodeId, distance: f64) {
        self.translate_on_axis(id, &Vector3::new(1.0, 0.0, 0.0), distance);
    }

    pub fn translate_y(&mut self, id: NodeId, distance: f64) {
        self.translate_on_axis(id, &Vector3::new(0.0, 1.0, 0.0), distance);
    }

    pub fn translate_z(&mut self, id: NodeId, distance: f64) {
        self.translate_on_axis(id, &Vector3::new(0.0, 0.0, 1.0), distance);
    }

    /// `localToWorld(vector)`.
    pub fn local_to_world<'t>(&mut self, id: NodeId, vector: &'t mut Vector3) -> &'t mut Vector3 {
        self.update_world_matrix(id, true, false);
        let m = self.node(id).matrix_world;
        vector.apply_matrix4(&m)
    }

    /// `worldToLocal(vector)`.
    pub fn world_to_local<'t>(&mut self, id: NodeId, vector: &'t mut Vector3) -> &'t mut Vector3 {
        self.update_world_matrix(id, true, false);
        let mut m = self.node(id).matrix_world;
        m.invert();
        vector.apply_matrix4(&m)
    }

    /// `lookAt(target)`: does not support non-uniformly-scaled
    /// parents, as three says.
    pub fn look_at(&mut self, id: NodeId, target: &Vector3) {
        let target = *target;
        let parent = self.node(id).parent;
        self.update_world_matrix(id, true, false);
        let mut position = Vector3::default();
        position.set_from_matrix_position(&self.node(id).matrix_world);
        let mut m1 = Matrix4::default();
        let up = self.node(id).up;
        if self.node(id).is_camera_or_light {
            m1.look_at(&position, &target, &up);
        } else {
            m1.look_at(&target, &position, &up);
        }
        self.node_mut(id).with_quaternion(|q| {
            q.set_from_rotation_matrix(&m1);
        });
        if let Some(parent) = parent {
            let parent_world = self.node(parent).matrix_world;
            let mut m1 = Matrix4::default();
            m1.extract_rotation(&parent_world);
            let mut q1 = Quaternion::default();
            q1.set_from_rotation_matrix(&m1);
            q1.invert();
            self.node_mut(id).with_quaternion(|q| {
                q.premultiply(&q1);
            });
        }
    }

    /// `parent.attach(child)`: reparents while keeping the child's
    /// world transform.
    pub fn attach(&mut self, parent: NodeId, child: NodeId) {
        self.update_world_matrix(parent, true, false);
        let mut m1 = self.node(parent).matrix_world;
        m1.invert();
        if let Some(old_parent) = self.node(child).parent {
            self.update_world_matrix(old_parent, true, false);
            let old_world = self.node(old_parent).matrix_world;
            m1.multiply(&old_world);
        }
        self.apply_matrix4(child, &m1);
        self.remove_from_parent(child);
        self.nodes[child as usize].parent = Some(parent);
        self.nodes[parent as usize].children.push(child);
        self.update_world_matrix(child, false, true);
    }

    /// `getWorldPosition(target)`.
    pub fn get_world_position(&mut self, id: NodeId, target: &mut Vector3) -> Vector3 {
        self.update_world_matrix(id, true, false);
        let m = self.node(id).matrix_world;
        target.set_from_matrix_position(&m);
        *target
    }

    /// `getWorldQuaternion(target)`: through decompose, as three
    /// does (so a negative determinant flips a scale first). The
    /// target is a plain quaternion, not a node's.
    pub fn get_world_quaternion(&mut self, id: NodeId, target: &mut Quaternion) -> Quaternion {
        self.update_world_matrix(id, true, false);
        let m = self.node(id).matrix_world;
        let (mut position, mut scale) = (Vector3::default(), Vector3::default());
        m.decompose(&mut position, target, &mut scale);
        *target
    }

    /// `getWorldScale(target)`.
    pub fn get_world_scale(&mut self, id: NodeId, target: &mut Vector3) -> Vector3 {
        self.update_world_matrix(id, true, false);
        let m = self.node(id).matrix_world;
        let mut position = Vector3::default();
        let mut quaternion = Quaternion::default();
        m.decompose(&mut position, &mut quaternion, target);
        *target
    }

    /// `getWorldDirection(target)`.
    pub fn get_world_direction(&mut self, id: NodeId, target: &mut Vector3) -> Vector3 {
        self.update_world_matrix(id, true, false);
        let e = self.node(id).matrix_world.elements;
        target.set(e[8], e[9], e[10]);
        target.normalize();
        *target
    }

    /// `node.traverse(f)`: the node, then each child's subtree in order.
    pub fn traverse(&self, id: NodeId, f: &mut impl FnMut(NodeId)) {
        f(id);
        for &child in &self.node(id).children {
            self.traverse(child, f);
        }
    }

    /// Whether `node` is `ancestor` or hangs from it.
    pub fn hangs_from(&self, node: NodeId, ancestor: NodeId) -> bool {
        let mut n = Some(node);
        while let Some(id) = n {
            if id == ancestor {
                return true;
            }
            n = self.node(id).parent;
        }
        false
    }

    /// `updateMatrixWorld(force)` from a node: composes the local
    /// matrix if asked, multiplies down when needed, then recurses
    /// into children in order, forcing them once this node's world
    /// matrix moved.
    pub fn update_matrix_world(&mut self, id: NodeId, force: bool) {
        let parent_world = self
            .node(id)
            .parent
            .map(|p| self.node(p).matrix_world);
        let mut force = force;
        {
            let node = self.node_mut(id);
            if node.matrix_auto_update {
                node.update_matrix();
            }
            if node.matrix_world_needs_update || force {
                if node.matrix_world_auto_update {
                    match parent_world {
                        None => {
                            let m = node.matrix;
                            node.matrix_world.copy(&m);
                        }
                        Some(pw) => {
                            let m = node.matrix;
                            node.matrix_world.multiply_matrices(&pw, &m);
                        }
                    }
                }
                node.matrix_world_needs_update = false;
                force = true;
            }
        }
        let mut i = 0;
        while i < self.node(id).children.len() {
            let child = self.node(id).children[i];
            self.update_matrix_world(child, force);
            i += 1;
        }
    }

    /// `updateWorldMatrix(updateParents, updateChildren)`. Unlike
    /// updateMatrixWorld it leaves `matrixWorldNeedsUpdate` as
    /// `updateMatrix` set it.
    pub fn update_world_matrix(&mut self, id: NodeId, update_parents: bool, update_children: bool) {
        let parent = self.node(id).parent;
        if update_parents {
            if let Some(p) = parent {
                self.update_world_matrix(p, true, false);
            }
        }
        let parent_world = parent.map(|p| self.node(p).matrix_world);
        let node = self.node_mut(id);
        if node.matrix_auto_update {
            node.update_matrix();
        }
        if node.matrix_world_auto_update {
            match parent_world {
                None => {
                    let m = node.matrix;
                    node.matrix_world.copy(&m);
                }
                Some(pw) => {
                    let m = node.matrix;
                    node.matrix_world.multiply_matrices(&pw, &m);
                }
            }
        }
        if update_children {
            let mut i = 0;
            while i < self.node(id).children.len() {
                let child = self.node(id).children[i];
                self.update_world_matrix(child, false, true);
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A child's world matrix is its parent's times its own, and forcing an
    // update from the root reaches every descendant.
    #[test]
    fn world_matrices_chain() {
        let mut tree = TransformTree::new();
        let root = tree.add_node(TransformNode::default());
        let child = tree.add_node(TransformNode::default());
        tree.add_child(root, child);
        tree.node_mut(root).position.set(1.0, 2.0, 3.0);
        tree.node_mut(child).position.set(0.5, 0.0, 0.0);
        tree.update_matrix_world(root, true);
        let mut p = Vector3::default();
        p.set_from_matrix_position(&tree.node(child).matrix_world);
        assert_eq!(p, Vector3::new(1.5, 2.0, 3.0));
        assert!(!tree.node(child).matrix_world_needs_update);
        tree.node_mut(root).position.x = 2.0;
        assert_eq!(tree.get_world_position(child, &mut p), Vector3::new(2.5, 2.0, 3.0));
        // updateWorldMatrix leaves the flag updateMatrix set.
        assert!(tree.node(child).matrix_world_needs_update);
    }

    // A write to one part of the rotation keeps the others as the last
    // quaternion write left them (worked out then, not before), and with
    // euler_sync off the rotation keeps what it last held.
    #[test]
    fn euler_follows_quaternion_writes() {
        let mut node = TransformNode::default();
        node.set_quaternion(0.0, 0.0, 0.0, 1.0);
        node.set_rotation_y(0.0);
        assert_eq!(node.quaternion, Quaternion::new(0.0, 0.0, 0.0, 1.0));
        node.euler_sync = false;
        node.set_quaternion(1.0, 0.0, 0.0, 0.0);
        assert_eq!(node.rotation(), Euler::default());
    }
}
