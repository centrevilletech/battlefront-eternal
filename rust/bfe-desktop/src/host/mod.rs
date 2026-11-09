//! Detached local server host controller (`HostController`) for `bfe-desktop`.
//!
//! Ports `createHostController`, `startOutside`, and `unitActive` from the desktop host,
//! together with the close-decision prompt and keep-running flow from the desktop host
//! (`before-quit` handler).
//!
//! ## Architecture and Responsibilities
//! - **Detached Process Lifecycle**: `HostController::start` spawns the bundled `bfeserver`
//!   binary (`--serve`) in its own process group (`process_group(0)` on Unix;
//!   `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW` on Windows) so the
//!   hosted match continues running after the native client exits. When launched under
//!   native Linux Steam (`outside == true`), [`start_outside`] delegates process creation to
//!   `systemd-run --user` so Steam's reaper does not hold the game in `"playing"` state or
//!   kill the server on exit.
//! - **State and Log Files**: Writes `<state_root>/config.json` (mode `0600`) using only
//!   valid `bfe-server` configuration keys (`name`, `engine`, `data`, `rotation`,
//!   `maxPlayers`, `onlineNoticeAccepted`, `master`, `relayPolicy`, `honorGlobalBans`,
//!   `gamePort`, plus optional `autoAssignTeams`, `ai`, and `autoSpawn`). Redirects child
//!   `stdout` and `stderr` to `<state_root>/hosting.log` (mode `0600`), rotating/truncating
//!   the log when it exceeds [`LOG_ROTATE_BYTES`] (`8_388_608` bytes).
//! - **Readiness and Attach**: Waits up to 60 seconds (120 polls at 500 ms) for `/admin`
//!   (`{"action": "status"}` returning `status == "running"`) and `GET /info` on the game
//!   port (`18461` by default) to succeed. On subsequent client launches or `start()` calls,
//!   [`HostController::attach`] detects an already-running server via `<state_root>/operator-token`,
//!   `/admin`, and `/info`, re-attaching instead of spawning a duplicate `bfeserver`.
//! - **Close Decision Data**: Exposes [`CloseChoice`] and [`ClosePrompt`] with the exact
//!   dialog strings from the desktop host (`"Hosted server is running"`,
//!   `"Other players can keep playing after you close the app."`, and button labels) so the
//!   native client shell can render the quit prompt with its own UI kit.

use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Default loopback TCP port for the hosted server's `/admin` and `/healthz` HTTP listener
/// (`const ADMIN_PORT = port('BFE_HOST_ADMIN_PORT', 18460)` in the desktop host).
pub const DEFAULT_ADMIN_PORT: u16 = 18460;

/// Default UDP gameplay and TCP `/info` port for a client-hosted server
/// (`const GAME_PORT = port('BFE_HOST_GAME_PORT', 18461)` in the desktop host).
pub const DEFAULT_GAME_PORT: u16 = 18461;

/// Maximum human player count allowed in a hosted match
/// (`MAX_MATCH_PLAYERS = 64` in `src/battle-limits.js`, imported by the desktop host).
pub const MAX_MATCH_PLAYERS: u32 = 64;

/// Maximum size in bytes (`8 MiB`) of `<state_root>/hosting.log` before `start()` truncates it
/// (`statSync(log).size > 8388608` in `createHostController` in the desktop host).
pub const LOG_ROTATE_BYTES: u64 = 8_388_608;

/// Default master directory URL written to `<state_root>/config.json`
/// (`master: 'https://online.battlefronteternal.com'` in `createHostController` in the desktop host).
pub const DEFAULT_MASTER_URL: &str = "https://online.battlefronteternal.com";

/// Default readiness poll count (`for (let i = 0; i < 120; i++)` in the desktop host).
pub const DEFAULT_READY_POLL_ATTEMPTS: usize = 120;

/// Default interval in milliseconds between readiness polls (`setTimeout(resolve, 500)` in the desktop host).
pub const DEFAULT_READY_POLL_INTERVAL_MS: u64 = 500;

/// Default loopback HTTP request timeout in milliseconds (`AbortSignal.timeout(3000)` in the desktop host).
pub const DEFAULT_HTTP_TIMEOUT_MS: u64 = 3000;

/// Windows `DETACHED_PROCESS` creation flag (`0x00000008`), detaching the child from the parent's console.
pub const WINDOWS_DETACHED_PROCESS: u32 = 0x0000_0008;

/// Windows `CREATE_NEW_PROCESS_GROUP` creation flag (`0x00000200`), isolating `Ctrl+C` / console signals.
pub const WINDOWS_CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Windows `CREATE_NO_WINDOW` creation flag (`0x08000000`), preventing a console window from appearing
/// (`windowsHide: true` in the desktop host).
pub const WINDOWS_CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Combined Win32 process creation flags used when spawning `bfeserver.exe` on Windows so the
/// server runs detached in the background without flashing a console window.
pub const WINDOWS_HOST_CREATION_FLAGS: u32 =
    WINDOWS_DETACHED_PROCESS | WINDOWS_CREATE_NEW_PROCESS_GROUP | WINDOWS_CREATE_NO_WINDOW;

/// Resolves a TCP/UDP port from an environment variable map, falling back to `fallback` when the
/// variable is unset, non-numeric, or outside `1..=65535`
/// (`const port = (name, fallback) => ...` in the desktop host).
pub fn resolve_port_from_env(env_map: &BTreeMap<String, String>, name: &str, fallback: u16) -> u16 {
    // Parse the environment variable as an integer in 1..=65535 (`value > 0 && value < 65536`).
    env_map
        .get(name)
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .and_then(|val| {
            if (1..=65535).contains(&val) {
                Some(val as u16)
            } else {
                None
            }
        })
        .unwrap_or(fallback)
}

/// Resolves the loopback operator HTTP port (`BFE_HOST_ADMIN_PORT`, default `18460`)
/// (`ADMIN_PORT` in the desktop host).
pub fn resolve_admin_port(env_map: &BTreeMap<String, String>) -> u16 {
    // Delegate to `resolve_port_from_env` with `DEFAULT_ADMIN_PORT` (18460).
    resolve_port_from_env(env_map, "BFE_HOST_ADMIN_PORT", DEFAULT_ADMIN_PORT)
}

/// Resolves the hosted game UDP and `/info` TCP port (`BFE_HOST_GAME_PORT`, default `18461`)
/// (`GAME_PORT` in the desktop host).
pub fn resolve_game_port(env_map: &BTreeMap<String, String>) -> u16 {
    // Delegate to `resolve_port_from_env` with `DEFAULT_GAME_PORT` (18461).
    resolve_port_from_env(env_map, "BFE_HOST_GAME_PORT", DEFAULT_GAME_PORT)
}

/// Resolves the path to the bundled `bfeserver` binary (`bfeserver` on Unix, `bfeserver.exe` on Windows).
///
/// Search order:
/// 1. `BFE_HOST_SERVER_BIN` environment variable when set and non-empty.
/// 2. Sibling of the running client executable (`<exe_dir>/bfeserver[.exe]`), matching
///    `build/native/stage.py` where `bfeserver` is staged next to `bfe-client`.
/// 3. `<project_root>/rust/target/release/bfeserver[.exe]` and `<project_root>/rust/target/debug/bfeserver[.exe]`
///    for development worktrees.
/// 4. Packaged Linux installation paths (`/opt/Battlefront Eternal/bfeserver` and `/usr/bin/bfeserver`).
pub fn resolve_server_executable(
    env_map: &BTreeMap<String, String>,
    current_exe: Option<&Path>,
    project_root: Option<&Path>,
) -> PathBuf {
    // Check explicit environment override first (`BFE_HOST_SERVER_BIN`).
    if let Some(custom) = env_map.get("BFE_HOST_SERVER_BIN") {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    // Select the platform-appropriate binary filename (`bfeserver.exe` on Windows, `bfeserver` on Unix).
    let bin_name = if cfg!(windows) {
        "bfeserver.exe"
    } else {
        "bfeserver"
    };

    let mut candidates = Vec::new();

    // Check alongside the running executable (`stage.py` places `bfeserver` next to `bfe-client`).
    if let Some(exe) = current_exe {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(bin_name));
        }
    }

    // Check workspace build directories (`rust/target/release` and `rust/target/debug`).
    if let Some(root) = project_root {
        candidates.push(root.join("rust").join("target").join("release").join(bin_name));
        candidates.push(root.join("rust").join("target").join("debug").join(bin_name));
        candidates.push(root.join("target").join("release").join(bin_name));
        candidates.push(root.join("target").join("debug").join(bin_name));
    }

    // Check standard Linux packaged locations from `build/native/linux.py`.
    if !cfg!(windows) {
        candidates.push(PathBuf::from("/opt/Battlefront Eternal/bfeserver"));
        candidates.push(PathBuf::from("/usr/bin/bfeserver"));
    }

    // Return the first candidate path that exists as a regular file on disk.
    for candidate in &candidates {
        if candidate.is_file() {
            return candidate.clone();
        }
    }

    // Return the first candidate (sibling of `current_exe` or default binary name) so
    // `HostController::start` reports the missing-runtime error cleanly if nothing exists.
    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from(bin_name))
}

/// Visibility mode for a client-hosted server (`options.mode` in the desktop host).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostMode {
    /// Private / LAN-only server (`onlineNoticeAccepted: false`, `relayPolicy: "never"`).
    Lan,
    /// Publicly listed server registered with the online master directory (`onlineNoticeAccepted: true`).
    Public,
}

impl HostMode {
    /// Returns the wire/config string (`"lan"` or `"public"`).
    pub fn as_str(self) -> &'static str {
        // Match the exact mode strings validated in `createHostController.start` (the desktop host).
        match self {
            Self::Lan => "lan",
            Self::Public => "public",
        }
    }

    /// Parses `"lan"` or `"public"` (also accepting `"private"` as an alias for `Lan`).
    pub fn parse(value: &str) -> Option<Self> {
        // Normalize case and whitespace before matching `"lan"` / `"private"` or `"public"`.
        match value.trim().to_ascii_lowercase().as_str() {
            "lan" | "private" => Some(Self::Lan),
            "public" => Some(Self::Public),
            _ => None,
        }
    }
}

