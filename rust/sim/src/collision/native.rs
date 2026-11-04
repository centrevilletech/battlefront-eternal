//! Owned collision storage for the native context. No legacy export or
//! ambient storage is used here. The geometric kernels are shared verbatim.
use super::CollisionError;
use crate::collision::{
    self, broad, spheres, Body, Corners, Grid, Hit, Ray, State, Tree, World, IO_LEN, MAX_BODIES,
    MISS,
};
use bfe_content::bvh::{build, Positions};
use bfe_content::collision::{MapCollision, TriangleGrid};
use bfe_content::install::{self, BvhTree, CollisionKernel, Installed};

/// Inputs are already-decoded geometry and numeric facts. Enabled flags are
/// supplied separately, by place in the world's order, and never call a host.
#[derive(Clone, Copy, Debug)]
pub enum CollisionQuery {
    Cast {
        world: u32,
        ray: Ray,
        far: f64,
        hypot_xz: f64,
        ignore: i32,
        static_only: bool,
        moving_only: bool,
    },
    Gather {
        world: u32,
        center: [f64; 3],
        radius: f64,
        cache: Option<u32>,
        blockers: Option<u32>,
        ignore: i32,
    },
    Overlap {
        world: u32,
        center: [f64; 3],
        radius: f64,
        ignore: i32,
    },
    Tree {
        tree: u32,
        ray: Ray,
        near: f64,
        far: f64,
    },
    Grid {
        grid: u32,
        ray: Ray,
        far: f64,
    },
    TerrainNear {
        grid: u32,
        center: [f64; 3],
        radius: f64,
    },
}

/// Owned snapshots survive any following query, growth, body removal or
/// context disposal. IO uses the legacy numeric slots, except 58: native
/// results return vectors instead of an address, so that slot stays zero.
/// `code` is the cast code, polygon/triangle count, or 0/1 for other queries.
#[derive(Clone, Debug)]
pub struct CollisionResult {
    pub code: i32,
    pub io: [f64; IO_LEN],
    pub candidates: Vec<u32>,
    pub enabled: Vec<u8>,
    /// The exact gather encoding (including fixed-set/triangle references).
    pub words: Vec<f64>,
    pub near: Vec<u32>,
}

struct Cache {
    world: u32,
    value: spheres::GatherCache,
}
pub(crate) struct NativeCollision {
    state: State,
    io: [f64; IO_LEN],
    candidates: Vec<u32>,
    enabled: Vec<u8>,
    caches: Vec<Cache>,
    gathered: spheres::Gathered,
    query: spheres::Bounds,
    words: Vec<f64>,
    near: Vec<u32>,
}
impl Default for NativeCollision {
    fn default() -> Self {
        Self {
            state: State::default(),
            io: [0.0; IO_LEN],
            candidates: vec![0; MAX_BODIES],
            enabled: vec![0; MAX_BODIES],
            caches: Vec::new(),
            gathered: spheres::Gathered::new(),
            query: [0.0; 6],
            words: Vec::new(),
            near: Vec::new(),
        }
    }
}
type Result<T> = std::result::Result<T, CollisionError>;
fn invalid(message: &'static str) -> CollisionError {
    CollisionError::Invalid(message)
}

