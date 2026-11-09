//! Non-Steam shortcut registration (`shortcuts.vdf`) and custom library artwork
//! installation (`userdata/<id>/config/grid/`).
//!
//! Mirrors the desktop host (`SHORTCUT_NAME`, `STEAM_ARG`,
//! `newShortcut`, `findShortcut`, `describeShortcut`, `shortcutCurrent`,
//! `upsertShortcut`, `removeShortcut`) and the desktop host
//! (`ARTWORK`, `LOGO_POSITION`, `writeShortcuts`, `placeArtwork`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discover::steam_roots;
use crate::steam::vdf::{
    compute_shortcut_app_id, compute_shortcut_game_id, normalize_shortcut_entry,
    parse_shortcuts_vdf, quote_exe, serialize_shortcuts_vdf, ShortcutEntry,
};

/// Display name used when registering Battlefront Eternal in Steam's `shortcuts.vdf`.
///
/// Mirrors `SHORTCUT_NAME` in the desktop host.
pub const SHORTCUT_NAME: &str = "Battlefront Eternal";

/// Command-line flag added to the Steam shortcut's `LaunchOptions` so the game
/// knows it was started by Steam even when launched through `flatpak-spawn --host`.
///
/// Mirrors `STEAM_ARG` in the desktop host.
pub const STEAM_ARG: &str = "--steam";

/// Custom Steam library artwork source files and their target `config/grid/<appId><suffix><ext>` suffixes.
///
/// Mirrors `ARTWORK` in the desktop host.
pub const ARTWORK: &[(&str, &str)] = &[
    ("capsule.jpg", ""),
    ("portrait.jpg", "p"),
    ("hero.jpg", "_hero"),
    ("logo.png", "_logo"),
    ("icon.png", "_icon"),
];

/// Enumerates `<steamRoot>/userdata/<accountId>/config` directories across all
/// discovered Steam roots for the current platform.
///
/// Filters `userdata` subdirectories to positive decimal account IDs (`^[1-9]\d{0,10}$`),
/// matching the userdata scan in `steamAccounts` (the desktop host) and
/// `shortcutsFile` / `gridDir` (`<root>/userdata/<account>/config`).
pub fn steam_userdata_dirs(
    platform: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
    extra_roots: &[PathBuf],
) -> Vec<PathBuf> {
    let mut roots = steam_roots(platform, home, env);
    roots.extend(extra_roots.iter().cloned());

    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();

    for root in roots {
        let userdata = root.join("userdata");
        let Ok(entries) = fs::read_dir(&userdata) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                continue;
            };
            // Account directories are positive base-10 integers ("0" is anonymous/unused).
            if !name.chars().all(|c| c.is_ascii_digit()) || name == "0" {
                continue;
            }
            let cfg_dir = userdata.join(&name).join("config");
            let key = cfg_dir.to_string_lossy().into_owned();
            if seen.insert(key) {
                out.push(cfg_dir);
            }
        }
    }
    out
}

/// Launch target resolved for registering a non-Steam shortcut.
///
/// Combines the executable and working directory from `gameLauncher` and
/// `shortcutLaunch` (the desktop host) with the shortcut icon path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutTarget {
    /// Path to executable or AppImage.
    pub exe: String,
    /// Working directory (`StartDir`).
    pub start_dir: String,
    /// Launch options (`"--steam"` or `"/path/to/root --steam"`).
    pub launch_options: String,
    /// Icon path (the desktop host or the desktop host).
    pub icon: String,
}

/// Options for [`resolve_executable_target`].
#[derive(Debug, Clone)]
pub struct ResolveTargetOptions {
    /// Target platform (`"linux"`, `"win32"`, `"darwin"`).
    pub platform: String,
    /// Environment variables (`APPIMAGE` on Linux).
    pub env: BTreeMap<String, String>,
    /// Current executable path (`std::env::current_exe()`).
    pub exec_path: PathBuf,
    /// Whether the build is packaged.
    pub packaged: bool,
    /// Project root directory (used for icon fallback and unpackaged dev launch options).
    pub project_root: PathBuf,
    /// Packaged `resourcesPath`, if running from a packaged build.
    pub resources_path: Option<PathBuf>,
}

