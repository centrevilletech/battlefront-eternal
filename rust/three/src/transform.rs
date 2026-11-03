//! `Object3D`'s transform semantics (r170 `src/core/Object3D.js`),
//! as a tree of nodes by index: the rigs, hulls, seats and bones
//! the sim poses are Object3D hierarchies, and their world matrices
//! are behaviour. Only the transform half of Object3D is here —
//! no events, layers, visibility or rendering state.
//!
//! The Euler/quaternion sync is the subtle part. In three, an
//! Object3D's `rotation` (an Euler) and `quaternion` keep each
//! other current through change callbacks: any write through an
//! Euler setter or method runs `quaternion.setFromEuler(rotation)`,
//! and any write through a Quaternion setter or method runs
//! `rotation.setFromQuaternion(quaternion, undefined)` (keeping the
//! Euler's current order). Code reads whichever it likes —
//! including reading a flyer root's XYZ-Euler `.y` as a heading
//! after the quaternion was written, which gives three's synced
//! (and past 90 degrees, surprising) value. This type reproduces
//! the sync: write rotation through `set_rotation*` and quaternion
//! through `set_quaternion*` (and the tree's transform methods),
//! which sync the sibling exactly where three's callbacks fire.
//! The fields are public for reading and for the writes three
//! makes without a callback in flight; a ported line that assigns
//! `node.rotation.x = v` in JavaScript is `set_rotation_x(v)`
//! here, and one that mutates the quaternion object in place
//! (`node.quaternion.multiply(q)`) ends with
//! [`TransformNode::sync_euler_from_quaternion`] — the tree methods
//! below already do this at the same points three does.

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
    /// The synced Euler; write through `set_rotation*` (see the
    /// module notes).
    pub rotation: Euler,
    /// The synced quaternion; write through `set_quaternion*`.
    pub quaternion: Quaternion,
    pub scale: Vector3,
    /// `up`, default (0, 1, 0).
    pub up: Vector3,
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
    pub children: Vec<NodeId>,
}