impl NativeCollision {
    pub(super) fn install(
        &mut self,
        col: &MapCollision,
        buildings: &BvhTree,
        terrain: Option<&BvhTree>,
    ) -> Result<Installed> {
        // Installation retains the existing partial-state/error contract.
        install::install(self, col, buildings, terrain).map_err(CollisionError::Content)
    }
    pub(super) fn shape(
        &mut self,
        positions: Corners,
        index: Vec<u32>,
        polygons: Vec<u32>,
    ) -> Result<u32> {
        let count = match &positions {
            Corners::F32(p) => p.len(),
            Corners::F64(p) => p.len(),
        };
        if count % 3 != 0
            || index.len() % 3 != 0
            || index.iter().any(|&i| i as usize >= count / 3)
            || polygons.len() < index.len() / 3
        {
            return Err(invalid("invalid collision shape"));
        }
        let tree = match &positions {
            Corners::F32(p) => build(Positions::F32(p), &index),
            Corners::F64(p) => build(Positions::F64(p), &index),
        };
        let at = self.state.positions.len();
        self.state.positions.push(positions);
        self.state.trees.push(Tree {
            roots: tree.roots,
            positions: at,
            index,
            indexed: true,
            indirect: tree.indirect,
            polygons,
        });
        Ok((self.state.trees.len() - 1) as u32)
    }
    pub(super) fn fixed_set(
        &mut self,
        tree: u32,
        bounds: &[f32],
        polygons: &[u32],
        offset: f64,
    ) -> Result<u32> {
        self.tree(tree)?;
        if bounds.len() % 6 != 0 || polygons.len() < bounds.len() / 6 {
            return Err(invalid("invalid fixed set"));
        }
        Ok(CollisionKernel::fixed_set(
            self, tree, bounds, polygons, offset,
        ))
    }
    fn world(&self, world: u32) -> Result<usize> {
        ((world as usize) < self.state.worlds.len())
            .then_some(world as usize)
            .ok_or_else(|| invalid("invalid collision world"))
    }
    fn tree(&self, tree: u32) -> Result<usize> {
        ((tree as usize) < self.state.trees.len())
            .then_some(tree as usize)
            .ok_or_else(|| invalid("invalid collision tree"))
    }
    fn grid(&self, grid: u32) -> Result<usize> {
        ((grid as usize) < self.state.grids.len())
            .then_some(grid as usize)
            .ok_or_else(|| invalid("invalid collision grid"))
    }
    fn flags(&self, world: usize, flags: &[u8]) -> Result<()> {
        if flags.len() != self.state.worlds[world].order.len() {
            return Err(invalid("flags must follow the world's body order"));
        }
        Ok(())
    }
    pub(super) fn body(&mut self, world: u32, body: Body) -> Result<u32> {
        let world = self.world(world)?;
        if body.tree >= self.state.trees.len() {
            return Err(invalid("invalid body tree"));
        }
        let t = &self.state.trees[body.tree];
        if t.polygons.len() < t.index.len() / 3 {
            return Err(invalid("body tree has no polygon numbers"));
        }
        if self.state.bodies.len() >= MAX_BODIES
            || self.state.worlds[world].order.len() >= MAX_BODIES
        {
            return Err(CollisionError::Capacity);
        }
        let id = self.state.bodies.len() as u32;
        if !body.moving {
            self.state.fixed_version += 1;
        }
        self.state.bodies.push(body);
        self.state.worlds[world].order.push(id);
        self.state.worlds[world].version += 1;
        Ok(id)
    }
    pub(super) fn place(
        &mut self,
        body: u32,
        bounds: [f64; 6],
        matrix: [f64; 16],
        inverse: [f64; 16],
    ) -> Result<()> {
        let b = self
            .state
            .bodies
            .get_mut(body as usize)
            .ok_or_else(|| invalid("invalid collision body"))?;
        b.bounds = bounds;
        if b.moving {
            b.matrix = matrix;
            b.inverse = inverse;
        } else {
            self.state.fixed_version += 1;
        }
        Ok(())
    }
    pub(super) fn order(&mut self, world: u32, order: &[u32]) -> Result<()> {
        let world = self.world(world)?;
        if order.len() > MAX_BODIES {
            return Err(CollisionError::Capacity);
        }
        if order.iter().any(|&b| b as usize >= self.state.bodies.len()) {
            return Err(invalid("invalid body in world order"));
        }
        let w = &mut self.state.worlds[world];
        w.order.clear();
        w.order.extend_from_slice(order);
        w.version += 1;
        Ok(())
    }
    /// Remove a body's first place in this world's order. Slots are never
    /// recycled, like the legacy kernel; the next body gets a new local id.
    pub(super) fn remove(&mut self, world: u32, body: u32) -> Result<bool> {
        let world = self.world(world)?;
        if body as usize >= self.state.bodies.len() {
            return Err(invalid("invalid collision body"));
        }
        let w = &mut self.state.worlds[world];
        let index = w.order.iter().position(|&b| b == body);
        if let Some(index) = index {
            w.order.remove(index);
        }
        w.version += 1;
        Ok(index.is_some())
    }
    pub(super) fn cache(&mut self, world: u32, margin: f64) -> Result<u32> {
        self.world(world)?;
        self.caches.push(Cache {
            world,
            value: spheres::GatherCache::new(margin),
        });
        Ok((self.caches.len() - 1) as u32)
    }
    pub(super) fn ground(
        &mut self,
        world: u32,
        x: f64,
        z: f64,
        top: f64,
    ) -> Result<Option<(f64, [f64; 3])>> {
        let world = self.world(world)?;
        Ok(spheres::ground(&mut self.state, world, x, z, top))
    }
    fn ray(&mut self, ray: Ray, far: f64) {
        self.io[..7].copy_from_slice(&[ray.ox, ray.oy, ray.oz, ray.dx, ray.dy, ray.dz, far]);
    }
    fn sphere(&mut self, center: [f64; 3], radius: f64) {
        self.io[..3].copy_from_slice(&center);
        self.io[3] = radius;
    }
    fn hit(&mut self, h: Hit) {
        self.io[10..17].copy_from_slice(&[h.distance, h.px, h.py, h.pz, h.nx, h.ny, h.nz]);
    }
    fn enabled(&mut self, count: usize, flags: &[u8]) {
        for c in 0..count {
            self.enabled[c] = flags[self.candidates[c] as usize];
        }
    }
    pub(super) fn run(&mut self, query: CollisionQuery, flags: &[u8]) -> Result<CollisionResult> {
        let mut count = 0;
        let code = match query {
            CollisionQuery::Cast {
                world,
                ray,
                far,
                hypot_xz,
                ignore,
                static_only,
                moving_only,
            } => {
                let world = self.world(world)?;
                self.flags(world, flags)?;
                self.ray(ray, far);
                self.io[7] = hypot_xz;
                count = broad::world_candidates_near_into(
                    &mut self.state,
                    world,
                    &ray,
                    far,
                    ignore as i64,
                    static_only,
                    moving_only,
                    &mut self.candidates,
                );
                self.enabled(count, flags);
                let (code, hit) = if moving_only {
                    collision::world_cast_moving_ray_in(
                        &self.state,
                        world,
                        &ray,
                        far,
                        &self.candidates[..count],
                        &self.enabled[..count],
                    )
                } else {
                    collision::world_cast_ray_in(
                        &mut self.state,
                        world,
                        &ray,
                        far,
                        hypot_xz,
                        &self.candidates[..count],
                        &self.enabled[..count],
                    )
                };
                if code != MISS {
                    self.hit(hit);
                }
                code
            }
            CollisionQuery::Gather {
                world,
                center,
                radius,
                cache,
                blockers,
                ignore,
            } => {
                let w = self.world(world)?;
                self.flags(w, flags)?;
                if blockers.is_some_and(|b| b as usize >= self.state.sets.len()) {
                    return Err(invalid("invalid blocker set"));
                }
                if let Some(id) = cache {
                    if self
                        .caches
                        .get(id as usize)
                        .is_none_or(|c| c.world != world)
                    {
                        return Err(invalid("invalid gather cache for world"));
                    }
                }
                self.sphere(center, radius);
                (self.query, count) = spheres::gather_candidates_into(
                    &self.state,
                    w,
                    center,
                    radius,
                    blockers.map(|b| b as usize),
                    cache.map(|id| &mut self.caches[id as usize].value),
                    ignore as i64,
                    &mut self.gathered,
                    &mut self.candidates,
                );
                self.enabled(count, flags);
                let n = spheres::gather_complete(
                    &self.state,
                    w,
                    &self.query,
                    &self.candidates[..count],
                    &self.enabled[..count],
                    &mut self.gathered,
                    &mut self.words,
                );
                self.io[59] = self.words.len() as f64;
                n as i32
            }
            CollisionQuery::Overlap {
                world,
                center,
                radius,
                ignore,
            } => {
                let world = self.world(world)?;
                self.flags(world, flags)?;
                self.sphere(center, radius);
                if spheres::overlaps_fixed(&mut self.state, world, center, radius) {
                    1
                } else {
                    count = spheres::candidates_into(
                        &self.state,
                        world,
                        &spheres::sphere_bounds(center, radius),
                        ignore as i64,
                        None,
                        &mut self.candidates,
                    );
                    self.enabled(count, flags);
                    spheres::overlaps_candidates(
                        &self.state,
                        world,
                        center,
                        radius,
                        &self.candidates[..count],
                        &self.enabled[..count],
                    ) as i32
                }
            }
            CollisionQuery::Tree {
                tree,
                ray,
                near,
                far,
            } => {
                let tree = self.tree(tree)?;
                self.ray(ray, far);
                self.io[8] = near;
                if let Some(hit) = collision::tree_raycast(&self.state, tree, &ray, near, far) {
                    self.hit(hit);
                    1
                } else {
                    0
                }
            }
            CollisionQuery::Grid { grid, ray, far } => {
                let grid = self.grid(grid)?;
                self.ray(ray, far);
                if let Some(hit) = collision::grid_raycast(&mut self.state, grid, &ray, far) {
                    self.hit(hit);
                    1
                } else {
                    0
                }
            }
            CollisionQuery::TerrainNear {
                grid,
                center,
                radius,
            } => {
                let grid = self.grid(grid)?;
                self.sphere(center, radius);
                spheres::terrain_near(&mut self.state, grid, center, radius, &mut self.near);
                self.near.len() as i32
            }
        };
        Ok(CollisionResult {
            code,
            io: self.io,
            candidates: self.candidates[..count].to_vec(),
            enabled: self.enabled[..count].to_vec(),
            words: self.words.clone(),
            near: self.near.clone(),
        })
    }
}

