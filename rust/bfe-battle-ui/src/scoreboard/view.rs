// Classic scoreboard view (`ClassicView` in `src/scoreboard/classic/view.js`).
//
// Owns the visual presentation state of the 2004 Classic scoreboard (`ifs_teamstats`,
// `ifs_personalstats`, and `ifs_awardstats`):
// - `intros`: a `2 x MAX_VISIBLE_ROWS x 5` table of `TitleIntro` instances for every visible
//   cell (`[name, kills, deaths, points, ping]`) on both team sides, restarted with a deterministic
//   `0.0..=0.15s` delay whenever the main `Table` page opens or returns from `DETAILS`/`AWARDS`.
// - `logos` and `icons`: 3D command-post hologram slots (`com_icon_republic`, `com_icon_cis`,
//   `com_icon_imperial`, `com_icon_alliance`) driven by linear [`Flight`] trajectories.
//   On `Table`, the two team logos grow from half size (`alpha = 0.0 -> 0.2`) over `0.4s` and spin
//   at `0.3` (left) and `0.25` (right) `OmegaY` (`30 deg/s` per unit). Entering `DETAILS` flies
//   the inspected team's logo at `20 OmegaY` (`600 deg/s`) to the top corner at half size
//   (`alpha = 0.4`), and returning to `Table` flies it back from `(at.x +- 0.15 * safe.w, at.y - 0.3 * safe.h)`.
//   Entering `AWARDS` spawns up to 8 mini team logos (`size = 0.2`, `alpha = 0.3`) that fly from
//   each winner's team logo position to the center gap at `+-20 OmegaY` before settling to a
//   deterministic per-row spin in `[-0.45, +0.45]`.
// - Hover animations (`hover_alpha`): hovered column sort icons and scroll arrows blink
//   asymmetrically (`0.4 -> 1.0` over `0.4s`, back to `0.4` over `0.2s`, matching `IFObj_UpdateBlinkyAnim`),
//   while hovered action buttons (`DONE`, `BACK`, `DETAILS`, `AWARDS`) pulse with a symmetric
//   triangle wave between `0.3` and `1.0` at `2.0` units/sec.

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

/// Row text color for AI bots (`rgba(128, 128, 128)` in `src/scoreboard/classic/view.js`).
pub(crate) const BOT_COLOR: [f64; 4] = [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0];

/// Row text color for non-local human players (`rgba(223, 255, 32)` yellow in `src/scoreboard/classic/view.js`; local player uses `WHITE`).
pub(crate) const HUMAN_COLOR: [f64; 4] = [223.0 / 255.0, 223.0 / 255.0, 32.0 / 255.0, 1.0];

/// Friendly team 3D hologram tint (`GREEN` in `src/scoreboard/classic/view.js`).
const GREEN_RGB: [f32; 3] = [32.0 / 255.0, 223.0 / 255.0, 32.0 / 255.0];

/// Enemy team 3D hologram tint (`RED` in `src/scoreboard/classic/view.js`).
const RED_RGB: [f32; 3] = [223.0 / 255.0, 32.0 / 255.0, 32.0 / 255.0];

/// Neutral team 3D hologram tint (`GREY` in `src/scoreboard/classic/view.js`).
const GREY_RGB: [f32; 3] = [192.0 / 255.0, 192.0 / 255.0, 192.0 / 255.0];

/// Horizontal glyph scale (`HScale = 0.8`) applied to player names and numeric columns on `Table`.
pub(crate) const ROW_SCALE: f64 = 0.8;

/// Maximum stagger delay in seconds before a table cell starts its `TitleIntro` (`INTRO_DELAY = 0.15`).
const INTRO_DELAY: f64 = 0.15;

/// Shell entry transition duration in seconds (`gShellAnimTime = 0.4s`, `ENTRY` in `src/scoreboard/classic/view.js`).
pub(crate) const ENTRY: f64 = 0.4;

/// Resting opacity of the two main team hologram logos on `Table` (`LOGO_ALPHA = 0.2`).
const LOGO_ALPHA: f64 = 0.2;

/// Resting spin rate (`0.3 * 30 deg/s = 9 deg/s`) for Team 1's left logo (`REST[1]` in `src/scoreboard/classic/view.js`).
const REST_LEFT: f64 = 0.3;

/// Resting spin rate (`0.25 * 30 deg/s = 7.5 deg/s`) for Team 2's right logo (`REST[2]` in `src/scoreboard/classic/view.js`).
const REST_RIGHT: f64 = 0.25;

/// Minimum opacity of the pulsing selection frame (`listbox_cursor`) on the right-hand list (`PULSE_LOW = 0.5`).
pub(crate) const PULSE_LOW: f64 = 0.5;

