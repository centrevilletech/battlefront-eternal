//! Prepared content version checks, toolchain paths, progress estimation, and
//! subprocess driver (`tools/prepare_content.py`).
//!
//! Mirrors `electron/prepare.js` and `electron/startup-content.js`.
//!
//! Note on `ContentVersion`:
//! `tools/content-version.json` is the single shared source of truth for both
//! the Python preparer and the desktop client. This module never defines a
//! hardcoded `Default` for `ContentVersion`. At runtime, the client loads
//! `<tools_dir>/content-version.json` via [`read_content_version`], where a
//! missing or unparsable file is returned as an explicit error. For checkout
//! builds and tests, [`checkout_content_version`] parses the compile-time
//! embedded `include_str!("../../../tools/content-version.json")` so moving or
//! corrupting the file breaks the build immediately.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::discover::{
    find_content, find_game_installs, inspect_content, inspect_install, installed_add_ons,
    ContentInfo, FindContentOptions, FindGameOptions, GameInstall,
};
use crate::paths::{normalize_comparable_path, resolve_from};
use crate::settings::{PreparationRecord, SettingsStore};

/// Compile-time embedded `tools/content-version.json` from the repository checkout.
///
/// Using `include_str!` ensures that if `tools/content-version.json` is moved
/// or removed, compilation fails rather than silently drifting.
pub const CHECKOUT_CONTENT_VERSION_JSON: &str =
    include_str!("../../../tools/content-version.json");

/// Expected schema versions for prepared content outputs (`tools/content-version.json`).
///
/// Intentional omission: `ContentVersion` does **not** implement `Default`.
/// Callers must load the version table from `tools/content-version.json` via
/// [`read_content_version`] (runtime) or [`checkout_content_version`] (checkout/tests).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentVersion {
    /// Overall `.prepare.json` state format version.
    pub state: u32,
    /// Map importer generation version (`mapImportVersion` in `.prepare.json`).
    pub maps: u32,
    /// Side/muzzle importer generation version (`muzzleVersion` in `.prepare.json`).
    pub sides: u32,
    /// Common asset importer generation version (`commonVersion` in `.prepare.json`).
    pub common: u32,
}

/// Parses a `tools/content-version.json` payload into a [`ContentVersion`].
///
/// Rejects missing fields, non-integer values, or malformed JSON with a
/// human-readable error message suitable for surfacing in the loader UI.
pub fn parse_content_version(raw: &str, source_label: &str) -> Result<ContentVersion, String> {
    // Parse strictly via serde_json so any missing or extra-typed required key fails.
    serde_json::from_str::<ContentVersion>(raw).map_err(|err| {
        format!(
            "Failed to parse content version file ({}): {}",
            source_label, err
        )
    })
}

/// Reads and parses `<tools_dir>/content-version.json` at runtime.
///
/// Mirrors how `electron/prepare.js` loads `../tools/content-version.json` from
/// the installed `tools` directory (`resourcesPath/tools/content-version.json`
/// in packaged builds). Never falls back to literal numbers.
pub fn read_content_version(tools_dir: &Path) -> Result<ContentVersion, String> {
    let path = tools_dir.join("content-version.json");
    let raw = fs::read_to_string(&path).map_err(|err| {
        format!(
            "Missing or unreadable content version file at {}: {}",
            path.display(),
            err
        )
    })?;
    parse_content_version(&raw, &path.display().to_string())
}

/// Parses the repository checkout's `tools/content-version.json` embedded via `include_str!`.
///
/// Used by unit tests and development checkouts when an explicit runtime tools
/// directory is not supplied. Returns an error if the embedded JSON is invalid.
pub fn checkout_content_version() -> Result<ContentVersion, String> {
    parse_content_version(CHECKOUT_CONTENT_VERSION_JSON, "tools/content-version.json")
}

/// Top-level output entries written by `prepare_content.py` into a content folder.
///
/// Mirrors `PREPARED_OUTPUTS` in `electron/prepare.js`.
pub const PREPARED_OUTPUTS: &[&str] = &["maps", "sides", "common", "audio", ".prepare.json"];

/// Step statuses that represent idle or unscheduled steps rather than active work.
///
/// Mirrors `IDLE` in `electron/prepare.js`.
pub const IDLE_STATUSES: &[&str] = &["unavailable", "off", "left-out"];

/// Baseline expected duration in seconds for each preparation step kind.
///
/// Mirrors `EXPECTED` in `electron/prepare.js`.
pub fn expected_baseline_seconds(kind: &str) -> f64 {
    match kind {
        "catalog" => 20.0,
        "catalog-update" => 5.0,
        "side" => 60.0,
        "common" => 90.0,
        "shell" => 30.0,
        "map" => 105.0,
        "environment" => 11.0,
        "hud" => 23.0,
        "audio" => 40.0,
        "textures" => 100.0,
        "movies" => 86.0,
        _ => 30.0,
    }
}

/// Options for resolving bundled toolchain and script paths.
///
/// Mirrors the argument object of `toolchainPaths` in `electron/prepare.js`.
#[derive(Debug, Clone)]
pub struct ToolchainOptions {
    /// Whether the application is running from a packaged build.
    pub packaged: bool,
    /// Path to packaged `resources` directory when `packaged` is true.
    pub resources_path: Option<PathBuf>,
    /// Path to the repository root when `packaged` is false.
    pub project_root: PathBuf,
    /// Target platform identifier (`"linux"`, `"darwin"`, or `"win32"`).
    pub platform: String,
    /// Target CPU architecture (`"x64"`, `"arm64"`, etc.).
    pub arch: String,
}

/// Resolved paths for the bundled toolchain, Python interpreter, and `prepare_content.py`.
///
/// Mirrors the return object of `toolchainPaths` in `electron/prepare.js`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainPaths {
    /// Root directory of the native helper binaries (`toolchain`).
    pub toolchain: PathBuf,
    /// Directory containing Python preparation scripts (`tools`).
    pub tools: PathBuf,
    /// Python executable path or command name (`python3` / `python`).
    pub python: PathBuf,
    /// Full path to `tools/prepare_content.py`.
    pub script: PathBuf,
    /// Full path to `tools/content-version.json`.
    pub content_version_file: PathBuf,
}

