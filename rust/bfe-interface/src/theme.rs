//! Classic 2004 menu theme renderer (`src/menus/themes/classic/index.js`,
//! `src/menus/themes/classic/draw.js`, and `rust/bfe-shell/src/render.rs`).
//!
//! Draws rows, frames, popups, lists, scrollbars, sliders, tabs, and buttons
//! with the exact geometry, textures, font metrics, and colours of
//! `rust/bfe-shell/src/render.rs` and `src/menus/themes/classic/{index,draw}.js`,
//! performing zero heap allocations per frame.

use crate::fonts::{Font, Fonts, FONT_LARGE, FONT_MEDIUM, FONT_SMALL, FONT_TINY};
use crate::options::OptionKind;
use crate::screen::MenuScreen;
use crate::space::{
    anchor, mul, rotate_x, rotate_y, rotate_z, translate, Batch, Color, Matrix, IDENTITY, UV,
};
use crate::stack::ScreenStack;
use crate::surface::Surface;
use crate::widgets::{
    ButtonWidget, HorizontalButtonsWidget, ListLook, ListWidget, OptionRowWidget,
    OptionsLayoutKind, OptionsWidget, PanelWidget, RowItem, RowsLayout, TabsWidget, TextLook,
    TextWidget, Widget,
};

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

/// Full-screen modal backdrop dimming overlay (`[0, 0, 0, 128/255]`, `DIM` in `draw.js`).
pub const DIM: Color = [0.0, 0.0, 0.0, 128.0 / 255.0];

/// Left offset of row and header 3-piece bars (`BAR_X = -4.7` in `classic/index.js` and `render.rs`).
pub const BAR_X: f64 = -4.7;

/// Left offset of row labels (`TEXT_X = 0.8` in `classic/index.js` and `render.rs`).
pub const TEXT_X: f64 = 0.8;

/// Horizontal inset of bottom-edge `BACK` / `ACCEPT` buttons (`BUTTON_INSET = 21.0`).
pub const BUTTON_INSET: f64 = 21.0;

/// Dimming factor for unfocused slider bars (`SLIDER_DIM = 0.5`).
pub const SLIDER_DIM: f64 = 0.5;

/// Alpha for scrollbar track (`TRACK_ALPHA = 0.5`).
pub const TRACK_ALPHA: f64 = 0.5;

/// Alpha for scrollbar thumb (`THUMB_ALPHA = 0.33`).
pub const THUMB_ALPHA: f64 = 0.33;

/// Period in seconds (`0.7s`) of one full button/bar pulse cycle (`PULSE` in `src/shell/gate.js`).
pub const PULSE_PERIOD: f64 = 0.7;

/// Minimum alpha reached halfway through the pulse (`LOW = 0.3` in `src/shell/gate.js`).
pub const PULSE_LOW: f64 = 0.3;

/// Computes the horizontal shift that centers an angled stack of `count` rows
/// (`rowsShift(width, pitch, count)` in `src/menus/themes/classic/index.js` and
/// `rows_shift` in `rust/bfe-shell/src/render.rs`).
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

/// Computes `(bar_width, flipped)` for row `k` (`0..n`) in a stack of `n` visible rows
/// (`rowBars(width, pitch, n)` in `src/menus/themes/classic/index.js` and
/// `row_bar_at` in `rust/bfe-shell/src/render.rs`).
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
#[inline]
pub fn selected_color(a: f64) -> Color {
    let g = (1.2 * a * 255.0 + 50.0).clamp(100.0, 255.0);
    [1.0, g / 255.0, 100.0 / 255.0, a]
}

/// Pulse factor (`0.3..1.0`) for focused buttons and bars (`menu_pulse` in `rust/bfe-shell/src/render.rs`).
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

#[inline]
pub fn tint_color(c: Color, a: f64) -> Color {
    [c[0], c[1], c[2], c[3] * a]
}

#[inline]
pub fn scaled_color(c: Color, k: f64, a: f64) -> Color {
    [c[0] * k, c[1] * k, c[2] * k, c[3] * a]
}

