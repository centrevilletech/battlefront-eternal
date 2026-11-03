//! A body's skeleton as the game builds it from its model (src/actor.js
//! buildActor; src/vehicles.js does the same for a hull's rig): each bone's
//! bind pose is its local matrix from the model, decomposed by three into a
//! position, quaternion and scale, and each bone hangs from its parent.
//!
//! What the simulation reads of a soldier's skeleton is its bind pose (a
//! world never poses a soldier's drawn bones: `Unit.present` is a page's),
//! its names and its parents: the rider's points (`rider.rs`) and a
//! droideka's hidden gun rig (`droideka.rs`) are made from it.

use super::three::{Euler, Matrix4, Quaternion, Vector3};
use super::tree::TransformNode;

/// A bone of a model: its name, its parent's index (negative for none) and
/// its local matrix (column-major, as the model's JSON gives it).
#[derive(Clone, Debug)]
pub struct BoneDesc {
    pub name: String,
    pub parent: i32,
    pub local: [f64; 16],
}

/// The skeleton of one model: per bone, its name, its parent and its bind
/// pose, by bone index.
#[derive(Clone, Debug)]
pub struct Skeleton {
    pub names: Vec<String>,
    /// The parent's index, -1 for none (as `bones.indexOf(bone.parent)`
    /// finds it: a bone hung from the body's mesh has none).
    pub parents: Vec<i32>,
    /// Each bone as buildActor leaves it: `fromArray(local).decompose(...)`
    /// into its position, quaternion and scale, the quaternion's change
    /// callback having synced its Euler once, before buildActor switches
    /// that callback off (src/actor.js NO_EULER) where `euler_sync` is
    /// false. Its matrices are the identity, as a new Bone's.
    pub bind: Vec<TransformNode>,
}

impl Skeleton {
    /// The skeleton of a model's bones. `euler_sync`: whether the bones keep
    /// three's Euler callback (a hull's rig does; a soldier's or droideka's
    /// actor switches it off after the decompose).
    pub fn new(bones: &[BoneDesc], euler_sync: bool) -> Skeleton {
        let mut bind = Vec::with_capacity(bones.len());
        for desc in bones {
            let mut node = TransformNode::default();
            let mut m = Matrix4::default();
            m.from_array(&desc.local, 0);
            node.decompose_from(&m);
            node.euler_sync = euler_sync;
            bind.push(node);
        }
        Skeleton {
            names: bones.iter().map(|b| b.name.clone()).collect(),
            parents: bones.iter().map(|b| if b.parent >= 0 { b.parent } else { -1 }).collect(),
            bind,
        }
    }

    /// A soldier's or droideka's skeleton as buildActor makes it: the
    /// bones' Euler callback switched off after their decompose, and the
    /// first bone hung from the body's mesh (so it has no parent bone, as
    /// `bones.indexOf(bone.parent)` finds), whatever its model says.
    pub fn actor(bones: &[BoneDesc]) -> Skeleton {
        let mut skeleton = Skeleton::new(bones, false);
        if let Some(first) = skeleton.parents.first_mut() {
            *first = -1;
        }
        skeleton
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// `byName[name]` of an object built bone by bone (src/actor.js,
    /// src/rider-pose.js): the last bone of that exact name.
    pub fn by_name(&self, name: &str) -> Option<usize> {
        self.names.iter().rposition(|n| n == name)
    }

    /// `bones.findIndex((bone) => bone.name.toLowerCase() === name)`
    /// (src/droideka.js): the first bone whose lower-cased name is `name`.
    pub fn find_lower(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| lower(n) == name)
    }

    /// The bind position of a bone.
    pub fn bind_position(&self, i: usize) -> Vector3 {
        self.bind[i].position
    }

    /// The bind quaternion of a bone.
    pub fn bind_quaternion(&self, i: usize) -> Quaternion {
        self.bind[i].quaternion
    }

    /// The bind Euler of a bone (as three holds it; nothing in the
    /// simulation reads it).
    pub fn bind_rotation(&mut self, i: usize) -> Euler {
        self.bind[i].rotation()
    }
}

/// `String.prototype.toLowerCase` for the ASCII names models carry (a
/// non-ASCII name is lowered by Unicode's rules, as JavaScript lowers it).
pub fn lower(s: &str) -> String {
    if s.is_ascii() {
        s.to_ascii_lowercase()
    } else {
        s.to_lowercase()
    }
}