/// Minimum opacity of a hovered sort icon or scroll arrow (`BLINK.low = 0.4`).
const BLINK_LOW: f64 = 0.4;

/// Ramp-up duration in seconds (`0.4 -> 1.0`) for hovered sort icons and scroll arrows (`BLINK.up = 0.4`).
const BLINK_UP: f64 = 0.4;

/// Ramp-down duration in seconds (`1.0 -> 0.4`) for hovered sort icons and scroll arrows (`BLINK.down = 0.2`).
const BLINK_DOWN: f64 = 0.2;

/// Minimum opacity of a hovered bottom action button (`BUTTON_LOW = 0.3`).
const BUTTON_LOW: f64 = 0.3;

/// Triangle-wave rate in opacity units per second for a hovered button (`BUTTON_RATE = 2.0`).
const BUTTON_RATE: f64 = 2.0;

/// Side length in interface units of the `mousecursor` sprite (`POINTER = 32.0`).
pub(crate) const POINTER_SIZE: f64 = 32.0;

/// Multiplier narrowing `list.name_w` (`0.65 * inner` -> `0.50 * inner`) when the network `PING` column is shown.
pub(crate) const PING_NAME_SHARE: f64 = 0.5 / 0.65;

/// Landed scale multiplier for the 8 mini team logos on `AWARDS` (`AWARD_ICON = 0.2`).
const AWARD_ICON_SIZE: f64 = 0.2;

/// Maximum visible rows supported per team side in the pre-allocated `intros` table (`16` at 768p, `13` at 624p).
pub(crate) const MAX_VISIBLE_ROWS: usize = 24;

/// Texture names and sort modes for the three column header icons (`HEADERS` in `src/scoreboard/classic/view.js`).
pub(crate) const HEADERS: [(&str, SortMode); 3] = [
    ("stats_kills", SortMode::Kills),
    ("stats_deaths", SortMode::Deaths),
    ("stats_cpp", SortMode::Points),
];

/// Asymmetric hover blink waveform (`IFObj_UpdateBlinkyAnim`, `blink` in `src/scoreboard/classic/view.js`).
///
/// Ramps linearly from `0.4` to `1.0` over the first `0.4s` (`BLINK_UP`), then drops from `1.0`
/// back to `0.4` over `0.2s` (`BLINK_DOWN`), repeating every `0.6s`.
pub(crate) fn blink(t: f64) -> f64 {
    let period = BLINK_UP + BLINK_DOWN;
    let u = t.rem_euclid(period);
    if u < BLINK_UP {
        BLINK_LOW + (1.0 - BLINK_LOW) * (u / BLINK_UP)
    } else {
        1.0 - (1.0 - BLINK_LOW) * ((u - BLINK_UP) / BLINK_DOWN)
    }
}

/// Symmetric triangle-wave pulse between `1.0` and `low` starting at `1.0` (`pulse` in `src/scoreboard/classic/view.js`).
///
/// Used both for Team 2's `listbox_cursor` selection frame (`low = 0.5`, `rate = 1.0`) and for
/// hovered bottom buttons (`low = 0.3`, `rate = 2.0`).
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
/// Button bounding box and label returned by `ClassicScoreboardView::button_box`
/// (`buttonBox` in `src/scoreboard/classic/view.js`).
#[derive(Clone, Debug)]
pub struct ScoreButtonBox {
    /// Hotspot target emitted when this button is hovered or clicked (`Details`, `Awards`, `Done`, or `Back`).
    pub kind: ScoreboardHitTarget,
    /// Localized uppercase label string (`"DETAILS"`, `"AWARDS"`, `"DONE"`, or `"BACK"`).
    pub text: String,
    /// Left X coordinate of the button's text and hit box in `1024xH` interface units.
    pub x0: f64,
    /// Top Y coordinate of the button's text and hit box (`center_y - line_height / 2.0`).
    pub top: f64,
    /// Measured text width in interface units.
    pub width: f64,
    /// Font line height in interface units (`gamefont_small` for `DETAILS`, `gamefont_medium` for others).
    pub height: f64,
}

/// Internal 3D hologram model slot for a team logo (`self.logos`) or award icon (`self.icons`).
#[derive(Clone, Debug)]
pub(crate) struct LogoSlot {
    /// Hologram model asset name (`"com_icon_republic"`, `"com_icon_cis"`, etc.).
    pub name: String,
    /// Owning team number (`1` or `2`), used to resolve friendly/enemy/neutral hologram tint.
    pub team: i32,
    /// Linear position/scale/opacity interpolation and Y-axis spin state (`Flight`).
    pub flight: Flight,
}

