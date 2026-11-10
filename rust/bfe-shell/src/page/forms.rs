//! Shared form label drawing for remaining legacy Page screens.
use super::Page;
use bfe_render::interface::{Batch, Color, Matrix};
pub(super) enum FormFace {
    Medium,
}
impl Page {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn form_label(
        &mut self,
        b: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        spacing: f64,
        color: Color,
        align: u32,
        face: FormFace,
    ) {
        if !self.text.contains_key(text) {
            self.text.insert(text.into(), text.encode_utf16().collect());
        }
        let font = match face {
            FormFace::Medium => &self.medium,
        };
        Self::draw_text(
            font,
            b,
            m,
            x,
            y,
            &self.text[text],
            color,
            align,
            size,
            spacing,
        );
    }
}
