//! Boot-time version handoff (`handOff`), CLI argument forwarding (`passedArguments`),
//! and background update check/install (`createUpdater` / `check_and_install`).
//!
//! Mirrors the desktop host (`passedArguments`, `readReleaseInfo`,
//! `handOff`, `createUpdater`).

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::launch::{
    append_update_log, launch_version, read_release_info, LaunchOptions, LaunchTarget,
    DEFAULT_READY_TIMEOUT_MS,
};
use crate::paths::{home_dir_from_env, normalize_lexical};
use crate::update::feed::{fetch_channel, fetch_manifest, DEFAULT_TIMEOUT_MS};
use crate::update::install::{
    install_release, InstallProgress, InstallReleaseOptions, DEFAULT_BLOB_TIMEOUT_MS,
    DEFAULT_CONCURRENCY,
};
use crate::update::keys::{default_keys, FEED};
use crate::update::state::{
    choose_launch, installed_versions, platform_key, read_state, record_attempt,
    resolve_update_root, InstalledVersion,
};
use crate::update::version::{is_newer, parse_version};

/// Default delay in milliseconds (`5_000`) after startup before running the
/// background update check.
///
/// Mirrors `setTimeout(() => updater.check().catch(() => {}), 5000)` in the desktop host.
pub const DEFAULT_CHECK_DELAY_MS: u64 = 5 * 1000;

/// Extracts the command-line arguments to forward to a handed-off child version.
///
/// Mirrors `passedArguments` in the desktop host.
pub fn passed_arguments(argv: &[String], is_packaged: bool) -> Vec<String> {
    if is_packaged {
        return argv.get(1..).unwrap_or(&[]).to_vec();
    }
    if argv.len() >= 2 && !argv[1].starts_with('-') {
        return argv.get(2..).unwrap_or(&[]).to_vec();
    }
    argv.get(1..).unwrap_or(&[]).to_vec()
}

/// Configuration for [`hand_off`] and [`check_and_install`].
///
/// Mirrors the parameter object of `handOff` and `createUpdater` in
/// the desktop host.
#[derive(Debug, Clone)]
pub struct UpdaterContext {
    /// Directory containing the running executable.
    pub own_dir: PathBuf,
    /// Optional explicit version override.
    pub own_version: Option<String>,
    /// Optional explicit channel override.
    pub own_channel: Option<String>,
    /// Optional explicit update root override.
    pub root: Option<PathBuf>,
    /// Optional explicit feed base URL override (`BFE_UPDATE_FEED` or `FEED`).
    pub feed_url: Option<String>,
    /// Trusted Ed25519 public keys (`key_id -> PEM`).
    pub keys: BTreeMap<String, String>,
    /// Target OS (`"linux"` or `"win32"`).
    pub platform: String,
    /// Target CPU architecture (`"x64"` or `"arm64"`).
    pub arch: String,
    /// Command-line arguments (`std::env::args()`).
    pub argv: Vec<String>,
    /// Whether the build is packaged (`true` for native installed/packaged binaries).
    pub is_packaged: bool,
    /// Process environment variables.
    pub env: BTreeMap<String, String>,
    /// User home directory.
    pub home: PathBuf,
    /// Readiness timeout in milliseconds when launching a child version (`DEFAULT_READY_TIMEOUT_MS`).
    pub launch_timeout_ms: u64,
    /// HTTP timeout in milliseconds for channel and manifest fetches (`DEFAULT_TIMEOUT_MS`).
    pub fetch_timeout_ms: u64,
    /// HTTP timeout in milliseconds for blob downloads (`DEFAULT_BLOB_TIMEOUT_MS`).
    pub blob_timeout_ms: u64,
    /// Maximum concurrent blob downloads (`DEFAULT_CONCURRENCY`).
    pub concurrency: usize,
}

