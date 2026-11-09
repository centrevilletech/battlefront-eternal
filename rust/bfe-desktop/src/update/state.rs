//! Persistent desktop auto-update state, installed version enumeration, crash-loop
//! rollback guard, and `current` symlink updates.
//!
//! Mirrors the desktop host (`INSTALL_RECORD`, `MAX_ATTEMPTS`,
//! `SUPPORTED_PLATFORMS`, `updateRoot`, `platformKey`, `versionsDir`, `readState`,
//! `writeState`, `installedVersions`, `chooseLaunch`, `recordAttempt`,
//! `recordHealthy`, `pointCurrent`, `collect`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Deserializer, Serialize};

use crate::paths::{normalize_lexical, resolve_from, update_root_with};
use crate::update::version::{compare_versions, parse_version};

/// Filename of the completion marker record written last inside an installed
/// version directory (`<root>/versions/<version>/.bfe-install.json`).
///
/// Mirrors `INSTALL_RECORD` in the desktop host.
pub const INSTALL_RECORD: &str = ".bfe-install.json";

/// Maximum unconfirmed launch attempts before a version is set aside (`2`).
///
/// Mirrors `MAX_ATTEMPTS` in the desktop host.
pub const MAX_ATTEMPTS: u32 = 2;

/// Platforms supported by the built-in auto-updater (`linux-x64`, `linux-arm64`, `win32-x64`).
///
/// Mirrors `SUPPORTED_PLATFORMS` in the desktop host.
pub const SUPPORTED_PLATFORMS: &[&str] = &["linux-x64", "linux-arm64", "win32-x64"];

/// Maps an OS and CPU architecture identifier to one of [`SUPPORTED_PLATFORMS`].
///
/// Accepts both platform names (`"win32"`, `"x64"`, `"arm64"`) and Rust
/// `std::env::consts` names (`"windows"`, `"x86_64"`, `"aarch64"`).
///
/// Mirrors `platformKey` in the desktop host.
pub fn platform_key(platform: &str, arch: &str) -> Option<&'static str> {
    let norm_plat = match platform {
        "linux" => "linux",
        "win32" | "windows" => "win32",
        _ => return None,
    };
    let norm_arch = match arch {
        "x64" | "x86_64" => "x64",
        "arm64" | "aarch64" => "arm64",
        _ => return None,
    };
    match (norm_plat, norm_arch) {
        ("linux", "x64") => Some("linux-x64"),
        ("linux", "arm64") => Some("linux-arm64"),
        ("win32", "x64") => Some("win32-x64"),
        _ => None,
    }
}

/// Returns the [`platform_key`] for the host target where this binary was compiled.
pub fn current_platform_key() -> Option<&'static str> {
    platform_key(std::env::consts::OS, std::env::consts::ARCH)
}

/// Resolves the root directory for installed versions and `state.json`, honouring
/// `BFE_UPDATE_ROOT` first and falling back to [`update_root_with`].
///
/// Mirrors `root = env.BFE_UPDATE_ROOT || updateRoot({ platform, env })` in
/// the desktop host and `updateRoot` in the desktop host.
pub fn resolve_update_root(
    platform: &str,
    env_map: &HashMap<String, String>,
    home: &Path,
) -> Option<PathBuf> {
    if let Some(explicit) = env_map.get("BFE_UPDATE_ROOT").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(explicit));
    }
    update_root_with(platform, env_map, home)
}

/// Returns `<root>/versions`.
///
/// Mirrors `versionsDir` in the desktop host.
pub fn versions_dir(root: &Path) -> PathBuf {
    root.join("versions")
}

/// Returns `<root>/state.json`.
///
/// Mirrors `stateFile` in the desktop host.
pub fn state_file_path(root: &Path) -> PathBuf {
    root.join("state.json")
}

/// Persistent launch-attempt and version-health state stored in `<root>/state.json`.
///
/// Mirrors `defaultState` and `normalizeState` in the desktop host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Unconfirmed launch attempt counts keyed by version string.
    #[serde(default)]
    pub attempts: BTreeMap<String, u32>,
    /// Versions that exceeded [`MAX_ATTEMPTS`] without calling [`record_healthy`].
    #[serde(default)]
    pub bad: Vec<String>,
    /// Recently confirmed healthy versions, newest confirmation first (up to 10).
    #[serde(default)]
    pub healthy: Vec<String>,
}