/// Resolves the toolchain root, `tools` directory, Python binary, and `prepare_content.py` script.
///
/// Mirrors `toolchainPaths` in `electron/prepare.js`.
pub fn toolchain_paths(opts: &ToolchainOptions) -> ToolchainPaths {
    // Map Node/Rust platform names to the toolchain subdirectory naming convention.
    let os = match opts.platform.as_str() {
        "linux" => "linux",
        "darwin" | "macos" => "mac",
        "win32" | "windows" => "win",
        other => other,
    };

    let (toolchain, tools) = if opts.packaged {
        let res = opts
            .resources_path
            .as_deref()
            .unwrap_or(opts.project_root.as_path());
        (res.join("toolchain"), res.join("tools"))
    } else {
        (
            opts.project_root
                .join("toolchain")
                .join(format!("{}-{}", os, opts.arch)),
            opts.project_root.join("tools"),
        )
    };

    let is_win = opts.platform == "win32" || opts.platform == "windows";
    let bundled = if is_win {
        toolchain.join("python").join("python.exe")
    } else {
        toolchain.join("python").join("bin").join("python3")
    };

    // Prefer the bundled Python interpreter when present on disk; otherwise fall back to PATH.
    let python = if bundled.exists() {
        bundled
    } else if is_win {
        PathBuf::from("python")
    } else {
        PathBuf::from("python3")
    };

    let script = tools.join("prepare_content.py");
    let content_version_file = tools.join("content-version.json");

    ToolchainPaths {
        toolchain,
        tools,
        python,
        script,
        content_version_file,
    }
}

/// Options for constructing the environment variables passed to `prepare_content.py`.
///
/// Mirrors the argument object of `toolEnv` in `electron/prepare.js`.
#[derive(Debug, Clone)]
pub struct ToolEnvOptions {
    /// Toolchain root directory resolved by [`toolchain_paths`].
    pub toolchain: PathBuf,
    /// Optional path to the original game install directory (`BFE_GAME_DIR`).
    pub game: Option<PathBuf>,
    /// Optional path to the target prepared content directory (`BFE_CONTENT_DIR`).
    pub content: Option<PathBuf>,
    /// Base environment variables to inherit from.
    pub env: BTreeMap<String, String>,
    /// Target platform (`"linux"`, `"darwin"`, or `"win32"`).
    pub platform: String,
}

/// Builds the environment variable map for running `prepare_content.py`.
///
/// Prepends existing `toolchain/bin` and `toolchain/ktx/bin` directories to `PATH`,
/// configures `KTX2_ENCODER` and shared library search paths when bundled KTX tools
/// are present, sets `BFE_GAME_DIR` / `BFE_CONTENT_DIR` and Python UTF-8/unbuffered
/// flags, and strips `PYTHONHOME`, `PYTHONPATH`, and `ELECTRON_RUN_AS_NODE`.
///
/// Mirrors `toolEnv` in `electron/prepare.js`.
pub fn tool_env(opts: &ToolEnvOptions) -> BTreeMap<String, String> {
    let is_win = opts.platform == "win32" || opts.platform == "windows";
    let sep = if is_win { ";" } else { ":" };

    let bin = opts.toolchain.join("bin");
    let ktx_bin = opts.toolchain.join("ktx").join("bin");
    let ktx_lib = opts.toolchain.join("ktx").join("lib");

    // Collect only the binary directories that actually exist on disk.
    let mut bins = Vec::new();
    if bin.exists() {
        bins.push(bin.to_string_lossy().into_owned());
    }
    if ktx_bin.exists() {
        bins.push(ktx_bin.to_string_lossy().into_owned());
    }

    let mut next = opts.env.clone();
    let existing_path = next
        .get("PATH")
        .or_else(|| next.get("Path"))
        .cloned()
        .unwrap_or_default();
    if !existing_path.is_empty() {
        bins.push(existing_path);
    }
    next.insert("PATH".to_string(), bins.join(sep));

    if let Some(game) = &opts.game {
        next.insert("BFE_GAME_DIR".to_string(), game.to_string_lossy().into_owned());
    }
    if let Some(content) = &opts.content {
        next.insert(
            "BFE_CONTENT_DIR".to_string(),
            content.to_string_lossy().into_owned(),
        );
    }

    next.insert("PYTHONUNBUFFERED".to_string(), "1".to_string());
    next.insert("PYTHONDONTWRITEBYTECODE".to_string(), "1".to_string());
    next.insert("PYTHONIOENCODING".to_string(), "utf-8".to_string());

    // Strip ambient Python / Electron variables that could hijack the bundled interpreter.
    next.remove("PYTHONHOME");
    next.remove("PYTHONPATH");
    next.remove("ELECTRON_RUN_AS_NODE");

    let ktx_exe = ktx_bin.join(if is_win { "ktx.exe" } else { "ktx" });
    if ktx_exe.exists() {
        next.insert(
            "KTX2_ENCODER".to_string(),
            ktx_exe.to_string_lossy().into_owned(),
        );
    }

    // Add bundled KTX shared libraries to the platform loader path when present.
    if ktx_lib.exists() && opts.platform == "linux" {
        let mut parts = vec![ktx_lib.to_string_lossy().into_owned()];
        if let Some(ld) = next.get("LD_LIBRARY_PATH").filter(|s| !s.is_empty()) {
            parts.push(ld.clone());
        }
        next.insert("LD_LIBRARY_PATH".to_string(), parts.join(":"));
    }
    if ktx_lib.exists() && (opts.platform == "darwin" || opts.platform == "macos") {
        let mut parts = vec![ktx_lib.to_string_lossy().into_owned()];
        if let Some(dyld) = next.get("DYLD_LIBRARY_PATH").filter(|s| !s.is_empty()) {
            parts.push(dyld.clone());
        }
        next.insert("DYLD_LIBRARY_PATH".to_string(), parts.join(":"));
    }

    next
}

