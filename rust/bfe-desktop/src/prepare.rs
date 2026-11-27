//! Prepared content version checks, toolchain paths, progress estimation, and
//! subprocess driver (`tools/prepare.py`).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;

use bfe_content::layout::{family_dir, family_dir_for_platform, prepare_state_file};
pub use bfe_content::prepare::{
    checkout_content_version, parse_content_version, prepared_content_current, read_content_version,
    read_prepare_state, ContentVersion, PrepareFailureRecord, PrepareState, PrepareStateExtras,
    CHECKOUT_CONTENT_VERSION_JSON,
};
use bfe_family::GameFamily;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::discover::{
    find_content_for, find_game_installs_for, find_swbf2_lvl_pc, game_profile, inspect_content,
    inspect_install_for, installed_add_ons, ContentInfo, FindContentOptions, FindGameOptions,
    GameInstall,
};
use crate::paths::{normalize_comparable_path, resolve_from};
use crate::settings::{PreparationRecord, SettingsStore};
use crate::swbf2_tree;

/// Top-level output entries written by `prepare.py` into a content folder.
///
/// Mirrors `PREPARED_OUTPUTS` in the desktop host.
pub const PREPARED_OUTPUTS: &[&str] = &["maps", "sides", "common", "audio", ".prepare.json"];

/// Step statuses that represent idle or unscheduled steps rather than active work.
///
/// Mirrors `IDLE` in the desktop host.
pub const IDLE_STATUSES: &[&str] = &["unavailable", "off", "left-out"];

/// Baseline expected duration in seconds for each preparation step kind.
///
/// Mirrors `EXPECTED` in the desktop host.
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
        _ => 30.0,
    }
}

/// Options for resolving bundled toolchain and script paths.
///
/// Mirrors the argument object of `toolchainPaths` in the desktop host.
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

/// Resolved paths for the bundled toolchain, Python interpreter, and `prepare.py`.
///
/// Mirrors the return object of `toolchainPaths` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolchainPaths {
    /// Root directory of the native helper binaries (`toolchain`).
    pub toolchain: PathBuf,
    /// Directory containing Python preparation scripts (`tools`).
    pub tools: PathBuf,
    /// Python executable path or command name (`python3` / `python`).
    pub python: PathBuf,
    /// Full path to `tools/prepare.py`.
    pub script: PathBuf,
    /// Full path to `tools/content-version.json`.
    pub content_version_file: PathBuf,
}

/// Resolves the toolchain root, `tools` directory, Python binary, and `prepare.py` script.
///
/// Mirrors `toolchainPaths` in the desktop host.
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

    let script = tools.join("prepare.py");
    let content_version_file = tools.join("content-version.json");

    ToolchainPaths {
        toolchain,
        tools,
        python,
        script,
        content_version_file,
    }
}

/// Options for constructing the environment variables passed to `prepare.py`.
///
/// Mirrors the argument object of `toolEnv` in the desktop host.
#[derive(Debug, Clone)]
pub struct ToolEnvOptions {
    /// Game family (`swbf1` or `swbf2`). Always written explicitly to `BFE_FAMILY`
    /// so an inherited value is never forwarded to preparation tools. `BFE_CONTENT_DIR`
    /// stays `<content>` for `swbf1` and resolves to `<content>/swbf2/.content` for `swbf2`.
    pub family: GameFamily,
    /// Toolchain root directory resolved by [`toolchain_paths`].
    pub toolchain: PathBuf,
    /// Optional path to the original game install directory (`BFE_GAME_DIR`).
    pub game: Option<PathBuf>,
    /// Optional path to the shared content root (`BFE_CONTENT_DIR` becomes
    /// `family_dir(content, family)`: `<content>` for `swbf1`, `<content>/swbf2/.content` for `swbf2`).
    pub content: Option<PathBuf>,
    /// Base environment variables to inherit from.
    pub env: BTreeMap<String, String>,
    /// Target platform (`"linux"`, `"darwin"`, or `"win32"`).
    pub platform: String,
}

/// Takes a variable out of `env` and returns its value. Windows names its variables without
/// regard to case and spells the search path `Path`, so there every spelling goes.
fn take_var(env: &mut BTreeMap<String, String>, is_win: bool, key: &str) -> Option<String> {
    let mut taken = env.remove(key);
    if is_win {
        let others: Vec<String> = env.keys().filter(|name| name.eq_ignore_ascii_case(key)).cloned().collect();
        for name in others {
            let value = env.remove(&name);
            taken = taken.or(value);
        }
    }
    taken
}

/// Sets a variable for the tools, in place of every spelling of it. On Windows a second spelling
/// left in the map reaches the child after this one and replaces it: the system's `Path` beside
/// this `PATH` took the bundled tools out of the search path again, and the tools did not find
/// the bundled FFmpeg.
fn set_var(env: &mut BTreeMap<String, String>, is_win: bool, key: &str, value: String) {
    take_var(env, is_win, key);
    env.insert(key.to_string(), value);
}

