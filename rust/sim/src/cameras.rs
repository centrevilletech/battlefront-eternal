//! The page camera owner beside its collision world. Algorithms and records
//! live in bfe-presentation; this module binds the existing native ray store
//! and the host's scalar Math library, with one coarse call per page frame.
use bfe_presentation::{
    cameras::{self, math::Math},
    layout::{camera_frame as f, camera_limits as limit},
};

use three::Vector3;
// The authority and display clocks each own one state machine. Registering
// either is cold; frames never allocate or replace the other's storage.
pub(crate) struct Owner {
    phases: [Option<cameras::Phase>; limit::OWNERS],
    flags: [u8; crate::collision::MAX_BODIES],
}
impl Owner {
    pub(crate) const fn new() -> Self { Self { phases: [None, None], flags: [0; crate::collision::MAX_BODIES] } }
}
const _: () = assert!(limit::BODIES == crate::collision::MAX_BODIES);
// Camera ids are one-based cold admissions into the two fixed clock slots.
// Zero and stale/excess ids cannot become an unchecked array index.
// The input, command and output allocations stay put for each admitted owner.
// Definition registration can grow content without replacing those buffers.
// A WASM memory growth invalidates host views, not these record addresses.
// The page bridge refreshes its typed views after every cold registration.
fn phase(owner: &mut Owner, id: u32) -> Option<&mut cameras::Phase> {
    owner.phases.get_mut(id.wrapping_sub(1) as usize)?.as_mut()
}

