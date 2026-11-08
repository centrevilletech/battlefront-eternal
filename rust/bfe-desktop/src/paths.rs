// Platform directory layout and local asset path resolution for the native
// desktop client.
//
// This module ports the path helpers from `electron/main.js` (`settingsFile`,
// `managedContentDir`, `contentRoot`, `hasGameData`, `likelyContentDirs`,
// `samePath`), `electron/update/state.js` (`updateRoot`), and
// `electron/content.js` (`CONTENT_ROOTS`, `DESKTOP_PREFIX`, `resolveBelow`,
// `locate`, `parseRange`, `parseContentArg`, `contentCandidates`,
// `defaultPreparedDir`). Keeping the exact directory names allows the native
// client to share prepared game data, settings, and installed updates with an
// existing installation without moving files on disk.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Top-level subdirectories produced by `tools/prepare_content.py` inside a prepared
/// game data folder.
///
/// Mirrors `CONTENT_ROOTS` in `electron/content.js`.
pub const CONTENT_ROOTS: &[&str] = &["maps", "sides", "common", "audio"];

/// URL path prefix for the desktop shell's own internal pages.
///
/// Mirrors `DESKTOP_PREFIX` in `electron/content.js`.
pub const DESKTOP_PREFIX: &str = "__desktop";

/// Custom protocol scheme and host used by the desktop shell.
///
/// Mirrors `SCHEME`, `HOST`, and `ORIGIN` in `electron/content.js`.
pub const SCHEME: &str = "app";
/// Host component of `app://bfe`.
pub const HOST: &str = "bfe";
/// Origin string `app://bfe`.
pub const ORIGIN: &str = "app://bfe";

/// Checkout subdirectories searched beneath the user's home folder by
/// [`likely_content_dirs`].
///
/// Mirrors `checkouts` in `likelyContentDirs` in `electron/main.js`.
const CHECKOUT_PARENTS: &[&str] = &[
    "",
    "src",
    "code",
    "Code",
    "dev",
    "git",
    "projects",
    "Projects",
    "repos",
    "Documents",
    "Desktop",
    "Downloads",
];

/// Summary of the directories used by the desktop client on the current host.
///
/// Returned by [`desktop_paths`] and [`resolve_desktop_paths`] and printed as JSON
/// by `desktop-tool paths`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPaths {
    /// Per-user configuration directory (`~/.config/Battlefront Eternal` on Linux).
    pub user_data_dir: PathBuf,
    /// Alias for `user_data_dir` used by update and CLI callers.
    pub user_data: PathBuf,
    /// Managed prepared content directory (`<userData>/game-data`).
    pub managed_content_dir: PathBuf,
    /// Active content root directory resolved from CLI/env/settings.
    pub content_root: PathBuf,
    /// Path to `<userData>/desktop-settings.json`.
    pub settings_file: PathBuf,
    /// Path to `<userData>/player-settings.json`.
    pub player_settings_file: PathBuf,
    /// Path to `<userData>/prepare-timings.json`.
    pub timings_file: PathBuf,
    /// Path to `<userData>/logs/prepare.log`.
    pub prepare_log_file: PathBuf,
    /// Optional self-update root directory (`~/.local/share/battlefront-eternal` on Linux).
    pub update_root: Option<PathBuf>,
    /// Repository or packaged application root directory.
    pub project_root: PathBuf,
}

/// Options for [`resolve_desktop_paths`].
#[derive(Debug, Clone, Default)]
pub struct DesktopPathsOptions {
    /// Target platform (`"linux"`, `"darwin"`, `"win32"`).
    pub platform: String,
    /// User's home directory.
    pub home: Option<PathBuf>,
    /// Current working directory.
    pub cwd: PathBuf,
    /// Project or packaged root directory.
    pub project_root: PathBuf,
    /// Directory containing the running executable.
    pub exec_dir: Option<PathBuf>,
    /// Packaged `resources` directory, if running from a packaged build.
    pub resources_path: Option<PathBuf>,
    /// Whether the build is packaged.
    pub packaged: bool,
    /// Environment variable map.
    pub env: BTreeMap<String, String>,
}

