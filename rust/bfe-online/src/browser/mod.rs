// Pure server browser state machine, filtering, sorting, selection, refresh
// cadence, battle query sanitization, compatibility checking, and GAME INFO
// inspector formatting.
//
// Ports `src/shell/server-browser.js`, `src/shell/server-browser-view.js`, and
// `src/runtime/battle-query.js`.

use std::cmp::Ordering;
// `BTreeMap` is imported inside `mod tests` for deterministic query maps.

use bfe_transport::layout::{network_contract, VERSION as TRANSPORT_LAYOUT_VERSION};
use serde::{Deserialize, Serialize};

// Auto-refresh interval (30s) when the server browser is open, matching
// `REFRESH_EVERY` in `src/shell/server-browser.js`.
pub const REFRESH_EVERY_MS: u64 = 30_000;

// Quiet retry interval (10s) after a refresh error while keeping stale rows,
// matching `RETRY_EVERY` in `src/shell/server-browser.js`.
pub const RETRY_EVERY_MS: u64 = 10_000;

// Browser listing sources (`SOURCES = [["internet", "INTERNET"], ["lan", "LAN"]]`
// in `src/shell/server-browser.js`).
// Selecting `Internet` queries the signed HTTPS master directory
// (`GET /v1/servers`), whereas `Lan` broadcasts a UDP `BFE-LAN/1` beacon on
// port 18452 to discover local servers without touching the internet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Internet,
    Lan,
}

impl Source {
    // Ordered list of source selector options for toolbar dropdown rendering.
    pub const ALL: [Source; 2] = [Source::Internet, Source::Lan];

    // Canonical lowercase wire/config identifier (`"internet"` or `"lan"`).
    // Option value string (`"any"`, `"50"`, `"100"`, `"150"`) matching `PINGS` in JS.
    // Column identifier used in CSS class `.c-{id}` and `COLUMNS[i].id` in JS.
    pub fn id(self) -> &'static str {
        match self {
            Self::Internet => "internet",
            Self::Lan => "lan",
        }
    }

    // Uppercase UI label displayed in `.bfe-browser-source` (`"INTERNET"` / `"LAN"`).
    // Uppercase display text shown in the ping filter dropdown.
    // Uppercase display label for the era dropdown and `GAME INFO` chips.
    // Uppercase column header text rendered in `.bfe-browser-col`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Internet => "INTERNET",
            Self::Lan => "LAN",
        }
    }

    // Parses a source identifier from a UI control or CLI flag, returning `None`
    // for unknown strings (`!SOURCES.some(([id]) => id === source)` in JS).
    // Parses a toolbar `<select>` option value back into a `PingFilter`.
    // Resolves a column identifier string into its `SortColumn` variant.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "internet" => Some(Self::Internet),
            "lan" => Some(Self::Lan),
            _ => None,
        }
    }
}

// Ping filter choices (`PINGS` in `src/shell/server-browser.js`):
// `[["any", "ANY PING"], ["50", "UNDER 50 MS"], ["100", "UNDER 100 MS"], ["150", "UNDER 150 MS"]]`.
// When a numeric threshold is active, both high-latency servers (`latency_ms >= threshold`)
// and unanswered servers (`latency_ms == None`, mapped to `Infinity` in JS) are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PingFilter {
    #[default]
    Any,
    Under50,
    Under100,
    Under150,
}

impl PingFilter {
    // Ordered choices rendered in the toolbar ping `<select>` control.
    pub const ALL: [PingFilter; 4] = [
        PingFilter::Any,
        PingFilter::Under50,
        PingFilter::Under100,
        PingFilter::Under150,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Under50 => "50",
            Self::Under100 => "100",
            Self::Under150 => "150",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "ANY PING",
            Self::Under50 => "UNDER 50 MS",
            Self::Under100 => "UNDER 100 MS",
            Self::Under150 => "UNDER 150 MS",
        }
    }

    // Strict upper bound in milliseconds (`finite(s.latencyMs) < Number(f.ping)`
    // in `src/shell/server-browser.js`), or `None` when `PingFilter::Any` is selected.
    pub fn threshold_ms(self) -> Option<u32> {
        match self {
            Self::Any => None,
            Self::Under50 => Some(50),
            Self::Under100 => Some(100),
            Self::Under150 => Some(150),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "any" => Some(Self::Any),
            "50" => Some(Self::Under50),
            "100" => Some(Self::Under100),
            "150" => Some(Self::Under150),
            _ => None,
        }
    }
}

// Era filter choices (`[["all", "ALL ERAS"], ...Object.entries(ERAS)]` in
// `src/shell/server-browser-view.js` and `ERAS` in `src/shell/server-browser.js`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraOption {
    All,
    Cw,
    Gcw,
}

impl EraOption {
    // Ordered era filter options for the toolbar `<select>` control.
    pub const ALL: [EraOption; 3] = [EraOption::All, EraOption::Cw, EraOption::Gcw];

    // Canonical filter key (`"all"`, `"cw"`, `"gcw"`) stored in `Filters::era`.
    pub fn id(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Cw => "cw",
            Self::Gcw => "gcw",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "ALL ERAS",
            Self::Cw => "CLONE WARS",
            Self::Gcw => "GALACTIC CIVIL WAR",
        }
    }
}

// Sortable table columns (`COLUMNS` in `src/shell/server-browser.js`).
// Clicking a column header sorts by that column or reverses the current sort
// direction when already active (`sortBy(key)` in `src/shell/server-browser.js`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Players,
    Map,
    Mode,
    Era,
    Ping,
    Location,
}

impl SortColumn {
    // Left-to-right column order in `.bfe-browser-head` (`src/shell/server-browser-view.js`).
    pub const ALL: [SortColumn; 7] = [
        SortColumn::Name,
        SortColumn::Players,
        SortColumn::Map,
        SortColumn::Mode,
        SortColumn::Era,
        SortColumn::Ping,
        SortColumn::Location,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Players => "players",
            Self::Map => "map",
            Self::Mode => "mode",
            Self::Era => "era",
            Self::Ping => "ping",
            Self::Location => "location",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "GAME NAME",
            Self::Players => "PLAYERS",
            Self::Map => "MAP",
            Self::Mode => "MODE",
            Self::Era => "ERA",
            Self::Ping => "PING",
            Self::Location => "LOCATION",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "name" => Some(Self::Name),
            "players" => Some(Self::Players),
            "map" => Some(Self::Map),
            "mode" => Some(Self::Mode),
            "era" => Some(Self::Era),
            "ping" => Some(Self::Ping),
            "location" => Some(Self::Location),
            _ => None,
        }
    }
}

// Active sort column and direction (`+1` ascending, `-1` descending),
// corresponding to `this.sort = { key, dir }` in `src/shell/server-browser.js`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSpec {
    pub column: SortColumn,
    pub dir: i8,
}

impl Default for SortSpec {
    // Matches `this.sort = { key: "ping", dir: 1 }` in `src/shell/server-browser.js`.
    fn default() -> Self {
        Self {
            column: SortColumn::Ping,
            dir: 1,
        }
    }
}

