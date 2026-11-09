//! Game installation and prepared content discovery across Steam, GOG, Heroic,
//! Wine, CrossOver, and local filesystem directories.
//!
//! Mirrors the desktop host.
//!
//! ## Discovery Hierarchy
//! - **Prepared content (`find_content`)**:
//!   1. Explicit directories (`--content`, `BFE_CONTENT_DIR`, or user-saved `settings.contentDir`)
//!      are checked first; the first valid folder wins with `source: "chosen"`.
//!   2. Known likely locations (`default_prepared_dir`, `./prepared`, `<app>/prepared`, etc.)
//!      are compared by map count (`richer_content`) and returned with `source: "found"`.
//!   3. When `search` is enabled, a bounded breadth-first search of the user's home directory
//!      (`depth: 4`, `budget_ms: 2500`, `max_dirs: 20_000`) returns the richest folder with
//!      `source: "searched"`.
//! - **Retail game installation (`find_game_installs`)**:
//!   Validates `GameData/Data/_LVL_PC/SIDE` and `GameData/Data/_LVL_PC/Common/<LANG>/common.lvl`
//!   case-insensitively so Linux/macOS filesystems handle any retail casing, and distinguishes
//!   Star Wars Battlefront (2004) from Star Wars Battlefront II (2005).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::paths::{normalize_comparable_path, resolve_from};

/// Steam App ID for Star Wars Battlefront (Classic, 2004).
///
/// Mirrors `BF1_STEAM_APP` in the desktop host.
pub const BF1_STEAM_APP: &str = "1058020";

/// Store URL for the GOG edition of Star Wars Battlefront (Classic, 2004).
///
/// Mirrors `BF1_GOG_PAGE` in the desktop host.
pub const BF1_GOG_PAGE: &str = "https://www.gog.com/game/star_wars_battlefront";

/// Directory basenames skipped when scanning the user's home folder for prepared content.
///
/// Skipping package caches, virtual environments, media folders, and system mounts
/// keeps `search_content` within its 2.5-second / 20,000-directory budget.
/// Mirrors `SKIP` in the desktop host.
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".cache",
    ".npm",
    ".nvm",
    ".cargo",
    ".rustup",
    "proc",
    "sys",
    "AppData",
    "Library",
    "Applications",
    "Pictures",
    "Music",
    "Movies",
    "Videos",
    "snap",
    ".var",
    ".wine",
    "go",
    ".gradle",
    ".m2",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    "release",
];

/// Relative paths on Windows drive roots (`C:\..Z:\`) checked for Steam libraries.
///
/// Mirrors `DRIVE_LIBRARIES` in the desktop host.
pub const DRIVE_LIBRARIES: &[&str] = &[
    "SteamLibrary",
    "Steam",
    "Program Files (x86)\\Steam",
    "Program Files\\Steam",
    "Games\\Steam",
    "Games\\SteamLibrary",
    "SteamGames",
];

/// Relative paths on Windows drive roots checked for GOG installations.
///
/// Mirrors `DRIVE_GOG_FOLDERS` in the desktop host.
pub const DRIVE_GOG_FOLDERS: &[&str] = &[
    "GOG Games",
    "Games\\GOG Games",
    "Games",
    "Program Files (x86)\\GOG Galaxy\\Games",
    "Program Files\\GOG Galaxy\\Games",
];

/// Relative paths inside Wine or CrossOver prefixes checked for GOG installations.
///
/// Mirrors `PREFIX_GOG_FOLDERS` in the desktop host.
pub const PREFIX_GOG_FOLDERS: &[&str] = &[
    "drive_c/GOG Games",
    "drive_c/Program Files (x86)/GOG Galaxy/Games",
    "drive_c/Program Files/GOG Galaxy/Games",
];

/// Metadata for a validated prepared content directory (`common/common.json` + `maps/*`).
///
/// Mirrors the object returned by `inspectContent` and `findContent` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentInfo {
    /// Canonical or resolved path to the prepared content directory.
    pub dir: PathBuf,
    /// Sorted list of map subdirectory names present under `<dir>/maps`.
    pub maps: Vec<String>,
    /// Discovery origin (`"chosen"`, `"found"`, or `"searched"`), when resolved via [`find_content`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Inspects `dir` to see if it is a valid prepared content folder.
