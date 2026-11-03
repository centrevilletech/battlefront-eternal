//! The live droideka body's clips and collision queries. Clips are copied
//! once through the pose sampler; casts and gathers use the same collision
//! world and body flags as the soldiers' movement, without a JavaScript
//! query or triangle copy (src/collision.js rayCast and rayGather).

use super::droideka::{DroidekaDef, DroidekaParts, ModeClips};
use super::world::{CastHit, GatherPolygon};
use crate::collision::spheres::{self, GatherCache, Gathered};
use crate::collision::{self, broad, candidate, MISS, TERRAIN};
use crate::pose::{self, Clip};
use crate::store::word_u32;
use std::ptr::addr_of_mut;
use std::rc::Rc;
use three::Vector3;

pub struct DroidekaClass {
    def: DroidekaDef,
    clips: Rc<ModeClips>,
}

impl DroidekaClass {
    /// Five optional class numbers as (present, value), followed by the
    /// fourteen modes' pose-sampler clip ids (-1 for a missing clip).
    pub fn read(row: &[f64]) -> Self {
        let number = |at: usize| if row[at] != 0.0 { Some(row[at + 1]) } else { None };
        let def = DroidekaDef {
            max_pitch_speed: number(0),
            ball_turn_speed: number(2),
            ball_max_lean: number(4),
            ball_acceleration: number(6),
            ball_rolling_friction: number(8),
        };
        let poses = pose::state();
        let by_mode = std::array::from_fn(|i| {
            let id = row[10 + i];
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
    pub fn new(class: &DroidekaClass) -> Self {
        Self { def: class.def, clips: class.clips.clone(), caches: std::array::from_fn(|_| GatherCache::new(0.75)), gathered: Gathered::new() }
    }

    /// The body's native tick needs its clips. The gun rig is still posed
    /// by unitSampleMuzzles, after the aimer has run.
    pub fn parts(&self) -> DroidekaParts {
        DroidekaParts { def: self.def, clips: self.clips.clone(), rig: None }
    }

    pub fn gather(&mut self, world: usize, center: &Vector3, radius: f64, cache: usize, out: &mut Vec<GatherPolygon>) {
        let s = collision::state();
        spheres::gather(s, world, [center.x, center.y, center.z], radius, None, Some(&mut self.caches[cache]), -1, flags(s.worlds[world].order.len()), &mut self.gathered);
        out.clear();
        for p in self.gathered.list() {
            let triangles = p.tris.iter().map(|t| [v3(t.a), v3(t.b), v3(t.c)]).collect();
            out.push(GatherPolygon { triangles, moving: p.moving });
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
    let s = collision::state();
    let ray = collision::Ray { ox: origin.x, oy: origin.y, oz: origin.z, dx: direction.x, dy: direction.y, dz: direction.z };
    let count = broad::world_candidates_near(s, world, &ray, far, -1, false, false);
    let flags = flags(s.worlds[world].order.len());
    let enabled = unsafe { &mut *addr_of_mut!(collision::ENABLED) };
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
