// Scoreboard state machine, sorting, selection, scrolling, and idle clock.
//
// Ports `src/scoreboard/model.js` (`CLOSED`, `PEEK`, `END`, `TABLE`, `DETAILS`, `AWARDS`,
// `KILLS`, `DEATHS`, `POINTS`, `NAME`, `IDLE_QUIT`, `compareRows`, `ScoreboardModel`).

use std::cmp::Ordering;

use super::stats::{ScoreRow, ScoreboardPort, TeamHeader};

/// Seconds without input before the end-of-match table quits (`IDLE_QUIT` in `src/scoreboard/model.js`).
pub const IDLE_QUIT: f64 = 30.0;

/// Maximum seconds subtracted from the idle clock in a single frame (`IDLE_STEP` in `src/scoreboard/model.js`).
pub const IDLE_STEP: f64 = 0.5;

/// Number of rows shown above the local player's row when the table opens (`ABOVE_PLAYER` in `src/scoreboard/model.js`).
pub const ABOVE_PLAYER: usize = 5;

/// Lifecycle state of the scoreboard (`CLOSED`, `PEEK`, `END` in `src/scoreboard/model.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardState {
    Closed,
    Peek,
    End,
}

/// Active sub-page while the scoreboard is open (`TABLE`, `DETAILS`, `AWARDS` in `src/scoreboard/model.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardPage {
    Table,
    Details,
    Awards,
}

/// Sort modes matching `ScriptCB_SetTeamStatsSortMode` (`KILLS`, `DEATHS`, `POINTS`, `NAME` in `src/scoreboard/model.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SortMode {
    Kills = 0,
    Deaths = 1,
    Points = 2,
    Name = 3,
}

impl SortMode {
    /// Tie-break order for a given primary `SortMode` (`ORDER` in `src/scoreboard/model.js`).
    pub fn order(self) -> [SortMode; 4] {
        match self {
            SortMode::Kills => [
                SortMode::Kills,
                SortMode::Deaths,
                SortMode::Points,
                SortMode::Name,
            ],
            SortMode::Deaths => [
                SortMode::Deaths,
                SortMode::Kills,
                SortMode::Points,
                SortMode::Name,
            ],
            SortMode::Points => [
                SortMode::Points,
                SortMode::Kills,
                SortMode::Deaths,
                SortMode::Name,
            ],
            SortMode::Name => [
                SortMode::Name,
                SortMode::Kills,
                SortMode::Deaths,
                SortMode::Points,
            ],
        }
    }
}

fn compare_key(key: SortMode, a: &ScoreRow, b: &ScoreRow) -> Ordering {
    match key {
        SortMode::Kills => b.kills.cmp(&a.kills),
        SortMode::Deaths => a.deaths.cmp(&b.deaths),
        SortMode::Points => b.points.cmp(&a.points),
        // Z before A by UTF-16 code units, matching the original game's negated string compare.
        SortMode::Name => b.name.encode_utf16().cmp(a.name.encode_utf16()),
    }
}

/// Compares two `ScoreRow` entries under `mode` and `reverse` (`compareRows` in `src/scoreboard/model.js`).
///
/// `reverse` flips only the primary key (`i == 0`); rows equal in all four keys preserve roster `slot` order.
pub fn compare_rows(mode: SortMode, reverse: bool, a: &ScoreRow, b: &ScoreRow) -> Ordering {
    let keys = mode.order();
    for (i, &key) in keys.iter().enumerate() {
        let ord = compare_key(key, a, b);
        if ord != Ordering::Equal {
            return if i == 0 && reverse { ord.reverse() } else { ord };
        }
    }
    a.slot.cmp(&b.slot)
}

/// Subject of the `DETAILS` page (`{ side, slot, name }` in `src/scoreboard/model.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailsSubject {
    pub side: usize,
    pub slot: usize,
    pub name: String,
}

/// Hover coordinates (`{ side, index }` in `src/scoreboard/model.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowCursor {
    pub side: usize,
    pub index: usize,
}

/// Events emitted by `ScoreboardModel` (`src/scoreboard/model.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScoreboardEvent {
    Open {
        state: BoardState,
        from: BoardState,
    },
    Close {
        state: BoardState,
    },
    Sort {
        mode: SortMode,
        reverse: bool,
    },
    Select {
        side: usize,
        index: usize,
    },
    Scroll {
        first: usize,
    },
    Page {
        page: BoardPage,
        from: BoardPage,
        side: Option<usize>,
    },
    Quit,
}

