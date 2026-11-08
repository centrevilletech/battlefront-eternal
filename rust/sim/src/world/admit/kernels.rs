//! Stage Kernels: the collision world as CollisionWorld's constructor makes
//! it through ray-kernel.js (src/collision.js 80-130): the terrain's grid,
//! the buildings' tree and the terrain's tree over the content's corners,
//! the world and the buildings as its fixed set. The trees are built here
//! as the kernel builds them for the page (three-mesh-bvh's CENTER split,
//! bfe_content::bvh) and installed through the content crate's installer,
//! which the content tests prove against the page's casts on every map.
//! The movement kernel takes no construction call: a soldier's mover is
//! made at its birth. The clip bank fills lazily, at the first sample.
use super::{AdmitError, Stage};
use crate::world::owner::World;
use bfe_content::bvh::{build, Positions};
use bfe_content::content::Content;
use bfe_content::install::Installed;

/// Installs the map's collision world and answers with the kernel ids the
/// later stages bind bodies and gathers to (the world, grid, trees, statics).
pub fn install(world: &mut World, content: &Content) -> Result<Installed, AdmitError> {
    let col = &content.collision;
    let buildings = build(Positions::F32(&col.positions), &col.indices);
    let terrain = (!col.terrain_indices.is_empty()).then(|| build(Positions::F32(&col.terrain_positions), &col.terrain_indices));
    world.collision.native_install(col, &buildings, terrain.as_ref())
        .map_err(|error| AdmitError::Content { stage: Stage::Kernels, message: format!("{error:?}") })
}