///
/// A folder is considered prepared content when `<dir>/common/common.json` exists.
/// Returns the resolved directory path and the sorted list of map subdirectories
/// inside `<dir>/maps`.
///
/// Mirrors `inspectContent` in the desktop host.
pub fn inspect_content(dir: &Path) -> Option<ContentInfo> {
    if dir.as_os_str().is_empty() {
        return None;
    }
    let common = dir.join("common").join("common.json");
    if !common.exists() {
        return None;
    }
    // Enumerate immediate map subdirectories under `<dir>/maps` and sort alphabetically.
    let mut maps = Vec::new();
    if let Ok(entries) = fs::read_dir(dir.join("maps")) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    maps.push(name.to_string());
                }
            }
        }
    }
    maps.sort();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    Some(ContentInfo {
        dir: resolve_from(&cwd, dir),
        maps,
        source: None,
    })
}

/// Returns the candidate with the larger `maps` count, preferring the existing
/// `best` when tied so earlier candidates in the search order win ties.
///
/// Mirrors `richerContent` in the desktop host.
fn richer_content(best: Option<ContentInfo>, candidate: ContentInfo) -> Option<ContentInfo> {
    match best {
        None => Some(candidate),
        Some(b) => {
            if candidate.maps.len() > b.maps.len() {
                Some(candidate)
            } else {
                Some(b)
            }
        }
    }
}

/// Options controlling the breadth-first home directory search in [`search_content`].
///
/// Mirrors the second argument of `searchContent(root, { depth, budgetMs, maxDirs })`
/// in the desktop host.
#[derive(Debug, Clone)]
pub struct SearchContentOptions {
    /// Maximum directory traversal depth below `root` (defaults to `4`).
    pub depth: usize,
    /// Maximum wall-clock time budget in milliseconds (defaults to `2500`).
    pub budget_ms: u64,
    /// Maximum number of directories to inspect before stopping (defaults to `20000`).
    pub max_dirs: usize,
}

impl Default for SearchContentOptions {
    fn default() -> Self {
        Self {
            depth: 4,
            budget_ms: 2500,
            max_dirs: 20_000,
        }
    }
}

/// Breadth-first searches `root` for the richest prepared content directory.
///
/// Stops descending into any directory that already contains a valid `common/common.json`,
/// skips symlinks, hidden dot-directories, and names in [`SKIP_DIRS`], and respects
/// `depth`, `budget_ms`, and `max_dirs`.
///
/// Mirrors `searchContent` in the desktop host.
pub fn search_content(root: &Path, opts: &SearchContentOptions) -> Option<ContentInfo> {
    if root.as_os_str().is_empty() {
        return None;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let deadline = Instant::now() + Duration::from_millis(opts.budget_ms);
    let skip: BTreeSet<&str> = SKIP_DIRS.iter().copied().collect();

    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    queue.push_back((resolve_from(&cwd, root), 0));
    let mut visited = 0usize;
    let mut best: Option<ContentInfo> = None;

    // Traverse directories in breadth-first order so shallow checkouts are found first.
    while let Some((dir, level)) = queue.pop_front() {
        if visited >= opts.max_dirs || Instant::now() > deadline {
            break;
        }
        visited += 1;

        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut child_dirs = Vec::new();
        let mut has_common = false;

        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            // Never traverse symbolic links to avoid directory cycles.
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                continue;
            };
            if name == "common" {
                has_common = true;
            }
            child_dirs.push(name);
        }

        // If `dir` is itself a valid prepared content root, record it and do not descend into its subtrees.
        if has_common {
            if let Some(found) = inspect_content(&dir) {
                best = richer_content(best, found);
                continue;
            }
        }
        if level >= opts.depth {
            continue;
        }
        for name in child_dirs {
            if name.starts_with('.') || skip.contains(name.as_str()) {
                continue;
            }
            queue.push_back((dir.join(name), level + 1));
        }
    }

    best
}

