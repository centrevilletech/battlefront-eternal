//! Zero-allocation per-frame rendering of the click gate, shell menus, and
//! loading screen onto [`bfe_render::interface::Batch`].
//!
//! In the Classic theme (`src/menus/themes/classic/index.js` and
//! `src/menus/themes/classic/draw.js`), all interface geometry is laid out in a
//! virtual `1024 x height` coordinate space (`height = 768 + (h / w - 0.75) * 768`)
//! with a 90% safe area (`anchor(ax, ay, safe, height)`).
//! - Menu row stacks (`draw_rows`) sit under an orange 3-piece header bar
//!   (`headerbuttonleft`, `headerbuttonmid`, `headerbuttonright`) and draw one
//!   3-piece blue button bar (`buttonleft`, `buttonmid`, `buttonright`) per row,
//!   shifted left by `rows_shift` and rotated in 3D (`rotation_euler`).
//! - Windows and listboxes (`draw_frame`) use 9-slice `border_3_pieces` frames
//!   with 16-unit corners (`nineSlice`).
//! - When optional UI textures have not yet been extracted, `fallback_color`
//!   and `piece_quad` render flat stand-in quads (and 6-sliver right-end
//!   triangles) matching `FALLBACK` and `triangle` in `draw.js`.
//!
//! Mirrors the geometry and colours of:
//! - `src/shell/gate.js` (`showGate`: `IW = 1024`, `AT = 0.82`, `pulse(t)`),
//! - `src/menus/themes/classic/index.js` and `src/menus/themes/classic/draw.js`
//!   (`paintRows`, `paintList`, `paintButton`, `paintText`, `paintPanel`,
//!   `paintHorizontal`, `rowsShift`, `rowBars`, `selectedColor`, `nineSlice`),
//! - `src/shell/screens.js` (`createScreens`: `main`, `sp`, `ia`, `options`,
//!   `profile`, `mp`),
//! - `src/shell/loading.js` (`LoadingView.prototype.draw`, `drawPictures`,
//!   `drawDisk`, `zoomView`, `onScreen`, `diskPlace`, `lightLevel`, `mixColor`).

use crate::{
    gate::{self, GATE_TEXT},
    // `LookId` and `TimelineAt` are consumed by `render::screens::draw_loading`.
    screens::{MenuRow, PopupState, ScreenId, ShellApp},
};
use bfe_render::interface::{
    anchor, mul, rotate_y, rotate_z, translate, Batch, Color, Font, Matrix, IDENTITY, UV,
};
use std::collections::HashMap;
mod screens;
pub use self::screens::draw_loading;
use self::screens::draw_instant_action;

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
/// Horizontal offset of a menu row's text relative to the row origin (`0.8` units).
/// Mirrors `TEXT_X` in `src/menus/themes/classic/index.js`.
const TEXT_X: f64 = 0.8;
/// Inset in interface units from the safe-area left/right edge to a bottom button
/// (`BUTTON_INSET = 21` in `src/menus/themes/classic/index.js`).
const BUTTON_INSET: f64 = 21.0;

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

    /// Resolves the best available poster still texture for the current
    /// [`ScreenId`] (`main`, `atat_screen`, `atte_screen`, `hoth_screen`,
    /// `coreship_screen`, `generic2_screen`, `generic3_screen`).
    pub fn backdrop_for(&self, screen: ScreenId) -> u32 {
        // Candidate poster texture names in priority order for each screen,
        // matching `SCREEN_MEDIA` (`src/shell/media.js`) and `instantAction`
        // (`his_brief_bg` in `src/shell/screens.js`).
        let candidates: &[&str] = match screen {
            ScreenId::Gate => &["atat_screen", "main", "iface_bgmeta_space"],
            ScreenId::Main | ScreenId::Popup => &["atat_screen", "main", "iface_bgmeta_space"],
            ScreenId::Singleplayer => &["atte_screen", "hoth_screen", "main"],
            ScreenId::InstantAction | ScreenId::InstantActionEra => {
                &["his_brief_bg", "generic2_screen", "main"]
            }
            ScreenId::Options | ScreenId::OptionsPage(_) => {
                &["coreship_screen", "generic3_screen", "main"]
            }
            ScreenId::Profile => &["generic2_screen", "coreship_screen", "main"],
            ScreenId::Multiplayer
            | ScreenId::MultiplayerList(_)
            | ScreenId::DirectConnect
            | ScreenId::HostDialog => &["hoth_screen", "generic3_screen", "main"],
        };
        // Return the first candidate texture that has a non-zero GL handle.
        for &name in candidates {
            let tex = self.get(name);
            if tex != 0 {
                return tex;
            }
        }
        0
    }

    /// Resolves the map preview still texture (`map_<id>` or `map_unk`).
    /// Looks up the per-map preview texture (`map_<id>` in lowercase ASCII)
    /// without heap allocation, falling back to `map_unk` or `his_brief_bg`.
    pub fn map_preview_for(&self, map_id: &str) -> u32 {
        // Format `"map_<lowercase_id>"` on the stack without heap allocation.
        let mut buf = [0u8; 32];
        let prefix = b"map_";
        let id_bytes = map_id.as_bytes();
        if prefix.len() + id_bytes.len() <= buf.len() {
            buf[..prefix.len()].copy_from_slice(prefix);
            for (i, &b) in id_bytes.iter().enumerate() {
                buf[prefix.len() + i] = b.to_ascii_lowercase();
            }
            if let Ok(key) = std::str::from_utf8(&buf[..prefix.len() + id_bytes.len()]) {
                let tex = self.get(key);
                if tex != 0 {
                    return tex;
                }
            }
        }
        // Fall back to `map_unk` (or `his_brief_bg` if `map_unk` is absent)
        // when the per-map preview still texture is not loaded.
        let unk = self.get("map_unk");
        if unk != 0 {
            unk
        } else {
            self.get("his_brief_bg")
        }
    }
}