/// TURN relay policy for a hosted server (`options.relay` in the desktop host).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelayPolicy {
    /// Automatically use TURN relay fallback when direct UDP hole-punching fails (`"auto"`).
    Auto,
    /// Route all player traffic exclusively through TURN relays (`"always"`).
    Always,
    /// Disable TURN relay usage (`"never"`).
    Never,
}

impl RelayPolicy {
    /// Returns the configuration string (`"auto"`, `"always"`, or `"never"`).
    pub fn as_str(self) -> &'static str {
        // Match the exact `relayPolicy` strings accepted by `ServerConfig` (`bfe-server/src/config.rs`).
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    /// Parses `"auto"`, `"always"`, or `"never"`.
    pub fn parse(value: &str) -> Option<Self> {
        // Normalize case and whitespace before matching `"auto"`, `"always"`, or `"never"`.
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }
}

/// Bot difficulty preset from the `SET GAME OPTIONS` screen (`rust/bfe-shell/src/screens.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AiDifficulty {
    /// Easy bot difficulty.
    Easy,
    /// Medium (default) bot difficulty.
    Medium,
    /// Hard bot difficulty.
    Hard,
}

/// Team assignment rule from the `SET GAME OPTIONS` screen (`rust/bfe-shell/src/screens.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TeamAssignment {
    /// Automatically assign joining players to balance teams (`autoAssignTeams: true`).
    AutoAssign,
    /// Allow joining players to choose their faction (`autoAssignTeams: false`).
    PlayerSelect,
}

/// Game and network options passed to [`HostController::start`], combining the desktop host
/// host dialog options (`src/shell/host-dialog.js` and the desktop host) with the
/// native client's `SET GAME OPTIONS` screen (`rust/bfe-shell/src/screens.rs`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostOptions {
    /// Server display name (`1..=80` non-whitespace characters; `options.name` in the desktop host).
    pub name: String,
    /// Server visibility (`HostMode::Lan` for private/LAN, `HostMode::Public` for master listing).
    pub mode: HostMode,
    /// Primary map identifier matching `^[a-z0-9_-]{1,32}$` (e.g. `"nab2"`).
    pub map: String,
    /// Primary faction identifier (`"rep"`, `"cis"`, `"all"`, or `"imp"`).
    pub side: String,
    /// Maximum human players (`2..=64`; `options.maxPlayers` in the desktop host).
    pub max_players: u32,
    /// TURN relay policy (`RelayPolicy::Auto`, `RelayPolicy::Always`, or `RelayPolicy::Never`).
    pub relay: RelayPolicy,
    /// Optional multi-map rotation entries (e.g. `["swbf1/nab2:rep:conquest", "swbf1/tat1:imp:conquest"]`).
    /// When empty, `start()` constructs `["swbf1/<map>:<side>:conquest"]` exactly like the desktop host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub playlist: Vec<String>,
    /// Optional session password from the host dialog (stored only in memory/UI, never written as an
    /// unknown key to `bfeserver`'s `config.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Optional bot count per team (`0..=32` from `SET GAME OPTIONS`). When `Some(0)`, writes `"ai": "off"`
    /// to `config.json`; otherwise uses `"classic"` AI mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_units_per_team: Option<u32>,
    /// Optional bot difficulty from `SET GAME OPTIONS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_difficulty: Option<AiDifficulty>,
    /// Optional reinforcement ticket percentage (`50..=500`) from `SET GAME OPTIONS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reinforcements_pct: Option<u32>,
    /// Optional capture point victory timer (`10..=60` seconds) from `SET GAME OPTIONS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub victory_timer_sec: Option<u32>,
    /// Optional team damage toggle from `SET GAME OPTIONS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friendly_fire: Option<bool>,
    /// Optional hero spawn toggle from `SET GAME OPTIONS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heroes_enabled: Option<bool>,
    /// Optional team assignment rule (`AutoAssign` maps to `autoAssignTeams: true` in `config.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_assignment: Option<TeamAssignment>,
    /// Optional immediate spawn toggle (`autoSpawn: true` in `config.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_spawn: Option<bool>,
}

impl Default for HostOptions {
    /// Returns default LAN host options matching `src/shell/host-dialog.js`
    /// (`name: "Battlefront Eternal LAN"`, `mode: Lan`, `map: "nab2"`, `side: "rep"`,
    /// `max_players: 8`, `relay: Auto`).
    fn default() -> Self {
        Self {
            name: "Battlefront Eternal LAN".to_string(),
            mode: HostMode::Lan,
            map: "nab2".to_string(),
            side: "rep".to_string(),
            max_players: 8,
            relay: RelayPolicy::Auto,
            playlist: Vec::new(),
            password: None,
            ai_units_per_team: None,
            ai_difficulty: None,
            reinforcements_pct: None,
            victory_timer_sec: None,
            friendly_fire: None,
            heroes_enabled: None,
            team_assignment: None,
            auto_spawn: None,
        }
    }
}

impl HostOptions {
    /// Validates host options against the exact rules in `createHostController.start` (the desktop host).
    ///
    /// Returns `Err("Invalid host settings")` if any field fails validation.
    pub fn validate(&self) -> Result<(), String> {
        // Reject blank names or names exceeding 80 characters (`!options.name.trim() || options.name.length > 80`).
        let trimmed = self.name.trim();
        if trimmed.is_empty() || self.name.chars().count() > 80 {
            return Err("Invalid host settings".to_string());
        }
        // Enforce `maxPlayers` bounds (`options.maxPlayers < 2 || options.maxPlayers > MAX_MATCH_PLAYERS`).
        if self.max_players < 2 || self.max_players > MAX_MATCH_PLAYERS {
            return Err("Invalid host settings".to_string());
        }
        // Validate map ID against `/^[a-z0-9_-]{1,32}$/` and faction ID against `['rep', 'cis', 'all', 'imp']`.
        if !is_valid_map_id(&self.map) {
            return Err("Invalid host settings".to_string());
        }
        if !matches!(self.side.as_str(), "rep" | "cis" | "all" | "imp") {
            return Err("Invalid host settings".to_string());
        }
        // Validate any explicit playlist rotation entries (`swbf1/<map>:<side>:<mode>`).
        for entry in &self.playlist {
            if !is_valid_rotation_entry(entry) {
                return Err("Invalid host settings".to_string());
            }
        }
        Ok(())
    }

    /// Returns the effective mission rotation array written to `config.json`
    /// (`rotation: [`swbf1/${options.map}:${options.side}:conquest`]` in the desktop host).
    pub fn effective_rotation(&self) -> Vec<String> {
        // Default to a single conquest entry when no multi-map playlist is specified.
        if self.playlist.is_empty() {
            vec![format!("swbf1/{}:{}:conquest", self.map, self.side)]
        } else {
            self.playlist.clone()
        }
    }

    /// Builds the `config.json` JSON object for `bfeserver` using strictly valid keys from
    /// `rust/bfe-server/src/config.rs` and the desktop host.
    pub fn to_server_config_json(
        &self,
        engine_root: &Path,
        content_root: &Path,
        game_port: u16,
        master_url: &str,
    ) -> Value {
        // Construct the base 10 keys written by `createHostController.start` in the desktop host.
        let mut obj = Map::new();
        obj.insert("name".to_string(), Value::String(self.name.trim().to_string()));
        obj.insert(
            "engine".to_string(),
            Value::String(engine_root.to_string_lossy().into_owned()),
        );
        obj.insert(
            "data".to_string(),
            Value::String(content_root.to_string_lossy().into_owned()),
        );
        obj.insert(
            "rotation".to_string(),
            Value::Array(
                self.effective_rotation()
                    .into_iter()
                    .map(Value::String)
                    .collect(),
            ),
        );
        obj.insert("maxPlayers".to_string(), json!(self.max_players));
        obj.insert(
            "onlineNoticeAccepted".to_string(),
            Value::Bool(self.mode == HostMode::Public),
        );
        obj.insert("master".to_string(), Value::String(master_url.to_string()));
        let relay_policy = if self.mode == HostMode::Lan {
            "never"
        } else {
            self.relay.as_str()
        };
        obj.insert(
            "relayPolicy".to_string(),
            Value::String(relay_policy.to_string()),
        );
        obj.insert("honorGlobalBans".to_string(), Value::Bool(true));
        obj.insert("gamePort".to_string(), json!(game_port));

        // Map optional `SET GAME OPTIONS` fields to real `ServerConfig` keys in `bfe-server/src/config.rs`.
        if let Some(units) = self.ai_units_per_team {
            let ai_mode = if units == 0 { "off" } else { "classic" };
            obj.insert("ai".to_string(), Value::String(ai_mode.to_string()));
        }
        if let Some(assignment) = self.team_assignment {
            obj.insert(
                "autoAssignTeams".to_string(),
                Value::Bool(assignment == TeamAssignment::AutoAssign),
            );
        }
        if let Some(auto_spawn) = self.auto_spawn {
            obj.insert("autoSpawn".to_string(), Value::Bool(auto_spawn));
        }

        Value::Object(obj)
    }
}