/// Builds the environment variable map for running `prepare.py`.
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
    let existing_path = take_var(&mut next, is_win, "PATH").unwrap_or_default();
    if !existing_path.is_empty() {
        bins.push(existing_path);
    }
    set_var(&mut next, is_win, "PATH", bins.join(sep));

    set_var(&mut next, is_win, "BFE_FAMILY", opts.family.as_str().to_string());
    if let Some(game) = &opts.game {
        set_var(&mut next, is_win, "BFE_GAME_DIR", game.to_string_lossy().into_owned());
    }
    if let Some(content) = &opts.content {
        let content_dir = family_dir_for_platform(content, opts.family, is_win);
        set_var(&mut next, is_win, "BFE_CONTENT_DIR", content_dir);
    }

    set_var(&mut next, is_win, "PYTHONUNBUFFERED", "1".to_string());
    set_var(&mut next, is_win, "PYTHONDONTWRITEBYTECODE", "1".to_string());
    set_var(&mut next, is_win, "PYTHONIOENCODING", "utf-8".to_string());

    // Strip ambient Python variables that could hijack the bundled interpreter.
    take_var(&mut next, is_win, "PYTHONHOME");
    take_var(&mut next, is_win, "PYTHONPATH");

    let ktx_exe = ktx_bin.join(if is_win { "ktx.exe" } else { "ktx" });
    if ktx_exe.exists() {
        set_var(&mut next, is_win, "KTX2_ENCODER", ktx_exe.to_string_lossy().into_owned());
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

/// Reads and parses the preparation receipt for `family` under `content_dir`,
/// returning `None` if missing or invalid.
///
/// - `GameFamily::Swbf1` reads `<content_dir>/.prepare.json` via [`read_prepare_state`].
/// - `GameFamily::Swbf2` reads `<content_dir>/swbf2/.content/.prepare-swbf2.json` and
///   requires `"family": "swbf2"` so older or unknown records are treated as not prepared.
pub fn read_prepare_state_for(
    content_dir: Option<&Path>,
    family: GameFamily,
) -> Option<PrepareState> {
    if family == GameFamily::Swbf1 {
        return read_prepare_state(content_dir);
    }
    let dir = family_dir(content_dir?, family);
    let raw = fs::read_to_string(dir.join(prepare_state_file(family))).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    if value.get("family").and_then(Value::as_str) != Some(GameFamily::Swbf2.as_str()) {
        return None;
    }
    serde_json::from_value(value).ok()
}

/// Normalizes a filesystem path for cross-platform comparison in `prepared_from`.
///
/// On Windows and macOS, paths are compared case-insensitively after resolution.
/// Mirrors `comparablePath` in the desktop host.
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

/// Returns the expected `_LVL_PC` / `_lvl_pc` path inside `game_dir` for `family`.
///
/// For `GameFamily::Swbf1`, keeps the canonical `GameData/Data/_LVL_PC` path
/// that `tools/zero_paths.py` and released builds write into `.prepare.json`,
/// preserving the released `prepared_from` comparison on case-sensitive disks.
pub fn game_lvl_pc_path(game_dir: &Path, family: GameFamily) -> PathBuf {
    match family {
        GameFamily::Swbf1 => game_dir.join("GameData").join("Data").join("_LVL_PC"),
        GameFamily::Swbf2 => find_swbf2_lvl_pc(game_dir)
            .unwrap_or_else(|| game_dir.join("GameData").join("data").join("_lvl_pc")),
    }
}

/// Checks whether `state` was prepared from the given 2004 (`swbf1`) `game`
/// installation root.
///
/// Compares `state.game` against `<game>/GameData/Data/_LVL_PC`.
/// Mirrors `preparedFrom` in the desktop host.
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

/// Checks whether `state` was prepared for `family` from the given `game`
/// installation root.
pub fn prepared_from_for(
    state: Option<&PrepareState>,
    game: Option<&Path>,
    family: GameFamily,
    platform: &str,
) -> bool {
    if family == GameFamily::Swbf1 {
        return prepared_from(state, game, platform);
    }
    let (Some(st), Some(game_dir)) = (state, game) else {
        return false;
    };
    let Some(state_game) = st.game.as_deref().filter(|s| !s.is_empty()) else {
        return false;
    };
    let expected = game_lvl_pc_path(game_dir, family);
    comparable_path_swbf2(Path::new(state_game), platform)
        == comparable_path_swbf2(&expected, platform)
}

/// [`comparable_path`] with a Windows verbatim spelling (`\\?\C:\...`,
/// `\\?\UNC\...`, as `canonicalize` writes it) read as the plain place it
/// names. Only the 2005 comparison does this; the released 2004 comparison
/// above keeps its exact released reading. A folder that exists is read
/// through its links (macOS spells a temporary folder `/var/...` where the
/// preparation records `/private/var/...`).
fn comparable_path_swbf2(p: &Path, platform: &str) -> String {
    let real = fs::canonicalize(p);
    strip_verbatim(&comparable_path(real.as_deref().unwrap_or(p), platform))
}

fn strip_verbatim(text: &str) -> String {
    let Some(rest) = text.strip_prefix("//?/") else {
        return text.to_string();
    };
    if rest.as_bytes().get(..4).is_some_and(|b| b.eq_ignore_ascii_case(b"unc/")) {
        format!("//{}", &rest[4..])
    } else {
        rest.to_string()
    }
}

/// Desired optional features when checking if prepared content satisfies user settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WantedPrepareOptions {
    /// Whether enhanced sound is required (`true` rejects content prepared with `enhancedSound: false`).
    pub enhanced_sound: Option<bool>,
}