/// Optional extra preparation flags stored in `.prepare.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareStateExtras {
    /// Whether enhanced sound banks were built (`None` or `Some(true)` means enabled).
    #[serde(default)]
    pub enhanced_sound: Option<bool>,
    /// Number of enhanced sound steps executed.
    #[serde(default)]
    pub sound_steps: Option<u32>,
}

/// Failure record for an individual preparation step stored in `.prepare.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareFailureRecord {
    /// Step identifier (e.g. `"addon:bes1g"`).
    pub id: String,
    /// Optional human-readable label for the step.
    #[serde(default)]
    pub label: Option<String>,
    /// Optional error description.
    #[serde(default)]
    pub error: Option<String>,
}

/// Deserialized representation of `<content>/.prepare.json`.
///
/// Mirrors the state object read by `readPrepareState` in `electron/prepare.js`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareState {
    /// State format version (`ContentVersion::state`).
    pub version: Option<u32>,
    /// Map import version (`ContentVersion::maps`).
    pub map_import_version: Option<u32>,
    /// Side/muzzle version (`ContentVersion::sides`).
    pub muzzle_version: Option<u32>,
    /// Common assets version (`ContentVersion::common`).
    pub common_version: Option<u32>,
    /// Whether preparation finished all required steps.
    pub complete: Option<bool>,
    /// Path to `<game>/GameData/Data/_LVL_PC` used during preparation.
    pub game: Option<String>,
    /// Add-on map IDs included in the preparation run.
    pub add_ons: Option<Vec<String>>,
    /// Step IDs that failed during preparation.
    pub failed: Option<Vec<Value>>,
    /// Step IDs that were skipped during preparation.
    pub skipped: Option<Vec<Value>>,
    /// Detailed failure records.
    pub failures: Option<Vec<PrepareFailureRecord>>,
    /// Optional extra preparation metadata (such as enhanced sound status).
    pub extras: Option<PrepareStateExtras>,
}

