//! Server configuration loading, saving, defaults, and prepared content inspection.

use crate::identity::write_mode_600;
use bfe_content::files::DirFiles;
use bfe_content::layout::{family_dir, is_swbf2_marked_dir};
use bfe_content_native::fingerprint;
use bfe_family::{swbf2, GameFamily};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Default online master directory URL (`defaults.master`).
pub const DEFAULT_MASTER_URL: &str = "https://online.battlefronteternal.com";
/// Default per-client outgoing snapshot budget in bytes/sec (`stateBytesPerSecond: 100000`).
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

/// The server state directory when `--dir` is not given: `BFE_STATE_DIR`, else
/// `$XDG_DATA_HOME/bfeserver`, else `~/.local/share/bfeserver`.
pub fn state_dir() -> PathBuf {
    let set = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = set("BFE_STATE_DIR") {
        return dir;
    }
    if let Some(dir) = set("XDG_DATA_HOME") {
        return dir.join("bfeserver");
    }
    set("HOME").unwrap_or_else(|| PathBuf::from(".")).join(".local/share/bfeserver")
}

/// Returns the path to `config.json` inside `dir` (`join(directory, "config.json")`).
pub fn config_path_in(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Returns `true` if `config.json` exists inside `dir` (`existsSync(file)`).
pub fn config_exists_in(dir: &Path) -> bool {
    config_path_in(dir).exists()
}

/// Returns the default playlist (`defaults.rotation = ["swbf1/nab2:rep:conquest"]`).
pub fn default_rotation() -> Vec<String> {
    vec!["swbf1/nab2:rep:conquest".to_string()]
}

/// Complete dedicated server configuration (`defaults` merged with `config.json`).
///
/// Unknown top-level JSON keys are preserved in `extra` so round-tripping `config.json` through
/// the interactive menu (`save()`) never strips operator-added fields.
#[derive(Clone, Debug, PartialEq)]
/// Resolved dedicated-server configuration matching the normalized runtime shape produced by
/// `loadConfig(dir)` and consumed by.
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
    /// Maximum simultaneous human players (`config.maxPlayers ?? 64`).
    pub max_players: Option<usize>,
    /// Maximum simultaneous sessions from a single IPv4 `/32` or IPv6 `/64` (`config.maxPlayersPerAddress ?? 8`).
    pub max_players_per_address: Option<usize>,
    /// Per-client snapshot state budget in bytes/sec (`config.stateBytesPerSecond ?? 100000`).
    pub state_bytes_per_second: usize,
    /// Whether joining players spawn immediately instead of opening the spawn screen (`config.autoSpawn === true`).
    pub auto_spawn: bool,
    /// Whether joining players are assigned a team automatically (`config.autoAssignTeams === true`).
    pub auto_assign_teams: bool,
    /// Bot mode: `"enhanced"` (the default for a new config), `"classic"` or `"off"`, the words
    /// the client's Instant Action gives its world (`aiMode`). A config.json written before
    /// Enhanced was the default has no `"ai"` key and keeps the Classic bots it ran.
    pub ai: String,
    /// Whether a hit from the victim's own team hurts (`friendlyFire`, on by default as in the
    /// 2004 game). Off adds `ff=0` to the battle query, which the world's admission reads.
    pub friendly_fire: bool,
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
    /// The games this server may host: [`bfe_family::Gate::BUILT`], which
    /// only code that starts a server can replace; never read from or written
    /// to `config.json`.
    pub gate: bfe_family::Gate,
}

impl Default for ServerConfig {
    /// Returns the default configuration (`const defaults = { ... }`), reading
    /// `BFE_ENGINE_DIR` and `BFE_DATA_DIR` from the process environment when present.
    /// Returns default dedicated-server settings matching `DEFAULT_CONFIG`:
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
            ai: "enhanced".to_string(),
            friendly_fire: true,
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
            gate: bfe_family::Gate::BUILT,
        }
    }
}