// The same host builtins that produced the old page camera's lens and
// quaternion. They exchange only doubles, with no scene or camera callback.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "camera_math")]
extern "C" {
    fn camera_sin(x: f64) -> f64;
    fn camera_cos(x: f64) -> f64;
    fn camera_tan(x: f64) -> f64;
    fn camera_atan(x: f64) -> f64;
    fn camera_atan2(y: f64, x: f64) -> f64;
    fn camera_asin(x: f64) -> f64;
    fn camera_acos(x: f64) -> f64;
    fn camera_exp(x: f64) -> f64;
    fn camera_log(x: f64) -> f64;
    fn camera_pow(x: f64, y: f64) -> f64;
    fn camera_hypot2(x: f64, y: f64) -> f64;
    fn camera_hypot(x: f64, y: f64, z: f64) -> f64;
    fn camera_random() -> f64;
}
#[cfg(target_arch = "wasm32")]
pub(crate) struct HostMath;
#[cfg(target_arch = "wasm32")]
impl Math for HostMath {
    fn sin(x: f64) -> f64 {
        unsafe { camera_sin(x) }
    }
    fn cos(x: f64) -> f64 {
        unsafe { camera_cos(x) }
    }
    fn tan(x: f64) -> f64 {
        unsafe { camera_tan(x) }
    }
    fn atan(x: f64) -> f64 {
        unsafe { camera_atan(x) }
    }
    fn atan2(y: f64, x: f64) -> f64 {
        unsafe { camera_atan2(y, x) }
    }
    fn asin(x: f64) -> f64 {
        unsafe { camera_asin(x) }
    }
    fn acos(x: f64) -> f64 {
        unsafe { camera_acos(x) }
    }
    fn exp(x: f64) -> f64 {
        unsafe { camera_exp(x) }
    }
    fn log(x: f64) -> f64 {
        unsafe { camera_log(x) }
    }
    fn hypot2(x: f64, y: f64) -> f64 {
        unsafe { camera_hypot2(x, y) }
    }
    fn hypot(x: f64, y: f64, z: f64) -> f64 {
        unsafe { camera_hypot(x, y, z) }
    }
    fn pow(x: f64, y: f64) -> f64 {
        unsafe { camera_pow(x, y) }
    }
}
// Ports the collision portion of SoldierCamera.clearance, VehicleCamera's
// corner rays and DeathShot's booms. Broad-phase candidates stay in Rust.
// Ignored bodies are stable native slots rather than page array positions.
// A turret may exclude several pieces; a hull normally excludes just itself.
// Oversized ignore counts are clamped before copying this stack record.
// Invalid world ids are empty space, never an unchecked table access.
// Cosmetic random is called only after live shake survives the native guards.
// Authority cameras consume none of that stream during their own frames.
struct CastWorld<'a, M> {
    collision: &'a mut crate::collision::CollisionOwner, flags: &'a [u8],
    math: std::marker::PhantomData<M>,
    world: usize,
    ignored: [i32; limit::IGNORED_BODIES],
    count: usize,
}
impl<M:Math> cameras::Cast for CastWorld<'_, M> {
    fn cast(&mut self, p: Vector3, d: Vector3, far: f64, static_only: bool) -> Option<f64> {
        use crate::collision as c;
        let s = &mut self.collision.state;
        if self.world >= s.worlds.len() {
            return None;
        }
        let ray = c::Ray {
            ox: p.x,
            oy: p.y,
            oz: p.z,
            dx: d.x,
            dy: d.y,
            dz: d.z,
        };
        let (candidates, enabled) = (&mut self.collision.candidates, &mut self.collision.enabled);
        let count =
            c::broad::world_candidates_near_into(s, self.world, &ray, far, -1, static_only, false, candidates);
        let flags = self.flags;
        // Flags are paired by body slot, not by this ray's candidate order.
        // Turrets can ignore several collision pieces, hulls one.
        for i in 0..count.min(c::MAX_BODIES) {
            let slot = s.worlds[self.world].order[candidates[i] as usize] as usize;
            enabled[i] = if self.ignored[..self.count].contains(&(slot as i32)) {
                0
            } else {
                flags.get(slot).copied().unwrap_or(0)
            };
        }
        let (code, hit) = c::world_cast_ray_in(
            s,
            self.world,
            &ray,
            far,
            M::hypot2(d.x, d.z),
            &candidates[..count],
            &enabled[..count],
        );
        (code != c::MISS).then_some(hit.distance)
    }
    fn random(&mut self) -> f64 {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            return camera_random();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            0.5
        }
    }
}
// Cold admission for the authority and page clocks of one battle instance.
// Each owns persistent followers and fixed buffers in bfe-presentation.
// A third owner is refused instead of displacing a live camera.
// Resource decoding and Vec allocation are confined to this cold path.
// The raw sim module shares memory with collision, avoiding duplicate meshes.
// The optional web feature belongs to the separate presentation web package.
pub(crate) fn camera_init_in(w: &mut crate::world::owner::World, ptr: *const u8, len: usize) -> u32 {
    let Some(bytes) = w.sim.scratch_bytes(ptr, len) else { return 0 };
    let Ok(content) = serde_json::from_slice::<cameras::Content>(bytes) else {
        return 0;
    };
    let phases = &mut w.cameras.phases;
    for (i, slot) in phases.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(cameras::Phase::new(content));
            return (i + 1) as u32;
        }
    }
    0
}
// SoldierCamera.setDef and recon class changes resolve immutable content here.
// A full table refuses the new id while existing handles remain unchanged.
// This cannot fail a running frame or evict a class still held by a soldier.
// Serialization, decoding and content allocation are confined to the cold edge.
// The steady frame sees no class strings, JSON values or dynamic lookup table.
pub(crate) fn camera_add_definition_in(w: &mut crate::world::owner::World, id: u32, ptr: *const u8, len: usize) -> i32 {
    let Some(bytes) = w.sim.scratch_bytes(ptr, len) else { return 0 };
    let Ok(def) = serde_json::from_slice(bytes) else {
        return -1;
    };
    phase(&mut w.cameras, id)
        .and_then(|p| p.add_definition(def))
        .map_or(-1, |i| i as i32)
}
// Buffer accessors are cold bindings, never one call per selected camera.
// Their lengths and offsets come from the shared generated presentation schema.
// Invalid owners return null without touching another clock's record.
// The page caches all addresses and only checks memory.buffer on later frames.
// Matrices and flags remain primitive records rather than imported scene objects.
pub(crate) fn camera_input_ptr_in(w: &mut crate::world::owner::World, id: u32) -> Option<&mut [f64]> {
    phase(&mut w.cameras, id).map_or(None, |p| Some(&mut p.input[..]))
}
pub(crate) fn camera_commands_ptr_in(w: &mut crate::world::owner::World, id: u32) -> Option<&mut [f64]> {
    phase(&mut w.cameras, id).map_or(None, |p| Some(&mut p.commands[..]))
}
pub(crate) fn camera_output_ptr_in(w: &mut crate::world::owner::World, id: u32) -> Option<&[f64]> {
    phase(&mut w.cameras, id).map_or(None, |p| Some(&p.output[..]))
}
pub(crate) fn camera_flags_ptr_in(w: &mut crate::world::owner::World) -> Option<&mut [u8]> {
    Some(&mut w.cameras.flags[..])
}
// The one displayed-frame export. There is no callback per camera or ray.
// Input and output pointers were obtained on cold owner initialization.
// The authority owner may also run on its independent fixed/replay clock.
// This wrapper does not interpret camera priority or construct scene objects.
// It binds borrowed collision storage and forwards one complete phase.
// An unknown owner simply leaves all admitted owners untouched.
pub(crate) fn camera_frame_in<M: Math>(w: &mut crate::world::owner::World, id: u32) {
    let Some(phase) = w.cameras.phases.get_mut(id.wrapping_sub(1) as usize).and_then(Option::as_mut) else { return };
    let a = &phase.input;
    let count = (a[f::IGNORED_COUNT] as usize).min(limit::IGNORED_BODIES);
    let mut ignored = [-1; limit::IGNORED_BODIES];
    for i in 0..count {
        ignored[i] = a[f::IGNORED + i] as i32;
    }
    if a[f::AUTHORITY] != 0.0 {
        let mut world=CastWorld::<cameras::math::V8> {
            collision: &mut w.collision, flags: &w.cameras.flags, world:a[f::WORLD] as usize,ignored,count,math:std::marker::PhantomData,
        };
        phase.frame::<cameras::math::V8,_>(&mut world);
    } else {
        let mut world=CastWorld::<M> {
            collision: &mut w.collision, flags: &w.cameras.flags, world:a[f::WORLD] as usize,ignored,count,math:std::marker::PhantomData,
        };
        phase.frame::<M,_>(&mut world);
    }
}

