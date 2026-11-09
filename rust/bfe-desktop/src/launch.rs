//! Process launch handoff, readiness token signalling, and version settlement.
//!
//! Mirrors the desktop host (`childEnvironment`, `signalReady`,
//! `launchVersion`, `DEFAULT_READY_TIMEOUT_MS`) and the lifecycle hooks in
//! the desktop host (`readReleaseInfo`, `updater.ready()`,
//! `updater.settle()`).
//!
//! ## Readiness Handshake
//! When the bootstrap build (desktop host or native) or an older installed version
//! hands off to a newer version under `<root>/versions/<version>/`, the parent
//! creates a 32-character hex token name under `<root>/launches/<token>` and passes
//! `BFE_LAUNCH_DIR` and `BFE_LAUNCH_TOKEN` in the child's environment.
//!
//! 1. Once the child's window exists and the first frame is drawn, [`ready`]
//!    ([`signal_ready`]) writes the child's PID to `<BFE_LAUNCH_DIR>/<BFE_LAUNCH_TOKEN>`.
//!    The parent sees the file within [`READY_POLL_MS`], removes it, logs the handoff,
//!    and exits with status `0` instead of waiting out the 30-second timeout and
//!    killing the child.
//! 2. Ten seconds after window creation, [`settle`] records the running version at
//!    the head of `healthy` in `<root>/state.json`, updates `<root>/current` on Linux
//!    when running inside `<root>/versions/<version>`, and calls [`collect`] to prune
//!    superseded version directories.

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::paths::{home_dir_from_env, normalize_lexical};
use crate::update::state::{
    collect, platform_key, point_current, record_healthy, resolve_update_root, versions_dir,
    INSTALL_RECORD,
};
use crate::update::version::parse_version;

/// Poll interval in milliseconds while waiting for a child version to write its
/// readiness token file.
///
/// Mirrors `READY_POLL_MS` in the desktop host.
pub const READY_POLL_MS: u64 = 100;

/// Default maximum wait in milliseconds (`30_000`) for a launched version to
/// signal readiness before the parent kills it and records a failed attempt.
///
/// Mirrors `DEFAULT_READY_TIMEOUT_MS` in the desktop host.
pub const DEFAULT_READY_TIMEOUT_MS: u64 = 30 * 1000;

/// Default delay in milliseconds (`10_000`) after window creation before
/// [`schedule_settle`] marks the running version healthy and prunes old versions.
///
/// Mirrors `setTimeout(() => updater.settle(), 10 * 1000)` in the desktop host.
pub const DEFAULT_SETTLE_DELAY_MS: u64 = 10 * 1000;

