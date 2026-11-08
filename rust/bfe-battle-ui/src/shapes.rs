//! Classic interface geometry, 9-slice containers, tilted plane projection,
//! word wrapping, flashy title bar intro, and `GameFamily` selector (`src/interface/shapes.js`,
//! `src/interface/space.js`, `src/interface/surface.js`, `src/interface/flashy.js`,
//! `src/menus/themes/classic/draw.js`, `MULTIPLAYER.md`).
//!
//! All battle UI screens (spawn screen, scoreboard, pause menu) share the
//! 1024xH interface plane at `z = -PLANE` (`-886.81`) viewed from the origin
//! through a 60-degree horizontal (`46.8` deg vertical) perspective camera.
//! Tilted containers rotate around Y or X in local anchor space and project
//! back onto the `1024xH` interface plane via [`plane_point`] and [`inside_quad`].

use std::fmt;

use bfe_render::interface::{
    alpha, anchor, mul, rotate_y, rotate_z, translate, Batch, Color, Font, Matrix, IDENTITY, UV,
};

pub use crate::surface::*;

/// Which game family a map, its rules, and its in-battle interface belong to (`MULTIPLAYER.md`).
///
/// Today only [`GameFamily::Swbf1`] (2004) is implemented; requesting [`GameFamily::Swbf2`] (2005)
/// from any layout or controller constructor returns an explicit [`GameFamilyError`] and never
/// silently falls back to the 2004 layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GameFamily {
    /// Star Wars Battlefront (2004) (`swbf1`).
    #[default]
    Swbf1,
    /// Star Wars Battlefront II (2005) (`swbf2`).
    Swbf2,
}

impl GameFamily {
    /// Canonical short identifier (`"swbf1"` or `"swbf2"`).
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            GameFamily::Swbf1 => "swbf1",
            GameFamily::Swbf2 => "swbf2",
        }
    }

    /// Parses a family identifier (`"swbf1"`, `"swbf2"`, or `"swbf2/hot1"` prefix).
    pub fn parse(id: &str) -> Option<Self> {
        let prefix = id.split('/').next().unwrap_or(id).trim();
        if prefix.eq_ignore_ascii_case("swbf1") || prefix.is_empty() {
            Some(GameFamily::Swbf1)
        } else if prefix.eq_ignore_ascii_case("swbf2") {
            Some(GameFamily::Swbf2)
        } else {
            None
        }
    }

    /// Verifies that this game family's in-battle UI is implemented, returning
    /// [`GameFamilyError`] for [`GameFamily::Swbf2`].
    #[inline]
    pub const fn require_available(self) -> Result<(), GameFamilyError> {
        match self {
            GameFamily::Swbf1 => Ok(()),
            GameFamily::Swbf2 => Err(GameFamilyError::not_available(self)),
        }
    }
}

impl fmt::Display for GameFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a caller requests an unimplemented [`GameFamily`] (such as [`GameFamily::Swbf2`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameFamilyError {
    pub family: GameFamily,
}

impl GameFamilyError {
    /// Constructs a "not available" error for `family`.
    #[inline]
    pub const fn not_available(family: GameFamily) -> Self {
        Self { family }
    }
}

impl fmt::Display for GameFamilyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "game family {} is not available", self.family.as_str())
    }
}

impl std::error::Error for GameFamilyError {}

/// Fixed horizontal width of the Classic interface plane (`src/interface/space.js` `IW = 1024`).
pub const IW: f64 = 1024.0;

/// Alias for [`IW`] (`1024.0`) matching `W` in `src/scoreboard/classic/layout.js`.
pub const W: f64 = IW;

/// Base 4:3 interface height (`768.0`).
pub const H_4_3: f64 = 768.0;

/// Horizontal field of view in degrees (`60.0`, `src/interface/space.js` `FOV`).
pub const FOV_DEG: f64 = 60.0;

/// Distance from the interface camera to the `1024xH` interface plane
/// (`1024 / (2 * tan(PI / 6)) = 886.8100134752651`, `src/interface/space.js` `PLANE`).
pub const PLANE: f64 = 886.810_013_475_265_1;

/// Safe-area inset fraction (`0.05` on each side, leaving `0.9` in the middle;
/// `src/interface/space.js` `SAFE`, `SAFE_SCALE`).
pub const SAFE: f64 = 0.05;
pub const SAFE_SCALE: f64 = 0.9;