/// Resolves the executable path, working directory, launch arguments, and icon
/// for registering Battlefront Eternal in `shortcuts.vdf`.
///
/// Mirrors the launcher and icon resolution in the desktop host
/// (`gameLauncher`, `shortcutLaunch`) and the desktop host (`add`).
pub fn resolve_executable_target(opts: &ResolveTargetOptions) -> ShortcutTarget {
    // On Linux, prefer the outer AppImage path when running from a mounted AppImage.
    let app_image = if opts.platform == "linux" {
        opts.env.get("APPIMAGE").filter(|s| !s.is_empty()).cloned()
    } else {
        None
    };
    let exe = app_image.unwrap_or_else(|| opts.exec_path.to_string_lossy().into_owned());
    let start_dir = Path::new(&exe)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| opts.project_root.to_string_lossy().into_owned());

    // Pass `--steam` (`STEAM_ARG`) so `startedBySteam` recognizes the Steam launch.
    let launch_options = if opts.packaged {
        STEAM_ARG.to_string()
    } else {
        format!("\"{}\" {}", opts.project_root.display(), STEAM_ARG)
    };

    let is_win = opts.platform == "win32" || opts.platform == "windows";
    let icon_name = if is_win { "icon.ico" } else { "icon.png" };
    let icon_candidates = [
        opts.resources_path.as_ref().map(|r| r.join(icon_name)),
        opts.exec_path.parent().map(|p| p.join(icon_name)),
        Some(opts.project_root.join(icon_name)),
        Some(opts.project_root.join("build").join("native").join(icon_name)),
        Some(opts.project_root.join("build").join("native").join("icon.png")),
    ];
    let icon = icon_candidates
        .into_iter()
        .flatten()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    ShortcutTarget {
        exe,
        start_dir,
        launch_options,
        icon,
    }
}

/// Checks whether a [`ShortcutEntry`] in `shortcuts.vdf` represents Battlefront Eternal.
///
/// Matches by [`SHORTCUT_NAME`], by quoted target executable path, or by
/// `--steam` (`STEAM_ARG`) + `battlefront[- ]eternal` in `Exe` / `LaunchOptions`
/// as in `findShortcut` (the desktop host).
pub fn matches_bfe_shortcut(entry: &ShortcutEntry, target_exe: Option<&str>) -> bool {
    if entry.app_name.trim().eq_ignore_ascii_case(SHORTCUT_NAME) {
        return true;
    }
    if let Some(exe) = target_exe.filter(|s| !s.is_empty()) {
        if quote_exe(&entry.exe) == quote_exe(exe) {
            return true;
        }
    }
    // Also match `findShortcut` in the desktop host: `--steam` token
    // in `LaunchOptions` and `battlefront[ -]eternal` in `Exe` or `LaunchOptions`.
    let has_steam_arg = entry
        .launch_options
        .split_whitespace()
        .any(|tok| tok == STEAM_ARG);
    if has_steam_arg {
        let combined = format!("{} {}", entry.exe, entry.launch_options).to_lowercase();
        if combined.contains("battlefront eternal") || combined.contains("battlefront-eternal") {
            return true;
        }
    }
    false
}

/// Result of upserting Battlefront Eternal into an existing list of [`ShortcutEntry`] items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertShortcutResult {
    /// Updated list of shortcuts.
    pub shortcuts: Vec<ShortcutEntry>,
    /// Upserted Battlefront Eternal shortcut entry.
    pub entry: ShortcutEntry,
    /// True if `shortcuts.vdf` needs to be rewritten (`!existed` or fields changed).
    pub changed: bool,
    /// True if a matching Battlefront Eternal entry was already present.
    pub existed: bool,
}

