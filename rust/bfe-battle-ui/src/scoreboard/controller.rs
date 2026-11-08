// Complete scoreboard controller combining `ScoreboardModel`, `ScoreboardInput`, and `ClassicScoreboardView`.
//
// Ports `createScoreboard` in `src/scoreboard/index.js`.
//
// Coordinates the three scoreboard layers:
// - `ScoreboardModel`: pure state machine (`Closed`, `Peek`, `End`), sorting, selection,
//   synchronized two-team scrolling, and the 30-second end-of-match idle timer.
// - `ScoreboardInput`: keyboard navigation, mouse hover/click/double-click, and `TabHold`
//   (the 0.5-second hold-vs-tap rule from `ScriptCB_CheckPlayerListDone`).
// - `ClassicScoreboardView`: 1024xH layout, `TitleIntro` text animations, and 3D hologram
//   `Flight` trajectories for team logos and award icons.

use super::input::{ScoreboardAction, ScoreboardInput};
use super::model::{BoardPage, BoardState, ScoreboardEvent, ScoreboardModel};
use super::stats::ScoreboardPort;
use super::view::ClassicScoreboardView;
use crate::shapes::{BattleUiSurface, GameFamily, GameFamilyError};

/// Complete scoreboard controller combining `ScoreboardModel`, `ScoreboardInput`, and `ClassicScoreboardView`
/// (`createScoreboard` in `src/scoreboard/index.js`).
pub struct ScoreboardController {
    pub family: GameFamily,
    pub model: ScoreboardModel,
    pub input: ScoreboardInput,
    pub view: ClassicScoreboardView,
    /// Latch ensuring the end-of-match table opens automatically only once when `port.state().finished` becomes `true`.
    pub ended: bool,
    /// Latch preventing duplicate `port.restart()` calls once `DONE` or the 30s idle timer has triggered.
    pub quitting: bool,
    /// Queued UI sound names (`"shell_select_change"`, `"shell_menu_enter"`, `"shell_menu_exit"`).
    pub sounds: Vec<&'static str>,
}

impl Default for ScoreboardController {
    fn default() -> Self {
        Self::new(768.0)
    }
}

impl ScoreboardController {
    /// Creates a new `ScoreboardController` for the given interface `height` (`768.0` at 4:3, `624.0` at 16:9).
    pub fn new(height: f64) -> Self {
        Self {
            family: GameFamily::Swbf1,
            model: ScoreboardModel::new(),
            input: ScoreboardInput::new(height),
            view: ClassicScoreboardView::new(),
            ended: false,
            quitting: false,
            sounds: Vec::with_capacity(8),
        }
    }

    /// Constructs a `ScoreboardController` for `family`, returning [`GameFamilyError`]
    /// if `family` is [`GameFamily::Swbf2`].
    pub fn try_with_family(family: GameFamily, height: f64) -> Result<Self, GameFamilyError> {
        let view = ClassicScoreboardView::try_with_family(family)?;
        Ok(Self {
            family,
            model: ScoreboardModel::new(),
            input: ScoreboardInput::new(height),
            view,
            ended: false,
            quitting: false,
            sounds: Vec::with_capacity(8),
        })
    }

    /// Whether `Tab` may open the peek scoreboard right now (`canPeek` in `src/scoreboard/index.js`).
    ///
    /// `Tab` cannot open the peek scoreboard when the scoreboard is already open, once `game_over`
    /// has stopped scores, while the spawn screen is open (`Tab` cycles command posts there),
    /// or while the pause menu is open.
    pub fn can_peek(&self, port: &dyn ScoreboardPort, spawn_open: bool, menu_open: bool) -> bool {
        !self.model.is_open() && !port.state().game_over && !spawn_open && !menu_open
    }

