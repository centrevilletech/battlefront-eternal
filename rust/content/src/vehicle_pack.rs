//! Read-only typed reader for 2005 (`GameFamily::Swbf2`) per-map vehicle packs
//! (`maps/<map>/vehicles/vehicles.json`).
//!
//! Parses class collision bindings, named attachment/fire points, seat weapon
//! lists, collision meshes, and collision primitives (sphere, cylinder, box)
//! with their bitmasks into deterministic Rust structs without altering any
//! 2004 (`GameFamily::Swbf1`) vehicle loading path.

mod validate;
pub use validate::{MAX_FILE_BYTES, MAX_SALVO, MAX_SEATS, MAX_WEAPONS};

use crate::js::to_number;
use crate::value::Value;
use std::collections::HashMap;

/// Collision mask bit for soldier hits (`1 << 0`).
pub const COLLISION_MASK_SOLDIER: u32 = 1 << 0;
/// Collision mask bit for vehicle hits (`1 << 1`).
pub const COLLISION_MASK_VEHICLE: u32 = 1 << 1;
/// Collision mask bit for building hits (`1 << 2`).
pub const COLLISION_MASK_BUILDING: u32 = 1 << 2;
/// Collision mask bit for terrain hits (`1 << 3`).
pub const COLLISION_MASK_TERRAIN: u32 = 1 << 3;
/// Collision mask bit for ordnance hits (`1 << 4`).
pub const COLLISION_MASK_ORDNANCE: u32 = 1 << 4;
/// Collision mask bit for flyer hits (`1 << 5`).
pub const COLLISION_MASK_FLYER: u32 = 1 << 5;

/// A single skeleton joint in a 2005 vehicle model.
#[derive(Clone, Debug, PartialEq)]
pub struct Swbf2VehicleJoint {
    pub name: String,
    pub parent: Option<usize>,
    pub local: [f64; 16],
    pub matrix: [f64; 16],
}

/// An authored collision triangle mesh attached to a vehicle model.
#[derive(Clone, Debug, PartialEq)]
pub struct Swbf2VehicleMesh {
    pub node: String,
    pub joint: Option<usize>,
    pub mask: u32,
    pub collides_with: Vec<String>,
    pub positions: Vec<[f32; 3]>,
    pub polygons: Vec<u32>,
    pub indices: Vec<u32>,
}

/// An authored collision primitive (`kind`: 0/1 sphere, 2 cylinder, 4 box)
/// with its model-space transform and pre-tessellated local triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct Swbf2VehiclePrimitive {
    pub name: String,
    pub parent: String,
    pub joint: Option<usize>,
    pub kind: u8,
    pub size: [f64; 3],
    pub matrix: [f64; 16],
    pub mask: u32,
    pub collides_with: Vec<String>,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// One vehicle or turret model entry in `vehicles.json["models"]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Swbf2VehicleModel {
    pub name: String,
    pub skeleton: Vec<Swbf2VehicleJoint>,
    pub meshes: Vec<Swbf2VehicleMesh>,
    pub primitives: Vec<Swbf2VehiclePrimitive>,
}

/// A resolved named attachment, aimer, fire point, or offset point on a class.
#[derive(Clone, Debug, PartialEq)]
pub struct Swbf2VehiclePoint {
    pub property: String,
    pub value: String,
    pub model: Option<String>,
    pub name: Option<String>,
    pub joint: Option<usize>,
    pub parent: Option<String>,
    pub matrix: Option<[f64; 16]>,
    pub offset: Option<[f64; 3]>,
}

impl Swbf2VehiclePoint {
    /// Returns the model-space 3D position of this point, applying `offset`
    /// inside `matrix` when both are authored.
    pub fn model_position(&self) -> Option<[f64; 3]> {
        match (self.matrix, self.offset) {
            (Some(m), Some(off)) => Some([
                m[0] * off[0] + m[4] * off[1] + m[8] * off[2] + m[12],
                m[1] * off[0] + m[5] * off[1] + m[9] * off[2] + m[13],
                m[2] * off[0] + m[6] * off[1] + m[10] * off[2] + m[14],
            ]),
            (Some(m), None) => Some([m[12], m[13], m[14]]),
            (None, Some(off)) => Some(off),
            (None, None) => None,
        }
    }
}