/// Returns true when `token` matches `/^[0-9a-f]{32}$/`.
///
/// Mirrors `TOKEN_PATTERN` in the desktop host.
pub fn is_valid_launch_token(token: &str) -> bool {
    token.len() == 32
        && token
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Generates a random 16-byte (32-hex-character) readiness token.
///
/// Mirrors `randomToken` in the desktop host.
pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("getrandom failed: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Inputs to [`child_environment`].
///
/// Mirrors the parameter object of `childEnvironment` in the desktop host.
#[derive(Debug, Clone, Default)]
pub struct ChildEnvOptions {
    /// Base environment variables inherited from the parent process.
    pub base_env: BTreeMap<String, String>,
    /// Directory of the initial bootstrap installation (`BFE_BOOTSTRAP_DIR`).
    pub bootstrap_dir: Option<PathBuf>,
    /// Version of the initial bootstrap installation (`BFE_BOOTSTRAP_VERSION`).
    pub bootstrap_version: Option<String>,
    /// Directory where the child must write its readiness file (`BFE_LAUNCH_DIR`).
    pub launch_dir: Option<PathBuf>,
    /// 32-character lowercase hex readiness token (`BFE_LAUNCH_TOKEN`).
    pub token: Option<String>,
}

/// Builds the environment map for a handed-off child version.
///
/// Preserves the outermost `BFE_BOOTSTRAP_DIR`,
/// `BFE_BOOTSTRAP_VERSION`, and `BFE_APPIMAGE`, and sets `BFE_LAUNCH_DIR` and
/// `BFE_LAUNCH_TOKEN` for the immediate child hop.
///
/// Mirrors `childEnvironment` in the desktop host.
pub fn child_environment(opts: &ChildEnvOptions) -> BTreeMap<String, String> {
    let mut env = opts.base_env.clone();
    if let Some(dir) = &opts.bootstrap_dir {
        if !dir.as_os_str().is_empty()
            && env
                .get("BFE_BOOTSTRAP_DIR")
                .map(|v| v.is_empty())
                .unwrap_or(true)
        {
            env.insert(
                "BFE_BOOTSTRAP_DIR".to_string(),
                dir.to_string_lossy().into_owned(),
            );
        }
    }
    if let Some(ver) = &opts.bootstrap_version {
        if !ver.is_empty()
            && env
                .get("BFE_BOOTSTRAP_VERSION")
                .map(|v| v.is_empty())
                .unwrap_or(true)
        {
            env.insert("BFE_BOOTSTRAP_VERSION".to_string(), ver.clone());
        }
    }
    if let Some(launch_dir) = &opts.launch_dir {
        if !launch_dir.as_os_str().is_empty() {
            env.insert(
                "BFE_LAUNCH_DIR".to_string(),
                launch_dir.to_string_lossy().into_owned(),
            );
        }
    }
    if let Some(token) = &opts.token {
        if !token.is_empty() {
            env.insert("BFE_LAUNCH_TOKEN".to_string(), token.clone());
        }
    }
    if let Some(appimage) = env.get("APPIMAGE").cloned().filter(|v| !v.is_empty()) {
        if env
            .get("BFE_APPIMAGE")
            .map(|v| v.is_empty())
            .unwrap_or(true)
        {
            env.insert("BFE_APPIMAGE".to_string(), appimage);
        }
    }
    env
}

/// Writes `<BFE_LAUNCH_DIR>/<BFE_LAUNCH_TOKEN>` containing `<pid>\n` using an atomic
/// temporary file rename inside `BFE_LAUNCH_DIR`.
///
/// Returns `true` if a valid launch token was present and the file was written, or
/// `false` if no valid token/directory was provided or writing failed.
///
/// Mirrors `signalReady` in the desktop host.
pub fn signal_ready_with(env_map: &BTreeMap<String, String>, pid: u32) -> bool {
    let Some(token) = env_map.get("BFE_LAUNCH_TOKEN").filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(dir) = env_map.get("BFE_LAUNCH_DIR").filter(|s| !s.is_empty()) else {
        return false;
    };
    if !is_valid_launch_token(token) {
        return false;
    }
    let resolved_dir = normalize_lexical(Path::new(dir));
    let target = normalize_lexical(&resolved_dir.join(token));
    if target.parent() != Some(resolved_dir.as_path()) {
        return false;
    }
    if fs::create_dir_all(&resolved_dir).is_err() {
        return false;
    }
    let mut nonce = [0u8; 6];
    let _ = getrandom::getrandom(&mut nonce);
    let hex_nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    let temp = resolved_dir.join(format!(".{token}.tmp-{pid}-{hex_nonce}"));
    let content = format!("{pid}\n");
    if fs::write(&temp, content.as_bytes()).is_ok() && fs::rename(&temp, &target).is_ok() {
        return true;
    }
    let _ = fs::remove_file(&temp);
    false
}

/// Signals launch readiness using the current process environment (`std::env::vars()`)
/// and process ID (`std::process::id()`).
///
/// Mirrors `updater.ready()` in the desktop host and `signalReady()` in
/// the desktop host.
pub fn signal_ready() -> bool {
    let env_map: BTreeMap<String, String> = env::vars().collect();
    signal_ready_with(&env_map, std::process::id())
}

/// Alias for [`signal_ready`] matching `updater.ready()` in the desktop host.
pub fn ready() -> bool {
    signal_ready()
}

/// Target version directory and executable passed to [`launch_version`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchTarget {
    /// Semantic version string (`"0.1.0-preview.332"`).
    pub version: String,
    /// Installed version directory (`<root>/versions/<version>`).
    pub dir: PathBuf,
    /// Path to the executable inside `dir`.
    pub executable: PathBuf,
}

/// Options for [`launch_version`].
///
/// Mirrors the parameter object of `launchVersion` in the desktop host.
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Installed version target to spawn.
    pub target: LaunchTarget,
    /// Command-line arguments forwarded to the child executable.
    pub args: Vec<String>,
    /// Base environment variables inherited from the parent process.
    pub env: BTreeMap<String, String>,
    /// Bootstrap installation directory (`BFE_BOOTSTRAP_DIR`).
    pub bootstrap_dir: Option<PathBuf>,
    /// Bootstrap installation version (`BFE_BOOTSTRAP_VERSION`).
    pub bootstrap_version: Option<String>,
    /// Update root directory (`<root>/launches` is created inside `root`).
    pub root: PathBuf,
    /// Maximum milliseconds to wait for the readiness token (`DEFAULT_READY_TIMEOUT_MS`).
    pub timeout_ms: u64,
}