/// Checks whether `state` is current, prepared for `family` from `game`,
/// includes the same sorted `add_ons` list, and satisfies `wanted` options.
pub fn prepared_for_family(
    state: Option<&PrepareState>,
    game: Option<&Path>,
    family: GameFamily,
    add_ons: &[String],
    wanted: &WantedPrepareOptions,
    platform: &str,
    content_version: &ContentVersion,
) -> bool {
    if !prepared_content_current(state, content_version)
        || !prepared_from_for(state, game, family, platform)
    {
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

/// Checks whether `state` is current, prepared from 2004 (`swbf1`) `game`,
/// includes the same sorted `add_ons` list, and satisfies `wanted` options.
///
/// Mirrors `preparedFor` in the desktop host.
pub fn prepared_for(
    state: Option<&PrepareState>,
    game: Option<&Path>,
    add_ons: &[String],
    wanted: &WantedPrepareOptions,
    platform: &str,
    content_version: &ContentVersion,
) -> bool {
    prepared_for_family(
        state,
        game,
        GameFamily::Swbf1,
        add_ons,
        wanted,
        platform,
        content_version,
    )
}

/// Summary of add-on failures and enhanced sound status extracted from `.prepare.json`.
///
/// Mirrors the return value of `preparedSummary` in the desktop host.
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
/// Mirrors `preparedSummary` in the desktop host.
pub fn prepared_summary(state: Option<&PrepareState>) -> PreparedSummary {
    let failures = state.and_then(|s| s.failures.clone()).unwrap_or_default();
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

/// Clears 2004 (`swbf1`) prepared output subdirectories (`maps`, `sides`, `common`, `audio`,
/// `.prepare.json`) inside `content` if its existing `.prepare.json` was not built from `game`.
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
/// Mirrors step objects in `snapshot.steps` in the desktop host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareStep {
    /// Step identifier (e.g. `"common"`, `"map:bes1a"`).
    #[serde(alias = "key")]
    pub id: String,
    /// Step category (`"catalog"`, `"side"`, `"common"`, `"map"`, `"audio"`, etc.).
    #[serde(default)]
    #[serde(alias = "phase")]
    pub kind: String,
    /// Human-readable step label.
    #[serde(default)]
    pub label: String,
    /// Current step status (`"pending"`, `"running"`, `"done"`, `"failed"`, `"skipped"`, etc.).
    #[serde(default = "pending_status")]
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

fn pending_status() -> String {
    "pending".into()
}

/// Live snapshot of a running or completed content preparation job.
///
/// Mirrors `createSnapshot()` in the desktop host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSnapshot {
    /// High-level phase (`"idle"`, `"starting"`, `"running"`, `"done"`, `"failed"`, `"cancelled"`).
    pub status: String,
    /// Read-only --check report, emitted before asking the player to prepare.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_report: Option<Value>,
    /// Rolling step-kind timings used by the first-run estimate.
    #[serde(default)]
    pub timings: StepTimings,
    /// Most recently started step, falling back to the first still running.
    #[serde(default)]
    pub current_step: Option<String>,
    /// Whether this run includes enhanced sound.
    #[serde(default)]
    pub enhanced_sound: bool,
    /// Game install directory path.
    pub game: Option<String>,
    /// Target content directory path.
    pub content: Option<String>,
    /// Timestamp (ms) when preparation started.
    pub started_at: Option<u64>,
    /// Timestamp (ms) when preparation finished.
    pub finished_at: Option<u64>,
    /// Number of parallel worker processes scheduled by `prepare.py`.
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
/// Mirrors `createSnapshot` in the desktop host.
pub fn create_snapshot() -> PrepareSnapshot {
    PrepareSnapshot {
        status: "idle".to_string(),
        check_report: None,
        timings: StepTimings::new(),
        current_step: None,
        enhanced_sound: true,
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

/// Appends one line to the run's log file, when there is one.
fn log_line(log: &Option<Arc<Mutex<fs::File>>>, line: &str) {
    if let Some(file) = log {
        if let Ok(mut file) = file.lock() {
            use std::io::Write;
            let _ = writeln!(file, "{}", line.trim_end());
        }
    }
}

/// Appends a trimmed log line to `snapshot.logs`, retaining at most the last 160 lines.
///
/// Mirrors `pushLog` in the desktop host.
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
/// Mirrors `recount` in the desktop host.
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

/// Applies a JSON event emitted by `prepare.py` to a [`PrepareSnapshot`].
///
/// Handles `"log"`, `"plan"`, `"step"`, `"playable"`, `"finished"`, `"busy"`,
/// and `"error"` events identically to `applyEvent` in the desktop host.
pub fn apply_event(snapshot: &mut PrepareSnapshot, event: &Value, now_ms: u64) {
    let Some(event_type) = event.get("event").and_then(|v| v.as_str()) else {
        return;
    };

    match event_type {
        "check" => {
            snapshot.check_report = Some(event.clone());
            snapshot.status = "done".into();
        }
        // Append diagnostic log lines emitted by the Python preparer.
        "log" => {
            let msg = event
                .get("line")
                .or_else(|| event.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
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
                let old = std::mem::take(&mut snapshot.steps);
                snapshot.steps = arr
                    .iter()
                    .filter_map(|item| {
                        let mut step: PrepareStep = serde_json::from_value(item.clone()).ok()?;
                        if let Some(previous) = old.iter().find(|p| p.id == step.id) {
                            step.status = previous.status.clone();
                            step.started_at = previous.started_at;
                            step.seconds = previous.seconds;
                            step.cached = previous.cached;
                        } else if item["available"] == false {
                            step.status = "unavailable".into();
                        }
                        Some(step)
                    })
                    .collect();
            }
            recount(snapshot);
        }
        // Update an existing step's status/progress or insert a dynamically discovered step.
        "step" => {
            let Some(step_id) = event
                .get("key")
                .or_else(|| event.get("id"))
                .and_then(|v| v.as_str())
            else {
                return;
            };
            if let Some(parallel) = event.get("parallel").and_then(Value::as_u64) {
                snapshot.workers = parallel.max(1) as u32;
            }
            let status = event.get("status").and_then(|v| v.as_str()).unwrap_or("");
            let new_status = if status == "start" { "running" } else { status }.to_string();
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
                .get("phase")
                .or_else(|| event.get("kind"))
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

            if new_status == "running" {
                snapshot.current_step = Some(step_id.into());
            } else if snapshot.current_step.as_deref() == Some(step_id) {
                snapshot.current_step = snapshot
                    .steps
                    .iter()
                    .find(|s| s.status == "running")
                    .map(|s| s.id.clone());
            }
            if let Some(seconds) = seconds {
                if let Some(step) = snapshot.steps.iter().find(|s| s.id == step_id) {
                    let mut timed = step.clone();
                    timed.kind = step.id.split(':').next().unwrap_or("").into();
                    snapshot.timings = record_timing(&snapshot.timings, &timed, seconds);
                }
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
                .unwrap_or_else(|| {
                    event
                        .get("failed")
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty)
                });
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
/// Mirrors `expectedSeconds` in the desktop host.
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
/// Mirrors `expectedSeconds(step, timings)` in the desktop host.
pub fn expected_seconds(step: &PrepareStep, timings: Option<&StepTimings>) -> f64 {
    let key = step.id.split(':').next().unwrap_or("");
    let kind = if matches!(
        key,
        "catalog"
            | "catalog-update"
            | "side"
            | "common"
            | "shell"
            | "map"
            | "environment"
            | "hud"
            | "audio"
            | "textures"
    ) {
        key
    } else {
        &step.kind
    };
    expected_seconds_for_kind(kind, timings)
}

/// Records a completed, non-cached step duration into the rolling timing history
/// (retaining at most the last 8 samples per step kind).
///
/// Mirrors `recordTiming` in the desktop host.
pub fn record_timing(timings: &StepTimings, step: &PrepareStep, seconds: f64) -> StepTimings {
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
        let left = by_progress.unwrap_or(full - elapsed).max(full * 0.08);
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
    let eta = (serial_remaining + map_remaining / workers)
        .round()
        .max(1.0) as u64;

    let phase_label = if running.len() == 1 {
        format!("Preparing {}", running[0].label)
    } else if running.len() > 1 {
        let preview: Vec<&str> = running.iter().take(2).map(|s| s.label.as_str()).collect();
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
/// Mirrors `planEstimate` in the desktop host.
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
        + kind_sec("catalog-update");
    let parallel = (counts.maps as f64 * kind_sec("map")) / (workers.max(1) as f64);
    PlanEstimateResult {
        total: (serial + parallel).round().max(1.0) as u64,
        sound: sound.round().max(0.0) as u64,
    }
}

/// Startup content resolution mode returned by [`resolve_startup_content`].
///
/// Mirrors the `mode` field returned by `startupContent` in the desktop host.
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
/// Mirrors the return value of `startupContent` in the desktop host.
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
    let family = GameFamily::Swbf1;
    let explicit_content: Vec<PathBuf> = saved
        .content_dir
        .as_ref()
        .or(saved.chosen_content_dir.as_ref())
        .map(|s| vec![PathBuf::from(s)])
        .unwrap_or_default();
    let extra_game: Vec<PathBuf> = saved
        .game_dir_for(family)
        .map(|s| vec![PathBuf::from(s)])
        .unwrap_or_default();

    let raw_content = find_content_for(
        &FindContentOptions {
            explicit: explicit_content,
            likely: likely_content_dirs.to_vec(),
            home: home.map(|p| p.to_path_buf()),
            search: true,
        },
        family,
    );

    let prepare_state = raw_content
        .as_ref()
        .and_then(|c| read_prepare_state_for(Some(&c.dir), family));
    let current =
        raw_content.is_some() && prepared_content_current(prepare_state.as_ref(), &content_version);
    let content = if current { raw_content.clone() } else { None };
    let stale_content = if raw_content.is_some() && !current {
        raw_content
    } else {
        None
    };

    let installs = find_game_installs_for(
        &FindGameOptions {
            platform: platform.to_string(),
            home: home.map(|p| p.to_path_buf()),
            env: env.clone(),
            extra: extra_game,
        },
        family,
    );

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
    let preps = saved.preparations_for(family);
    let prepared_game_dir = preps
        .values()
        .next()
        .map(|p: &PreparationRecord| PathBuf::from(&p.game_dir))
        .or_else(|| saved.game_dir_for(family).map(PathBuf::from));

    if let Some(cand) = prepared_game_dir {
        if let Some(valid) = inspect_install_for(&cand, family) {
            if installs.len() <= 1 {
                let matched = installs.first().cloned().unwrap_or(GameInstall {
                    dir: valid.dir.clone(),
                    family: valid.family,
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

/// Options for spawning a live `prepare.py` subprocess via [`PrepareSession::start`].
#[derive(Debug, Clone)]
pub struct PrepareSpawnOptions {
    /// Game family (`swbf1` or `swbf2`) to prepare.
    pub family: GameFamily,
    /// Resolved toolchain and script paths.
    pub toolchain: ToolchainPaths,
    /// Original game installation directory.
    pub game_dir: PathBuf,
    /// Shared content root directory (`BFE_CONTENT_DIR` is `<content_dir>` for
    /// `swbf1`, or `<content_dir>/swbf2/.content` for `swbf2`).
    pub content_dir: PathBuf,
    /// Base environment variables.
    pub env: BTreeMap<String, String>,
    /// Target platform (`"linux"`, `"darwin"`, `"win32"`).
    pub platform: String,
    /// Whether to include discovered add-on campaigns (`BFE_SKIP_ADDONS` when `false`).
    pub include_add_ons: bool,
    /// Whether to build enhanced sound (`--no-enhanced-sound` when `false`).
    pub enhanced_sound: bool,
    /// Whether to run the read-only `--check` preflight.
    pub plan_only: bool,
}

/// Handle to a running `prepare.py` child process and its live [`PrepareSnapshot`].
pub struct PrepareSession {
    /// Shared live snapshot updated as JSON lines arrive from stdout.
    pub snapshot: Arc<Mutex<PrepareSnapshot>>,
    /// Receiver that yields a cloned [`PrepareSnapshot`] after each event.
    pub events: Receiver<PrepareSnapshot>,
    child: Arc<Mutex<Option<Child>>>,
}

impl PrepareSession {
    /// Spawns `python prepare.py` with the configured arguments and begins
    /// streaming JSON progress events on a background reader thread.
    ///
    /// First verifies that `<toolchain.tools>/content-version.json` is present and
    /// valid via [`read_content_version`].
    pub fn start(opts: PrepareSpawnOptions) -> Result<Self, String> {
        Self::start_with_timings(opts, None, None)
    }

    /// Load and save measured tool durations alongside the desktop profile, and
    /// write the run's output to `log_file` (`logs/prepare.log`, replaced each run)
    /// so a player can send it when preparation fails.
    pub fn start_with_timings(
        opts: PrepareSpawnOptions,
        timings_file: Option<PathBuf>,
        log_file: Option<PathBuf>,
    ) -> Result<Self, String> {
        Self::spawn(opts, timings_file, log_file, false)
    }

    /// Check prepared audio metadata after the menu opens. The optional steps use prepared banks.
    pub fn start_sound_maintenance(opts: PrepareSpawnOptions, log_file: PathBuf) -> Result<Self, String> {
        Self::spawn(opts, None, Some(log_file), true)
    }

    fn spawn(opts: PrepareSpawnOptions, timings_file: Option<PathBuf>, log_file: Option<PathBuf>,
             sound_maintenance: bool) -> Result<Self, String> {
        // Validate the shared content-version.json before launching the Python preparer.
        let _version = read_content_version(&opts.toolchain.tools)?;

        let mut child_game = opts.game_dir.clone();
        let mut swbf2_lock = None;
        let mut checked_dir = None;
        match opts.family {
            GameFamily::Swbf1 => {
                if !sound_maintenance && inspect_install_for(&opts.game_dir, opts.family).is_none() {
                    return Err(format!(
                        "That folder does not look like a {} install.",
                        game_profile(opts.family).display_name
                    ));
                }
                if !opts.plan_only && !sound_maintenance {
                    clear_prepared_for(Some(&opts.content_dir), Some(&opts.game_dir), &opts.platform);
                }
            }
            // Sound maintenance of a 2005 tree validates no install, but it takes the tree's lock like every other writer.
            GameFamily::Swbf2 if sound_maintenance => {
                let tools = swbf2_tree::swbf2_tools(Some(&opts.toolchain.tools))?;
                let session = swbf2_tree::complete_session(&opts.content_dir, &tools)?;
                checked_dir = Some(session.tree.dir);
                swbf2_lock = Some(session.lock);
            }
            GameFamily::Swbf2 => {
                let install = swbf2_tree::identify_install(&opts.game_dir)?;
                child_game = install.root.clone();
                if opts.plan_only {
                    checked_dir = Some(swbf2_tree::locate(&opts.content_dir)?.dir);
                } else {
                    let tools = swbf2_tree::swbf2_tools(Some(&opts.toolchain.tools))?;
                    let start = swbf2_tree::begin_identified(&swbf2_tree::Ops::REAL, &opts.content_dir, install, &tools)
                        .map_err(|e| e.message().to_string())?;
                    checked_dir = Some(start.tree.dir.clone());
                    swbf2_lock = Some(start.lock);
                }
            }
        }

        let mut child_env = tool_env(&ToolEnvOptions {
            family: opts.family,
            toolchain: opts.toolchain.toolchain.clone(),
            game: Some(child_game),
            content: Some(opts.content_dir.clone()),
            env: opts.env.clone(),
            platform: opts.platform.clone(),
        });
        let is_win = opts.platform == "win32" || opts.platform == "windows";
        let mut held_lock = None;
        if let Some(path) = checked_dir {
            let dir = path.to_str().ok_or("The 2005 data folder cannot be passed to the tools.")?;
            set_var(&mut child_env, is_win, "BFE_CONTENT_DIR", dir.to_string());
        }
        if let Some(lock) = swbf2_lock {
            set_var(
                &mut child_env,
                is_win,
                swbf2_tree::LOCK_ENV,
                swbf2_tree::LOCK_HANDOVER.to_string(),
            );
            lock.release_tools_part()
                .map_err(|e| format!("Could not pass the preparation lock on: {e}"))?;
            held_lock = Some(lock);
        }

        eprintln!(
            "preparation tools: python={} script={} toolchain={}",
            opts.toolchain.python.display(),
            opts.toolchain.script.display(),
            opts.toolchain.toolchain.display()
        );
        let mut cmd = Command::new(&opts.toolchain.python);
        // prepare.py reads its input/output roots from tool_env and always
        // emits @@-framed progress. Its only read-only mode is --check.
        cmd.arg("-u").arg(&opts.toolchain.script).arg("--jobs").arg(
            thread::available_parallelism()
                .map_or(1, usize::from)
                .to_string(),
        );
        if sound_maintenance {
            cmd.arg("--maintain-sound");
        } else if opts.plan_only {
            cmd.arg("--check");
        }
        if !opts.enhanced_sound {
            cmd.arg("--no-enhanced-sound");
        }
        cmd.env_clear();
        for (k, v) in &child_env {
            cmd.env(k, v);
        }
        if !opts.include_add_ons {
            cmd.env("BFE_SKIP_ADDONS", "1");
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(lock) = &held_lock {
            let handed = lock
                .file()
                .try_clone()
                .map_err(|e| format!("Could not pass the preparation lock on: {e}"))?;
            cmd.stdin(Stdio::from(handed));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        // The client is a windowed program: without this every Python process would open a console.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let log = log_file.and_then(|path| {
            fs::create_dir_all(path.parent()?).ok()?;
            fs::File::create(path).ok().map(|file| Arc::new(Mutex::new(file)))
        });
        log_line(
            &log,
            &format!(
                "prepare.py from {} into {} (enhanced sound {}, check only {})",
                opts.game_dir.display(),
                opts.content_dir.display(),
                opts.enhanced_sound,
                opts.plan_only
            ),
        );
        let mut child = cmd.spawn().map_err(|e| {
            let message = format!("Failed to spawn prepare.py: {}", e);
            log_line(&log, &message);
            message
        })?;

        let stderr = child
            .stderr
            .take()
            .ok_or("Failed to capture preparation stderr")?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture prepare.py stdout".to_string())?;

        let mut initial = create_snapshot();
        initial.status = "starting".to_string();
        initial.enhanced_sound = opts.enhanced_sound;
        initial.timings = timings_file
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        initial.started_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
        );
        initial.game = Some(opts.game_dir.to_string_lossy().into_owned());
        initial.content = Some(opts.content_dir.to_string_lossy().into_owned());

        let snapshot = Arc::new(Mutex::new(initial));
        let child_arc = Arc::new(Mutex::new(Some(child)));
        let (tx, rx) = mpsc::channel();

        let stderr_snapshot = Arc::clone(&snapshot);
        let stderr_log = log.clone();
        let stderr_reader = thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                log_line(&stderr_log, &line);
                if let Ok(mut snap) = stderr_snapshot.lock() {
                    push_log(&mut snap, &line);
                }
            }
        });
        let snap_clone = Arc::clone(&snapshot);
        let child_wait = Arc::clone(&child_arc);
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
                    if let Ok(val) =
                        serde_json::from_str::<Value>(trimmed.strip_prefix("@@").unwrap_or(trimmed))
                    {
                        // Steps, errors and the end go in the log; the frequent busy ticks and the plan do not.
                        if !matches!(val["event"].as_str(), Some("busy" | "plan")) {
                            log_line(&log, trimmed);
                        }
                        apply_event(&mut guard, &val, now_ms);
                    } else {
                        log_line(&log, trimmed);
                        push_log(&mut guard, trimmed);
                    }
                    if let Some(path) = timings_file.as_ref() {
                        if let Ok(body) = serde_json::to_vec(&guard.timings) {
                            let _ = bfe_storage::write_text_record(path, &body, |text| {
                                serde_json::from_str::<StepTimings>(text).is_ok()
                            });
                        }
                    }
                    let est = estimate(&guard, Some(&guard.timings), now_ms);
                    guard.eta_seconds = est.eta_seconds;
                    guard.phase_label = est.phase_label;
                    // The menu releases its maintenance session on completion. Send that
                    // notification after wait() below, so dropping it cannot kill the worker.
                    if !sound_maintenance || !matches!(guard.status.as_str(), "done" | "failed" | "cancelled") {
                        let _ = tx.send(guard.clone());
                    }
                }
            }
            // EOF alone is not success: a missing script, import error or
            // crashed worker may never emit a terminal progress event.
            let result = child_wait
                .lock()
                .ok()
                .and_then(|mut c| c.as_mut().map(Child::wait));
            // The tools have exited, and with them their last write: the 2005 tree's lock goes.
            drop(held_lock);
            // Preserve the final diagnostic lines before publishing failure.
            let _ = stderr_reader.join();
            if let Ok(mut snap) = snap_clone.lock() {
                if let Some(Ok(status)) = result {
                    if !status.success()
                        && snap.check_report.is_none()
                        && snap.status != "failed"
                        && snap.status != "cancelled"
                    {
                        snap.status = "failed".into();
                        snap.error = Some(format!(
                            "Preparation failed (exit {}). See the log.",
                            status
                                .code()
                                .map_or_else(|| "signal".into(), |n| n.to_string())
                        ));
                    } else if status.success() && snap.status != "done" && snap.status != "failed" {
                        snap.status = "failed".into();
                        snap.error = Some(
                            "Preparation ended without a completion event. See the log.".into(),
                        );
                    }
                }
                log_line(&log, &format!("finished: {}", snap.error.as_deref().unwrap_or(&snap.status)));
                let _ = tx.send(snap.clone());
            }
        });

        Ok(Self {
            snapshot,
            events: rx,
            child: child_arc,
        })
    }

    /// Terminates the running `prepare.py` process if it is still active.
    pub fn cancel(&self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                if !matches!(child.try_wait(), Ok(None)) {
                    return;
                }
                #[cfg(unix)]
                // SAFETY: this child started its own process group; its positive
                // PID identifies only the preparation tree owned by this session.
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGTERM);
                }
                let _ = child.kill();
            }
        }
        if let Ok(mut snap) = self.snapshot.lock() {
            snap.status = "cancelled".to_string();
            snap.error = Some("Preparation cancelled.".to_string());
        }
    }
}

/// Inspects a content folder after preparation for `family` and verifies it: for 2004 with
/// [`verify_prepared_outputs`], for 2005 with the test preparation and export use
/// ([`swbf2_tree::verify`]).
pub fn verify_prepared_outputs_for(
    content_dir: &Path,
    game_dir: &Path,
    family: GameFamily,
    platform: &str,
    tools_dir: Option<&Path>,
) -> Result<bool, String> {
    if family == GameFamily::Swbf1 {
        return verify_prepared_outputs(content_dir, game_dir, platform, tools_dir);
    }
    // Tools that cannot prepare the 2005 game cannot vouch for a 2005 tree either.
    match swbf2_tree::swbf2_tools(tools_dir) {
        Ok(tools) => swbf2_tree::verify(content_dir, game_dir, &tools),
        Err(_) => Ok(false),
    }
}

/// Inspects a 2004 (`swbf1`) content folder after preparation and verifies that its
/// `.prepare.json` matches `tools/content-version.json` and the installed add-ons from `game_dir`.
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

#[cfg(test)]
mod tool_protocol_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verbatim_spellings_read_as_the_place_they_name() {
        assert_eq!(strip_verbatim("//?/D:/zz/GameData/data/_lvl_pc"), "D:/zz/GameData/data/_lvl_pc");
        assert_eq!(strip_verbatim("//?/UNC/server/share/zz"), "//server/share/zz");
        assert_eq!(strip_verbatim("//?/unc/server/share/zz"), "//server/share/zz");
        assert_eq!(strip_verbatim("D:/zz/GameData/data/_lvl_pc"), "D:/zz/GameData/data/_lvl_pc");
        assert_eq!(strip_verbatim("/zz/GameData/data/_lvl_pc"), "/zz/GameData/data/_lvl_pc");
        assert_eq!(strip_verbatim("//server/share/zz"), "//server/share/zz");
        // Folder names past ASCII, one straddling the fourth byte.
        assert_eq!(strip_verbatim("//?/ab\u{4e2d}/zz"), "ab\u{4e2d}/zz");
        assert_eq!(strip_verbatim("//?/a\u{e9}b/zz"), "a\u{e9}b/zz");
        assert_eq!(strip_verbatim("//?/\u{1f600}zz"), "\u{1f600}zz");
        assert_eq!(strip_verbatim("//?/UNC/\u{4e2d}\u{6587}/share/zz"), "//\u{4e2d}\u{6587}/share/zz");
        assert_eq!(strip_verbatim("//?/"), "");
        assert_eq!(strip_verbatim("//?/un"), "un");
    }

    #[test]
    fn actual_tool_plan_and_keyed_step_events_advance_progress() {
        let mut snap = create_snapshot();
        apply_event(
            &mut snap,
            &json!({"event":"plan","steps":[
                {"key":"map:nab2","phase":"maps","label":"Naboo","weight":120,"available":true}
            ]}),
            10,
        );
        assert_eq!(snap.steps[0].id, "map:nab2");
        apply_event(
            &mut snap,
            &json!({"event":"step","key":"map:nab2","phase":"maps","label":"Naboo","status":"start"}),
            20,
        );
        assert_eq!(snap.steps[0].status, "running");
        assert_eq!(snap.steps[0].started_at, Some(20));
        apply_event(
            &mut snap,
            &json!({"event":"step","key":"map:nab2","status":"done","seconds":1.5}),
            30,
        );
        assert_eq!(snap.done, 1);
        assert_eq!(snap.percent, 1.);
    }

    #[test]
    fn finished_uses_the_tools_failure_list() {
        let mut snap = create_snapshot();
        apply_event(&mut snap, &json!({"event":"finished","failed":[]}), 10);
        assert!(snap.complete);
        assert_eq!(snap.status, "done");
        apply_event(
            &mut snap,
            &json!({"event":"finished","failed":["common"]}),
            20,
        );
        assert!(!snap.complete);
        assert_eq!(snap.status, "failed");
    }

    #[test]
    fn a_check_report_never_marks_content_prepared() {
        let mut snap = create_snapshot();
        apply_event(
            &mut snap,
            &json!({"event":"check","ok":false,"problems":[{"code":"program","message":"Missing program"}]}),
            10,
        );
        assert_eq!(snap.status, "done");
        assert!(!snap.complete);
        assert!(!snap.playable);
        assert_eq!(
            snap.check_report.unwrap()["problems"][0]["message"],
            "Missing program"
        );
    }
}

impl Drop for PrepareSession {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod child_protocol_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn fixture() -> (PathBuf, PrepareSpawnOptions) {
        let root = std::env::temp_dir().join(format!(
            "bfe-prepare-child-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let game = root.join("game");
        let tools = root.join("tools");
        let content = root.join("content");
        fs::create_dir_all(game.join("GameData/Data/_LVL_PC/SIDE")).unwrap();
        fs::create_dir_all(game.join("GameData/Data/_LVL_PC/Common/ENG")).unwrap();
        fs::write(
            game.join("GameData/Data/_LVL_PC/Common/ENG/common.lvl"),
            b"fixture",
        )
        .unwrap();
        fs::create_dir_all(&tools).unwrap();
        fs::write(
            tools.join("content-version.json"),
            include_str!("../../../tools/content-version.json"),
        )
        .unwrap();
        let options = PrepareSpawnOptions {
            family: GameFamily::Swbf1,
            toolchain: ToolchainPaths {
                toolchain: root.join("toolchain"),
                tools: tools.clone(),
                python: if cfg!(windows) {
                    "python".into()
                } else {
                    "python3".into()
                },
                script: tools.join("prepare.py"),
                content_version_file: tools.join("content-version.json"),
            },
            game_dir: game,
            content_dir: content,
            env: std::env::vars().collect(),
            platform: if cfg!(windows) {
                "win32".into()
            } else {
                "linux".into()
            },
            include_add_ons: true,
            enhanced_sound: true,
            plan_only: false,
        };
        (root, options)
    }
    fn terminal(session: &PrepareSession) -> PrepareSnapshot {
        // The child is an interpreter: on a loaded machine its first event can take seconds to arrive.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let snap = session
                .events
                .recv_timeout(left)
                .expect("preparer must publish progress or an exit failure before the deadline");
            if snap.status == "done" || snap.status == "failed" {
                return snap;
            }
        }
    }

    #[test]
    fn sound_maintenance_finishes_before_the_menu_releases_its_session() {
        let (root, options) = fixture();
        fs::create_dir_all(&options.content_dir).unwrap();
        let closed = options.content_dir.join("closed");
        fs::write(&options.toolchain.script, r#"import os, pathlib, time
print('@@{"event":"finished","failed":[]}', flush=True)
time.sleep(0.25)
pathlib.Path(os.environ['BFE_CONTENT_DIR'], 'closed').touch()
"#).unwrap();
        let log = root.join("sound.log");
        let session = PrepareSession::start_sound_maintenance(options, log.clone()).unwrap();
        assert_eq!(terminal(&session).status, "done");
        drop(session);
        assert!(closed.is_file());
        assert!(fs::read_to_string(log).unwrap().contains("finished: done"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sound_maintenance_runs_for_portable_content_without_clearing_it() {
        let (root, mut options) = fixture();
        options.game_dir = root.join("absent");
        options.env.insert("BFE_FAMILY".into(), "swbf2".into());
        fs::create_dir_all(&options.content_dir).unwrap();
        let marker = options.content_dir.join("keep");
        fs::write(&marker, "prepared").unwrap();
        fs::write(&options.toolchain.script, r#"import os, sys
assert '--maintain-sound' in sys.argv and '--check' not in sys.argv
assert os.environ['BFE_FAMILY'] == 'swbf1'
print('@@{"event":"finished","failed":[]}', flush=True)
"#).unwrap();
        let session = PrepareSession::start_sound_maintenance(options, root.join("sound.log")).unwrap();
        assert_eq!(terminal(&session).status, "done");
        assert_eq!(fs::read_to_string(marker).unwrap(), "prepared");
        drop(session);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn real_child_arguments_environment_and_framed_output_reach_the_snapshot() {
        let (root, mut options) = fixture();
        options.env.insert("BFE_FAMILY".to_string(), "swbf2".to_string());
        fs::write(&options.toolchain.script,r#"import argparse,json,os
p=argparse.ArgumentParser();p.add_argument('--jobs');a=p.parse_args()
assert os.environ['BFE_FAMILY'] == 'swbf1'
assert os.environ['BFE_GAME_DIR'].endswith('game')
assert os.path.basename(os.environ['BFE_CONTENT_DIR']) == 'content'
for event in [{'event':'plan','steps':[{'key':'map:nab2','phase':'maps','label':'Naboo','weight':120,'available':True}]},{'event':'step','key':'map:nab2','status':'start'},{'event':'step','key':'map:nab2','status':'done'},{'event':'finished','complete':True,'playable':True}]:
 print('@@'+json.dumps(event),flush=True)
"#).unwrap();
        let log = root.join("logs/prepare.log");
        let session = PrepareSession::start_with_timings(options, None, Some(log.clone())).unwrap();
        let snap = terminal(&session);
        assert_eq!(snap.status, "done");
        assert!(snap.complete);
        assert_eq!(snap.done, 1);
        drop(session);
        // The steps are in the log, the plan is not.
        let text = fs::read_to_string(&log).unwrap();
        assert!(text.contains(r#""key": "map:nab2", "status": "done""#) && !text.contains(r#""event": "plan""#), "{text}");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unreadable_timing_history_survives_preparation() {
        let (root, options) = fixture();
        let file = root.join("prepare-timings.json");
        let damaged = br#"{"copy":"yesterday"}"#;
        fs::write(&file, damaged).unwrap();
        fs::write(&options.toolchain.script, r#"print('@@{"event":"finished","complete":true}')"#).unwrap();
        let session = PrepareSession::start_with_timings(options, Some(file.clone()), None).unwrap();
        assert_eq!(terminal(&session).status, "done");
        drop(session);
        assert!(serde_json::from_slice::<StepTimings>(&fs::read(&file).unwrap()).is_ok());
        assert_eq!(fs::read(file.with_added_extension("bak")).ok().as_deref(), Some(&damaged[..]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_nonzero_child_without_events_becomes_a_terminal_failure() {
        let (root, options) = fixture();
        // The Python process starts, but its missing script can emit no events.
        let log = root.join("logs/prepare.log");
        let session = PrepareSession::start_with_timings(options, None, Some(log.clone())).unwrap();
        let snap = terminal(&session);
        assert_eq!(snap.status, "failed");
        assert!(snap.error.unwrap().contains("Preparation failed (exit"));
        drop(session);
        // The player can send the log: Python's own complaint and how the run ended are in it.
        let text = fs::read_to_string(&log).unwrap();
        assert!(text.contains("prepare.py") && text.contains("finished: Preparation failed (exit"), "{text}");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn a_read_only_check_preserves_existing_outputs_and_accepts_a_failed_check_report() {
        let (root, mut options) = fixture();
        options.plan_only = true;
        fs::create_dir_all(options.content_dir.join("common")).unwrap();
        let kept = options.content_dir.join("common/kept.json");
        fs::write(&kept, b"keep").unwrap();
        fs::write(&options.toolchain.script,r#"import argparse,json,sys
p=argparse.ArgumentParser();p.add_argument('--jobs');p.add_argument('--check',action='store_true');a=p.parse_args();assert a.check
print('@@'+json.dumps({'event':'check','ok':False,'problems':[{'code':'program','message':'Synthetic program'}]}),flush=True)
sys.exit(1)
"#).unwrap();
        let session = PrepareSession::start(options).unwrap();
        let snap = terminal(&session);
        assert_eq!(snap.status, "done");
        assert!(!snap.complete);
        assert_eq!(snap.check_report.unwrap()["ok"], false);
        assert_eq!(fs::read(&kept).unwrap(), b"keep");
        drop(session);
        fs::remove_dir_all(root).unwrap();
    }

    /// Publishes, in the fixture's tools folder, the 2005 generation and as many of the keys a
    /// 2005 start needs as asked for (`swbf2Lock`, `swbf2Outputs`).
    fn swbf2_tools_file(tools: &Path, handover: bool, inventory: bool) {
        let mut value = serde_json::to_value(checkout_content_version().unwrap()).unwrap();
        value["swbf2"] = Value::from(8u64);
        if handover {
            value["swbf2Lock"] = Value::from(swbf2_tree::LOCK_HANDOVER);
        }
        if inventory {
            value["swbf2Outputs"] = Value::from(1u64);
        }
        fs::write(tools.join("content-version.json"), value.to_string()).unwrap();
    }

    /// Every entry under `dir` with a file's bytes (`None` for a folder), for comparing all
    /// names and bytes.
    fn everything(dir: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn walk(base: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            let Ok(entries) = fs::read_dir(at) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let rel = path.strip_prefix(base).unwrap().to_path_buf();
                if path.is_dir() && !path.is_symlink() {
                    out.insert(rel, None);
                    walk(base, &path, out);
                } else {
                    out.insert(rel, fs::read(&path).ok());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    /// A made-up 2005 install the validators accept.
    fn swbf2_install(at: &Path) {
        let lvl = at.join("GameData/data/_lvl_pc");
        fs::create_dir_all(lvl.join("side")).unwrap();
        fs::write(at.join("GameData/BattlefrontII.exe"), b"made up").unwrap();
        for name in ["common.lvl", "core.lvl", "mission.lvl", "ingame.lvl", "shell.lvl"] {
            fs::write(lvl.join(name), b"made up").unwrap();
        }
    }

    /// Drains the session's progress until its reader has seen the tools exit.
    fn ended(session: &PrepareSession) {
        loop {
            match session.events.recv_timeout(std::time::Duration::from_secs(30)) {
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("the tools did not end"),
            }
        }
    }

    /// Stands in for tools that adopt the client's lock: standard input must be the held lock
    /// on the folder they write, a fresh open of it must find it held, and they finish a
    /// made-up complete tree once the test lets them.
    #[cfg(unix)]
    const ADOPTING_TOOLS: &str = r#"import fcntl,json,os,sys,time
content=os.environ['BFE_CONTENT_DIR']
lock=os.path.join(content,'.prepare.lock')
assert os.environ.get('BFE_PREPARE_LOCK')=='stdin','no lock was handed over'
assert os.environ['BFE_FAMILY']=='swbf2'
assert os.path.realpath(content)==content,'the tools must get the resolved folder'
assert os.path.samestat(os.fstat(0),os.stat(lock)),'standard input is not the lock'
fcntl.flock(0,fcntl.LOCK_EX|fcntl.LOCK_NB)
other=open(lock,'a+b')
try:
    fcntl.flock(other.fileno(),fcntl.LOCK_EX|fcntl.LOCK_NB)
    sys.exit('the lock was free while the tools ran')
except BlockingIOError:
    pass
root=os.path.dirname(os.path.dirname(content))
open(os.path.join(root,'ready'),'w').close()
waited=0
while not os.path.exists(os.path.join(root,'go')):
    time.sleep(0.02)
    waited+=1
    if waited>3000:
        sys.exit('nobody said go')
version=json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)),'content-version.json')))
def put(rel,body):
    path=os.path.join(content,rel)
    os.makedirs(os.path.dirname(path),exist_ok=True)
    open(path,'w').write(body)
    return [rel,len(body.encode())]
outputs={'map:zz1':[put('maps/zz1/map.json','{"made":"up"}'),put('maps/zz1/geom.bin','mesh')],'common':[put('common/common.json','{"shared":1}')],'*':[put('audio/index.json','{}')]}
import hashlib
for name, body in [('anims.bin','keys'),('manifest.json','{"version":2,"animsData":"anims.bin","rigs":{},"textures":{}}')]:
    outputs['common'].append([*put('firstperson/'+name,body), hashlib.sha256(body.encode()).hexdigest()])
game=os.path.realpath(os.path.join(os.environ['BFE_GAME_DIR'],'GameData','data','_lvl_pc'))
record={'family':'swbf2','version':version['state'],'mapImportVersion':version['maps'],'muzzleVersion':version['sides'],'commonVersion':version['common'],'swbf2Version':version['swbf2'],'game':game,'plan':['map:zz1','common'],'done':['map:zz1','common'],'outputs':outputs,'addOns':[],'failed':[],'skipped':[],'complete':True}
put('.prepare-swbf2.tmp',json.dumps(record))
os.replace(os.path.join(content,'.prepare-swbf2.tmp'),os.path.join(content,'.prepare-swbf2.json'))
print('@@'+json.dumps({'event':'finished','complete':True}),flush=True)
"#;

    #[cfg(unix)]
    #[test]
    fn a_2005_preparation_holds_the_tools_lock_from_its_checks_to_their_last_write() {
        let (root, mut options) = fixture();
        let game = root.join("game-2005");
        swbf2_install(&game);
        let content = root.join("content");
        fs::create_dir_all(&content).unwrap();
        let alias = root.join("content-alias");
        std::os::unix::fs::symlink(&content, &alias).unwrap();
        options.family = GameFamily::Swbf2;
        options.game_dir = game.clone();
        options.content_dir = alias.clone();
        swbf2_tools_file(&options.toolchain.tools, true, true);
        fs::write(&options.toolchain.script, ADOPTING_TOOLS).unwrap();

        let session = PrepareSession::start(options.clone()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !content.join("ready").exists() {
            assert!(std::time::Instant::now() < deadline, "the tools did not start");
            thread::sleep(std::time::Duration::from_millis(20));
        }
        // While the tools run, a second start, for this copy or another, is busy and changes
        // no name or byte.
        let tree = fs::canonicalize(&content).unwrap().join("swbf2/.content");
        let other = root.join("game-2005-other");
        swbf2_install(&other);
        let before = everything(&content);
        let mut seconds = Vec::new();
        for from in [game.clone(), other] {
            let mut second = options.clone();
            second.game_dir = from;
            seconds.push((PrepareSession::start(second), everything(&content)));
        }
        // Let the tools finish before judging, so a failure here cannot leave them waiting.
        fs::write(content.join("go"), b"").unwrap();
        for (second, after) in seconds {
            match second {
                Err(error) => assert!(error.contains("already running"), "{error}"),
                Ok(session) => {
                    session.cancel();
                    panic!("a second start must fail");
                }
            }
            assert_eq!(after, before);
        }
        let snap = terminal(&session);
        assert_eq!(snap.status, "done", "{:?} {:?}", snap.error, snap.logs);
        ended(&session);
        // The lock went with the tools, and what they made verifies.
        let lock = fs::OpenOptions::new()
            .read(true)
            .append(true)
            .open(tree.join(".prepare.lock"))
            .unwrap();
        // A process another test starts can hold a copy of the lock for a moment between its
        // fork and its exec; the tools' own hold is long gone once that moment passes.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while lock.try_lock().is_err() {
            assert!(std::time::Instant::now() < deadline, "the lock outlived the tools");
            thread::sleep(std::time::Duration::from_millis(25));
        }
        drop(lock);
        let tools = swbf2_tree::swbf2_tools(Some(&options.toolchain.tools)).unwrap();
        assert!(swbf2_tree::verify(&alias, &game, &tools).unwrap());
        assert!(verify_prepared_outputs_for(&alias, &game, GameFamily::Swbf2, "linux", Some(&options.toolchain.tools)).unwrap());
        assert!(tree.join(bfe_content::layout::SWBF2_MARKER_FILE).is_file());
        fs::remove_dir_all(root).unwrap();
    }

    /// Tools that do not publish the lock handover and the inventory are never started for the
    /// 2005 game, and the start changes no name or byte of the content folder.
    #[test]
    fn tools_without_the_handover_are_refused_before_any_change() {
        let (root, mut options) = fixture();
        let game = root.join("game-2005");
        swbf2_install(&game);
        options.family = GameFamily::Swbf2;
        options.game_dir = game.clone();
        let ran = root.join("tools-ran");
        fs::write(
            &options.toolchain.script,
            format!("open({:?},'w').close()\n", ran.to_string_lossy()),
        )
        .unwrap();
        // A fresh folder, a stale tree and an unmarked tree, each with 2004 data beside it.
        let content = options.content_dir.clone();
        fs::create_dir_all(content.join("common")).unwrap();
        fs::write(content.join("common/common.json"), b"{}").unwrap();
        let tree = bfe_content::layout::family_dir(&content, GameFamily::Swbf2);
        for (label, keys) in [("generation only", (false, false)), ("lock only", (true, false)), ("inventory only", (false, true))] {
            swbf2_tools_file(&options.toolchain.tools, keys.0, keys.1);
            for state in ["none", "stale", "unmarked"] {
                let _ = fs::remove_dir_all(content.join("swbf2"));
                if state != "none" {
                    fs::create_dir_all(tree.join("maps/zz1")).unwrap();
                    fs::write(tree.join("maps/zz1/map.json"), b"{}").unwrap();
                    let game_field = if state == "stale" { "/made/up/elsewhere" } else { "/made/up/here" };
                    fs::write(
                        tree.join(bfe_content::layout::SWBF2_PREPARE_STATE_FILE),
                        format!("{{\"family\":\"swbf2\",\"game\":\"{game_field}\"}}"),
                    )
                    .unwrap();
                }
                let before = everything(&content);
                let error = PrepareSession::start(options.clone()).err().unwrap_or_default();
                assert!(error.contains("cannot prepare the 2005 game"), "{label}, {state}: {error}");
                assert_eq!(everything(&content), before, "{label}, {state}");
                assert!(!ran.exists(), "{label}, {state}: the tools ran");
            }
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn both_2005_starts_pass_the_physical_install_and_exact_destination() {
        use std::os::unix::ffi::OsStringExt;
        let (root, mut options) = fixture();
        let game = root.join("game");
        swbf2_install(&game);
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&game, &alias).unwrap();
        options.game_dir = alias;
        options.family = GameFamily::Swbf2;
        options.env.insert("EXPECTED_GAME".into(), fs::canonicalize(game).unwrap().to_str().unwrap().into());
        swbf2_tools_file(&options.toolchain.tools, true, true);
        let script = r#"import json, os, sys
assert os.environ['BFE_GAME_DIR'] == os.environ['EXPECTED_GAME']
event = {'event': 'check', 'ok': True} if '--check' in sys.argv else {'event': 'finished', 'complete': True}
print('@@' + json.dumps(event), flush=True)
"#;
        fs::write(&options.toolchain.script, script).unwrap();
        for check in [true, false] {
            options.plan_only = check;
            let session = PrepareSession::start(options.clone()).unwrap();
            let snap = terminal(&session);
            assert_eq!(snap.status, "done", "{:?} {:?}", snap.error, snap.logs);
            ended(&session);
        }
        options.content_dir = root.join(std::ffi::OsString::from_vec(vec![0xfe]));
        for check in [true, false] {
            options.plan_only = check;
            assert!(PrepareSession::start(options.clone()).is_err());
            assert!(!options.content_dir.exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// A 2005 check (`--check`) takes no lock and makes, marks or clears nothing.
    #[test]
    fn a_2005_check_takes_no_lock_and_changes_nothing() {
        let (root, mut options) = fixture();
        let game = root.join("game-2005");
        swbf2_install(&game);
        options.family = GameFamily::Swbf2;
        options.game_dir = game;
        options.plan_only = true;
        swbf2_tools_file(&options.toolchain.tools, true, true);
        fs::write(&options.toolchain.script, r#"import argparse,json,os,sys
p=argparse.ArgumentParser();p.add_argument('--jobs');p.add_argument('--check',action='store_true');p.add_argument('--no-enhanced-sound',action='store_true');a=p.parse_args();assert a.check
assert os.environ.get('BFE_PREPARE_LOCK') is None,'a check is handed no lock'
print('@@'+json.dumps({'event':'check','ok':True,'problems':[]}),flush=True)
"#).unwrap();
        let content = options.content_dir.clone();
        let tree = bfe_content::layout::family_dir(&content, GameFamily::Swbf2);
        for state in ["none", "stale"] {
            let _ = fs::remove_dir_all(&content);
            fs::create_dir_all(&content).unwrap();
            if state == "stale" {
                fs::create_dir_all(tree.join("maps/zz1")).unwrap();
                fs::write(tree.join("maps/zz1/map.json"), b"{}").unwrap();
                fs::write(tree.join(bfe_content::layout::SWBF2_PREPARE_STATE_FILE), b"{\"family\":\"swbf2\"}").unwrap();
            }
            let before = everything(&content);
            let session = PrepareSession::start(options.clone()).unwrap();
            let snap = terminal(&session);
            assert_eq!(snap.status, "done", "{state}: {:?}", snap.error);
            ended(&session);
            assert_eq!(everything(&content), before, "{state}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    /// Tools that take their own lock by name, as before the handover, find it held: they
    /// report busy, and the start's checks left the tree as they found it.
    #[cfg(unix)]
    #[test]
    fn tools_that_do_not_adopt_the_lock_find_it_held_and_touch_nothing() {
        let (root, mut options) = fixture();
        let game = root.join("game-2005");
        swbf2_install(&game);
        options.family = GameFamily::Swbf2;
        options.game_dir = game;
        swbf2_tools_file(&options.toolchain.tools, true, true);
        fs::write(&options.toolchain.script, r#"import fcntl,json,os,sys
content=os.environ['BFE_CONTENT_DIR']
handle=open(os.path.join(content,'.prepare.lock'),'a+b')
try:
    fcntl.flock(handle.fileno(),fcntl.LOCK_EX|fcntl.LOCK_NB)
except OSError:
    print('@@'+json.dumps({'event':'busy','message':'Another preparation is already running in '+content}),flush=True)
    sys.exit(3)
open(os.path.join(content,'written-without-the-lock'),'w').close()
"#).unwrap();
        let session = PrepareSession::start(options.clone()).unwrap();
        let snap = terminal(&session);
        assert_eq!(snap.status, "failed");
        assert!(snap.error.unwrap_or_default().contains("already running"));
        ended(&session);
        let tree = bfe_content::layout::family_dir(&options.content_dir, GameFamily::Swbf2);
        assert!(!tree.join("written-without-the-lock").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