/// Scale factor of the `shell_cube` hologram room (`1.6 / 0.009`,
/// `src/interface/models.js` `ROOM_SCALE`).
pub const ROOM_SCALE: f64 = 1.6 / 0.009;

/// Alias for [`ROOM_SCALE`].
pub const CUBE_UNITS: f64 = ROOM_SCALE;

/// Scale factor per unit of a spinning faction logo on the interface plane
/// (`src/interface/models.js` `LOGO_UNITS = 221.3`).
pub const LOGO_UNITS: f64 = 221.3;

/// Scale factor per meter of a standing soldier body on the interface plane
/// (`0.936 / (0.006 / (1024 / 800)) = 199.68`, `src/interface/models.js` `BODY_UNITS`).
pub const BODY_UNITS: f64 = 0.936 / (0.006 / (1024.0 / 800.0));

/// Alias for [`BODY_UNITS`].
pub const PREVIEW_UNITS: f64 = BODY_UNITS;

/// Scale factor per meter of a standing droideka body (`0.9 * BODY_UNITS`,
/// `src/interface/models.js` `DROIDEKA_UNITS`).
pub const DROIDEKA_UNITS: f64 = 0.9 * BODY_UNITS;

/// Standard RGBA color constants (`src/menus/themes/classic/draw.js`).
pub const WHITE: Color = [1.0, 1.0, 1.0, 1.0];
pub const BLACK: Color = [0.0, 0.0, 0.0, 1.0];
pub const GREY: Color = [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0];
pub const DIMMED: f64 = 150.0 / 255.0;

/// Converts 0..255 RGBA bytes to `[f64; 4]` (`rgba` in `src/menus/themes/classic/draw.js`).
#[inline]
pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    [
        r as f64 / 255.0,
        g as f64 / 255.0,
        b as f64 / 255.0,
        a as f64 / 255.0,
    ]
}

/// Multiplies a color's alpha by `a` (`tint` in `src/menus/themes/classic/draw.js`).
#[inline]
pub const fn tint(color: Color, a: f64) -> Color {
    [color[0], color[1], color[2], color[3] * a]
}

/// Scales a color's RGB channels by `k` (`scaled` in `src/menus/themes/classic/draw.js`).
#[inline]
pub const fn scaled(color: Color, k: f64) -> Color {
    [color[0] * k, color[1] * k, color[2] * k, color[3]]
}

/// Focused text yellow color whose green follows `a` (`selectedColor` in `src/menus/themes/classic/draw.js`).
#[inline]
pub fn selected_color(a: f64) -> Color {
    let g = (1.2 * a * 255.0 + 50.0).clamp(100.0, 255.0);
    [1.0, g / 255.0, 100.0 / 255.0, a]
}

/// Computes the interface height `H` for a window `w x h` pixels
/// (`src/interface/space.js` `interfaceHeight`).
///
/// At 4:3 (`w = 1024, h = 768`), `H = 768`; at 16:9 (`w = 1280, h = 720`), `H = 576`.
#[inline]
pub fn interface_height(w: f64, h: f64) -> f64 {
    (768.0 + (h / w.max(1.0) - 0.75) * 768.0).floor()
}

/// Maps canvas pixel coordinates `(client_x, client_y)` from the top-left corner
/// into `1024xH` interface units (`src/interface/surface.js` `toInterface`).
#[inline]
pub fn to_interface(
    client_x: f64,
    client_y: f64,
    client_w: f64,
    client_h: f64,
    height: f64,
) -> [f64; 2] {
    [
        (client_x / client_w.max(1.0)) * IW,
        (client_y / client_h.max(1.0)) * height,
    ]
}

/// Multiplies `a` by a rotation of `angle` radians around the X axis
/// (`THREE.Matrix4.makeRotationX`, used by the spawn screen's 45-degree `SPAWN` button).
#[inline]
pub fn rotate_x(a: Matrix, angle: f64) -> Matrix {
    let (s, c) = (angle.sin(), angle.cos());
    mul(
        a,
        [
            1.0, 0.0, 0.0, 0.0, // col 0
            0.0, c, s, 0.0, // col 1
            0.0, -s, c, 0.0, // col 2
            0.0, 0.0, 0.0, 1.0, // col 3
        ],
    )
}

