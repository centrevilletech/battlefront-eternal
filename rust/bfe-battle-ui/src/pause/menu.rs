// Classic in-battle pause menu (`ifs_pausemenu`), confirm popups, and `still_command`.
//
// Ports `src/pause/index.js`, `src/menus/themes/classic/index.js`,
// `src/menus/popup.js`, and `stillCommand` from `src/runtime/commands.js`.
//
// Key design invariants from the 2004 game (`ifs_pausemenu`):
// - The main pause menu stack sits at safe-area anchor `(0.6, 0.5)` with `y = 20` and a
//   25-degree Y-axis 3D tilt (`rot_y_deg = 25.0`), without dimming the frozen world behind it.
// - Its horizontal container offset (`rows_shift`) is computed from `LIST_COUNT = 11` (which
//   includes the 5 hidden PC debug/console/friends rows in the original Lua script) rather than
//   the number of visible rows, while individual button bar widths (`row_bars`) step outward
//   to the vertical midpoint and back in across the `n` visible rows.
// - Every destructive choice (`RESPAWN`, `RESTART MISSION`, `QUIT`, `EXIT TO WINDOWS`) pushes a
//   `Confirm` popup with `focused_button = 1` (`NO`) focused by default (`focus = "no"` in
//   `src/menus/popup.js`) so an accidental double-tap on `Enter` or `Space` never aborts a match.
// - Pressing `Escape` to close the main pause menu sets `resume_on_release = true` and waits for
//   the `Escape` keyup before closing (`LOCK_GRACE = 0.25s` also ignores an `Escape` keydown that
//   arrived right after a browser pointer-unlock caused by that same physical key press).

use super::overrides::{with_settings, PauseSettingsLookup};
use crate::scoreboard::PlayerListRow;
use crate::shapes::{
    inside_quad, BattleUiSurface, GameFamily, GameFamilyError, TitleIntro, FONT_NAME_MEDIUM, W,
};

/// Pause menu layout constants (`LAYOUT` in `src/pause/index.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PauseLayoutConfig {
    /// Safe-area anchor `[rx, ry]` (`[0.6, 0.5]` in `ifs_pausemenu`).
    pub at: [f64; 2],
    /// Vertical offset in interface units below the safe-area anchor (`20.0`).
    pub y: f64,
    /// Y-axis 3D rotation in degrees (`25.0`), tilting the button stack in perspective.
    pub rot_y_deg: f64,
    /// Font name used for the title bar and menu rows (`"gamefont_medium"`).
    pub font: &'static str,
    /// Base width of the top and bottom row bars before per-row pitch stepping (`260.0`).
    pub width: f64,
    /// Vertical spacing added to the font line height to form the row pitch (`5.0`).
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
///
/// The original `ifs_pausemenu` defines 11 buttons (5 of which—free camera, player list, console,
/// friends, recent players—are hidden in single-player PC), and `AddVerticalButtons` sizes the
/// container shift from all 11 entries.
pub const LIST_COUNT: usize = 11;

/// Seconds after pointer-unlock during which an `Escape` keydown is treated as the same press (`LOCK_GRACE = 250ms` in `src/pause/index.js`).
///
/// When the player presses `Escape` while pointer-locked, the browser releases pointer lock first
/// (which opens the menu with `opened_by = Some("pointer")`) and then delivers the `Escape` keydown;
/// ignoring `Escape` within `0.25s` prevents that single press from immediately closing the menu.
pub const LOCK_GRACE: f64 = 0.25;

/// Seconds before `"CLICK TO RESUME"` renders while waiting for pointer lock (`PROMPT_DELAY = 0.15s` in `src/pause/index.js`).
///
/// Avoids flashing `"CLICK TO RESUME"` on a single frame when pointer lock is reacquired immediately.
pub const PROMPT_DELAY: f64 = 0.15;

/// Stagger delay in seconds between consecutive menu row `TitleIntro` animations (`ROW_INTRO = 0.1` in `src/menus/themes/classic/index.js`).
const ROW_INTRO: f64 = 0.1;

/// Neutral input command packet sent each tick while paused in a network game (`stillCommand` in `src/runtime/commands.js`).
///
/// In a multiplayer match the world continues ticking while the local pause menu is open.
/// Sending an explicit neutral command (rather than omitting gamepad axes) is required because
/// the server's `InputMailbox` retains the last gamepad state when a packet omits gamepad data,
/// which would otherwise leave a stick held when entering the menu deflecting the player's unit.
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
///
/// Replicates the original `AddVerticalButtons` width progression: starting from `width` (plus
/// one `pitch` when `count` is odd), the bar width grows by `pitch` for each row above `count / 2`
/// and shrinks by `pitch` below `count / 2`. The container is shifted left by `-widest / 2.0`.
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
    /// Width of the stretched middle bar piece (`buttonmid`) before adding the 5-unit pad.
    pub width: f64,
    /// Whether the right-end diagonal cap (`buttonright`) is vertically flipped (`i > n / 2`),
    /// forming a convex chevron silhouette along the right edge of the button stack.
    pub flipped: bool,
}

