// Classic in-battle pause menu (`ifs_pausemenu`), confirm popups, and `still_command`.
//
// Ports `src/pause/index.js`, `src/menus/themes/classic/index.js`,
// `src/menus/popup.js`, and `stillCommand` from `src/runtime/commands.js`.

use super::overrides::{with_settings, PauseSettingsLookup};
use crate::scoreboard::PlayerListRow;
use crate::shapes::{
    inside_quad, BattleUiSurface, GameFamily, GameFamilyError, TitleIntro, FONT_NAME_MEDIUM, W,
};

/// Pause menu layout constants (`LAYOUT` in `src/pause/index.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PauseLayoutConfig {
    pub at: [f64; 2],
    pub y: f64,
    pub rot_y_deg: f64,
    pub font: &'static str,
    pub width: f64,
    pub y_spacing: f64,
}

/// Default Classic pause menu layout (`LAYOUT` in `src/pause/index.js`).
pub const LAYOUT: PauseLayoutConfig = PauseLayoutConfig {
    at: [0.6, 0.5],
    y: 20.0,
    rot_y_deg: 25.0,
    font: FONT_NAME_MEDIUM,
    width: 260.0,
    y_spacing: 5.0,
};

/// Selects the pause menu layout for `family` (`MULTIPLAYER.md`).
///
/// Only [`GameFamily::Swbf1`] (2004) is implemented; [`GameFamily::Swbf2`] returns an
/// explicit [`GameFamilyError`] and never silently falls back to the 2004 layout.
pub fn pause_layout_for_family(family: GameFamily) -> Result<PauseLayoutConfig, GameFamilyError> {
    family.require_available()?;
    Ok(LAYOUT)
}

/// Total row count of the 2004 pause menu container, including the 5 hidden console/debug rows (`LIST_COUNT = 11` in `src/pause/index.js`).
pub const LIST_COUNT: usize = 11;

/// Seconds after pointer-unlock during which an `Escape` keydown is treated as the same press (`LOCK_GRACE = 250ms` in `src/pause/index.js`).
pub const LOCK_GRACE: f64 = 0.25;

/// Seconds before `"CLICK TO RESUME"` renders while waiting for pointer lock (`PROMPT_DELAY = 0.15s` in `src/pause/index.js`).
pub const PROMPT_DELAY: f64 = 0.15;

const ROW_INTRO: f64 = 0.1;

/// Neutral input command packet sent each tick while paused in a network game (`stillCommand` in `src/runtime/commands.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StillCommand {
    pub kind: &'static str,
    pub tick: u32,
    pub seq: u32,
}

/// Builds a neutral human command sent each tick while paused in a network game (`stillCommand` in `src/runtime/commands.js`).
#[inline]
pub const fn still_command(tick: u32, seq: u32) -> StillCommand {
    StillCommand {
        kind: "input",
        tick,
        seq,
    }
}

/// Computes the horizontal container shift for a stack of `count` rows (`rowsShift` in `src/menus/themes/classic/index.js`).
pub fn rows_shift(width: f64, pitch: f64, count: usize) -> f64 {
    let mut w = width + if count % 2 == 1 { pitch } else { 0.0 };
    let mut widest = 0.0_f64;
    let half = count / 2;
    for i in 1..=count {
        if i < half {
            w += pitch;
        } else if i > half {
            w -= pitch;
        }
        widest = widest.max(w);
    }
    -widest / 2.0
}

/// Computed bar width and right-end flip flag for a visible row (`rowBars` in `src/menus/themes/classic/index.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowBarSpec {
    pub width: f64,
    pub flipped: bool,
}

/// Computes bar widths and flip flags for `n` visible rows (`rowBars` in `src/menus/themes/classic/index.js`).
pub fn row_bars(width: f64, pitch: f64, n: usize) -> Vec<RowBarSpec> {
    let half = n / 2;
    let mut out = Vec::with_capacity(n);
    let mut w = width;
    for i in 1..=n {
        out.push(RowBarSpec {
            width: w,
            flipped: i > half,
        });
        if i < half {
            w += pitch;
        } else if i > half {
            w -= pitch;
        }
    }
    out
}

