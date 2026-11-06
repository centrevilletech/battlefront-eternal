//! Live pose ABI for the simulation module. Sampling and blending are
//! shared with the presentation host through bfe-pose.
pub use bfe_pose::{js_max, js_min, Clip, Pose};
use std::ptr::addr_of_mut;
const STRIDE: usize = 7;
#[derive(Default)]
pub struct State {
    pub clips: Vec<Clip>,
    pub poses: Vec<Pose>,
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
    let State { clips, poses } = s;
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
