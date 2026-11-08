//! Classic 2004 menu theme renderer (`src/menus/themes/classic/index.js`,
//! `src/menus/themes/classic/draw.js`, and `rust/bfe-shell/src/render.rs`).
//!
//! Draws rows, frames, popups, lists, scrollbars, sliders, tabs, and buttons
//! with the exact geometry, textures, font metrics, and colours of
//! `rust/bfe-shell/src/render.rs` and `src/menus/themes/classic/{index,draw}.js`,
//! performing zero heap allocations per frame.

// Font metrics, 3D matrix transforms, and surface textures used by classic theme primitives.
// Widget-level painters (`paint_rows_slice`, `paint_list`, `paint_options`, etc.) live in `classic.rs`.
// All public constants (`WHITE`, `BLACK`, `BAR_X`, `BUTTON_INSET`, etc.) remain exported from `theme`.
use crate::fonts::Font;
use crate::space::{rotate_x, rotate_y, rotate_z, Batch, Color, Matrix, IDENTITY, UV};
use crate::surface::Surface;

/// White RGBA (`[1.0, 1.0, 1.0, 1.0]`, `WHITE` in `src/menus/themes/classic/draw.js`).
pub const WHITE: Color = [1.0, 1.0, 1.0, 1.0];

/// Black RGBA (`[0.0, 0.0, 0.0, 1.0]`, `BLACK` in `src/menus/themes/classic/draw.js`).
pub const BLACK: Color = [0.0, 0.0, 0.0, 1.0];

/// Disabled grey RGBA (`[128/255, 128/255, 128/255, 1.0]`, `GREY` in `src/menus/themes/classic/draw.js`).
pub const GREY: Color = [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0];

/// Yellow footer note colour (`rgba(223, 223, 32)` in `src/menus/themes/classic/index.js`).
pub const NOTE_YELLOW: Color = [223.0 / 255.0, 223.0 / 255.0, 32.0 / 255.0, 1.0];

/// Dimming factor (`150 / 255`) for an unfocused menu row bar (`DIMMED` in `draw.js`).
pub const DIMMED: f64 = 150.0 / 255.0;

/// Full-screen modal backdrop dimming overlay (`[0, 0, 0, 128/255]`, `DIM` in `classic/index.js`).
pub const DIM: Color = [0.0, 0.0, 0.0, 128.0 / 255.0];

/// Left offset of row and header 3-piece bars (`BAR_X = -4.7` in `classic/index.js` and `render.rs`).
pub const BAR_X: f64 = -4.7;

/// Left offset of row labels (`TEXT_X = 0.8` in `classic/index.js` and `render.rs`).
pub const TEXT_X: f64 = 0.8;

/// Horizontal inset of bottom-edge `BACK` / `ACCEPT` buttons (`BUTTON_INSET = 21.0` in `classic/index.js`).
pub const BUTTON_INSET: f64 = 21.0;

/// Dimming factor for unfocused slider bars (`SLIDER_DIM = 0.5` in `classic/draw.js`).
pub const SLIDER_DIM: f64 = 0.5;

/// Alpha for scrollbar track (`TRACK_ALPHA = 0.5` in `classic/index.js`).
pub const TRACK_ALPHA: f64 = 0.5;

/// Alpha for scrollbar thumb (`THUMB_ALPHA = 0.33` in `classic/index.js`).
pub const THUMB_ALPHA: f64 = 0.33;

/// Period in seconds (`0.7s`) of one full button/bar pulse cycle (`PULSE` in `src/menus/stack.js`).
pub const PULSE_PERIOD: f64 = 0.7;

/// Minimum alpha reached halfway through the pulse (`PULSE.low = 0.3` in `src/menus/stack.js`).
pub const PULSE_LOW: f64 = 0.3;

