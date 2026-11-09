//! Instant Action playlist actions, multiplayer server browser synchronization,
//! join/host target resolution, and keyboard dispatch for [`ShellApp`]
//! (`src/shell/screens.js`, `src/shell/index.js`).
//!
//! Mirrors:
//! - `gamePicker()` in `src/shell/index.js` (`GAME_PICKER_OPTIONS`: `swbf1`
//!   `BATTLEFRONT (2004)` enabled, `swbf2` `BATTLEFRONT II (2005), PLANNED` disabled),
//! - `joinServer(server, kind)`, `requireProfile()`, `network(kind)`, `host()`,
//!   and `instantAction()` (`add`, `remove`, `order`, `launch`) in
//!   `src/shell/screens.js`,
//! - `bindMenuInput` in `src/menus/input.js` (`ArrowUp`/`ArrowDown`/`w`/`s`,
//!   `ArrowLeft`/`ArrowRight`/`a`/`d`, `Enter`/`Space`, `Escape`/`Backspace`,
//!   and `Delete` on `InstantAction`).

use super::{
    IaFocus, OptionsPageId, PendingMpAction, PopupState, ScreenId, ShellAction, ShellApp,
    ShellSettings,
};
use crate::{
    data::ShellMap,
    launch::{BattleRequest, GameFamily, PlaylistEntry, DEFAULT_MODE, MAX_PLAYLIST},
    profile::{clean_player_name, has_profile, DEFAULT_PLAYER_NAME},
};
use bfe_online::{
    browser::{
        BattleSummary, BrowserState, RotationItem, ServerBrowser, ServerBrowserService,
        ServerEntry, Source,
    },
    direct::{parse_direct_target, DEFAULT_DIRECT_PORT},
};
use serde::{Deserialize, Serialize};

/// Instant Action `AVAILABLE MAPS` game picker options: the 2004 game (`swbf1`)
/// is enabled and selected by default, while the 2005 game (`swbf2`) is listed
/// as planned and disabled.
///
/// Mirrors `gamePicker()` in `src/shell/index.js`.
pub const GAME_PICKER_OPTIONS: [(GameFamily, &str, bool); 2] = [
    (GameFamily::Swbf1, "BATTLEFRONT (2004)", true),
    (GameFamily::Swbf2, "BATTLEFRONT II (2005), PLANNED", false),
];

/// Source kind for a multiplayer session (`"LAN"` or `"ONLINE"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MultiplayerKind {
    /// Local network server discovery and direct IP/host connection (`"LAN"`).
    Lan,
    /// Master-server internet listing (`"ONLINE"`).
    Online,
}

impl MultiplayerKind {
    /// Returns the uppercase screen title (`"LAN"` or `"ONLINE"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lan => "LAN",
            Self::Online => "ONLINE",
        }
    }

    /// Converts this [`MultiplayerKind`] into the corresponding
    /// [`bfe_online::browser::Source`] (`Source::Lan` or `Source::Internet`).
    pub fn to_source(self) -> Source {
        match self {
            Self::Lan => Source::Lan,
            Self::Online => Source::Internet,
        }
    }
}

