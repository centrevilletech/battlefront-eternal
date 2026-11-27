//! Server browser metadata validation and sanitization.

use crate::config::ServerConfig;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Sanitized operator sponsor block (`sponsorOf(value)`).
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

/// Active battle summary inside `BrowserInfo` (`battle` object in `browserInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BattleInfo {
    /// Written only for a 2005 battle (`"swbf2"`); a 2004 battle has no such
    /// field, also when one was read.
    #[serde(default, skip_serializing_if = "no_2005_family")]
    pub family: Option<bfe_family::GameFamily>,
    /// The map, its title, side and era are never written as null: a released client reads
    /// strings here and refuses the whole server list over a null (an unknown one is left out).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub era: Option<String>,
    pub mode: String,
}

// Whether a battle's family is written: only another game than the 2004 one.
fn no_2005_family(family: &Option<bfe_family::GameFamily>) -> bool {
    family.is_none_or(|family| family.is_default())
}

/// Complete server browser metadata payload (`browserInfo`).
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
    pub friendly_fire: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<u64>,
}

/// Sanitizes and clips an optional string (`const clip = (value, length) => ...`).
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

/// Validates an `https:` URL with no username or password (`export function httpsUrl(value)`).
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

/// Parses and sanitizes `config.sponsor` (`export function sponsorOf(value)`).
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

use bfe_content::layout::family_dir;
use bfe_family::GameFamily;

/// Loads map IDs to display names (e.g. `"BESPIN: CLOUD CITY"`) from `common/shell/shell.json`
/// in the prepared game data of `family` (`export function mapNames(data)`).
///
/// A row is keyed by its `id` as written, as always. A row with a `family`
/// field (which the 2004 preparation never writes) is keyed by the map id a
/// battle of that game publishes: its own id for the 2004 game, the qualified
/// `swbf2/<map>` for the 2005 game. A row whose `family` this build does not
/// know, or whose `family` contradicts its id, names no map at all. A `family`
/// of JSON `null` is a row without the field.
pub fn map_names_for(data_dir: &str, family: GameFamily) -> HashMap<String, String> {
    if data_dir.trim().is_empty() {
        return HashMap::new();
    }
    let base = Path::new(data_dir);
    for root in [base.to_path_buf(), base.join("data")] {
        let dir = if family == GameFamily::Swbf2 && bfe_content::layout::is_swbf2_marked_dir(&root) {
            root
        } else {
            family_dir(&root, family)
        };
        let candidate = dir.join("common/shell/shell.json");
        if let Ok(raw) = fs::read_to_string(&candidate) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                if let Some(maps) = parsed.get("maps").and_then(Value::as_array) {
                    let mut out = HashMap::with_capacity(maps.len());
                    for entry in maps {
                        if let (Some(id), Some(name)) = (
                            entry.get("id").and_then(Value::as_str),
                            entry.get("name").and_then(Value::as_str),
                        ) {
                            let key = match entry.get("family").filter(|tag| !tag.is_null()) {
                                None => Some(id.to_string()),
                                Some(tag) => published_id(tag, id),
                            };
                            if let Some(key) = key {
                                out.insert(key, name.to_string());
                            }
                        }
                    }
                    return out;
                }
            }
        }
    }
    HashMap::new()
}

// The map id a battle publishes for a catalog row that names its game, or
// None when the row's game is unknown or disagrees with its id.
fn published_id(tag: &Value, id: &str) -> Option<String> {
    let family = tag.as_str().and_then(bfe_family::GameFamily::parse)?;
    let qualified = id.contains('/');
    match family {
        bfe_family::GameFamily::Swbf1 => (!bfe_family::swbf2::names_swbf2(id)).then(|| id.to_string()),
        bfe_family::GameFamily::Swbf2 if bfe_family::swbf2::bare_map(id).is_some() => Some(id.to_string()),
        bfe_family::GameFamily::Swbf2 => (!qualified).then(|| format!("{}{id}", bfe_family::swbf2::PREFIX)),
    }
}

/// Loads 2004 (`swbf1`) map IDs to display names (e.g. `"BESPIN: CLOUD CITY"`) from
/// `common/shell/shell.json` in the prepared game data (`export function mapNames(data)`).
pub fn map_names(data_dir: &str) -> HashMap<String, String> {
    map_names_for(data_dir, GameFamily::Swbf1)
}

