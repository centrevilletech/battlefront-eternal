use crate::render_bridge::Bridge;

impl Bridge {
    /// Copies the built [`bfe_render::hud_model::HudModel`] quad batch and texture
    /// groups into the native [`bfe_render::hud::HudPass`] GPU staging buffers.
    pub fn hud_sync_batch(&mut self, full: bool) -> u32 {
        use bfe_render::hud::*;
        let b = &self.hud_model.batch;
        let h = &mut self.r.hud;
        let (start, end) = if full { (0, b.count * 4) } else { (b.start, b.end) };
        if end > start {
            h.position[start * 3..end * 3].copy_from_slice(&b.position[start * 3..end * 3]);
            h.uv[start * 2..end * 2].copy_from_slice(&b.uv[start * 2..end * 2]);
            h.mask_uv[start * 2..end * 2].copy_from_slice(&b.mask_uv[start * 2..end * 2]);
            h.tint[start * 4..end * 4].copy_from_slice(&b.tint[start * 4..end * 4]);
        }
        h.input[IN_UPLOAD] = start as f64;
        h.input[IN_UPLOAD + 1] = end as f64;
        h.input[IN_GROUPS] = b.draws.len().min(MAX_GROUPS) as f64;
        for (i, draw) in b.draws.iter().take(MAX_GROUPS).enumerate() {
            let o = IN_FIRST_GROUP + i * GROUP;
            h.input[o] = draw.first as f64;
            h.input[o + 1] = draw.count as f64;
            h.input[o + 2] = self.hud_model.asset(draw.texture).texture as f64;
            h.input[o + 3] = self
                .hud_model
                .asset(if draw.mask == 0 { draw.texture } else { draw.mask })
                .texture as f64;
            h.input[o + 4] = u8::from(draw.mask != 0) as f64;
        }
        b.count as u32
    }
}