/// A weapon entry inside one seat of a vehicle class.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Swbf2VehicleSeatWeapon {
    pub section: u32,
    pub class: String,
    pub fire_points: Vec<usize>,
    pub extra_offsets: Vec<[f64; 3]>,
}

/// A seat section (`BODY`, `TURRET1`, ...) in a vehicle class.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Swbf2VehicleSeat {
    pub name: String,
    pub points: Vec<usize>,
    pub weapons: Vec<Swbf2VehicleSeatWeapon>,
}

/// One class entry in `vehicles.json["classes"]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Swbf2VehicleClass {
    pub name: String,
    pub base: String,
    pub model: Option<String>,
    pub collision_model: Option<String>,
    pub points: Vec<Swbf2VehiclePoint>,
    pub seats: Vec<Swbf2VehicleSeat>,
}

/// Assembled model-space collision triangles and tight AABB produced from a
/// vehicle model's meshes and primitives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Swbf2CollisionGeometry {
    pub positions: Vec<f32>,
    pub indices: Vec<u32>,
    pub polygons: Vec<u32>,
    pub bounds_min: [f64; 3],
    pub bounds_max: [f64; 3],
    pub mesh_count: usize,
    pub primitive_count: usize,
}

/// Read-only typed view of `maps/<map>/vehicles/vehicles.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Swbf2VehiclePack {
    pub version: u32,
    pub classes: HashMap<String, Swbf2VehicleClass>,
    pub models: HashMap<String, Swbf2VehicleModel>,
    class_lower: HashMap<String, String>,
    model_lower: HashMap<String, String>,
}

const IDENTITY_MATRIX: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

fn read_matrix16(value: Option<&Value>) -> Option<[f64; 16]> {
    let arr = value?.as_array()?;
    if arr.len() < 16 {
        return None;
    }
    let mut out = [0.0; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        let v = to_number(arr.get(i));
        if !v.is_finite() {
            return None;
        }
        *slot = v;
    }
    Some(out)
}

fn read_vec3(value: Option<&Value>) -> Option<[f64; 3]> {
    let arr = value?.as_array()?;
    if arr.len() < 3 {
        return None;
    }
    let v = [
        to_number(arr.first()),
        to_number(arr.get(1)),
        to_number(arr.get(2)),
    ];
    v.iter().all(|x| x.is_finite()).then_some(v)
}

fn read_usize_opt(value: Option<&Value>) -> Option<usize> {
    let v = value?;
    if v.is_null() {
        return None;
    }
    let n = to_number(Some(v));
    (n.is_finite() && n >= 0.0).then_some(n as usize)
}

fn read_vec3_list(value: Option<&Value>) -> Vec<[f32; 3]> {
    let Some(arr) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        if let Some(triple) = item.as_array() {
            if triple.len() >= 3 {
                let x = to_number(triple.first());
                let y = to_number(triple.get(1));
                let z = to_number(triple.get(2));
                if x.is_finite() && y.is_finite() && z.is_finite() {
                    out.push([x as f32, y as f32, z as f32]);
                }
            }
        }
    }
    out
}

fn read_u32_list(value: Option<&Value>) -> Vec<u32> {
    let Some(arr) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            let n = to_number(Some(v));
            (n.is_finite() && n >= 0.0).then_some(n as u32)
        })
        .collect()
}

