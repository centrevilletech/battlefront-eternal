//! Tests that run the 2004 (`swbf1`) code paths beside the released build's
//! versions of the same structs and functions (module `released`) on the same
//! inputs, so any difference on 2004 inputs fails, and that check the 2005
//! (`swbf2`) game's isolation from them.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use bfe_content::layout::{family_dir, is_swbf2_marked_dir, SWBF2_PREPARE_STATE_FILE};
use bfe_desktop::archive::{
    export_content_for, import_content, import_content_for, inspect_archive, inspect_archive_for,
    ManifestFileEntry, SourceReceipt, ARCHIVE_FORMAT, ARCHIVE_VERSION, MANIFEST_NAME,
};
use bfe_desktop::discover::{
    find_game_installs, find_gog_install, find_steam_install, inspect_content,
    inspect_install, inspect_install_for, parse_gog_registry, parse_gog_registry_for,
    parse_heroic_installed, search_content, FindGameOptions, SearchContentOptions,
};
use bfe_desktop::paths::{normalize_comparable_path, resolve_from};
use bfe_desktop::prepare::{
    checkout_content_version, clear_prepared_for, game_lvl_pc_path, prepared_from,
    read_prepare_state, tool_env, PrepareFailureRecord, PrepareStateExtras, ToolEnvOptions,
};
use bfe_desktop::settings::{
    DesktopSettings, PreparationRecord, PresenceSettingsRecord, SettingsPatch, SettingsStore,
    SteamSettingsRecord,
};
use bfe_family::GameFamily;
use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bfe-parity-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn make_swbf1_install(root: &Path) {
    fs::create_dir_all(root.join("GameData/Data/_LVL_PC/SIDE")).unwrap();
    fs::create_dir_all(root.join("GameData/Data/_LVL_PC/Common/ENG")).unwrap();
    fs::write(root.join("GameData/Data/_LVL_PC/Common/ENG/common.lvl"), b"bf1").unwrap();
}

fn make_swbf2_install(root: &Path) {
    fs::create_dir_all(root.join("GameData/data/_lvl_pc/side")).unwrap();
    fs::write(root.join("GameData/BattlefrontII.exe"), b"exe").unwrap();
    for name in ["common.lvl", "core.lvl", "mission.lvl", "ingame.lvl", "shell.lvl"] {
        fs::write(root.join("GameData/data/_lvl_pc").join(name), b"bf2").unwrap();
    }
}

/// The released build's versions of the functions under test.
mod released {
    use super::*;