/// Builds the full server browser metadata (`export function browserInfo`).
pub fn browser_info(
    config: &ServerConfig,
    query: &str,
    started_at_ms: u64,
    names: &HashMap<String, String>,
    now_ms: u64,
) -> BrowserInfo {
    let family = config.rotation.first().map_or(bfe_family::GameFamily::Swbf1,
        |entry| crate::config::rotation_family(entry));
    browser_info_for_family(config, family, query, started_at_ms, names, now_ms)
}

/// Describe the current match using the checked rotation slot.
pub fn browser_info_for_family(
    config: &ServerConfig, family: bfe_family::GameFamily, query: &str, started_at_ms: u64,
    names: &HashMap<String, String>, now_ms: u64,
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
    // The title the prepared game data gives the map, else its id (a 2005 map's prepared
    // data may name none): every listed battle has a title a released client can show.
    let map_name = map
        .as_deref()
        .and_then(|m| names.get(m).and_then(|n| clip_text(Some(n.as_str()), 60)).or_else(|| clip_text(Some(m), 60)));
    let uptime = now_ms.saturating_sub(started_at_ms) / 1000;
    BrowserInfo {
        description: clip_text(config.description.as_deref(), 280),
        sponsor: sponsor_of(config.sponsor.as_ref()),
        location: clip_text(config.location.as_deref(), 80),
        query: Some(query.to_string()),
        battle: Some(BattleInfo {
            // The game of the battle is the game of its rotation entry.
            family: (family != bfe_family::GameFamily::Swbf1).then_some(family),
            map,
            map_name,
            side,
            era,
            mode,
        }),
        bots: Some(ai.as_deref() != Some("off")),
        auto_assign_teams: Some(config.auto_assign_teams),
        friendly_fire: Some(config.friendly_fire),
        uptime: Some(uptime),
    }
}