    /// Opens the scoreboard in `Peek` mode (`peek` in `src/scoreboard/index.js`).
    ///
    /// Starts the `TabHold` timer at `self.input.time` so releasing `Tab` within `0.5s` pins the
    /// scoreboard open, whereas holding `Tab` past `0.5s` closes it on release.
    pub fn peek(
        &mut self,
        port: &dyn ScoreboardPort,
        surface: &dyn BattleUiSurface,
        spawn_open: bool,
        menu_open: bool,
    ) -> bool {
        if !self.can_peek(port, spawn_open, menu_open) {
            return false;
        }
        self.model.open(BoardState::Peek, port);
        self.input.tab.start(self.input.time);
        self.drain_model_events(port, surface.height());
        true
    }

    /// Closes the peek scoreboard (`close` in `src/scoreboard/index.js`).
    ///
    /// Only `BoardState::Peek` can be closed via `close`; the end-of-match table (`BoardState::End`)
    /// stays up until `quit` (`DONE` or 30s idle).
    pub fn close(&mut self, port: &dyn ScoreboardPort, height: f64) {
        if self.model.state == BoardState::Peek {
            self.model.close();
            self.drain_model_events(port, height);
        }
    }

    /// Executes `DONE` or the 30s end-of-match auto-quit (`quit` in `src/scoreboard/index.js`).
    ///
    /// Calls `port.restart()` to advance to the next battle in the Instant Action playlist
    /// (`ScriptCB_QuitFromStats` in the 2004 game). Matches the original's audio quirk where
    /// quitting from `AWARDS` plays `"shell_menu_enter"` while quitting from `Table` or `DETAILS`
    /// plays `"shell_menu_exit"`.
    pub fn quit(&mut self, port: &mut dyn ScoreboardPort) -> bool {
        if self.model.state != BoardState::End || self.quitting || !port.can_restart() {
            return false;
        }
        if !port.restart() {
            return false;
        }
        self.quitting = true;
        self.sounds.push(if self.model.page == BoardPage::Awards {
            "shell_menu_enter"
        } else {
            "shell_menu_exit"
        });
        true
    }

    /// Steps back from `DETAILS`/`AWARDS` to `Table`, or closes `Peek` (`back` in `src/scoreboard/index.js`).
    pub fn back(&mut self, port: &dyn ScoreboardPort, height: f64) {
        if self.model.page != BoardPage::Table {
            self.model.back(port);
        } else {
            self.close(port, height);
        }
        self.drain_model_events(port, height);
    }

    /// Executes a scoreboard button action (`act` in `src/scoreboard/index.js`).
    ///
    /// Switching directly between `Details` and `Awards` (e.g. via page tabs) steps through
    /// `model.back(port)` first while clearing the intermediate `Table` transition event
    /// (`passing = true` in `src/scoreboard/index.js`) so only the target page's enter sound plays.
    pub fn act(&mut self, action: ScoreboardAction, port: &mut dyn ScoreboardPort, height: f64) {
        if !self.model.is_open() {
            return;
        }
        self.model.touch();
        match action {
            ScoreboardAction::Back => self.back(port, height),
            ScoreboardAction::Done => {
                self.quit(port);
            }
            ScoreboardAction::Details => {
                if self.model.page != BoardPage::Table && self.model.page != BoardPage::Details {
                    self.model.back(port);
                    self.model.events.clear();
                }
                self.model.show_details();
                self.drain_model_events(port, height);
            }
            ScoreboardAction::Awards => {
                if self.model.page != BoardPage::Table && self.model.page != BoardPage::Awards {
                    self.model.back(port);
                    self.model.events.clear();
                }
                self.model.show_awards();
                self.drain_model_events(port, height);
            }
        }
    }