/// Inserts or updates the Battlefront Eternal shortcut inside `existing_shortcuts`.
///
/// Preserves user-chosen tags, hidden state, and `last_play_time` when updating
/// an existing shortcut.
///
/// Mirrors `upsertShortcut` and `newShortcut` in the desktop host.
pub fn upsert_shortcut_in_list(
    existing_shortcuts: &[ShortcutEntry],
    target: &ShortcutTarget,
) -> UpsertShortcutResult {
    let appid = compute_shortcut_app_id(&target.exe, SHORTCUT_NAME);
    let idx = existing_shortcuts
        .iter()
        .position(|item| matches_bfe_shortcut(item, Some(&target.exe)));
    let prev = idx.map(|i| &existing_shortcuts[i]);

    // Build the normalized entry, preserving user-owned properties from `prev` when present.
    let desired = normalize_shortcut_entry(&ShortcutEntry {
        appid,
        app_name: SHORTCUT_NAME.to_string(),
        exe: quote_exe(&target.exe),
        start_dir: target.start_dir.clone(),
        icon: if !target.icon.is_empty() {
            target.icon.clone()
        } else {
            prev.map(|p| p.icon.clone()).unwrap_or_default()
        },
        shortcut_path: prev.map(|p| p.shortcut_path.clone()).unwrap_or_default(),
        launch_options: target.launch_options.clone(),
        is_hidden: prev.map(|p| p.is_hidden).unwrap_or(0),
        allow_desktop_config: 1,
        allow_overlay: 1,
        open_vr: 0,
        devkit: 0,
        devkit_game_id: String::new(),
        devkit_override_app_id: 0,
        last_play_time: prev.map(|p| p.last_play_time).unwrap_or(0),
        flatpak_app_id: prev.map(|p| p.flatpak_app_id.clone()).unwrap_or_default(),
        sort_as: SHORTCUT_NAME.to_string(),
        tags: prev
            .filter(|p| !p.tags.is_empty())
            .map(|p| p.tags.clone())
            .unwrap_or_else(|| vec!["Battlefront Eternal".to_string()]),
    });

    if let Some(i) = idx {
        let current = normalize_shortcut_entry(&existing_shortcuts[i]);
        let changed = current != desired;
        let mut next = existing_shortcuts.to_vec();
        next[i] = desired.clone();
        UpsertShortcutResult {
            shortcuts: next,
            entry: desired,
            changed,
            existed: true,
        }
    } else {
        let mut next = existing_shortcuts.to_vec();
        next.push(desired.clone());
        UpsertShortcutResult {
            shortcuts: next,
            entry: desired,
            changed: true,
            existed: false,
        }
    }
}

/// Installs custom Steam library grid images (`<appId>.png`, `<appId>p.png`,
/// `<appId>_hero.png`, `<appId>_logo.png`, `<appId>_icon.png`) into `<config_dir>/grid`
/// without overwriting user-customized artwork.
///
/// Mirrors `placeArtwork` and `ARTWORK` in the desktop host.
pub fn install_grid_artwork(config_dir: &Path, app_id: u32, icon_path: &Path) -> Vec<PathBuf> {
    if icon_path.as_os_str().is_empty() || !icon_path.exists() {
        return Vec::new();
    }
    let grid_dir = config_dir.join("grid");
    if fs::create_dir_all(&grid_dir).is_err() {
        return Vec::new();
    }
    let ext = icon_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| format!(".{}", s.to_lowercase()))
        .unwrap_or_else(|| ".png".to_string());
    let targets = [
        format!("{}{}", app_id, ext),
        format!("{}p{}", app_id, ext),
        format!("{}_hero{}", app_id, ext),
        format!("{}_logo{}", app_id, ext),
        format!("{}_icon{}", app_id, ext),
    ];
    let mut written = Vec::new();
    for file_name in targets {
        let dest = grid_dir.join(file_name);
        if !dest.exists() && fs::copy(icon_path, &dest).is_ok() {
            written.push(dest);
        }
    }
    written
}

/// Summary returned by [`ensure_steam_shortcut`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnsureShortcutSummary {
    /// True if at least one Steam userdata directory was found and updated.
    pub ok: bool,
    /// Optional error reason when `ok == false`.
    pub reason: Option<String>,
    /// Computed 32-bit unsigned non-Steam AppID.
    pub app_id: u32,
    /// Computed 64-bit Steam GameID decimal string.
    pub game_id: String,
    /// Executable path registered in `shortcuts.vdf`.
    pub exe: String,
    /// Paths to `shortcuts.vdf` files that were modified.
    pub updated_files: Vec<PathBuf>,
    /// Paths to `shortcuts.vdf` files that already had an up-to-date shortcut.
    pub unchanged_files: Vec<PathBuf>,
    /// Paths to grid artwork files written.
    pub artwork_files: Vec<PathBuf>,
}