/// Identifies a row on the main pause menu (`src/pause/index.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseRowId {
    Resume,
    Options,
    Suicide,
    Restart,
    PlayerList,
    Quit,
    Exit,
}

/// Identifies a confirmation popup question (`QUESTIONS` in `src/pause/index.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmKind {
    Suicide,
    Restart,
    Quit,
    Exit,
}

impl ConfirmKind {
    /// Returns the screen ID matching `src/pause/index.js` (`"confirm-suicide"`, etc.).
    pub fn screen_id(self) -> &'static str {
        match self {
            ConfirmKind::Suicide => "confirm-suicide",
            ConfirmKind::Restart => "confirm-restart",
            ConfirmKind::Quit => "confirm-quit",
            ConfirmKind::Exit => "confirm-exit",
        }
    }
}

/// Active screen on top of the pause menu stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PauseScreenState {
    /// Main angled pause menu (`id: "pause"`).
    Main,
    /// `YES`/`NO` confirmation popup (`id: "confirm-*"`).
    Confirm {
        kind: ConfirmKind,
        message: String,
        /// `0` = `YES`, `1` = `NO` (`NO` is focused by default).
        focused_button: usize,
    },
    /// Options sub-menu (`id: "options"`).
    Options { focused_row: usize },
    /// Network `PLAYER LIST` sub-screen (`id: "playerlist"`).
    PlayerList { selected: usize, first: usize },
}

impl PauseScreenState {
    /// Screen identifier matching `MenuScreen.id` in JS.
    pub fn id(&self) -> &'static str {
        match self {
            PauseScreenState::Main => "pause",
            PauseScreenState::Confirm { kind, .. } => kind.screen_id(),
            PauseScreenState::Options { .. } => "options",
            PauseScreenState::PlayerList { .. } => "playerlist",
        }
    }
}

/// Configuration for the `RESTART MISSION` row (`restart` option in `createPauseMenu`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PauseRestartMode {
    /// Default single-player / local battle behavior: navigates to `with_settings(restart_url, settings)`.
    Url(String),
    /// Custom host restart action with optional custom row label and prompt.
    Custom {
        label: Option<String>,
        prompt: Option<String>,
    },
    /// Hidden (`restart: false` for a client connected to a remote server).
    Hidden,
}

/// Configuration for the `RESPAWN` row (`enableRespawn` in `src/pause/index.js`).
#[derive(Clone, Debug, Default)]
pub struct PauseRespawnConfig {
    pub label: Option<String>,
    pub prompt: Option<String>,
    pub available: bool,
}

/// Record of a page navigation triggered by `RESTART MISSION`, `QUIT`, or `EXIT TO WINDOWS` (`menu.leaving` in `src/pause/index.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeavingRecord {
    pub url: String,
    pub why: &'static str,
}

/// Hotspot recorded during `PauseMenu::draw` for pointer hover and clicks.
#[derive(Clone, Debug)]
pub struct PauseHotspot {
    pub target: PauseHitTarget,
    pub corners: [[f64; 2]; 4],
}

/// Target of a pause menu hotspot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseHitTarget {
    MainRow(usize),
    OptionRow(usize),
    PlayerRow(usize),
    ConfirmButton(usize),
    BackButton,
}

