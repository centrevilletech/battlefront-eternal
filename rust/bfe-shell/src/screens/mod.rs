//! Shell screens (`MAIN MENU`, `SINGLEPLAYER`, `INSTANT ACTION`, `OPTIONS`,
//! `PROFILE MANAGEMENT`, `MULTIPLAYER`, `EXIT TO WINDOWS`) and integration with
//! [`bfe_online::browser::ServerBrowserService`] (`src/shell/screens.js`,
//! `src/shell/index.js`).
//!
//! Ports `createScreens` from `src/shell/screens.js` natively:
//! - Screen construction and state transitions are pure and renderer-independent
//!   so all menu navigation, Instant Action playlist editing, era picker popups,
//!   options pages, profile management, multiplayer gating, and exit confirmation
//!   can be exercised in headless tests and drawn onto `bfe_render::interface::Batch`.
//! - Every screen maps to its [`ShellMedia`] cue via [`ScreenId::media_id`]
//!   (`"start"`, `"main"`, `"sp"`, `"ia"`, `"options"`, `"profile"`, `"mp"`),
//!   matching `SCREEN_MEDIA` in `src/shell/media.js` and `mediaId()` in
//!   `src/shell/index.js`.
//! - Row lists are cached in `ShellApp::cached_rows` whenever the active screen
//!   or underlying state changes so `draw_shell` can render at 60+ FPS with
//!   zero per-frame heap allocations.
//! - Sound cues (`"shell_select_change"`, `"shell_menu_accept"`,
//!   `"shell_menu_enter"`, `"shell_menu_cancel"`, `"shell_menu_error"`,
//!   `"shell_menu_exit"`, `"shell_keyboard_key"`, `"shell_keyboard_backspace"`)
//!   are queued in `ShellApp::sound_queue` (`INTERFACE_SOUNDS` in `src/shell/audio.js`).

use crate::{
    data::ShellData,
    launch::{BattleRequest, GameFamily, PlaylistEntry},
    media::{MediaEvent, ShellMedia},
    profile::ProfileState,
    version::{version_line, AboutInfo},
};
use bfe_online::{browser::ServerEntry, direct::DEFAULT_DIRECT_PORT};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod multiplayer;
pub use self::multiplayer::*;

/// Identifiers for every screen and modal in the shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenId {
    /// Click/key gate (`ifs_start`: `PRESS ANY KEY OR CLICK TO BEGIN`).
    Gate,
    /// `MAIN MENU` (`SINGLEPLAYER`, `MULTIPLAYER`, `OPTIONS`, `PROFILE MANAGEMENT`,
    /// `EXPORT CONTENT`, `IMPORT CONTENT`, `EXIT TO WINDOWS`).
    Main,
    /// `SINGLEPLAYER` (`HISTORICAL CAMPAIGN` disabled, `GALACTIC CONQUEST`
    /// disabled, `INSTANT ACTION`).
    Singleplayer,
    /// `INSTANT ACTION` dual-list playlist builder (`AVAILABLE MAPS`,
    /// `SELECTED MAPS`, `ADD`/`REMOVE`, `PLAY IN ORDER`/`RANDOM ORDER`, `LAUNCH`).
    InstantAction,
    /// Era picker popup pushed when adding a map that supports both `cw` and `gcw`.
    InstantActionEra,
    /// `OPTIONS` top menu (`GAME OPTIONS`, `VIDEO OPTIONS`, `AUDIO OPTIONS`,
    /// `CONTROL OPTIONS`, `ONLINE OPTIONS`, `CREDITS`).
    Options,
    /// Sub-page of `OPTIONS` (`GAME OPTIONS`, `VIDEO OPTIONS`, `AUDIO OPTIONS`,
    /// `CONTROL OPTIONS`).
    OptionsPage(OptionsPageId),
    /// `PROFILE MANAGEMENT` (`<playerName>`, `CREATE PROFILE`, `DELETE PROFILE`).
    Profile,
    /// `MULTIPLAYER` (`HOST`, `LAN`, `ONLINE`).
    Multiplayer,
    /// `LAN` or `ONLINE` server list screen (`mp-lan` / `mp-online`).
    MultiplayerList(MultiplayerKind),
    /// Direct connection address input modal (`DIRECT CONNECTION`).
    DirectConnect,
    /// Host server configuration screen (`SET GAME OPTIONS`).
    HostDialog,
    /// Modal notice popup (`OK`), exit confirmation (`YES` / `NO`), or
    /// 3-button hosted server close prompt.
    Popup,
}

impl ScreenId {
    /// Returns the `SCREEN_MEDIA` key (`"start"`, `"main"`, `"sp"`, `"ia"`,
    /// `"options"`, `"profile"`, `"mp"`) for [`ShellMedia`].
    pub fn media_id(self) -> &'static str {
        match self {
            Self::Gate => "start",
            Self::Main => "main",
            Self::Singleplayer => "sp",
            Self::InstantAction | Self::InstantActionEra => "ia",
            Self::Options | Self::OptionsPage(_) => "options",
            Self::Profile => "profile",
            Self::Multiplayer
            | Self::MultiplayerList(_)
            | Self::DirectConnect
            | Self::HostDialog => "mp",
            Self::Popup => "main",
        }
    }
}

/// Sub-pages of the `OPTIONS` screen (`bfe-interface` option pages).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionsPageId {
    /// `GAME OPTIONS` (`menuTheme`, `battleSize`, `difficulty`, `reset`).
    Game,
    /// `VIDEO OPTIONS` (`resolution`, `antialiasing`, `shadows`, `water`, `reset`).
    Video,
    /// `AUDIO OPTIONS` (`masterVolume`, `musicVolume`, `effectsVolume`, `interfaceVolume`, `reset`).
    Audio,
    /// `CONTROL OPTIONS` (`sensitivity`, `invertY`, `reset`).
    Controls,
}

impl OptionsPageId {
    /// Returns the uppercase screen header title for this options sub-page.
    pub fn title(self) -> &'static str {
        match self {
            Self::Game => "GAME OPTIONS",
            Self::Video => "VIDEO OPTIONS",
            Self::Audio => "AUDIO OPTIONS",
            Self::Controls => "CONTROL OPTIONS",
        }
    }
}

/// One row in a `Rows` menu screen (`id`, `label`, `disabled`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuRow {
    /// Stable action identifier (`"sp"`, `"mp"`, `"options"`, server id, etc.).
    pub id: String,
    /// Uppercase text rendered inside the row's bar.
    pub label: String,
    /// Whether the row is non-selectable and rendered in `GREY`.
    pub disabled: bool,
}

impl MenuRow {
    /// Constructs an enabled, selectable [`MenuRow`].
    pub fn enabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
        }
    }

    /// Constructs a disabled (`GREY`, non-selectable) [`MenuRow`].
    pub fn disabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: true,
        }
    }
}

/// Focus target inside the `INSTANT ACTION` screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IaFocus {
    /// Left `AVAILABLE MAPS` listbox (`id: "available"`).
    #[default]
    Available,
    /// Right `SELECTED MAPS` playlist listbox (`id: "selected"`).
    Selected,
    /// Center `ADD` horizontal button (`id: "add"`).
    Add,
    /// Center `REMOVE` horizontal button (`id: "remove"`).
    Remove,
    /// Bottom-left `BACK` button (`id: "back"`).
    Back,
    /// Bottom-center `PLAY IN ORDER` / `RANDOM ORDER` toggle button (`id: "order"`).
    Order,
    /// Bottom-right `LAUNCH` button (`id: "launch"`).
    Launch,
}

