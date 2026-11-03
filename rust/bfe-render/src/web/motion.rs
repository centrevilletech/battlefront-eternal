// The motion pass's bindings (motion.rs): the frame's values and records the
// page writes each frame at motion_input_ptr, the drawn rigs' bones now and
// last frame at motion_bones_ptr and motion_previous_ptr.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn motion_input_ptr(&mut self) -> u32 { self.r.motion.input.as_mut_ptr() as u32 }
    pub fn motion_input_len(&self) -> u32 { self.r.motion.input.len() as u32 }
    pub fn motion_bones_ptr(&mut self) -> u32 { self.r.motion.bones.as_mut_ptr() as u32 }
    pub fn motion_bones_len(&self) -> u32 { self.r.motion.bones.len() as u32 }
    pub fn motion_previous_ptr(&mut self) -> u32 { self.r.motion.previous.as_mut_ptr() as u32 }
    pub fn motion_previous_len(&self) -> u32 { self.r.motion.previous.len() as u32 }
    // A record's size and where the records start in the input (f64 slots).
    pub fn motion_record_stride(&self) -> u32 { crate::motion::REC_STRIDE as u32 }
    pub fn motion_records_at(&self) -> u32 { crate::motion::IN_RECORDS as u32 }
}
