//! The shell's fonts and textures, and the map loading screen (planet, timeline
//! and rings, `screens::draw_loading`) drawn onto [`bfe_render::interface::Batch`].

use crate::{
    gate,
    screens::{ScreenId, ShellApp},
};
use bfe_render::interface::{
    anchor, mul, rotate_y, rotate_z, translate, Batch, Color, Font, Matrix, IDENTITY,
};
use std::collections::HashMap;
mod screens;
pub use self::screens::draw_loading;

/// White RGBA (`[1.0, 1.0, 1.0, 1.0]`).
/// Mirrors `WHITE` in `src/menus/themes/classic/draw.js`.
pub const WHITE: Color = [1.0, 1.0, 1.0, 1.0];

/// Black RGBA (`[0.0, 0.0, 0.0, 1.0]`).
/// Mirrors `BLACK` in `src/menus/themes/classic/draw.js`.
pub const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

/// Disabled grey RGBA (`[128/255, 128/255, 128/255, 1.0]`).
/// Mirrors `GREY` in `src/menus/themes/classic/draw.js`.
pub const GREY: Color = [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0];

/// Dimming factor (`150 / 255`) for an unfocused menu row bar.
/// Mirrors `DIMMED` in `src/menus/themes/classic/draw.js`.
pub const DIMMED: f64 = 150.0 / 255.0;

/// Tracking crosshair line fallback colour (`rgba(40, 190, 255, 0.95)`).
/// Mirrors `LINE` in `src/shell/loading.js`.
pub const TRACK_LINE_COLOR: Color = [40.0 / 255.0, 190.0 / 255.0, 1.0, 0.95];

/// Horizontal offset of a menu row's bar relative to the row origin (`-4.7` units).
/// Calibrated on the retail pause menu (`BAR_X` in `src/menus/themes/classic/index.js`).
const BAR_X: f64 = -4.7;

/// Bitmap font set used by the shell (`gamefont_large`, `gamefont_medium`,
/// `gamefont_small`, `gamefont_tiny`).
#[derive(Default)]
pub struct ShellFonts {
    /// `gamefont_large` (screen titles and `SINGLEPLAYER` rows).
    pub large: Font,
    /// `gamefont_medium` (`MAIN MENU` rows, popups, buttons, and `LOADING` text).
    pub medium: Font,
    /// `gamefont_small` (column headings and main-menu version line).
    pub small: Font,
    /// `gamefont_tiny` (Instant Action list rows and loading screen planet name).
    pub tiny: Font,
}

impl ShellFonts {
    /// Returns the font matching `name`, falling back to `medium`.
    /// Looks up ``gamefont_large``, ``gamefont_medium``, ``gamefont_small``,
    /// or ``gamefont_tiny`` by name, defaulting to `self.medium`.
    pub fn get(&self, name: &str) -> &Font {
        match name {
            "gamefont_large" => &self.large,
            "gamefont_medium" => &self.medium,
            "gamefont_small" => &self.small,
            "gamefont_tiny" => &self.tiny,
            _ => &self.medium,
        }
    }
}

/// One stage image uploaded to a GL texture for the loading screen, with its
/// pixel dimensions and optional zoom target `[x, y, w, h]` in pixels.
#[derive(Clone, Copy, Debug, Default)]
pub struct StageTexture {
    /// GL texture handle (`0` when not loaded).
    pub texture: u32,
    /// Image width in pixels (`iw` in `zoomView`).
    pub width: f64,
    /// Image height in pixels (`ih` in `zoomView`).
    pub height: f64,
    /// Optional pixel rectangle `[x, y, w, h]` into which the next stage zooms.
    pub target: Option<[f64; 4]>,
}

