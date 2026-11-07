//! Live pose ABI for the simulation module. Sampling and blending are
//! shared with the presentation host through bfe-pose.
pub use bfe_pose::{js_max, js_min, Clip, Pose};
use std::ptr::addr_of_mut;
const STRIDE: usize = 7;
#[derive(Default)]
pub struct State {
    pub clips: Vec<Clip>,
    pub poses: Vec<Pose>,
    /// Cold rig construction exchange; never resized by a frame or tick.
    pub timelines: Vec<bfe_pose::timeline::Timeline>,
    pub construction_input: Vec<u8>,
    pub construction_output: Vec<u8>,
}

static mut STATE: Option<State> = None;

/// The module's state. One thread: JavaScript calls in one at a time.
pub fn state() -> &'static mut State {
    unsafe { (*addr_of_mut!(STATE)).get_or_insert_with(State::default) }
}

#[no_mangle]
pub extern "C" fn clip_new(frames: u32, tracks: u32, fps: f64, root_motion: u32) -> u32 {
    let s = state();
    let (frames, tracks) = (frames as usize, tracks as usize);
    s.clips.push(Clip {
        frames,
        fps,
        root_motion: root_motion != 0,
        tracks: vec![0; tracks],
        data: vec![0.0; tracks * frames * STRIDE],
    });
    (s.clips.len() - 1) as u32
}

#[no_mangle]
pub extern "C" fn clip_data(id: u32) -> *mut f32 {
    state().clips[id as usize].data.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn clip_tracks(id: u32) -> *mut i32 {
    state().clips[id as usize].tracks.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn pose_new(count: u32, root: i32) -> u32 {
    let s = state();
    s.poses.push(Pose::new(count as usize, root));
    (s.poses.len() - 1) as u32
}

#[no_mangle]
pub extern "C" fn pose_clear(id: u32) {
    state().poses[id as usize].clear()
}

#[no_mangle]
pub extern "C" fn pose_add(pose: u32, clip: u32, time: f64, weight: f64, looping: u32) {
    let s = state();
    let State { clips, poses, .. } = s;
    poses[pose as usize].add(&clips[clip as usize], time, weight, looping != 0);
}

#[no_mangle]
pub extern "C" fn pose_q(id: u32) -> *mut f32 {
    state().poses[id as usize].q.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn pose_t(id: u32) -> *mut f32 {
    state().poses[id as usize].t.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn pose_w(id: u32) -> *mut f32 {
    state().poses[id as usize].w.as_mut_ptr()
}

/// Computes the pose's apply finals and returns them (count * 7 f64).
#[no_mangle]
pub extern "C" fn pose_out(id: u32) -> *mut f64 {
    let pose = &mut state().poses[id as usize];
    pose.apply_out();
    pose.out.as_mut_ptr()
}

/// Vehicle constructors share one cold exchange with the required sampler.
#[no_mangle]
pub extern "C" fn pose_construction_input(length: usize) -> *mut u8 {
    let s = state();
    s.construction_input.resize(length, 0);
    s.construction_input.as_mut_ptr()
}
#[no_mangle]
pub extern "C" fn pose_construct_vehicle() -> u32 {
    let s = state();
    match bfe_pose::rig::construction::vehicle(&s.construction_input) {
        Ok(bytes) => { s.construction_output = bytes; 0 }
        Err(error) => { s.construction_output = error.into_bytes(); 1 }
    }
}
/// vehicleBatches performs geometry packing only at resource load.
#[no_mangle]
pub extern "C" fn pose_construct_batches() -> u32 {
    let s = state();
    match bfe_pose::rig::construction::batches(&s.construction_input) {
        Ok(bytes) => { s.construction_output = bytes; 0 }
        Err(error) => { s.construction_output = error.into_bytes(); 1 }
    }
}
#[no_mangle]
pub extern "C" fn pose_construction_output() -> *const u8 { state().construction_output.as_ptr() }
#[no_mangle]
pub extern "C" fn pose_construction_length() -> usize { state().construction_output.len() }

/// Snapshot rings are born with the page, not with authoritative worlds.
#[no_mangle]
pub extern "C" fn timeline_new() -> usize {
    let s=state();s.timelines.push(bfe_pose::timeline::Timeline::new());s.timelines.len()-1
}
// A stale constructor handle can accompany memory-view rebinding during a
// frame. Pointer queries return null instead of indexing outside the store.
#[no_mangle]
pub extern "C" fn timeline_input(id:usize) -> *mut f64 {
    state().timelines.get_mut(id).map_or(std::ptr::null_mut(),|t|t.input.as_mut_ptr())
}
#[no_mangle]
pub extern "C" fn timeline_output(id:usize) -> *const f64 {
    state().timelines.get(id).map_or(std::ptr::null(),|t|t.output.as_ptr())
}
#[no_mangle]
pub extern "C" fn timeline_push(id:usize) { if let Some(t)=state().timelines.get_mut(id) {t.push();} }
#[no_mangle]
pub extern "C" fn timeline_present(id:usize) { if let Some(t)=state().timelines.get_mut(id) {t.present();} }
#[no_mangle]
pub extern "C" fn timeline_reset(id:usize, epoch:f64) { if let Some(t)=state().timelines.get_mut(id) {t.reset(epoch);} }
#[no_mangle]
pub extern "C" fn timeline_remove(id:usize, entity:f64) { if let Some(t)=state().timelines.get_mut(id) {t.remove(entity);} }