/// Host game configuration (`SET GAME OPTIONS`) submitted from the `HOST` screen
/// (`ScreenId::HostDialog`) and mapped to `bfe_desktop::host::HostOptions`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostConfig {
    /// Public server name shown in the LAN/online browser.
    pub server_name: String,
    /// Game family (`GameFamily::Swbf1` or `GameFamily::Swbf2`).
    pub family: GameFamily,
    /// Mission map identifier (e.g. `"nab2"`, `"geo1"`).
    pub map: String,
    /// Starting faction side (`"rep"`, `"cis"`, `"all"`, or `"imp"`).
    pub side: String,
    /// Era code (`"cw"` or `"gcw"`).
    pub era: String,
    /// Game mode identifier (default `"conquest"`).
    pub mode: String,
    /// Maximum player slots (`1..=64`, default `32`).
    pub max_players: u32,
    /// AI bot count per team (`0..=32`, default `16`).
    pub bots: u32,
    /// Whether the server only listens on the local network (`true` = LAN only).
    pub lan_only: bool,
    /// Relay policy (`"auto"`, `"always"`, `"direct"`).
    pub relay_policy: String,
    /// AI difficulty (`"easy"`, `"medium"`, `"hard"`).
    pub ai_difficulty: String,
    /// Reinforcements percentage (`50`, `100`, `150`, `200`).
    pub reinforcements_pct: u32,
    /// Victory timer in seconds (`0` = untimed, `300`, `600`, `900`, `1200`).
    pub victory_timer_sec: u32,
    /// Friendly fire toggle (`false` by default).
    pub friendly_fire: bool,
    /// AI heroes toggle (`true` by default).
    pub heroes_enabled: bool,
    /// Auto-assign teams (`true` = auto-assign, `false` = player chooses).
    pub auto_assign_teams: bool,
    /// Auto-spawn toggle (`false` by default).
    pub auto_spawn: bool,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            server_name: "PLAYER 1'S GAME".to_string(),
            family: GameFamily::Swbf1,
            map: "nab2".to_string(),
            side: "rep".to_string(),
            era: "cw".to_string(),
            mode: DEFAULT_MODE.to_string(),
            max_players: 32,
            bots: 16,
            lan_only: false,
            relay_policy: "auto".to_string(),
            ai_difficulty: "medium".to_string(),
            reinforcements_pct: 100,
            victory_timer_sec: 0,
            friendly_fire: false,
            heroes_enabled: true,
            auto_assign_teams: false,
            auto_spawn: false,
        }
    }
}

/// Resolved multiplayer join or host target launched by the shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultiplayerLaunch {
    /// Whether the session is `LAN` (direct address/port) or `ONLINE`.
    pub kind: MultiplayerKind,
    /// Server identifier (`server.id`).
    pub server_id: String,
    /// Resolved game family (`swbf1` or `swbf2`).
    pub family: GameFamily,
    /// Resolved mission map id (`"nab2"` fallback).
    pub map: String,
    /// Resolved starting faction side (`"rep"`, `"cis"`, `"all"`, or `"imp"`).
    pub side: String,
    /// Resolved era (`"cw"` for `rep`/`cis`/`cw`, else `"gcw"`).
    pub era: String,
    /// Resolved game mode (`"conquest"` fallback).
    pub mode: String,
    /// Direct UDP/WebRTC address when `kind == MultiplayerKind::Lan`.
    pub address: Option<String>,
    /// Direct port (`18451` fallback) when `kind == MultiplayerKind::Lan`.
    pub port: Option<u16>,
    /// True when `kind != MultiplayerKind::Lan` and `server.relay_policy == Some("always")`.
    pub relay_always: bool,
    /// Cleaned player callsign (`clean_player_name(player_name)`).
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
    // Parse the fallback mission (`<family>/<map>:<side>:<mode>`) from the
    // server's first rotation item, defaulting to `swbf1/nab2:rep:conquest`.
    let (rot_family, rot_map, rot_side, rot_mode) = server
        .rotation
        .first()
        .and_then(parse_rotation_mission)
        .unwrap_or((GameFamily::Swbf1, "nab2", "rep", DEFAULT_MODE));

    // Prefer the server's live `battle` summary (`map`, `side`, `era`, `mode`)
    // when present, falling back to the parsed rotation mission (`joinServer`).
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
    // Derive `era` from `battle.era` when non-empty; otherwise infer `"cw"`
    // for `rep`/`cis`/`cw` sides and `"gcw"` for `imp`/`all`/`gcw` (`joinServer`).
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

    // `LAN` joins include the server's `address` and `port` (defaulting to
    // `18451`); `ONLINE` joins resolve via the signaling/master server id and
    // honor `relayPolicy === "always"` (`joinServer` in `src/shell/screens.js`).
    let (address, port) = if kind == MultiplayerKind::Lan {
        (
            server.address.clone(),
            Some(server.port.unwrap_or(DEFAULT_DIRECT_PORT)),
        )
    } else {
        (None, None)
    };
    // Only `ONLINE` sessions apply `relayPolicy === "always"` (`joinServer` in
    // `src/shell/screens.js`).
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