/// Options for [`find_content`].
///
/// Mirrors the argument object of `findContent` in the desktop host.
#[derive(Debug, Clone, Default)]
pub struct FindContentOptions {
    /// Explicitly chosen content directories checked first (first valid match wins).
    pub explicit: Vec<PathBuf>,
    /// Known/likely content directories compared by map richness.
    pub likely: Vec<PathBuf>,
    /// Optional home directory to search when neither `explicit` nor `likely` matches.
    pub home: Option<PathBuf>,
    /// Whether to fall back to [`search_content`] under `home`.
    pub search: bool,
}

/// Locates prepared content from explicit choices, known likely locations, or a home search.
///
/// Tags the returned [`ContentInfo::source`] as `"chosen"`, `"found"`, or `"searched"`.
/// Mirrors `findContent` in the desktop host.
pub fn find_content(opts: &FindContentOptions) -> Option<ContentInfo> {
    // 1. User-chosen or CLI/env-forced directories take top priority.
    for dir in &opts.explicit {
        if let Some(mut content) = inspect_content(dir) {
            content.source = Some("chosen".to_string());
            return Some(content);
        }
    }
    // 2. Compare all known likely directories and pick the one with the most maps.
    let mut known: Option<ContentInfo> = None;
    for dir in &opts.likely {
        if let Some(content) = inspect_content(dir) {
            known = richer_content(known, content);
        }
    }
    if let Some(mut k) = known {
        k.source = Some("found".to_string());
        return Some(k);
    }
    // 3. Fall back to a bounded breadth-first search of the user's home directory.
    if opts.search {
        if let Some(home) = &opts.home {
            if let Some(mut searched) = search_content(home, &SearchContentOptions::default()) {
                searched.source = Some("searched".to_string());
                return Some(searched);
            }
        }
    }
    None
}

/// Finds a direct child entry of `dir` matching `name` case-insensitively.
///
/// Performs an exact probe first (`dir.join(name)`), then scans directory entries
/// comparing lowercase names so Linux/macOS case-sensitive filesystems still resolve
/// retail `GameData/Data/_LVL_PC/Common/ENG/common.lvl` paths regardless of casing.
///
/// Mirrors `childIgnoreCase` in the desktop host.
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

/// Walks a sequence of path components below `root` case-insensitively.
///
/// Mirrors `walkIgnoreCase` in the desktop host.
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

/// Validated Star Wars Battlefront (2004) installation directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedInstall {
    /// Resolved path to the installation root (containing `GameData`).
    pub dir: PathBuf,
}

/// Inspects `dir` to verify that it contains a genuine Star Wars Battlefront (2004) install.
///
/// Checks that `<dir>/GameData/Data/_LVL_PC` has both a `SIDE` directory and a
/// `Common` directory containing at least one language subfolder with `common.lvl`
/// (e.g. `Common/ENG/common.lvl`), distinguishing SWBF1 from SWBF2.
///
/// Mirrors `inspectInstall` in the desktop host.
pub fn inspect_install(dir: &Path) -> Option<ValidatedInstall> {
    let lvl = walk_ignore_case(dir, &["GameData", "Data", "_LVL_PC"])?;
    let side = child_ignore_case(&lvl, "SIDE")?;
    let common = child_ignore_case(&lvl, "Common")?;
    if !side.exists() || !common.exists() {
        return None;
    }
    // Classic SWBF1 places `common.lvl` inside a language subfolder (`Common/ENG/common.lvl`),
    // whereas SWBF2 places `common.lvl` directly under `Common/common.lvl`.
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
    Some(ValidatedInstall {
        dir: resolve_from(&cwd, dir),
    })
}

/// Lists sorted lowercase add-on directory IDs installed under `<dir>/GameData/AddOn`.
///
/// Only includes subdirectories that contain both `addme.script` and `Data/_LVL_PC`.
/// Mirrors `installedAddOns` in the desktop host.
pub fn installed_add_ons(dir: &Path) -> Vec<String> {
    let Some(addon_root) = walk_ignore_case(dir, &["GameData", "AddOn"]) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&addon_root) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let folder = addon_root.join(&name);
        // Official Jabba's Palace (`tat3`) and community add-ons require both
        // `addme.script` and `Data/_LVL_PC` inside their `AddOn/<id>` folder.
        if child_ignore_case(&folder, "addme.script").is_some()
            && walk_ignore_case(&folder, &["Data", "_LVL_PC"]).is_some()
        {
            ids.push(name.to_lowercase());
        }
    }
    ids.sort();
    ids
}

