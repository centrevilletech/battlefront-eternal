//! `DETAILS` and `AWARDS` sub-pages, 3D hologram logo flights, and 2D batch drawing
//! for `ClassicScoreboardView` (`src/scoreboard/classic/pages.js`, `src/scoreboard/classic/view.js`).
//!
//! Architecture and 2004 game fidelity (`ifs_teamstats`, `ifs_personalstats`, `ifs_awardstats`):
//! - `Flight` models the 2004 `AnimationMgr` linear interpolation of a 3D hologram's 2D plane
//!   coordinates `(x, y)`, scale parameter `size`, and opacity `alpha` over `length` seconds,
//!   while rotating around the Y axis at `omega * OMEGA` (`30 deg/s` per `OmegaY` unit) in transit
//!   and settling to `rest * OMEGA` once landed.
//! - `StatsPage` renders both `DETAILS` (`ifs_personalstats`: `"PERSONAL STATS"`, player name,
//!   and 9+1 stat rows in `gamefont_small` below 3 blank rows) and `AWARDS` (`ifs_awardstats`:
//!   `"PLAYER AWARDS"` and 8 award rows in `gamefont_medium` below 2 blank rows, with a small
//!   spinning team hologram icon in the center gap between each award label and winner).
//! - `ClassicScoreboardView::draw_under` and `draw_over` split the 2D batch pass around the 3D
//!   hologram pass (`logo_placements`): first drawing the `opaque_black` backdrop (`alpha = 119/255`)
//!   and nine-sliced `border_3_pieces` window underneath the 3D hologram models, and then drawing
//!   all team titles, column sort icons, scroll arrows, selection/hover cursors, 80%-width rows,
//!   action buttons (`DONE`/`BACK`, `AWARDS`, and the 3-piece `DETAILS` tab), and `mousecursor` on top.

use bfe_render::interface::{anchor, mul, scale, translate, Batch, Matrix};

use super::input::ScoreboardHitTarget;
use super::layout::{
    stats_page_layout, team_stats_layout, StatsPageRowsConfig, TeamSideLayout, TeamStatsLayout,
    AWARDS_ROWS, DETAILS_ROWS, ICON, PITCH,
};
use super::model::{BoardPage, BoardState, ScoreboardModel};
use super::stats::ScoreboardPort;
use super::view::{
    pulse, ClassicScoreboardView, BOT_COLOR, ENTRY, HEADERS, HUMAN_COLOR,
    MAX_VISIBLE_ROWS, PING_NAME_SHARE, POINTER_SIZE, PULSE_LOW, ROW_SCALE,
};
use crate::shapes::{
    draw_nine_slice, rgba, tint, BattleUiSurface, TitleIntro, FONT_NAME_MEDIUM, FONT_NAME_SMALL, W,
    WHITE,
};

/// Base Y-axis angular velocity in radians per second per `OmegaY` unit (`30 deg/s` = `PI / 6`, `OMEGA` in `src/scoreboard/classic/view.js`).
pub const OMEGA: f64 = std::f64::consts::PI / 6.0; // 30 deg/s

/// Duration in seconds over which the `"PERSONAL STATS"` and `"PLAYER AWARDS"` headings fade in (`TITLE_FADE = 0.4` in `src/scoreboard/classic/pages.js`).
pub const TITLE_FADE: f64 = 0.4;

/// Maximum stagger delay in seconds before a `StatsPage` label or value row starts its `TitleIntro` (`ROW_DELAY = 0.25` in `src/scoreboard/classic/pages.js`).
pub const ROW_DELAY: f64 = 0.25;

/// Fixed capacity of `TitleIntro` slots per column on a `StatsPage` (accommodating 9 personal stats + `CONNECTION`).
pub const MAX_STAT_ROWS: usize = 16;

