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
pub extern "C" fn clip_new(frames: u32, tracks: u32, fps: f64, root_motion: u32) -> u32 { state().clip_new(frames, tracks, fps, root_motion) }

#[no_mangle]
pub extern "C" fn clip_data(id: u32) -> *mut f32 { state().clip_data(id) }

#[no_mangle]
pub extern "C" fn clip_tracks(id: u32) -> *mut i32 { state().clip_tracks(id) }

#[no_mangle]
pub extern "C" fn pose_new(count: u32, root: i32) -> u32 { state().pose_new(count, root) }

#[no_mangle]
pub extern "C" fn pose_clear(id: u32) { state().pose_clear(id) }

#[no_mangle]
pub extern "C" fn pose_add(pose: u32, clip: u32, time: f64, weight: f64, looping: u32) { state().pose_add(pose, clip, time, weight, looping) }

#[no_mangle]
pub extern "C" fn pose_q(id: u32) -> *mut f32 { state().pose_q(id) }

#[no_mangle]
pub extern "C" fn pose_t(id: u32) -> *mut f32 { state().pose_t(id) }

#[no_mangle]
pub extern "C" fn pose_w(id: u32) -> *mut f32 { state().pose_w(id) }

/// Computes the pose's apply finals and returns them (count * 7 f64).
#[no_mangle]
pub extern "C" fn pose_out(id: u32) -> *mut f64 { state().pose_out(id) }

impl State {
    pub fn clip_new(&mut self, frames: u32, tracks: u32, fps: f64, root_motion: u32) -> u32 {
        let s = self;
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
    pub fn clip_data(&mut self, id: u32) -> *mut f32 {
        self.clips[id as usize].data.as_mut_ptr()
    }
    pub fn clip_tracks(&mut self, id: u32) -> *mut i32 {
        self.clips[id as usize].tracks.as_mut_ptr()
    }
    pub fn pose_new(&mut self, count: u32, root: i32) -> u32 {
        let s = self;
        s.poses.push(Pose::new(count as usize, root));
        (s.poses.len() - 1) as u32
    }
    pub fn pose_clear(&mut self, id: u32) {
        self.poses[id as usize].clear()
    }
    pub fn pose_add(&mut self, pose: u32, clip: u32, time: f64, weight: f64, looping: u32) {
        let s = self;
        let State { clips, poses, .. } = s;
        poses[pose as usize].add(&clips[clip as usize], time, weight, looping != 0);
    }
    pub fn pose_q(&mut self, id: u32) -> *mut f32 {
        self.poses[id as usize].q.as_mut_ptr()
    }
    pub fn pose_t(&mut self, id: u32) -> *mut f32 {
        self.poses[id as usize].t.as_mut_ptr()
    }
    pub fn pose_w(&mut self, id: u32) -> *mut f32 {
        self.poses[id as usize].w.as_mut_ptr()
    }
    pub fn pose_out(&mut self, id: u32) -> *mut f64 {
        let pose = &mut self.poses[id as usize];
        pose.apply_out();
        pose.out.as_mut_ptr()
    }
}

/// Vehicle constructors share one cold exchange with the required sampler.
#[no_mangle]
pub extern "C" fn pose_construction_input(length: usize) -> *mut u8 { state().pose_construction_input(length) }
#[no_mangle]
pub extern "C" fn pose_construct_vehicle() -> u32 { state().pose_construct_vehicle() }
/// vehicleBatches performs geometry packing only at resource load.
#[no_mangle]
pub extern "C" fn pose_construct_batches() -> u32 { state().pose_construct_batches() }
#[no_mangle]
pub extern "C" fn pose_construction_output() -> *const u8 { state().pose_construction_output() }
#[no_mangle]
pub extern "C" fn pose_construction_length() -> usize { state().pose_construction_length() }

/// Snapshot rings are born with the page, not with authoritative worlds.
#[no_mangle]
pub extern "C" fn timeline_new() -> usize { state().timeline_new() }
// A stale constructor handle can accompany memory-view rebinding during a
// frame. Pointer queries return null instead of indexing outside the store.
#[no_mangle]
pub extern "C" fn timeline_input(id:usize) -> *mut f64 { state().timeline_input(id) }
#[no_mangle]
pub extern "C" fn timeline_output(id:usize) -> *const f64 { state().timeline_output(id) }
#[no_mangle]
pub extern "C" fn timeline_push(id:usize) { state().timeline_push(id) }
#[no_mangle]
pub extern "C" fn timeline_present(id:usize) { state().timeline_present(id) }
#[no_mangle]
pub extern "C" fn timeline_reset(id:usize, epoch:f64) { state().timeline_reset(id, epoch) }
#[no_mangle]
pub extern "C" fn timeline_remove(id:usize, entity:f64) { state().timeline_remove(id, entity) }

impl State {
    pub fn pose_construction_input(&mut self, length: usize) -> *mut u8 {
        let s = self;
        s.construction_input.resize(length, 0);
        s.construction_input.as_mut_ptr()
    }
    pub fn pose_construct_vehicle(&mut self) -> u32 {
        let s = self;
        match bfe_pose::rig::construction::vehicle(&s.construction_input) {
            Ok(bytes) => { s.construction_output = bytes; 0 }
            Err(error) => { s.construction_output = error.into_bytes(); 1 }
        }
    }
    pub fn pose_construct_batches(&mut self) -> u32 {
        let s = self;
        match bfe_pose::rig::construction::batches(&s.construction_input) {
            Ok(bytes) => { s.construction_output = bytes; 0 }
            Err(error) => { s.construction_output = error.into_bytes(); 1 }
        }
    }
    pub fn pose_construction_output(&mut self) -> *const u8 { self.construction_output.as_ptr() }
    pub fn pose_construction_length(&mut self) -> usize { self.construction_output.len() }
    pub fn timeline_new(&mut self) -> usize {
        let s=self;s.timelines.push(bfe_pose::timeline::Timeline::new());s.timelines.len()-1
    }
    pub fn timeline_input(&mut self, id:usize) -> *mut f64 {
        self.timelines.get_mut(id).map_or(std::ptr::null_mut(),|t|t.input.as_mut_ptr())
    }
    pub fn timeline_output(&mut self, id:usize) -> *const f64 {
        self.timelines.get(id).map_or(std::ptr::null(),|t|t.output.as_ptr())
    }
    pub fn timeline_push(&mut self, id:usize) { if let Some(t)=self.timelines.get_mut(id) {t.push();} }
    pub fn timeline_present(&mut self, id:usize) { if let Some(t)=self.timelines.get_mut(id) {t.present();} }
    pub fn timeline_reset(&mut self, id:usize, epoch:f64) { if let Some(t)=self.timelines.get_mut(id) {t.reset(epoch);} }
    pub fn timeline_remove(&mut self, id:usize, entity:f64) { if let Some(t)=self.timelines.get_mut(id) {t.remove(entity);} }
}