/// Outcome returned by [`launch_version`].
///
/// Mirrors the `{ ok, reason, detail, pid }` object returned by `launchVersion` in
/// the desktop host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOutcome {
    /// True when the child wrote its readiness token before exiting or timing out.
    pub ok: bool,
    /// Outcome code: `"ready"`, `"missing-executable"`, `"spawn-error"`, `"exited-early"`, or `"timeout"`.
    pub reason: String,
    /// Optional diagnostic detail (`exit code`, signal, or OS spawn error).
    pub detail: Option<String>,
    /// Spawned child process ID when `ok` is `true`.
    pub pid: Option<u32>,
}

/// Spawns `opts.target.executable` detached with a fresh `BFE_LAUNCH_TOKEN` and polls
/// `<root>/launches/<token>` every [`READY_POLL_MS`] up to `opts.timeout_ms`.
///
/// If the child does not write its readiness token before exiting or before `timeout_ms`
/// elapses, kills the child (on timeout), cleans up the token file, and returns `ok: false`.
///
/// Mirrors `launchVersion` in the desktop host.
pub fn launch_version(opts: &LaunchOptions) -> LaunchOutcome {
    if opts.target.executable.as_os_str().is_empty() || !opts.target.executable.exists() {
        return LaunchOutcome {
            ok: false,
            reason: "missing-executable".to_string(),
            detail: None,
            pid: None,
        };
    }

    let launch_dir = opts.root.join("launches");
    if let Err(err) = fs::create_dir_all(&launch_dir) {
        return LaunchOutcome {
            ok: false,
            reason: "spawn-error".to_string(),
            detail: Some(err.to_string()),
            pid: None,
        };
    }

    let token = match random_token() {
        Ok(t) => t,
        Err(err) => {
            return LaunchOutcome {
                ok: false,
                reason: "spawn-error".to_string(),
                detail: Some(err),
                pid: None,
            }
        }
    };
    let ready_file = launch_dir.join(&token);
    let _ = fs::remove_file(&ready_file);

    let child_env = child_environment(&ChildEnvOptions {
        base_env: opts.env.clone(),
        bootstrap_dir: opts.bootstrap_dir.clone(),
        bootstrap_version: opts.bootstrap_version.clone(),
        launch_dir: Some(launch_dir.clone()),
        token: Some(token),
    });

    let mut cmd = Command::new(&opts.target.executable);
    cmd.args(&opts.args)
        .current_dir(&opts.target.dir)
        .env_clear()
        .envs(&child_env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    // An executable written moments ago can be reported busy (ETXTBSY) while another thread of this process is
    // between fork and exec and still holds the write descriptor; that clears within milliseconds, so retry briefly.
    let mut busy_retries = 0;
    let mut child = loop {
        match cmd.spawn() {
            Ok(c) => break c,
            Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && busy_retries < 20 => {
                busy_retries += 1;
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => {
                let _ = fs::remove_file(&ready_file);
                return LaunchOutcome {
                    ok: false,
                    reason: "spawn-error".to_string(),
                    detail: Some(err.to_string()),
                    pid: None,
                };
            }
        }
    };

    let pid = child.id();
    let deadline = Instant::now() + Duration::from_millis(opts.timeout_ms);

    loop {
        if ready_file.exists() {
            let _ = fs::remove_file(&ready_file);
            return LaunchOutcome {
                ok: true,
                reason: "ready".to_string(),
                detail: None,
                pid: Some(pid),
            };
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                // Re-check once after exit in case the child wrote the file just before exiting.
                if ready_file.exists() {
                    let _ = fs::remove_file(&ready_file);
                    return LaunchOutcome {
                        ok: true,
                        reason: "ready".to_string(),
                        detail: None,
                        pid: Some(pid),
                    };
                }
                let _ = fs::remove_file(&ready_file);
                return LaunchOutcome {
                    ok: false,
                    reason: "exited-early".to_string(),
                    detail: Some(format_exit_status(status)),
                    pid: None,
                };
            }
            Ok(None) => {}
            Err(err) => {
                let _ = fs::remove_file(&ready_file);
                return LaunchOutcome {
                    ok: false,
                    reason: "spawn-error".to_string(),
                    detail: Some(err.to_string()),
                    pid: None,
                };
            }
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&ready_file);
            return LaunchOutcome {
                ok: false,
                reason: "timeout".to_string(),
                detail: None,
                pid: None,
            };
        }

        thread::sleep(Duration::from_millis(READY_POLL_MS));
    }
}