/// Resolves a [`DesktopPaths`] snapshot from explicit [`DesktopPathsOptions`].
pub fn resolve_desktop_paths(opts: &DesktopPathsOptions) -> DesktopPaths {
    let env_hash: HashMap<String, String> = opts
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let home = opts
        .home
        .clone()
        .unwrap_or_else(|| home_dir_from_env(&env_hash));
    let user_data = user_data_dir_with(&opts.platform, &env_hash, &home);
    let managed = managed_content_dir(&user_data);
    let settings = crate::settings::read_desktop_settings(&user_data);
    let forced = opts
        .env
        .get("BFE_CONTENT_DIR")
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    let chosen = settings.chosen_content_dir.as_deref().map(PathBuf::from);
    let found = settings.found_content_dir.as_deref().map(PathBuf::from);
    let active_root = content_root(
        forced.as_deref(),
        chosen.as_deref(),
        found.as_deref(),
        &user_data,
    );
    DesktopPaths {
        settings_file: settings_file(&user_data),
        player_settings_file: player_settings_file(&user_data),
        timings_file: timings_file(&user_data),
        prepare_log_file: prepare_log_file(&user_data),
        user_data_dir: user_data.clone(),
        user_data,
        managed_content_dir: managed,
        content_root: active_root,
        update_root: update_root_with(&opts.platform, &env_hash, &home),
        project_root: opts.project_root.clone(),
    }
}

/// Directory layout for the desktop auto-updater under `<userData>/updates`.
///
/// Mirrors `updatePaths` in `electron/update/state.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePaths {
    /// Root update directory (`<userData>/updates`).
    pub root: PathBuf,
    /// Path to `update-state.json` (`<userData>/updates/update-state.json`).
    pub state_file: PathBuf,
    /// Installed payload slots directory (`<userData>/updates/payloads`).
    pub payloads_dir: PathBuf,
    /// Temporary extraction staging directory (`<userData>/updates/staging`).
    pub staging_dir: PathBuf,
    /// Downloaded full installers directory (`<userData>/updates/downloads`).
    pub downloads_dir: PathBuf,
}

/// Computes the [`UpdatePaths`] subdirectories inside `user_data_dir`.
///
/// Mirrors `updatePaths` in `electron/update/state.js`.
pub fn update_paths(user_data_dir: &Path) -> UpdatePaths {
    let root = user_data_dir.join("updates");
    UpdatePaths {
        state_file: root.join("update-state.json"),
        payloads_dir: root.join("payloads"),
        staging_dir: root.join("staging"),
        downloads_dir: root.join("downloads"),
        root,
    }
}

/// Inclusive 0-based byte range parsed from an HTTP `Range` header.
///
/// Mirrors the `{ start, end }` object returned by `parseRange` in
/// `electron/content.js`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// Inclusive starting byte offset.
    pub start: u64,
    /// Inclusive ending byte offset.
    pub end: u64,
}

/// Roots passed to [`locate`] when mapping a URL path to a local file on disk.
///
/// Mirrors the `{ appRoot, contentRoot, uiRoot }` parameter of `locate` in
/// `electron/content.js`.
#[derive(Debug, Clone)]
pub struct LocateRoots<'a> {
    /// Root directory for application static assets (`src/`).
    pub app_root: &'a Path,
    /// Root directory for prepared game content (`maps/`, `sides/`, `common/`, `audio/`).
    pub content_root: Option<&'a Path>,
    /// Root directory for desktop UI pages (`/__desktop/`).
    pub ui_root: Option<&'a Path>,
}

/// Resolves `target` relative to `base` and lexically normalizes `.` and `..`
/// components without requiring the path to exist on disk.
///
/// Mirrors Node's `path.resolve(base, target)`.
pub fn resolve_from(base: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        normalize_lexical(target)
    } else {
        normalize_lexical(&base.join(target))
    }
}

/// Normalizes a path into a clean forward-slash string with no trailing slash
/// for cross-platform map keys and comparisons.
pub fn normalize_comparable_path(path: &Path) -> String {
    let norm = normalize_lexical(path);
    let s = norm.to_string_lossy().replace('\\', "/");
    if s.len() > 1 {
        s.trim_end_matches('/').to_string()
    } else {
        s
    }
}

/// Parses `--content <dir>` or `--content=<dir>` from command-line arguments.
///
/// Mirrors `parseContentArg` in `electron/content.js`.
pub fn parse_content_arg(argv: &[String], cwd: &Path) -> Option<PathBuf> {
    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--content" || arg == "--content-dir" {
            if let Some(val) = argv.get(i + 1).filter(|s| !s.trim().is_empty()) {
                return Some(resolve_from(cwd, Path::new(val.trim())));
            }
        }
        if let Some(val) = arg
            .strip_prefix("--content=")
            .or_else(|| arg.strip_prefix("--content-dir="))
        {
            if !val.trim().is_empty() {
                return Some(resolve_from(cwd, Path::new(val.trim())));
            }
        }
        i += 1;
    }
    None
}