#[inline]
fn mul_color(a: Color, b: Color) -> Color {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

/// Fallback solid colour when a texture handle is `0` (`FALLBACK` in `classic/draw.js` and `render.rs`).
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

/// Zero-allocation string draw matching `draw_str` in `rust/bfe-shell/src/render.rs`.
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

/// Zero-allocation string width measurement matching `str_width` in `rust/bfe-shell/src/render.rs`.
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

/// Draws a horizontal slider (`NewHSlider` in `src/menus/themes/classic/draw.js`):
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
    /// Row `index` inside a `Rows` widget (`widget_index`).
    Row { widget_index: usize, row_index: usize },
    /// Entry `row_index` inside a `List` widget (`widget_index`).
    ListRow {
        widget_index: usize,
        row_index: usize,
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
        widget_index: usize,
        item_index: usize,
        slider_x: Option<[f64; 2]>,
    },
}

/// Interactive hit region in local plane coordinates (`m`) for [`crate::input::MenuInput`].
#[derive(Clone, Debug)]
pub struct HitRegion {
    pub m: Matrix,
    pub rect: [f64; 4],
    pub target: HitTarget,
}

/// Classic 2004 menu theme renderer (`ClassicTheme` in `src/menus/themes/classic/index.js`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ClassicTheme;

impl ClassicTheme {
    /// Draws an angled stack of menu rows under an orange header bar, matching
    /// `draw_rows` in `rust/bfe-shell/src/render.rs` and `paintRows` in
    /// `src/menus/themes/classic/index.js` pixel-for-pixel.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_rows_slice(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        title: &str,
        rows: &[RowItem],
        selected: usize,
        focused_widget: bool,
        pulse: f64,
        layout: RowsLayout,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        let font = fonts.by_name(layout.font);
        let yh = font.height;
        let y_spacing = layout.y_spacing;
        let p = yh + y_spacing;
        let n = rows.iter().filter(|r| !r.hidden).count();
        if n == 0 {
            return;
        }
        let shift = rows_shift(layout.width, p, layout.list_count.unwrap_or(n));
        let base = anchor(layout.at[0], layout.at[1], true, surface.height);
        let moved = translate(base, shift + layout.x, layout.y, 0.0);
        let m = mul(
            moved,
            rotation_euler(layout.rot_x, layout.rot_y, layout.rot_z, layout.xyz_order),
        );
        let y0 = -(n as f64 * p) * 0.5;

        if !title.is_empty() {
            let ty = y0 - 1.75 * p;
            let width = layout.width - 1.25 * (yh - y_spacing) - 0.65 * p;
            let by = ty - 0.25 * p;
            let gh = font.height;
            draw_bar(
                batch,
                surface,
                m,
                ["headerbuttonleft", "headerbuttonmid", "headerbuttonright"],
                BAR_X,
                by,
                width,
                ty + gh + 0.25 * p,
                false,
                WHITE,
            );
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

        let mut k = 0usize;
        for row in rows {
            if row.hidden {
                continue;
            }
            let cy = y0 + (k as f64) * p;
            let focused = focused_widget && k == selected && !row.disabled;
            let a = if focused { pulse } else { 1.0 };
            let bar_color = if focused {
                tint_color(WHITE, a)
            } else {
                scaled_color(WHITE, DIMMED, a)
            };
            let (bar_w, flipped) = row_bar_at(layout.width, p, n, k);
            draw_bar(
                batch,
                surface,
                m,
                ["buttonleft", "buttonmid", "buttonright"],
                BAR_X,
                cy - p * 0.5,
                bar_w + 5.0,
                cy + p * 0.5,
                flipped,
                bar_color,
            );
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
            if let Some(h) = hits.as_deref_mut() {
                h.push(HitRegion {
                    m,
                    rect: [BAR_X, cy - p * 0.5, BAR_X + bar_w + 5.0 + p, cy + p * 0.5],
                    target: HitTarget::Row {
                        widget_index,
                        row_index: k,
                    },
                });
            }
            k += 1;
        }
    }