/// Extracts quoted key-value pairs matching `"<key>" "<value>"` (case-insensitive key)
/// from a Valve KeyValues (`libraryfolders.vdf` or `appmanifest_*.acf`) text buffer,
/// unescaping `\\` to `\`.
fn parse_vdf_key_values(text: &str, wanted_key_lower: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    // Scan quoted tokens across the file and match `"<wanted_key>" whitespace "<value>"`.
    while i < bytes.len() {
        if bytes[i] != b'"' {
            i += 1;
            continue;
        }
        let (key, next_idx) = read_vdf_quoted(text, i);
        i = next_idx;
        if key.to_lowercase() != wanted_key_lower {
            continue;
        }
        // Skip horizontal/vertical whitespace between key and value.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'"' {
            let (val, after_val) = read_vdf_quoted(text, i);
            i = after_val;
            out.push(val.replace("\\\\", "\\"));
        }
    }
    out
}

/// Reads a single double-quoted VDF token starting at `start_quote`, preserving
/// escaped sequences so the caller can unescape `\\`.
fn read_vdf_quoted(text: &str, start_quote: usize) -> (String, usize) {
    let bytes = text.as_bytes();
    let mut i = start_quote + 1;
    let mut buf = String::new();
    while i < bytes.len() {
        let ch = text[i..].chars().next().unwrap();
        let len = ch.len_utf8();
        if ch == '\\' && i + len < bytes.len() {
            let next_ch = text[i + len..].chars().next().unwrap();
            buf.push(ch);
            buf.push(next_ch);
            i += len + next_ch.len_utf8();
            continue;
        }
        if ch == '"' {
            return (buf, i + len);
        }
        buf.push(ch);
        i += len;
    }
    (buf, i)
}

/// Parses Steam library root paths from `libraryfolders.vdf` text.
///
/// Mirrors `parseLibraryFolders` in the desktop host.
pub fn parse_library_folders(text: &str) -> Vec<String> {
    parse_vdf_key_values(text, "path")
}

/// Parses the `installdir` value from a Steam `appmanifest_*.acf` text.
///
/// Mirrors `parseInstallDir` in the desktop host.
pub fn parse_install_dir(text: &str) -> Option<String> {
    parse_vdf_key_values(text, "installdir").into_iter().next()
}

/// Converts a Windows drive path (`C:\GOG Games\...`) inside a Wine prefix (`prefix`)
/// into a host filesystem path (`<prefix>/drive_c/GOG Games/...`).
///
/// Mirrors `winePath` in the desktop host.
pub fn wine_path(prefix: Option<&Path>, win_path: &str) -> Option<PathBuf> {
    let pfx = prefix?;
    let bytes = win_path.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return None;
    }
    if bytes[2] != b'\\' && bytes[2] != b'/' {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let rest: Vec<&str> = win_path[3..]
        .split(['\\', '/'])
        .filter(|s| !s.is_empty())
        .collect();
    let mut out = pfx.join(format!("drive_{}", drive));
    for part in rest {
        out.push(part);
    }
    Some(out)
}

/// Parses GOG Galaxy / Inno Setup installation paths from a Wine `system.reg` file
/// or `reg.exe query` output.
///
/// Mirrors `parseGogRegistry` in the desktop host.
pub fn parse_gog_registry(text: &str, prefix: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        // Wine system.reg format: `"path"="C:\\GOG Games\\Star Wars - Battlefront"`
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
                            let resolved =
                                wine_path(prefix, &value).unwrap_or_else(|| PathBuf::from(&value));
                            dirs.push(resolved);
                        }
                    }
                }
            }
        }
        // Windows reg.exe output format: `path    REG_SZ    C:\GOG Games\...`
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
                        let resolved =
                            wine_path(prefix, value).unwrap_or_else(|| PathBuf::from(value));
                        dirs.push(resolved);
                    }
                }
            }
        }
    }
    dirs
}