impl ServerConfig {
    /// Merges a parsed `config.json` value over `ServerConfig::default()`
    /// (`{ ...defaults, ...JSON.parse(readFileSync(file, "utf8")) }`).
    /// A known key with a value of the wrong type or out of range is an error naming the key,
    /// never a silent fall back to its default; a JSON `null` counts as absent.
    pub fn from_json_value(value: &Value) -> Result<Self, String> {
        let mut cfg = Self::default();
        let Some(obj) = value.as_object() else {
            return Err("the configuration is not a JSON object".to_string());
        };
        let mut bad = Vec::new();
        let get = |key: &str| obj.get(key).filter(|v| !v.is_null());
        let text = |key: &str, bad: &mut Vec<String>| match get(key) {
            None => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(v) => {
                bad.push(format!("\"{key}\" must be a string, not {v}"));
                None
            }
        };
        let flag = |key: &str, bad: &mut Vec<String>| match get(key) {
            None => None,
            Some(Value::Bool(b)) => Some(*b),
            Some(v) => {
                bad.push(format!("\"{key}\" must be true or false, not {v}"));
                None
            }
        };
        // A whole number in `range`; 64.0 is 64, 1.9 is an error.
        let number = |key: &str, range: std::ops::RangeInclusive<u64>, bad: &mut Vec<String>| {
            let v = get(key)?;
            let n = v.as_u64().or_else(|| {
                v.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0 && *f <= u64::MAX as f64).map(|f| f as u64)
            });
            match n {
                Some(n) if range.contains(&n) => Some(n),
                _ => {
                    bad.push(format!(
                        "\"{key}\" must be a whole number from {} to {}, not {v}",
                        range.start(),
                        range.end()
                    ));
                    None
                }
            }
        };
        if let Some(s) = text("name", &mut bad) {
            cfg.name = s;
        }
        if let Some(s) = text("engine", &mut bad) {
            cfg.engine = s;
        }
        if let Some(s) = text("data", &mut bad) {
            cfg.data = s;
        }
        match get("rotation") {
            None => {}
            Some(Value::Array(arr)) if arr.iter().all(Value::is_string) => {
                cfg.rotation = arr.iter().filter_map(Value::as_str).map(str::to_string).collect();
            }
            Some(v) => bad.push(format!("\"rotation\" must be a list of playlist entries (strings), not {v}")),
        }
        if let Some(s) = text("master", &mut bad) {
            cfg.master = s;
        }
        if let Some(b) = flag("onlineNoticeAccepted", &mut bad) {
            cfg.online_notice_accepted = b;
        }
        if let Some(s) = text("relayPolicy", &mut bad) {
            if matches!(s.as_str(), "auto" | "never" | "always") {
                cfg.relay_policy = s;
            } else {
                bad.push(format!("\"relayPolicy\" must be auto, never or always, not \"{s}\""));
            }
        }
        if let Some(b) = flag("honorGlobalBans", &mut bad) {
            cfg.honor_global_bans = b;
        }
        if let Some(n) = number("maxPlayers", 1..=u32::MAX as u64, &mut bad) {
            cfg.max_players = Some(n as usize);
        }
        if let Some(n) = number("maxPlayersPerAddress", 1..=u32::MAX as u64, &mut bad) {
            cfg.max_players_per_address = Some(n as usize);
        }
        if let Some(n) = number(
            "stateBytesPerSecond",
            MIN_STATE_BYTES_PER_SECOND as u64..=MAX_STATE_BYTES_PER_SECOND as u64,
            &mut bad,
        ) {
            cfg.state_bytes_per_second = n as usize;
        }
        if let Some(b) = flag("autoSpawn", &mut bad) {
            cfg.auto_spawn = b;
        }
        if let Some(b) = flag("autoAssignTeams", &mut bad) {
            cfg.auto_assign_teams = b;
        }
        // Older servers wrote no "ai" for their default, Classic.
        cfg.ai = match text("ai", &mut bad) {
            Some(mode) if matches!(mode.as_str(), "enhanced" | "classic" | "off") => mode,
            Some(mode) => {
                bad.push(format!("\"ai\" must be enhanced, classic or off, not \"{mode}\""));
                "classic".to_string()
            }
            None => "classic".to_string(),
        };
        if let Some(b) = flag("friendlyFire", &mut bad) {
            cfg.friendly_fire = b;
        }
        if let Some(n) = number("gamePort", 1..=65535, &mut bad) {
            cfg.game_port = Some(n as u16);
        }
        if let Some(s) = text("bind", &mut bad) {
            cfg.bind = Some(s);
        }
        if let Some(b) = flag("discovery", &mut bad) {
            cfg.discovery = b;
        }
        if let Some(n) = number("discoveryPort", 1..=65535, &mut bad) {
            cfg.discovery_port = n as u16;
        }
        match get("stun") {
            None => {}
            Some(Value::String(s)) => cfg.stun = Some(s.clone()),
            Some(Value::Array(arr)) if arr.iter().all(Value::is_string) => {
                cfg.stun = arr.first().and_then(Value::as_str).map(str::to_string);
            }
            Some(v) => bad.push(format!("\"stun\" must be a host:port string, not {v}")),
        }
        if let Some(n) = number("rotationDelayMs", 0..=u32::MAX as u64, &mut bad) {
            cfg.rotation_delay_ms = Some(n);
        }
        if let Some(b) = flag("networkThread", &mut bad) {
            cfg.network_thread = b;
        }
        cfg.content = text("content", &mut bad);
        cfg.description = text("description", &mut bad);
        cfg.sponsor = get("sponsor").cloned();
        cfg.location = text("location", &mut bad);
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
            "friendlyFire",
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
        if bad.is_empty() {
            Ok(cfg)
        } else {
            Err(bad.join("; "))
        }
    }

    /// Serializes the configuration back to a JSON object for `config.json` (`save()`).
    /// Serializes `self` into the exact camelCase JSON layout written to `<state_dir>/config.json`
    /// by `saveConfig(dir, cfg)`.
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
        if !self.friendly_fire {
            map.insert("friendlyFire".to_string(), Value::Bool(false));
        }
        // Always written: a file without it reads as an older server's Classic.
        map.insert("ai".to_string(), Value::String(self.ai.clone()));
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

    /// Loads `config.json` from `dir`: the defaults when the file does not exist, an error naming
    /// the file when it exists but cannot be read, is not a JSON object, or holds an unusable value.
    pub fn load_checked(dir: &Path) -> Result<Self, String> {
        let path = config_path_in(dir);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        match serde_json::from_str::<Value>(&raw) {
            Ok(val) if val.is_object() => {
                Self::from_json_value(&val).map_err(|e| format!("{}: {e}", path.display()))
            }
            Ok(_) => Err(format!("{} is not a JSON object", path.display())),
            Err(e) => Err(format!("{} is not valid JSON: {e}", path.display())),
        }
    }

    /// Applies the documented environment overrides (`BFE_DATA_DIR`, `BFE_GAME_PORT`, `BFE_BIND`,
    /// `BFE_DISCOVERY_PORT`, `BFE_MASTER_URL`, `BFE_ACCEPT_ONLINE=1`, `BFE_RELAY_POLICY`) through
    /// `get`. Returns the variables that were set but not usable.
    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) -> Vec<String> {
        let mut bad = Vec::new();
        let var = |name: &str| get(name).filter(|v| !v.trim().is_empty());
        let port = |name: &str, bad: &mut Vec<String>| {
            let raw = var(name)?;
            match raw.trim().parse::<u16>() {
                Ok(p) if p > 0 => Some(p),
                _ => {
                    bad.push(format!("{name}={raw}"));
                    None
                }
            }
        };
        if let Some(v) = var("BFE_DATA_DIR") {
            self.data = v;
        }
        if let Some(p) = port("BFE_GAME_PORT", &mut bad) {
            self.game_port = Some(p);
        }
        if let Some(v) = var("BFE_BIND") {
            self.bind = Some(v);
        }
        if let Some(p) = port("BFE_DISCOVERY_PORT", &mut bad) {
            self.discovery_port = p;
        }
        if let Some(v) = var("BFE_MASTER_URL") {
            self.master = v;
        }
        if let Some(v) = var("BFE_ACCEPT_ONLINE") {
            match v.trim() {
                "1" => self.online_notice_accepted = true,
                "0" => self.online_notice_accepted = false,
                _ => bad.push(format!("BFE_ACCEPT_ONLINE={v}")),
            }
        }
        if let Some(v) = var("BFE_RELAY_POLICY") {
            let policy = v.trim();
            if matches!(policy, "auto" | "never" | "always") {
                self.relay_policy = policy.to_string();
            } else {
                bad.push(format!("BFE_RELAY_POLICY={v}"));
            }
        }
        bad
    }

    /// Writes `config.json` into `dir` with mode `0600` (`save()`:
    /// `writeFileSync(file, JSON.stringify(config, null, 2) + "\n", { mode: 0o600 })`).
    pub fn save_to_dir(&self, dir: &Path) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| format!("failed to create state dir: {e}"))?;
        let mut json = serde_json::to_vec_pretty(&self.to_json_value())
            .map_err(|e| format!("failed to serialize config.json: {e}"))?;
        json.push(b'\n');
        write_mode_600(&config_path_in(dir), &json)
    }
}