    #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct PrepareState {
        pub version: Option<u32>,
        pub map_import_version: Option<u32>,
        pub muzzle_version: Option<u32>,
        pub common_version: Option<u32>,
        pub complete: Option<bool>,
        pub game: Option<String>,
        pub add_ons: Option<Vec<String>>,
        pub failed: Option<Vec<Value>>,
        pub skipped: Option<Vec<Value>>,
        pub failures: Option<Vec<PrepareFailureRecord>>,
        pub extras: Option<PrepareStateExtras>,
    }

    pub fn read_prepare_state(content_dir: Option<&Path>) -> Option<PrepareState> {
        let dir = content_dir?;
        let raw = fs::read_to_string(dir.join(".prepare.json")).ok()?;
        serde_json::from_str(&raw).ok()
    }

    fn comparable_path(p: &Path, platform: &str) -> String {
        let resolved = resolve_from(
            &std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            p,
        );
        let text = normalize_comparable_path(&resolved);
        if platform == "win32" || platform == "windows" || platform == "darwin" || platform == "macos" {
            text.to_lowercase()
        } else {
            text
        }
    }

    pub fn prepared_from(state: Option<&PrepareState>, game: Option<&Path>, platform: &str) -> bool {
        let (Some(st), Some(game_dir)) = (state, game) else {
            return false;
        };
        let Some(state_game) = st.game.as_deref().filter(|s| !s.is_empty()) else {
            return false;
        };
        let expected = game_dir.join("GameData").join("Data").join("_LVL_PC");
        comparable_path(Path::new(state_game), platform) == comparable_path(&expected, platform)
    }

    pub fn clear_prepared_for(content: Option<&Path>, game: Option<&Path>, platform: &str) -> bool {
        let (Some(content_dir), Some(game_dir)) = (content, game) else {
            return false;
        };
        let state = read_prepare_state(Some(content_dir));
        if prepared_from(state.as_ref(), Some(game_dir), platform) {
            return false;
        }
        let outputs = ["maps", "sides", "common", "audio", ".prepare.json"];
        if !outputs.iter().any(|entry| content_dir.join(entry).exists()) {
            return false;
        }
        for entry in outputs {
            let target = content_dir.join(entry);
            if target.is_dir() {
                let _ = fs::remove_dir_all(&target);
            } else if target.exists() {
                let _ = fs::remove_file(&target);
            }
        }
        true
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct DesktopSettings {
        #[serde(skip_serializing_if = "Option::is_none")]
        pub chosen_content_dir: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub content_dir: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub found_content_dir: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub preparing_content_dir: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub game_dir: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub enhanced_sound: Option<bool>,
        #[serde(default = "default_update_channel")]
        pub update_channel: String,
        #[serde(default = "default_true")]
        pub auto_check_updates: bool,
        #[serde(default = "default_true")]
        pub discord_presence: bool,
        #[serde(default)]
        pub preparations: BTreeMap<String, PreparationRecord>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub presence: Option<PresenceSettingsRecord>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub steam: Option<SteamSettingsRecord>,
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

    pub fn load_settings(raw: &str) -> DesktopSettings {
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn child_ignore_case(dir: &Path, name: &str) -> Option<PathBuf> {
        let direct = dir.join(name);
        if direct.exists() {
            return Some(direct);
        }
        let want = name.to_lowercase();
        let entries = fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            if let Some(entry_name) = entry.file_name().to_str() {
                if entry_name.to_lowercase() == want {
                    return Some(dir.join(entry_name));
                }
            }
        }
        None
    }

    pub fn walk_ignore_case(root: &Path, parts: &[&str]) -> Option<PathBuf> {
        if root.as_os_str().is_empty() {
            return None;
        }
        let mut cur = root.to_path_buf();
        for part in parts {
            cur = child_ignore_case(&cur, part)?;
        }
        Some(cur)
    }

    pub fn inspect_install(dir: &Path) -> Option<PathBuf> {
        let lvl = walk_ignore_case(dir, &["GameData", "Data", "_LVL_PC"])?;
        let side = child_ignore_case(&lvl, "SIDE")?;
        let common = child_ignore_case(&lvl, "Common")?;
        if !side.exists() || !common.exists() {
            return None;
        }
        let entries = fs::read_dir(&common).ok()?;
        let mut has_common_lvl = false;
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                let sub = common.join(entry.file_name());
                if child_ignore_case(&sub, "common.lvl").is_some() {
                    has_common_lvl = true;
                    break;
                }
            }
        }
        if !has_common_lvl {
            return None;
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Some(resolve_from(&cwd, dir))
    }

    pub fn parse_gog_registry(text: &str, prefix: Option<&Path>) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix('"') {
                if let Some(eq_idx) = rest.find("\"=\"") {
                    let key = &rest[..eq_idx];
                    let val_part = &rest[eq_idx + 3..];
                    if let Some(end_quote) = val_part.rfind('"') {
                        let k_lower = key.to_lowercase();
                        if k_lower == "path" || k_lower == "installlocation" {
                            let raw_val = &val_part[..end_quote];
                            let value = raw_val.replace("\\\\", "\\");
                            if value.to_lowercase().contains("battlefront") {
                                let resolved = bfe_desktop::discover::wine_path(prefix, &value)
                                    .unwrap_or_else(|| PathBuf::from(&value));
                                dirs.push(resolved);
                            }
                        }
                    }
                }
            }
            let tokens: Vec<&str> = trimmed.split_whitespace().collect();
            if tokens.len() >= 3 {
                let key_lower = tokens[0].to_lowercase();
                let reg_type = tokens[1].to_uppercase();
                if (key_lower == "path" || key_lower == "installlocation")
                    && (reg_type == "REG_SZ" || reg_type == "REG_EXPAND_SZ")
                {
                    if let Some(pos) = trimmed.find(tokens[1]) {
                        let value = trimmed[pos + tokens[1].len()..].trim();
                        if value.to_lowercase().contains("battlefront") {
                            let resolved = bfe_desktop::discover::wine_path(prefix, value)
                                .unwrap_or_else(|| PathBuf::from(value));
                            dirs.push(resolved);
                        }
                    }
                }
            }
        }
        dirs
    }

    pub fn parse_heroic_installed(text: &str) -> Vec<PathBuf> {
        let Ok(data) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        let mut entries = Vec::new();
        if let Some(arr) = data.as_array() {
            entries.extend(arr.iter());
        } else if let Some(obj) = data.as_object() {
            if let Some(installed) = obj.get("installed").and_then(|v| v.as_array()) {
                entries.extend(installed.iter());
            } else {
                entries.extend(obj.values());
            }
        }
        let mut out = Vec::new();
        for item in entries {
            let Some(install_path) = item.get("install_path").and_then(|v| v.as_str()) else {
                continue;
            };
            if install_path.is_empty() {
                continue;
            }
            let title = item
                .get("title")
                .or_else(|| item.get("appName"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let combined = format!("{title} {install_path}").to_lowercase();
            if combined.contains("battlefront") {
                out.push(PathBuf::from(install_path));
            }
        }
        out
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    pub struct ArchiveManifest {
        pub format: String,
        pub version: u32,
        #[serde(default, alias = "createdAt")]
        pub created: Option<String>,
        #[serde(default)]
        pub source: Option<SourceReceipt>,
        pub files: Vec<ManifestFileEntry>,
    }

    pub fn validate_archive_manifest(raw: &[u8]) -> Result<ArchiveManifest, String> {
        let manifest: ArchiveManifest = serde_json::from_slice(raw)
            .map_err(|_| "Unsupported or invalid content export".to_string())?;
        if manifest.format != ARCHIVE_FORMAT
            || manifest.version != ARCHIVE_VERSION
            || manifest.files.is_empty()
        {
            return Err("Unsupported or invalid content export".to_string());
        }
        Ok(manifest)
    }

    pub fn find_gog_install(opts: &FindGameOptions) -> Option<(PathBuf, String, String)> {
        let home = opts.home.as_deref();
        for candidate in gog_records(&opts.platform, home, &opts.env) {
            if let Some(dir) = inspect_install(&candidate) {
                return Some((dir, "gog".to_string(), "your GOG library".to_string()));
            }
        }
        for parent in bfe_desktop::discover::gog_folders(&opts.platform, home) {
            let Ok(entries) = fs::read_dir(&parent) else {
                continue;
            };
            for entry in entries.flatten() {
                if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                    continue;
                };
                if !name.to_lowercase().contains("battlefront") {
                    continue;
                }
                if let Some(dir) = inspect_install(&parent.join(&name)) {
                    return Some((
                        dir,
                        "folder".to_string(),
                        format!("a copy in {}", parent.display()),
                    ));
                }
            }
        }
        None
    }

    fn gog_records(platform: &str, home: Option<&Path>, env: &BTreeMap<String, String>) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let mut heroic_files = Vec::new();
        if let Some(h) = home {
            if platform == "linux" {
                let config = env
                    .get("XDG_CONFIG_HOME")
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| h.join(".config"));
                heroic_files.push(config.join("heroic/gog_store/installed.json"));
                heroic_files.push(
                    h.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/gog_store/installed.json"),
                );
            }
        }
        for file in heroic_files {
            if let Ok(raw) = fs::read_to_string(&file) {
                dirs.extend(parse_heroic_installed(&raw));
            }
        }
        if let Some(h) = home {
            let wine = h.join(".wine");
            if wine.join("drive_c").exists() {
                if let Ok(raw) = fs::read_to_string(wine.join("system.reg")) {
                    dirs.extend(parse_gog_registry(&raw, Some(&wine)));
                }
            }
        }
        dirs
    }

    pub fn common_folders(library: &Path) -> Vec<PathBuf> {
        let common = library.join("steamapps/common");
        let mut dirs = vec![common.join("Star Wars Battlefront (Classic, 2004)")];
        if let Ok(entries) = fs::read_dir(&common) {
            for entry in entries.flatten() {
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        if name.to_lowercase().contains("battlefront") {
                            dirs.push(common.join(name));
                        }
                    }
                }
            }
        }
        dirs
    }

    pub fn find_game_installs(opts: &FindGameOptions) -> Vec<(PathBuf, String, String, bool)> {
        let mut installs: Vec<(PathBuf, String, String, bool)> = Vec::new();
        let mut by_key: BTreeMap<String, usize> = BTreeMap::new();
        let mut add = |candidate: &Path, source: &str, how: String| {
            let Some(dir) = inspect_install(candidate) else {
                return;
            };
            let key = normalize_comparable_path(&dir);
            if let Some(&idx) = by_key.get(&key) {
                if installs[idx].1 == "chosen" && (source == "steam" || source == "gog") {
                    installs[idx] = (dir, source.to_string(), how, true);
                }
                return;
            }
            by_key.insert(key, installs.len());
            installs.push((dir, source.to_string(), how, source == "chosen"));
        };
        for dir in &opts.extra {
            add(dir, "chosen", "the folder you chose".to_string());
        }
        let listed = bfe_desktop::discover::steam_libraries(&opts.platform, opts.home.as_deref(), &opts.env, &[]);
        for library in &listed {
            let manifest = library.join("steamapps/appmanifest_1058020.acf");
            if let Ok(raw) = fs::read_to_string(&manifest) {
                if let Some(install_dir) = bfe_desktop::discover::parse_install_dir(&raw) {
                    add(&library.join("steamapps/common").join(install_dir), "steam", "your Steam library".to_string());
                }
            }
        }
        for candidate in gog_records(&opts.platform, opts.home.as_deref(), &opts.env) {
            add(&candidate, "gog", "your GOG library".to_string());
        }
        for library in &listed {
            for candidate in common_folders(library) {
                add(
                    &candidate,
                    "folder",
                    format!("a copy in {} that Steam didn't install", library.display()),
                );
            }
        }
        for parent in bfe_desktop::discover::gog_folders(&opts.platform, opts.home.as_deref()) {
            let Ok(entries) = fs::read_dir(&parent) else {
                continue;
            };
            for entry in entries.flatten() {
                if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    continue;
                }
                let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                    continue;
                };
                if !name.to_lowercase().contains("battlefront") {
                    continue;
                }
                add(&parent.join(name), "folder", format!("a copy in {}", parent.display()));
            }
        }
        installs
    }
}