/// Classic scoreboard view (`ClassicView` in `src/scoreboard/classic/view.js`).
pub struct ClassicScoreboardView {
    /// Regions published by a themed painter in the same interface coordinates
    /// as Classic. All pointer and keyboard clicks keep using this view query.
    pub theme_hotspots: Option<Vec<([f64; 4], ScoreboardHitTarget)>>,
    /// Game family (`Swbf1` 2004 layout; `Swbf2` returns `GameFamilyError`).
    pub family: GameFamily,
    /// Total elapsed seconds since `open()` was called on the view.
    pub time: f64,
    /// Elapsed seconds since `enter_table()` last transitioned onto `BoardPage::Table`
    /// (drives the `0.4s` `ENTRY` fade-in of sort headers and scroll arrows).
    pub entered: f64,
    /// `[side_idx][row_idx][col_idx]` flashy text intros (`intros` in `src/scoreboard/classic/view.js`).
    pub(crate) intros: [[[TitleIntro; 5]; MAX_VISIBLE_ROWS]; 2],
    /// Compact hover key (`ScoreboardHitTarget::hover_key`) of the element currently under the cursor.
    pub(crate) hovered: Option<&'static str>,
    /// Timestamp (`self.time`) when `self.hovered` last changed, anchoring the [`blink`] / [`pulse`] wave.
    pub(crate) hovered_at: f64,
    /// Team 1 (`logos[0]`) and Team 2 (`logos[1]`) 3D hologram slots.
    pub(crate) logos: [Option<LogoSlot>; 2],
    /// Up to 8 mini 3D team hologram slots displayed in the center gap on `BoardPage::Awards`.
    pub(crate) icons: Vec<LogoSlot>,
    /// `DETAILS` (`ifs_personalstats`) sub-page renderer.
    pub details_page: StatsPage,
    /// `AWARDS` (`ifs_awardstats`) sub-page renderer.
    pub awards_page: StatsPage,
    /// True while the pause menu is open over the scoreboard, suppressing the scoreboard's `mousecursor`.
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
            theme_hotspots: None,
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