// Active toolbar filter state (`FILTERS` in `src/shell/server-browser.js`).
// `name` performs a case-insensitive substring match across `server.name`,
// `server.battle.map_name`, and `server.location`; `map` and `era` default to
// `"all"`; `hide_full` and `hide_empty` toggle capacity filtering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filters {
    pub name: String,
    pub map: String,
    pub era: String,
    pub ping: PingFilter,
    pub hide_full: bool,
    pub hide_empty: bool,
}

impl Default for Filters {
    // Matches `FILTERS = { name: "", map: "all", era: "all", ping: "any", hideFull: false, hideEmpty: false }`
    // in `src/shell/server-browser.js`.
    fn default() -> Self {
        Self {
            name: String::new(),
            map: "all".to_string(),
            era: "all".to_string(),
            ping: PingFilter::Any,
            hide_full: false,
            hide_empty: false,
        }
    }
}

impl Filters {
    // Returns true when any filter diverges from `FILTERS` defaults so the UI
    // can distinguish `"NO SERVERS MATCH THESE FILTERS"` from `"NO SERVERS ONLINE"`.
    pub fn is_filtered(&self) -> bool {
        !self.name.trim().is_empty()
            || self.map != "all"
            || self.era != "all"
            || self.ping != PingFilter::Any
            || self.hide_full
            || self.hide_empty
    }
}

// Single-field mutation passed to `ServerBrowserService::set_filter`, mirroring
// `setFilter(key, value)` in `src/shell/server-browser.js`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterUpdate {
    Name(String),
    Map(String),
    Era(String),
    Ping(PingFilter),
    HideFull(bool),
    HideEmpty(bool),
}

// High-level lifecycle state of the server browser table (`this.state` in
// `src/shell/server-browser.js`: `"idle"`, `"loading"`, `"notice"`, `"ready"`,
// or `"error"` with an uppercase failure message).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BrowserState {
    #[default]
    Idle,
    Loading,
    Notice,
    Ready,
    Error(String),
}

impl BrowserState {
    // Lowercase state discriminator matching `w.state` in `src/shell/server-browser-view.js`
    // (used for placeholder CSS classes `.bfe-browser-empty.{state}`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Notice => "notice",
            Self::Ready => "ready",
            Self::Error(_) => "error",
        }
    }

    // Returns the uppercase error message when the browser is in `BrowserState::Error`.
    pub fn error_message(&self) -> Option<&str> {
        match self {
            Self::Error(msg) => Some(msg.as_str()),
            _ => None,
        }
    }
}

// Distinguishes an initial foreground load (which transitions `state` to
// `Loading` and shows `"SEARCHING FOR SERVERS…"`) from a background periodic
// or retry refresh (`refresh({ quiet: true })` in `src/shell/server-browser.js`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshKind {
    Initial,
    Quiet,
}

// Keyboard navigation keys handled while the server table has focus (`key(code)`
// in `src/shell/server-browser.js`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserKey {
    ArrowUp,
    ArrowDown,
    PageUp,
    PageDown,
    Home,
    End,
}

impl BrowserKey {
    // Maps a DOM `KeyboardEvent.key` string (`"ArrowUp"`, `"ArrowDown"`,
    // `"PageUp"`, `"PageDown"`, `"Home"`, `"End"`) to its `BrowserKey` variant.
    pub fn from_key_name(key: &str) -> Option<Self> {
        match key {
            "ArrowUp" => Some(Self::ArrowUp),
            "ArrowDown" => Some(Self::ArrowDown),
            "PageUp" => Some(Self::PageUp),
            "PageDown" => Some(Self::PageDown),
            "Home" => Some(Self::Home),
            "End" => Some(Self::End),
            _ => None,
        }
    }
}

// Advertised server IPv4/IPv6 endpoint from the master listing or `/info`
// (`endpoints` array in `electron/network.js::withLatency`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EndpointAddr {
    // Numeric IPv4 or IPv6 literal (`"203.0.113.10"` or `"2001:db8::1"`).
    pub address: String,
    // Game / `/info` port on the dedicated server (`1..=65535`).
    pub port: u16,
    // Optional transport label advertised by the master (`"udp"` / `"webrtc"`).
    #[serde(default)]
    pub transport: Option<String>,
    // Whether the master server verified reachability of this endpoint.
    #[serde(default)]
    pub verified: Option<bool>,
}

// Wire and simulation compatibility contract (`NETWORK_CONTRACT` in
// `src/runtime/network-contract.js` and `bfe_transport::layout::network_contract`).
// Two peers can play together iff both `sim` (`simulation`) and `protocol` match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerContract {
    // Wire framing version string (`"BFE-UDP/4"`).
    #[serde(default)]
    pub version: String,
    // Numeric network protocol revision (`network_contract::PROTOCOL`).
    #[serde(default)]
    pub protocol: u32,
    // Active rules/content profile identifier (`network_contract::PROFILE`).
    #[serde(default)]
    pub profile: String,
    // Deterministic simulation contract identifier (`network_contract::SIMULATION`).
    // Accepts both `"sim"` and `"simulation"` in JSON payloads via `serde(alias)`.
    #[serde(alias = "simulation", default)]
    pub sim: String,
    // Server simulation tick rate in Hz (30 Hz in standard Battlefront Eternal).
    #[serde(rename = "tickRate", default)]
    pub tick_rate: u32,
}

impl ServerContract {
    // Returns the active `NETWORK_CONTRACT` constants generated in
    // `bfe_transport::layout::network_contract`.
    pub fn current() -> Self {
        Self {
            version: format!("BFE-UDP/{TRANSPORT_LAYOUT_VERSION}"),
            protocol: network_contract::PROTOCOL as u32,
            profile: network_contract::PROFILE.to_string(),
            sim: network_contract::SIMULATION.to_string(),
            tick_rate: 30,
        }
    }
}

impl Default for ServerContract {
    fn default() -> Self {
        Self::current()
    }
}

// Current battle summary nested under `server.battle` in master and LAN server
// payloads (`src/shell/server-browser.js` and `src/shell/server-browser-view.js`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct BattleSummary {
    // Short map slug (`"nab2"`, `"hot1"`, `"bes2"`, ...).
    #[serde(default)]
    pub map: String,
    // Human-readable map title (`"NABOO: THEED"`, `"HOTH: ECHO BASE"`, ...).
    #[serde(rename = "mapName", default)]
    pub map_name: String,
    // Default faction/side slug (`"rep"`, `"cis"`, `"all"`, `"imp"`).
    #[serde(default)]
    pub side: String,
    // Era code (`"cw"` for Clone Wars, `"gcw"` for Galactic Civil War).
    #[serde(default)]
    pub era: String,
    // Game mode slug (`"conquest"`).
    #[serde(default)]
    pub mode: String,
    // Whether AI bots fill open team slots in this battle.
    #[serde(default)]
    pub bots: Option<bool>,
    // Whether joining players are auto-assigned to balance teams.
    #[serde(rename = "autoAssignTeams", default)]
    pub auto_assign_teams: Option<bool>,
    // Target AI bot count per team (`aiPerTeam` in host/server config).
    #[serde(rename = "aiPerTeam", default)]
    pub ai_per_team: Option<u32>,
    // Hero unit rule setting for the battle, if advertised.
    #[serde(default)]
    pub heroes: Option<String>,
    // Whether friendly fire damage is enabled on this server.
    #[serde(rename = "friendlyFire", default)]
    pub friendly_fire: Option<bool>,
    // AI difficulty tier when bots are enabled.
    #[serde(default)]
    pub difficulty: Option<String>,
    // Starting reinforcement ticket multiplier or count.
    #[serde(default)]
    pub reinforcements: Option<f64>,
    // Target score required for victory in score-based modes.
    #[serde(rename = "victoryScore", default)]
    pub victory_score: Option<f64>,
    // Minimum human players required before the match begins.
    #[serde(rename = "minPlayers", default)]
    pub min_players: Option<u32>,
}

