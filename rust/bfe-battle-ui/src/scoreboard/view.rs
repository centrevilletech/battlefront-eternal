// Classic scoreboard view (`ClassicView` in `src/scoreboard/classic/view.js`).

use super::input::ScoreboardHitTarget;
use super::layout::{
    row_at, stats_page_layout, team_stats_layout, team_stats_layout_for_family, TeamStatsLayout,
    AWARDS_ROWS, DETAILS_ROWS, DISPLAY_PAD, ICON, STRING_PAD,
};
use super::model::{BoardPage, BoardState, ScoreboardModel, SortMode};
use super::pages::{hash_delay, Flight, FlightPoint, StatsPage, StatsPageRow};
use super::stats::{AwardRow, ScoreboardPort};
use crate::shapes::{
    BattleUiSurface, GameFamily, GameFamilyError, TitleIntro, FONT_NAME_MEDIUM, FONT_NAME_SMALL,
    LOGO_UNITS,
};

pub use super::controller::ScoreboardController;
pub use super::pages::LogoPlacement;

pub(crate) const BOT_COLOR: [f64; 4] = [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0];
pub(crate) const HUMAN_COLOR: [f64; 4] = [223.0 / 255.0, 223.0 / 255.0, 32.0 / 255.0, 1.0];
const GREEN_RGB: [f32; 3] = [32.0 / 255.0, 223.0 / 255.0, 32.0 / 255.0];
const RED_RGB: [f32; 3] = [223.0 / 255.0, 32.0 / 255.0, 32.0 / 255.0];
const GREY_RGB: [f32; 3] = [192.0 / 255.0, 192.0 / 255.0, 192.0 / 255.0];

pub(crate) const ROW_SCALE: f64 = 0.8;
const INTRO_DELAY: f64 = 0.15;
pub(crate) const ENTRY: f64 = 0.4;
const LOGO_ALPHA: f64 = 0.2;
const REST_LEFT: f64 = 0.3;
const REST_RIGHT: f64 = 0.25;
pub(crate) const PULSE_LOW: f64 = 0.5;
const BLINK_LOW: f64 = 0.4;
const BLINK_UP: f64 = 0.4;
const BLINK_DOWN: f64 = 0.2;
const BUTTON_LOW: f64 = 0.3;
const BUTTON_RATE: f64 = 2.0;
pub(crate) const POINTER_SIZE: f64 = 32.0;
pub(crate) const PING_NAME_SHARE: f64 = 0.5 / 0.65;
const AWARD_ICON_SIZE: f64 = 0.2;
pub(crate) const MAX_VISIBLE_ROWS: usize = 24;

pub(crate) const HEADERS: [(&str, SortMode); 3] = [
    ("stats_kills", SortMode::Kills),
    ("stats_deaths", SortMode::Deaths),
    ("stats_cpp", SortMode::Points),
];

pub(crate) fn blink(t: f64) -> f64 {
    let period = BLINK_UP + BLINK_DOWN;
    let u = t.rem_euclid(period);
    if u < BLINK_UP {
        BLINK_LOW + (1.0 - BLINK_LOW) * (u / BLINK_UP)
    } else {
        1.0 - (1.0 - BLINK_LOW) * ((u - BLINK_UP) / BLINK_DOWN)
    }
}

pub(crate) fn pulse(t: f64, low: f64, rate: f64) -> f64 {
    let span = 1.0 - low;
    let u = (t * rate).rem_euclid(2.0 * span);
    if u < span {
        1.0 - u
    } else {
        low + (u - span)
    }
}