/// Computes bar widths and flip flags for `n` visible rows (`rowBars` in `src/menus/themes/classic/index.js`).
///
/// Mirrors `ShowHideVerticalButtons`: rows `1..=n/2` widen by `pitch` per row with an unflipped
/// right cap, the middle row holds peak width, and rows past `n / 2` narrow by `pitch` per row
/// with a vertically flipped right cap.
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
///
/// Mirrors the `ScreenStack` in `src/pause/index.js`: `Main` sits at the bottom of the stack,
/// and activating a row pushes `Confirm`, `Options`, or `PlayerList` on top of it.
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
///
/// Matches the three `restart` modes in `src/pause/index.js`:
/// - `Url`: single-player / local battle default (`restart === undefined`), reloading the battle URL
///   with current option overrides applied via [`with_settings`].
/// - `Custom`: multiplayer host action (`restart = { label, prompt, run }`), invoking a host reset
///   and resuming play.
/// - `Hidden`: remote multiplayer client (`restart === false`), omitting the row altogether.
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
///
/// Supplied by the spawn controller (`pauseRespawn`) to override the row label/prompt if needed
/// and to report whether the local player currently has a living soldier that can suicide/respawn.
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
///
/// Stores the 4 projected screen-plane corners (`[[x, y]; 4]`) after applying the row container's
/// 25-degree Y-axis 3D rotation so [`inside_quad`] hit-tests accurately against angled buttons.
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
    /// Active game family (`Swbf1` 2004 layout; `Swbf2` returns `GameFamilyError`).
    pub family: GameFamily,
    /// Whether the pause menu overlay is currently open (`isOpen` in `src/pause/index.js`).
    pub is_open: bool,
    /// Timestamp (`self.time`) when `open()` was last called, used with [`LOCK_GRACE`].
    pub opened_at: f64,
    /// Reason string passed to `open(why)` (`"key"` or `"pointer"`).
    pub opened_by: Option<String>,
    /// Set on `Escape` keydown while on `Main`; the menu closes only on the matching `Escape`
    /// keyup so the browser does not immediately drop a newly reacquired pointer lock.
    pub resume_on_release: bool,
    /// Elapsed seconds waiting for a pointer-lock click after `close(true)`, or `None`.
    pub waiting_time: Option<f64>,
    /// Accumulated animation clock in seconds (`stack.time` in `src/menus/stack.js`).
    pub time: f64,
    /// Index of the focused row on `PauseScreenState::Main`.
    pub focused_row: usize,
    /// Whether the bottom-left `BACK` button currently holds hover/selection focus.
    pub back_focused: bool,
    /// Active screen stack (`Main` at index 0, plus optional `Confirm`, `Options`, or `PlayerList`).
    pub stack: Vec<PauseScreenState>,
    /// Cursor position in `1024xH` interface coordinates (`stack.pointer` in `src/pause/index.js`).
    pub pointer: [f64; 2],
    /// Whether the Classic `mousecursor` quad is visible (`stack.pointer.visible`).
    pub pointer_visible: bool,
    /// Master toggle for drawing the software cursor (`stack.showPointer`).
    pub show_pointer: bool,
    /// Optional `RESPAWN` row configuration supplied by the spawn screen (`enableRespawn`).
    pub respawn: Option<PauseRespawnConfig>,
    /// Controls whether `RESTART MISSION` reloads a URL, runs a custom host callback, or hides.
    pub restart_mode: PauseRestartMode,
    /// Target URL navigated to when confirming `QUIT` or falling back from `EXIT TO WINDOWS`.
    pub main_menu_url: String,
    /// Whether the network `PLAYER LIST` row is included on the main pause screen.
    pub show_player_list: bool,
    /// Connected human players displayed on the `PLAYER LIST` sub-screen.
    pub players: Vec<PlayerListRow>,
    /// True while the DOM `#audio-panel` modal is open (`audioPanelOpen()` in `src/pause/index.js`),
    /// which takes precedence over pause menu `Escape` handling.
    pub audio_panel_open: bool,
    /// True while another screen in `setEscapeOwners` (spawn screen or scoreboard) owns `Escape`.
    pub escape_owner_open: bool,
    /// Most recent navigation triggered by `RESTART MISSION`, `QUIT`, or `EXIT TO WINDOWS`.
    pub leaving: Option<LeavingRecord>,
    /// Count of confirmed `RESPAWN` (suicide) actions executed.
    pub respawn_triggered: usize,
    /// Count of confirmed custom host `RESTART MISSION` actions executed.
    pub custom_restart_triggered: usize,
    /// Count of confirmed `EXIT TO WINDOWS` `closeWindow()` calls executed.
    pub window_close_triggered: usize,
    /// Total number of times the pause menu has transitioned from closed to open.
    pub open_events: usize,
    /// History of `resume` booleans passed to each `close(resume)` call.
    pub close_events: Vec<bool>,
    /// Queued UI sound names (`"shell_menu_enter"`, `"shell_menu_accept"`, `"shell_menu_cancel"`, etc.).
    pub sounds: Vec<&'static str>,
    /// `FlashyText` intro animation for the orange `"GAME PAUSED"` header bar.
    pub(crate) title_intro: TitleIntro,
    /// Staggered `FlashyText` intro animations for the up to `LIST_COUNT` (`11`) menu rows.
    pub(crate) row_intros: [TitleIntro; LIST_COUNT],
    /// `FlashyText` intro animation for the bottom-left `"BACK"` button.
    pub(crate) back_intro: TitleIntro,
    /// `FlashyText` intro animation for `"YES"` inside confirmation popups.
    pub(crate) popup_yes_intro: TitleIntro,
    /// `FlashyText` intro animation for `"NO"` inside confirmation popups.
    pub(crate) popup_no_intro: TitleIntro,
    /// Screen-plane bounding quads recorded during the most recent `PauseMenu::draw` pass.
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
    ///
    /// Builds the ordered row list (`RESUME GAME`, `OPTIONS`, `RESPAWN`, optional `RESTART MISSION`,
    /// optional network `PLAYER LIST`, `QUIT`, `EXIT TO WINDOWS`) localized via `surface`.
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
    ///
    /// Warps the menu pointer to `(512, height / 2)` (matching the 2004 game's center warp),
    /// resets focus to row `0` (`RESUME GAME`), and starts the staggered `TitleIntro` animations.
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

    /// Starts the title bar, `BACK` button, and staggered `(k + 1) * 0.1s` row intro animations.
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
    ///
    /// Opening the popup is intentionally silent (`confirmPopup` in `src/menus/popup.js`);
    /// sound effects play only when the user answers `YES` (`shell_menu_accept`) or `NO`/`Escape`
    /// (`shell_menu_cancel`).
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
    ///
    /// Plays `shell_menu_enter` and either closes the menu (`Resume`), pushes a sub-screen
    /// (`Options`, `PlayerList`), or pushes a `Confirm` popup (`Suicide`, `Restart`, `Quit`, `Exit`).
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
    ///
    /// Canceling pops the popup and plays `shell_menu_cancel`. Confirming plays `shell_menu_accept`
    /// and executes the corresponding action: triggering soldier suicide + resume (`Suicide`),
    /// rewriting the battle URL via [`with_settings`] or running the host restart hook (`Restart`),
    /// navigating to the main menu (`Quit`), or requesting window close with main-menu fallback (`Exit`).
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
    ///
    /// - `Escape` on a sub-screen (`Confirm`, `Options`, `PlayerList`) pops back to `Main`
    ///   immediately on keydown.
    /// - `Escape` on `Main` sets `resume_on_release = true` on keydown (unless suppressed by
    ///   [`LOCK_GRACE`] after a pointer-unlock open) and closes the menu on the subsequent keyup.
    /// - Arrow keys / `WASD`, `Enter` / `Space`, and `Backspace` navigate the active screen.
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
                    // Ignore `Escape` keydown if pointer-unlock opened the menu within `LOCK_GRACE`.
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

        // Dispatch navigation keys to whichever screen currently sits at the top of `self.stack`.
        match self.stack.last_mut() {
            // Main pause menu: Up/Down/W/S cycle `focused_row` with wrap-around and clear `back_focused`;
            // Enter/Space activates either the focused main row or the bottom-left `BACK` button.
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
            // Confirmation popup: Left/Right/A/D toggle between `YES` (`0`) and `NO` (`1`),
            // while Enter/Space answers the prompt with the currently focused button.
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
            // Options sub-screen: Up/Down/W/S cycle the 4 option category buttons (`0..4`),
            // and `Backspace` pops back to the main pause screen.
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
            // Network player list: Up/Down/W/S clamp selection within `0..players.len()`,
            // and `Backspace` pops back to the main pause screen.
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
    ///
    /// Clamps the cursor to `[0..1024, 0..height]` and checks [`Self::hit`] against the most
    /// recently drawn quad hotspots so hovering a row, confirm button, or `BACK` focuses it.
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
    ///
    /// Moves the pointer to `(x, y)` first, then dispatches the topmost hit hotspot (`MainRow`,
    /// `OptionRow`, `PlayerRow`, `ConfirmButton`, or `BackButton`).
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
    ///
    /// Iterates in reverse draw order so topmost widgets win when bounding quads overlap.
    /// A theme replaces the presentation's hit geometry; actions stay here.
    pub fn set_theme_hotspots(&mut self, hotspots: &mut Vec<PauseHotspot>) {
        std::mem::swap(&mut self.hotspots,hotspots);
    }

    pub fn hit(&self, x: f64, y: f64) -> Option<PauseHitTarget> {
        for hotspot in self.hotspots.iter().rev() {
            if inside_quad(x, y, &hotspot.corners) {
                return Some(hotspot.target);
            }
        }
        None
    }

    /// Steps animations and timers by `dt` seconds (`update` in `src/pause/index.js`).
    /// Steps animations and timers by `dt` seconds (`update` in `src/pause/index.js`).
    ///
    /// While `is_open` or `waiting_time.is_some()` (`"CLICK TO RESUME"`), advances `self.time`
    /// (driving `self.pulse()`) and all active `TitleIntro` state machines.
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
