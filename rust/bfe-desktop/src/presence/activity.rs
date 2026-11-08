//! Rich presence activity formatting and state helpers for Discord and Steam Friends.
//!
//! Mirrors `electron/presence/activity.js` (`ERAS`, `SIDES`, `SCREENS`,
//! `DETAIL_LEVELS`, `fit`, `titleCase`, `discordActivity`) and
//! `electron/presence/application.js` (`DISCORD_APPLICATION_ID`, `DISCORD_LARGE_IMAGE`).
//!
//! ## Activity Formatting Rules
//! - **Privacy detail levels (`DETAIL_LEVELS`)**:
//!   - `"off"` suppresses rich presence entirely (`discord_activity` returns `None`).
//!   - `"game"` publishes only the base start timestamp and application icon (`"Battlefront Eternal"`),
//!     hiding the user's current scene, map, faction, and server name.
//!   - `"full"` publishes scene-specific `details`, `state`, and multiplayer `party` metadata.
//! - **Discord string constraints (`fit`)**:
//!   Discord rejects activity strings shorter than 2 characters or longer than 128 characters.
//!   [`fit`] collapses whitespace runs, drops strings under 2 characters, and truncates strings
//!   over `max` characters with a single horizontal ellipsis (`\u{2026}`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Battlefront Eternal's public Discord Application ID.
///
/// Mirrors `DISCORD_APPLICATION_ID` in `electron/presence/application.js`.
pub const DISCORD_APPLICATION_ID: &str = "1561126739894210652";

/// Rich Presence large image asset key (`"logo"`).
///
/// Mirrors `DISCORD_LARGE_IMAGE` in `electron/presence/application.js`.
pub const DISCORD_LARGE_IMAGE: &str = "logo";

/// Supported Discord presence detail levels (`"full"`, `"game"`, `"off"`).
///
/// Mirrors `DETAIL_LEVELS` in `electron/presence/activity.js`.
pub const DETAIL_LEVELS: &[&str] = &["full", "game", "off"];

/// Returns the display name for an era code (`"cw"` -> `"Clone Wars"`, `"gcw"` -> `"Galactic Civil War"`).
///
/// Mirrors `ERAS` in `electron/presence/activity.js`.
pub fn era_name(era: &str) -> Option<&'static str> {
    // Map lowercase era codes (`cw`, `gcw`) to their full Star Wars era labels.
    match era {
        "cw" => Some("Clone Wars"),
        "gcw" => Some("Galactic Civil War"),
        _ => None,
    }
}

/// Returns the display name for a side code (`"rep"`, `"cis"`, `"imp"`, `"all"`).
///
/// Mirrors `SIDES` in `electron/presence/activity.js`.
pub fn side_name(side: &str) -> Option<&'static str> {
    // Map lowercase 3-letter side codes to their Discord rich presence labels.
    match side {
        "rep" => Some("Republic"),
        "cis" => Some("CIS"),
        "imp" => Some("Empire"),
        "all" => Some("Rebel Alliance"),
        _ => None,
    }
}

/// Returns the menu subtitle for a front-end screen ID (`"ia"`, `"sp"`, `"mp"`).
///
/// Mirrors `SCREENS` in `electron/presence/activity.js`.
pub fn screen_subtitle(screen: &str) -> Option<&'static str> {
    // Map front-end menu screen identifiers to their secondary activity text.
    match screen {
        "ia" | "sp" => Some("Choosing Instant Action maps"),
        "mp" => Some("Looking for a server"),
        _ => None,
    }
}

/// Fits a string into Discord's 2..=128 character range by collapsing runs of
/// whitespace, returning `None` when fewer than 2 characters remain, and
/// truncating with a single ellipsis (`\u{2026}`) when longer than `max`.
///
/// Mirrors `fit` in `electron/presence/activity.js`.
pub fn fit(text: Option<&str>, max: usize) -> Option<String> {
    // Collapse consecutive ASCII/Unicode whitespace into single spaces.
    let raw = text?;
    let value: String = raw.split_whitespace().collect::<Vec<&str>>().join(" ");
    let count = value.chars().count();
    // Discord Rich Presence requires strings to be at least 2 characters long.
    if count < 2 {
        return None;
    }
    if count <= max {
        Some(value)
    } else {
        // Reserve 1 character for the trailing ellipsis (`\u{2026}`).
        let prefix: String = value.chars().take(max.saturating_sub(1)).collect();
        Some(format!("{}\u{2026}", prefix.trim_end()))
    }
}