/// The world coordinator runs the authority camera at its original slot.
/// Its completed matrix feeds offline LOD directly, before any host mirror.
#[cfg(target_arch = "wasm32")]
pub(crate) fn world_frame(w: &mut crate::world::owner::World, id:u32, offline:bool) {
    use bfe_presentation::layout::camera_output as o;
    camera_frame_in::<HostMath>(w, id);
    let Some(phase)=phase(&mut w.cameras, id) else {return};
    let output=&phase.output;
    crate::units::f3::world_camera_in(&mut w.units, [output[o::POSITION],output[o::POSITION+1],output[o::POSITION+2]]);
    if offline {
        if let Some(battle)=w.ai.battle_mut() {
            if battle.exchange.camera_words.len()>=crate::gen::AiPhaseCamera::WORDS {
                let camera=crate::gen::AiPhaseCamera {
                    elements:output[o::MATRIX..o::MATRIX+16].try_into().unwrap(),
                    fov:output[o::FOV],aspect:phase.input[f::CAMERA_ASPECT],
                    near:output[o::NEAR],far:output[o::FAR],zoom:1.0,
                };
                camera.put(&mut battle.exchange.camera_words[..crate::gen::AiPhaseCamera::WORDS],&mut Vec::new());
            }
        }
    }
}

// A cold fixture or preview may restart its camera without replacing the
// fixed record buffers. No frame uses this lifecycle command.
pub(crate) fn camera_reset_in(w: &mut crate::world::owner::World, id: u32) {
    if let Some(p) = phase(&mut w.cameras, id) {
        p.reset_cameras();
    }
}