/// The per-client snapshot budget in bytes per second: the default when unset, an error outside
/// `MIN_STATE_BYTES_PER_SECOND..=MAX_STATE_BYTES_PER_SECOND`.
pub fn check_state_budget(value: Option<usize>) -> Result<usize, String> {
    let raw = value.unwrap_or(DEFAULT_STATE_BYTES_PER_SECOND);
    if !(MIN_STATE_BYTES_PER_SECOND..=MAX_STATE_BYTES_PER_SECOND).contains(&raw) {
        return Err(format!(
            "stateBytesPerSecond {raw} is outside {MIN_STATE_BYTES_PER_SECOND} to {MAX_STATE_BYTES_PER_SECOND}"
        ));
    }
    Ok(raw)
}

/// Resolves a rotation entry like `"swbf1/nab2:rep:conquest"` or `"swbf1/nab2/cw"` into a battle
/// query (`launch()` and `battleQuery(map, side)`).
pub fn with_friendly_fire(query: &str, friendly_fire: bool) -> String {
    if friendly_fire { query.to_string() } else { format!("{query}&ff=0") }
}

/// A bot mode word as the world's admission reads it: `"enhanced"`, `"off"`, else `"classic"`.
pub fn ai_mode(ai: &str) -> &'static str {
    match ai {
        "enhanced" => "enhanced",
        "off" => "off",
        _ => "classic",
    }
}

/// The game a rotation entry belongs to. The server has always read an entry
/// with the literal prefix `swbf2/` as the 2005 game and every other entry,
/// whatever its spelling, as the 2004 game. The entry's family is decided here
/// once and travels with it (admission, publication); nothing reads it again
/// from the battle query, which a 2004 entry can fill with any text.
pub fn rotation_family(entry: &str) -> GameFamily {
    if swbf2::is_entry(entry) {
        GameFamily::Swbf2
    } else {
        GameFamily::Swbf1
    }
}