impl Default for TransformNode {
    fn default() -> Self {
        TransformNode {
            position: Vector3::default(),
            rotation: Euler::default(),
            quaternion: Quaternion::default(),
            scale: Vector3::new(1.0, 1.0, 1.0),
            up: Vector3::new(0.0, 1.0, 0.0),
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
    /// three's `onRotationChange`: `quaternion.setFromEuler(rotation, false)`.
    pub fn sync_quaternion_from_euler(&mut self) {
        let euler = self.rotation;
        self.quaternion.set_from_euler(&euler);
    }

    /// three's `onQuaternionChange`:
    /// `rotation.setFromQuaternion(quaternion, undefined, false)` —
    /// the undefined order keeps the Euler's current order.
    pub fn sync_euler_from_quaternion(&mut self) {
        let q = self.quaternion;
        self.rotation.set_from_quaternion(&q, None);
    }

    /// `rotation.set(x, y, z, order?)`, with the sync.
    pub fn set_rotation(&mut self, x: f64, y: f64, z: f64, order: Option<EulerOrder>) {
        self.rotation.set(x, y, z, order);
        self.sync_quaternion_from_euler();
    }

    /// `rotation.x = value`, with the sync.
    pub fn set_rotation_x(&mut self, x: f64) {
        self.rotation.x = x;
        self.sync_quaternion_from_euler();
    }

    pub fn set_rotation_y(&mut self, y: f64) {
        self.rotation.y = y;
        self.sync_quaternion_from_euler();
    }

    pub fn set_rotation_z(&mut self, z: f64) {
        self.rotation.z = z;
        self.sync_quaternion_from_euler();
    }

    /// `rotation.order = order`, with the sync.
    pub fn set_rotation_order(&mut self, order: EulerOrder) {
        self.rotation.order = order;
        self.sync_quaternion_from_euler();
    }

    /// `rotation.copy(euler)`, with the sync.
    pub fn copy_rotation(&mut self, euler: &Euler) {
        self.rotation.copy(euler);
        self.sync_quaternion_from_euler();
    }

    /// Mutate the Euler as three code does (any mix of field writes
    /// and method calls), then sync once, where three's callback
    /// would have run after each write. One sync at the end equals
    /// three's per-write syncs because `setFromEuler` reads only
    /// the finished Euler.
    pub fn with_rotation(&mut self, f: impl FnOnce(&mut Euler)) {
        f(&mut self.rotation);
        self.sync_quaternion_from_euler();
    }

    /// `quaternion.set(x, y, z, w)`, with the sync.
    pub fn set_quaternion(&mut self, x: f64, y: f64, z: f64, w: f64) {
        self.quaternion.set(x, y, z, w);
        self.sync_euler_from_quaternion();
    }

    /// `quaternion.copy(q)`, with the sync.
    pub fn copy_quaternion(&mut self, q: &Quaternion) {
        self.quaternion.copy(q);
        self.sync_euler_from_quaternion();
    }

    /// Mutate the quaternion as three code does, then sync the
    /// Euler once (see `with_rotation`).
    pub fn with_quaternion(&mut self, f: impl FnOnce(&mut Quaternion)) {
        f(&mut self.quaternion);
        self.sync_euler_from_quaternion();
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

    /// `applyMatrix4(matrix)` on a node.
    pub fn apply_matrix4(&mut self, id: NodeId, matrix: &Matrix4) {
        let node = self.node_mut(id);
        if node.matrix_auto_update {
            node.update_matrix();
        }
        node.matrix.premultiply(matrix);
        let m = node.matrix;
        let mut q = node.quaternion;
        m.decompose(&mut node.position, &mut q, &mut node.scale);
        node.quaternion = q;
        // decompose's setFromRotationMatrix fires the sync in three.
        node.sync_euler_from_quaternion();
    }

    /// `applyQuaternion(q)`.
    pub fn apply_quaternion(&mut self, id: NodeId, q: &Quaternion) {
        let node = self.node_mut(id);
        node.with_quaternion(|quat| {
            quat.premultiply(q);
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
    pub fn get_world_position<'t>(&mut self, id: NodeId, target: &'t mut Vector3) -> &'t mut Vector3 {
        self.update_world_matrix(id, true, false);
        let m = self.node(id).matrix_world;
        target.set_from_matrix_position(&m)
    }

    /// `getWorldQuaternion(target)`: through decompose, as three
    /// does (so a negative determinant flips a scale first).
    pub fn get_world_quaternion<'t>(&mut self, id: NodeId, target: &'t mut Quaternion) -> &'t mut Quaternion {
        self.update_world_matrix(id, true, false);
        let m = self.node(id).matrix_world;
        let mut position = Vector3::default();
        let mut scale = Vector3::default();
        m.decompose(&mut position, target, &mut scale);
        target
    }

    /// `getWorldScale(target)`.
    pub fn get_world_scale<'t>(&mut self, id: NodeId, target: &'t mut Vector3) -> &'t mut Vector3 {
        self.update_world_matrix(id, true, false);
        let m = self.node(id).matrix_world;
        let mut position = Vector3::default();
        let mut quaternion = Quaternion::default();
        m.decompose(&mut position, &mut quaternion, target);
        target
    }

    /// `getWorldDirection(target)`.
    pub fn get_world_direction<'t>(&mut self, id: NodeId, target: &'t mut Vector3) -> &'t mut Vector3 {
        self.update_world_matrix(id, true, false);
        let e = self.node(id).matrix_world.elements;
        target.set(e[8], e[9], e[10]);
        target.normalize()
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
        let children = self.node(id).children.clone();
        for child in children {
            self.update_matrix_world(child, force);
        }
    }

    /// `updateWorldMatrix(updateParents, updateChildren)`.
    pub fn update_world_matrix(&mut self, id: NodeId, update_parents: bool, update_children: bool) {
        if update_parents {
            if let Some(parent) = self.node(id).parent {
                self.update_world_matrix(parent, true, false);
            }
        }
        self.compose_world(id);
        if update_children {
            let children = self.node(id).children.clone();
            for child in children {
                self.update_world_matrix(child, false, true);
            }
        }
    }

    // The per-node half of updateWorldMatrix: compose local (if
    // auto), then world = parent ? parent.matrixWorld * matrix :
    // matrix, when matrixWorldAutoUpdate. Note three's
    // updateWorldMatrix does not consult matrixWorldNeedsUpdate.
    fn compose_world(&mut self, id: NodeId) {
        let parent = self.node(id).parent;
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
    }
}