// Screen events select shots immediately so their cut effect keeps the HUD's
// paint ordering. Geometry never runs in a per-frame browser director call.
pub(crate) fn camera_director_show_in(w: &mut crate::world::owner::World, id: u32, index: f64) -> i32 {
    phase(&mut w.cameras, id).map_or(-1, |p| {
        let index = p.show_shot(index);
        p.output[bfe_presentation::layout::camera_output::DIRECTOR_REVISION] =
            p.director.revision() as f64;
        index
    })
}
pub(crate) fn camera_director_follow_in<M: Math>(w: &mut crate::world::owner::World, id: u32, x: f64, y: f64, z: f64) -> i32 {
    phase(&mut w.cameras, id).map_or(-1, |p| {
        let index = p.follow_shot::<M>(Vector3::new(x, y, z));
        p.output[bfe_presentation::layout::camera_output::DIRECTOR_REVISION] =
            p.director.revision() as f64;
        index
    })
}
pub(crate) fn camera_director_best_in(w: &mut crate::world::owner::World, id: u32, x: f64, y: f64, z: f64) -> i32 {
    phase(&mut w.cameras, id).map_or(-1, |p| p.best_shot(Vector3::new(x, y, z)))
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn camera_init(ptr: *const u8, len: usize) -> u32 { camera_init_in(crate::abi::facade::cold(), ptr, len) }
    #[no_mangle]
    pub extern "C" fn camera_add_definition(id: u32, ptr: *const u8, len: usize) -> i32 { camera_add_definition_in(crate::abi::facade::cold(), id, ptr, len) }
    #[no_mangle]
    pub extern "C" fn camera_input_ptr(id: u32) -> *mut f64 { camera_input_ptr_in(crate::abi::facade::cold(), id).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub extern "C" fn camera_commands_ptr(id: u32) -> *mut f64 { camera_commands_ptr_in(crate::abi::facade::cold(), id).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub extern "C" fn camera_output_ptr(id: u32) -> *const f64 { camera_output_ptr_in(crate::abi::facade::cold(), id).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn camera_flags_ptr() -> *mut u8 { camera_flags_ptr_in(crate::abi::facade::cold()).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub extern "C" fn camera_frame(id: u32) { camera_frame_in::<HostMath>(crate::abi::facade::cold(), id) }
    #[no_mangle]
    pub extern "C" fn camera_reset(id: u32) { camera_reset_in(crate::abi::facade::cold(), id) }
    #[no_mangle]
    pub extern "C" fn camera_director_show(id: u32, index: f64) -> i32 { camera_director_show_in(crate::abi::facade::cold(), id, index) }
    #[no_mangle]
    pub extern "C" fn camera_director_follow(id: u32, x: f64, y: f64, z: f64) -> i32 { camera_director_follow_in::<HostMath>(crate::abi::facade::cold(), id, x, y, z) }
    #[no_mangle]
    pub extern "C" fn camera_director_best(id: u32, x: f64, y: f64, z: f64) -> i32 { camera_director_best_in(crate::abi::facade::cold(), id, x, y, z) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_admission_refuses_foreign_and_overlong_scratch_spans() {
        let mut w = crate::world::owner::World::new();
        let scratch = w.sim.scratch_room(2);
        scratch.copy_from_slice(b"{}");
        let ptr = scratch.as_ptr();
        for (ptr, len) in [(std::ptr::null(), 2), (ptr, 3), (ptr, usize::MAX), (ptr.wrapping_add(3), 0)] {
            assert_eq!(camera_init_in(&mut w, ptr, len), 0);
            assert_eq!(camera_add_definition_in(&mut w, 1, ptr, len), 0);
            assert!(w.cameras.phases.iter().all(Option::is_none));
        }
        // An in-bounds parse error keeps the original definition error code.
        assert_eq!(camera_add_definition_in(&mut w, 1, ptr, 2), -1);
        let content = br#"{"definitions":[],"motions":[]}"#;
        let scratch = w.sim.scratch_room(content.len());
        scratch.copy_from_slice(content);
        let ptr = scratch.as_ptr();
        assert_eq!(camera_init_in(&mut w, ptr, content.len()), 1);
        let output = camera_output_ptr_in(&mut w, 1).unwrap().as_ptr();
        assert_eq!(camera_add_definition_in(&mut w, 1, ptr, content.len() + 1), 0);
        assert_eq!(camera_output_ptr_in(&mut w, 1).unwrap().as_ptr(), output);
    }
}
