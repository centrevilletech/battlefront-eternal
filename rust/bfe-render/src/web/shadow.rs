// The shadow pass's bindings (shadow.rs): a cascade's pass, written by the
// page (src/rust-render/shadow.js) and drawn in three's shadow pass's place.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn caster_input_ptr(&mut self) -> u32 { self.r.shadow.input.as_mut_ptr() as u32 }
    pub fn caster_input_len(&self) -> u32 { self.r.shadow.input.len() as u32 }
    pub fn shadow_frame(&mut self) -> Result<Vec<u32>, JsValue> {
        Ok(self.r.shadow_frame().map_err(|e| JsValue::from_str(&e))?.values())
    }
    // A rewritten geometry's buffer (its slot) or index.
    pub fn geometry_buffer_data(&mut self, g: u32, slot: u32, data: &[u8]) { self.r.core.geometry_buffer_data(g, slot, data) }
    pub fn geometry_index_data(&mut self, g: u32, data: &[u8]) { self.r.core.geometry_index_data(g, data) }
}