fn format_exit_status(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        code.to_string()
    } else {
        status.to_string()
    }
}

/// Release metadata read from `<ownDir>/resources/bfe-release.json`,
/// `<ownDir>/bfe-release.json`, `<ownDir>/package.json`, or `<ownDir>/.bfe-install.json`.
///
/// Mirrors `readReleaseInfo` in the desktop host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Semantic version of the running build (`"0.1.0-preview.332"`).
    pub version: Option<String>,
    /// Update channel of the running build (`"development-preview"`).
    pub channel: Option<String>,
}

#[derive(Deserialize)]
struct RawReleaseJson {
    version: Option<String>,
    channel: Option<String>,
    #[serde(rename = "bfeUpdateChannel")]
    bfe_update_channel: Option<String>,
}

/// Reads version and channel metadata from `own_dir`.
///
/// Search order:
/// 1. `<own_dir>/resources/bfe-release.json`
/// 2. `<own_dir>/bfe-release.json`
/// 3. `<own_dir>/package.json` (`version` and `bfeUpdateChannel`)
/// 4. `<own_dir>/.bfe-install.json` (`version`, defaulting channel to `"development-preview"`)
///
/// Mirrors `readReleaseInfo` in the desktop host.
pub fn read_release_info(own_dir: &Path) -> ReleaseInfo {
    for rel in ["resources/bfe-release.json", "bfe-release.json"] {
        let candidate = own_dir.join(rel);
        if let Ok(bytes) = fs::read(&candidate) {
            if let Ok(parsed) = serde_json::from_slice::<RawReleaseJson>(&bytes) {
                if let Some(ver) = parsed.version.filter(|v| parse_version(v).is_some()) {
                    return ReleaseInfo {
                        version: Some(ver),
                        channel: parsed.channel.filter(|c| !c.is_empty()),
                    };
                }
            }
        }
    }

    let pkg_path = own_dir.join("package.json");
    if let Ok(bytes) = fs::read(&pkg_path) {
        if let Ok(parsed) = serde_json::from_slice::<RawReleaseJson>(&bytes) {
            if let Some(ver) = parsed.version.filter(|v| parse_version(v).is_some()) {
                return ReleaseInfo {
                    version: Some(ver),
                    channel: parsed.bfe_update_channel.filter(|c| !c.is_empty()),
                };
            }
        }
    }

    let install_path = own_dir.join(INSTALL_RECORD);
    if let Ok(bytes) = fs::read(&install_path) {
        if let Ok(parsed) = serde_json::from_slice::<RawReleaseJson>(&bytes) {
            if let Some(ver) = parsed.version.filter(|v| parse_version(v).is_some()) {
                return ReleaseInfo {
                    version: Some(ver),
                    channel: parsed
                        .channel
                        .filter(|c| !c.is_empty())
                        .or_else(|| Some("development-preview".to_string())),
                };
            }
        }
    }

    ReleaseInfo::default()
}