/// Builds a rotation matrix in `YXZ` Euler order (`rotation` in `src/menus/themes/classic/index.js`).
#[inline]
pub fn make_rotation_yxz(rx: f64, ry: f64, rz: f64) -> Matrix {
    let m = rotate_y(IDENTITY, ry);
    let m = rotate_x(m, rx);
    rotate_z(m, rz)
}

/// Multiplies `a` by a non-uniform scale `(sx, sy, sz)` (`THREE.Matrix4.makeScale`).
#[inline]
pub fn scale_xyz(a: Matrix, sx: f64, sy: f64, sz: f64) -> Matrix {
    mul(
        a,
        [
            sx, 0.0, 0.0, 0.0, // col 0
            0.0, sy, 0.0, 0.0, // col 1
            0.0, 0.0, sz, 0.0, // col 2
            0.0, 0.0, 0.0, 1.0, // col 3
        ],
    )
}

/// Projects a local point `(x, y, z)` under matrix `m` onto the `1024xH` interface
/// plane (`src/interface/space.js` `planePoint`).
#[inline]
pub fn plane_point(m: Matrix, x: f64, y: f64, z: f64, height: f64) -> [f64; 2] {
    let px = m[0] * x + m[4] * y + m[8] * z + m[12];
    let py = m[1] * x + m[5] * y + m[9] * z + m[13];
    let pz = m[2] * x + m[6] * y + m[10] * z + m[14];
    let k = PLANE / (-pz).max(1e-3);
    [px * k + IW * 0.5, (384.0 - py * k) * (height / 768.0)]
}

/// Alias for [`plane_point`] at `z = 0.0` (`s.toPlane(m, x, y)` in `src/interface/surface.js`).
#[inline]
pub fn to_plane_point(m: &Matrix, x: f64, y: f64, height: f64) -> [f64; 2] {
    plane_point(*m, x, y, 0.0, height)
}

/// Projects the four corners of `(x0, y0)..(x1, y1)` under `m` onto the interface
/// plane (`src/menus/themes/classic/draw.js` `corners`).
#[inline]
pub fn projected_corners(
    m: Matrix,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    height: f64,
) -> [[f64; 2]; 4] {
    [
        plane_point(m, x0, y0, 0.0, height),
        plane_point(m, x1, y0, 0.0, height),
        plane_point(m, x1, y1, 0.0, height),
        plane_point(m, x0, y1, 0.0, height),
    ]
}