/// Converts an all-uppercase catalog title such as `"NABOO: THEED"` into
/// title case (`"Naboo: Theed"`), leaving mixed-case strings untouched.
///
/// Mirrors `titleCase` in `electron/presence/activity.js`.
pub fn title_case(name: &str) -> String {
    // Preserve strings that already contain lowercase characters.
    let text = name.trim();
    if text.is_empty() || text != text.to_uppercase() {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut capitalize_next = true;
    for ch in lower.chars() {
        if capitalize_next && ch.is_alphabetic() {
            for upper in ch.to_uppercase() {
                out.push(upper);
            }
            capitalize_next = false;
        } else {
            out.push(ch);
            // Capitalize the first letter following whitespace or punctuation separators (`:`, `(`, `/`, `-`).
            capitalize_next = ch.is_whitespace() || matches!(ch, ':' | '(' | '/' | '-');
        }
    }
    out
}

/// Server metadata attached to a [`PresenceSceneState`] during an online or LAN battle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceServerInfo {
    /// Sanitized server display name (`<= 64` chars).
    pub name: Option<String>,
    /// Current player count reported by the server list.
    pub players: Option<u32>,
    /// Maximum player capacity of the server.
    pub max_players: Option<u32>,
    /// True if the local user is hosting this server.
    pub hosted: bool,
}

/// Scene state consumed by [`discord_activity`].
///
/// Mirrors the `state` object passed to `discordActivity` in `electron/presence/activity.js`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceSceneState {
    /// Active scene (`"loader"`, `"menu"`, `"battle"`, or `"unknown"`).
    pub scene: String,
    /// Loader mode when `scene == "loader"` (`Some("update")`, `Some("preparing")`, or `None`).
    pub loader: Option<String>,
    /// Active front-end screen when `scene == "menu"` (`"ia"`, `"sp"`, `"mp"`, etc.).
    pub screen: Option<String>,
    /// Map ID when `scene == "battle"` (`"bes2a"`, etc.).
    pub map: Option<String>,
    /// Catalog map name (`"NABOO: THEED"` or `"Naboo: Theed"`).
    pub map_name: Option<String>,
    /// Era code (`"cw"` or `"gcw"`).
    pub era: Option<String>,
    /// Side code (`"rep"`, `"cis"`, `"imp"`, `"all"`).
    pub side: Option<String>,
    /// True if the battle is a network match (online or LAN).
    pub network: bool,
    /// True if the network battle is a direct LAN match.
    pub lan: bool,
    /// Known server details for a network match.
    pub server: Option<PresenceServerInfo>,
    /// Live player count reported by the battle session.
    pub players: Option<u32>,
    /// Per-session salted party identifier for a network server.
    pub party_id: Option<String>,
    /// Battle start timestamp (seconds or milliseconds).
    pub since: Option<u64>,
    /// Application start timestamp (seconds or milliseconds).
    pub started: Option<u64>,
}

