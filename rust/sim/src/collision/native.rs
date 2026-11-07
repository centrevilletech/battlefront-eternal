//! Native validation and owned result copies over CollisionOwner. Geometry,
//! cast buffers and gather banks are the same storage used by the shared kernels.
use crate::abi::context::CollisionError;
use crate::collision::{
    self, broad, spheres, Body, CollisionOwner, Corners, Hit, Ray, State, Tree, IO_LEN, MAX_BODIES,
    MISS,
};
use bfe_content::bvh::{build, Positions};
use bfe_content::collision::MapCollision;
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

type Result<T> = std::result::Result<T, CollisionError>;
fn invalid(message: &'static str) -> CollisionError {
    CollisionError::Invalid(message)
}

impl CollisionOwner {
    pub(crate) fn native_movement_state(&mut self, world: u32) -> Result<&mut State> {
        let world = self.valid_world(world)?;
        if self.state.worlds[world].statics.is_none() {
            return Err(invalid("collision world has no fixed set"));
        }
        Ok(&mut self.state)
    }

    pub(crate) fn native_install(
        &mut self,
        col: &MapCollision,
        buildings: &BvhTree,
        terrain: Option<&BvhTree>,
    ) -> Result<Installed> {
        // Installation retains the existing partial-state/error contract.
        install::install(self, col, buildings, terrain).map_err(CollisionError::Content)
    }
    pub(crate) fn native_shape(
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
    pub(crate) fn native_add_fixed_set(
        &mut self,
        tree: u32,
        bounds: &[f32],
        polygons: &[u32],
        offset: f64,
    ) -> Result<u32> {
        self.valid_tree(tree)?;
        if bounds.len() % 6 != 0 || polygons.len() < bounds.len() / 6 {
            return Err(invalid("invalid fixed set"));
        }
        Ok(CollisionKernel::fixed_set(
            self, tree, bounds, polygons, offset,
        ))
    }
    fn valid_world(&self, world: u32) -> Result<usize> {
        ((world as usize) < self.state.worlds.len())
            .then_some(world as usize)
            .ok_or_else(|| invalid("invalid collision world"))
    }
    fn valid_tree(&self, tree: u32) -> Result<usize> {
        ((tree as usize) < self.state.trees.len())
            .then_some(tree as usize)
            .ok_or_else(|| invalid("invalid collision tree"))
    }
    fn valid_grid(&self, grid: u32) -> Result<usize> {
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
    pub(crate) fn native_body(&mut self, world: u32, body: Body) -> Result<u32> {
        let world = self.valid_world(world)?;
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
    pub(crate) fn native_place(
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
    pub(crate) fn native_order(&mut self, world: u32, order: &[u32]) -> Result<()> {
        let world = self.valid_world(world)?;
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
    pub(crate) fn native_remove(&mut self, world: u32, body: u32) -> Result<bool> {
        let world = self.valid_world(world)?;
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
    pub(crate) fn native_cache(&mut self, world: u32, margin: f64) -> Result<u32> {
        self.valid_world(world)?;
        self.native_cache_worlds.resize(self.gathers.caches.len(), None);
        self.native_cache_worlds.push(Some(world));
        self.gathers.caches.push(spheres::GatherCache::new(margin));
        Ok((self.gathers.caches.len() - 1) as u32)
    }
    pub(crate) fn native_ground(
        &mut self,
        world: u32,
        x: f64,
        z: f64,
        top: f64,
    ) -> Result<Option<(f64, [f64; 3])>> {
        let world = self.valid_world(world)?;
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
    pub(crate) fn native_run(&mut self, query: CollisionQuery, flags: &[u8]) -> Result<CollisionResult> {
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
                let world = self.valid_world(world)?;
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
                let w = self.valid_world(world)?;
                self.flags(w, flags)?;
                if self.state.worlds[w].statics.is_none() {
                    return Err(invalid("collision world has no fixed set"));
                }
                if blockers.is_some_and(|b| b as usize >= self.state.sets.len()) {
                    return Err(invalid("invalid blocker set"));
                }
                if let Some(id) = cache {
                    if self
                        .native_cache_worlds
                        .get(id as usize)
                        .is_none_or(|&w| w != Some(world))
                    {
                        return Err(invalid("invalid gather cache for world"));
                    }
                }
                self.sphere(center, radius);
                (self.gathers.query, count) = spheres::gather_candidates_into(
                    &self.state,
                    w,
                    center,
                    radius,
                    blockers.map(|b| b as usize),
                    cache.map(|id| &mut self.gathers.caches[id as usize]),
                    ignore as i64,
                    self.gathers.gathered.get_or_insert_with(spheres::Gathered::new),
                    &mut self.candidates,
                );
                self.enabled(count, flags);
                let n = spheres::gather_complete(
                    &self.state,
                    w,
                    &self.gathers.query,
                    &self.candidates[..count],
                    &self.enabled[..count],
                    self.gathers.gathered.get_or_insert_with(spheres::Gathered::new),
                    &mut self.gathers.out,
                );
                self.io[59] = self.gathers.out.len() as f64;
                n as i32
            }
            CollisionQuery::Overlap {
                world,
                center,
                radius,
                ignore,
            } => {
                let world = self.valid_world(world)?;
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
                let tree = self.valid_tree(tree)?;
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
                let grid = self.valid_grid(grid)?;
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
                let grid = self.valid_grid(grid)?;
                self.sphere(center, radius);
                spheres::terrain_near(&mut self.state, grid, center, radius, &mut self.gathers.near);
                self.gathers.near.len() as i32
            }
        };
        Ok(CollisionResult {
            code,
            io: self.io,
            candidates: self.candidates[..count].to_vec(),
            enabled: self.enabled[..count].to_vec(),
            words: self.gathers.out.clone(),
            near: self.gathers.near.clone(),
        })
    }
}
