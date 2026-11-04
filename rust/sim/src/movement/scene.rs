//! What a soldier's tick asks of the collision world (src/collision.js),
//! answered from the ray kernel's own copies of it: the ground under a point,
//! the first hit along a ray, the polygons near a sphere, and whether a
//! sphere overlaps anything. Bodies are switched on and off by a plain
//! JavaScript field, so the caller hands in each one's `enabled`, by its
//! place in the world's order, before the tick.

use super::soldier::{js_hypot2, PART_CACHES, V3};
use crate::collision::spheres::{closest_point_to_point, gather, ground, overlaps_sphere, GatherCache, Gathered, Polygon, Tri};
use crate::collision::{triangle_normal, world_candidates_into, world_cast_ray_in, State, MAX_BODIES, MISS, TERRAIN};
use std::ptr::addr_of_mut;

/// What a contact may hurt him on (collision-damage.js): terrain counts in
/// full, a building's or prop's upward share only; a moving body not at all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Terrain,
    Building,
}

/// A cast's hit: CollisionWorld.cast's point, normal (facing back along the
/// ray) and whether it was the terrain.
#[derive(Clone, Copy)]
pub struct CastHit {
    pub point: V3,
    pub normal: V3,
    pub terrain: bool,
}

/// CollisionWorld.ground's one exact column it remembers (lastGround).
pub struct GroundCache {
    x: f64,
    z: f64,
    top: f64,
    hit: Option<(f64, V3)>,
    // Legacy scenes retain the module's candidate/enabled storage. A native
    // world's scene opts into its own buffers without changing Scene's
    // construction contract for the existing replay consumers.
    cast: Option<CastBuffers>,
}

struct CastBuffers {
    candidates: Vec<u32>,
    enabled: Vec<u8>,
}

impl Default for GroundCache {
    fn default() -> GroundCache {
        GroundCache { x: f64::NAN, z: f64::NAN, top: f64::NAN, hit: None, cast: None }
    }
}

impl GroundCache {
    #[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
    pub(crate) fn owned() -> Self {
        Self { cast: Some(CastBuffers { candidates: vec![0; MAX_BODIES], enabled: vec![0; MAX_BODIES] }), ..Self::default() }
    }
}

pub struct Scene<'a> {
    pub s: &'a mut State,
    pub world: usize,
    /// Each body's `enabled`, by its place in the world's order.
    pub flags: &'a [u8],
    pub gathered: &'a mut Gathered,
    /// The mover's gather caches, one per body part.
    pub caches: &'a mut [GatherCache; PART_CACHES],
    pub ground_cache: &'a mut GroundCache,
}

impl Scene<'_> {
    /// ground(x, z, top): the terrain's height and face normal (its y made
    /// positive) straight under (x, z), searched down from `top`.
    pub fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<(f64, V3)> {
        self.s.worlds[self.world].grid?;
        let cached = &mut *self.ground_cache;
        if cached.x == x && cached.z == z && cached.top == top {
            return cached.hit;
        }
        cached.x = x;
        cached.z = z;
        cached.top = top;
        let hit = ground(self.s, self.world, x, z, top).map(|(height, normal)| (height, V3::from(normal)));
        cached.hit = hit;
        hit
    }

    /// cast(origin, direction, far, out): the first hit against the
    /// buildings, the enabled bodies and the terrain.
    pub fn cast(&mut self, origin: V3, direction: V3, far: f64) -> Option<CastHit> {
        let (candidates, enabled): (&mut [u32], &mut [u8]) = if let Some(buffers) = self.ground_cache.cast.as_mut() {
            (&mut buffers.candidates[..], &mut buffers.enabled[..])
        } else {
            unsafe { (&mut *addr_of_mut!(crate::collision::CANDIDATES), &mut *addr_of_mut!(crate::collision::ENABLED)) }
        };
        cast_in(self.s, self.world, self.flags, origin, direction, far, candidates, enabled)
    }

    /// gather(center, radius, false, the part's cache, ignore): the polygons
    /// near a body sphere, in `polygons()`.
    pub fn gather(&mut self, center: V3, radius: f64, cache: usize, ignore: i64) -> usize {
        gather(self.s, self.world, center.arr(), radius, None, Some(&mut self.caches[cache]), ignore, self.flags, self.gathered)
    }

    pub fn polygons(&self) -> &[Polygon] {
        self.gathered.list()
    }

    pub fn polygons_mut(&mut self) -> &mut [Polygon] {
        self.gathered.list_mut()
    }

    #[inline(always)]
    pub fn closest_point(&self, tri: &Tri, p: V3) -> [f64; 3] {
        closest_point_to_point(tri.a, tri.b, tri.c, p.arr())
    }

    /// Triangle.getNormal of a gathered polygon's triangle.
    pub fn triangle_normal(&self, polygon: usize, tri: usize) -> [f64; 3] {
        let t = &self.gathered.list()[polygon].tris[tri];
        triangle_normal(t.a, t.b, t.c)
    }

    /// overlapsSphere(center, radius): whether anything comes within radius.
    pub fn overlaps_sphere(&mut self, center: V3, radius: f64) -> bool {
        overlaps_sphere(self.s, self.world, center.arr(), radius, -1, self.flags)
    }
}

fn cast_in(s: &mut State, world: usize, flags: &[u8], origin: V3, direction: V3, far: f64, candidates: &mut [u32], enabled: &mut [u8]) -> Option<CastHit> {
        let ray = crate::collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
        let count = world_candidates_into(s, world, &ray, -1, false, false, candidates);
        // The candidates' enabled flags, as rayCandidates writes them.
        for c in 0..count {
            enabled[c] = flags[candidates[c] as usize];
        }
        let (code, hit) = world_cast_ray_in(s, world, &ray, far, js_hypot2(direction.x, direction.z), &candidates[..count], &enabled[..count]);
        if code == MISS {
            return None;
        }
        Some(CastHit { point: V3::new(hit.px, hit.py, hit.pz), normal: V3::new(hit.nx, hit.ny, hit.nz), terrain: code == TERRAIN })
}
