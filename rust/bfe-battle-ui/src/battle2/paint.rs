use super::Caption;
use crate::BattleUiSurface;
use bfe_render::{
    hud_model2::{catalog::Catalog, Widget, PLANE},
    interface::{anchor, Color, Font, Matrix, QUADS, UV},
};
use std::fmt::{Arguments, Write};

pub const WHITE: Color = [0.95, 0.96, 0.98, 1.];

/// One model unit of a HUD model, at a `Scale` of one, covers a fifth of the picture's height
/// (measured on the 2005 game's frames: the infinity disc, the reticle and the weapon
/// backdrop all agree).
pub const MODEL_UNIT: f64 = 0.2 * PLANE[1];

/// The game's HUD text: plane units per font unit, and the extra advance after each glyph
/// (measured on the 2005 game's frames at 720 lines: glyphs 1.08 times their font size with
/// 2 pixels between them, over five strings in three fonts).
pub const TEXT_SCALE: f64 = 1.08 * PLANE[1] / 720.;
pub const TEXT_SPACING: f64 = 2. * PLANE[1] / 720.;

/// How a line of the game's HUD text is set: its font, horizontal alignment (0 left, 1 right,
/// 2 centre), vertical alignment (0 top, 1 bottom, 2 centre), drop shadow, size against the
/// HUD's measured text size ([`TEXT_SCALE`]) and the space added after each glyph.
#[derive(Clone, Copy, Debug)]
pub struct Setting<'a> {
    pub font: &'a str,
    pub align: u32,
    pub valign: u32,
    pub shadow: bool,
    pub scale: f64,
    pub spacing: f64,
}

/// Where wrapped lines go: the top row's top-left corner, the width a row may fill, the
/// distance between rows and how many rows may be used.
#[derive(Clone, Copy, Debug)]
pub struct Rows {
    pub at: [f64; 2],
    pub width: f64,
    pub step: f64,
    pub count: usize,
}

/// The font role of one of the game's HUD fonts (tiny, small, medium, large).
pub fn font_role(name: &str) -> usize {
    let is = |font: &str| name.eq_ignore_ascii_case(font);
    if is("gamefont_tiny") || is("gamefont_super_tiny") {
        0
    } else if is("gamefont_medium") {
        2
    } else if is("gamefont_large") {
        3
    } else {
        1
    }
}
pub const DARK: Color = [0.015, 0.026, 0.045, 0.9];

#[derive(Clone, Copy)]
pub struct Style {
    pub accent: Color,
    pub ink: Color,
    pub outline: bool,
    pub bitmap: bool,
}

impl Style {
    pub fn for_theme(theme: &str) -> Self {
        match theme {
            "bf1" => Self {
                accent: [0.64, 0.77, 0.91, 1.],
                ink: WHITE,
                outline: false,
                bitmap: true,
            },
            "bf2" => Self {
                accent: [0.96, 0.24, 0.17, 1.],
                ink: WHITE,
                outline: true,
                bitmap: false,
            },
            _ => Self {
                accent: [1., 0.72, 0.32, 1.],
                ink: [1., 0.93, 0.80, 1.],
                outline: false,
                bitmap: false,
            },
        }
    }
}

pub(super) fn glyph(font: &Font, ch: char) -> u16 {
    if let Ok(code) = u16::try_from(u32::from(ch)) {
        if font.glyphs.contains_key(&code) {
            return code;
        }
    }
    for code in [0xfffd, u16::from(b'?')] {
        if font.glyphs.contains_key(&code) {
            return code;
        }
    }
    font.glyphs
        .iter()
        .filter(|(_, g)| g.bounds[2] > g.bounds[0])
        .map(|(code, _)| *code)
        .min()
        .unwrap_or_default()
}

struct Line<'a> {
    font: &'a Font,
    chars: [u16; 512],
    len: usize,
    discarded: usize,
}

impl Write for Line<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if self.len == self.chars.len() {
                self.discarded += 1 + chars.count();
                break;
            }
            self.chars[self.len] = glyph(self.font, ch);
            self.len += 1;
        }
        Ok(())
    }
}

impl<'a> Caption<'a> {
    pub(super) fn resolve(self, catalog: &'a Catalog) -> &'a str {
        match self {
            Self::Key(key) => catalog.localized(key).unwrap_or(key),
            Self::Literal(text) => text,
        }
    }
}

pub struct Painter<'a> {
    pub surface: &'a mut dyn BattleUiSurface,
    pub catalog: &'a Catalog,
    pub style: Style,
    pub matrix: Matrix,
    pub unit: f64,
    viewport: [f64; 2],
    /// The share of the window the plane leaves free on each axis.
    slack: [f64; 2],
    pub limit: usize,
    pub glyph_limit: usize,
    pub omitted_quads: usize,
}

