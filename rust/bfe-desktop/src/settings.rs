// Atomic persistence for player settings (`player-settings.json`) and desktop
// host preferences (`desktop-settings.json` / `settings.json`).
//
// This module ports `electron/player-settings.js` (`PLAYER_SETTINGS_FILE`,
// `cleanRecord`, `readPlayerSettings`, `writePlayerSettings`,
// `shadowedStorage`) and the settings helpers in `electron/main.js`
// (`loadSettings`, `saveSettings`) and `electron/steam/relaunch.js`
// (`readSteamSettings`).
//
// Why `player-settings.json` exists alongside browser storage: during an
// in-place self-update, the newly launched build runs briefly alongside the
// old process while waiting for its window (or until the game exits when
// started by Steam). A plain file written atomically by the desktop host avoids
// cross-process storage locks so profile names, options, and key bindings are
// never lost across an update hand-off.

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Filename for the main-process player settings store inside `userData`.
///
/// Mirrors `PLAYER_SETTINGS_FILE` in `electron/player-settings.js`.
pub const PLAYER_SETTINGS_FILE: &str = "player-settings.json";

/// Filename for the desktop host's own configuration inside `userData`.
///
/// Mirrors `settingsFile()` in `electron/main.js`.
pub const DESKTOP_SETTINGS_FILE: &str = "desktop-settings.json";

/// Maximum serialized JSON byte length of `player-settings.json` (`256 KiB`).
///
/// Mirrors `LIMIT` in `electron/player-settings.js`.
pub const LIMIT: usize = 256 * 1024;

static SAVE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Stored player settings record `{ complete, values }`.
///
/// Mirrors the record validated by `cleanRecord` in `electron/player-settings.js`.
/// When `complete` is true, `values` holds every setting; when `complete` is
/// false (written by a shadowed session started by an update before the file
/// existed), `values` holds only the keys changed during that session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerSettingsRecord {
    /// True if `values` represents a full snapshot of player settings.
    pub complete: bool,
    /// Key-value map of player settings.
    pub values: BTreeMap<String, Value>,
}

/// Presence preferences stored under `presence` in `desktop-settings.json`.
///
/// Mirrors `settings.presence` in `electron/main.js`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceSettingsRecord {
    /// Discord rich presence preference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord: Option<String>,
}

/// Steam library integration record stored under `steam` in `desktop-settings.json`.
///
/// Mirrors `settings.steam` in `electron/main.js` and `electron/steam/index.js`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamSettingsRecord {
    /// Non-Steam shortcut AppID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appid: Option<u32>,
    /// Steam root directory path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Steam installation kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Steam userdata account ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Whether launching via Steam is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch: Option<bool>,
    /// Whether the Steam Overlay is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<bool>,
    /// Whether the user declined Steam integration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declined: Option<bool>,
    /// Executable path registered in `shortcuts.vdf`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exe: Option<String>,
    /// Start directory registered in `shortcuts.vdf`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_dir: Option<String>,
    /// Launch options registered in `shortcuts.vdf`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_options: Option<String>,
    /// Whether Flatpak/sandbox access was granted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_access: Option<bool>,
}

/// Record of a completed content preparation run stored by [`SettingsStore`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationRecord {
    /// Source game installation directory.
    pub game_dir: String,
    /// Prepared add-on campaign IDs.
    #[serde(default)]
    pub add_ons: Vec<String>,
    /// Timestamp when preparation completed.
    #[serde(default)]
    pub updated_at: String,
}

/// Persistent desktop host state stored in `desktop-settings.json` / `settings.json`.
///
/// Mirrors `settings` in `electron/main.js`, keeping any additional keys via
/// `extra` so unknown fields are preserved on save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettings {
    /// User-chosen prepared content directory (`chosenContentDir`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen_content_dir: Option<String>,
    /// Alias `contentDir` for `resolve_startup_content` compatibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_dir: Option<String>,
    /// Discovered prepared content directory (`foundContentDir`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_content_dir: Option<String>,
    /// Target directory currently being prepared (`preparingContentDir`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preparing_content_dir: Option<String>,
    /// User-chosen Star Wars Battlefront (2004) install directory (`gameDir`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_dir: Option<String>,
    /// Whether enhanced sound banks are enabled (`enhancedSound`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enhanced_sound: Option<bool>,
    /// Selected update channel (`"stable"` or `"nightly"`).
    #[serde(default = "default_update_channel")]
    pub update_channel: String,
    /// Whether automatic update checks are enabled on startup.
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    /// Whether Discord Rich Presence is enabled.
    #[serde(default = "default_true")]
    pub discord_presence: bool,
    /// Historical content preparation records keyed by content directory path.
    #[serde(default)]
    pub preparations: BTreeMap<String, PreparationRecord>,
    /// Presence settings object (`presence`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<PresenceSettingsRecord>,
    /// Steam integration settings object (`steam`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam: Option<SteamSettingsRecord>,
    /// Additional forward-compatible fields preserved across saves.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_update_channel() -> String {
    "stable".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            chosen_content_dir: None,
            content_dir: None,
            found_content_dir: None,
            preparing_content_dir: None,
            game_dir: None,
            enhanced_sound: None,
            update_channel: "stable".to_string(),
            auto_check_updates: true,
            discord_presence: true,
            preparations: BTreeMap::new(),
            presence: None,
            steam: None,
            extra: BTreeMap::new(),
        }
    }
}