/// Classic in-battle pause menu (`createPauseMenu` in `src/pause/index.js`).
pub struct PauseMenu {
    pub family: GameFamily,
    pub is_open: bool,
    pub opened_at: f64,
    pub opened_by: Option<String>,
    pub resume_on_release: bool,
    pub waiting_time: Option<f64>,
    pub time: f64,
    pub focused_row: usize,
    pub back_focused: bool,
    pub stack: Vec<PauseScreenState>,
    pub pointer: [f64; 2],
    pub pointer_visible: bool,
    pub show_pointer: bool,
    pub respawn: Option<PauseRespawnConfig>,
    pub restart_mode: PauseRestartMode,
    pub main_menu_url: String,
    pub show_player_list: bool,
    pub players: Vec<PlayerListRow>,
    pub audio_panel_open: bool,
    pub escape_owner_open: bool,
    pub leaving: Option<LeavingRecord>,
    pub respawn_triggered: usize,
    pub custom_restart_triggered: usize,
    pub window_close_triggered: usize,
    pub open_events: usize,
    pub close_events: Vec<bool>,
    pub sounds: Vec<&'static str>,
    pub(crate) title_intro: TitleIntro,
    pub(crate) row_intros: [TitleIntro; LIST_COUNT],
    pub(crate) back_intro: TitleIntro,
    pub(crate) popup_yes_intro: TitleIntro,
    pub(crate) popup_no_intro: TitleIntro,
    pub(crate) hotspots: Vec<PauseHotspot>,
}

impl Default for PauseMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl PauseMenu {
    /// Creates a new closed `PauseMenu` with default Classic layout and URLs.
    pub fn new() -> Self {
        Self {
            family: GameFamily::Swbf1,
            is_open: false,
            opened_at: f64::NEG_INFINITY,
            opened_by: None,
            resume_on_release: false,
            waiting_time: None,
            time: 0.0,
            focused_row: 0,
            back_focused: false,
            stack: Vec::with_capacity(4),
            pointer: [512.0, 384.0],
            pointer_visible: true,
            show_pointer: true,
            respawn: None,
            restart_mode: PauseRestartMode::Url("http://localhost/?map=nab2".to_string()),
            main_menu_url: "http://localhost/".to_string(),
            show_player_list: false,
            players: Vec::new(),
            audio_panel_open: false,
            escape_owner_open: false,
            leaving: None,
            respawn_triggered: 0,
            custom_restart_triggered: 0,
            window_close_triggered: 0,
            open_events: 0,
            close_events: Vec::new(),
            sounds: Vec::with_capacity(8),
            title_intro: TitleIntro::default(),
            row_intros: [TitleIntro::default(); LIST_COUNT],
            back_intro: TitleIntro::default(),
            popup_yes_intro: TitleIntro::default(),
            popup_no_intro: TitleIntro::default(),
            hotspots: Vec::with_capacity(16),
        }
    }

    /// Constructs a `PauseMenu` for `family`, returning [`GameFamilyError`]
    /// if `family` is [`GameFamily::Swbf2`].
    pub fn try_with_family(family: GameFamily) -> Result<Self, GameFamilyError> {
        let _layout = pause_layout_for_family(family)?;
        Ok(Self::new())
    }

    /// Pulse wave `0..1` used by focused Classic buttons and `"CLICK TO RESUME"` (`stack.pulse` in `src/menus/stack.js`).
    #[inline]
    pub fn pulse(&self) -> f64 {
        0.5 + 0.5 * (self.time * 4.0).sin()
    }

    /// Whether `RESUME` is currently waiting for pointer lock (`waiting` in `src/pause/index.js`).
    #[inline]
    pub fn waiting(&self) -> bool {
        self.waiting_time.is_some()
    }

    /// Whether the HUD should be hidden by the pause overlay (`overlay.hidesHud` in `src/pause/index.js`).
    ///
    /// While `is_open` is `true`, the HUD hides; while `waiting()` is `true` (`CLICK TO RESUME`),
    /// the HUD remains visible.
    #[inline]
    pub fn hides_hud(&self) -> bool {
        self.is_open
    }

    /// Top screen on the pause stack, if open.
    pub fn top(&self) -> Option<&PauseScreenState> {
        self.stack.last()
    }