/// Texture handle table for shell UI pieces, screen backdrops, map preview
/// stills, and loading screen art.
#[derive(Clone, Debug)]
pub struct ShellTextures {
    /// Composited native movie texture; shared with page-theme backdrops.
    pub movie: u32,
    /// Delayed, composited Instant Action flyover texture.
    pub movie_preview: u32,
    /// 1x1 solid white texture handle (always non-zero when uploaded; `1` in
    /// headless tests).
    pub solid: u32,
    /// Named texture handles from `common.json` and `shell.json`.
    pub by_name: HashMap<String, u32>,
    /// Loading screen stage pictures (`0..4`) for the active map.
    pub loading_stages: Vec<StageTexture>,
}

impl Default for ShellTextures {
    /// Creates a default texture table with `solid = 1` for headless tests.
    fn default() -> Self {
        Self {
            movie: 0,
            movie_preview: 0,
            solid: 1,
            by_name: HashMap::new(),
            loading_stages: Vec::new(),
        }
    }
}

impl ShellTextures {
    /// Looks up a named texture handle (`0` if not loaded).
    pub fn get(&self, name: &str) -> u32 {
        self.by_name.get(name).copied().unwrap_or(0)
    }

    /// Returns the live composited movie, or Instant Action's static art.
    pub fn backdrop_for(&self, screen: ScreenId) -> u32 {
        match screen {
            ScreenId::InstantAction | ScreenId::InstantActionEra => self.get("his_brief_bg"),
            _ => self.movie,
        }
    }

    /// Returns the delayed live flyover texture for the selected map.
    pub fn map_preview_for(&self, _map_id: &str) -> u32 {
        self.movie_preview
    }
}

/// Computes the horizontal shift that centers an angled stack of `count` rows.
pub fn rows_shift(width: f64, pitch: f64, count: usize) -> f64 {
    let mut w = width + if count % 2 == 1 { pitch } else { 0.0 };
    let mut widest = 0.0_f64;
    let half = count / 2;
    for i in 1..=count {
        if i < half {
            w += pitch;
        } else if i > half {
            w -= pitch;
        }
        widest = widest.max(w);
    }
    -widest * 0.5
}

/// Computes `(bar_width, flipped)` for row `k` (`0..n`) in a stack of `n`
/// visible rows: the bar width grows by `pitch` per row above `n / 2` and
/// shrinks by `pitch` per row below `n / 2`, flipping the right triangle cap
/// vertically once `i > n / 2` (`ShowHideVerticalButtons`).
///
/// Mirrors `rowBars(width, pitch, n)` in `src/menus/themes/classic/index.js`.
pub fn row_bar_at(width: f64, pitch: f64, n: usize, k: usize) -> (f64, bool) {
    let half = n / 2;
    let mut w = width;
    for i in 1..=n {
        let flipped = i > half;
        if i - 1 == k {
            return (w, flipped);
        }
        if i < half {
            w += pitch;
        } else if i > half {
            w -= pitch;
        }
    }
    (width, false)
}

/// Focused text colour without a bar: yellow with green channel following `a`.
///
/// Mirrors `selectedColor(a)` in `src/menus/themes/classic/draw.js`:
/// `(255, clamp(1.2 * a * 255 + 50, 100, 255), 100, a)`.
pub fn selected_color(a: f64) -> Color {
    let g = (1.2 * a * 255.0 + 50.0).clamp(100.0, 255.0);
    [1.0, g / 255.0, 100.0 / 255.0, a]
}

/// Pulse factor (`0.3..1.0`) for focused buttons and bars, using the same
/// cosine pulse curve as the click gate (`gate::pulse(elapsed)`).
pub fn menu_pulse(elapsed: f64) -> f64 {
    gate::pulse(elapsed)
}

/// Listbox selection frame pulse (`1.0 -> 0.5 -> 1.0`, one cycle per second).
/// Mirrors `cursorPulse(time)` in `src/menus/themes/classic/index.js`.
pub fn cursor_pulse(time: f64) -> f64 {
    let u = (time.rem_euclid(1.0)) * 2.0;
    if u < 1.0 {
        1.0 - 0.5 * u
    } else {
        0.5 + 0.5 * (u - 1.0)
    }
}

