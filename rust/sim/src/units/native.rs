//! The live droideka body's clips and collision queries. Clips are copied
//! once through the pose sampler; casts and gathers use the same collision
//! world and body flags as the soldiers' movement, without a JavaScript
//! query or triangle copy (src/collision.js rayCast and rayGather).

use super::droideka::{DroidekaDef, DroidekaParts, ModeClips};
use super::world::CastHit;
use crate::collision::spheres::{self, GatherCache, Gathered};
use crate::collision::{self, broad, MISS, TERRAIN};
use crate::pose::Clip;
use crate::store::word_u32;
use std::rc::Rc;
use three::Vector3;

pub struct DroidekaClass {
    def: DroidekaDef,
    clips: Rc<ModeClips>,
}

impl DroidekaClass {
    // Droideka.arm needs clips for its hidden gun, without allocating a
    // temporary body and collision caches solely to borrow that class data.
    pub fn parts(&self) -> DroidekaParts {
        DroidekaParts { def: self.def, clips: self.clips.clone(), rig: None }
    }

    /// Five optional class numbers as (present, value), followed by the
    /// fourteen modes' pose-sampler clip ids (-1 for a missing clip).
    pub fn read(poses: &crate::pose::State, row: &[f64]) -> Self {
        let input=crate::gen::F3DroidekaClass::take(row,&[]);
        let def=DroidekaDef {
            max_pitch_speed:input.max_pitch_speed,ball_turn_speed:input.ball_turn_speed,
            ball_max_lean:input.ball_max_lean,ball_acceleration:input.ball_acceleration,
            ball_rolling_friction:input.ball_rolling_friction,
        };
        let by_mode = std::array::from_fn(|i| {
            let id = input.clips[i];
            if id < 0.0 {
                return None;
            }
            let c = &poses.clips[word_u32(id) as usize];
            Some(Rc::new(Clip { frames: c.frames, fps: c.fps, root_motion: c.root_motion, tracks: c.tracks.clone(), data: c.data.clone() }))
        });
        Self { def, clips: Rc::new(ModeClips { by_mode }) }
    }
}

/// A pooled body's gather caches persist across lives, as gatherCaches on
/// the JavaScript Droideka does. Its class's clips are shared.
pub struct DroidekaBody {
    def: DroidekaDef,
    clips: Rc<ModeClips>,
    caches: [GatherCache; 2],
    gathered: Gathered,
}

impl DroidekaBody {
    pub fn new(collision: &collision::State, class: &DroidekaClass, world: usize) -> Self {
        // Droideka.touchPolygons borrows these buffers on every collision pass.
        // Construct the working set now; clear only logical counts in a tick.
        let mut gathered = Gathered::new();
        gathered.reserve(256, 16);
        let caches = std::array::from_fn(|_| {
            let mut cache = GatherCache::new(0.75);
            cache.reserve(512, 512, 512);
            cache
        });
        // Dense authored polygons receive their exact immutable capacities
        // while loading, without observing or invalidating a live gather.
        gathered.prepare_dense(collision, world, 16);
        Self { def: class.def, clips: class.clips.clone(), caches, gathered }
    }

    /// The body's native tick needs its clips. The gun rig is still posed
    /// by unitSampleMuzzles, after the aimer has run.
    pub fn parts(&self) -> DroidekaParts {
        DroidekaParts { def: self.def, clips: self.clips.clone(), rig: None }
    }

    // Droideka.touchPolygons consumes the same ordered triangles as page
    // prediction. Convert one borrowed triangle on the stack, not an entire
    // gather into a second set of growable vectors at each collision pass.
    pub fn visit_gathered(&mut self, s: &collision::State, flags: &[u8], world: usize, center: &Vector3, radius: f64,
        cache: usize, visit: &mut dyn FnMut(bool, &[Vector3; 3])) {
        spheres::gather(s, world, [center.x, center.y, center.z], radius, None,
            Some(&mut self.caches[cache]), -1, flags, &mut self.gathered);
        for polygon in self.gathered.list() {
            for triangle in &polygon.tris {
                visit(polygon.moving, &[v3(triangle.a), v3(triangle.b), v3(triangle.c)]);
            }
        }
    }

}

fn v3(p: [f64; 3]) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}