/// Parses a [`RotationItem`] (`"<family>/<map>:<side>:<mode>"` slug or
/// structured `{ map, era, mode }` entry) into `(family, map, side, mode)`.
fn parse_rotation_mission(item: &RotationItem) -> Option<(GameFamily, &str, &str, &str)> {
    match item {
        // String slug format `"<family>/<map>:<side>:<mode>"` (or `"<map>:<side>:<mode>"`
        // defaulting to `GameFamily::Swbf1`).
        RotationItem::Slug(slug) => {
            let (family, rest) = match slug.split_once('/') {
                Some((fam, tail)) => (GameFamily::parse(fam)?, tail),
                None => (GameFamily::Swbf1, slug.as_str()),
            };
            // Split `<map>:<side>[:<mode>]`, requiring non-empty `map` and `side`
            // and defaulting `mode` to `DEFAULT_MODE` (`"conquest"`).
            let mut parts = rest.split(':');
            let map = parts.next().filter(|s| !s.is_empty())?;
            let side = parts.next().filter(|s| !s.is_empty())?;
            let mode = parts
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or(DEFAULT_MODE);
            Some((family, map, side, mode))
        }
        // Structured `{ map, era, mode }` rotation object: maps `"gcw"` to
        // `"imp"` and `"cw"` to `"rep"`, defaulting empty `mode` to `DEFAULT_MODE`.
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
    // Split bracketed IPv6 (`[fe80::1]:18451`) or `host:port` into `(host, port)`,
    // defaulting to `DEFAULT_DIRECT_PORT` (`18451`) when no port is given.
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
    // Validate host and port through `bfe_online::direct::parse_direct_target`
    // and construct a synthetic `direct-<address>:<port>` `ServerEntry`.
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

/// Builds a local loopback (`127.0.0.1:18451`) [`ServerEntry`] from a
/// [`HostConfig`] so hosting uses the same `resolve_server_join` pipeline as
/// joining (`host()` in `src/shell/screens.js`).
pub fn hosted_server_entry(config: &HostConfig) -> ServerEntry {
    // Use the configured side when non-empty; otherwise default to Republic
    // (`"rep"`) in Clone Wars or Rebel Alliance (`"all"`) in Galactic Civil War.
    let side = if !config.side.is_empty() {
        config.side.as_str()
    } else if config.era == "cw" {
        "rep"
    } else {
        "all"
    };
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
        relay_policy: Some(config.relay_policy.clone()),
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

impl ShellApp {
    /// Adds the currently highlighted map in `AVAILABLE MAPS` to `SELECTED MAPS`
    /// (or pushes the era picker popup when the map has both `cw` and `gcw`).
    ///
    /// Mirrors `add(screen)` in `instantAction()` in `src/shell/screens.js`.
    pub fn ia_add(&mut self) {
        let Some(map) = self.data.maps.get(self.ia_available_selected).cloned() else {
            self.sound_queue.push("shell_menu_error");
            return;
        };
        // Reject unextracted maps or maps without any playable era (`cw`/`gcw`).
        let choices = Self::map_choices(&map);
        if map.extracted == Some(false) || choices.is_empty() {
            self.sound_queue.push("shell_menu_error");
            return;
        }
        // Enforce the 32-entry playlist cap (`MAX_PLAYLIST` in `src/launch.js`),
        // showing the `"SELECTED MAPLIST IS FULL."` notice popup when full.
        if self.ia_entries.len() >= MAX_PLAYLIST {
            self.sound_queue.push("shell_menu_error");
            self.popup = Some(PopupState::Notice {
                message: "SELECTED MAPLIST IS FULL.".to_string(),
            });
            return;
        }
        // Single-era maps append immediately; dual-era maps push the
        // `InstantActionEra` (`CLONE WARS` / `GALACTIC CIVIL WAR`) picker.
        if choices.len() == 1 {
            self.ia_append_with_family(map.family, &map.id, &choices[0]);
            self.sound_queue.push("shell_menu_accept");
        } else {
            self.ia_era_choices = Some((map.id.clone(), map.name.clone(), choices, 0));
            self.sound_queue.push("shell_menu_enter");
            self.push_screen(ScreenId::InstantActionEra);
        }
    }

    /// Returns the extracted eras (`"cw"` and/or `"gcw"`) available for `map`.
    /// Mirrors `eras(map)` in `instantAction()` in `src/shell/screens.js`.
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

    /// Appends `(family, map_id, era, DEFAULT_MODE)` to `self.ia_entries` if
    /// under [`MAX_PLAYLIST`] and moves `ia_selected_cursor` to the new entry.
    /// Mirrors `append(era)` in `instantAction()` in `src/shell/screens.js`.
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
        // Remove the entry at `ia_selected_cursor`, clamp the cursor into the
        // remaining range (`0..ia_entries.len()`), and play `"shell_menu_accept"`.
        let idx = self.ia_selected_cursor.min(self.ia_entries.len() - 1);
        self.ia_entries.remove(idx);
        self.ia_selected_cursor = if self.ia_entries.is_empty() {
            0
        } else {
            self.ia_selected_cursor.min(self.ia_entries.len() - 1)
        };
        self.sound_queue.push("shell_menu_accept");
    }

    /// Toggles Instant Action's `PLAY IN ORDER` (`ia_random = false`) /
    /// `RANDOM ORDER` (`ia_random = true`) button and plays `"shell_menu_accept"`.
    /// Mirrors the `"order"` button action in `instantAction()` (`src/shell/screens.js`).
    pub fn ia_toggle_order(&mut self) {
        self.ia_random = !self.ia_random;
        self.sound_queue.push("shell_menu_accept");
    }

    /// Launches the current Instant Action playlist if non-empty via
    /// [`BattleRequest::launch`], playing `"shell_menu_accept"` and storing
    /// `Some(ShellAction::LaunchBattle(req))` in `self.pending_action`.
    ///
    /// Mirrors the `"launch"` button action in `instantAction()` (`src/shell/screens.js`).
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
        // Map `BrowserState` (`Notice`, `Error`, `Loading`/`Idle`/`Ready`) into
        // `ShellApp`'s multiplayer list fields (`network(kind)` in `screens.js`).
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
        // 1. Active modal popup (`Notice` `OK`, `ConfirmExit` `YES`/`NO`, or
        //    `HostClosePrompt` 3-button decision).
        if let Some(popup) = self.popup.clone() {
            match popup {
                PopupState::Notice { .. } => {
                    self.popup = None;
                    self.sound_queue.push("shell_menu_accept");
                }
                // Confirming `YES` on `EXIT TO WINDOWS?` plays `"shell_menu_exit"`
                // and emits `ShellAction::ExitToWindows`; `NO` plays `"shell_menu_cancel"`.
                PopupState::ConfirmExit { selected_yes, .. } => {
                    self.popup = None;
                    if selected_yes {
                        self.sound_queue.push("shell_menu_exit");
                        self.pending_action = Some(ShellAction::ExitToWindows);
                    } else {
                        self.sound_queue.push("shell_menu_cancel");
                    }
                }
                // 3-button host server close prompt (`KeepServer` = 0, `StopServer` = 1, `Cancel` = 2).
                PopupState::HostClosePrompt { selected, .. } => {
                    self.popup = None;
                    if selected == 2 {
                        self.sound_queue.push("shell_menu_cancel");
                    } else {
                        self.sound_queue.push("shell_menu_exit");
                    }
                    self.pending_action = Some(ShellAction::HostCloseDecision(selected));
                }
            }
            return;
        }

        // 2. Profile callsign editor (`CREATE PROFILE` or multiplayer `requireProfile`
        //    gate): commits the cleaned name and resumes any pending `Host`/`Join`.
        if self.profile.editing {
            self.profile.submit();
            self.rebuild_cached_rows();
            self.sound_queue.push("shell_menu_accept");
            // If the profile dialog was opened by `require_profile_for` and the
            // submitted callsign is not `PLAYER 1`, resume the deferred `Host`/`Join`.
            if let Some(pending) = self.pending_mp_after_profile.take() {
                if self.profile.has_profile() {
                    match pending {
                        PendingMpAction::Host => {
                            self.open_host_dialog();
                        }
                        PendingMpAction::Join(server, kind) => {
                            let launch = resolve_server_join(&server, kind, &self.profile.name);
                            self.pending_action = Some(ShellAction::LaunchMultiplayer(launch));
                        }
                    }
                }
            }
            return;
        }

        // 3. Dispatch `Enter` / primary click for the topmost screen on `self.stack`.
        let screen = self.current_screen();
        match screen {
            // `Gate`: primary click or `Enter` unlocks into `MAIN MENU`.
            ScreenId::Gate => {
                self.unlock_gate();
            }
            // `MAIN MENU`: opens `Singleplayer`, `Multiplayer`, `Options`, or
            // `Profile`, shows the desktop notice for content transfer, or
            // opens the `EXIT TO WINDOWS?` confirmation popup.
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
                    // Content transfer requires the desktop bridge (`transferContent`
                    // in `src/shell/screens.js`).
                    "export-content" | "import-content" => {
                        self.sound_queue.push("shell_menu_accept");
                        self.popup = Some(PopupState::Notice {
                            message: "CONTENT TRANSFER REQUIRES THE DESKTOP CLIENT.".to_string(),
                        });
                    }
                    // `EXIT TO WINDOWS` opens the `YES`/`NO` confirmation dialog.
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
            // `SINGLEPLAYER`: opens `InstantAction` when `"instant"` is accepted
            // (`"campaign"` and `"conquest"` are disabled).
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
            // `INSTANT ACTION`: `Available`/`Add` adds a map, `Selected`/`Remove`
            // removes the highlighted entry, `Order` toggles random order, and
            // `Launch` starts the battle.
            ScreenId::InstantAction => match self.ia_focus {
                IaFocus::Available | IaFocus::Add => self.ia_add(),
                IaFocus::Selected | IaFocus::Remove => self.ia_remove(),
                IaFocus::Back => self.back(),
                IaFocus::Order => self.ia_toggle_order(),
                // Deterministic LCG step for keyboard-triggered `RANDOM ORDER`
                // launches when no external RNG closure is passed.
                IaFocus::Launch => {
                    let mut seed = 0x1234_5678u32;
                    self.ia_launch(|| {
                        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                        (seed as f64) / ((1u64 << 32) as f64)
                    });
                }
            },
            // `InstantActionEra`: appends the chosen era (`"cw"` or `"gcw"`) to
            // `ia_entries` and pops the era picker back to `InstantAction`.
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
            // `OPTIONS`: opens the selected options sub-page, `ONLINE` browser,
            // or the `CREDITS` notice popup.
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
                    // `CONTROL OPTIONS` opens the mouse sensitivity and invert-Y sub-page.
                    "controls" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::OptionsPage(OptionsPageId::Controls));
                    }
                    "online" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.open_multiplayer_list(MultiplayerKind::Online, browser);
                    }
                    // `CREDITS` opens a notice popup describing Battlefront Eternal.
                    "credits" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.popup = Some(PopupState::Notice {
                            message: "BATTLEFRONT ETERNAL\nA fan-made mod for the Steam versions of\nStar Wars: Battlefront (2004) and Battlefront II (2005).\nBattlefront II support is planned.".to_string(),
                        });
                    }
                    _ => {}
                }
            }
            // `OptionsPage`: `RESET TO DEFAULTS` restores `ShellSettings::default()`;
            // pressing `Enter` on any other option steps it forward by `+1`.
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
            // `PROFILE MANAGEMENT`: `CREATE PROFILE` opens the callsign editor;
            // `DELETE PROFILE` resets the callsign to `DEFAULT_PLAYER_NAME` (`"PLAYER 1"`).
            ScreenId::Profile => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                let Some(row) = rows.get(idx) else { return };
                match row.id.as_str() {
                    // `CREATE PROFILE` opens the callsign input modal.
                    "new" => {
                        self.profile.begin_edit("");
                        self.sound_queue.push("shell_menu_enter");
                    }
                    // `DELETE PROFILE` resets the player name to `DEFAULT_PLAYER_NAME`.
                    "delete" => {
                        self.profile.delete_profile();
                        self.rebuild_cached_rows();
                        self.sound_queue.push("shell_menu_accept");
                    }
                    _ => {}
                }
            }
            // `MULTIPLAYER`: `HOST` checks `require_profile_for` before opening
            // `SET GAME OPTIONS` (`ScreenId::HostDialog`); `LAN` and `ONLINE`
            // open the corresponding server list.
            ScreenId::Multiplayer => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                let Some(row) = rows.get(idx) else { return };
                match row.id.as_str() {
                    // Hosting requires a non-default profile (`hasProfile()`),
                    // then pushes `SET GAME OPTIONS` (`ScreenId::HostDialog`).
                    "host" => {
                        if !self.require_profile_for(PendingMpAction::Host) {
                            return;
                        }
                        self.open_host_dialog();
                    }
                    // `LAN` opens the local discovery and direct-connection list.
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
            // `MultiplayerList`: handles privacy notice acceptance (`"notice"`),
            // list refresh (`"refresh"`), opening `DIRECT CONNECTION` (`"direct"`),
            // or joining the selected server (gated by `require_profile_for`).
            ScreenId::MultiplayerList(kind) => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                let Some(row) = rows.get(idx) else { return };
                match row.id.as_str() {
                    // Accepting `"ACCEPT ONLINE PRIVACY NOTICE"` clears
                    // `mp_needs_notice` and populates the server rows.
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
                    // `REFRESH` / `RETRY` re-synchronizes the list from `browser`.
                    "refresh" => {
                        self.refresh_multiplayer_list(kind, browser);
                        self.sound_queue.push("shell_menu_accept");
                    }
                    // `DIRECT CONNECTION` opens the `host:port` input modal.
                    "direct" => {
                        self.sound_queue.push("shell_menu_enter");
                        self.push_screen(ScreenId::DirectConnect);
                    }
                    // Any other row id is a server id from `self.mp_servers`:
                    // select it in `browser`, verify a non-default profile via
                    // `require_profile_for`, and emit `ShellAction::LaunchMultiplayer`.
                    server_id => {
                        if let Some(server) =
                            self.mp_servers.iter().find(|s| s.id == server_id).cloned()
                        {
                            browser.select(Some(&server.id));
                            if !self.require_profile_for(PendingMpAction::Join(
                                Box::new(server.clone()),
                                kind,
                            )) {
                                return;
                            }
                            let launch = resolve_server_join(&server, kind, &self.profile.name);
                            self.sound_queue.push("shell_menu_accept");
                            self.pending_action = Some(ShellAction::LaunchMultiplayer(launch));
                        }
                    }
                }
            }
            // `DirectConnect`: validates `direct_connect_draft` via `direct_connect_entry`
            // and joins over `LAN` (or shows an error notice popup if invalid).
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
            // `HostDialog` (`SET GAME OPTIONS`): pressing `Enter` on `"launch"`
            // starts the hosted session via `execute_host`; pressing `Enter` on
            // any option row steps that option forward (`+1`).
            ScreenId::HostDialog => {
                let rows = self.rows_for(screen);
                let idx = self.selected_row_index(screen);
                if let Some(row) = rows.get(idx) {
                    if row.id == "launch" {
                        self.execute_host();
                    } else {
                        self.adjust_host_option(1);
                    }
                } else {
                    self.execute_host();
                }
            }
            ScreenId::Popup => {}
        }
    }

    /// Initializes `self.host_config` defaults from the current profile and
    /// extracted maps, then pushes `ScreenId::HostDialog` (`SET GAME OPTIONS`).
    pub fn open_host_dialog(&mut self) {
        self.host_config.server_name = format!("{}'S GAME", self.profile.name);
        if self.host_config.map.is_empty()
            || !self
                .data
                .maps
                .iter()
                .any(|m| m.extracted != Some(false) && m.id == self.host_config.map)
        {
            if let Some(first_map) = self.data.maps.iter().find(|m| m.extracted != Some(false)) {
                self.host_config.family = first_map.family;
                self.host_config.map = first_map.id.clone();
                let choices = Self::map_choices(first_map);
                if let Some(first_era) = choices.first() {
                    self.host_config.era = first_era.clone();
                    self.host_config.side = if first_era == "gcw" {
                        "all".to_string()
                    } else {
                        "rep".to_string()
                    };
                }
            }
        }
        self.sound_queue.push("shell_menu_enter");
        self.push_screen(ScreenId::HostDialog);
    }

    /// Ensures the player has a custom profile (`name != "PLAYER 1"`) before
    /// hosting or joining multiplayer; if not, stores `action` and opens the
    /// profile editor with the multiplayer prompt message.
    ///
    /// Mirrors `requireProfile()` in `src/shell/screens.js`.
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

    /// Builds a [`ServerEntry`] from `self.host_config` and sets
    /// `pending_action = Some(ShellAction::LaunchMultiplayer(launch))`.
    pub fn execute_host(&mut self) {
        self.host_config.server_name = format!("{}'S GAME", self.profile.name);
        if self.host_config.map.is_empty() {
            if let Some(first_map) = self.data.maps.iter().find(|m| m.extracted != Some(false)) {
                self.host_config.family = first_map.family;
                self.host_config.map = first_map.id.clone();
            } else {
                self.host_config.family = GameFamily::Swbf1;
                self.host_config.map = "nab2".to_string();
            }
        }
        let server = hosted_server_entry(&self.host_config);
        let launch = resolve_server_join(&server, MultiplayerKind::Lan, &self.profile.name);
        self.sound_queue.push("shell_menu_accept");
        self.pending_action = Some(ShellAction::LaunchMultiplayer(launch));
    }

    /// Refreshes `browser` for `kind` (`LAN` or `ONLINE`) and pushes
    /// `ScreenId::MultiplayerList(kind)`.
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
        // Switch `browser` between `Source::Lan` and `Source::Internet` if
        // needed before synchronizing its rows.
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
        // 1. On the click gate (`ScreenId::Gate`), any non-modifier key that
        //    satisfies `gate::counts_as_press` unlocks into `ScreenId::Main`.
        if self.current_screen() == ScreenId::Gate {
            if crate::gate::counts_as_press("keydown", key, ctrl, meta, alt) {
                self.unlock_gate();
                return true;
            }
            return false;
        }

        // 2. While editing a profile callsign, route `Enter`, `Escape`,
        //    `Backspace`, and printable characters to `self.profile`.
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
                    // Append unmodified single-character keys to `profile.draft`.
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

        // 3. On `DirectConnect`, route `Enter`, `Escape`, `Backspace`, and
        //    printable address characters to `self.direct_connect_draft`.
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
                    // Append unmodified single-character keys to `direct_connect_draft`.
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

        // 4. Standard menu navigation keys (`bindMenuInput` in `src/menus/input.js`):
        //    arrow keys / WASD move focus, `Enter`/`Space` accepts, `Escape`/`Backspace`
        //    goes back, and `Delete` removes the selected Instant Action entry.
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
            // `onKey` in `instantAction()` (`src/shell/screens.js`): `Delete`
            // removes the currently selected map from `SELECTED MAPS`.
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

/// Convenience helper returning a default [`ServerBrowser`] configured with
/// the standard network contract (`bfe-sim-28`) for callers and tests.
pub fn default_server_browser() -> ServerBrowser {
    ServerBrowser::default()
}
