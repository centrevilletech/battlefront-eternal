//! Small static models as src/models.js modelPack builds them from a pack's
//! records and buffers (weapons.bin, a vehicle pack's models.bin and
//! extras.bin, props.bin): each part's vertices (position, normal, uv: 8
//! floats), its indices rebased to the part, its bounding sphere as
//! BufferGeometry.computeBoundingSphere makes it, and its record (texture,
//! flags, bone, joints...). The simulation reads the positions and spheres:
//! item radii and pickup centres, muzzles, wheels, gear feet.

use crate::js::{relative_range, to_integer_or_infinity, to_uint32, typed_get};
use crate::three::{bounding_box, bounding_sphere, Box3, Positions, Vector3};
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct Part {
    /// `floats.slice(base, base + vertexCount * 8)`.
    pub data: Vec<f32>,
    /// The part's indices less its first vertex (a Uint32Array).
    pub index: Vec<u32>,
    /// computeBoundingSphere's centre and radius.
    pub sphere_center: Vector3,
    pub sphere_radius: f64,
    /// The pack's record for the part (texture, additive, transparent,
    /// flags, scroll, bone, joints, skinJoints, skinWeights).
    pub record: Value,
}

/// The fields modelPack copies from a part's record, in its order.
pub const PART_FIELDS: [&str; 9] = ["texture", "additive", "transparent", "flags", "scroll", "bone", "joints", "skinJoints", "skinWeights"];

impl Part {
    pub fn field(&self, key: &str) -> Option<&Value> {
        self.record.get(key)
    }

    /// The part's bone name, when it has one.
    pub fn bone(&self) -> Option<&str> {
        self.record.get("bone").and_then(Value::as_str)
    }

    /// The interleaved buffer's vertex count, `array.length / stride` (a
    /// loop `i < count` runs ceil of it).
    pub fn count(&self) -> usize {
        self.data.len().div_ceil(8)
    }

    /// Vertex `i`'s position (getX/getY/getZ; an element past the end reads
    /// undefined, which arithmetic sees as NaN).
    pub fn position(&self, i: usize) -> Vector3 {
        let at = |k: usize| self.data.get(i * 8 + k).map(|&f| f as f64).unwrap_or(f64::NAN);
        Vector3::new(at(0), at(1), at(2))
    }

    pub fn normal(&self, i: usize) -> Vector3 {
        let at = |k: usize| self.data.get(i * 8 + k).map(|&f| f as f64).unwrap_or(f64::NAN);
        Vector3::new(at(3), at(4), at(5))
    }

    pub fn uv(&self, i: usize) -> (f64, f64) {
        let at = |k: usize| self.data.get(i * 8 + k).map(|&f| f as f64).unwrap_or(f64::NAN);
        (at(6), at(7))
    }

    /// computeBoundingBox over the part's positions.
    pub fn bounding_box(&self) -> Box3 {
        bounding_box(&PartPositions(self))
    }
}

pub struct PartPositions<'a>(pub &'a Part);

impl Positions for PartPositions<'_> {
    fn count(&self) -> usize {
        self.0.count()
    }
    fn get(&self, i: usize) -> Vector3 {
        self.0.position(i)
    }
}

/// A model: its parts, shared wherever the JavaScript shares the array.
pub type Model = Rc<Vec<Part>>;

/// Models by name, in insertion order (a JavaScript Map).
#[derive(Clone, Debug, Default)]
pub struct ModelMap {
    order: Vec<(String, Model)>,
    index: HashMap<String, usize>,
}

impl ModelMap {
    pub fn get(&self, name: &str) -> Option<&Model> {
        self.index.get(name).map(|&i| &self.order[i].1)
    }

    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Map.set: an existing name keeps its place.
    pub fn set(&mut self, name: &str, model: Model) {
        match self.index.get(name) {
            Some(&i) => self.order[i].1 = model,
            None => {
                self.index.insert(name.to_string(), self.order.len());
                self.order.push((name.to_string(), model));
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Model)> {
        self.order.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

fn number(v: Option<&Value>) -> f64 {
    // The record's numbers as arithmetic reads them (undefined is NaN).
    match v {
        Some(Value::Number(n)) => *n,
        Some(other) => crate::js::to_number(Some(other)),
        None => f64::NAN,
    }
}

/// src/models.js modelPack(records, buffer, indexBuffer).
pub fn model_pack(records: Option<&Value>, floats: &[f32], indices: &[u32]) -> Result<ModelMap, String> {
    let mut models = ModelMap::default();
    let records = match records.filter(|r| r.truthy()).and_then(Value::as_object) {
        Some(o) => o,
        None => return Ok(models),
    };
    for (name, parts) in records.entries() {
        let list = parts.as_array().ok_or_else(|| format!("model {name}: parts is not an array"))?;
        let mut out = Vec::with_capacity(list.len());
        for part in list {
            let vertex_offset = number(part.get("vertexOffset"));
            let vertex_count = number(part.get("vertexCount"));
            let base = vertex_offset / 4.0;
            let (from, to) = relative_range(floats.len(), base, Some(base + vertex_count * 8.0));
            let data = floats[from..to].to_vec();
            let first = number(part.get("indexOffset")) / 4.0;
            let vertex_base = vertex_offset / 32.0;
            let index_count = number(part.get("indexCount"));
            let length = to_integer_or_infinity(index_count);
            if !(0.0..=9007199254740991.0).contains(&length) {
                return Err(format!("model {name}: invalid typed array length"));
            }
            let mut index = vec![0u32; length as usize];
            let mut k = 0usize;
            while (k as f64) < index_count {
                let read = typed_get(indices, first + k as f64).map(|x| x as f64).unwrap_or(f64::NAN);
                if k < index.len() {
                    index[k] = to_uint32(read - vertex_base);
                }
                k += 1;
            }
            let mut p = Part { data, index, sphere_center: Vector3::default(), sphere_radius: 0.0, record: part.clone() };
            let (center, radius) = bounding_sphere(&PartPositions(&p));
            p.sphere_center = center;
            p.sphere_radius = radius;
            out.push(p);
        }
        models.set(name, Rc::new(out));
    }
    Ok(models)
}