/// Pure scoreboard model (`ScoreboardModel` in `src/scoreboard/model.js`).
pub struct ScoreboardModel {
    pub state: BoardState,
    pub mode: SortMode,
    pub reverse: bool,
    /// Team 1 rows at index 0, team 2 rows at index 1 (`lists[1]` and `lists[2]` in JS).
    pub lists: [Vec<ScoreRow>; 2],
    /// Team 1 header at index 0, team 2 header at index 1 (`titles[1]` and `titles[2]` in JS).
    pub titles: [Option<TeamHeader>; 2],
    /// Local player's team (`1` or `2`).
    pub team: i32,
    /// Team side whose list holds the selection (`1` or `2`).
    pub side: usize,
    /// 0-based selected row index within `lists[side - 1]`.
    pub selected: usize,
    /// Row currently under the mouse pointer (`{ side, index }`).
    pub hover: Option<RowCursor>,
    /// First visible row index across both synchronized lists.
    pub first: usize,
    /// Number of rows visible at once (set by the view's layout, 16 at 768p).
    pub shown: usize,
    /// Remaining seconds before the end-of-match table auto-quits.
    pub idle: f64,
    /// Active page (`Table`, `Details`, or `Awards`).
    pub page: BoardPage,
    /// Subject inspected on the `DETAILS` page.
    pub subject: Option<DetailsSubject>,
    /// Events emitted since the last `drain_events` call.
    pub events: Vec<ScoreboardEvent>,
}

impl Default for ScoreboardModel {
    fn default() -> Self {
        Self::new()
    }
}

impl ScoreboardModel {
    /// Creates a closed `ScoreboardModel` with default sort (`KILLS`) and 16 visible rows.
    pub fn new() -> Self {
        Self {
            state: BoardState::Closed,
            mode: SortMode::Kills,
            reverse: false,
            lists: [Vec::new(), Vec::new()],
            titles: [None, None],
            team: 1,
            side: 1,
            selected: 0,
            hover: None,
            first: 0,
            shown: 16,
            idle: IDLE_QUIT,
            page: BoardPage::Table,
            subject: None,
            events: Vec::with_capacity(8),
        }
    }

    /// Whether the scoreboard is currently open (`Peek` or `End`).
    #[inline]
    pub fn is_open(&self) -> bool {
        self.state != BoardState::Closed
    }

    /// Length of the longer team list; both lists scroll together as one (`length` in `src/scoreboard/model.js`).
    #[inline]
    pub fn length(&self) -> usize {
        self.lists[0].len().max(self.lists[1].len())
    }

    /// Helper to convert 1-based side (`1` or `2`) to 0-based array index (`0` or `1`).
    #[inline]
    fn side_idx(side: usize) -> usize {
        if side == 2 {
            1
        } else {
            0
        }
    }

    /// Opens the scoreboard in `state` (`Peek` or `End`), refreshes rows from `port`,
    /// selects the local player's row, and scrolls so 5 rows appear above it (`open` in `src/scoreboard/model.js`).
    pub fn open(&mut self, state: BoardState, port: &dyn ScoreboardPort) -> bool {
        if state != BoardState::Peek && state != BoardState::End {
            return false;
        }
        let was = self.state;
        self.state = state;
        self.idle = IDLE_QUIT;
        self.hover = None;
        self.page = BoardPage::Table;
        self.subject = None;
        self.refresh(port);
        if let Some((side, index)) = self.find_human() {
            self.side = side;
            self.selected = index;
        } else {
            self.side = 1;
            self.selected = 0;
        }
        self.first = self.selected.saturating_sub(ABOVE_PLAYER);
        self.autoscroll();
        if was != state {
            self.events.push(ScoreboardEvent::Open { state, from: was });
        }
        true
    }

    /// Closes the scoreboard (`close` in `src/scoreboard/model.js`).
    pub fn close(&mut self) {
        if self.state == BoardState::Closed {
            return;
        }
        let state = self.state;
        self.state = BoardState::Closed;
        self.hover = None;
        self.events.push(ScoreboardEvent::Close { state });
    }