impl DesktopSettings {
    /// Ports `enhancedSoundWanted` from `electron/main.js`: returns true
    /// unless the player explicitly set `enhancedSound` to `false`.
    pub fn enhanced_sound_wanted(&self) -> bool {
        self.enhanced_sound != Some(false)
    }
}

/// Patch object applied by [`SettingsStore::save`].
#[derive(Debug, Clone, Default)]
pub struct SettingsPatch {
    /// Optional replacement for `content_dir` / `chosen_content_dir`.
    pub content_dir: Option<Option<String>>,
    /// Optional replacement for `game_dir`.
    pub game_dir: Option<Option<String>>,
    /// Optional replacement for `update_channel`.
    pub update_channel: Option<String>,
    /// Optional replacement for `auto_check_updates`.
    pub auto_check_updates: Option<bool>,
    /// Optional replacement for `discord_presence`.
    pub discord_presence: Option<bool>,
    /// Optional replacement for `preparations`.
    pub preparations: Option<BTreeMap<String, PreparationRecord>>,
}

/// File-backed store for [`DesktopSettings`].
#[derive(Debug, Clone)]
pub struct SettingsStore {
    file_path: PathBuf,
}

impl SettingsStore {
    /// Creates a new [`SettingsStore`] backed by `file_path`.
    pub fn new(file_path: impl AsRef<Path>) -> Self {
        Self {
            file_path: file_path.as_ref().to_path_buf(),
        }
    }

    /// Loads [`DesktopSettings`] from disk, returning [`DesktopSettings::default`] if absent or invalid.
    pub fn load(&self) -> DesktopSettings {
        let Ok(raw) = fs::read_to_string(&self.file_path) else {
            return DesktopSettings::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    /// Applies `patch` to the current settings on disk and writes the result atomically.
    pub fn save(&self, patch: &SettingsPatch) -> Result<DesktopSettings, String> {
        let mut current = self.load();
        if let Some(cd) = &patch.content_dir {
            current.content_dir = cd.clone();
            current.chosen_content_dir = cd.clone();
        }
        if let Some(gd) = &patch.game_dir {
            current.game_dir = gd.clone();
        }
        if let Some(ch) = &patch.update_channel {
            current.update_channel = ch.clone();
        }
        if let Some(ac) = patch.auto_check_updates {
            current.auto_check_updates = ac;
        }
        if let Some(dp) = patch.discord_presence {
            current.discord_presence = dp;
        }
        if let Some(preps) = &patch.preparations {
            current.preparations = preps.clone();
        }
        let dir = self
            .file_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let name = self
            .file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(DESKTOP_SETTINGS_FILE);
        let body = serde_json::to_string_pretty(&current).map_err(|e| e.to_string())?;
        write_atomic(dir, name, body.as_bytes()).map_err(|e| e.to_string())?;
        Ok(current)
    }
}

/// Structured player settings document used by [`PlayerSettingsStore`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSettingsDocument {
    /// Schema version (`1`).
    pub version: u32,
    /// Gameplay, audio, and video settings map.
    pub settings: BTreeMap<String, Value>,
    /// Action keybinding lists.
    pub keybindings: BTreeMap<String, Vec<String>>,
    /// Selected audio output device ID.
    pub audio_output_device_id: String,
    /// Whether the player has completed first-launch setup.
    pub has_launched_before: bool,
    /// Bookmarked multiplayer server URLs/addresses.
    pub favorite_servers: Vec<String>,
    /// Recently joined multiplayer servers.
    pub recent_servers: Vec<Value>,
    /// Last updated epoch timestamp.
    pub updated_at: u64,
}

impl Default for PlayerSettingsDocument {
    fn default() -> Self {
        Self {
            version: 1,
            settings: BTreeMap::new(),
            keybindings: BTreeMap::new(),
            audio_output_device_id: String::new(),
            has_launched_before: false,
            favorite_servers: Vec::new(),
            recent_servers: Vec::new(),
            updated_at: 0,
        }
    }
}

/// Partial update applied via [`PlayerSettingsStore::save`].
#[derive(Debug, Clone, Default)]
pub struct PlayerSettingsPatch {
    /// Settings key-value entries to merge.
    pub settings: Option<BTreeMap<String, Value>>,
    /// Keybindings to merge.
    pub keybindings: Option<BTreeMap<String, Vec<String>>>,
    /// Optional replacement audio output device ID.
    pub audio_output_device_id: Option<String>,
    /// Optional replacement `has_launched_before` flag.
    pub has_launched_before: Option<bool>,
    /// Optional replacement favorite servers list.
    pub favorite_servers: Option<Vec<String>>,
    /// Optional replacement recent servers list.
    pub recent_servers: Option<Vec<Value>>,
    /// Optional replacement timestamp.
    pub updated_at: Option<u64>,
}

/// File-backed store for [`PlayerSettingsDocument`].
#[derive(Debug, Clone)]
pub struct PlayerSettingsStore {
    file_path: PathBuf,
}

impl PlayerSettingsStore {
    /// Creates a new [`PlayerSettingsStore`] backed by `file_path`.
    pub fn new(file_path: impl AsRef<Path>) -> Self {
        Self {
            file_path: file_path.as_ref().to_path_buf(),
        }
    }