/// Builds the Discord Rich Presence activity object for `state`, `detail`
/// (`"full"`, `"game"`, or `"off"`), and art asset key `image`.
///
/// Returns `None` when `detail == "off"` or `state.scene == "unknown"`.
/// Mirrors `discordActivity` in `electron/presence/activity.js`.
pub fn discord_activity(
    state: &PresenceSceneState,
    detail: &str,
    image: &str,
) -> Option<Value> {
    // Respect the user's privacy setting ("off") and ignore unknown scenes.
    if detail == "off" || state.scene.is_empty() || state.scene == "unknown" {
        return None;
    }
    let start_ts = state.started.unwrap_or(0);
    let mut activity = serde_json::Map::new();
    activity.insert("timestamps".to_string(), json!({ "start": start_ts }));
    if !image.is_empty() {
        activity.insert(
            "assets".to_string(),
            json!({
                "large_image": image,
                "large_text": "Battlefront Eternal",
            }),
        );
    }
    // In "game" detail mode, omit scene/map/server details and publish only the running state.
    if detail != "full" {
        return Some(Value::Object(activity));
    }

    // Prefer the title-cased catalog map name (`"Naboo: Theed"`), falling back to the uppercase map ID.
    let resolved_map = state
        .map_name
        .as_deref()
        .map(title_case)
        .filter(|s| !s.is_empty())
        .or_else(|| state.map.as_ref().map(|m| m.to_uppercase()));

    match state.scene.as_str() {
        "loader" => {
            let text = match state.loader.as_deref() {
                Some("update") => "Updating the game",
                Some("preparing") => "Preparing game data",
                _ => "Starting up",
            };
            activity.insert("details".to_string(), Value::String(text.to_string()));
        }
        "menu" => {
            activity.insert("details".to_string(), Value::String("In the menus".to_string()));
            if let Some(sub) = state
                .screen
                .as_deref()
                .and_then(screen_subtitle)
                .and_then(|s| fit(Some(s), 128))
            {
                activity.insert("state".to_string(), Value::String(sub));
            }
        }
        "battle" => {
            // Distinguish solo Instant Action, player-hosted servers, LAN games, and public Online servers.
            let kind = if !state.network {
                "Instant Action"
            } else if state.server.as_ref().is_some_and(|s| s.hosted) {
                "Hosting"
            } else if state.lan {
                "LAN"
            } else {
                "Online"
            };
            let raw_details = match &resolved_map {
                Some(m) => format!("{}: {}", kind, m),
                None => kind.to_string(),
            };
            if let Some(d) = fit(Some(&raw_details), 128) {
                activity.insert("details".to_string(), Value::String(d));
            }

            // Combine era ("Clone Wars") and side ("Republic") into the fallback subtitle.
            let era_side = {
                let mut parts = Vec::new();
                if let Some(e) = state.era.as_deref().and_then(era_name) {
                    parts.push(e);
                }
                if let Some(s) = state.side.as_deref().and_then(side_name) {
                    parts.push(s);
                }
                fit(Some(&parts.join(", ")), 128)
            };

            if state.network {
                // For network battles, show the server name on the second line when available.
                let st = state
                    .server
                    .as_ref()
                    .and_then(|s| fit(s.name.as_deref(), 128))
                    .or(era_side);
                if let Some(s) = st {
                    activity.insert("state".to_string(), Value::String(s));
                }
                // Attach party size `[players, maxPlayers]` only when `1 <= players <= maxPlayers`.
                let players = state
                    .players
                    .or_else(|| state.server.as_ref().and_then(|s| s.players));
                let most = state.server.as_ref().and_then(|s| s.max_players);
                if let (Some(p), Some(m), Some(pid)) = (players, most, state.party_id.as_ref()) {
                    if p >= 1 && m >= p && !pid.is_empty() {
                        activity.insert("party".to_string(), json!({ "id": pid, "size": [p, m] }));
                    }
                }
            } else if let Some(s) = era_side {
                activity.insert("state".to_string(), Value::String(s));
            }

            let battle_start = state.since.or(state.started).unwrap_or(0);
            activity.insert("timestamps".to_string(), json!({ "start": battle_start }));
        }
        _ => {}
    }

    Some(Value::Object(activity))
}

/// Recognized presence phase names for [`RawPresenceInput`].
pub const VALID_PHASES: &[&str] = &["menu", "preparing", "lobby", "loading", "match"];