impl UpdaterContext {
    /// Captures [`UpdaterContext`] from the current process environment and executable location.
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
            feed_url: None,
            keys: default_keys(),
            platform: env::consts::OS.to_string(),
            arch: env::consts::ARCH.to_string(),
            argv: env::args().collect(),
            is_packaged: true,
            env: env_map,
            home,
            launch_timeout_ms: DEFAULT_READY_TIMEOUT_MS,
            fetch_timeout_ms: DEFAULT_TIMEOUT_MS,
            blob_timeout_ms: DEFAULT_BLOB_TIMEOUT_MS,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }

    fn resolve_active(&self) -> Option<ResolvedUpdaterEnv> {
        if self.env.get("BFE_DISABLE_UPDATES").map(|s| s.as_str()) == Some("1")
            || self.env.get("BFE_NO_UPDATE").map(|s| s.as_str()) == Some("1")
        {
            return None;
        }
        let info = read_release_info(&self.own_dir);
        let own_version = self
            .own_version
            .clone()
            .or(info.version)
            .filter(|v| parse_version(v).is_some())?;
        let own_channel = self
            .env
            .get("BFE_UPDATE_CHANNEL")
            .filter(|c| !c.is_empty())
            .cloned()
            .or_else(|| self.own_channel.clone().filter(|c| !c.is_empty()))
            .or(info.channel)
            .filter(|c| !c.is_empty())?;
        let env_hash: HashMap<String, String> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let root = self
            .root
            .clone()
            .or_else(|| resolve_update_root(&self.platform, &env_hash, &self.home))?;
        let plat = platform_key(&self.platform, &self.arch)?;
        let feed_url = self
            .feed_url
            .clone()
            .or_else(|| self.env.get("BFE_UPDATE_FEED").cloned())
            .or_else(|| self.env.get("BFE_UPDATE_FEED_URL").cloned())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| FEED.to_string());
        let bootstrap_dir = self
            .env
            .get("BFE_BOOTSTRAP_DIR")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.own_dir.clone());
        let bootstrap_version = self
            .env
            .get("BFE_BOOTSTRAP_VERSION")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| own_version.clone());

        Some(ResolvedUpdaterEnv {
            own_version,
            own_channel,
            root,
            plat,
            feed_url,
            bootstrap_dir,
            bootstrap_version,
        })
    }
}

struct ResolvedUpdaterEnv {
    own_version: String,
    own_channel: String,
    root: PathBuf,
    plat: &'static str,
    feed_url: String,
    bootstrap_dir: PathBuf,
    bootstrap_version: String,
}

/// Outcome returned by [`hand_off`].
///
/// Mirrors `{ handedOff, target }` returned by `handOff` in the desktop host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffOutcome {
    /// True if a newer installed version was spawned and signalled readiness.
    pub handed_off: bool,
    /// Installed version that took over execution when `handed_off` is `true`.
    pub target: Option<InstalledVersion>,
}

/// Checks `<root>/versions` for a newer eligible version than the running build.
///
/// Tries up to 4 candidate versions (newest first). For each candidate:
/// 1. Increments `attempts[<version>]` in `<root>/state.json` (marking it `bad` if it
///    reaches `MAX_ATTEMPTS` without having been confirmed `healthy`).
/// 2. Spawns the candidate executable with `BFE_LAUNCH_DIR` and `BFE_LAUNCH_TOKEN`
///    and waits up to `ctx.launch_timeout_ms` for the readiness token file.
/// 3. If the child signals readiness, logs `handed off to <version>` in `<root>/update.log`
///    and returns `handed_off: true`.
/// 4. Otherwise logs `handoff to <version> failed (<reason>)` and falls back to the
///    next older installed version or the running build.
///
/// Mirrors `handOff` in the desktop host.
pub fn hand_off(ctx: &UpdaterContext) -> HandoffOutcome {
    let Some(resolved) = ctx.resolve_active() else {
        return HandoffOutcome {
            handed_off: false,
            target: None,
        };
    };

    for _ in 0..4 {
        let Some(candidate) = choose_launch(
            &resolved.root,
            &resolved.own_version,
            Some(resolved.plat),
            Some(&ctx.own_dir),
        ) else {
            break;
        };

        let _ = record_attempt(&resolved.root, &candidate.version);
        let result = launch_version(&LaunchOptions {
            target: LaunchTarget {
                version: candidate.version.clone(),
                dir: candidate.dir.clone(),
                executable: candidate.executable.clone(),
            },
            args: passed_arguments(&ctx.argv, ctx.is_packaged),
            env: ctx.env.clone(),
            bootstrap_dir: Some(resolved.bootstrap_dir.clone()),
            bootstrap_version: Some(resolved.bootstrap_version.clone()),
            root: resolved.root.clone(),
            timeout_ms: ctx.launch_timeout_ms,
        });

        if result.ok {
            append_update_log(
                &resolved.root,
                Some(&resolved.own_version),
                &format!("handed off to {}", candidate.version),
            );
            return HandoffOutcome {
                handed_off: true,
                target: Some(candidate),
            };
        }

        let detail = result
            .detail
            .as_deref()
            .map(|d| format!(": {d}"))
            .unwrap_or_default();
        append_update_log(
            &resolved.root,
            Some(&resolved.own_version),
            &format!(
                "handoff to {} failed ({}{})",
                candidate.version, result.reason, detail
            ),
        );
    }

    HandoffOutcome {
        handed_off: false,
        target: None,
    }
}