/// Computes the horizontal shift that centers an angled stack of `count` rows
/// (`rowsShift(width, pitch, count)` in `src/menus/themes/classic/index.js` and
/// `rows_shift` in `rust/bfe-shell/src/render.rs`).
///
/// Replicates the 2004 `AddVerticalButtons` container offset: the bar width steps outward by
/// `pitch` per row down to the middle (`count / 2`) and inward past it, starting one `pitch`
/// wider when `count` is odd, then shifts the whole container left by half the widest bar.
pub fn rows_shift(width: f64, pitch: f64, count: usize) -> f64 {
    // Odd row counts start one `pitch` wider at the top row (`count % 2 === 1 ? pitch : 0`
    // in `rowsShift` in `src/menus/themes/classic/index.js`).
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

/// Computes `(bar_width, flipped)` for row `k` (`0..n`) in a stack of `n` visible rows
/// (`rowBars(width, pitch, n)` in `src/menus/themes/classic/index.js` and
/// `row_bar_at` in `rust/bfe-shell/src/render.rs`).
///
/// Mirrors the 2004 `ShowHideVerticalButtons` chevron geometry: rows above the middle grow wider
/// by `pitch` with an unflipped right triangle cap, while rows below the middle (`i > half`)
/// flip the right cap vertically (`uv = [0, 1, 1, 0]`) and step narrower by `pitch`.
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

/// Focused text colour without a bar: yellow with green channel following `a`
/// (`selectedColor(a)` in `src/menus/themes/classic/draw.js` and `render.rs`).
///
/// Computes `G = clamp(1.2 * a * 255 + 50, 100, 255)` so focused buttons, tabs, and option
/// values pulse between warm amber (`a = 0.3`) and bright yellow (`a = 1.0`).
#[inline]
pub fn selected_color(a: f64) -> Color {
    let g = (1.2 * a * 255.0 + 50.0).clamp(100.0, 255.0);
    [1.0, g / 255.0, 100.0 / 255.0, a]
}

/// Pulse factor (`0.3..1.0`) for focused buttons and bars (`PULSE` in `src/menus/stack.js`
/// and `menu_pulse` in `rust/bfe-shell/src/render.rs`).
///
/// Evaluates a triangle wave of period `PULSE_PERIOD = 0.7s` oscillating between `1.0` and
/// `PULSE_LOW = 0.3` (matching `gButton_CurSizeAdd` in the 2004 shell scripts).
#[inline]
pub fn menu_pulse(elapsed: f64) -> f64 {
    let phase = (elapsed / PULSE_PERIOD) % 1.0;
    let phase = if phase < 0.0 { phase + 1.0 } else { phase };
    let tri = if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    };
    PULSE_LOW + (1.0 - PULSE_LOW) * (1.0 - tri)
}

/// Listbox selection frame pulse (`1.0 -> 0.5 -> 1.0`, one cycle per second)
/// (`cursorPulse(time)` in `src/menus/themes/classic/index.js` and `render.rs`).
#[inline]
pub fn cursor_pulse(time: f64) -> f64 {
    let u = (time.rem_euclid(1.0)) * 2.0;
    if u < 1.0 {
        1.0 - 0.5 * u
    } else {
        0.5 + 0.5 * (u - 1.0)
    }
}

/// Builds a 3D Euler rotation matrix in degrees (`rotation` in `src/menus/themes/classic/index.js`
/// and `rotation_euler` in `rust/bfe-shell/src/render.rs`).
///
/// Defaults to `YXZ` intrinsic order (`xyz_order == false`) to match `new THREE.Euler(rx, ry, rz, "YXZ")`
/// in `classic/index.js`, or `XYZ` when `xyz_order` is `true`.
pub fn rotation_euler(rot_x_deg: f64, rot_y_deg: f64, rot_z_deg: f64, xyz_order: bool) -> Matrix {
    let rx = rot_x_deg.to_radians();
    let ry = rot_y_deg.to_radians();
    let rz = rot_z_deg.to_radians();
    if xyz_order {
        rotate_z(rotate_y(rotate_x(IDENTITY, rx), ry), rz)
    } else {
        rotate_z(rotate_x(rotate_y(IDENTITY, ry), rx), rz)
    }
}

/// Multiplies the alpha channel of `c` by `a` (`tint(color, a)` in `src/menus/themes/classic/draw.js`).
#[inline]
pub fn tint_color(c: Color, a: f64) -> Color {
    [c[0], c[1], c[2], c[3] * a]
}