/// Modal popup kind (`Ok` notice, `ConfirmExit` `YES`/`NO`, or `HostClosePrompt` 3-button dialog).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopupState {
    /// Single-button notice dialog (`okPopup` in `src/menus/index.js`).
    Notice {
        /// Centered message text displayed above the `OK` button.
        message: String,
    },
    /// Two-button confirmation dialog (`confirmPopup` in `src/menus/index.js`).
    ConfirmExit {
        /// Prompt text (`"EXIT TO WINDOWS?"`).
        message: String,
        /// Whether `YES` (`true`) or `NO` (`false`) currently has focus.
        selected_yes: bool,
    },
    /// Online service consent; cancel is selected until the player accepts.
    OnlineNotice { message: String, selected_accept: bool },
    /// Three-button close prompt when a hosted server is running (`bfe_desktop::host::ClosePrompt`).
    HostClosePrompt {
        /// Dialog title (`"Hosted server is running"`).
        title: String,
        /// Dialog message (`"<name> is currently hosting a match."`).
        message: String,
        /// Button labels (`["Keep server running", "Stop server and quit", "Cancel"]`).
        buttons: [String; 3],
        /// Currently highlighted button index (`0..3`).
        selected: usize,
    },
}

/// Action returned to the host application when a shell action completes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellAction {
    /// Launch a singleplayer/Instant Action battle (`BattleRequest`).
    LaunchBattle(BattleRequest),
    /// Launch or join a multiplayer session (`MultiplayerLaunch`).
    LaunchMultiplayer(MultiplayerLaunch),
    /// Close the application window (`EXIT TO WINDOWS` confirmed).
    ExitToWindows,
    /// User decision on the 3-button `HostClosePrompt` (`0` = KeepServer, `1` = StopServer, `2` = Cancel).
    HostCloseDecision(usize),
}

/// Option value stored in [`ShellSettings`] for the `OPTIONS` pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptionItem {
    /// Setting key in `ShellSettings` (e.g. `"masterVolume"`, `"menuTheme"`).
    pub key: String,
    /// Display label on the left side of the option row.
    pub label: String,
    /// Formatted current value on the right side of the option row.
    pub value_text: String,
}

/// Player settings for volumes, theme, and options pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellSettings {
    /// Master output volume (`0.0..=1.0`, default `1.0`).
    pub master_volume: f32,
    /// Shell and battle music volume (`0.0..=1.0`, default `1.0`).
    pub music_volume: f32,
    /// Menu interface and transition whoosh volume (`0.0..=1.0`, default `1.0`).
    pub interface_volume: f32,
    /// Battle sound effects volume (`0.0..=1.0`, default `1.0`).
    pub effects_volume: f32,
    /// Active front-end look (`"eternal"`, `"bf1"`, or `"bf2"`).
    pub menu_theme: String,
    /// AI unit count preset (`"full"`, `"large"`, or `"small"`).
    pub battle_size: String,
    /// AI difficulty preset (`"medium"`).
    pub difficulty: String,
    /// Look sensitivity (`0.1..=1.0`, default `0.5`).
    pub mouse_sensitivity: f32,
    /// Whether vertical mouse look is inverted (`false` by default).
    pub invert_y: bool,
}

impl Default for ShellSettings {
    /// Returns the default player settings matching `src/settings.js`.
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            music_volume: 1.0,
            interface_volume: 1.0,
            effects_volume: 1.0,
            menu_theme: "eternal".to_string(),
            battle_size: "full".to_string(),
            difficulty: "medium".to_string(),
            mouse_sensitivity: 0.5,
            invert_y: false,
        }
    }
}

/// Complete state of the front-end shell (`createScreens` in `src/shell/screens.js`).
pub struct ShellApp {
    /// Loaded `shell.json`, `common.json`, and localization strings.
    pub data: ShellData,
    /// Screen movie, camera flythrough, whoosh, and music controller (`src/shell/media.js`).
    pub media: ShellMedia,
    /// Current audio, video, game, and control settings.
    pub settings: ShellSettings,
    /// Player profile name and callsign editor state (`src/shell/profile-dialog.js`).
    pub profile: ProfileState,
    /// Formatted bottom-right `MAIN MENU` version string (`src/shell/version.js`).
    pub version_text: String,
    /// Navigation stack of screens (`Gate` -> `Main` -> `Singleplayer` -> `InstantAction`, etc.).
    pub stack: Vec<ScreenId>,
    /// Selected row index per row-based screen.
    pub row_cursor: BTreeMap<&'static str, usize>,
    /// Cached rows for the active base screen, updated when state changes so
    /// per-frame rendering (`draw_shell`) performs zero heap allocations.
    pub cached_rows: Vec<MenuRow>,
    /// Selected game family in Instant Action's `AVAILABLE MAPS` game picker
    /// (`GameFamily::Swbf1` = `BATTLEFRONT (2004)`, `GameFamily::Swbf2` planned and disabled).
    pub ia_family: GameFamily,
    /// Instant Action available map cursor (`0..data.maps.len()`).
    pub ia_available_selected: usize,
    /// Instant Action selected playlist cursor (`0..ia_entries.len()`).
    pub ia_selected_cursor: usize,
    /// Instant Action active focus control.
    pub ia_focus: IaFocus,
    /// Instant Action playlist entries (`[{ family, map, era, mode }]`, capped at `MAX_PLAYLIST`).
    pub ia_entries: Vec<PlaylistEntry>,
    /// Instant Action `RANDOM ORDER` toggle (`false` = `PLAY IN ORDER`, `true` = `RANDOM ORDER`).
    pub ia_random: bool,
    /// Map id and era choices (`["cw", "gcw"]`) for the `InstantActionEra` popup.
    pub ia_era_choices: Option<(String, String, Vec<String>, usize)>,
    /// Current options sub-page cursor.
    pub options_page_cursor: usize,
    /// Visible server entries synchronized from [`ServerBrowserService`].
    pub mp_servers: Vec<ServerEntry>,
    /// Status or error line synchronized from [`ServerBrowserService`].
    pub mp_status: Option<String>,
    /// Whether the online screen is waiting for privacy notice acceptance (`BrowserState::Notice`).
    pub mp_needs_notice: bool,
    /// Direct connect draft address (`"127.0.0.1:18451"`).
    pub direct_connect_draft: String,
    pub direct_port_draft: String,
    pub direct_field: usize,
    /// Host game configuration (`SET GAME OPTIONS` on `ScreenId::HostDialog`).
    pub host_config: HostConfig,
    /// Pending multiplayer action after the profile prompt finishes.
    pub(super) pending_mp_after_profile: Option<PendingMpAction>,
    /// Active modal popup (`Notice`, `ConfirmExit`, or `HostClosePrompt`), if any.
    pub popup: Option<PopupState>,
    /// Elapsed time on the click gate (for `gate::pulse`).
    pub gate_elapsed: f64,
    /// Elapsed time on the current screen (for button label pulse).
    pub pulse_elapsed: f64,
    /// Queued interface sound names (`"shell_select_change"`, `"shell_menu_accept"`, etc.).
    pub sound_queue: Vec<&'static str>,
    /// Queued media events from the latest screen transition.
    pub media_events: Vec<MediaEvent>,
    /// Latest [`ShellAction`] triggered by the user (`LaunchBattle`, `LaunchMultiplayer`, `ExitToWindows`).
    pub pending_action: Option<ShellAction>,
}

/// Multiplayer action deferred while `require_profile_for` prompts the player
/// to replace `DEFAULT_PLAYER_NAME` (`"PLAYER 1"`) with a custom callsign.
#[derive(Clone, Debug)]
pub(super) enum PendingMpAction {
    /// Resume hosting a local game once a profile callsign is saved.
    Host,
    /// Resume joining the selected [`ServerEntry`] once a profile callsign is saved.
    Join(Box<ServerEntry>, MultiplayerKind),
}

