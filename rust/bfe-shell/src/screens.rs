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

use crate::{
    data::{ShellData, ShellMap},
    launch::{BattleRequest, GameFamily, PlaylistEntry, DEFAULT_MODE, MAX_PLAYLIST},
    media::{MediaEvent, ShellMedia},
    profile::{clean_player_name, has_profile, ProfileState, DEFAULT_PLAYER_NAME},
    version::{version_line, AboutInfo},
};
use bfe_online::{
    browser::{
        BattleSummary, BrowserState, RotationItem, ServerBrowser, ServerBrowserService,
        ServerEntry, Source,
    },
    direct::{parse_direct_target, DEFAULT_DIRECT_PORT},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Game picker options in Instant Action's `AVAILABLE MAPS` header:
/// `BATTLEFRONT (2004)` is selected (`swbf1`), and `BATTLEFRONT II (2005), PLANNED`
/// is listed as disabled (`swbf2`).
///
/// Mirrors `gamePicker()` in `src/shell/index.js`.
pub const GAME_PICKER_OPTIONS: [(GameFamily, &str, bool); 2] = [
    (GameFamily::Swbf1, "BATTLEFRONT (2004)", true),
    (GameFamily::Swbf2, "BATTLEFRONT II (2005), PLANNED", false),
];

/// Source kind for a multiplayer session (`"LAN"` or `"ONLINE"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiplayerKind {
    Lan,
    Online,
}

impl MultiplayerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lan => "LAN",
            Self::Online => "ONLINE",
        }
    }

    pub fn to_source(self) -> Source {
        match self {
            Self::Lan => Source::Lan,
            Self::Online => Source::Internet,
        }
    }
}

/// Host game configuration submitted from the `HOST` screen/dialog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostConfig {
    pub server_name: String,
    pub family: GameFamily,
    pub map: String,
    pub era: String,
    pub mode: String,
    pub max_players: u32,
    pub bots: u32,
    pub lan_only: bool,
}

/// Resolved multiplayer join or host target launched by the shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiplayerLaunch {
    pub kind: MultiplayerKind,
    pub server_id: String,
    pub family: GameFamily,
    pub map: String,
    pub side: String,
    pub era: String,
    pub mode: String,
    pub address: Option<String>,
    pub port: Option<u16>,
    pub relay_always: bool,
    pub player_name: String,
}

