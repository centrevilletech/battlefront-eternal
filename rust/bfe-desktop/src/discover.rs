//! Game installation and prepared content discovery across Steam, GOG, Heroic,
//! Wine, CrossOver, and local filesystem directories.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bfe_content::layout::{family_dir, is_swbf2_marked_dir};
use bfe_family::GameFamily;
use serde::{Deserialize, Serialize};

use crate::paths::{normalize_comparable_path, resolve_from};

/// Steam App ID for Star Wars Battlefront (Classic, 2004).
///
/// Mirrors `BF1_STEAM_APP` in the desktop host.
pub const BF1_STEAM_APP: &str = "1058020";

/// Steam App ID for Star Wars Battlefront II (Classic, 2005).
pub const BF2_STEAM_APP: &str = "6060";

/// Store URL for the GOG edition of Star Wars Battlefront (Classic, 2004).
///
/// Mirrors `BF1_GOG_PAGE` in the desktop host.
pub const BF1_GOG_PAGE: &str = "https://www.gog.com/game/star_wars_battlefront";

/// Store URL for the GOG edition of Star Wars Battlefront II (Classic, 2005).
pub const BF2_GOG_PAGE: &str = "https://www.gog.com/game/star_wars_battlefront_ii";

/// Store identifiers and expected install folder names for one game family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameProfile {
    /// Game family (`swbf1` or `swbf2`).
    pub family: GameFamily,
    /// Human-readable game name.
    pub display_name: &'static str,
    /// Steam App ID string.
    pub steam_app: &'static str,
    /// GOG store page URL.
    pub gog_page: &'static str,
    /// Default folder names under `steamapps/common`.
    pub steam_folders: &'static [&'static str],
    /// Default folder names under GOG / Heroic install roots.
    pub gog_folders: &'static [&'static str],
    /// Executable filename inside `GameData/`.
    pub exe_name: &'static str,
}

const SWBF1_PROFILE: GameProfile = GameProfile {
    family: GameFamily::Swbf1,
    display_name: "Star Wars Battlefront (2004)",
    steam_app: BF1_STEAM_APP,
    gog_page: BF1_GOG_PAGE,
    steam_folders: &["Star Wars Battlefront (Classic, 2004)"],
    gog_folders: &["Star Wars - Battlefront", "STAR WARS Battlefront"],
    exe_name: "Battlefront.exe",
};

const SWBF2_PROFILE: GameProfile = GameProfile {
    family: GameFamily::Swbf2,
    display_name: "Star Wars Battlefront II (2005)",
    steam_app: BF2_STEAM_APP,
    gog_page: BF2_GOG_PAGE,
    steam_folders: &[
        "Star Wars Battlefront II",
        "Star Wars Battlefront II (Classic, 2005)",
    ],
    gog_folders: &[
        "Star Wars - Battlefront II",
        "STAR WARS Battlefront II",
        "Star Wars Battlefront II",
    ],
    exe_name: "BattlefrontII.exe",
};

/// Returns the store and folder profile for `family`.
pub fn game_profile(family: GameFamily) -> GameProfile {
    match family {
        GameFamily::Swbf1 => SWBF1_PROFILE,
        GameFamily::Swbf2 => SWBF2_PROFILE,
    }
}

/// Returns true when `haystack` contains `needle` not immediately followed by
/// an ASCII alphanumeric character that changes the identifier (e.g. so
/// `"battlefront2004"` does not match `"battlefront2"`).
fn contains_version_tag(haystack: &str, needle: &str, reject_following_digit: bool) -> bool {
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        let end = start + pos + needle.len();
        let next = bytes.get(end).copied();
        let ok = match next {
            None => true,
            Some(b) if reject_following_digit => !b.is_ascii_digit(),
            Some(b) => !b.is_ascii_alphabetic(),
        };
        if ok {
            return true;
        }
        start += pos + 1;
    }
    false
}