/// Button bounding box and label returned by `ClassicScoreboardView::button_box`.
#[derive(Clone, Debug)]
pub struct ScoreButtonBox {
    pub kind: ScoreboardHitTarget,
    pub text: String,
    pub x0: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct LogoSlot {
    pub name: String,
    pub team: i32,
    pub flight: Flight,
}

/// Classic scoreboard view (`ClassicView` in `src/scoreboard/classic/view.js`).
pub struct ClassicScoreboardView {
    pub family: GameFamily,
    pub time: f64,
    pub entered: f64,
    /// `[side_idx][row_idx][col_idx]` flashy text intros (`intros` in `src/scoreboard/classic/view.js`).
    pub(crate) intros: [[[TitleIntro; 5]; MAX_VISIBLE_ROWS]; 2],
    pub(crate) hovered: Option<&'static str>,
    pub(crate) hovered_at: f64,
    pub(crate) logos: [Option<LogoSlot>; 2],
    pub(crate) icons: Vec<LogoSlot>,
    pub details_page: StatsPage,
    pub awards_page: StatsPage,
    pub hide_pointer: bool,
}

impl Default for ClassicScoreboardView {
    fn default() -> Self {
        Self::new()
    }
}

impl ClassicScoreboardView {
    /// Creates a new `ClassicScoreboardView`.
    pub fn new() -> Self {
        Self {
            family: GameFamily::Swbf1,
            time: 0.0,
            entered: 0.0,
            intros: [[[TitleIntro::default(); 5]; MAX_VISIBLE_ROWS]; 2],
            hovered: None,
            hovered_at: 0.0,
            logos: [None, None],
            icons: Vec::with_capacity(8),
            details_page: StatsPage::new(true),
            awards_page: StatsPage::new(false),
            hide_pointer: false,
        }
    }

    /// Constructs a `ClassicScoreboardView` for `family`, returning [`GameFamilyError`]
    /// if `family` is [`GameFamily::Swbf2`].
    pub fn try_with_family(family: GameFamily) -> Result<Self, GameFamilyError> {
        family.require_available()?;
        Ok(Self::new())
    }

    fn fallback_logo(side: Option<&str>) -> Option<&'static str> {
        match side {
            Some("rep") => Some("com_icon_republic"),
            Some("cis") => Some("com_icon_cis"),
            Some("imp") => Some("com_icon_imperial"),
            Some("all") => Some("com_icon_alliance"),
            _ => None,
        }
    }

    /// Initializes the view when the scoreboard opens (`open` in `src/scoreboard/classic/view.js`).
    pub fn open(&mut self, model: &mut ScoreboardModel, port: &dyn ScoreboardPort, height: f64) {
        let layout = team_stats_layout_for_family(self.family, height)
            .unwrap_or_else(|_| team_stats_layout(height));
        model.shown = layout.shown;
        self.ensure_logos(port);
        self.time = 0.0;
        self.enter_table(None, height);
    }

    fn ensure_logos(&mut self, port: &dyn ScoreboardPort) {
        let teams = port.teams();
        for (idx, team_num) in [1i32, 2i32].into_iter().enumerate() {
            let name = teams[idx].as_ref().and_then(|t| {
                t.logo
                    .clone()
                    .or_else(|| Self::fallback_logo(t.side.as_deref()).map(|s| s.to_string()))
            });
            if let Some(name) = name {
                let same = self.logos[idx]
                    .as_ref()
                    .map(|l| l.name == name)
                    .unwrap_or(false);
                if !same {
                    self.logos[idx] = Some(LogoSlot {
                        name,
                        team: team_num,
                        flight: Flight::default(),
                    });
                }
            } else {
                self.logos[idx] = None;
            }
        }
    }

    fn ensure_icons(&mut self, awards: &[AwardRow], port: &dyn ScoreboardPort, height: f64) {
        self.icons.clear();
        let teams = port.teams();
        let page_layout = stats_page_layout(height, AWARDS_ROWS);
        let table = team_stats_layout(height);
        for (k, award) in awards.iter().enumerate().take(8) {
            let Some(team) = award.team else {
                continue;
            };
            let idx = if team == 2 { 1 } else { 0 };
            let name = teams[idx].as_ref().and_then(|t| {
                t.logo
                    .clone()
                    .or_else(|| Self::fallback_logo(t.side.as_deref()).map(|s| s.to_string()))
            });
            let Some(name) = name else {
                continue;
            };
            let start = table.side(if team == 2 { 2 } else { 1 }).logo;
            let at = page_layout.icons[k];
            let mut flight = Flight::default();
            let omega = if k % 2 == 0 { -20.0 } else { 20.0 };
            flight.start(
                FlightPoint {
                    x: start[0],
                    y: start[1],
                    size: table.logo_size,
                    alpha: 0.2,
                },
                FlightPoint {
                    x: at[0],
                    y: at[1],
                    size: AWARD_ICON_SIZE,
                    alpha: 0.3,
                },
                ENTRY,
                omega,
                None,
            );
            self.icons.push(LogoSlot { name, team, flight });
        }
    }