    /// Refreshes both teams' sorted rows and team headers from `port` (`refresh` in `src/scoreboard/model.js`).
    pub fn refresh(&mut self, port: &dyn ScoreboardPort) {
        let rows = port.roster();
        self.team = port.human_team();
        let mode = self.mode;
        let reverse = self.reverse;
        for (idx, team_num) in [1i32, 2i32].into_iter().enumerate() {
            self.lists[idx].clear();
            self.lists[idx].extend(rows.iter().filter(|r| r.team == team_num).cloned());
            self.lists[idx].sort_by(|a, b| compare_rows(mode, reverse, a, b));
        }
        self.titles = port.teams();
        self.clamp();
    }

    /// Finds `(side, index)` of the local human player's row (`findHuman` in `src/scoreboard/model.js`).
    pub fn find_human(&self) -> Option<(usize, usize)> {
        for side in [1usize, 2usize] {
            if let Some(index) = self.lists[Self::side_idx(side)].iter().position(|r| r.local) {
                return Some((side, index));
            }
        }
        None
    }

    /// Returns the row at `index` on `side` (`1` or `2`), or `None` if past the end (`row` in `src/scoreboard/model.js`).
    pub fn row(&self, side: usize, index: usize) -> Option<&ScoreRow> {
        if side != 1 && side != 2 {
            return None;
        }
        self.lists[Self::side_idx(side)].get(index)
    }

    /// Sorts by `mode` (toggling `reverse` if already in `mode`) and refreshes (`sortBy` in `src/scoreboard/model.js`).
    pub fn sort_by(&mut self, mode: SortMode, port: &dyn ScoreboardPort) -> bool {
        if self.page != BoardPage::Table {
            return false;
        }
        if mode == self.mode {
            self.reverse = !self.reverse;
        } else {
            self.mode = mode;
            self.reverse = false;
        }
        self.idle = IDLE_QUIT;
        self.refresh(port);
        self.events.push(ScoreboardEvent::Sort {
            mode: self.mode,
            reverse: self.reverse,
        });
        true
    }

    /// Clamps `selected` to the active side's list length and `first` to the scrollable range (`clamp` in `src/scoreboard/model.js`).
    pub fn clamp(&mut self) {
        let count = self.lists[Self::side_idx(self.side)].len();
        self.selected = self.selected.min(count.saturating_sub(1));
        let max_first = self.length().saturating_sub(self.shown);
        self.first = self.first.min(max_first);
    }

    /// Scrolls `first` so `selected` remains in view (`autoscroll` in `src/scoreboard/model.js`).
    pub fn autoscroll(&mut self) {
        self.clamp();
        if self.first > self.selected {
            self.first = self.selected;
        }
        if self.first + self.shown <= self.selected {
            self.first = (self.selected + 1).saturating_sub(self.shown);
        }
        self.clamp();
    }

    /// Selects `index` on `side` (`1` or `2`) (`select` in `src/scoreboard/model.js`).
    pub fn select(&mut self, side: usize, index: usize) -> bool {
        if self.page != BoardPage::Table || self.row(side, index).is_none() {
            return false;
        }
        let changed = side != self.side || index != self.selected;
        self.side = side;
        self.selected = index;
        self.idle = IDLE_QUIT;
        self.autoscroll();
        if changed {
            self.events.push(ScoreboardEvent::Select { side, index });
        }
        changed
    }

    /// Moves selection up or down by `step` within the active list (`move` in `src/scoreboard/model.js`).
    pub fn move_selection(&mut self, step: isize) -> bool {
        let len = self.lists[Self::side_idx(self.side)].len();
        if len == 0 {
            return false;
        }
        let target = (self.selected as isize + step).clamp(0, len as isize - 1) as usize;
        self.select(self.side, target)
    }

    /// Pages up (`step < 0`) or down (`step > 0`): jumps to the top/bottom of the visible window first,
    /// then by a full `shown` page (`turnPage` in `src/scoreboard/model.js`).
    pub fn turn_page(&mut self, step: isize) -> bool {
        let len = self.lists[Self::side_idx(self.side)].len();
        if len == 0 {
            return false;
        }
        let last = len - 1;
        let top = self.selected.saturating_sub(self.first);
        let index = if step < 0 {
            if top > 0 {
                self.first
            } else {
                self.selected.saturating_sub(self.shown)
            }
        } else if top + 1 < self.shown {
            (self.first + self.shown.saturating_sub(1)).min(last)
        } else {
            (self.selected + self.shown).min(last)
        };
        self.select(self.side, index)
    }

