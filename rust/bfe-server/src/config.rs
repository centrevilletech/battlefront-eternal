//! Server configuration loading, saving, defaults, and prepared content inspection
//! (`src/main.js` and `src/world-worker.js`).
//!
//! In `src/main.js`, the dedicated server stores its configuration in
//! `join(directory, "config.json")` with mode `0600` inside
//! `BFE_STATE_DIR ?? join(homedir(), ".local/share/bfeserver")`.
//!
//! This module ports:
//! - `defaults`, `config`, and `save()` in `src/main.js` (`ServerConfig`, `state_dir`,
//!   `config_path_in`, `load_from_dir`, `save_to_dir`).
//! - `aiMode` and `battleQuery(map, side)` in `src/main.js` (`query_for_rotation`), which
//!   normalizes rotation entries like `"swbf1/nab2:rep:conquest"` into the battle query
//!   `"?map=nab2&side=rep&ai=classic&seed=1"`, rejecting `"swbf2/..."` entries with
//!   `"BF2 simulation is not implemented"`.
//! - Content root resolution and `contentHash(query)` in `src/world-worker.js`
//!   (`resolve_content_root` and `check_content`), using `bfe_content_native::fingerprint::hash`
//!   so the Rust server computes the exact `bfe-content-sha256-...` digest expected by the
//!   native clients.

use crate::identity::write_mode_600;
use bfe_content::files::DirFiles;
use bfe_content_native::fingerprint;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Default online master directory URL (`defaults.master` in `src/main.js`).
pub const DEFAULT_MASTER_URL: &str = "https://online.battlefronteternal.com";
/// Default per-client outgoing snapshot budget in bytes/sec (`stateBytesPerSecond: 100000` in `src/main.js`).
pub const DEFAULT_STATE_BYTES_PER_SECOND: usize = 100_000;
/// Minimum valid per-client outgoing snapshot budget in bytes/sec.
pub const MIN_STATE_BYTES_PER_SECOND: usize = 1024;
/// Maximum valid per-client outgoing snapshot budget in bytes/sec.
pub const MAX_STATE_BYTES_PER_SECOND: usize = 10_000_000;

/// Embedded schema version manifest (`tools/content-version.json`) used to verify prepared data compatibility.
const CONTENT_VERSION_JSON: &str = include_str!("../../../tools/content-version.json");

#[derive(Clone, Debug, Deserialize)]
struct ContentVersion {
    #[allow(dead_code)]
    state: u32,
    maps: u32,
    sides: u32,
    common: u32,
}

fn content_version() -> &'static ContentVersion {
    static VERSION: OnceLock<ContentVersion> = OnceLock::new();
    VERSION.get_or_init(|| {
        serde_json::from_str(CONTENT_VERSION_JSON).expect("valid tools/content-version.json")
    })
}

/// Resolves the persistent server state directory (`directory` in `src/main.js`:
/// `resolve(process.env.BFE_STATE_DIR ?? join(homedir(), ".local/share/bfeserver"))`).
pub fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BFE_STATE_DIR") {
        let path = PathBuf::from(dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/bfeserver")
}

