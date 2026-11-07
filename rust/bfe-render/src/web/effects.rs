// The effects pass's bindings (effects.rs): where the page writes the
// frame's draws (f64) and their instances (f32), and the batches and their
// owners, each registered once.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn fx_clipped(&self)->f64{self.r.effects.clipped as f64}
    pub fn fx_input_ptr(&mut self) -> u32 { self.r.effects.input.as_mut_ptr() as u32 }
    pub fn fx_input_len(&self) -> u32 { self.r.effects.input.len() as u32 }
    pub fn fx_data_ptr(&mut self) -> u32 { self.r.effects.data.as_mut_ptr() as u32 }
    pub fn fx_data_len(&self) -> u32 { self.r.effects.data.len() as u32 }
    pub fn fx_matrices_ptr(&mut self) -> u32 { self.r.effects.matrices.as_mut_ptr() as u32 }
    pub fn fx_matrices_len(&self) -> u32 { self.r.effects.matrices.len() as u32 }
    // Room for `floats` of instances a frame (the data moves: views again).
    pub fn fx_reserve(&mut self, floats: u32) {
        self.remember_fx_reserve(floats); // recovery
 self.r.effects.reserve(floats as usize) }

    // An analytic batches' owner: its tracks' texture and their rows.
    pub fn fx_add_owner(&mut self, tracks: u32, track_rows: f32) -> u32 {
        self.remember_fx_add_owner(tracks, track_rows); // recovery
 self.r.effects.add_owner(&self.r.core, tracks, track_rows) }

    // A particle batch: program, texture (-1 none), three's blending (7
    // values), its corner's location, its instance attributes (location,
    // size pairs, in the order the page writes them), its owner (-1 none).
    pub fn fx_add_batch(&mut self, program: u32, texture: i32, blend: &[u32], corner: u32, attributes: &[u32], owner: i32) -> u32 {
        self.remember_fx_add_batch(program, texture, blend, corner, attributes, owner); // recovery
 self.r.effects.add_batch(&self.r.core, program, texture, blend, corner, attributes, owner)
    }
    pub fn fx_batch_texture(&mut self, batch: u32, texture: i32) {
        self.remember_fx_batch_texture(batch, texture); // recovery
 self.r.effects.batch_texture(&self.r.core, batch, texture) }
    // A batch's own material values ("name:size:form,..."), written after its
    // instances; a texture its program samples by name, on a unit.
    pub fn fx_batch_values(&mut self, batch: u32, values: &str) {
        self.remember_fx_batch_values(batch, values); // recovery
 self.r.effects.batch_values(&self.r.core, batch, values) }
    // A batch whose instances the page writes interleaved, as it keeps them.
    pub fn fx_batch_interleaved(&mut self, batch: u32) {
        self.remember_fx_batch_interleaved(batch); // recovery
 self.r.effects.batch_interleaved(batch) }
    // A batch three draws single-sided: its side (0 front, 1 back).
    pub fn fx_batch_side(&mut self, batch: u32, side: u32) {
        self.remember_fx_batch_side(batch, side); // recovery
 self.r.effects.batch_side(batch, side) }
    // A batch's depth state when it isn't a particle's: flags (1 test, 2 write), function.
    pub fn fx_batch_depth(&mut self, batch: u32, flags: u32, func: u32) {
        self.remember_fx_batch_depth(batch, flags, func); // recovery
 self.r.effects.batch_depth(batch, flags, func) }
    pub fn fx_batch_sampler(&mut self, batch: u32, name: &str, unit: u32, texture: i32) {
        self.remember_fx_batch_sampler(batch, name, unit, texture); // recovery
 self.r.effects.batch_sampler(&self.r.core, batch, name, unit, texture) }
    // A 3D texture a batch samples (the jetpack flame's noise).
    pub fn fx_batch_sampler_3d(&mut self, batch: u32, name: &str, unit: u32, texture: i32) {
        self.remember_fx_batch_sampler_3d(batch, name, unit, texture); // recovery
 self.r.effects.batch_sampler_3d(&self.r.core, batch, name, unit, texture) }
    // A batch's base instead of the corner quad (add_batch's corner then
    // 4294967295): its vertex attributes (location, size pairs), their data
    // (attribute after attribute, `vertices` each), its 16-bit indices.
    pub fn fx_batch_base(&mut self, batch: u32, attributes: &[u32], vertices: u32, data: &[f32], index: &[u16]) {
        self.remember_fx_batch_base(batch, attributes, vertices, data, index); // recovery
 self.r.effects.batch_base(&self.r.core, batch, attributes, vertices, data, index)
    }

    // A dynamic mesh: texture (-1 none), three's blending (7 values), depth
    // flags (1 test, 2 write) and function, its vertex attributes' sizes and
    // its material values ("name:size,..."), in the order the page writes
    // them; then each of three's passes of it, in drawing order: program,
    // side (0 front, 1 back, 2 both), the attributes' locations.
    #[allow(clippy::too_many_arguments)]
    pub fn fx_add_mesh(&mut self, texture: i32, blend: &[u32], depth_flags: u32, depth_func: u32, sizes: &[u32], values: &str) -> u32 {
        self.remember_fx_add_mesh(texture, blend, depth_flags, depth_func, sizes, values); // recovery
 self.r.effects.add_mesh(&self.r.core, texture, blend, depth_flags, depth_func, sizes, values)
    }
    pub fn fx_mesh_pass(&mut self, mesh: u32, program: u32, side: u32, locations: &[u32]) -> u32 {
        self.remember_fx_mesh_pass(mesh, program, side, locations); // recovery
 self.r.effects.mesh_pass(&self.r.core, mesh, program, side, locations) }
    pub fn fx_mesh_texture(&mut self, mesh: u32, texture: i32) {
        self.remember_fx_mesh_texture(mesh, texture); // recovery
 self.r.effects.mesh_texture(&self.r.core, mesh, texture) }

    // A model: texture (-1 none), three's blending (7 values), depth flags
    // (1 test, 2 write) and function, its material values ("name:size" or
    // "name:size:form", comma-separated, form 1 a float array, 4 a vec4
    // array; in the order the page writes them), its geometry's bounding sphere
    // (centre, radius; a radius below 0: never culled); then each of three's
    // passes of it, in drawing order: program, side, the geometry registered
    // for that program; then any texture it samples besides its map, by name,
    // on a unit.
    #[allow(clippy::too_many_arguments)]
    pub fn fx_add_model(&mut self, texture: i32, blend: &[u32], depth_flags: u32, depth_func: u32, values: &str, sphere: &[f64]) -> u32 {
        self.remember_fx_add_model(texture, blend, depth_flags, depth_func, values, sphere); // recovery
 self.r.effects.add_model(&self.r.core, texture, blend, depth_flags, depth_func, values, sphere)
    }
    pub fn fx_model_pass(&mut self, model: u32, program: u32, side: u32, geometry: u32) -> u32 {
        self.remember_fx_model_pass(model, program, side, geometry); // recovery
 self.r.effects.model_pass(&self.r.core, model, program, side, geometry) }
    pub fn fx_model_texture(&mut self, model: u32, texture: i32) {
        self.remember_fx_model_texture(model, texture); // recovery
 self.r.effects.model_texture(&self.r.core, model, texture) }
    pub fn fx_model_sampler(&mut self, model: u32, name: &str, unit: u32, texture: i32) {
        self.remember_fx_model_sampler(model, name, unit, texture); // recovery
 self.r.effects.model_sampler(&self.r.core, model, name, unit, texture) }
}

#[wasm_bindgen]
impl BfeRenderer {
    // Existing descriptors survive settings changes. Rebind both draw owners
    // before publishing the next frame, including mirror and probe items.
    pub fn effect_settings(&mut self,shadows:bool)->Result<(),JsValue>{
        self.remember_effect_settings(shadows); // recovery
 // Preserve first program admissions separately from the final setting.
        // Repeated toggles then keep their original numeric program identities
        // without retaining a resource command for every settings transition.
        let programs=self.r.effects.descriptor_programs().into_iter().chain(self.r.far.descriptor_programs());
        let keys=programs.filter_map(|id|self.r.core.program_key(id).and_then(|key|crate::effect_settings::key(key,shadows))).collect::<Vec<_>>();
        for key in keys {if !self.r.core.has_owned_program(&key){self.add_owned_program(&key)?;}}
 self.r.effects.effect_settings(&mut self.r.core,shadows).map_err(|e|JsValue::from_str(&e))?;
        self.r.far.effect_settings(&mut self.r.core,shadows).map_err(|e|JsValue::from_str(&e))
    }
}
