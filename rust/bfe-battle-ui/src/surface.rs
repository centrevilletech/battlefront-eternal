//! `BattleUiSurface` abstraction, `HudBatchSurface` adapter, `OwnedUiSurface` test/demo surface,
//! font slot constants, and `BattleUiAssets` (`src/interface/surface.js`, `src/interface/fonts.js`).

use std::collections::HashMap;

use bfe_render::interface::{Asset, Batch, Color, Font, Glyph, Matrix};

use crate::shapes::{glyph_height, interface_height, place_interface_model};

/// Font index constants matching `HudModel::fonts`.
pub const FONT_TINY: usize = 0;
pub const FONT_SMALL: usize = 1;
pub const FONT_MEDIUM: usize = 2;
pub const FONT_LARGE: usize = 3;

/// Font name strings matching `common.json` (`src/interface/fonts.js`).
pub const FONT_NAME_TINY: &str = "gamefont_tiny";
pub const FONT_NAME_SMALL: &str = "gamefont_small";
pub const FONT_NAME_MEDIUM: &str = "gamefont_medium";
pub const FONT_NAME_LARGE: &str = "gamefont_large";

/// Maps a font name string (`"gamefont_tiny"`..`"gamefont_large"`) to its index (`0..4`).
#[inline]
pub fn font_index_by_name(name: &str) -> usize {
    match name {
        "gamefont_small" => FONT_SMALL,
        "gamefont_medium" => FONT_MEDIUM,
        "gamefont_large" => FONT_LARGE,
        _ => FONT_TINY,
    }
}

/// Small drawing trait abstracting the 2D HUD/interface quad batch and bitmap
/// fonts (`task-battle-ui.md`). Allows pure-Rust unit tests to record quads and
/// labels without OpenGL, and lets `bfe-interface` plug in directly when published.
pub trait BattleUiSurface {
    /// Current interface plane height `H`.
    fn height(&self) -> f64;
    /// Window aspect ratio (`width / height`).
    fn aspect(&self) -> f64 {
        4.0 / 3.0
    }
    /// Solid 1x1 white fallback texture asset ID.
    fn solid_texture(&self) -> u32 {
        bfe_render::hud_model::layout::HudArt::SOLID
    }
    /// Resolves a named texture asset ID (`"border_3_pieces"`, `"mousecursor"`, etc.), or `0` if absent.
    fn texture(&self, _name: &str) -> u32 {
        0
    }
    /// Resolves a localized string key with a fallback (`strings[key] || fallback`).
    fn string<'a>(&'a self, _key: &str, fallback: &'a str) -> &'a str {
        fallback
    }
    /// Line height of the requested font slot (`FONT_TINY..=FONT_LARGE`).
    fn font_height(&self, font: usize) -> f64;
    /// Advance width of `text` in `font` at `scale` and `spacing`.
    fn font_width(&self, font: usize, text: &[u16], scale: f64, spacing: f64) -> f64;
    /// Metadata (`texture`, `width`, `height`, `ready`) of `asset_id`.
    fn asset(&self, asset_id: u32) -> Asset;
    /// Mutable access to the underlying `Batch` for emitting quads.
    fn batch_mut(&mut self) -> &mut Batch;
    /// Immutable access to font `font` (`0..4`).
    fn font(&self, font: usize) -> &Font;
    /// Splits `batch` and `fonts` simultaneously so callers can draw text without
    /// cloning a `Font`.
    fn batch_and_font(&mut self, font: usize) -> (&mut Batch, &Font);

    /// Line height by font name (`"gamefont_medium"`, etc.).
    fn line_height(&self, font_name: &str) -> f64 {
        self.font_height(font_index_by_name(font_name))
    }

    /// Deepest glyph reach by font name (`glyphHeight` in `src/interface/fonts.js`).
    fn glyph_height_by_name(&self, font_name: &str) -> f64 {
        glyph_height(self.font(font_index_by_name(font_name)))
    }

