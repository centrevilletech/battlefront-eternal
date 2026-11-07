// Auxiliary post bindings (post.rs): individual screen/replay operations
// at post_input_ptr (and, for the full-screen passes outside the post chain,
// each step's own state at post_states_ptr), and the targets (three's, by
// their colour texture) it draws into.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn post_input_ptr(&mut self) -> u32 { self.r.post.input.as_mut_ptr() as u32 }
    pub fn post_input_len(&self) -> u32 { self.r.post.input.len() as u32 }
    pub fn post_target(&mut self, texture: u32) -> u32 {
        self.remember_post_target(texture); // recovery
 self.r.post.add_target(&mut self.r.core, texture) }
    pub fn post_states_ptr(&mut self) -> u32 { self.r.post.states.as_mut_ptr() as u32 }
    pub fn post_states_len(&self) -> u32 { self.r.post.states.len() as u32 }
}

// The retained post owner has a facts block and published temporal state.
// Pointers expose fixed storage; no frame result allocates a bindgen array.
#[wasm_bindgen]
impl BfeRenderer {
    pub fn post_chain_input_ptr(&mut self) -> u32 { self.r.post_chain.input.as_mut_ptr() as u32 }
    pub fn post_chain_input_len(&self) -> u32 { self.r.post_chain.input.len() as u32 }
    pub fn post_chain_state_ptr(&mut self) -> u32 { self.r.post_chain.state.as_mut_ptr() as u32 }
    pub fn post_chain_state_len(&self) -> u32 { self.r.post_chain.state.len() as u32 }
    pub fn post_chain_begin_frame(&mut self) { self.r.post_chain.begin(); }
    pub fn post_chain_plan_frame(&mut self) -> u32 { self.r.post_chain.plan(&mut self.r.post.input) as u32 }
}