    /// Replays row intros and starts logo flights when entering `Table` (`enterTable` in `src/scoreboard/classic/view.js`).
    pub fn enter_table(&mut self, returning_side: Option<usize>, height: f64) {
        self.entered = 0.0;
        let layout = team_stats_layout(height);
        let shown = layout.shown.min(MAX_VISIBLE_ROWS);
        for side_idx in 0..2 {
            for k in 0..shown {
                for col in 0..5 {
                    let seed = (side_idx * 256 + k * 8 + col + 1) as u32;
                    self.intros[side_idx][k][col].start(hash_delay(seed, INTRO_DELAY));
                }
            }
        }
        let full = layout.logo_size;
        for (idx, side) in [1usize, 2usize].into_iter().enumerate() {
            if let Some(logo) = &mut self.logos[idx] {
                let at = layout.side(side).logo;
                let back = returning_side == Some(side);
                let sign = if side == 1 { -1.0 } else { 1.0 };
                let from = if back {
                    FlightPoint {
                        x: at[0] + sign * 0.15 * layout.safe.w,
                        y: at[1] - 0.3 * layout.safe.h,
                        size: full / 2.0,
                        alpha: 0.4,
                    }
                } else {
                    FlightPoint {
                        x: at[0],
                        y: at[1],
                        size: full / 2.0,
                        alpha: 0.0,
                    }
                };
                let rest = if side == 1 { REST_LEFT } else { REST_RIGHT };
                logo.flight.start(
                    from,
                    FlightPoint {
                        x: at[0],
                        y: at[1],
                        size: full,
                        alpha: LOGO_ALPHA,
                    },
                    ENTRY,
                    if back { 20.0 } else { 1.0 },
                    Some(rest),
                );
            }
        }
    }

    /// Reacts to a page transition (`pageChanged` in `src/scoreboard/classic/view.js`).
    pub fn page_changed(
        &mut self,
        page: BoardPage,
        from: BoardPage,
        side: Option<usize>,
        model: &ScoreboardModel,
        port: &dyn ScoreboardPort,
        height: f64,
    ) {
        if page == BoardPage::Table {
            let returning = if from == BoardPage::Details {
                side
            } else {
                None
            };
            self.enter_table(returning, height);
            return;
        }
        if page == BoardPage::Details {
            let slot = model.subject.as_ref().map(|s| s.slot).unwrap_or(0);
            let rows = port
                .personal(slot)
                .into_iter()
                .map(|r| StatsPageRow {
                    key: r.key,
                    label: r.label,
                    value: r.value,
                })
                .collect();
            self.details_page.open(rows);
            if let Some(subj) = &model.subject {
                let idx = if subj.side == 2 { 1 } else { 0 };
                if let Some(logo) = &mut self.logos[idx] {
                    let table = team_stats_layout(height);
                    let at = table.side(subj.side).logo;
                    let corner = stats_page_layout(height, DETAILS_ROWS).corner(subj.side);
                    logo.flight.start(
                        FlightPoint {
                            x: at[0],
                            y: at[1],
                            size: table.logo_size,
                            alpha: LOGO_ALPHA,
                        },
                        FlightPoint {
                            x: corner[0],
                            y: corner[1],
                            size: table.logo_size / 2.0,
                            alpha: 0.4,
                        },
                        ENTRY,
                        20.0,
                        Some(0.3),
                    );
                }
            }
            return;
        }
        if page == BoardPage::Awards {
            let award_rows = port.awards();
            let rows = award_rows
                .iter()
                .map(|a| StatsPageRow {
                    key: a.key,
                    label: a.label,
                    value: a.text.clone(),
                })
                .collect();
            self.awards_page.open(rows);
            self.ensure_icons(&award_rows, port, height);
        }
    }

