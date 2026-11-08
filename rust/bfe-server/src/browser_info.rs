//! Server browser metadata validation and sanitization (`src/browser-info.js`).
//!
//! What a server browser shows of a server beyond how to reach it:
//! - the operator's `description`, `sponsor`, and `location` from `config.json`,
//! - the battle it plays (`map`, `mapName`, `side`, `era`, `mode`, and the `query` a player's
//!   client loads for it),
//! - its rules (`bots`, `autoAssignTeams`), and
//! - how long the service has run (`uptime` in whole seconds).
//!
//! Both the online directory listing relayed by the master (`OnlineServer.listing` in
//! `src/online-server.js`) and the direct `/info` endpoint (`startLocalServer` in
//! `src/local-server.js`) include these fields. LAN discovery replies (`lanInfo` in
//! `src/local-server.js`) strip `description` and `sponsor` so the response fits in one UDP
//! datagram.

use crate::config::ServerConfig;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Sanitized operator sponsor block (`sponsorOf(value)` in `src/browser-info.js`).
///
/// A sponsor entry requires a non-empty `name` (clipped to 60 characters); `url` and `logo`
/// are validated by [`https_url`] so a player's browser only ever opens or fetches credential-free
/// `https:` URLs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SponsorInfo {
    pub name: String,
    pub url: Option<String>,
    pub logo: Option<String>,
}

/// Active battle summary inside `BrowserInfo` (`battle` object in `browserInfo` in `src/browser-info.js`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleInfo {
    pub map: Option<String>,
    pub map_name: Option<String>,
    pub side: Option<String>,
    pub era: Option<String>,
    pub mode: String,
}

/// Complete server browser metadata payload (`browserInfo` in `src/browser-info.js`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sponsor: Option<SponsorInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battle: Option<BattleInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bots: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_assign_teams: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<u64>,
}

/// Sanitizes and clips an optional string (`const clip = (value, length) => ...` in `src/browser-info.js`).
///
/// Control characters (`\x00-\x1f` and `\x7f`) are replaced with spaces before trimming and
/// slicing to `length` characters; empty or whitespace-only strings return `None`.
pub fn clip_text(value: Option<&str>, length: usize) -> Option<String> {
    let raw = value?;
    if raw.trim().is_empty() {
        return None;
    }
    let mut cleaned = String::with_capacity(raw.len());
    for ch in raw.chars() {
        let code = ch as u32;
        if code <= 0x1f || code == 0x7f {
            cleaned.push(' ');
        } else {
            cleaned.push(ch);
        }
    }
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(length).collect())
    }
}

/// Validates an `https:` URL with no username or password (`export function httpsUrl(value)` in `src/browser-info.js`).
///
/// Rejects strings longer than 512 bytes, non-HTTPS schemes, and URLs containing `@` in the
/// authority (`!url.username && !url.password`), because the client UI fetches sponsor logos and
/// links directly from this field.
pub fn https_url(value: Option<&str>) -> Option<String> {
    let raw = value?;
    if raw.len() > 512 {
        return None;
    }
    let rest = raw.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    if rest.contains('/') || rest.contains('?') || rest.contains('#') {
        Some(raw.to_string())
    } else {
        Some(format!("{raw}/"))
    }
}

/// Parses and sanitizes `config.sponsor` (`export function sponsorOf(value)` in `src/browser-info.js`).
///
/// Requires a non-empty sponsor `name` (clipped to 60 characters); `url` and `logo` are optional
/// and filtered through [`https_url`].
pub fn sponsor_of(value: Option<&Value>) -> Option<SponsorInfo> {
    let val = value?;
    if let Some(s) = val.as_str() {
        let name = clip_text(Some(s), 60)?;
        return Some(SponsorInfo {
            name,
            url: None,
            logo: None,
        });
    }
    let obj = val.as_object()?;
    let name = clip_text(obj.get("name").and_then(Value::as_str), 60)?;
    let url = https_url(obj.get("url").and_then(Value::as_str));
    let logo = https_url(obj.get("logo").and_then(Value::as_str));
    Some(SponsorInfo { name, url, logo })
}