/// Multiplies `a` by an X-axis rotation of `angle` radians.
pub fn rotate_x(a: Matrix, angle: f64) -> Matrix {
    let (s, c) = (angle.sin(), angle.cos());
    mul(
        a,
        [1., 0., 0., 0., 0., c, s, 0., 0., -s, c, 0., 0., 0., 0., 1.],
    )
}

/// Builds a 4x4 Euler rotation matrix in degrees.
/// Uses `"XYZ"` order when `xyz_order` is `true` (`MAIN MENU` in
/// `src/shell/screens.js`), or the default `"YXZ"` order from
/// `rotation(layout)` in `src/menus/themes/classic/index.js`.
fn rotation_euler(rot_x_deg: f64, rot_y_deg: f64, rot_z_deg: f64, xyz_order: bool) -> Matrix {
    let rx = rot_x_deg.to_radians();
    let ry = rot_y_deg.to_radians();
    let rz = rot_z_deg.to_radians();
    // `MAIN MENU` specifies `order: "XYZ"`; all other row screens use the
    // Classic theme's default `"YXZ"` Euler rotation order.
    if xyz_order {
        rotate_z(rotate_y(rotate_x(IDENTITY, rx), ry), rz)
    } else {
        // Default "YXZ" Euler order in `src/menus/themes/classic/index.js`.
        rotate_z(rotate_x(rotate_y(IDENTITY, ry), rx), rz)
    }
}

/// Multiplies `c`'s alpha channel by `a`.
/// Mirrors `tint(color, a)` in `src/menus/themes/classic/draw.js`.
pub(super) fn tint_color(c: Color, a: f64) -> Color {
    [c[0], c[1], c[2], c[3] * a]
}




/// Encodes `s` into a fixed 160-code-unit stack UTF-16 buffer without heap
/// allocation so [`Font::draw`] and [`Font::width`] can run at 0 allocations/frame.
fn encode_utf16_stack<'a>(s: &str, buf: &'a mut [u16; 160]) -> &'a [u16] {
    let mut len = 0;
    for ch in s.encode_utf16() {
        if len >= buf.len() {
            break;
        }
        buf[len] = ch;
        len += 1;
    }
    &buf[..len]
}

/// Draws `text` with `font` at `(x, y)` under transform `m` using a stack
/// UTF-16 buffer (zero heap allocations).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_str(
    font: &Font,
    batch: &mut Batch,
    m: Matrix,
    x: f64,
    y: f64,
    text: &str,
    tint: Color,
    align: u32,
) {
    let mut buf = [0u16; 160];
    let slice = encode_utf16_stack(text, &mut buf);
    font.draw(batch, m, x, y, slice, tint, align, 1.0, 0.0, 0.0);
}




/// Layout parameters for [`draw_rows`], mirroring the `Rows` widget layout in
/// `src/menus/themes/classic/index.js` and `src/shell/screens.js`.
#[derive(Clone, Copy)]
struct RowsLayout<'a> {
    /// Safe-area anchor `[ax, ay]` (default `[0.5, 0.5]`).
    at: [f64; 2],
    /// Horizontal offset in interface units added to `rows_shift`.
    x: f64,
    /// Vertical offset in interface units from the anchor (default `20.0`).
    y: f64,
    /// Base bar width in interface units (default `350.0`).
    width: f64,
    /// X-axis tilt in degrees (default `0.0`; `-20.0` on `MAIN MENU`).
    rot_x: f64,
    /// Y-axis turn in degrees (default `35.0`; `-30.0` on `MAIN MENU`).
    rot_y: f64,
    /// Z-axis roll in degrees (default `0.0`; `1.0` on `MAIN MENU`).
    rot_z: f64,
    /// Whether to apply Euler angles in `XYZ` order (`true` on `MAIN MENU`)
    /// instead of the Classic theme's default `YXZ` order.
    xyz_order: bool,
    /// Bitmap font name (`"gamefont_medium"` or `"gamefont_large"`).
    font: &'a str,
    /// Optional fixed row count (`Some(7)` on `MAIN MENU`) passed to [`rows_shift`].
    list_count: Option<usize>,
}

