//! A soldier's body as src/actor.js buildActor builds it, the parts the
//! simulation reads: the skeleton (each bone's local matrix decomposed into
//! position, quaternion and scale, as the bones are posed at bind; the
//! world matrices `body.bind(skeleton)` computes; the skeleton's inverses),
//! the packed vertex positions and each vertex's bone (9 floats a vertex in
//! the file: position, normal, uv, bone), and the hand bone weapons hang
//! from. The headless world builds these too: hit shapes, droideka barrels
//! and rider poses read them.

use crate::js::{relative_range, to_uint16, typed_get};
use crate::three::{Matrix4, Quaternion, Vector3};
use crate::value::Value;

#[derive(Clone, Debug)]
pub struct ActorBone {
    pub name: String,
    /// The record's parent (-1 for none), as written.
    pub parent: f64,
    pub local: [f64; 16],
    /// Matrix4.fromArray(local).decompose(position, quaternion, scale).
    pub position: Vector3,
    pub quaternion: Quaternion,
    pub scale: Vector3,
    /// matrixWorld after `body.bind(skeleton)` (identity for a bone the
    /// body's tree does not reach).
    pub world: Matrix4,
    /// skeleton.boneInverses[i].
    pub inverse: Matrix4,
}

#[derive(Clone, Debug)]
pub struct Actor {
    pub def: Value,
    pub bones: Vec<ActorBone>,
    /// The body's matrixWorld at bind (bindMatrix).
    pub bind_matrix: Matrix4,
    /// Packed positions, 3 a vertex (a Float32Array).
    pub positions: Vec<f32>,
    /// Each vertex's bone (skinIndex x, a Uint16Array of 4 a vertex; the
    /// other three are 0).
    pub skin_index: Vec<u16>,
    /// The bone weapons hang from (`actor.attach`).
    pub attach: Option<String>,
}

fn num(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => *n,
        Some(other) => crate::js::to_number(Some(other)),
        None => f64::NAN,
    }
}

impl Actor {
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn position(&self, i: usize) -> Vector3 {
        Vector3::new(self.positions[i * 3] as f64, self.positions[i * 3 + 1] as f64, self.positions[i * 3 + 2] as f64)
    }

    /// The first bone with this name in lower case (`bones.find`).
    pub fn bone_by_lower_name(&self, name: &str) -> Option<usize> {
        let lower = name.to_lowercase();
        self.bones.iter().position(|b| b.name.to_lowercase() == lower)
    }

    /// `byName[name]`: the last bone with the name (later assignments win).
    pub fn bone_by_name(&self, name: &str) -> Option<usize> {
        self.bones.iter().rposition(|b| b.name == name)
    }
}

/// buildActor(actor, actorBuf, actorIndexBuf): the simulation's parts.
pub fn build_actor(def: &Value, verts: &[f32]) -> Result<Actor, String> {
    let records = def.get("bones").and_then(Value::as_array).ok_or("actor has no bones")?;
    let mut bones: Vec<ActorBone> = Vec::with_capacity(records.len());
    for desc in records {
        let local_v = desc.get("local").and_then(Value::as_array).ok_or("bone without local")?;
        let mut local = [f64::NAN; 16];
        for (i, slot) in local.iter_mut().enumerate() {
            *slot = local_v.get(i).map(|v| num(Some(v))).unwrap_or(f64::NAN);
        }
        let (position, quaternion, scale) = Matrix4::from_array(&local).decompose();
        bones.push(ActorBone {
            name: desc.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            parent: num(desc.get("parent")),
            local,
            position,
            quaternion,
            scale,
            world: Matrix4::identity(),
            inverse: Matrix4::identity(),
        });
    }
    // The tree: bones[parent].add(bones[i]) in order, then body.add(bones[0])
    // (add takes a child from its old parent first). Node n = bones.len() is
    // the body.
    let n = bones.len();
    let body = n;
    let mut parent_of: Vec<Option<usize>> = vec![None; n + 1];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n + 1];
    let add = |parent: usize, child: usize, parent_of: &mut Vec<Option<usize>>, children: &mut Vec<Vec<usize>>| {
        if let Some(old) = parent_of[child] {
            children[old].retain(|&c| c != child);
        }
        parent_of[child] = Some(parent);
        children[parent].push(child);
    };
    for i in 0..n {
        let parent = bones[i].parent;
        if parent >= 0.0 {
            if parent.fract() != 0.0 || parent as usize >= n {
                return Err(format!("bone {i}: parent {parent} is not a bone"));
            }
            add(parent as usize, i, &mut parent_of, &mut children);
        }
    }
    if n > 0 {
        add(body, 0, &mut parent_of, &mut children);
    }
    // body.updateMatrixWorld(true): the body's matrix (identity pose) under
    // the root's matrixWorld (never updated: identity), then down the tree.
    let body_matrix = Matrix4::compose(&Vector3::new(0.0, 0.0, 0.0), &Quaternion::default(), &Vector3::new(1.0, 1.0, 1.0));
    let body_world = Matrix4::multiply_matrices(&Matrix4::identity(), &body_matrix);
    let mut stack: Vec<(usize, Matrix4)> = children[body].iter().rev().map(|&c| (c, body_world)).collect();
    while let Some((i, parent_world)) = stack.pop() {
        let b = &bones[i];
        let matrix = Matrix4::compose(&b.position, &b.quaternion, &b.scale);
        let world = Matrix4::multiply_matrices(&parent_world, &matrix);
        bones[i].world = world;
        for &c in children[i].iter().rev() {
            stack.push((c, world));
        }
    }
    for b in &mut bones {
        b.inverse = b.world.inverted();
    }
    // The packed geometry.
    let parts = def.get("parts").and_then(Value::as_array).ok_or("actor has no parts")?;
    let mut count = 0.0;
    for part in parts {
        count += num(part.get("vertexCount"));
    }
    let count = crate::js::to_integer_or_infinity(count);
    if !(0.0..=1e9).contains(&count) {
        return Err("bad vertex count".into());
    }
    let count = count as usize;
    let mut positions = vec![0f32; count * 3];
    let mut skin_index = vec![0u16; count * 4];
    let mut cursor = 0usize;
    for part in parts {
        let source = num(part.get("vertexOffset")) / 4.0;
        let vertex_count = num(part.get("vertexCount"));
        let mut i = 0usize;
        while (i as f64) < vertex_count {
            let at = source + (i * 9) as f64;
            let (from, to) = relative_range(verts.len(), at, Some(at + 3.0));
            let src = &verts[from..to];
            let dst = cursor * 3;
            if dst + src.len() > positions.len() {
                return Err("offset is out of bounds".into());
            }
            positions[dst..dst + src.len()].copy_from_slice(src);
            let bone = typed_get(verts, at + 8.0).map(|f| f as f64).unwrap_or(f64::NAN);
            if cursor * 4 < skin_index.len() {
                skin_index[cursor * 4] = to_uint16(bone);
            }
            cursor += 1;
            i += 1;
        }
    }
    let attach = def.get("attach").and_then(Value::as_str).map(str::to_string);
    Ok(Actor { def: def.clone(), bones, bind_matrix: body_world, positions, skin_index, attach })
}