/// Linearly interpolates between `a` (`t = 0.0`) and `b` (`t = 1.0`) (`lerp` in `src/scoreboard/classic/view.js`).
#[inline]
pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Deterministic pseudo-random delay in `[0.0, max_delay]` avoiding allocation or external RNG state.
///
/// Uses a PCG-style 32-bit permutation (`747_796_405` multiplier + xorshift-multiply-xorshift)
/// to replace `Math.random() * max_delay` from `src/scoreboard/classic/pages.js` and `view.js`
/// with reproducible per-row/per-column stagger delays (`ROW_DELAY = 0.25s`, `INTRO_DELAY = 0.15s`).
#[inline]
pub(crate) fn hash_delay(seed: u32, max_delay: f64) -> f64 {
    let x = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let y = ((x >> ((x >> 28).wrapping_add(4))) ^ x).wrapping_mul(277_803_737);
    let z = (y >> 22) ^ y;
    (z as f64 / u32::MAX as f64) * max_delay
}

/// Endpoint state of a 3D hologram flight (`Flight` in `src/scoreboard/classic/view.js`).
///
/// Captures the 2D interface-plane coordinates `(x, y)`, scale factor `size` (multiplied by
/// `LOGO_UNITS` when placed in 3D), and hologram material opacity `alpha`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FlightPoint {
    /// Horizontal position in `1024xH` interface units.
    pub x: f64,
    /// Vertical position in `1024xH` interface units.
    pub y: f64,
    /// Scale multiplier (`W / 800 = 1.28` at full table size, half size in `DETAILS`, `0.2` for `AWARDS` icons).
    pub size: f64,
    /// Hologram opacity (`0.2` on the table, `0.4` in `DETAILS`, `0.3` for landed `AWARDS` icons).
    pub alpha: f64,
}

/// Linear 3D model flight with spin (`Flight` in `src/scoreboard/classic/view.js`).
///
/// Interpolates linearly from `from` to `to` over `length` seconds (`ENTRY = 0.4s` for `gShellAnimTime`).
/// While `time < length` (`flying()`), Y rotation advances at `omega * OMEGA` radians/sec (`1.0` when
/// appearing in place, `+-20.0` = `+-600 deg/s` when flying across the screen); once landed, spin
/// settles to `rest * OMEGA` (`0.3` for left logo / `DETAILS`, `0.25` for right logo, or `[-0.45, 0.45]`
/// for landed award icons).
#[derive(Clone, Copy, Debug, Default)]
pub struct Flight {
    pub from: FlightPoint,
    pub to: FlightPoint,
    /// Elapsed flight time in seconds, clamped to `length`.
    pub time: f64,
    /// Total flight duration in seconds (`ENTRY = 0.4s`).
    pub length: f64,
    /// Spin rate multiplier in `OmegaY` units (`30 deg/s` each) while in transit.
    pub omega: f64,
    /// Spin rate multiplier in `OmegaY` units after landing (`None` while award icons await landing).
    pub rest: Option<f64>,
    /// Accumulated Y-axis rotation angle in radians (`root.rotation.y`).
    pub spin: f64,
}

