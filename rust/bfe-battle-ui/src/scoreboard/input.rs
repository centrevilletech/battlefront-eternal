// Scoreboard keyboard and mouse input, `TabHold` hold-or-tap rule, and double-click details.
//
// Ports `src/scoreboard/input.js` (`HOLD`, `DOUBLE_CLICK`, `TabHold`, `ScoreboardInput`).

use std::collections::HashSet;

use super::model::{RowCursor, ScoreboardModel, SortMode};
use super::stats::ScoreboardPort;
use crate::shapes::W;

/// Seconds `Tab` must be held for the peek table to close on release (`HOLD` in `src/scoreboard/input.js`).
pub const HOLD: f64 = 0.5;

/// Maximum seconds between two clicks on the same row to open `DETAILS` (`DOUBLE_CLICK` in `src/scoreboard/input.js`).
pub const DOUBLE_CLICK: f64 = 0.4;

/// Hotspot target under the scoreboard cursor (`src/scoreboard/input.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreboardHitTarget {
    Sort { mode: SortMode, side: usize },
    Scroll { step: isize, side: usize },
    Row { side: usize, index: usize },
    Done,
    Back,
    Details,
    Awards,
}

impl ScoreboardHitTarget {
    /// Returns a compact hover key matching `ClassicView.update` (`"${kind}:${mode ?? step ?? ""}:${side ?? ""}"`).
    pub fn hover_key(self) -> &'static str {
        match self {
            ScoreboardHitTarget::Sort {
                mode: SortMode::Kills,
                side: 1,
            } => "sort:0:1",
            ScoreboardHitTarget::Sort {
                mode: SortMode::Kills,
                side: _,
            } => "sort:0:2",
            ScoreboardHitTarget::Sort {
                mode: SortMode::Deaths,
                side: 1,
            } => "sort:1:1",
            ScoreboardHitTarget::Sort {
                mode: SortMode::Deaths,
                side: _,
            } => "sort:1:2",
            ScoreboardHitTarget::Sort {
                mode: SortMode::Points,
                side: 1,
            } => "sort:2:1",
            ScoreboardHitTarget::Sort {
                mode: SortMode::Points,
                side: _,
            } => "sort:2:2",
            ScoreboardHitTarget::Sort {
                mode: SortMode::Name,
                side: 1,
            } => "sort:3:1",
            ScoreboardHitTarget::Sort {
                mode: SortMode::Name,
                side: _,
            } => "sort:3:2",
            ScoreboardHitTarget::Scroll { step, side: 1 } if step < 0 => "scroll:-1:1",
            ScoreboardHitTarget::Scroll { step, side: _ } if step < 0 => "scroll:-1:2",
            ScoreboardHitTarget::Scroll { step: _, side: 1 } => "scroll:1:1",
            ScoreboardHitTarget::Scroll { step: _, side: _ } => "scroll:1:2",
            ScoreboardHitTarget::Row { side: 1, .. } => "row::1",
            ScoreboardHitTarget::Row { .. } => "row::2",
            ScoreboardHitTarget::Done => "done::",
            ScoreboardHitTarget::Back => "back::",
            ScoreboardHitTarget::Details => "details::",
            ScoreboardHitTarget::Awards => "awards::",
        }
    }
}

/// Immediate button action triggered inside `click` (`act(kind, detail)` in `src/scoreboard/input.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreboardAction {
    Done,
    Back,
    Details,
    Awards,
}

/// `Tab` hold-or-tap state machine (`TabHold` in `src/scoreboard/input.js`, `ScriptCB_CheckPlayerListDone`).
///
/// For the first `0.5s` (`HOLD`), a frame without `Tab` held marks the list as tapped;
/// after `0.5s`, a held list closes on the first frame without `Tab`, whereas a tapped
/// list transitions back to held once `Tab` is pressed again.
#[derive(Clone, Copy, Debug, Default)]
pub struct TabHold {
    pub opened_at: f64,
    tapped: bool,
}

impl TabHold {
    /// Starts tracking a freshly opened `Tab` peek table at `now`.
    pub fn start(&mut self, now: f64) {
        self.opened_at = now;
        self.tapped = false;
    }