/// Single file entry inside `<version>/.bfe-install.json`.
///
/// Mirrors the `files` array written by `installRelease` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallFileEntry {
    /// Relative POSIX path beneath the version directory.
    pub path: String,
    /// Uncompressed file size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 of the uncompressed file.
    pub sha256: String,
    /// Optional POSIX file permission mode (`0o755` for executables, `0o644` for regular files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

fn deserialize_files_or_count<'de, D>(deserializer: D) -> Result<Vec<InstallFileEntry>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FilesOrCount {
        Entries(Vec<InstallFileEntry>),
        Count(#[allow(dead_code)] u64),
    }

    match Option::<FilesOrCount>::deserialize(deserializer)? {
        Some(FilesOrCount::Entries(entries)) => Ok(entries),
        Some(FilesOrCount::Count(_)) | None => Ok(Vec::new()),
    }
}

/// Completion marker written last to `<root>/versions/<version>/.bfe-install.json`.
///
/// Mirrors `record` in `installRelease` (the desktop host) and
/// `installedVersions` (the desktop host).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRecord {
    /// Semantic version string matching the parent directory name.
    pub version: String,
    /// Release identifier (`"preview-332"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    /// Source git commit SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Platform key (`"linux-x64"`, `"linux-arm64"`, `"win32-x64"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    /// Relative path to the main executable inside the version directory.
    pub executable: String,
    /// Manifest file list (or integer file count when written by the desktop host's `installRelease`).
    #[serde(default, deserialize_with = "deserialize_files_or_count")]
    pub files: Vec<InstallFileEntry>,
    /// ISO-8601 timestamp when installation completed.
    #[serde(rename = "installedAt", default, skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<String>,
}

/// Verified installed version discovered under `<root>/versions/<version>`.
///
/// Mirrors the objects returned by `installedVersions` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledVersion {
    /// Semantic version string (`"0.1.0-preview.332"`).
    pub version: String,
    /// Release identifier (`"preview-332"`), if present in `.bfe-install.json`.
    pub release_id: Option<String>,
    /// Source git commit SHA, if present in `.bfe-install.json`.
    pub revision: Option<String>,
    /// Platform key (`"linux-x64"`, etc.), if present in `.bfe-install.json`.
    pub platform: Option<String>,
    /// Absolute or root-joined path to `<root>/versions/<version>`.
    pub dir: PathBuf,
    /// Resolved path to the executable inside `dir`.
    pub executable: PathBuf,
    /// ISO-8601 timestamp when installation completed.
    pub installed_at: Option<String>,
}

/// Sanitizes a raw [`State`] loaded from disk, dropping invalid version strings,
/// zero counts, and duplicate entries in `bad` and `healthy`.
///
/// Mirrors `normalizeState` in the desktop host.
pub fn normalize_state(raw: State) -> State {
    let mut attempts = BTreeMap::new();
    for (version, count) in raw.attempts {
        if parse_version(&version).is_some() && count > 0 {
            attempts.insert(version, count);
        }
    }
    let mut bad = Vec::new();
    let mut seen_bad = BTreeSet::new();
    for v in raw.bad {
        if parse_version(&v).is_some() && seen_bad.insert(v.clone()) {
            bad.push(v);
        }
    }
    let mut healthy = Vec::new();
    let mut seen_healthy = BTreeSet::new();
    for v in raw.healthy {
        if parse_version(&v).is_some() && seen_healthy.insert(v.clone()) {
            healthy.push(v);
        }
    }
    State {
        attempts,
        bad,
        healthy,
    }
}

/// Reads and normalizes `<root>/state.json`, returning [`State::default`] when
/// missing or corrupt.
///
/// Mirrors `readState` in the desktop host.
pub fn read_state(root: &Path) -> State {
    let path = state_file_path(root);
    let Ok(bytes) = fs::read(&path) else {
        return State::default();
    };
    let Ok(parsed) = serde_json::from_slice::<State>(&bytes) else {
        return State::default();
    };
    normalize_state(parsed)
}