/// Scales the RGB channels of `c` by `k` and its alpha channel by `a`
/// (`scaled(color, k)` + `tint` in `src/menus/themes/classic/draw.js`).
#[inline]
pub fn scaled_color(c: Color, k: f64, a: f64) -> Color {
    [c[0] * k, c[1] * k, c[2] * k, c[3] * a]
}

/// Component-wise RGBA multiplication used when tinting solid fallback colours.
#[inline]
pub(super) fn mul_color(a: Color, b: Color) -> Color {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

/// Fallback solid colour when a texture handle is `0` (`FALLBACK` in `classic/draw.js` and `render.rs`).
///
/// Ensures menu bars, 9-slice frames, sliders, and faction icons remain legible in headless tests
/// or before `common.lvl` textures finish loading.
pub fn fallback_color(name: &str) -> Color {
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
        "slider_sound" => [40.0 / 255.0, 95.0 / 255.0, 150.0 / 255.0, 1.0],
        "rep_icon" => [180.0 / 255.0, 45.0 / 255.0, 45.0 / 255.0, 1.0],
        "cis_icon" => [70.0 / 255.0, 130.0 / 255.0, 200.0 / 255.0, 1.0],
        "imp_icon" => [170.0 / 255.0, 180.0 / 255.0, 195.0 / 255.0, 1.0],
        "all_icon" => [220.0 / 255.0, 140.0 / 255.0, 40.0 / 255.0, 1.0],
        _ => WHITE,
    }
}

/// Encodes up to 160 UTF-16 code units of `s` into a stack buffer `buf` so [`draw_str`] and
/// [`str_width`] never allocate on the heap during per-frame rendering.
pub(super) fn encode_utf16_stack<'a>(s: &str, buf: &'a mut [u16; 160]) -> &'a [u16] {
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

/// Zero-allocation string draw matching `draw_str` in `rust/bfe-shell/src/render.rs`
/// and `Fonts.draw` in `src/interface/fonts.js`.
#[allow(clippy::too_many_arguments)]
pub fn draw_str(
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

/// Zero-allocation string width measurement matching `str_width` in `rust/bfe-shell/src/render.rs`
/// and `Fonts.width` in `src/interface/fonts.js`.
pub fn str_width(font: &Font, text: &str) -> f64 {
    let mut buf = [0u16; 160];
    let slice = encode_utf16_stack(text, &mut buf);
    font.width(slice, 1.0, 0.0)
}

/// Emits a textured quad or its geometric fallback (`piece_quad` in `rust/bfe-shell/src/render.rs`
/// and `quad` in `src/menus/themes/classic/draw.js`).
#[allow(clippy::too_many_arguments)]
pub fn piece_quad(
    batch: &mut Batch,
    surface: &Surface,
    m: Matrix,
    name: &str,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    uv: [f64; 4],
    color: Color,
) {
    let tex = surface.texture(name);
    if tex != 0 {
        batch.quad(m, [x0, y0, x1, y1], tex, uv, color, 0.0, 0);
    } else if name == "buttonright" || name == "headerbuttonright" {
        // When the right-end triangle cap texture is not loaded, emit a solid fallback quad
        // tinted by `fallback_color(name)`.
        let fb = mul_color(fallback_color(name), color);
        let d = y1 - y0;
        let flipped = uv[1] > uv[3];
        let corners = if flipped {
            [[x0, y0], [x0, y1], [x1 - d, y0], [x1, y1]]
        } else {
            [[x0, y0], [x0, y1], [x1, y0], [x1 - d, y1]]
        };
        batch.corners(
            m,
            [x0, y0, x1, y1],
            surface.solid,
            [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0], [1.0, 1.0]],
            fb,
            0.0,
            0,
        );
        let _ = corners;
    } else {
        let fb = mul_color(fallback_color(name), color);
        batch.quad(m, [x0, y0, x1, y1], surface.solid, UV, fb, 0.0, 0);
    }
}