/// Reads and parses `<content_dir>/.prepare.json`, returning `None` if missing or invalid.
///
/// Mirrors `readPrepareState` in `electron/prepare.js`.
pub fn read_prepare_state(content_dir: Option<&Path>) -> Option<PrepareState> {
    let dir = content_dir?;
    let raw = fs::read_to_string(dir.join(".prepare.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Checks whether a `.prepare.json` state matches the required [`ContentVersion`]
/// and completed with zero failed or skipped steps.
///
/// Mirrors `preparedContentCurrent` in `electron/prepare.js`.
pub fn prepared_content_current(
    state: Option<&PrepareState>,
    content_version: &ContentVersion,
) -> bool {
    let Some(st) = state else {
        return false;
    };
    let failed_empty = st.failed.as_ref().map(|v| v.is_empty()).unwrap_or(false);
    let skipped_empty = st.skipped.as_ref().map(|v| v.is_empty()).unwrap_or(false);

    st.version == Some(content_version.state)
        && st.map_import_version == Some(content_version.maps)
        && st.muzzle_version == Some(content_version.sides)
        && st.common_version == Some(content_version.common)
        && st.complete == Some(true)
        && failed_empty
        && skipped_empty
}

/// Normalizes a filesystem path for cross-platform comparison in `prepared_from`.
///
/// On Windows and macOS, paths are compared case-insensitively after resolution.
/// Mirrors `comparablePath` in `electron/prepare.js`.
fn comparable_path(p: &Path, platform: &str) -> String {
    let resolved = resolve_from(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")), p);
    let text = normalize_comparable_path(&resolved);
    if platform == "win32" || platform == "windows" || platform == "darwin" || platform == "macos"
    {
        text.to_lowercase()
    } else {
        text
    }
}

/// Checks whether `state` was prepared from the given `game` installation root.
///
/// Compares `state.game` against `<game>/GameData/Data/_LVL_PC`.
/// Mirrors `preparedFrom` in `electron/prepare.js`.
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

/// Desired optional features when checking if prepared content satisfies user settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WantedPrepareOptions {
    /// Whether enhanced sound is required (`true` rejects content prepared with `enhancedSound: false`).
    pub enhanced_sound: Option<bool>,
}

/// Checks whether `state` is current, prepared from `game`, includes the same
/// sorted `add_ons` list, and satisfies `wanted` options (such as `enhanced_sound`).
///
/// Mirrors `preparedFor` in `electron/prepare.js`.
pub fn prepared_for(
    state: Option<&PrepareState>,
    game: Option<&Path>,
    add_ons: &[String],
    wanted: &WantedPrepareOptions,
    platform: &str,
    content_version: &ContentVersion,
) -> bool {
    if !prepared_content_current(state, content_version) || !prepared_from(state, game, platform) {
        return false;
    };
    let Some(st) = state else {
        return false;
    };
    let Some(state_addons) = st.add_ons.as_ref() else {
        return false;
    };

    // Compare sorted add-on lists for exact equality.
    let mut wanted_addons: Vec<String> = add_ons.to_vec();
    wanted_addons.sort();
    if state_addons.len() != wanted_addons.len()
        || state_addons
            .iter()
            .zip(wanted_addons.iter())
            .any(|(a, b)| a != b)
    {
        return false;
    }

    // If the user wants enhanced sound, reject content explicitly prepared without it.
    let state_enhanced = st.extras.as_ref().and_then(|e| e.enhanced_sound);
    if wanted.enhanced_sound == Some(true) && state_enhanced == Some(false) {
        return false;
    }

    true
}

/// Summary of add-on failures and enhanced sound status extracted from `.prepare.json`.
///
/// Mirrors the return value of `preparedSummary` in `electron/prepare.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedSummary {
    /// Failure records whose `id` starts with `"addon:"`.
    pub add_on_failures: Vec<PrepareFailureRecord>,
    /// Whether enhanced sound is enabled (`true` unless explicitly `false`).
    pub enhanced_sound: bool,
    /// Number of enhanced sound steps recorded.
    pub sound_steps: u32,
    /// All failure records recorded in `.prepare.json`.
    pub failures: Vec<PrepareFailureRecord>,
}

/// Extracts a [`PreparedSummary`] from an optional [`PrepareState`].
///
/// Mirrors `preparedSummary` in `electron/prepare.js`.
pub fn prepared_summary(state: Option<&PrepareState>) -> PreparedSummary {
    let failures = state
        .and_then(|s| s.failures.clone())
        .unwrap_or_default();
    let add_on_failures = failures
        .iter()
        .filter(|f| f.id.starts_with("addon:"))
        .cloned()
        .collect();
    let enhanced_sound = state
        .and_then(|s| s.extras.as_ref())
        .and_then(|e| e.enhanced_sound)
        != Some(false);
    let sound_steps = state
        .and_then(|s| s.extras.as_ref())
        .and_then(|e| e.sound_steps)
        .unwrap_or(0);

    PreparedSummary {
        add_on_failures,
        enhanced_sound,
        sound_steps,
        failures,
    }
}

/// Clears prepared output subdirectories (`maps`, `sides`, `common`, `audio`, `.prepare.json`)
/// inside `content` if its existing `.prepare.json` was not built from `game`.
///
/// Returns `true` when existing prepared outputs were removed so a clean build
/// from `game` can proceed without stale assets from another install.
///
/// Mirrors `clearPreparedFor` in `electron/prepare.js`.
pub fn clear_prepared_for(content: Option<&Path>, game: Option<&Path>, platform: &str) -> bool {
    let (Some(content_dir), Some(game_dir)) = (content, game) else {
        return false;
    };
    let state = read_prepare_state(Some(content_dir));
    if prepared_from(state.as_ref(), Some(game_dir), platform) {
        return false;
    }
    let has_prepared = PREPARED_OUTPUTS
        .iter()
        .any(|entry| content_dir.join(entry).exists());
    if !has_prepared {
        return false;
    }
    for entry in PREPARED_OUTPUTS {
        let target = content_dir.join(entry);
        if target.is_dir() {
            let _ = fs::remove_dir_all(&target);
        } else if target.exists() {
            let _ = fs::remove_file(&target);
        }
    }
    true
}

/// Single step entry in a [`PrepareSnapshot`].
///
/// Mirrors step objects in `snapshot.steps` in `electron/prepare.js`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareStep {
    /// Step identifier (e.g. `"common"`, `"map:bes1a"`).
    pub id: String,
    /// Step category (`"catalog"`, `"side"`, `"common"`, `"map"`, `"audio"`, etc.).
    #[serde(default)]
    pub kind: String,
    /// Human-readable step label.
    #[serde(default)]
    pub label: String,
    /// Current step status (`"pending"`, `"running"`, `"done"`, `"failed"`, `"skipped"`, etc.).
    pub status: String,
    /// Optional detail string emitted by the preparer.
    #[serde(default)]
    pub detail: Option<String>,
    /// Whether the step completed from cache without re-running conversion.
    #[serde(default)]
    pub cached: bool,
    /// Elapsed wall time in seconds once completed.
    #[serde(default)]
    pub seconds: Option<f64>,
    /// Timestamp (milliseconds since epoch) when the step transitioned to `"running"`.
    #[serde(default)]
    pub started_at: Option<u64>,
    /// Fractional sub-step progress (`0.0..=1.0`) reported while running.
    #[serde(default)]
    pub progress: Option<f64>,
}

/// Live snapshot of a running or completed content preparation job.
///
/// Mirrors `createSnapshot()` in `electron/prepare.js`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSnapshot {
    /// High-level phase (`"idle"`, `"starting"`, `"running"`, `"done"`, `"failed"`, `"cancelled"`).
    pub status: String,
    /// Game install directory path.
    pub game: Option<String>,
    /// Target content directory path.
    pub content: Option<String>,
    /// Timestamp (ms) when preparation started.
    pub started_at: Option<u64>,
    /// Timestamp (ms) when preparation finished.
    pub finished_at: Option<u64>,
    /// Number of parallel worker processes scheduled by `prepare_content.py`.
    pub workers: u32,
    /// Ordered list of preparation steps.
    pub steps: Vec<PrepareStep>,
    /// Count of completed steps.
    pub done: usize,
    /// Count of active scheduled steps (excluding `IDLE_STATUSES`).
    pub total: usize,
    /// Overall fractional progress (`0.0..=1.0`).
    pub percent: f64,
    /// Maps that have finished preparing and are ready to play.
    pub playable_maps: Vec<String>,
    /// Whether minimum playable content is ready.
    pub playable: bool,
    /// Whether all scheduled steps completed cleanly.
    pub complete: bool,
    /// Recent log lines (capped at the last 160 lines).
    pub logs: Vec<String>,
    /// Fatal error message if `status == "failed"`.
    pub error: Option<String>,
    /// Estimated remaining seconds (`None` when complete or unknown).
    pub eta_seconds: Option<u64>,
    /// Human-readable phase label for the progress bar.
    pub phase_label: String,
}

/// Creates a fresh idle [`PrepareSnapshot`].
///
/// Mirrors `createSnapshot` in `electron/prepare.js`.
pub fn create_snapshot() -> PrepareSnapshot {
    PrepareSnapshot {
        status: "idle".to_string(),
        game: None,
        content: None,
        started_at: None,
        finished_at: None,
        workers: 1,
        steps: Vec::new(),
        done: 0,
        total: 0,
        percent: 0.0,
        playable_maps: Vec::new(),
        playable: false,
        complete: false,
        logs: Vec::new(),
        error: None,
        eta_seconds: None,
        phase_label: String::new(),
    }
}

/// Appends a trimmed log line to `snapshot.logs`, retaining at most the last 160 lines.
///
/// Mirrors `pushLog` in `electron/prepare.js`.
pub fn push_log(snapshot: &mut PrepareSnapshot, line: &str) {
    let text = line.trim();
    if text.is_empty() {
        return;
    }
    let start = snapshot.logs.len().saturating_sub(159);
    let mut next: Vec<String> = snapshot.logs[start..].to_vec();
    next.push(text.to_string());
    snapshot.logs = next;
}