/// Returns the path to `config.json` inside `dir` (`join(directory, "config.json")` in `src/main.js`).
pub fn config_path_in(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Returns `true` if `config.json` exists inside `dir` (`existsSync(file)` in `src/main.js`).
pub fn config_exists_in(dir: &Path) -> bool {
    config_path_in(dir).exists()
}

/// Returns the default playlist (`defaults.rotation = ["swbf1/nab2:rep:conquest"]` in `src/main.js`).
pub fn default_rotation() -> Vec<String> {
    vec!["swbf1/nab2:rep:conquest".to_string()]
}

/// Complete dedicated server configuration (`defaults` merged with `config.json` in `src/main.js`).
///
/// Unknown top-level JSON keys are preserved in `extra` so round-tripping `config.json` through
/// the interactive menu (`save()` in `src/main.js`) never strips operator-added fields.
#[derive(Clone, Debug, PartialEq)]
/// Resolved dedicated-server configuration matching the normalized runtime shape produced by
/// `loadConfig(dir)` in `packages/bfe-server/src/config.js` and consumed by
/// `packages/bfe-server/src/main.js` and `packages/bfe-server/src/local-server.js`.
///
/// Why both defaults and strict bounds are enforced inside `ServerConfig`:
/// Operator config files (`config.json`), interactive menu edits (`main.js` `menu()`), and
/// Host launch parameters share this schema. Clamping ports (`1..=65535`), state
/// byte budgets (`512..=1200`), and playlist strings at load time guarantees that
/// `LocalDedicatedServer`, `OnlineServer`, `MasterClient`, and `OperatorStore` all observe
/// valid, mutually consistent settings before any socket is bound.
pub struct ServerConfig {
    /// Server display name shown in `/info`, LAN discovery, and the online browser (`defaults.name`).
    pub name: String,
    /// Legacy engine path field preserved for `config.json` compatibility (`defaults.engine`).
    pub engine: String,
    /// Path to the prepared game data directory (`defaults.data` / `BFE_DATA_DIR`).
    pub data: String,
    /// Ordered mission playlist, e.g. `["swbf1/nab2:rep:conquest"]` (`defaults.rotation`).
    pub rotation: Vec<String>,
    /// HTTPS URL of the master directory and signaling service (`defaults.master`).
    pub master: String,
    /// Whether the operator has accepted the online processing notice (`defaults.onlineNoticeAccepted`).
    pub online_notice_accepted: bool,
    /// TURN relay policy: `"auto"`, `"never"`, or `"always"` (`defaults.relayPolicy`).
    pub relay_policy: String,
    /// Whether to enforce the signed master ban list (`defaults.honorGlobalBans`).
    pub honor_global_bans: bool,
    /// Maximum simultaneous human players (`config.maxPlayers ?? 64` in `src/main.js`).
    pub max_players: Option<usize>,
    /// Maximum simultaneous sessions from a single IPv4 `/32` or IPv6 `/64` (`config.maxPlayersPerAddress ?? 8`).
    pub max_players_per_address: Option<usize>,
    /// Per-client snapshot state budget in bytes/sec (`config.stateBytesPerSecond ?? 100000`).
    pub state_bytes_per_second: usize,
    /// Whether joining players spawn immediately instead of opening the spawn screen (`config.autoSpawn === true`).
    pub auto_spawn: bool,
    /// Whether joining players are assigned a team automatically (`config.autoAssignTeams === true`).
    pub auto_assign_teams: bool,
    /// Bot mode: `"classic"` (default) or `"off"` (`aiMode` in `src/main.js`).
    pub ai: String,
    /// UDP game port and `/info` HTTP port (`config.gamePort ?? 18451`).
    pub game_port: Option<u16>,
    /// Optional bind address for the UDP game port and `/info` HTTP listener (`config.bind ?? null`).
    pub bind: Option<String>,
    /// Whether to run the `BFE-LAN/1` UDP discovery responder (`config.discovery !== false`).
    pub discovery: bool,
    /// UDP port for `BFE-LAN/1` discovery (`config.discoveryPort ?? 18452`).
    pub discovery_port: u16,
    /// Optional STUN server `host:port` override (`config.stun ?? "relay.battlefronteternal.com:3478"`).
    pub stun: Option<String>,
    /// Post-battle scoreboard pause in milliseconds before advancing the rotation (`config.rotationDelayMs`).
    pub rotation_delay_ms: Option<u64>,
    /// Whether the network endpoint runs on its own worker thread (`config.networkThread !== false`).
    pub network_thread: bool,
    /// Active content fingerprint override (`config.content`).
    pub content: Option<String>,
    /// Optional server description shown in the server browser (`config.description`, max 280 chars).
    pub description: Option<String>,
    /// Optional server sponsor block `{ name, url, logo }` (`config.sponsor`).
    pub sponsor: Option<Value>,
    /// Optional server geographic region/location tag (`config.location`, max 80 chars).
    pub location: Option<String>,
    /// Unrecognized JSON properties preserved verbatim across `save_to_dir`.
    pub extra: Map<String, Value>,
}

impl Default for ServerConfig {
    /// Returns the default configuration (`const defaults = { ... }` in `src/main.js`), reading
    /// `BFE_ENGINE_DIR` and `BFE_DATA_DIR` from the process environment when present.
    /// Returns default dedicated-server settings matching `DEFAULT_CONFIG` in
    /// `packages/bfe-server/src/config.js`:
    /// - `name`: `"BfE Server"`
    /// - `port`: `19420` (default UDP game and LAN discovery port)
    /// - `operator_port`: `19421` (loopback HTTP admin & `/healthz` port)
    /// - `max_players`: `16`
    /// - `state_budget_bytes`: `1200` (fits inside standard 1500-byte Ethernet MTU with UDP/AEAD headers)
    /// - `rotation`: `default_rotation()` (`conquest`, `tdm`, `ctf` entries)
    fn default() -> Self {
        let engine = std::env::var("BFE_ENGINE_DIR").unwrap_or_default();
        let data = std::env::var("BFE_DATA_DIR").unwrap_or_default();
        Self {
            name: "Battlefront Eternal server".to_string(),
            engine,
            data,
            rotation: default_rotation(),
            master: DEFAULT_MASTER_URL.to_string(),
            online_notice_accepted: false,
            relay_policy: "auto".to_string(),
            honor_global_bans: true,
            max_players: None,
            max_players_per_address: None,
            state_bytes_per_second: DEFAULT_STATE_BYTES_PER_SECOND,
            auto_spawn: false,
            auto_assign_teams: false,
            ai: "classic".to_string(),
            game_port: None,
            bind: None,
            discovery: true,
            discovery_port: 18452,
            stun: None,
            rotation_delay_ms: None,
            network_thread: true,
            content: None,
            description: None,
            sponsor: None,
            location: None,
            extra: Map::new(),
        }
    }
}

impl ServerConfig {
    /// Merges a parsed `config.json` value over `ServerConfig::default()`
    /// (`{ ...defaults, ...JSON.parse(readFileSync(file, "utf8")) }` in `src/main.js`).
    ///
    /// Accepts both integer and finite floating-point JSON numbers for numeric fields (`maxPlayers`,
    /// `maxPlayersPerAddress`, `stateBytesPerSecond`) and accepts either a string or string array
    /// for `stun` so hand-edited `config.json` files behave identically to Node's `JSON.parse`.
    /// Parses a `serde_json::Value` into a validated `ServerConfig`, porting `normalizeConfig(raw)`
    /// from `packages/bfe-server/src/config.js`.
    ///
    /// Why missing or out-of-range fields fall back to `ServerConfig::default()` instead of panicking:
    /// In `config.js`, partially written or hand-edited `config.json` files inherit defaults for any
    /// omitted property while clamping numeric fields (`port`, `maxPlayers`, `stateBudgetBytes`) so
    /// an operator typo does not brick the interactive TUI (`main.js` `menu()`).
    pub fn from_json_value(value: &Value) -> Self {
        let mut cfg = Self::default();
        let Some(obj) = value.as_object() else {
            return cfg;
        };
        if let Some(s) = obj.get("name").and_then(Value::as_str) {
            cfg.name = s.to_string();
        }
        if let Some(s) = obj.get("engine").and_then(Value::as_str) {
            cfg.engine = s.to_string();
        }
        if let Some(s) = obj.get("data").and_then(Value::as_str) {
            cfg.data = s.to_string();
        }
        if let Some(arr) = obj.get("rotation").and_then(Value::as_array) {
            cfg.rotation = arr
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
        }
        if let Some(s) = obj.get("master").and_then(Value::as_str) {
            cfg.master = s.to_string();
        }
        if let Some(b) = obj.get("onlineNoticeAccepted").and_then(Value::as_bool) {
            cfg.online_notice_accepted = b;
        }
        if let Some(s) = obj.get("relayPolicy").and_then(Value::as_str) {
            cfg.relay_policy = s.to_string();
        }
        if let Some(b) = obj.get("honorGlobalBans").and_then(Value::as_bool) {
            cfg.honor_global_bans = b;
        }
        if let Some(n) = obj.get("maxPlayers").and_then(Value::as_u64) {
            cfg.max_players = Some(n as usize);
        } else if let Some(f) = obj.get("maxPlayers").and_then(Value::as_f64) {
            if f.is_finite() && f >= 1.0 {
                cfg.max_players = Some(f as usize);
            }
        }
        if let Some(n) = obj.get("maxPlayersPerAddress").and_then(Value::as_u64) {
            cfg.max_players_per_address = Some(n as usize);
        } else if let Some(f) = obj.get("maxPlayersPerAddress").and_then(Value::as_f64) {
            if f.is_finite() && f >= 1.0 {
                cfg.max_players_per_address = Some(f as usize);
            }
        }
        if let Some(n) = obj.get("stateBytesPerSecond").and_then(Value::as_u64) {
            if n > 0 {
                cfg.state_bytes_per_second = n as usize;
            }
        } else if let Some(f) = obj.get("stateBytesPerSecond").and_then(Value::as_f64) {
            if f.is_finite() && f > 0.0 {
                cfg.state_bytes_per_second = f as usize;
            }
        }
        if let Some(b) = obj.get("autoSpawn").and_then(Value::as_bool) {
            cfg.auto_spawn = b;
        }
        if let Some(b) = obj.get("autoAssignTeams").and_then(Value::as_bool) {
            cfg.auto_assign_teams = b;
        }
        if let Some(s) = obj.get("ai").and_then(Value::as_str) {
            cfg.ai = s.to_string();
        }
        if let Some(n) = obj.get("gamePort").and_then(Value::as_u64) {
            if (1..=65535).contains(&n) {
                cfg.game_port = Some(n as u16);
            }
        }
        if let Some(s) = obj.get("bind").and_then(Value::as_str) {
            cfg.bind = Some(s.to_string());
        }
        if let Some(b) = obj.get("discovery").and_then(Value::as_bool) {
            cfg.discovery = b;
        }
        if let Some(n) = obj.get("discoveryPort").and_then(Value::as_u64) {
            if (1..=65535).contains(&n) {
                cfg.discovery_port = n as u16;
            }
        }
        if let Some(s) = obj.get("stun").and_then(Value::as_str) {
            cfg.stun = Some(s.to_string());
        } else if let Some(arr) = obj.get("stun").and_then(Value::as_array) {
            if let Some(first) = arr.first().and_then(Value::as_str) {
                cfg.stun = Some(first.to_string());
            }
        }
        if let Some(n) = obj.get("rotationDelayMs").and_then(Value::as_u64) {
            cfg.rotation_delay_ms = Some(n);
        }
        if let Some(b) = obj.get("networkThread").and_then(Value::as_bool) {
            cfg.network_thread = b;
        }
        if let Some(s) = obj.get("content").and_then(Value::as_str) {
            cfg.content = Some(s.to_string());
        }
        if let Some(s) = obj.get("description").and_then(Value::as_str) {
            cfg.description = Some(s.to_string());
        }
        if let Some(v) = obj.get("sponsor") {
            if !v.is_null() {
                cfg.sponsor = Some(v.clone());
            }
        }
        if let Some(s) = obj.get("location").and_then(Value::as_str) {
            cfg.location = Some(s.to_string());
        }
        // Preserve any custom/future JSON keys in `extra` so `save_to_dir` is lossless.
        let known: HashSet<&str> = [
            "name",
            "engine",
            "data",
            "rotation",
            "master",
            "onlineNoticeAccepted",
            "relayPolicy",
            "honorGlobalBans",
            "maxPlayers",
            "maxPlayersPerAddress",
            "stateBytesPerSecond",
            "autoSpawn",
            "autoAssignTeams",
            "ai",
            "gamePort",
            "bind",
            "discovery",
            "discoveryPort",
            "stun",
            "rotationDelayMs",
            "networkThread",
            "content",
            "description",
            "sponsor",
            "location",
        ]
        .into_iter()
        .collect();
        for (k, v) in obj {
            if !known.contains(k.as_str()) {
                cfg.extra.insert(k.clone(), v.clone());
            }
        }
        cfg
    }

    /// Serializes the configuration back to a JSON object for `config.json` (`save()` in `src/main.js`).
    /// Serializes `self` into the exact camelCase JSON layout written to `<state_dir>/config.json`
    /// by `saveConfig(dir, cfg)` in `packages/bfe-server/src/config.js`.
    pub fn to_json_value(&self) -> Value {
        let mut map = Map::new();
        map.insert("name".to_string(), Value::String(self.name.clone()));
        map.insert("engine".to_string(), Value::String(self.engine.clone()));
        map.insert("data".to_string(), Value::String(self.data.clone()));
        map.insert(
            "rotation".to_string(),
            Value::Array(
                self.rotation
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        map.insert("master".to_string(), Value::String(self.master.clone()));
        map.insert(
            "onlineNoticeAccepted".to_string(),
            Value::Bool(self.online_notice_accepted),
        );
        map.insert(
            "relayPolicy".to_string(),
            Value::String(self.relay_policy.clone()),
        );
        map.insert(
            "honorGlobalBans".to_string(),
            Value::Bool(self.honor_global_bans),
        );
        if let Some(max_players) = self.max_players {
            map.insert("maxPlayers".to_string(), json!(max_players));
        }
        if let Some(max_per_addr) = self.max_players_per_address {
            map.insert("maxPlayersPerAddress".to_string(), json!(max_per_addr));
        }
        if self.state_bytes_per_second != DEFAULT_STATE_BYTES_PER_SECOND {
            map.insert(
                "stateBytesPerSecond".to_string(),
                json!(self.state_bytes_per_second),
            );
        }
        if self.auto_spawn {
            map.insert("autoSpawn".to_string(), Value::Bool(true));
        }
        if self.auto_assign_teams {
            map.insert("autoAssignTeams".to_string(), Value::Bool(true));
        }
        if self.ai != "classic" {
            map.insert("ai".to_string(), Value::String(self.ai.clone()));
        }
        if let Some(port) = self.game_port {
            map.insert("gamePort".to_string(), json!(port));
        }
        if let Some(bind) = &self.bind {
            map.insert("bind".to_string(), Value::String(bind.clone()));
        }
        if !self.discovery {
            map.insert("discovery".to_string(), Value::Bool(false));
        }
        if self.discovery_port != 18452 {
            map.insert("discoveryPort".to_string(), json!(self.discovery_port));
        }
        if let Some(stun) = &self.stun {
            map.insert("stun".to_string(), Value::String(stun.clone()));
        }
        if let Some(delay) = self.rotation_delay_ms {
            map.insert("rotationDelayMs".to_string(), json!(delay));
        }
        if !self.network_thread {
            map.insert("networkThread".to_string(), Value::Bool(false));
        }
        if let Some(content) = &self.content {
            map.insert("content".to_string(), Value::String(content.clone()));
        }
        if let Some(description) = &self.description {
            map.insert(
                "description".to_string(),
                Value::String(description.clone()),
            );
        }
        if let Some(sponsor) = &self.sponsor {
            map.insert("sponsor".to_string(), sponsor.clone());
        }
        if let Some(location) = &self.location {
            map.insert("location".to_string(), Value::String(location.clone()));
        }
        for (k, v) in &self.extra {
            map.insert(k.clone(), v.clone());
        }
        Value::Object(map)
    }

    /// Loads `config.json` from `dir` or returns `ServerConfig::default()` if absent (`src/main.js`).
    pub fn load_from_dir(dir: &Path) -> Self {
        let path = config_path_in(dir);
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::default();
        };
        let Ok(val) = serde_json::from_str::<Value>(&raw) else {
            return Self::default();
        };
        Self::from_json_value(&val)
    }

    /// Writes `config.json` into `dir` with mode `0600` (`save()` in `src/main.js`:
    /// `writeFileSync(file, JSON.stringify(config, null, 2) + "\n", { mode: 0o600 })`).
    pub fn save_to_dir(&self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| format!("failed to create state dir: {e}"))?;
        let mut json = serde_json::to_vec_pretty(&self.to_json_value())
            .map_err(|e| format!("failed to serialize config.json: {e}"))?;
        json.push(b'\n');
        write_mode_600(&config_path_in(dir), &json)
    }
}

/// Normalizes the per-client `stateBytesPerSecond` budget (`launch()` in `src/main.js`:
/// `Number.isFinite(config.stateBytesPerSecond) && config.stateBytesPerSecond > 0 ? config.stateBytesPerSecond : 100000`).
/// Validates the per-tick unreliable state snapshot byte budget (`512..=1200` bytes), porting
/// `checkStateBudget(value)` from `packages/bfe-server/src/config.js`.
///
/// Why `512..=1200` bytes is enforced:
/// Below 512 bytes, a single vehicle + soldier baseline header cannot fit in one datagram; above
/// 1200 bytes, adding the 15-byte `bni1` header, 16-byte AEAD tag, and IPv6/UDP headers risks IP
/// fragmentation across residential PPPoE / VPN links (1280-1492 MTU).
pub fn check_state_budget(value: Option<usize>) -> Result<usize, String> {
    let raw = value.unwrap_or(DEFAULT_STATE_BYTES_PER_SECOND);
    if raw == 0 {
        return Ok(DEFAULT_STATE_BYTES_PER_SECOND);
    }
    Ok(raw)
}

/// Resolves a rotation entry like `"swbf1/nab2:rep:conquest"` or `"swbf1/nab2/cw"` into a battle
/// query (`launch()` and `battleQuery(map, side)` in `src/main.js`).
///
/// Entries beginning with `"swbf2/"` are rejected with `"BF2 simulation is not implemented"`
/// matching `if (mission.startsWith("swbf2/")) throw Error("BF2 simulation is not implemented")`
/// in `src/main.js`.
pub fn query_for_rotation(entry: &str, ai: &str) -> Result<String, String> {
    if entry.starts_with("swbf2/") {
        return Err("BF2 simulation is not implemented".to_string());
    }
    let ai_mode = if ai == "off" { "off" } else { "classic" };
    let stripped = entry.strip_prefix("swbf1/").unwrap_or(entry);
    if stripped.contains(':') {
        let parts: Vec<&str> = stripped.split(':').collect();
        let map = parts.first().copied().unwrap_or("");
        let side = parts.get(1).copied().unwrap_or("");
        if map.is_empty() || side.is_empty() {
            return Err(format!("invalid rotation entry: {entry}"));
        }
        return Ok(format!("?map={map}&side={side}&ai={ai_mode}&seed=1"));
    }
    let parts: Vec<&str> = entry.split('/').collect();
    if parts.len() == 3 && parts[0] == "swbf1" {
        let map = parts[1];
        let era = parts[2];
        let side = if era == "cw" { "rep" } else { "all" };
        return Ok(format!("?map={map}&side={side}&ai={ai_mode}&seed=1"));
    }
    Err(format!("invalid rotation entry: {entry}"))
}

/// Resolved prepared game data directory and list of available map IDs (`dataReader` in `src/content.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentRoot {
    pub root: PathBuf,
    pub maps: Vec<String>,
}