/// Validates a map identifier against `/^[a-z0-9_-]{1,32}$/` (the desktop host).
fn is_valid_map_id(map: &str) -> bool {
    // Reject empty strings, identifiers over 32 bytes, or characters outside `[a-z0-9_-]`.
    !map.is_empty()
        && map.len() <= 32
        && map
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Validates a rotation entry against `swbf1/<map>:<side>:<mode>` (`valid_playlist_entry` in `bfe-server`).
fn is_valid_rotation_entry(entry: &str) -> bool {
    // Require the `swbf1/` family prefix (`BF2 simulation is not implemented` in `bfe-server`).
    let Some(rest) = entry.strip_prefix("swbf1/") else {
        return false;
    };
    // Split `<map>:<side>:<mode>` and validate each segment.
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    is_valid_map_id(parts[0])
        && matches!(parts[1], "rep" | "cis" | "all" | "imp")
        && is_valid_map_id(parts[2])
}

/// Extracts the map ID from a rotation entry (`"swbf1/nab2:rep:conquest"` -> `Some("nab2")`).
fn map_id_from_rotation_entry(entry: &str) -> Option<&str> {
    // Strip the `swbf1/` prefix and take the `<map>` component before the first colon.
    let rest = entry.strip_prefix("swbf1/")?;
    rest.split(':').next()
}

/// Connected player summary returned by `/admin` `{"action": "status"}` (`OperatorControl::players`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostedPlayer {
    /// 64-character lowercase SHA-256 hex player identity.
    pub id: String,
    /// Player display name.
    #[serde(default)]
    pub name: String,
    /// Remote peer socket address string.
    #[serde(default)]
    pub address: Option<String>,
    /// Smoothed round-trip time in milliseconds.
    #[serde(default)]
    pub rtt: Option<f64>,
    /// Count of dropped input frames.
    #[serde(default)]
    pub dropped_inputs: Option<u64>,
    /// Extra per-peer network counters preserved from `bfeserver`.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Live status snapshot returned by [`HostController::status`], combining `/admin`
/// (`{"action": "status"}`) and `/info` on the game port.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    /// Whether the hosted server's `/admin` endpoint is reachable and active (`active` in the desktop host).
    pub active: bool,
    /// Whether `HostController::start` is currently polling for readiness (`starting` in the desktop host).
    pub starting: bool,
    /// Service lifecycle status reported by `bfeserver` (`"setup"`, `"loading"`, `"running"`, or `"fault"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// OS process identifier reported by `bfeserver` (`/admin` `pid`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Current 60 Hz authoritative simulation tick (`/admin` `tick`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick: Option<u64>,
    /// Unix timestamp in milliseconds when the server started (`/admin` `startedAt`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    /// Whole-second server uptime computed from `started_at` or reported by `/info` (`uptime`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<u64>,
    /// Server display name from `/info`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Active map identifier from `/info` (`battle.map` or `query`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    /// Human-readable map title from `/info` (`battle.mapName`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map_name: Option<String>,
    /// Active faction identifier from `/info` (`battle.side`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    /// Active game mode from `/info` (`battle.mode`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Active battle query string from `/info` (`query`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Maximum human player capacity from `/info` (`maxPlayers`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_players: Option<u32>,
    /// Bound UDP game and TCP `/info` port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Loopback TCP operator `/admin` port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin_port: Option<u16>,
    /// Connected human players reported by `/admin` (`players`).
    #[serde(default)]
    pub players: Vec<HostedPlayer>,
    /// Active local bans reported by `/admin` (`bans`).
    #[serde(default)]
    pub bans: Vec<Value>,
    /// Transport metrics snapshot reported by `/admin` (`transport`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<Value>,
}

/// Join descriptor returned by [`HostController::start`], [`HostController::join_info`], and
/// [`HostController::attach`] (`joinInfo()` in the desktop host).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostJoinInfo {
    /// Loopback IPv4 address (`"127.0.0.1"`).
    pub address: String,
    /// Bound UDP gameplay and TCP `/info` port (default `18461`).
    pub port: u16,
    /// Server public key digest (`id` in `/info`).
    #[serde(default)]
    pub id: String,
    /// Server display name (`name` in `/info`).
    #[serde(default)]
    pub name: String,
    /// Active battle query string (`query` in `/info`, e.g. `"?map=nab2&side=rep&ai=classic&seed=1&match=..."`).
    #[serde(default)]
    pub query: String,
    /// Content SHA-256 fingerprint (`fingerprint` in `/info`).
    #[serde(default)]
    pub fingerprint: String,
    /// Content identifier (`content` in `/info`).
    #[serde(default)]
    pub content: String,
    /// Connected player count (`players` in `/info`).
    #[serde(default)]
    pub players: u32,
    /// Maximum human player count (`maxPlayers` in `/info`).
    #[serde(default)]
    pub max_players: u32,
    /// Server process ID when known from `/admin`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Additional `/info` fields (`protocol`, `profile`, `battle`, `rotation`, `uptime`, etc.).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Persistent attach metadata written to `<state_root>/attach.json` (mode `0600`) when a server
/// is started or kept running on client close so the next client startup can inspect or attach to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachRecord {
    /// Server OS process ID reported by `/admin`.
    pub pid: Option<u32>,
    /// Loopback TCP operator `/admin` port.
    pub admin_port: u16,
    /// UDP gameplay and TCP `/info` port.
    pub game_port: u16,
    /// Unix timestamp in milliseconds when the server started.
    pub started_at: Option<u64>,
    /// Optional `systemd --user` transient unit name when launched via [`start_outside`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Whether the user chose [`CloseChoice::KeepRunning`] when closing the client.
    pub kept_running: bool,
}

/// User choice when closing the client while a hosted server is running
/// (`before-quit` dialog in the desktop host, lines 1061-1068).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseChoice {
    /// Leave the detached `bfeserver` running in the background and write the attach record
    /// (`choice === 0`: `"Keep server running"` in the desktop host).
    KeepRunning,
    /// Stop the hosted server gracefully via `/admin` (`{"action": "stop"}`) and quit the client
    /// (`choice === 1`: `"Stop server and quit"` in the desktop host).
    StopAndQuit,
    /// Abort closing the client and keep playing/hosting (`choice === 2`: `"Cancel"` in the desktop host).
    Cancel,
}

/// Dialog copy and button labels for the close confirmation prompt when a hosted server is active
/// (`dialog.showMessageBoxSync` in `before-quit` in the desktop host, lines 1061-1068).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosePrompt {
    /// Dialog window title (`"Hosted server is running"` in the desktop host).
    pub title: &'static str,
    /// Dialog body message (`"Other players can keep playing after you close the app."` in the desktop host).
    pub message: &'static str,
    /// Ordered button labels (`["Keep server running", "Stop server and quit", "Cancel"]` in the desktop host).
    pub buttons: [&'static str; 3],
    /// Ordered choices corresponding 1:1 with `buttons`.
    pub choices: [CloseChoice; 3],
    /// Default button index (`0` -> `CloseChoice::KeepRunning`).
    pub default_id: usize,
    /// Cancel button index (`2` -> `CloseChoice::Cancel`).
    pub cancel_id: usize,
}

impl Default for ClosePrompt {
    /// Returns the exact close confirmation prompt from the desktop host (`before-quit`).
    fn default() -> Self {
        Self::new()
    }
}

impl ClosePrompt {
    /// Constructs the close confirmation prompt with the exact strings from the desktop host (lines 1061-1068).
    pub const fn new() -> Self {
        Self {
            title: "Hosted server is running",
            message: "Other players can keep playing after you close the app.",
            buttons: [
                "Keep server running",
                "Stop server and quit",
                "Cancel",
            ],
            choices: [
                CloseChoice::KeepRunning,
                CloseChoice::StopAndQuit,
                CloseChoice::Cancel,
            ],
            default_id: 0,
            cancel_id: 2,
        }
    }

    /// Maps a button index (`0`, `1`, or `2`) to its corresponding [`CloseChoice`],
    /// defaulting to [`CloseChoice::Cancel`] for out-of-range indices.
    pub const fn choice_for_index(&self, index: usize) -> CloseChoice {
        match index {
            0 => CloseChoice::KeepRunning,
            1 => CloseChoice::StopAndQuit,
            _ => CloseChoice::Cancel,
        }
    }
}

/// Parameters for [`start_outside`] (`export function startOutside` in the desktop host).
#[derive(Clone, Debug)]
pub struct StartOutsideOptions {
    /// Path to the `bfeserver` binary.
    pub executable: PathBuf,
    /// Command-line arguments passed to `bfeserver` (e.g. `["--serve"]`).
    pub args: Vec<String>,
    /// Environment variables passed via `--setenv=KEY=VALUE`.
    pub env: BTreeMap<String, String>,
    /// Path to the hosting log file (`<state_root>/hosting.log`).
    pub log: PathBuf,
    /// Program used to launch transient user units (defaults to `"systemd-run"`).
    pub systemd_run_program: String,
}

/// Starts `bfeserver` outside the current process tree using the user's `systemd` instance
/// (`export function startOutside` in the desktop host).
///
/// When the game is started by native Linux Steam, Steam's reaper waits for every process in the
/// game's cgroup/tree. Starting `bfeserver` via `systemd-run --user --collect --quiet` places it
/// in its own transient user unit (`battlefront-eternal-server-<timestamp>`) so closing the client
/// does not leave Steam showing the game as `"playing"` and Steam's STOP button does not kill
/// the hosted server.
///
/// Returns `Some(unit_name)` when `systemd-run` exits with status `0`, or `None` when `systemd-run`
/// is unavailable or fails (in which case the caller falls back to direct detached spawning).
pub fn start_outside(options: &StartOutsideOptions) -> Option<String> {
    // Generate a unique transient unit name stamped with current Unix epoch milliseconds.
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let unit = format!("battlefront-eternal-server-{now_ms}");
    let log_str = options.log.to_string_lossy();

    // Build `systemd-run --user --collect --quiet --unit=<unit>` with log redirection (the desktop host).
    let mut cmd = Command::new(&options.systemd_run_program);
    cmd.arg("--user")
        .arg("--collect")
        .arg("--quiet")
        .arg(format!("--unit={unit}"))
        .arg(format!("--property=StandardOutput=append:{log_str}"))
        .arg(format!("--property=StandardError=append:{log_str}"));

    // Forward each server environment variable via `--setenv=KEY=VALUE`.
    for (key, value) in &options.env {
        cmd.arg(format!("--setenv={key}={value}"));
    }

    // Append the `bfeserver` executable and its `--serve` arguments.
    cmd.arg(&options.executable);
    for arg in &options.args {
        cmd.arg(arg);
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(WINDOWS_HOST_CREATION_FLAGS);
    }

    // Resolve `Some(unit)` on exit code 0, or `None` on spawn error / non-zero exit (the desktop host).
    match cmd.status() {
        Ok(status) if status.success() => Some(unit),
        _ => None,
    }
}

/// Checks whether a transient `systemd --user` unit started by [`start_outside`] is currently active
/// (`const unitActive = unit => spawnSync('systemctl', ['--user', 'is-active', '--quiet', unit])` in the desktop host).
pub fn unit_active(unit: &str) -> bool {
    unit_active_with_program("systemctl", unit)
}

