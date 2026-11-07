// The shadow pass's bindings (shadow.rs): a cascade's pass, written by the
// page (src/rust-render/shadow.js) and drawn in three's shadow pass's place.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn static_shadow_input_ptr(&mut self)->u32 {self.r.shadow.static_view.as_mut_ptr() as u32}
    pub fn static_shadow_input_len(&self)->u32 {self.r.shadow.static_view.len() as u32}
    pub fn caster_input_ptr(&mut self) -> u32 { self.r.shadow.input.as_mut_ptr() as u32 }
    pub fn caster_input_len(&self) -> u32 { self.r.shadow.input.len() as u32 }
    // A rewritten geometry's buffer (its slot) or index.
    // ShadowPass uploads mutable caster attributes through the same retained
    // geometry table as other passes. Full replacements supersede old contents.
    pub fn geometry_buffer_data(&mut self, g: u32, slot: u32, data: &[u8]) {
        self.remember_geometry_buffer_data(g, slot, data); // recovery
 self.r.core.geometry_buffer_data(g, slot, data) }
    pub fn geometry_index_data(&mut self, g: u32, data: &[u8]) {
        self.remember_geometry_index_data(g, data); // recovery
 self.r.core.geometry_index_data(g, data) }
    // Part of one, at a byte offset (an attribute's update ranges).
    // Overlapping subranges need their original last-write order on recovery.
    // Offset alone is not a replacement key: shorter writes preserve a suffix.
    pub fn geometry_buffer_sub_data(&mut self, g: u32, slot: u32, offset: i32, data: &[u8]) {
        self.remember_geometry_buffer_sub_data(g, slot, offset, data); // recovery
 self.r.core.geometry_buffer_sub_data(g, slot, offset, data) }
    pub fn geometry_index_sub_data(&mut self, g: u32, offset: i32, data: &[u8]) {
        self.remember_geometry_index_sub_data(g, offset, data); // recovery
 self.r.core.geometry_index_sub_data(g, offset, data) }
}