    /// Loads [`PlayerSettingsDocument`] from disk, or returns [`PlayerSettingsDocument::default`].
    pub fn load(&self) -> PlayerSettingsDocument {
        let Ok(raw) = fs::read_to_string(&self.file_path) else {
            return PlayerSettingsDocument::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    /// Merges `patch` into the stored [`PlayerSettingsDocument`] and writes it atomically.
    pub fn save(&self, patch: &PlayerSettingsPatch) -> Result<PlayerSettingsDocument, String> {
        let mut doc = self.load();
        if let Some(s) = &patch.settings {
            for (k, v) in s {
                doc.settings.insert(k.clone(), v.clone());
            }
        }
        if let Some(kb) = &patch.keybindings {
            for (k, v) in kb {
                doc.keybindings.insert(k.clone(), v.clone());
            }
        }
        if let Some(dev) = &patch.audio_output_device_id {
            doc.audio_output_device_id = dev.clone();
        }
        if let Some(hl) = patch.has_launched_before {
            doc.has_launched_before = hl;
        }
        if let Some(fav) = &patch.favorite_servers {
            doc.favorite_servers = fav.clone();
        }
        if let Some(rec) = &patch.recent_servers {
            doc.recent_servers = rec.clone();
        }
        if let Some(ts) = patch.updated_at {
            doc.updated_at = ts;
        }
        let dir = self
            .file_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let name = self
            .file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(PLAYER_SETTINGS_FILE);
        let body = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
        write_atomic(dir, name, body.as_bytes()).map_err(|e| e.to_string())?;
        Ok(doc)
    }
}

/// Ports `cleanRecord` from `electron/player-settings.js`.
///
/// Validates that `record` is a JSON object with a boolean `complete` field
/// and a non-array object `values` field whose compact JSON serialization fits
/// within [`LIMIT`] (`256 KiB`).
pub fn clean_record(record: &Value) -> Option<PlayerSettingsRecord> {
    let obj = record.as_object()?;
    let complete = obj.get("complete")?.as_bool()?;
    let values_obj = obj.get("values")?.as_object()?;

    let clean = PlayerSettingsRecord {
        complete,
        values: values_obj
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    };
    let text = serde_json::to_string(&clean).ok()?;
    if text.len() <= LIMIT {
        Some(clean)
    } else {
        None
    }
}

/// Ports `readPlayerSettings` from `electron/player-settings.js`.
///
/// Reads `<dir>/player-settings.json` and validates it with [`clean_record`],
/// returning `None` if the file is missing, unreadable, or malformed.
pub fn read_player_settings(dir: &Path) -> Option<PlayerSettingsRecord> {
    let text = fs::read_to_string(dir.join(PLAYER_SETTINGS_FILE)).ok()?;
    let parsed: Value = serde_json::from_str(&text).ok()?;
    clean_record(&parsed)
}

/// Ports `writePlayerSettings` from `electron/player-settings.js`.
///
/// Validates `record` with [`clean_record`], writes the compact JSON to
/// `<dir>/player-settings.json.<pid>.tmp`, and renames it over
/// `player-settings.json`. Returns `false` if the record is invalid or the
/// disk write fails.
pub fn write_player_settings_value(dir: &Path, record: &Value) -> bool {
    let Some(clean) = clean_record(record) else {
        return false;
    };
    write_player_settings(dir, &clean)
}

/// Typed version of [`write_player_settings_value`] accepting a
/// [`PlayerSettingsRecord`] directly.
pub fn write_player_settings(dir: &Path, record: &PlayerSettingsRecord) -> bool {
    let Ok(text) = serde_json::to_string(record) else {
        return false;
    };
    if text.len() > LIMIT {
        return false;
    }
    let file = dir.join(PLAYER_SETTINGS_FILE);
    let temp = dir.join(format!("{PLAYER_SETTINGS_FILE}.{}.tmp", std::process::id()));
    if fs::create_dir_all(dir).is_err() {
        return false;
    }
    if fs::write(&temp, text.as_bytes()).is_err() {
        let _ = fs::remove_file(&temp);
        return false;
    }
    if fs::rename(&temp, &file).is_err() {
        let _ = fs::remove_file(&temp);
        return false;
    }
    true
}

/// Ports `shadowedStorage` from `electron/player-settings.js`.
///
/// Returns true when `BFE_UPDATED_FROM` is set and non-empty in the process
/// environment, indicating that this process was started by an older running
/// version after installing an update.
pub fn shadowed_storage(env_map: Option<&HashMap<String, String>>) -> bool {
    match env_map {
        Some(map) => map
            .get("BFE_UPDATED_FROM")
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        None => env::var("BFE_UPDATED_FROM")
            .map(|v| !v.is_empty())
            .unwrap_or(false),
    }
}

/// Ports `loadSettings` from `electron/main.js`.
///
/// Reads `<user_data_dir>/desktop-settings.json`, returning a default empty
/// [`DesktopSettings`] when the file is absent or cannot be parsed.
pub fn read_desktop_settings(user_data_dir: &Path) -> DesktopSettings {
    let file = user_data_dir.join(DESKTOP_SETTINGS_FILE);
    let Ok(text) = fs::read_to_string(&file) else {
        return DesktopSettings::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Ports `saveSettings` from `electron/main.js`.
///
/// Atomically writes `settings` as 2-space pretty-printed JSON to
/// `<user_data_dir>/desktop-settings.json.<pid>.<seq>.tmp` and renames it over
/// `desktop-settings.json`.
pub fn write_desktop_settings(
    user_data_dir: &Path,
    settings: &DesktopSettings,
) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_atomic(user_data_dir, DESKTOP_SETTINGS_FILE, body.as_bytes())
}

/// Ports `readSteamSettings` from `electron/steam/relaunch.js`.
///
/// Reads the `steam` object from `<user_data_dir>/desktop-settings.json`
/// synchronously before full application initialization.
pub fn read_steam_settings(user_data_dir: &Path) -> Option<SteamSettingsRecord> {
    let text = fs::read_to_string(user_data_dir.join(DESKTOP_SETTINGS_FILE)).ok()?;
    let parsed: DesktopSettings = serde_json::from_str(&text).ok()?;
    parsed.steam
}

/// Looks up a single setting key across `player-settings.json` (`values`) and
/// `desktop-settings.json`. Used by `desktop-tool settings get KEY`.
pub fn get_setting(user_data_dir: &Path, key: &str) -> Option<String> {
    if let Some(player) = read_player_settings(user_data_dir) {
        if let Some(val) = player.values.get(key) {
            return match val {
                Value::String(s) => Some(s.clone()),
                Value::Null => None,
                other => Some(other.to_string()),
            };
        }
    }
    let desktop_file = user_data_dir.join(DESKTOP_SETTINGS_FILE);
    if let Ok(text) = fs::read_to_string(&desktop_file) {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) {
            if let Some(val) = map.get(key) {
                return match val {
                    Value::String(s) => Some(s.clone()),
                    Value::Null => None,
                    other => Some(other.to_string()),
                };
            }
        }
    }
    None
}

/// Writes `bytes` to `<dir>/<file_name>.<pid>.<seq>.tmp` and renames it over
/// `<dir>/<file_name>`. Removes the temporary file if either step fails.
pub(crate) fn write_atomic(dir: &Path, file_name: &str, bytes: &[u8]) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let seq = SAVE_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    let pid = std::process::id();
    let tmp_path = dir.join(format!("{file_name}.{pid}.{seq}.tmp"));
    let dst_path = dir.join(file_name);
    if let Err(err) = fs::write(&tmp_path, bytes) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    if let Err(err) = fs::rename(&tmp_path, &dst_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}