    /// Visible rows on the main pause screen (`visibleRows` in `src/pause/index.js`).
    pub fn visible_rows(&self, surface: &dyn BattleUiSurface) -> Vec<(PauseRowId, String)> {
        let mut out = Vec::with_capacity(7);
        out.push((
            PauseRowId::Resume,
            surface.string("common.resume", "RESUME GAME").to_string(),
        ));
        out.push((
            PauseRowId::Options,
            surface.string("ifs.main.options", "OPTIONS").to_string(),
        ));
        let suicide_label = self
            .respawn
            .as_ref()
            .and_then(|r| r.label.clone())
            .unwrap_or_else(|| surface.string("game.pause.suicide", "RESPAWN").to_string());
        out.push((PauseRowId::Suicide, suicide_label));

        match &self.restart_mode {
            PauseRestartMode::Url(_) => {
                out.push((
                    PauseRowId::Restart,
                    surface
                        .string("common.restart", "RESTART MISSION")
                        .to_string(),
                ));
            }
            PauseRestartMode::Custom { label, .. } => {
                out.push((
                    PauseRowId::Restart,
                    label.clone().unwrap_or_else(|| {
                        surface
                            .string("common.restart", "RESTART MISSION")
                            .to_string()
                    }),
                ));
            }
            PauseRestartMode::Hidden => {}
        }

        if self.show_player_list {
            out.push((
                PauseRowId::PlayerList,
                surface
                    .string("ifs.pause.playerlist", "PLAYER LIST")
                    .to_string(),
            ));
        }

        out.push((
            PauseRowId::Quit,
            surface.string("common.quit", "QUIT").to_string(),
        ));
        out.push((
            PauseRowId::Exit,
            surface
                .string("common.quit2windows", "EXIT TO WINDOWS")
                .to_string(),
        ));
        out
    }

    /// Configures `RESPAWN` (`enableRespawn` in `src/pause/index.js`).
    pub fn enable_respawn(&mut self, config: Option<PauseRespawnConfig>) {
        self.respawn = config;
    }

    /// Sets whether `"CLICK TO RESUME"` is waiting for a pointer-lock click (`awaitPointer` in `src/pause/index.js`).
    pub fn await_pointer(&mut self, on: bool) {
        self.waiting_time = if on { Some(0.0) } else { None };
    }

    /// Opens the pause menu (`open(why)` in `src/pause/index.js`).
    pub fn open(&mut self, why: &str, height: f64) {
        if self.is_open {
            return;
        }
        self.is_open = true;
        self.resume_on_release = false;
        self.opened_at = self.time;
        self.opened_by = Some(why.to_string());
        self.waiting_time = None;
        self.stack.clear();
        self.stack.push(PauseScreenState::Main);
        self.focused_row = 0;
        self.back_focused = false;
        self.pointer = [512.0, height / 2.0];
        self.pointer_visible = true;
        self.start_main_intros();
        self.open_events += 1;
    }

    fn start_main_intros(&mut self) {
        self.title_intro.start(0.0);
        for (k, intro) in self.row_intros.iter_mut().enumerate() {
            intro.start((k as f64 + 1.0) * ROW_INTRO);
        }
        self.back_intro.start(0.0);
    }

    /// Closes the pause menu (`close(resume)` in `src/pause/index.js`).
    pub fn close(&mut self, resume: bool) {
        if !self.is_open {
            return;
        }
        self.is_open = false;
        self.stack.clear();
        self.hotspots.clear();
        self.close_events.push(resume);
    }