/// Computes the horizontal shift that centers an angled stack of `count` rows.
///
/// Mirrors `rowsShift(width, pitch, count)` in `src/menus/themes/classic/index.js`:
/// ```js
/// let w = width + (count % 2 === 1 ? pitch : 0);
/// let widest = 0;
/// const half = Math.floor(count / 2);
/// for (let i = 1; i <= count; i++) {
///   if (i < half) w += pitch;
///   else if (i > half) w -= pitch;
///   widest = Math.max(widest, w);
/// }
/// return -widest / 2;
/// ```
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

/// Scales `c`'s RGB channels by `k` and its alpha channel by `a`.
/// Mirrors `tint(scaled(color, k), a)` in `src/menus/themes/classic/draw.js`.
fn scaled_color(c: Color, k: f64, a: f64) -> Color {
    [c[0] * k, c[1] * k, c[2] * k, c[3] * a]
}

/// Component-wise RGBA multiplication of two colours.
fn mul_color(a: Color, b: Color) -> Color {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

/// Flat RGBA stand-in colour when a UI piece texture has not been extracted.
/// Mirrors `FALLBACK` in `src/menus/themes/classic/draw.js`.
fn fallback_color(name: &str) -> Color {
    // Steel-blue button bars (`button*`), orange header bars (`headerbutton*`),
    // translucent navy panel frame (`border_3_pieces`), orange selection cursor
    // (`listbox_cursor`), slider track/thumb (`slider_bg`/`slider_fg`), and
    // era faction icon stand-ins (`rep_icon`, `cis_icon`, `imp_icon`, `all_icon`).
    match name {
        "buttonleft" => [57.0 / 255.0, 125.0 / 255.0, 180.0 / 255.0, 1.0],
        "buttonmid" => [40.0 / 255.0, 95.0 / 255.0, 150.0 / 255.0, 1.0],
        "buttonright" => [24.0 / 255.0, 72.0 / 255.0, 131.0 / 255.0, 1.0],
        "headerbuttonleft" => [1.0, 165.0 / 255.0, 24.0 / 255.0, 1.0],
        "headerbuttonmid" => [1.0, 133.0 / 255.0, 24.0 / 255.0, 1.0],
        "headerbuttonright" => [1.0, 101.0 / 255.0, 24.0 / 255.0, 1.0],
        "border_3_pieces" => [0.0, 44.0 / 255.0, 98.0 / 255.0, 180.0 / 255.0],
        "listbox_cursor" => [1.0, 165.0 / 255.0, 24.0 / 255.0, 1.0],
        "listbox_hilight" => [57.0 / 255.0, 125.0 / 255.0, 180.0 / 255.0, 1.0],
        "slider_bg" => [8.0 / 255.0, 40.0 / 255.0, 65.0 / 255.0, 1.0],
        "slider_fg" => [1.0, 101.0 / 255.0, 24.0 / 255.0, 1.0],
        "rep_icon" => [180.0 / 255.0, 45.0 / 255.0, 45.0 / 255.0, 1.0],
        "cis_icon" => [70.0 / 255.0, 130.0 / 255.0, 200.0 / 255.0, 1.0],
        "imp_icon" => [170.0 / 255.0, 180.0 / 255.0, 195.0 / 255.0, 1.0],
        "all_icon" => [220.0 / 255.0, 140.0 / 255.0, 40.0 / 255.0, 1.0],
        _ => WHITE,
    }
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

/// Measures the advance width of `text` in `font` using a stack UTF-16 buffer.
pub(super) fn str_width(font: &Font, text: &str) -> f64 {
    let mut buf = [0u16; 160];
    let slice = encode_utf16_stack(text, &mut buf);
    font.width(slice, 1.0, 0.0)
}

/// Draws a UI piece quad (`name`) or its untextured stand-in (`FALLBACK` in
/// `src/menus/themes/classic/draw.js`, including the 6-sliver diagonal
/// triangle fallback for `buttonright` and `headerbuttonright`).
#[allow(clippy::too_many_arguments)]
pub(super) fn piece_quad(
    batch: &mut Batch,
    textures: &ShellTextures,
    m: Matrix,
    name: &str,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    uv: [f64; 4],
    color: Color,
) {
    // When the named UI piece texture is loaded, emit a single textured quad;
    // otherwise emit the untextured fallback shape onto `textures.solid`.
    let tex = textures.get(name);
    if tex != 0 {
        batch.quad(m, [x0, y0, x1, y1], tex, uv, color, 0.0, 0);
    } else if name == "buttonright" || name == "headerbuttonright" {
        // Approximate the triangular right cap with 6 horizontal slivers
        // (`triangle` in `src/menus/themes/classic/draw.js`).
        let fb = mul_color(fallback_color(name), color);
        let steps = 6;
        let flipped = uv[1] > uv[3];
        // Step through the 6 vertical bands from `y0` to `y1`, widening or
        // narrowing each sliver according to whether the cap is vertically flipped.
        for i in 0..steps {
            let a = y0 + ((y1 - y0) * (i as f64)) / (steps as f64);
            let b = y0 + ((y1 - y0) * ((i + 1) as f64)) / (steps as f64);
            let k = if flipped {
                1.0 - ((i as f64) + 0.5) / (steps as f64)
            } else {
                ((i as f64) + 0.5) / (steps as f64)
            };
            batch.quad(
                m,
                [x0, a, x0 + (x1 - x0) * k, b],
                textures.solid,
                UV,
                fb,
                0.0,
                0,
            );
        }
    } else {
        // Solid rectangular stand-in tinted by `fallback_color(name) * color`.
        let fb = mul_color(fallback_color(name), color);
        batch.quad(m, [x0, y0, x1, y1], textures.solid, UV, fb, 0.0, 0);
    }
}

/// Draws a 3-piece horizontal bar (`[left, mid, right]`).
/// Mirrors `bar` in `src/menus/themes/classic/draw.js`.
#[allow(clippy::too_many_arguments)]
fn draw_bar(
    batch: &mut Batch,
    textures: &ShellTextures,
    m: Matrix,
    names: [&str; 3],
    x0: f64,
    y0: f64,
    width: f64,
    y1: f64,
    flipped: bool,
    color: Color,
) {
    // Left cap is `[x0, x0 + h]`, stretched middle is `[x0 + h, x0 + max(width, h)]`,
    // and right cap is `[mid_end, mid_end + h]` (vertically flipped when `flipped`).
    let h = y1 - y0;
    let mid_end = x0 + width.max(h);
    let end = mid_end + h;
    let pieces = [
        (x0, x0 + h, names[0], false),
        (x0 + h, mid_end, names[1], false),
        (mid_end, end, names[2], true),
    ];
    // Emit each non-empty piece (`left`, `mid`, `right`), flipping the right
    // cap's V coordinates (`[0, 1, 1, 0]`) for rows below the stack midpoint.
    for (a, b, name, is_right) in pieces {
        if b <= a {
            continue;
        }
        let uv = if is_right && flipped {
            [0.0, 1.0, 1.0, 0.0]
        } else {
            UV
        };
        piece_quad(batch, textures, m, name, a, y0, b, y1, uv, color);
    }
}

/// Draws a 9-slice window frame centered on `(x, y)`.
/// Mirrors `frame` in `src/menus/themes/classic/draw.js` and `nineSlice` in
/// `src/interface/shapes.js`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_frame(
    batch: &mut Batch,
    textures: &ShellTextures,
    m: Matrix,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    name: &str,
    color: Color,
) {
    // When `border_3_pieces` (or `listbox_cursor`) is not yet loaded, draw a
    // single inset solid quad with `fallback_color(name)` (`frame` in `draw.js`).
    let tex = textures.get(name);
    if tex == 0 {
        let fb = fallback_color(name);
        batch.quad(
            m,
            [
                x - w * 0.5 + 4.0,
                y - h * 0.5 + 4.0,
                x + w * 0.5 - 4.0,
                y + h * 0.5 - 4.0,
            ],
            textures.solid,
            UV,
            [fb[0], fb[1], fb[2], fb[3] * color[3]],
            0.0,
            0,
        );
        return;
    }
    // 9-slice grid with 16-unit corners and UV splits `[0.0, 0.25, 0.75, 1.0]`
    // (`nineSlice` in `src/interface/shapes.js`).
    let corner = 16.0;
    let mut xs = [-w * 0.5, -w * 0.5 + corner, w * 0.5 - corner, w * 0.5];
    let mut ys = [-h * 0.5, -h * 0.5 + corner, h * 0.5 - corner, h * 0.5];
    // Collapse the inner 9-slice splits to `0.0` if the box is narrower or
    // shorter than two 16-unit corners (`32.0` units).
    if xs[2] < xs[1] {
        xs[1] = 0.0;
        xs[2] = 0.0;
    }
    if ys[2] < ys[1] {
        ys[1] = 0.0;
        ys[2] = 0.0;
    }
    // Emit the 3x3 grid of 9-slice patches with UV splits `[0.0, 0.25, 0.75, 1.0]`.
    let us = [0.0, 0.25, 0.75, 1.0];
    for j in 0..3 {
        for i in 0..3 {
            // Skip degenerate patches when the frame is smaller than its corners.
            if xs[i + 1] <= xs[i] || ys[j + 1] <= ys[j] {
                continue;
            }
            batch.quad(
                m,
                [x + xs[i], y + ys[j], x + xs[i + 1], y + ys[j + 1]],
                tex,
                [us[i], us[j], us[i + 1], us[j + 1]],
                color,
                0.0,
                0,
            );
        }
    }
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

/// Shared row geometry for the main and singleplayer menu input paths.
fn player_menu_layout(screen: ScreenId) -> RowsLayout<'static> {
    match screen {
        ScreenId::Main => RowsLayout { at: [0.5, 0.65], x: 0.0, y: 30.0, width: 250.0,
            rot_x: -20.0, rot_y: -30.0, rot_z: 1.0, xyz_order: true,
            font: "gamefont_medium", list_count: Some(7) },
        ScreenId::Singleplayer => RowsLayout { width: 650.0, font: "gamefont_large", ..Default::default() },
        _ => RowsLayout::default(),
    }
}

/// Hit the rendered row plane rather than an unrelated fixed 768-high grid.
pub fn hit_player_menu(app: &ShellApp, fonts: &ShellFonts, w: f64, h: f64, x: f64, y: f64) -> Option<usize> {
    let screen = app.current_screen();
    if !matches!(screen, ScreenId::Main | ScreenId::Singleplayer) { return None; }
    let layout = player_menu_layout(screen);
    let n = app.active_rows().len();
    let p = fonts.get(layout.font).height + 5.0;
    let (_, _, height) = gate::gate_layout(w, h);
    let m = mul(translate(anchor(layout.at[0], layout.at[1], true, height),
        rows_shift(layout.width, p, layout.list_count.unwrap_or(n)) + layout.x, layout.y, 0.0),
        rotation_euler(layout.rot_x, layout.rot_y, layout.rot_z, layout.xyz_order));
    let [lx, ly] = bfe_interface::space::local_point(&m, 2.0*x/w-1.0, 1.0-2.0*y/h)?;
    for k in 0..n {
        let cy = -(n as f64*p)*0.5 + k as f64*p;
        let (bar_w, _) = row_bar_at(layout.width, p, n, k);
        if lx >= BAR_X && lx <= BAR_X+bar_w+5.0 && ly >= cy-p*0.5 && ly <= cy+p*0.5 { return Some(k); }
    }
    None
}

/// Draws an angled stack of menu rows under an orange header bar.
/// Mirrors `paintRows` in `src/menus/themes/classic/index.js`.
#[allow(clippy::too_many_arguments)]
fn draw_rows(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    height: f64,
    title: &str,
    rows: &[MenuRow],
    selected: usize,
    pulse: f64,
    layout: RowsLayout<'_>,
) {
    // Row pitch `p = yh + 5.0` and container horizontal shift `rows_shift`
    // (`paintRows` in `src/menus/themes/classic/index.js`).
    let font = fonts.get(layout.font);
    let yh = font.height;
    let y_spacing = 5.0;
    let p = yh + y_spacing;
    let n = rows.len();
    if n == 0 {
        return;
    }
    // Anchor at `layout.at` in the safe area, translate by `(shift + x, y)`,
    // and apply the 3D Euler rotation matrix (`paintRows`).
    let shift = rows_shift(layout.width, p, layout.list_count.unwrap_or(n));
    let base = anchor(layout.at[0], layout.at[1], true, height);
    let moved = translate(base, shift + layout.x, layout.y, 0.0);
    let m = mul(
        moved,
        rotation_euler(layout.rot_x, layout.rot_y, layout.rot_z, layout.xyz_order),
    );
    let y0 = -(n as f64 * p) * 0.5;

    // Orange title bar (`headerbuttonleft`/`mid`/`right`): positioned `1.75 * p`
    // above the first row, with black title text inset by `0.5 * p + TEXT_X`.
    if !title.is_empty() {
        let ty = y0 - 1.75 * p;
        let width = layout.width - 1.25 * (yh - y_spacing) - 0.65 * p;
        let by = ty - 0.25 * p;
        let gh = font.height;
        draw_bar(
            batch,
            textures,
            m,
            ["headerbuttonleft", "headerbuttonmid", "headerbuttonright"],
            BAR_X,
            by,
            width,
            ty + gh + 0.25 * p,
            false,
            WHITE,
        );
        // Draw the title string in `BLACK` centered vertically inside the
        // orange header bar (`(by + ty + gh + 0.25 * p) / 2 - capsMiddle`).
        draw_str(
            font,
            batch,
            m,
            p * 0.5 + TEXT_X,
            (by + ty + gh + 0.25 * p) * 0.5 - font.caps_middle(),
            title,
            BLACK,
            0,
        );
    }

    // Draw each visible row's 3-piece blue button bar (`buttonleft`/`mid`/`right`)
    // and label (`WHITE` when enabled, `GREY` when disabled; unfocused bars dimmed
    // by `DIMMED = 150 / 255`).
    for (k, row) in rows.iter().enumerate() {
        let cy = y0 + (k as f64) * p;
        // Only the selected enabled row pulses with `pulse`; all other rows
        // use alpha `1.0` and a bar dimmed by `DIMMED` (`150 / 255`).
        let focused = k == selected && !row.disabled;
        let a = if focused { pulse } else { 1.0 };
        let bar_color = if focused {
            tint_color(WHITE, a)
        } else {
            scaled_color(WHITE, DIMMED, a)
        };
        // Compute this row's bar width (`bar_w + 5.0`) and whether its right
        // triangular end piece is flipped (`k >= n / 2`).
        let (bar_w, flipped) = row_bar_at(layout.width, p, n, k);
        draw_bar(
            batch,
            textures,
            m,
            ["buttonleft", "buttonmid", "buttonright"],
            BAR_X,
            cy - p * 0.5,
            bar_w + 5.0,
            cy + p * 0.5,
            flipped,
            bar_color,
        );
        // Enabled row labels render in `WHITE`; disabled rows render in `GREY`.
        let text_color = if !row.disabled {
            tint_color(WHITE, a)
        } else {
            tint_color(GREY, a)
        };
        draw_str(
            font,
            batch,
            m,
            TEXT_X,
            cy - font.caps_middle(),
            &row.label,
            text_color,
            0,
        );
    }
}

/// Draws a bottom-edge button (`BACK` on left, `middle` in center, `accept` on right).
/// Mirrors `paintButton` in `src/menus/themes/classic/index.js`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_bottom_button(
    batch: &mut Batch,
    fonts: &ShellFonts,
    height: f64,
    place: &str,
    label: &str,
    focused: bool,
    enabled: bool,
    pulse: f64,
) {
    // Resolve safe-area bottom anchor `(ax, ay)` and text alignment (`0` left,
    // `1` right, `2` center) from `BUTTON_PLACES` (`src/menus/themes/classic/index.js`).
    let (ax, ay, align) = match place {
        "accept" => (1.0, 1.0, 1),
        "middle" => (0.5, 1.0, 2),
        _ => (0.0, 1.0, 0),
    };
    let m = anchor(ax, ay, true, height);
    // Offset left/right buttons by `BUTTON_INSET = 21` units from the safe edge
    // and place the capital-letter midline 12 units above the safe bottom.
    let x = if align == 0 {
        BUTTON_INSET
    } else if align == 1 {
        -BUTTON_INSET
    } else {
        0.0
    };
    let y = -12.0 - fonts.medium.caps_middle();
    // Focused bottom buttons pulse yellow (`selected_color(pulse)`); disabled
    // buttons (`LAUNCH` with an empty playlist) render in `GREY`.
    let color = if !enabled {
        GREY
    } else if focused {
        selected_color(pulse)
    } else {
        WHITE
    };
    draw_str(&fonts.medium, batch, m, x, y, label, color, align);
}

