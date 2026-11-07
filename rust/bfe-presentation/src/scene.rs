//! Retained numeric trees for the page's placed rigs. Three supplies resource
//! shells at birth; subsequent local/world matrices and attached skin inverses
//! are published here, without a second browser scene walk.
use crate::layout::scene_node as n;
use serde::Deserialize;
use three::{NodeId, Quaternion, TransformNode, TransformTree, Vector3};

#[derive(Deserialize)]
// Local matrices matter for placed roots whose TRS fields are deliberately
// unrelated to placement. Other nodes retain the normal authored TRS rule.
pub struct Node {
    pub parent: i32,
    pub pose: [f64; 10],
    pub matrix: [f64; 16],
    pub auto: bool,
    pub skinned: bool,
}
#[derive(Deserialize, Default)]
pub struct Input {
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub bones: Vec<usize>,
    #[serde(default)]
    pub parent: Option<[f64;16]>,
}
// Only registration owns vectors. All frame operations reuse the tree and
// its fixed output range, including attached skin inverses.
pub struct Scene {
    pub tree: TransformTree,
    pub nodes: Vec<NodeId>,
    pub bones: Vec<usize>,
    skinned: Vec<bool>,
    pub start: usize,
    first: bool,
    parent: Option<NodeId>,
}
impl Scene {
    /// buildRig's complete tree, in parent-before-child order. Invalid
    /// parents become roots; neither a cycle nor a stale bone can panic.
    pub fn new(input: Input, output: &mut Vec<f64>) -> Self {
        let mut tree = TransformTree::new();
        let mut nodes = Vec::with_capacity(input.nodes.len());
        let mut skinned = Vec::with_capacity(input.nodes.len());
        // The resource traversal gives parents before children. A malformed
        // parent is detached rather than creating a cycle in the frame walk.
        for (i, d) in input.nodes.into_iter().enumerate() {
            let p = d.pose;
            let mut node = TransformNode::placed(Vector3::new(p[0], p[1], p[2]), Vector3::new(p[7], p[8], p[9]));
            // Preserve the quaternion as authored, without normalization:
            // prop easing intentionally stores its components through f32.
            node.copy_quaternion(&Quaternion::new(p[3], p[4], p[5], p[6]));
            node.matrix.from_array(&d.matrix, 0);
            node.matrix_auto_update = d.auto;
            let id = tree.add_node(node);
            if d.parent >= 0 && (d.parent as usize) < i {
                tree.add_child(nodes[d.parent as usize], id);
            }
            nodes.push(id);
            skinned.push(d.skinned);
        }
        // Placed roots still multiply by the scene's matrix. Copying the
        // root matrix instead would change signed zeros at this boundary.
        let parent = input.parent.map(|matrix| {
            let mut node=TransformNode::default();node.matrix.from_array(&matrix,0);node.matrix_auto_update=false;
            let parent=tree.add_node(node);
            for &id in &nodes {if tree.node(id).parent.is_none() {tree.add_child(parent,id);}}
            parent
        });
        // Offsets survive later registrations even if the backing allocation
        // moves. The browser rebinds its typed view at the phase boundary.
        let start = output.len();
        output.resize(start + nodes.len() * n::STRIDE, 0.0);
        Self { tree, nodes, bones: input.bones, skinned, start, first: true, parent }
    }
    /// PropBone records replace just the posed bones' transforms. Scale and
    /// matrix-authored roots remain as buildRig made them.
    pub fn pose(&mut self, bones: &[f64]) {
        for (i, &index) in self.bones.iter().enumerate() {
            let Some(&id) = self.nodes.get(index) else { continue };
            // A stale/missing bone handle leaves that node at its prior pose.
            // No frame path can index beyond the registered transform range.
            let Some(p) = bones.get(i * 7..i * 7 + 7) else { continue };
            let node = self.tree.node_mut(id);
            node.set_quaternion(p[0], p[1], p[2], p[3]);
            node.position.set(p[4], p[5], p[6]);
        }
    }
    /// The phase publishes only changed rows. Attached-mode bind inverses
    /// follow each mesh's world matrix, including meshes below an aimer.
    pub fn publish(&mut self, output: &mut [f64]) {
        if let Some(parent)=self.parent {self.tree.update_matrix_world(parent,true);}
        for &id in &self.nodes {
            if self.tree.node(id).parent.is_none() {
                self.tree.update_matrix_world(id, true);
            }
        }
        for (i, &id) in self.nodes.iter().enumerate() {
            let node = self.tree.node(id);
            let out = &mut output[self.start + i * n::STRIDE..][..n::STRIDE];
            // Local and world matrices can change independently when a
            // parent moves. Compare both before touching the host's shells.
            let changed = self.first || out[n::MATRIX..n::MATRIX + 16] != node.matrix.elements
                || out[n::WORLD..n::WORLD + 16] != node.matrix_world.elements;
            out[n::CHANGED] = changed as u8 as f64;
            if !changed { continue; }
            out[n::MATRIX..n::MATRIX + 16].copy_from_slice(&node.matrix.elements);
            out[n::WORLD..n::WORLD + 16].copy_from_slice(&node.matrix_world.elements);
            // Three attached mode binds through the current mesh frame,
            // rather than a single inverse shared by the entire skeleton.
            if self.skinned[i] {
                let mut inverse = node.matrix_world;
                inverse.invert();
                out[n::BIND_INVERSE..n::BIND_INVERSE + 16].copy_from_slice(&inverse.elements);
            }
        }
        self.first = false;
    }
}
