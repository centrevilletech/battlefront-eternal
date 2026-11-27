//! The collision content: the map's buildings (collision.bin, its index and
//! polygon numbers), the terrain (terrain_collision.bin and its index), and
//! what src/collision.js derives from them at load (each triangle's bounds,
//! the terrain's TriangleGrid); game.js's kill height; the bodies' triangle
//! arrays (a vehicle pack's collision shapes and blockers, the props'
//! ranges of props_collision.bin). The trees over them (three-mesh-bvh's)
//! are built by the JavaScript and copied in (src/install.rs).

use crate::bin_arrays::{f32s, u32s};
use crate::js::{js_max, js_min, to_uint32};
use crate::value::Value;
use std::rc::Rc;

/// triangleBounds: per triangle min x, y, z then max x, y, z
/// (a Float32Array).
pub fn triangle_bounds(positions: &[f32], indices: &[u32]) -> Vec<f32> {
    let mut out = vec![0f32; indices.len() * 2];
    let corner = |i: Option<&u32>, k: usize| -> f64 {
        match i {
            Some(&i) => positions.get(i as usize * 3 + k).map(|&f| f as f64).unwrap_or(f64::NAN),
            None => f64::NAN,
        }
    };
    let mut t = 0;
    let mut o = 0;
    while t < indices.len() {
        let (a, b, c) = (indices.get(t), indices.get(t + 1), indices.get(t + 2));
        for k in 0..3 {
            let (pa, pb, pc) = (corner(a, k), corner(b, k), corner(c, k));
            // Math.min(a, b, c): from +Infinity, left to right.
            let lo = js_min(js_min(js_min(f64::INFINITY, pa), pb), pc);
            let hi = js_max(js_max(js_max(f64::NEG_INFINITY, pa), pb), pc);
            if o + k < out.len() {
                out[o + k] = lo as f32;
            }
            if o + 3 + k < out.len() {
                out[o + 3 + k] = hi as f32;
            }
        }
        t += 3;
        o += 6;
    }
    out
}

/// TriangleGrid: the terrain's triangles filed by the ground
/// cells their bounds cover.
#[derive(Clone, Debug)]
pub struct TriangleGrid {
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
    pub size: f64,
    pub nx: f64,
    pub nz: f64,
    pub cells: Vec<u32>,
    pub start: Vec<u32>,
}

impl TriangleGrid {
    pub fn new(positions: &[f32], indices: &[u32]) -> TriangleGrid {
        let count = indices.len() as f64 / 3.0;
        let bounds = triangle_bounds(positions, indices);
        let (mut min_x, mut min_z, mut max_x, mut max_z, mut area) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY, 0.0f64);
        let b = |o: usize| bounds.get(o).map(|&f| f as f64).unwrap_or(f64::NAN);
        let mut o = 0;
        while o < bounds.len() {
            min_x = js_min(min_x, b(o));
            min_z = js_min(min_z, b(o + 2));
            max_x = js_max(max_x, b(o + 3));
            max_z = js_max(max_z, b(o + 5));
            area += (b(o + 3) - b(o)) * (b(o + 5) - b(o + 2));
            o += 6;
        }
        let mut size = js_max(0.5, (area / js_max(1.0, count)).sqrt());
        size = js_max(size, ((max_x - min_x) * (max_z - min_z) / (1u32 << 20) as f64).sqrt());
        let pad = 1e-3;
        let min_x0 = min_x - pad;
        let min_z0 = min_z - pad;
        let nx = js_max(1.0, ((max_x + pad - min_x0) / size).ceil());
        let nz = js_max(1.0, ((max_z + pad - min_z0) / size).ceil());
        let max_x0 = min_x0 + nx * size;
        let max_z0 = min_z0 + nz * size;
        let cell = |v: f64, min: f64, n: f64| js_min(n - 1.0, js_max(0.0, ((v - min) / size).floor()));
        let cells_n = (nx * nz) as usize;
        let mut start = vec![0u32; cells_n + 1];
        let triangles = count.ceil() as usize;
        let each = |f: &mut dyn FnMut(usize, usize)| {
            for t in 0..triangles {
                let o = t * 6;
                let x0 = cell(b(o) - pad, min_x0, nx);
                let x1 = cell(b(o + 3) + pad, min_x0, nx);
                let z0 = cell(b(o + 2) - pad, min_z0, nz);
                let z1 = cell(b(o + 5) + pad, min_z0, nz);
                let mut z = z0;
                while z <= z1 {
                    let mut x = x0;
                    while x <= x1 {
                        f((z * nx + x) as usize, t);
                        x += 1.0;
                    }
                    z += 1.0;
                }
            }
        };
        each(&mut |c, _| start[c + 1] = start[c + 1].wrapping_add(1));
        for c in 0..cells_n {
            start[c + 1] = start[c + 1].wrapping_add(start[c]);
        }
        let mut fill: Vec<u32> = start[..cells_n].to_vec();
        let mut cells = vec![0u32; start[cells_n] as usize];
        each(&mut |c, t| {
            let at = fill[c] as usize;
            if at < cells.len() {
                cells[at] = to_uint32(t as f64);
            }
            fill[c] = fill[c].wrapping_add(1);
        });
        TriangleGrid { min_x: min_x0, min_z: min_z0, max_x: max_x0, max_z: max_z0, size, nx, nz, cells, start }
    }
}