impl Flight {
    /// Starts a flight from `from` to `to` over `length` seconds (`Flight.start` in `src/scoreboard/classic/view.js`).
    ///
    /// Resets `self.time = 0.0` while preserving `self.spin` so a hologram that starts a new
    /// flight (e.g. flying from `Table` to the `DETAILS` corner and back) continues rotating
    /// smoothly from its current Y-axis angle without snapping.
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
///
/// Pairs a localization `key` (`"ifs.stats.headshots"`, `"ifs.stats.tankbuster"`, etc.) and
/// English `label` fallback with the formatted right-column `value` string (e.g. `"12"`,
/// `"RIFLEMAN 3:42"`, or `"CLONE 1 (5)"`). When `value` is empty, only the left-column label draws.
#[derive(Clone, Debug, Default)]
pub struct StatsPageRow {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
}

/// `DETAILS` or `AWARDS` page renderer (`StatsPage` in `src/scoreboard/classic/pages.js`).
///
/// Manages the per-page elapsed clock `time` (driving the 0.4s `TITLE_FADE` on the page heading),
/// the `TitleIntro` animations for the heading (`title_intro`), player name (`name_intro` on `DETAILS`),
/// and up to `MAX_STAT_ROWS` (`16`) left-aligned/right-aligned row pairs (`label_intros`, `value_intros`),
/// plus the current `content` rows fetched from [`ScoreboardPort`].
pub struct StatsPage {
    /// `true` for `DETAILS` (`ifs_personalstats`), `false` for `AWARDS` (`ifs_awardstats`).
    pub is_details: bool,
    /// Row pitch, center gap, blank-row offset, and font configuration (`DETAILS_ROWS` or `AWARDS_ROWS`).
    pub rows_cfg: StatsPageRowsConfig,
    /// Elapsed seconds since `open()` was called on this page.
    pub time: f64,
    /// `FlashyText` intro for `"PERSONAL STATS"` or `"PLAYER AWARDS"`.
    pub title_intro: TitleIntro,
    /// `FlashyText` intro for the inspected player's uppercase name on `DETAILS`.
    pub name_intro: TitleIntro,
    /// Staggered `FlashyText` intros for the right-aligned stat/award labels (`0.0..=0.25s` delay).
    pub label_intros: [TitleIntro; MAX_STAT_ROWS],
    /// Staggered `FlashyText` intros for the left-aligned stat/award values (`0.0..=0.25s` delay).
    pub value_intros: [TitleIntro; MAX_STAT_ROWS],
    /// Current `StatsPageRow` entries rendered by `draw()`.
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
    ///
    /// Resets `self.time = 0.0` (restarting the 0.4s `TITLE_FADE` on `"PERSONAL STATS"` or
    /// `"PLAYER AWARDS"`), starts `title_intro` and `name_intro` immediately (`delay = 0.0`),
    /// and assigns each row's label and value `TitleIntro` a deterministic stagger delay in
    /// `[0.0, ROW_DELAY]` (`0.0..=0.25s`) via [`hash_delay`].
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
    ///
    /// Called every frame while `BoardPage::Details` is open so live personal stats (such as
    /// `LONGEST LIVING STREAK` or `FAVORITE CLASS` clock times during in-battle `Peek`) update
    /// in place without retriggering the `TitleIntro` sweep.
    pub fn refresh(&mut self, content: Vec<StatsPageRow>) {
        self.content = content;
    }

    /// Steps the page timer and flashy text intros (`update` in `src/scoreboard/classic/pages.js`).
    ///
    /// Advances `self.time` (which drives the 0.4-second `TITLE_FADE` on the page heading),
    /// `title_intro`, `name_intro`, and the active prefix (`0..content.len().min(MAX_STAT_ROWS)`)
    /// of `label_intros` and `value_intros`.
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
        // Compute the `DETAILS` (`DETAILS_ROWS`) or `AWARDS` (`AWARDS_ROWS`) layout for the current height.
        // The page title fades in over `TITLE_FADE` (`0.4s`) while playing its `TitleIntro` at `layout.title`,
        // and `DETAILS` draws the inspected player's uppercase name 30 units below the title.
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