// Entry in `server.rotation`: either a compact battle slug string such as
// `"swbf1/nab2:cw:conquest"` (used by `src/shell/server-browser-view.js`) or a
// structured `{ map, era, mode }` object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RotationItem {
    Slug(String),
    Entry {
        #[serde(default)]
        map: String,
        #[serde(default)]
        era: String,
        #[serde(default)]
        mode: String,
    },
}

impl RotationItem {
    // Extracts the map id from either `"swbf1/kam1:cw:conquest"` or
    // `{ map: "kam1", ... }`, matching `src/shell/server-browser-view.js`.
    pub fn map_id(&self) -> Option<&str> {
        match self {
            Self::Slug(s) => {
                let rest = s.strip_prefix("swbf1/").unwrap_or(s.as_str());
                let id = rest.split(':').next().unwrap_or("");
                (!id.is_empty()).then_some(id)
            }
            Self::Entry { map, .. } => (!map.is_empty()).then_some(map.as_str()),
        }
    }
}

// Untrusted sponsor object received in a server's listing (`server.sponsor` in
// `src/shell/server-browser-view.js`), sanitized via `clean_sponsor` before display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RawSponsor {
    // Raw sponsor organization name (trimmed and capped to 60 chars by `clean_sponsor`).
    #[serde(default)]
    pub name: String,
    // Optional sponsor landing page URL (accepted only when `https://`).
    #[serde(default)]
    pub url: Option<String>,
    // Optional sponsor banner image URL (accepted only when `https://`).
    #[serde(default)]
    pub logo: Option<String>,
}

// Deserialized server listing entry from the master server (`GET /v1/servers`),
// LAN discovery (`BFE-LAN/1`), or direct `/info` query (`electron/network.js`).
// Uses `#[serde(default)]` across fields so partial LAN or mock entries parse
// cleanly without rejecting unknown optional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ServerEntry {
    // 64-char lowercase hex SHA-256 of the server's SPKI DER Base64 public key.
    #[serde(default)]
    pub id: String,
    // Operator-configured server title (`"UNNAMED SERVER"` fallback in UI).
    #[serde(default)]
    pub name: String,
    // Two-line operator description shown in `GAME INFO`.
    #[serde(default)]
    pub description: String,
    // Responder IP populated for LAN discovery and Direct Connect `/info` results.
    #[serde(default)]
    pub address: Option<String>,
    // Responder UDP/HTTP game port (`18451` by default).
    #[serde(default)]
    pub port: Option<u16>,
    // Active human player count currently connected (`c-players` column).
    #[serde(default)]
    pub players: u32,
    // Maximum player capacity (`maxPlayers` in JSON, up to 64).
    #[serde(rename = "maxPlayers", default)]
    pub max_players: u32,
    // Operator-supplied region text (`"Alabama, US"`, `"Frankfurt, DE"`).
    #[serde(default)]
    pub location: String,
    // Optional explicit ISO 3166-1 alpha-2 country code (`"US"`, `"DE"`).
    #[serde(default)]
    pub country: String,
    // Public IPv4/IPv6 endpoints probed concurrently by `with_latency`.
    #[serde(default)]
    pub endpoints: Vec<EndpointAddr>,
    // TURN relay policy (`"auto"`, `"always"`, or `"never"`).
    #[serde(rename = "relayPolicy", default)]
    pub relay_policy: Option<String>,
    // Optional raw battle query string sanitized by `battle_params`.
    #[serde(default)]
    pub query: Option<String>,
    // Content pack identifier (`"swbf1"` by default).
    #[serde(default)]
    pub content: Option<String>,
    // Structured current battle metadata (`map`, `mapName`, `era`, `mode`, ...).
    #[serde(default)]
    pub battle: Option<BattleSummary>,
    // Server-level bot fill override (`BOTS FILL TEAMS` vs `NO BOTS` chip).
    #[serde(default)]
    pub bots: Option<bool>,
    // Server-level team assignment override (`TEAMS ASSIGNED` vs `PICK YOUR TEAM`).
    #[serde(rename = "autoAssignTeams", default)]
    pub auto_assign_teams: Option<bool>,
    // Map rotation queue (`rotation[1]` is displayed as `NEXT` in `GAME INFO`).
    #[serde(default)]
    pub rotation: Vec<RotationItem>,
    // Top-level simulation contract version (`"bfe-sim-..."`).
    #[serde(default)]
    pub simulation: Option<String>,
    // Top-level wire protocol version number.
    #[serde(default)]
    pub protocol: Option<u32>,
    // Nested wire/simulation contract object from `/info`.
    #[serde(default)]
    pub contract: Option<ServerContract>,
    // Server process uptime in seconds, formatted by `uptime_text`.
    #[serde(default)]
    pub uptime: Option<u64>,
    // Optional hosting sponsor attribution (`Hosting Generously Donated By`).
    #[serde(default)]
    pub sponsor: Option<RawSponsor>,
    // Optional `sha-256 XX:XX:...` server key fingerprint.
    #[serde(default)]
    pub fingerprint: Option<String>,
    // Base64-encoded 44-byte SPKI DER Ed25519 public key of the server.
    #[serde(rename = "publicKey", default)]
    pub public_key: Option<String>,
    // Lowest measured TCP connect latency in milliseconds (`None` if unreachable).
    #[serde(rename = "latencyMs", default)]
    pub latency_ms: Option<u32>,
}

// Known map catalog entry (`{ id, name }` in `src/shell/server-browser-view.js`)
// used to populate the map filter dropdown and resolve map slugs in `GAME INFO`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapCatalogEntry {
    pub id: String,
    pub name: String,
}

impl MapCatalogEntry {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
}

// Signal bar tier (`q0`..`q4` in `.bfe-bars`) computed from a server's TCP
// handshake latency in `src/shell/server-browser-view.js::quality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingQuality {
    Good,
    Fair,
    Slow,
    Poor,
    Dead,
}

impl PingQuality {
    // Signal bars (`q0`..`q4`) matching `quality(ms)` in
    // `src/shell/server-browser-view.js`: 4 under 60ms, 3 under 100ms, 2 under
    // 160ms, 1 otherwise, 0 when unknown.
    pub fn bars(self) -> u8 {
        match self {
            Self::Good => 4,
            Self::Fair => 3,
            Self::Slow => 2,
            Self::Poor => 1,
            Self::Dead => 0,
        }
    }
}