/// Runs [`hand_off`] for the current process and exits with status `0` if a newer
/// installed version took over execution.
///
/// Mirrors the `handOff()` check in the desktop host.
pub fn boot_handoff_or_continue() {
    let ctx = UpdaterContext::from_current_process();
    let outcome = hand_off(&ctx);
    if outcome.handed_off {
        std::process::exit(0);
    }
}

/// Result of [`check_and_install`].
///
/// Mirrors the state transitions of `updater.check()` and `updater.install()` in
/// the desktop host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckOutcome {
    /// Auto-updates are disabled (unpackaged build or `BFE_DISABLE_UPDATES=1` / `BFE_NO_UPDATE=1`).
    Disabled,
    /// Running build is already at or newer than the channel's published release.
    UpToDate {
        /// Running build version.
        version: String,
    },
    /// Channel release is already installed in `<root>/versions/<version>` awaiting restart.
    AlreadyInstalled {
        /// Installed target version.
        version: String,
    },
    /// Channel release was previously marked `bad` after failing launch attempts.
    SkippedBad {
        /// Skipped version string.
        version: String,
    },
    /// Channel release was downloaded, verified, and installed into `<root>/versions/<version>`.
    Installed {
        /// Newly installed version string.
        version: String,
        /// Path to `<root>/versions/<version>`.
        dir: PathBuf,
        /// Number of files reused from local trees.
        reused: usize,
        /// Number of files downloaded from `.zst` blobs.
        downloaded: usize,
    },
    /// Update check or installation failed with an error message (logged to `<root>/update.log`).
    Failed(String),
}

