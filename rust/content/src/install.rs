//! A battle's collision world in the collision kernel's state, as
//! src/ray-kernel.js makes it from a CollisionWorld (src/collision.js
//! 160-165): the terrain's grid, the buildings' tree and the terrain's tree
//! over the content's corners (one copy of each corner array), the world,
//! and the buildings as its fixed set for gathers. The trees are the
//! JavaScript's (three-mesh-bvh's packed nodes and indirect buffers),
//! handed over as built: tools/collision-trees.mjs writes them for a host
//! without JavaScript, and `read_trees` reads that file.
//!
//! The kernel is rust/sim's collision module. The simulation module links
//! this crate, not the other way round, so the kernel's side is a trait:
//! the module implements `CollisionKernel` over its state and `install`
//! drives it in ray-kernel.js's order (tests/collision.rs has the
//! implementation over today's state, and proves the result).

use crate::collision::{MapCollision, TriangleGrid};

/// A MeshBVH (indirect) as ray-kernel.js copies one: its roots' packed node
/// words and its indirect buffer.
#[derive(Clone, Debug, Default)]
pub struct BvhTree {
    pub roots: Vec<Vec<u32>>,
    pub indirect: Vec<u32>,
}

/// The ids the world was made with, in the kernel's state.
#[derive(Clone, Copy, Debug)]
pub struct Installed {
    pub world: u32,
    pub grid: i32,
    pub buildings: u32,
    pub terrain: i32,
    pub statics: u32,
}

/// The collision kernel's constructors, as ray-kernel.js calls them.
pub trait CollisionKernel {
    /// positions_new and its copy: f32 corners kept as f32. Returns its id.
    fn positions(&mut self, data: &[f32]) -> usize;
    /// grid_new over `positions`: the grid's index, cells, starts, numbers
    /// and `triangles` stamps.
    fn grid(&mut self, positions: usize, index: &[u32], grid: &TriangleGrid, triangles: usize) -> i32;
    /// tree_new over `positions` with its index (empty: none), indirect
    /// buffer and roots.
    fn tree(&mut self, positions: usize, index: &[u32], tree: &BvhTree) -> u32;
    /// world_new(tree, terrain, grid).
    fn world(&mut self, tree: u32, terrain: i32, grid: i32) -> u32;
    /// set_new and its bounds and polygon numbers.
    fn fixed_set(&mut self, tree: u32, bounds: &[f32], polygons: &[u32], offset: f64) -> u32;
    /// world_statics(world, set).
    fn world_statics(&mut self, world: u32, set: u32);
}

/// collision.js 160-165 through ray-kernel.js: the grid (the terrain's
/// corners copied), then the buildings' tree and the terrain's (over the
/// same terrain copy), the world, and the buildings' fixed set.
pub fn install(k: &mut dyn CollisionKernel, col: &MapCollision, buildings: &BvhTree, terrain: Option<&BvhTree>) -> Result<Installed, String> {
    let mut terrain_positions: Option<usize> = None;
    let grid = match &col.grid {
        Some(g) => {
            let p = k.positions(&col.terrain_positions);
            terrain_positions = Some(p);
            k.grid(p, &col.terrain_indices, g, col.terrain_indices.len() / 3)
        }
        None => -1,
    };
    let building_positions = k.positions(&col.positions);
    let buildings_tree = k.tree(building_positions, &col.indices, buildings);
    let terrain_tree = match (terrain, col.terrain_indices.is_empty()) {
        (Some(t), false) => {
            let p = match terrain_positions {
                Some(p) => p,
                None => k.positions(&col.terrain_positions),
            };
            k.tree(p, &col.terrain_indices, t) as i32
        }
        (None, false) => return Err("the terrain has triangles but no tree".into()),
        _ => -1,
    };
    let world = k.world(buildings_tree, terrain_tree, grid);
    // ray-kernel.js fixedSet(staticSet, bvh).
    let count = col.tri_bounds.len() / 6;
    if col.polygons.len() < count {
        return Err("A fixed triangle has no polygon number".into());
    }
    let statics = k.fixed_set(buildings_tree, &col.tri_bounds, &col.polygons[..count], 0.0);
    k.world_statics(world, statics);
    Ok(Installed { world, grid, buildings: buildings_tree, terrain: terrain_tree, statics })
}

/// A cast the JavaScript answered (tools/collision-trees.mjs).
#[derive(Clone, Copy, Debug)]
pub struct Cast {
    pub origin: [f64; 3],
    pub direction: [f64; 3],
    pub far: f64,
    /// -2 miss, -3 terrain, -1 the buildings (the kernel's MISS, TERRAIN, STATIC).
    pub code: i32,
    /// distance, point, normal.
    pub hit: [f64; 7],
}

/// Little-endian reads through a file's bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let slice = self.bytes.get(self.at..self.at + n).ok_or("trees file ends early")?;
        self.at += n;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }

    fn f64s<const N: usize>(&mut self) -> Result<[f64; N], String> {
        let mut out = [0.0; N];
        for slot in &mut out {
            let b = self.take(8)?;
            *slot = f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        }
        Ok(out)
    }

    fn words(&mut self) -> Result<Vec<u32>, String> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.u32()?);
        }
        Ok(out)
    }

    fn tree(&mut self) -> Result<BvhTree, String> {
        let n = self.u32()? as usize;
        let mut roots = Vec::with_capacity(n);
        for _ in 0..n {
            roots.push(self.words()?);
        }
        Ok(BvhTree { roots, indirect: self.words()? })
    }
}

/// The trees file tools/collision-trees.mjs writes: the buildings' tree,
/// the terrain's, and the casts.
pub fn read_trees(bytes: &[u8]) -> Result<(BvhTree, Option<BvhTree>, Vec<Cast>), String> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != b"BFEC" {
        return Err("not a trees file".into());
    }
    if r.u32()? != 1 {
        return Err("trees file version".into());
    }
    let buildings = r.tree()?;
    let terrain = if r.u32()? == 1 { Some(r.tree()?) } else { None };
    let n = r.u32()? as usize;
    let mut casts = Vec::with_capacity(n);
    for _ in 0..n {
        let ray: [f64; 7] = r.f64s()?;
        let code = r.i32()?;
        let hit: [f64; 7] = r.f64s()?;
        casts.push(Cast { origin: [ray[0], ray[1], ray[2]], direction: [ray[3], ray[4], ray[5]], far: ray[6], code, hit });
    }
    Ok((buildings, terrain, casts))
}