/// Resolves a server's battle target (`family`, `map`, `side`, `era`, `mode`,
/// `address`, `port`, `relay_always`) from its [`ServerEntry`] `battle` summary
/// or `<family>/<map>:<side>:<mode>` rotation entry.
///
/// Mirrors `joinServer(server, kind)` in `src/shell/screens.js` and the
/// `<family>/<map>:<side>:<mode>` rotation format in `MULTIPLAYER.md`
/// (lines 106-183).
pub fn resolve_server_join(
    server: &ServerEntry,
    kind: MultiplayerKind,
    player_name: &str,
) -> MultiplayerLaunch {
    let (rot_family, rot_map, rot_side, rot_mode) = server
        .rotation
        .first()
        .and_then(parse_rotation_mission)
        .unwrap_or((GameFamily::Swbf1, "nab2", "rep", DEFAULT_MODE));

    let battle = server.battle.as_ref();
    let map = battle
        .map(|b| b.map.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(rot_map)
        .to_string();
    let side = battle
        .map(|b| b.side.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(rot_side)
        .to_string();
    let era = battle
        .map(|b| b.era.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if matches!(side.as_str(), "rep" | "cis" | "cw") {
                "cw".to_string()
            } else {
                "gcw".to_string()
            }
        });
    let mode = battle
        .map(|b| b.mode.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(rot_mode)
        .to_string();

    let (address, port) = if kind == MultiplayerKind::Lan {
        (
            server.address.clone(),
            Some(server.port.unwrap_or(DEFAULT_DIRECT_PORT)),
        )
    } else {
        (None, None)
    };
    let relay_always =
        kind != MultiplayerKind::Lan && server.relay_policy.as_deref() == Some("always");

    MultiplayerLaunch {
        kind,
        server_id: server.id.clone(),
        family: rot_family,
        map,
        side,
        era,
        mode,
        address,
        port,
        relay_always,
        player_name: clean_player_name(player_name),
    }
}

fn parse_rotation_mission(item: &RotationItem) -> Option<(GameFamily, &str, &str, &str)> {
    match item {
        RotationItem::Slug(slug) => {
            let (family, rest) = match slug.split_once('/') {
                Some((fam, tail)) => (GameFamily::parse(fam)?, tail),
                None => (GameFamily::Swbf1, slug.as_str()),
            };
            let mut parts = rest.split(':');
            let map = parts.next().filter(|s| !s.is_empty())?;
            let side = parts.next().filter(|s| !s.is_empty())?;
            let mode = parts
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(DEFAULT_MODE);
            Some((family, map, side, mode))
        }
        RotationItem::Entry { map, era, mode } => {
            if map.is_empty() {
                return None;
            }
            let side = if era == "gcw" { "imp" } else { "rep" };
            let mode_str = if mode.is_empty() {
                DEFAULT_MODE
            } else {
                mode.as_str()
            };
            Some((GameFamily::Swbf1, map.as_str(), side, mode_str))
        }
    }
}

/// Builds a [`ServerEntry`] for a direct connection target (`"host:port"`),
/// validating the address with [`bfe_online::direct::parse_direct_target`].
pub fn direct_connect_entry(address: &str) -> Result<ServerEntry, String> {
    let trimmed = address.trim();
    let default_port = DEFAULT_DIRECT_PORT.to_string();
    let (host_str, port_str) = if let Some(rest) = trimmed.strip_prefix('[') {
        match rest.split_once(']') {
            Some((h, after)) => {
                let p = after.strip_prefix(':').unwrap_or(default_port.as_str());
                (h, p)
            }
            None => (trimmed, default_port.as_str()),
        }
    } else {
        match trimmed.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') => (h, p),
            _ => (trimmed, default_port.as_str()),
        }
    };
    let target = parse_direct_target(host_str, port_str).map_err(|e| e.to_string())?;
    Ok(ServerEntry {
        id: format!("direct-{}:{}", target.address, target.port),
        name: target.host_port_string(),
        address: Some(target.address),
        port: Some(target.port),
        players: 0,
        max_players: 32,
        rotation: vec![RotationItem::Slug("swbf1/nab2:rep:conquest".to_string())],
        ..ServerEntry::default()
    })
}

/// Builds a local hosted [`ServerEntry`] from a [`HostConfig`].
pub fn hosted_server_entry(config: &HostConfig) -> ServerEntry {
    let side = if config.era == "cw" { "rep" } else { "all" };
    let mode = if config.mode.is_empty() {
        DEFAULT_MODE
    } else {
        config.mode.as_str()
    };
    ServerEntry {
        id: "local-host".to_string(),
        name: config.server_name.clone(),
        address: Some("127.0.0.1".to_string()),
        port: Some(DEFAULT_DIRECT_PORT),
        players: 1,
        max_players: config.max_players,
        battle: Some(BattleSummary {
            map: config.map.clone(),
            side: side.to_string(),
            era: config.era.clone(),
            mode: mode.to_string(),
            bots: Some(config.bots > 0),
            ..BattleSummary::default()
        }),
        rotation: vec![RotationItem::Slug(format!(
            "{}/{}:{side}:{mode}",
            config.family.as_str(),
            config.map
        ))],
        latency_ms: Some(1),
        ..ServerEntry::default()
    }
}

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
    /// Host server dialog modal (`HOST`).
    HostDialog,
    /// Modal notice popup (`OK`) or exit confirmation (`YES` / `NO`).
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
    Game,
    Video,
    Audio,
    Controls,
}

impl OptionsPageId {
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
    pub id: String,
    pub label: String,
    pub disabled: bool,
}

impl MenuRow {
    pub fn enabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
        }
    }

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
    #[default]
    Available,
    Selected,
    Add,
    Remove,
    Back,
    Order,
    Launch,
}

/// Modal popup kind (`Ok` notice or `ConfirmExit` `YES`/`NO`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopupState {
    Notice {
        message: String,
    },
    ConfirmExit {
        message: String,
        selected_yes: bool,
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
}