/// PrepareState / prepared_from / clear_prepared_for parity on extra `"family"` members,
/// plus 2005 receipt consistency, unknown/older record repair, and receipt-first clearing.
#[test]
fn prepare_state_and_clear_match_released_behavior() {
    let tmp = TempDir::new("receipts");
    let game = tmp.path().join("game");
    make_swbf1_install(&game);
    let expected_game = game.join("GameData/Data/_LVL_PC").to_string_lossy().into_owned();
    let ver = checkout_content_version().unwrap();

    let extra_families: &[(&str, Value)] = &[
        ("swbf2", json!("swbf2")),
        ("future", json!("future")),
        ("number", json!(7)),
        ("null", Value::Null),
        ("object", json!({"nested": true})),
        ("array", json!(["swbf1"])),
    ];

    for (label, family_val) in extra_families {
        let base_dir = tmp.path().join(format!("base-{label}"));
        let head_dir = tmp.path().join(format!("head-{label}"));
        for d in [&base_dir, &head_dir] {
            fs::create_dir_all(d.join("maps/madeup")).unwrap();
            fs::write(d.join("maps/madeup/map.json"), b"{}").unwrap();
            let body = json!({
                "family": family_val,
                "version": ver.state,
                "mapImportVersion": ver.maps,
                "muzzleVersion": ver.sides,
                "commonVersion": ver.common,
                "complete": true,
                "game": expected_game,
                "addOns": [],
                "failed": [],
                "skipped": [],
            });
            fs::write(d.join(".prepare.json"), serde_json::to_vec(&body).unwrap()).unwrap();
        }

        let base_state = released::read_prepare_state(Some(&base_dir));
        let head_state = read_prepare_state(Some(&head_dir));
        assert_eq!(
            head_state.is_some(),
            base_state.is_some(),
            "read_prepare_state mismatch for family={label}"
        );
        assert_eq!(
            prepared_from(head_state.as_ref(), Some(&game), "linux"),
            released::prepared_from(base_state.as_ref(), Some(&game), "linux"),
            "prepared_from mismatch for family={label}"
        );

        let base_cleared = released::clear_prepared_for(Some(&base_dir), Some(&game), "linux");
        let head_cleared = clear_prepared_for(Some(&head_dir), Some(&game), "linux");
        assert_eq!(
            head_cleared, base_cleared,
            "clear_prepared_for mismatch for family={label}"
        );
        assert!(
            !head_cleared && head_dir.join("maps/madeup/map.json").is_file(),
            "2004 folder with extra family={label} was unexpectedly deleted"
        );
    }

}

/// tool_env always sets BFE_FAMILY explicitly (never forwarding inherited BFE_FAMILY)
/// and preserves Windows special roots for 2005.
#[test]
fn tool_env_sets_explicit_family_and_preserves_windows_swbf2_special_roots() {
    let tmp = TempDir::new("tool-env");
    let mut inherited = BTreeMap::new();
    inherited.insert("PATH".to_string(), "/usr/bin".to_string());
    inherited.insert("BFE_FAMILY".to_string(), "swbf2".to_string());

    // 2004 with a stray inherited BFE_FAMILY=swbf2 must explicitly override it to swbf1.
    let out_with_inherited = tool_env(&ToolEnvOptions {
        family: GameFamily::Swbf1,
        toolchain: tmp.path().join("tc"),
        game: Some(PathBuf::from("/games/bf1")),
        content: Some(PathBuf::from("/data/content")),
        env: inherited,
        platform: "linux".to_string(),
    });
    assert_eq!(
        out_with_inherited.get("BFE_FAMILY").map(String::as_str),
        Some("swbf1"),
        "2004 tool_env must override stray inherited BFE_FAMILY=swbf2 with swbf1"
    );
    assert_eq!(out_with_inherited["BFE_CONTENT_DIR"], "/data/content");

    // 2004 without inherited BFE_FAMILY also sets BFE_FAMILY=swbf1 explicitly.
    let mut clean_env = BTreeMap::new();
    clean_env.insert("PATH".to_string(), "/usr/bin".to_string());
    let out_clean = tool_env(&ToolEnvOptions {
        family: GameFamily::Swbf1,
        toolchain: tmp.path().join("tc"),
        game: Some(PathBuf::from("/games/bf1")),
        content: Some(PathBuf::from("/data/content")),
        env: clean_env.clone(),
        platform: "linux".to_string(),
    });
    assert_eq!(
        out_clean.get("BFE_FAMILY").map(String::as_str),
        Some("swbf1"),
        "2004 tool_env must set BFE_FAMILY=swbf1 explicitly"
    );

    // Windows case-variant inherited `bfe_family=swbf2` is also replaced cleanly for 2004.
    let mut win_inherited = BTreeMap::new();
    win_inherited.insert("Path".to_string(), r"C:\Windows\System32".to_string());
    win_inherited.insert("bfe_family".to_string(), "swbf2".to_string());
    let win_bf1 = tool_env(&ToolEnvOptions {
        family: GameFamily::Swbf1,
        toolchain: tmp.path().join("tc"),
        game: Some(PathBuf::from(r"C:\games\bf1")),
        content: Some(PathBuf::from(r"C:\data\content")),
        env: win_inherited,
        platform: "win32".to_string(),
    });
    assert_eq!(win_bf1.get("BFE_FAMILY").map(String::as_str), Some("swbf1"));
    assert!(!win_bf1.contains_key("bfe_family"));

    // 2005 Windows special roots must never collapse `\` or `/` to relative, nor turn `D:` into `D:\`.
    for (input, must_not_be, must_start_with) in [
        ("\\", "swbf2", "\\swbf2\\"),
        ("/", "swbf2", "\\swbf2\\"),
        ("D:", "D:\\swbf2", "D:swbf2\\"),
        ("D:\\", "D:swbf2", "D:\\swbf2\\"),
        ("\\\\server\\share", "swbf2", "\\\\server\\share\\swbf2\\"),
        ("C:\\data\\content", "swbf2", "C:\\data\\content\\swbf2\\"),
    ] {
        let win_out = tool_env(&ToolEnvOptions {
            family: GameFamily::Swbf2,
            toolchain: tmp.path().join("tc"),
            game: Some(PathBuf::from(r"C:\games\bf2")),
            content: Some(PathBuf::from(input)),
            env: clean_env.clone(),
            platform: "win32".to_string(),
        });
        assert_eq!(win_out.get("BFE_FAMILY").map(String::as_str), Some("swbf2"));
        let actual = &win_out["BFE_CONTENT_DIR"];
        assert_ne!(actual, must_not_be, "Windows root {input:?} produced broken {actual:?}");
        assert!(
            actual.starts_with(must_start_with),
            "Windows root {input:?} produced {actual:?}, expected prefix {must_start_with:?}"
        );
    }
}

