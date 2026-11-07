//! The page's retained sound phase shares its existing collision world.
//! Commands target the independent audio render module, never simulation I/O.
use ::audio_control::{Collision, Hit, Point};
use ::audio_control::runtime::Runtime;
use ::audio_control::phase::index;
use ::audio_control::layout::{frame, plane};
use crate::collision::{self, Ray};

#[no_mangle]
pub extern "C" fn sound_new(seed: u32) -> u32 {
    crate::abi::world().page.sound.insert(Runtime::new(seed)).unwrap_or(0) as u32
}
#[no_mangle]
pub extern "C" fn sound_free(h: u32) { crate::abi::world().page.sound.remove(h as usize); }
#[no_mangle]
pub extern "C" fn sound_input(h: u32) -> *mut f64 { crate::abi::world().page.sound.get_mut(h as usize).map_or(std::ptr::null_mut(), |r| r.input.as_mut_ptr()) }
#[no_mangle]
pub extern "C" fn sound_info(h: u32) -> *mut f64 { crate::abi::world().page.sound.get_mut(h as usize).map_or(std::ptr::null_mut(), |r| r.info.as_mut_ptr()) }
#[no_mangle]
pub extern "C" fn sound_commands(h: u32) -> *const f64 { crate::abi::world().page.sound.get_mut(h as usize).map_or(std::ptr::null_mut(), |r| r.phase.engine.commands.as_ptr()) }
#[no_mangle]
pub extern "C" fn sound_placements(h: u32) -> *const f64 { crate::abi::world().page.sound.get_mut(h as usize).map_or(std::ptr::null_mut(), |r| r.phase.engine.placements.as_ptr()) }
#[no_mangle]
pub extern "C" fn sound_requests(h: u32) -> *const f64 { crate::abi::world().page.sound.get_mut(h as usize).map_or(std::ptr::null_mut(), |r| r.requests.as_ptr()) }
#[no_mangle]
pub extern "C" fn sound_follows(h: u32) -> *const u32 { crate::abi::world().page.sound.get_mut(h as usize).map_or(std::ptr::null_mut(), |r| r.follows.as_ptr()) }
#[no_mangle]
pub extern "C" fn sound_debug(h: u32) -> *const f64 { let Some(r) = crate::abi::world().page.sound.get_mut(h as usize) else { return std::ptr::null() }; r.diagnostic(); r.debug.as_ptr() }
#[no_mangle]
pub extern "C" fn sound_import(h: u32, kind: u32, length: u32) { if let Some(r) = crate::abi::world().page.sound.get_mut(h as usize) { r.import(kind as usize, length as usize); } }
#[no_mangle]
pub extern "C" fn sound_ready(h: u32) { if let Some(r) = crate::abi::world().page.sound.get_mut(h as usize) { r.ready(); } }

struct Map<'a> { world: usize, flags: &'a [u8], planes: &'a [[f64; plane::STRIDE]], candidates: &'a mut [u32], enabled: &'a mut [u8] }
impl Collision for Map<'_> {
    fn cast(&mut self, from: Point, direction: Point, reach: f64) -> Option<Hit> {
        if self.world == ::audio_control::engine::NONE {
            // Synthetic fixtures can describe analytic planes as cold map
            // resources. Production maps always borrow their native BVH.
            let mut distance = f64::INFINITY;
            for p in self.planes {
                let denominator = p[plane::NX] * direction[0] + p[plane::NY] * direction[1] + p[plane::NZ] * direction[2];
                if denominator > 0.0 { let origin = p[plane::NX] * from[0] + p[plane::NY] * from[1] + p[plane::NZ] * from[2]; distance = distance.min((p[plane::DISTANCE] - origin) / denominator); }
            }
            if distance < 0.0 || distance > reach { return None; }
            return Some(Hit { distance, point: [from[0] + direction[0] * distance, from[1] + direction[1] * distance, from[2] + direction[2] * distance], terrain: false });
        }
        let state = collision::state(); let ray = Ray { ox: from[0], oy: from[1], oz: from[2], dx: direction[0], dy: direction[1], dz: direction[2] };
        let count = collision::world_candidates_into(state, self.world, &ray, -1, false, false, self.candidates);
        for i in 0..count { let slot = state.worlds[self.world].order[self.candidates[i] as usize] as usize; self.enabled[i] = self.flags[slot]; }
        let hypot_xz = v8math::hypot(direction[0], direction[2]);
        let (code, hit) = collision::world_cast_ray_in(state, self.world, &ray, reach, hypot_xz, &self.candidates[..count], &self.enabled[..count]);
        (code != collision::MISS).then_some(Hit { distance: hit.distance, point: [hit.px, hit.py, hit.pz], terrain: code == collision::TERRAIN })
    }
}

/// One gameplay sound-control call per presented frame. Both ray budgets
/// execute inside Rust against the current collision state and body flags.
#[no_mangle]
pub extern "C" fn sound_step(h: u32, length: u32) {
    let Some(r) = crate::abi::world().page.sound.get_mut(h as usize) else { return }; r.clear_output(); r.feed(length as usize);
    let mut map = Map { world: index(r.phase.frame[frame::WORLD]), flags: &r.body_flags, planes: &r.planes, candidates: &mut r.candidates, enabled: &mut r.enabled };
    r.phase.step(&mut map, &r.events); r.publish();
}