impl ShellApp {
    /// Creates a new `ShellApp` starting on the click gate (`ScreenId::Gate`),
    /// matching `createShell` in `src/shell/index.js`.
    pub fn new(data: ShellData, about: Option<&AboutInfo>) -> Self {
        let mut media = ShellMedia::new(&data);
        let media_events = media.enter_immediate(&data, "start", false);
        let version_text = version_line(about, None);
        let mut app = Self {
            data,
            media,
            settings: ShellSettings::default(),
            profile: ProfileState::default(),
            version_text,
            stack: vec![ScreenId::Gate],
            row_cursor: BTreeMap::new(),
            cached_rows: Vec::new(),
            ia_family: GameFamily::Swbf1,
            ia_available_selected: 0,
            ia_selected_cursor: 0,
            ia_focus: IaFocus::Available,
            ia_entries: Vec::new(),
            ia_random: false,
            ia_era_choices: None,
            options_page_cursor: 0,
            mp_servers: Vec::new(),
            mp_status: None,
            mp_needs_notice: false,
            direct_connect_draft: String::new(),
            direct_port_draft: DEFAULT_DIRECT_PORT.to_string(),
            direct_field:0,
            host_config: HostConfig::default(),
            pending_mp_after_profile: None,
            popup: None,
            gate_elapsed: 0.0,
            pulse_elapsed: 0.0,
            sound_queue: Vec::new(),
            media_events,
            pending_action: None,
        };
        app.rebuild_cached_rows();
        app
    }

    /// Rebuilds `self.cached_rows` for the current base screen.
    pub fn rebuild_cached_rows(&mut self) {
        let base = self.stack.last().copied().unwrap_or(ScreenId::Main);
        self.cached_rows = self.rows_for(base);
    }

    /// Returns the cached rows for the current base screen without allocating.
    pub fn active_rows(&self) -> &[MenuRow] {
        &self.cached_rows
    }

    /// Returns the currently highlighted row index in `self.cached_rows` for
    /// `screen` without allocating.
    pub fn selected_cached_row_index(&self, screen: ScreenId) -> usize {
        if self.cached_rows.is_empty() {
            return 0;
        }
        let key = Self::screen_key(screen);
        let raw = self
            .row_cursor
            .get(key)
            .copied()
            .unwrap_or(0)
            .min(self.cached_rows.len() - 1);
        // If the stored cursor row is enabled, return it directly; otherwise
        // fall back to the first enabled row in `cached_rows`.
        if !self.cached_rows[raw].disabled {
            return raw;
        }
        self.cached_rows
            .iter()
            .position(|r| !r.disabled)
            .unwrap_or(raw)
    }

