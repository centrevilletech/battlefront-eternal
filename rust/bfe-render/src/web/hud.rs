// The HUD pass's bindings (hud.rs): its values and the mirrors of the
// batch's vertex arrays the page writes into, and the batch's index, once.
use super::BfeRenderer;
use crate::hud_model::layout::HudGlyph as G;
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

// The model survives context reconstruction: asset handles and clocks are CPU
// state. GPU handles are refreshed by the page before the next build.
#[wasm_bindgen]
impl BfeRenderer {
    pub fn hud_model_input_ptr(&mut self) -> u32 { self.hud_model.input.as_mut_ptr() as u32 }
    pub fn hud_model_input_len(&self) -> u32 { self.hud_model.input.len() as u32 }
    pub fn hud_model_text(&mut self, text: &str, uppercase: bool) -> u32 { self.hud_model.add_text(text, uppercase) }
    pub fn hud_model_asset(&mut self, id: u32, texture: i32, width: f64, height: f64, ready: bool) {
        if let Some(asset) = self.hud_model.assets.get_mut(id as usize) {
            *asset = crate::interface::Asset { texture, width, height, ready };
        }
    }
    pub fn hud_model_font(&mut self, font: u32, height: f64, rows: &[f64]) {
        let f = &mut self.hud_model.fonts[font as usize]; f.height = height;
        assert_eq!(rows.len() % G::WORDS, 0, "HUD glyph layout mismatch");
        for row in rows.chunks_exact(G::WORDS) {
            f.glyphs.insert(row[G::CODE] as u16, crate::interface::Glyph { page:row[G::PAGE] as u32, advance:row[G::ADVANCE], bounds:[row[G::X0],row[G::Y0],row[G::X1],row[G::Y1]], uv:[row[G::U0],row[G::V0],row[G::U1],row[G::V1]] });
        }
    }
    // Immediate scalar events cannot back up behind a slow loading frame.
    pub fn hud_model_event(&mut self, kind: u32, a: f64, b: f64, c: f64, d: f64) { self.hud_model.event(kind, a, b, c, d); }
    // Loss/stat flashes depend on intermediate samples even when their final
    // bar/number state is unchanged. No queue or geometry is involved here.
    pub fn hud_model_observe(&mut self, kind: u32, a: f64, b: f64, c: f64, d: f64) { self.hud_model.event(kind, a, b, c, d); }
    // BattleHud.update publishes the latest state rows once.
    pub fn hud_model_update(&mut self) -> f64 { self.hud_model.update(); self.hud_model.time }
    // Publish native geometry directly into HudPass's retained GPU mirrors.
    // No vertices or draw records cross back through JavaScript for battle.
    pub fn hud_model_build(&mut self, mode: u32, full: bool) -> u32 {
        use crate::hud::*;
        self.hud_model.build(mode);
        let b=&self.hud_model.batch; let h=&mut self.r.hud;
        let (start,end)=if full {(0,b.count*4)}else{(b.start,b.end)};
        if end>start {
            h.position[start*3..end*3].copy_from_slice(&b.position[start*3..end*3]);
            h.uv[start*2..end*2].copy_from_slice(&b.uv[start*2..end*2]);
            h.mask_uv[start*2..end*2].copy_from_slice(&b.mask_uv[start*2..end*2]);
            h.tint[start*4..end*4].copy_from_slice(&b.tint[start*4..end*4]);
        }
        h.input[IN_UPLOAD]=start as f64; h.input[IN_UPLOAD+1]=end as f64;
        h.input[IN_GROUPS]=b.draws.len().min(MAX_GROUPS) as f64;
        for (i,draw) in b.draws.iter().take(MAX_GROUPS).enumerate() {
            let o=IN_FIRST_GROUP+i*GROUP;
            h.input[o]=draw.first as f64; h.input[o+1]=draw.count as f64;
            h.input[o+2]=self.hud_model.asset(draw.texture).texture as f64;
            h.input[o+3]=self.hud_model.asset(if draw.mask==0{draw.texture}else{draw.mask}).texture as f64;
            h.input[o+4]=(draw.mask!=0) as u8 as f64;
        }
        b.count as u32
    }
}