        // Render each stat or award row starting after `rows_cfg.blank` empty rows (`3` for `DETAILS`,
        // `2` for `AWARDS`): labels are right-aligned (`align = 1`) ending at `win.x - gap`, and non-empty
        // values are uppercased and left-aligned (`align = 0`) starting at `win.x + gap`.
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
///
/// Returned by [`ClassicScoreboardView::logo_placements`] so the 3D interface pass between
/// `draw_under` and `draw_over` can position, scale (`size * LOGO_UNITS`), rotate (`rotation_y`),
/// and tint (`GREEN_RGB` friendly, `RED_RGB` enemy, `GREY_RGB` neutral) each command-post hologram.
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
    /// Draws a nine-sliced window or cursor frame (`border_3_pieces`, `listbox_cursor`, `listbox_hilight`)
    /// centered at `(x, y)` with 16-unit corners (`window` in `src/scoreboard/classic/view.js`),
    /// or a translucent dark-blue solid quad inset by 4 units when the texture is unavailable.
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
        // 1. Fill the full `1024xH` screen with `opaque_black` (or a solid `rgba(0, 0, 0, 119)`
        //    quad when `opaque_black` is absent) so the live battle world remains dimly visible behind
        //    the scoreboard.
        // 2. Draw the nine-sliced `border_3_pieces` main window (`layout.window`) over the dimmed
        //    backdrop, underneath the 3D hologram models rendered between `draw_under` and `draw_over`.
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
        // Dispatch foreground 2D drawing according to the active sub-page (`model.page`):
        // - `Details` and `Awards` delegate row rendering to `StatsPage::draw` and draw only the
        //   bottom-left `"BACK"` button (`draw_back`).
        // - `Table` renders both teams' columns (`draw_side` for sides `1` and `2`) and the bottom
        //   action buttons (`DONE`/`BACK`, `AWARDS`, and the 3-piece `DETAILS` tab).
        // Finally, draw the software `mousecursor` unless hidden by an open pause menu (`hide_pointer`).
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

    /// Draws one team's half of the `Table` page (`drawSide` in `src/scoreboard/classic/view.js`).
    ///
    /// Renders, from top to bottom / back to front:
    /// 1. The team title (`"REPUBLIC : 150"`, uppercase team name and remaining reinforcements).
    /// 2. The three `30x30` column sort icons (`stats_kills`, `stats_deaths`, `stats_cpp`), fading
    ///    in over `ENTRY` (`0.4s`) and blinking between `0.4` and `1.0` when hovered.
    /// 3. In network `Peek` mode (`Tab` in multiplayer), the `"PING"` column header one column-stride
    ///    left of `KILLS`, level with the sort icons.
    /// 4. The `small_arrow` up/down scroll indicators when `can_scroll_up()` / `can_scroll_down()` is true.
    /// 5. The hovered row highlight (`listbox_hilight` at full opacity, unless it is the selected row)
    ///    and the selected row frame (`listbox_cursor`, pulsing `1.0 <-> 0.5` on Team 2's right list
    ///    and solid `1.0` on Team 1's left list, matching the 2004 Lua script's `ListManager` quirk).
    /// 6. The visible rows (`0..layout.shown`) drawn in `gamefont_medium` at `ROW_SCALE` (`0.8` horizontal
    ///    width): local player in `WHITE`, other human players in `HUMAN_COLOR` (yellow), and AI bots
    ///    in `BOT_COLOR` (grey), with player names clipped by whole characters to fit `max_name_w`.
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
        // Resolve the geometry for `side` (`1` = left, `2` = right) and draw the uppercase
        // team header (`"REPUBLIC : 150"`, combining `TeamHeader::name` and remaining tickets)
        // at `list.title` in `FONT_NAME_MEDIUM`.
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

        // Column sort icons (`stats_kills`, `stats_deaths`, `stats_cpp`) and scroll arrows fade
        // in over `ENTRY` (`0.4s`) whenever the table opens (`shown = (entered / 0.4).clamp(0, 1)`),
        // modulated by `hover_alpha` when the pointer hovers a header icon.
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

        // Network `Peek` mode (`Tab` during online play) inserts a `PING` column one column-stride
        // (`columns[1] - columns[0]`) to the left of `KILLS` (`columns[0]`), narrowing `name_w` by
        // `PING_NAME_SHARE` (`0.5 / 0.65`) so long player names never collide with 3-digit ping numbers.
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

