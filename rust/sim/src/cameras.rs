//! The page camera owner beside its collision world. Algorithms and records
//! live in bfe-presentation; this module binds the existing native ray store
//! and the host's scalar Math library, with one coarse call per page frame.
use bfe_presentation::{
    cameras::{self, math::Math},
    layout::{camera_frame as f, camera_limits as limit},
};
use std::ptr::addr_of_mut;
use three::Vector3;
// The authority and display clocks each own one state machine. Registering
// either is cold; frames never allocate or replace the other's storage.
static mut PHASES: [Option<cameras::Phase>; limit::OWNERS] = [None, None];
const _: () = assert!(limit::BODIES == crate::collision::MAX_BODIES);
static mut FLAGS: [u8; crate::collision::MAX_BODIES] = [0; crate::collision::MAX_BODIES];
// Camera ids are one-based cold admissions into the two fixed clock slots.
// Zero and stale/excess ids cannot become an unchecked array index.
// The input, command and output allocations stay put for each admitted owner.
// Definition registration can grow content without replacing those buffers.
// A WASM memory growth invalidates host views, not these record addresses.
// The page bridge refreshes its typed views after every cold registration.
fn phase(id: u32) -> Option<&'static mut cameras::Phase> {
    unsafe {
        (&mut *addr_of_mut!(PHASES))
            .get_mut(id.wrapping_sub(1) as usize)?
            .as_mut()
    }
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
struct HostMath;
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
#[cfg(not(target_arch = "wasm32"))]
type HostMath = cameras::math::V8;
// Ports the collision portion of SoldierCamera.clearance, VehicleCamera's
// corner rays and DeathShot's booms. Broad-phase candidates stay in Rust.
// Ignored bodies are stable native slots rather than page array positions.
// A turret may exclude several pieces; a hull normally excludes just itself.
// Oversized ignore counts are clamped before copying this stack record.
// Invalid world ids are empty space, never an unchecked table access.
// Cosmetic random is called only after live shake survives the native guards.
// Authority cameras consume none of that stream during their own frames.
struct World {
    world: usize,
    ignored: [i32; limit::IGNORED_BODIES],
    count: usize,
}
impl cameras::Cast for World {
    fn cast(&mut self, p: Vector3, d: Vector3, far: f64, static_only: bool) -> Option<f64> {
        use crate::collision as c;
        let s = c::state();
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
        let count =
            c::broad::world_candidates_near(s, self.world, &ray, far, -1, static_only, false);
        let candidates = unsafe { &*std::ptr::addr_of!(c::CANDIDATES) };
        let enabled = unsafe { &mut *addr_of_mut!(c::ENABLED) };
        let flags = unsafe { &*std::ptr::addr_of!(FLAGS) };
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
        let (code, hit) = c::world_cast_ray(
            s,
            self.world,
            &ray,
            far,
            HostMath::hypot2(d.x, d.z),
            count,
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
#[no_mangle]
// Cold admission for the authority and page clocks of one battle instance.
// Each owns persistent followers and fixed buffers in bfe-presentation.
// A third owner is refused instead of displacing a live camera.
// Resource decoding and Vec allocation are confined to this cold path.
// The raw sim module shares memory with collision, avoiding duplicate meshes.
// The optional web feature belongs to the separate presentation web package.
pub extern "C" fn camera_init(ptr: *const u8, len: usize) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(content) = serde_json::from_slice::<cameras::Content>(bytes) else {
        return 0;
    };
    let phases = unsafe { &mut *addr_of_mut!(PHASES) };
    for (i, slot) in phases.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(cameras::Phase::new(content));
            return (i + 1) as u32;
        }
    }
    0
}
#[no_mangle]
// SoldierCamera.setDef and recon class changes resolve immutable content here.
// A full table refuses the new id while existing handles remain unchanged.
// This cannot fail a running frame or evict a class still held by a soldier.
// Serialization, decoding and content allocation are confined to the cold edge.
// The steady frame sees no class strings, JSON values or dynamic lookup table.
pub extern "C" fn camera_add_definition(id: u32, ptr: *const u8, len: usize) -> i32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let Ok(def) = serde_json::from_slice(bytes) else {
        return -1;
    };
    phase(id)
        .and_then(|p| p.add_definition(def))
        .map_or(-1, |i| i as i32)
}
#[no_mangle]
// Buffer accessors are cold bindings, never one call per selected camera.
// Their lengths and offsets come from the shared generated presentation schema.
// Invalid owners return null without touching another clock's record.
// The page caches all addresses and only checks memory.buffer on later frames.
// Matrices and flags remain primitive records rather than imported scene objects.
pub extern "C" fn camera_input_ptr(id: u32) -> *mut f64 {
    phase(id).map_or(std::ptr::null_mut(), |p| p.input.as_mut_ptr())
}
#[no_mangle]
pub extern "C" fn camera_commands_ptr(id: u32) -> *mut f64 {
    phase(id).map_or(std::ptr::null_mut(), |p| p.commands.as_mut_ptr())
}
#[no_mangle]
pub extern "C" fn camera_output_ptr(id: u32) -> *const f64 {
    phase(id).map_or(std::ptr::null(), |p| p.output.as_ptr())
}
#[no_mangle]
pub extern "C" fn camera_flags_ptr() -> *mut u8 {
    addr_of_mut!(FLAGS).cast::<u8>()
}
#[no_mangle]
// The one displayed-frame export. There is no callback per camera or ray.
// Input and output pointers were obtained on cold owner initialization.
// The authority owner may also run on its independent fixed/replay clock.
// This wrapper does not interpret camera priority or construct scene objects.
// It binds borrowed collision storage and forwards one complete phase.
// An unknown owner simply leaves all admitted owners untouched.
pub extern "C" fn camera_frame(id: u32) {
    let Some(phase) = phase(id) else { return };
    let a = &phase.input;
    let count = (a[f::IGNORED_COUNT] as usize).min(limit::IGNORED_BODIES);
    let mut ignored = [-1; limit::IGNORED_BODIES];
    for i in 0..count {
        ignored[i] = a[f::IGNORED + i] as i32;
    }
    let mut world = World {
        world: a[f::WORLD] as usize,
        ignored,
        count,
    };
    phase.frame::<HostMath, World>(&mut world);
}

// A cold fixture or preview may restart its camera without replacing the
// fixed record buffers. No frame uses this lifecycle command.
#[no_mangle]
pub extern "C" fn camera_reset(id: u32) {
    if let Some(p) = phase(id) {
        p.reset_cameras();
    }
}

// Screen events select shots immediately so their cut effect keeps the HUD's
// paint ordering. Geometry never runs in a per-frame browser director call.
#[no_mangle]
pub extern "C" fn camera_director_show(id: u32, index: f64) -> i32 {
    phase(id).map_or(-1, |p| {
        let index = p.show_shot(index);
        p.output[bfe_presentation::layout::camera_output::DIRECTOR_REVISION] =
            p.director.revision() as f64;
        index
    })
}
#[no_mangle]
pub extern "C" fn camera_director_follow(id: u32, x: f64, y: f64, z: f64) -> i32 {
    phase(id).map_or(-1, |p| {
        let index = p.follow_shot::<HostMath>(Vector3::new(x, y, z));
        p.output[bfe_presentation::layout::camera_output::DIRECTOR_REVISION] =
            p.director.revision() as f64;
        index
    })
}
#[no_mangle]
pub extern "C" fn camera_director_best(id: u32, x: f64, y: f64, z: f64) -> i32 {
    phase(id).map_or(-1, |p| p.best_shot(Vector3::new(x, y, z)))
}
