//! The live droideka body's clips and collision queries. Clips are copied
//! once through the pose sampler; casts and gathers use the same collision
//! world and body flags as the soldiers' movement, without a JavaScript
//! query or triangle copy (src/collision.js rayCast and rayGather).

use super::droideka::{DroidekaDef, DroidekaParts, ModeClips};
use super::world::CastHit;
use crate::collision::spheres::{self, GatherCache, Gathered};
use crate::collision::{self, broad, candidate, MISS, TERRAIN};
use crate::pose::{self, Clip};
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
    pub fn read(row: &[f64]) -> Self {
        let input=crate::gen::F3DroidekaClass::take(row,&[]);
        let def=DroidekaDef {
            max_pitch_speed:input.max_pitch_speed,ball_turn_speed:input.ball_turn_speed,
            ball_max_lean:input.ball_max_lean,ball_acceleration:input.ball_acceleration,
            ball_rolling_friction:input.ball_rolling_friction,
        };
        let poses = pose::state();
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
    pub fn new(class: &DroidekaClass, world: usize) -> Self {
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
        gathered.prepare_dense(collision::state(), world, 16);
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
    pub fn visit_gathered(&mut self, world: usize, center: &Vector3, radius: f64,
        cache: usize, visit: &mut dyn FnMut(bool, &[Vector3; 3])) {
        let s = collision::state();
        spheres::gather(s, world, [center.x, center.y, center.z], radius, None,
            Some(&mut self.caches[cache]), -1, flags(s.worlds[world].order.len()), &mut self.gathered);
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

fn flags(count: usize) -> &'static [u8] {
    // SAFETY: the unit phase writes this fixed-size array before its tick
    // and after each hook. Neither query calls JavaScript or retains it.
    unsafe { std::slice::from_raw_parts(crate::movement::movement_flags_ptr(), count) }
}

pub fn cast(world: usize, origin: &Vector3, direction: &Vector3, far: f64) -> Option<CastHit> {
    cast_static(world, origin, direction, far, false)
}

pub fn cast_static(world: usize, origin: &Vector3, direction: &Vector3, far: f64, static_only: bool) -> Option<CastHit> {
    let s = collision::state();
    let ray = collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
    let count = broad::world_candidates_near(s, world, &ray, far, -1, static_only, false);
    let flags = flags(s.worlds[world].order.len());
    let enabled = collision::enabled();
    for (i, flag) in enabled.iter_mut().enumerate().take(count) {
        *flag = flags[candidate(i)];
    }
    let (code, hit) = collision::world_cast_ray(s, world, &ray, far, v8math::hypot(direction.x, direction.z), count);
    if code == MISS {
        None
    } else {
        Some(CastHit { point: Vector3::new(hit.px, hit.py, hit.pz), normal: Vector3::new(hit.nx, hit.ny, hit.nz), distance: hit.distance, terrain: code == TERRAIN })
    }
}

/// OrdnanceWorld.cast and castMoving use the same live collision tree as
/// soldier movement, including the shooter's ignored body and moving mask.
pub fn ordnance_cast(world: usize, origin: [f64; 3], direction: [f64; 3], far: f64, ignore: Option<crate::store::Ref<crate::gen::Body>>, static_only: bool, moving: bool) -> Option<crate::ordnance::world::Hit> {
    let ignored = match ignore {
        Some(body)=>{
            let collision=crate::vehicles::module::collision_world()?;
            if collision.body_reference(body.slot)!=Some(body) {return None;}
            collision.kernel_slot(body.slot)? as i64
        },
        None=>-1,
    };
    let s = collision::state();
    let ray = collision::Ray { ox: origin[0], oy: origin[1], oz: origin[2], dx: direction[0], dy: direction[1], dz: direction[2] };
    let count = broad::world_candidates_near(s, world, &ray, far, ignored, static_only, moving);
    let flags = flags(s.worlds[world].order.len());
    let enabled = collision::enabled();
    for (i, flag) in enabled.iter_mut().enumerate().take(count) { *flag = flags[candidate(i)]; }
    // CollisionWorld.castMoving excludes terrain and fixed meshes as well as
    // fixed body candidates. Bolts already retained their static impact at
    // launch; treating it as a moving hit would overwrite its terrain surface.
    let (code, h) = if moving { collision::world_cast_moving_ray(s, world, &ray, far, count) }
        else { collision::world_cast_ray(s, world, &ray, far, v8math::hypot(direction[0], direction[2]), count) };
    if code == MISS {return None;}
    let body=if code >= 0 {Some(super::f3::body_reference(code as usize)?)}else{None};
    Some(crate::ordnance::world::Hit {distance:h.distance,point:[h.px,h.py,h.pz],normal:[h.nx,h.ny,h.nz],terrain:code==TERRAIN,body})
}