/// Draws a 3-piece horizontal bar (`buttonleft`/`buttonmid`/`buttonright` or
/// `headerbuttonleft`/`headerbuttonmid`/`headerbuttonright`), matching `draw_bar`
/// in `rust/bfe-shell/src/render.rs` and `bar` in `src/menus/themes/classic/draw.js`.
///
/// The left cap spans `[x0, x0 + h]`, the stretched middle spans `[x0 + h, x0 + max(width, h)]`,
/// and the right triangle cap spans `[mid_end, mid_end + h]` (with vertically inverted UVs when
/// `flipped` is `true` for rows below the vertical midpoint of an angled menu stack).
#[allow(clippy::too_many_arguments)]
pub fn draw_bar(
    batch: &mut Batch,
    surface: &Surface,
    m: Matrix,
    names: [&str; 3],
    x0: f64,
    y0: f64,
    width: f64,
    y1: f64,
    flipped: bool,
    color: Color,
) {
    let h = y1 - y0;
    if h <= 0.0 {
        return;
    }
    let mid_end = x0 + width.max(h);
    let end = mid_end + h;
    let pieces = [
        (x0, x0 + h, names[0], false),
        (x0 + h, mid_end, names[1], false),
        (mid_end, end, names[2], true),
    ];
    for (a, b, name, is_right) in pieces {
        if b <= a {
            continue;
        }
        let uv = if is_right && flipped {
            [0.0, 1.0, 1.0, 0.0]
        } else {
            UV
        };
        piece_quad(batch, surface, m, name, a, y0, b, y1, uv, color);
    }
}