/// Queries `<feed_url>/updates/<channel>.json`, verifies the Ed25519 signature, and
/// if a newer release is published and not marked `bad`, downloads and stages its
/// platform manifest and missing blobs into `<root>/versions/<version>`.
///
/// Mirrors `updater.check()` and `updater.install()` in the desktop host.
pub fn check_and_install<F>(ctx: &UpdaterContext, on_progress: Option<F>) -> CheckOutcome
where
    F: FnMut(InstallProgress),
{
    let Some(resolved) = ctx.resolve_active() else {
        return CheckOutcome::Disabled;
    };

    let step: Result<CheckOutcome, String> = (|| {
        let channel_data = fetch_channel(
            &resolved.feed_url,
            &resolved.own_channel,
            &ctx.keys,
            ctx.fetch_timeout_ms,
        )?;
        let release = &channel_data.release;
        if !is_newer(&release.version, &resolved.own_version) {
            return Ok(CheckOutcome::UpToDate {
                version: resolved.own_version.clone(),
            });
        }

        let existing = installed_versions(&resolved.root, Some(resolved.plat));
        if let Some(found) = existing.iter().find(|v| v.version == release.version) {
            let _ = found;
            return Ok(CheckOutcome::AlreadyInstalled {
                version: release.version.clone(),
            });
        }

        let state = read_state(&resolved.root);
        if state.bad.contains(&release.version) {
            append_update_log(
                &resolved.root,
                Some(&resolved.own_version),
                &format!("skipping bad release {}", release.version),
            );
            return Ok(CheckOutcome::SkippedBad {
                version: release.version.clone(),
            });
        }

        append_update_log(
            &resolved.root,
            Some(&resolved.own_version),
            &format!("downloading {} ({})", release.version, release.id),
        );

        let manifest = fetch_manifest(
            &resolved.feed_url,
            release,
            resolved.plat,
            ctx.fetch_timeout_ms,
        )?;

        let mut local_trees = vec![ctx.own_dir.clone()];
        if normalize_lexical(&resolved.bootstrap_dir) != normalize_lexical(&ctx.own_dir) {
            local_trees.push(resolved.bootstrap_dir.clone());
        }
        for v in existing {
            local_trees.push(v.dir);
        }

        let result = install_release(
            &InstallReleaseOptions {
                root: resolved.root.clone(),
                manifest,
                base_url: resolved.feed_url.clone(),
                local_trees,
                concurrency: ctx.concurrency,
                timeout_ms: ctx.blob_timeout_ms,
            },
            on_progress,
        )?;

        append_update_log(
            &resolved.root,
            Some(&resolved.own_version),
            &format!(
                "installed {} ({} reused, {} downloaded)",
                release.version, result.reused, result.downloaded
            ),
        );

        Ok(CheckOutcome::Installed {
            version: release.version.clone(),
            dir: result.dir,
            reused: result.reused,
            downloaded: result.downloaded,
        })
    })();

    match step {
        Ok(outcome) => outcome,
        Err(err) => {
            append_update_log(
                &resolved.root,
                Some(&resolved.own_version),
                &format!("update check failed: {err}"),
            );
            CheckOutcome::Failed(err)
        }
    }
}

/// Starts a newly installed version in the current process's place and waits for its
/// readiness token (`BFE_LAUNCH_DIR/BFE_LAUNCH_TOKEN`).
///
/// Mirrors `updater.launch(installed)` in the desktop host.
pub fn launch_installed(ctx: &UpdaterContext, version: &str) -> bool {
    let Some(resolved) = ctx.resolve_active() else {
        return false;
    };
    let Some(candidate) = installed_versions(&resolved.root, Some(resolved.plat))
        .into_iter()
        .find(|v| v.version == version)
    else {
        return false;
    };

    let result = launch_version(&LaunchOptions {
        target: LaunchTarget {
            version: candidate.version.clone(),
            dir: candidate.dir.clone(),
            executable: candidate.executable.clone(),
        },
        args: passed_arguments(&ctx.argv, ctx.is_packaged),
        env: ctx.env.clone(),
        bootstrap_dir: Some(resolved.bootstrap_dir),
        bootstrap_version: Some(resolved.bootstrap_version),
        root: resolved.root.clone(),
        timeout_ms: ctx.launch_timeout_ms,
    });

    if !result.ok {
        let _ = record_attempt(&resolved.root, &candidate.version);
        let detail = result
            .detail
            .as_deref()
            .map(|d| format!(": {d}"))
            .unwrap_or_default();
        append_update_log(
            &resolved.root,
            Some(&resolved.own_version),
            &format!("{} did not start ({}{})", candidate.version, result.reason, detail),
        );
    }
    result.ok
}

/// Schedules [`check_and_install`] on a background thread after [`DEFAULT_CHECK_DELAY_MS`]
/// (5 seconds, or `BFE_CHECK_DELAY_MS` if set in the environment), and restarts into the
/// newly installed version once its window signals readiness.
///
/// Mirrors `bootUpdate` (`updater.check` -> `updater.install` -> `updater.launch`) in
/// the desktop host.
pub fn schedule_background_check() {
    let delay_ms = env::var("BFE_CHECK_DELAY_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_CHECK_DELAY_MS);
    let _ = thread::Builder::new()
        .name("bfe-update-check".to_string())
        .spawn(move || {
            thread::sleep(Duration::from_millis(delay_ms));
            let ctx = UpdaterContext::from_current_process();
            if let CheckOutcome::Installed { version, .. } =
                check_and_install::<fn(InstallProgress)>(&ctx, None)
            {
                if launch_installed(&ctx, &version) {
                    std::process::exit(0);
                }
            }
        });
}
