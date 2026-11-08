// Complete scoreboard controller combining `ScoreboardModel`, `ScoreboardInput`, and `ClassicScoreboardView`.
//
// Ports `createScoreboard` in `src/scoreboard/index.js`.

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
    pub ended: bool,
    pub quitting: bool,
    pub sounds: Vec<&'static str>,
}

impl Default for ScoreboardController {
    fn default() -> Self {
        Self::new(768.0)
    }
}

impl ScoreboardController {
    /// Creates a new `ScoreboardController`.
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
    pub fn can_peek(&self, port: &dyn ScoreboardPort, spawn_open: bool, menu_open: bool) -> bool {
        !self.model.is_open() && !port.state().game_over && !spawn_open && !menu_open
    }

    /// Opens the scoreboard in `Peek` mode (`peek` in `src/scoreboard/index.js`).
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
    pub fn close(&mut self, port: &dyn ScoreboardPort, height: f64) {
        if self.model.state == BoardState::Peek {
            self.model.close();
            self.drain_model_events(port, height);
        }
    }

    /// Executes `DONE` or the 30s end-of-match auto-quit (`quit` in `src/scoreboard/index.js`).
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

        if self.model.state == BoardState::Peek {
            let tab_held = self.input.held.contains("Tab");
            if self.input.tab.step(tab_held, self.input.time) {
                self.model.close();
                self.drain_model_events(port, height);
                return;
            }
            self.model.refresh(port);
        }

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
