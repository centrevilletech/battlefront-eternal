//! Unified desktop rich-presence controller, server-name sanitizer, and map catalog reader.
//!
//! Mirrors `electron/presence/index.js` (`SCREENS`, `cleanReport`, `serverName`,
//! `cleanServer`, `readMapNames`, `createPresence`).
//!
//! ## Overview
//! This module coordinates rich-presence updates across Discord and Steam Friends:
//! - [`server_name`] strips Unicode control characters, directional overrides, and
//!   line/paragraph separators from untrusted multiplayer server names before they
//!   are sent to Discord or Steam.
//! - [`read_map_names`] loads `common/shell/shell.json` from the prepared content
//!   directory so stock and add-on mission IDs resolve to their localized catalog titles.
//! - [`PresenceController`] normalizes raw scene reports and publishes them to the
//!   local Discord IPC socket when enabled.

pub mod activity;
pub mod discord;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::presence::activity::{
    format_discord_activity, format_steam_rich_presence, normalize_presence_state,
    NormalizedPresenceState, RawPresenceInput,
};
use crate::presence::discord::{try_publish_discord_activity, DEFAULT_DISCORD_CLIENT_ID};

/// Validated front-end screen names accepted by [`clean_report`].
///
/// Mirrors `SCREENS` in `electron/presence/index.js`.
pub const SCREENS: &[&str] = &["main", "sp", "ia", "mp", "options", "profile", "start"];

/// Sanitizes a server display name by stripping Unicode control/format characters
/// and line/paragraph separators (`\u{2028}`, `\u{2029}`), collapsing whitespace,
/// and truncating to at most 64 Unicode scalar values.
///
/// Mirrors `serverName` in `electron/presence/index.js`.
pub fn server_name(name: &str) -> Option<String> {
    // Strip ASCII/Unicode control codes and invisible formatting/bidi characters first.
    let filtered: String = name
        .chars()
        .filter(|&c| !c.is_control() && !is_unicode_format_or_line_sep(c))
        .collect();
    // Collapse consecutive whitespace into single spaces and cap at 64 characters.
    let collapsed: String = filtered.split_whitespace().collect::<Vec<&str>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed.chars().take(64).collect())
    }
}

/// Checks for Unicode format characters (`Cf`, directional overrides/marks) and line separators.
fn is_unicode_format_or_line_sep(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}

/// Reads the prepared map catalog (`common/shell/shell.json`) under `content_root`
/// and returns a lowercase `map_id -> display_name` (truncated to 48 chars) map.
///
/// Mirrors `readMapNames` in `electron/presence/index.js`.
pub fn read_map_names(content_root: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let path = content_root.join("common").join("shell").join("shell.json");
    let Ok(raw) = fs::read_to_string(path) else {
        return out;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return out;
    };
    // Support both array (`maps: [{ id, name }, ...]`) and object (`maps: { id: { name } }`) schemas.
    match parsed.get("maps") {
        Some(Value::Array(arr)) => {
            for item in arr {
                if let (Some(id), Some(name)) = (
                    item.get("id").and_then(|v| v.as_str()),
                    item.get("name").and_then(|v| v.as_str()),
                ) {
                    out.insert(id.to_lowercase(), name.chars().take(48).collect());
                }
            }
        }
        Some(Value::Object(obj)) => {
            for (id, item) in obj {
                if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                    out.insert(id.to_lowercase(), name.chars().take(48).collect());
                }
            }
        }
        _ => {}
    }
    out
}

/// Status snapshot of the desktop presence controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceStatus {
    /// Whether rich presence broadcasting is enabled by user settings.
    pub enabled: bool,
    /// Last normalized presence state, if any.
    pub state: Option<NormalizedPresenceState>,
    /// Formatted Discord `SET_ACTIVITY` payload for the current state.
    pub discord_activity: Option<Value>,
    /// Formatted Steam Friends rich-presence key/value pairs for the current state.
    pub steam_pairs: BTreeMap<String, String>,
    /// Connected Discord IPC socket path, if active.
    pub discord_socket: Option<String>,
}

/// Coordinates rich presence state normalization and publishing to Discord and Steam.
///
/// Mirrors the state machine created by `createPresence` in `electron/presence/index.js`.
#[derive(Debug, Clone)]
pub struct PresenceController {
    enabled: bool,
    client_id: String,
    last_state: Option<NormalizedPresenceState>,
    last_discord_socket: Option<String>,
}

impl Default for PresenceController {
    fn default() -> Self {
        Self::new(true, DEFAULT_DISCORD_CLIENT_ID.to_string())
    }
}

impl PresenceController {
    /// Creates a new [`PresenceController`].
    pub fn new(enabled: bool, client_id: String) -> Self {
        Self {
            enabled,
            client_id: if client_id.is_empty() {
                DEFAULT_DISCORD_CLIENT_ID.to_string()
            } else {
                client_id
            },
            last_state: None,
            last_discord_socket: None,
        }
    }

    /// Enables or disables rich presence broadcasting.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.last_discord_socket = None;
        }
    }

    /// Normalizes and stores a new [`RawPresenceInput`] activity state.
    pub fn set_activity(&mut self, raw: &RawPresenceInput) -> NormalizedPresenceState {
        let normalized = normalize_presence_state(raw);
        self.last_state = Some(normalized.clone());
        normalized
    }

    /// Clears the current activity state.
    pub fn clear(&mut self) {
        self.last_state = None;
        self.last_discord_socket = None;
    }

    /// Attempts to publish the current activity to a local Discord client if enabled.
    pub fn publish_to_discord(&mut self, platform: &str, env: &BTreeMap<String, String>) -> bool {
        if !self.enabled {
            return false;
        }
        let activity = self.last_state.as_ref().map(format_discord_activity);
        match try_publish_discord_activity(&self.client_id, activity.as_ref(), platform, env) {
            Ok(sock) => {
                self.last_discord_socket = Some(sock);
                true
            }
            Err(_) => {
                self.last_discord_socket = None;
                false
            }
        }
    }

    /// Returns a snapshot of the controller's current state and formatted payloads.
    pub fn status(&self) -> PresenceStatus {
        let discord_activity = self.last_state.as_ref().map(format_discord_activity);
        let steam_pairs = self
            .last_state
            .as_ref()
            .map(format_steam_rich_presence)
            .unwrap_or_default();
        PresenceStatus {
            enabled: self.enabled,
            state: self.last_state.clone(),
            discord_activity,
            steam_pairs,
            discord_socket: self.last_discord_socket.clone(),
        }
    }
}