// Sanitized sponsor attribution ready for rendering in `.bfe-sponsor` inside
// the `GAME INFO` panel (`src/shell/server-browser-view.js`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SponsorInfo {
    // Sanitized sponsor display name (1..=60 characters).
    pub name: String,
    // Validated `https://` sponsor link href, if any.
    pub url: Option<String>,
    // Validated `https://` sponsor logo URL, if any.
    pub logo: Option<String>,
    // Human-readable link label (`"outerrim.example/servers ↗"`), if `url` is valid.
    pub display_link: Option<String>,
}

// Complete `GAME INFO` inspector view model returned by `describe_server` /
// `ServerBrowserService::describe` (`ServersView.describe` in
// `src/shell/server-browser-view.js`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    // Active map slug (`"nab2"`, `"hot1"`, ...) for `MapPreview` lookup.
    pub map_id: String,
    // Caption shown underneath the map flyover preview frame.
    pub preview_caption: String,
    // Whether the server's simulation and wire protocol match the client's contract.
    pub compatible: bool,
    // Two-letter uppercase ISO country code (`"US"`, `"DE"`, or `""`).
    pub country: String,
    // Regional-indicator flag emoji pair (`"🇺🇸"`, `"🇩🇪"`, or `""`).
    pub flag: String,
    // Uppercase rule badges (`CONQUEST`, `CLONE WARS`, `BOTS FILL TEAMS`, ...).
    pub chips: Vec<String>,
    // Ordered `(label, value)` pairs for `<dl class="bfe-facts">`.
    pub facts: Vec<(&'static str, String)>,
    // Validated hosting sponsor attribution, if present.
    pub sponsor: Option<SponsorInfo>,
}

// Trait contract published for the `shell` lane so the server browser UI can
// drive either the live `ServerBrowser` or a test fixture.
pub trait ServerBrowserService {
    // Returns the active listing source (`Source::Internet` or `Source::Lan`).
    fn source(&self) -> Source;
    // Switches the active source, clearing current rows and returning `true` when
    // the source changed so the caller can start a fresh load.
    fn set_source(&mut self, source: Source) -> bool;
    // Returns the current toolbar filter settings.
    fn filters(&self) -> &Filters;
    // Applies a single filter update and returns `true` if the filter value changed.
    fn set_filter(&mut self, update: FilterUpdate) -> bool;
    // Resets all filters to `Filters::default()` and reconciles selection.
    fn clear_filters(&mut self);
    // Returns the active sort column and direction (`+1` asc, `-1` desc).
    fn sort_spec(&self) -> SortSpec;
    // Sorts by `column`, or reverses direction if `column` is already active.
    fn sort_by(&mut self, column: SortColumn);
    // Returns the filtered and sorted slice of visible server rows.
    fn rows(&self) -> Vec<&ServerEntry>;
    // Returns the currently selected server if it is present in `rows()`.
    fn selected(&self) -> Option<&ServerEntry>;
    // Selects a server by ID (or clears selection when `None` or empty).
    fn select(&mut self, id: Option<&str>);
    // Handles keyboard navigation (`ArrowUp`/`Down`, `PageUp`/`Down`, `Home`/`End`).
    fn key(&mut self, key: BrowserKey, shown_rows: usize) -> bool;
    // Builds the `GAME INFO` inspector model for `server` against `maps`.
    fn describe(&self, server: &ServerEntry, maps: &[MapCatalogEntry]) -> GameInfo;
    // Returns the current browser lifecycle state (`Idle`, `Loading`, `Notice`, `Ready`, `Error`).
    fn state(&self) -> &BrowserState;
    // Checks whether an initial or periodic/retry refresh should be triggered at `now_ms`.
    fn needs_refresh(&self, now_ms: u64) -> Option<RefreshKind>;
}

mod info;
pub use info::*;

// Stateful server browser model porting `class ServerList` in
// `src/shell/server-browser.js`.
//
// Owns:
// - `source`: active directory (`Internet` master list vs `Lan` UDP broadcast).
// - `filters` and `sort`: toolbar filter predicates and active sort column/dir.
// - `servers`: latest completed server list for the current `source`.
// - `selected_id`: ID of the highlighted row, preserved across refreshes and
//   filter edits via `keep_selection` (`keep()` in `src/shell/server-browser.js`).
// - `state`, `stale`, `refreshed_at`, `failed_at`: lifecycle state and timestamps
//   used by `needs_refresh` (`tick()` in `src/shell/server-browser.js`) to
//   schedule 30s periodic renewals and 10s quiet retries while preserving rows.
// - `loads` and `busy`: monotonically increasing request token (`++this.loads`)
//   that discards out-of-order responses when the user switches source or triggers
//   a manual refresh while a previous fetch is still in flight.
pub struct ServerBrowser {
    source: Source,
    filters: Filters,
    sort: SortSpec,
    servers: Vec<ServerEntry>,
    selected_id: Option<String>,
    state: BrowserState,
    stale: String,
    refreshed_at: u64,
    failed_at: u64,
    loads: u64,
    busy: bool,
    contract: ServerContract,
}

impl Default for ServerBrowser {
    // Constructs a `ServerBrowser` bound to the active `ServerContract::current()`
    // wire and simulation constants.
    fn default() -> Self {
        Self::new(ServerContract::current())
    }
}

impl ServerBrowser {
    // Initializes an idle `ServerBrowser` on the `"internet"` source sorted
    // ascending by `Ping`, matching `new ServerList()` in `src/shell/server-browser.js`.
    pub fn new(contract: ServerContract) -> Self {
        Self {
            source: Source::Internet,
            filters: Filters::default(),
            sort: SortSpec::default(),
            servers: Vec::new(),
            selected_id: None,
            state: BrowserState::Idle,
            stale: String::new(),
            refreshed_at: 0,
            failed_at: 0,
            loads: 0,
            busy: false,
            contract,
        }
    }

    // Returns the active compatibility contract checked by `compatible` and `describe`.
    pub fn contract(&self) -> &ServerContract {
        &self.contract
    }

    // Returns the unfiltered server slice from the most recent completed refresh.
    pub fn servers(&self) -> &[ServerEntry] {
        &self.servers
    }

    // Returns the currently highlighted server ID, if any (`this.selectedId` in JS).
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    // Non-empty when the last quiet background renewal failed while `state == Ready`;
    // holds the uppercase reason shown as the tooltip on `"CAN'T UPDATE, RETRYING…"`.
    pub fn stale(&self) -> &str {
        &self.stale
    }

    // Timestamp (ms) when `finish_refresh_ok` last updated `servers` (`this.updatedAt`).
    pub fn refreshed_at(&self) -> u64 {
        self.refreshed_at
    }

    // Timestamp (ms) when a quiet renewal last failed (`this.failedAt`), used to
    // schedule `RETRY_EVERY_MS` (10s) retries instead of `REFRESH_EVERY_MS` (30s).
    pub fn failed_at(&self) -> u64 {
        self.failed_at
    }

    // True while an async master or LAN discovery request is in flight (`this.busy`).
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    // Monotonic token of the latest `begin_refresh` call (`this.loads` in JS).
    pub fn active_load_token(&self) -> u64 {
        self.loads
    }