/// Draws a vertical list scrollbar (`slider_bg` track at `0.5` alpha and
/// `slider_fg` thumb at `0.33` alpha, minimum 6 units tall).
/// Mirrors `scrollBar` in `src/menus/themes/classic/index.js`.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_scroll_bar(
    batch: &mut Batch,
    textures: &ShellTextures,
    m: Matrix,
    x: f64,
    top: f64,
    height: f64,
    first: usize,
    shown: usize,
    count: usize,
) {
    // Full-height scrollbar track (`slider_bg` at `TRACK_ALPHA = 0.5`).
    piece_quad(
        batch,
        textures,
        m,
        "slider_bg",
        x,
        top,
        x + 16.0,
        top + height,
        UV,
        tint_color(WHITE, 0.5),
    );
    // Thumb span `[t0, t1]` proportional to `first .. first + shown` out of
    // `count`, clamped to at least 6 interface units high.
    let t0 = top + (first as f64 / count.max(1) as f64) * height;
    let t1 = (top + ((first + shown) as f64 / count.max(1) as f64) * height)
        .max(t0 + 6.0)
        .min(top + height);
    // Visible-window scrollbar thumb (`slider_fg` at `THUMB_ALPHA = 0.33`).
    piece_quad(
        batch,
        textures,
        m,
        "slider_fg",
        x,
        t0,
        x + 16.0,
        t1,
        UV,
        tint_color(WHITE, 0.33),
    );
}