        // Render the up (`step = -1`) and down (`step = 1`) scroll arrows centered above and
        // below the team list whenever `first > 0` or `first + shown < length`. Both team lists
        // share the same synchronized scroll offset, so arrows appear on both sides together.
        if model.can_scroll_up() {
            self.draw_arrow(surface, batch, m, list.up, -1, side, shown);
        }
        if model.can_scroll_down() {
            self.draw_arrow(surface, batch, m, list.down, 1, side, shown);
        }

        // Draw the blue hover frame (`listbox_hilight`) when the mouse is over a non-selected row.
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

        // Draw the orange selection cursor (`listbox_cursor`). In the 2004 game's `ifs_teamstats` Lua,
        // populating the right-hand list (`side == 2`) last resets `ListManager`'s pulse owner to the
        // right list, so `side == 2` pulses between `0.5` and `1.0` while `side == 1` stays at `1.0`.
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

        // Compute the maximum pixel/interface width allowed for player names on this side:
        // in offline matches or on the end-of-match table, names may use the full `0.65 * inner`
        // (`list.name_w`), whereas in network `Peek` mode they are scaled by `PING_NAME_SHARE`
        // (`0.50 / 0.65`) to reserve `0.15 * inner` for the `PING` column before `KILLS`.
        let max_name_w = if ping {
            list.name_w * PING_NAME_SHARE
        } else {
            list.name_w
        };
        // Render each visible player row `k` (`0..layout.shown`):
        // - Color-code the local player (`row.local`) in `WHITE`, remote human players (`row.human`)
        //   in yellow (`HUMAN_COLOR`), and AI bots in grey (`BOT_COLOR`).
        // - Uppercase and clip the player's name to `max_name_w` (`0.65 * inner` offline, or
        //   `0.50 * inner` when the network `PING` column is active), then draw it left-aligned
        //   at `list.name_x` using its per-cell `TitleIntro` (`intros[side_idx][k][0]`).
        // - Draw `kills`, `deaths`, and `points` right-aligned (`align = 1`) at `list.columns[0..3]`
        //   using `intros[side_idx][k][1..4]`, followed by `ping` at `ping_x` (`intros[side_idx][k][4]`)
        //   when in network `Peek` mode.
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
            // Draw the three numeric columns (`KILLS`, `DEATHS`, `CP POINTS`) right-aligned
            // (`align = 1`) at `list.columns[0..3]` with horizontal glyph compression `ROW_SCALE` (`0.8`),
            // followed by the optional multiplayer `PING` column at `ping_x`.
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
            // Only human players in a network game have `row.ping = Some(ms)`; AI bots
            // (`row.ping == None`) leave the `PING` column blank, matching `ClassicView.drawSide`.
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

    /// Draws the up (`step < 0`) or down (`step > 0`) `small_arrow` scroll button (`drawArrow` in `src/scoreboard/classic/view.js`).
    ///
    /// Because the authored `small_arrow` texture points right (`+X`), the original Lua script
    /// rotates it 90 degrees by assigning diagonal UV corners (`[[1, 1], [0, 1], [1, 0], [0, 0]]`
    /// for up and `[[0, 1], [1, 1], [0, 0], [1, 0]]` for down), keeping the lit top edge of the
    /// arrow texture on the right side.
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

    /// Draws a nine-sliced row cursor frame (`listbox_cursor` or `listbox_hilight`) around visible row `k`
    /// (`drawCursor` in `src/scoreboard/classic/view.js`).
    ///
    /// Vertically offset by `layout.cursor_dy` (`+4.0` units below the row center) with height
    /// `PITCH + 10.0` (`37.0` units), matching `ListManager`'s cursor geometry.
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

    /// Truncates `text` one character at a time from the right until its width in `FONT_NAME_MEDIUM`
    /// at `ROW_SCALE` (`0.8`) fits within `max_width` (`clip` in `src/scoreboard/classic/view.js`).
    fn clip(surface: &dyn BattleUiSurface, text: &str, max_width: f64) -> String {
        let mut out = text.to_string();
        while !out.is_empty() && surface.text_width(FONT_NAME_MEDIUM, &out, ROW_SCALE) > max_width {
            out.pop();
        }
        out
    }