fn read_string_list(value: Option<&Value>) -> Vec<String> {
    let Some(arr) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

#[inline]
fn transform_point(m: &[f64; 16], p: [f32; 3]) -> [f64; 3] {
    let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    [
        m[0] * x + m[4] * y + m[8] * z + m[12],
        m[1] * x + m[5] * y + m[9] * z + m[13],
        m[2] * x + m[6] * y + m[10] * z + m[14],
    ]
}

fn parse_model(name: &str, rec: &Value) -> Swbf2VehicleModel {
    let skeleton = rec
        .get("skeleton")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|j| Swbf2VehicleJoint {
                    name: j.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    parent: read_usize_opt(j.get("parent")),
                    local: read_matrix16(j.get("local")).unwrap_or(IDENTITY_MATRIX),
                    matrix: read_matrix16(j.get("matrix")).unwrap_or(IDENTITY_MATRIX),
                })
                .collect()
        })
        .unwrap_or_default();

    let meshes = rec
        .get("meshes")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|m| Swbf2VehicleMesh {
                    node: m.get("node").and_then(Value::as_str).unwrap_or("").to_string(),
                    joint: read_usize_opt(m.get("joint")),
                    mask: v8math::max(0.0, to_number(m.get("mask"))) as u32,
                    collides_with: read_string_list(m.get("collidesWith")),
                    positions: read_vec3_list(m.get("positions")),
                    polygons: m.get("polygons").and_then(Value::as_array).unwrap_or_default().iter().enumerate()
                        .flat_map(|(i, face)| std::iter::repeat_n(i as u32, face.as_array().map_or(0, |f| f.len().saturating_sub(2))))
                        .collect(),
                    indices: read_u32_list(m.get("indices")),
                })
                .collect()
        })
        .unwrap_or_default();

    let primitives = rec
        .get("primitives")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|p| Swbf2VehiclePrimitive {
                    name: p.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    parent: p.get("parent").and_then(Value::as_str).unwrap_or("").to_string(),
                    joint: read_usize_opt(p.get("joint")),
                    kind: v8math::max(0.0, to_number(p.get("kind"))) as u8,
                    size: read_vec3(p.get("size")).unwrap_or([0.0; 3]),
                    matrix: read_matrix16(p.get("matrix")).unwrap_or(IDENTITY_MATRIX),
                    mask: v8math::max(0.0, to_number(p.get("mask"))) as u32,
                    collides_with: read_string_list(p.get("collidesWith")),
                    positions: read_vec3_list(p.get("positions")),
                    indices: read_u32_list(p.get("indices")),
                })
                .collect()
        })
        .unwrap_or_default();

    Swbf2VehicleModel {
        name: name.to_string(),
        skeleton,
        meshes,
        primitives,
    }
}

