//! A battle's collision world in the ray kernel's state (the rays crate), as
//! src/ray-kernel.js makes it from a CollisionWorld (src/collision.js
//! 160-165): the terrain's grid, the buildings' tree and the terrain's tree
//! over the content's corners (one copy of each corner array), the world,
//! and the buildings as its fixed set for gathers. The trees are the
//! JavaScript's (three-mesh-bvh's packed nodes and indirect buffers),
//! handed over as built: tools/collision-trees.mjs writes them for a host
//! without JavaScript, and `read_trees` reads that file.
//!
//! The rays crate moves into rust/sim; this moves with it then.

use crate::collision::MapCollision;
use rays::spheres::FixedSet;
use rays::{state, Corners, Grid, Tree};

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

/// A corner array's copy (ray-kernel.js positions(): f32 corners stay f32).
fn push_positions(data: &[f32]) -> usize {
    let s = state();
    s.positions.push(Corners::F32(data.to_vec()));
    s.positions.len() - 1
}

/// ray-kernel.js tree(bvh): its index, indirect buffer and roots.
fn push_tree(bvh: &BvhTree, positions: usize, index: &[u32]) -> u32 {
    let s = state();
    s.trees.push(Tree {
        roots: bvh.roots.clone(),
        positions,
        index: index.to_vec(),
        indexed: !index.is_empty(),
        indirect: bvh.indirect.clone(),
        polygons: Vec::new(),
    });
    (s.trees.len() - 1) as u32
}

/// collision.js 160-165 through ray-kernel.js: the grid (the terrain's
/// corners copied), then the buildings' tree and the terrain's (over the
/// same terrain copy), the world, and the buildings' fixed set.
pub fn install(col: &MapCollision, buildings: &BvhTree, terrain: Option<&BvhTree>) -> Result<Installed, String> {
    let mut terrain_positions: Option<usize> = None;
    let grid = match &col.grid {
        Some(g) => {
            let p = push_positions(&col.terrain_positions);
            terrain_positions = Some(p);
            let s = state();
            s.grids.push(Grid {
                positions: p,
                index: col.terrain_indices.to_vec(),
                cells: g.cells.clone(),
                start: g.start.clone(),
                stamp: vec![0; col.terrain_indices.len() / 3],
                serial: 0,
                min_x: g.min_x,
                min_z: g.min_z,
                max_x: g.max_x,
                max_z: g.max_z,
                size: g.size,
                nx: g.nx,
                nz: g.nz,
            });
            (s.grids.len() - 1) as i32
        }
        None => -1,
    };
    let building_positions = push_positions(&col.positions);
    let buildings_tree = push_tree(buildings, building_positions, &col.indices);
    let terrain_tree = match (terrain, col.terrain_indices.is_empty()) {
        (Some(t), false) => {
            let p = match terrain_positions {
                Some(p) => p,
                None => push_positions(&col.terrain_positions),
            };
            push_tree(t, p, &col.terrain_indices) as i32
        }
        (None, false) => return Err("the terrain has triangles but no tree".into()),
        _ => -1,
    };
    let world = rays::world_new(buildings_tree, terrain_tree, grid);
    // ray-kernel.js fixedSet(staticSet, bvh).
    let count = col.tri_bounds.len() / 6;
    if col.polygons.len() < count {
        return Err("A fixed triangle has no polygon number".into());
    }
    let statics = {
        let s = state();
        s.sets.push(FixedSet { tree: buildings_tree as usize, bounds: col.tri_bounds.clone(), polygons: col.polygons[..count].to_vec(), offset: 0.0 });
        (s.sets.len() - 1) as u32
    };
    rays::spheres::world_statics(world, statics);
    Ok(Installed { world, grid, buildings: buildings_tree, terrain: terrain_tree, statics })
}

/// A cast the JavaScript answered (tools/collision-trees.mjs).
#[derive(Clone, Copy, Debug)]
pub struct Cast {
    pub origin: [f64; 3],
    pub direction: [f64; 3],
    pub far: f64,
    /// -2 miss, -3 terrain, -1 the buildings (rays' MISS, TERRAIN, STATIC).
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