    /// Translates queued `ScoreboardEvent` items emitted by `self.model` into view transitions,
    /// input cursor resets, and interface sound effects (`model.on(...)` in `src/scoreboard/index.js`).
    fn drain_model_events(&mut self, port: &dyn ScoreboardPort, height: f64) {
        let events: Vec<ScoreboardEvent> = std::mem::take(&mut self.model.events);
        for ev in events {
            match ev {
                ScoreboardEvent::Open { .. } => {
                    self.quitting = false;
                    self.view.open(&mut self.model, port, height);
                    self.input.set_height(height);
                    self.input.reset();
                }
                ScoreboardEvent::Close { .. } => {
                    self.sounds.push("shell_menu_exit");
                }
                ScoreboardEvent::Sort { .. }
                | ScoreboardEvent::Select { .. }
                | ScoreboardEvent::Scroll { .. } => {
                    self.sounds.push("shell_select_change");
                }
                ScoreboardEvent::Page { page, from, side } => {
                    self.view
                        .page_changed(page, from, side, &self.model, port, height);
                    self.sounds.push(if page == BoardPage::Table {
                        "shell_menu_exit"
                    } else {
                        "shell_menu_enter"
                    });
                }
                ScoreboardEvent::Quit => {}
            }
        }
    }

    /// Steps one frame of the scoreboard (`update` in `src/scoreboard/index.js`).
    ///
    /// Handles:
    /// 1. Automatic transition into `BoardState::End` once `port.state().finished` becomes `true`.
    /// 2. Auto-closing `BoardState::Peek` if `game_over` or `spawn_open` becomes `true`.
    /// 3. Pausing scoreboard input and hiding its cursor while `menu_open` (the pause menu) covers it.
    /// 4. Draining queued keyboard/mouse inputs and button actions.
    /// 5. Stepping `TabHold` and live-refreshing roster scores while in `Peek` mode.
    /// 6. Advancing the 30-second end-of-match idle timer and `ClassicScoreboardView` animations.
    pub fn update(
        &mut self,
        dt: f64,
        port: &mut dyn ScoreboardPort,
        surface: &dyn BattleUiSurface,
        spawn_open: bool,
        menu_open: bool,
    ) {
        let height = surface.height();
        let state = port.state();
        if state.finished && !self.ended {
            self.ended = true;
            if self.model.state == BoardState::Peek {
                self.model.close();
            }
            self.model.open(BoardState::End, port);
            self.drain_model_events(port, height);
        }
        if !self.model.is_open() {
            return;
        }
        if self.model.state == BoardState::Peek && (state.game_over || spawn_open) {
            self.model.close();
            self.drain_model_events(port, height);
            return;
        }
        self.view.hide_pointer = menu_open;
        if menu_open {
            return;
        }

        // Sync the current interface height (`768` at 4:3, `624` at 16:9) before hit-testing
        // queued keyboard `Enter` presses and pointer hover coordinates against `self.view`.
        self.input.set_height(height);
        {
            let view_ref = &self.view;
            let port_ref: &dyn ScoreboardPort = &*port;
            self.input
                .update(dt, &mut self.model, port_ref, |m, x, y| {
                    view_ref.hit(x, y, m, port_ref, surface)
                });
        }

        let pending_actions: Vec<ScoreboardAction> = std::mem::take(&mut self.input.actions);
        for action in pending_actions {
            self.act(action, port, height);
        }
        self.drain_model_events(port, height);

        // In `Peek` mode, check `TabHold::step` to see whether releasing `Tab` should close
        // the overlay, and refresh live scores from `port` every frame while it stays open.
        if self.model.state == BoardState::Peek {
            let tab_held = self.input.held.contains("Tab");
            if self.input.tab.step(tab_held, self.input.time) {
                self.model.close();
                self.drain_model_events(port, height);
                return;
            }
            self.model.refresh(port);
        }

        // On the end-of-match screen, `model.update(dt)` counts down the 30-second idle clock
        // and returns `true` when it expires, advancing to the next battle via `self.quit(port)`.
        if self.model.update(dt) {
            self.model.events.clear();
            self.quit(port);
            return;
        }
        self.drain_model_events(port, height);
        self.view
            .update(dt, &mut self.model, port, surface, self.input.cursor);
    }
}