/// Full-field released golden bytes + `games` null/empty/string/array/order/duplicate parity.
#[test]
fn settings_golden_bytes_and_games_extra_match_released_behavior() {
    let tmp = TempDir::new("settings");

    // 1. Frozen literal golden JSON emitted by the released build covering all 12
    //    DesktopSettings fields + all 11 Steam fields + presence.discord + preparations + extra.
    const FROZEN_RELEASED_DESKTOP_SETTINGS_JSON: &str = r#"{
  "chosenContentDir": "/chosen",
  "contentDir": "/content",
  "foundContentDir": "/found",
  "preparingContentDir": "/preparing",
  "gameDir": "/games/bf1",
  "enhancedSound": true,
  "updateChannel": "nightly",
  "autoCheckUpdates": false,
  "discordPresence": false,
  "preparations": {
    "/data/content": {
      "gameDir": "/games/bf1",
      "addOns": [
        "zz9"
      ],
      "updatedAt": "2026-01-01T00:00:00Z"
    }
  },
  "presence": {
    "discord": "full"
  },
  "steam": {
    "appid": 42,
    "root": "/steam",
    "kind": "native",
    "account": "12345",
    "launch": true,
    "overlay": true,
    "declined": false,
    "exe": "/bin/bfe-client",
    "startDir": "/bin",
    "launchOptions": "--fast",
    "grantedAccess": true
  },
  "A-before-games": 1,
  "games": {
    "swbf2": {
      "gameDir": "/games/bf2"
    }
  },
  "z-after-games": "tail"
}"#;

    let mut preps = BTreeMap::new();
    preps.insert(
        "/data/content".to_string(),
        PreparationRecord {
            game_dir: "/games/bf1".to_string(),
            add_ons: vec!["zz9".to_string()],
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        },
    );
    let mut extra = BTreeMap::new();
    extra.insert("A-before-games".to_string(), json!(1));
    extra.insert("games".to_string(), json!({"swbf2": {"gameDir": "/games/bf2"}}));
    extra.insert("z-after-games".to_string(), json!("tail"));

    let base_full = released::DesktopSettings {
        chosen_content_dir: Some("/chosen".to_string()),
        content_dir: Some("/content".to_string()),
        found_content_dir: Some("/found".to_string()),
        preparing_content_dir: Some("/preparing".to_string()),
        game_dir: Some("/games/bf1".to_string()),
        enhanced_sound: Some(true),
        update_channel: "nightly".to_string(),
        auto_check_updates: false,
        discord_presence: false,
        preparations: preps.clone(),
        presence: Some(PresenceSettingsRecord {
            discord: Some("full".to_string()),
        }),
        steam: Some(SteamSettingsRecord {
            appid: Some(42),
            root: Some("/steam".to_string()),
            kind: Some("native".to_string()),
            account: Some("12345".to_string()),
            launch: Some(true),
            overlay: Some(true),
            declined: Some(false),
            exe: Some("/bin/bfe-client".to_string()),
            start_dir: Some("/bin".to_string()),
            launch_options: Some("--fast".to_string()),
            granted_access: Some(true),
        }),
        extra,
    };
    assert_eq!(
        serde_json::to_string_pretty(&base_full).unwrap(),
        FROZEN_RELEASED_DESKTOP_SETTINGS_JSON,
        "the released struct must match the frozen golden bytes"
    );

    // Load the frozen golden JSON through current SettingsStore and re-save untouched.
    let path = tmp.path().join("golden.json");
    fs::write(&path, FROZEN_RELEASED_DESKTOP_SETTINGS_JSON).unwrap();
    let store = SettingsStore::new(&path);
    let loaded: DesktopSettings = store.load();
    let reserialized = serde_json::to_string_pretty(&loaded).unwrap();
    assert_eq!(
        reserialized, FROZEN_RELEASED_DESKTOP_SETTINGS_JSON,
        "full-field DesktopSettings serialization drifted from frozen released golden bytes"
    );

    // 2. Exotic `games` values, ordering relative to `A-before-games`, and duplicate `games` keys.
    let cases = [
        ("games-null", r#"{"gameDir":"/bf1","A-before-games":1,"games":null}"#),
        ("games-empty-obj", r#"{"gameDir":"/bf1","A-before-games":1,"games":{}}"#),
        ("games-string", r#"{"gameDir":"/bf1","A-before-games":1,"games":"modded"}"#),
        ("games-array", r#"{"gameDir":"/bf1","A-before-games":1,"games":[1,2]}"#),
        (
            "games-duplicate-keys",
            r#"{"gameDir":"keep","enhancedSound":false,"autoCheckUpdates":false,"games":{"future":1},"games":{"opaque":2}}"#,
        ),
    ];

    for (label, raw) in cases {
        let base_doc = released::load_settings(raw);
        let base_saved = serde_json::to_string_pretty(&base_doc).unwrap();

        let p = tmp.path().join(format!("{label}.json"));
        fs::write(&p, raw).unwrap();
        let st = SettingsStore::new(&p);
        let head_saved_doc = st.save(&SettingsPatch::default()).unwrap();
        let head_saved = fs::read_to_string(&p).unwrap();

        assert_eq!(
            head_saved, base_saved,
            "SettingsStore byte-for-byte mismatch vs released base for {label}"
        );
        assert_eq!(
            head_saved_doc.game_dir, base_doc.game_dir,
            "game_dir mismatch for {label}"
        );
        assert_eq!(
            head_saved_doc.enhanced_sound, base_doc.enhanced_sound,
            "enhanced_sound mismatch for {label}"
        );
        assert_eq!(
            head_saved_doc.auto_check_updates, base_doc.auto_check_updates,
            "auto_check_updates mismatch for {label}"
        );
    }
}

/// 2005-only content is never adopted as 2004, while a real 2004 folder literally named `swbf2` still works.
#[test]
fn swbf2_only_content_not_adopted_as_swbf1_while_literal_swbf2_2004_folder_works() {
    let tmp = TempDir::new("isolation");

    // Case A: Home contains ONLY 2005 prepared content under `home/content`.
    let home_bf2_only = tmp.path().join("home-bf2-only");
    let content = home_bf2_only.join("content");
    let bf2_root = family_dir(&content, GameFamily::Swbf2);
    bfe_desktop::swbf2_tree::mark_new_tree(&family_dir(&content, GameFamily::Swbf2)).unwrap();
    fs::create_dir_all(bf2_root.join("common")).unwrap();
    fs::create_dir_all(bf2_root.join("maps/testmap1")).unwrap();
    fs::write(bf2_root.join("common/common.json"), b"{}").unwrap();
    fs::write(bf2_root.join("maps/testmap1/map.json"), b"{}").unwrap();

    assert!(
        !is_swbf2_marked_dir(&content.join("swbf2")),
        "the outer swbf2 container must never be marked"
    );
    assert!(is_swbf2_marked_dir(&bf2_root));

    let found_as_2004 = search_content(&home_bf2_only, &SearchContentOptions::default());
    assert_eq!(
        found_as_2004, None,
        "2004 home search must never adopt a 2005-only content tree: got {found_as_2004:?}"
    );
    assert_eq!(
        inspect_content(&content),
        None,
        "2004 inspect_content must reject 2005-only shared root"
    );
    assert_eq!(
        inspect_content(&content.join("swbf2")),
        None,
        "2004 inspect_content must reject 2005 swbf2 container (no common/common.json)"
    );
    assert_eq!(
        inspect_content(&bf2_root),
        None,
        "2004 inspect_content must reject swbf2-marked .content dir"
    );
    assert_eq!(
        bfe_desktop::discover::search_content_for(
            &home_bf2_only,
            GameFamily::Swbf2,
            &SearchContentOptions::default()
        )
        .map(|c| c.dir),
        Some(content.clone()),
        "2005 home search must find the shared content root"
    );

    // Case B: A genuine 2004 content folder literally named `swbf2` still works even with a
    // marked `.content` folder inside it.
    let home_literal = tmp.path().join("home-literal");
    let literal_2004 = home_literal.join("swbf2");
    fs::create_dir_all(literal_2004.join("common")).unwrap();
    fs::create_dir_all(literal_2004.join("maps/qq7")).unwrap();
    fs::write(literal_2004.join("common/common.json"), b"{}").unwrap();
    fs::write(literal_2004.join("maps/qq7/map.json"), b"{}").unwrap();
    bfe_desktop::swbf2_tree::mark_new_tree(&family_dir(&home_literal, GameFamily::Swbf2)).unwrap();

    let inspected = inspect_content(&literal_2004);
    assert!(
        inspected.is_some(),
        "genuine 2004 folder named swbf2 must pass inspect_content even beside swbf2/.content"
    );
    let searched = search_content(&home_literal, &SearchContentOptions::default());
    assert_eq!(
        searched.as_ref().map(|c| c.dir.clone()),
        Some(literal_2004),
        "genuine 2004 folder named swbf2 must still be found by search_content"
    );
}

/// A real flat 2004 folder is selected as the released build selects it, named `swbf2` and with
/// an obsolete marker of an earlier 2005 build at its top, before any start has repaired
/// anything; a 2005 tree is known by positive evidence, its dedicated record alone or an empty
/// marker, and never adopted as 2004.
#[test]
fn a_flat_2004_folder_keeps_its_released_selection_and_2005_trees_stay_2005() {
    let tmp = TempDir::new("selection");
    let flat_2004 = |dir: &Path| {
        fs::create_dir_all(dir.join("common")).unwrap();
        fs::create_dir_all(dir.join("maps/qq7")).unwrap();
        fs::write(dir.join("common/common.json"), b"{}").unwrap();
        fs::write(dir.join("maps/qq7/map.json"), b"{}").unwrap();
        fs::write(
            dir.join(".prepare.json"),
            br#"{"version":2,"complete":true,"game":"/made/up/GameData/Data/_LVL_PC"}"#,
        )
        .unwrap();
    };
    let marker_bodies: [&[u8]; 3] = [br#"{"format":1,"family":"swbf2"}"#, b"", b"{"];
    for (n, marker) in marker_bodies.iter().enumerate() {
        let home = tmp.path().join(format!("home-flat-{n}"));
        let dir = home.join("swbf2");
        flat_2004(&dir);
        fs::write(dir.join(bfe_content::layout::SWBF2_MARKER_FILE), marker).unwrap();
        assert!(!is_swbf2_marked_dir(&dir), "marker {n}");
        let inspected = inspect_content(&dir).expect("the 2004 folder is inspected as released");
        assert_eq!(inspected.maps, vec!["qq7".to_string()], "marker {n}");
        let found = search_content(&home, &SearchContentOptions::default());
        assert_eq!(found.map(|c| c.dir), Some(dir.clone()), "marker {n}");
    }
    // A 2005 tree with its record and no marker, and one with only an empty marker beside its
    // files: 2005 both, so neither is 2004 content.
    for (n, (record, marker)) in [(true, None), (false, Some(&b""[..])), (true, Some(&b""[..]))].into_iter().enumerate() {
        let home = tmp.path().join(format!("home-2005-{n}"));
        let content = home.join("content");
        let tree = family_dir(&content, GameFamily::Swbf2);
        fs::create_dir_all(tree.join("common")).unwrap();
        fs::create_dir_all(tree.join("maps/zz1")).unwrap();
        fs::write(tree.join("common/common.json"), b"{}").unwrap();
        fs::write(tree.join("maps/zz1/map.json"), b"{}").unwrap();
        if record {
            fs::write(tree.join(SWBF2_PREPARE_STATE_FILE), br#"{"family":"swbf2"}"#).unwrap();
        }
        if let Some(marker) = marker {
            fs::write(tree.join(bfe_content::layout::SWBF2_MARKER_FILE), marker).unwrap();
        }
        assert!(is_swbf2_marked_dir(&tree), "case {n}");
        assert_eq!(inspect_content(&tree), None, "case {n}");
        assert_eq!(search_content(&home, &SearchContentOptions::default()), None, "case {n}");
        assert_eq!(
            bfe_desktop::discover::search_content_for(&home, GameFamily::Swbf2, &SearchContentOptions::default())
                .map(|c| c.dir),
            Some(content.clone()),
            "case {n}"
        );
    }
}

/// Discovery parity on filesystem names, GOG sibling order & symlinks, GOG registry, and Heroic JSON.
#[test]
fn discovery_filesystem_gog_heroic_match_released_behavior() {
    let tmp = TempDir::new("discovery");

    // 1. Filesystem cases: 2004 installs with names that mention 2 / II / 2005 / SWBF1, plus pure 2005.
    let home = tmp.path().join("home");
    let steam_common = home.join(".local/share/Steam/steamapps/common");
    let gog_games = home.join("GOG Games");
    fs::create_dir_all(&steam_common).unwrap();
    fs::create_dir_all(&gog_games).unwrap();

    for name in [
        "Battlefront2ndCopy",
        "Battlefront II backup",
        "Battlefront 2005 archive",
        "SWBF1",
    ] {
        make_swbf1_install(&steam_common.join(name));
        make_swbf1_install(&gog_games.join(name));
    }
    // Pure 2005 install beside them (must be rejected by inspect_install on both base and head).
    make_swbf2_install(&steam_common.join("Star Wars Battlefront II"));
    make_swbf2_install(&gog_games.join("Star Wars - Battlefront II"));

    let opts = FindGameOptions {
        platform: "linux".to_string(),
        home: Some(home.clone()),
        env: BTreeMap::new(),
        extra: Vec::new(),
    };

    let base_all = released::find_game_installs(&opts);
    let head_all: Vec<(PathBuf, String, String, bool)> = find_game_installs(&opts)
        .into_iter()
        .map(|i| (i.dir, i.source, i.how, i.chosen))
        .collect();
    assert_eq!(
        head_all, base_all,
        "2004 find_game_installs diverged from released base"
    );

    let base_gog = released::find_gog_install(&opts);
    let head_gog = find_gog_install(&opts).map(|i| (i.dir, i.source, i.how));
    assert_eq!(
        head_gog, base_gog,
        "2004 find_gog_install diverged from released base"
    );

    // 1b. Loose GOG symlink named `Star Wars - Battlefront`: base skipped symlinks in `read_dir`.
    #[cfg(unix)]
    {
        let home_sym = tmp.path().join("home-sym");
        let gog_sym = home_sym.join("GOG Games");
        fs::create_dir_all(&gog_sym).unwrap();
        let target = tmp.path().join("external-bf1");
        make_swbf1_install(&target);
        std::os::unix::fs::symlink(&target, gog_sym.join("Star Wars - Battlefront")).unwrap();

        let sym_opts = FindGameOptions {
            platform: "linux".to_string(),
            home: Some(home_sym),
            env: BTreeMap::new(),
            extra: Vec::new(),
        };
        assert_eq!(
            find_gog_install(&sym_opts).map(|i| i.dir),
            released::find_gog_install(&sym_opts).map(|(d, _, _)| d),
            "2004 find_gog_install must not probe symlinked default basename ahead of read_dir"
        );
    }

    // 2. GOG registry corpus (28+ tricky lines: conflicting title/path, Battlefront2ndCopy, SWBF1, Wine timestamps).
    let reg_corpus = [
        "[Software\\\\GOG.com\\\\Games\\\\1] 12345\n\"gameName\"=\"Star Wars Battlefront II\"\n\"path\"=\"C:\\\\Games\\\\Battlefront2ndCopy\"\n",
        "[Software\\\\GOG.com\\\\Games\\\\2] 23456\n\"gameName\"=\"Battlefront 2005 archive\"\n\"path\"=\"C:\\\\Games\\\\Battlefront II backup\"\n",
        "[Software\\\\GOG.com\\\\Games\\\\3]\n\"path\"=\"C:\\\\Games\\\\SWBF1\"\n",
        "HKEY_LOCAL_MACHINE\\SOFTWARE\\GOG\n    gameName    REG_SZ    Star Wars Battlefront II\n    path    REG_SZ    C:\\Games\\Battlefront_Copy\n",
        "HKEY_LOCAL_MACHINE\\SOFTWARE\\GOG\n    installLocation    REG_EXPAND_SZ    D:\\Battlefront2ndCopy\n",
    ];
    let prefix = Path::new("/wine");
    for (idx, reg) in reg_corpus.iter().enumerate() {
        assert_eq!(
            parse_gog_registry(reg, Some(prefix)),
            released::parse_gog_registry(reg, Some(prefix)),
            "2004 parse_gog_registry diverged on case {idx}"
        );
    }

    // 3. Heroic installed.json corpus (object, array, map, conflicting titles/paths, SWBF1).
    let heroic_corpus = [
        r#"{"installed":[{"title":"Battlefront 2005 archive","install_path":"/games/Battlefront2ndCopy"},{"title":"SWBF1","install_path":"/games/SWBF1"},{"title":"Star Wars Battlefront II","install_path":"/games/Battlefront"}]}"#,
        r#"[{"appName":"bf2_backup","title":"Battlefront II backup","install_path":"/games/bf1_copy"}]"#,
        r#"{"entry1":{"title":"Other","install_path":"/games/Battlefront2ndCopy"}}"#,
    ];
    for (idx, raw) in heroic_corpus.iter().enumerate() {
        assert_eq!(
            parse_heroic_installed(raw),
            released::parse_heroic_installed(raw),
            "2004 parse_heroic_installed diverged on case {idx}"
        );
    }

    // 4. Steam find_steam_install with Battlefront2ndCopy only.
    let home_steam = tmp.path().join("home-steam");
    let lib = home_steam.join(".local/share/Steam");
    make_swbf1_install(&lib.join("steamapps/common/Battlefront2ndCopy"));
    let steam_opts = FindGameOptions {
        platform: "linux".to_string(),
        home: Some(home_steam),
        env: BTreeMap::new(),
        extra: Vec::new(),
    };
    let found = find_steam_install(&steam_opts);
    assert_eq!(
        found.as_ref().map(|i| i.dir.clone()),
        Some(lib.join("steamapps/common/Battlefront2ndCopy")),
        "2004 find_steam_install must not veto Battlefront2ndCopy"
    );
}

/// 2005 additive discovery: Wine timestamped `[B] 23456` section boundary + case-sensitive split merged install.
#[test]
fn swbf2_wine_timestamped_header_and_case_sensitive_split_install() {
    // 1. Wine `[A] 12345` followed immediately by `[B] 23456` without a blank line:
    //    `gameName` from section A must NOT leak into section B!
    let wine_reg = concat!(
        "[Software\\\\GOG.com\\\\Games\\\\A] 12345\n",
        "\"gameName\"=\"Star Wars Battlefront II\"\n",
        "[Software\\\\GOG.com\\\\Games\\\\B] 23456\n",
        "\"path\"=\"C:\\\\Games\\\\UnrelatedGame\"\n",
    );
    let parsed_bf2 = parse_gog_registry_for(wine_reg, Some(Path::new("/wine")), GameFamily::Swbf2);
    assert!(
        parsed_bf2.is_empty(),
        "Wine [B] 23456 header failed to reset section gameName: got {parsed_bf2:?}"
    );

    // 2. Case-sensitive split merged install: `GameData/Data/_LVL_PC` (2004) + `GameData/data/_lvl_pc` (2005).
    let tmp = TempDir::new("split-case");
    let merged = tmp.path().join("merged");
    make_swbf1_install(&merged);
    make_swbf2_install(&merged);
    // Only test when filesystem actually distinguished `Data` and `data`.
    if merged.join("GameData/Data").exists()
        && merged.join("GameData/data").exists()
        && !same_file::is_same_file(merged.join("GameData/Data"), merged.join("GameData/data"))
            .unwrap_or(true)
    {
        assert!(
            inspect_install(&merged).is_some(),
            "2004 validator must accept merged install"
        );
        assert!(
            inspect_install_for(&merged, GameFamily::Swbf2).is_some(),
            "2005 validator must find GameData/data/_lvl_pc even when GameData/Data/_LVL_PC exists"
        );
        assert_eq!(
            game_lvl_pc_path(&merged, GameFamily::Swbf2),
            merged.join("GameData/data/_lvl_pc"),
            "2005 game_lvl_pc_path must resolve GameData/data/_lvl_pc on split case-sensitive disks"
        );
    }
}

/// Exhaustive 6x6 entry-type matrix for `(SIDE, common.lvl)` comparing against released `inspect_install`.
#[cfg(unix)]
#[test]
fn inspect_install_6x6_entry_type_matrix_matches_released_behavior() {
    #[derive(Clone, Copy, Debug)]
    enum Kind {
        File,
        Dir,
        SymFile,
        SymDir,
        SymDangling,
        Absent,
    }
    let kinds = [
        Kind::File,
        Kind::Dir,
        Kind::SymFile,
        Kind::SymDir,
        Kind::SymDangling,
        Kind::Absent,
    ];

    let tmp = TempDir::new("install-matrix");
    let real_file = tmp.path().join("target_file");
    let real_dir = tmp.path().join("target_dir");
    fs::write(&real_file, b"x").unwrap();
    fs::create_dir_all(&real_dir).unwrap();
    let missing = tmp.path().join("does_not_exist");

    let make_entry = |path: &Path, kind: Kind| match kind {
        Kind::File => fs::write(path, b"x").unwrap(),
        Kind::Dir => fs::create_dir_all(path).unwrap(),
        Kind::SymFile => std::os::unix::fs::symlink(&real_file, path).unwrap(),
        Kind::SymDir => std::os::unix::fs::symlink(&real_dir, path).unwrap(),
        Kind::SymDangling => std::os::unix::fs::symlink(&missing, path).unwrap(),
        Kind::Absent => {}
    };

    for (i, &side_kind) in kinds.iter().enumerate() {
        for (j, &common_kind) in kinds.iter().enumerate() {
            let root = tmp.path().join(format!("case-{i}-{j}"));
            let lvl = root.join("GameData/Data/_LVL_PC");
            let eng = lvl.join("Common/ENG");
            fs::create_dir_all(&eng).unwrap();
            make_entry(&lvl.join("SIDE"), side_kind);
            make_entry(&eng.join("common.lvl"), common_kind);

            let base_res = released::inspect_install(&root);
            let head_res = inspect_install(&root).map(|v| v.dir);
            assert_eq!(
                head_res, base_res,
                "inspect_install 6x6 mismatch for SIDE={side_kind:?}, common.lvl={common_kind:?}"
            );
        }
    }
}

fn build_synthetic_zip(path: &Path, entries: &[(&str, &[u8])]) {
    // Minimal store-mode ZIP writer matching standard PKZIP layout.
    let mut file = fs::File::create(path).unwrap();
    let mut offset = 0u32;
    let mut central: Vec<(Vec<u8>, u32, u32, u32)> = Vec::new();
    for &(name, data) in entries {
        let mut crc_h = Crc32Hasher::new();
        crc_h.update(data);
        let crc = crc_h.finalize();
        let name_b = name.as_bytes();
        let size = data.len() as u32;
        let local_off = offset;
        let mut hdr = Vec::new();
        hdr.extend_from_slice(&0x04034b50u32.to_le_bytes());
        hdr.extend_from_slice(&20u16.to_le_bytes());
        hdr.extend_from_slice(&0x0800u16.to_le_bytes());
        hdr.extend_from_slice(&0u16.to_le_bytes());
        hdr.extend_from_slice(&0u16.to_le_bytes());
        hdr.extend_from_slice(&0x21u16.to_le_bytes());
        hdr.extend_from_slice(&crc.to_le_bytes());
        hdr.extend_from_slice(&size.to_le_bytes());
        hdr.extend_from_slice(&size.to_le_bytes());
        hdr.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        hdr.extend_from_slice(&0u16.to_le_bytes());
        hdr.extend_from_slice(name_b);
        file.write_all(&hdr).unwrap();
        file.write_all(data).unwrap();
        offset += hdr.len() as u32 + size;
        central.push((name_b.to_vec(), crc, size, local_off));
    }
    let cd_start = offset;
    for (name_b, crc, size, local_off) in &central {
        let mut rec = Vec::new();
        rec.extend_from_slice(&0x02014b50u32.to_le_bytes());
        rec.extend_from_slice(&45u16.to_le_bytes());
        rec.extend_from_slice(&20u16.to_le_bytes());
        rec.extend_from_slice(&0x0800u16.to_le_bytes());
        rec.extend_from_slice(&0u16.to_le_bytes());
        rec.extend_from_slice(&0u16.to_le_bytes());
        rec.extend_from_slice(&0x21u16.to_le_bytes());
        rec.extend_from_slice(&crc.to_le_bytes());
        rec.extend_from_slice(&size.to_le_bytes());
        rec.extend_from_slice(&size.to_le_bytes());
        rec.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        rec.extend_from_slice(&0u16.to_le_bytes());
        rec.extend_from_slice(&0u16.to_le_bytes());
        rec.extend_from_slice(&0u16.to_le_bytes());
        rec.extend_from_slice(&0u16.to_le_bytes());
        rec.extend_from_slice(&(0o100644u32 << 16).to_le_bytes());
        rec.extend_from_slice(&local_off.to_le_bytes());
        rec.extend_from_slice(name_b);
        file.write_all(&rec).unwrap();
        offset += rec.len() as u32;
    }
    let cd_size = offset - cd_start;
    let mut eocd = Vec::new();
    eocd.extend_from_slice(&0x06054b50u32.to_le_bytes());
    eocd.extend_from_slice(&0u16.to_le_bytes());
    eocd.extend_from_slice(&0u16.to_le_bytes());
    eocd.extend_from_slice(&(central.len() as u16).to_le_bytes());
    eocd.extend_from_slice(&(central.len() as u16).to_le_bytes());
    eocd.extend_from_slice(&cd_size.to_le_bytes());
    eocd.extend_from_slice(&cd_start.to_le_bytes());
    eocd.extend_from_slice(&0u16.to_le_bytes());
    file.write_all(&eocd).unwrap();
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// v1 archives with extra `"family"` values stay accepted as 2004; 2005 exports are declined
/// by both the released build and the 2004 import path, and import only via the 2005 import path.
#[test]
fn archive_v1_tolerates_extra_family_and_swbf2_declined_by_2004_import() {
    let tmp = TempDir::new("archives");

    // 1. v1 archive carrying `"family": "future"` or `"family": 7` (ignored by released build).
    let common_bytes = b"{}";
    let map_bytes = b"{}";
    for (label, extra_family) in [("future", json!("future")), ("number", json!(7))] {
        let manifest = json!({
            "format": ARCHIVE_FORMAT,
            "version": ARCHIVE_VERSION,
            "family": extra_family,
            "files": [
                {"path": "common/common.json", "size": common_bytes.len(), "sha256": sha256_hex(common_bytes)},
                {"path": "maps/qq7/map.json", "size": map_bytes.len(), "sha256": sha256_hex(map_bytes)},
            ]
        });
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        assert!(
            released::validate_archive_manifest(&manifest_bytes).is_ok(),
            "released build accepts v1 with family={label}"
        );
        let zip_path = tmp.path().join(format!("v1-{label}.bfecontent"));
        build_synthetic_zip(
            &zip_path,
            &[
                (MANIFEST_NAME, &manifest_bytes),
                ("common/common.json", common_bytes),
                ("maps/qq7/map.json", map_bytes),
            ],
        );
        let inspected = inspect_archive(&zip_path);
        assert!(
            inspected.is_ok(),
            "inspect_archive must match released build and accept v1 with family={label}: {inspected:?}"
        );
        assert_eq!(inspected.unwrap().family, GameFamily::Swbf1);
        assert!(
            inspect_archive_for(&zip_path, GameFamily::Swbf2).is_err(),
            "2005 inspect_archive_for must decline a 2004 archive"
        );
    }

    // 2. 2005 export must be DECLINED by the released build AND by 2004 inspect_archive/import_content,
    //    and imported cleanly only through import_content_for(..., GameFamily::Swbf2, ...).
    let content = tmp.path().join("src-content");
    let bf2_root = family_dir(&content, GameFamily::Swbf2);
    let ver = checkout_content_version().unwrap();
    fs::create_dir_all(bf2_root.join("common")).unwrap();
    fs::create_dir_all(bf2_root.join("maps/testmap1")).unwrap();
    fs::write(bf2_root.join("common/common.json"), b"{}").unwrap();
    fs::write(bf2_root.join("maps/testmap1/map.json"), b"{}").unwrap();
    fs::write(bf2_root.join("maps/testmap1/geom.bin"), b"mesh").unwrap();
    let prep = json!({
        "family": "swbf2",
        "version": ver.state,
        "mapImportVersion": ver.maps,
        "muzzleVersion": ver.sides,
        "commonVersion": ver.common,
        "swbf2Version": 8,
        "complete": true,
        "game": "/games/swbf2/GameData/data/_lvl_pc",
        "addOns": [],
        "plan": ["map:testmap1", "common"],
        "done": ["common", "map:testmap1"],
        "outputs": {
            "map:testmap1": [["maps/testmap1/map.json", 2], ["maps/testmap1/geom.bin", 4]],
            "common": [["common/common.json", 2]],
            "*": [],
        },
        "failed": [],
        "skipped": [],
    });
    let prep_bytes = serde_json::to_vec(&prep).unwrap();
    fs::write(bf2_root.join(SWBF2_PREPARE_STATE_FILE), &prep_bytes).unwrap();
    // Tools that prepare the 2005 game publish its generation, the lock handover and the
    // inventory format beside the shared versions.
    let tools = tmp.path().join("tools");
    let mut published: Value =
        serde_json::from_str(bfe_desktop::prepare::CHECKOUT_CONTENT_VERSION_JSON).unwrap();
    published["swbf2"] = json!(8);
    published["swbf2Lock"] = json!("stdin");
    published["swbf2Outputs"] = json!(1);
    fs::create_dir_all(&tools).unwrap();
    fs::write(tools.join("content-version.json"), published.to_string()).unwrap();

    let archive = tmp.path().join("swbf2.bfecontent");
    let exported =
        export_content_for(&content, GameFamily::Swbf2, &archive, Some(&tools)).unwrap();
    assert_eq!(exported.family, GameFamily::Swbf2);

    // Read the manifest bytes back from the archive and give them to the released validator.
    let mut f = fs::File::open(&archive).unwrap();
    let entries = bfe_desktop::archive::read_zip_entries(&mut f).unwrap();
    let manifest_entry = entries.get(MANIFEST_NAME).unwrap();
    // Read raw manifest bytes from local entry (store mode).
    use std::io::{Read, Seek, SeekFrom};
    f.seek(SeekFrom::Start(manifest_entry.offset)).unwrap();
    let mut lfh = [0u8; 30];
    f.read_exact(&mut lfh).unwrap();
    let nlen = u16::from_le_bytes([lfh[26], lfh[27]]) as u64;
    let elen = u16::from_le_bytes([lfh[28], lfh[29]]) as u64;
    f.seek(SeekFrom::Start(manifest_entry.offset + 30 + nlen + elen))
        .unwrap();
    let mut raw_manifest = vec![0u8; manifest_entry.size as usize];
    f.read_exact(&mut raw_manifest).unwrap();

    assert!(
        released::validate_archive_manifest(&raw_manifest).is_err(),
        "released build must decline a 2005 archive instead of importing it as flat 2004"
    );

    // 2004 inspect_archive and import_content must decline the 2005 archive with the released error.
    let err_inspect = inspect_archive(&archive).unwrap_err();
    assert_eq!(
        err_inspect,
        format!("Not prepared game content: {SWBF2_PREPARE_STATE_FILE}")
    );
    let dest_2004 = tmp.path().join("dest-2004");
    let err_import = import_content(&archive, &dest_2004).unwrap_err();
    assert_eq!(
        err_import,
        format!("Not prepared game content: {SWBF2_PREPARE_STATE_FILE}")
    );
    assert!(
        fs::read_dir(&dest_2004).unwrap().next().is_none(),
        "declined 2005 import must leave no content folder in the 2004 destination"
    );

    // 2005 inspect_archive_for and import_content_for succeed and mark ONLY swbf2/.content.
    let inspected_bf2 = inspect_archive_for(&archive, GameFamily::Swbf2).unwrap();
    assert_eq!(inspected_bf2.family, GameFamily::Swbf2);
    let dest = tmp.path().join("dest-2005");
    let imported = import_content_for(&archive, GameFamily::Swbf2, &dest, Some(&tools)).unwrap();
    assert_eq!(imported.family, GameFamily::Swbf2);
    assert!(!is_swbf2_marked_dir(&imported.content_dir.join("swbf2")));
    assert!(is_swbf2_marked_dir(&family_dir(
        &imported.content_dir,
        GameFamily::Swbf2
    )));
    assert!(family_dir(&imported.content_dir, GameFamily::Swbf2)
        .join("maps/testmap1/map.json")
        .is_file());
    assert!(!imported.content_dir.join("common/common.json").exists());
    assert_eq!(inspect_content(&imported.content_dir), None);
    assert_eq!(
        inspect_content(&family_dir(&imported.content_dir, GameFamily::Swbf2)),
        None
    );
}

#[test]
fn a_flat_copy_accepts_the_extensions_ignored_by_the_released_reader() {
    let tmp = TempDir::new("flat-extensions");
    let dir = tmp.path().join("swbf2");
    fs::create_dir_all(dir.join("maps/qq7")).unwrap();
    fs::create_dir_all(dir.join("common")).unwrap();
    fs::write(dir.join("common/common.json"), "{}").unwrap();
    fs::write(dir.join("maps/qq7/map.json"), "{}").unwrap();
    fs::write(dir.join(bfe_content::layout::SWBF2_MARKER_FILE), "").unwrap();
    let v = bfe_desktop::prepare::checkout_content_version().unwrap();
    let receipt = serde_json::json!({"version":v.state,"mapImportVersion":v.maps,"muzzleVersion":v.sides,
        "commonVersion":v.common,"failed":[],"skipped":[],"complete":true,"family":"swbf2","swbf2Version":null});
    for record in [Some(receipt.to_string()), Some("{".into()), None] {
        let path = dir.join(".prepare.json");
        if let Some(text) = record { fs::write(path, text).unwrap(); } else { fs::remove_file(path).unwrap(); }
        assert!(inspect_content(&dir).is_some());
    }
    fs::create_dir(dir.join(SWBF2_PREPARE_STATE_FILE)).unwrap();
    assert!(inspect_content(&dir).is_some());
}