/// The one family check of a server rotation: every entry must belong to a
/// game `gate` lets the server host. It reads no content, so it can run first.
pub fn check_families(rotation: &[String], gate: bfe_family::Gate) -> Result<(), String> {
    for entry in rotation {
        gate.require(rotation_family(entry)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The menu's check of an edited playlist entry (`choice === "3"` in
/// `menu()`): a 2004 entry as `/^swbf1\/[a-z0-9_-]+:[a-z0-9_-]+:[a-z0-9_-]+$/`, a
/// 2005 entry in the complete form of `bfe_family::swbf2`.
pub fn valid_playlist_entry(entry: &str) -> bool {
    if rotation_family(entry) == GameFamily::Swbf2 {
        return swbf2::Entry::parse_complete(entry).is_some();
    }
    let Some(rest) = entry.strip_prefix("swbf1/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
    })
}

/// The battle query of a rotation entry. A 2004 entry's map and side are
/// copied as written, as they always were. A 2005 entry (complete form) names
/// its map by the qualified id, so a released client refuses the battle before
/// it reads content: `?map=swbf2/tat2&side=all&ai=enhanced&seed=1&mode=eli`.
pub fn query_for_rotation(entry: &str, ai: &str) -> Result<String, String> {
    query_for_rotation_family(rotation_family(entry), entry, ai)
}

pub fn query_for_rotation_family(family: GameFamily, entry: &str, ai: &str) -> Result<String, String> {
    if family == GameFamily::Swbf2 {
        let parsed = swbf2::Entry::parse_complete(entry).ok_or_else(|| format!("invalid rotation entry: {entry}"))?;
        let mut query = format!("?map={}&side={}&ai={}&seed=1", parsed.qualified_map(), parsed.side(), ai_mode(ai));
        if parsed.mode != bfe_family::DEFAULT_MODE {
            query.push_str("&mode=");
            query.push_str(&parsed.mode);
        }
        return Ok(query);
    }
    let ai_mode = ai_mode(ai);
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

/// Resolved prepared game data directory and list of available map IDs (`dataReader`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentRoot {
    pub root: PathBuf,
    pub maps: Vec<String>,
}

fn maps_in(family_root: &Path) -> Option<Vec<String>> {
    let maps_path = family_root.join("maps.json");
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
                    return Some(maps);
                }
            }
        }
    }
    let maps_dir = family_root.join("maps");
    if let Ok(entries) = fs::read_dir(&maps_dir) {
        let mut maps = Vec::new();
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                maps.push(entry.file_name().to_string_lossy().to_ascii_lowercase());
            }
        }
        if !maps.is_empty() {
            maps.sort();
            return Some(maps);
        }
    }
    None
}

/// Resolves the prepared content directory for `family` under `dir`.
///
/// Accepts either the direct prepared content folder or its parent directory
/// containing a `data/` subdirectory. For `swbf1` the family root is the
/// content root itself; for `swbf2` it is `<root>/swbf2/.content` or a
/// standalone 2005-marked folder directly.
pub fn resolve_content_root_for(dir: &str, family: GameFamily) -> Result<ContentRoot, String> {
    if dir.trim().is_empty() {
        return Err("missing content path".to_string());
    }
    let base = PathBuf::from(dir);
    let candidates = [base.clone(), base.join("data")];
    for candidate in &candidates {
        if !candidate.exists() {
            continue;
        }
        if family == GameFamily::Swbf1 && is_swbf2_marked_dir(candidate) {
            continue;
        }
        let family_root = family_dir(candidate, family);
        if let Some(maps) = maps_in(&family_root) {
            return Ok(ContentRoot {
                root: family_root,
                maps,
            });
        }
        if family == GameFamily::Swbf2 && is_swbf2_marked_dir(candidate) {
            if let Some(maps) = maps_in(candidate) {
                return Ok(ContentRoot {
                    root: candidate.clone(),
                    maps,
                });
            }
        }
    }
    Err(format!("no prepared content found under {dir}"))
}

/// Resolves the prepared 2004 (`swbf1`) content directory (`workerData.data`).
pub fn resolve_content_root(dir: &str) -> Result<ContentRoot, String> {
    resolve_content_root_for(dir, GameFamily::Swbf1)
}

/// The verified content digest of a battle: for a 2004 battle the released
/// fingerprint of its query over the content root; for a 2005 battle the
/// approved 2005 fingerprint of its content query (map, mode and mission) over
/// the 2005 content folder.
pub fn battle_digest(root: &Path, family: GameFamily, query: &str) -> Result<String, String> {
    match family {
        GameFamily::Swbf1 => fingerprint::hash(&DirFiles::from_resolved_root(root, GameFamily::Swbf1), query),
        GameFamily::Swbf2 => {
            let (folder, content_query) = swbf2_content(root, query)?;
            fingerprint::hash_for_family(&DirFiles::from_resolved_root(&folder, GameFamily::Swbf2), GameFamily::Swbf2, &content_query)
        }
    }
}

/// Where a 2005 battle's content is read and with which query: the 2005
/// content folder, and the battle query naming the bare map. A standalone
/// 2005-marked folder is already that folder and is not nested a second time.
pub fn swbf2_content(root: &Path, query: &str) -> Result<(PathBuf, String), String> {
    let bare = bfe_family::owner::query_map(query).and_then(swbf2::bare_map).ok_or("invalid Battlefront II battle query")?;
    let content_query = swbf2::content_query(query, bare).ok_or("invalid Battlefront II battle query")?;
    let folder = if is_swbf2_marked_dir(root) {
        root.to_path_buf()
    } else {
        GameFamily::Swbf2.content_dir(root)
    };
    Ok((folder, content_query))
}

/// Validated content root, rotation queries, and initial content fingerprint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckedContent {
    /// The game of `rotation[0]`, the battle this content starts.
    pub family: GameFamily,
    pub root: PathBuf,
    pub maps: Vec<String>,
    pub rotation: Vec<String>,
    pub queries: Vec<String>,
    /// The content digest of the battle the rotation starts with. None for a
    /// 2005 battle: its world load computes the one digest its start uses.
    pub fingerprint: Option<String>,
    pub first_query: String,
}

/// Whether a rotation names a 2005 battle. Such a rotation passes over the
/// entries a server has no prepared content for; a 2004-only one is checked
/// and scheduled as released.
pub fn names_swbf2(rotation: &[String]) -> bool {
    rotation.iter().any(|entry| rotation_family(entry) == GameFamily::Swbf2)
}

