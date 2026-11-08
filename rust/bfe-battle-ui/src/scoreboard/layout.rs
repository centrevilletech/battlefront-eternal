// Classic scoreboard geometry in the 1024xH interface space.
//
// Ports `src/scoreboard/classic/layout.js` (`teamStatsLayout`, `rowAt`,
// `statsPageLayout`, `DETAILS_ROWS`, `AWARDS_ROWS`).
//
// Layout structure from the 2004 game's `ifs_teamstats`, `ifs_personalstats`, and `ifs_awardstats`
// Lua scripts, calibrated against retail Geonosis end-of-match captures:
// - All coordinates live in the 1024xH interface plane (`H = 768` at 4:3, `H = 624` at 16:9),
//   bounded by the 90% safe area (`SAFE = 0.05`, `SAFE_SCALE = 0.9`).
// - The main nine-sliced window (`border_3_pieces`) spans the full safe width and `0.82` of
//   the safe height, centered at `0.48` down the safe area.
// - Two side-by-side team lists (Team 1 on the left, Team 2 on the right) sit centered at `0.53`
//   down the safe area with rows spaced `PITCH = 27.0` units apart (`16` rows at 4:3, `13` at 16:9).
// - Inside each row, the player name occupies `0.65` of `inner` width starting `2.0` units from
//   `left`, while `KILLS`, `DEATHS`, and `CP POINTS` are right-aligned in three columns each
//   occupying `COLUMN = 0.133` of `inner` width at the right end of the row.

use crate::shapes::{GameFamily, GameFamilyError, SAFE, SAFE_SCALE, W};

/// Row height and vertical pitch in interface units (`PITCH` in `src/scoreboard/classic/layout.js`).
pub const PITCH: f64 = 27.0;

/// Side length of a column sort icon or scroll arrow (`ICON` in `src/scoreboard/classic/layout.js`).
pub const ICON: f64 = 30.0;

/// Share of the row inner width occupied by each numeric column (`COLUMN = 0.133` in `src/scoreboard/classic/layout.js`).
pub const COLUMN: f64 = 0.133;

/// Fractional horizontal offsets for the three column header icons (`HEADER` in `src/scoreboard/classic/layout.js`).
///
/// Positions the centers of `stats_kills` (`0.2`), `stats_deaths` (`0.32`), and `stats_cpp` (`0.44`)
/// across each team's half of the safe area.
pub const HEADER: [f64; 3] = [0.2, 0.32, 0.44];

/// Extra label width padding (5 units) shifting `AWARDS` left of `awards_right` (`STRING_PAD` in `src/scoreboard/classic/layout.js`).
pub const STRING_PAD: f64 = 5.0;

/// Extra display box padding (8 units) centering `DETAILS` on its bottom tab (`DISPLAY_PAD` in `src/scoreboard/classic/layout.js`).
pub const DISPLAY_PAD: f64 = 8.0;

/// Rectangular box `{ x, y, w, h }` in interface coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Layout geometry for one team side (`sides[1]` or `sides[2]` in `src/scoreboard/classic/layout.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeamSideLayout {
    /// Horizontal center of this team's list column.
    pub x: f64,
    /// Left edge of the rows in this team's list (`x - inner / 2.0`).
    pub left: f64,
    /// Right edge of the row hotspot (`left + width`).
    pub right: f64,
    /// Bounding box for the team title (`"REPUBLIC : 150"`).
    pub title: ScoreRect,
    /// Left X coordinate where player names start (`left + 2.0`).
    pub name_x: f64,
    /// Maximum width available for player names (`0.65 * inner`, narrowed when `PING` is shown).
    pub name_w: f64,
    /// Right-aligned X coordinates for the `[KILLS, DEATHS, POINTS]` columns.
    pub columns: [f64; 3],
    /// Top-left `[x, y]` positions of the three `30x30` column sort icons.
    pub headers: [[f64; 2]; 3],
    /// Center `[x, y]` of the up-scroll arrow.
    pub up: [f64; 2],
    /// Center `[x, y]` of the down-scroll arrow.
    pub down: [f64; 2],
    /// Center `[x, y]` of the 3D spinning team hologram logo (`0.25` or `0.75` across the safe area).
    pub logo: [f64; 2],
}

/// Complete Classic team stats table layout (`teamStatsLayout` in `src/scoreboard/classic/layout.js`).
///
/// Includes the 90% safe area (`safe`), nine-sliced `window`, per-team list geometries (`sides`),
/// `ListManager` selection cursor metrics (`cursor_w`, `cursor_h = PITCH + 10`, `cursor_dy = 4`),
/// 3D hologram `logo_size` (`W / 800.0`), and bottom button anchors (`details`, `done`, `awards_*`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TeamStatsLayout {
    pub h: f64,
    pub safe: ScoreRect,
    pub window: ScoreRect,
    /// `sides[0]` is team 1 (left), `sides[1]` is team 2 (right).
    pub sides: [TeamSideLayout; 2],
    pub shown: usize,
    pub top: f64,
    pub width: f64,
    pub cursor_w: f64,
    pub cursor_h: f64,
    pub cursor_dy: f64,
    pub text_top: f64,
    pub logo_size: f64,
    pub details: [f64; 2],
    pub done: [f64; 2],
    pub awards_right: f64,
    pub awards_y: f64,
}