    /// Draws text playing a `TitleIntro` (`drawText` in `src/scoreboard/classic/view.js`).
    ///
    /// Reproduces the 2004 scoreboard's horizontal glyph compression (`h_scale = ROW_SCALE = 0.8`
    /// on table rows, `1.0` on sub-pages):
    /// 1. While `state.block` is active, a white cursor block of width `fh * h_scale` and height `fh`
    ///    sweeps across `[left, left + width]`.
    /// 2. The text itself is drawn with uniform scale `h_scale` inside `surface.draw_text`, and the
    ///    vertical scale of `scale_m` is set to `state.text / h_scale` around the line midpoint
    ///    `mid = y + fh / 2.0` so glyphs narrow horizontally to 80% while stretching back to 100%
    ///    full line height (`fh`) once `state.text` reaches `1.0`.
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
        // During the initial sweep of `TitleIntro`, draw the white `cursor` block of width
        // `fh * h_scale` and height `fh` moving left-to-right across the text's horizontal span.
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
        // Because `surface.draw_text` scales both X and Y by `h_scale` (`0.8` on table rows),
        // setting `scale_m[5] = state.text / h_scale` around the vertical midpoint `mid = y + fh / 2.0`
        // cancels out the vertical `h_scale` contraction once `state.text == 1.0`, leaving glyphs
        // narrowed to 80% width while preserving 100% full line height (`fh`).
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

    /// Draws the bottom action buttons on the `Table` page (`drawButtons` in `src/scoreboard/classic/view.js`):
    /// - Bottom-left: `"DONE"` on the end-of-match table (when `port.can_restart()` is true) or
    ///   `"BACK"` during in-battle `Peek`.
    /// - Bottom-right: `"AWARDS"` on the end-of-match table only.
    /// - Bottom-center under the window edge: `"DETAILS"` in `gamefont_small` over its 3-piece
    ///   tab bar (`backbutton_left`, `backbutton_mid`, `backbutton_right`, extending 22 units left
    ///   of the label and 20 units right of it, with square end caps of width `small_h`).
    pub(crate) fn draw_buttons(
        &self,
        surface: &dyn BattleUiSurface,
        batch: &mut Batch,
        m: Matrix,
        layout: &TeamStatsLayout,
        model: &ScoreboardModel,
        port: &dyn ScoreboardPort,
    ) {
        // On the end-of-match screen (`BoardState::End`), the bottom-left `"DONE"` button is
        // drawn only when `port.can_restart()` is true (in online matches, `DONE` is hidden
        // while waiting for the server's next launch), and `"AWARDS"` appears at the bottom-right.
        // During in-battle `Peek`, the bottom-left button reads `"BACK"` and is always shown.
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
        // Draw the 3-piece `"DETAILS"` tab bar (`backbutton_left`, `backbutton_mid`, `backbutton_right`)
        // centered under the main window's bottom edge: the bar extends 22 units left of the label
        // and 20 units right of it, with square end pieces of width `small_h`.
        let details = self.button_box("details", layout, model, surface);
        let x0 = details.x0 - 22.0;
        let x1 = details.x0 + details.width + 20.0;
        let y0 = details.top;
        let y1 = details.top + small_h;
        // Assemble the three horizontal slices of the `"DETAILS"` tab background (`backbutton_left`,
        // `backbutton_mid`, `backbutton_right`), falling back to a dark navy quad
        // (`rgba(16, 40, 80, 220)`) if the tab textures have not been extracted yet.
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

    /// Draws the bottom-left `"BACK"` button on the `DETAILS` and `AWARDS` sub-pages
    /// (`drawBack` in `src/scoreboard/classic/view.js`).
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

    /// Draws the 32x32 Classic `mousecursor` sprite (or an 8x8 solid white fallback quad)
    /// over all scoreboard layers (`drawPointer` in `src/scoreboard/classic/view.js`).
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