/// Ensures Battlefront Eternal is registered in every Steam account's
/// `userdata/<id>/config/shortcuts.vdf` and installs default grid artwork.
///
/// Writes updated `shortcuts.vdf` files atomically via a PID-tagged temporary file
/// as in `writeShortcuts` (the desktop host).
pub fn ensure_steam_shortcut(
    opts: &ResolveTargetOptions,
    home: Option<&Path>,
    extra_roots: &[PathBuf],
) -> EnsureShortcutSummary {
    let target = resolve_executable_target(opts);
    let app_id = compute_shortcut_app_id(&target.exe, SHORTCUT_NAME);
    let game_id = compute_shortcut_game_id(app_id);
    let config_dirs = steam_userdata_dirs(&opts.platform, home, &opts.env, extra_roots);

    if config_dirs.is_empty() {
        return EnsureShortcutSummary {
            ok: false,
            reason: Some("No Steam userdata directory was found on this machine.".to_string()),
            app_id,
            game_id,
            exe: target.exe,
            updated_files: Vec::new(),
            unchanged_files: Vec::new(),
            artwork_files: Vec::new(),
        };
    }

    let mut updated_files = Vec::new();
    let mut unchanged_files = Vec::new();
    let mut artwork_files = Vec::new();

    for config_dir in config_dirs {
        let vdf_file = config_dir.join("shortcuts.vdf");
        let existing = fs::read(&vdf_file)
            .ok()
            .and_then(|bytes| parse_shortcuts_vdf(&bytes).ok())
            .unwrap_or_default();

        let res = upsert_shortcut_in_list(&existing, &target);
        if res.changed || !vdf_file.exists() {
            if fs::create_dir_all(&config_dir).is_ok() {
                let tmp = vdf_file.with_extension(format!("vdf.{}.tmp", std::process::id()));
                let serialized = serialize_shortcuts_vdf(&res.shortcuts);
                if fs::write(&tmp, &serialized).is_ok() && fs::rename(&tmp, &vdf_file).is_ok() {
                    updated_files.push(vdf_file);
                }
            }
        } else {
            unchanged_files.push(vdf_file);
        }

        if !target.icon.is_empty() {
            artwork_files.extend(install_grid_artwork(
                &config_dir,
                res.entry.appid,
                Path::new(&target.icon),
            ));
        }
    }

    EnsureShortcutSummary {
        ok: true,
        reason: None,
        app_id,
        game_id,
        exe: target.exe,
        updated_files,
        unchanged_files,
        artwork_files,
    }
}

/// Read-only inspection status of the Battlefront Eternal non-Steam shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectShortcutStatus {
    /// True if at least one Steam userdata directory exists on the machine.
    pub steam_detected: bool,
    /// True if at least one account's `shortcuts.vdf` contains Battlefront Eternal.
    pub registered: bool,
    /// Computed 32-bit unsigned non-Steam AppID.
    pub app_id: u32,
    /// Computed 64-bit Steam GameID decimal string.
    pub game_id: String,
    /// Target executable path.
    pub exe: String,
    /// Number of Steam user accounts inspected.
    pub account_count: usize,
}

/// Inspects all Steam userdata accounts without modifying any files on disk.
///
/// Mirrors the read-only shortcut inspection in `state` (the desktop host).
pub fn inspect_steam_shortcut(
    opts: &ResolveTargetOptions,
    home: Option<&Path>,
    extra_roots: &[PathBuf],
) -> InspectShortcutStatus {
    let target = resolve_executable_target(opts);
    let app_id = compute_shortcut_app_id(&target.exe, SHORTCUT_NAME);
    let game_id = compute_shortcut_game_id(app_id);
    let config_dirs = steam_userdata_dirs(&opts.platform, home, &opts.env, extra_roots);

    let mut registered = false;
    for config_dir in &config_dirs {
        let vdf_file = config_dir.join("shortcuts.vdf");
        if let Ok(bytes) = fs::read(&vdf_file) {
            if let Ok(entries) = parse_shortcuts_vdf(&bytes) {
                if entries
                    .iter()
                    .any(|item| matches_bfe_shortcut(item, Some(&target.exe)))
                {
                    registered = true;
                    break;
                }
            }
        }
    }

    InspectShortcutStatus {
        steam_detected: !config_dirs.is_empty(),
        registered,
        app_id,
        game_id,
        exe: target.exe,
        account_count: config_dirs.len(),
    }
}
