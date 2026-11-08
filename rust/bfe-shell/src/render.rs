//! Zero-allocation per-frame rendering of the click gate, shell menus, and
//! loading screen onto [`bfe_render::interface::Batch`].
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
    loading::{
        self, disk, disk_place, light_level, mix_color_rgb, name_pos, on_screen, zoom_view, LookId,
        TimelineAt,
    },
    media::preview_rect,
    screens::{IaFocus, MenuRow, PopupState, ScreenId, ShellApp},
};
use bfe_render::interface::{
    anchor, mul, rotate_y, rotate_z, translate, Batch, Color, Font, Matrix, IDENTITY, UV,
};
use std::collections::HashMap;

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

const BAR_X: f64 = -4.7;
const TEXT_X: f64 = 0.8;
const BUTTON_INSET: f64 = 21.0;

/// Bitmap font set used by the shell (`gamefont_large`, `gamefont_medium`,
/// `gamefont_small`, `gamefont_tiny`).
#[derive(Default)]
pub struct ShellFonts {
    pub large: Font,
    pub medium: Font,
    pub small: Font,
    pub tiny: Font,
}

impl ShellFonts {
    /// Returns the font matching `name`, falling back to `medium`.
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
    pub texture: u32,
    pub width: f64,
    pub height: f64,
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
        for &name in candidates {
            let tex = self.get(name);
            if tex != 0 {
                return tex;
            }
        }
        0
    }

    /// Resolves the map preview still texture (`map_<id>` or `map_unk`).
    pub fn map_preview_for(&self, map_id: &str) -> u32 {
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
/// visible rows.
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

/// Pulse factor (`0.3..1.0`) for focused buttons and bars.
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

fn rotation_euler(rot_x_deg: f64, rot_y_deg: f64, rot_z_deg: f64, xyz_order: bool) -> Matrix {
    let rx = rot_x_deg.to_radians();
    let ry = rot_y_deg.to_radians();
    let rz = rot_z_deg.to_radians();
    if xyz_order {
        rotate_z(rotate_y(rotate_x(IDENTITY, rx), ry), rz)
    } else {
        // Default "YXZ" Euler order in `src/menus/themes/classic/index.js`.
        rotate_z(rotate_x(rotate_y(IDENTITY, ry), rx), rz)
    }
}

fn tint_color(c: Color, a: f64) -> Color {
    [c[0], c[1], c[2], c[3] * a]
}

fn scaled_color(c: Color, k: f64, a: f64) -> Color {
    [c[0] * k, c[1] * k, c[2] * k, c[3] * a]
}

fn mul_color(a: Color, b: Color) -> Color {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

fn fallback_color(name: &str) -> Color {
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

/// Encodes `s` into a stack UTF-16 buffer without heap allocation.
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

#[allow(clippy::too_many_arguments)]
fn draw_str(
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

fn str_width(font: &Font, text: &str) -> f64 {
    let mut buf = [0u16; 160];
    let slice = encode_utf16_stack(text, &mut buf);
    font.width(slice, 1.0, 0.0)
}

#[allow(clippy::too_many_arguments)]
fn piece_quad(
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
    let tex = textures.get(name);
    if tex != 0 {
        batch.quad(m, [x0, y0, x1, y1], tex, uv, color, 0.0, 0);
    } else if name == "buttonright" || name == "headerbuttonright" {
        let fb = mul_color(fallback_color(name), color);
        let steps = 6;
        let flipped = uv[1] > uv[3];
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
    let h = y1 - y0;
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
        piece_quad(batch, textures, m, name, a, y0, b, y1, uv, color);
    }
}

/// Draws a 9-slice window frame centered on `(x, y)`.
/// Mirrors `frame` in `src/menus/themes/classic/draw.js` and `nineSlice` in
/// `src/interface/shapes.js`.
#[allow(clippy::too_many_arguments)]
fn draw_frame(
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

#[derive(Clone, Copy)]
struct RowsLayout<'a> {
    at: [f64; 2],
    x: f64,
    y: f64,
    width: f64,
    rot_x: f64,
    rot_y: f64,
    rot_z: f64,
    xyz_order: bool,
    font: &'a str,
    list_count: Option<usize>,
}

impl<'a> Default for RowsLayout<'a> {
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
    let font = fonts.get(layout.font);
    let yh = font.height;
    let y_spacing = 5.0;
    let p = yh + y_spacing;
    let n = rows.len();
    if n == 0 {
        return;
    }
    let shift = rows_shift(layout.width, p, layout.list_count.unwrap_or(n));
    let base = anchor(layout.at[0], layout.at[1], true, height);
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

    for (k, row) in rows.iter().enumerate() {
        let cy = y0 + (k as f64) * p;
        let focused = k == selected && !row.disabled;
        let a = if focused { pulse } else { 1.0 };
        let bar_color = if focused {
            tint_color(WHITE, a)
        } else {
            scaled_color(WHITE, DIMMED, a)
        };
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
fn draw_bottom_button(
    batch: &mut Batch,
    fonts: &ShellFonts,
    height: f64,
    place: &str,
    label: &str,
    focused: bool,
    enabled: bool,
    pulse: f64,
) {
    let (ax, ay, align) = match place {
        "accept" => (1.0, 1.0, 1),
        "middle" => (0.5, 1.0, 2),
        _ => (0.0, 1.0, 0),
    };
    let m = anchor(ax, ay, true, height);
    let x = if align == 0 {
        BUTTON_INSET
    } else if align == 1 {
        -BUTTON_INSET
    } else {
        0.0
    };
    let y = -12.0 - fonts.medium.caps_middle();
    let color = if !enabled {
        GREY
    } else if focused {
        selected_color(pulse)
    } else {
        WHITE
    };
    draw_str(&fonts.medium, batch, m, x, y, label, color, align);
}

/// Draws the Instant Action dual-list playlist screen.
/// Mirrors `instantAction` in `src/shell/screens.js`, `gamePicker` in
/// `src/shell/index.js`, and `paintList` in `src/menus/themes/classic/index.js`.
#[allow(clippy::too_many_arguments)]
fn draw_instant_action(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    app: &ShellApp,
    viewport_w: f64,
    viewport_h: f64,
    height: f64,
    pulse: f64,
) {
    // Top title: INSTANT ACTION
    let top_m = anchor(0.5, 0.0, true, height);
    draw_str(
        &fonts.large,
        batch,
        top_m,
        0.0,
        10.0,
        "INSTANT ACTION",
        WHITE,
        2,
    );

    // Column headings: AVAILABLE MAPS: + game picker (`BATTLEFRONT (2004)`
    // selected, `BATTLEFRONT II (2005), PLANNED` disabled) and SELECTED MAPS:
    let avail_hdr = anchor(0.25, 0.0, true, height);
    draw_str(
        &fonts.small,
        batch,
        avail_hdr,
        0.0,
        36.0,
        "AVAILABLE MAPS:",
        WHITE,
        2,
    );
    draw_str(
        &fonts.tiny,
        batch,
        avail_hdr,
        0.0,
        54.0,
        app.ia_selected_game_label(),
        WHITE,
        2,
    );
    let sel_hdr = anchor(0.75, 0.0, true, height);
    draw_str(
        &fonts.small,
        batch,
        sel_hdr,
        0.0,
        45.0,
        "SELECTED MAPS:",
        WHITE,
        2,
    );

    let box_w = 442.0;
    let box_h = (height - 210.0).clamp(160.0, 398.0);
    let box_y = -10.0;
    let pitch = 26.0;
    let inset = 15.0;
    let shown = (((box_h - 2.0 * inset) / pitch).floor() as usize).max(1);

    // Left list: AVAILABLE MAPS (side = -1, x = -227, rotY = 35)
    {
        let base = anchor(0.5, 0.5, true, height);
        let m = rotate_y(translate(base, -227.0, 0.0, 0.0), 35.0_f64.to_radians());
        draw_frame(
            batch,
            textures,
            m,
            0.0,
            box_y,
            box_w,
            box_h,
            "border_3_pieces",
            WHITE,
        );
        let count = app.data.maps.len();
        let scroll = count > shown;
        let first = if count > shown && app.ia_available_selected >= shown {
            (app.ia_available_selected + 1 - shown).min(count - shown)
        } else {
            0
        };
        let top = box_y - box_h * 0.5 + inset;
        let left = -box_w * 0.5 + 15.0;
        let inner = box_w - 30.0 - if scroll { 26.0 } else { 0.0 };
        let focused = app.ia_focus == IaFocus::Available;
        for k in 0..shown {
            let idx = first + k;
            let Some(map) = app.data.maps.get(idx) else {
                break;
            };
            let cy = top + (k as f64) * pitch + pitch * 0.5;
            if idx == app.ia_available_selected {
                let a = if focused {
                    cursor_pulse(app.pulse_elapsed)
                } else {
                    1.0
                };
                draw_frame(
                    batch,
                    textures,
                    m,
                    left + inner * 0.5,
                    cy,
                    inner + 6.0,
                    pitch + 10.0,
                    "listbox_cursor",
                    tint_color(WHITE, a),
                );
            }
            let disabled = map.extracted == Some(false);
            draw_str(
                &fonts.tiny,
                batch,
                m,
                left + 8.0,
                cy - fonts.tiny.caps_middle(),
                &map.name,
                if disabled { GREY } else { WHITE },
                0,
            );
        }
        if scroll {
            draw_scroll_bar(
                batch,
                textures,
                m,
                box_w * 0.5 - 34.0,
                top,
                (shown as f64) * pitch,
                first,
                shown,
                count,
            );
        }
    }

    // Right list: SELECTED MAPS (side = 1, x = 227, rotY = -35)
    {
        let base = anchor(0.5, 0.5, true, height);
        let m = rotate_y(translate(base, 227.0, 0.0, 0.0), (-35.0_f64).to_radians());
        draw_frame(
            batch,
            textures,
            m,
            0.0,
            box_y,
            box_w,
            box_h,
            "border_3_pieces",
            WHITE,
        );
        let count = app.ia_entries.len();
        let scroll = count > shown;
        let first = if count > shown && app.ia_selected_cursor >= shown {
            (app.ia_selected_cursor + 1 - shown).min(count - shown)
        } else {
            0
        };
        let top = box_y - box_h * 0.5 + inset;
        let left = -box_w * 0.5 + 15.0;
        let inner = box_w - 30.0 - if scroll { 26.0 } else { 0.0 };
        let focused = app.ia_focus == IaFocus::Selected;
        for k in 0..shown {
            let idx = first + k;
            let Some(entry) = app.ia_entries.get(idx) else {
                break;
            };
            let cy = top + (k as f64) * pitch + pitch * 0.5;
            if idx == app.ia_selected_cursor {
                let a = if focused {
                    cursor_pulse(app.pulse_elapsed)
                } else {
                    1.0
                };
                draw_frame(
                    batch,
                    textures,
                    m,
                    left + inner * 0.5,
                    cy,
                    inner + 6.0,
                    pitch + 10.0,
                    "listbox_cursor",
                    tint_color(WHITE, a),
                );
            }
            let map_name = app
                .data
                .maps
                .iter()
                .find(|m| m.id == entry.map)
                .map(|m| m.name.as_str())
                .unwrap_or(&entry.map);
            draw_str(
                &fonts.tiny,
                batch,
                m,
                left + 8.0,
                cy - fonts.tiny.caps_middle(),
                map_name,
                WHITE,
                0,
            );
            let icons = if entry.era.as_deref() == Some("cw") {
                ["rep_icon", "cis_icon"]
            } else {
                ["imp_icon", "all_icon"]
            };
            let size = 16.9;
            for (icon_i, icon_name) in icons.iter().enumerate() {
                let right = left + inner - ((icons.len() - icon_i - 1) as f64) * (size + 3.0);
                piece_quad(
                    batch,
                    textures,
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
        if scroll {
            draw_scroll_bar(
                batch,
                textures,
                m,
                box_w * 0.5 - 34.0,
                top,
                (shown as f64) * pitch,
                first,
                shown,
                count,
            );
        }
    }

    // Lower-right map preview still (`preview_rect` in `src/shell/media.js`)
    if let Some(map) = app.data.maps.get(app.ia_available_selected) {
        let preview_tex = textures.map_preview_for(&map.id);
        if preview_tex != 0 {
            let [px, py, pw, ph] = preview_rect(viewport_w, viewport_h);
            let sx = 1024.0 / viewport_w.max(1.0);
            let sy = height / viewport_h.max(1.0);
            let m = anchor(0.0, 0.0, false, height);
            let x0 = px * sx;
            let y0 = py * sy;
            let x1 = (px + pw) * sx;
            let y1 = (py + ph) * sy;
            draw_frame(
                batch,
                textures,
                m,
                (x0 + x1) * 0.5,
                (y0 + y1) * 0.5,
                (x1 - x0) + 12.0,
                (y1 - y0) + 12.0,
                "border_3_pieces",
                WHITE,
            );
            batch.quad(m, [x0, y0, x1, y1], preview_tex, UV, WHITE, 0.0, 0);
        }
    }

    // Horizontal buttons: ADD and REMOVE at y = 220 from center
    let mid_m = anchor(0.5, 0.5, false, height);
    let y_edit = 220.0 - fonts.medium.caps_middle();
    let add_color = if app.ia_focus == IaFocus::Add {
        selected_color(pulse)
    } else {
        WHITE
    };
    let rem_color = if app.ia_focus == IaFocus::Remove {
        selected_color(pulse)
    } else {
        WHITE
    };
    draw_str(&fonts.medium, batch, mid_m, -40.0, y_edit, "ADD", add_color, 2);
    draw_str(
        &fonts.medium,
        batch,
        mid_m,
        45.0,
        y_edit,
        "REMOVE",
        rem_color,
        2,
    );

    // Bottom buttons: BACK, PLAY IN ORDER / RANDOM ORDER, LAUNCH
    draw_bottom_button(
        batch,
        fonts,
        height,
        "back",
        app.say("common.back", "BACK"),
        app.ia_focus == IaFocus::Back,
        true,
        pulse,
    );
    let order_label = if app.ia_random {
        "RANDOM ORDER"
    } else {
        "PLAY IN ORDER"
    };
    draw_bottom_button(
        batch,
        fonts,
        height,
        "middle",
        order_label,
        app.ia_focus == IaFocus::Order,
        true,
        pulse,
    );
    draw_bottom_button(
        batch,
        fonts,
        height,
        "accept",
        "LAUNCH",
        app.ia_focus == IaFocus::Launch,
        !app.ia_entries.is_empty(),
        pulse,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_scroll_bar(
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
    let t0 = top + (first as f64 / count.max(1) as f64) * height;
    let t1 = (top + ((first + shown) as f64 / count.max(1) as f64) * height)
        .max(t0 + 6.0)
        .min(top + height);
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

/// Draws a modal popup (`Notice` `OK` or `ConfirmExit` `YES`/`NO`) or text input
/// modal (`Profile` callsign or `DirectConnect` address) without heap allocation.
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
    let m = anchor(0.5, 0.5, false, height);
    draw_frame(
        batch,
        textures,
        m,
        0.0,
        0.0,
        360.0,
        190.0,
        "border_3_pieces",
        WHITE,
    );

    let mut y = -68.0;
    for line in message.lines() {
        draw_str(&fonts.medium, batch, m, 0.0, y, line, WHITE, 2);
        y += fonts.medium.height;
    }
    if let Some(extra) = extra_line {
        draw_str(&fonts.medium, batch, m, 0.0, y, extra, WHITE, 2);
    }

    let count = buttons.len();
    let by = 63.0 - fonts.medium.caps_middle();
    for (i, &(label, focused)) in buttons.iter().enumerate() {
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

/// Populates `batch` with the current shell screen's geometry for a viewport of
/// `(viewport_w, viewport_h)` pixels.
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
    batch.begin();
    let (_, gate_y, height) = gate::gate_layout(viewport_w, viewport_h);
    let active = app.current_screen();
    let base_screen = app.stack.last().copied().unwrap_or(ScreenId::Main);

    // 1. Full-screen poster backdrop (`1024 x height`).
    let bg_tex = textures.backdrop_for(base_screen);
    let full_m = anchor(0.0, 0.0, false, height);
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
                RowsLayout {
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
                RowsLayout {
                    width: 650.0,
                    font: "gamefont_large",
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
        ScreenId::Options
        | ScreenId::OptionsPage(_)
        | ScreenId::Profile
        | ScreenId::Multiplayer
        | ScreenId::MultiplayerList(_) => {
            let rows = app.active_rows();
            let sel = app.selected_cached_row_index(base_screen);
            let width = match base_screen {
                ScreenId::MultiplayerList(_) | ScreenId::OptionsPage(_) => 620.0,
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
        }
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
        ScreenId::HostDialog => {
            draw_popup_box(
                batch,
                fonts,
                textures,
                height,
                "HOST GAME",
                None,
                &[("LAUNCH", true), ("CANCEL", false)],
                pulse,
            );
        }
        ScreenId::Gate | ScreenId::Popup => {}
    }

    // 4. Profile editing modal or active popup over the current screen.
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
        }
    }
}

/// Populates `batch` with the loading screen's stage picture, zoom crop,
/// tracking crosshairs, top-left planet name, rotating `LOADING` disk, and
/// 28-LED ring (`src/shell/loading.js` `LoadingView.prototype.draw`).
///
/// Performs zero heap allocations per frame.
#[allow(clippy::too_many_arguments)]
pub fn draw_loading(
    batch: &mut Batch,
    fonts: &ShellFonts,
    textures: &ShellTextures,
    state: &TimelineAt,
    spin: f64,
    planet: &str,
    loading_text: &str,
    look: LookId,
    viewport_w: f64,
    viewport_h: f64,
) {
    batch.begin();
    let w = viewport_w.max(1.0);
    let h = viewport_h.max(1.0);
    let height = 768.0 + (h / w - 0.75) * 768.0;
    let full_m = anchor(0.0, 0.0, false, height);
    let sx = 1024.0 / w;
    let sy = height / h;

    // 1. Black clear quad + stage picture (zooming if in `Zoom` phase) +
    //    next stage picture growing out of its target rectangle + tracking lines.
    batch.quad(
        full_m,
        [0.0, 0.0, 1024.0, height],
        textures.solid,
        UV,
        BLACK,
        0.0,
        0,
    );

    let shown = textures.loading_stages.get(state.stage).copied();
    let next = textures.loading_stages.get(state.stage + 1).copied();

    if let Some(stage_pic) = shown.filter(|s| s.texture != 0) {
        let iw = stage_pic.width.max(1.0);
        let ih = stage_pic.height.max(1.0);
        let view = match (state.zoom, stage_pic.target) {
            (Some(u), Some(target)) => zoom_view(u, iw, ih, target),
            _ => [0.0, 0.0, iw, ih],
        };
        let uv = [
            view[0] / iw,
            view[1] / ih,
            (view[0] + view[2]) / iw,
            (view[1] + view[3]) / ih,
        ];
        batch.quad(
            full_m,
            [0.0, 0.0, 1024.0, height],
            stage_pic.texture,
            uv,
            WHITE,
            0.0,
            0,
        );

        if let (Some(target), Some(lines)) = (stage_pic.target, state.lines) {
            let [x, y, rw, rh] = on_screen(target, view, w, h);
            if let (Some(u), Some(next_pic)) = (state.zoom, next.filter(|s| s.texture != 0)) {
                let a = (u / 0.2).clamp(0.0, 1.0);
                batch.quad(
                    full_m,
                    [x * sx, y * sy, (x + rw) * sx, (y + rh) * sy],
                    next_pic.texture,
                    UV,
                    tint_color(WHITE, a),
                    0.0,
                    0,
                );
            }

            // Tracking lines: 2 interface units thick (`src/shell/loading.js`).
            let line_w = (2.0 * w / loading::IW).max(1.5);
            let y_top = y * lines.y;
            let y_bot = h - (h - (y + rh)) * lines.y;
            draw_tracking_line(batch, textures, full_m, true, y_top, line_w, w, h, sx, sy);
            draw_tracking_line(batch, textures, full_m, true, y_bot, line_w, w, h, sx, sy);

            if let Some(lx) = lines.x {
                let x_left = x * lx;
                let x_right = w - (w - (x + rw)) * lx;
                draw_tracking_line(batch, textures, full_m, false, x_left, line_w, w, h, sx, sy);
                draw_tracking_line(
                    batch, textures, full_m, false, x_right, line_w, w, h, sx, sy,
                );
            }
        }
    } else {
        let backdrop = textures.get("iface_bgmeta_space");
        if backdrop != 0 {
            batch.quad(
                full_m,
                [0.0, 0.0, 1024.0, height],
                backdrop,
                UV,
                WHITE,
                0.0,
                0,
            );
        }
    }

    // 2. Top-left planet name (`NAME.x = 51.2`, `top = NAME.top * H + NAME.y`).
    if state.phase != loading::LoadingPhase::Hold || shown.is_none() {
        let top = name_pos::TOP * height + name_pos::Y;
        draw_str(
            &fonts.tiny,
            batch,
            full_m,
            name_pos::X,
            top,
            planet,
            WHITE,
            0,
        );
    }

    // 3. Lower-right rotating disk, 28-LED ring, and tilted `LOADING` text
    //    (`drawDisk` in `src/shell/loading.js`).
    let dp = disk_place(w, h);
    let cx = dp.x * sx;
    let cy = dp.y * sy;
    let disk_base = translate(full_m, cx, cy, 0.0);
    // Horizontal squash `DISK.turn = 0.866`.
    let mut squash = IDENTITY;
    squash[0] = disk::TURN;
    let disk_m = mul(disk_base, squash);

    let disk_tex = textures.get("load_rotating_disk");
    let disk_tint = match look {
        LookId::Bf1 => [0.15, 0.72, 0.96, 1.0],
        LookId::Eternal => [0.77, 0.80, 0.85, 0.85],
        LookId::Bf2 => [0.22, 0.45, 0.78, 0.90],
    };
    for &[size, speed, alpha] in &disk::LAYERS {
        let layer_m = rotate_z(disk_m, spin * speed);
        let half = size * 0.5;
        let tex = if disk_tex != 0 {
            disk_tex
        } else {
            textures.solid
        };
        let c = tint_color(disk_tint, alpha);
        batch.quad(layer_m, [-half, -half, half, half], tex, UV, c, 0.0, 0);
    }

    // 28 LEDs around the ring at radius `DISK.ring = 52`.
    let stops: &[&str] = match look {
        LookId::Bf1 => &["#0b0d0f", "#5c6166", "#b9bcbe"],
        LookId::Eternal => &["#15100c", "#4a2407", "#b5560b", "#ff9a1f", "#ffd27a"],
        LookId::Bf2 => &["#101826", "#3d0c08", "#a5180e", "#e0362a", "#ff6450"],
    };
    let led_tex = {
        let t = textures.get("load_led_0");
        if t != 0 {
            t
        } else {
            textures.solid
        }
    };
    let head = spin * loading::lights::SPEED;
    for i in 0..disk::LEDS {
        let angle = (i as f64 / disk::LEDS as f64) * std::f64::consts::TAU;
        let led_m = translate(rotate_z(disk_m, angle), disk::RING, 0.0, 0.0);
        if look == LookId::Bf2 {
            batch.quad(
                led_m,
                [-4.5, -4.5, 4.5, 4.5],
                textures.solid,
                UV,
                [6.0 / 255.0, 8.0 / 255.0, 12.0 / 255.0, 1.0],
                0.0,
                0,
            );
        }
        let level = light_level(head - (i as f64 / disk::LEDS as f64));
        let [r, g, b] = mix_color_rgb(stops, level);
        batch.quad(
            led_m,
            [-3.5, -3.5, 3.5, 3.5],
            led_tex,
            UV,
            [r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, 1.0],
            0.0,
            0,
        );
    }

    // Tilted `LOADING` text across the center (`DISK.text_deg = -14.0`).
    let text_m = rotate_z(disk_m, disk::TEXT_DEG.to_radians());
    let text_w = str_width(&fonts.medium, loading_text);
    let _ = text_w;
    draw_str(
        &fonts.medium,
        batch,
        text_m,
        0.0,
        -12.0,
        loading_text,
        [220.0 / 255.0, 220.0 / 255.0, 1.0, 1.0],
        2,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_tracking_line(
    batch: &mut Batch,
    textures: &ShellTextures,
    m: Matrix,
    across: bool,
    at: f64,
    width: f64,
    w: f64,
    h: f64,
    sx: f64,
    sy: f64,
) {
    let edge_tex = if across {
        textures.get("load_edge_horz")
    } else {
        textures.get("load_edge_vert")
    };
    if edge_tex != 0 {
        if across {
            batch.quad(
                m,
                [
                    0.0,
                    (at - width * 4.0) * sy,
                    w * sx,
                    (at + width * 4.0) * sy,
                ],
                edge_tex,
                UV,
                WHITE,
                0.0,
                0,
            );
        } else {
            batch.quad(
                m,
                [
                    (at - width * 4.0) * sx,
                    0.0,
                    (at + width * 4.0) * sx,
                    h * sy,
                ],
                edge_tex,
                UV,
                WHITE,
                0.0,
                0,
            );
        }
    } else if across {
        batch.quad(
            m,
            [
                0.0,
                (at - width * 0.5) * sy,
                w * sx,
                (at + width * 0.5) * sy,
            ],
            textures.solid,
            UV,
            TRACK_LINE_COLOR,
            0.0,
            0,
        );
    } else {
        batch.quad(
            m,
            [
                (at - width * 0.5) * sx,
                0.0,
                (at + width * 0.5) * sx,
                h * sy,
            ],
            textures.solid,
            UV,
            TRACK_LINE_COLOR,
            0.0,
            0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ShellData;
    use crate::launch::GameFamily;
    use crate::loading::LoadingTimeline;

    #[test]
    fn rows_shift_and_row_bar_at_match_classic_theme() {
        // 7 rows on MAIN MENU with width 250, pitch 29:
        let s = rows_shift(250.0, 29.0, 7);
        assert!((s - (-168.5)).abs() < 1e-6);

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