/// Appends a timestamped diagnostic line to `<root>/update.log`.
///
/// Mirrors `log` inside `createUpdater` in the desktop host.
pub fn append_update_log(root: &Path, own_version: Option<&str>, message: &str) {
    if fs::create_dir_all(root).is_err() {
        return;
    }
    let log_path = root.join("update.log");
    let ts = iso8601_now();
    let ver = own_version.unwrap_or("?");
    let line = format!("[{ts}] [{ver}] {message}\n");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Formats the current UTC system time as an ISO-8601 string (`YYYY-MM-DDTHH:MM:SS.mmmZ`).
pub fn iso8601_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil_from_days algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Explicit configuration for [`settle_with`].
///
/// Mirrors the environment of `updater.settle()` in the desktop host.
#[derive(Debug, Clone)]
pub struct SettleOptions {
    /// Directory containing the running executable.
    pub own_dir: PathBuf,
    /// Optional explicit version override.
    pub own_version: Option<String>,
    /// Optional explicit channel override.
    pub own_channel: Option<String>,
    /// Optional explicit update root override.
    pub root: Option<PathBuf>,
    /// Optional explicit bootstrap version to preserve during `collect`.
    pub bootstrap_version: Option<String>,
    /// Target OS name (`"linux"` or `"win32"`).
    pub platform: String,
    /// Target CPU architecture (`"x64"` or `"arm64"`).
    pub arch: String,
    /// Environment map.
    pub env: BTreeMap<String, String>,
    /// User home directory.
    pub home: PathBuf,
}

impl SettleOptions {
    /// Captures [`SettleOptions`] from the current process environment and executable path.
    pub fn from_current_process() -> Self {
        let env_map: BTreeMap<String, String> = env::vars().collect();
        let env_hash: HashMap<String, String> = env_map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let home = home_dir_from_env(&env_hash);
        let own_dir = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .or_else(|| env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            own_dir,
            own_version: None,
            own_channel: None,
            root: None,
            bootstrap_version: None,
            platform: env::consts::OS.to_string(),
            arch: env::consts::ARCH.to_string(),
            env: env_map,
            home,
        }
    }
}

/// Outcome returned by [`settle`] and [`settle_with`].
///
/// Mirrors `{ settled, removed }` returned by `updater.settle()` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettleOutcome {
    /// True if the running build settled its health state in `<root>/state.json`.
    pub settled: bool,
    /// Version directory names pruned from `<root>/versions`.
    pub removed: Vec<String>,
}