/// Whether the prepared content under `data` holds the map of one rotation
/// entry: a 2004 map in the flat root's map list, a 2005 map as a folder in
/// the 2005 content folder. An entry whose query cannot be built has no
/// content a battle could start from.
pub fn entry_has_content(data: &str, entry: &str, ai: &str) -> bool {
    let Ok(query) = query_for_rotation(entry, ai) else { return false };
    let map = query.trim_start_matches('?').split('&').find_map(|p| p.strip_prefix("map=")).unwrap_or("");
    // A content root that cannot be resolved at all decides nothing here: the
    // battle load reports it as before, so the entry counts as present.
    if rotation_family(entry) == GameFamily::Swbf2 {
        let Some(bare) = swbf2::bare_map(map) else { return false };
        let mut resolved_any = false;
        if let Ok(resolved) = resolve_content_root(data) {
            resolved_any = true;
            if GameFamily::Swbf2.content_dir(&resolved.root).join("maps").join(bare).join("map.json").is_file() {
                return true;
            }
        }
        if let Ok(resolved) = resolve_content_root_for(data, GameFamily::Swbf2) {
            resolved_any = true;
            if resolved.root.join("maps").join(bare).join("map.json").is_file() {
                return true;
            }
        }
        return !resolved_any;
    }
    match resolve_content_root(data) {
        Ok(resolved) => resolved.maps.iter().any(|m| m == map),
        // A folder holding only the 2005 game resolves for that game: it
        // plainly has no 2004 map. Only a folder that resolves for neither
        // game decides nothing here.
        Err(_) => resolve_content_root_for(data, GameFamily::Swbf2).is_err(),
    }
}