/// Input environment for [`content_candidates`].
///
/// Mirrors the argument object of `contentCandidates` in `electron/content.js`.
#[derive(Debug, Clone, Default)]
pub struct ContentCandidateEnv {
    /// Command-line arguments (`process.argv`).
    pub argv: Vec<String>,
    /// Environment variables (`BFE_CONTENT_DIR`).
    pub env: BTreeMap<String, String>,
    /// Current working directory (`process.cwd()`).
    pub cwd: PathBuf,
    /// Project root directory.
    pub project_root: PathBuf,
    /// Optional userData directory (`app.getPath("userData")`).
    pub user_data: Option<PathBuf>,
    /// Optional directory containing the executable (`path.dirname(process.execPath)`).
    pub exec_dir: Option<PathBuf>,
    /// Optional packaged resources directory (`process.resourcesPath`).
    pub resources_path: Option<PathBuf>,
    /// Whether the application is packaged (`app.isPackaged`).
    pub packaged: bool,
}

/// Returns the ordered, deduplicated list of candidate prepared content directories.
///
/// Order:
/// 1. `--content` / `--content-dir` CLI flag
/// 2. `BFE_CONTENT_DIR` environment variable
/// 3. `<userData>/content` and `<userData>/game-data`
/// 4. Packaged `<resourcesPath>/content` and `<execDir>/content`, or unpackaged
///    `<projectRoot>/public`, `<projectRoot>/content`, `<cwd>/public`, `<cwd>/content`
///
/// Mirrors `contentCandidates` in `electron/content.js`.
pub fn content_candidates(opts: &ContentCandidateEnv) -> Vec<PathBuf> {
    let mut raw = Vec::new();
    if let Some(cli) = parse_content_arg(&opts.argv, &opts.cwd) {
        raw.push(cli);
    }
    if let Some(env_dir) = opts
        .env
        .get("BFE_CONTENT_DIR")
        .filter(|s| !s.trim().is_empty())
    {
        raw.push(resolve_from(&opts.cwd, Path::new(env_dir.trim())));
    }
    if let Some(ud) = &opts.user_data {
        raw.push(resolve_from(&opts.cwd, &ud.join("content")));
        raw.push(resolve_from(&opts.cwd, &ud.join("game-data")));
    }
    if opts.packaged {
        if let Some(res) = &opts.resources_path {
            raw.push(resolve_from(&opts.cwd, &res.join("content")));
        }
        if let Some(ed) = &opts.exec_dir {
            raw.push(resolve_from(&opts.cwd, &ed.join("content")));
        }
    } else {
        raw.push(resolve_from(&opts.cwd, &opts.project_root.join("public")));
        raw.push(resolve_from(&opts.cwd, &opts.project_root.join("content")));
        raw.push(resolve_from(&opts.cwd, &opts.cwd.join("public")));
        raw.push(resolve_from(&opts.cwd, &opts.cwd.join("content")));
    }

    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for p in raw {
        let key = normalize_comparable_path(&p);
        if seen.insert(key) {
            out.push(p);
        }
    }
    out
}

/// Returns the default target directory where newly prepared content should be written.
///
/// Mirrors `defaultPreparedDir` in `electron/content.js`.
pub fn default_prepared_dir(
    user_data: Option<&Path>,
    project_root: &Path,
    packaged: bool,
) -> PathBuf {
    if packaged {
        if let Some(ud) = user_data {
            return ud.join("content");
        }
    }
    if let Some(ud) = user_data {
        if !project_root.join("public").exists() {
            return ud.join("content");
        }
    }
    project_root.join("public")
}

/// Returns the per-user configuration directory matching Electron's
/// `app.getPath("userData")` for `productName: "Battlefront Eternal"`.
///
/// On Linux this is `${XDG_CONFIG_HOME:-~/.config}/Battlefront Eternal`;
/// on Windows it is `%APPDATA%\Battlefront Eternal`.
pub fn user_data_dir() -> PathBuf {
    let env_map: HashMap<String, String> = env::vars().collect();
    let home = home_dir_from_env(&env_map);
    let platform = current_platform_name();
    user_data_dir_with(platform, &env_map, &home)
}