    /// Maps a faction key (`"rep"`, `"cis"`, `"imp"`, `"all"`) to its default command-post
    /// hologram model name when `TeamHeader::logo` is `None` (`FALLBACK` in `src/scoreboard/classic/view.js`).
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
        // Synchronize `model.shown` with the current resolution's visible row count (`16` at 768p,
        // `13` at 624p) before ensuring the two 3D team hologram slots (`self.logos`) and starting
        // the table cell `TitleIntro` and hologram fade-in flights.
        let layout = team_stats_layout_for_family(self.family, height)
            .unwrap_or_else(|_| team_stats_layout(height));
        model.shown = layout.shown;
        self.ensure_logos(port);
        self.time = 0.0;
        self.enter_table(None, height);
    }

    /// Ensures `self.logos[0..2]` hold the current hologram model names for Team 1 and Team 2
    /// (`ensureLogos` in `src/scoreboard/classic/view.js`), preserving an existing `LogoSlot`
    /// (and its accumulated `spin`) when the model name has not changed.
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

    /// Rebuilds the 8 mini award hologram slots (`self.icons`) when entering `AWARDS`
    /// (`ensureIcons` in `src/scoreboard/classic/view.js`).
    ///
    /// For each award with a winning `team` (`1` or `2`), starts a `Flight` from that team's
    /// table logo position (`size = table.logo_size`, `alpha = 0.2`) to `page_layout.icons[k]`
    /// (`size = 0.2`, `alpha = 0.3`) over `ENTRY` (`0.4s`), spinning at `+-20 OmegaY` (`+-600 deg/s`)
    /// with `rest = None` until the flights land.
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
        // Reset `self.entered = 0.0` so column sort headers and scroll arrows fade in over
        // `ENTRY` (`0.4s`), and start every visible cell's `TitleIntro` (`side_idx` 0..2,
        // row `k` 0..shown, column `col` 0..5 for `[name, kills, deaths, points, ping]`)
        // with a deterministic pseudo-random delay in `[0.0, INTRO_DELAY]` (`0.0..=0.15s`).
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
        // Launch the two team hologram flights (`enterTable` in `src/scoreboard/classic/view.js`):
        // - If returning from `DETAILS` on `returning_side == Some(side)`, that side's logo flies
        //   back from `(at.x +- 0.15 * safe.w, at.y - 0.3 * safe.h)` at half size, `alpha = 0.4`,
        //   and fast spin (`omega = 20.0`).
        // - Otherwise the logo grows in place at `at` from half size and `alpha = 0.0` (`omega = 1.0`).
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
        // Transitioning to `DETAILS`: open `self.details_page` with the inspected character's
        // personal stat rows and fly that character's team hologram logo from its table center
        // (`at`) to the top window corner (`corner`) at half size (`table.logo_size / 2.0`),
        // `alpha = 0.4`, fast transit spin (`omega = 20.0`), and resting spin `0.3`.
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
        // Transitioning to `AWARDS`: open `self.awards_page` with the 8 formatted award rows
        // and launch the mini team-logo flights from each winner's side to `page_layout.icons[k]`.
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
        // Step the view's master clock `time` and table-entry clock `entered`, refresh
        // `model.shown` from the current interface height, and advance all active table-cell
        // `TitleIntro` animations and 3D team logo `Flight` state machines by `dt` seconds.
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
        // Once the first award icon finishes its 0.4s flight (`!flying()`), assign each icon a
        // deterministic resting spin rate in `[-0.45, +0.45]` (`update` in `src/scoreboard/classic/view.js`).
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

        // Hit-test the current cursor position to update `self.hovered`. Whenever the hovered
        // element's key changes, record `self.hovered_at = self.time` so `hover_alpha` starts
        // its blink or pulse waveform cleanly from `t = 0.0` (`1.0` initial opacity).
        let target = self.hit(cursor[0], cursor[1], model, port, surface);
        let key = target.map(|t| t.hover_key());
        if key != self.hovered {
            self.hovered = key;
            self.hovered_at = self.time;
        }
    }

    /// Returns the opacity multiplier for hotspot `key` (`hoverAlpha` in `src/scoreboard/classic/view.js`).
    ///
    /// Non-hovered elements render at `1.0`; hovered buttons pulse via [`pulse`] (`0.3..=1.0`),
    /// and hovered sort icons / scroll arrows blink via [`blink`] (`0.4..=1.0`).
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
        // Map each hologram's `team` to its relation color relative to the local player's
        // `model.team`: `GREEN_RGB` for friendly (`rel > 0`), `RED_RGB` for enemy (`rel < 0`),
        // and `GREY_RGB` for neutral (`rel == 0`).
        // Team logos are visible on `Table` and (for the inspected team only) on `Details`;
        // mini award icons are visible only on `Awards`.
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

        // Emit 3D placement transforms for the two main team hologram logos (`self.logos[0..2]`):
        // - Each logo's 2D plane coordinates `(st.x, st.y)` are projected into 3D interface-camera
        //   space via `surface.place_on_plane(st.x, st.y)`.
        // - World scale is `st.size.max(1e-3) * LOGO_UNITS` (`LOGO_UNITS = 14.0`), and `rotation_y`
        //   is the accumulated `logo.flight.spin` in radians.
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

        // Emit 3D placement transforms for the up to 8 mini award hologram icons (`self.icons`),
        // which spin in the center gap between each award's label and winner on `BoardPage::Awards`.
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
        // - `"details"`: centered horizontally at `layout.details[0]` using `FONT_NAME_SMALL`
        //   with `DISPLAY_PAD` (`8.0` units) added to the centered box width.
        // - `"awards"`: right-aligned in `FONT_NAME_MEDIUM` ending `STRING_PAD` (`5.0` units)
        //   left of `layout.awards_right`.
        // - `"done"` / `"back"`: left-aligned in `FONT_NAME_MEDIUM` at `layout.done[0]`, reading
        //   `"DONE"` on the end-of-match `Table` page and `"BACK"` everywhere else.
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
        if let Some(regions) = &self.theme_hotspots {
            return regions.iter().rev().find_map(|(r, target)| {
                (x >= r[0] && y >= r[1] && x <= r[2] && y <= r[3])
                    .then_some(*target)
            });
        }
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

        // On `DETAILS` and `AWARDS`, only the bottom-left `"BACK"` button is interactive.
        // On `Table`, test both teams' column sort headers, scroll arrows, and visible player rows
        // before testing the bottom action buttons (`DONE`/`BACK`, `AWARDS`, `DETAILS`).
        if model.page != BoardPage::Table {
            return check_button("back");
        }

        // Check each team side (`1` = left, `2` = right) in order:
        // 1. The three `30x30` column sort icons (`stats_kills`, `stats_deaths`, `stats_cpp`).
        // 2. The `30x30` up/down scroll arrows centered at `list.up` and `list.down` (active only
        //    when `model.can_scroll_up()` / `model.can_scroll_down()` is true).
        // 3. The visible player rows via `row_at(&layout, side, x, y)` (active only when a
        //    non-blank `ScoreRow` exists at `model.first + k`).
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

        // Test the bottom action buttons (`"done"`/`"back"`, `"awards"`, `"details"`):
        // - `"awards"` is only present on the end-of-match table (`BoardState::End`).
        // - `"done"` on the end-of-match table is skipped when `!port.can_restart()` (e.g. an
        //   online client waiting for the server's next map launch).
        // - `"done"` and `"awards"` include a 5-unit right-edge hit padding (`pad = 5.0`), whereas
        //   `"details"` uses its exact `button_box` width (`pad = 0.0`).
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