    /// Vertical midpoint of capital `'A'` (`capsMiddle` in `src/interface/fonts.js`).
    fn caps_middle(&self, font_name: &str) -> f64 {
        let f = self.font(font_index_by_name(font_name));
        if let Some(g) = f.glyphs.get(&(b'A' as u16)) {
            if g.bounds[0] < g.bounds[2] {
                return (g.bounds[1] + g.bounds[3]) * 0.5;
            }
        }
        f.height * 0.5
    }

    /// Measures `&str` width in `font_name` at `scale`.
    fn text_width(&self, font_name: &str, text: &str, scale: f64) -> f64 {
        let units: Vec<u16> = text.encode_utf16().collect();
        self.font_width(font_index_by_name(font_name), &units, scale, 1.0)
    }

    /// Draws `&str` text into `batch` using `font_name`.
    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &self,
        batch: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &str,
        font_name: &str,
        color: Color,
        align: u32,
        scale: f64,
    ) {
        let units: Vec<u16> = text.encode_utf16().collect();
        self.font(font_index_by_name(font_name))
            .draw(batch, m, x, y, &units, color, align, scale, 1.0, 0.0);
    }

    /// Places a 3D interface model at `(x, y)` on the interface plane.
    fn place_on_plane(&self, x: f64, y: f64) -> [f64; 3] {
        place_interface_model(x, y, self.aspect(), self.height())
    }
}

/// Concrete [`BattleUiSurface`] backed by a [`Batch`], four [`Font`]s, and an
/// [`Asset`] slice (such as `bfe_render::hud_model::HudModel`).
pub struct HudBatchSurface<'a> {
    pub height: f64,
    pub aspect: f64,
    pub batch: &'a mut Batch,
    pub fonts: &'a [Font; 4],
    pub assets: &'a [Asset],
    pub textures: Option<&'a HashMap<String, u32>>,
    pub strings: Option<&'a HashMap<String, String>>,
}

impl<'a> HudBatchSurface<'a> {
    /// Creates a `HudBatchSurface` with default 4:3 aspect and optional texture/string maps.
    pub fn new(
        height: f64,
        batch: &'a mut Batch,
        fonts: &'a [Font; 4],
        assets: &'a [Asset],
    ) -> Self {
        Self {
            height,
            aspect: 4.0 / 3.0,
            batch,
            fonts,
            assets,
            textures: None,
            strings: None,
        }
    }
}

impl<'a> BattleUiSurface for HudBatchSurface<'a> {
    #[inline]
    fn height(&self) -> f64 {
        self.height
    }

    #[inline]
    fn aspect(&self) -> f64 {
        self.aspect
    }

    #[inline]
    fn texture(&self, name: &str) -> u32 {
        self.textures
            .and_then(|m| m.get(name).copied())
            .unwrap_or(0)
    }

    #[inline]
    fn string<'b>(&'b self, key: &str, fallback: &'b str) -> &'b str {
        self.strings
            .and_then(|m| m.get(key).map(|s| s.as_str()))
            .unwrap_or(fallback)
    }

    #[inline]
    fn font_height(&self, font: usize) -> f64 {
        self.fonts[font.min(3)].height
    }

    #[inline]
    fn font_width(&self, font: usize, text: &[u16], scale: f64, spacing: f64) -> f64 {
        self.fonts[font.min(3)].width(text, scale, spacing)
    }

    #[inline]
    fn asset(&self, asset_id: u32) -> Asset {
        self.assets.get(asset_id as usize).copied().unwrap_or(Asset {
            texture: -1,
            width: 0.0,
            height: 0.0,
            ready: false,
        })
    }

    #[inline]
    fn batch_mut(&mut self) -> &mut Batch {
        self.batch
    }

    #[inline]
    fn font(&self, font: usize) -> &Font {
        &self.fonts[font.min(3)]
    }

    #[inline]
    fn batch_and_font(&mut self, font: usize) -> (&mut Batch, &Font) {
        (self.batch, &self.fonts[font.min(3)])
    }
}

