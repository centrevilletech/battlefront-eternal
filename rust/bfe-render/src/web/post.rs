// The post pass's bindings (post.rs): the steps the page writes each frame
// at post_input_ptr, and the targets (three's, by their colour texture) it
// draws into.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn post_input_ptr(&mut self) -> u32 { self.r.post.input.as_mut_ptr() as u32 }
    pub fn post_input_len(&self) -> u32 { self.r.post.input.len() as u32 }
    pub fn post_target(&mut self, texture: u32) -> u32 { self.r.post.add_target(&mut self.r.core, texture) }
}