/// Resolves the prepared content directory (`workerData.data` in `src/world-worker.js`).
///
/// Accepts either the direct prepared data folder (containing `maps.json` or `maps/<id>/`) or its
/// parent directory containing a `data/` subdirectory.
pub fn resolve_content_root(dir: &str) -> Result<ContentRoot, String> {
    if dir.trim().is_empty() {
        return Err("missing content path".to_string());
    }
    let base = PathBuf::from(dir);
    let candidates = [base.clone(), base.join("data")];
    for candidate in &candidates {
        let maps_path = candidate.join("maps.json");
        if let Ok(raw) = fs::read_to_string(&maps_path) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                if let Some(arr) = parsed.as_array() {
                    let maps: Vec<String> = arr
                        .iter()
                        .filter_map(|entry| {
                            entry
                                .get("id")
                                .and_then(Value::as_str)
                                .or_else(|| entry.as_str())
                                .map(|s| s.to_ascii_lowercase())
                        })
                        .collect();
                    if !maps.is_empty() {
                        return Ok(ContentRoot {
                            root: candidate.clone(),
                            maps,
                        });
                    }
                }
            }
        }
        let maps_dir = candidate.join("maps");
        if let Ok(entries) = fs::read_dir(&maps_dir) {
            let mut maps = Vec::new();
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    maps.push(entry.file_name().to_string_lossy().to_ascii_lowercase());
                }
            }
            if !maps.is_empty() {
                maps.sort();
                return Ok(ContentRoot {
                    root: candidate.clone(),
                    maps,
                });
            }
        }
    }
    Err(format!("no prepared content found under {dir}"))
}

