//! Native collision queries never invoke a host. Each private owner borrow
//! ends before the owned answer returns to a handler or another consumer.
use super::{Context, Error};
use crate::collision::{Body, Corners};
use bfe_content::collision::MapCollision;
use bfe_content::install::{BvhTree, Installed};

pub use crate::collision::native::{CollisionQuery, CollisionResult};

#[derive(Clone, Debug)]
pub enum CollisionError {
    Context(Error),
    Invalid(&'static str),
    Content(String),
    Capacity,
}
impl From<Error> for CollisionError {
    fn from(error: Error) -> Self {
        Self::Context(error)
    }
}
impl std::fmt::Display for CollisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Context(e) => e.fmt(f),
            Self::Invalid(s) => f.write_str(s),
            Self::Content(s) => f.write_str(s),
            Self::Capacity => f.write_str("too many collision bodies"),
        }
    }
}
impl std::error::Error for CollisionError {}
type Result<T> = std::result::Result<T, CollisionError>;

impl Context {
    // Only private, callback-free operations enter this borrow. The owned
    // result is copied before World is released to another context operation.
    fn collision_owner<T>(&self, f: impl FnOnce(&mut crate::collision::CollisionOwner) -> Result<T>) -> Result<T> {
        let _flight = self.enter()?;
        self.world(|w| f(&mut w.collision))?
    }

    /// Install decoded content and its existing packed BVHs. As with the
    /// content installer, a content error can leave its preceding allocations.
    /// Geometry and packed nodes are trusted inputs, not a wire format decoder.
    pub fn collision_install(
        &self,
        content: &MapCollision,
        buildings: &BvhTree,
        terrain: Option<&BvhTree>,
    ) -> Result<Installed> {
        self.collision_owner(|owner| owner.native_install(content, buildings, terrain))
    }
    /// Build a body's immutable tree with the existing native BVH builder.
    pub fn collision_shape(
        &self,
        positions: Corners,
        index: Vec<u32>,
        polygons: Vec<u32>,
    ) -> Result<u32> {
        self.collision_owner(|owner| owner.native_shape(positions, index, polygons))
    }
    pub fn collision_fixed_set(
        &self,
        tree: u32,
        bounds: &[f32],
        polygons: &[u32],
        offset: f64,
    ) -> Result<u32> {
        self.collision_owner(|owner| owner.native_add_fixed_set(tree, bounds, polygons, offset))
    }
    /// Append a body to a world's order. The slot is local and never reused.
    pub fn collision_body(&self, world: u32, body: Body) -> Result<u32> {
        self.collision_owner(|owner| owner.native_body(world, body))
    }
    pub fn collision_place(
        &self,
        body: u32,
        bounds: [f64; 6],
        matrix: [f64; 16],
        inverse: [f64; 16],
    ) -> Result<()> {
        self.collision_owner(|owner| owner.native_place(body, bounds, matrix, inverse))
    }
    pub fn collision_order(&self, world: u32, bodies: &[u32]) -> Result<()> {
        self.collision_owner(|owner| owner.native_order(world, bodies))
    }
    pub fn collision_remove(&self, world: u32, body: u32) -> Result<bool> {
        self.collision_owner(|owner| owner.native_remove(world, body))
    }
    /// A cache is bound to its world. Numeric ids have meaning only here.
    pub fn collision_cache(&self, world: u32, margin: f64) -> Result<u32> {
        self.collision_owner(|owner| owner.native_cache(world, margin))
    }
    /// Atomic query from owned scene facts and plain enabled flags. Callers
    /// can invoke this from a host callback, then reenter any context safely.
    pub fn collision_query(&self, query: CollisionQuery, flags: &[u8]) -> Result<CollisionResult> {
        self.collision_owner(|owner| owner.native_run(query, flags))
    }
    pub fn collision_ground(
        &self,
        world: u32,
        x: f64,
        z: f64,
        top: f64,
    ) -> Result<Option<(f64, [f64; 3])>> {
        self.collision_owner(|owner| owner.native_ground(world, x, z, top))
    }
}