/// Draws a modal popup (`Notice` `OK`, `ConfirmExit` `YES`/`NO`, or `HostClosePrompt`)
/// or text input modal (`Profile` callsign or `DirectConnect` address) without heap allocation.
#[allow(clippy::too_many_arguments)]
fn draw_popup_box(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    height: f64,
    message: &str,
    extra_line: Option<&str>,
    buttons: &[(&str, bool)],
    pulse: f64,
) {
    // Centered `360 x 190` 9-slice popup window frame (`border_3_pieces`),
    // widened to `520 x 210` when 3 buttons (`HostClosePrompt`) are present.
    let m = anchor(0.5, 0.5, false, height);
    let (box_w, box_h) = if buttons.len() >= 3 {
        (520.0, 210.0)
    } else {
        (360.0, 190.0)
    };
    draw_frame(
        batch,
        textures,
        m,
        0.0,
        0.0,
        box_w,
        box_h,
        "border_3_pieces",
        WHITE,
    );

    // Draw centered prompt lines (`gamefont_medium`) inside the popup frame,
    // followed by the optional draft input line (`extra_line`).
    let mut y = -68.0;
    for line in message.lines() {
        draw_str(&fonts.medium, batch, m, 0.0, y, line, WHITE, 2);
        y += fonts.medium.height;
    }
    // When an input draft (`profile.draft` or `direct_connect_draft`) is active,
    // draw it centered on the line immediately below the prompt text.
    if let Some(extra) = extra_line {
        draw_str(&fonts.medium, batch, m, 0.0, y, extra, WHITE, 2);
    }

    // Horizontal popup buttons (`paintHorizontal` in `src/menus/themes/classic/index.js`):
    // centered at `x = 0` when `count == 1`, or spaced 85 units apart (`-40` and `45`)
    // at `y = 63` below center; for 3 buttons (`HostClosePrompt`), stack vertically
    // or space across the wider dialog.
    let count = buttons.len();
    if count >= 3 {
        let mut by = 10.0;
        for &(label, focused) in buttons {
            let color = if focused {
                selected_color(pulse)
            } else {
                WHITE
            };
            draw_str(&fonts.medium, batch, m, 0.0, by, label, color, 2);
            by += fonts.medium.height + 4.0;
        }
    } else {
        let by = 63.0 - fonts.medium.caps_middle();
        for (i, &(label, focused)) in buttons.iter().enumerate() {
            // Place a single button at `x = 0`, or pair buttons at `-40` and `45`.
            let x = if count == 1 {
                0.0
            } else {
                -40.0 + (i as f64) * 85.0 - ((count as f64) - 2.0) * 42.5
            };
            let color = if focused {
                selected_color(pulse)
            } else {
                WHITE
            };
            draw_str(&fonts.medium, batch, m, x, by, label, color, 2);
        }
    }
}