/// Recomputes `total`, `done`, and `percent` from `snapshot.steps`.
///
/// Excludes steps whose status is in [`IDLE_STATUSES`] (`unavailable`, `off`, `left-out`).
/// Mirrors `recount` in `electron/prepare.js`.
pub fn recount(snapshot: &mut PrepareSnapshot) {
    let active: Vec<&PrepareStep> = snapshot
        .steps
        .iter()
        .filter(|s| !IDLE_STATUSES.contains(&s.status.as_str()))
        .collect();
    snapshot.total = active.len();
    snapshot.done = active.iter().filter(|s| s.status == "done").count();
    // Sum completed steps as 1.0 and running steps by their clamped sub-step progress (<= 0.98).
    let progressed: f64 = active
        .iter()
        .map(|s| {
            if s.status == "done" {
                1.0
            } else if s.status == "running" {
                s.progress.unwrap_or(0.0).clamp(0.0, 0.98)
            } else {
                0.0
            }
        })
        .sum();
    snapshot.percent = if snapshot.total > 0 {
        (progressed / snapshot.total as f64).min(1.0)
    } else {
        0.0
    };
}

/// Applies a JSON event emitted by `prepare_content.py` to a [`PrepareSnapshot`].
///
/// Handles `"log"`, `"plan"`, `"step"`, `"playable"`, `"finished"`, `"busy"`,
/// and `"error"` events identically to `applyEvent` in `electron/prepare.js`.
pub fn apply_event(snapshot: &mut PrepareSnapshot, event: &Value, now_ms: u64) {
    let Some(event_type) = event.get("event").and_then(|v| v.as_str()) else {
        return;
    };

    match event_type {
        // Append diagnostic log lines emitted by the Python preparer.
        "log" => {
            let msg = event.get("message").and_then(|v| v.as_str()).unwrap_or("");
            push_log(snapshot, msg);
        }
        // Replace the step list and worker count when the initial execution plan arrives.
        "plan" => {
            snapshot.status = "running".to_string();
            snapshot.game = event
                .get("game")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| snapshot.game.clone());
            snapshot.content = event
                .get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| snapshot.content.clone());
            let w = event
                .get("workers")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                .max(1) as u32;
            snapshot.workers = w;
            if let Some(arr) = event.get("steps").and_then(|v| v.as_array()) {
                snapshot.steps = arr
                    .iter()
                    .filter_map(|item| serde_json::from_value::<PrepareStep>(item.clone()).ok())
                    .collect();
            }
            recount(snapshot);
        }
        // Update an existing step's status/progress or insert a dynamically discovered step.
        "step" => {
            let Some(step_id) = event.get("id").and_then(|v| v.as_str()) else {
                return;
            };
            let new_status = event
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let detail = event
                .get("detail")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let cached = event
                .get("cached")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let seconds = event.get("seconds").and_then(|v| v.as_f64());
            let progress = event.get("progress").and_then(|v| v.as_f64());
            let kind = event
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let label = event
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(existing) = snapshot.steps.iter_mut().find(|s| s.id == step_id) {
                let was_running = existing.status == "running";
                existing.status = new_status.clone();
                if detail.is_some() {
                    existing.detail = detail.clone();
                }
                if event.get("cached").is_some() {
                    existing.cached = cached;
                }
                if seconds.is_some() {
                    existing.seconds = seconds;
                }
                if progress.is_some() {
                    existing.progress = progress;
                }
                if !kind.is_empty() {
                    existing.kind = kind;
                }
                if !label.is_empty() {
                    existing.label = label;
                }
                if new_status == "running" && !was_running {
                    existing.started_at = Some(now_ms);
                    existing.progress = Some(progress.unwrap_or(0.0));
                }
                if new_status == "done" {
                    existing.progress = Some(1.0);
                }
            } else {
                snapshot.steps.push(PrepareStep {
                    id: step_id.to_string(),
                    kind,
                    label: label.clone(),
                    status: new_status.clone(),
                    detail: detail.clone(),
                    cached,
                    seconds,
                    started_at: if new_status == "running" {
                        Some(now_ms)
                    } else {
                        None
                    },
                    progress: if new_status == "done" {
                        Some(1.0)
                    } else {
                        progress
                    },
                });
            }

            if new_status == "failed" {
                let lbl = snapshot
                    .steps
                    .iter()
                    .find(|s| s.id == step_id)
                    .map(|s| {
                        if s.label.is_empty() {
                            s.id.as_str()
                        } else {
                            s.label.as_str()
                        }
                    })
                    .unwrap_or(step_id);
                let msg = format!("{}: {}", lbl, detail.as_deref().unwrap_or("failed"));
                push_log(snapshot, &msg);
            }
            recount(snapshot);
        }
        // Record the set of playable maps once core assets and at least one map are ready.
        "playable" => {
            snapshot.playable = true;
            if let Some(maps) = event.get("maps").and_then(|v| v.as_array()) {
                snapshot.playable_maps = maps
                    .iter()
                    .filter_map(|m| m.as_str().map(|s| s.to_string()))
                    .collect();
            }
        }
        // Transition the snapshot to `"done"` or `"failed"` when the preparer exits.
        "finished" => {
            let complete = event
                .get("complete")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            snapshot.complete = complete;
            snapshot.playable = event
                .get("playable")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                || snapshot.playable;
            snapshot.status = if complete {
                "done".to_string()
            } else {
                "failed".to_string()
            };
            snapshot.finished_at = Some(now_ms);
            recount(snapshot);
        }
        // Report folder lock contention when another preparer is already writing to `content`.
        "busy" => {
            snapshot.status = "failed".to_string();
            let msg = event
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Another preparation is already using that folder.")
                .to_string();
            snapshot.error = Some(msg.clone());
            push_log(snapshot, &msg);
        }
        // Report fatal top-level preparer errors.
        "error" => {
            snapshot.status = "failed".to_string();
            let msg = event
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Content preparation failed.")
                .to_string();
            snapshot.error = Some(msg.clone());
            push_log(snapshot, &msg);
        }
        _ => {}
    }
}