impl TeamStatsLayout {
    /// Returns the `TeamSideLayout` for 1-based `side` (`1` or `2`).
    #[inline]
    pub fn side(&self, side: usize) -> &TeamSideLayout {
        if side == 2 {
            &self.sides[1]
        } else {
            &self.sides[0]
        }
    }
}

/// Selects the scoreboard table layout for `family` (`MULTIPLAYER.md`).
///
/// Only [`GameFamily::Swbf1`] (2004) is implemented; [`GameFamily::Swbf2`] returns an
/// explicit [`GameFamilyError`] and never silently falls back to the 2004 layout.
pub fn team_stats_layout_for_family(
    family: GameFamily,
    h: f64,
) -> Result<TeamStatsLayout, GameFamilyError> {
    family.require_available()?;
    Ok(team_stats_layout(h))
}

/// Computes the Classic scoreboard table layout for interface height `h` (`teamStatsLayout` in `src/scoreboard/classic/layout.js`).
pub fn team_stats_layout(h: f64) -> TeamStatsLayout {
    let sw = SAFE_SCALE * W;
    let sh = SAFE_SCALE * h;
    let ox = SAFE * W;
    let oy = SAFE * h;
    let at = |rx: f64, ry: f64| [ox + rx * sw, oy + ry * sh];

    let height = 0.82 * sh;
    let half = 0.5 * sw;
    let win_c = at(0.5, 0.48);
    let window = ScoreRect {
        x: win_c[0],
        y: win_c[1],
        w: sw,
        h: height,
    };
    let lists = at(0.5, 0.53);
    let buttons = at(0.5, 0.5);
    let width = half - 20.0;
    let inner = width - 10.0;
    let column = COLUMN * inner;
    let shown = (((0.85 * height) / PITCH).floor() as isize - 1).max(1) as usize;
    let top = lists[1] - (shown as f64 * PITCH) / 2.0 + PITCH / 2.0;

    // Build symmetrical left (`sign = -1.0`) and right (`sign = 1.0`) team list columns.
    let build_side = |side: usize, sign: f64| -> TeamSideLayout {
        let x = lists[0] + (sign * half) / 2.0;
        let left = x - inner / 2.0;
        let right = left + width;
        let title = ScoreRect {
            x: x - half / 2.0 + 15.0,
            y: lists[1] - height / 2.0,
            w: half / 2.0,
            h: 30.0,
        };
        let columns = [
            left + inner - column * 2.0,
            left + inner - column,
            left + inner,
        ];
        let header_y = buttons[1] - height / 2.0 + 0.03 * h;
        let headers = [
            [
                buttons[0] + half * HEADER[0] - ICON / 2.0 + (sign * half) / 2.0,
                header_y,
            ],
            [
                buttons[0] + half * HEADER[1] - ICON / 2.0 + (sign * half) / 2.0,
                header_y,
            ],
            [
                buttons[0] + half * HEADER[2] - ICON / 2.0 + (sign * half) / 2.0,
                header_y,
            ],
        ];
        let arrow_x = buttons[0] + (sign * half) / 2.0;
        let up = [arrow_x, buttons[1] - height / 2.0 + ICON * 2.5 - 10.0];
        let down = [arrow_x, buttons[1] + height / 2.0 - ICON * 1.5 + 15.0];
        let logo = at(if side == 1 { 0.25 } else { 0.75 }, 0.5);

        TeamSideLayout {
            x,
            left,
            right,
            title,
            name_x: left + 2.0,
            name_w: 0.65 * inner,
            columns,
            headers,
            up,
            down,
            logo,
        }
    };

    TeamStatsLayout {
        h,
        safe: ScoreRect {
            x: ox,
            y: oy,
            w: sw,
            h: sh,
        },
        window,
        sides: [build_side(1, -1.0), build_side(2, 1.0)],
        shown,
        top,
        width,
        cursor_w: width,
        cursor_h: PITCH + 10.0,
        cursor_dy: 4.0,
        text_top: -10.0,
        logo_size: W / 800.0,
        details: [buttons[0], buttons[1] + height * 0.485 + 4.0],
        done: [ox + 20.0, oy + sh - 15.0],
        awards_right: ox + sw - 15.0,
        awards_y: oy + sh - 15.0,
    }
}

/// Returns the visible row index (`0..layout.shown`) under interface point `(x, y)` on `side` (`1` or `2`),
/// or `None` if outside (`rowAt` in `src/scoreboard/classic/layout.js`).
pub fn row_at(layout: &TeamStatsLayout, side: usize, x: f64, y: f64) -> Option<usize> {
    let list = layout.side(side);
    if x < list.left || x > list.right {
        return None;
    }
    let k = ((y - (layout.top - PITCH / 2.0)) / PITCH).floor() as isize;
    if k >= 0 && (k as usize) < layout.shown {
        Some(k as usize)
    } else {
        None
    }
}