/// Pure version of [`user_data_dir`] that accepts an explicit platform name,
/// environment map, and home directory for deterministic cross-platform tests.
pub fn user_data_dir_with(
    platform: &str,
    env_map: &HashMap<String, String>,
    home: &Path,
) -> PathBuf {
    if let Some(override_dir) = env_map.get("BFE_USER_DATA_DIR") {
        let trimmed = override_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    match platform {
        "win32" | "windows" => {
            let base = env_map
                .get("APPDATA")
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData").join("Roaming"));
            base.join("Battlefront Eternal")
        }
        "darwin" | "macos" => home
            .join("Library")
            .join("Application Support")
            .join("Battlefront Eternal"),
        _ => {
            let base = env_map
                .get("XDG_CONFIG_HOME")
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".config"));
            base.join("Battlefront Eternal")
        }
    }
}

/// Ports `updateRoot` from `electron/update/state.js`.
///
/// Returns the per-user directory where self-updated application builds are
/// stored (`~/.local/share/battlefront-eternal` on Linux,
/// `%LOCALAPPDATA%\Battlefront Eternal` on Windows, or `None` elsewhere).
pub fn update_root() -> Option<PathBuf> {
    let env_map: HashMap<String, String> = env::vars().collect();
    let home = home_dir_from_env(&env_map);
    let platform = current_platform_name();
    update_root_with(platform, &env_map, &home)
}

/// Pure version of [`update_root`] taking explicit `platform`, `env_map`, and
/// `home` values, matching `updateRoot({ platform, env, home })` in
/// `electron/update/state.js`.
pub fn update_root_with(
    platform: &str,
    env_map: &HashMap<String, String>,
    home: &Path,
) -> Option<PathBuf> {
    match platform {
        "linux" => {
            let base = env_map
                .get("XDG_DATA_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local").join("share"));
            Some(base.join("battlefront-eternal"))
        }
        "win32" | "windows" => {
            let base = env_map
                .get("LOCALAPPDATA")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData").join("Local"));
            Some(base.join("Battlefront Eternal"))
        }
        _ => None,
    }
}

/// Ports `managedContentDir` from `electron/main.js`.
///
/// Returns `<userData>/game-data`, the application-managed folder into which
/// `tools/prepare_content.py` extracts and converts the player's retail copy.
pub fn managed_content_dir(user_data: &Path) -> PathBuf {
    user_data.join("game-data")
}

/// Ports `settingsFile` from `electron/main.js`.
///
/// Returns `<userData>/desktop-settings.json`.
pub fn settings_file(user_data: &Path) -> PathBuf {
    user_data.join("desktop-settings.json")
}

/// Returns `<userData>/player-settings.json`, matching `PLAYER_SETTINGS_FILE`
/// in `electron/player-settings.js`.
pub fn player_settings_file(user_data: &Path) -> PathBuf {
    user_data.join(crate::settings::PLAYER_SETTINGS_FILE)
}

/// Ports `timingsFile` from `electron/main.js`.
///
/// Returns `<userData>/prepare-timings.json`.
pub fn timings_file(user_data: &Path) -> PathBuf {
    user_data.join("prepare-timings.json")
}

/// Returns `<userData>/logs/prepare.log`, the log file path passed to
/// `new Preparer` in `electron/main.js`.
pub fn prepare_log_file(user_data: &Path) -> PathBuf {
    user_data.join("logs").join("prepare.log")
}

/// Ports `hasGameData` from `electron/main.js`.
///
/// Returns true when `root` is non-empty and `<root>/common/common.json` exists.
pub fn has_game_data(root: &Path) -> bool {
    root.join("common").join("common.json").exists()
}

/// Ports `samePath` from `electron/main.js`.
///
/// Compares two directory paths as the target platform's filesystem does:
/// exact byte match on Linux, ASCII case-folded on Windows and macOS.
pub fn same_path(a: Option<&Path>, b: Option<&Path>, platform: &str) -> bool {
    let (Some(pa), Some(pb)) = (a, b) else {
        return false;
    };
    if pa.as_os_str().is_empty() || pb.as_os_str().is_empty() {
        return false;
    }
    let xa = normalize_lexical(pa).to_string_lossy().to_string();
    let xb = normalize_lexical(pb).to_string_lossy().to_string();
    if platform == "linux" {
        xa == xb
    } else {
        xa.eq_ignore_ascii_case(&xb)
    }
}

