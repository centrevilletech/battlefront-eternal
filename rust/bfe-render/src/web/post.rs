// The post pass's bindings (post.rs): the steps the page writes each frame
// at post_input_ptr (and, for the full-screen passes outside the post chain,
// each step's own state at post_states_ptr), and the targets (three's, by
// their colour texture) it draws into.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn post_input_ptr(&mut self) -> u32 { self.r.post.input.as_mut_ptr() as u32 }
    pub fn post_input_len(&self) -> u32 { self.r.post.input.len() as u32 }
    pub fn post_target(&mut self, texture: u32) -> u32 { self.r.post.add_target(&mut self.r.core, texture) }
    pub fn post_states_ptr(&mut self) -> u32 { self.r.post.states.as_mut_ptr() as u32 }
    pub fn post_states_len(&self) -> u32 { self.r.post.states.len() as u32 }
}