/// Atomically writes `state` to `<root>/state.json` via a sibling `.tmp` rename.
///
/// Mirrors `writeState` in the desktop host and `writeJsonAtomic` in
/// the desktop host.
pub fn write_state(root: &Path, state: &State) -> Result<State, String> {
    let normalized = normalize_state(state.clone());
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let target = state_file_path(root);
    let mut nonce = [0u8; 6];
    let _ = getrandom::getrandom(&mut nonce);
    let hex_nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let temp = root.join(format!(
        ".state.json.tmp-{}-{}",
        std::process::id(),
        hex_nonce
    ));
    let mut payload = serde_json::to_vec_pretty(&normalized).map_err(|e| e.to_string())?;
    payload.push(b'\n');
    if let Err(err) = fs::write(&temp, &payload).and_then(|_| fs::rename(&temp, &target)) {
        let _ = fs::remove_file(&temp);
        return Err(err.to_string());
    }
    Ok(normalized)
}

/// Lists all valid installed versions under `<root>/versions`, sorted newest-first.
///
/// Only directories containing a valid `.bfe-install.json` whose `version` matches
/// the directory name and whose `executable` resolves inside the version directory
/// and exists on disk are returned.
///
/// Mirrors `installedVersions` in the desktop host.
pub fn installed_versions(root: &Path, platform: Option<&str>) -> Vec<InstalledVersion> {
    let dir = versions_dir(root);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut list = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let ver_dir = dir.join(&name);
        let record_path = ver_dir.join(INSTALL_RECORD);
        let Ok(raw) = fs::read(&record_path) else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<InstallRecord>(&raw) else {
            continue;
        };
        if parse_version(&record.version).is_none() || record.version != name {
            continue;
        }
        if let Some(expected_plat) = platform {
            if record.platform.as_deref() != Some(expected_plat) {
                continue;
            }
        }
        if record.executable.is_empty() {
            continue;
        }
        let norm_dir = normalize_lexical(&ver_dir);
        let exe = resolve_from(&norm_dir, Path::new(&record.executable));
        if (exe != norm_dir && !exe.starts_with(&norm_dir)) || !exe.exists() {
            continue;
        }
        list.push(InstalledVersion {
            version: record.version,
            release_id: record.release_id,
            revision: record.revision,
            platform: record.platform,
            dir: ver_dir,
            executable: exe,
            installed_at: record.installed_at,
        });
    }
    list.sort_by(|a, b| {
        match compare_versions(&b.version, &a.version).unwrap_or(0) {
            cmp if cmp < 0 => std::cmp::Ordering::Less,
            cmp if cmp > 0 => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        }
    });
    list
}

/// Selects the newest installed version newer than `current_version` that is eligible
/// for launch handoff (not `own_dir`, not in `state.bad`, and either in `state.healthy`
/// or with fewer than [`MAX_ATTEMPTS`] unconfirmed attempts).
///
/// Mirrors `chooseLaunch` in the desktop host.
pub fn choose_launch(
    root: &Path,
    current_version: &str,
    platform: Option<&str>,
    own_dir: Option<&Path>,
) -> Option<InstalledVersion> {
    parse_version(current_version)?;
    let state = read_state(root);
    let candidates = installed_versions(root, platform);
    let resolved_own = own_dir.map(normalize_lexical);
    for item in candidates {
        if compare_versions(&item.version, current_version).unwrap_or(0) <= 0 {
            break;
        }
        if let Some(own) = &resolved_own {
            if normalize_lexical(&item.dir) == *own {
                continue;
            }
        }
        if state.bad.contains(&item.version) {
            continue;
        }
        let attempts = state.attempts.get(&item.version).copied().unwrap_or(0);
        let healthy = state.healthy.contains(&item.version);
        if !healthy && attempts >= MAX_ATTEMPTS {
            continue;
        }
        return Some(item);
    }
    None
}