/// Point-in-quad test for a convex quad `corners` in either winding order
/// (`src/interface/shapes.js` `insideQuad`).
#[inline]
pub fn inside_quad(x: f64, y: f64, corners: &[[f64; 2]; 4]) -> bool {
    let mut sign = 0.0;
    for i in 0..4 {
        let a = corners[i];
        let b = corners[(i + 1) & 3];
        let cross = (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
        if cross == 0.0 {
            continue;
        }
        let s = cross.signum();
        if sign == 0.0 {
            sign = s;
        } else if s != sign {
            return false;
        }
    }
    true
}

/// Places a 3D interface model at interface point `(x, y)` in the `InterfaceCamera`'s
/// world space (`src/interface/models.js` `InterfaceCamera.place`).
#[inline]
pub fn place_interface_model(x: f64, y: f64, aspect: f64, height: f64) -> [f64; 3] {
    [
        (x - 512.0) * 0.75 * aspect,
        (height * 0.5 - y) * (768.0 / height.max(1.0)),
        -PLANE,
    ]
}

/// Computes the 9-slice pieces `[x0, y0, x1, y1, u0, v0, u1, v1]` of a `w x h`
/// rectangle centered on `(0, 0)` with `corner`-unit corners and texture cuts
/// at `0.25` and `0.75` (`src/interface/shapes.js` `nineSlice`).
///
/// Returns `(pieces, len)` on the stack with zero heap allocations. A side under
/// `2 * corner` collapses its middle row or column.
pub fn nine_slice_pieces(w: f64, h: f64, corner: f64) -> ([[f64; 8]; 9], usize) {
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
    let mut out = [[0.0; 8]; 9];
    let mut len = 0usize;
    for j in 0..3 {
        for i in 0..3 {
            if xs[i + 1] <= xs[i] || ys[j + 1] <= ys[j] {
                continue;
            }
            out[len] = [
                xs[i],
                ys[j],
                xs[i + 1],
                ys[j + 1],
                us[i],
                us[j],
                us[i + 1],
                us[j + 1],
            ];
            len += 1;
        }
    }
    (out, len)
}

/// Convenience wrapper returning a `Vec<[f64; 8]>` of 9-slice pieces (`nineSlice` in `src/interface/shapes.js`).
pub fn nine_slice(w: f64, h: f64, corner: f64) -> Vec<[f64; 8]> {
    let (pieces, len) = nine_slice_pieces(w, h, corner);
    pieces[..len].to_vec()
}

/// Draws a 9-sliced texture centered at `(cx, cy)` with `corner`-unit corners.
#[allow(clippy::too_many_arguments)]
pub fn draw_nine_slice(
    batch: &mut Batch,
    m: Matrix,
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
    corner: f64,
    texture: u32,
    color: Color,
) {
    let (pieces, len) = nine_slice_pieces(w, h, corner);
    for p in &pieces[..len] {
        batch.quad(
            m,
            [cx + p[0], cy + p[1], cx + p[2], cy + p[3]],
            texture,
            [p[4], p[5], p[6], p[7]],
            color,
            0.0,
            0,
        );
    }
}

/// Draws a 9-sliced window centered at `(cx, cy)` under `m` into `batch`,
/// falling back to a solid navy rectangle if `texture == 0`
/// (`src/spawn-screen/classic/view.js` `drawBackground`, `src/scoreboard/classic/view.js` `window`).
#[allow(clippy::too_many_arguments)]
pub fn draw_nine_slice_window(
    batch: &mut Batch,
    m: Matrix,
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
    texture: u32,
    solid_texture: u32,
    color: Color,
) {
    if texture == 0 {
        batch.quad(
            m,
            [
                cx - w * 0.5 + 4.0,
                cy - h * 0.5 + 4.0,
                cx + w * 0.5 - 4.0,
                cy + h * 0.5 - 4.0,
            ],
            solid_texture,
            UV,
            [0.0, 0.17, 0.38, 0.7 * color[3]],
            0.0,
            0,
        );
        return;
    }
    draw_nine_slice(batch, m, cx, cy, w, h, 16.0, texture, color);
}

/// Returns how far below a line's top the deepest glyph in `font` reaches
/// (`src/interface/fonts.js` `Fonts.glyphHeight`).
pub fn glyph_height(font: &Font) -> f64 {
    let mut bottom = font.height;
    for g in font.glyphs.values() {
        if g.bounds[0] < g.bounds[2] {
            bottom = bottom.max(g.bounds[3]);
        }
    }
    bottom
}

/// Breaks `&str` `text` into lines that fit within `max_width` using `measure` (`wrapLines` in `src/interface/shapes.js`).
pub fn wrap_lines(text: &str, max_width: f64, mut measure: impl FnMut(&str) -> f64) -> Vec<String> {
    let mut out = Vec::new();
    for raw_para in text.split('\n') {
        let para = raw_para.trim_end_matches('\r');
        let mut line = String::new();
        for word in para.split_whitespace() {
            if line.is_empty() {
                line.push_str(word);
            } else {
                let candidate = format!("{} {}", line, word);
                if measure(&candidate) > max_width {
                    out.push(line);
                    line = word.to_string();
                } else {
                    line = candidate;
                }
            }
        }
        out.push(line);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Breaks UTF-16 `text` into lines that fit within `max_width` using `font.width`,
/// writing `(start, end)` ranges of `text` into `out_ranges` without heap allocation
/// (`src/interface/shapes.js` `wrapLines`).
pub fn wrap_lines_ranges(
    font: &Font,
    text: &[u16],
    max_width: f64,
    scale: f64,
    out_ranges: &mut Vec<(usize, usize)>,
) {
    out_ranges.clear();
    if text.is_empty() {
        out_ranges.push((0, 0));
        return;
    }
    let mut para_start = 0usize;
    while para_start <= text.len() {
        let mut para_end = para_start;
        while para_end < text.len()
            && text[para_end] != b'\n' as u16
            && text[para_end] != b'\r' as u16
        {
            para_end += 1;
        }
        wrap_paragraph(font, text, para_start, para_end, max_width, scale, out_ranges);
        if para_end >= text.len() {
            break;
        }
        if text[para_end] == b'\r' as u16
            && para_end + 1 < text.len()
            && text[para_end + 1] == b'\n' as u16
        {
            para_start = para_end + 2;
        } else {
            para_start = para_end + 1;
        }
    }
}

fn wrap_paragraph(
    font: &Font,
    text: &[u16],
    start: usize,
    end: usize,
    max_width: f64,
    scale: f64,
    out_ranges: &mut Vec<(usize, usize)>,
) {
    let mut line_start = 0usize;
    let mut line_end = 0usize;
    let mut has_line = false;
    let mut i = start;
    while i < end {
        while i < end && (text[i] == b' ' as u16 || text[i] == b'\t' as u16) {
            i += 1;
        }
        if i >= end {
            break;
        }
        let word_start = i;
        while i < end && text[i] != b' ' as u16 && text[i] != b'\t' as u16 {
            i += 1;
        }
        let word_end = i;
        if !has_line {
            line_start = word_start;
            line_end = word_end;
            has_line = true;
        } else {
            let candidate_w = font.width(&text[line_start..word_end], scale, 1.0);
            if candidate_w > max_width {
                out_ranges.push((line_start, line_end));
                line_start = word_start;
                line_end = word_end;
            } else {
                line_end = word_end;
            }
        }
    }
    if has_line {
        out_ranges.push((line_start, line_end));
    } else {
        out_ranges.push((start, start));
    }
}

/// Stage durations of the 2004 interface's flashy text intro (`src/interface/flashy.js`
/// `INTRO = [0.1, 0.05, 0.05, 0.15, 0.15]`, total `0.5s`).
pub const INTRO: [f64; 5] = [0.1, 0.05, 0.05, 0.15, 0.15];
pub const ENDS: [f64; 5] = [0.1, 0.15, 0.2, 0.35, 0.5];
pub const INTRO_LENGTH: f64 = 0.5;

/// Sweeping white cursor block state during stages 2 and 3 of [`TitleIntro`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashyBlock {
    pub x: f64,
    pub alpha: f64,
}

/// Instantaneous frame state of a [`TitleIntro`] (`TitleIntro.frame` in `src/interface/flashy.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashyFrame {
    pub bar: f64,
    pub block: Option<FlashyBlock>,
    pub text: f64,
    pub alpha: f64,
}

impl Default for FlashyFrame {
    fn default() -> Self {
        Self {
            bar: 1.0,
            block: None,
            text: 1.0,
            alpha: 1.0,
        }
    }
}

/// Alias for [`FlashyFrame`].
pub type IntroFrame = FlashyFrame;

/// The 5-stage flashy text/bar intro (`src/interface/flashy.js` `TitleIntro`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleIntro {
    pub time: f64,
}

