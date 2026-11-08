//! Bitmap font registry, measurement, and quad emission (`src/interface/fonts.js`).
//!
//! Ports `Fonts` (`get`, `width`, `draw`) from `src/interface/fonts.js` on top of
//! [`bfe_render::interface::Font`] and [`bfe_render::interface::Glyph`].
//! Uses the exact `f64` advance/bound rounding (`(v * scale + 0.5).floor() + spacing`)
//! and alignment convention (`0` = left, `1` = right, `2` = center) as
//! `src/interface/fonts.js` and `rust/bfe-shell/src/render.rs`.

pub use bfe_render::interface::{Batch, Font, Glyph};
use crate::space::{Color, Matrix, WHITE_COLOR};
use std::collections::HashMap;

/// Largest bitmap font (`"gamefont_large"`, default height `28.0`).
pub const FONT_LARGE: &str = "gamefont_large";

/// Medium bitmap font (`"gamefont_medium"`, default height `20.0`).
pub const FONT_MEDIUM: &str = "gamefont_medium";

/// Small bitmap font (`"gamefont_small"`, default height `16.0`).
pub const FONT_SMALL: &str = "gamefont_small";

/// Tiny bitmap font (`"gamefont_tiny"`, default height `14.0`).
pub const FONT_TINY: &str = "gamefont_tiny";

/// Horizontal text alignment matching `align` in `src/interface/fonts.js` (`0` = left, `1` = right, `2` = center).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// Left-aligned (`align = 0`).
    #[default]
    Left = 0,
    /// Right-aligned (`align = 1`).
    Right = 1,
    /// Horizontally centered (`align = 2`).
    Center = 2,
}

impl TextAlign {
    /// Converts numeric alignment (`0`, `1`, `2`) into [`TextAlign`].
    #[inline]
    pub const fn from_u32(align: u32) -> Self {
        match align {
            1 => Self::Right,
            2 => Self::Center,
            _ => Self::Left,
        }
    }

    /// Returns the numeric alignment code (`0` = left, `1` = right, `2` = center).
    #[inline]
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// Options passed to [`Fonts::draw_text`] (`options` in `Fonts.draw` in `src/interface/fonts.js`).
#[derive(Clone, Copy, Debug)]
pub struct TextDraw {
    pub scale: f64,
    pub scale_y: f64,
    pub spacing: f64,
    pub align: u32,
    pub color: Color,
    pub alpha: f64,
    pub z: f64,
}

impl Default for TextDraw {
    fn default() -> Self {
        Self {
            scale: 1.0,
            scale_y: 1.0,
            spacing: 0.0,
            align: 0,
            color: WHITE_COLOR,
            alpha: 1.0,
            z: 0.0,
        }
    }
}

/// Computes the horizontal advance width of `text` in `font`
/// (`Fonts.width(font, text, scale, spacing)` in `src/interface/fonts.js`).
#[inline]
pub fn font_text_width(font: &Font, text: &str, scale: f64, spacing: f64) -> f64 {
    let mut width = 0.0;
    for ch in text.encode_utf16() {
        if let Some(g) = font.glyphs.get(&ch) {
            width += (g.advance * scale + 0.5).floor() + spacing;
        }
    }
    width
}

/// Emits textured glyph quads for `text` into `batch`
/// (`Fonts.draw(batch, m, font, text, x, y, options)` in `src/interface/fonts.js`).
pub fn font_draw_text(
    font: &Font,
    batch: &mut Batch,
    m: &Matrix,
    text: &str,
    x: f64,
    y: f64,
    opts: TextDraw,
) {
    if text.is_empty() {
        return;
    }
    let width = font_text_width(font, text, opts.scale, opts.spacing);
    let mut pen = match opts.align {
        1 => x - width,
        2 => x - width * 0.5,
        _ => x,
    };
    let tint = [
        opts.color[0],
        opts.color[1],
        opts.color[2],
        opts.color[3] * opts.alpha,
    ];
    let sy = opts.scale * opts.scale_y;
    for ch in text.encode_utf16() {
        if let Some(g) = font.glyphs.get(&ch) {
            let x0 = (g.bounds[0] * opts.scale + 0.5).floor();
            let y0 = (g.bounds[1] * sy + 0.5).floor();
            let x1 = (g.bounds[2] * opts.scale + 0.5).floor();
            let y1 = (g.bounds[3] * sy + 0.5).floor();
            if g.bounds[0] < g.bounds[2] && g.page != 0 {
                batch.quad(
                    *m,
                    [pen + x0, y + y0, pen + x1, y + y1],
                    g.page,
                    g.uv,
                    tint,
                    opts.z,
                    0,
                );
            }
            pen += (g.advance * opts.scale + 0.5).floor() + opts.spacing;
        }
    }
}

/// Four-font bitmap registry (`gamefont_large`, `gamefont_medium`, `gamefont_small`, `gamefont_tiny`)
/// (`Fonts` in `src/interface/fonts.js` and `ShellFonts` in `rust/bfe-shell/src/render.rs`).
#[derive(Default)]
pub struct Fonts {
    pub large: Font,
    pub medium: Font,
    pub small: Font,
    pub tiny: Font,
}

impl Fonts {
    /// Creates a [`Fonts`] table with default metric glyphs for headless unit tests.
    pub fn with_builtin_metrics() -> Self {
        Self {
            large: builtin_font(28.0, 14.0),
            medium: builtin_font(20.0, 10.0),
            small: builtin_font(16.0, 8.0),
            tiny: builtin_font(14.0, 7.0),
        }
    }