    /// Flips selection between team 1 and team 2 at the same row (or clamped to the other list's last row) (`flip` in `src/scoreboard/model.js`).
    pub fn flip(&mut self) -> bool {
        let side = if self.side == 1 { 2 } else { 1 };
        let count = self.lists[Self::side_idx(side)].len();
        if count == 0 || self.page != BoardPage::Table {
            return false;
        }
        self.side = side;
        self.selected = self.selected.min(count - 1);
        self.idle = IDLE_QUIT;
        self.autoscroll();
        self.events.push(ScoreboardEvent::Select {
            side,
            index: self.selected,
        });
        true
    }

    /// Scrolls the view by `step` rows while keeping the selection inside the visible window (`scroll` in `src/scoreboard/model.js`).
    pub fn scroll(&mut self, step: isize) -> bool {
        let max_first = self.length().saturating_sub(self.shown) as isize;
        let first = (self.first as isize + step).clamp(0, max_first) as usize;
        if first == self.first || self.page != BoardPage::Table {
            return false;
        }
        self.first = first;
        if self.selected < first {
            self.selected = first;
        }
        if self.selected >= first + self.shown {
            self.selected = first + self.shown.saturating_sub(1);
        }
        let count = self.lists[Self::side_idx(self.side)].len();
        self.selected = self.selected.min(count.saturating_sub(1));
        self.idle = IDLE_QUIT;
        self.events.push(ScoreboardEvent::Scroll { first });
        true
    }

    /// Whether there are rows above the visible window (`canScrollUp` in `src/scoreboard/model.js`).
    #[inline]
    pub fn can_scroll_up(&self) -> bool {
        self.first > 0
    }

    /// Whether there are rows below the visible window (`canScrollDown` in `src/scoreboard/model.js`).
    #[inline]
    pub fn can_scroll_down(&self) -> bool {
        self.first + self.shown < self.length()
    }

    /// Resets the 30-second end-of-match idle timer on any user input (`touch` in `src/scoreboard/model.js`).
    #[inline]
    pub fn touch(&mut self) {
        self.idle = IDLE_QUIT;
    }

    /// Opens the `DETAILS` page for the currently selected row (`showDetails` in `src/scoreboard/model.js`).
    pub fn show_details(&mut self) -> bool {
        if !self.is_open() || self.page != BoardPage::Table {
            return false;
        }
        let Some(row) = self.row(self.side, self.selected) else {
            return false;
        };
        let subject = DetailsSubject {
            side: self.side,
            slot: row.slot,
            name: row.name.clone(),
        };
        self.page = BoardPage::Details;
        self.subject = Some(subject);
        self.idle = IDLE_QUIT;
        self.events.push(ScoreboardEvent::Page {
            page: BoardPage::Details,
            from: BoardPage::Table,
            side: Some(self.side),
        });
        true
    }

    /// Opens the `AWARDS` page on the end-of-match scoreboard (`showAwards` in `src/scoreboard/model.js`).
    pub fn show_awards(&mut self) -> bool {
        if self.state != BoardState::End || self.page != BoardPage::Table {
            return false;
        }
        self.page = BoardPage::Awards;
        self.idle = IDLE_QUIT;
        self.events.push(ScoreboardEvent::Page {
            page: BoardPage::Awards,
            from: BoardPage::Table,
            side: None,
        });
        true
    }

    /// Returns from `DETAILS` or `AWARDS` to `TABLE`, preserving selection (`back` in `src/scoreboard/model.js`).
    pub fn back(&mut self, port: &dyn ScoreboardPort) -> bool {
        if !self.is_open() || self.page == BoardPage::Table {
            return false;
        }
        let from = self.page;
        let side = self.subject.as_ref().map(|s| s.side);
        self.page = BoardPage::Table;
        self.idle = IDLE_QUIT;
        self.refresh(port);
        self.events.push(ScoreboardEvent::Page {
            page: BoardPage::Table,
            from,
            side: if from == BoardPage::Details {
                side
            } else {
                None
            },
        });
        true
    }

    /// Advances the end-of-match idle timer by at most `IDLE_STEP` (`0.5s`) per frame;
    /// emits `ScoreboardEvent::Quit` and returns `true` when `idle < 0` (`update` in `src/scoreboard/model.js`).
    pub fn update(&mut self, dt: f64) -> bool {
        if self.state != BoardState::End {
            return false;
        }
        self.idle -= dt.clamp(0.0, IDLE_STEP);
        if self.idle >= 0.0 {
            return false;
        }
        self.idle = IDLE_QUIT;
        self.events.push(ScoreboardEvent::Quit);
        true
    }
}