impl Default for TitleIntro {
    fn default() -> Self {
        Self::new()
    }
}

impl TitleIntro {
    #[inline]
    pub const fn new() -> Self {
        Self { time: INTRO_LENGTH }
    }

    /// Starts the intro after `delay` seconds (`TitleIntro.start`).
    #[inline]
    pub fn start(&mut self, delay: f64) {
        self.time = -delay;
    }

    /// Advances the intro clock by `dt` seconds (`TitleIntro.update`).
    #[inline]
    pub fn update(&mut self, dt: f64) {
        if self.time < INTRO_LENGTH {
            self.time = (self.time + dt).min(INTRO_LENGTH);
        }
    }

    /// Whether the intro has completed all 5 stages (`TitleIntro.done`).
    #[inline]
    pub fn done(&self) -> bool {
        self.time >= INTRO_LENGTH
    }

    /// Computes the current bar, block, vertical text scale, and text alpha (`TitleIntro.frame`).
    pub fn frame(&self) -> FlashyFrame {
        let t = self.time;
        let at = |stage: usize| -> f64 {
            let prev = if stage == 0 { 0.0 } else { ENDS[stage - 1] };
            ((t - prev) / INTRO[stage]).clamp(0.0, 1.0)
        };
        if t < 0.0 {
            FlashyFrame {
                bar: 0.0,
                block: None,
                text: 0.0,
                alpha: 0.0,
            }
        } else if t < ENDS[0] {
            FlashyFrame {
                bar: at(0),
                block: None,
                text: 0.0,
                alpha: 0.0,
            }
        } else if t < ENDS[1] {
            FlashyFrame {
                bar: 1.0,
                block: Some(FlashyBlock {
                    x: 0.0,
                    alpha: at(1),
                }),
                text: 0.0,
                alpha: 0.0,
            }
        } else if t < ENDS[2] {
            FlashyFrame {
                bar: 1.0,
                block: Some(FlashyBlock {
                    x: at(2),
                    alpha: 1.0,
                }),
                text: 0.0,
                alpha: 0.0,
            }
        } else if t < ENDS[3] {
            FlashyFrame {
                bar: 1.0,
                block: None,
                text: at(3),
                alpha: 1.0,
            }
        } else if t < ENDS[4] {
            FlashyFrame {
                bar: 1.0,
                block: None,
                text: 1.0,
                alpha: at(4),
            }
        } else {
            FlashyFrame {
                bar: 1.0,
                block: None,
                text: 1.0,
                alpha: 1.0,
            }
        }
    }
}

