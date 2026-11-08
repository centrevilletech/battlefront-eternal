//! `DETAILS` and `AWARDS` sub-pages, 3D hologram logo flights, and 2D batch drawing
//! for `ClassicScoreboardView` (`src/scoreboard/classic/pages.js`, `src/scoreboard/classic/view.js`).

use bfe_render::interface::{anchor, mul, scale, translate, Batch, Matrix};

use super::input::ScoreboardHitTarget;
use super::layout::{
    stats_page_layout, team_stats_layout, StatsPageRowsConfig, TeamSideLayout, TeamStatsLayout,
    AWARDS_ROWS, DETAILS_ROWS, ICON, PITCH,
};
use super::model::{BoardPage, BoardState, ScoreboardModel};
use super::stats::ScoreboardPort;
use super::view::{
    pulse, ClassicScoreboardView, ScoreButtonBox, BOT_COLOR, ENTRY, HEADERS, HUMAN_COLOR,
    MAX_VISIBLE_ROWS, PING_NAME_SHARE, POINTER_SIZE, PULSE_LOW, ROW_SCALE,
};
use crate::shapes::{
    draw_nine_slice, rgba, tint, BattleUiSurface, TitleIntro, FONT_NAME_MEDIUM, FONT_NAME_SMALL, W,
    WHITE,
};

pub const OMEGA: f64 = std::f64::consts::PI / 6.0; // 30 deg/s
pub const TITLE_FADE: f64 = 0.4;
pub const ROW_DELAY: f64 = 0.25;
pub const MAX_STAT_ROWS: usize = 16;

#[inline]
pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Deterministic pseudo-random delay in `[0.0, max_delay]` avoiding allocation or external RNG state.
#[inline]
pub(crate) fn hash_delay(seed: u32, max_delay: f64) -> f64 {
    let x = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let y = ((x >> ((x >> 28).wrapping_add(4))) ^ x).wrapping_mul(277_803_737);
    let z = (y >> 22) ^ y;
    (z as f64 / u32::MAX as f64) * max_delay
}

/// Endpoint state of a 3D hologram flight (`Flight` in `src/scoreboard/classic/view.js`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightPoint {
    pub x: f64,
    pub y: f64,
    pub size: f64,
    pub alpha: f64,
}

/// Linear 3D model flight with spin (`Flight` in `src/scoreboard/classic/view.js`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Flight {
    pub from: FlightPoint,
    pub to: FlightPoint,
    pub time: f64,
    pub length: f64,
    pub omega: f64,
    pub rest: Option<f64>,
    pub spin: f64,
}

impl Flight {
    /// Starts a flight from `from` to `to` over `length` seconds.
    pub fn start(
        &mut self,
        from: FlightPoint,
        to: FlightPoint,
        length: f64,
        omega: f64,
        rest: Option<f64>,
    ) {
        self.from = from;
        self.to = to;
        self.time = 0.0;
        self.length = length;
        self.omega = omega;
        self.rest = rest;
    }

    /// Whether the flight is still in transit (`flying` in `src/scoreboard/classic/view.js`).
    #[inline]
    pub fn flying(&self) -> bool {
        self.time < self.length
    }

    /// Advances spin and elapsed time by `dt` seconds (`update` in `src/scoreboard/classic/view.js`).
    pub fn update(&mut self, dt: f64) {
        let rate = if self.flying() {
            self.omega
        } else {
            self.rest.unwrap_or(0.0)
        };
        self.spin += rate * OMEGA * dt;
        self.time = (self.time + dt).min(self.length);
    }

    /// Current interpolated position, size, and opacity (`state` in `src/scoreboard/classic/view.js`).
    pub fn state(&self) -> FlightPoint {
        let t = if self.length > 0.0 {
            (self.time / self.length).clamp(0.0, 1.0)
        } else {
            1.0
        };
        FlightPoint {
            x: lerp(self.from.x, self.to.x, t),
            y: lerp(self.from.y, self.to.y, t),
            size: lerp(self.from.size, self.to.size, t),
            alpha: lerp(self.from.alpha, self.to.alpha, t),
        }
    }
}

