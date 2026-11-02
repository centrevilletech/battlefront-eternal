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
    pub fn fx_matrices_ptr(&mut self) -> u32 { self.r.effects.matrices.as_mut_ptr() as u32 }
    pub fn fx_matrices_len(&self) -> u32 { self.r.effects.matrices.len() as u32 }
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

    // A dynamic mesh: texture (-1 none), three's blending (7 values), depth
    // flags (1 test, 2 write) and function, its vertex attributes' sizes and
    // its material values ("name:size,..."), in the order the page writes
    // them; then each of three's passes of it, in drawing order: program,
    // side (0 front, 1 back, 2 both), the attributes' locations.
    #[allow(clippy::too_many_arguments)]
    pub fn fx_add_mesh(&mut self, texture: i32, blend: &[u32], depth_flags: u32, depth_func: u32, sizes: &[u32], values: &str) -> u32 {
        self.r.effects.add_mesh(&self.r.core, texture, blend, depth_flags, depth_func, sizes, values)
    }
    pub fn fx_mesh_pass(&mut self, mesh: u32, program: u32, side: u32, locations: &[u32]) -> u32 { self.r.effects.mesh_pass(&self.r.core, mesh, program, side, locations) }
    pub fn fx_mesh_texture(&mut self, mesh: u32, texture: i32) { self.r.effects.mesh_texture(&self.r.core, mesh, texture) }

    // A model: texture (-1 none), three's blending (7 values), depth flags
    // (1 test, 2 write) and function, its material values ("name:size" or
    // "name:size:form", comma-separated, form 1 a float array, 4 a vec4
    // array; in the order the page writes them), its geometry's bounding sphere
    // (centre, radius); then each of three's passes of it, in drawing order:
    // program, side, the geometry registered for that program.
    #[allow(clippy::too_many_arguments)]
    pub fn fx_add_model(&mut self, texture: i32, blend: &[u32], depth_flags: u32, depth_func: u32, values: &str, sphere: &[f64]) -> u32 {
        self.r.effects.add_model(&self.r.core, texture, blend, depth_flags, depth_func, values, sphere)
    }
    pub fn fx_model_pass(&mut self, model: u32, program: u32, side: u32, geometry: u32) -> u32 { self.r.effects.model_pass(&self.r.core, model, program, side, geometry) }
    pub fn fx_model_texture(&mut self, model: u32, texture: i32) { self.r.effects.model_texture(&self.r.core, model, texture) }
}
