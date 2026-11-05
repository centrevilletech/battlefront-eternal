// The instance pass's bindings (instances.rs): where its records start in
// `input`, and their size (f64 slots).
use super::BfeRenderer;
use crate::instances::{IN_RECORDS, REC_STRIDE};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn records_at(&self) -> u32 { IN_RECORDS as u32 }
    pub fn record_stride(&self) -> u32 { REC_STRIDE as u32 }
    // A blending rig's or part's material: three's blending (7 values), depth flags
    // (1 test, 2 write) and function, its passes in drawing order (program,
    // side pairs); records of KIND_BLENDED and KIND_BLENDED_RIGID name it
    // (REC_BLEND).
    pub fn inst_add_blend(&mut self, blend: &[u32], depth_flags: u32, depth_func: u32, passes: &[u32]) -> u32 {
        self.remember_inst_add_blend(blend, depth_flags, depth_func, passes); // recovery
 self.r.instances.add_blend(blend, depth_flags, depth_func, passes)
    }
}