/// One row of a `StatsPage` (`DETAILS` or `AWARDS`, `src/scoreboard/classic/pages.js`).
#[derive(Clone, Debug, Default)]
pub struct StatsPageRow {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
}

/// `DETAILS` or `AWARDS` page renderer (`StatsPage` in `src/scoreboard/classic/pages.js`).
pub struct StatsPage {
    pub is_details: bool,
    pub rows_cfg: StatsPageRowsConfig,
    pub time: f64,
    pub title_intro: TitleIntro,
    pub name_intro: TitleIntro,
    pub label_intros: [TitleIntro; MAX_STAT_ROWS],
    pub value_intros: [TitleIntro; MAX_STAT_ROWS],
    pub content: Vec<StatsPageRow>,
}

impl StatsPage {
    /// Creates a new `StatsPage` for either `DETAILS` (`is_details = true`) or `AWARDS` (`is_details = false`).
    pub fn new(is_details: bool) -> Self {
        Self {
            is_details,
            rows_cfg: if is_details {
                DETAILS_ROWS
            } else {
                AWARDS_ROWS
            },
            time: 0.0,
            title_intro: TitleIntro::default(),
            name_intro: TitleIntro::default(),
            label_intros: [TitleIntro::default(); MAX_STAT_ROWS],
            value_intros: [TitleIntro::default(); MAX_STAT_ROWS],
            content: Vec::with_capacity(MAX_STAT_ROWS),
        }
    }

    /// Opens the page with `content` and restarts all flashy text intros (`open` in `src/scoreboard/classic/pages.js`).
    pub fn open(&mut self, content: Vec<StatsPageRow>) {
        self.time = 0.0;
        self.content = content;
        self.title_intro.start(0.0);
        self.name_intro.start(0.0);
        for i in 0..MAX_STAT_ROWS {
            self.label_intros[i].start(hash_delay(i as u32 * 2 + 11, ROW_DELAY));
            self.value_intros[i].start(hash_delay(i as u32 * 2 + 12, ROW_DELAY));
        }
    }

    /// Refreshes row values without restarting intros (`refresh` in `src/scoreboard/classic/pages.js`).
    pub fn refresh(&mut self, content: Vec<StatsPageRow>) {
        self.content = content;
    }

    /// Steps the page timer and flashy text intros (`update` in `src/scoreboard/classic/pages.js`).
    pub fn update(&mut self, dt: f64) {
        self.time += dt;
        self.title_intro.update(dt);
        self.name_intro.update(dt);
        for i in 0..self.content.len().min(MAX_STAT_ROWS) {
            self.label_intros[i].update(dt);
            self.value_intros[i].update(dt);
        }
    }

    /// Draws the `DETAILS` or `AWARDS` text rows (`draw` in `src/scoreboard/classic/pages.js`).
    pub fn draw(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        subject_name: Option<&str>,
    ) {
        let layout = stats_page_layout(surface.height(), self.rows_cfg);
        let title_str = if self.is_details {
            surface.string("ifs.stats.personalstatstitle", "PERSONAL STATS")
        } else {
            surface.string("ifs.stats.awardstatstitle", "PLAYER AWARDS")
        };
        let fade = (self.time / TITLE_FADE).clamp(0.0, 1.0);
        ClassicScoreboardView::draw_flashy_text(
            surface,
            batch,
            m,
            layout.title[0],
            layout.title[1],
            title_str,
            WHITE,
            2,
            Some(&self.title_intro),
            FONT_NAME_MEDIUM,
            1.0,
            fade,
        );
        if self.is_details {
            let name_upper = subject_name.unwrap_or("").to_uppercase();
            ClassicScoreboardView::draw_flashy_text(
                surface,
                batch,
                m,
                layout.name[0],
                layout.name[1],
                &name_upper,
                WHITE,
                2,
                Some(&self.name_intro),
                FONT_NAME_MEDIUM,
                1.0,
                1.0,
            );
        }

        let font = self.rows_cfg.font;
        for (i, row) in self.content.iter().enumerate().take(MAX_STAT_ROWS) {
            let k = self.rows_cfg.blank + i;
            if k >= layout.shown {
                break;
            }
            let y = layout.row_top + k as f64 * layout.pitch - 10.0;
            let label_text = surface.string(row.key, row.label);
            ClassicScoreboardView::draw_flashy_text(
                surface,
                batch,
                m,
                layout.label_right,
                y,
                label_text,
                WHITE,
                1,
                Some(&self.label_intros[i]),
                font,
                1.0,
                1.0,
            );
            if !row.value.is_empty() {
                let val_upper = row.value.to_uppercase();
                ClassicScoreboardView::draw_flashy_text(
                    surface,
                    batch,
                    m,
                    layout.value_left,
                    y,
                    &val_upper,
                    WHITE,
                    0,
                    Some(&self.value_intros[i]),
                    font,
                    1.0,
                    1.0,
                );
            }
        }
    }
}