/// Loads map IDs to display names (e.g. `"BESPIN: CLOUD CITY"`) from `common/shell/shell.json`
/// in the prepared game data (`export function mapNames(data)` in `src/browser-info.js`).
pub fn map_names(data_dir: &str) -> HashMap<String, String> {
    if data_dir.trim().is_empty() {
        return HashMap::new();
    }
    let base = Path::new(data_dir);
    for candidate in [
        base.join("common/shell/shell.json"),
        base.join("data/common/shell/shell.json"),
    ] {
        if let Ok(raw) = fs::read_to_string(&candidate) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                if let Some(maps) = parsed.get("maps").and_then(Value::as_array) {
                    let mut out = HashMap::with_capacity(maps.len());
                    for entry in maps {
                        if let (Some(id), Some(name)) = (
                            entry.get("id").and_then(Value::as_str),
                            entry.get("name").and_then(Value::as_str),
                        ) {
                            out.insert(id.to_string(), name.to_string());
                        }
                    }
                    return out;
                }
            }
        }
    }
    HashMap::new()
}

/// Builds the full server browser metadata (`export function browserInfo` in `src/browser-info.js`).
///
/// Maps `side` (`rep`/`cis` -> `cw`, `all`/`imp` -> `gcw`) via `ERAS`, extracts `mode` from the
/// third colon-delimited segment of `config.rotation[0]` (defaulting to `"conquest"`), determines
/// `bots` from `params.get('ai') !== 'off'`, and computes `uptime` in seconds since `started_at_ms`.
/// Builds the rich server-browser metadata object (`SponsorInfo`, `BattleInfo`, map title,
/// description, banner HTTPS URL, and player/bot counts), porting `browserInfo(config, state, maps)`
/// from `packages/bfe-server/src/browser-info.js`.
pub fn browser_info(
    config: &ServerConfig,
    query: &str,
    started_at_ms: u64,
    names: &HashMap<String, String>,
    now_ms: u64,
) -> BrowserInfo {
    let map = query_param(query, "map");
    let side = query_param(query, "side");
    let ai = query_param(query, "ai");
    let mode = config
        .rotation
        .first()
        .and_then(|r| r.split(':').nth(2))
        .filter(|s| !s.is_empty())
        .unwrap_or("conquest")
        .to_string();
    let era = match side.as_deref() {
        Some("rep") | Some("cis") => Some("cw".to_string()),
        Some("all") | Some("imp") => Some("gcw".to_string()),
        _ => query_param(query, "era"),
    };
    let map_name = map
        .as_deref()
        .and_then(|m| names.get(m))
        .and_then(|n| clip_text(Some(n.as_str()), 60));
    let uptime = now_ms.saturating_sub(started_at_ms) / 1000;
    BrowserInfo {
        description: clip_text(config.description.as_deref(), 280),
        sponsor: sponsor_of(config.sponsor.as_ref()),
        location: clip_text(config.location.as_deref(), 80),
        query: Some(query.to_string()),
        battle: Some(BattleInfo {
            map,
            map_name,
            side,
            era,
            mode,
        }),
        bots: Some(ai.as_deref() != Some("off")),
        auto_assign_teams: Some(config.auto_assign_teams),
        uptime: Some(uptime),
    }
}

/// Sanitizes operator-provided `description`, `sponsor`, and `location` fields from `config.json`.
/// Sanitizes untrusted server-browser metadata received from LAN discovery or the master directory,
/// porting `cleanBrowserInfo(raw)` from `packages/bfe-server/src/browser-info.js`.
///
/// Enforces character-length caps (`clip_text`) and `https://`-only URL validation (`https_url`) so
/// malicious server listings cannot inject `javascript:` or `http://` tracking URLs into the browser UI.
pub fn clean_browser_info(
    description: Option<&str>,
    sponsor: Option<&Value>,
    location: Option<&str>,
) -> Result<BrowserInfo, String> {
    Ok(BrowserInfo {
        description: clip_text(description, 280),
        sponsor: sponsor_of(sponsor),
        location: clip_text(location, 80),
        ..BrowserInfo::default()
    })
}

impl BrowserInfo {
    /// Merges the serialized `BrowserInfo` fields into a `/info` or online listing JSON object.
    pub fn insert_into(&self, map: &mut Map<String, Value>) {
        if let Ok(Value::Object(obj)) = serde_json::to_value(self) {
            for (k, v) in obj {
                map.insert(k, v);
            }
        }
    }

    /// Strips `description` and `sponsor` for single-datagram LAN discovery responses
    /// (`getInfo` in `startLocalServer` in `src/local-server.js`).
    pub fn insert_lan_into(&self, map: &mut Map<String, Value>) {
        map.remove("description");
        map.remove("sponsor");
        if let Some(location) = &self.location {
            map.insert("location".to_string(), Value::String(location.clone()));
        }
    }
}

/// Extracts a single query parameter value from a `?k=v&...` battle query string.
fn query_param(query: &str, key: &str) -> Option<String> {
    let trimmed = query.trim_start_matches('?');
    for pair in trimmed.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}