/// Historical step timing samples keyed by step `kind` (e.g. `"map"`, `"side"`).
pub type StepTimings = BTreeMap<String, Vec<f64>>;

/// Returns the expected duration in seconds for a step kind using historical
/// median timings when available, or [`expected_baseline_seconds`] otherwise.
///
/// Mirrors `expectedSeconds` in `electron/prepare.js`.
pub fn expected_seconds_for_kind(kind: &str, timings: Option<&StepTimings>) -> f64 {
    if let Some(samples) = timings.and_then(|t| t.get(kind)) {
        let mut valid: Vec<f64> = samples
            .iter()
            .copied()
            .filter(|n| n.is_finite() && *n > 0.2)
            .collect();
        if !valid.is_empty() {
            valid.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            return valid[valid.len() / 2];
        }
    }
    expected_baseline_seconds(kind)
}

/// Returns the expected duration in seconds for a [`PrepareStep`].
///
/// Mirrors `expectedSeconds(step, timings)` in `electron/prepare.js`.
pub fn expected_seconds(step: &PrepareStep, timings: Option<&StepTimings>) -> f64 {
    expected_seconds_for_kind(&step.kind, timings)
}

/// Records a completed, non-cached step duration into the rolling timing history
/// (retaining at most the last 8 samples per step kind).
///
/// Mirrors `recordTiming` in `electron/prepare.js`.
pub fn record_timing(
    timings: &StepTimings,
    step: &PrepareStep,
    seconds: f64,
) -> StepTimings {
    if step.kind.is_empty()
        || !seconds.is_finite()
        || seconds <= 0.0
        || step.status != "done"
        || step.cached
    {
        return timings.clone();
    }
    let mut next = timings.clone();
    let list = next.entry(step.kind.clone()).or_default();
    let start = list.len().saturating_sub(7);
    let mut updated = list[start..].to_vec();
    updated.push((seconds * 100.0).round() / 100.0);
    *list = updated;
    next
}

/// Computed ETA and phase label for a [`PrepareSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstimateResult {
    /// Estimated remaining seconds (`None` when complete, failed, or empty).
    pub eta_seconds: Option<u64>,
    /// Human-readable description of the current preparation phase.
    pub phase_label: String,
}

/// Estimates remaining preparation time and the human-readable phase label.
///
/// Calibrates baseline expectations against actual non-cached step durations once
/// at least 3 samples have completed (clamped to `0.5..=3.0`) and accounts for
/// parallel worker concurrency on map steps.
///
/// Mirrors `estimate` in `electron/prepare.js`.
pub fn estimate(
    snapshot: &PrepareSnapshot,
    timings: Option<&StepTimings>,
    now_ms: u64,
) -> EstimateResult {
    if snapshot.status == "done" {
        return EstimateResult {
            eta_seconds: Some(0),
            phase_label: "Ready to play".to_string(),
        };
    }
    if snapshot.status == "failed" || snapshot.status == "cancelled" {
        return EstimateResult {
            eta_seconds: None,
            phase_label: snapshot
                .error
                .clone()
                .unwrap_or_else(|| "Preparation stopped".to_string()),
        };
    }
    if snapshot.steps.is_empty() {
        return EstimateResult {
            eta_seconds: None,
            phase_label: "Inspecting game files".to_string(),
        };
    }

    // Measure host speed relative to expected durations across completed non-cached steps.
    let mut actual_total = 0.0;
    let mut expected_done = 0.0;
    let mut samples = 0usize;
    for step in &snapshot.steps {
        if step.status == "done" && !step.cached {
            if let Some(sec) = step.seconds.filter(|s| s.is_finite() && *s > 0.2) {
                actual_total += sec;
                expected_done += expected_seconds(step, timings);
                samples += 1;
            }
        }
    }
    let calibration = if samples >= 3 && expected_done > 0.0 {
        (actual_total / expected_done).clamp(0.5, 3.0)
    } else {
        1.0
    };

    let running: Vec<&PrepareStep> = snapshot
        .steps
        .iter()
        .filter(|s| s.status == "running")
        .collect();
    let pending: Vec<&PrepareStep> = snapshot
        .steps
        .iter()
        .filter(|s| s.status == "pending")
        .collect();

    let mut serial_remaining = 0.0;
    let mut map_remaining = 0.0;

    // Sum remaining time for currently running steps using sub-step progress when available.
    for step in &running {
        let full = expected_seconds(step, timings) * calibration;
        let elapsed = step
            .started_at
            .map(|st| now_ms.saturating_sub(st) as f64 / 1000.0)
            .unwrap_or(0.0)
            .max(0.0);
        let by_progress = step
            .progress
            .filter(|p| p.is_finite() && *p > 0.05)
            .map(|p| full * (1.0 - p.min(0.98)));
        let left = by_progress
            .unwrap_or(full - elapsed)
            .max(full * 0.08);
        if step.kind == "map" {
            map_remaining += left;
        } else {
            serial_remaining += left;
        }
    }

    // Add full calibrated duration for all pending steps.
    for step in &pending {
        let full = expected_seconds(step, timings) * calibration;
        if step.kind == "map" {
            map_remaining += full;
        } else {
            serial_remaining += full;
        }
    }

    // Map steps execute across `snapshot.workers` processes in parallel; serial steps do not.
    let workers = snapshot.workers.max(1) as f64;
    let eta = (serial_remaining + map_remaining / workers).round().max(1.0) as u64;

    let phase_label = if running.len() == 1 {
        format!("Preparing {}", running[0].label)
    } else if running.len() > 1 {
        let preview: Vec<&str> = running
            .iter()
            .take(2)
            .map(|s| s.label.as_str())
            .collect();
        let extra = if running.len() > 2 {
            format!(" +{}", running.len() - 2)
        } else {
            String::new()
        };
        format!("Preparing {}{}", preview.join(", "), extra)
    } else if !pending.is_empty() {
        format!("Queued: {}", pending[0].label)
    } else {
        "Finishing up".to_string()
    };

    EstimateResult {
        eta_seconds: Some(eta),
        phase_label,
    }
}