/// Testable helper for [`unit_active`] allowing a custom `systemctl` command path.
pub fn unit_active_with_program(systemctl_program: &str, unit: &str) -> bool {
    // Run `systemctl --user is-active --quiet <unit>` and check for exit status 0.
    Command::new(systemctl_program)
        .arg("--user")
        .arg("is-active")
        .arg("--quiet")
        .arg(unit)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Configuration for constructing a [`HostController`] (`createHostController` parameters in the desktop host).
#[derive(Clone, Debug)]
pub struct HostControllerConfig {
    /// Path to the `bfeserver` executable.
    pub executable: PathBuf,
    /// Legacy engine root path written to `config.json` (`engineRoot` in the desktop host).
    pub engine_root: PathBuf,
    /// Prepared game content directory (`contentRoot` in the desktop host).
    pub content_root: PathBuf,
    /// Host state directory (`<userData>/server` in the desktop host, containing `config.json`,
    /// `operator-token`, `operator.sqlite`, `hosting.log`, and `attach.json`).
    pub state_root: PathBuf,
    /// Loopback TCP operator `/admin` port (default `18460`, overridable via `BFE_HOST_ADMIN_PORT`).
    pub admin_port: u16,
    /// UDP gameplay and TCP `/info` port (default `18461`, overridable via `BFE_HOST_GAME_PORT`).
    pub game_port: u16,
    /// Master server base URL (default `"https://online.battlefronteternal.com"`).
    pub master_url: String,
    /// Whether the user has accepted the online privacy notice (`consent` callback in the desktop host).
    pub online_consent: bool,
    /// Whether to attempt launching via `systemd-run --user` (`outside` callback in the desktop host,
    /// true when launched under native Linux Steam).
    pub launch_outside: bool,
    /// Program name/path for `systemd-run` (defaults to `"systemd-run"`).
    pub systemd_run_program: String,
    /// Program name/path for `systemctl` (defaults to `"systemctl"`).
    pub systemctl_program: String,
    /// Number of readiness poll iterations in `start()` (defaults to `120`).
    pub ready_poll_attempts: usize,
    /// Delay between readiness poll iterations in milliseconds (defaults to `500` ms; `120 * 500 ms = 60 s`).
    pub ready_poll_interval: Duration,
    /// Socket read/write/connect timeout for loopback `/admin` and `/info` HTTP calls (defaults to `3000` ms).
    pub http_timeout: Duration,
}

impl HostControllerConfig {
    /// Creates a [`HostControllerConfig`] for `state_root` and `content_root`, resolving ports and
    /// the `bfeserver` binary path from the current environment.
    pub fn new(state_root: PathBuf, content_root: PathBuf) -> Self {
        // Snapshot the current environment to resolve ports, binary path, and Steam launch status.
        let env_map: BTreeMap<String, String> = env::vars().collect();
        let current_exe = env::current_exe().ok();
        let executable =
            resolve_server_executable(&env_map, current_exe.as_deref(), None);
        let engine_root = content_root.clone();
        let admin_port = resolve_admin_port(&env_map);
        let game_port = resolve_game_port(&env_map);
        // Enable `systemd-run --user` on Linux when the client was launched by Steam (the desktop host).
        let launch_outside = cfg!(target_os = "linux")
            && crate::steam::client::launched_under_steam(&env_map, &env::args().collect::<Vec<_>>());

        Self {
            executable,
            engine_root,
            content_root,
            state_root,
            admin_port,
            game_port,
            master_url: DEFAULT_MASTER_URL.to_string(),
            online_consent: false,
            launch_outside,
            systemd_run_program: "systemd-run".to_string(),
            systemctl_program: "systemctl".to_string(),
            ready_poll_attempts: DEFAULT_READY_POLL_ATTEMPTS,
            ready_poll_interval: Duration::from_millis(DEFAULT_READY_POLL_INTERVAL_MS),
            http_timeout: Duration::from_millis(DEFAULT_HTTP_TIMEOUT_MS),
        }
    }
}

/// Manages a client-hosted `bfeserver` instance across the native client's lifecycle,
/// porting `createHostController` from the desktop host and the quit/keep-running
/// integration from the desktop host.
#[derive(Clone, Debug)]
pub struct HostController {
    /// Immutable paths, ports, and timeouts for this controller.
    config: HostControllerConfig,
    /// Synchronized runtime state tracking in-flight startup and the last attached session.
    state: Arc<Mutex<ControllerInner>>,
}

#[derive(Debug, Default)]
struct ControllerInner {
    /// True while `start()` is actively spawning and waiting for readiness.
    starting: bool,
    /// Last attached or spawned server join info.
    attached: Option<HostJoinInfo>,
    /// Transient `systemd --user` unit name if started via `start_outside`.
    unit: Option<String>,
}

impl HostController {
    /// Creates a new [`HostController`] and ensures `<state_root>` exists with mode `0700` on Unix
    /// (`mkdirSync(stateRoot, { recursive: true, mode: 0o700 })` in `createHostController` in the desktop host).
    pub fn new(config: HostControllerConfig) -> Result<Self, String> {
        ensure_dir_mode_700(&config.state_root)?;
        Ok(Self {
            config,
            state: Arc::new(Mutex::new(ControllerInner::default())),
        })
    }

    /// Returns a reference to the controller's configuration.
    pub fn config(&self) -> &HostControllerConfig {
        &self.config
    }

    /// Updates the prepared content root directory used when validating maps and writing `config.json`.
    pub fn set_content_root(&mut self, content_root: PathBuf) {
        self.config.content_root = content_root;
    }

    /// Updates whether the user has accepted the online privacy notice (`consent` in the desktop host).
    pub fn set_online_consent(&mut self, accepted: bool) {
        self.config.online_consent = accepted;
    }

    /// Returns the path to `<state_root>/hosting.log` where `bfeserver` stdout and stderr are written.
    pub fn log_path(&self) -> PathBuf {
        self.config.state_root.join("hosting.log")
    }

    /// Returns the path to `<state_root>/config.json`.
    pub fn config_path(&self) -> PathBuf {
        self.config.state_root.join("config.json")
    }

    /// Returns the path to `<state_root>/operator-token`.
    pub fn token_path(&self) -> PathBuf {
        self.config.state_root.join("operator-token")
    }

    /// Returns the path to `<state_root>/attach.json`.
    pub fn attach_path(&self) -> PathBuf {
        self.config.state_root.join("attach.json")
    }

    /// Sends an authenticated `POST /admin` JSON request to `127.0.0.1:<admin_port>` using the
    /// bearer token in `<state_root>/operator-token` (`async function admin(value)` in the desktop host).
    ///
    /// Returns `Err("Cannot manage the hosted server")` if the token file cannot be read, the
    /// loopback HTTP request fails, or the server responds with a non-2xx HTTP status.
    pub fn admin(&self, value: &Value) -> Result<Value, String> {
        // Read the 64-hex operator token generated by `bfeserver` in `<state_root>/operator-token`.
        let token = fs::read_to_string(self.token_path())
            .map_err(|_| "Cannot manage the hosted server".to_string())?;
        let token = token.trim();
        if token.is_empty() {
            return Err("Cannot manage the hosted server".to_string());
        }
        // Serialize the JSON command payload and send `POST /admin` with `Authorization: Bearer <token>`.
        let body = serde_json::to_vec(value)
            .map_err(|_| "Cannot manage the hosted server".to_string())?;
        let auth = format!("Bearer {token}");
        let headers = [
            ("Content-Type", "application/json"),
            ("Authorization", auth.as_str()),
        ];
        let (status, resp_body) = loopback_http_request(
            self.config.admin_port,
            "POST",
            "/admin",
            &headers,
            Some(&body),
            self.config.http_timeout,
        )
        .map_err(|_| "Cannot manage the hosted server".to_string())?;

        // Any non-2xx status maps to `"Cannot manage the hosted server"` (`if (!response.ok) throw Error(...)`).
        if !(200..300).contains(&status) {
            return Err("Cannot manage the hosted server".to_string());
        }
        serde_json::from_slice(&resp_body)
            .map_err(|_| "Cannot manage the hosted server".to_string())
    }

    /// Queries the hosted server's live status by calling `/admin` (`{"action": "status"}`) and,
    /// when running, enriching the result with `/info` from the game port (`status()` in the desktop host).
    ///
    /// Never fails: when no server is running or `/admin` is unreachable, returns a [`HostStatus`]
    /// with `active: false`.
    pub fn status(&self) -> HostStatus {
        // Snapshot whether a `start()` call is currently in progress.
        let starting = self
            .state
            .lock()
            .map(|g| g.starting)
            .unwrap_or(false);

        // Query `/admin` (`{"action": "status"}`); return `active: false` if unreachable.
        let Ok(admin_val) = self.admin(&json!({ "action": "status" })) else {
            return HostStatus {
                active: false,
                starting,
                ..HostStatus::default()
            };
        };

        // Extract `status`, `pid`, `tick`, `startedAt`, `players`, `bans`, and `transport` from `/admin`.
        let status_str = admin_val
            .get("status")
            .and_then(Value::as_str)
            .map(String::from);
        let pid = admin_val
            .get("pid")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let tick = admin_val.get("tick").and_then(Value::as_u64);
        let started_at = admin_val.get("startedAt").and_then(Value::as_u64);
        let players: Vec<HostedPlayer> = admin_val
            .get("players")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let bans: Vec<Value> = admin_val
            .get("bans")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let transport = admin_val.get("transport").cloned();

        // Compute elapsed uptime in seconds from `startedAt` as a baseline before `/info` is queried.
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut uptime = started_at.map(|s| now_ms.saturating_sub(s) / 1000);

        let mut name = None;
        let mut map = None;
        let mut map_name = None;
        let mut side = None;
        let mut mode = None;
        let mut query = None;
        let mut max_players = None;
        let mut port = Some(self.config.game_port);

        // Enrich with `GET /info` from the game port (`name`, `query`, `maxPlayers`, `uptime`, `battle`).
        if let Ok(info_val) = self.fetch_info_raw() {
            name = info_val
                .get("name")
                .and_then(Value::as_str)
                .map(String::from);
            query = info_val
                .get("query")
                .and_then(Value::as_str)
                .map(String::from);
            max_players = info_val
                .get("maxPlayers")
                .and_then(Value::as_u64)
                .map(|v| v as u32);
            if let Some(p) = info_val.get("port").and_then(Value::as_u64) {
                port = Some(p as u16);
            }
            if let Some(u) = info_val.get("uptime").and_then(Value::as_u64) {
                uptime = Some(u);
            }
            if let Some(battle) = info_val.get("battle").and_then(Value::as_object) {
                map = battle.get("map").and_then(Value::as_str).map(String::from);
                map_name = battle
                    .get("mapName")
                    .and_then(Value::as_str)
                    .map(String::from);
                side = battle.get("side").and_then(Value::as_str).map(String::from);
                mode = battle.get("mode").and_then(Value::as_str).map(String::from);
            }
            // Fall back to parsing `?map=...&side=...` from `query` when `battle` is omitted.
            if map.is_none() {
                if let Some(ref q) = query {
                    map = extract_query_param(q, "map");
                }
            }
            if side.is_none() {
                if let Some(ref q) = query {
                    side = extract_query_param(q, "side");
                }
            }
        }

        HostStatus {
            active: true,
            starting,
            status: status_str,
            pid,
            tick,
            started_at,
            uptime,
            name,
            map,
            map_name,
            side,
            mode,
            query,
            max_players,
            port,
            admin_port: Some(self.config.admin_port),
            players,
            bans,
            transport,
        }
    }

    /// Queries `/admin` (`{"action": "status"}`) and `GET /info` on the game port to build a
    /// [`HostJoinInfo`] for `127.0.0.1:<game_port>` (`async function joinInfo()` in the desktop host).
    ///
    /// Error strings match the desktop host verbatim:
    /// - `"Cannot manage the hosted server"` if `/admin` is unreachable or unauthorized.
    /// - `"Hosted server is still starting"` if `status != "running"`.
    /// - `"Hosted server is unavailable"` if `GET /info` fails or returns non-2xx.
    pub fn join_info(&self) -> Result<HostJoinInfo, String> {
        // Verify `/admin` reports `status == "running"` (`if (current.status !== 'running') throw Error('Hosted server is still starting')`).
        let current = self.admin(&json!({ "action": "status" }))?;
        if current.get("status").and_then(Value::as_str) != Some("running") {
            return Err("Hosted server is still starting".to_string());
        }
        let pid = current
            .get("pid")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let started_at = current.get("startedAt").and_then(Value::as_u64);

        // Fetch `GET http://127.0.0.1:<game_port>/info` (`if (!response.ok) throw Error('Hosted server is unavailable')`).
        let info_val = self
            .fetch_info_raw()
            .map_err(|_| "Hosted server is unavailable".to_string())?;
        let mut obj = info_val
            .as_object()
            .cloned()
            .ok_or_else(|| "Hosted server is unavailable".to_string())?;

        // Merge `address: "127.0.0.1"` and `port: GAME_PORT` (`{ ...await response.json(), address: '127.0.0.1', port: GAME_PORT }`).
        obj.insert(
            "address".to_string(),
            Value::String("127.0.0.1".to_string()),
        );
        obj.insert("port".to_string(), json!(self.config.game_port));
        if let Some(p) = pid {
            obj.insert("pid".to_string(), json!(p));
        }

        let join: HostJoinInfo = serde_json::from_value(Value::Object(obj))
            .map_err(|_| "Hosted server is unavailable".to_string())?;

        // Persist the attach record so subsequent startups or CLI tools know the active server details.
        let unit = self.state.lock().ok().and_then(|g| g.unit.clone());
        let _ = self.write_attach_record(&AttachRecord {
            pid,
            admin_port: self.config.admin_port,
            game_port: self.config.game_port,
            started_at,
            unit,
            kept_running: false,
        });
        if let Ok(mut guard) = self.state.lock() {
            guard.attached = Some(join.clone());
        }

        Ok(join)
    }

    /// Probes `<state_root>/operator-token`, `/admin` (`{"action": "status"}`), and `/info` on startup
    /// to attach to an already-running `bfeserver` left behind by a previous client session.
    ///
    /// Returns `Ok(Some(HostJoinInfo))` when an active running server is found and adopted,
    /// `Ok(None)` when no server is running, or `Err(message)` if a server is mid-startup or faulted.
    pub fn attach(&self) -> Result<Option<HostJoinInfo>, String> {
        // Without `<state_root>/operator-token`, no previous hosted server exists to attach to.
        if !self.token_path().is_file() {
            return Ok(None);
        }
        // Probe `/admin`; if connection fails, any previous server has already exited.
        let Ok(current) = self.admin(&json!({ "action": "status" })) else {
            return Ok(None);
        };
        let status_str = current
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if status_str != "running" {
            return Err("Hosted server is still starting".to_string());
        }
        let join = self.join_info()?;
        Ok(Some(join))
    }

    /// Starts a detached `bfeserver` instance with `options`, or attaches to the existing server if
    /// one is already running (`async function start(options)` in the desktop host).
    ///
    /// Preserves the exact validation rules, file permissions (`0600`), environment variables,
    /// 60-second readiness poll loop (120 * 500 ms), and error strings of the desktop host.
    pub fn start(&self, options: &HostOptions) -> Result<HostJoinInfo, String> {
        // Step 1: If a server is already active in `<state_root>`, attach to it instead of spawning a second `bfeserver`
        // (`const current = await status(); if (current.active) return joinInfo();` in the desktop host).
        let current = self.status();
        if current.active {
            return self.join_info();
        }

        // Step 2: Validate `options` (`if (!options || ...) throw Error('Invalid host settings')`).
        options.validate()?;

        // Step 3: Require online privacy notice consent before public hosting
        // (`if (options.mode === 'public' && !await consent()) throw Error('Accept the online privacy notice before hosting publicly.')`).
        if options.mode == HostMode::Public && !self.config.online_consent {
            return Err("Accept the online privacy notice before hosting publicly.".to_string());
        }

        // Step 4: Verify prepared content (`common/common.json` and `maps/<map>/map.json`) before spawning
        // (`if (!existsSync(path.join(content, 'common/common.json')) || !existsSync(path.join(content, 'maps', options.map, 'map.json')))` in the desktop host).
        let content = &self.config.content_root;
        if !content.join("common").join("common.json").is_file()
            || !content
                .join("maps")
                .join(&options.map)
                .join("map.json")
                .is_file()
        {
            return Err("Import the selected map from your game before hosting.".to_string());
        }
        for entry in &options.playlist {
            if let Some(map_id) = map_id_from_rotation_entry(entry) {
                if !content.join("maps").join(map_id).join("map.json").is_file() {
                    return Err(
                        "Import the selected map from your game before hosting.".to_string()
                    );
                }
            }
        }

        // Step 5: Verify that the bundled `bfeserver` executable exists
        // (`throw Error('The server runtime is missing. Install the complete desktop release.')`).
        if !self.config.executable.is_file() {
            return Err(
                "The server runtime is missing. Install the complete desktop release.".to_string(),
            );
        }
        let executable_abs =
            fs::canonicalize(&self.config.executable).unwrap_or_else(|_| self.config.executable.clone());

        ensure_dir_mode_700(&self.config.state_root)?;

        // Step 6: Write `<state_root>/config.json` with mode `0600` using strictly valid `bfeserver` config keys.
        let server_cfg = options.to_server_config_json(
            &self.config.engine_root,
            content,
            self.config.game_port,
            &self.config.master_url,
        );
        let cfg_bytes = serde_json::to_vec_pretty(&server_cfg)
            .map_err(|e| format!("Failed to serialize server config: {e}"))?;
        write_file_mode_600(&self.config_path(), &cfg_bytes)?;

        // Step 7: Open `<state_root>/hosting.log` (mode `0600`), truncating if over `LOG_ROTATE_BYTES` (8 MiB).
        let log_path = self.log_path();
        if let Ok(meta) = fs::metadata(&log_path) {
            if meta.len() > LOG_ROTATE_BYTES {
                write_file_mode_600(&log_path, b"")?;
            }
        }
        let log_stdout = open_log_append_600(&log_path)?;
        let log_stderr = log_stdout
            .try_clone()
            .map_err(|e| format!("Failed to clone hosting log descriptor: {e}"))?;

        // Step 8: Build the child environment (`BFE_STATE_DIR`, `BFE_ENGINE_DIR`, `BFE_DATA_DIR`, `PORT`).
        let mut server_env = BTreeMap::new();
        server_env.insert(
            "BFE_STATE_DIR".to_string(),
            self.config.state_root.to_string_lossy().into_owned(),
        );
        server_env.insert(
            "BFE_ENGINE_DIR".to_string(),
            self.config.engine_root.to_string_lossy().into_owned(),
        );
        server_env.insert(
            "BFE_DATA_DIR".to_string(),
            content.to_string_lossy().into_owned(),
        );
        server_env.insert("PORT".to_string(), self.config.admin_port.to_string());

        let args = vec!["--serve".to_string()];

        if let Ok(mut guard) = self.state.lock() {
            guard.starting = true;
            guard.unit = None;
        }

        // Ensure `starting` is reset when `start()` returns.
        struct StartingGuard(Arc<Mutex<ControllerInner>>);
        impl Drop for StartingGuard {
            fn drop(&mut self) {
                if let Ok(mut g) = self.0.lock() {
                    g.starting = false;
                }
            }
        }
        let _starting_guard = StartingGuard(Arc::clone(&self.state));

        // Step 9: Spawn outside via `systemd-run --user` when `launch_outside` is enabled, falling
        // back to a detached `std::process::Command` child (`if (outside()) unit = await startOutside(...)`).
        let mut unit: Option<String> = None;
        let mut child: Option<Child> = None;

        if self.config.launch_outside {
            unit = start_outside(&StartOutsideOptions {
                executable: executable_abs.clone(),
                args: args.clone(),
                env: server_env.clone(),
                log: log_path.clone(),
                systemd_run_program: self.config.systemd_run_program.clone(),
            });
            if let Ok(mut guard) = self.state.lock() {
                guard.unit = unit.clone();
            }
        }

        if unit.is_none() {
            drop(log_stdout);
            drop(log_stderr);
            let stdout_file = open_log_append_600(&log_path)?;
            let stderr_file = stdout_file
                .try_clone()
                .map_err(|e| format!("Failed to clone hosting log descriptor: {e}"))?;

            let mut cmd = Command::new(&executable_abs);
            cmd.args(&args)
                .current_dir(&self.config.state_root)
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout_file))
                .stderr(Stdio::from(stderr_file));

            for (k, v) in &server_env {
                cmd.env(k, v);
            }

            // Place the child in its own process group on Unix (`detached: true` in Node)
            // so terminal signals sent to the client do not terminate `bfeserver`.
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }

            // Use `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW` on Windows
            // so `bfeserver.exe` survives client exit without opening a console window.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(WINDOWS_HOST_CREATION_FLAGS);
            }

            let spawned = cmd
                .spawn()
                .map_err(|e| format!("Failed to start server process: {e}"))?;
            child = Some(spawned);
        }

        // Step 10: Poll `join_info()` up to `ready_poll_attempts` (120 * 500 ms = 60 s), checking
        // whether the child process or transient systemd unit exited early on each iteration
        // (`const ready = (async () => { for (let i = 0; i < 120; i++) ... })()` in the desktop host).
        for i in 0..self.config.ready_poll_attempts {
            if let Some(ref unit_name) = unit {
                if i > 0
                    && !unit_active_with_program(&self.config.systemctl_program, unit_name)
                {
                    return Err("Server exited. See the hosting log.".to_string());
                }
            }
            if let Some(ref mut proc) = child {
                match proc.try_wait() {
                    Ok(Some(status)) => {
                        let code = status.code().unwrap_or(1);
                        return Err(format!("Server exited ({code}). See the hosting log."));
                    }
                    Ok(None) => {}
                    Err(err) => {
                        return Err(format!("Server process error: {err}"));
                    }
                }
            }

            if let Ok(info) = self.join_info() {
                return Ok(info);
            }

            thread::sleep(self.config.ready_poll_interval);
        }

        Err("The server is still starting. Check its status or hosting log.".to_string())
    }

    /// Stops the hosted server gracefully via `POST /admin` `{"action": "stop"}` using the
    /// bearer token in `<state_root>/operator-token` (`if (action === 'stop') return admin({ action: 'stop' })` in the desktop host).
    ///
    /// Removes `<state_root>/attach.json` once the stop command is accepted.
    pub fn stop(&self) -> Result<Value, String> {
        // Send `{"action": "stop"}` over loopback `/admin` and clear the local attach state.
        let resp = self.admin(&json!({ "action": "stop" }))?;
        let _ = fs::remove_file(self.attach_path());
        if let Ok(mut guard) = self.state.lock() {
            guard.attached = None;
            guard.unit = None;
        }
        Ok(resp)
    }

    /// Restarts the active match on the hosted server via `POST /admin` `{"action": "restart"}`
    /// (`if (action === 'restart') return admin({ action: 'restart' })` in the desktop host).
    pub fn restart(&self) -> Result<Value, String> {
        self.admin(&json!({ "action": "restart" }))
    }

    /// Kicks a player by 64-character lowercase hex identity (`action === 'kick'` in the desktop host).
    pub fn kick(&self, id: &str, reason: &str) -> Result<Value, String> {
        self.moderation_action("kick", id, reason)
    }

    /// Bans a player for 24 hours by 64-character lowercase hex identity (`action === 'ban'` in the desktop host).
    pub fn ban(&self, id: &str, reason: &str) -> Result<Value, String> {
        self.moderation_action("ban", id, reason)
    }

    /// Removes a local ban for a 64-character lowercase hex identity (`action === 'unban'` in the desktop host).
    pub fn unban(&self, id: &str, reason: &str) -> Result<Value, String> {
        self.moderation_action("unban", id, reason)
    }

    /// Shared moderation helper porting `if (action === 'kick' || action === 'ban' || action === 'unban')`
    /// from `createHostController` in the desktop host.
    fn moderation_action(&self, action: &str, id: &str, reason: &str) -> Result<Value, String> {
        // Require a 64-character lowercase hex player digest (`!/^[a-f0-9]{64}$/.test(value.id)`).
        if !is_hex_64_lower(id) {
            return Err("Invalid player identity".to_string());
        }
        // Clip the operator reason to 240 characters (`String(value.reason || '').slice(0, 240)`).
        let clipped_reason: String = reason.chars().take(240).collect();
        let mut payload = Map::new();
        payload.insert("action".to_string(), Value::String(action.to_string()));
        payload.insert("id".to_string(), Value::String(id.to_string()));
        if action == "ban" {
            payload.insert("hours".to_string(), json!(24));
        }
        payload.insert("reason".to_string(), Value::String(clipped_reason));
        self.admin(&Value::Object(payload))
    }

    /// Returns `Some(ClosePrompt)` when the hosted server is currently active so the client shell
    /// can prompt the user before closing, or `None` when no hosted server is running
    /// (`before-quit` handler in the desktop host, lines 1060-1071).
    pub fn close_prompt(&self) -> Option<ClosePrompt> {
        if self.status().active {
            Some(ClosePrompt::new())
        } else {
            None
        }
    }

    /// Applies the user's [`CloseChoice`] when closing the client while a hosted server is running
    /// (`before-quit` handler in the desktop host, lines 1061-1071):
    /// - [`CloseChoice::KeepRunning`]: leaves `bfeserver` running in the background, updates
    ///   `<state_root>/attach.json` with `kept_running: true`, and returns `Ok(true)` (allow client exit).
    /// - [`CloseChoice::StopAndQuit`]: sends `POST /admin {"action": "stop"}` to stop the server
    ///   and returns `Ok(true)` (allow client exit).
    /// - [`CloseChoice::Cancel`]: leaves the server running and returns `Ok(false)` (abort client exit).
    pub fn apply_close_choice(&self, choice: CloseChoice) -> Result<bool, String> {
        match choice {
            CloseChoice::KeepRunning => {
                // Record `kept_running: true` in `<state_root>/attach.json` so the next launch finds the server.
                let current = self.status();
                let unit = self.state.lock().ok().and_then(|g| g.unit.clone());
                let record = AttachRecord {
                    pid: current.pid,
                    admin_port: self.config.admin_port,
                    game_port: self.config.game_port,
                    started_at: current.started_at,
                    unit,
                    kept_running: true,
                };
                self.write_attach_record(&record)?;
                Ok(true)
            }
            CloseChoice::StopAndQuit => {
                // Stop the server (`host('stop')` in the desktop host) and allow the client to exit.
                let _ = self.stop();
                Ok(true)
            }
            CloseChoice::Cancel => {
                // Cancel the close event (`event.preventDefault()` in the desktop host).
                Ok(false)
            }
        }
    }

    /// Reads `<state_root>/attach.json` if present.
    pub fn read_attach_record(&self) -> Option<AttachRecord> {
        let raw = fs::read_to_string(self.attach_path()).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Writes `<state_root>/attach.json` with mode `0600`.
    fn write_attach_record(&self, record: &AttachRecord) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(record)
            .map_err(|e| format!("Failed to serialize attach record: {e}"))?;
        write_file_mode_600(&self.attach_path(), &bytes)
    }

    /// Performs `GET http://127.0.0.1:<game_port>/info` and parses the JSON response.
    fn fetch_info_raw(&self) -> Result<Value, String> {
        let (status, body) = loopback_http_request(
            self.config.game_port,
            "GET",
            "/info",
            &[],
            None,
            self.config.http_timeout,
        )?;
        if !(200..300).contains(&status) {
            return Err(format!("HTTP {status}"));
        }
        serde_json::from_slice(&body).map_err(|e| e.to_string())
    }
}