fn parse_class(name: &str, rec: &Value) -> Swbf2VehicleClass {
    let base = rec.get("base").and_then(Value::as_str).unwrap_or("").to_string();
    let model = rec
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let collision_model = rec
        .get("collisionModel")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let points = rec
        .get("points")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|p| Swbf2VehiclePoint {
                    property: p.get("property").and_then(Value::as_str).unwrap_or("").to_string(),
                    value: p.get("value").and_then(Value::as_str).unwrap_or("").to_string(),
                    model: p.get("model").and_then(Value::as_str).map(str::to_string),
                    name: p.get("name").and_then(Value::as_str).map(str::to_string),
                    joint: read_usize_opt(p.get("joint")),
                    parent: p.get("parent").and_then(Value::as_str).map(str::to_string),
                    matrix: read_matrix16(p.get("matrix")),
                    offset: read_vec3(p.get("offset")),
                })
                .collect()
        })
        .unwrap_or_default();

    let seats = rec
        .get("seats")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|s| {
                    let seat_name = s.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let seat_points = read_u32_list(s.get("points"))
                        .into_iter()
                        .map(|i| i as usize)
                        .collect();
                    let weapons = s
                        .get("weapons")
                        .and_then(Value::as_array)
                        .map(|wlist| {
                            wlist
                                .iter()
                                .map(|w| {
                                    let extra_offsets = w
                                        .get("extraOffsets")
                                        .and_then(Value::as_array)
                                        .map(|offs| {
                                            offs.iter()
                                                .filter_map(|o| read_vec3(Some(o)))
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    Swbf2VehicleSeatWeapon {
                                        section: v8math::max(0.0, to_number(w.get("section"))) as u32,
                                        class: w
                                            .get("class")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .to_string(),
                                        fire_points: read_u32_list(w.get("firePoints"))
                                            .into_iter()
                                            .map(|i| i as usize)
                                            .collect(),
                                        extra_offsets,
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Swbf2VehicleSeat {
                        name: seat_name,
                        points: seat_points,
                        weapons,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Swbf2VehicleClass {
        name: name.to_string(),
        base,
        model,
        collision_model,
        points,
        seats,
    }
}

impl Swbf2VehiclePack {
    /// Parses a `maps/<map>/vehicles/vehicles.json` document into a read-only
    /// typed pack. Returns an error if the JSON root is malformed.
    pub fn from_value(value: &Value, source: &str) -> Result<Self, String> {
        validate::pack(value).map_err(|e| format!("{source}: {e}"))?;
        let obj = value
            .as_object()
            .ok_or_else(|| format!("{source}: vehicle pack root must be a JSON object"))?;
        let version = v8math::max(0.0, to_number(obj.get("version"))) as u32;
        let mut classes = HashMap::new();
        let mut class_lower = HashMap::new();
        if let Some(cls_map) = obj.get("classes").and_then(Value::as_object) {
            for (k, v) in cls_map.entries() {
                let parsed = parse_class(k, v);
                class_lower.insert(k.to_ascii_lowercase(), k.to_string());
                classes.insert(k.to_string(), parsed);
            }
        }
        let mut models = HashMap::new();
        let mut model_lower = HashMap::new();
        if let Some(mod_map) = obj.get("models").and_then(Value::as_object) {
            for (k, v) in mod_map.entries() {
                let parsed = parse_model(k, v);
                model_lower.insert(k.to_ascii_lowercase(), k.to_string());
                models.insert(k.to_string(), parsed);
            }
        }
        Ok(Self {
            version,
            classes,
            models,
            class_lower,
            model_lower,
        })
    }

    /// Case-insensitive class lookup.
    pub fn class(&self, name: &str) -> Option<&Swbf2VehicleClass> {
        self.classes
            .get(name)
            .or_else(|| self.class_lower.get(&name.to_ascii_lowercase()).and_then(|k| self.classes.get(k)))
    }

    /// Case-insensitive model lookup.
    pub fn model(&self, name: &str) -> Option<&Swbf2VehicleModel> {
        self.models
            .get(name)
            .or_else(|| self.model_lower.get(&name.to_ascii_lowercase()).and_then(|k| self.models.get(k)))
    }

    /// Resolves the collision model for class `class_name` (preferring
    /// `collision_model`, then `model`).
    pub fn collision_model_for(&self, class_name: &str) -> Option<&Swbf2VehicleModel> {
        let cls = self.class(class_name)?;
        cls.collision_model
            .as_deref()
            .and_then(|m| self.model(m))
            .or_else(|| cls.model.as_deref().and_then(|m| self.model(m)))
    }
}

impl Swbf2VehicleClass {
    /// Returns all model-space muzzle positions authored for weapon `weapon_class`
    /// (matching ASCII case-insensitively, preferring `seat_index` when given).
    pub fn weapon_muzzles(&self, weapon_class: &str, seat_index: Option<usize>) -> Vec<[f64; 3]> {
        let collect = |w: &Swbf2VehicleSeatWeapon| -> Vec<[f64; 3]> {
            let mut pts = Vec::new();
            for &pt_idx in &w.fire_points {
                if let Some(pos) = self.points.get(pt_idx).and_then(Swbf2VehiclePoint::model_position) {
                    pts.push(pos);
                }
            }
            if pts.is_empty() {
                pts.extend_from_slice(&w.extra_offsets);
            }
            pts
        };

        if let Some(sidx) = seat_index {
            if let Some(seat) = self.seats.get(sidx) {
                for w in &seat.weapons {
                    if w.class.eq_ignore_ascii_case(weapon_class) {
                        let pts = collect(w);
                        if !pts.is_empty() {
                            return pts;
                        }
                    }
                }
            }
        }
        if seat_index.is_some() { return Vec::new(); }
        for seat in &self.seats {
            for w in &seat.weapons {
                if w.class.eq_ignore_ascii_case(weapon_class) {
                    let pts = collect(w);
                    if !pts.is_empty() {
                        return pts;
                    }
                }
            }
        }
        Vec::new()
    }

    /// Returns all model-space fire points authored across all weapons in `BODY`
    /// / first seat, ordered by weapon index.
    pub fn all_weapon_muzzles(&self) -> Vec<(String, Vec<[f64; 3]>)> {
        let mut out = Vec::new();
        for seat in &self.seats {
            for w in &seat.weapons {
                let mut pts = Vec::new();
                for &pt_idx in &w.fire_points {
                    if let Some(pos) = self.points.get(pt_idx).and_then(Swbf2VehiclePoint::model_position) {
                        pts.push(pos);
                    }
                }
                if pts.is_empty() {
                    pts.extend_from_slice(&w.extra_offsets);
                }
                out.push((w.class.clone(), pts));
            }
        }
        out
    }
}

impl Swbf2VehicleModel {
    /// Assembles combined model-space collision triangles from all meshes and
    /// primitives whose `mask` matches `mask_filter` (pass `0` to include every
    /// mesh and primitive regardless of mask). When a non-zero `mask_filter`
    /// selects zero triangles, falls back to all meshes and primitives.
    pub fn build_collision_geometry(&self, mask_filter: u32) -> Swbf2CollisionGeometry {
        let geom = self.collect_geometry(mask_filter);
        if geom.indices.is_empty() && mask_filter != 0 {
            self.collect_geometry(0)
        } else {
            geom
        }
    }

    fn collect_geometry(&self, mask_filter: u32) -> Swbf2CollisionGeometry {
        let mut positions: Vec<f32> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut polygons: Vec<u32> = Vec::new();
        let mut bmin = [f64::INFINITY; 3];
        let mut bmax = [f64::NEG_INFINITY; 3];
        let mut mesh_count = 0usize;
        let mut primitive_count = 0usize;
        let mut next_poly = 0u32;

        let mut expand = |p: [f64; 3]| {
            for axis in 0..3 {
                if p[axis] < bmin[axis] {
                    bmin[axis] = p[axis];
                }
                if p[axis] > bmax[axis] {
                    bmax[axis] = p[axis];
                }
            }
        };

        for mesh in &self.meshes {
            if mask_filter != 0 && mesh.mask != 0 && (mesh.mask & mask_filter) == 0 {
                continue;
            }
            if mesh.positions.is_empty() || mesh.indices.len() < 3 {
                continue;
            }
            let mat = mesh
                .joint
                .and_then(|j| self.skeleton.get(j))
                .map(|j| j.matrix)
                .unwrap_or(IDENTITY_MATRIX);
            let base_vert = (positions.len() / 3) as u32;
            let vert_count = mesh.positions.len() as u32;
            for &p in &mesh.positions {
                let w = transform_point(&mat, p);
                expand(w);
                positions.push(w[0] as f32);
                positions.push(w[1] as f32);
                positions.push(w[2] as f32);
            }
            let tri_count = mesh.indices.len() / 3;
            for t in 0..tri_count {
                let a = mesh.indices[t * 3];
                let b = mesh.indices[t * 3 + 1];
                let c = mesh.indices[t * 3 + 2];
                if a < vert_count && b < vert_count && c < vert_count {
                    indices.push(base_vert + a);
                    indices.push(base_vert + b);
                    indices.push(base_vert + c);
                    let poly = mesh.polygons.get(t).copied().unwrap_or(next_poly);
                    polygons.push(poly);
                    next_poly = next_poly.max(poly + 1);
                }
            }
            mesh_count += 1;
        }

        for prim in &self.primitives {
            if mask_filter != 0 && prim.mask != 0 && (prim.mask & mask_filter) == 0 {
                continue;
            }
            if prim.positions.is_empty() || prim.indices.len() < 3 {
                continue;
            }
            let base_vert = (positions.len() / 3) as u32;
            let vert_count = prim.positions.len() as u32;
            for &p in &prim.positions {
                let w = transform_point(&prim.matrix, p);
                expand(w);
                positions.push(w[0] as f32);
                positions.push(w[1] as f32);
                positions.push(w[2] as f32);
            }
            let tri_count = prim.indices.len() / 3;
            for t in 0..tri_count {
                let a = prim.indices[t * 3];
                let b = prim.indices[t * 3 + 1];
                let c = prim.indices[t * 3 + 2];
                if a < vert_count && b < vert_count && c < vert_count {
                    indices.push(base_vert + a);
                    indices.push(base_vert + b);
                    indices.push(base_vert + c);
                    polygons.push(next_poly);
                    next_poly += 1;
                }
            }
            primitive_count += 1;
        }

        if !bmin[0].is_finite() {
            bmin = [0.0; 3];
            bmax = [0.0; 3];
        }

        Swbf2CollisionGeometry {
            positions,
            indices,
            polygons,
            bounds_min: bmin,
            bounds_max: bmax,
            mesh_count,
            primitive_count,
        }
    }

    /// Computes hull-contact bounds specifically from primitives/meshes marked
    /// for vehicle/building/terrain collision (falling back to overall bounds).
    pub fn hull_contact_bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mask = COLLISION_MASK_VEHICLE | COLLISION_MASK_BUILDING | COLLISION_MASK_TERRAIN;
        let geom = self.build_collision_geometry(mask);
        if geom.indices.is_empty() {
            None
        } else {
            Some((geom.bounds_min, geom.bounds_max))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn rejects_ambiguous_names_and_unusable_mount_geometry() {
        let pack = r#"{"version":1,"models":{"tile":{"primitives":[{"kind":2}]}},
            "classes":{"sled":{"points":[{"offset":[2,1,-3]}],"seats":[{"weapons":[
                {"section":0,"class":"marker","firePoints":[0]}]}]}}}"#;
        let load = |s: &str| Swbf2VehiclePack::from_value(&parse(s).unwrap(), "mounts");
        assert!(load(pack).is_ok());
        for (original, replacement, reason) in [
            (r#""class":"marker","#, "", "class name"),
            (r#""marker""#, "2", "class name"),
            (r#""offset":[2,1,-3]"#, "", "matrix or offset"),
            (r#""offset":[2,1,-3]"#, r#""matrix":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1]"#, "affine"),
            (r#""kind":2"#, r#""kind":3"#, "primitive kind"),
            (r#""sled":{"#, r#""SLED":{},"sled":{"#, "ambiguous vehicle class"),
            (r#""tile":{"#, r#""Tile":{},"tile":{"#, "ambiguous vehicle model"),
        ] {
            let error = load(&pack.replace(original, replacement)).unwrap_err();
            assert!(error.contains(reason), "{error}");
        }
    }

    #[test]
    fn parses_mesh_primitive_only_models_and_named_firepoints() {
        let raw = parse(
            r#"{
                "family": "swbf2",
                "version": 1,
                "classes": {
                    "synth_interceptor": {
                        "base": "flyer",
                        "model": "synth_prim_hull",
                        "collisionModel": "synth_prim_hull",
                        "points": [
                            {"property": "AimerNodeName", "value": "hp_left", "name": "hp_left",
                             "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, -3.5,0.2,4.0,1]},
                            {"property": "AimerNodeName", "value": "hp_right", "name": "hp_right",
                             "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 3.5,0.2,4.0,1]}
                        ],
                        "seats": [
                            {
                                "name": "BODY",
                                "points": [0, 1],
                                "weapons": [
                                    {"section": 1, "class": "synth_cannon", "firePoints": [0, 1], "extraOffsets": []},
                                    {"section": 2, "class": "synth_guided", "firePoints": [], "extraOffsets": [[0.0, -1.0, 5.0]]}
                                ]
                            }
                        ]
                    }
                },
                "models": {
                    "synth_prim_hull": {
                        "skeleton": [{"name": "root", "parent": null, "local": [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1], "matrix": [1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],
                        "meshes": [],
                        "primitives": [
                            {
                                "name": "p_wing_box",
                                "parent": "root",
                                "joint": 0,
                                "kind": 4,
                                "size": [4.0, 0.5, 2.0],
                                "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0.0,1.0,0.0,1],
                                "mask": 17,
                                "collidesWith": ["soldier", "ordnance"],
                                "positions": [[-4,-0.5,-2],[4,-0.5,-2],[4,0.5,-2],[-4,0.5,-2]],
                                "indices": [0,1,2, 0,2,3]
                            }
                        ]
                    }
                }
            }"#,
        )
        .unwrap();

        let pack = Swbf2VehiclePack::from_value(&raw, "test/vehicles.json").unwrap();
        assert_eq!(pack.version, 1);
        let cls = pack.class("SYNTH_INTERCEPTOR").unwrap();
        let muzzles = cls.weapon_muzzles("synth_cannon", Some(0));
        assert_eq!(muzzles, vec![[-3.5, 0.2, 4.0], [3.5, 0.2, 4.0]]);
        let guided = cls.weapon_muzzles("synth_guided", Some(0));
        assert_eq!(guided, vec![[0.0, -1.0, 5.0]]);

        let model = pack.collision_model_for("synth_interceptor").unwrap();
        let geom = model.build_collision_geometry(COLLISION_MASK_ORDNANCE);
        assert_eq!(geom.mesh_count, 0);
        assert_eq!(geom.primitive_count, 1);
        assert_eq!(geom.indices.len(), 6);
        assert!((geom.bounds_min[0] - (-4.0)).abs() < 1e-6);
        assert!((geom.bounds_max[0] - 4.0).abs() < 1e-6);
        assert!((geom.bounds_min[1] - 0.5).abs() < 1e-6);
        assert!((geom.bounds_max[1] - 1.5).abs() < 1e-6);
    }
}