/// Stock Star Wars Battlefront (2004) mission map ID to display title table
/// used when no `shell.json` map catalog is loaded.
pub fn map_display_name(map_id: &str) -> Option<&'static str> {
    // Map stock 5-character mission codes (`bes1a`..`yav2r`) to human-readable planet/arena titles.
    match map_id {
        "bes1a" | "bes1r" => Some("Bespin: Platforms"),
        "bes2a" | "bes2r" => Some("Bespin: Cloud City"),
        "end1a" => Some("Endor: Bunker"),
        "geo1r" => Some("Geonosis: Spire"),
        "hot1i" => Some("Hoth: Echo Base"),
        "kam1c" => Some("Kamino: Tipoca City"),
        "kas1a" | "kas1c" => Some("Kashyyyk: Islands"),
        "kas2a" | "kas2c" => Some("Kashyyyk: Docks"),
        "nab1a" | "nab1c" => Some("Naboo: Plains"),
        "nab2a" | "nab2c" => Some("Naboo: Theed"),
        "rhn1i" | "rhn1r" => Some("Rhen Var: Harbor"),
        "rhn2a" | "rhn2c" => Some("Rhen Var: Citadel"),
        "tat1i" | "tat1r" => Some("Tatooine: Dune Sea"),
        "tat2i" | "tat2r" => Some("Tatooine: Mos Eisley"),
        "tat3a" | "tat3c" => Some("Tatooine: Jabba's Palace"),
        "yav1c" | "yav1i" => Some("Yavin 4: Temple"),
        "yav2i" | "yav2r" => Some("Yavin 4: Arena"),
        _ => None,
    }
}

/// Returns the full faction display title for a faction code (`"all"`, `"imp"`, `"rep"`, `"cis"`).
pub fn faction_display_name(faction_code: &str) -> Option<&'static str> {
    match faction_code {
        "all" => Some("Rebel Alliance"),
        "imp" => Some("Galactic Empire"),
        "rep" => Some("Galactic Republic"),
        "cis" => Some("Confederacy of Independent Systems"),
        _ => None,
    }
}

/// Returns the era display title for an era code (`"gcw"`, `"cw"`).
pub fn era_display_name(era_code: &str) -> Option<&'static str> {
    era_name(era_code)
}

/// Structured presence input from the native client or `desktop-tool presence test`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawPresenceInput {
    /// Activity phase (`"menu"`, `"preparing"`, `"lobby"`, `"loading"`, `"match"`).
    pub phase: Option<String>,
    /// Match mode (`"instant-action"`, `"multiplayer"`, `"splitscreen"`).
    pub mode: Option<String>,
    /// Map identifier (`"bes2a"`, etc.).
    pub map_id: Option<String>,
    /// Explicit map display name override.
    pub map_name: Option<String>,
    /// Era code (`"gcw"` or `"cw"`).
    pub era: Option<String>,
    /// Explicit era display name override.
    pub era_name: Option<String>,
    /// Player faction code (`"all"`, `"imp"`, `"rep"`, `"cis"`).
    pub faction: Option<String>,
    /// Explicit faction display name override.
    pub faction_name: Option<String>,
    /// Currently spawned unit class display name.
    pub unit_name: Option<String>,
    /// Multiplayer room or lobby name.
    pub room_name: Option<String>,
    /// Deterministic party/session identifier for Discord party grouping.
    pub party_id: Option<String>,
    /// Current human player count in a multiplayer room.
    pub party_size: Option<i64>,
    /// Maximum human player capacity in a multiplayer room.
    pub party_max: Option<i64>,
    /// Millisecond or second epoch timestamp when the current activity started.
    pub started_at: Option<u64>,
    /// Preparation step detail string when `phase == "preparing"`.
    pub detail: Option<String>,
    /// Preparation completion percentage (`0..=100`) when `phase == "preparing"`.
    pub progress: Option<i64>,
}