    /// Opens a confirmation popup (`ask(kind)` in `src/pause/index.js`) with `NO` (`1`) focused first.
    pub fn ask(&mut self, kind: ConfirmKind, surface: &dyn BattleUiSurface) {
        let message = match kind {
            ConfirmKind::Suicide => self
                .respawn
                .as_ref()
                .and_then(|r| r.prompt.clone())
                .unwrap_or_else(|| {
                    surface
                        .string(
                            "game.pause.suicide_prompt",
                            "DO YOU REALLY WISH TO KILL YOUR CURRENT PLAYER?",
                        )
                        .to_string()
                }),
            ConfirmKind::Restart => match &self.restart_mode {
                PauseRestartMode::Custom {
                    prompt: Some(p), ..
                } => p.clone(),
                _ => surface
                    .string(
                        "ifs.pause.warn_restart",
                        "DO YOU REALLY WANT TO RESTART THIS BATTLE?",
                    )
                    .to_string(),
            },
            ConfirmKind::Quit => surface
                .string(
                    "ifs.pause.warn_host_quit",
                    "LEAVING THE GAME NOW WILL END THIS SESSION. ARE YOU SURE YOU WANT TO LEAVE?",
                )
                .to_string(),
            ConfirmKind::Exit => surface
                .string("ifs.main.askquit", "REALLY EXIT TO WINDOWS?")
                .to_string(),
        };
        self.popup_yes_intro.start(0.0);
        self.popup_no_intro.start(0.0);
        self.back_focused = false;
        self.stack.push(PauseScreenState::Confirm {
            kind,
            message,
            focused_button: 1, // NO focused by default (`focus = "no"` in `src/menus/popup.js`)
        });
    }

    /// Activates a row on the main pause screen.
    pub fn activate_main_row(
        &mut self,
        row_index: usize,
        surface: &dyn BattleUiSurface,
        settings: &dyn PauseSettingsLookup,
    ) {
        let rows = self.visible_rows(surface);
        let Some(&(row_id, _)) = rows.get(row_index) else {
            return;
        };
        self.sounds.push("shell_menu_enter");
        match row_id {
            PauseRowId::Resume => self.close(true),
            PauseRowId::Options => {
                self.stack.push(PauseScreenState::Options { focused_row: 0 });
            }
            PauseRowId::Suicide => self.ask(ConfirmKind::Suicide, surface),
            PauseRowId::Restart => self.ask(ConfirmKind::Restart, surface),
            PauseRowId::PlayerList => {
                self.stack.push(PauseScreenState::PlayerList {
                    selected: 0,
                    first: 0,
                });
            }
            PauseRowId::Quit => self.ask(ConfirmKind::Quit, surface),
            PauseRowId::Exit => self.ask(ConfirmKind::Exit, surface),
        }
        let _ = settings;
    }

    /// Confirms (`yes = true`) or cancels (`yes = false`) the active confirmation popup (`QUESTIONS` in `src/pause/index.js`).
    pub fn answer_confirm(&mut self, yes: bool, settings: &dyn PauseSettingsLookup) {
        let Some(PauseScreenState::Confirm { kind, .. }) = self.stack.last().cloned() else {
            return;
        };
        self.stack.pop();
        if !yes {
            self.sounds.push("shell_menu_cancel");
            return;
        }
        self.sounds.push("shell_menu_accept");
        match kind {
            ConfirmKind::Suicide => {
                if let Some(respawn) = &self.respawn {
                    if respawn.available {
                        self.respawn_triggered += 1;
                    }
                }
                self.close(true);
            }
            ConfirmKind::Restart => match &self.restart_mode {
                PauseRestartMode::Url(url) => {
                    let target = with_settings(url, settings);
                    self.leaving = Some(LeavingRecord {
                        url: target,
                        why: "restart",
                    });
                }
                PauseRestartMode::Custom { .. } => {
                    self.custom_restart_triggered += 1;
                    self.close(true);
                }
                PauseRestartMode::Hidden => {}
            },
            ConfirmKind::Quit => {
                self.leaving = Some(LeavingRecord {
                    url: self.main_menu_url.clone(),
                    why: "quit",
                });
            }
            ConfirmKind::Exit => {
                self.window_close_triggered += 1;
                self.leaving = Some(LeavingRecord {
                    url: self.main_menu_url.clone(),
                    why: "exit",
                });
            }
        }
    }