    // Begins a refresh operation and returns `Some(token)` (`const token = ++this.loads`
    // in `src/shell/server-browser.js`), or `None` if `quiet` is true while a
    // load is already busy (`if (quiet && this.busy) return`).
    //
    // Foreground refreshes (`!quiet`) and initial refreshes when not yet `Ready`
    // transition `state` to `BrowserState::Loading` (`"SEARCHING FOR SERVERS…"`),
    // whereas quiet renewals in `BrowserState::Ready` keep the existing table
    // visible while the background probe runs.
    pub fn begin_refresh(&mut self, quiet: bool) -> Option<u64> {
        if quiet && self.busy {
            return None;
        }
        self.loads += 1;
        self.busy = true;
        if !(quiet && self.state == BrowserState::Ready) {
            self.state = BrowserState::Loading;
        }
        Some(self.loads)
    }

    // Applies a completed server list if `token` matches `self.loads`
    // (`if (token !== this.loads) return` in `src/shell/server-browser.js`).
    // Clears any stale warning, records `refreshed_at = now_ms`, transitions
    // `state` to `BrowserState::Ready`, and runs `keep_selection()` so the
    // player's highlighted server stays selected if still present.
    pub fn finish_refresh_ok(&mut self, token: u64, list: Vec<ServerEntry>, now_ms: u64) -> bool {
        if token != self.loads {
            return false;
        }
        self.busy = false;
        self.stale.clear();
        self.servers = list;
        self.state = BrowserState::Ready;
        self.refreshed_at = now_ms;
        self.failed_at = 0;
        self.keep_selection();
        true
    }

    // Transitions to `BrowserState::Notice` (`"ONLINE PLAY NEEDS YOUR OK FIRST"`
    // in `src/shell/server-browser-view.js`) when the master bridge reports that
    // the player has not yet accepted `ONLINE_NOTICE_VERSION`.
    pub fn finish_refresh_notice(&mut self, token: u64) -> bool {
        if token != self.loads {
            return false;
        }
        self.busy = false;
        self.stale.clear();
        self.servers.clear();
        self.selected_id = None;
        self.state = BrowserState::Notice;
        true
    }

    // Applies a failed refresh if `token` matches `self.loads`.
    // Strips IPC prefixes via `error_text`, uppercases the message (defaulting to
    // `"THE SERVER LIST COULD NOT BE LOADED"`), and either:
    // - preserves `self.servers` and records `self.stale` + `self.failed_at` when
    //   a `quiet` background renewal fails in `BrowserState::Ready`, or
    // - clears `self.servers` and transitions to `BrowserState::Error(msg)`.
    pub fn finish_refresh_err(&mut self, token: u64, quiet: bool, err: &str, now_ms: u64) -> bool {
        if token != self.loads {
            return false;
        }
        self.busy = false;
        let cleaned = error_text(err).to_ascii_uppercase();
        let msg = if cleaned.is_empty() {
            "THE SERVER LIST COULD NOT BE LOADED".to_string()
        } else {
            cleaned
        };
        if quiet && self.state == BrowserState::Ready {
            self.stale = msg;
            self.failed_at = now_ms;
        } else {
            self.servers.clear();
            self.selected_id = None;
            self.state = BrowserState::Error(msg);
        }
        true
    }

    // Ports the six-clause filter predicate in `ServerList.rows()` in
    // `src/shell/server-browser.js`:
    // 1. `name`: case-insensitive substring match against `server.name`,
    //    `server.battle.map_name`, or `server.location`.
    // 2. `map`: exact match on `server.battle.map` unless `"all"`.
    // 3. `era`: exact match on `server.battle.era` unless `"all"`.
    // 4. `ping`: requires a finite `latency_ms < threshold` unless `PingFilter::Any`.
    // 5. `hide_full`: excludes servers where `players >= max_players` (for `max_players > 0`).
    // 6. `hide_empty`: excludes servers where `players == 0`.
    pub fn matches_filters(&self, server: &ServerEntry) -> bool {
        let q = self.filters.name.trim().to_ascii_lowercase();
        if !q.is_empty() {
            let name = server.name.to_ascii_lowercase();
            let map_name = server
                .battle
                .as_ref()
                .map(|b| b.map_name.to_ascii_lowercase())
                .unwrap_or_default();
            let loc = server.location.to_ascii_lowercase();
            if !name.contains(&q) && !map_name.contains(&q) && !loc.contains(&q) {
                return false;
            }
        }
        if self.filters.map != "all" {
            let map = server.battle.as_ref().map(|b| b.map.as_str()).unwrap_or("");
            if map != self.filters.map {
                return false;
            }
        }
        if self.filters.era != "all" {
            let era = server.battle.as_ref().map(|b| b.era.as_str()).unwrap_or("");
            if era != self.filters.era {
                return false;
            }
        }
        if let Some(max_ping) = self.filters.ping.threshold_ms() {
            let Some(ms) = server.latency_ms else {
                return false;
            };
            if ms >= max_ping {
                return false;
            }
        }
        if self.filters.hide_full
            && server.max_players > 0
            && server.players >= server.max_players
        {
            return false;
        }
        if self.filters.hide_empty && server.players == 0 {
            return false;
        }
        true
    }