/// Validated, normalized presence state produced by [`normalize_presence_state`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedPresenceState {
    /// Validated phase (`"menu"`, `"preparing"`, `"lobby"`, `"loading"`, or `"match"`).
    pub phase: String,
    /// Match mode (`"instant-action"`, `"multiplayer"`, or `"splitscreen"`).
    pub mode: Option<String>,
    /// Lowercase map ID (`"bes2a"`).
    pub map_id: Option<String>,
    /// Resolved human-readable map name (`"Bespin: Cloud City"`).
    pub map_name: Option<String>,
    /// Lowercase era code (`"gcw"` or `"cw"`).
    pub era: Option<String>,
    /// Resolved human-readable era name (`"Galactic Civil War"`).
    pub era_name: Option<String>,
    /// Lowercase faction code (`"all"`, `"imp"`, `"rep"`, `"cis"`).
    pub faction: Option<String>,
    /// Resolved human-readable faction name (`"Rebel Alliance"`).
    pub faction_name: Option<String>,
    /// Spawned unit class display name.
    pub unit_name: Option<String>,
    /// Multiplayer room name.
    pub room_name: Option<String>,
    /// Party identifier (`<= 96` chars).
    pub party_id: Option<String>,
    /// Current party size (`>= 1`).
    pub party_size: Option<u32>,
    /// Maximum party capacity (`>= party_size`).
    pub party_max: Option<u32>,
    /// Start timestamp normalized to Unix epoch seconds.
    pub started_at: Option<u64>,
    /// Preparation detail text.
    pub detail: Option<String>,
    /// Preparation progress (`0..=100`).
    pub progress: Option<u32>,
}

/// Collapses runs of whitespace into single spaces, trims, and truncates to `max_len` characters.
fn clean_text(value: Option<&str>, max_len: usize) -> Option<String> {
    // Strip leading/trailing whitespace and collapse internal runs to single spaces.
    let raw = value?;
    let collapsed: String = raw.split_whitespace().collect::<Vec<&str>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    if collapsed.chars().count() > max_len {
        Some(collapsed.chars().take(max_len).collect())
    } else {
        Some(collapsed)
    }
}

/// Validates that an optional integer falls within `min..=max`.
fn clean_int(value: Option<i64>, min: i64, max: i64) -> Option<u32> {
    // Reject negative or out-of-range integer values.
    let n = value?;
    if n < min || n > max {
        return None;
    }
    Some(n as u32)
}

/// Normalizes a [`RawPresenceInput`] into a canonical [`NormalizedPresenceState`].
///
/// Infers `map_name` from [`map_display_name`], `faction_name` from [`faction_display_name`],
/// `era` (`"cw"` vs `"gcw"`) from `faction`, and converts millisecond `started_at`
/// timestamps (`> 10_000_000_000`) into Unix epoch seconds.
pub fn normalize_presence_state(input: &RawPresenceInput) -> NormalizedPresenceState {
    // Fall back to `"menu"` if `phase` is missing or unrecognized.
    let raw_phase = clean_text(input.phase.as_deref(), 24)
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "menu".to_string());
    let phase = if VALID_PHASES.contains(&raw_phase.as_str()) {
        raw_phase
    } else {
        "menu".to_string()
    };

    // Default `mode` to `"instant-action"` during active `"loading"` or `"match"` phases.
    let raw_mode = clean_text(input.mode.as_deref(), 24).map(|s| s.to_lowercase());
    let mode = match raw_mode.as_deref() {
        Some("multiplayer") => Some("multiplayer".to_string()),
        Some("splitscreen") => Some("splitscreen".to_string()),
        Some("instant-action") => Some("instant-action".to_string()),
        _ if phase == "match" || phase == "loading" => Some("instant-action".to_string()),
        _ => None,
    };

    // Resolve `map_name` from `map_display_name(map_id)` when no explicit override was supplied.
    let map_id = clean_text(input.map_id.as_deref(), 16).map(|s| s.to_lowercase());
    let map_name = clean_text(input.map_name.as_deref(), 64).or_else(|| {
        map_id
            .as_deref()
            .and_then(map_display_name)
            .map(|s| s.to_string())
            .or_else(|| map_id.as_ref().map(|id| id.to_uppercase()))
    });

    let faction = clean_text(input.faction.as_deref(), 12).map(|s| s.to_lowercase());
    let faction_name = clean_text(input.faction_name.as_deref(), 48).or_else(|| {
        faction
            .as_deref()
            .and_then(faction_display_name)
            .map(|s| s.to_string())
    });

    // Infer the era (`"cw"` or `"gcw"`) from the faction when `era` is not explicitly provided.
    let inferred_era = match faction.as_deref() {
        Some("rep") | Some("cis") => Some("cw".to_string()),
        Some("all") | Some("imp") => Some("gcw".to_string()),
        _ => None,
    };
    let era = clean_text(input.era.as_deref(), 12)
        .map(|s| s.to_lowercase())
        .or(inferred_era);
    let era_name = clean_text(input.era_name.as_deref(), 36)
        .or_else(|| era.as_deref().and_then(era_display_name).map(|s| s.to_string()));

    let unit_name = clean_text(input.unit_name.as_deref(), 48);
    let room_name = clean_text(input.room_name.as_deref(), 64);
    let party_id = clean_text(input.party_id.as_deref(), 96);
    let party_size = clean_int(input.party_size, 1, 128);
    let raw_party_max = clean_int(input.party_max, 1, 128);
    let party_max = match (party_size, raw_party_max) {
        (Some(sz), Some(mx)) if mx >= sz => Some(mx),
        (None, Some(mx)) => Some(mx),
        _ => None,
    };

    // Convert millisecond timestamps (`> 10_000_000_000`) to Unix epoch seconds.
    let started_at = input.started_at.and_then(|ts| {
        if ts == 0 {
            None
        } else if ts > 10_000_000_000 {
            Some(ts / 1000)
        } else {
            Some(ts)
        }
    });

    let detail = clean_text(input.detail.as_deref(), 80);
    let progress = clean_int(input.progress, 0, 100);

    NormalizedPresenceState {
        phase,
        mode,
        map_id,
        map_name,
        era,
        era_name,
        faction,
        faction_name,
        unit_name,
        room_name,
        party_id,
        party_size,
        party_max,
        started_at,
        detail,
        progress,
    }
}