impl<'a> Default for RowsLayout<'a> {
    /// Returns the default `rowScreen` layout from `src/shell/screens.js`:
    /// `at: [0.5, 0.5]`, `y: 20`, `width: 350`, `rotY: 35`, `"gamefont_medium"`.
    fn default() -> Self {
        Self {
            at: [0.5, 0.5],
            x: 0.0,
            y: 20.0,
            width: 350.0,
            rot_x: 0.0,
            rot_y: 35.0,
            rot_z: 0.0,
            xyz_order: false,
            font: "gamefont_medium",
            list_count: None,
        }
    }
}

/// Shared row geometry for rendering and pointer picking on shell row screens.
fn player_menu_layout(screen: ScreenId) -> RowsLayout<'static> {
    match screen {
        ScreenId::Main => RowsLayout {
            at: [0.5, 0.65],
            x: 0.0,
            y: 30.0,
            width: 250.0,
            rot_x: -20.0,
            rot_y: -30.0,
            rot_z: 1.0,
            xyz_order: true,
            font: "gamefont_medium",
            list_count: Some(7),
        },
        ScreenId::MultiplayerList(_) | ScreenId::OptionsPage(_) | ScreenId::HostDialog => {
            RowsLayout {
                width: 620.0,
                ..RowsLayout::default()
            }
        }
        _ => RowsLayout::default(),
    }
}

/// Hit the rendered row plane rather than an unrelated fixed 768-high grid.
pub fn hit_player_menu(
    app: &ShellApp,
    fonts: &ShellFonts,
    w: f64,
    h: f64,
    x: f64,
    y: f64,
) -> Option<usize> {
    let screen = app.current_screen();
    let layout = player_menu_layout(screen);
    let n = app.active_rows().len();
    let p = fonts.get(layout.font).height + 5.0;
    let (_, _, height) = gate::gate_layout(w, h);
    let m = mul(
        translate(
            anchor(layout.at[0], layout.at[1], true, height),
            rows_shift(layout.width, p, layout.list_count.unwrap_or(n)) + layout.x,
            layout.y,
            0.0,
        ),
        rotation_euler(layout.rot_x, layout.rot_y, layout.rot_z, layout.xyz_order),
    );
    let [lx, ly] = bfe_interface::space::local_point(&m, 2.0 * x / w - 1.0, 1.0 - 2.0 * y / h)?;
    for k in 0..n {
        let cy = -(n as f64 * p) * 0.5 + k as f64 * p;
        let (bar_w, _) = row_bar_at(layout.width, p, n, k);
        if lx >= BAR_X && lx <= BAR_X + bar_w + 5.0 && ly >= cy - p * 0.5 && ly <= cy + p * 0.5 {
            return Some(k);
        }
    }
    None
}