    /// Looks up a [`Font`] by its `common.lvl` name (`Fonts.get(name)` in `src/interface/fonts.js`).
    #[inline]
    pub fn by_name(&self, name: &str) -> &Font {
        match name {
            FONT_LARGE => &self.large,
            FONT_SMALL => &self.small,
            FONT_TINY => &self.tiny,
            _ => &self.medium,
        }
    }

    /// Returns the line height of `font_name`.
    #[inline]
    pub fn height(&self, font_name: &str) -> f64 {
        self.by_name(font_name).height
    }

    /// Returns the bottom bound of capital `'A'` in `font_name` (`glyphs.get(65)?.bounds?.[3] || height`).
    #[inline]
    pub fn glyph_height(&self, font_name: &str) -> f64 {
        let f = self.by_name(font_name);
        f.glyphs.get(&65).map_or(f.height, |g| g.bounds[3])
    }

    /// Returns the vertical center of capital `'A'` in `font_name` (`Font.capsMiddle()` in `fonts.js`).
    #[inline]
    pub fn caps_middle(&self, font_name: &str) -> f64 {
        self.by_name(font_name).caps_middle()
    }

    /// Measures `text` in `font_name` at `scale` and `spacing` (`Fonts.width` in `src/interface/fonts.js`).
    #[inline]
    pub fn width(&self, font_name: &str, text: &str, scale: f64, spacing: f64) -> f64 {
        font_text_width(self.by_name(font_name), text, scale, spacing)
    }

    /// Draws `text` in `font_name` into `batch` (`Fonts.draw` in `src/interface/fonts.js`).
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(
        &self,
        batch: &mut Batch,
        m: &Matrix,
        font_name: &str,
        text: &str,
        x: f64,
        y: f64,
        opts: TextDraw,
    ) {
        font_draw_text(self.by_name(font_name), batch, m, text, x, y, opts);
    }
}

fn builtin_font(height: f64, advance: f64) -> Font {
    let mut glyphs = HashMap::with_capacity(96);
    for code in 32u16..=126u16 {
        let adv = if code == 32 {
            (advance * 0.5).round()
        } else {
            advance
        };
        let bounds = if code == 32 {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            [0.0, 2.0, adv - 1.0, height - 2.0]
        };
        glyphs.insert(
            code,
            Glyph {
                page: 1,
                advance: adv,
                bounds,
                uv: [0.0, 0.0, 1.0, 1.0],
            },
        );
    }
    Font { height, glyphs }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::space::IDENTITY;

    // Verifies `Fonts.width` and `Fonts.draw` against `src/interface/fonts.js`.
    #[test]
    fn font_width_and_draw_emit_expected_quads() {
        let fonts = Fonts::with_builtin_metrics();
        let w = fonts.width(FONT_MEDIUM, "AB C", 1.0, 0.0);
        // 'A' (10) + 'B' (10) + ' ' (5) + 'C' (10) = 35
        assert!((w - 35.0).abs() < 1e-9);

        let mut batch = Batch::new();
        batch.begin();
        fonts.draw_text(
            &mut batch,
            &IDENTITY,
            FONT_MEDIUM,
            "AB C",
            0.0,
            0.0,
            TextDraw::default(),
        );
        // Space (' ') has empty bounds, so 3 visible glyph quads are emitted.
        assert_eq!(batch.count, 3);
    }
}