/// Returns the human-readable match mode title (`"Multiplayer"`, `"Splitscreen"`, or `"Instant Action"`).
fn mode_label(mode: Option<&str>) -> &'static str {
    match mode {
        Some("multiplayer") => "Multiplayer",
        Some("splitscreen") => "Splitscreen",
        _ => "Instant Action",
    }
}

/// Formats a [`NormalizedPresenceState`] into a Discord `SET_ACTIVITY` payload.
///
/// Populates `details`, `state`, `instance`, optional `timestamps.start`,
/// `assets` (`large_image`, `large_text`, and faction/era `small_image`), and
/// multiplayer `party` (`id` + `size: [party_size, party_max]`).
pub fn format_discord_activity(state: &NormalizedPresenceState) -> Value {
    let details: String;
    let state_line: String;

    // Select the primary `details` headline and secondary `state_line` based on `state.phase`.
    match state.phase.as_str() {
        "preparing" => {
            details = "Preparing Game Content".to_string();
            if let Some(pct) = state.progress {
                state_line = match &state.detail {
                    Some(d) => format!("{} ({}%)", d, pct),
                    None => format!("{}% complete", pct),
                };
            } else {
                state_line = state
                    .detail
                    .clone()
                    .unwrap_or_else(|| "Converting classic assets".to_string());
            }
        }
        "lobby" => {
            details = "In Multiplayer Lobby".to_string();
            if let (Some(sz), Some(mx)) = (state.party_size, state.party_max) {
                state_line = match &state.room_name {
                    Some(room) => format!("{} ({}/{})", room, sz, mx),
                    None => format!("Waiting for players ({}/{})", sz, mx),
                };
            } else {
                state_line = state
                    .room_name
                    .clone()
                    .unwrap_or_else(|| "Browsing rooms".to_string());
            }
        }
        "loading" => {
            details = match &state.map_name {
                Some(m) => format!("Loading {}", m),
                None => "Loading Battle".to_string(),
            };
            state_line = match &state.era_name {
                Some(era) => format!("{} - {}", mode_label(state.mode.as_deref()), era),
                None => mode_label(state.mode.as_deref()).to_string(),
            };
        }
        "match" => {
            let map_label = state.map_name.as_deref().unwrap_or("In Battle");
            details = format!(
                "{} - {}",
                mode_label(state.mode.as_deref()),
                map_label
            );
            let mut role_parts = Vec::new();
            if let Some(f) = &state.faction_name {
                role_parts.push(f.as_str());
            } else if let Some(e) = &state.era_name {
                role_parts.push(e.as_str());
            }
            if let Some(u) = &state.unit_name {
                role_parts.push(u.as_str());
            }
            if !role_parts.is_empty() {
                state_line = role_parts.join(" - ");
            } else if state.mode.as_deref() == Some("multiplayer") {
                if let Some(room) = &state.room_name {
                    state_line = room.clone();
                } else {
                    state_line = "In Combat".to_string();
                }
            } else {
                state_line = "In Combat".to_string();
            }
        }
        _ => {
            details = "In Main Menu".to_string();
            state_line = state
                .detail
                .clone()
                .unwrap_or_else(|| "Ready for Instant Action".to_string());
        }
    }

    let mut activity = serde_json::Map::new();
    activity.insert("details".to_string(), Value::String(details));
    activity.insert("state".to_string(), Value::String(state_line));
    activity.insert("instance".to_string(), Value::Bool(false));

    if let Some(start) = state.started_at {
        activity.insert("timestamps".to_string(), json!({ "start": start }));
    }

    // Attach the Battlefront Eternal logo and optional faction/era small badge.
    let small_key = state.faction.as_deref().or(state.era.as_deref());
    let small_text = state
        .faction_name
        .as_deref()
        .or(state.era_name.as_deref());
    let mut assets = serde_json::Map::new();
    assets.insert(
        "large_image".to_string(),
        Value::String("bfe_logo".to_string()),
    );
    assets.insert(
        "large_text".to_string(),
        Value::String("Battlefront Eternal".to_string()),
    );
    if let (Some(sk), Some(st)) = (small_key, small_text) {
        assets.insert("small_image".to_string(), Value::String(sk.to_string()));
        assets.insert("small_text".to_string(), Value::String(st.to_string()));
    }
    activity.insert("assets".to_string(), Value::Object(assets));

    // Attach party size `[party_size, party_max]` for multiplayer matches and lobbies.
    if let (Some(sz), Some(mx)) = (state.party_size, state.party_max) {
        if state.mode.as_deref() == Some("multiplayer") || state.phase == "lobby" {
            let mut party = serde_json::Map::new();
            let pid = state
                .party_id
                .clone()
                .or_else(|| state.room_name.clone());
            if let Some(id) = pid {
                party.insert("id".to_string(), Value::String(id));
            }
            party.insert("size".to_string(), json!([sz, mx]));
            activity.insert("party".to_string(), Value::Object(party));
        }
    }

    Value::Object(activity)
}

/// Formats a [`NormalizedPresenceState`] into Steam Friends rich-presence key/value pairs.
///
/// Sets `status`, `steam_display` (`"#StatusFull"`), and optional `steam_player_group`
/// and `steam_player_group_size` keys.
pub fn format_steam_rich_presence(state: &NormalizedPresenceState) -> BTreeMap<String, String> {
    // Combine the Discord `details` and `state` lines into Steam's single-line `status` string.
    let discord = format_discord_activity(state);
    let details = discord
        .get("details")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let st = discord
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let status = if !st.is_empty() {
        format!("{}: {}", details, st)
    } else {
        details.to_string()
    };

    let mut pairs = BTreeMap::new();
    pairs.insert("status".to_string(), status);
    pairs.insert("steam_display".to_string(), "#StatusFull".to_string());
    if let Some(group) = state.party_id.as_ref().or(state.room_name.as_ref()) {
        pairs.insert("steam_player_group".to_string(), group.clone());
    }
    if let Some(sz) = state.party_size {
        pairs.insert("steam_player_group_size".to_string(), sz.to_string());
    }
    pairs
}