/// Sanitizes operator-provided `description`, `sponsor`, and `location` fields from `config.json`.
/// Sanitizes untrusted server-browser metadata received from LAN discovery or the master directory,
/// porting `cleanBrowserInfo(raw)`.
///
/// Enforces character-length caps (`clip_text`) and `https://`-only URL validation (`https_url`) so
/// malicious server listings cannot inject `javascript:` or `http://` tracking URLs into the client UI.
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
    /// (`getInfo` in `startLocalServer`).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn names_from(rows: Value) -> HashMap<String, String> {
        static RUN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let run = RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bfe-map-names-{}-{run}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("common/shell")).unwrap();
        fs::write(dir.join("common/shell/shell.json"), serde_json::json!({ "maps": rows }).to_string()).unwrap();
        let names = map_names(dir.to_str().unwrap());
        let _ = fs::remove_dir_all(&dir);
        names
    }

    #[test]
    fn catalog_rows_name_only_the_map_of_their_own_game() {
        let plain = serde_json::json!({"id": "kam1", "name": "First title"});
        let tagged_old = serde_json::json!({"id": "kam1", "family": "swbf1", "name": "Tagged title"});
        let unknown = serde_json::json!({"id": "kam1", "family": "swbf9", "name": "Unknown title"});
        let contradictory = serde_json::json!({"id": "swbf2/kam1", "family": "swbf1", "name": "Contradictory title"});
        let later = serde_json::json!({"id": "kam1", "family": "swbf2", "name": "Later title"});
        for rows in [
            vec![plain.clone(), unknown.clone(), contradictory.clone(), later.clone()],
            vec![later.clone(), contradictory.clone(), unknown.clone(), plain.clone()],
        ] {
            let names = names_from(Value::Array(rows));
            assert_eq!(names.get("kam1").map(String::as_str), Some("First title"));
            assert_eq!(names.get("swbf2/kam1").map(String::as_str), Some("Later title"));
            assert_eq!(names.len(), 2);
        }
        // Rows without a family keep the released rule: the last row of an id names it.
        let names = names_from(serde_json::json!([plain, {"id": "kam1", "name": "Second title"}, tagged_old]));
        assert_eq!(names.get("kam1").map(String::as_str), Some("Tagged title"));
        // A family of null is no family.
        let names = names_from(serde_json::json!([{"id": "tat2", "name": "Fiction", "family": null}]));
        assert_eq!(names.get("tat2").map(String::as_str), Some("Fiction"));
    }

    // A summary read with the 2004 game named writes no family back; the 2005 game's is kept.
    #[test]
    fn a_2004_summary_never_writes_its_family() {
        let read: BattleInfo = serde_json::from_str(r#"{"family":"swbf1","map":"kam1","mapName":null,"side":"rep","era":"cw","mode":"conquest"}"#).unwrap();
        assert_eq!(read.family, Some(bfe_family::GameFamily::Swbf1), "kept in memory");
        assert_eq!(serde_json::to_string(&read).unwrap(), r#"{"map":"kam1","side":"rep","era":"cw","mode":"conquest"}"#);
        let later = BattleInfo { family: Some(bfe_family::GameFamily::Swbf2), ..read };
        assert!(serde_json::to_string(&later).unwrap().starts_with(r#"{"family":"swbf2","#));
    }

    #[test]
    fn a_2004_battle_summary_is_the_released_summary() {
        let names = HashMap::from([("kam1".to_string(), "First title".to_string()), ("swbf2/kam1".to_string(), "Later title".to_string())]);
        let config = ServerConfig { rotation: vec!["swbf1/kam1:rep:ConQuest".into()], ..ServerConfig::default() };
        let info = browser_info(&config, "?map=kam1&side=rep&ai=enhanced&seed=1&match=00", 0, &names, 0);
        assert_eq!(
            serde_json::to_string(&info.battle).unwrap(),
            r#"{"map":"kam1","mapName":"First title","side":"rep","era":"cw","mode":"ConQuest"}"#
        );
        // A 2004 entry's query can hold any text; the summary still describes a 2004 battle.
        let config = ServerConfig { rotation: vec!["swbf1/kam1:all&family=swbf2:duel".into()], ..ServerConfig::default() };
        let info = browser_info(&config, "?map=kam1&side=all&family=swbf2&ai=enhanced&seed=1", 0, &names, 0);
        assert_eq!(
            serde_json::to_string(&info.battle).unwrap(),
            r#"{"map":"kam1","mapName":"First title","side":"all","era":"gcw","mode":"duel"}"#
        );

        let config = ServerConfig { rotation: vec!["swbf2/kam1:gcw:hunt".into()], ..ServerConfig::default() };
        let query = crate::config::query_for_rotation(&config.rotation[0], "enhanced").unwrap();
        let info = browser_info(&config, &query, 0, &names, 0);
        assert_eq!(
            serde_json::to_string(&info.battle).unwrap(),
            r#"{"family":"swbf2","map":"swbf2/kam1","mapName":"Later title","side":"all","era":"gcw","mode":"hunt"}"#
        );
    }

    // A battle whose prepared game data names no title for its map (a 2005 map can) lists the
    // map's id as its title: never a null, which a released client's list reader refuses
    // together with every other server in the list.
    #[test]
    fn a_battle_without_a_map_title_lists_its_map_id_never_a_null() {
        let names = HashMap::new();
        let config = ServerConfig { rotation: vec!["swbf2/tat2:gcw:eli".into()], ..ServerConfig::default() };
        let query = crate::config::query_for_rotation(&config.rotation[0], "classic").unwrap();
        let info = browser_info(&config, &query, 0, &names, 0);
        let written = serde_json::to_value(&info.battle).unwrap();
        assert_eq!((&written["family"], &written["map"], &written["mapName"]), (&serde_json::json!("swbf2"), &serde_json::json!("swbf2/tat2"), &serde_json::json!("swbf2/tat2")), "{written}");
        let config = ServerConfig { rotation: vec!["swbf1/kam1:rep:conquest".into()], ..ServerConfig::default() };
        let info = browser_info(&config, "?map=kam1&side=rep&ai=enhanced&seed=1", 0, &names, 0);
        assert!(serde_json::to_string(&info.battle).unwrap().contains(r#""mapName":"kam1""#));
        let unknown = BattleInfo { map: None, map_name: None, side: None, era: None, ..info.battle.unwrap() };
        assert_eq!(serde_json::to_string(&unknown).unwrap(), r#"{"mode":"conquest"}"#, "no null is written");
    }
}
