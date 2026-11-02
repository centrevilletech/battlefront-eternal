// The effects pass's bindings (effects.rs): where the page writes the
// frame's draws (f64) and their instances (f32), and the batches and their
// owners, each registered once.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn fx_input_ptr(&mut self) -> u32 { self.r.effects.input.as_mut_ptr() as u32 }
    pub fn fx_input_len(&self) -> u32 { self.r.effects.input.len() as u32 }
    pub fn fx_data_ptr(&mut self) -> u32 { self.r.effects.data.as_mut_ptr() as u32 }
    pub fn fx_data_len(&self) -> u32 { self.r.effects.data.len() as u32 }
    // Room for `floats` of instances a frame (the data moves: views again).
    pub fn fx_reserve(&mut self, floats: u32) { self.r.effects.reserve(floats as usize) }

    // An analytic batches' owner: its tracks' texture and their rows.
    pub fn fx_add_owner(&mut self, tracks: u32, track_rows: f32) -> u32 { self.r.effects.add_owner(&self.r.core, tracks, track_rows) }

    // A particle batch: program, texture (-1 none), three's blending (7
    // values), its corner's location, its instance attributes (location,
    // size pairs, in the order the page writes them), its owner (-1 none).
    pub fn fx_add_batch(&mut self, program: u32, texture: i32, blend: &[u32], corner: u32, attributes: &[u32], owner: i32) -> u32 {
        self.r.effects.add_batch(&self.r.core, program, texture, blend, corner, attributes, owner)
    }
    pub fn fx_batch_texture(&mut self, batch: u32, texture: i32) { self.r.effects.batch_texture(&self.r.core, batch, texture) }
}