/// Records the running version as healthy in `<root>/state.json`, updates `<root>/current`
/// on Linux when running from `<root>/versions/<version>`, and garbage-collects superseded
/// version directories.
///
/// Mirrors `updater.settle()` in the desktop host.
pub fn settle_with(opts: &SettleOptions) -> SettleOutcome {
    let info = read_release_info(&opts.own_dir);
    let own_version = opts
        .own_version
        .clone()
        .or(info.version)
        .filter(|v| parse_version(v).is_some());
    let own_channel = opts
        .env
        .get("BFE_UPDATE_CHANNEL")
        .filter(|c| !c.is_empty())
        .cloned()
        .or_else(|| opts.own_channel.clone().filter(|c| !c.is_empty()))
        .or(info.channel);
    let env_hash: HashMap<String, String> = opts
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let root = opts
        .root
        .clone()
        .or_else(|| resolve_update_root(&opts.platform, &env_hash, &opts.home));
    let plat = platform_key(&opts.platform, &opts.arch);

    let disabled = opts.env.get("BFE_DISABLE_UPDATES").map(|s| s.as_str()) == Some("1");
    let (Some(version), Some(_channel), Some(root_dir), Some(plat_key)) =
        (own_version, own_channel, root, plat)
    else {
        return SettleOutcome {
            settled: false,
            removed: Vec::new(),
        };
    };
    if disabled {
        return SettleOutcome {
            settled: false,
            removed: Vec::new(),
        };
    }

    if let Err(err) = record_healthy(&root_dir, &version) {
        append_update_log(&root_dir, Some(&version), &format!("settle failed: {err}"));
        return SettleOutcome {
            settled: false,
            removed: Vec::new(),
        };
    }

    if plat_key.starts_with("linux-") {
        let norm_own = normalize_lexical(&opts.own_dir);
        let norm_versions = normalize_lexical(&versions_dir(&root_dir));
        if norm_own != norm_versions && norm_own.starts_with(&norm_versions) {
            point_current(&root_dir, &opts.own_dir);
        }
    }

    let mut keep = vec![version.clone()];
    if let Some(boot_ver) = opts
        .bootstrap_version
        .clone()
        .or_else(|| opts.env.get("BFE_BOOTSTRAP_VERSION").cloned())
        .filter(|v| !v.is_empty())
    {
        keep.push(boot_ver);
    }
    // Preserve any newer staged version awaiting next-launch handoff so a fast
    // background update check (5s) is not pruned when settlement runs at 10s.
    for cand in crate::update::state::installed_versions(&root_dir, Some(plat_key)) {
        if crate::update::version::is_newer(&cand.version, &version) {
            keep.push(cand.version);
        }
    }

    let removed = collect(&root_dir, &keep, 2);
    if !removed.is_empty() {
        append_update_log(
            &root_dir,
            Some(&version),
            &format!("pruned old versions: {}", removed.join(", ")),
        );
    }

    SettleOutcome {
        settled: true,
        removed,
    }
}

/// Settles the running process using [`SettleOptions::from_current_process`].
///
/// Mirrors `updater.settle()` in the desktop host.
pub fn settle() -> SettleOutcome {
    settle_with(&SettleOptions::from_current_process())
}