/// Row configuration for `DETAILS` or `AWARDS` (`DETAILS_ROWS`, `AWARDS_ROWS` in `src/scoreboard/classic/layout.js`).
///
/// Both sub-pages render a two-column key/value list inside the main scoreboard window:
/// labels end right-aligned at `win.x - gap`, values start left-aligned at `win.x + gap`,
/// and the first `blank` rows under the title are left empty.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatsPageRowsConfig {
    pub height: f64,
    pub pitch: f64,
    pub gap: f64,
    pub blank: usize,
    pub font: &'static str,
}

/// Row metrics for `DETAILS` (`ifs_personalstats` in `src/scoreboard/classic/layout.js`).
///
/// Uses `gamefont_small` with 3 blank rows at the top so both the `"PERSONAL STATS"` title
/// and the inspected player's uppercase name fit above the 9 stat rows (+ optional `CONNECTION`).
pub const DETAILS_ROWS: StatsPageRowsConfig = StatsPageRowsConfig {
    height: 26.0,
    pitch: 31.0,
    gap: 15.0,
    blank: 3,
    font: "gamefont_small",
};

/// Row metrics for `AWARDS` (`ifs_awardstats` in `src/scoreboard/classic/layout.js`).
///
/// Uses `gamefont_medium` with 2 blank rows under `"PLAYER AWARDS"` and a wider `25.0`-unit
/// center gap (`50.0` units total between label and value) so the 8 mini team-logo holograms
/// can spin between each award's title and winner name.
pub const AWARDS_ROWS: StatsPageRowsConfig = StatsPageRowsConfig {
    height: 28.0,
    pitch: 34.0,
    gap: 25.0,
    blank: 2,
    font: "gamefont_medium",
};

/// Layout geometry for `DETAILS` and `AWARDS` sub-pages (`statsPageLayout` in `src/scoreboard/classic/layout.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatsPageLayout {
    pub table: TeamStatsLayout,
    pub shown: usize,
    pub row_top: f64,
    pub pitch: f64,
    pub label_right: f64,
    pub value_left: f64,
    pub title: [f64; 3],
    pub name: [f64; 3],
    pub icons: [[f64; 2]; 8],
}

impl StatsPageLayout {
    /// Target corner position where the inspected team's logo flies on `DETAILS` (`corner` in `src/scoreboard/classic/layout.js`).
    ///
    /// Team 1's logo flies to the top-left corner (`W/2 - 0.4 * safe.w`) and Team 2's to the
    /// top-right corner (`W/2 + 0.4 * safe.w`) at `H/2 - 0.3 * safe.h` and half size.
    #[inline]
    pub fn corner(&self, side: usize) -> [f64; 2] {
        let sign = if side == 1 { -1.0 } else { 1.0 };
        [
            W / 2.0 + sign * 0.4 * self.table.safe.w,
            self.table.h / 2.0 - 0.3 * self.table.safe.h,
        ]
    }
}

/// Selects the `DETAILS` or `AWARDS` page layout for `family` (`MULTIPLAYER.md`).
///
/// Only [`GameFamily::Swbf1`] is implemented; [`GameFamily::Swbf2`] returns [`GameFamilyError`].
pub fn stats_page_layout_for_family(
    family: GameFamily,
    h: f64,
    rows: StatsPageRowsConfig,
) -> Result<StatsPageLayout, GameFamilyError> {
    family.require_available()?;
    Ok(stats_page_layout(h, rows))
}

/// Computes the `DETAILS` or `AWARDS` page layout (`statsPageLayout` in `src/scoreboard/classic/layout.js`).
///
/// Replicates the original `ifs_awardstats` Lua formula for the 8 centered award icon positions
/// (`icons[0..8]`), including the `+10.0` vertical offset applied specifically at `h == 768.0`.
pub fn stats_page_layout(h: f64, rows: StatsPageRowsConfig) -> StatsPageLayout {
    let table = team_stats_layout(h);
    let win = table.window;
    let shown = ((win.h / rows.pitch).floor() as isize - 1).max(1) as usize;
    let top = win.y - (shown as f64 * rows.pitch) / 2.0 + rows.height / 2.0;
    let sh = table.safe.h;
    let title = [W / 2.0, table.safe.y + 0.1 * sh, 460.0];
    let name = [W / 2.0, title[1] + 30.0, 460.0];
    let mut icons = [[0.0; 2]; 8];
    for (i, icon) in icons.iter_mut().enumerate() {
        let extra = if (h - 768.0).abs() < 1e-6 { 10.0 } else { 0.0 };
        *icon = [
            W / 2.0,
            h / 2.0 - win.h / 2.0 + (1.8 + i as f64 + 1.0) * rows.pitch
                - 0.35 * rows.pitch * (h / 600.0 - 1.0)
                + extra,
        ];
    }

    StatsPageLayout {
        table,
        shown,
        row_top: top,
        pitch: rows.pitch,
        label_right: win.x - rows.gap,
        value_left: win.x + rows.gap,
        title,
        name,
        icons,
    }
}