/// Counts of planned steps by category passed to [`plan_estimate`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanCounts {
    /// Number of side steps.
    pub sides: u32,
    /// Number of common steps.
    pub common: u32,
    /// Number of shell steps.
    pub shell: u32,
    /// Number of map steps.
    pub maps: u32,
    /// Number of audio steps.
    pub audio: u32,
    /// Number of enhanced sound steps.
    pub sound: u32,
    /// Number of texture steps.
    pub textures: u32,
    /// Number of movie steps.
    pub movies: u32,
}

/// Estimated total and sound-only seconds returned by [`plan_estimate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEstimateResult {
    /// Total estimated preparation duration in seconds.
    pub total: u64,
    /// Portion of the estimate attributable to enhanced sound steps.
    pub sound: u64,
}

/// Computes an upfront time estimate for a preparation plan before execution starts.
///
/// Mirrors `planEstimate` in `electron/prepare.js`.
pub fn plan_estimate(
    counts: &PlanCounts,
    workers: u32,
    timings: Option<&StepTimings>,
    enhanced_sound: bool,
) -> PlanEstimateResult {
    let kind_sec = |kind: &str| expected_seconds_for_kind(kind, timings);
    let sound_steps = if enhanced_sound { counts.sound } else { 0 };
    let sound = sound_steps as f64 * kind_sec("audio");
    let serial = kind_sec("catalog")
        + counts.sides as f64 * kind_sec("side")
        + counts.common as f64 * kind_sec("common")
        + counts.shell as f64 * kind_sec("shell")
        + kind_sec("environment")
        + kind_sec("hud")
        + (counts.audio + sound_steps) as f64 * kind_sec("audio")
        + counts.textures as f64 * kind_sec("textures")
        + counts.movies as f64 * kind_sec("movies")
        + kind_sec("catalog-update");
    let parallel = (counts.maps as f64 * kind_sec("map")) / (workers.max(1) as f64);
    PlanEstimateResult {
        total: (serial + parallel).round().max(1.0) as u64,
        sound: sound.round().max(0.0) as u64,
    }
}

/// Startup content resolution mode returned by [`resolve_startup_content`].
///
/// Mirrors the `mode` field returned by `startupContent` in `electron/startup-content.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupMode {
    /// Content is current and ready to enter the main menu immediately.
    Menu,
    /// Only one valid install exists and was previously chosen; start preparing automatically.
    Prepare,
    /// One or more installs were found and require user confirmation.
    Confirm,
    /// Prepared content exists on disk but its version is outdated and no game install was found.
    Outdated,
    /// Neither prepared content nor a game installation could be found.
    Missing,
}

/// Complete startup content resolution result.
///
/// Mirrors the return value of `startupContent` in `electron/startup-content.js`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupContentResult {
    /// Startup action the desktop shell should take.
    pub mode: StartupMode,
    /// Discovered current prepared content, if any.
    pub content: Option<ContentInfo>,
    /// Primary selected game installation, if any.
    pub install: Option<GameInstall>,
    /// All candidate game installations discovered on the machine.
    pub installs: Vec<GameInstall>,
    /// Pre-selected game installation directory for the confirmation modal.
    pub selected: Option<PathBuf>,
}

/// Resolves the startup content state from saved settings, candidate directories,
/// and discovered game installations.
///
/// Loads `tools/content-version.json` from `tools_dir` (when provided) or from
/// the compile-time embedded checkout copy, returning an explicit error if the
/// version file is missing or unparsable.
///
/// Mirrors `startupContent` in `electron/startup-content.js`.
pub fn resolve_startup_content(
    settings: &SettingsStore,
    likely_content_dirs: &[PathBuf],
    home: Option<&Path>,
    platform: &str,
    env: &BTreeMap<String, String>,
    tools_dir: Option<&Path>,
) -> Result<StartupContentResult, String> {
    let content_version = match tools_dir {
        Some(dir) => read_content_version(dir)?,
        None => checkout_content_version()?,
    };

    let saved = settings.load();
    let explicit_content: Vec<PathBuf> = saved
        .content_dir
        .as_ref()
        .or(saved.chosen_content_dir.as_ref())
        .map(|s| vec![PathBuf::from(s)])
        .unwrap_or_default();
    let extra_game: Vec<PathBuf> = saved
        .game_dir
        .as_ref()
        .map(|s| vec![PathBuf::from(s)])
        .unwrap_or_default();

    let raw_content = find_content(&FindContentOptions {
        explicit: explicit_content,
        likely: likely_content_dirs.to_vec(),
        home: home.map(|p| p.to_path_buf()),
        search: true,
    });

    let prepare_state = raw_content
        .as_ref()
        .and_then(|c| read_prepare_state(Some(&c.dir)));
    let current = raw_content.is_some()
        && prepared_content_current(prepare_state.as_ref(), &content_version);
    let content = if current { raw_content.clone() } else { None };
    let stale_content = if raw_content.is_some() && !current {
        raw_content
    } else {
        None
    };

    let installs = find_game_installs(&FindGameOptions {
        platform: platform.to_string(),
        home: home.map(|p| p.to_path_buf()),
        env: env.clone(),
        extra: extra_game,
    });

    let install = installs.first().cloned();
    if let Some(c) = content {
        return Ok(StartupContentResult {
            mode: StartupMode::Menu,
            content: Some(c),
            install,
            installs,
            selected: None,
        });
    }

    // Check if the user previously prepared from a specific install that is still valid.
    let prepared_game_dir = saved
        .preparations
        .values()
        .next()
        .map(|p: &PreparationRecord| PathBuf::from(&p.game_dir))
        .or_else(|| saved.game_dir.as_ref().map(PathBuf::from));

    if let Some(cand) = prepared_game_dir {
        if let Some(valid) = inspect_install(&cand) {
            if installs.len() <= 1 {
                let matched = installs.first().cloned().unwrap_or(GameInstall {
                    dir: valid.dir.clone(),
                    source: "chosen".to_string(),
                    how: "the folder you chose".to_string(),
                    chosen: true,
                });
                return Ok(StartupContentResult {
                    mode: StartupMode::Prepare,
                    content: None,
                    install: Some(matched.clone()),
                    installs: if installs.is_empty() {
                        vec![matched]
                    } else {
                        installs
                    },
                    selected: Some(valid.dir),
                });
            }
        }
    }

    if let Some(first) = install {
        let selected = Some(first.dir.clone());
        return Ok(StartupContentResult {
            mode: StartupMode::Confirm,
            content: None,
            install: Some(first),
            installs,
            selected,
        });
    }

    if stale_content.is_some() {
        return Ok(StartupContentResult {
            mode: StartupMode::Outdated,
            content: None,
            install: None,
            installs,
            selected: None,
        });
    }

    Ok(StartupContentResult {
        mode: StartupMode::Missing,
        content: None,
        install: None,
        installs,
        selected: None,
    })
}