    /// Returns the Instant Action `AVAILABLE MAPS` game picker entries:
    /// `BATTLEFRONT (2004)` (`Swbf1`, enabled) and `BATTLEFRONT II (2005), PLANNED`
    /// (`Swbf2`, disabled).
    ///
    /// Mirrors `gamePicker()` in `src/shell/index.js`.
    pub fn ia_game_picker_options(&self) -> &'static [(GameFamily, &'static str, bool)] {
        &GAME_PICKER_OPTIONS
    }

    /// Returns the label of the currently selected game family in Instant Action
    /// (`"BATTLEFRONT (2004)"`).
    pub fn ia_selected_game_label(&self) -> &'static str {
        GAME_PICKER_OPTIONS
            .iter()
            .find(|(fam, _, _)| *fam == self.ia_family)
            .map(|(_, label, _)| *label)
            .unwrap_or("BATTLEFRONT (2004)")
    }

    /// Attempts to select a game family in Instant Action's `AVAILABLE MAPS`
    /// game picker. Returns `false` for disabled (`PLANNED`) families such as
    /// `GameFamily::Swbf2`.
    pub fn ia_select_family(&mut self, family: GameFamily) -> bool {
        if let Some((_, _, true)) = GAME_PICKER_OPTIONS
            .iter()
            .find(|(fam, _, _)| *fam == family)
        {
            self.ia_family = family;
            true
        } else {
            false
        }
    }

    /// Returns the topmost active [`ScreenId`].
    pub fn current_screen(&self) -> ScreenId {
        // An active modal popup (`Notice`, `ConfirmExit`, or `HostClosePrompt`)
        // takes input priority over the navigation stack.
        if self.popup.is_some() {
            return ScreenId::Popup;
        }
        self.stack.last().copied().unwrap_or(ScreenId::Main)
    }

    /// Looks up a localized string in `data.strings`, falling back to `fallback`.
    /// Mirrors `say(key, fallback)` in `src/shell/screens.js`.
    pub fn say<'a>(&'a self, key: &str, fallback: &'a str) -> &'a str {
        self.data
            .strings
            .get(key)
            .map(String::as_str)
            .unwrap_or(fallback)
    }

    /// Advances the shell clocks (`gate_elapsed`, `pulse_elapsed`, and
    /// `media.update(dt)`).
    pub fn tick(&mut self, dt: f64) {
        if dt.is_finite() && dt > 0.0 {
            self.gate_elapsed += dt;
            self.pulse_elapsed += dt;
            // Step the shell media timeline (camera flythrough, bus dip, and
            // `ShellMusic` silence interval) and record any started track.
            if let Some(step) = self.media.update(dt) {
                self.media_events.push(MediaEvent::Music(step));
            }
        }
    }

    /// Drains queued interface sound cues (`"shell_select_change"`, etc.).
    pub fn drain_sounds(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.sound_queue)
    }

    /// Drains queued [`MediaEvent`]s from recent transitions or music ticks.
    pub fn drain_media_events(&mut self) -> Vec<MediaEvent> {
        std::mem::take(&mut self.media_events)
    }

    /// Takes any pending [`ShellAction`] (`LaunchBattle`, `LaunchMultiplayer`,
    /// `ExitToWindows`, `HostCloseDecision`).
    pub fn take_action(&mut self) -> Option<ShellAction> {
        self.pending_action.take()
    }

    /// Returns the screen title for `screen` as a borrowed `&str` without
    /// heap allocation.
    pub fn screen_title_str(&self, screen: ScreenId) -> &str {
        match screen {
            ScreenId::Gate => "",
            ScreenId::Main => self.say("ifs.main.title", "MAIN MENU"),
            ScreenId::Singleplayer => self.say("ifs.main.sp", "SINGLEPLAYER"),
            ScreenId::InstantAction => "INSTANT ACTION",
            ScreenId::InstantActionEra => self
                .ia_era_choices
                .as_ref()
                .map(|(_, name, _, _)| name.as_str())
                .unwrap_or("SELECT ERA"),
            ScreenId::Options => "OPTIONS",
            ScreenId::OptionsPage(p) => p.title(),
            ScreenId::Profile => "PROFILE MANAGEMENT",
            ScreenId::Multiplayer => "MULTIPLAYER",
            ScreenId::MultiplayerList(kind) => kind.as_str(),
            ScreenId::DirectConnect => "DIRECT CONNECTION",
            ScreenId::HostDialog => "SET GAME OPTIONS",
            ScreenId::Popup => "",
        }
    }

    /// Returns the screen title for `screen`.
    pub fn screen_title(&self, screen: ScreenId) -> String {
        self.screen_title_str(screen).to_string()
    }

    /// Returns the rows for any row-based screen (`Main`, `Singleplayer`,
    /// `Options`, `Profile`, `Multiplayer`, `MultiplayerList`, `InstantActionEra`, `HostDialog`).
    ///
    /// Mirrors the `rowScreen` definitions in `src/shell/screens.js`.
    pub fn rows_for(&self, screen: ScreenId) -> Vec<MenuRow> {
        match screen {
            // `main()` in `src/shell/screens.js`: 7 top-level rows.
            ScreenId::Main => vec![
                MenuRow::enabled("sp", "SINGLEPLAYER"),
                MenuRow::enabled("mp", "MULTIPLAYER"),
                MenuRow::enabled("options", "OPTIONS"),
                MenuRow::enabled("profile", "PROFILE MANAGEMENT"),
                MenuRow::enabled("export-content", "EXPORT CONTENT"),
                MenuRow::enabled("import-content", "IMPORT CONTENT"),
                MenuRow::enabled("quit", "EXIT TO WINDOWS"),
            ],
            // `singleplayer()` in `src/shell/screens.js`: `HISTORICAL CAMPAIGN`
            // and `GALACTIC CONQUEST` are disabled; `INSTANT ACTION` is enabled.
            ScreenId::Singleplayer => vec![
                MenuRow::disabled("campaign", "HISTORICAL CAMPAIGN"),
                MenuRow::disabled("conquest", "GALACTIC CONQUEST"),
                MenuRow::enabled("instant", "INSTANT ACTION"),
            ],
            // `optionsMenu` in `src/menus/options.js`: sub-page links and `CREDITS`.
            ScreenId::Options => vec![
                MenuRow::enabled("game", "GAME OPTIONS"),
                MenuRow::enabled("video", "VIDEO OPTIONS"),
                MenuRow::enabled("audio", "AUDIO OPTIONS"),
                MenuRow::enabled("controls", "CONTROL OPTIONS"),
                MenuRow::enabled("online", "ONLINE OPTIONS"),
                MenuRow::enabled("credits", "CREDITS"),
            ],
            ScreenId::OptionsPage(page) => self.option_rows(page),
            // `profile()` in `src/shell/screens.js`: current player name
            // (disabled header row), `CREATE PROFILE`, and `DELETE PROFILE`.
            ScreenId::Profile => vec![
                MenuRow::disabled("player", self.profile.name.clone()),
                MenuRow::enabled("new", "CREATE PROFILE"),
                MenuRow::enabled("delete", "DELETE PROFILE"),
            ],
            // Classic `multiplayer()` in `src/shell/screens.js`: `HOST`, `LAN`, `ONLINE`.
            ScreenId::Multiplayer => vec![
                MenuRow::enabled("host", "HOST"),
                MenuRow::enabled("lan", "LAN"),
                MenuRow::enabled("online", "ONLINE"),
            ],
            ScreenId::MultiplayerList(kind) => {
                // Mirrors `network(kind)` in `src/shell/screens.js`: privacy notice
                // gate for `ONLINE`, error + `RETRY` on failure, or server rows
                // followed by `REFRESH` (and `DIRECT CONNECTION` on `LAN`).
                let mut rows = Vec::new();
                if self.mp_needs_notice && kind != MultiplayerKind::Lan {
                    rows.push(MenuRow::enabled("notice", "ACCEPT ONLINE PRIVACY NOTICE"));
                    return rows;
                }
                if let Some(err) = &self.mp_status {
                    rows.push(MenuRow::disabled("error", err.clone()));
                    rows.push(MenuRow::enabled("refresh", "RETRY"));
                    return rows;
                }
                // Format each discovered or master-listed server as
                // `"<name>  <players>/<maxPlayers>"` (`network(kind)` in `screens.js`).
                for srv in &self.mp_servers {
                    rows.push(MenuRow::enabled(
                        srv.id.clone(),
                        format!("{}  {}/{}", srv.name, srv.players, srv.max_players),
                    ));
                }
                // When no servers are returned, show a disabled placeholder row
                // before `REFRESH` (and `DIRECT CONNECTION` on `LAN`).
                if rows.is_empty() {
                    rows.push(MenuRow::disabled("empty", "NO SERVERS AVAILABLE"));
                }
                rows.push(MenuRow::enabled("refresh", "REFRESH"));
                if kind == MultiplayerKind::Lan {
                    rows.push(MenuRow::enabled("direct", "DIRECT CONNECTION"));
                }
                rows
            }
            ScreenId::InstantActionEra => {
                // Era picker rows (`"cw"` -> `"CLONE WARS"`, `"gcw"` -> `"GALACTIC CIVIL WAR"`).
                if let Some((_, _, eras, _)) = &self.ia_era_choices {
                    eras.iter()
                        .map(|era| {
                            let label = if era == "cw" {
                                "CLONE WARS"
                            } else {
                                "GALACTIC CIVIL WAR"
                            };
                            MenuRow::enabled(era.clone(), label)
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            }
            ScreenId::HostDialog => self.host_option_rows(),
            _ => Vec::new(),
        }
    }

    /// Builds the `SET GAME OPTIONS` rows for `ScreenId::HostDialog`, mirroring
    /// `hostDialog` (`SET GAME OPTIONS`) in `src/shell/host-dialog.js` and
    /// `bfe_desktop::host::HostOptions`.
    fn host_option_rows(&self) -> Vec<MenuRow> {
        let cfg = &self.host_config;
        let map_label = self
            .data
            .maps
            .iter()
            .find(|m| m.id == cfg.map)
            .map(|m| m.name.as_str())
            .unwrap_or(cfg.map.as_str())
            .to_ascii_uppercase();
        let era_label = if cfg.era == "gcw" {
            "GALACTIC CIVIL WAR"
        } else {
            "CLONE WARS"
        };
        let side_label = match cfg.side.as_str() {
            "cis" => "CIS",
            "all" => "REBEL ALLIANCE",
            "imp" => "GALACTIC EMPIRE",
            _ => "REPUBLIC",
        };
        let mode_label = if cfg.lan_only {
            "LAN ONLY"
        } else {
            "INTERNET + LAN"
        };
        let relay_label = match cfg.relay_policy.as_str() {
            "always" => "ALWAYS RELAY",
            "direct" => "DIRECT ONLY",
            _ => "AUTO",
        };
        let timer_label = if cfg.victory_timer_sec == 0 {
            "UNTIMED".to_string()
        } else {
            format!("{} MIN", cfg.victory_timer_sec / 60)
        };
        vec![
            MenuRow::enabled("launch", "LAUNCH HOSTED SERVER"),
            MenuRow::enabled("map", format!("MISSION MAP: {map_label} ({era_label})")),
            MenuRow::enabled("side", format!("STARTING FACTION: {side_label}")),
            MenuRow::enabled("mode", format!("VISIBILITY: {mode_label}")),
            MenuRow::enabled("maxPlayers", format!("MAX PLAYERS: {}", cfg.max_players)),
            MenuRow::enabled("relay", format!("RELAY POLICY: {relay_label}")),
            MenuRow::enabled("aiUnits", format!("AI UNITS PER TEAM: {}", cfg.bots)),
            MenuRow::enabled(
                "aiDifficulty",
                format!("AI DIFFICULTY: {}", cfg.ai_difficulty.to_ascii_uppercase()),
            ),
            MenuRow::enabled(
                "reinforcements",
                format!("REINFORCEMENTS: {}%", cfg.reinforcements_pct),
            ),
            MenuRow::enabled("victoryTimer", format!("VICTORY TIMER: {timer_label}")),
            MenuRow::enabled(
                "friendlyFire",
                format!(
                    "FRIENDLY FIRE: {}",
                    if cfg.friendly_fire { "ON" } else { "OFF" }
                ),
            ),
            MenuRow::enabled(
                "heroes",
                format!(
                    "AI HEROES: {}",
                    if cfg.heroes_enabled { "ON" } else { "OFF" }
                ),
            ),
            MenuRow::enabled(
                "teamAssignment",
                format!(
                    "TEAM ASSIGNMENT: {}",
                    if cfg.auto_assign_teams {
                        "AUTO-ASSIGN"
                    } else {
                        "PLAYER CHOOSES"
                    }
                ),
            ),
            MenuRow::enabled(
                "autoSpawn",
                format!("AUTO-SPAWN: {}", if cfg.auto_spawn { "ON" } else { "OFF" }),
            ),
            MenuRow::enabled("serverName", format!("SERVER NAME: {}", cfg.server_name)),
        ]
    }

    /// Steps the highlighted `SET GAME OPTIONS` row on `ScreenId::HostDialog`
    /// left (`delta = -1`) or right (`delta = 1`).
    pub(super) fn adjust_host_option(&mut self, delta: isize) {
        let rows = self.rows_for(ScreenId::HostDialog);
        let idx = self.selected_row_index(ScreenId::HostDialog);
        let Some(row) = rows.get(idx) else {
            return;
        };
        match row.id.as_str() {
            "map" => {
                let extracted: Vec<(GameFamily, String, Vec<String>)> = self
                    .data
                    .maps
                    .iter()
                    .filter(|m| m.extracted != Some(false))
                    .map(|m| {
                        let eras: Vec<String> = m
                            .playable_eras()
                            .into_iter()
                            .map(str::to_string)
                            .collect();
                        let eras = if eras.is_empty() {
                            vec!["cw".to_string()]
                        } else {
                            eras
                        };
                        (m.family, m.id.clone(), eras)
                    })
                    .collect();
                if !extracted.is_empty() {
                    let cur = extracted
                        .iter()
                        .position(|(_, id, _)| *id == self.host_config.map)
                        .unwrap_or(0) as isize;
                    let len = extracted.len() as isize;
                    let next = (((cur + delta) % len) + len) % len;
                    let (fam, id, eras) = &extracted[next as usize];
                    self.host_config.family = *fam;
                    self.host_config.map = id.clone();
                    if !eras.contains(&self.host_config.era) {
                        self.host_config.era = eras[0].clone();
                    }
                    self.host_config.side = if self.host_config.era == "gcw" {
                        "all".to_string()
                    } else {
                        "rep".to_string()
                    };
                    self.sound_queue.push("shell_select_change");
                }
            }
            "side" => {
                let sides = [
                    ("cw", "rep"),
                    ("cw", "cis"),
                    ("gcw", "all"),
                    ("gcw", "imp"),
                ];
                let cur = sides
                    .iter()
                    .position(|(_, s)| *s == self.host_config.side)
                    .unwrap_or(0) as isize;
                let len = sides.len() as isize;
                let next = (((cur + delta) % len) + len) % len;
                let (era, side) = sides[next as usize];
                self.host_config.era = era.to_string();
                self.host_config.side = side.to_string();
                self.sound_queue.push("shell_select_change");
            }
            "mode" => {
                self.host_config.lan_only = !self.host_config.lan_only;
                self.sound_queue.push("shell_select_change");
            }
            "maxPlayers" => {
                let presets = [4u32, 8, 16, 24, 32, 64];
                let cur = presets
                    .iter()
                    .position(|&p| p == self.host_config.max_players)
                    .unwrap_or(4) as isize;
                let len = presets.len() as isize;
                let next = (((cur + delta) % len) + len) % len;
                self.host_config.max_players = presets[next as usize];
                self.sound_queue.push("shell_select_change");
            }
            "relay" => {
                let policies = ["auto", "always", "direct"];
                let cur = policies
                    .iter()
                    .position(|&p| p == self.host_config.relay_policy)
                    .unwrap_or(0) as isize;
                let len = policies.len() as isize;
                let next = (((cur + delta) % len) + len) % len;
                self.host_config.relay_policy = policies[next as usize].to_string();
                self.sound_queue.push("shell_select_change");
            }
            "aiUnits" => {
                let presets = [0u32, 8, 16, 24, 32];
                let cur = presets
                    .iter()
                    .position(|&b| b == self.host_config.bots)
                    .unwrap_or(2) as isize;
                let len = presets.len() as isize;
                let next = (((cur + delta) % len) + len) % len;
                self.host_config.bots = presets[next as usize];
                self.sound_queue.push("shell_select_change");
            }
            "aiDifficulty" => {
                let diffs = ["easy", "medium", "hard"];
                let cur = diffs
                    .iter()
                    .position(|&d| d == self.host_config.ai_difficulty)
                    .unwrap_or(1) as isize;
                let len = diffs.len() as isize;
                let next = (((cur + delta) % len) + len) % len;
                self.host_config.ai_difficulty = diffs[next as usize].to_string();
                self.sound_queue.push("shell_select_change");
            }
            "reinforcements" => {
                let presets = [50u32, 100, 150, 200];
                let cur = presets
                    .iter()
                    .position(|&r| r == self.host_config.reinforcements_pct)
                    .unwrap_or(1) as isize;
                let len = presets.len() as isize;
                let next = (((cur + delta) % len) + len) % len;
                self.host_config.reinforcements_pct = presets[next as usize];
                self.sound_queue.push("shell_select_change");
            }
            "victoryTimer" => {
                let presets = [0u32, 300, 600, 900, 1200];
                let cur = presets
                    .iter()
                    .position(|&t| t == self.host_config.victory_timer_sec)
                    .unwrap_or(0) as isize;
                let len = presets.len() as isize;
                let next = (((cur + delta) % len) + len) % len;
                self.host_config.victory_timer_sec = presets[next as usize];
                self.sound_queue.push("shell_select_change");
            }
            "friendlyFire" => {
                self.host_config.friendly_fire = !self.host_config.friendly_fire;
                self.sound_queue.push("shell_select_change");
            }
            "heroes" => {
                self.host_config.heroes_enabled = !self.host_config.heroes_enabled;
                self.sound_queue.push("shell_select_change");
            }
            "teamAssignment" => {
                self.host_config.auto_assign_teams = !self.host_config.auto_assign_teams;
                self.sound_queue.push("shell_select_change");
            }
            "autoSpawn" => {
                self.host_config.auto_spawn = !self.host_config.auto_spawn;
                self.sound_queue.push("shell_select_change");
            }
            _ => {}
        }
        self.rebuild_cached_rows();
    }

    /// Builds the `LABEL: VALUE` rows and `RESET TO DEFAULTS` row for an
    /// [`OptionsPageId`] sub-page.
    fn option_rows(&self, page: OptionsPageId) -> Vec<MenuRow> {
        // Format each option sub-page's rows from `self.settings`.
        match page {
            OptionsPageId::Game => vec![
                MenuRow::enabled(
                    "menuTheme",
                    format!("MENU THEME: {}", self.settings.menu_theme.to_ascii_uppercase()),
                ),
                MenuRow::enabled(
                    "battleSize",
                    format!(
                        "BATTLE SIZE: {}",
                        self.settings.battle_size.to_ascii_uppercase()
                    ),
                ),
                MenuRow::enabled(
                    "difficulty",
                    format!(
                        "DIFFICULTY: {}",
                        self.settings.difficulty.to_ascii_uppercase()
                    ),
                ),
                MenuRow::enabled("reset", "RESET TO DEFAULTS"),
            ],
            // Video quality preset rows (`src/menus/options.js`).
            OptionsPageId::Video => vec![
                MenuRow::enabled("resolution", "RESOLUTION: NATIVE"),
                MenuRow::enabled("antialiasing", "ANTIALIASING: HIGH"),
                MenuRow::enabled("shadows", "SHADOWS: HIGH"),
                MenuRow::enabled("water", "WATER QUALITY: ULTRA"),
                MenuRow::enabled("reset", "RESET TO DEFAULTS"),
            ],
            // Audio volume rows (`0%..=100%`) for master, music, effects, and interface.
            OptionsPageId::Audio => vec![
                MenuRow::enabled(
                    "masterVolume",
                    format!(
                        "MASTER VOLUME: {}%",
                        (self.settings.master_volume * 100.0).round() as u32
                    ),
                ),
                MenuRow::enabled(
                    "musicVolume",
                    format!(
                        "MUSIC VOLUME: {}%",
                        (self.settings.music_volume * 100.0).round() as u32
                    ),
                ),
                MenuRow::enabled(
                    "effectsVolume",
                    format!(
                        "EFFECTS VOLUME: {}%",
                        (self.settings.effects_volume * 100.0).round() as u32
                    ),
                ),
                MenuRow::enabled(
                    "interfaceVolume",
                    format!(
                        "INTERFACE VOLUME: {}%",
                        (self.settings.interface_volume * 100.0).round() as u32
                    ),
                ),
                MenuRow::enabled("reset", "RESET TO DEFAULTS"),
            ],
            // Mouse sensitivity (`10%..=100%`) and Y-axis inversion toggle.
            OptionsPageId::Controls => vec![
                MenuRow::enabled(
                    "sensitivity",
                    format!(
                        "MOUSE SENSITIVITY: {}%",
                        (self.settings.mouse_sensitivity * 100.0).round() as u32
                    ),
                ),
                MenuRow::enabled(
                    "invertY",
                    format!(
                        "INVERT Y AXIS: {}",
                        if self.settings.invert_y { "YES" } else { "NO" }
                    ),
                ),
                MenuRow::enabled("reset", "RESET TO DEFAULTS"),
            ],
        }
    }

    /// Returns the stable `row_cursor` map key for a [`ScreenId`].
    fn screen_key(screen: ScreenId) -> &'static str {
        match screen {
            ScreenId::Gate => "gate",
            ScreenId::Main => "main",
            ScreenId::Singleplayer => "sp",
            ScreenId::InstantAction => "ia",
            ScreenId::InstantActionEra => "ia-era",
            ScreenId::Options => "options",
            ScreenId::OptionsPage(OptionsPageId::Game) => "options-game",
            ScreenId::OptionsPage(OptionsPageId::Video) => "options-video",
            ScreenId::OptionsPage(OptionsPageId::Audio) => "options-audio",
            ScreenId::OptionsPage(OptionsPageId::Controls) => "options-controls",
            ScreenId::Profile => "profile",
            ScreenId::Multiplayer => "mp",
            ScreenId::MultiplayerList(MultiplayerKind::Lan) => "mp-lan",
            ScreenId::MultiplayerList(MultiplayerKind::Online) => "mp-online",
            ScreenId::DirectConnect => "direct",
            ScreenId::HostDialog => "host",
            ScreenId::Popup => "popup",
        }
    }

    /// Returns the currently highlighted row index for `screen`, ensuring it
    /// points at an enabled row when possible.
    pub fn selected_row_index(&self, screen: ScreenId) -> usize {
        // Fast path: reuse `cached_rows` when querying the active top-of-stack screen.
        if self.stack.last().copied() == Some(screen) && !self.cached_rows.is_empty() {
            return self.selected_cached_row_index(screen);
        }
        let rows = self.rows_for(screen);
        if rows.is_empty() {
            return 0;
        }
        let key = Self::screen_key(screen);
        let raw = self.row_cursor.get(key).copied().unwrap_or(0).min(rows.len() - 1);
        if !rows[raw].disabled {
            return raw;
        }
        rows.iter().position(|r| !r.disabled).unwrap_or(raw)
    }

    /// Sets the highlighted row index for `screen`.
    pub fn set_selected_row_index(&mut self, screen: ScreenId, index: usize) {
        let rows = self.rows_for(screen);
        if index < rows.len() && !rows[index].disabled {
            let key = Self::screen_key(screen);
            // Queue `"shell_select_change"` whenever the highlighted row changes.
            let prev = self.row_cursor.insert(key, index);
            if prev != Some(index) {
                self.sound_queue.push("shell_select_change");
            }
        }
    }

    /// Pushes `screen` onto the navigation stack and updates [`ShellMedia`].
    pub fn push_screen(&mut self, screen: ScreenId) {
        let prev_media = self
            .stack
            .last()
            .map(|s| s.media_id())
            .unwrap_or("start");
        self.stack.push(screen);
        self.rebuild_cached_rows();
        // Initialize this screen's cursor to its first enabled row on first visit.
        let key = Self::screen_key(screen);
        if !self.row_cursor.contains_key(key) {
            let first_enabled = self
                .cached_rows
                .iter()
                .position(|r| !r.disabled)
                .unwrap_or(0);
            self.row_cursor.insert(key, first_enabled);
        }
        // Entering `INSTANT ACTION` ensures `ia_available_selected` points at an
        // extracted map and triggers `media.preview(map.id)` (`onEnter` in `screens.js`).
        if screen == ScreenId::InstantAction {
            if let Some(first_idx) = self
                .data
                .maps
                .iter()
                .position(|m| m.extracted != Some(false))
            {
                if self.ia_available_selected >= self.data.maps.len() {
                    self.ia_available_selected = first_idx;
                }
            }
            let map_id = self
                .data
                .maps
                .get(self.ia_available_selected)
                .map(|m| m.id.clone());
            let ev = self.media.preview(&self.data, map_id.as_deref());
            self.media_events.push(ev);
        }
        // Transition background movie and music when entering a screen with a
        // different `media_id` (or when first arriving on `Main` from `Gate`).
        let new_media = screen.media_id();
        if new_media != prev_media || self.stack.len() == 2 {
            let evs = self.media.enter_immediate(&self.data, new_media, false);
            self.media_events.extend(evs);
        }
    }

    /// Pops the current screen (or invokes `Main`'s `backAction` which pushes
    /// `Profile`, matching `screen.backAction = push(profile)` in `src/shell/screens.js`).
    pub fn back(&mut self) {
        // Dismissing a modal popup (`Notice`, `ConfirmExit`, or `HostClosePrompt`)
        // plays `"shell_menu_cancel"` without popping the underlying screen.
        if let Some(popup) = self.popup.take() {
            self.sound_queue.push("shell_menu_cancel");
            if matches!(popup, PopupState::HostClosePrompt { .. }) {
                self.pending_action = Some(ShellAction::HostCloseDecision(2));
            }
            return;
        }
        // Cancelling the profile callsign editor discards any pending
        // multiplayer host/join action (`requireProfile` returning `false`).
        if self.profile.editing {
            self.profile.cancel();
            self.pending_mp_after_profile = None;
            self.sound_queue.push("shell_menu_cancel");
            return;
        }
        match self.current_screen() {
            ScreenId::Gate => {}
            ScreenId::Main => {
                // In `src/shell/screens.js`: `screen.backAction = push(profile)`.
                self.sound_queue.push("shell_menu_enter");
                self.push_screen(ScreenId::Profile);
            }
            _ => {
                // Pop the current screen and notify `ShellMedia` with `back = true`
                // so returning to a screen triggers its camera return move.
                if self.stack.len() > 1 {
                    self.stack.pop();
                    self.rebuild_cached_rows();
                    self.sound_queue.push("shell_menu_cancel");
                    let top = self.current_screen();
                    let evs = self.media.enter_immediate(&self.data, top.media_id(), true);
                    self.media_events.extend(evs);
                }
            }
        }
    }

    /// Unlocks the initial click gate (`ScreenId::Gate`) and transitions into
    /// `ScreenId::Main`.
    pub fn unlock_gate(&mut self) {
        if self.current_screen() == ScreenId::Gate {
            self.stack.clear();
            self.stack.push(ScreenId::Main);
            self.rebuild_cached_rows();
            let first_enabled = self
                .cached_rows
                .iter()
                .position(|r| !r.disabled)
                .unwrap_or(0);
            self.row_cursor
                .insert(Self::screen_key(ScreenId::Main), first_enabled);
            self.sound_queue.push("shell_menu_enter");
            let evs = self.media.enter_immediate(&self.data, "main", false);
            self.media_events.extend(evs);
        }
    }

    /// Moves the selection cursor up (`delta = -1`) or down (`delta = 1`) on
    /// the active screen.
    pub fn move_vertical(&mut self, delta: isize) {
        if let Some(PopupState::HostClosePrompt { selected, .. }) = &mut self.popup {
            let next = (((*selected as isize + delta) % 3) + 3) % 3;
            *selected = next as usize;
            self.sound_queue.push("shell_select_change");
            return;
        }
        if self.popup.is_some() || self.profile.editing {
            return;
        }
        let screen = self.current_screen();
        // In `INSTANT ACTION`, Up/Down scrolls whichever listbox (`Available`
        // or `Selected`) is focused, or returns focus to `Available`.
        if screen == ScreenId::InstantAction {
            match self.ia_focus {
                IaFocus::Available => {
                    if !self.data.maps.is_empty() {
                        let len = self.data.maps.len() as isize;
                        let next = (self.ia_available_selected as isize + delta).clamp(0, len - 1)
                            as usize;
                        // Updating the `AVAILABLE MAPS` selection plays
                        // `"shell_select_change"` and updates the lower-right map preview.
                        if next != self.ia_available_selected {
                            self.ia_available_selected = next;
                            self.sound_queue.push("shell_select_change");
                            let map_id = self.data.maps.get(next).map(|m| m.id.clone());
                            let ev = self.media.preview(&self.data, map_id.as_deref());
                            self.media_events.push(ev);
                        }
                    }
                }
                IaFocus::Selected => {
                    if !self.ia_entries.is_empty() {
                        let len = self.ia_entries.len() as isize;
                        let next =
                            (self.ia_selected_cursor as isize + delta).clamp(0, len - 1) as usize;
                        if next != self.ia_selected_cursor {
                            self.ia_selected_cursor = next;
                            self.sound_queue.push("shell_select_change");
                        }
                    }
                }
                _ => {
                    self.ia_focus = IaFocus::Available;
                }
            }
            return;
        }

        // Wrap-around vertical navigation across enabled rows on row-based screens.
        let rows = self.rows_for(screen);
        let enabled_indices: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.disabled)
            .map(|(i, _)| i)
            .collect();
        if enabled_indices.is_empty() {
            return;
        }
        let current = self.selected_row_index(screen);
        let pos = enabled_indices
            .iter()
            .position(|&i| i == current)
            .unwrap_or(0);
        let len = enabled_indices.len() as isize;
        let next_pos = (((pos as isize + delta) % len) + len) % len;
        let next_idx = enabled_indices[next_pos as usize];
        self.set_selected_row_index(screen, next_idx);
    }

    /// Moves focus horizontally (`delta = -1` or `1`) on screens with multiple
    /// columns or buttons (`InstantAction`, `ConfirmExit` popup, `HostClosePrompt`,
    /// `OptionsPage` sliders, `HostDialog` options).
    pub fn move_horizontal(&mut self, delta: isize) {
        // Left/Right on `EXIT TO WINDOWS?` toggles focus between `YES` and `NO`.
        if let Some(PopupState::ConfirmExit { selected_yes, .. } | PopupState::OnlineNotice {selected_accept:selected_yes,..}) = &mut self.popup {
            *selected_yes = !*selected_yes;
            self.sound_queue.push("shell_select_change");
            return;
        }
        // Left/Right on `HostClosePrompt` cycles across the 3 buttons (`0..3`).
        if let Some(PopupState::HostClosePrompt { selected, .. }) = &mut self.popup {
            let next = (((*selected as isize + delta) % 3) + 3) % 3;
            *selected = next as usize;
            self.sound_queue.push("shell_select_change");
            return;
        }
        let screen = self.current_screen();
        // Left/Right on an options sub-page steps the selected slider or toggle.
        if let ScreenId::OptionsPage(page) = screen {
            self.adjust_option(page, delta);
            return;
        }
        // Left/Right on `SET GAME OPTIONS` (`ScreenId::HostDialog`) steps the selected host option.
        if screen == ScreenId::HostDialog {
            self.adjust_host_option(delta);
            return;
        }
        // Left/Right in `INSTANT ACTION` cycles focus across the two listboxes,
        // `ADD`/`REMOVE`, and the three bottom buttons.
        if screen == ScreenId::InstantAction {
            let order = [
                IaFocus::Available,
                IaFocus::Add,
                IaFocus::Remove,
                IaFocus::Selected,
                IaFocus::Back,
                IaFocus::Order,
                IaFocus::Launch,
            ];
            let pos = order
                .iter()
                .position(|&f| f == self.ia_focus)
                .unwrap_or(0) as isize;
            let len = order.len() as isize;
            let next = (((pos + delta) % len) + len) % len;
            self.ia_focus = order[next as usize];
            self.sound_queue.push("shell_select_change");
        }
    }

    /// Steps the currently selected option on `page` left (`delta = -1`) or
    /// right (`delta = 1`), clamping volumes to `0.0..=1.0`, cycling
    /// `menu_theme` across `["eternal", "bf1", "bf2"]`, and queuing
    /// `"shell_select_change"`.
    pub(super) fn adjust_option(&mut self, page: OptionsPageId, delta: isize) {
        let rows = self.rows_for(ScreenId::OptionsPage(page));
        let idx = self.selected_row_index(ScreenId::OptionsPage(page));
        let Some(row) = rows.get(idx) else {
            return;
        };
        let step = (delta as f32) * 0.1;
        match (page, row.id.as_str()) {
            (OptionsPageId::Audio, "masterVolume") => {
                self.settings.master_volume = (self.settings.master_volume + step).clamp(0.0, 1.0);
                self.sound_queue.push("shell_select_change");
            }
            (OptionsPageId::Audio, "musicVolume") => {
                self.settings.music_volume = (self.settings.music_volume + step).clamp(0.0, 1.0);
                self.sound_queue.push("shell_select_change");
            }
            (OptionsPageId::Audio, "effectsVolume") => {
                self.settings.effects_volume =
                    (self.settings.effects_volume + step).clamp(0.0, 1.0);
                self.sound_queue.push("shell_select_change");
            }
            (OptionsPageId::Audio, "interfaceVolume") => {
                self.settings.interface_volume =
                    (self.settings.interface_volume + step).clamp(0.0, 1.0);
                self.sound_queue.push("shell_select_change");
            }
            // Cycle the front-end look across `"eternal"`, `"bf1"`, and `"bf2"`.
            (OptionsPageId::Game, "menuTheme") => {
                let themes = ["eternal", "bf1", "bf2"];
                let cur = themes
                    .iter()
                    .position(|&t| t == self.settings.menu_theme)
                    .unwrap_or(0) as isize;
                let next = (((cur + delta) % 3) + 3) % 3;
                self.settings.menu_theme = themes[next as usize].to_string();
                self.sound_queue.push("shell_select_change");
            }
            (OptionsPageId::Controls, "sensitivity") => {
                self.settings.mouse_sensitivity =
                    (self.settings.mouse_sensitivity + step).clamp(0.1, 1.0);
                self.sound_queue.push("shell_select_change");
            }
            (OptionsPageId::Controls, "invertY") => {
                self.settings.invert_y = !self.settings.invert_y;
                self.sound_queue.push("shell_select_change");
            }
            _ => {}
        }
        self.rebuild_cached_rows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_online::browser::{RotationItem, ServerBrowser};
    use serde_json::json;

    // Builds minimal `ShellData` with one single-era map (`geo1`: `cw`) and
    // one dual-era map (`bes2`: `cw` + `gcw`) for navigation tests.
    fn test_shell_data() -> ShellData {
        let shell = json!({
            "maps": [
                {
                    "id": "geo1",
                    "name": "GEONOSIS: SPIRE",
                    "extracted": true,
                    "eras": { "cw": { "teams": ["rep", "cis"] } }
                },
                {
                    "id": "bes2",
                    "name": "BESPIN: CLOUD CITY",
                    "extracted": true,
                    "eras": {
                        "cw": { "teams": ["rep", "cis"] },
                        "gcw": { "teams": ["all", "imp"] }
                    }
                }
            ]
        });
        crate::data::read_shell_data(Some(&shell), None, None)
    }

    // Verifies that Instant Action's `AVAILABLE MAPS` game picker defaults to
    // `BATTLEFRONT (2004)` (`Swbf1`) and lists `BATTLEFRONT II (2005), PLANNED`
    // (`Swbf2`) as disabled (`gamePicker()` in `src/shell/index.js`).
    #[test]
    fn game_picker_lists_swbf1_selected_and_swbf2_planned_disabled() {
        let mut app = ShellApp::new(test_shell_data(), None);
        assert_eq!(app.ia_family, GameFamily::Swbf1);
        assert_eq!(app.ia_selected_game_label(), "BATTLEFRONT (2004)");
        assert_eq!(
            app.ia_game_picker_options(),
            &[
                (GameFamily::Swbf1, "BATTLEFRONT (2004)", true),
                (GameFamily::Swbf2, "BATTLEFRONT II (2005), PLANNED", false),
            ]
        );
        // Selecting Swbf2 is rejected because it is planned/disabled.
        assert!(!app.ia_select_family(GameFamily::Swbf2));
        assert_eq!(app.ia_family, GameFamily::Swbf1);
    }

    // Verifies the end-to-end singleplayer flow: unlocking `Gate`, opening
    // `Singleplayer` -> `InstantAction`, adding a single-era map (`geo1:cw`)
    // and a dual-era map (`bes2:gcw` via `InstantActionEra`), and launching.
    #[test]
    fn gate_unlocks_to_main_menu_and_navigates_to_instant_action_and_launches()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut browser = ServerBrowser::default();
        let mut app = ShellApp::new(test_shell_data(), None);
        assert_eq!(app.current_screen(), ScreenId::Gate);

        // Modifier keys don't unlock the gate; Enter unlocks to Main.
        assert!(!app.handle_key("F12", false, false, false, &mut browser));
        assert_eq!(app.current_screen(), ScreenId::Gate);
        assert!(app.handle_key("Enter", false, false, false, &mut browser));
        assert_eq!(app.current_screen(), ScreenId::Main);

        // Enter on SINGLEPLAYER (row 0) opens Singleplayer, where row 2 (INSTANT ACTION) is the first enabled row.
        app.accept(&mut browser);
        assert_eq!(app.current_screen(), ScreenId::Singleplayer);
        assert_eq!(app.selected_row_index(ScreenId::Singleplayer), 2);

        // Enter on INSTANT ACTION opens InstantAction.
        app.accept(&mut browser);
        assert_eq!(app.current_screen(), ScreenId::InstantAction);

        // Map 0 (geo1) has only 1 era ("cw"), so Enter adds geo1:cw directly.
        app.accept(&mut browser);
        assert_eq!(
            app.ia_entries,
            vec![PlaylistEntry::new("geo1", Some("cw"))]
        );

        // Move down to Map 1 (bes2, which has both "cw" and "gcw") and press Enter -> pushes InstantActionEra picker.
        app.move_vertical(1);
        app.accept(&mut browser);
        assert_eq!(app.current_screen(), ScreenId::InstantActionEra);
        // Select row 1 ("gcw" = GALACTIC CIVIL WAR) and accept.
        app.move_vertical(1);
        app.accept(&mut browser);
        assert_eq!(app.current_screen(), ScreenId::InstantAction);
        assert_eq!(
            app.ia_entries,
            vec![
                PlaylistEntry::new("geo1", Some("cw")),
                PlaylistEntry::new("bes2", Some("gcw")),
            ]
        );

        // Focus Launch and press Enter -> emits ShellAction::LaunchBattle.
        app.ia_focus = IaFocus::Launch;
        app.accept(&mut browser);
        let action = app.take_action();
        match action {
            Some(ShellAction::LaunchBattle(req)) => {
                assert_eq!(req.family, GameFamily::Swbf1);
                assert_eq!(req.map, "geo1");
                assert_eq!(req.era.as_deref(), Some("cw"));
                assert_eq!(req.mode, "conquest");
                assert_eq!(req.playlist.ok_or("missing playlist")?.entries.len(), 2);
            }
            other => return Err(format!("expected LaunchBattle, got {other:?}").into()),
        }
        Ok(())
    }

    // Verifies that `ONLINE` requires accepting the privacy notice when
    // `ServerBrowser` is in `BrowserState::Notice`, and that joining a server
    // while still named `PLAYER 1` prompts for a custom profile callsign first.
    #[test]
    fn multiplayer_requires_profile_and_online_requires_privacy_notice()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut browser = ServerBrowser::default();
        let token = browser.begin_refresh(false).ok_or("missing refresh token")?;
        browser.finish_refresh_notice(token);

        let mut app = ShellApp::new(test_shell_data(), None);
        app.unlock_gate();

        // Navigate to MULTIPLAYER (row 1).
        app.move_vertical(1);
        app.accept(&mut browser);
        assert_eq!(app.current_screen(), ScreenId::Multiplayer);

        // Select ONLINE (row 2).
        app.move_vertical(2);
        app.accept(&mut browser);
        assert_eq!(
            app.current_screen(),
            ScreenId::MultiplayerList(MultiplayerKind::Online)
        );
        // While browser is in BrowserState::Notice, the only row is "ACCEPT ONLINE PRIVACY NOTICE".
        let rows = app.rows_for(app.current_screen());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "notice");

        // Transition the browser to Ready with one server and accept the notice row.
        let tok2 = browser.begin_refresh(false).ok_or("missing refresh token")?;
        browser.finish_refresh_ok(
            tok2,
            vec![ServerEntry {
                id: "srv-1".to_string(),
                name: "CLOUD CITY 24/7".to_string(),
                players: 12,
                max_players: 32,
                rotation: vec![RotationItem::Slug("swbf1/bes2:imp:conquest".to_string())],
                relay_policy: Some("always".to_string()),
                latency_ms: Some(25),
                ..ServerEntry::default()
            }],
            1000,
        );
        app.accept(&mut browser);
        let rows = app.rows_for(app.current_screen());
        assert_eq!(rows[0].id, "srv-1");

        // With DEFAULT_PLAYER_NAME ("PLAYER 1"), pressing Enter on srv-1 opens the profile prompt first.
        app.accept(&mut browser);
        assert!(app.profile.editing);
        assert!(app.take_action().is_none());

        // Type "JOSH" and press Enter -> completes profile creation and launches the server join!
        for ch in ['J', 'O', 'S', 'H'] {
            app.handle_key(&ch.to_string(), false, false, false, &mut browser);
        }
        app.handle_key("Enter", false, false, false, &mut browser);
        assert!(!app.profile.editing);
        assert_eq!(app.profile.name, "JOSH");
        match app.take_action() {
            Some(ShellAction::LaunchMultiplayer(mp)) => {
                assert_eq!(mp.kind, MultiplayerKind::Online);
                assert_eq!(mp.server_id, "srv-1");
                assert_eq!(mp.family, GameFamily::Swbf1);
                assert_eq!(mp.map, "bes2");
                assert_eq!(mp.side, "imp");
                assert_eq!(mp.era, "gcw");
                assert_eq!(mp.mode, "conquest");
                assert!(mp.relay_always);
                assert_eq!(mp.player_name, "JOSH");
            }
            other => return Err(format!("expected LaunchMultiplayer, got {other:?}").into()),
        }
        Ok(())
    }

    // Verifies the `MULTIPLAYER -> HOST -> SET GAME OPTIONS` flow (`ScreenId::HostDialog`).
    #[test]
    fn multiplayer_host_opens_set_game_options_and_launches()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut browser = ServerBrowser::default();
        let mut app = ShellApp::new(test_shell_data(), None);
        app.profile.name = "JOSH".to_string();
        app.unlock_gate();

        // Navigate to MULTIPLAYER (row 1) -> HOST (row 0).
        app.move_vertical(1);
        app.accept(&mut browser);
        assert_eq!(app.current_screen(), ScreenId::Multiplayer);
        app.accept(&mut browser);
        assert_eq!(app.current_screen(), ScreenId::HostDialog);
        assert_eq!(app.screen_title_str(ScreenId::HostDialog), "SET GAME OPTIONS");

        // Move down to MISSION MAP (row 1), step right to `bes2`, then return to LAUNCH (row 0).
        app.move_vertical(1);
        app.move_horizontal(1);
        assert_eq!(app.host_config.map, "bes2");
        app.move_vertical(-1);
        app.accept(&mut browser);
        match app.take_action() {
            Some(ShellAction::LaunchMultiplayer(mp)) => {
                assert_eq!(mp.server_id, "local-host");
                assert_eq!(mp.map, "bes2");
                assert_eq!(mp.player_name, "JOSH");
            }
            other => return Err(format!("expected LaunchMultiplayer, got {other:?}").into()),
        }
        Ok(())
    }
}
