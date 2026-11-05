// The HUD pass's bindings (hud.rs): its values and the mirrors of the
// batch's vertex arrays the page writes into, and the batch's index, once.
use super::BfeRenderer;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn hud_input_ptr(&mut self) -> u32 { self.r.hud.input.as_mut_ptr() as u32 }
    pub fn hud_input_len(&self) -> u32 { self.r.hud.input.len() as u32 }
    pub fn hud_position_ptr(&mut self) -> u32 { self.r.hud.position.as_mut_ptr() as u32 }
    pub fn hud_position_len(&self) -> u32 { self.r.hud.position.len() as u32 }
    pub fn hud_uv_ptr(&mut self) -> u32 { self.r.hud.uv.as_mut_ptr() as u32 }
    pub fn hud_uv_len(&self) -> u32 { self.r.hud.uv.len() as u32 }
    pub fn hud_mask_uv_ptr(&mut self) -> u32 { self.r.hud.mask_uv.as_mut_ptr() as u32 }
    pub fn hud_mask_uv_len(&self) -> u32 { self.r.hud.mask_uv.len() as u32 }
    pub fn hud_tint_ptr(&mut self) -> u32 { self.r.hud.tint.as_mut_ptr() as u32 }
    pub fn hud_tint_len(&self) -> u32 { self.r.hud.tint.len() as u32 }
// Batch indices are static and journalled once. Live HUD vertices remain in
// shared CPU mirrors; rebuild reuploads them after recreating this VAO.
// A restored empty allocation is not enough even if its logical id is stable.
// No HUD frame is submitted while those mirrors are being transferred.
    pub fn hud_init(&mut self, index: &[u32]) {
        self.remember_hud_init(index); // recovery
 self.r.hud.init(&mut self.r.core, index) }
}