/// Schedules [`settle`] to run on a background thread after [`DEFAULT_SETTLE_DELAY_MS`]
/// (10 seconds, or `BFE_SETTLE_DELAY_MS` if set in the environment).
///
/// Mirrors `setTimeout(() => updater.settle(), 10 * 1000)` in the desktop host.
pub fn schedule_settle() {
    let delay_ms = env::var("BFE_SETTLE_DELAY_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_SETTLE_DELAY_MS);
    let _ = thread::Builder::new()
        .name("bfe-update-settle".to_string())
        .spawn(move || {
            thread::sleep(Duration::from_millis(delay_ms));
            let _ = settle();
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::state::{read_state, write_state, InstallRecord, State};

    fn make_temp_dir(label: &str) -> PathBuf {
        let mut nonce = [0u8; 6];
        let _ = getrandom::getrandom(&mut nonce);
        let hex: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        let dir = env::temp_dir().join(format!("bfe-launch-test-{label}-{hex}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn child_environment_preserves_bootstrap() {
        let mut base = BTreeMap::new();
        base.insert("APPIMAGE".to_string(), "/opt/bfe.AppImage".to_string());

        let env1 = child_environment(&ChildEnvOptions {
            base_env: base,
            bootstrap_dir: Some(PathBuf::from("/opt/Battlefront Eternal")),
            bootstrap_version: Some("0.1.0-preview.331".to_string()),
            launch_dir: Some(PathBuf::from("/var/lib/bfe/launches")),
            token: Some("0123456789abcdef0123456789abcdef".to_string()),
        });
        assert_eq!(
            env1.get("BFE_BOOTSTRAP_DIR").map(|s| s.as_str()),
            Some("/opt/Battlefront Eternal")
        );
        assert_eq!(
            env1.get("BFE_BOOTSTRAP_VERSION").map(|s| s.as_str()),
            Some("0.1.0-preview.331")
        );
        assert_eq!(
            env1.get("BFE_APPIMAGE").map(|s| s.as_str()),
            Some("/opt/bfe.AppImage")
        );

        // Second hop preserves original bootstrap values while updating launch token.
        let env2 = child_environment(&ChildEnvOptions {
            base_env: env1,
            bootstrap_dir: Some(PathBuf::from("/other/dir")),
            bootstrap_version: Some("0.1.0-preview.332".to_string()),
            launch_dir: Some(PathBuf::from("/var/lib/bfe/launches")),
            token: Some("fedcba9876543210fedcba9876543210".to_string()),
        });
        assert_eq!(
            env2.get("BFE_BOOTSTRAP_DIR").map(|s| s.as_str()),
            Some("/opt/Battlefront Eternal")
        );
        assert_eq!(
            env2.get("BFE_BOOTSTRAP_VERSION").map(|s| s.as_str()),
            Some("0.1.0-preview.331")
        );
        assert_eq!(
            env2.get("BFE_LAUNCH_TOKEN").map(|s| s.as_str()),
            Some("fedcba9876543210fedcba9876543210")
        );
    }

    #[test]
    fn signal_ready_validates_token_and_writes_pid() {
        let dir = make_temp_dir("signal");
        let token = "0123456789abcdef0123456789abcdef";
        let mut env_map = BTreeMap::new();
        env_map.insert(
            "BFE_LAUNCH_DIR".to_string(),
            dir.to_string_lossy().into_owned(),
        );
        env_map.insert("BFE_LAUNCH_TOKEN".to_string(), token.to_string());

        assert!(signal_ready_with(&env_map, 4242));
        let written = fs::read_to_string(dir.join(token)).unwrap();
        assert_eq!(written, "4242\n");

        // Invalid tokens (path traversal or non-hex) are rejected.
        env_map.insert("BFE_LAUNCH_TOKEN".to_string(), "../escape".to_string());
        assert!(!signal_ready_with(&env_map, 4242));

        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn launch_version_detects_ready_early_exit_and_timeout() {
        use std::os::unix::fs::PermissionsExt;

        let root = make_temp_dir("launch");
        let ver_dir = root.join("versions").join("0.1.0-preview.332");
        fs::create_dir_all(&ver_dir).unwrap();

        // 1. Script that writes BFE_LAUNCH_DIR/BFE_LAUNCH_TOKEN immediately.
        let ok_exe = ver_dir.join("ok.sh");
        fs::write(
            &ok_exe,
            "#!/bin/sh\necho $$ > \"$BFE_LAUNCH_DIR/$BFE_LAUNCH_TOKEN\"\nsleep 1\n",
        )
        .unwrap();
        fs::set_permissions(&ok_exe, fs::Permissions::from_mode(0o755)).unwrap();

        let res_ok = launch_version(&LaunchOptions {
            target: LaunchTarget {
                version: "0.1.0-preview.332".to_string(),
                dir: ver_dir.clone(),
                executable: ok_exe,
            },
            args: Vec::new(),
            env: BTreeMap::new(),
            bootstrap_dir: None,
            bootstrap_version: None,
            root: root.clone(),
            timeout_ms: 5000,
        });
        assert!(res_ok.ok, "{} {:?}", res_ok.reason, res_ok.detail);
        assert_eq!(res_ok.reason, "ready");
        assert!(res_ok.pid.is_some());

        // 2. Script that exits 1 without writing the token.
        let fail_exe = ver_dir.join("fail.sh");
        fs::write(&fail_exe, "#!/bin/sh\nexit 7\n").unwrap();
        fs::set_permissions(&fail_exe, fs::Permissions::from_mode(0o755)).unwrap();

        let res_fail = launch_version(&LaunchOptions {
            target: LaunchTarget {
                version: "0.1.0-preview.332".to_string(),
                dir: ver_dir.clone(),
                executable: fail_exe,
            },
            args: Vec::new(),
            env: BTreeMap::new(),
            bootstrap_dir: None,
            bootstrap_version: None,
            root: root.clone(),
            timeout_ms: 2000,
        });
        assert!(!res_fail.ok);
        assert_eq!(res_fail.reason, "exited-early");

        // 3. Script that sleeps past timeout without writing the token.
        let slow_exe = ver_dir.join("slow.sh");
        fs::write(&slow_exe, "#!/bin/sh\nsleep 10\n").unwrap();
        fs::set_permissions(&slow_exe, fs::Permissions::from_mode(0o755)).unwrap();

        let res_timeout = launch_version(&LaunchOptions {
            target: LaunchTarget {
                version: "0.1.0-preview.332".to_string(),
                dir: ver_dir,
                executable: slow_exe,
            },
            args: Vec::new(),
            env: BTreeMap::new(),
            bootstrap_dir: None,
            bootstrap_version: None,
            root: root.clone(),
            timeout_ms: 250,
        });
        assert!(!res_timeout.ok);
        assert_eq!(res_timeout.reason, "timeout");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn settle_marks_healthy_points_current_and_prunes_old_versions() {
        let root = make_temp_dir("settle");
        let versions = versions_dir(&root);

        for ver in [
            "0.1.0-preview.330",
            "0.1.0-preview.331",
            "0.1.0-preview.332",
        ] {
            let vdir = versions.join(ver);
            fs::create_dir_all(vdir.join("resources")).unwrap();
            let exe = vdir.join("bfe-client");
            fs::write(&exe, b"bin").unwrap();
            let rec = InstallRecord {
                version: ver.to_string(),
                release_id: Some(format!("r-{ver}")),
                revision: None,
                platform: Some("linux-x64".to_string()),
                executable: "bfe-client".to_string(),
                files: Vec::new(),
                installed_at: None,
            };
            fs::write(
                vdir.join(INSTALL_RECORD),
                serde_json::to_vec_pretty(&rec).unwrap(),
            )
            .unwrap();
            fs::write(
                vdir.join("resources/bfe-release.json"),
                format!(
                    r#"{{"version":"{ver}","channel":"development-preview"}}"#
                ),
            )
            .unwrap();
        }

        // Seed state with 331 healthy and 332 having 1 attempt.
        let mut initial = State::default();
        initial.healthy.push("0.1.0-preview.331".to_string());
        initial.attempts.insert("0.1.0-preview.332".to_string(), 1);
        write_state(&root, &initial).unwrap();

        let own_dir = versions.join("0.1.0-preview.332");
        let outcome = settle_with(&SettleOptions {
            own_dir: own_dir.clone(),
            own_version: None,
            own_channel: None,
            root: Some(root.clone()),
            bootstrap_version: None,
            platform: "linux".to_string(),
            arch: "x64".to_string(),
            env: BTreeMap::new(),
            home: root.clone(),
        });

        assert!(outcome.settled);
        assert_eq!(outcome.removed, vec!["0.1.0-preview.330".to_string()]);
        let after = read_state(&root);
        assert_eq!(
            after.healthy,
            vec![
                "0.1.0-preview.332".to_string(),
                "0.1.0-preview.331".to_string()
            ]
        );
        assert!(!after.attempts.contains_key("0.1.0-preview.332"));

        #[cfg(unix)]
        {
            let link = fs::read_link(root.join("current")).unwrap();
            assert_eq!(link, PathBuf::from("versions/0.1.0-preview.332"));
        }

        let _ = fs::remove_dir_all(&root);
    }
}