// The content installer already defines the construction order and errors.
// This adapter only chooses the storage each existing owned type goes into.
impl CollisionKernel for NativeCollision {
    fn positions(&mut self, data: &[f32]) -> usize {
        self.state.positions.push(Corners::F32(data.to_vec()));
        self.state.positions.len() - 1
    }
    fn grid(&mut self, positions: usize, index: &[u32], g: &TriangleGrid, triangles: usize) -> i32 {
        self.state.grids.push(Grid {
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
        (self.state.grids.len() - 1) as i32
    }
    fn tree(&mut self, positions: usize, index: &[u32], tree: &BvhTree) -> u32 {
        self.state.trees.push(Tree {
            roots: tree.roots.clone(),
            positions,
            index: index.to_vec(),
            indexed: !index.is_empty(),
            indirect: tree.indirect.clone(),
            polygons: Vec::new(),
        });
        (self.state.trees.len() - 1) as u32
    }
    fn world(&mut self, tree: u32, terrain: i32, grid: i32) -> u32 {
        self.state.worlds.push(World {
            tree: tree as usize,
            terrain: (terrain >= 0).then_some(terrain as usize),
            grid: (grid >= 0).then_some(grid as usize),
            order: Vec::new(),
            version: 0,
            statics: None,
            broad: Default::default(),
        });
        (self.state.worlds.len() - 1) as u32
    }
    fn fixed_set(&mut self, tree: u32, bounds: &[f32], polygons: &[u32], offset: f64) -> u32 {
        self.state.sets.push(spheres::FixedSet {
            tree: tree as usize,
            bounds: bounds.to_vec(),
            polygons: polygons.to_vec(),
            offset,
        });
        (self.state.sets.len() - 1) as u32
    }
    fn world_statics(&mut self, world: u32, set: u32) {
        self.state.worlds[world as usize].statics = Some(set as usize);
    }
}