/// Draws a 9-slice window or cursor frame centered on `(x, y)`, matching `draw_frame`
/// in `rust/bfe-shell/src/render.rs` and `frame` in `src/menus/themes/classic/draw.js`.
#[allow(clippy::too_many_arguments)]
pub fn draw_frame(
    batch: &mut Batch,
    surface: &Surface,
    m: Matrix,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    name: &str,
    color: Color,
) {
    let tex = surface.texture(name);
    // If the 9-slice texture is not loaded, draw a single 4-unit-inset solid fallback quad.
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
            surface.solid,
            UV,
            [fb[0], fb[1], fb[2], fb[3] * color[3]],
            0.0,
            0,
        );
        return;
    }
    // Decompose into a 3x3 grid with 16-unit corners and UV cuts at `[0.0, 0.25, 0.75, 1.0]`,
    // collapsing the middle column or row if `w < 32` or `h < 32` (`nineSlice` in `shapes.js`).
    let corner = 16.0;
    let mut xs = [-w * 0.5, -w * 0.5 + corner, w * 0.5 - corner, w * 0.5];
    let mut ys = [-h * 0.5, -h * 0.5 + corner, h * 0.5 - corner, h * 0.5];
    if xs[2] < xs[1] {
        xs[1] = 0.0;
        xs[2] = 0.0;
    }
    if ys[2] < ys[1] {
        ys[1] = 0.0;
        ys[2] = 0.0;
    }
    let us = [0.0, 0.25, 0.75, 1.0];
    for j in 0..3 {
        for i in 0..3 {
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

/// Draws a vertical scrollbar (`slider_bg` track + `slider_fg` thumb), matching
/// `draw_scroll_bar` in `rust/bfe-shell/src/render.rs` and `scrollBar` in `classic/index.js`.
///
/// The track is 16 units wide at `TRACK_ALPHA = 0.5`, and the thumb spans the visible slice
/// `[first / count, (first + shown) / count]` (clamped to at least 6 units tall) at `THUMB_ALPHA = 0.33`.
#[allow(clippy::too_many_arguments)]
pub fn draw_scroll_bar(
    batch: &mut Batch,
    surface: &Surface,
    m: Matrix,
    x: f64,
    top: f64,
    height: f64,
    first: usize,
    shown: usize,
    count: usize,
) {
    piece_quad(
        batch,
        surface,
        m,
        "slider_bg",
        x,
        top,
        x + 16.0,
        top + height,
        UV,
        tint_color(WHITE, TRACK_ALPHA),
    );
    let denom = count.max(1) as f64;
    let t0 = top + (first as f64 / denom) * height;
    let t1 = (top + ((first + shown) as f64 / denom) * height)
        .max(t0 + 6.0)
        .min(top + height);
    piece_quad(
        batch,
        surface,
        m,
        "slider_fg",
        x,
        t0,
        x + 16.0,
        t1,
        UV,
        tint_color(WHITE, THUMB_ALPHA),
    );
}

/// Draws a horizontal slider (`slider` / `NewHSlider` in `src/menus/themes/classic/draw.js`):
/// navy `slider_bg` track `x0..x1`, `y0..y1`, and orange `slider_fg` thumb `0.1 * w` wide
/// at fraction `ratio` (`focused` is solid alpha `1.0`, unfocused is `SLIDER_DIM = 0.5`).
#[allow(clippy::too_many_arguments)]
pub fn draw_slider_bar(
    batch: &mut Batch,
    surface: &Surface,
    m: Matrix,
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    ratio: f64,
    focused: bool,
    _pulse: f64,
) {
    // Focused sliders draw at full alpha (`1.0`); unfocused sliders dim to `SLIDER_DIM = 0.5`.
    // The orange `slider_fg` thumb is `10%` of the track width (at least `6.0` units wide)
    // and travels across `[x0, x0 + w - tw]` as `ratio` moves from `0.0` to `1.0`.
    let a = if focused { 1.0 } else { SLIDER_DIM };
    let color = tint_color(WHITE, a);
    let tw = (0.1 * w).max(6.0);
    let tx = x0 + (w - tw) * ratio.clamp(0.0, 1.0);
    piece_quad(
        batch,
        surface,
        m,
        "slider_bg",
        x0,
        y0,
        x0 + w,
        y0 + h,
        UV,
        color,
    );
    piece_quad(
        batch,
        surface,
        m,
        "slider_fg",
        tx,
        y0,
        tx + tw,
        y0 + h,
        UV,
        color,
    );
}

/// Target of an interactive 3D hit region for pointer hover/click/drag.
#[derive(Clone, Debug, PartialEq)]
pub enum HitTarget {
    /// Row `row_index` inside a `Rows` widget (`widget_index`).
    Row { widget_index: usize, row_index: usize },
    /// Entry `row_index` inside a `List` widget (`widget_index`).
    ListRow {
        /// Index of the [`ListWidget`] in `MenuScreen::widgets`.
        widget_index: usize,
        /// Row index within `ListWidget::rows`.
        row_index: usize,
        /// Optional local `[x0, x1]` bounds when the row contains a draggable slider.
        slider_x: Option<[f64; 2]>,
    },
    /// Bottom or standalone `Button` widget (`widget_index`).
    Button { widget_index: usize },
    /// Button `button_index` inside a `HorizontalButtons` widget (`widget_index`).
    HorizontalButton {
        widget_index: usize,
        button_index: usize,
    },
    /// Tab `tab_index` inside a `Tabs` widget (`widget_index`).
    Tab { widget_index: usize, tab_index: usize },
    /// Item `item_index` inside an `Options` widget (`widget_index`).
    OptionsItem {
        /// Index of the [`OptionsWidget`] in `MenuScreen::widgets`.
        widget_index: usize,
        /// Item index within `OptionsWidget::items`.
        item_index: usize,
        /// Optional local `[x0, x1]` bounds when `item.kind == OptionKind::Slider`.
        slider_x: Option<[f64; 2]>,
    },
}

/// Interactive hit region in local plane coordinates (`m`) for [`crate::input::MenuInput`].
#[derive(Clone, Debug)]
pub struct HitRegion {
    /// 4x4 widget-to-camera transform matrix used by [`crate::space::local_point`] to intersect pointer rays.
    pub m: Matrix,
    /// Bounding rectangle `[x0, y0, x1, y1]` in the local `z = 0` plane of `m`.
    pub rect: [f64; 4],
    /// Widget and sub-item identified when a pointer ray hits `rect`.
    pub target: HitTarget,
}

/// Classic 2004 menu theme renderer (`ClassicTheme` in `src/menus/themes/classic/index.js`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ClassicTheme;

mod classic;