    /// Handles a key press or release (`onKey` / `onKeyUp` + `ScreenStack` navigation in `src/pause/index.js`).
    pub fn handle_key(
        &mut self,
        code: &str,
        down: bool,
        repeat: bool,
        surface: &dyn BattleUiSurface,
        settings: &dyn PauseSettingsLookup,
    ) -> bool {
        if code == "Escape" {
            if down {
                if repeat {
                    return false;
                }
                if self.is_open {
                    if self.audio_panel_open {
                        return false;
                    }
                    if self.stack.len() > 1 {
                        // Subscreen or confirm popup: pop back to main screen.
                        let was_confirm =
                            matches!(self.stack.last(), Some(PauseScreenState::Confirm { .. }));
                        self.stack.pop();
                        self.sounds.push(if was_confirm {
                            "shell_menu_cancel"
                        } else {
                            "shell_menu_exit"
                        });
                        return true;
                    }
                    if self.opened_by.as_deref() == Some("pointer")
                        && self.time - self.opened_at < LOCK_GRACE
                    {
                        return true;
                    }
                    self.resume_on_release = true;
                    return true;
                }
                if self.escape_owner_open || self.audio_panel_open {
                    return false;
                }
                self.open("key", surface.height());
                return true;
            } else {
                self.opened_by = None;
                if !self.resume_on_release {
                    return false;
                }
                self.resume_on_release = false;
                if self.is_open && self.stack.len() == 1 {
                    self.close(true);
                    return true;
                }
                return false;
            }
        }

        if !down || !self.is_open || self.audio_panel_open {
            return false;
        }

        match self.stack.last_mut() {
            Some(PauseScreenState::Main) => {
                let row_count = self.visible_rows(surface).len();
                match code {
                    "ArrowUp" | "KeyW" => {
                        if row_count > 0 {
                            self.back_focused = false;
                            self.focused_row = (self.focused_row + row_count - 1) % row_count;
                            self.sounds.push("shell_select_change");
                        }
                        true
                    }
                    "ArrowDown" | "KeyS" => {
                        if row_count > 0 {
                            self.back_focused = false;
                            self.focused_row = (self.focused_row + 1) % row_count;
                            self.sounds.push("shell_select_change");
                        }
                        true
                    }
                    "Enter" | "NumpadEnter" | "Space" if !repeat => {
                        if self.back_focused {
                            self.close(true);
                        } else {
                            let idx = self.focused_row;
                            self.activate_main_row(idx, surface, settings);
                        }
                        true
                    }
                    _ => false,
                }
            }
            Some(PauseScreenState::Confirm { focused_button, .. }) => match code {
                "ArrowLeft" | "KeyA" | "ArrowRight" | "KeyD" => {
                    *focused_button = if *focused_button == 0 { 1 } else { 0 };
                    self.sounds.push("shell_select_change");
                    true
                }
                "Enter" | "NumpadEnter" | "Space" if !repeat => {
                    let yes = *focused_button == 0;
                    self.answer_confirm(yes, settings);
                    true
                }
                _ => false,
            },
            Some(PauseScreenState::Options { focused_row }) => match code {
                "ArrowUp" | "KeyW" => {
                    *focused_row = (*focused_row + 3) % 4;
                    self.sounds.push("shell_select_change");
                    true
                }
                "ArrowDown" | "KeyS" => {
                    *focused_row = (*focused_row + 1) % 4;
                    self.sounds.push("shell_select_change");
                    true
                }
                "Backspace" if !repeat => {
                    self.stack.pop();
                    self.sounds.push("shell_menu_exit");
                    true
                }
                _ => false,
            },
            Some(PauseScreenState::PlayerList { selected, .. }) => {
                let count = self.players.len();
                match code {
                    "ArrowUp" | "KeyW" => {
                        if count > 0 {
                            *selected = selected.saturating_sub(1);
                            self.sounds.push("shell_select_change");
                        }
                        true
                    }
                    "ArrowDown" | "KeyS" => {
                        if count > 0 {
                            *selected = (*selected + 1).min(count - 1);
                            self.sounds.push("shell_select_change");
                        }
                        true
                    }
                    "Backspace" if !repeat => {
                        self.stack.pop();
                        self.sounds.push("shell_menu_exit");
                        true
                    }
                    _ => false,
                }
            }
            None => false,
        }
    }