/// Draws text with its line top at `y`, playing an optional [`TitleIntro`] and
/// narrowing horizontally by `scale` while stretching vertically back to full
/// line height (`src/scoreboard/classic/view.js` `drawText` and
/// `src/menus/themes/classic/draw.js` `flashyText`).
#[allow(clippy::too_many_arguments)]
pub fn draw_flashy_text(
    batch: &mut Batch,
    font: &Font,
    m: Matrix,
    x: f64,
    y: f64,
    text: &[u16],
    color: Color,
    align: u32,
    intro: Option<&TitleIntro>,
    scale: f64,
    extra_alpha: f64,
    cursor_or_solid_tex: u32,
) {
    if text.is_empty() || scale <= 0.0 {
        return;
    }
    let fh = font.height;
    let width = font.width(text, scale, 1.0);
    let st = intro.map_or(FlashyFrame::default(), TitleIntro::frame);
    let left = match align {
        1 => x - width,
        2 => x - width * 0.5,
        _ => x,
    };
    if let Some(block) = st.block {
        let bx = left + block.x * (width - fh * scale).max(0.0);
        batch.quad(
            m,
            [bx, y, bx + fh * scale, y + fh],
            cursor_or_solid_tex,
            UV,
            [1.0, 1.0, 1.0, block.alpha * extra_alpha],
            0.0,
            0,
        );
    }
    if st.text <= 0.01 || st.alpha <= 0.0 {
        return;
    }
    let mid = y + fh * 0.5;
    let at = translate(
        scale_xyz(translate(m, x, mid, 0.0), 1.0, st.text / scale, 1.0),
        0.0,
        (-fh * 0.5) * scale,
        0.0,
    );
    font.draw(
        batch,
        at,
        0.0,
        0.0,
        text,
        alpha(color, st.alpha * extra_alpha),
        align,
        scale,
        1.0,
        0.0,
    );
}

/// Encodes a Rust `&str` as UTF-16 code units into `out`, optionally uppercasing
/// ASCII letters (`a..=z` -> `A..=Z`), clearing `out` first so pre-allocated
/// `Vec<u16>` buffers never allocate on the heap during a frame.
#[inline]
pub fn encode_utf16_into(s: &str, uppercase: bool, out: &mut Vec<u16>) {
    out.clear();
    for u in s.encode_utf16() {
        let c = if uppercase && (b'a' as u16..=b'z' as u16).contains(&u) {
            u - 32
        } else {
            u
        };
        out.push(c);
    }
}

/// Formats a signed integer `n` as UTF-16 digits into `out` without heap allocation.
pub fn format_i64_utf16_into(n: i64, out: &mut Vec<u16>) {
    out.clear();
    if n == 0 {
        out.push(b'0' as u16);
        return;
    }
    let neg = n < 0;
    let mut mag = if neg {
        n.wrapping_neg() as u64
    } else {
        n as u64
    };
    let mut digits = [0u16; 20];
    let mut len = 0usize;
    while mag > 0 {
        digits[len] = b'0' as u16 + (mag % 10) as u16;
        mag /= 10;
        len += 1;
    }
    if neg {
        out.push(b'-' as u16);
    }
    for i in (0..len).rev() {
        out.push(digits[i]);
    }
}

/// Helper returning `anchor(0.0, 0.0, false, height)`.
#[inline]
pub fn root_anchor(height: f64) -> Matrix {
    anchor(0.0, 0.0, false, height)
}

/// Helper returning `IDENTITY`.
#[inline]
pub const fn identity_matrix() -> Matrix {
    IDENTITY
}