/// Validates the prepared content directory and rotation, and computes the opening mission's
/// `bfe-content-sha256-...` digest (`contentHash(workerData.query)`).
/// Verifies that the content directory contains `version.json`, all required maps referenced by
/// `rotation`, and valid mode/bot descriptors, porting `checkContent(data, rotation, ai)`.
pub fn check_content(data: &str, rotation: &[String], ai: &str, gate: bfe_family::Gate) -> Result<CheckedContent, String> {
    // A missing content folder must not hide that a game cannot be played.
    check_families(rotation, gate)?;
    let family = rotation.first().map(|entry| rotation_family(entry)).unwrap_or_default();
    // The flat 2004 root and its map list stay separate from the 2005 content
    // folder. A 2005-only folder has no 2004 maps, and a standalone 2005 folder
    // already points directly at its prepared content without another
    // `swbf2/.content` appended beneath it.
    let (root, maps, swbf2_dir) = match resolve_content_root(data) {
        Ok(resolved) => {
            let swbf2_dir = GameFamily::Swbf2.content_dir(&resolved.root);
            (resolved.root, resolved.maps, swbf2_dir)
        }
        Err(error) if family == GameFamily::Swbf2 => {
            let resolved = resolve_content_root_for(data, GameFamily::Swbf2).map_err(|_| error)?;
            let swbf2_dir = resolved.root.clone();
            let root = resolved
                .root
                .parent()
                .and_then(|p| p.parent())
                .filter(|_| resolved.root.ends_with(Path::new("swbf2").join(".content")))
                .map_or_else(|| resolved.root.clone(), Path::to_path_buf);
            (root, Vec::new(), swbf2_dir)
        }
        Err(error) => return Err(error),
    };
    if rotation.is_empty() {
        return Err("rotation must contain at least one map".to_string());
    }
    let map_set: HashSet<&str> = maps.iter().map(String::as_str).collect();
    let ver = content_version();
    let _ = (ver.common, ver.sides, ver.maps);
    let passes_over = names_swbf2(rotation);
    let mut queries = Vec::with_capacity(rotation.len());
    for (at, entry) in rotation.iter().enumerate() {
        let query = query_for_rotation(entry, ai)?;
        let map = query
            .trim_start_matches('?')
            .split('&')
            .find_map(|p| p.strip_prefix("map="))
            .unwrap_or("");
        // A 2005 map lives in the 2005 content folder under its bare id.
        // The battle this content starts must be present. A later entry of a
        // rotation that names a 2005 battle may have no content here: the
        // rotation skips it, with a log line, when its turn comes. A 2004-only
        // rotation keeps the released check of every entry.
        let absent = if let Some(bare) = swbf2::bare_map(map).filter(|_| rotation_family(entry) == GameFamily::Swbf2) {
            (!swbf2_dir.join("maps").join(bare).join("map.json").is_file()).then(|| swbf2_dir.clone())
        } else {
            (!map_set.contains(map)).then(|| root.clone())
        };
        if let Some(folder) = absent {
            if at == 0 || !passes_over {
                return Err(format!("map {map} is not present in {}", folder.display()));
            }
        }
        queries.push(query);
    }
    let first_query = queries[0].clone();
    let fp = match rotation_family(&rotation[0]) {
        GameFamily::Swbf1 => Some(battle_digest(&root, GameFamily::Swbf1, &first_query)?),
        GameFamily::Swbf2 => None,
    };
    Ok(CheckedContent {
        family,
        root,
        maps,
        rotation: rotation.to_vec(),
        queries,
        fingerprint: fp,
        first_query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_overrides_config_values() {
        let mut cfg = ServerConfig { data: "/old".into(), game_port: Some(1), ..Default::default() };
        let env = |name: &str| {
            match name {
                "BFE_DATA_DIR" => Some("/new"),
                "BFE_GAME_PORT" => Some("19000"),
                "BFE_DISCOVERY_PORT" => Some("nope"),
                "BFE_ACCEPT_ONLINE" => Some("1"),
                "BFE_RELAY_POLICY" => Some("never"),
                _ => None,
            }
            .map(str::to_string)
        };
        let bad = cfg.apply_env(env);
        assert_eq!(cfg.data, "/new");
        assert_eq!(cfg.game_port, Some(19000));
        assert_eq!(cfg.discovery_port, 18452);
        assert!(cfg.online_notice_accepted);
        assert_eq!(cfg.relay_policy, "never");
        assert_eq!(bad, vec!["BFE_DISCOVERY_PORT=nope".to_string()]);
    }

    #[test]
    fn broken_config_file_is_an_error_and_missing_is_defaults() {
        let dir = std::env::temp_dir().join(format!("bfe-config-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(ServerConfig::load_checked(&dir).is_ok());
        fs::write(config_path_in(&dir), "{\"data\": ").unwrap();
        assert!(ServerConfig::load_checked(&dir).unwrap_err().contains("not valid JSON"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_content_root_reads_flat_swbf1_and_nested_swbf2_without_moving() {
        let dir = std::env::temp_dir().join(format!(
            "bfe-server-layout-families-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("maps/qq7")).unwrap();
        fs::write(dir.join("maps/qq7/map.json"), b"{}").unwrap();
        fs::create_dir_all(dir.join("common")).unwrap();
        fs::write(dir.join("common/common.json"), b"{}").unwrap();

        let bf2_dir = family_dir(&dir, GameFamily::Swbf2);
        fs::create_dir_all(bf2_dir.join("maps/zz1")).unwrap();
        fs::write(bf2_dir.join("maps/zz1/map.json"), b"{}").unwrap();

        let resolved1 = resolve_content_root(&dir.to_string_lossy()).unwrap();
        assert_eq!(resolved1.root, dir);
        assert_eq!(resolved1.maps, vec!["qq7".to_string()]);
        assert!(!dir.join("swbf1").exists());

        let resolved2 = resolve_content_root_for(&dir.to_string_lossy(), GameFamily::Swbf2).unwrap();
        assert_eq!(resolved2.root, bf2_dir);
        assert_eq!(resolved2.maps, vec!["zz1".to_string()]);
        // Passing the already-resolved family root to DirFiles::from_resolved_root reads once without double-nesting.
        use bfe_content::files::Files as _;
        let bf2_files = DirFiles::from_resolved_root(&resolved2.root, GameFamily::Swbf2);
        assert_eq!(
            bf2_files.read("maps/zz1/map.json").as_deref(),
            Some(&b"{}"[..])
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn resolve_content_root_reads_read_only_2004_folder() {
        use std::os::unix::fs::PermissionsExt;
        if fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| s.lines().find(|l| l.starts_with("Uid:")).map(str::to_string))
            .is_some_and(|line| line.split_whitespace().nth(2) == Some("0"))
        {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "bfe-server-layout-ro-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("maps/qq7")).unwrap();
        fs::write(dir.join("maps/qq7/map.json"), b"{}").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();

        let resolved = resolve_content_root(&dir.to_string_lossy()).unwrap();
        assert_eq!(resolved.root, dir);
        assert_eq!(resolved.maps, vec!["qq7".to_string()]);

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unusable_values_are_errors_naming_the_key() {
        for (bad, key) in [
            (json!({"ai": "Enhanced"}), "\"ai\""),
            (json!({"ai": "enahnced"}), "\"ai\""),
            (json!({"ai": false}), "\"ai\""),
            (json!({"gamePort": 0}), "gamePort"),
            (json!({"gamePort": 70000}), "gamePort"),
            (json!({"gamePort": "18451"}), "gamePort"),
            (json!({"discoveryPort": 0}), "discoveryPort"),
            (json!({"maxPlayers": 1.9}), "maxPlayers"),
            (json!({"maxPlayers": 0}), "maxPlayers"),
            (json!({"stateBytesPerSecond": 0}), "stateBytesPerSecond"),
            (json!({"stateBytesPerSecond": 100}), "stateBytesPerSecond"),
            (json!({"stateBytesPerSecond": 20_000_000}), "stateBytesPerSecond"),
            (json!({"rotation": ["swbf1/nab2:rep:conquest", 3]}), "rotation"),
            (json!({"friendlyFire": "no"}), "friendlyFire"),
            (json!({"relayPolicy": "sometimes"}), "relayPolicy"),
        ] {
            let err = ServerConfig::from_json_value(&bad).unwrap_err();
            assert!(err.contains(key), "{bad}: {err}");
        }
        // Whole numbers written as floats and nulls are fine.
        let ok = ServerConfig::from_json_value(&json!({"maxPlayers": 64.0, "bind": null, "gamePort": 18451})).unwrap();
        assert_eq!((ok.max_players, ok.bind, ok.game_port), (Some(64), None, Some(18451)));
    }

    #[test]
    fn a_config_file_with_an_unusable_value_does_not_load() {
        let dir = std::env::temp_dir().join(format!("bfe-config-bad-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(config_path_in(&dir), "{\"ai\": \"Enhanced\"}").unwrap();
        let err = ServerConfig::load_checked(&dir).unwrap_err();
        assert!(err.contains("config.json") && err.contains("\"ai\""), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_budget_outside_its_bounds_is_an_error() {
        assert_eq!(check_state_budget(None), Ok(DEFAULT_STATE_BYTES_PER_SECOND));
        assert_eq!(check_state_budget(Some(MIN_STATE_BYTES_PER_SECOND)), Ok(MIN_STATE_BYTES_PER_SECOND));
        for bad in [0, MIN_STATE_BYTES_PER_SECOND - 1, MAX_STATE_BYTES_PER_SECOND + 1] {
            assert!(check_state_budget(Some(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn unusable_environment_values_are_reported() {
        let mut cfg = ServerConfig { online_notice_accepted: true, ..Default::default() };
        let env = |name: &str| {
            match name {
                "BFE_ACCEPT_ONLINE" => Some("yes"),
                "BFE_RELAY_POLICY" => Some(" never "),
                _ => None,
            }
            .map(str::to_string)
        };
        let bad = cfg.apply_env(env);
        assert!(cfg.online_notice_accepted, "unchanged by an unusable value");
        assert_eq!(cfg.relay_policy, "never");
        assert_eq!(bad, vec!["BFE_ACCEPT_ONLINE=yes".to_string()]);
    }

    #[test]
    fn new_configs_field_enhanced_bots_and_old_ones_keep_theirs() {
        // A new server: Enhanced, written out, read back the same.
        let fresh = ServerConfig::default();
        assert_eq!(fresh.ai, "enhanced");
        assert_eq!(ServerConfig::from_json_value(&fresh.to_json_value()).unwrap().ai, "enhanced");
        // A config.json from before: Classic was its unwritten default.
        assert_eq!(ServerConfig::from_json_value(&json!({"name": "Old"})).unwrap().ai, "classic");
        for mode in ["enhanced", "classic", "off"] {
            assert_eq!(ServerConfig::from_json_value(&json!({"ai": mode})).unwrap().ai, mode);
            let query = query_for_rotation("swbf1/nab2:rep:conquest", mode).unwrap();
            assert!(query.contains(&format!("&ai={mode}&")), "{query}");
        }
    }

    // Released outputs of the rotation query builder and the menu check for
    // 2004 entries, including entries whose side field carries query text and
    // entries that spell a family other than with the literal `swbf2/` prefix.
    #[test]
    fn a_2004_rotation_entry_keeps_the_released_query_and_menu_answer() {
        for (entry, query, menu) in [
            ("swbf1/nab2:rep:conquest", "?map=nab2&side=rep&ai=enhanced&seed=1", true),
            ("swbf1/nab2:cw:conquest", "?map=nab2&side=cw&ai=enhanced&seed=1", true),
            ("swbf1/nab2:rep:duel", "?map=nab2&side=rep&ai=enhanced&seed=1", true),
            ("swbf1/end1:ewk:conquest", "?map=end1&side=ewk&ai=enhanced&seed=1", true),
            ("swbf1/nab2/cis", "?map=nab2&side=all&ai=enhanced&seed=1", false),
            ("swbf1/hot1/XYZ", "?map=hot1&side=all&ai=enhanced&seed=1", false),
            ("swbf1/nab2:cis:conquest:backup", "?map=nab2&side=cis&ai=enhanced&seed=1", false),
            ("swbf1/nab2:imp&side=rep:duel", "?map=nab2&side=imp&side=rep&ai=enhanced&seed=1", false),
            ("swbf1/tat2:all&family=swbf2:duel", "?map=tat2&side=all&family=swbf2&ai=enhanced&seed=1", false),
            ("swbf1/nab2:r%65p:conquest", "?map=nab2&side=r%65p&ai=enhanced&seed=1", false),
            ("SWBF1/NAB2:REP:CONQUEST", "?map=SWBF1/NAB2&side=REP&ai=enhanced&seed=1", false),
            ("SWBF2/kam9:gcw:hunt", "?map=SWBF2/kam9&side=gcw&ai=enhanced&seed=1", false),
            (" swbf2/kam9:rep:hunt", "?map= swbf2/kam9&side=rep&ai=enhanced&seed=1", false),
        ] {
            assert_eq!(rotation_family(entry), GameFamily::Swbf1, "{entry}");
            assert_eq!(query_for_rotation(entry, "enhanced").as_deref(), Ok(query), "{entry}");
            assert_eq!(valid_playlist_entry(entry), menu, "{entry}");
        }
        // The menu trims an edited entry before it checks it.
        assert!(valid_playlist_entry(" swbf2/kam9:gcw:hunt".trim()));
    }

    #[test]
    fn a_2005_entry_uses_the_shared_grammar_and_a_qualified_map() {
        assert_eq!(rotation_family("swbf2/kam9:gcw:hunt"), GameFamily::Swbf2);
        assert_eq!(
            query_for_rotation("swbf2/kam9:gcw:hunt", "enhanced").as_deref(),
            Ok("?map=swbf2/kam9&side=all&ai=enhanced&seed=1&mode=hunt")
        );
        assert_eq!(
            query_for_rotation("swbf2/kam9::conquest", "classic").as_deref(),
            Ok("?map=swbf2/kam9&side=rep&ai=classic&seed=1")
        );
        for bad in ["swbf2/kam9:rep:hunt", "swbf2/kam9:gcw", "swbf2/Kam9:gcw:hunt", "swbf2/kam9:gcw:hu-nt"] {
            assert_eq!(query_for_rotation(bad, "enhanced"), Err(format!("invalid rotation entry: {bad}")));
            assert!(!valid_playlist_entry(bad), "{bad}");
        }
        assert!(valid_playlist_entry("swbf2/kam9::hunt"));
    }

    #[test]
    fn the_family_check_comes_first_and_keeps_the_released_order_for_2004() {
        use bfe_family::Gate;
        let unavailable = "Battlefront II (2005) is not available in this build".to_string();
        // A missing content folder does not hide the refusal.
        for data in ["", "/nonexistent/content", " "] {
            let rotation = ["swbf1/nab2:rep:conquest".to_string(), "swbf2/kam9:gcw:hunt".to_string()];
            assert_eq!(check_content(data, &rotation, "enhanced", Gate::BUILT).err(), Some(unavailable.clone()));
            assert_eq!(check_families(&rotation, Gate::BUILT), Err(unavailable.clone()));
            assert_eq!(check_families(&rotation, ServerConfig::default().gate), Err(unavailable.clone()));
        }
        // A 2004 rotation reports what the released server reported, in its order.
        assert_eq!(check_content("", &[], "enhanced", Gate::BUILT).err().as_deref(), Some("missing content path"));
        assert_eq!(check_content("", &["swbf1/nab2:rep:conquest".to_string()], "enhanced", Gate::BUILT).err().as_deref(), Some("missing content path"));
        assert_eq!(check_families(&["SWBF2/kam9:gcw:hunt".to_string()], Gate::BUILT), Ok(()));
    }

    struct OpenGate;
    impl bfe_family::Availability for OpenGate {
        fn plays(&self, _: GameFamily) -> bool { true }
        fn name(&self) -> &'static str { "test-open" }
    }
    const OPEN: bfe_family::Gate = bfe_family::Gate::new(&OpenGate);

    #[test]
    fn check_content_does_not_leak_2005_maps_into_2004_map_list() {
        let dir = std::env::temp_dir().join(format!("bfe-config-2005-only-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let bf2 = family_dir(&dir, GameFamily::Swbf2);
        fs::create_dir_all(bf2.join("maps/tat2")).unwrap();
        fs::write(bf2.join("maps/tat2/map.json"), b"{}").unwrap();
        fs::write(bf2.join(bfe_content::layout::SWBF2_MARKER_FILE), bfe_content::layout::SWBF2_MARKER_BODY).unwrap();

        let data = dir.to_string_lossy().into_owned();
        let rotation = vec!["swbf2/tat2:gcw:eli".to_string(), "swbf1/tat2:all:conquest".to_string()];
        let checked = check_content(&data, &rotation, "off", OPEN).unwrap();
        assert_eq!(checked.family, GameFamily::Swbf2);
        assert!(checked.maps.is_empty(), "2005 maps leaked into 2004 map list: {:?}", checked.maps);
        assert!(entry_has_content(&data, "swbf2/tat2:gcw:eli", "off"));
        assert!(!entry_has_content(&data, "swbf1/tat2:all:conquest", "off"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_content_accepts_standalone_2005_folder_without_double_nesting() {
        let dir = std::env::temp_dir().join(format!("bfe-config-standalone-2005-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let standalone = dir.join("standalone-bf2");
        fs::create_dir_all(standalone.join("maps/tat2")).unwrap();
        fs::write(standalone.join("maps/tat2/map.json"), b"{}").unwrap();
        fs::write(standalone.join(bfe_content::layout::SWBF2_PREPARE_STATE_FILE), b"{}").unwrap();

        let data = standalone.to_string_lossy().into_owned();
        let rotation = vec!["swbf2/tat2:gcw:eli".to_string()];
        let checked = check_content(&data, &rotation, "off", OPEN).unwrap();
        assert_eq!(checked.family, GameFamily::Swbf2);
        assert!(checked.maps.is_empty(), "standalone 2005 folder must not populate 2004 maps: {:?}", checked.maps);
        let (folder, query) = swbf2_content(&checked.root, &checked.first_query).unwrap();
        assert_eq!(folder, standalone);
        assert_eq!(query, "?map=tat2&side=all&ai=off&seed=1&mode=eli&era=gcw");
        assert!(entry_has_content(&data, "swbf2/tat2:gcw:eli", "off"));
        assert!(!entry_has_content(&data, "swbf1/tat2:all:conquest", "off"));

        // Pointing directly at a nested swbf2/.content folder also avoids double nesting.
        let nested_parent = dir.join("nested");
        let nested_bf2 = family_dir(&nested_parent, GameFamily::Swbf2);
        fs::create_dir_all(nested_bf2.join("maps/tat2")).unwrap();
        fs::write(nested_bf2.join("maps/tat2/map.json"), b"{}").unwrap();
        fs::write(nested_bf2.join(bfe_content::layout::SWBF2_MARKER_FILE), bfe_content::layout::SWBF2_MARKER_BODY).unwrap();
        let nested_checked = check_content(&nested_bf2.to_string_lossy(), &rotation, "off", OPEN).unwrap();
        assert!(nested_checked.maps.is_empty());
        assert_eq!(swbf2_content(&nested_checked.root, &nested_checked.first_query).unwrap().0, nested_bf2);

        // A 2004-only folder with a map of the same bare id must never be read as 2005.
        let flat_2004 = dir.join("flat-2004");
        fs::create_dir_all(flat_2004.join("maps/tat2")).unwrap();
        fs::write(flat_2004.join("maps/tat2/map.json"), b"{}").unwrap();
        assert!(check_content(&flat_2004.to_string_lossy(), &rotation, "off", OPEN).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

}