impl<'a> Painter<'a> {
    pub fn new(
        surface: &'a mut dyn BattleUiSurface,
        catalog: &'a Catalog,
        style: Style,
        size: [f64; 2],
    ) -> Self {
        let [w, h] = size.map(|v| v.max(1.));
        let unit = (w / PLANE[0]).min(h / PLANE[1]);
        let mut matrix = anchor(
            (1. - unit * PLANE[0] / w) * 0.5,
            (1. - unit * PLANE[1] / h) * 0.5,
            false,
            768.,
        );
        matrix[0] = unit * 1024. / w;
        matrix[5] = -unit * 768. / h;
        Self {
            surface,
            catalog,
            style,
            matrix,
            unit,
            viewport: [w, h],
            slack: [1. - unit * PLANE[0] / w, 1. - unit * PLANE[1] / h],
            limit: QUADS - 1,
            glyph_limit: 512,
            omitted_quads: 0,
        }
    }

    /// Pin the plane to the window's edges (0 = left or top, 0.5 = centred, 1 = right or
    /// bottom) for what is drawn next. The plane keeps its size; nothing is stretched.
    pub fn place(&mut self, pin: [f64; 2]) {
        self.matrix[12] = (pin[0] * self.slack[0] - 0.5) * PLANE[0];
        self.matrix[13] = -(pin[1] * self.slack[1] - 0.5) * PLANE[1];
    }

    pub fn text_layers(&self) -> usize {
        if self.style.outline {
            3
        } else {
            2
        }
    }