/// Parses Heroic Games Launcher's `installed.json` for GOG installs whose title
/// or path mentions `"battlefront"`.
///
/// Mirrors `parseHeroicInstalled` in the desktop host.
pub fn parse_heroic_installed(text: &str) -> Vec<PathBuf> {
    let Ok(data) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    // Heroic stores installed entries either as a top-level array, an `{ installed: [...] }`
    // object, or a map keyed by appName.
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
        let combined = format!("{} {}", title, install_path).to_lowercase();
        if combined.contains("battlefront") {
            out.push(PathBuf::from(install_path));
        }
    }
    out
}

/// Candidate default Steam root directories for the given platform, home folder, and environment.
///
/// Mirrors `steamRoots` in the desktop host.
pub fn steam_roots(
    platform: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> Vec<PathBuf> {
    if platform == "win32" || platform == "windows" {
        let pf86 = env
            .get("PROGRAMFILES(X86)")
            .cloned()
            .unwrap_or_else(|| "C:\\Program Files (x86)".to_string());
        let pf = env
            .get("PROGRAMFILES")
            .cloned()
            .unwrap_or_else(|| "C:\\Program Files".to_string());
        return vec![
            PathBuf::from(pf86).join("Steam"),
            PathBuf::from(pf).join("Steam"),
        ];
    }
    let Some(h) = home else {
        return Vec::new();
    };
    if platform == "darwin" || platform == "macos" {
        return vec![h.join("Library/Application Support/Steam")];
    }
    // Linux native, symlinked, Flatpak, and Snap Steam root locations.
    vec![
        h.join(".local/share/Steam"),
        h.join(".steam/steam"),
        h.join(".steam/root"),
        h.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        h.join("snap/steam/common/.local/share/Steam"),
    ]
}

/// Enumerates all Steam library folders referenced by `steam_roots` and their
/// `steamapps/libraryfolders.vdf` manifests.
///
/// Mirrors `steamLibraries` in the desktop host.
pub fn steam_libraries(
    platform: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
    extra_roots: &[PathBuf],
) -> Vec<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut roots = steam_roots(platform, home, env);
    roots.extend(extra_roots.iter().cloned());

    let mut seen = BTreeSet::new();
    let mut libraries = Vec::new();
    // Deduplicate library paths after resolving symlinks so `.steam/steam` and
    // `.local/share/Steam` do not scan the same library twice.
    let mut add = |dir: &Path| {
        if dir.as_os_str().is_empty() {
            return;
        }
        let resolved = resolve_from(&cwd, dir);
        let key = normalize_comparable_path(&resolved);
        if seen.insert(key) {
            libraries.push(resolved);
        }
    };

    for root in roots {
        add(&root);
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(raw) = fs::read_to_string(&vdf) {
            for lib in parse_library_folders(&raw) {
                add(Path::new(&lib));
            }
        }
    }
    libraries
}

/// Checks Windows drive roots (`C:\..Z:\`) for common unindexed Steam library folders.
///
/// Mirrors `driveLibraries` in the desktop host.
pub fn drive_libraries(platform: &str) -> Vec<PathBuf> {
    if platform != "win32" && platform != "windows" {
        return Vec::new();
    }
    let mut libraries = Vec::new();
    for code in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", code as char));
        if !root.exists() {
            continue;
        }
        for rel in DRIVE_LIBRARIES {
            let candidate = root.join(rel);
            if candidate.join("steamapps").exists() {
                libraries.push(candidate);
            }
        }
    }
    libraries
}

/// Checks a single Steam library root for an installed `appmanifest_1058020.acf`
/// and validates its `steamapps/common/<installdir>` target.
///
/// Mirrors `manifestInstall` in the desktop host.
fn manifest_install(library: &Path) -> Option<ValidatedInstall> {
    let manifest = library
        .join("steamapps")
        .join(format!("appmanifest_{}.acf", BF1_STEAM_APP));
    let raw = fs::read_to_string(manifest).ok()?;
    let install_dir = parse_install_dir(&raw)?;
    let candidate = library.join("steamapps").join("common").join(install_dir);
    inspect_install(&candidate)
}

