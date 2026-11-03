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

/// collision.js triangleBounds: per triangle min x, y, z then max x, y, z
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

/// collision.js TriangleGrid: the terrain's triangles filed by the ground
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

/// A body's triangles: corners, indices into them, each triangle's polygon
/// number (collision.js addBody/createBodyShape's arguments).
#[derive(Clone, Debug)]
pub struct BodyTriangles {
    pub positions: Rc<[f32]>,
    pub indices: Vec<u32>,
    pub polygons: Vec<u32>,
}

/// The map's collision.
#[derive(Clone, Debug)]
pub struct MapCollision {
    pub positions: Rc<[f32]>,
    pub indices: Rc<[u32]>,
    pub polygons: Rc<[u32]>,
    pub terrain_positions: Rc<[f32]>,
    pub terrain_indices: Rc<[u32]>,
    /// The buildings' triangle bounds (the static set's).
    pub tri_bounds: Vec<f32>,
    /// The terrain's grid (when it has triangles).
    pub grid: Option<TriangleGrid>,
    /// game.js: below the battlefield, KILL_Y or 10 m under the lowest
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
        let tri_bounds = triangle_bounds(&positions, &indices);
        let grid = if terrain_indices.is_empty() { None } else { Some(TriangleGrid::new(&terrain_positions, &terrain_indices)) };
        let mut kill_y = KILL_Y;
        let mut i = 1;
        while i < terrain_positions.len() {
            kill_y = js_min(kill_y, terrain_positions[i] as f64 - 10.0);
            i += 3;
        }
        Ok(MapCollision { positions, indices, polygons, terrain_positions, terrain_indices, tri_bounds, grid, kill_y })
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
    }
}

/// props.js `body(range)`: triangles `[first, first + count)` of the props'
/// collision (its corners shared whole).
pub fn range_shape(positions: &Rc<[f32]>, indices: &[u32], polygons: &[u32], range: &Value) -> Option<BodyTriangles> {
    if !range.truthy() {
        return None;
    }
    let first = crate::js::to_number(range.at(0));
    let count = crate::js::to_number(range.at(1));
    let (a, b) = crate::js::relative_range(indices.len(), first * 3.0, Some((first + count) * 3.0));
    let (c, d) = crate::js::relative_range(polygons.len(), first, Some(first + count));
    Some(BodyTriangles { positions: positions.clone(), indices: indices[a..b].to_vec(), polygons: polygons[c..d].to_vec() })
}