pub fn cast_in(world_ctx: &mut crate::world::owner::World, world: usize, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit> {
    cast_static_in(world_ctx, world, origin, direction, far, false)
}

pub fn cast_static_in(world_ctx: &mut crate::world::owner::World, world: usize, origin: &Vector3, direction: &Vector3, far: f64, static_only: bool) -> Option<CastHit> {
    let s = &mut world_ctx.collision.state;
    let ray = collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
    let candidates = &mut world_ctx.collision.candidates;
    let enabled = &mut world_ctx.collision.enabled;
    let count = broad::world_candidates_near_into(s, world, &ray, far, -1, static_only, false, candidates);
    let flags = world_ctx.movement.flags_mut();
    for (i, flag) in enabled.iter_mut().enumerate().take(count) {
        *flag = flags[candidates[i] as usize];
    }
    let (code, hit) = collision::world_cast_ray_in(s, world, &ray, far, v8math::hypot(direction.x, direction.z), &candidates[..count], &enabled[..count]);
    if code == MISS {
        None
    } else {
        Some(CastHit { point: Vector3::new(hit.px, hit.py, hit.pz), normal: Vector3::new(hit.nx, hit.ny, hit.nz), distance: hit.distance, terrain: code == TERRAIN })
    }
}

/// OrdnanceWorld.cast and castMoving use the same live collision tree as
/// soldier movement, including the shooter's ignored body and moving mask.
pub fn ordnance_cast_in(world_ctx: &mut crate::world::owner::World, world: usize, origin: [f64; 3], direction: [f64; 3], far: f64, ignore: Option<crate::store::Ref<crate::gen::Body>>, static_only: bool, moving: bool) -> Option<crate::ordnance::world::Hit> {
    ordnance_cast_parts_in(&mut world_ctx.collision, &mut world_ctx.movement, &world_ctx.fleet,
        world, origin, direction, far, ignore, static_only, moving)
}

/// Cold spawn and publication callbacks already borrow their Store. Lend
/// disjoint physical owners to the same caster without acquiring a facade.
pub(crate) fn ordnance_cast_parts_in(collision_owner: &mut collision::CollisionOwner,
    movement: &mut crate::movement::MovementOwner, fleet: &crate::vehicles::module::Fleet,
    world: usize, origin: [f64; 3], direction: [f64; 3], far: f64,
    ignore: Option<crate::store::Ref<crate::gen::Body>>, static_only: bool, moving: bool) -> Option<crate::ordnance::world::Hit> {
    let ignored = match ignore {
        Some(body)=>{
            let collision=fleet.collision.as_ref()?;
            if collision.body_reference(body.slot)!=Some(body) {return None;}
            collision.kernel_slot(body.slot)? as i64
        },
        None=>-1,
    };
    let s = &mut collision_owner.state;
    let ray = collision::Ray { ox: origin[0], oy: origin[1], oz: origin[2], dx: direction[0], dy: direction[1], dz: direction[2] };
    let candidates = &mut collision_owner.candidates;
    let enabled = &mut collision_owner.enabled;
    let count = broad::world_candidates_near_into(s, world, &ray, far, ignored, static_only, moving, candidates);
    let flags = movement.flags_mut();
    for (i, flag) in enabled.iter_mut().enumerate().take(count) { *flag = flags[candidates[i] as usize]; }
    // CollisionWorld.castMoving excludes terrain and fixed meshes as well as
    // fixed body candidates. Bolts already retained their static impact at
    // launch; treating it as a moving hit would overwrite its terrain surface.
    let (code, h) = if moving { collision::world_cast_moving_ray_in(s, world, &ray, far, &candidates[..count], &enabled[..count]) }
        else { collision::world_cast_ray_in(s, world, &ray, far, v8math::hypot(direction[0], direction[2]), &candidates[..count], &enabled[..count]) };
    if code == MISS {return None;}
    // A stale or unknown body identity is rejected rather than indexed.
    let body=if code >= 0 {Some(fleet.collision.as_ref()?.kernel_reference(code as usize)?)}else{None};
    Some(crate::ordnance::world::Hit {distance:h.distance,point:[h.px,h.py,h.pz],normal:[h.nx,h.ny,h.nz],terrain:code==TERRAIN,body})
}

// facade shims: retained for unconverted callers.
#[cfg(target_arch = "wasm32")]
pub fn cast(world: usize, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit>{ cast_in(crate::abi::world(), world, origin, direction, far) }
#[cfg(target_arch = "wasm32")]
pub fn ordnance_cast(world: usize, origin: [f64; 3], direction: [f64; 3], far: f64, ignore: Option<crate::store::Ref<crate::gen::Body>>, static_only: bool, moving: bool) -> Option<crate::ordnance::world::Hit>{ ordnance_cast_in(crate::abi::world(), world, origin, direction, far, ignore, static_only, moving) }
