// The world pass's bindings (world.rs): the map's opaque buckets, each
// registered once, then put in drawing order.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    // bounds: sphere centre and radius, then box min and max (world space).
    pub fn add_world(&mut self, geometry: u32, program: u32, order: u32, flags: u32, bounds: &[f64]) -> u32 {
        self.r.world.add_world(geometry, program, order, flags, bounds)
    }
    pub fn world_texture(&mut self, w: u32, unit: u32, texture: u32) { self.r.world.world_texture(&self.r.core, w, unit, texture) }
    pub fn world_uniform(&mut self, w: u32, name: &str, values: &[f32], int: bool) -> bool { self.r.world.world_uniform(&self.r.core, w, name, values, int) }
    pub fn world_cells(&mut self, w: u32, positions: &[f32], index: &[u32], cell_size: f64) { self.r.world.world_cells(w, positions, index, cell_size) }
    pub fn sort(&mut self, by_program: bool) { self.r.world.sort(by_program) }
    // Cells given whole: [first index, end] and a box [min, max] each.
    pub fn world_runs(&mut self, w: u32, ranges: &[u32], boxes: &[f64]) { self.r.world.world_runs(w, ranges, boxes) }
    pub fn world_shown(&mut self, w: u32, shown: bool) { self.r.world.world_shown(w, shown) }
}