    // Ports `compare(column.value(a), column.value(b)) * this.sort.dir || i - j`
    // in `src/shell/server-browser.js`.
    //
    // Key behaviors preserved from the JS implementation:
    // - `SortColumn::Ping` maps `None` (`!Number.isFinite(s.latencyMs)`) to
    //   `u64::MAX` (`Infinity` in JS) so unanswered servers sort after answered
    //   ones under ascending ping sort.
    // - `SortColumn::Map` compares `battle.map_name` when non-empty and falls
    //   back to the raw `battle.map` slug (`s.battle?.mapName || s.battle?.map`).
    // - Ties after applying `self.sort.dir` always break by the original listing
    //   index `a.0.cmp(&b.0)` (`|| i - j` in JS), independent of `sort.dir`.
    fn compare_entries(&self, a: (usize, &ServerEntry), b: (usize, &ServerEntry)) -> Ordering {
        let ord = match self.sort.column {
            SortColumn::Players => a.1.players.cmp(&b.1.players),
            SortColumn::Ping => {
                let av = a.1.latency_ms.map(u64::from).unwrap_or(u64::MAX);
                let bv = b.1.latency_ms.map(u64::from).unwrap_or(u64::MAX);
                av.cmp(&bv)
            }
            SortColumn::Name => a
                .1
                .name
                .to_ascii_lowercase()
                .cmp(&b.1.name.to_ascii_lowercase()),
            SortColumn::Map => {
                let am = a
                    .1
                    .battle
                    .as_ref()
                    .map(|bt| {
                        if !bt.map_name.is_empty() {
                            bt.map_name.as_str()
                        } else {
                            bt.map.as_str()
                        }
                    })
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let bm = b
                    .1
                    .battle
                    .as_ref()
                    .map(|bt| {
                        if !bt.map_name.is_empty() {
                            bt.map_name.as_str()
                        } else {
                            bt.map.as_str()
                        }
                    })
                    .unwrap_or("")
                    .to_ascii_lowercase();
                am.cmp(&bm)
            }
            SortColumn::Mode => {
                let am = a
                    .1
                    .battle
                    .as_ref()
                    .map(|bt| bt.mode.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let bm = b
                    .1
                    .battle
                    .as_ref()
                    .map(|bt| bt.mode.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                am.cmp(&bm)
            }
            SortColumn::Era => {
                let ae = a
                    .1
                    .battle
                    .as_ref()
                    .map(|bt| bt.era.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let be = b
                    .1
                    .battle
                    .as_ref()
                    .map(|bt| bt.era.as_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                ae.cmp(&be)
            }
            SortColumn::Location => a
                .1
                .location
                .to_ascii_lowercase()
                .cmp(&b.1.location.to_ascii_lowercase()),
        };
        let directed = if self.sort.dir < 0 {
            ord.reverse()
        } else {
            ord
        };
        if directed == Ordering::Equal {
            a.0.cmp(&b.0)
        } else {
            directed
        }
    }

    // Ports `keep()` in `src/shell/server-browser.js`:
    //   if (!rows.some(server => server.id === this.selectedId))
    //     this.selectedId = rows[0]?.id ?? null;
    // Ensures the `GAME INFO` inspector and keyboard cursor always point to a
    // visible row whenever the filtered list is non-empty, or `None` when empty.
    pub fn keep_selection(&mut self) {
        let visible_ids: Vec<String> = self.rows().into_iter().map(|s| s.id.clone()).collect();
        if let Some(current) = &self.selected_id {
            if visible_ids.iter().any(|id| id == current) {
                return;
            }
        }
        self.selected_id = visible_ids.into_iter().next();
    }
}

impl ServerBrowserService for ServerBrowser {
    fn source(&self) -> Source {
        self.source
    }

    // Ports `setSource(source)` in `src/shell/server-browser.js`. Returns true
    // when the source changed so the caller can trigger a refresh.
    fn set_source(&mut self, next: Source) -> bool {
        if self.source == next {
            return false;
        }
        self.source = next;
        self.servers.clear();
        self.selected_id = None;
        self.stale.clear();
        true
    }

    fn filters(&self) -> &Filters {
        &self.filters
    }

    // Ports `setFilter(key, value)` in `src/shell/server-browser.js`: mutates
    // the targeted filter field and, when the value actually changed, re-runs
    // `keep_selection()` so a newly filtered-out selection snaps to `rows()[0]`.
    fn set_filter(&mut self, update: FilterUpdate) -> bool {
        let changed = match update {
            FilterUpdate::Name(v) => {
                let changed = self.filters.name != v;
                self.filters.name = v;
                changed
            }
            FilterUpdate::Map(v) => {
                let changed = self.filters.map != v;
                self.filters.map = v;
                changed
            }
            FilterUpdate::Era(v) => {
                let changed = self.filters.era != v;
                self.filters.era = v;
                changed
            }
            FilterUpdate::Ping(v) => {
                let changed = self.filters.ping != v;
                self.filters.ping = v;
                changed
            }
            FilterUpdate::HideFull(v) => {
                let changed = self.filters.hide_full != v;
                self.filters.hide_full = v;
                changed
            }
            FilterUpdate::HideEmpty(v) => {
                let changed = self.filters.hide_empty != v;
                self.filters.hide_empty = v;
                changed
            }
        };
        if changed {
            self.keep_selection();
        }
        changed
    }

    // Resets all toolbar filters to `Filters::default()` (`FILTERS` in JS) and
    // reconciles `selected_id` against the newly unfiltered row order.
    fn clear_filters(&mut self) {
        self.filters = Filters::default();
        self.keep_selection();
    }

    fn sort_spec(&self) -> SortSpec {
        self.sort
    }

    // Ports `sortBy(key)` in `src/shell/server-browser.js`:
    //   this.sort = this.sort.key === key
    //     ? { key, dir: -this.sort.dir }
    //     : { key, dir: column.first ?? 1 };
    // Selecting `SortColumn::Players` for the first time starts descending (`-1`,
    // fullest servers first), whereas all other columns start ascending (`+1`).
    fn sort_by(&mut self, column: SortColumn) {
        if self.sort.column == column {
            self.sort.dir = -self.sort.dir;
        } else {
            self.sort.column = column;
            self.sort.dir = if column == SortColumn::Players { -1 } else { 1 };
        }
    }

    // Ports `rows()` in `src/shell/server-browser.js`: filters `self.servers`
    // with `matches_filters`, pairs each surviving entry with its original
    // listing index for stable tie-breaking, and sorts via `compare_entries`.
    fn rows(&self) -> Vec<&ServerEntry> {
        let mut indexed: Vec<(usize, &ServerEntry)> = self
            .servers
            .iter()
            .enumerate()
            .filter(|(_, s)| self.matches_filters(s))
            .collect();
        indexed.sort_by(|&a, &b| self.compare_entries(a, b));
        indexed.into_iter().map(|(_, s)| s).collect()
    }

    // Ports `selected()` in `src/shell/server-browser.js`: looks up `selected_id`
    // within the currently visible `rows()` slice so a hidden server is never
    // returned as selected.
    fn selected(&self) -> Option<&ServerEntry> {
        let id = self.selected_id.as_deref()?;
        self.rows().into_iter().find(|s| s.id == id)
    }

    // Explicitly sets `selected_id` from a row click or programmatic selection,
    // normalizing empty strings to `None`.
    fn select(&mut self, id: Option<&str>) {
        self.selected_id = id.filter(|s| !s.is_empty()).map(str::to_string);
    }

    // Ports `key(code)` in `src/shell/server-browser.js`:
    // - `ArrowUp` at row 0 and `ArrowDown` at `rows.len() - 1` return `false` so
    //   the menu screen can hand focus off to the filter bar above or the action
    //   buttons (`BACK`, `REFRESH`, `DIRECT CONNECT`, `JOIN`) below.
    // - `PageUp` and `PageDown` step by `max(shown_rows - 1, 1)` (`const page =
    //   Math.max(this.shown - 1, 1)` in JS), clamped to `0..=last`.
    // - `Home` and `End` jump to the first and last visible rows.
    fn key(&mut self, key: BrowserKey, shown_rows: usize) -> bool {
        let visible_ids: Vec<String> = self.rows().into_iter().map(|s| s.id.clone()).collect();
        if visible_ids.is_empty() {
            return false;
        }
        let current = self
            .selected_id
            .as_ref()
            .and_then(|sel| visible_ids.iter().position(|id| id == sel))
            .unwrap_or(0);
        let page = shown_rows.saturating_sub(1).max(1);
        let last = visible_ids.len() - 1;
        let next = match key {
            BrowserKey::ArrowUp => {
                if current == 0 {
                    return false;
                }
                current - 1
            }
            BrowserKey::ArrowDown => {
                if current >= last {
                    return false;
                }
                current + 1
            }
            BrowserKey::PageUp => current.saturating_sub(page),
            BrowserKey::PageDown => (current + page).min(last),
            BrowserKey::Home => 0,
            BrowserKey::End => last,
        };
        self.selected_id = Some(visible_ids[next].clone());
        true
    }

    fn describe(&self, server: &ServerEntry, maps: &[MapCatalogEntry]) -> GameInfo {
        describe_server(server, maps, &self.contract)
    }

    fn state(&self) -> &BrowserState {
        &self.state
    }

    // Ports `tick()` and `onEnter` in `src/shell/server-browser.js`:
    // - Returns `None` while a load is already in flight (`busy` or `Loading`).
    // - Returns `Some(RefreshKind::Initial)` when `Idle` so entering the screen
    //   kicks off the initial fetch.
    // - When `Ready`, returns `Some(RefreshKind::Quiet)` after `RETRY_EVERY_MS`
    //   (10s) if the previous quiet renewal failed (`!stale.is_empty()`), or
    //   after `REFRESH_EVERY_MS` (30s) following a clean refresh.
    fn needs_refresh(&self, now_ms: u64) -> Option<RefreshKind> {
        if self.busy || self.state == BrowserState::Loading {
            return None;
        }
        if self.state == BrowserState::Idle {
            return Some(RefreshKind::Initial);
        }
        if self.state != BrowserState::Ready {
            return None;
        }
        if !self.stale.is_empty() {
            if now_ms.saturating_sub(self.failed_at) > RETRY_EVERY_MS {
                return Some(RefreshKind::Quiet);
            }
        } else if now_ms.saturating_sub(self.refreshed_at) > REFRESH_EVERY_MS {
            return Some(RefreshKind::Quiet);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Builds a 4-server fixture covering the browser's sorting, filtering, and
    // compatibility cases:
    // - `"a"` (`Alpha`): 12/64 players, 90ms ping, US, Clone Wars on `nab2`,
    //   2-battle rotation, uptime 2h32m, bots + auto-teams enabled, sponsored.
    // - `"b"` (`Bravo`): 64/64 full, 20ms lowest ping, US, GCW on `hot1`.
    // - `"c"` (`Charlie`): 0/64 empty, `None` ping (unanswered), DE, Clone Wars.
    // - `"d"` (`Delta`): 3/64 players, 140ms ping, incompatible `simulation`.
    fn test_servers() -> Vec<ServerEntry> {
        let contract = ServerContract::current();
        vec![
            ServerEntry {
                id: "a".to_string(),
                name: "Alpha".to_string(),
                players: 12,
                max_players: 64,
                latency_ms: Some(90),
                location: "Alabama, US".to_string(),
                simulation: Some(contract.sim.clone()),
                protocol: Some(contract.protocol),
                uptime: Some(9_120),
                bots: Some(true),
                auto_assign_teams: Some(true),
                rotation: vec![
                    RotationItem::Slug("swbf1/nab2:cw:conquest".to_string()),
                    RotationItem::Slug("swbf1/hot1:gcw:conquest".to_string()),
                ],
                sponsor: Some(RawSponsor {
                    name: " Outer Rim Hosting ".to_string(),
                    url: Some("https://www.outerrim.example/servers/".to_string()),
                    logo: Some("https://cdn.outerrim.example/banner.png".to_string()),
                }),
                battle: Some(BattleSummary {
                    map: "nab2".to_string(),
                    map_name: "NABOO: THEED".to_string(),
                    era: "cw".to_string(),
                    mode: "conquest".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ServerEntry {
                id: "b".to_string(),
                name: "Bravo".to_string(),
                players: 64,
                max_players: 64,
                latency_ms: Some(20),
                location: "Alabama, US".to_string(),
                simulation: Some(contract.sim.clone()),
                protocol: Some(contract.protocol),
                battle: Some(BattleSummary {
                    map: "hot1".to_string(),
                    map_name: "HOTH: ECHO BASE".to_string(),
                    era: "gcw".to_string(),
                    mode: "conquest".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ServerEntry {
                id: "c".to_string(),
                name: "Charlie".to_string(),
                players: 0,
                max_players: 64,
                latency_ms: None,
                location: "Frankfurt, DE".to_string(),
                simulation: Some(contract.sim.clone()),
                protocol: Some(contract.protocol),
                battle: Some(BattleSummary {
                    map: "nab2".to_string(),
                    map_name: "NABOO: THEED".to_string(),
                    era: "cw".to_string(),
                    mode: "conquest".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ServerEntry {
                id: "d".to_string(),
                name: "Delta".to_string(),
                players: 3,
                max_players: 64,
                latency_ms: Some(140),
                location: "Alabama, US".to_string(),
                simulation: Some("bfe-sim-1".to_string()),
                protocol: Some(contract.protocol),
                battle: Some(BattleSummary {
                    map: "nab2".to_string(),
                    map_name: "NABOO: THEED".to_string(),
                    era: "cw".to_string(),
                    mode: "conquest".to_string(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ]
    }

    // Verifies default ascending ping order (`20ms`, `90ms`, `140ms`, `None`),
    // auto-selection of the first visible row (`"b"`), `SortColumn::Players`
    // starting descending (`first: -1` in `src/shell/server-browser.js`) before
    // toggling to ascending on a second click, and case-insensitive `Name` sort.
    #[test]
    fn sorts_by_ping_by_default_unknown_last_and_players_starts_fullest(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut browser = ServerBrowser::default();
        let token = browser.begin_refresh(false).ok_or("expected refresh token")?;
        browser.finish_refresh_ok(token, test_servers(), 1_000);

        // Default sort is ascending `Ping` (`b=20ms`, `a=90ms`, `d=140ms`, `c=None/MAX`),
        // and `keep_selection()` selects the first visible row (`"b"`).
        assert_eq!(browser.state(), &BrowserState::Ready);
        let ids: Vec<&str> = browser.rows().into_iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "a", "d", "c"]);
        assert_eq!(browser.selected().map(|s| s.id.as_str()), Some("b"));

        // First click on `PLAYERS` sorts descending (`first: -1` -> 64, 12, 3, 0).
        browser.sort_by(SortColumn::Players);
        assert_eq!(
            browser.rows().into_iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "a", "d", "c"]
        );

        // Second click on `PLAYERS` reverses direction to ascending (0, 3, 12, 64).
        browser.sort_by(SortColumn::Players);
        assert_eq!(
            browser.rows().into_iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "d", "a", "b"]
        );

        // Clicking `GAME NAME` sorts alphabetically (`Alpha`, `Bravo`, `Charlie`, `Delta`).
        browser.sort_by(SortColumn::Name);
        assert_eq!(
            browser.rows().into_iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c", "d"]
        );
        Ok(())
    }

    // Verifies every filter predicate (`Name` matching map title and location,
    // `Era`, `PingFilter::Under100` excluding both 140ms and `None`, `HideFull`,
    // `HideEmpty`, and `Map`) and checks that `keep_selection()` advances the
    // selection when the previously selected server is filtered out.
    #[test]
    fn filters_by_name_map_era_ping_full_and_empty_and_preserves_selection(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut browser = ServerBrowser::default();
        let token = browser.begin_refresh(false).ok_or("expected refresh token")?;
        browser.finish_refresh_ok(token, test_servers(), 1_000);

        // Search query matches `battle.map_name` (`"HOTH: ECHO BASE"` on `"b"`).
        browser.set_filter(FilterUpdate::Name("hoth".to_string()));
        assert_eq!(
            browser.rows().into_iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );

        // Search query also matches `server.location` (`"Frankfurt, DE"` on `"c"`).
        browser.set_filter(FilterUpdate::Name("frankfurt".to_string()));
        assert_eq!(
            browser.rows().into_iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["c"]
        );

        // Clearing the search query and filtering by `era = "gcw"` isolates `"b"`.
        browser.set_filter(FilterUpdate::Name(String::new()));
        browser.set_filter(FilterUpdate::Era("gcw".to_string()));
        assert_eq!(
            browser.rows().into_iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );

        // `PingFilter::Under100` keeps 20ms (`"b"`) and 90ms (`"a"`), dropping 140ms and `None`.
        browser.set_filter(FilterUpdate::Era("all".to_string()));
        browser.set_filter(FilterUpdate::Ping(PingFilter::Under100));
        assert_eq!(
            browser.rows().into_iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"]
        );

        // `HideFull(true)` hides `"b"` (64/64) and `keep_selection()` snaps to `"a"`.
        browser.set_filter(FilterUpdate::Ping(PingFilter::Any));
        browser.set_filter(FilterUpdate::HideFull(true));
        assert!(!browser.rows().iter().any(|s| s.id == "b"));
        assert_eq!(browser.selected().map(|s| s.id.as_str()), Some("a"));

        // Adding `HideEmpty(true)` also hides `"c"` (0/64), leaving `"a"` and `"d"`.
        browser.set_filter(FilterUpdate::HideEmpty(true));
        assert_eq!(
            browser.rows().into_iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "d"]
        );

        // Filtering by `"hot1"` while `HideFull` is active leaves zero matching rows.
        browser.set_filter(FilterUpdate::Map("hot1".to_string()));
        assert!(browser.rows().is_empty());
        assert!(browser.selected().is_none());
        Ok(())
    }

    // Verifies keyboard navigation (`ArrowUp`, `ArrowDown`, `End`, `Home`,
    // `PageDown` with `shown_rows = 3` -> step of 2) and confirms that pressing
    // `ArrowUp` on the first row or `ArrowDown` on the last row returns `false`
    // to hand focus off to surrounding screen widgets.
    #[test]
    fn navigates_rows_with_keys_and_hands_off_focus_at_ends(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut browser = ServerBrowser::default();
        let token = browser.begin_refresh(false).ok_or("expected refresh token")?;
        browser.finish_refresh_ok(token, test_servers(), 1_000);

        assert_eq!(browser.selected_id(), Some("b"));
        assert!(!browser.key(BrowserKey::ArrowUp, 3));
        assert!(browser.key(BrowserKey::ArrowDown, 3));
        assert_eq!(browser.selected_id(), Some("a"));
        assert!(browser.key(BrowserKey::End, 3));
        assert_eq!(browser.selected_id(), Some("c"));
        assert!(!browser.key(BrowserKey::ArrowDown, 3));
        assert!(browser.key(BrowserKey::Home, 3));
        assert!(browser.key(BrowserKey::PageDown, 3));
        assert_eq!(browser.selected_id(), Some("d"));
        Ok(())
    }

    // Verifies `describe_server` (`GAME INFO` chips, US regional-indicator flag,
    // rotation next-map lookup, uptime formatting, version compatibility badge,
    // and link classification) along with `battle_params` query allowlisting.
    #[test]
    fn describes_game_info_facts_chips_flags_sponsor_and_battle_params() {
        let browser = ServerBrowser::default();
        let servers = test_servers();
        let maps = vec![
            MapCatalogEntry::new("nab2", "Naboo: Theed"),
            MapCatalogEntry::new("hot1", "Hoth: Echo Base"),
        ];

        let info_a = browser.describe(&servers[0], &maps);
        assert!(info_a.compatible);
        assert_eq!(info_a.country, "US");
        assert_eq!(info_a.flag, "\u{1F1FA}\u{1F1F8}");
        assert_eq!(
            info_a.chips,
            vec![
                "CONQUEST",
                "CLONE WARS",
                "BOTS FILL TEAMS",
                "TEAMS ASSIGNED"
            ]
        );
        assert_eq!(info_a.facts[0], ("PLAYERS", "12 OF 64".to_string()));
        assert_eq!(info_a.facts[1], ("PING", "90 MS".to_string()));
        assert_eq!(info_a.facts[2], ("LOCATION", "Alabama, US".to_string()));
        assert_eq!(info_a.facts[3], ("NEXT", "Hoth: Echo Base".to_string()));
        assert_eq!(info_a.facts[4], ("ROTATION", "2 BATTLES".to_string()));
        assert_eq!(info_a.facts[5], ("UP FOR", "2H 32M".to_string()));
        assert_eq!(info_a.facts[7], ("LINK", "DIRECT OR RELAY".to_string()));

        let info_d = browser.describe(&servers[3], &maps);
        assert!(!info_d.compatible);
        assert_eq!(
            info_d.facts[6],
            ("VERSION", "UPDATE TO JOIN".to_string())
        );

        let raw_q = format!(
            "?map=bes2&side=imp&ai=off&seed=1&match={}&noreload=1&shell=1&ai=classic&era=gcw",
            "a".repeat(32)
        );
        assert_eq!(
            battle_params(Some(&raw_q)),
            format!(
                "map=bes2&side=imp&ai=off&seed=1&match={}&era=gcw",
                "a".repeat(32)
            )
        );
        assert_eq!(
            battle_params(Some("?map=../x&side=rep&ai=enhanced&seed=12345678901")),
            "side=rep"
        );
        assert_eq!(battle_params(None), "");
    }

    // Verifies the refresh state machine (`Initial` when idle, `Quiet` after
    // 30s when healthy, concurrent quiet refresh suppression while `busy`,
    // stale-row preservation + 10s retry cadence on quiet failure, and Electron
    // IPC prefix stripping in `error_text`).
    #[test]
    fn schedules_periodic_refresh_and_quiet_retry_and_formats_errors(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut browser = ServerBrowser::default();
        assert_eq!(browser.needs_refresh(0), Some(RefreshKind::Initial));

        let token = browser.begin_refresh(false).ok_or("expected refresh token")?;
        browser.finish_refresh_ok(token, test_servers(), 0);
        assert_eq!(browser.needs_refresh(30_000), None);
        assert_eq!(browser.needs_refresh(31_000), Some(RefreshKind::Quiet));

        let token2 = browser.begin_refresh(true).ok_or("expected refresh token")?;
        // While busy, another quiet refresh is not started.
        assert_eq!(browser.begin_refresh(true), None);
        browser.finish_refresh_err(
            token2,
            true,
            "Error invoking remote method 'bfe:network': Error: Can't reach the online service (ETIMEDOUT). Check your connection and try again.",
            31_000,
        );
        assert_eq!(browser.state(), &BrowserState::Ready);
        assert_eq!(browser.rows().len(), 4);
        assert_eq!(
            browser.stale(),
            "CAN'T REACH THE ONLINE SERVICE (ETIMEDOUT). CHECK YOUR CONNECTION AND TRY AGAIN."
        );
        assert_eq!(browser.needs_refresh(36_000), None);
        assert_eq!(browser.needs_refresh(42_000), Some(RefreshKind::Quiet));

        assert_eq!(
            error_text("Error invoking remote method 'bfe:network': TypeError: fetch failed"),
            "fetch failed"
        );
        assert_eq!(
            error_text("Error: Accept the online privacy notice first."),
            "Accept the online privacy notice first."
        );
        Ok(())
    }
}