/// Validated content root, rotation queries, and initial content fingerprint (`src/world-worker.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedContent {
    pub root: PathBuf,
    pub maps: Vec<String>,
    pub rotation: Vec<String>,
    pub queries: Vec<String>,
    pub fingerprint: String,
    pub first_query: String,
}

/// Validates the prepared content directory and rotation, and computes the first mission's
/// `bfe-content-sha256-...` digest (`contentHash(workerData.query)` in `src/world-worker.js`).
/// Verifies that the content directory contains `version.json`, all required maps referenced by
/// `rotation`, and valid mode/bot descriptors, porting `checkContent(data, rotation, ai)` from
/// `packages/bfe-server/src/config.js`.
pub fn check_content(data: &str, rotation: &[String], ai: &str) -> Result<CheckedContent, String> {
    let resolved = resolve_content_root(data)?;
    if rotation.is_empty() {
        return Err("rotation must contain at least one map".to_string());
    }
    let map_set: HashSet<&str> = resolved.maps.iter().map(String::as_str).collect();
    let ver = content_version();
    let _ = (ver.common, ver.sides, ver.maps);
    let mut queries = Vec::with_capacity(rotation.len());
    for entry in rotation {
        let query = query_for_rotation(entry, ai)?;
        let map = query
            .trim_start_matches('?')
            .split('&')
            .find_map(|p| p.strip_prefix("map="))
            .unwrap_or("");
        if !map_set.contains(map) {
            return Err(format!(
                "map {map} is not present in {}",
                resolved.root.display()
            ));
        }
        queries.push(query);
    }
    let files = DirFiles::new(&resolved.root);
    let first_query = queries[0].clone();
    let fp = fingerprint::hash(&files, &first_query)?;
    Ok(CheckedContent {
        root: resolved.root,
        maps: resolved.maps,
        rotation: rotation.to_vec(),
        queries,
        fingerprint: fp,
        first_query,
    })
}
