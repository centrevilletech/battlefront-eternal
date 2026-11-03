// The far pass's bindings (far.rs): its input, and its items, each
// registered once from a mesh of the far scene.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    // Where the page writes the far phase's values each frame (f64).
    pub fn far_input_ptr(&mut self) -> u32 { self.r.far.input.as_mut_ptr() as u32 }
    pub fn far_input_len(&self) -> u32 { self.r.far.input.len() as u32 }

    // The uniforms the far pass sets for a program: names (comma-separated),
    // GL types, array lengths. undefined when it can't set them all.
    pub fn far_table(&mut self, program: u32, names: &str, types: &[u32], counts: &[u32]) -> Option<u32> {
        self.r.far.add_table(&mut self.r.core, program, names, types, counts)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn far_item(&mut self, program: u32, table: u32, geometry: u32, flags: u32, depth_func: u32, blend: &[u32], order: &[f64], sphere: &[f64], slot: u32) -> u32 {
        self.r.far.add_item(&self.r.core, program, table, geometry, flags, depth_func, blend, order, sphere, slot)
    }
    pub fn far_source(&mut self, item: u32, uniform: u32, kind: u32, values: &[f32], at: u32) { self.r.far.source(item, uniform, kind, values, at) }
    pub fn far_range(&mut self, item: u32, start: i32, count: i32) { self.r.far.range(item, start, count) }
    pub fn far_base(&mut self, item: u32, base: i32) { self.r.far.base(item, base) }
    #[allow(clippy::too_many_arguments)]
    pub fn far_instanced(&mut self, item: u32, location: u32, columns: u32, rows: i32, max: u32, count: u32, at: u32) {
        self.r.far.instanced(&mut self.r.core, item, location, columns, rows, max, count, at)
    }
    pub fn far_cells(&mut self, item: u32, positions: &[f32], index: &[u32], cell_size: f64) { self.r.far.cells(item, positions, index, cell_size) }
    pub fn far_terrain(&mut self, item: u32, level: u32, index: &[u32], cells: &[f64]) { self.r.far.terrain(&mut self.r.core, item, level, index, cells) }
    pub fn far_items(&self) -> u32 { self.r.far.items() }

    // The far phase from a water's mirror camera (renderer.rs
    // far_mirror_frame): its stats, as far_frame's.
    pub fn far_mirror_frame(&mut self) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.far_mirror_frame().map_err(|e| JsValue::from_str(&e))?.values())
    }
}