/// Ports `contentRoot` from `electron/main.js`.
///
/// Selects the active content folder in priority order: `forced_content_dir`
/// (`--content-dir=` or `BFE_CONTENT_DIR`), `chosen_content_dir` from
/// `desktop-settings.json`, `active_content_dir`, or `managed_content_dir`.
pub fn content_root(
    forced_content_dir: Option<&Path>,
    chosen_content_dir: Option<&Path>,
    active_content_dir: Option<&Path>,
    user_data: &Path,
) -> PathBuf {
    if let Some(forced) = forced_content_dir.filter(|p| !p.as_os_str().is_empty()) {
        return normalize_lexical(forced);
    }
    if let Some(chosen) = chosen_content_dir.filter(|p| !p.as_os_str().is_empty()) {
        return normalize_lexical(chosen);
    }
    if let Some(active) = active_content_dir.filter(|p| !p.as_os_str().is_empty()) {
        return normalize_lexical(active);
    }
    normalize_lexical(&managed_content_dir(user_data))
}

/// Ports `likelyContentDirs` from `electron/main.js`.
///
/// Lists candidate folders where extracted game data may already sit on the
/// machine: `foundContentDir` from settings, `<projectRoot>/public` when not
/// packaged, `game-data` beside the executable or AppImage, and common
/// `battlefront-eternal/public` checkouts under the home directory.
pub fn likely_content_dirs(
    found_content_dir: Option<&Path>,
    packaged: bool,
    project_root: &Path,
    exe_path: &Path,
    app_image: Option<&Path>,
    home: &Path,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(found) = found_content_dir.filter(|p| !p.as_os_str().is_empty()) {
        dirs.push(found.to_path_buf());
    }
    if !packaged {
        dirs.push(project_root.join("public"));
    }
    if let Some(exe_dir) = exe_path.parent() {
        dirs.push(exe_dir.join("game-data"));
    }
    if let Some(app_img) = app_image {
        if let Some(app_img_dir) = app_img.parent() {
            dirs.push(app_img_dir.join("game-data"));
        }
    }
    for sub in CHECKOUT_PARENTS {
        let base = if sub.is_empty() {
            home.to_path_buf()
        } else {
            home.join(sub)
        };
        dirs.push(base.join("battlefront-eternal").join("public"));
    }
    dirs
}