/// Per-triangle collision mask bits carried in 2005 `*_mask.bin` files.
/// Zero means the triangle collides with every category.
pub mod mask {
    pub const ALL: u8 = 0;
    pub const SOLDIER: u8 = 1 << 0;
    pub const VEHICLE: u8 = 1 << 1;
    pub const BUILDING: u8 = 1 << 2;
    pub const TERRAIN: u8 = 1 << 3;
    pub const ORDNANCE: u8 = 1 << 4;
    pub const FLYER: u8 = 1 << 5;

    #[inline]
    pub const fn matches(triangle_mask: u8, category_bit: u8) -> bool {
        triangle_mask == ALL || (triangle_mask & category_bit) != 0
    }
}

/// Raw buffers for one static collision source (the base world or a composed layer).
#[derive(Clone, Copy, Debug)]
pub struct CollisionLayerBytes<'a> {
    pub positions: &'a [u8],
    pub indices: &'a [u8],
    pub polygons: &'a [u8],
    pub masks: Option<&'a [u8]>,
}

/// A body's triangles: corners, indices into them, each triangle's polygon
/// number (addBody/createBodyShape's arguments), and optional per-triangle
/// 2005 collision mask bytes.
#[derive(Clone, Debug)]
pub struct BodyTriangles {
    pub positions: Rc<[f32]>,
    pub indices: Vec<u32>,
    pub polygons: Vec<u32>,
    pub masks: Vec<u8>,
}

/// The map's collision.
#[derive(Clone, Debug)]
pub struct MapCollision {
    pub positions: Rc<[f32]>,
    pub indices: Rc<[u32]>,
    pub polygons: Rc<[u32]>,
    /// Per-triangle 2005 collision masks (empty for 2004 maps).
    pub masks: Rc<[u8]>,
    pub terrain_positions: Rc<[f32]>,
    pub terrain_indices: Rc<[u32]>,
    /// True when the map authored a terrain mesh/collision heightfield.
    pub has_terrain: bool,
    /// The buildings' triangle bounds (the static set's).
    pub tri_bounds: Vec<f32>,
    /// The terrain's grid (when it has triangles).
    pub grid: Option<TriangleGrid>,
    /// below the battlefield, KILL_Y or 10 m under the lowest
    /// ground.
    pub kill_y: f64,
}

pub const KILL_Y: f64 = -110.0;

impl MapCollision {
    pub fn load(coll: &[u8], index: &[u8], poly: &[u8], ground: &[u8], ground_index: &[u8]) -> Result<MapCollision, String> {
        let positions: Rc<[f32]> = Rc::from(f32s(coll).map_err(|e| e.to_string())?);
        let indices: Rc<[u32]> = Rc::from(u32s(index).map_err(|e| e.to_string())?);
        let polygons: Rc<[u32]> = Rc::from(u32s(poly).map_err(|e| e.to_string())?);
        let terrain_positions: Rc<[f32]> = Rc::from(f32s(ground).map_err(|e| e.to_string())?);
        let terrain_indices: Rc<[u32]> = Rc::from(u32s(ground_index).map_err(|e| e.to_string())?);
        let has_terrain = !terrain_indices.is_empty();
        let tri_bounds = triangle_bounds(&positions, &indices);
        let grid = if terrain_indices.is_empty() { None } else { Some(TriangleGrid::new(&terrain_positions, &terrain_indices)) };
        let mut kill_y = KILL_Y;
        let mut i = 1;
        while i < terrain_positions.len() {
            kill_y = js_min(kill_y, terrain_positions[i] as f64 - 10.0);
            i += 3;
        }
        Ok(MapCollision {
            positions,
            indices,
            polygons,
            masks: Rc::from(Vec::new()),
            terrain_positions,
            terrain_indices,
            has_terrain,
            tri_bounds,
            grid,
            kill_y,
        })
    }

