// The view pass's bindings (view.rs): where the page writes what the view
// blends each frame (its parts are instance records, web/instances.rs).
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn view_input_ptr(&mut self) -> u32 { self.r.view.input.as_mut_ptr() as u32 }
    pub fn view_input_len(&self) -> u32 { self.r.view.input.len() as u32 }
}