/// 3D placement state for a scoreboard team logo or award icon (`place` in `src/scoreboard/classic/view.js`).
#[derive(Clone, Debug)]
pub struct LogoPlacement {
    pub model_name: String,
    pub team: i32,
    pub position: [f32; 3],
    pub scale: f32,
    pub rotation_y: f32,
    pub color: [f32; 3],
    pub opacity: f32,
    pub visible: bool,
}

impl ClassicScoreboardView {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw_window(
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        name: &str,
        color: [f64; 4],
    ) {
        let texture = surface.texture(name);
        if texture == 0 {
            batch.quad(
                m,
                [
                    x - w / 2.0 + 4.0,
                    y - h / 2.0 + 4.0,
                    x + w / 2.0 - 4.0,
                    y + h / 2.0 - 4.0,
                ],
                surface.solid_texture(),
                [0.0, 0.0, 1.0, 1.0],
                tint([0.0, 0.17, 0.38, 1.0], 0.7 * color[3]),
                0.0,
                0,
            );
            return;
        }
        draw_nine_slice(batch, m, x, y, w, h, 16.0, texture, color);
    }

    /// Draws the dimmed backdrop and nine-sliced `border_3_pieces` window under the 3D logos (`drawUnder` in `src/scoreboard/classic/view.js`).
    pub fn draw_under(&self, surface: &dyn BattleUiSurface, batch: &mut Batch) {
        let h = surface.height();
        let layout = team_stats_layout(h);
        let m = anchor(0.0, 0.0, false, h);
        let black = surface.texture("opaque_black");
        if black != 0 {
            batch.quad(m, [0.0, 0.0, W, h], black, [0.0, 0.0, 1.0, 1.0], WHITE, 0.0, 0);
        } else {
            batch.quad(
                m,
                [0.0, 0.0, W, h],
                surface.solid_texture(),
                [0.0, 0.0, 1.0, 1.0],
                rgba(0, 0, 0, 119),
                0.0,
                0,
            );
        }
        let box_rect = layout.window;
        Self::draw_window(
            surface,
            batch,
            m,
            box_rect.x,
            box_rect.y,
            box_rect.w,
            box_rect.h,
            "border_3_pieces",
            WHITE,
        );
    }