/// Self-contained [`BattleUiSurface`] owning its own [`Batch`], four synthetic or loaded [`Font`]s,
/// texture lookup table, and string table for standalone demos and unit tests.
pub struct OwnedUiSurface {
    pub width_px: u32,
    pub height_px: u32,
    pub height: f64,
    pub aspect: f64,
    pub solid: u32,
    pub batch: Batch,
    pub fonts: [Font; 4],
    pub assets: Vec<Asset>,
    pub textures: HashMap<String, u32>,
    pub strings: HashMap<String, String>,
}

impl Default for OwnedUiSurface {
    fn default() -> Self {
        Self::new(1024, 768)
    }
}

impl OwnedUiSurface {
    /// Creates an `OwnedUiSurface` for viewport `(width_px, height_px)` with default fallback ASCII glyph metrics.
    pub fn new(width_px: u32, height_px: u32) -> Self {
        let aspect = width_px.max(1) as f64 / height_px.max(1) as f64;
        let height = interface_height(width_px as f64, height_px as f64);
        let make_default_font = |line_h: f64, adv: f64| -> Font {
            let mut glyphs = HashMap::new();
            for ch in 32u16..=126u16 {
                glyphs.insert(
                    ch,
                    Glyph {
                        page: 1,
                        advance: adv,
                        bounds: [0.0, 2.0, adv - 1.0, line_h - 2.0],
                        uv: [0.0, 0.0, 1.0, 1.0],
                    },
                );
            }
            Font {
                height: line_h,
                glyphs,
            }
        };
        let fonts = [
            make_default_font(15.0, 7.0),  // FONT_TINY (0)
            make_default_font(21.0, 9.0),  // FONT_SMALL (1)
            make_default_font(23.0, 11.0), // FONT_MEDIUM (2)
            make_default_font(32.0, 15.0), // FONT_LARGE (3)
        ];
        Self {
            width_px,
            height_px,
            height,
            aspect,
            solid: bfe_render::hud_model::layout::HudArt::SOLID,
            batch: Batch::new(),
            fonts,
            assets: vec![
                Asset {
                    texture: 0,
                    width: 64.0,
                    height: 64.0,
                    ready: true,
                };
                64
            ],
            textures: HashMap::new(),
            strings: HashMap::new(),
        }
    }

    /// Updates viewport dimensions and recomputes `height` and `aspect`.
    pub fn set_viewport(&mut self, width_px: u32, height_px: u32) {
        self.width_px = width_px.max(1);
        self.height_px = height_px.max(1);
        self.aspect = self.width_px as f64 / self.height_px as f64;
        self.height = interface_height(self.width_px as f64, self.height_px as f64);
    }
}

impl BattleUiSurface for OwnedUiSurface {
    #[inline]
    fn height(&self) -> f64 {
        self.height
    }

    #[inline]
    fn aspect(&self) -> f64 {
        self.aspect
    }

    #[inline]
    fn solid_texture(&self) -> u32 {
        self.solid
    }

    #[inline]
    fn texture(&self, name: &str) -> u32 {
        self.textures.get(name).copied().unwrap_or(0)
    }

    #[inline]
    fn string<'a>(&'a self, key: &str, fallback: &'a str) -> &'a str {
        self.strings.get(key).map(|s| s.as_str()).unwrap_or(fallback)
    }

    #[inline]
    fn font_height(&self, font: usize) -> f64 {
        self.fonts[font.min(3)].height
    }

    #[inline]
    fn font_width(&self, font: usize, text: &[u16], scale: f64, spacing: f64) -> f64 {
        self.fonts[font.min(3)].width(text, scale, spacing)
    }

    #[inline]
    fn asset(&self, asset_id: u32) -> Asset {
        self.assets.get(asset_id as usize).copied().unwrap_or(Asset {
            texture: -1,
            width: 0.0,
            height: 0.0,
            ready: false,
        })
    }

    #[inline]
    fn batch_mut(&mut self) -> &mut Batch {
        &mut self.batch
    }

    #[inline]
    fn font(&self, font: usize) -> &Font {
        &self.fonts[font.min(3)]
    }