    /// Moves the menu pointer in interface coordinates (`512, 384` center) and updates hover focus.
    pub fn move_pointer(&mut self, x: f64, y: f64, height: f64) {
        self.pointer[0] = x.clamp(0.0, W);
        self.pointer[1] = y.clamp(0.0, height);
        self.pointer_visible = true;
        if let Some(target) = self.hit(self.pointer[0], self.pointer[1]) {
            match target {
                PauseHitTarget::MainRow(idx) => {
                    self.back_focused = false;
                    self.focused_row = idx;
                }
                PauseHitTarget::OptionRow(idx) => {
                    self.back_focused = false;
                    if let Some(PauseScreenState::Options { focused_row }) = self.stack.last_mut() {
                        *focused_row = idx;
                    }
                }
                PauseHitTarget::PlayerRow(idx) => {
                    self.back_focused = false;
                    if let Some(PauseScreenState::PlayerList { selected, .. }) =
                        self.stack.last_mut()
                    {
                        *selected = idx;
                    }
                }
                PauseHitTarget::ConfirmButton(idx) => {
                    self.back_focused = false;
                    if let Some(PauseScreenState::Confirm { focused_button, .. }) =
                        self.stack.last_mut()
                    {
                        *focused_button = idx;
                    }
                }
                PauseHitTarget::BackButton => {
                    self.back_focused = true;
                }
            }
        }
    }

    /// Clicks at interface coordinates `(x, y)` (`bindMenuInput` in `src/menus/input.js`).
    pub fn click(
        &mut self,
        x: f64,
        y: f64,
        surface: &dyn BattleUiSurface,
        settings: &dyn PauseSettingsLookup,
    ) -> Option<PauseHitTarget> {
        self.move_pointer(x, y, surface.height());
        let target = self.hit(x, y)?;
        match target {
            PauseHitTarget::MainRow(idx) => {
                self.focused_row = idx;
                self.activate_main_row(idx, surface, settings);
            }
            PauseHitTarget::OptionRow(idx) => {
                if let Some(PauseScreenState::Options { focused_row }) = self.stack.last_mut() {
                    *focused_row = idx;
                }
            }
            PauseHitTarget::PlayerRow(idx) => {
                if let Some(PauseScreenState::PlayerList { selected, .. }) = self.stack.last_mut() {
                    *selected = idx;
                }
            }
            PauseHitTarget::ConfirmButton(idx) => {
                self.answer_confirm(idx == 0, settings);
            }
            PauseHitTarget::BackButton => {
                if self.stack.len() > 1 {
                    let was_confirm =
                        matches!(self.stack.last(), Some(PauseScreenState::Confirm { .. }));
                    self.stack.pop();
                    self.sounds.push(if was_confirm {
                        "shell_menu_cancel"
                    } else {
                        "shell_menu_exit"
                    });
                } else {
                    self.close(true);
                }
            }
        }
        Some(target)
    }

    /// Hit-tests interface point `(x, y)` against the most recently drawn hotspots (`ClassicTheme.hit`).
    pub fn hit(&self, x: f64, y: f64) -> Option<PauseHitTarget> {
        for hotspot in self.hotspots.iter().rev() {
            if inside_quad(x, y, &hotspot.corners) {
                return Some(hotspot.target);
            }
        }
        None
    }

    /// Steps animations and timers by `dt` seconds (`update` in `src/pause/index.js`).
    pub fn update(&mut self, dt: f64) {
        if self.is_open || self.waiting_time.is_some() {
            self.time += dt;
            self.title_intro.update(dt);
            for intro in &mut self.row_intros {
                intro.update(dt);
            }
            self.back_intro.update(dt);
            self.popup_yes_intro.update(dt);
            self.popup_no_intro.update(dt);
        }
        if let Some(w) = &mut self.waiting_time {
            *w += dt;
        }
    }
}