    /// Whether the peek table was tapped (`Tab` released within `HOLD` seconds of opening).
    #[inline]
    pub fn tapped(&self) -> bool {
        self.tapped
    }

    /// Evaluates one frame with whether `Tab` is currently `held` at `now`;
    /// returns `true` when the peek table should close this frame (`step` in `src/scoreboard/input.js`).
    pub fn step(&mut self, held: bool, now: f64) -> bool {
        if now < self.opened_at + HOLD {
            if !held {
                self.tapped = true;
            }
            return false;
        }
        if !self.tapped {
            return !held;
        }
        if held {
            self.tapped = false;
        }
        false
    }
}

#[derive(Clone, Copy, Debug)]
enum PendingScoreInput {
    Move(isize),
    TurnPage(isize),
    Flip,
    Target(ScoreboardHitTarget),
}

#[derive(Clone, Copy, Debug)]
struct LastClick {
    side: usize,
    index: usize,
    time: f64,
}

/// Scoreboard keyboard and mouse input handler (`ScoreboardInput` in `src/scoreboard/input.js`).
pub struct ScoreboardInput {
    pub width: f64,
    pub height: f64,
    pub held: HashSet<String>,
    pending: Vec<PendingScoreInput>,
    pub time: f64,
    pub tab: TabHold,
    pub cursor: [f64; 2],
    last_click: Option<LastClick>,
    pub actions: Vec<ScoreboardAction>,
}

impl Default for ScoreboardInput {
    fn default() -> Self {
        Self::new(768.0)
    }
}

impl ScoreboardInput {
    /// Creates a new `ScoreboardInput` for interface dimensions `(1024, height)`.
    pub fn new(height: f64) -> Self {
        Self {
            width: W,
            height,
            held: HashSet::with_capacity(16),
            pending: Vec::with_capacity(16),
            time: 0.0,
            tab: TabHold::default(),
            cursor: [W / 2.0, height / 2.0],
            last_click: None,
            actions: Vec::with_capacity(4),
        }
    }

    /// Updates the current interface height (`768` at 4:3, `624` at 16:9).
    #[inline]
    pub fn set_height(&mut self, height: f64) {
        self.height = height;
    }

    /// Resets pending inputs and centers the cursor when the scoreboard opens (`reset` in `src/scoreboard/input.js`).
    pub fn reset(&mut self) {
        self.pending.clear();
        self.actions.clear();
        self.last_click = None;
        self.cursor = [self.width / 2.0, self.height / 2.0];
    }

    /// Processes a key press or release (`key` in `src/scoreboard/input.js`).
    ///
    /// Keys are tracked in `self.held` even while `active` or `act` is `false` so `Tab` held
    /// as the board opens is known. Returns `true` when the key is swallowed by the scoreboard.
    pub fn key<F>(
        &mut self,
        code: &str,
        down: bool,
        repeat: bool,
        act: bool,
        active: bool,
        hit_fn: F,
    ) -> bool
    where
        F: FnMut(f64, f64) -> Option<ScoreboardHitTarget>,
    {
        if !down {
            self.held.remove(code);
            return false;
        }
        self.held.insert(code.to_string());
        if !act || !active {
            return false;
        }
        match code {
            "ArrowUp" => self.pending.push(PendingScoreInput::Move(-1)),
            "ArrowDown" => self.pending.push(PendingScoreInput::Move(1)),
            "PageUp" => self.pending.push(PendingScoreInput::TurnPage(-1)),
            "PageDown" => self.pending.push(PendingScoreInput::TurnPage(1)),
            "ArrowLeft" | "ArrowRight" => self.pending.push(PendingScoreInput::Flip),
            "Enter" | "NumpadEnter" if !repeat => {
                let (cx, cy) = (self.cursor[0], self.cursor[1]);
                self.click(cx, cy, hit_fn);
            }
            "Enter" | "NumpadEnter" => {}
            _ => {}
        }
        matches!(
            code,
            "Tab"
                | "ArrowUp"
                | "ArrowDown"
                | "ArrowLeft"
                | "ArrowRight"
                | "PageUp"
                | "PageDown"
                | "Enter"
                | "NumpadEnter"
        )
    }