/// Options for spawning a live `prepare_content.py` subprocess via [`PrepareSession::start`].
#[derive(Debug, Clone)]
pub struct PrepareSpawnOptions {
    /// Resolved toolchain and script paths.
    pub toolchain: ToolchainPaths,
    /// Original game installation directory.
    pub game_dir: PathBuf,
    /// Target content output directory.
    pub content_dir: PathBuf,
    /// Base environment variables.
    pub env: BTreeMap<String, String>,
    /// Target platform (`"linux"`, `"darwin"`, `"win32"`).
    pub platform: String,
    /// Whether to include discovered add-on campaigns (`--no-addons` when `false`).
    pub include_add_ons: bool,
    /// Whether to build enhanced sound (`--no-enhanced-sound` when `false`).
    pub enhanced_sound: bool,
    /// Whether to run in `--plan-only` mode.
    pub plan_only: bool,
}

/// Handle to a running `prepare_content.py` child process and its live [`PrepareSnapshot`].
pub struct PrepareSession {
    /// Shared live snapshot updated as JSON lines arrive from stdout.
    pub snapshot: Arc<Mutex<PrepareSnapshot>>,
    /// Receiver that yields a cloned [`PrepareSnapshot`] after each event.
    pub events: Receiver<PrepareSnapshot>,
    child: Arc<Mutex<Option<Child>>>,
}

impl PrepareSession {
    /// Spawns `python prepare_content.py` with the configured arguments and begins
    /// streaming JSON progress events on a background reader thread.
    ///
    /// First verifies that `<toolchain.tools>/content-version.json` is present and
    /// valid via [`read_content_version`].
    pub fn start(opts: PrepareSpawnOptions) -> Result<Self, String> {
        // Validate the shared content-version.json before launching the Python preparer.
        let _version = read_content_version(&opts.toolchain.tools)?;

        if inspect_install(&opts.game_dir).is_none() {
            return Err(
                "That folder does not look like a Star Wars Battlefront (2004) install."
                    .to_string(),
            );
        }

        clear_prepared_for(
            Some(&opts.content_dir),
            Some(&opts.game_dir),
            &opts.platform,
        );

        let child_env = tool_env(&ToolEnvOptions {
            toolchain: opts.toolchain.toolchain.clone(),
            game: Some(opts.game_dir.clone()),
            content: Some(opts.content_dir.clone()),
            env: opts.env.clone(),
            platform: opts.platform.clone(),
        });

        let mut cmd = Command::new(&opts.toolchain.python);
        cmd.arg(&opts.toolchain.script)
            .arg("--game")
            .arg(&opts.game_dir)
            .arg("--content")
            .arg(&opts.content_dir);
        if opts.plan_only {
            cmd.arg("--plan-only");
        } else {
            cmd.arg("--json-progress");
        }
        if !opts.include_add_ons {
            cmd.arg("--no-addons");
        }
        if !opts.enhanced_sound {
            cmd.arg("--no-enhanced-sound");
        }
        cmd.env_clear();
        for (k, v) in &child_env {
            cmd.env(k, v);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn prepare_content.py: {}", e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture prepare_content.py stdout".to_string())?;

        let mut initial = create_snapshot();
        initial.status = "starting".to_string();
        initial.game = Some(opts.game_dir.to_string_lossy().into_owned());
        initial.content = Some(opts.content_dir.to_string_lossy().into_owned());

        let snapshot = Arc::new(Mutex::new(initial));
        let child_arc = Arc::new(Mutex::new(Some(child)));
        let (tx, rx) = mpsc::channel();

        let snap_clone = Arc::clone(&snapshot);
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                if let Ok(mut guard) = snap_clone.lock() {
                    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
                        apply_event(&mut guard, &val, now_ms);
                    } else {
                        push_log(&mut guard, trimmed);
                    }
                    let est = estimate(&guard, None, now_ms);
                    guard.eta_seconds = est.eta_seconds;
                    guard.phase_label = est.phase_label;
                    let _ = tx.send(guard.clone());
                }
            }
        });

        Ok(Self {
            snapshot,
            events: rx,
            child: child_arc,
        })
    }

    /// Terminates the running `prepare_content.py` process if it is still active.
    pub fn cancel(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
        }
        if let Ok(mut snap) = self.snapshot.lock() {
            snap.status = "cancelled".to_string();
            snap.error = Some("Preparation cancelled.".to_string());
        }
    }
}

/// Inspects a content folder after preparation and verifies that its `.prepare.json`
/// matches `tools/content-version.json` and the installed add-ons from `game_dir`.
pub fn verify_prepared_outputs(
    content_dir: &Path,
    game_dir: &Path,
    platform: &str,
    tools_dir: Option<&Path>,
) -> Result<bool, String> {
    let version = match tools_dir {
        Some(dir) => read_content_version(dir)?,
        None => checkout_content_version()?,
    };
    if inspect_content(content_dir).is_none() {
        return Ok(false);
    }
    let state = read_prepare_state(Some(content_dir));
    let addons = installed_add_ons(game_dir);
    let _ = BTreeSet::from_iter(addons.iter());
    Ok(prepared_for(
        state.as_ref(),
        Some(game_dir),
        &addons,
        &WantedPrepareOptions::default(),
        platform,
        &version,
    ))
}
