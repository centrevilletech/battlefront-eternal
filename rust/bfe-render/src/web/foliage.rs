// The foliage pass's bindings (foliage.rs): each class registered once (its
// cards, patches and program), and the selection the page asks for each
// time its foliage updates, from the values it writes at foliage_input_ptr.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn foliage_input_ptr(&mut self) -> u32 { self.r.foliage.input.as_mut_ptr() as u32 }
    pub fn foliage_input_len(&self) -> u32 { self.r.foliage.input.len() as u32 }
    #[allow(clippy::too_many_arguments)]
    pub fn foliage_class(&mut self, program: u32, texture: u32, grass: bool, wiggle: f32, vertices: &[f32], index: &[u32], locations: &[i32],
        rows: &[f32], bounds: &[f64], map_matrix: &[f32], diffuse: &[f32], opacity: f32, alpha_test: f32) -> u32 {
        self.remember_foliage_class(program, texture, grass, wiggle, vertices, index, locations, rows, bounds, map_matrix, diffuse, opacity, alpha_test); // recovery
 self.r.foliage.add_class(&mut self.r.core, program, texture, grass, wiggle, vertices, index, locations, rows, bounds, map_matrix, diffuse, opacity, alpha_test)
    }
    // The selection for this update (FoliageGpuBatch.update), drawn by the
    // next near phase; the patches selected.
    pub fn foliage_frame(&mut self) -> u32 { self.r.foliage.select() }
    pub fn foliage_selected(&self, class: u32) -> u32 { self.r.foliage.selected(class) }
    // Off: the renderer draws no foliage (three does).

}