    #[inline]
    fn batch_and_font(&mut self, font: usize) -> (&mut Batch, &Font) {
        (&mut self.batch, &self.fonts[font.min(3)])
    }
}

/// Asset IDs used by the Classic battle UI screens (spawn screen, scoreboard,
/// pause menu). Maps to built-in `HudArt` slots (`SOLID`, `HUD_FLAG_ICON`, `STATS_*`)
/// plus custom interface textures appended to `HudModel::assets` (or `0` when a
/// texture is absent so the views fall back to their solid/stand-in geometry).
#[derive(Clone, Copy, Debug)]
pub struct BattleUiAssets {
    pub solid: u32,
    pub post_icon: u32,
    pub border_3_pieces: u32,
    pub opaque_rect: u32,
    pub opaque_black: u32,
    pub headerbuttonleft: u32,
    pub headerbuttonmid: u32,
    pub headerbuttonright: u32,
    pub buttonleft: u32,
    pub buttonmid: u32,
    pub buttonright: u32,
    pub backbutton_left: u32,
    pub backbutton_mid: u32,
    pub backbutton_right: u32,
    pub listbox_cursor: u32,
    pub listbox_hilight: u32,
    pub small_arrow: u32,
    pub stats_kills: u32,
    pub stats_deaths: u32,
    pub stats_cpp: u32,
    pub cursor: u32,
    pub mousecursor: u32,
}

impl BattleUiAssets {
    /// Resolves `BattleUiAssets` handles from `surface.texture(name)`.
    pub fn from_surface(surface: &dyn BattleUiSurface) -> Self {
        use bfe_render::hud_model::layout::HudArt as A;
        let tex_or = |name: &str, fallback: u32| {
            let id = surface.texture(name);
            if id != 0 { id } else { fallback }
        };
        Self {
            solid: surface.solid_texture(),
            post_icon: tex_or("hud_flag_icon", A::HUD_FLAG_ICON),
            border_3_pieces: surface.texture("border_3_pieces"),
            opaque_rect: surface.texture("opaque_rect"),
            opaque_black: surface.texture("opaque_black"),
            headerbuttonleft: surface.texture("headerbuttonleft"),
            headerbuttonmid: surface.texture("headerbuttonmid"),
            headerbuttonright: surface.texture("headerbuttonright"),
            buttonleft: surface.texture("buttonleft"),
            buttonmid: surface.texture("buttonmid"),
            buttonright: surface.texture("buttonright"),
            backbutton_left: surface.texture("backbutton_left"),
            backbutton_mid: surface.texture("backbutton_mid"),
            backbutton_right: surface.texture("backbutton_right"),
            listbox_cursor: surface.texture("listbox_cursor"),
            listbox_hilight: surface.texture("listbox_hilight"),
            small_arrow: surface.texture("small_arrow"),
            stats_kills: tex_or("stats_kills", A::STATS_KILLS),
            stats_deaths: tex_or("stats_deaths", A::STATS_DEATHS),
            stats_cpp: tex_or("stats_cpp", A::STATS_CPP),
            cursor: surface.texture("cursor"),
            mousecursor: surface.texture("mousecursor"),
        }
    }
}

impl Default for BattleUiAssets {
    fn default() -> Self {
        use bfe_render::hud_model::layout::HudArt as A;
        Self {
            solid: A::SOLID,
            post_icon: A::HUD_FLAG_ICON,
            border_3_pieces: 0,
            opaque_rect: 0,
            opaque_black: 0,
            headerbuttonleft: 0,
            headerbuttonmid: 0,
            headerbuttonright: 0,
            buttonleft: 0,
            buttonmid: 0,
            buttonright: 0,
            backbutton_left: 0,
            backbutton_mid: 0,
            backbutton_right: 0,
            listbox_cursor: 0,
            listbox_hilight: 0,
            small_arrow: 0,
            stats_kills: A::STATS_KILLS,
            stats_deaths: A::STATS_DEATHS,
            stats_cpp: A::STATS_CPP,
            cursor: 0,
            mousecursor: 0,
        }
    }
}