/// Returns candidate `steamapps/common/<folder>` directories inside `library`
/// whose name contains `"battlefront"` (with `"Star Wars Battlefront (Classic, 2004)"` first).
///
/// Mirrors `commonFolders` in the desktop host.
fn common_folders(library: &Path) -> Vec<PathBuf> {
    let common = library.join("steamapps").join("common");
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

/// Discovers Wine and CrossOver prefix directories under `home` that contain `drive_c`.
///
/// Checks `~/.wine`, every subdirectory of `~/Games/*`, and every CrossOver bottle
/// under `~/Library/Application Support/CrossOver/Bottles/*`, keeping only directories
/// that contain a `drive_c` subdirectory.
///
/// Mirrors `winePrefixes` in the desktop host.
fn wine_prefixes(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(h) = home else {
        return Vec::new();
    };
    let mut prefixes = vec![h.join(".wine")];
    if let Ok(entries) = fs::read_dir(h.join("Games")) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                prefixes.push(h.join("Games").join(entry.file_name()));
            }
        }
    }
    let bottles = h.join("Library/Application Support/CrossOver/Bottles");
    if let Ok(entries) = fs::read_dir(&bottles) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                prefixes.push(bottles.join(entry.file_name()));
            }
        }
    }
    prefixes
        .into_iter()
        .filter(|dir| dir.join("drive_c").exists())
        .collect()
}

/// Reads GOG install records from Heroic Launcher config files and Wine `system.reg` registries.
///
/// Mirrors `gogRecords` in the desktop host.
pub fn gog_records(
    platform: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut heroic_files = Vec::new();

    // Probe native and Flatpak Heroic GOG store `installed.json` manifests per platform.
    if platform == "win32" || platform == "windows" {
        if let Some(appdata) = env.get("APPDATA").filter(|s| !s.is_empty()) {
            heroic_files.push(PathBuf::from(appdata).join("heroic/gog_store/installed.json"));
        }
    } else if platform == "darwin" || platform == "macos" {
        if let Some(h) = home {
            heroic_files
                .push(h.join("Library/Application Support/heroic/gog_store/installed.json"));
        }
    } else if let Some(h) = home {
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

    for file in heroic_files {
        if let Ok(raw) = fs::read_to_string(&file) {
            dirs.extend(parse_heroic_installed(&raw));
        }
    }

    // Also inspect Wine/CrossOver `system.reg` files for GOG Inno Setup registry keys.
    for prefix in wine_prefixes(home) {
        if let Ok(raw) = fs::read_to_string(prefix.join("system.reg")) {
            dirs.extend(parse_gog_registry(&raw, Some(&prefix)));
        }
    }

    dirs
}

/// Parent folders where GOG Galaxy, offline GOG installers, or Heroic place games.
///
/// Mirrors `gogFolders` in the desktop host.
pub fn gog_folders(platform: &str, home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    if platform == "win32" || platform == "windows" {
        for code in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", code as char));
            if !root.exists() {
                continue;
            }
            for rel in DRIVE_GOG_FOLDERS {
                let candidate = root.join(rel);
                if candidate.exists() {
                    folders.push(candidate);
                }
            }
        }
    }
    if let Some(h) = home {
        for rel in ["GOG Games", "Games/GOG Games", "Games/Heroic", "Games"] {
            let candidate = h.join(rel);
            if candidate.exists() {
                folders.push(candidate);
            }
        }
        for prefix in wine_prefixes(Some(h)) {
            for rel in PREFIX_GOG_FOLDERS {
                let candidate = prefix.join(rel);
                if candidate.exists() {
                    folders.push(candidate);
                }
            }
        }
    }
    folders
}