#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ShellData;
    use crate::loading::LoadingTimeline;
    use crate::loading::LookId;

    #[test]
    fn pointer_hits_rendered_menu_rows_in_wide_and_standard_windows() {
        let mut app = ShellApp::new(ShellData::default(), None);
        let fonts = ShellFonts::default();
        for screen in [
            ScreenId::Main,
            ScreenId::Options,
            ScreenId::Multiplayer,
            ScreenId::HostDialog,
        ] {
            app.stack = vec![screen];
            app.rebuild_cached_rows();
            for (w, h) in [
                (1280.0, 720.0),
                (1024.0, 768.0),
                (1280.0, 800.0),
                (2560.0, 1600.0),
            ] {
                let (_, _, height) = gate::gate_layout(w, h);
                let layout = player_menu_layout(screen);
                let p = fonts.get(layout.font).height + 5.0;
                let n = app.active_rows().len();
                let m = mul(
                    translate(
                        anchor(layout.at[0], layout.at[1], true, height),
                        rows_shift(layout.width, p, layout.list_count.unwrap_or(n)) + layout.x,
                        layout.y,
                        0.0,
                    ),
                    rotation_euler(layout.rot_x, layout.rot_y, layout.rot_z, layout.xyz_order),
                );
                for k in 0..n {
                    let cy = -(n as f64 * p) * 0.5 + k as f64 * p;
                    let [x, y] = bfe_interface::space::plane_point(&m, 50.0, cy, 0.0, height);
                    assert_eq!(
                        hit_player_menu(&app, &fonts, w, h, x * w / 1024.0, y * h / height),
                        Some(k)
                    );
                }
            }
        }
    }

    #[test]
    fn rows_shift_and_row_bar_at_match_classic_theme() {
        // 7 rows on MAIN MENU with width 250, pitch 29:
        let s = rows_shift(250.0, 29.0, 7);
        assert!((s - (-168.5)).abs() < 1e-6);

        // Verify top row (`k = 0`), widest row before midpoint (`k = 2`),
        // and flipped bottom row (`k = 6`).
        let (w0, f0) = row_bar_at(250.0, 29.0, 7, 0);
        let (w2, f2) = row_bar_at(250.0, 29.0, 7, 2);
        let (w6, f6) = row_bar_at(250.0, 29.0, 7, 6);
        assert_eq!((w0, f0), (250.0, false));
        assert_eq!((w2, f2), (308.0, false));
        assert_eq!((w6, f6), (221.0, true));
    }

    #[test]
    fn draw_loading_emits_quads_in_track_and_zoom_phases() {
        let mut batch = Batch::new();
        let mut fonts = ShellFonts::default();
        // Populate basic ASCII glyphs so text drawing emits quads.
        for ch in 32u16..=95u16 {
            let g = bfe_render::interface::Glyph {
                page: 2,
                advance: 10.0,
                bounds: [0.0, 0.0, 9.0, 18.0],
                uv: [0.0, 0.0, 0.1, 0.1],
            };
            fonts.large.glyphs.insert(ch, g);
            fonts.medium.glyphs.insert(ch, g);
            fonts.small.glyphs.insert(ch, g);
            fonts.tiny.glyphs.insert(ch, g);
        }
        // Configure two loading stage textures (stage 0 with a zoom target box,
        // stage 1 full-frame) so both `Track` and `Zoom` phases emit quads.
        let mut textures = ShellTextures::default();
        textures.loading_stages.push(StageTexture {
            texture: 3,
            width: 512.0,
            height: 512.0,
            target: Some([128.0, 128.0, 256.0, 256.0]),
        });
        textures.loading_stages.push(StageTexture {
            texture: 4,
            width: 512.0,
            height: 512.0,
            target: None,
        });

        // Loading screen in track and zoom phases
        let tl = LoadingTimeline::new(3.5, 1.0, 0.0, vec![Some([0.25, 0.25, 0.75, 0.75]), None]);
        let state_track = tl.at(1.0);
        draw_loading(
            &mut batch,
            &fonts,
            &textures,
            &state_track,
            1.0,
            "NABOO",
            "LOADING",
            LookId::Bf1,
            1280.0,
            720.0,
        );
        assert!(batch.count > 35);

        // At `t = 3.8s` (`3.5s` track + `0.3s` into zoom), `draw_loading`
        // renders the zoomed stage 0 crop, stage 1 preview, and disk in `Eternal` look.
        let state_zoom = tl.at(3.8);
        draw_loading(
            &mut batch,
            &fonts,
            &textures,
            &state_zoom,
            3.8,
            "NABOO",
            "LOADING",
            LookId::Eternal,
            1280.0,
            720.0,
        );
        assert!(batch.count > 25);
    }
}