/// Validates a 64-character lowercase hexadecimal identity (`/^[a-f0-9]{64}$/` in the desktop host).
fn is_hex_64_lower(value: &str) -> bool {
    // Check that the string is 64 ASCII lowercase hex characters (`0-9` or `a-f`).
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Extracts a single parameter from a `?k=v&...` query string.
fn extract_query_param(query: &str, key: &str) -> Option<String> {
    // Strip any leading `?` and scan `&`-delimited `key=value` pairs.
    let trimmed = query.trim_start_matches('?');
    for pair in trimmed.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key && !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Ensures `dir` exists and sets permissions to `0700` on Unix (`mkdirSync(stateRoot, { recursive: true, mode: 0o700 })`).
fn ensure_dir_mode_700(dir: &Path) -> Result<(), String> {
    // Create the directory hierarchy and restrict Unix permissions to owner rwx (`0700`).
    fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create host state directory {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| {
            format!(
                "Failed to set 0700 permissions on {}: {e}",
                dir.display()
            )
        })?;
    }
    Ok(())
}

/// Writes `content` to `path` and sets file permissions to `0600` on Unix (`mode: 0o600` in the desktop host).
fn write_file_mode_600(path: &Path, content: &[u8]) -> Result<(), String> {
    // Open `path` with create+truncate and mode `0600` on Unix so secrets and configs are owner-only.
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("Failed to open {} for writing: {e}", path.display()))?;
    file.write_all(content)
        .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;
    file.flush()
        .map_err(|e| format!("Failed to flush {}: {e}", path.display()))?;
    // Explicitly enforce `0600` even if the file already existed with broader permissions.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Opens `path` in append mode with `0600` permissions on Unix (`openSync(log, 'a', 0o600)` in the desktop host).
fn open_log_append_600(path: &Path) -> Result<File, String> {
    // Open or create the log file in append mode with `0600` permissions on Unix.
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("Failed to open hosting log {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(file)
}

/// Minimal HTTP/1.1 client over `std::net::TcpStream` for loopback `/admin` and `/info` requests.
///
/// Connects to `127.0.0.1:<port>` with `timeout`, sends the request headers and optional body,
/// and reads the response status code and body without pulling in any external HTTP crate.
fn loopback_http_request(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    // Connect to `127.0.0.1:<port>` with bounded connect, read, and write timeouts.
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let mut stream = TcpStream::connect_timeout(&addr, timeout)
        .map_err(|e| format!("connect {addr}: {e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    // Format the HTTP/1.1 request line, headers, and `Content-Length` when a body is present.
    let body_bytes = body.unwrap_or(&[]);
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n"
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if body.is_some() {
        req.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    req.push_str("\r\n");

    // Write the HTTP request headers and optional payload bytes to the loopback socket.
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("write headers: {e}"))?;
    if !body_bytes.is_empty() {
        stream
            .write_all(body_bytes)
            .map_err(|e| format!("write body: {e}"))?;
    }
    let _ = stream.flush();

    // Read the complete response until EOF (`Connection: close`) and split headers from body at `\r\n\r\n`.
    let mut raw = Vec::with_capacity(4096);
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("read response: {e}"))?;

    let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err("malformed HTTP response".to_string());
    };
    let header_str = String::from_utf8_lossy(&raw[..header_end]);
    let status_line = header_str.lines().next().unwrap_or_default();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("invalid HTTP status line: {status_line}"))?;

    let payload = raw[header_end + 4..].to_vec();
    Ok((status_code, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Creates a unique temporary directory under `std::env::temp_dir()` for a unit test.
    fn temp_test_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let dir = env::temp_dir().join(format!("bfe-host-test-{label}-{pid}-{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    /// Allocates an ephemeral loopback TCP port (`127.0.0.1:0`) for isolated controller tests.
    fn free_loopback_port() -> u16 {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).unwrap();
        listener.local_addr().unwrap().port()
    }

    /// Populates a minimal prepared content directory with `common/common.json` and `maps/nab2/map.json`.
    fn create_fake_content_dir(root: &Path) {
        let common_dir = root.join("common");
        let map_dir = root.join("maps").join("nab2");
        fs::create_dir_all(&common_dir).unwrap();
        fs::create_dir_all(&map_dir).unwrap();
        fs::write(common_dir.join("common.json"), b"{}").unwrap();
        fs::write(map_dir.join("map.json"), b"{}").unwrap();
    }

    /// Verifies `resolve_admin_port` (`18460`) and `resolve_game_port` (`18461`) against
    /// `BFE_HOST_ADMIN_PORT` and `BFE_HOST_GAME_PORT` (`const port = (name, fallback) => ...` in the desktop host).
    #[test]
    fn port_resolution_honors_env_and_bounds() {
        // Verify default ports and `BFE_HOST_ADMIN_PORT` / `BFE_HOST_GAME_PORT` overrides (the desktop host).
        let mut env_map = BTreeMap::new();
        assert_eq!(resolve_admin_port(&env_map), 18460);
        assert_eq!(resolve_game_port(&env_map), 18461);

        env_map.insert("BFE_HOST_ADMIN_PORT".to_string(), "19460".to_string());
        env_map.insert("BFE_HOST_GAME_PORT".to_string(), "19461".to_string());
        assert_eq!(resolve_admin_port(&env_map), 19460);
        assert_eq!(resolve_game_port(&env_map), 19461);

        for invalid in ["0", "65536", "-1", "abc", ""] {
            env_map.insert("BFE_HOST_ADMIN_PORT".to_string(), invalid.to_string());
            assert_eq!(resolve_admin_port(&env_map), 18460);
        }
    }

    /// Verifies `HostOptions::validate` and `HostOptions::to_server_config_json` against the
    /// validation rules in the desktop host and the config schema in `bfe-server/src/config.rs`.
    #[test]
    fn host_options_validation_and_config_mapping_match_server_schema() {
        // Verify validation rules and strict `config.json` key mapping (`createHostController.start` in the desktop host).
        let mut opts = HostOptions::default();
        assert!(opts.validate().is_ok());

        opts.name = "   ".to_string();
        assert_eq!(opts.validate().unwrap_err(), "Invalid host settings");
        opts.name = "x".repeat(81);
        assert_eq!(opts.validate().unwrap_err(), "Invalid host settings");
        opts.name = "Echo Base".to_string();

        opts.max_players = 1;
        assert_eq!(opts.validate().unwrap_err(), "Invalid host settings");
        opts.max_players = 65;
        assert_eq!(opts.validate().unwrap_err(), "Invalid host settings");
        opts.max_players = 16;

        opts.map = "../bad".to_string();
        assert_eq!(opts.validate().unwrap_err(), "Invalid host settings");
        opts.map = "nab2".to_string();

        opts.side = "rebels".to_string();
        assert_eq!(opts.validate().unwrap_err(), "Invalid host settings");
        opts.side = "rep".to_string();

        let cfg_json = opts.to_server_config_json(
            Path::new("/opt/example/engine"),
            Path::new("/opt/example/content"),
            18461,
            DEFAULT_MASTER_URL,
        );
        let obj = cfg_json.as_object().unwrap();
        let expected_keys = [
            "name",
            "engine",
            "data",
            "rotation",
            "maxPlayers",
            "onlineNoticeAccepted",
            "master",
            "relayPolicy",
            "honorGlobalBans",
            "gamePort",
        ];
        assert_eq!(obj.len(), expected_keys.len());
        for k in expected_keys {
            assert!(obj.contains_key(k), "missing expected key {k}");
        }
        assert_eq!(obj["name"], "Echo Base");
        assert_eq!(obj["onlineNoticeAccepted"], false);
        assert_eq!(obj["relayPolicy"], "never");
        assert_eq!(obj["rotation"], json!(["swbf1/nab2:rep:conquest"]));
    }

    /// Verifies that `HostController::start` returns the exact error strings from the desktop host
    /// when online consent, prepared map data, or the `bfeserver` binary is missing.
    #[test]
    fn start_enforces_consent_content_and_binary_errors() {
        // Verify exact error strings for missing consent, missing map, and missing binary (`tests/desktop-host.mjs`).
        let dir = temp_test_dir("validation");
        let state_root = dir.join("state");
        let content_root = dir.join("content");
        let fake_bin = dir.join("bfeserver");

        let mut cfg = HostControllerConfig::new(state_root.clone(), content_root.clone());
        cfg.executable = fake_bin.clone();
        cfg.online_consent = false;
        let controller = HostController::new(cfg).unwrap();

        // Public mode without consent fails immediately.
        let mut pub_opts = HostOptions {
            mode: HostMode::Public,
            ..HostOptions::default()
        };
        assert_eq!(
            controller.start(&pub_opts).unwrap_err(),
            "Accept the online privacy notice before hosting publicly."
        );

        // LAN mode with missing prepared map fails with the exact map import message.
        pub_opts.mode = HostMode::Lan;
        assert_eq!(
            controller.start(&pub_opts).unwrap_err(),
            "Import the selected map from your game before hosting."
        );

        // Create content; now missing server binary is reported.
        create_fake_content_dir(&content_root);
        assert_eq!(
            controller.start(&pub_opts).unwrap_err(),
            "The server runtime is missing. Install the complete desktop release."
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Verifies `ClosePrompt` and `CloseChoice` against the `before-quit` dialog in the desktop host.
    #[test]
    fn close_prompt_matches_host_strings() {
        // Verify close dialog copy and button indices from the desktop host (`before-quit`).
        let prompt = ClosePrompt::new();
        assert_eq!(prompt.title, "Hosted server is running");
        assert_eq!(
            prompt.message,
            "Other players can keep playing after you close the app."
        );
        assert_eq!(
            prompt.buttons,
            ["Keep server running", "Stop server and quit", "Cancel"]
        );
        assert_eq!(prompt.choice_for_index(0), CloseChoice::KeepRunning);
        assert_eq!(prompt.choice_for_index(1), CloseChoice::StopAndQuit);
        assert_eq!(prompt.choice_for_index(2), CloseChoice::Cancel);
    }

    /// Verifies `start_outside` and `unit_active_with_program` (`startOutside` and `unitActive` in the desktop host).
    #[cfg(unix)]
    #[test]
    fn start_outside_builds_systemd_user_unit_and_checks_active() {
        // Verify `start_outside` and `unit_active_with_program` (`tests/desktop-host.mjs`).
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_test_dir("outside");
        let args_out = dir.join("systemd-args.txt");
        let fake_systemd = dir.join("fake-systemd-run.sh");
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nexit 0\n",
            args_out.display()
        );
        fs::write(&fake_systemd, script.as_bytes()).unwrap();
        fs::set_permissions(&fake_systemd, fs::Permissions::from_mode(0o755)).unwrap();

        let mut env_map = BTreeMap::new();
        env_map.insert("PORT".to_string(), "18460".to_string());
        let log_path = dir.join("hosting.log");
        let unit = start_outside(&StartOutsideOptions {
            executable: PathBuf::from("/opt/example/bfeserver"),
            args: vec!["--serve".to_string()],
            env: env_map,
            log: log_path.clone(),
            systemd_run_program: fake_systemd.to_string_lossy().into_owned(),
        })
        .expect("systemd-run should return unit name");

        assert!(unit.starts_with("battlefront-eternal-server-"));
        let recorded = fs::read_to_string(&args_out).unwrap();
        assert!(recorded.contains("--user"));
        assert!(recorded.contains("--collect"));
        assert!(recorded.contains(&format!("--unit={unit}")));
        assert!(recorded.contains(&format!(
            "--property=StandardOutput=append:{}",
            log_path.display()
        )));
        assert!(recorded.contains("--setenv=PORT=18460"));
        assert!(recorded.contains("/opt/example/bfeserver"));
        assert!(recorded.contains("--serve"));

        assert!(unit_active_with_program("true", &unit));
        assert!(!unit_active_with_program("false", &unit));

        let _ = fs::remove_dir_all(&dir);
    }

    /// Verifies the full `HostController` lifecycle (`start`, `status`, `attach`, `kick`, `ban`,
    /// `unban`, `restart`, `apply_close_choice`, and `stop`) against a fake server process.
    #[cfg(unix)]
    #[test]
    fn fake_server_lifecycle_spawn_attach_moderation_close_and_stop() {
        // End-to-end test using a fake Python HTTP server script acting as `bfeserver --serve`.
        // Covers:
        // 1. Detached spawn, `config.json` (0600), `hosting.log` (0600) rotation, and readiness polling.
        // 2. Second `HostController` attaching via `attach()` and `start()` without spawning a second process.
        // 3. `status()`, `restart()`, `kick()`, `ban()`, and `unban()` moderation validation.
        // 4. `CloseChoice::KeepRunning` writing `attach.json` and `stop()` shutting down the server and logging unregistration.
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_test_dir("lifecycle");
        let state_root = dir.join("state");
        let content_root = dir.join("content");
        create_fake_content_dir(&content_root);

        let admin_port = free_loopback_port();
        let game_port = free_loopback_port();

        // Write a tiny Python fake `bfeserver` that reads `BFE_STATE_DIR` and `PORT`, writes `operator-token`,
        // serves `/admin` on `PORT` and `/info` on `gamePort` from `config.json`, logs every command to stdout,
        // and exits cleanly when `{"action": "stop"}` is received.
        let fake_server = dir.join("fake-bfeserver.py");
        let py_script = r#"#!/usr/bin/env python3
import http.server
import json
import os
import sys
import threading
import time

state_dir = os.environ["BFE_STATE_DIR"]
admin_port = int(os.environ["PORT"])
with open(os.path.join(state_dir, "config.json"), "r", encoding="utf-8") as f:
    cfg = json.load(f)
game_port = int(cfg["gamePort"])
token = "a" * 64
token_path = os.path.join(state_dir, "operator-token")
with open(token_path, "w", encoding="utf-8") as f:
    f.write(token)
os.chmod(token_path, 0o600)

started_at = int(time.time() * 1000)
stop_event = threading.Event()
commands = []

class AdminHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass
    def do_POST(self):
        if self.path != "/admin":
            self.send_response(404)
            self.end_headers()
            return
        auth = self.headers.get("Authorization", "")
        if auth != f"Bearer {token}":
            self.send_response(401)
            self.end_headers()
            return
        length = int(self.headers.get("Content-Length", "0"))
        body = json.loads(self.rfile.read(length).decode("utf-8"))
        action = body.get("action")
        commands.append(body)
        print(f"admin_action:{action}", flush=True)
        if action == "status":
            resp = {
                "status": "running",
                "tick": 120,
                "pid": os.getpid(),
                "startedAt": started_at,
                "bans": [],
                "players": [{"id": "b" * 64, "name": "PilotOne", "rtt": 12.5}],
            }
        elif action == "stop":
            resp = {"ok": True}
            print("bfeserver stopping: unregistering service and closing sessions", flush=True)
            stop_event.set()
        else:
            resp = {"ok": True, "received": body}
        payload = json.dumps(resp).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

class InfoHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass
    def do_GET(self):
        if self.path != "/info":
            self.send_response(404)
            self.end_headers()
            return
        resp = {
            "id": "c" * 64,
            "name": cfg["name"],
            "query": "?map=nab2&side=rep&ai=classic&seed=1&match=" + ("d" * 32),
            "fingerprint": "bfe-content-sha256-" + ("e" * 64),
            "content": "bfe-content-sha256-" + ("e" * 64),
            "players": 1,
            "maxPlayers": cfg["maxPlayers"],
            "port": game_port,
            "uptime": 5,
            "battle": {
                "map": "nab2",
                "mapName": "Naboo: Theed",
                "side": "rep",
                "era": "cw",
                "mode": "conquest",
            },
        }
        payload = json.dumps(resp).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

admin_srv = http.server.ThreadingHTTPServer(("127.0.0.1", admin_port), AdminHandler)
info_srv = http.server.ThreadingHTTPServer(("127.0.0.1", game_port), InfoHandler)
threading.Thread(target=admin_srv.serve_forever, daemon=True).start()
threading.Thread(target=info_srv.serve_forever, daemon=True).start()
print("bfeserver service started; control menu: bfeserver", flush=True)

while not stop_event.is_set():
    time.sleep(0.05)

admin_srv.shutdown()
info_srv.shutdown()
print("bfeserver stopped", flush=True)
"#;
        fs::write(&fake_server, py_script.as_bytes()).unwrap();
        fs::set_permissions(&fake_server, fs::Permissions::from_mode(0o755)).unwrap();

        // Pre-populate a >8 MiB log file (`LOG_ROTATE_BYTES + 1024`) to verify log truncation on `start()`
        // (`if (existsSync(log) && statSync(log).size > 8388608) writeFileSync(log, '')` in the desktop host).
        fs::create_dir_all(&state_root).unwrap();
        let log_path = state_root.join("hosting.log");
        fs::write(&log_path, vec![b'x'; (LOG_ROTATE_BYTES as usize) + 1024]).unwrap();

        // Configure `HostController` to run against the temporary state directory, content root, and ports.
        let mut cfg = HostControllerConfig::new(state_root.clone(), content_root.clone());
        cfg.executable = fake_server.clone();
        cfg.admin_port = admin_port;
        cfg.game_port = game_port;
        cfg.launch_outside = false;
        cfg.ready_poll_attempts = 40;
        cfg.ready_poll_interval = Duration::from_millis(50);

        // Construct the controller and LAN `HostOptions` (`mode: Lan`, `map: "nab2"`, `side: "rep"`, `max_players: 12`).
        let controller = HostController::new(cfg.clone()).unwrap();
        let opts = HostOptions {
            name: "Test LAN Host".to_string(),
            mode: HostMode::Lan,
            map: "nab2".to_string(),
            side: "rep".to_string(),
            max_players: 12,
            relay: RelayPolicy::Auto,
            ..HostOptions::default()
        };

        // Start the detached fake server and verify `joinInfo()` returns `127.0.0.1:<game_port>` and the child PID.
        let join = controller.start(&opts).expect("fake server should start");
        assert_eq!(join.address, "127.0.0.1");
        assert_eq!(join.port, game_port);
        assert_eq!(join.name, "Test LAN Host");
        assert_eq!(join.max_players, 12);
        let first_pid = join.pid.expect("pid should be reported");

        // Verify `config.json` and `hosting.log` permissions (`0600`) and log truncation.
        let cfg_mode = fs::metadata(controller.config_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let log_mode = fs::metadata(controller.log_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(cfg_mode, 0o600);
        assert_eq!(log_mode, 0o600);
        assert!(fs::metadata(controller.log_path()).unwrap().len() < LOG_ROTATE_BYTES);

        // Verify `status()` reports running state, player list, map, and PID.
        let st = controller.status();
        assert!(st.active);
        assert_eq!(st.status.as_deref(), Some("running"));
        assert_eq!(st.pid, Some(first_pid));
        assert_eq!(st.map.as_deref(), Some("nab2"));
        assert_eq!(st.players.len(), 1);
        assert_eq!(st.players[0].name, "PilotOne");

        // Simulate client exit with `CloseChoice::KeepRunning` (`before-quit` in the desktop host).
        assert!(controller.close_prompt().is_some());
        assert!(controller.apply_close_choice(CloseChoice::KeepRunning).unwrap());
        let attach_rec = controller
            .read_attach_record()
            .expect("attach record should be written");
        assert!(attach_rec.kept_running);
        assert_eq!(attach_rec.pid, Some(first_pid));

        // Create a second `HostController` representing a fresh client startup; verify `attach()`
        // and `start()` reuse the running server PID without spawning a second process.
        let mut second_cfg = cfg.clone();
        second_cfg.executable = dir.join("nonexistent-binary");
        let second_controller = HostController::new(second_cfg).unwrap();
        let attached = second_controller
            .attach()
            .expect("attach should succeed")
            .expect("running server should be found");
        assert_eq!(attached.pid, Some(first_pid));

        let joined_again = second_controller
            .start(&opts)
            .expect("start() on active server should attach instead of spawning");
        assert_eq!(joined_again.pid, Some(first_pid));

        // Verify moderation actions (`kick`, `ban`, `unban`, `restart`) and 64-hex player ID validation
        // (`if (action === 'kick' || action === 'ban' || action === 'unban')` in the desktop host).
        let valid_id = "b".repeat(64);
        assert_eq!(
            second_controller.kick("invalid-id", "reason").unwrap_err(),
            "Invalid player identity"
        );
        // Valid 64-character lowercase hex player IDs succeed over `/admin`.
        assert!(second_controller.kick(&valid_id, "afk").is_ok());
        assert!(second_controller.ban(&valid_id, "griefing").is_ok());
        assert!(second_controller.unban(&valid_id, "").is_ok());
        assert!(second_controller.restart().is_ok());

        // Stop the server via `POST /admin {"action": "stop"}` and verify `<state_root>/hosting.log`
        // records the graceful shutdown and unregistration output.
        second_controller.stop().expect("stop should succeed");
        thread::sleep(Duration::from_millis(150));
        // Confirm `status()` reports `active: false` and `attach.json` was removed after `stop()`.
        assert!(!second_controller.status().active);
        assert!(second_controller.read_attach_record().is_none());
        let log_contents = fs::read_to_string(controller.log_path()).unwrap();
        assert!(log_contents.contains("unregistering service"));
        assert!(log_contents.contains("bfeserver stopped"));

        // Clean up temporary test directory.
        let _ = fs::remove_dir_all(&dir);
    }

    /// Verifies that `HostController::start` reports the exact error strings from the desktop host
    /// when the child process exits prematurely or when readiness polling times out.
    #[cfg(unix)]
    #[test]
    fn early_exit_and_readiness_timeout_report_exact_errors() {
        // Verify exact error strings when the child exits early (`Server exited (7). See the hosting log.`)
        // or never becomes ready (`The server is still starting. Check its status or hosting log.`).
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_test_dir("errors");
        let state_root = dir.join("state");
        let content_root = dir.join("content");
        create_fake_content_dir(&content_root);

        // 1. Early exit with code 7.
        let exit_bin = dir.join("exit7.sh");
        fs::write(&exit_bin, b"#!/bin/sh\nexit 7\n").unwrap();
        fs::set_permissions(&exit_bin, fs::Permissions::from_mode(0o755)).unwrap();

        let mut cfg = HostControllerConfig::new(state_root.clone(), content_root.clone());
        cfg.executable = exit_bin;
        cfg.admin_port = free_loopback_port();
        cfg.game_port = free_loopback_port();
        cfg.launch_outside = false;
        cfg.ready_poll_attempts = 10;
        cfg.ready_poll_interval = Duration::from_millis(30);

        let controller = HostController::new(cfg.clone()).unwrap();
        let err = controller.start(&HostOptions::default()).unwrap_err();
        assert_eq!(err, "Server exited (7). See the hosting log.");

        // 2. Readiness timeout (child stays alive briefly without opening `/admin` or `/info`).
        let sleep_bin = dir.join("sleep.sh");
        fs::write(&sleep_bin, b"#!/bin/sh\nsleep 1\n").unwrap();
        fs::set_permissions(&sleep_bin, fs::Permissions::from_mode(0o755)).unwrap();

        cfg.executable = sleep_bin;
        cfg.ready_poll_attempts = 3;
        cfg.ready_poll_interval = Duration::from_millis(30);
        cfg.http_timeout = Duration::from_millis(20);
        let controller2 = HostController::new(cfg).unwrap();
        let err2 = controller2.start(&HostOptions::default()).unwrap_err();
        assert_eq!(
            err2,
            "The server is still starting. Check its status or hosting log."
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