/// Discovered game installation with provenance metadata (`source`, `how`, `chosen`).
///
/// Mirrors the install records returned by `findGameInstalls` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInstall {
    /// Resolved path to the installation root.
    pub dir: PathBuf,
    /// Provenance category: `"chosen"`, `"steam"`, `"gog"`, or `"folder"`.
    pub source: String,
    /// Human-readable description of where the install was found.
    pub how: String,
    /// True if this folder was also explicitly passed in `extra` (`"chosen"`).
    #[serde(default)]
    pub chosen: bool,
}

/// Options for [`find_steam_install`], [`find_gog_install`], [`find_game_installs`],
/// and [`find_game_install`].
#[derive(Debug, Clone, Default)]
pub struct FindGameOptions {
    /// Target platform (`"linux"`, `"darwin"`, `"win32"`).
    pub platform: String,
    /// User's home directory.
    pub home: Option<PathBuf>,
    /// Environment variables (`PROGRAMFILES(X86)`, `APPDATA`, etc.).
    pub env: BTreeMap<String, String>,
    /// Additional user-chosen candidate directories (or extra Steam roots in `find_steam_install`).
    pub extra: Vec<PathBuf>,
}

/// Finds a Steam installation of Star Wars Battlefront (2004).
///
/// Mirrors `findSteamInstall` in the desktop host.
pub fn find_steam_install(opts: &FindGameOptions) -> Option<GameInstall> {
    let listed = steam_libraries(&opts.platform, opts.home.as_deref(), &opts.env, &opts.extra);
    for library in &listed {
        if let Some(found) = manifest_install(library) {
            return Some(GameInstall {
                dir: found.dir,
                source: "steam".to_string(),
                how: "your Steam library".to_string(),
                chosen: false,
            });
        }
    }
    let drives = drive_libraries(&opts.platform);
    for library in &drives {
        if let Some(found) = manifest_install(library) {
            return Some(GameInstall {
                dir: found.dir,
                source: "steam".to_string(),
                how: "your Steam library".to_string(),
                chosen: false,
            });
        }
    }
    for library in listed.iter().chain(drives.iter()) {
        for candidate in common_folders(library) {
            if let Some(found) = inspect_install(&candidate) {
                return Some(GameInstall {
                    dir: found.dir,
                    source: "folder".to_string(),
                    how: format!("a copy in {} that Steam didn't install", library.display()),
                    chosen: false,
                });
            }
        }
    }
    None
}

/// Finds a GOG / Heroic / Wine installation of Star Wars Battlefront (2004).
///
/// Mirrors `findGogInstall` in the desktop host.
pub fn find_gog_install(opts: &FindGameOptions) -> Option<GameInstall> {
    for candidate in gog_records(&opts.platform, opts.home.as_deref(), &opts.env) {
        if let Some(found) = inspect_install(&candidate) {
            return Some(GameInstall {
                dir: found.dir,
                source: "gog".to_string(),
                how: "your GOG library".to_string(),
                chosen: false,
            });
        }
    }
    for parent in gog_folders(&opts.platform, opts.home.as_deref()) {
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
            if let Some(found) = inspect_install(&parent.join(&name)) {
                return Some(GameInstall {
                    dir: found.dir,
                    source: "folder".to_string(),
                    how: format!("a copy in {}", parent.display()),
                    chosen: false,
                });
            }
        }
    }
    None
}