/// Option value stored in [`ShellSettings`] for the `OPTIONS` pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptionItem {
    pub key: String,
    pub label: String,
    pub value_text: String,
}

/// Player settings for volumes, theme, and options pages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellSettings {
    pub master_volume: f32,
    pub music_volume: f32,
    pub interface_volume: f32,
    pub effects_volume: f32,
    pub menu_theme: String,
    pub battle_size: String,
    pub difficulty: String,
    pub mouse_sensitivity: f32,
    pub invert_y: bool,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            master_volume: 1.0,
            music_volume: 1.0,
            interface_volume: 1.0,
            effects_volume: 1.0,
            menu_theme: "eternal".to_string(),
            battle_size: "normal".to_string(),
            difficulty: "medium".to_string(),
            mouse_sensitivity: 0.5,
            invert_y: false,
        }
    }
}

/// Complete state of the front-end shell (`createScreens` in `src/shell/screens.js`).
pub struct ShellApp {
    pub data: ShellData,
    pub media: ShellMedia,
    pub settings: ShellSettings,
    pub profile: ProfileState,
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
    /// Pending multiplayer action after the profile prompt finishes.
    pending_mp_after_profile: Option<PendingMpAction>,
    /// Active modal popup (`Notice` or `ConfirmExit`), if any.
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

#[derive(Clone, Debug)]
enum PendingMpAction {
    Host,
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
            direct_connect_draft: format!("127.0.0.1:{DEFAULT_DIRECT_PORT}"),
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
    /// `ExitToWindows`).
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
            ScreenId::HostDialog => "HOST GAME",
            ScreenId::Popup => "",
        }
    }

    /// Returns the screen title for `screen`.
    pub fn screen_title(&self, screen: ScreenId) -> String {
        self.screen_title_str(screen).to_string()
    }

    /// Returns the rows for any row-based screen (`Main`, `Singleplayer`,
    /// `Options`, `Profile`, `Multiplayer`, `MultiplayerList`, `InstantActionEra`).
    ///
    /// Mirrors the `rowScreen` definitions in `src/shell/screens.js`.
    pub fn rows_for(&self, screen: ScreenId) -> Vec<MenuRow> {
        match screen {
            ScreenId::Main => vec![
                MenuRow::enabled("sp", "SINGLEPLAYER"),
                MenuRow::enabled("mp", "MULTIPLAYER"),
                MenuRow::enabled("options", "OPTIONS"),
                MenuRow::enabled("profile", "PROFILE MANAGEMENT"),
                MenuRow::enabled("export-content", "EXPORT CONTENT"),
                MenuRow::enabled("import-content", "IMPORT CONTENT"),
                MenuRow::enabled("quit", "EXIT TO WINDOWS"),
            ],
            ScreenId::Singleplayer => vec![
                MenuRow::disabled("campaign", "HISTORICAL CAMPAIGN"),
                MenuRow::disabled("conquest", "GALACTIC CONQUEST"),
                MenuRow::enabled("instant", "INSTANT ACTION"),
            ],
            ScreenId::Options => vec![
                MenuRow::enabled("game", "GAME OPTIONS"),
                MenuRow::enabled("video", "VIDEO OPTIONS"),
                MenuRow::enabled("audio", "AUDIO OPTIONS"),
                MenuRow::enabled("controls", "CONTROL OPTIONS"),
                MenuRow::enabled("online", "ONLINE OPTIONS"),
                MenuRow::enabled("credits", "CREDITS"),
            ],
            ScreenId::OptionsPage(page) => self.option_rows(page),
            ScreenId::Profile => vec![
                MenuRow::disabled("player", self.profile.name.clone()),
                MenuRow::enabled("new", "CREATE PROFILE"),
                MenuRow::enabled("delete", "DELETE PROFILE"),
            ],
            ScreenId::Multiplayer => vec![
                MenuRow::enabled("host", "HOST"),
                MenuRow::enabled("lan", "LAN"),
                MenuRow::enabled("online", "ONLINE"),
            ],
            ScreenId::MultiplayerList(kind) => {
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
                for srv in &self.mp_servers {
                    rows.push(MenuRow::enabled(
                        srv.id.clone(),
                        format!("{}  {}/{}", srv.name, srv.players, srv.max_players),
                    ));
                }
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
            _ => Vec::new(),
        }
    }

    fn option_rows(&self, page: OptionsPageId) -> Vec<MenuRow> {
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
            OptionsPageId::Video => vec![
                MenuRow::enabled("resolution", "RESOLUTION: NATIVE"),
                MenuRow::enabled("antialiasing", "ANTIALIASING: HIGH"),
                MenuRow::enabled("shadows", "SHADOWS: HIGH"),
                MenuRow::enabled("water", "WATER QUALITY: ULTRA"),
                MenuRow::enabled("reset", "RESET TO DEFAULTS"),
            ],
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
        let key = Self::screen_key(screen);
        if !self.row_cursor.contains_key(key) {
            let first_enabled = self
                .cached_rows
                .iter()
                .position(|r| !r.disabled)
                .unwrap_or(0);
            self.row_cursor.insert(key, first_enabled);
        }
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
        let new_media = screen.media_id();
        if new_media != prev_media || self.stack.len() == 2 {
            let evs = self.media.enter_immediate(&self.data, new_media, false);
            self.media_events.extend(evs);
        }
    }

    /// Pops the current screen (or invokes `Main`'s `backAction` which pushes
    /// `Profile`, matching `screen.backAction = push(profile)` in `src/shell/screens.js`).
    pub fn back(&mut self) {
        if self.popup.is_some() {
            self.popup = None;
            self.sound_queue.push("shell_menu_cancel");
            return;
        }
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
        if self.popup.is_some() || self.profile.editing {
            return;
        }
        let screen = self.current_screen();
        if screen == ScreenId::InstantAction {
            match self.ia_focus {
                IaFocus::Available => {
                    if !self.data.maps.is_empty() {
                        let len = self.data.maps.len() as isize;
                        let next = (self.ia_available_selected as isize + delta).clamp(0, len - 1)
                            as usize;
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
    /// columns or buttons (`InstantAction`, `ConfirmExit` popup, `OptionsPage` sliders).
    pub fn move_horizontal(&mut self, delta: isize) {
        if let Some(PopupState::ConfirmExit { selected_yes, .. }) = &mut self.popup {
            *selected_yes = !*selected_yes;
            self.sound_queue.push("shell_select_change");
            return;
        }
        let screen = self.current_screen();
        if let ScreenId::OptionsPage(page) = screen {
            self.adjust_option(page, delta);
            return;
        }
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

    fn adjust_option(&mut self, page: OptionsPageId, delta: isize) {
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

    /// Adds the currently highlighted map in `AVAILABLE MAPS` to `SELECTED MAPS`
    /// (or pushes the era picker popup when the map has both `cw` and `gcw`).
    ///
    /// Mirrors `add(screen)` in `instantAction()` in `src/shell/screens.js`.
    pub fn ia_add(&mut self) {
        let Some(map) = self.data.maps.get(self.ia_available_selected).cloned() else {
            self.sound_queue.push("shell_menu_error");
            return;
        };
        let choices = Self::map_choices(&map);
        if map.extracted == Some(false) || choices.is_empty() {
            self.sound_queue.push("shell_menu_error");
            return;
        }
        if self.ia_entries.len() >= MAX_PLAYLIST {
            self.sound_queue.push("shell_menu_error");
            self.popup = Some(PopupState::Notice {
                message: "SELECTED MAPLIST IS FULL.".to_string(),
            });
            return;
        }
        if choices.len() == 1 {
            self.ia_append_with_family(map.family, &map.id, &choices[0]);
            self.sound_queue.push("shell_menu_accept");
        } else {
            self.ia_era_choices = Some((map.id.clone(), map.name.clone(), choices, 0));
            self.sound_queue.push("shell_menu_enter");
            self.push_screen(ScreenId::InstantActionEra);
        }
    }

    fn map_choices(map: &ShellMap) -> Vec<String> {
        let playable = map.playable_eras();
        if !playable.is_empty() {
            return playable.into_iter().map(str::to_string).collect();
        }
        map.eras
            .keys()
            .filter(|e| *e == "cw" || *e == "gcw")
            .cloned()
            .collect()
    }

    fn ia_append_with_family(&mut self, family: GameFamily, map_id: &str, era: &str) {
        if self.ia_entries.len() < MAX_PLAYLIST {
            self.ia_entries.push(PlaylistEntry::with_family_and_mode(
                family,
                map_id,
                Some(era),
                DEFAULT_MODE,
            ));
            self.ia_selected_cursor = self.ia_entries.len() - 1;
        }
    }

    /// Removes the highlighted entry in `SELECTED MAPS`.
    /// Mirrors `remove()` in `instantAction()` in `src/shell/screens.js`.
    pub fn ia_remove(&mut self) {
        if self.ia_entries.is_empty() {
            return;
        }
        let idx = self.ia_selected_cursor.min(self.ia_entries.len() - 1);
        self.ia_entries.remove(idx);
        self.ia_selected_cursor = if self.ia_entries.is_empty() {
            0
        } else {
            self.ia_selected_cursor.min(self.ia_entries.len() - 1)
        };
        self.sound_queue.push("shell_menu_accept");
    }

    /// Toggles Instant Action's `PLAY IN ORDER` / `RANDOM ORDER` button.
    pub fn ia_toggle_order(&mut self) {
        self.ia_random = !self.ia_random;
        self.sound_queue.push("shell_menu_accept");
    }

    /// Launches the current Instant Action playlist if non-empty, setting
    /// `self.pending_action = Some(ShellAction::LaunchBattle(req))`.
    pub fn ia_launch<F>(&mut self, rand: F) -> Option<BattleRequest>
    where
        F: FnMut() -> f64,
    {
        if self.ia_entries.is_empty() {
            self.sound_queue.push("shell_menu_error");
            return None;
        }
        let req = BattleRequest::launch(&self.ia_entries, self.ia_random, rand).ok()?;
        self.sound_queue.push("shell_menu_accept");
        self.pending_action = Some(ShellAction::LaunchBattle(req.clone()));
        Some(req)
    }

    /// Synchronizes `mp_servers`, `mp_status`, and `mp_needs_notice` from a
    /// [`ServerBrowserService`] implementation (`bfe_online::browser::ServerBrowser`).
    pub fn sync_from_browser(&mut self, browser: &dyn ServerBrowserService) {
        match browser.state() {
            BrowserState::Notice => {
                self.mp_needs_notice = true;
                self.mp_status = None;
                self.mp_servers.clear();
            }
            BrowserState::Error(msg) => {
                self.mp_needs_notice = false;
                self.mp_status = Some(msg.clone());
                self.mp_servers.clear();
            }
            BrowserState::Loading | BrowserState::Idle | BrowserState::Ready => {
                self.mp_needs_notice = false;
                self.mp_status = None;
                self.mp_servers = browser.rows().into_iter().cloned().collect();
            }
        }
        self.rebuild_cached_rows();
    }

    /// Activates the currently highlighted item on the active screen (`Enter` or
    /// primary click), driving `browser` (`ServerBrowserService`) for multiplayer
    /// screens.
    pub fn accept(&mut self, browser: &mut dyn ServerBrowserService) {
        if let Some(popup) = self.popup.clone() {
            match popup {
                PopupState::Notice { .. } => {
                    self.popup = None;
                    self.sound_queue.push("shell_menu_accept");
                }
                PopupState::ConfirmExit { selected_yes, .. } => {
                    self.popup = None;
                    if selected_yes {
                        self.sound_queue.push("shell_menu_exit");
                        self.pending_action = Some(ShellAction::ExitToWindows);
                    } else {
                        self.sound_queue.push("shell_menu_cancel");
                    }
                }
            }
            return;
        }

        if self.profile.editing {
            self.profile.submit();
            self.rebuild_cached_rows();
            self.sound_queue.push("shell_menu_accept");
            if let Some(pending) = self.pending_mp_after_profile.take() {
                if self.profile.has_profile() {
                    match pending {
                        PendingMpAction::Host => self.execute_host(),
                        PendingMpAction::Join(server, kind) => {
                            let launch = resolve_server_join(&server, kind, &self.profile.name);
                            self.pending_action = Some(ShellAction::LaunchMultiplayer(launch));
                        }
                    }
                }
            }
            return;
        }

        let screen = self.current_screen();
        match screen {
            ScreenId::Gate => {
                self.unlock_gate();
            }
            ScreenId::Main => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                let Some(row) = rows.get(idx) else { return };
                match row.id.as_str() {
                    "sp" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::Singleplayer);
                    }
                    "mp" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::Multiplayer);
                    }
                    "options" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::Options);
                    }
                    "profile" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::Profile);
                    }
                    "export-content" | "import-content" => {
                        self.sound_queue.push("shell_menu_accept");
                        self.popup = Some(PopupState::Notice {
                            message: "CONTENT TRANSFER REQUIRES THE DESKTOP CLIENT.".to_string(),
                        });
                    }
                    "quit" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.popup = Some(PopupState::ConfirmExit {
                            message: "EXIT TO WINDOWS?".to_string(),
                            selected_yes: true,
                        });
                    }
                    _ => {}
                }
            }
            ScreenId::Singleplayer => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                if let Some(row) = rows.get(idx) {
                    if row.id == "instant" && !row.disabled {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::InstantAction);
                    }
                }
            }
            ScreenId::InstantAction => match self.ia_focus {
                IaFocus::Available | IaFocus::Add => self.ia_add(),
                IaFocus::Selected | IaFocus::Remove => self.ia_remove(),
                IaFocus::Back => self.back(),
                IaFocus::Order => self.ia_toggle_order(),
                IaFocus::Launch => {
                    let mut seed = 0x1234_5678u32;
                    self.ia_launch(|| {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        (seed as f64) / ((1u64 << 32) as f64)
                    });
                }
            },
            ScreenId::InstantActionEra => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                if let (Some((map_id, _, _, _)), Some(row)) =
                    (self.ia_era_choices.clone(), rows.get(idx))
                {
                    let family = self
                        .data
                        .maps
                        .iter()
                        .find(|m| m.id == map_id)
                        .map(|m| m.family)
                        .unwrap_or(self.ia_family);
                    self.ia_append_with_family(family, &map_id, &row.id);
                    self.ia_era_choices = None;
                    self.stack.pop();
                    self.rebuild_cached_rows();
                    self.sound_queue.push("shell_menu_accept");
                }
            }
            ScreenId::Options => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                let Some(row) = rows.get(idx) else { return };
                match row.id.as_str() {
                    "game" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::OptionsPage(OptionsPageId::Game));
                    }
                    "video" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::OptionsPage(OptionsPageId::Video));
                    }
                    "audio" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::OptionsPage(OptionsPageId::Audio));
                    }
                    "controls" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::OptionsPage(OptionsPageId::Controls));
                    }
                    "online" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.open_multiplayer_list(MultiplayerKind::Online, browser);
                    }
                    "credits" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.popup = Some(PopupState::Notice {
                            message: "BATTLEFRONT ETERNAL\nA fan-made mod for the Steam versions of\nStar Wars: Battlefront (2004) and Battlefront II (2005).\nBattlefront II support is planned.".to_string(),
                        });
                    }
                    _ => {}
                }
            }
            ScreenId::OptionsPage(page) => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                if let Some(row) = rows.get(idx) {
                    if row.id == "reset" {
                        self.settings = ShellSettings::default();
                        self.rebuild_cached_rows();
                        self.sound_queue.push("shell_menu_accept");
                    } else {
                        self.adjust_option(page, 1);
                    }
                }
            }
            ScreenId::Profile => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                let Some(row) = rows.get(idx) else { return };
                match row.id.as_str() {
                    "new" => {
                        self.profile.begin_edit("");
                        self.sound_queue.push("shell_menu_enter");
                    }
                    "delete" => {
                        self.profile.delete_profile();
                        self.rebuild_cached_rows();
                        self.sound_queue.push("shell_menu_accept");
                    }
                    _ => {}
                }
            }
            ScreenId::Multiplayer => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                let Some(row) = rows.get(idx) else { return };
                match row.id.as_str() {
                    "host" => {
                        if !self.require_profile_for(PendingMpAction::Host) {
                            return;
                        }
                        self.execute_host();
                    }
                    "lan" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.open_multiplayer_list(MultiplayerKind::Lan, browser);
                    }
                    "online" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.open_multiplayer_list(MultiplayerKind::Online, browser);
                    }
                    _ => {}
                }
            }
            ScreenId::MultiplayerList(kind) => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                let Some(row) = rows.get(idx) else { return };
                match row.id.as_str() {
                    "notice" => {
                        self.mp_needs_notice = false;
                        self.sync_from_browser(browser);
                        if self.mp_needs_notice {
                            // When the caller uses a `ServerBrowser` still in `Notice`
                            // state, clear `mp_needs_notice` and populate visible rows.
                            self.mp_needs_notice = false;
                            self.mp_servers = browser.rows().into_iter().cloned().collect();
                            self.rebuild_cached_rows();
                        }
                        self.sound_queue.push("shell_menu_accept");
                    }
                    "refresh" => {
                        self.refresh_multiplayer_list(kind, browser);
                        self.sound_queue.push("shell_menu_accept");
                    }
                    "direct" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::DirectConnect);
                    }
                    server_id => {
                        if let Some(server) =
                            self.mp_servers.iter().find(|s| s.id == server_id).cloned()
                        {
                            browser.select(Some(&server.id));
                            if !self
                                .require_profile_for(PendingMpAction::Join(Box::new(server.clone()), kind))
                            {
                                return;
                            }
                            let launch = resolve_server_join(&server, kind, &self.profile.name);
                            self.sound_queue.push("shell_menu_accept");
                            self.pending_action = Some(ShellAction::LaunchMultiplayer(launch));
                        }
                    }
                }
            }
            ScreenId::DirectConnect => {
                let addr = self.direct_connect_draft.clone();
                match direct_connect_entry(&addr) {
                    Ok(server) => {
                        self.stack.pop();
                        self.rebuild_cached_rows();
                        if !self.require_profile_for(PendingMpAction::Join(
                            Box::new(server.clone()),
                            MultiplayerKind::Lan,
                        )) {
                            return;
                        }
                        let launch =
                            resolve_server_join(&server, MultiplayerKind::Lan, &self.profile.name);
                        self.sound_queue.push("shell_menu_accept");
                        self.pending_action = Some(ShellAction::LaunchMultiplayer(launch));
                    }
                    Err(msg) => {
                        self.sound_queue.push("shell_menu_error");
                        self.popup = Some(PopupState::Notice { message: msg });
                    }
                }
            }
            ScreenId::HostDialog => {
                self.execute_host();
            }
            ScreenId::Popup => {}
        }
    }

    fn require_profile_for(&mut self, action: PendingMpAction) -> bool {
        if has_profile(&self.profile.name) {
            return true;
        }
        self.pending_mp_after_profile = Some(action);
        self.profile.begin_edit(
            "Create a profile before playing multiplayer. Other players will see this name.",
        );
        self.sound_queue.push("shell_menu_enter");
        false
    }

    fn execute_host(&mut self) {
        let first_map = self
            .data
            .maps
            .iter()
            .find(|m| m.extracted != Some(false))
            .map(|m| (m.family, m.id.clone()))
            .unwrap_or_else(|| (GameFamily::Swbf1, "nab2".to_string()));
        let cfg = HostConfig {
            server_name: format!("{}'S GAME", self.profile.name),
            family: first_map.0,
            map: first_map.1,
            era: "cw".to_string(),
            mode: DEFAULT_MODE.to_string(),
            max_players: 32,
            bots: 16,
            lan_only: false,
        };
        let server = hosted_server_entry(&cfg);
        let launch = resolve_server_join(&server, MultiplayerKind::Lan, &self.profile.name);
        self.sound_queue.push("shell_menu_accept");
        self.pending_action = Some(ShellAction::LaunchMultiplayer(launch));
    }

    fn open_multiplayer_list(
        &mut self,
        kind: MultiplayerKind,
        browser: &mut dyn ServerBrowserService,
    ) {
        self.refresh_multiplayer_list(kind, browser);
        self.push_screen(ScreenId::MultiplayerList(kind));
    }

    /// Synchronizes the multiplayer server list for `kind` from `browser`.
    pub fn refresh_multiplayer_list(
        &mut self,
        kind: MultiplayerKind,
        browser: &mut dyn ServerBrowserService,
    ) {
        let target_source = kind.to_source();
        if browser.source() != target_source {
            browser.set_source(target_source);
        }
        self.sync_from_browser(browser);
    }

    /// Handles a keyboard key event (`"Enter"`, `"Escape"`, `"ArrowUp"`,
    /// `"ArrowDown"`, `"ArrowLeft"`, `"ArrowRight"`, `"Delete"`, `"Backspace"`,
    /// or printable characters when editing a profile or direct-connect address).
    pub fn handle_key(
        &mut self,
        key: &str,
        ctrl: bool,
        meta: bool,
        alt: bool,
        browser: &mut dyn ServerBrowserService,
    ) -> bool {
        if self.current_screen() == ScreenId::Gate {
            if crate::gate::counts_as_press("keydown", key, ctrl, meta, alt) {
                self.unlock_gate();
                return true;
            }
            return false;
        }

        if self.profile.editing {
            match key {
                "Enter" => {
                    self.accept(browser);
                    return true;
                }
                "Escape" => {
                    self.back();
                    return true;
                }
                "Backspace" => {
                    self.profile.backspace();
                    self.sound_queue.push("shell_keyboard_backspace");
                    return true;
                }
                _ => {
                    if key.chars().count() == 1 && !ctrl && !meta && !alt {
                        if let Some(ch) = key.chars().next() {
                            self.profile.push_char(ch);
                            self.sound_queue.push("shell_keyboard_key");
                            return true;
                        }
                    }
                    return false;
                }
            }
        }

        if self.current_screen() == ScreenId::DirectConnect {
            match key {
                "Enter" => {
                    self.accept(browser);
                    return true;
                }
                "Escape" => {
                    self.back();
                    return true;
                }
                "Backspace" => {
                    self.direct_connect_draft.pop();
                    self.sound_queue.push("shell_keyboard_backspace");
                    return true;
                }
                _ => {
                    if key.chars().count() == 1 && !ctrl && !meta && !alt {
                        if let Some(ch) = key.chars().next() {
                            self.direct_connect_draft.push(ch);
                            self.sound_queue.push("shell_keyboard_key");
                            return true;
                        }
                    }
                    return false;
                }
            }
        }

        match key {
            "ArrowUp" | "w" | "W" => {
                self.move_vertical(-1);
                true
            }
            "ArrowDown" | "s" | "S" => {
                self.move_vertical(1);
                true
            }
            "ArrowLeft" | "a" | "A" => {
                self.move_horizontal(-1);
                true
            }
            "ArrowRight" | "d" | "D" => {
                self.move_horizontal(1);
                true
            }
            "Enter" | " " => {
                self.accept(browser);
                true
            }
            "Escape" | "Backspace" => {
                self.back();
                true
            }
            "Delete" if self.current_screen() == ScreenId::InstantAction => {
                self.ia_remove();
                true
            }
            _ => false,
        }
    }

    /// Resets the player profile to `DEFAULT_PLAYER_NAME` (`"PLAYER 1"`).
    pub fn reset_profile(&mut self) {
        self.profile.name = DEFAULT_PLAYER_NAME.to_string();
        self.rebuild_cached_rows();
    }
}

/// Convenience helper returning a default [`ServerBrowser`] for callers that do
/// not need custom contract settings.
pub fn default_server_browser() -> ServerBrowser {
    ServerBrowser::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    #[test]
    fn gate_unlocks_to_main_menu_and_navigates_to_instant_action_and_launches() {
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
                assert_eq!(req.playlist.unwrap().entries.len(), 2);
            }
            other => panic!("expected LaunchBattle, got {other:?}"),
        }
    }

    #[test]
    fn multiplayer_requires_profile_and_online_requires_privacy_notice() {
        let mut browser = ServerBrowser::default();
        let token = browser.begin_refresh(false).unwrap();
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
        let tok2 = browser.begin_refresh(false).unwrap();
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
            other => panic!("expected LaunchMultiplayer, got {other:?}"),
        }
    }
}
