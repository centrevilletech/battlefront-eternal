//! The collision world the content crate installs (bfe_content::install):
//! its trees, terrain grid and fixed sets put in this module's collision
//! state as src/ray-kernel.js puts the JavaScript's, so a world made from
//! content in Rust casts as the JavaScript's CollisionWorld does
//! (rust/content/tests/collision.rs proves it on the game's maps).

use bfe_content::collision::TriangleGrid;
use bfe_content::install::{BvhTree, CollisionKernel};

use super::spheres::FixedSet;
use super::{CollisionOwner, Corners, Grid, Tree};

/// Instance entry for ray-kernel.js construction. Owned installers receive
/// CollisionOwner directly and use the same content-copy implementation.
pub struct Kernel;

impl CollisionKernel for CollisionOwner {
    fn positions(&mut self, data: &[f32]) -> usize {
        let s = &mut self.state;
        s.positions.push(Corners::F32(data.to_vec()));
        s.positions.len() - 1
    }

    fn grid(&mut self, positions: usize, index: &[u32], g: &TriangleGrid, triangles: usize) -> i32 {
        let s = &mut self.state;
        s.grids.push(Grid {
            positions,
            index: index.to_vec(),
            cells: g.cells.clone(),
            start: g.start.clone(),
            stamp: vec![0; triangles],
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

    fn tree(&mut self, positions: usize, index: &[u32], tree: &BvhTree) -> u32 {
        let s = &mut self.state;
        s.trees.push(Tree { roots: tree.roots.clone(), positions, index: index.to_vec(), indexed: !index.is_empty(), indirect: tree.indirect.clone(), polygons: Vec::new() });
        (s.trees.len() - 1) as u32
    }

    fn world(&mut self, tree: u32, terrain: i32, grid: i32) -> u32 {
        self.world_new(tree, terrain, grid)
    }

    fn fixed_set(&mut self, tree: u32, bounds: &[f32], polygons: &[u32], offset: f64) -> u32 {
        let s = &mut self.state;
        s.sets.push(FixedSet { tree: tree as usize, bounds: bounds.to_vec(), polygons: polygons.to_vec(), offset });
        (s.sets.len() - 1) as u32
    }

    fn world_statics(&mut self, world: u32, set: u32) {
        self.world_statics(world, set);
    }
}

impl CollisionKernel for Kernel {
    fn positions(&mut self, data: &[f32]) -> usize { CollisionKernel::positions(super::owner(), data) }
    fn grid(&mut self, positions: usize, index: &[u32], grid: &TriangleGrid, triangles: usize) -> i32 {
        CollisionKernel::grid(super::owner(), positions, index, grid, triangles)
    }
    fn tree(&mut self, positions: usize, index: &[u32], tree: &BvhTree) -> u32 { CollisionKernel::tree(super::owner(), positions, index, tree) }
    fn world(&mut self, tree: u32, terrain: i32, grid: i32) -> u32 { CollisionKernel::world(super::owner(), tree, terrain, grid) }
    fn fixed_set(&mut self, tree: u32, bounds: &[f32], polygons: &[u32], offset: f64) -> u32 {
        CollisionKernel::fixed_set(super::owner(), tree, bounds, polygons, offset)
    }
    fn world_statics(&mut self, world: u32, set: u32) { CollisionKernel::world_statics(super::owner(), world, set); }
}