/// Returns true when `text` (a folder name, registry path, or launcher title)
/// refers to Star Wars Battlefront II (2005).
fn name_is_swbf2(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if contains_version_tag(&lower, "battlefrontii", false)
        || contains_version_tag(&lower, "battlefront2", true)
        || contains_version_tag(&lower, "battlefront2005", true)
        || contains_version_tag(&lower, "swbf2", true)
    {
        return true;
    }
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let has_bf = tokens.contains(&"battlefront");
    if !has_bf {
        return false;
    }
    if tokens.contains(&"2005") {
        return true;
    }
    tokens
        .windows(2)
        .any(|w| w[0] == "battlefront" && (w[1] == "ii" || w[1] == "2"))
}

/// Returns true when `text` matches `family`'s title or folder name without
/// matching the other Battlefront game.
pub fn matches_family_name(text: &str, family: GameFamily) -> bool {
    let lower = text.to_ascii_lowercase();
    let is_bf2 = name_is_swbf2(&lower);
    match family {
        GameFamily::Swbf2 => is_bf2,
        GameFamily::Swbf1 => (lower.contains("battlefront") || lower.contains("swbf1")) && !is_bf2,
    }
}

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

/// Where the Windows Steam client installs itself inside a Wine prefix.
pub const PREFIX_STEAM_ROOTS: &[&str] = &[
    "drive_c/Program Files (x86)/Steam",
    "drive_c/Program Files/Steam",
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

/// Inspects `dir` to see if it is a valid prepared 2004 (`swbf1`) content folder.
///
/// Mirrors `inspectContent` in the desktop host while rejecting directories
/// carrying a positive `.bfe-family.json` marker for `swbf2`.
pub fn inspect_content(dir: &Path) -> Option<ContentInfo> {
    if dir.as_os_str().is_empty() || is_swbf2_marked_dir(dir) {
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

/// Inspects `dir` to see if it holds valid prepared content for `family`.
///
/// Checks `<dir>/common/common.json` for `swbf1` via [`inspect_content`], or
/// `<dir>/swbf2/.content/common/common.json` for `swbf2`, returning
/// `ContentInfo` whose `dir` is the shared content root.
pub fn inspect_content_for(dir: &Path, family: GameFamily) -> Option<ContentInfo> {
    if family == GameFamily::Swbf1 {
        return inspect_content(dir);
    }
    if dir.as_os_str().is_empty() {
        return None;
    }
    let family_root = family_dir(dir, family);
    let common = family_root.join("common").join("common.json");
    if !common.exists() {
        return None;
    }
    let mut maps = Vec::new();
    if let Ok(entries) = fs::read_dir(family_root.join("maps")) {
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

/// Breadth-first searches `root` for the richest prepared 2004 (`swbf1`) content directory.
///
/// Mirrors `searchContent` in the desktop host. Dot-prefixed directories (such as
/// `swbf2/.content`) are skipped, and any directory carrying a `.bfe-family.json`
/// marker for `swbf2` is rejected by [`inspect_content`].
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

/// Breadth-first searches `root` for the richest prepared content directory for `family`.
pub fn search_content_for(
    root: &Path,
    family: GameFamily,
    opts: &SearchContentOptions,
) -> Option<ContentInfo> {
    if family == GameFamily::Swbf1 {
        return search_content(root, opts);
    }
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

    while let Some((dir, level)) = queue.pop_front() {
        if visited >= opts.max_dirs || Instant::now() > deadline {
            break;
        }
        visited += 1;

        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        let mut child_dirs = Vec::new();
        let mut has_family_dir = false;

        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                continue;
            };
            if name == family.as_str() {
                has_family_dir = true;
            }
            child_dirs.push(name);
        }

        if has_family_dir {
            if let Some(found) = inspect_content_for(&dir, family) {
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

/// Locates prepared content for `family` from explicit choices, known likely locations,
/// or a home search.
pub fn find_content_for(opts: &FindContentOptions, family: GameFamily) -> Option<ContentInfo> {
    // 1. User-chosen or CLI/env-forced directories take top priority.
    for dir in &opts.explicit {
        if let Some(mut content) = inspect_content_for(dir, family) {
            content.source = Some("chosen".to_string());
            return Some(content);
        }
    }
    // 2. Compare all known likely directories and pick the one with the most maps.
    let mut known: Option<ContentInfo> = None;
    for dir in &opts.likely {
        if let Some(content) = inspect_content_for(dir, family) {
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
            if let Some(mut searched) =
                search_content_for(home, family, &SearchContentOptions::default())
            {
                searched.source = Some("searched".to_string());
                return Some(searched);
            }
        }
    }
    None
}

/// Locates prepared 2004 (`swbf1`) content from explicit choices, known likely locations,
/// or a home search.
///
/// Tags the returned [`ContentInfo::source`] as `"chosen"`, `"found"`, or `"searched"`.
/// Mirrors `findContent` in the desktop host.
pub fn find_content(opts: &FindContentOptions) -> Option<ContentInfo> {
    find_content_for(opts, GameFamily::Swbf1)
}

/// Finds a direct child entry of `dir` matching `name` case-insensitively.
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

/// Walks all case-insensitive path matches under `root` for `parts`, putting exact-case
/// children before case-folded siblings at each level.
fn walk_ignore_case_all(root: &Path, parts: &[&str]) -> Vec<PathBuf> {
    if root.as_os_str().is_empty() {
        return Vec::new();
    }
    let mut current = vec![root.to_path_buf()];
    for part in parts {
        let want = part.to_lowercase();
        let mut next = Vec::new();
        let mut seen = BTreeSet::new();
        for dir in current {
            let direct = dir.join(part);
            if direct.exists() && seen.insert(direct.clone()) {
                next.push(direct);
            }
            if let Ok(entries) = fs::read_dir(&dir) {
                let mut matched: Vec<PathBuf> = entries
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().into_string().ok()?;
                        (name.to_lowercase() == want).then(|| dir.join(name))
                    })
                    .collect();
                matched.sort();
                for path in matched {
                    if seen.insert(path.clone()) {
                        next.push(path);
                    }
                }
            }
        }
        current = next;
        if current.is_empty() {
            break;
        }
    }
    current
}

/// Finds the `_lvl_pc` directory inside a 2005 (`swbf2`) installation, checking
/// canonical `GameData/data/_lvl_pc` first so case-sensitive split trees that
/// also hold 2004 `GameData/Data/_LVL_PC` resolve the 2005 folder cleanly.
pub fn find_swbf2_lvl_pc(dir: &Path) -> Option<PathBuf> {
    let candidates = walk_ignore_case_all(dir, &["GameData", "data", "_lvl_pc"]);
    for lvl in &candidates {
        if lvl.is_dir()
            && child_ignore_case(lvl, "side").is_some_and(|p| p.is_dir())
            && has_swbf2_root_lvls(lvl)
        {
            return Some(lvl.clone());
        }
    }
    candidates.into_iter().find(|p| p.is_dir())
}

/// Validated game installation directory and its identified family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedInstall {
    /// Resolved path to the installation root (containing `GameData`).
    pub dir: PathBuf,
    /// Identified game family (`swbf1` or `swbf2`).
    #[serde(default)]
    pub family: GameFamily,
}

/// Returns true when `_lvl_pc/` directly contains the 2005 root level archives
/// (`common.lvl`, `core.lvl`, `mission.lvl`, `ingame.lvl`, `shell.lvl`).
fn has_swbf2_root_lvls(lvl: &Path) -> bool {
    ["common.lvl", "core.lvl", "mission.lvl", "ingame.lvl", "shell.lvl"]
        .iter()
        .all(|name| child_ignore_case(lvl, name).is_some_and(|p| p.is_file()))
}

/// Inspects `dir` to verify that it contains a genuine install for `family`.
///
/// Each family checks its own positive markers:
/// - `swbf1` uses [`inspect_install`].
/// - `swbf2` requires `GameData/BattlefrontII.exe`, `GameData/data/_lvl_pc/side`,
///   and root `common.lvl`, `core.lvl`, `mission.lvl`, `ingame.lvl`, `shell.lvl`.
///
/// If a folder holds complete markers for both games (even across distinct
/// `Data/_LVL_PC` and `data/_lvl_pc` trees on case-sensitive disks), each
/// validator accepts it for its own game.
pub fn inspect_install_for(dir: &Path, family: GameFamily) -> Option<ValidatedInstall> {
    if family == GameFamily::Swbf1 {
        return inspect_install(dir);
    }
    let has_bf2_exe = walk_ignore_case(dir, &["GameData", "BattlefrontII.exe"])
        .is_some_and(|p| p.is_file());
    if !has_bf2_exe {
        return None;
    }
    let lvl = find_swbf2_lvl_pc(dir)?;
    if !lvl.is_dir()
        || !child_ignore_case(&lvl, "side").is_some_and(|p| p.is_dir())
        || !has_swbf2_root_lvls(&lvl)
    {
        return None;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    Some(ValidatedInstall {
        dir: resolve_from(&cwd, dir),
        family: GameFamily::Swbf2,
    })
}

/// Inspects `dir` against both supported game families and returns the matching
/// [`ValidatedInstall`], if any.
pub fn inspect_any_install(dir: &Path) -> Option<ValidatedInstall> {
    inspect_install(dir).or_else(|| inspect_install_for(dir, GameFamily::Swbf2))
}

/// Inspects `dir` to verify that it contains a genuine Star Wars Battlefront (2004) install:
/// `GameData/Data/_LVL_PC` (any case) with a `SIDE` and a `Common` entry and a `common.lvl`.
pub fn inspect_install(dir: &Path) -> Option<ValidatedInstall> {
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
    Some(ValidatedInstall {
        dir: resolve_from(&cwd, dir),
        family: GameFamily::Swbf1,
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

/// Matches a launcher/registry record `(title, full_path)` against `family`.
///
/// When `title` explicitly names either Battlefront game, that classification
/// takes precedence (preventing an unrelated parent folder name from overriding
/// an explicit title). Otherwise, matches against the full `full_path` so
/// installs whose `"battlefront"` marker sits in a parent folder are preserved.
fn record_matches_family(title: &str, full_path: &str, family: GameFamily) -> bool {
    let title_is_bf1 = matches_family_name(title, GameFamily::Swbf1);
    let title_is_bf2 = matches_family_name(title, GameFamily::Swbf2);
    if title_is_bf1 || title_is_bf2 {
        return match family {
            GameFamily::Swbf1 => title_is_bf1,
            GameFamily::Swbf2 => title_is_bf2,
        };
    }
    matches_family_name(full_path, family)
}

/// Parses GOG Galaxy / Inno Setup installation paths for 2004 (`swbf1`) from a
/// Wine `system.reg` file or `reg.exe query` output.
///
/// Mirrors `parseGogRegistry` in the desktop host without name vetoes.
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

/// Parses GOG Galaxy / Inno Setup installation paths for `family` from a Wine
/// `system.reg` file or `reg.exe query` output.
///
/// For `GameFamily::Swbf1`, runs [`parse_gog_registry`] unchanged.
pub fn parse_gog_registry_for(
    text: &str,
    prefix: Option<&Path>,
    family: GameFamily,
) -> Vec<PathBuf> {
    if family == GameFamily::Swbf1 {
        return parse_gog_registry(text, prefix);
    }
    let mut dirs = Vec::new();
    let mut section_game_name = String::new();
    let mut section_paths: Vec<String> = Vec::new();

    let flush = |game_name: &mut String, paths: &mut Vec<String>, out: &mut Vec<PathBuf>| {
        for raw in paths.drain(..) {
            if record_matches_family(game_name, &raw, family) {
                let resolved = wine_path(prefix, &raw).unwrap_or_else(|| PathBuf::from(&raw));
                out.push(resolved);
            }
        }
        game_name.clear();
    };

    for line in text.lines() {
        let trimmed = line.trim();
        // Wine `system.reg` section headers carry a trailing Unix timestamp (`[Key] 1234567890`).
        let is_section_boundary = trimmed.is_empty()
            || (trimmed.starts_with('[') && trimmed.contains(']'))
            || trimmed
                .get(..5)
                .is_some_and(|p| p.eq_ignore_ascii_case("hkey_"));
        if is_section_boundary {
            flush(&mut section_game_name, &mut section_paths, &mut dirs);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('"') {
            if let Some(eq_idx) = rest.find("\"=\"") {
                let key = &rest[..eq_idx];
                let val_part = &rest[eq_idx + 3..];
                if let Some(end_quote) = val_part.rfind('"') {
                    let k_lower = key.to_lowercase();
                    let raw_val = &val_part[..end_quote];
                    let value = raw_val.replace("\\\\", "\\");
                    if k_lower == "gamename" || k_lower == "displayname" {
                        section_game_name = value;
                    } else if k_lower == "path" || k_lower == "installlocation" {
                        section_paths.push(value);
                    }
                }
            }
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if tokens.len() >= 3 {
            let key_lower = tokens[0].to_lowercase();
            let reg_type = tokens[1].to_uppercase();
            if reg_type == "REG_SZ" || reg_type == "REG_EXPAND_SZ" {
                if let Some(pos) = trimmed.find(tokens[1]) {
                    let value = trimmed[pos + tokens[1].len()..].trim();
                    if key_lower == "gamename" || key_lower == "displayname" {
                        section_game_name = value.to_string();
                    } else if key_lower == "path" || key_lower == "installlocation" {
                        section_paths.push(value.to_string());
                    }
                }
            }
        }
    }
    flush(&mut section_game_name, &mut section_paths, &mut dirs);
    dirs
}

/// Parses Heroic Games Launcher's `installed.json` for 2004 (`swbf1`) GOG installs.
///
/// Mirrors `parseHeroicInstalled` in the desktop host without name vetoes.
pub fn parse_heroic_installed(text: &str) -> Vec<PathBuf> {
    let Ok(data) = serde_json::from_str::<serde_json::Value>(text) else {
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

/// Parses Heroic Games Launcher's `installed.json` for GOG installs matching `family`.
///
/// For `GameFamily::Swbf1`, runs [`parse_heroic_installed`] unchanged.
pub fn parse_heroic_installed_for(text: &str, family: GameFamily) -> Vec<PathBuf> {
    if family == GameFamily::Swbf1 {
        return parse_heroic_installed(text);
    }
    let Ok(data) = serde_json::from_str::<serde_json::Value>(text) else {
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
        if record_matches_family(title, install_path, family) {
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
    // Steam's Mac client does not install this Windows game: a Mac player's Steam copy is the
    // Windows client's, inside a Wine bottle (CrossOver, Whisky, Heroic).
    if platform == "darwin" || platform == "macos" {
        for prefix in wine_prefixes(home) {
            for rel in PREFIX_STEAM_ROOTS {
                roots.push(prefix.join(rel));
            }
        }
    }
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

/// Checks a single Steam library root for `family`'s app manifest
/// and validates its `steamapps/common/<installdir>` target.
fn manifest_install(library: &Path, family: GameFamily) -> Option<ValidatedInstall> {
    let app_id = game_profile(family).steam_app;
    let manifest = library
        .join("steamapps")
        .join(format!("appmanifest_{app_id}.acf"));
    let raw = fs::read_to_string(manifest).ok()?;
    let install_dir = parse_install_dir(&raw)?;
    let candidate = library.join("steamapps").join("common").join(install_dir);
    inspect_install_for(&candidate, family)
}

/// Returns candidate `steamapps/common/<folder>` directories inside `library`
/// for `family`.
///
/// For `GameFamily::Swbf1`: `"Star Wars Battlefront (Classic, 2004)"` followed by
/// every subdirectory whose name contains `"battlefront"`, with no name vetoes.
fn common_folders(library: &Path, family: GameFamily) -> Vec<PathBuf> {
    let common = library.join("steamapps").join("common");
    if family == GameFamily::Swbf1 {
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
        return dirs;
    }
    let mut dirs: Vec<PathBuf> = game_profile(family)
        .steam_folders
        .iter()
        .map(|name| common.join(name))
        .collect();
    if let Ok(entries) = fs::read_dir(&common) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    if matches_family_name(name, family) {
                        dirs.push(common.join(name));
                    }
                }
            }
        }
    }
    dirs
}

/// Discovers Wine, CrossOver, Whisky and Heroic prefix directories under `home` that contain `drive_c`.
fn wine_prefixes(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(h) = home else {
        return Vec::new();
    };
    let mut prefixes = vec![h.join(".wine")];
    // Lutris's prefixes, CrossOver's and Whisky's bottles, and the prefix Heroic gives each game.
    for parent in [
        h.join("Games"),
        h.join("Library/Application Support/CrossOver/Bottles"),
        h.join("Library/Containers/com.isaacmarovitz.Whisky/Bottles"),
        h.join("Games/Heroic/Prefixes"),
        h.join("Games/Heroic/Prefixes/default"),
    ] {
        if let Ok(entries) = fs::read_dir(&parent) {
            for entry in entries.flatten() {
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    prefixes.push(parent.join(entry.file_name()));
                }
            }
        }
    }
    prefixes
        .into_iter()
        .filter(|dir| dir.join("drive_c").exists())
        .collect()
}

/// Reads GOG install records for `family` from Heroic Launcher config files
/// and Wine `system.reg` registries.
pub fn gog_records_for(
    platform: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
    family: GameFamily,
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
            dirs.extend(parse_heroic_installed_for(&raw, family));
        }
    }

    // Also inspect Wine/CrossOver `system.reg` files for GOG Inno Setup registry keys.
    for prefix in wine_prefixes(home) {
        if let Ok(raw) = fs::read_to_string(prefix.join("system.reg")) {
            dirs.extend(parse_gog_registry_for(&raw, Some(&prefix), family));
        }
    }

    dirs
}

/// Reads GOG install records for 2004 (`swbf1`) from Heroic Launcher config files
/// and Wine `system.reg` registries.
///
/// Mirrors `gogRecords` in the desktop host.
pub fn gog_records(
    platform: &str,
    home: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> Vec<PathBuf> {
    gog_records_for(platform, home, env, GameFamily::Swbf1)
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
    /// Game family (`swbf1` or `swbf2`).
    #[serde(default)]
    pub family: GameFamily,
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

/// Finds a Steam installation for `family`.
pub fn find_steam_install_for(opts: &FindGameOptions, family: GameFamily) -> Option<GameInstall> {
    let listed = steam_libraries(&opts.platform, opts.home.as_deref(), &opts.env, &opts.extra);
    for library in &listed {
        if let Some(found) = manifest_install(library, family) {
            return Some(GameInstall {
                dir: found.dir,
                family: found.family,
                source: "steam".to_string(),
                how: "your Steam library".to_string(),
                chosen: false,
            });
        }
    }
    let drives = drive_libraries(&opts.platform);
    for library in &drives {
        if let Some(found) = manifest_install(library, family) {
            return Some(GameInstall {
                dir: found.dir,
                family: found.family,
                source: "steam".to_string(),
                how: "your Steam library".to_string(),
                chosen: false,
            });
        }
    }
    for library in listed.iter().chain(drives.iter()) {
        for candidate in common_folders(library, family) {
            if let Some(found) = inspect_install_for(&candidate, family) {
                return Some(GameInstall {
                    dir: found.dir,
                    family: found.family,
                    source: "folder".to_string(),
                    how: format!("a copy in {} that Steam didn't install", library.display()),
                    chosen: false,
                });
            }
        }
    }
    None
}

/// Finds a Steam installation of Star Wars Battlefront (2004).
///
/// Mirrors `findSteamInstall` in the desktop host.
pub fn find_steam_install(opts: &FindGameOptions) -> Option<GameInstall> {
    find_steam_install_for(opts, GameFamily::Swbf1)
}

/// Finds a GOG / Heroic / Wine installation of Star Wars Battlefront (2004).
///
/// Mirrors `findGogInstall` in the desktop host without extra default-name
/// probes or name vetoes.
pub fn find_gog_install(opts: &FindGameOptions) -> Option<GameInstall> {
    for candidate in gog_records(&opts.platform, opts.home.as_deref(), &opts.env) {
        if let Some(found) = inspect_install(&candidate) {
            return Some(GameInstall {
                dir: found.dir,
                family: GameFamily::Swbf1,
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
                    family: GameFamily::Swbf1,
                    source: "folder".to_string(),
                    how: format!("a copy in {}", parent.display()),
                    chosen: false,
                });
            }
        }
    }
    None
}

/// Finds a GOG / Heroic / Wine installation for `family`.
///
/// For `GameFamily::Swbf1`, runs [`find_gog_install`] unchanged.
pub fn find_gog_install_for(opts: &FindGameOptions, family: GameFamily) -> Option<GameInstall> {
    if family == GameFamily::Swbf1 {
        return find_gog_install(opts);
    }
    for candidate in gog_records_for(&opts.platform, opts.home.as_deref(), &opts.env, family) {
        if let Some(found) = inspect_install_for(&candidate, family) {
            return Some(GameInstall {
                dir: found.dir,
                family: found.family,
                source: "gog".to_string(),
                how: "your GOG library".to_string(),
                chosen: false,
            });
        }
    }
    for parent in gog_folders(&opts.platform, opts.home.as_deref()) {
        for default_name in game_profile(family).gog_folders {
            if let Some(found) = inspect_install_for(&parent.join(default_name), family) {
                return Some(GameInstall {
                    dir: found.dir,
                    family: found.family,
                    source: "folder".to_string(),
                    how: format!("a copy in {}", parent.display()),
                    chosen: false,
                });
            }
        }
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
            if !matches_family_name(&name, family) {
                continue;
            }
            if let Some(found) = inspect_install_for(&parent.join(&name), family) {
                return Some(GameInstall {
                    dir: found.dir,
                    family: found.family,
                    source: "folder".to_string(),
                    how: format!("a copy in {}", parent.display()),
                    chosen: false,
                });
            }
        }
    }
    None
}

/// Discovers all valid installations for `family` on the system.
pub fn find_game_installs_for(opts: &FindGameOptions, family: GameFamily) -> Vec<GameInstall> {
    let mut installs: Vec<GameInstall> = Vec::new();
    let mut by_key: BTreeMap<String, usize> = BTreeMap::new();
    let profile = game_profile(family);

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
        let Some(found) = inspect_install_for(candidate, family) else {
            return;
        };
        let key = key_for(&found.dir);
        if let Some(&idx) = by_key.get(&key) {
            if installs[idx].source == "chosen" && (source == "steam" || source == "gog") {
                installs[idx] = GameInstall {
                    dir: found.dir,
                    family: found.family,
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
            family: found.family,
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
    let manifest_name = format!("appmanifest_{}.acf", profile.steam_app);

    // Step 2: Steam libraries listed in `libraryfolders.vdf` with this family's app manifest.
    for library in &listed {
        let manifest = library.join("steamapps").join(&manifest_name);
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

    // Step 3: Windows drive Steam libraries with this family's app manifest.
    for library in &drives {
        let manifest = library.join("steamapps").join(&manifest_name);
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
    for candidate in gog_records_for(&opts.platform, opts.home.as_deref(), &opts.env, family) {
        add(&candidate, "gog", "your GOG library".to_string());
    }

    // Step 5: unindexed copies remaining in `steamapps/common` without an active manifest.
    for library in listed.iter().chain(drives.iter()) {
        for candidate in common_folders(library, family) {
            add(
                &candidate,
                "folder",
                format!("a copy in {} that Steam didn't install", library.display()),
            );
        }
    }

    // Step 6: standalone copies inside GOG parent directories (`GOG Games/*`, `Games/Heroic/*`).
    for parent in gog_folders(&opts.platform, opts.home.as_deref()) {
        if family != GameFamily::Swbf1 {
            for default_name in profile.gog_folders {
                add(
                    &parent.join(default_name),
                    "folder",
                    format!("a copy in {}", parent.display()),
                );
            }
        }
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
            let matches = if family == GameFamily::Swbf1 {
                name.to_lowercase().contains("battlefront")
            } else {
                matches_family_name(&name, family)
            };
            if !matches {
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

/// Discovers all valid Star Wars Battlefront (2004) installations on the system.
pub fn find_game_installs(opts: &FindGameOptions) -> Vec<GameInstall> {
    find_game_installs_for(opts, GameFamily::Swbf1)
}

/// Returns the highest-priority game installation for `family` discovered by
/// [`find_game_installs_for`].
pub fn find_game_install_for(opts: &FindGameOptions, family: GameFamily) -> Option<GameInstall> {
    find_game_installs_for(opts, family).into_iter().next()
}

/// Returns the highest-priority 2004 (`swbf1`) game installation discovered by
/// [`find_game_installs`].
pub fn find_game_install(opts: &FindGameOptions) -> Option<GameInstall> {
    find_game_install_for(opts, GameFamily::Swbf1)
}