/// Populates `batch` with the current shell screen's geometry for a viewport of
/// `(viewport_w, viewport_h)` pixels:
/// 1. Full-screen poster backdrop (`textures.backdrop_for(base_screen)`) or
///    dark space-blue fallback quad.
/// 2. Initial click/key gate (`ScreenId::Gate`: pulsing `GATE_TEXT` at
///    `AT = 0.82` of interface height).
/// 3. Active base screen (`Main`, `Singleplayer`, `InstantAction`,
///    `InstantActionEra`, `Options`, `OptionsPage`, `Profile`, `Multiplayer`,
///    `MultiplayerList`, `DirectConnect`, or `HostDialog`).
/// 4. Modal overlay (`profile.editing` callsign editor or `PopupState` notice /
///    `EXIT TO WINDOWS?` confirmation / `HostClosePrompt`) over a dimmed backdrop.
///
/// Performs zero heap allocations per frame.
pub fn draw_shell(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    app: &ShellApp,
    viewport_w: f64,
    viewport_h: f64,
) {
    // Reset the vertex/index batch for the new frame and compute the virtual
    // interface height and gate text vertical position (`gate_layout`).
    batch.begin();
    let (_, gate_y, height) = gate::gate_layout(viewport_w, viewport_h);
    let active = app.current_screen();
    let base_screen = app.stack.last().copied().unwrap_or(ScreenId::Main);

    // 1. Full-screen poster backdrop (`1024 x height`).
    let bg_tex = textures.backdrop_for(base_screen);
    let full_m = anchor(0.0, 0.0, false, height);
    // Draw the screen's full-bleed background texture if available, or a
    // deep space-navy solid quad (`[0.04, 0.06, 0.10, 1.0]`) as fallback.
    if bg_tex != 0 {
        batch.quad(full_m, [0.0, 0.0, 1024.0, height], bg_tex, UV, WHITE, 0.0, 0);
    } else {
        batch.quad(
            full_m,
            [0.0, 0.0, 1024.0, height],
            textures.solid,
            UV,
            [0.04, 0.06, 0.10, 1.0],
            0.0,
            0,
        );
    }

    // 2. Click/key gate (`ScreenId::Gate`).
    // On the initial click/key gate, draw only the backdrop and the pulsing
    // centered `PRESS ANY KEY OR CLICK TO BEGIN` prompt (`src/shell/gate.js`).
    if active == ScreenId::Gate {
        let a = gate::pulse(app.gate_elapsed);
        let y = gate_y - fonts.medium.caps_middle();
        draw_str(
            &fonts.medium,
            batch,
            full_m,
            gate::IW * 0.5,
            y,
            GATE_TEXT,
            tint_color(WHITE, a),
            2,
        );
        return;
    }

    let pulse = menu_pulse(app.pulse_elapsed);

    // 3. Active screen content.
    match base_screen {
        // `MAIN MENU`: 7-row angled stack at `at = [0.5, 0.65]`, `y = 30`,
        // `width = 250`, `rotX = -20`, `rotY = -30`, `rotZ = 1` (`XYZ` Euler
        // order), plus the bottom-right version string (`src/shell/screens.js`).
        ScreenId::Main => {
            let rows = app.active_rows();
            let sel = app.selected_cached_row_index(ScreenId::Main);
            draw_rows(
                batch,
                fonts,
                textures,
                height,
                app.screen_title_str(ScreenId::Main),
                rows,
                sel,
                pulse,
                player_menu_layout(ScreenId::Main),
            );
            // Bottom-right version text (`src/shell/screens.js`: `at: [1, 1], x: -8, y: -18, align: 1, alpha: 0.7`).
            let ver_m = anchor(1.0, 1.0, true, height);
            draw_str(
                &fonts.small,
                batch,
                ver_m,
                -8.0,
                -18.0,
                &app.version_text,
                tint_color(WHITE, 0.7),
                1,
            );
        }
        // `SINGLEPLAYER`: 3 rows in `gamefont_large` with `width = 650` and a
        // bottom-left `BACK` button (`src/shell/screens.js`).
        ScreenId::Singleplayer => {
            let rows = app.active_rows();
            let sel = app.selected_cached_row_index(ScreenId::Singleplayer);
            draw_rows(
                batch,
                fonts,
                textures,
                height,
                app.screen_title_str(ScreenId::Singleplayer),
                rows,
                sel,
                pulse,
                player_menu_layout(ScreenId::Singleplayer),
            );
            draw_bottom_button(
                batch,
                fonts,
                height,
                "back",
                app.say("common.back", "BACK"),
                false,
                true,
                pulse,
            );
        }
        // `INSTANT ACTION`: dual-list playlist builder (`draw_instant_action`).
        ScreenId::InstantAction => {
            draw_instant_action(
                batch,
                fonts,
                textures,
                app,
                viewport_w,
                viewport_h,
                height,
                pulse,
            );
        }
        // `InstantActionEra`: draws the underlying Instant Action screen, dims
        // it with a half-alpha black quad, and overlays the `CLONE WARS` /
        // `GALACTIC CIVIL WAR` picker rows at `at = [0.25, 0.5]`.
        ScreenId::InstantActionEra => {
            draw_instant_action(
                batch,
                fonts,
                textures,
                app,
                viewport_w,
                viewport_h,
                height,
                pulse,
            );
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, height],
                textures.solid,
                UV,
                [0.0, 0.0, 0.0, 0.5],
                0.0,
                0,
            );
            let rows = app.active_rows();
            let sel = app.selected_cached_row_index(ScreenId::InstantActionEra);
            draw_rows(
                batch,
                fonts,
                textures,
                height,
                app.screen_title_str(ScreenId::InstantActionEra),
                rows,
                sel,
                pulse,
                RowsLayout {
                    at: [0.25, 0.5],
                    width: 350.0,
                    ..RowsLayout::default()
                },
            );
            draw_bottom_button(
                batch,
                fonts,
                height,
                "back",
                app.say("common.back", "BACK"),
                false,
                true,
                pulse,
            );
        }
        // Standard angled row screens (`OPTIONS`, `OptionsPage`, `PROFILE
        // MANAGEMENT`, `MULTIPLAYER`, `LAN`/`ONLINE` server lists, and `SET GAME OPTIONS`).
        ScreenId::Options
        | ScreenId::OptionsPage(_)
        | ScreenId::Profile
        | ScreenId::Multiplayer
        | ScreenId::MultiplayerList(_)
        | ScreenId::HostDialog => {
            let rows = app.active_rows();
            let sel = app.selected_cached_row_index(base_screen);
            // Server lists (`LAN`/`ONLINE`), option sub-pages, and `SET GAME OPTIONS`
            // (`HostDialog`) use a wider `620`-unit bar stack so labels and values
            // fit on one line; top-level menus use `350` units.
            let width = match base_screen {
                ScreenId::MultiplayerList(_)
                | ScreenId::OptionsPage(_)
                | ScreenId::HostDialog => 620.0,
                _ => 350.0,
            };
            draw_rows(
                batch,
                fonts,
                textures,
                height,
                app.screen_title_str(base_screen),
                rows,
                sel,
                pulse,
                RowsLayout {
                    width,
                    ..RowsLayout::default()
                },
            );
            draw_bottom_button(
                batch,
                fonts,
                height,
                "back",
                app.say("common.back", "BACK"),
                false,
                true,
                pulse,
            );
            if base_screen == ScreenId::HostDialog {
                draw_bottom_button(
                    batch,
                    fonts,
                    height,
                    "accept",
                    app.say("ifs.mp.create.launch", "LAUNCH"),
                    sel == 0,
                    true,
                    pulse,
                );
            }
        }
        // `DIRECT CONNECTION`: address entry modal with `CONNECT` / `CANCEL`.
        ScreenId::DirectConnect => {
            draw_popup_box(
                batch,
                fonts,
                textures,
                height,
                "DIRECT CONNECTION",
                Some(&app.direct_connect_draft),
                &[("CONNECT", true), ("CANCEL", false)],
                pulse,
            );
        }
        ScreenId::Gate | ScreenId::Popup => {}
    }

    // 4. Profile editing modal or active popup over the current screen:
    //    dims the screen with a `[0, 0, 0, 0.55]` quad and draws `draw_popup_box`.
    if app.profile.editing {
        batch.quad(
            full_m,
            [0.0, 0.0, 1024.0, height],
            textures.solid,
            UV,
            [0.0, 0.0, 0.0, 0.55],
            0.0,
            0,
        );
        // Default to `"ENTER PROFILE CALLSIGN:"` when opened from `PROFILE
        // MANAGEMENT`, or show the multiplayer gate message when opened by
        // `require_profile_for`.
        let prompt = if app.profile.message.is_empty() {
            "ENTER PROFILE CALLSIGN:"
        } else {
            &app.profile.message
        };
        draw_popup_box(
            batch,
            fonts,
            textures,
            height,
            prompt,
            Some(&app.profile.draft),
            &[("OK", true), ("CANCEL", false)],
            pulse,
        );
    } else if let Some(popup) = &app.popup {
        batch.quad(
            full_m,
            [0.0, 0.0, 1024.0, height],
            textures.solid,
            UV,
            [0.0, 0.0, 0.0, 0.55],
            0.0,
            0,
        );
        // Single-button `Notice` (`OK`), two-button `ConfirmExit` (`YES` / `NO`),
        // or three-button `HostClosePrompt`.
        match popup {
            PopupState::Notice { message } => {
                draw_popup_box(
                    batch,
                    fonts,
                    textures,
                    height,
                    message,
                    None,
                    &[("OK", true)],
                    pulse,
                );
            }
            PopupState::ConfirmExit {
                message,
                selected_yes,
            } => {
                draw_popup_box(
                    batch,
                    fonts,
                    textures,
                    height,
                    message,
                    None,
                    &[("YES", *selected_yes), ("NO", !*selected_yes)],
                    pulse,
                );
            }
            PopupState::HostClosePrompt {
                title,
                message,
                buttons,
                selected,
            } => {
                draw_popup_box(
                    batch,
                    fonts,
                    textures,
                    height,
                    title,
                    Some(message),
                    &[
                        (&buttons[0], *selected == 0),
                        (&buttons[1], *selected == 1),
                        (&buttons[2], *selected == 2),
                    ],
                    pulse,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ShellData;
    use crate::launch::GameFamily;
    use crate::loading::LoadingTimeline;
    use crate::loading::LookId;

    #[test]
    fn pointer_hits_rendered_menu_rows_in_wide_and_standard_windows() {
        let mut app = ShellApp::new(ShellData::default(), None);
        let fonts = ShellFonts::default();
        for screen in [ScreenId::Main, ScreenId::Singleplayer] {
            app.stack = vec![screen];
            app.rebuild_cached_rows();
            for (w, h) in [(1280.0, 720.0), (1024.0, 768.0)] {
                let (_, _, height) = gate::gate_layout(w, h);
                let layout = player_menu_layout(screen);
                let p = fonts.get(layout.font).height + 5.0;
                let n = app.active_rows().len();
                let m = mul(translate(anchor(layout.at[0], layout.at[1], true, height),
                    rows_shift(layout.width, p, layout.list_count.unwrap_or(n)) + layout.x, layout.y, 0.0),
                    rotation_euler(layout.rot_x, layout.rot_y, layout.rot_z, layout.xyz_order));
                for k in 0..n {
                    let cy = -(n as f64*p)*0.5 + k as f64*p;
                    let [x, y] = bfe_interface::space::plane_point(&m, 50.0, cy, 0.0, height);
                    assert_eq!(hit_player_menu(&app, &fonts, w, h, x*w/1024.0, y*h/height), Some(k));
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
    fn draw_shell_and_draw_loading_emit_quads_for_every_screen() {
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

        // Add one extracted Clone Wars map (`nab2`) so Instant Action can add
        // and render a playlist entry with faction icons.
        let mut data = ShellData::default();
        data.maps.push(crate::data::ShellMap {
            family: GameFamily::Swbf1,
            id: "nab2".to_string(),
            name: "Naboo: Theed".to_string(),
            name_key: None,
            extracted: Some(true),
            eras: std::collections::BTreeMap::from([(
                "cw".to_string(),
                crate::data::MapEraInfo {
                    teams: vec!["rep".to_string(), "cis".to_string()],
                    ..Default::default()
                },
            )]),
            fly: None,
            load: None,
        });

        let mut app = ShellApp::new(data, None);
        // Gate screen
        draw_shell(&mut batch, &fonts, &textures, &app, 1280.0, 720.0);
        assert!(batch.count > 10);

        // Main menu
        app.unlock_gate();
        draw_shell(&mut batch, &fonts, &textures, &app, 1280.0, 720.0);
        assert!(batch.count > 50);

        // Instant Action
        app.push_screen(ScreenId::InstantAction);
        app.ia_add();
        draw_shell(&mut batch, &fonts, &textures, &app, 1280.0, 720.0);
        assert!(batch.count > 40);

        // Loading screen in track and zoom phases
        let tl = LoadingTimeline::new(
            3.5,
            1.0,
            0.0,
            vec![Some([0.25, 0.25, 0.75, 0.75]), None],
        );
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
        assert!(batch.count > 35);
    }
}