/// Discovers all valid Star Wars Battlefront (2004) installations on the system.
///
/// Preserves the exact discovery order and upgrade semantics of `findGameInstalls`
/// in the desktop host:
/// 1. User-chosen `extra` directories (`source: "chosen"`).
/// 2. Steam manifest installs in listed libraries (`source: "steam"`).
/// 3. Steam manifest installs in drive libraries (`source: "steam"`).
/// 4. GOG registry / Heroic records (`source: "gog"`).
/// 5. Unindexed `steamapps/common` folders (`source: "folder"`).
/// 6. GOG parent folder subdirectories (`source: "folder"`).
///
/// If a folder initially added as `"chosen"` is later discovered as `"steam"` or
/// `"gog"`, it is upgraded in place (`source: "steam" | "gog"`, `chosen: true`)
/// while keeping its leading position in the returned list.
pub fn find_game_installs(opts: &FindGameOptions) -> Vec<GameInstall> {
    let mut installs: Vec<GameInstall> = Vec::new();
    let mut by_key: BTreeMap<String, usize> = BTreeMap::new();

    // Normalize paths per-platform (case-folded on Windows and macOS, exact on Linux).
    let key_for = |dir: &Path| -> String {
        let norm = normalize_comparable_path(dir);
        if opts.platform == "win32"
            || opts.platform == "windows"
            || opts.platform == "darwin"
            || opts.platform == "macos"
        {
            norm.to_lowercase()
        } else {
            norm
        }
    };

    // Insert a validated install or upgrade an existing `"chosen"` record when
    // the same directory is also recognized as an official Steam or GOG install.
    let mut add = |candidate: &Path, source: &str, how: String| {
        let Some(found) = inspect_install(candidate) else {
            return;
        };
        let key = key_for(&found.dir);
        if let Some(&idx) = by_key.get(&key) {
            if installs[idx].source == "chosen" && (source == "steam" || source == "gog") {
                installs[idx] = GameInstall {
                    dir: found.dir,
                    source: source.to_string(),
                    how,
                    chosen: true,
                };
            }
            return;
        }
        let index = installs.len();
        by_key.insert(key, index);
        installs.push(GameInstall {
            dir: found.dir,
            source: source.to_string(),
            how,
            chosen: source == "chosen",
        });
    };

    // Step 1: explicitly saved or chosen game folders.
    for dir in &opts.extra {
        add(dir, "chosen", "the folder you chose".to_string());
    }

    let listed = steam_libraries(&opts.platform, opts.home.as_deref(), &opts.env, &[]);
    let drives = drive_libraries(&opts.platform);

    // Step 2: Steam libraries listed in `libraryfolders.vdf` with `appmanifest_1058020.acf`.
    for library in &listed {
        let manifest = library
            .join("steamapps")
            .join(format!("appmanifest_{}.acf", BF1_STEAM_APP));
        if let Ok(raw) = fs::read_to_string(&manifest) {
            if let Some(install_dir) = parse_install_dir(&raw) {
                add(
                    &library.join("steamapps").join("common").join(install_dir),
                    "steam",
                    "your Steam library".to_string(),
                );
            }
        }
    }

    // Step 3: Windows drive Steam libraries with `appmanifest_1058020.acf`.
    for library in &drives {
        let manifest = library
            .join("steamapps")
            .join(format!("appmanifest_{}.acf", BF1_STEAM_APP));
        if let Ok(raw) = fs::read_to_string(&manifest) {
            if let Some(install_dir) = parse_install_dir(&raw) {
                add(
                    &library.join("steamapps").join("common").join(install_dir),
                    "steam",
                    "your Steam library".to_string(),
                );
            }
        }
    }

    // Step 4: Heroic `installed.json` and Wine registry GOG records.
    for candidate in gog_records(&opts.platform, opts.home.as_deref(), &opts.env) {
        add(&candidate, "gog", "your GOG library".to_string());
    }

    // Step 5: unindexed copies remaining in `steamapps/common` without an active manifest.
    for library in listed.iter().chain(drives.iter()) {
        for candidate in common_folders(library) {
            add(
                &candidate,
                "folder",
                format!("a copy in {} that Steam didn't install", library.display()),
            );
        }
    }

    // Step 6: standalone copies inside GOG parent directories (`GOG Games/*`, `Games/Heroic/*`).
    for parent in gog_folders(&opts.platform, opts.home.as_deref()) {
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
            add(
                &parent.join(name),
                "folder",
                format!("a copy in {}", parent.display()),
            );
        }
    }

    installs
}

/// Returns the highest-priority game installation discovered by [`find_game_installs`].
///
/// Returns `None` when no valid Star Wars Battlefront (2004) installation is found
/// in any user-chosen folder, Steam library, GOG registry/Heroic manifest, or
/// standard parent folder.
///
/// Mirrors `findGameInstall` in the desktop host.
pub fn find_game_install(opts: &FindGameOptions) -> Option<GameInstall> {
    find_game_installs(opts).into_iter().next()
}