    pub fn viewport_point(&self, xy: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|i| (xy[i] - 0.5) * self.viewport[i] / self.unit + PLANE[i] / 2.)
    }

    pub fn art(&self, name: &str) -> u32 {
        let direct = self.surface.texture(name);
        if direct != 0 {
            return direct;
        }
        self.catalog
            .meshes
            .get(name)
            .map_or(0, |t| self.surface.texture(t))
    }

    /// Where a model's picture lies with the model's origin at `at`, as large as the model
    /// is, or `None` without its prepared extent.
    pub fn model_rect(&self, mesh: &str, at: [f64; 2], scale: [f64; 2]) -> Option<[f64; 4]> {
        let catalog = self.catalog;
        let key = catalog
            .meshes
            .get(mesh)
            .or_else(|| {
                catalog
                    .meshes
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(mesh))
                    .map(|(_, icon)| icon)
            })
            .map_or(mesh, String::as_str);
        let &[left, bottom, right, top] = catalog.bounds.get(key).or_else(|| {
            catalog
                .bounds
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, extent)| extent)
        })?;
        let [kx, ky] = scale.map(|s| s.abs() * MODEL_UNIT);
        Some([
            at[0] + left * kx,
            at[1] - top * ky,
            (right - left) * kx,
            (top - bottom) * ky,
        ])
    }

    /// Draw a model's picture at its true size (see [`Painter::model_rect`]).
    pub fn model(&mut self, mesh: &str, at: [f64; 2], scale: [f64; 2], color: Color) -> bool {
        let Some(rect) = self.model_rect(mesh, at, scale) else {
            return false;
        };
        let texture = self.mesh_art(mesh);
        if texture == 0 {
            return false;
        }
        let asset = self.surface.asset(texture);
        // The prepared picture keeps a one pixel margin around the model.
        let uv = if asset.width > 2. && asset.height > 2. {
            [
                1. / asset.width,
                1. / asset.height,
                1. - 1. / asset.width,
                1. - 1. / asset.height,
            ]
        } else {
            UV
        };
        self.image(rect, texture, uv, color, 0);
        true
    }

    pub fn mesh_art(&self, name: &str) -> u32 {
        let direct = self.catalog.meshes.get(name).map_or_else(
            || self.surface.texture(name),
            |icon| self.surface.texture(icon),
        );
        if direct != 0 || !name.bytes().any(|b| b.is_ascii_uppercase()) {
            return direct;
        }
        let lower = name.to_ascii_lowercase();
        self.catalog.meshes.get(&lower).map_or_else(
            || self.surface.texture(&lower),
            |icon| self.surface.texture(icon),
        )
    }

    pub fn image(&mut self, r: [f64; 4], texture: u32, uv: [f64; 4], color: Color, mask: u32) {
        let [x, y, w, h] = r;
        self.projected_image(self.matrix, [x, y, x + w, y + h], texture, uv, color, mask);
    }

    pub fn projected_image(
        &mut self,
        matrix: Matrix,
        bounds: [f64; 4],
        texture: u32,
        uv: [f64; 4],
        color: Color,
        mask: u32,
    ) {
        if texture == 0 {
            return;
        }
        if self.surface.batch_mut().count >= self.limit {
            self.omitted_quads += 1;
            return;
        }
        self.surface
            .batch_mut()
            .quad(matrix, bounds, texture, uv, color, 0., mask);
    }

    pub fn solid(&mut self, r: [f64; 4], color: Color) {
        self.image(r, self.surface.solid_texture(), UV, color, 0);
    }

    pub fn icon(&mut self, r: [f64; 4], name: &str, color: Color) {
        self.image(r, self.art(name), UV, color, 0);
    }

    /// A bordered box's background as the 2005 game draws it: the box's rect grown by its
    /// border on every side, in nine pieces whose edges are the border wide and take as many
    /// of the picture's pixels.
    pub fn bordered(&mut self, w: &Widget, color: Color) {
        let texture = self.art(&w.bitmap);
        let [x, y, width, height] = w.rect;
        if texture == 0 || width <= 0. || height <= 0. {
            return;
        }
        let asset = self.surface.asset(texture);
        if asset.width < 1. || asset.height < 1. {
            return;
        }
        let [bx, by] = w.border;
        let u = (bx / asset.width).min(0.5);
        let v = (by / asset.height).min(0.5);
        let xs = [x - bx, x, x + width, x + width + bx];
        let ys = [y - by, y, y + height, y + height + by];
        let us = [0., u, 1. - u, 1.];
        let vs = [0., v, 1. - v, 1.];
        for j in 0..3 {
            for i in 0..3 {
                let piece = [xs[i], ys[j], xs[i + 1] - xs[i], ys[j + 1] - ys[j]];
                if piece[2] > 0. && piece[3] > 0. {
                    self.image(piece, texture, [us[i], vs[j], us[i + 1], vs[j + 1]], color, 0);
                }
            }
        }
    }

    pub fn panel(&mut self, r: [f64; 4]) {
        self.solid(r, DARK);
        let [x, y, w, h] = r;
        let t = 1.35 / self.unit.max(0.7);
        let mut edge = self.style.accent;
        edge[3] = 0.72;
        self.solid([x, y, w, t], edge);
        edge[3] = 0.28;
        self.solid([x, y + h - t, w, t], edge);
        self.solid([x, y, t, h], edge);
        self.solid([x + w - t, y, t, h], edge);
    }

    /// Draw a text element as the game does: its own font at the game's size, placed by its
    /// alignment inside its text box, with a drop shadow when the definition asks for one.
    pub fn game_text(
        &mut self,
        w: &bfe_render::hud_model2::Widget,
        at: [f64; 2],
        color: Color,
        args: Arguments<'_>,
    ) {
        self.game_line(w, at, w.text_box, w.valign, color, args);
    }

    /// One line in `w`'s font and style inside `text_box` with the given vertical alignment.
    pub fn game_line(
        &mut self,
        w: &bfe_render::hud_model2::Widget,
        at: [f64; 2],
        text_box: [f64; 2],
        valign: u32,
        color: Color,
        args: Arguments<'_>,
    ) {
        let setting = Setting {
            font: &w.font,
            align: w.align,
            valign,
            shadow: w.shadow,
            scale: 1.,
            spacing: TEXT_SPACING,
        };
        self.set_line(setting, at, text_box, color, args);
    }

    /// One line of the game's HUD text as `setting` sets it, inside `text_box` from `at`.
    pub fn set_line(
        &mut self,
        setting: Setting<'_>,
        at: [f64; 2],
        text_box: [f64; 2],
        color: Color,
        args: Arguments<'_>,
    ) {
        let Setting {
            font: font_name,
            align,
            valign,
            shadow,
            scale,
            spacing,
        } = setting;
        let scale = TEXT_SCALE * scale;
        let role = font_role(font_name);
        let layers = if shadow { 2 } else { 1 };
        let matrix = self.matrix;
        let (batch, font) = self.surface.batch_and_font(role);
        let mut line = Line {
            font,
            chars: [0; 512],
            len: 0,
            discarded: 0,
        };
        let _ = line.write_fmt(args);
        let allowed = self
            .glyph_limit
            .min(self.limit.saturating_sub(batch.count) / layers);
        let len = line.len.min(allowed);
        self.omitted_quads += (line.len - len + line.discarded) * layers;
        let text = &line.chars[..len];
        let share = |align: u32| match align {
            1 => 1.,
            2 => 0.5,
            _ => 0.,
        };
        let height = font.height * scale;
        let x = at[0] + text_box[0] * share(align);
        let y = if text_box[1] > 0. {
            at[1] + (text_box[1] - height) * share(valign)
        } else {
            at[1] - height * share(valign)
        };
        if shadow {
            let offset = 1.;
            font.draw(
                batch,
                matrix,
                x + offset,
                y + offset,
                text,
                [0., 0., 0., color[3]],
                align,
                scale,
                spacing,
                0.,
            );
        }
        font.draw(
            batch,
            matrix,
            x,
            y,
            text,
            color,
            align,
            scale,
            spacing,
            0.,
        );
    }

    /// The rows `args` takes when broken at spaces to fit `rows.width` (the definition's
    /// `TextBreak Word`), at most `rows.count`.
    pub fn wrapped_rows(&mut self, setting: Setting<'_>, rows: Rows, args: Arguments<'_>) -> usize {
        self.wrap(setting, rows, None, args)
    }

    /// Draw `args` broken at spaces to fit `rows.width`, one row per `rows.step` downward from
    /// `rows.at`, at most `rows.count` rows; returns the rows drawn.
    pub fn set_wrapped(
        &mut self,
        setting: Setting<'_>,
        rows: Rows,
        color: Color,
        args: Arguments<'_>,
    ) -> usize {
        self.wrap(setting, rows, Some(color), args)
    }

    fn wrap(
        &mut self,
        setting: Setting<'_>,
        rows: Rows,
        color: Option<Color>,
        args: Arguments<'_>,
    ) -> usize {
        let scale = TEXT_SCALE * setting.scale;
        let spacing = setting.spacing;
        let role = font_role(setting.font);
        let layers = if setting.shadow { 2 } else { 1 };
        let matrix = self.matrix;
        let glyph_limit = self.glyph_limit;
        let limit = self.limit;
        let (batch, font) = self.surface.batch_and_font(role);
        let mut line = Line {
            font,
            chars: [0; 512],
            len: 0,
            discarded: 0,
        };
        let _ = line.write_fmt(args);
        let text = &line.chars[..line.len.min(glyph_limit)];
        let mut omitted = if color.is_some() {
            (line.len - text.len() + line.discarded) * layers
        } else {
            0
        };
        let space = glyph(font, ' ');
        let x = rows.at[0]
            + rows.width
                * match setting.align {
                    1 => 1.,
                    2 => 0.5,
                    _ => 0.,
                };
        let mut start = 0;
        let mut row = 0;
        while row < rows.count {
            while start < text.len() && text[start] == space {
                start += 1;
            }
            if start == text.len() {
                break;
            }
            // The longest run from `start` that fits, ended at a space where one is found.
            let mut end = start + 1;
            let mut fit = text.len();
            let mut last_space = None;
            // The run's width as the font measures it: each glyph's rounded advance and spacing.
            let mut run_width = 0.;
            for i in start + 1..=text.len() {
                run_width += font.width(&text[i - 1..i], scale, spacing);
                if run_width > rows.width {
                    fit = i - 1;
                    break;
                }
                if i < text.len() && text[i] == space {
                    last_space = Some(i);
                }
                end = i;
            }
            if fit < text.len() {
                end = last_space.filter(|&b| b > start).unwrap_or(fit.max(start + 1));
            }
            if let Some(color) = color {
                let run = &text[start..end];
                let allowed = run.len().min(limit.saturating_sub(batch.count) / layers);
                omitted += (run.len() - allowed) * layers;
                let run = &run[..allowed];
                let y = rows.at[1] + row as f64 * rows.step;
                if setting.shadow {
                    let black = [0., 0., 0., color[3]];
                    font.draw(batch, matrix, x + 1., y + 1., run, black, setting.align, scale, spacing, 0.);
                }
                font.draw(batch, matrix, x, y, run, color, setting.align, scale, spacing, 0.);
            }
            start = end;
            row += 1;
        }
        self.omitted_quads += omitted;
        row
    }

    pub fn game_label(
        &mut self,
        w: &bfe_render::hud_model2::Widget,
        at: [f64; 2],
        color: Color,
        caption: Caption<'_>,
    ) {
        let value = caption.resolve(self.catalog);
        self.game_text(w, at, color, format_args!("{value}"));
    }

    /// (x,y) is the top of a line; the alignment is left, right or centred.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        at: [f64; 2],
        width: f64,
        height: f64,
        align: u32,
        role: usize,
        color: Color,
        args: Arguments<'_>,
    ) {
        let layers = self.text_layers();
        let (batch, font) = self.surface.batch_and_font(role);
        let mut line = Line {
            font,
            chars: [0; 512],
            len: 0,
            discarded: 0,
        };
        let _ = line.write_fmt(args);
        let preferred_px = if role == 0 { 11. } else { 9. };
        let mut scale = height.max(preferred_px / self.unit) / font.height.max(1.);
        let mut len = line.len;
        self.omitted_quads += line.discarded * layers;
        let allowed = self
            .glyph_limit
            .min(self.limit.saturating_sub(batch.count) / layers);
        if len > allowed {
            self.omitted_quads += (len - allowed) * layers;
            len = allowed;
            if len > 0 {
                line.chars[len - 1] = if font.glyphs.contains_key(&0x2026) {
                    0x2026
                } else {
                    u16::from(b'.')
                };
            }
        }
        let full = font.width(&line.chars[..len], scale, 0.);
        if full > width && width > 0. {
            let minimum = 9. / (self.unit * font.height.max(1.));
            let mut low = minimum;
            let mut high = scale;
            if font.width(&line.chars[..len], minimum, 0.) <= width {
                for _ in 0..8 {
                    let mid = (low + high) * 0.5;
                    if font.width(&line.chars[..len], mid, 0.) <= width {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
            } else {
                let ellipsis = if font.glyphs.contains_key(&0x2026) {
                    0x2026
                } else {
                    u16::from(b'.')
                };
                let mut used = font.width(&[ellipsis], minimum, 0.);
                let measured = len;
                len = 0;
                if used <= width {
                    for ch in &line.chars[..measured] {
                        used += font.width(std::slice::from_ref(ch), minimum, 0.);
                        if used > width {
                            break;
                        }
                        len += 1;
                    }
                    line.chars[len] = ellipsis;
                    len += 1;
                }
            }
            scale = low;
        }
        let text = &line.chars[..len];
        let baseline = at[1] + font.caps_middle().mul_add(-scale, height * 0.5);
        let shadow = [0.008, 0.012, 0.022, color[3] * 0.94];
        let stroke = (1. / self.unit).clamp(0.7, 2.2);
        let first = batch.count;
        if self.style.outline {
            font.draw(
                batch,
                self.matrix,
                at[0] - stroke,
                baseline,
                text,
                shadow,
                align,
                scale,
                0.,
                0.,
            );
        }
        font.draw(
            batch,
            self.matrix,
            at[0] + stroke,
            baseline + stroke,
            text,
            shadow,
            align,
            scale,
            0.,
            0.,
        );
        font.draw(
            batch,
            self.matrix,
            at[0],
            baseline,
            text,
            color,
            align,
            scale,
            0.,
            0.,
        );
        // At small sizes a rounded stroke can fall entirely between two pixel centres.
        // Keep its original advance and put the ink over the nearest physical pixel.
        if self.unit < 1. {
            let pixel = (1024. / self.viewport[0]) as f32;
            let origin = self.viewport[0] as f32 * 0.5;
            for i in first..batch.count {
                let xy = &mut batch.position[i * 12..i * 12 + 12];
                if xy[6] - xy[0] < 1.5 * pixel {
                    let centre =
                        (((xy[0] + xy[6]) / (2. * pixel) + origin).floor() + 0.5 - origin) * pixel;
                    xy[0] = centre - 0.75 * pixel;
                    xy[3] = xy[0];
                    xy[6] = centre + 0.75 * pixel;
                    xy[9] = xy[6];
                    batch.start = batch.start.min(i * 4);
                    batch.end = batch.end.max((i + 1) * 4);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn label(
        &mut self,
        at: [f64; 2],
        width: f64,
        height: f64,
        align: u32,
        role: usize,
        color: Color,
        caption: Caption<'_>,
    ) {
        let value = caption.resolve(self.catalog);
        self.text(
            at,
            width,
            height,
            align,
            role,
            color,
            format_args!("{value}"),
        );
    }

    pub fn friend(&self, same: bool) -> Color {
        if same {
            [0.20, 0.71, 1., 1.]
        } else {
            [1., 0.29, 0.20, 1.]
        }
    }
}

pub fn point(p: super::Point, f: f32) -> [f64; 2] {
    p.map(|s| f64::from(s.at(f)))
}