/// Resolves the repository or packaged application root directory.
///
/// Checks `BFE_PROJECT_ROOT` first, then walks ancestors of the executable
/// and current working directory looking for `tools/content-version.json`.
pub fn project_root() -> PathBuf {
    if let Ok(explicit) = env::var("BFE_PROJECT_ROOT") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(cwd) = env::current_dir() {
        for ancestor in cwd.ancestors() {
            if ancestor.join("tools").join("content-version.json").is_file() {
                return ancestor.to_path_buf();
            }
        }
    }
    if let Ok(exe) = env::current_exe() {
        for ancestor in exe.ancestors() {
            if ancestor.join("tools").join("content-version.json").is_file() {
                return ancestor.to_path_buf();
            }
        }
        if let Some(parent) = exe.parent() {
            return parent.to_path_buf();
        }
    }
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Builds a complete [`DesktopPaths`] snapshot for the current host, reading
/// `desktop-settings.json` to resolve the active `content_root`.
pub fn desktop_paths() -> DesktopPaths {
    let user_data = user_data_dir();
    let managed = managed_content_dir(&user_data);
    let settings = crate::settings::read_desktop_settings(&user_data);
    let forced = env::var("BFE_CONTENT_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from);
    let chosen = settings.chosen_content_dir.as_deref().map(PathBuf::from);
    let found = settings.found_content_dir.as_deref().map(PathBuf::from);
    let active_root = content_root(
        forced.as_deref(),
        chosen.as_deref(),
        found.as_deref(),
        &user_data,
    );
    DesktopPaths {
        settings_file: settings_file(&user_data),
        player_settings_file: player_settings_file(&user_data),
        timings_file: timings_file(&user_data),
        prepare_log_file: prepare_log_file(&user_data),
        user_data_dir: user_data.clone(),
        user_data,
        managed_content_dir: managed,
        content_root: active_root,
        update_root: update_root(),
        project_root: project_root(),
    }
}

/// Ports the `TYPES` table lookup in `electron/content.js`.
///
/// Maps a file's extension to its HTTP `Content-Type` header value.
pub fn content_type(file_path: &Path) -> &'static str {
    let ext = file_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "ktx2" => "image/ktx2",
        "wav" => "audio/wav",
        "ogg" | "opus" => "audio/ogg",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "bik" | "bin" => "application/octet-stream",
        "ttf" => "font/ttf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Ports `resolveBelow` from `electron/content.js`.
///
/// Decodes percent-escapes in `url_path`, rejects NUL bytes, and resolves the
/// path beneath `root`, returning `None` if it would escape `root`.
pub fn resolve_below(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path)?;
    if decoded.contains('\0') {
        return None;
    }
    let base = normalize_lexical(root);
    let posix_norm = posix_normalize(&format!("/{decoded}"));
    let rel = posix_norm.trim_start_matches('/');
    let candidate = normalize_lexical(&base.join(rel));
    if candidate == base || candidate.starts_with(&base) {
        Some(candidate)
    } else {
        None
    }
}

/// Ports `locate` from `electron/content.js`.
///
/// Routes `/maps/...`, `/sides/...`, `/common/...` and `/audio/...` to
/// `content_root`, `/__desktop/...` to `ui_root`, and everything else to
/// `app_root` (mapping `/` to `/index.html`).
pub fn locate(url_path: &str, roots: &LocateRoots<'_>) -> Option<PathBuf> {
    let first = url_path.split('/').nth(1).unwrap_or("");
    if CONTENT_ROOTS.contains(&first) {
        let content_root = roots.content_root?;
        return resolve_below(content_root, url_path);
    }
    if first == DESKTOP_PREFIX {
        let ui_root = roots.ui_root?;
        let rest = &url_path[(DESKTOP_PREFIX.len() + 1).min(url_path.len())..];
        return resolve_below(ui_root, rest);
    }
    let target = if url_path == "/" {
        "/index.html"
    } else {
        url_path
    };
    resolve_below(roots.app_root, target)
}

/// Ports `parseRange` from `electron/content.js`.
///
/// Supports `"bytes=a-b"`, `"bytes=a-"` and suffix ranges `"bytes=-n"` against
/// a file of `size` bytes. Returns `None` when the header does not match
/// `^bytes=(\d*)-(\d*)$` or both sides are empty, `Some(Err(()))` when
/// `"unsatisfiable"`, and `Some(Ok(ByteRange))` when valid.
pub fn parse_range(header: Option<&str>, size: u64) -> Option<Result<ByteRange, ()>> {
    let trimmed = header.unwrap_or("").trim();
    let rest = trimmed.strip_prefix("bytes=")?;
    let (left, right) = rest.split_once('-')?;
    if left.is_empty() && right.is_empty() {
        return None;
    }
    if !left.bytes().all(|b| b.is_ascii_digit()) || !right.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if size == 0 {
        return Some(Err(()));
    }
    let (start, end) = if left.is_empty() {
        let suffix: u64 = right.parse().ok()?;
        if suffix == 0 {
            return Some(Err(()));
        }
        let s = size.saturating_sub(suffix);
        (s, size - 1)
    } else {
        let s: u64 = left.parse().ok()?;
        let e: u64 = if right.is_empty() {
            size - 1
        } else {
            right.parse::<u64>().ok()?.min(size - 1)
        };
        (s, e)
    };
    if start > end || start >= size {
        return Some(Err(()));
    }
    Some(Ok(ByteRange { start, end }))
}

/// Lexically normalizes a filesystem path, resolving `.` and `..` segments
/// relative to the current working directory if relative, without requiring
/// the target to exist on disk.
pub fn normalize_lexical(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut out = PathBuf::new();
    for comp in abs.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn posix_normalize(path: &str) -> String {
    let mut stack: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            stack.pop();
        } else {
            stack.push(seg);
        }
    }
    format!("/{}", stack.join("/"))
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hi = hex_val(bytes[i + 1])?;
            let lo = hex_val(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn current_platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

pub(crate) fn home_dir_from_env(env_map: &HashMap<String, String>) -> PathBuf {
    for key in ["HOME", "USERPROFILE"] {
        if let Some(val) = env_map.get(key) {
            if !val.trim().is_empty() {
                return PathBuf::from(val);
            }
        }
    }
    let _ = fs::metadata(".");
    PathBuf::from(".")
}
