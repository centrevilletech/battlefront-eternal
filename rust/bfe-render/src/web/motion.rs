// The motion pass's bindings (motion.rs): the frame's values and records the
// page writes each frame at motion_input_ptr, the drawn rigs' bones now and
// last frame at motion_bones_ptr and motion_previous_ptr, and the meshes that
// draw their own motion (motion_extra).
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
    // The meshes that draw their own motion (the sky's ships): one registered
    // a mesh (program, geometry, its instances' attributes: [location,
    // columns, rows] each, for up to max instances), and each frame's at
    // motion_extras_ptr (f64) with their instances' values at
    // motion_extra_values_ptr (f32).
    pub fn motion_extra(&mut self, program: u32, geometry: u32, max: u32, layout: &[u32]) -> u32 {
        self.remember_motion_extra(program, geometry, max, layout); // recovery
 self.r.motion.add_extra(&mut self.r.core, program, geometry, max, layout)
    }
    pub fn motion_extras_ptr(&mut self) -> u32 { self.r.motion.extras_input.as_mut_ptr() as u32 }
    pub fn motion_extras_len(&self) -> u32 { self.r.motion.extras_input.len() as u32 }
    pub fn motion_extra_values_ptr(&mut self) -> u32 { self.r.motion.extra_values.as_mut_ptr() as u32 }
    pub fn motion_extra_values_len(&self) -> u32 { self.r.motion.extra_values.len() as u32 }
}