/// Increments the launch attempt counter for `version` in `<root>/state.json`.
/// When an unconfirmed version reaches [`MAX_ATTEMPTS`], adds it to `state.bad`.
///
/// Mirrors `recordAttempt` in the desktop host.
pub fn record_attempt(root: &Path, version: &str) -> Result<State, String> {
    let mut state = read_state(root);
    let count = state.attempts.get(version).copied().unwrap_or(0) + 1;
    state.attempts.insert(version.to_string(), count);
    if !state.healthy.contains(&version.to_string())
        && count >= MAX_ATTEMPTS
        && !state.bad.contains(&version.to_string())
    {
        state.bad.push(version.to_string());
    }
    write_state(root, &state)
}

/// Records `version` as healthy in `<root>/state.json`, clearing any failed attempt
/// counter or `bad` marker and placing `version` at the front of `healthy` (capped at 10).
///
/// Mirrors `recordHealthy` in the desktop host.
pub fn record_healthy(root: &Path, version: &str) -> Result<State, String> {
    let mut state = read_state(root);
    state.attempts.remove(version);
    state.bad.retain(|v| v != version);
    state.healthy.retain(|v| v != version);
    state.healthy.insert(0, version.to_string());
    state.healthy.truncate(10);
    write_state(root, &state)
}

/// Atomically points `<root>/current` at the relative path from `root` to `dir`
/// on Unix so Linux launcher scripts (`~/.local/bin/battlefront`) can exec the
/// newest healthy version directly without unpacking the AppImage.
///
/// Mirrors `pointCurrent` in the desktop host.
pub fn point_current(root: &Path, dir: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let norm_root = normalize_lexical(root);
        let norm_dir = normalize_lexical(dir);
        let link = root.join("current");
        let target = norm_dir
            .strip_prefix(&norm_root)
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|_| dir.strip_prefix(root).unwrap_or(dir).to_path_buf());
        if let Ok(existing) = fs::read_link(&link) {
            if existing == target {
                return false;
            }
        }
        if let Ok(meta) = fs::symlink_metadata(&link) {
            if meta.is_dir() {
                return false;
            }
        }
        let temp = root.join(format!(".current-{}", std::process::id()));
        let _ = fs::remove_file(&temp);
        if symlink(&target, &temp).is_ok() && fs::rename(&temp, &link).is_ok() {
            return true;
        }
        let _ = fs::remove_file(&temp);
        false
    }
    #[cfg(not(unix))]
    {
        let _ = (root, dir);
        false
    }
}

/// Garbage-collects installed version directories under `<root>/versions` that are
/// not in `keep_versions` or the first `keep_healthy` entries of `state.healthy`,
/// and prunes launch readiness files under `<root>/launches` older than 5 minutes.
///
/// Mirrors `collect` in the desktop host.
pub fn collect(root: &Path, keep_versions: &[String], keep_healthy: usize) -> Vec<String> {
    let mut state = read_state(root);
    let mut keep: BTreeSet<String> = keep_versions
        .iter()
        .filter(|v| !v.is_empty())
        .cloned()
        .collect();
    for version in state.healthy.iter().take(keep_healthy) {
        keep.insert(version.clone());
    }

    let dir = versions_dir(root);
    let mut removed = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
                continue;
            };
            if keep.contains(&name) {
                continue;
            }
            let target = dir.join(&name);
            let _ = fs::remove_dir_all(&target);
            if !target.exists() {
                removed.push(name.clone());
                state.attempts.remove(&name);
                state.bad.retain(|v| v != &name);
                state.healthy.retain(|v| v != &name);
            }
        }
        if !removed.is_empty() {
            let _ = write_state(root, &state);
        }
    }

    // Prune stale readiness tokens older than 5 minutes in `<root>/launches`.
    let launches = root.join("launches");
    if let Ok(entries) = fs::read_dir(&launches) {
        let cutoff = SystemTime::now().checked_sub(Duration::from_secs(5 * 60));
        for entry in entries.flatten() {
            let file = launches.join(entry.file_name());
            if let (Some(cutoff_time), Ok(meta)) = (cutoff, fs::metadata(&file)) {
                if let Ok(mtime) = meta.modified() {
                    if mtime < cutoff_time {
                        if meta.is_dir() {
                            let _ = fs::remove_dir_all(&file);
                        } else {
                            let _ = fs::remove_file(&file);
                        }
                    }
                }
            }
        }
    }

    removed
}