    /// Composes one or more collision sources (base world followed by active
    /// layers in deterministic order), preserving per-triangle masks and
    /// admitting void-terrain maps (`terrain = None`) without stand-in files.
    pub fn load_composed(
        layers: &[CollisionLayerBytes<'_>],
        terrain: Option<(&[u8], &[u8])>,
        min_world_y: Option<f64>,
    ) -> Result<MapCollision, String> {
        let mut positions: Vec<f32> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut polygons: Vec<u32> = Vec::new();
        let mut masks: Vec<u8> = Vec::new();
        let mut poly_offset = 0u32;
        for layer in layers {
            let layer_pos = f32s(layer.positions).map_err(|e| e.to_string())?;
            let layer_idx = u32s(layer.indices).map_err(|e| e.to_string())?;
            let layer_poly = u32s(layer.polygons).map_err(|e| e.to_string())?;
            if !layer_pos.len().is_multiple_of(3) {
                return Err("collision vertex stream is not a multiple of 3 floats".into());
            }
            if let Some(bad) = layer_pos.iter().position(|v| !v.is_finite()) {
                return Err(format!("collision vertex float at index {bad} is non-finite"));
            }
            if !layer_idx.len().is_multiple_of(3) {
                return Err("collision index stream is not a multiple of 3 indices".into());
            }
            let vertex_base = u32::try_from(positions.len() / 3)
                .map_err(|_| "composed collision vertex count overflow".to_string())?;
            let layer_verts = (layer_pos.len() / 3) as u32;
            for &ix in &layer_idx {
                if ix >= layer_verts {
                    return Err(format!("collision index {ix} exceeds layer vertex count {layer_verts}"));
                }
                indices.push(vertex_base.checked_add(ix).ok_or("collision index overflow")?);
            }
            positions.extend_from_slice(&layer_pos);
            let tri_count = layer_idx.len() / 3;
            if layer_poly.len() < tri_count {
                return Err(format!(
                    "collision polygon stream has {} entries for {tri_count} triangles",
                    layer_poly.len()
                ));
            }
            let mut max_poly = 0u32;
            for &p in &layer_poly[..tri_count] {
                let rebased = poly_offset.saturating_add(p);
                max_poly = max_poly.max(p.saturating_add(1));
                polygons.push(rebased);
            }
            poly_offset = poly_offset.saturating_add(max_poly.max(tri_count as u32));
            match layer.masks {
                Some(raw) => {
                    if raw.len() == tri_count {
                        masks.extend_from_slice(raw);
                    } else if raw.is_empty() {
                        masks.resize(masks.len() + tri_count, mask::ALL);
                    } else {
                        return Err(format!(
                            "collision mask length {} does not match triangle count {tri_count}",
                            raw.len()
                        ));
                    }
                }
                None => masks.resize(masks.len() + tri_count, mask::ALL),
            }
        }
        let (terrain_positions, terrain_indices, has_terrain): (Rc<[f32]>, Rc<[u32]>, bool) = match terrain {
            Some((ground, ground_index)) if !ground.is_empty() && !ground_index.is_empty() => {
                let gpos = f32s(ground).map_err(|e| e.to_string())?;
                let gidx = u32s(ground_index).map_err(|e| e.to_string())?;
                if !gpos.len().is_multiple_of(3) {
                    return Err("terrain collision vertex stream is not a multiple of 3 floats".into());
                }
                if let Some(bad) = gpos.iter().position(|v| !v.is_finite()) {
                    return Err(format!("terrain collision vertex float at index {bad} is non-finite"));
                }
                if !gidx.len().is_multiple_of(3) {
                    return Err("terrain collision index stream is not a multiple of 3 indices".into());
                }
                let gverts = (gpos.len() / 3) as u32;
                if let Some(&bad) = gidx.iter().find(|&&ix| ix >= gverts) {
                    return Err(format!("terrain collision index {bad} exceeds vertex count {gverts}"));
                }
                (Rc::from(gpos), Rc::from(gidx), true)
            }
            _ => (Rc::from(Vec::new()), Rc::from(Vec::new()), false),
        };
        let tri_bounds = triangle_bounds(&positions, &indices);
        let grid = if terrain_indices.is_empty() {
            None
        } else {
            Some(TriangleGrid::new(&terrain_positions, &terrain_indices))
        };
        let mut kill_y = KILL_Y;
        if has_terrain {
            let mut i = 1;
            while i < terrain_positions.len() {
                kill_y = js_min(kill_y, terrain_positions[i] as f64 - 10.0);
                i += 3;
            }
        } else {
            // Void-terrain maps (such as space hangars placed below y = -110)
            // derive their floor below both authored map bounds and static
            // collision geometry so spawned soldiers stay above kill_y.
            // Provisional / not settled: the 100 m margin below lowest geometry
            // or bounds awaits space-rules calibration.
            if let Some(bound_min_y) = min_world_y {
                if !bound_min_y.is_finite() {
                    return Err("map bounds min y is non-finite".into());
                }
                kill_y = js_min(kill_y, bound_min_y - 100.0);
            }
            let mut i = 1;
            while i < positions.len() {
                kill_y = js_min(kill_y, positions[i] as f64 - 100.0);
                i += 3;
            }
        }
        Ok(MapCollision {
            positions: Rc::from(positions),
            indices: Rc::from(indices),
            polygons: Rc::from(polygons),
            masks: Rc::from(masks),
            terrain_positions,
            terrain_indices,
            has_terrain,
            tri_bounds,
            grid,
            kill_y,
        })
    }
}

/// `new Float32Array(array)` of a JSON array (each element ToNumber, then
/// rounded to f32).
pub fn f32_array(v: Option<&Value>) -> Vec<f32> {
    v.and_then(Value::as_array).unwrap_or(&[]).iter().map(|x| crate::js::to_number(Some(x)) as f32).collect()
}

/// `new Uint32Array(array)` of a JSON array (each element ToUint32).
pub fn u32_array(v: Option<&Value>) -> Vec<u32> {
    v.and_then(Value::as_array).unwrap_or(&[]).iter().map(|x| to_uint32(crate::js::to_number(Some(x)))).collect()
}

/// A shape from a JSON record { positions, indices, polygons } (a vehicle
/// pack's `collision[name]`, its `blockers`).
pub fn json_shape(record: &Value) -> BodyTriangles {
    BodyTriangles {
        positions: Rc::from(f32_array(record.get("positions"))),
        indices: u32_array(record.get("indices")),
        polygons: u32_array(record.get("polygons")),
        masks: Vec::new(),
    }
}

/// `body(range)`: triangles `[first, first + count)` of the props'
/// collision (its corners shared whole).
pub fn range_shape(positions: &Rc<[f32]>, indices: &[u32], polygons: &[u32], range: &Value) -> Option<BodyTriangles> {
    if !range.truthy() {
        return None;
    }
    let first = crate::js::to_number(range.at(0));
    let count = crate::js::to_number(range.at(1));
    let (a, b) = crate::js::relative_range(indices.len(), first * 3.0, Some((first + count) * 3.0));
    let (c, d) = crate::js::relative_range(polygons.len(), first, Some(first + count));
    Some(BodyTriangles {
        positions: positions.clone(),
        indices: indices[a..b].to_vec(),
        polygons: polygons[c..d].to_vec(),
        masks: Vec::new(),
    })
}

fn parse_swbf2_collision_range(
    indices_len: usize,
    polygons_len: usize,
    masks_len: usize,
    range: &Value,
    label: &str,
) -> Result<Option<(usize, usize, usize, usize)>, String> {
    if !range.truthy() {
        return Ok(None);
    }
    let (first_f, count_f) = if range.as_array().is_some() {
        (crate::js::to_number(range.at(0)), crate::js::to_number(range.at(1)))
    } else if range.as_object().is_some() {
        (
            crate::js::to_number(range.get("firstTriangle")),
            crate::js::to_number(range.get("triangleCount")),
        )
    } else {
        return Err(format!("{label}: collision range must be an array or object"));
    };
    if !first_f.is_finite()
        || !count_f.is_finite()
        || first_f < 0.0
        || count_f < 0.0
        || first_f.fract() != 0.0
        || count_f.fract() != 0.0
    {
        return Err(format!(
            "{label}: invalid prop collision range ({first_f}, {count_f})"
        ));
    }
    if count_f == 0.0 {
        return Ok(None);
    }
    let first = first_f as usize;
    let count = count_f as usize;
    let end_tri = first
        .checked_add(count)
        .ok_or_else(|| format!("{label}: prop collision range overflow"))?;
    let start_idx = first
        .checked_mul(3)
        .ok_or_else(|| format!("{label}: prop collision index start overflow"))?;
    let end_idx = end_tri
        .checked_mul(3)
        .ok_or_else(|| format!("{label}: prop collision index end overflow"))?;
    if end_idx > indices_len {
        return Err(format!(
            "{label}: prop collision triangle range [{first}, {end_tri}) exceeds index buffer ({} triangles)",
            indices_len / 3
        ));
    }
    if end_tri > polygons_len {
        return Err(format!(
            "{label}: prop collision polygon buffer has {polygons_len} entries, need {end_tri}"
        ));
    }
    if masks_len > 0 && end_tri > masks_len {
        return Err(format!(
            "{label}: prop collision mask buffer has {masks_len} entries, need {end_tri}"
        ));
    }
    Ok(Some((first, end_tri, start_idx, end_idx)))
}

/// Extracts a 2005 model-space prop collision triangle range, validates bounds
/// and finiteness strictly, and compacts referenced corners in model space
/// without applying an instance placement matrix (used for world-animated
/// props admitted as moving bodies in the simulation kernel).
pub fn range_shape_local_swbf2(
    positions: &[f32],
    indices: &[u32],
    polygons: &[u32],
    masks: &[u8],
    range: &Value,
    label: &str,
) -> Result<Option<BodyTriangles>, String> {
    let Some((first, end_tri, start_idx, end_idx)) =
        parse_swbf2_collision_range(indices.len(), polygons.len(), masks.len(), range, label)?
    else {
        return Ok(None);
    };
    let vertex_count = positions.len() / 3;
    let slice_idx = &indices[start_idx..end_idx];
    let mut remap: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut out_pos: Vec<f32> = Vec::new();
    let mut out_idx: Vec<u32> = Vec::with_capacity(slice_idx.len());
    for &orig in slice_idx {
        let vi = orig as usize;
        if vi >= vertex_count {
            return Err(format!(
                "{label}: prop collision vertex index {orig} exceeds vertex count {vertex_count}"
            ));
        }
        let mapped = if let Some(&existing) = remap.get(&orig) {
            existing
        } else {
            let x = positions[vi * 3];
            let y = positions[vi * 3 + 1];
            let z = positions[vi * 3 + 2];
            if !(x.is_finite() && y.is_finite() && z.is_finite()) {
                return Err(format!(
                    "{label}: non-finite prop collision vertex at index {orig}"
                ));
            }
            let id = (out_pos.len() / 3) as u32;
            out_pos.push(x);
            out_pos.push(y);
            out_pos.push(z);
            remap.insert(orig, id);
            id
        };
        out_idx.push(mapped);
    }
    let mask_slice = if masks.is_empty() {
        Vec::new()
    } else {
        masks[first..end_tri].to_vec()
    };
    Ok(Some(BodyTriangles {
        positions: Rc::from(out_pos),
        indices: out_idx,
        polygons: polygons[first..end_tri].to_vec(),
        masks: mask_slice,
    }))
}

/// Extracts a 2005 model-space prop collision triangle range, validates bounds
/// and finiteness strictly, compacts referenced corners, and transforms every
/// corner into world space using `matrix` (column-major 4x4).
///
/// 2005 `props_collision.bin` stores each model's collision geometry in model
/// space once and shares ranges across instances, whereas fixed bodies in the
/// simulation kernel carry world-space triangles with no body transform.
/// Per-triangle masks are retained on `BodyTriangles.masks` for future solver
/// filtering (not yet applied during ray/wall queries).
pub fn range_shape_transformed_swbf2(
    positions: &[f32],
    indices: &[u32],
    polygons: &[u32],
    masks: &[u8],
    range: &Value,
    matrix: &[f64; 16],
    label: &str,
) -> Result<Option<BodyTriangles>, String> {
    let Some((first, end_tri, start_idx, end_idx)) =
        parse_swbf2_collision_range(indices.len(), polygons.len(), masks.len(), range, label)?
    else {
        return Ok(None);
    };
    if matrix.iter().any(|v| !v.is_finite() || !(*v as f32).is_finite()) {
        return Err(format!("{label}: non-finite prop placement matrix"));
    }
    let vertex_count = positions.len() / 3;
    let slice_idx = &indices[start_idx..end_idx];
    let mut remap: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut out_pos: Vec<f32> = Vec::new();
    let mut out_idx: Vec<u32> = Vec::with_capacity(slice_idx.len());
    for &orig in slice_idx {
        let vi = orig as usize;
        if vi >= vertex_count {
            return Err(format!(
                "{label}: prop collision vertex index {orig} exceeds vertex count {vertex_count}"
            ));
        }
        let mapped = if let Some(&existing) = remap.get(&orig) {
            existing
        } else {
            let x = positions[vi * 3] as f64;
            let y = positions[vi * 3 + 1] as f64;
            let z = positions[vi * 3 + 2] as f64;
            if !(x.is_finite() && y.is_finite() && z.is_finite()) {
                return Err(format!(
                    "{label}: non-finite prop collision vertex at index {orig}"
                ));
            }
            let wx = (matrix[0] * x + matrix[4] * y + matrix[8] * z + matrix[12]) as f32;
            let wy = (matrix[1] * x + matrix[5] * y + matrix[9] * z + matrix[13]) as f32;
            let wz = (matrix[2] * x + matrix[6] * y + matrix[10] * z + matrix[14]) as f32;
            if !(wx.is_finite() && wy.is_finite() && wz.is_finite()) {
                return Err(format!(
                    "{label}: non-finite world-space prop collision vertex at index {orig}"
                ));
            }
            let id = (out_pos.len() / 3) as u32;
            out_pos.push(wx);
            out_pos.push(wy);
            out_pos.push(wz);
            remap.insert(orig, id);
            id
        };
        out_idx.push(mapped);
    }
    // Preserve outward triangle winding under mirrored placement matrices.
    let det3 = matrix[0] * (matrix[5] * matrix[10] - matrix[9] * matrix[6])
        - matrix[4] * (matrix[1] * matrix[10] - matrix[9] * matrix[2])
        + matrix[8] * (matrix[1] * matrix[6] - matrix[5] * matrix[2]);
    if det3 < 0.0 {
        for tri in out_idx.as_chunks_mut::<3>().0 {
            tri.swap(1, 2);
        }
    }
    let mask_slice = if masks.is_empty() {
        Vec::new()
    } else {
        masks[first..end_tri].to_vec()
    };
    Ok(Some(BodyTriangles {
        positions: Rc::from(out_pos),
        indices: out_idx,
        polygons: polygons[first..end_tri].to_vec(),
        masks: mask_slice,
    }))
}

#[cfg(test)]
mod swbf2_prop_collision_tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn prop_collision_bakes_model_space_triangles_to_instance_world_space() {
        // Model-space triangle around the origin, placed at (100, -50, -1200).
        let positions = vec![
            0.0f32, 0.0, 0.0,
            2.0, 0.0, 0.0,
            0.0, 3.0, 0.0,
        ];
        let indices = vec![0u32, 1, 2];
        let polygons = vec![7u32];
        let masks = vec![mask::BUILDING];
        let range = parse(r#"{"firstTriangle": 0, "triangleCount": 1}"#).unwrap();
        let matrix = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            100.0, -50.0, -1200.0, 1.0,
        ];
        let baked = range_shape_transformed_swbf2(
            &positions,
            &indices,
            &polygons,
            &masks,
            &range,
            &matrix,
            "test_prop",
        )
        .unwrap()
        .unwrap();
        assert_eq!(baked.indices, vec![0, 1, 2]);
        assert_eq!(baked.polygons, vec![7]);
        assert_eq!(baked.masks, vec![mask::BUILDING]);
        assert_eq!(&*baked.positions, &[
            100.0, -50.0, -1200.0,
            102.0, -50.0, -1200.0,
            100.0, -47.0, -1200.0,
        ]);