    /// Draws a bottom-edge or positioned button, matching `draw_bottom_button` in
    /// `rust/bfe-shell/src/render.rs` and `paintButton` in `src/menus/themes/classic/index.js`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_button(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        button: &ButtonWidget,
        focused: bool,
        pulse: f64,
        widget_index: usize,
        hits: Option<&mut Vec<HitRegion>>,
    ) {
        if button.hidden {
            return;
        }
        let (ax, ay, default_align) = match button.place {
            "accept" => (1.0, 1.0, 1u32),
            "middle" => (0.5, 1.0, 2u32),
            _ => (0.0, 1.0, 0u32),
        };
        let at = button.at.unwrap_or([ax, ay]);
        let align = button.align.unwrap_or(default_align);
        let m = anchor(at[0], at[1], true, surface.height);
        let x = button.x.unwrap_or(if align == 0 {
            BUTTON_INSET
        } else if align == 1 {
            -BUTTON_INSET
        } else {
            0.0
        });
        let font = fonts.by_name(button.font);
        let y = button.y.unwrap_or(-12.0) - font.caps_middle();
        let color = if button.disabled {
            GREY
        } else if focused {
            selected_color(pulse)
        } else {
            WHITE
        };
        draw_str(font, batch, m, x, y, &button.text, color, align);

        if let Some(h) = hits {
            let w = str_width(font, &button.text).max(48.0);
            let x0 = match align {
                1 => x - w,
                2 => x - w * 0.5,
                _ => x,
            };
            h.push(HitRegion {
                m,
                rect: [x0 - 4.0, -27.5, x0 + w + 4.0, -2.5],
                target: HitTarget::Button { widget_index },
            });
        }
    }

    /// Draws a [`ListWidget`] inside a 9-slice `border_3_pieces` frame with `listbox_cursor`
    /// selection highlight, `listbox_hilight` hover highlight, and scrollbar, matching
    /// `paintList` in `src/menus/themes/classic/index.js` and `draw_instant_action` in `render.rs`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_list(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        list: &ListWidget,
        focused: bool,
        elapsed: f64,
        _pulse: f64,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if list.hidden {
            return;
        }
        let look: ListLook = list.look;
        let base = anchor(look.at[0], look.at[1], true, surface.height);
        let m = rotate_y(
            translate(base, look.x, look.y, 0.0),
            look.rot_y.to_radians(),
        );
        if look.show_frame {
            draw_frame(
                batch,
                surface,
                m,
                look.box_x,
                look.box_y,
                look.box_w,
                look.box_h,
                "border_3_pieces",
                WHITE,
            );
        }
        let count = list.rows.len();
        let shown = list.shown.max(1);
        let scroll = count > shown;
        let first = list.first.min(count.saturating_sub(1));
        let top = look.box_y - look.box_h * 0.5 + look.inset;
        let left = look.box_x - look.box_w * 0.5 + look.pad_left;
        let inner = look.box_w - look.pad_left - look.pad_right - if scroll { 26.0 } else { 0.0 };
        let font = fonts.by_name(look.font);

        for k in 0..shown {
            let idx = first + k;
            let Some(entry) = list.rows.get(idx) else {
                break;
            };
            let cy = top + (k as f64) * look.pitch + look.pitch * 0.5;
            if idx == list.selected {
                let a = if focused {
                    cursor_pulse(elapsed)
                } else {
                    1.0
                };
                draw_frame(
                    batch,
                    surface,
                    m,
                    left + inner * 0.5,
                    cy,
                    inner + 6.0,
                    look.pitch + 10.0,
                    "listbox_cursor",
                    tint_color(WHITE, a),
                );
            } else if list.hover_row == Some(idx) {
                draw_frame(
                    batch,
                    surface,
                    m,
                    left + inner * 0.5,
                    cy,
                    inner + 6.0,
                    look.pitch + 10.0,
                    "listbox_hilight",
                    WHITE,
                );
            }
            let text_color = if entry.disabled { GREY } else { WHITE };
            let ty = cy - font.caps_middle();

            if entry.cell_count >= 2 && list.columns.len() >= 2 {
                let col0_x = left + 8.0 + list.columns[0].width * inner;
                draw_str(
                    font,
                    batch,
                    m,
                    col0_x,
                    ty,
                    &entry.cells[0],
                    text_color,
                    list.columns[0].align,
                );
                let col1_x = left + 8.0 + list.columns[1].width * inner;
                draw_str(
                    font,
                    batch,
                    m,
                    col1_x,
                    ty,
                    &entry.cells[1],
                    text_color,
                    list.columns[1].align,
                );
            } else if entry.cell_count >= 2 {
                draw_str(font, batch, m, left + 8.0, ty, &entry.cells[0], text_color, 0);
                draw_str(
                    font,
                    batch,
                    m,
                    left + 8.0 + 0.4 * inner,
                    ty,
                    &entry.cells[1],
                    text_color,
                    0,
                );
            } else {
                draw_str(font, batch, m, left + 8.0, ty, &entry.cells[0], text_color, 0);
            }

            if !entry.icons.is_empty() {
                let size = 16.9;
                for (icon_i, icon_name) in entry.icons.iter().enumerate() {
                    let right =
                        left + inner - ((entry.icons.len() - icon_i - 1) as f64) * (size + 3.0);
                    piece_quad(
                        batch,
                        surface,
                        m,
                        icon_name,
                        right - size,
                        cy - size * 0.5,
                        right,
                        cy + size * 0.5,
                        UV,
                        WHITE,
                    );
                }
            }

            if let Some(h) = hits.as_deref_mut() {
                h.push(HitRegion {
                    m,
                    rect: [left, cy - look.pitch * 0.5, left + inner, cy + look.pitch * 0.5],
                    target: HitTarget::ListRow {
                        widget_index,
                        row_index: idx,
                        slider_x: entry
                            .slider_ratio
                            .map(|_| [left + inner * 0.55, left + inner - 8.0]),
                    },
                });
            }
        }

        if scroll {
            draw_scroll_bar(
                batch,
                surface,
                m,
                look.box_x + look.box_w * 0.5 - 34.0,
                top,
                (shown as f64) * look.pitch,
                first,
                shown,
                count,
            );
        }
    }

    /// Draws a [`TextWidget`] (`title`, `heading`, `popup`, `subtitle`, `note`, `label`), matching
    /// `paintText` in `src/menus/themes/classic/index.js` and `draw_str` in `render.rs`.
    pub fn paint_text(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        text: &TextWidget,
    ) {
        if text.hidden || text.text.is_empty() {
            return;
        }
        if text.look == TextLook::Popup {
            let font = fonts.by_name(FONT_MEDIUM);
            let m = anchor(0.5, 0.5, false, surface.height);
            let color = tint_color(text.color.unwrap_or(WHITE), text.alpha);
            let mut y = -74.0;
            for raw_line in text.text.lines() {
                let bytes = raw_line.as_bytes();
                let mut line_start = 0usize;
                let mut line_end = 0usize;
                let mut i = 0usize;
                while i < bytes.len() {
                    while i < bytes.len() && bytes[i] == b' ' {
                        i += 1;
                    }
                    if i >= bytes.len() {
                        break;
                    }
                    let word_start = i;
                    while i < bytes.len() && bytes[i] != b' ' {
                        i += 1;
                    }
                    let word_end = i;
                    if line_start == line_end {
                        line_start = word_start;
                        line_end = word_end;
                    } else {
                        let candidate = &raw_line[line_start..word_end];
                        if str_width(font, candidate) <= 260.0 {
                            line_end = word_end;
                        } else {
                            draw_str(
                                font,
                                batch,
                                m,
                                0.0,
                                y,
                                &raw_line[line_start..line_end],
                                color,
                                2,
                            );
                            y += font.height;
                            line_start = word_start;
                            line_end = word_end;
                        }
                    }
                }
                if line_end > line_start {
                    draw_str(
                        font,
                        batch,
                        m,
                        0.0,
                        y,
                        &raw_line[line_start..line_end],
                        color,
                        2,
                    );
                    y += font.height;
                }
            }
            return;
        }
        let default_font = match text.look {
            TextLook::Title | TextLook::Heading => FONT_LARGE,
            TextLook::Popup | TextLook::Subtitle | TextLook::Label => FONT_MEDIUM,
            TextLook::Note => FONT_SMALL,
        };
        let font_name = text.font.unwrap_or(default_font);
        let font = fonts.by_name(font_name);
        let m = anchor(text.at[0], text.at[1], text.safe, surface.height);
        let base_color = text.color.unwrap_or(WHITE);
        let color = tint_color(base_color, text.alpha);

        let mut y = text.y;
        for line in text.text.lines() {
            draw_str(font, batch, m, text.x, y, line, color, text.align);
            y += font.height;
        }
    }

    /// Draws a 9-slice [`PanelWidget`] (`paintPanel` in `src/menus/themes/classic/index.js`).
    pub fn paint_panel(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        panel: &PanelWidget,
    ) {
        if panel.hidden {
            return;
        }
        let m = anchor(panel.at[0], panel.at[1], panel.safe, surface.height);
        draw_frame(
            batch,
            surface,
            m,
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            panel.texture,
            panel.color,
        );
        if !panel.title.is_empty() {
            let font = fonts.by_name(FONT_MEDIUM);
            draw_str(
                font,
                batch,
                m,
                panel.x,
                panel.y - panel.h * 0.5 - font.height - 4.0,
                &panel.title,
                WHITE,
                2,
            );
        }
    }

    /// Draws a centered row of horizontal buttons (`ADD` / `REMOVE`, `YES` / `NO`, `OK`),
    /// matching `draw_popup_box` and `draw_instant_action` in `rust/bfe-shell/src/render.rs`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_horizontal_buttons(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        hb: &HorizontalButtonsWidget,
        focused: bool,
        pulse: f64,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if hb.hidden || hb.buttons.is_empty() {
            return;
        }
        let m = anchor(0.5, 0.5, false, surface.height);
        let font = fonts.by_name(hb.font);
        let by = hb.y - font.caps_middle();
        let count = hb.buttons.len();
        for (i, item) in hb.buttons.iter().enumerate() {
            let x = if count == 1 {
                0.0
            } else {
                -40.0 + (i as f64) * hb.spacing - ((count as f64) - 2.0) * (hb.spacing * 0.5)
            };
            let color = if item.disabled {
                GREY
            } else if focused && i == hb.selected {
                selected_color(pulse)
            } else {
                WHITE
            };
            draw_str(font, batch, m, x, by, &item.label, color, 2);
            if let Some(h) = hits.as_deref_mut() {
                h.push(HitRegion {
                    m,
                    rect: [x - 40.0, hb.y - 22.5, x + 40.0, hb.y + 22.5],
                    target: HitTarget::HorizontalButton {
                        widget_index,
                        button_index: i,
                    },
                });
            }
        }
    }

    /// Draws the 3-piece horizontal orange header-bar tabs (`INFANTRY` / `VEHICLE` / `STARFIGHTER`
    /// on `CONTROL OPTIONS`), matching `paintTabs` in `src/menus/themes/classic/index.js`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_tabs(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        tabs: &TabsWidget,
        focused: bool,
        pulse: f64,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if tabs.hidden {
            return;
        }
        let m = anchor(0.0, 0.0, true, surface.height);
        let font = fonts.by_name(tabs.font);
        let caps = font.caps_middle();
        let sw = 0.9 * 1024.0;
        let y0 = tabs.y - 13.0;
        let y1 = tabs.y + 11.0;
        for (i, tab) in tabs.tabs.iter().enumerate() {
            let x = 16.0 + (i as f64) * (1.0 / 3.0) * sw;
            let width = 0.30 * sw;
            let active = tab.id == tabs.active;
            let is_focused = focused && i == tabs.selected;
            let a = if active { 1.0 } else { 0.4 };
            draw_bar(
                batch,
                surface,
                m,
                ["headerbuttonleft", "headerbuttonmid", "headerbuttonright"],
                x - 8.0,
                y0,
                width - (y1 - y0),
                y1,
                false,
                tint_color(WHITE, a),
            );
            let base_color = if is_focused {
                selected_color(pulse)
            } else if active {
                WHITE
            } else {
                [200.0 / 255.0, 200.0 / 255.0, 200.0 / 255.0, 1.0]
            };
            let text_color = tint_color(base_color, if is_focused { 1.0 } else { a });
            draw_str(
                font,
                batch,
                m,
                x + 4.0,
                (y0 + y1) * 0.5 - caps,
                &tab.label,
                text_color,
                0,
            );
            if let Some(hr) = hits.as_deref_mut() {
                hr.push(HitRegion {
                    m,
                    rect: [x - 8.0, y0, x + width, y1],
                    target: HitTarget::Tab {
                        widget_index,
                        tab_index: i,
                    },
                });
            }
        }
    }

    /// Draws the two-column `label | value` [`OptionsWidget`] (`game`, `video`, `audio`, `controls`),
    /// matching `paintOptions` and `OPTION_LOOKS` in `src/menus/themes/classic/index.js` pixel-for-pixel.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_options(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        opts: &OptionsWidget,
        focused: bool,
        _elapsed: f64,
        pulse: f64,
        widget_index: usize,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if opts.hidden {
            return;
        }
        let safe_h = 0.9 * surface.height;
        let safe_w = 0.9 * 1024.0;
        let (
            font_name,
            labels_at,
            values_at,
            top,
            pitch,
            row_h,
            text_dy,
            bottom_opt,
            slider_cfg,
            number_at,
            scroll_at,
            align_left,
        ) = match opts.layout {
            OptionsLayoutKind::Game => (
                FONT_MEDIUM,
                [0.5, -15.0],
                Some([0.5, 15.0]),
                150.0,
                40.0,
                35.0,
                2.0,
                None,
                None,
                None,
                None,
                false,
            ),
            OptionsLayoutKind::Video => (
                FONT_MEDIUM,
                [0.5, -15.0],
                Some([0.5, 15.0]),
                32.0,
                30.0,
                28.0,
                0.0,
                Some(60.0),
                Some(([0.5, 15.0], None, 110.0, 22.0, false)),
                Some([0.5, 140.0]),
                Some([0.5, 380.0]),
                false,
            ),
            OptionsLayoutKind::Audio => (
                FONT_MEDIUM,
                [0.4, 0.0],
                Some([0.4, 35.0]),
                55.0,
                35.0,
                35.0,
                0.0,
                None,
                Some(([0.4, 20.0], Some([1.0, -176.8]), 0.0, 24.0, false)),
                Some([1.0, 0.0]),
                None,
                false,
            ),
            OptionsLayoutKind::Controls => (
                FONT_TINY,
                [0.15, 0.0],
                None,
                121.0,
                24.5,
                25.0,
                0.0,
                None,
                Some(([0.4, 0.0], None, 0.25, 24.0, true)),
                Some([1.0, -2.0]),
                None,
                true,
            ),
        };

        let font = fonts.by_name(font_name);
        let caps = font.caps_middle();

        let total_visible = opts.items.iter().filter(|it| !it.hidden).count();
        if total_visible == 0 {
            return;
        }
        let shown = if let Some(bottom) = bottom_opt {
            let room = safe_h - top - bottom;
            ((room / pitch).floor() as usize).max(1)
        } else {
            total_visible
        };
        let first = opts.first.min(total_visible.saturating_sub(shown));
        let end = (first + shown).min(total_visible);

        let labels_m = anchor(labels_at[0], 0.0, true, surface.height);
        let values_m = values_at.map(|va| anchor(va[0], 0.0, true, surface.height));

        let mut vis_pos = 0usize;
        for (item_idx, item) in opts.items.iter().enumerate() {
            if item.hidden {
                continue;
            }
            if vis_pos < first || vis_pos >= end {
                vis_pos += 1;
                continue;
            }
            let k = vis_pos - first;
            vis_pos += 1;

            let cy = top + (k as f64) * pitch;
            let y = cy - caps + text_dy;
            let row_focused = focused && item_idx == opts.selected;
            let enabled = !opts.disabled && (!item.disabled || item.kind == OptionKind::Fixed);
            let greyed = !enabled || (item.kind == OptionKind::Fixed && item.dim);

            let label_align = if align_left { 0 } else { 1 };
            if !item.label.is_empty() {
                draw_str(
                    font,
                    batch,
                    labels_m,
                    labels_at[1],
                    y,
                    &item.label,
                    WHITE,
                    label_align,
                );
            }

            let row_pulse = if row_focused { pulse } else { 1.0 };
            let value_color = if greyed {
                GREY
            } else if row_focused {
                selected_color(row_pulse)
            } else {
                WHITE
            };

            let mut slider_x = None;
            if item.kind == OptionKind::Slider {
                if let Some((from_at, to_opt, s_width, s_height, relative)) = slider_cfg {
                    let from_m = anchor(from_at[0], 0.0, true, surface.height);
                    let x0 = from_at[1];
                    let x1 = if let Some(to_at) = to_opt {
                        let to_x = (0.05 + 0.9 * to_at[0]) * 1024.0 + to_at[1];
                        let from_x = (0.05 + 0.9 * from_at[0]) * 1024.0 + from_at[1];
                        x0 + (to_x - from_x)
                    } else if relative {
                        x0 + s_width * safe_w
                    } else {
                        x0 + s_width
                    };
                    draw_slider_bar(
                        batch,
                        surface,
                        from_m,
                        x0,
                        cy - s_height * 0.5,
                        x1 - x0,
                        s_height,
                        item.ratio,
                        row_focused,
                        row_pulse,
                    );
                    slider_x = Some([x0, x1]);
                    if let Some(num_at) = number_at {
                        let num_m = anchor(num_at[0], 0.0, true, surface.height);
                        let num_align = if num_at[0] >= 1.0 { 1 } else { 0 };
                        draw_str(
                            font,
                            batch,
                            num_m,
                            num_at[1],
                            y,
                            &item.value_text,
                            value_color,
                            num_align,
                        );
                    }
                }
            } else if let (Some(vm), Some(va)) = (values_m, values_at) {
                draw_str(font, batch, vm, va[1], y, &item.value_text, value_color, 0);
            } else if let Some(num_at) = number_at {
                let num_m = anchor(num_at[0], 0.0, true, surface.height);
                draw_str(
                    font,
                    batch,
                    num_m,
                    num_at[1],
                    y,
                    &item.value_text,
                    value_color,
                    1,
                );
            }

            if let Some(h) = hits.as_deref_mut() {
                h.push(HitRegion {
                    m: labels_m,
                    rect: [-400.0, cy - row_h * 0.5, 450.0, cy + row_h * 0.5],
                    target: HitTarget::OptionsItem {
                        widget_index,
                        item_index: item_idx,
                        slider_x,
                    },
                });
            }
        }

        // Focused row's note under the rows (`paintOptions` in `classic/index.js`).
        if focused {
            if let Some(item) = opts.items.get(opts.selected) {
                if !item.note.is_empty() {
                    let note_y = if let Some(bottom) = bottom_opt {
                        safe_h - bottom + 8.0
                    } else {
                        top + (total_visible.min(shown) as f64) * pitch + 6.0
                    };
                    let note_m = anchor(0.5, 0.0, true, surface.height);
                    let tiny = fonts.by_name(FONT_TINY);
                    draw_str(tiny, batch, note_m, 0.0, note_y, &item.note, NOTE_YELLOW, 2);
                }
            }
        }

        // Scrollbar when rows exceed `shown` (`paintOptions` in `classic/index.js`).
        if let Some(sc_at) = scroll_at {
            if total_visible > shown {
                let sc_m = anchor(sc_at[0], 0.0, true, surface.height);
                let sc_x = sc_at[1];
                let sc_top = top - pitch * 0.5;
                let sc_h = (shown as f64) * pitch;
                draw_scroll_bar(batch, surface, sc_m, sc_x, sc_top, sc_h, first, shown, total_visible);
            }
        }
    }

    /// Draws a standalone [`OptionRowWidget`] (`paintOptionRow` in `src/menus/themes/classic/index.js`).
    pub fn paint_option_row(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        row: &OptionRowWidget,
        focused: bool,
        pulse: f64,
    ) {
        if row.hidden {
            return;
        }
        let m = anchor(row.at[0], row.at[1], row.safe, surface.height);
        let font = fonts.by_name(row.font);
        let color = if row.disabled {
            GREY
        } else if focused {
            selected_color(pulse)
        } else {
            WHITE
        };
        let ty = row.y - font.caps_middle();
        draw_str(font, batch, m, row.x, ty, &row.label, color, 0);
        if row.kind == OptionKind::Slider {
            let sx = row.x + row.width - row.slider_width - 56.0;
            draw_slider_bar(
                batch,
                surface,
                m,
                sx,
                row.y - row.height * 0.5,
                row.slider_width,
                row.height,
                row.ratio,
                focused,
                pulse,
            );
        }
        draw_str(
            font,
            batch,
            m,
            row.x + row.width,
            ty,
            &row.value_text,
            color,
            1,
        );
    }

    /// Paints a single [`MenuScreen`] onto `batch` without allocating.
    /// When `!is_top` (a modal popup is over this screen), `Rows`, `Options`, `List`, and `Tabs`
    /// are hidden (`UNDER_POPUP` in `src/menus/themes/classic/index.js`).
    pub fn paint_screen(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        screen: &MenuScreen,
        is_top: bool,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        if screen.dim_behind {
            let full_m = anchor(0.0, 0.0, false, surface.height);
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, surface.height],
                surface.solid,
                UV,
                DIM,
                0.0,
                0,
            );
        }
        let pulse = if is_top {
            menu_pulse(screen.elapsed)
        } else {
            1.0
        };
        let focused_idx = if is_top { screen.focused_index() } else { None };

        for (idx, widget) in screen.widgets.iter().enumerate() {
            if !is_top
                && matches!(
                    widget,
                    Widget::Rows(_) | Widget::Options(_) | Widget::List(_) | Widget::Tabs(_)
                )
            {
                continue;
            }
            let is_focused = focused_idx == Some(idx);
            let h_ref = if is_top { hits.as_deref_mut() } else { None };
            match widget {
                Widget::Rows(rows) => {
                    Self::paint_rows_slice(
                        batch,
                        fonts,
                        surface,
                        &rows.title,
                        &rows.rows,
                        rows.selected,
                        is_focused,
                        pulse,
                        rows.layout,
                        idx,
                        h_ref,
                    );
                }
                Widget::List(list) => {
                    Self::paint_list(
                        batch,
                        fonts,
                        surface,
                        list,
                        is_focused,
                        screen.elapsed,
                        pulse,
                        idx,
                        h_ref,
                    );
                }
                Widget::Button(btn) => {
                    Self::paint_button(batch, fonts, surface, btn, is_focused, pulse, idx, h_ref);
                }
                Widget::Text(txt) => {
                    Self::paint_text(batch, fonts, surface, txt);
                }
                Widget::Panel(panel) => {
                    Self::paint_panel(batch, fonts, surface, panel);
                }
                Widget::HorizontalButtons(hb) => {
                    Self::paint_horizontal_buttons(
                        batch, fonts, surface, hb, is_focused, pulse, idx, h_ref,
                    );
                }
                Widget::Tabs(tabs) => {
                    Self::paint_tabs(batch, fonts, surface, tabs, is_focused, pulse, idx, h_ref);
                }
                Widget::Options(opts) => {
                    Self::paint_options(
                        batch,
                        fonts,
                        surface,
                        opts,
                        is_focused,
                        screen.elapsed,
                        pulse,
                        idx,
                        h_ref,
                    );
                }
                Widget::OptionRow(row) => {
                    Self::paint_option_row(batch, fonts, surface, row, is_focused, pulse);
                }
            }
        }
    }

    /// Paints an entire [`ScreenStack`] over `backdrop_texture` (`0` uses fallback dark background),
    /// performing zero heap allocations per frame when `hits` is `None`.
    pub fn paint_stack(
        batch: &mut Batch,
        fonts: &Fonts,
        surface: &Surface,
        stack: &ScreenStack,
        backdrop_texture: u32,
        mut hits: Option<&mut Vec<HitRegion>>,
    ) {
        batch.begin();
        if let Some(h) = hits.as_deref_mut() {
            h.clear();
        }
        let full_m = anchor(0.0, 0.0, false, surface.height);
        if backdrop_texture != 0 {
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, surface.height],
                backdrop_texture,
                UV,
                WHITE,
                0.0,
                0,
            );
        } else {
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, surface.height],
                surface.solid,
                UV,
                [0.04, 0.06, 0.10, 1.0],
                0.0,
                0,
            );
        }

        let visible = stack.visible_screens();
        let len = visible.len();
        for (i, screen) in visible.iter().enumerate() {
            let is_top = i + 1 == len;
            let h_ref = if is_top { hits.as_deref_mut() } else { None };
            Self::paint_screen(batch, fonts, surface, screen, is_top, h_ref);
        }
    }
}