    /// Advances animations, text intros, logo flights, and hover state (`update` in `src/scoreboard/classic/view.js`).
    pub fn update(
        &mut self,
        dt: f64,
        model: &mut ScoreboardModel,
        port: &dyn ScoreboardPort,
        surface: &dyn BattleUiSurface,
        cursor: [f64; 2],
    ) {
        self.time += dt;
        self.entered += dt;
        let layout = team_stats_layout(surface.height());
        model.shown = layout.shown;
        let shown = layout.shown.min(MAX_VISIBLE_ROWS);
        for side_idx in 0..2 {
            for k in 0..shown {
                for col in 0..5 {
                    self.intros[side_idx][k][col].update(dt);
                }
            }
        }
        for logo in self.logos.iter_mut().flatten() {
            logo.flight.update(dt);
        }
        let landed = self
            .icons
            .first()
            .map(|ic| !ic.flight.flying())
            .unwrap_or(false);
        for (i, icon) in self.icons.iter_mut().enumerate() {
            icon.flight.update(dt);
            if landed && icon.flight.rest.is_none() {
                icon.flight.rest = Some(hash_delay(i as u32 + 99, 0.9) - 0.45);
            }
        }
        match model.page {
            BoardPage::Details => {
                if let Some(subj) = &model.subject {
                    let rows = port
                        .personal(subj.slot)
                        .into_iter()
                        .map(|r| StatsPageRow {
                            key: r.key,
                            label: r.label,
                            value: r.value,
                        })
                        .collect();
                    self.details_page.refresh(rows);
                }
                self.details_page.update(dt);
            }
            BoardPage::Awards => {
                self.awards_page.update(dt);
            }
            BoardPage::Table => {}
        }

        let target = self.hit(cursor[0], cursor[1], model, port, surface);
        let key = target.map(|t| t.hover_key());
        if key != self.hovered {
            self.hovered = key;
            self.hovered_at = self.time;
        }
    }

    pub(crate) fn hover_alpha(&self, key: &str, is_button: bool) -> f64 {
        if self.hovered != Some(key) {
            return 1.0;
        }
        let t = self.time - self.hovered_at;
        if is_button {
            pulse(t, BUTTON_LOW, BUTTON_RATE)
        } else {
            blink(t)
        }
    }