        // Out-of-range triangle request errors cleanly rather than producing an empty body.
        let bad_range = parse(r#"{"firstTriangle": 0, "triangleCount": 5}"#).unwrap();
        assert!(range_shape_transformed_swbf2(
            &positions,
            &indices,
            &polygons,
            &masks,
            &bad_range,
            &matrix,
            "test_prop",
        )
        .is_err());

        // Matrix elements that are finite in f64 but overflow f32 must error.
        let huge = [1e300; 16];
        assert!(range_shape_transformed_swbf2(
            &positions,
            &indices,
            &polygons,
            &masks,
            &range,
            &huge,
            "test_prop",
        )
        .is_err());

        // Local-space helper preserves unbaked model-space vertices and masks.
        let local = range_shape_local_swbf2(&positions, &indices, &polygons, &masks, &range, "test_prop")
            .unwrap()
            .unwrap();
        assert_eq!(&*local.positions, &positions[..]);
        assert_eq!(local.masks, vec![mask::BUILDING]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_bytes(xs: &[f32]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn u32_bytes(xs: &[u32]) -> Vec<u8> {
        xs.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn composed_collision_rebases_layers_keeps_masks_and_handles_void_terrain() {
        let base_pos = f32_bytes(&[0.0, -200.0, 0.0, 2.0, -200.0, 0.0, 0.0, -200.0, 2.0]);
        let base_idx = u32_bytes(&[0, 1, 2]);
        let base_poly = u32_bytes(&[0]);
        let base_mask = [mask::SOLDIER | mask::ORDNANCE];

        let layer_pos = f32_bytes(&[10.0, -250.0, 0.0, 12.0, -250.0, 0.0, 10.0, -250.0, 2.0]);
        let layer_idx = u32_bytes(&[0, 1, 2]);
        let layer_poly = u32_bytes(&[0]);
        let layer_mask = [mask::FLYER];

        let col = MapCollision::load_composed(
            &[
                CollisionLayerBytes {
                    positions: &base_pos,
                    indices: &base_idx,
                    polygons: &base_poly,
                    masks: Some(&base_mask),
                },
                CollisionLayerBytes {
                    positions: &layer_pos,
                    indices: &layer_idx,
                    polygons: &layer_poly,
                    masks: Some(&layer_mask),
                },
            ],
            None,
            Some(-1800.0),
        )
        .unwrap();

        assert!(!col.has_terrain);
        assert!(col.grid.is_none());
        assert!(col.terrain_positions.is_empty());
        assert!(col.terrain_indices.is_empty());
        assert_eq!(&*col.indices, &[0, 1, 2, 3, 4, 5]);
        assert_eq!(&*col.masks, &[mask::SOLDIER | mask::ORDNANCE, mask::FLYER]);
        assert!(mask::matches(col.masks[0], mask::SOLDIER));
        assert!(!mask::matches(col.masks[0], mask::VEHICLE));
        assert!(col.kill_y <= -1900.0);
    }

    #[test]
    fn range_shape_transformed_reads_object_range_and_masks() {
        let pos: Rc<[f32]> = Rc::from(vec![
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
            2.0, 0.0, 0.0, 3.0, 0.0, 0.0, 2.0, 1.0, 0.0,
        ]);
        let idx = vec![0, 1, 2, 3, 4, 5];
        let poly = vec![10, 11];
        let masks = vec![mask::SOLDIER, mask::BUILDING];
        let identity = [
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ];
        let obj = crate::json::parse(r#"{"firstTriangle":1,"triangleCount":1}"#).unwrap();
        let shape = range_shape_transformed_swbf2(&pos, &idx, &poly, &masks, &obj, &identity, "test")
            .unwrap()
            .unwrap();
        assert_eq!(shape.indices, vec![0, 1, 2]);
        assert_eq!(&*shape.positions, &[2.0, 0.0, 0.0, 3.0, 0.0, 0.0, 2.0, 1.0, 0.0]);
        assert_eq!(shape.polygons, vec![11]);
        assert_eq!(shape.masks, vec![mask::BUILDING]);
    }
}