    /// Draws the rows, headers, buttons, sub-pages, and mouse cursor over the 3D logos (`drawOver` in `src/scoreboard/classic/view.js`).
    pub fn draw_over(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        model: &ScoreboardModel,
        port: &dyn ScoreboardPort,
        cursor: Option<[f64; 2]>,
    ) {
        let h = surface.height();
        let layout = team_stats_layout(h);
        let m = anchor(0.0, 0.0, false, h);
        match model.page {
            BoardPage::Details => {
                let subj_name = model.subject.as_ref().map(|s| s.name.as_str());
                self.details_page.draw(surface, batch, m, subj_name);
                self.draw_back(surface, batch, m, &layout, model);
            }
            BoardPage::Awards => {
                self.awards_page.draw(surface, batch, m, None);
                self.draw_back(surface, batch, m, &layout, model);
            }
            BoardPage::Table => {
                for side in [1usize, 2usize] {
                    self.draw_side(surface, batch, m, side, &layout, model, port);
                }
                self.draw_buttons(surface, batch, m, &layout, model, port);
            }
        }
        if let Some(c) = cursor {
            if !self.hide_pointer {
                self.draw_pointer(surface, batch, m, c);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_side(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        side: usize,
        layout: &TeamStatsLayout,
        model: &ScoreboardModel,
        port: &dyn ScoreboardPort,
    ) {
        let list = layout.side(side);
        let side_idx = if side == 2 { 1 } else { 0 };
        if let Some(team) = &model.titles[side_idx] {
            let title = format!("{} : {}", team.name.to_uppercase(), team.reinforcements);
            surface.draw_text(
                batch,
                m,
                list.title.x,
                list.title.y,
                &title,
                FONT_NAME_MEDIUM,
                WHITE,
                0,
                1.0,
            );
        }

        let shown = (self.entered / ENTRY).clamp(0.0, 1.0);
        for (j, &(tex_name, mode)) in HEADERS.iter().enumerate() {
            let at = list.headers[j];
            let hover_key = ScoreboardHitTarget::Sort { mode, side }.hover_key();
            let a = self.hover_alpha(hover_key, false) * shown;
            let texture = surface.texture(tex_name);
            if texture != 0 {
                batch.quad(
                    m,
                    [at[0], at[1], at[0] + ICON, at[1] + ICON],
                    texture,
                    [0.0, 0.0, 1.0, 1.0],
                    tint(WHITE, a),
                    0.0,
                    0,
                );
            }
        }

        let ping = port.network() && model.state == BoardState::Peek;
        let ping_x = list.columns[0] - (list.columns[1] - list.columns[0]);
        if ping {
            let y = list.headers[0][1] + ICON / 2.0 - surface.line_height(FONT_NAME_MEDIUM) / 2.0;
            let ping_label = surface.string("ifs.MPLobby.ping_header", "PING");
            Self::draw_flashy_text(
                surface,
                batch,
                m,
                ping_x,
                y,
                ping_label,
                WHITE,
                1,
                None,
                FONT_NAME_MEDIUM,
                ROW_SCALE,
                shown,
            );
        }

        if model.can_scroll_up() {
            self.draw_arrow(surface, batch, m, list.up, -1, side, shown);
        }
        if model.can_scroll_down() {
            self.draw_arrow(surface, batch, m, list.down, 1, side, shown);
        }

        if let Some(hover) = model.hover {
            if hover.side == side && !(hover.side == model.side && hover.index == model.selected) {
                let k = hover.index as isize - model.first as isize;
                if k >= 0
                    && (k as usize) < layout.shown
                    && model.row(side, hover.index).is_some()
                {
                    self.draw_cursor(
                        surface,
                        batch,
                        m,
                        list,
                        layout,
                        k as usize,
                        "listbox_hilight",
                        1.0,
                    );
                }
            }
        }

        if model.side == side {
            let k = model.selected as isize - model.first as isize;
            let a = if side == 2 {
                pulse(self.time, PULSE_LOW, 1.0)
            } else {
                1.0
            };
            if k >= 0
                && (k as usize) < layout.shown
                && model.row(side, model.selected).is_some()
            {
                self.draw_cursor(
                    surface,
                    batch,
                    m,
                    list,
                    layout,
                    k as usize,
                    "listbox_cursor",
                    a,
                );
            }
        }

        let max_name_w = if ping {
            list.name_w * PING_NAME_SHARE
        } else {
            list.name_w
        };
        for k in 0..layout.shown.min(MAX_VISIBLE_ROWS) {
            let Some(row) = model.row(side, model.first + k) else {
                continue;
            };
            let color = if row.local {
                WHITE
            } else if row.human {
                HUMAN_COLOR
            } else {
                BOT_COLOR
            };
            let y = layout.top + k as f64 * PITCH + layout.text_top;
            let name = Self::clip(surface, &row.name.to_uppercase(), max_name_w);
            Self::draw_flashy_text(
                surface,
                batch,
                m,
                list.name_x,
                y,
                &name,
                color,
                0,
                Some(&self.intros[side_idx][k][0]),
                FONT_NAME_MEDIUM,
                ROW_SCALE,
                1.0,
            );
            for (j, val) in [row.kills, row.deaths, row.points].into_iter().enumerate() {
                let s = val.to_string();
                Self::draw_flashy_text(
                    surface,
                    batch,
                    m,
                    list.columns[j],
                    y,
                    &s,
                    color,
                    1,
                    Some(&self.intros[side_idx][k][j + 1]),
                    FONT_NAME_MEDIUM,
                    ROW_SCALE,
                    1.0,
                );
            }
            if ping {
                if let Some(p) = row.ping {
                    let s = p.to_string();
                    Self::draw_flashy_text(
                        surface,
                        batch,
                        m,
                        ping_x,
                        y,
                        &s,
                        color,
                        1,
                        Some(&self.intros[side_idx][k][4]),
                        FONT_NAME_MEDIUM,
                        ROW_SCALE,
                        1.0,
                    );
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_arrow(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        at: [f64; 2],
        step: isize,
        side: usize,
        shown: f64,
    ) {
        let texture = surface.texture("small_arrow");
        if texture == 0 {
            return;
        }
        let hover_key = ScoreboardHitTarget::Scroll { step, side }.hover_key();
        let a = self.hover_alpha(hover_key, false) * shown;
        let h = ICON / 2.0;
        let uv = if step < 0 {
            [[1.0, 1.0], [0.0, 1.0], [1.0, 0.0], [0.0, 0.0]]
        } else {
            [[0.0, 1.0], [1.0, 1.0], [0.0, 0.0], [1.0, 0.0]]
        };
        batch.corners(
            m,
            [at[0] - h, at[1] - h, at[0] + h, at[1] + h],
            texture,
            uv,
            tint(WHITE, a),
            0.0,
            0,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cursor(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        list: &TeamSideLayout,
        layout: &TeamStatsLayout,
        k: usize,
        name: &str,
        a: f64,
    ) {
        let y = layout.top + k as f64 * PITCH + layout.cursor_dy;
        Self::draw_window(
            surface,
            batch,
            m,
            list.x,
            y,
            layout.cursor_w,
            layout.cursor_h,
            name,
            tint(WHITE, a),
        );
    }

    fn clip(surface: &dyn BattleUiSurface, text: &str, max_width: f64) -> String {
        let mut out = text.to_string();
        while !out.is_empty() && surface.text_width(FONT_NAME_MEDIUM, &out, ROW_SCALE) > max_width {
            out.pop();
        }
        out
    }

    /// Draws text playing a `TitleIntro` (`drawText` in `src/scoreboard/classic/view.js`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_flashy_text(
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        x: f64,
        y: f64,
        text: &str,
        color: [f64; 4],
        align: u32,
        intro: Option<&TitleIntro>,
        font: &str,
        h_scale: f64,
        alpha: f64,
    ) {
        if text.is_empty() {
            return;
        }
        let fh = surface.line_height(font);
        let width = surface.text_width(font, text, h_scale);
        let state = intro.map(|i| i.frame()).unwrap_or_default();
        let left = match align {
            1 => x - width,
            2 => x - width / 2.0,
            _ => x,
        };
        if let Some(block) = state.block {
            let bx = left + block.x * (width - fh * h_scale).max(0.0);
            let cursor_tex = {
                let id = surface.texture("cursor");
                if id != 0 {
                    id
                } else {
                    surface.solid_texture()
                }
            };
            batch.quad(
                m,
                [bx, y, bx + fh * h_scale, y + fh],
                cursor_tex,
                [0.0, 0.0, 1.0, 1.0],
                tint(WHITE, block.alpha * alpha),
                0.0,
                0,
            );
        }
        if state.text <= 0.01 || state.alpha <= 0.0 {
            return;
        }
        let mid = y + fh / 2.0;
        let mut scale_m = scale(
            [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
            1.0,
        );
        scale_m[5] = state.text / h_scale.max(1e-6);
        let at = translate(
            mul(translate(m, x, mid, 0.0), scale_m),
            0.0,
            (-fh / 2.0) * h_scale,
            0.0,
        );
        surface.draw_text(
            batch,
            at,
            0.0,
            0.0,
            text,
            font,
            tint(color, state.alpha * alpha),
            align,
            h_scale,
        );
    }

    pub(crate) fn draw_buttons(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        layout: &TeamStatsLayout,
        model: &ScoreboardModel,
        port: &dyn ScoreboardPort,
    ) {
        let end = model.state == BoardState::End;
        let small_h = surface.line_height(FONT_NAME_SMALL);
        let done = self.button_box("done", layout, model, surface);
        if !end || port.can_restart() {
            surface.draw_text(
                batch,
                m,
                done.x0,
                done.top,
                &done.text,
                FONT_NAME_MEDIUM,
                tint(WHITE, self.hover_alpha(done.kind.hover_key(), true)),
                0,
                1.0,
            );
        }
        if end {
            let awards = self.button_box("awards", layout, model, surface);
            surface.draw_text(
                batch,
                m,
                awards.x0,
                awards.top,
                &awards.text,
                FONT_NAME_MEDIUM,
                tint(WHITE, self.hover_alpha("awards::", true)),
                0,
                1.0,
            );
        }
        let details = self.button_box("details", layout, model, surface);
        let x0 = details.x0 - 22.0;
        let x1 = details.x0 + details.width + 20.0;
        let y0 = details.top;
        let y1 = details.top + small_h;
        let pieces = [
            (x0, x0 + small_h, "backbutton_left"),
            (x0 + small_h, x1 - small_h, "backbutton_mid"),
            (x1 - small_h, x1, "backbutton_right"),
        ];
        for (a, b, tex_name) in pieces {
            let texture = surface.texture(tex_name);
            if texture != 0 {
                batch.quad(m, [a, y0, b, y1], texture, [0.0, 0.0, 1.0, 1.0], WHITE, 0.0, 0);
            } else {
                batch.quad(
                    m,
                    [a, y0, b, y1],
                    surface.solid_texture(),
                    [0.0, 0.0, 1.0, 1.0],
                    rgba(16, 40, 80, 220),
                    0.0,
                    0,
                );
            }
        }
        surface.draw_text(
            batch,
            m,
            details.x0,
            details.top,
            &details.text,
            FONT_NAME_SMALL,
            tint(WHITE, self.hover_alpha("details::", true)),
            0,
            1.0,
        );
    }

    pub(crate) fn draw_back(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        layout: &TeamStatsLayout,
        model: &ScoreboardModel,
    ) {
        let back = self.button_box("back", layout, model, surface);
        surface.draw_text(
            batch,
            m,
            back.x0,
            back.top,
            &back.text,
            FONT_NAME_MEDIUM,
            tint(WHITE, self.hover_alpha("back::", true)),
            0,
            1.0,
        );
    }

    pub(crate) fn draw_pointer(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        cursor: [f64; 2],
    ) {
        let texture = surface.texture("mousecursor");
        if texture != 0 {
            batch.quad(
                m,
                [
                    cursor[0],
                    cursor[1],
                    cursor[0] + POINTER_SIZE,
                    cursor[1] + POINTER_SIZE,
                ],
                texture,
                [0.0, 0.0, 1.0, 1.0],
                WHITE,
                0.0,
                0,
            );
        } else {
            batch.quad(
                m,
                [cursor[0], cursor[1], cursor[0] + 8.0, cursor[1] + 8.0],
                surface.solid_texture(),
                [0.0, 0.0, 1.0, 1.0],
                WHITE,
                0.0,
                0,
            );
        }
    }
}

#[allow(dead_code)]
type _ScoreButtonBoxAlias = ScoreButtonBox;