    /// Computes the current 3D placements for the team hologram logos and award icons (`renderModels` in `src/scoreboard/classic/view.js`).
    pub fn logo_placements(
        &self,
        model: &ScoreboardModel,
        port: &dyn ScoreboardPort,
        surface: &dyn BattleUiSurface,
    ) -> Vec<LogoPlacement> {
        let mut out = Vec::with_capacity(10);
        let team_color = |team: i32| -> [f32; 3] {
            let rel = port.relation(model.team, team);
            if rel > 0 {
                GREEN_RGB
            } else if rel < 0 {
                RED_RGB
            } else {
                GREY_RGB
            }
        };

        for (idx, side) in [1usize, 2usize].into_iter().enumerate() {
            if let Some(logo) = &self.logos[idx] {
                let visible = model.page == BoardPage::Table
                    || (model.page == BoardPage::Details
                        && model.subject.as_ref().map(|s| s.side) == Some(side));
                let st = logo.flight.state();
                let pos = surface.place_on_plane(st.x, st.y);
                out.push(LogoPlacement {
                    model_name: logo.name.clone(),
                    team: logo.team,
                    position: [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                    scale: (st.size.max(1e-3) * LOGO_UNITS) as f32,
                    rotation_y: logo.flight.spin as f32,
                    color: team_color(logo.team),
                    opacity: st.alpha as f32,
                    visible: visible && st.alpha > 0.001,
                });
            }
        }

        for icon in &self.icons {
            let visible = model.page == BoardPage::Awards;
            let st = icon.flight.state();
            let pos = surface.place_on_plane(st.x, st.y);
            out.push(LogoPlacement {
                model_name: icon.name.clone(),
                team: icon.team,
                position: [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                scale: (st.size.max(1e-3) * LOGO_UNITS) as f32,
                rotation_y: icon.flight.spin as f32,
                color: team_color(icon.team),
                opacity: st.alpha as f32,
                visible: visible && st.alpha > 0.001,
            });
        }
        out
    }

    /// Computes the bounding box and label for `"details"`, `"awards"`, `"done"`, or `"back"` (`buttonBox` in `src/scoreboard/classic/view.js`).
    pub fn button_box(
        &self,
        kind: &str,
        layout: &TeamStatsLayout,
        model: &ScoreboardModel,
        surface: &dyn BattleUiSurface,
    ) -> ScoreButtonBox {
        if kind == "details" {
            let text = surface.string("ifs.Stats.details", "DETAILS").to_string();
            let width = surface.text_width(FONT_NAME_SMALL, &text, 1.0);
            let h = surface.line_height(FONT_NAME_SMALL);
            return ScoreButtonBox {
                kind: ScoreboardHitTarget::Details,
                text,
                x0: layout.details[0] - (width + DISPLAY_PAD) / 2.0,
                top: layout.details[1] - h / 2.0,
                width,
                height: h,
            };
        }
        let h = surface.line_height(FONT_NAME_MEDIUM);
        if kind == "awards" {
            let text = surface.string("ifs.Stats.awards", "AWARDS").to_string();
            let width = surface.text_width(FONT_NAME_MEDIUM, &text, 1.0);
            return ScoreButtonBox {
                kind: ScoreboardHitTarget::Awards,
                text,
                x0: layout.awards_right - width - STRING_PAD,
                top: layout.awards_y - h / 2.0,
                width,
                height: h,
            };
        }
        let is_done =
            kind == "done" && model.state == BoardState::End && model.page == BoardPage::Table;
        let text = if is_done {
            surface.string("ifs.stats.done", "DONE").to_string()
        } else {
            surface.string("common.back", "BACK").to_string()
        };
        let width = surface.text_width(FONT_NAME_MEDIUM, &text, 1.0);
        ScoreButtonBox {
            kind: if is_done {
                ScoreboardHitTarget::Done
            } else {
                ScoreboardHitTarget::Back
            },
            text,
            x0: layout.done[0],
            top: layout.done[1] - h / 2.0,
            width,
            height: h,
        }
    }

    /// Hit-tests interface point `(x, y)` against column headers, scroll arrows, rows, and buttons (`hit` in `src/scoreboard/classic/view.js`).
    pub fn hit(
        &self,
        x: f64,
        y: f64,
        model: &ScoreboardModel,
        port: &dyn ScoreboardPort,
        surface: &dyn BattleUiSurface,
    ) -> Option<ScoreboardHitTarget> {
        let layout = team_stats_layout(surface.height());
        let inside = |x0: f64, y0: f64, x1: f64, y1: f64| x >= x0 && x <= x1 && y >= y0 && y <= y1;
        let check_button = |kind: &str| -> Option<ScoreboardHitTarget> {
            let box_rect = self.button_box(kind, &layout, model, surface);
            let pad = if kind == "details" { 0.0 } else { 5.0 };
            if inside(
                box_rect.x0,
                box_rect.top,
                box_rect.x0 + box_rect.width + pad,
                box_rect.top + box_rect.height,
            ) {
                Some(box_rect.kind)
            } else {
                None
            }
        };

        if model.page != BoardPage::Table {
            return check_button("back");
        }

        for side in [1usize, 2usize] {
            let list = layout.side(side);
            for (j, &(_, mode)) in HEADERS.iter().enumerate() {
                let at = list.headers[j];
                if inside(at[0], at[1], at[0] + ICON, at[1] + ICON) {
                    return Some(ScoreboardHitTarget::Sort { mode, side });
                }
            }
            let h = ICON / 2.0;
            if model.can_scroll_up()
                && inside(
                    list.up[0] - h,
                    list.up[1] - h,
                    list.up[0] + h,
                    list.up[1] + h,
                )
            {
                return Some(ScoreboardHitTarget::Scroll { step: -1, side });
            }
            if model.can_scroll_down()
                && inside(
                    list.down[0] - h,
                    list.down[1] - h,
                    list.down[0] + h,
                    list.down[1] + h,
                )
            {
                return Some(ScoreboardHitTarget::Scroll { step: 1, side });
            }
            if let Some(k) = row_at(&layout, side, x, y) {
                if model.row(side, model.first + k).is_some() {
                    return Some(ScoreboardHitTarget::Row {
                        side,
                        index: model.first + k,
                    });
                }
            }
        }

        let buttons: &[&str] = if model.state == BoardState::End {
            &["done", "awards", "details"]
        } else {
            &["done", "details"]
        };
        for &kind in buttons {
            if kind == "done" && model.state == BoardState::End && !port.can_restart() {
                continue;
            }
            if let Some(found) = check_button(kind) {
                return Some(found);
            }
        }
        None
    }
}