    /// Moves the cursor in interface coordinates (`move` in `src/scoreboard/input.js`).
    #[inline]
    pub fn move_cursor(&mut self, x: f64, y: f64) {
        self.cursor[0] = x.clamp(0.0, self.width);
        self.cursor[1] = y.clamp(0.0, self.height);
    }

    /// Nudges the cursor by `(dx, dy)` in interface units (`nudge` in `src/scoreboard/input.js`).
    #[inline]
    pub fn nudge(&mut self, dx: f64, dy: f64) {
        self.move_cursor(self.cursor[0] + dx, self.cursor[1] + dy);
    }

    /// Handles a mouse button press (`button` in `src/scoreboard/input.js`).
    pub fn button<F>(&mut self, index: u8, down: bool, active: bool, hit_fn: F) -> bool
    where
        F: FnMut(f64, f64) -> Option<ScoreboardHitTarget>,
    {
        if !down || !active || index != 0 {
            return false;
        }
        let (cx, cy) = (self.cursor[0], self.cursor[1]);
        self.click(cx, cy, hit_fn);
        true
    }

    /// Performs a left click at `(x, y)` against `hit_fn` (`click` in `src/scoreboard/input.js`).
    pub fn click<F>(
        &mut self,
        x: f64,
        y: f64,
        mut hit_fn: F,
    ) -> Option<ScoreboardHitTarget>
    where
        F: FnMut(f64, f64) -> Option<ScoreboardHitTarget>,
    {
        let target = hit_fn(x, y)?;
        match target {
            ScoreboardHitTarget::Row { side, index } => {
                let again = self
                    .last_click
                    .map(|last| {
                        last.side == side
                            && last.index == index
                            && self.time - last.time < DOUBLE_CLICK
                    })
                    .unwrap_or(false);
                self.last_click = if again {
                    None
                } else {
                    Some(LastClick {
                        side,
                        index,
                        time: self.time,
                    })
                };
                self.pending.push(PendingScoreInput::Target(target));
                if again {
                    self.actions.push(ScoreboardAction::Details);
                }
            }
            ScoreboardHitTarget::Sort { .. } | ScoreboardHitTarget::Scroll { .. } => {
                self.pending.push(PendingScoreInput::Target(target));
            }
            ScoreboardHitTarget::Done => self.actions.push(ScoreboardAction::Done),
            ScoreboardHitTarget::Back => self.actions.push(ScoreboardAction::Back),
            ScoreboardHitTarget::Details => self.actions.push(ScoreboardAction::Details),
            ScoreboardHitTarget::Awards => self.actions.push(ScoreboardAction::Awards),
        }
        Some(target)
    }

    /// Applies queued presses and updates `model.hover` once per frame (`update` in `src/scoreboard/input.js`).
    pub fn update<F>(
        &mut self,
        dt: f64,
        model: &mut ScoreboardModel,
        port: &dyn ScoreboardPort,
        mut hit_fn: F,
    ) where
        F: FnMut(&ScoreboardModel, f64, f64) -> Option<ScoreboardHitTarget>,
    {
        self.time += dt;
        for idx in 0..self.pending.len() {
            if !model.is_open() {
                break;
            }
            model.touch();
            match self.pending[idx] {
                PendingScoreInput::Move(step) => {
                    model.move_selection(step);
                }
                PendingScoreInput::TurnPage(step) => {
                    model.turn_page(step);
                }
                PendingScoreInput::Flip => {
                    model.flip();
                }
                PendingScoreInput::Target(ScoreboardHitTarget::Sort { mode, .. }) => {
                    model.sort_by(mode, port);
                }
                PendingScoreInput::Target(ScoreboardHitTarget::Scroll { step, .. }) => {
                    model.scroll(step);
                }
                PendingScoreInput::Target(ScoreboardHitTarget::Row { side, index }) => {
                    model.select(side, index);
                }
                _ => {}
            }
        }
        self.pending.clear();

        let under = if model.is_open() {
            hit_fn(model, self.cursor[0], self.cursor[1])
        } else {
            None
        };
        model.hover = match under {
            Some(ScoreboardHitTarget::Row { side, index }) => Some(RowCursor { side, index }),
            _ => None,
        };
    }
}
