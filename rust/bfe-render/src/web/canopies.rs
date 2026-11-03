// The tree canopies pass's bindings (canopies.rs): its input, its copy of
// the leaves, and its mesh, registered once.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn canopies_input_ptr(&mut self) -> u32 { self.r.canopies.input.as_mut_ptr() as u32 }
    pub fn canopies_input_len(&self) -> u32 { self.r.canopies.input.len() as u32 }
    pub fn canopies_leaves_ptr(&mut self) -> u32 { self.r.canopies.leaves.as_mut_ptr() as u32 }
    pub fn canopies_leaves_len(&self) -> u32 { self.r.canopies.leaves.len() as u32 }
    // The mesh: its geometry (core.rs), its per-instance attributes'
    // locations and floats an instance, for up to `max` instances.
    pub fn canopies_mesh(&mut self, geometry: u32, locations: &[u32], sizes: &[u32], max: u32) {
        self.r.canopies.set_mesh(&self.r.core, geometry, locations, sizes, max)
    }
}
