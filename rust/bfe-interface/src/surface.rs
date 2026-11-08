//! 2D/3D interface drawing surface (`src/interface/surface.js`).
//!
//! Ports `InterfaceSurface` and `commonTextureUrl` from `src/interface/surface.js`.

use crate::fonts::{Fonts, TextDraw, FONT_MEDIUM};
use crate::space::{anchor_matrix, interface_height, Matrix, Point, IH, IW};
pub use bfe_render::interface::{Batch, Draw, Font, QUADS};
use std::collections::HashMap;

/// Resolves a texture name in `common.json`'s `textures` list (`commonTextureUrl` in `src/interface/surface.js`).
pub fn common_texture_url(common: &serde_json::Value, name: &str) -> Option<String> {
    let target = name.to_lowercase();
    let list = common.get("textures")?.as_array()?;
    for entry in list {
        let Some(entry_name) = entry.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        if entry_name.to_lowercase() == target {
            if let Some(file) = entry.get("file").and_then(|v| v.as_str()) {
                return Some(format!("/assets/swbf1/{}", file.trim_start_matches('/')));
            }
        }
    }
    None
}

/// Lightweight surface state (`height`, `solid` texture handle, and named texture map)
/// used by [`crate::theme::ClassicTheme`] (`Surface` in `src/interface/surface.js` and
/// `ShellTextures` in `rust/bfe-shell/src/render.rs`).
#[derive(Clone, Debug)]
pub struct Surface {
    /// 1x1 solid white texture handle (default `1`).
    pub solid: u32,
    /// Virtual interface height (`768.0` at `4:3`, or `768 + (h / w - 0.75) * 768`).
    pub height: f64,
    /// Named texture handles (`buttonleft`, `buttonmid`, `buttonright`, `border_3_pieces`, etc.).
    pub by_name: HashMap<String, u32>,
}

impl Default for Surface {
    fn default() -> Self {
        Self::new(IH)
    }
}

impl Surface {
    /// Creates a new [`Surface`] with virtual `height` and `solid = 1`.
    pub fn new(height: f64) -> Self {
        Self {
            solid: 1,
            height: if height.is_finite() && height > 0.0 {
                height
            } else {
                IH
            },
            by_name: HashMap::new(),
        }
    }

    /// Computes the interface coordinate height `768 + (vh / vw - 0.75) * 768` matching
    /// `gate_layout` in `rust/bfe-shell/src/gate.rs` and `InterfaceSurface.resize` in `surface.js`.
    pub fn from_viewport(vw: f64, vh: f64) -> Self {
        let w = vw.max(1.0);
        let h = vh.max(1.0);
        let height = 768.0 + (h / w - 0.75) * 768.0;
        Self::new(height)
    }

    /// Registers a named texture handle (`"buttonleft"`, `"headerbuttonmid"`, etc.).
    pub fn set_texture(&mut self, name: impl Into<String>, handle: u32) {
        let key = name.into();
        self.by_name.insert(key.to_lowercase(), handle);
        self.by_name.insert(key, handle);
    }

    /// Looks up a named texture handle (`0` if not loaded), matching `ShellTextures::get`
    /// in `rust/bfe-shell/src/render.rs`.
    #[inline]
    pub fn texture(&self, name: &str) -> u32 {
        if let Some(&h) = self.by_name.get(name) {
            return h;
        }
        self.by_name
            .get(&name.to_ascii_lowercase())
            .copied()
            .unwrap_or(0)
    }

    /// Resolves the `OPTIONS` screen poster backdrop (`coreship_screen`, `generic3_screen`, `main`),
    /// matching `ShellTextures::backdrop_for(ScreenId::Options)` in `rust/bfe-shell/src/render.rs`.
    pub fn options_backdrop(&self) -> u32 {
        for name in ["coreship_screen", "generic3_screen", "main"] {
            let tex = self.texture(name);
            if tex != 0 {
                return tex;
            }
        }
        0
    }
}

/// Retained 2D/3D interface drawing surface (`InterfaceSurface` in `src/interface/surface.js`).
pub struct InterfaceSurface {
    pub batch: Batch,
    pub fonts: Fonts,
    pub surface: Surface,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub aspect: f64,
}

impl Default for InterfaceSurface {
    fn default() -> Self {
        Self::new(Fonts::with_builtin_metrics())
    }
}

impl InterfaceSurface {
    pub fn new(fonts: Fonts) -> Self {
        Self {
            batch: Batch::new(),
            fonts,
            surface: Surface::new(IH),
            pixel_width: 1024,
            pixel_height: 768,
            aspect: 1024.0 / 768.0,
        }
    }

    pub fn resize(&mut self, pixel_width: u32, pixel_height: u32) {
        self.pixel_width = pixel_width.max(1);
        self.pixel_height = pixel_height.max(1);
        self.aspect = (self.pixel_width as f64) / (self.pixel_height as f64);
        self.surface.height = interface_height(self.aspect);
    }

    #[inline]
    pub fn anchor(&self, ax: f64, ay: f64, safe: bool) -> Matrix {
        anchor_matrix(ax, ay, safe, self.surface.height)
    }

    #[inline]
    pub fn font(&self, name: &str) -> &Font {
        let key = if name.is_empty() { FONT_MEDIUM } else { name };
        self.fonts.by_name(key)
    }

    #[inline]
    pub fn draw_text(
        &mut self,
        m: &Matrix,
        font_name: &str,
        text: &str,
        x: f64,
        y: f64,
        opts: TextDraw,
    ) {
        self.fonts
            .draw_text(&mut self.batch, m, font_name, text, x, y, opts);
    }

    #[inline]
    pub fn to_interface(
        &self,
        client_x: f64,
        client_y: f64,
        rect_left: f64,
        rect_top: f64,
        rect_width: f64,
        rect_height: f64,
    ) -> Point {
        let w = rect_width.max(1.0);
        let h = rect_height.max(1.0);
        [
            ((client_x - rect_left) / w) * IW,
            ((client_y - rect_top) / h) * self.surface.height,
        ]
    }
}
