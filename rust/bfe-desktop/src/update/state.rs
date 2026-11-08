//! Persistent desktop auto-update state, version slot management, crash-loop
//! rollback guard, and `current` symlink updates.
//!
//! Mirrors `electron/update/state.js` (`INSTALL_RECORD`, `MAX_ATTEMPTS`,
//! `SUPPORTED_PLATFORMS`, `versionsDir`, `platformKey`, `pointCurrent`) and
//! provides structured slot state management.
//!
//! ## Update Lifecycle & Crash-Loop Guard
//! 1. When a new payload version is installed and verified, [`activate_payload_slot`]
//!    promotes the prior `active` slot to `previous` and sets the new slot as `active`
//!    with `confirmed: false` and `boot_attempts: 0`.
//! 2. On each startup before the shell loads the payload, [`resolve_active_payload_for_launch`]
//!    checks `active.boot_attempts < MAX_ATTEMPTS` (`2`), verifies the payload directory's
//!    tree SHA-256, and increments `boot_attempts`.
//! 3. Once the application completes startup cleanly, [`mark_active_payload_boot_succeeded`]
//!    sets `confirmed: true` and resets `boot_attempts: 0`.
//! 4. If an unconfirmed payload crashes or fails tree verification twice, it is rolled back
//!    to `previous` (or cleared back to the base shell) and pruned by [`prune_payload_slots`].

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::UpdatePaths;
use crate::update::feed::normalize_channel;
use crate::update::tree::verify_payload_directory;
use crate::update::version::{compare_versions, parse_version};

/// Filename of the completion marker record written last inside an installed
/// version directory (`<root>/versions/<version>/.bfe-install.json`).
///
/// Mirrors `INSTALL_RECORD` in `electron/update/state.js`.
pub const INSTALL_RECORD: &str = ".bfe-install.json";

/// Maximum unconfirmed launch attempts before a version is set aside (`2`).
///
/// Mirrors `MAX_ATTEMPTS` in `electron/update/state.js`.
pub const MAX_ATTEMPTS: u32 = 2;

/// Alias for [`MAX_ATTEMPTS`] used by payload slot rollback checks.
pub const MAX_FAILED_BOOTS: u32 = MAX_ATTEMPTS;

/// Platforms supported by the built-in auto-updater (`linux-x64`, `linux-arm64`, `win32-x64`).
///
/// Mirrors `SUPPORTED_PLATFORMS` in `electron/update/state.js`.
pub const SUPPORTED_PLATFORMS: &[&str] = &["linux-x64", "linux-arm64", "win32-x64"];

/// Schema version for `update-state.json` (`1`).
pub const STATE_SCHEMA: u32 = 1;

/// Returns `<root>/versions`.
///
/// Mirrors `versionsDir` in `electron/update/state.js`.
pub fn versions_dir(root: &Path) -> PathBuf {
    root.join("versions")
}

/// Atomically points `<root>/current` at the relative path from `root` to `dir`
/// on Unix so Linux launcher scripts (`~/.local/bin/battlefront`) can exec the
/// newest healthy version directly without unpacking the AppImage.
///
/// Mirrors `pointCurrent` in `electron/update/state.js`.
pub fn point_current(root: &Path, dir: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = root.join("current");
        let target = dir.strip_prefix(root).unwrap_or(dir);
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
        if symlink(target, &temp).is_ok() && fs::rename(&temp, &link).is_ok() {
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

/// Installed payload slot record stored in `update-state.json` (`active` or `previous`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadSlot {
    /// Semantic version string (`"0.2.1"`).
    pub version: String,
    /// Release channel (`"stable"` or `"nightly"`).
    pub channel: String,
    /// Minimum shell version required by this payload.
    pub min_shell_version: String,
    /// Expected tree SHA-256 of the payload directory.
    pub tree_sha256: String,
    /// Archive SHA-256 of the downloaded `.zip` payload, if known.
    pub archive_sha256: Option<String>,
    /// Timestamp when the slot was installed.
    pub installed_at: Option<String>,
    /// Number of consecutive boots that have not yet called [`mark_active_payload_boot_succeeded`].
    #[serde(default)]
    pub boot_attempts: u32,
    /// Whether the payload has completed at least one healthy boot.
    #[serde(default)]
    pub confirmed: bool,
}

/// Pending downloaded full installer record stored in `update-state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInstaller {
    /// Target version of the downloaded installer.
    pub version: String,
    /// Channel of the downloaded installer.
    pub channel: String,
    /// Installer package kind (`"appimage"`, `"nsis"`, `"dmg"`, `"zip"`, `"tar.gz"`).
    pub kind: String,
    /// Local path to the downloaded installer file.
    pub path: String,
    /// Expected SHA-256 hex digest of the installer file.
    pub sha256: String,
    /// Expected byte size of the installer file.
    pub size: u64,
    /// Timestamp when the installer finished downloading.
    pub downloaded_at: Option<String>,
}

/// Persistent update state serialized to `<userData>/updates/update-state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    /// State schema version (`1`).
    pub schema: u32,
    /// Selected update channel (`"stable"` or `"nightly"`).
    pub channel: String,
    /// Whether automatic background update checks are enabled.
    pub auto_check: bool,
    /// Currently active payload slot, if any.
    pub active: Option<PayloadSlot>,
    /// Previously active payload slot kept for instant rollback, if any.
    pub previous: Option<PayloadSlot>,
    /// Pending full installer waiting to be applied on restart, if any.
    pub pending_installer: Option<PendingInstaller>,
    /// Timestamp of the last completed update check.
    pub last_check_at: Option<String>,
    /// Version discovered during the last update check.
    pub last_available_version: Option<String>,
    /// Last update error message or rollback code.
    pub last_error: Option<String>,
    /// Version that was most recently rolled back due to failed boots or tampering.
    pub rolled_back_version: Option<String>,
}

impl Default for UpdateState {
    fn default() -> Self {
        Self {
            schema: STATE_SCHEMA,
            channel: "stable".to_string(),
            auto_check: true,
            active: None,
            previous: None,
            pending_installer: None,
            last_check_at: None,
            last_available_version: None,
            last_error: None,
            rolled_back_version: None,
        }
    }
}

/// Validates and normalizes a [`PayloadSlot`].
///
/// Returns `None` if `version`, `min_shell_version`, or `tree_sha256` is invalid.
pub fn normalize_slot(slot: Option<PayloadSlot>) -> Option<PayloadSlot> {
    let s = slot?;
    if parse_version(&s.version).is_none() || parse_version(&s.min_shell_version).is_none() {
        return None;
    }
    if s.tree_sha256.len() != 64
        || !s
            .tree_sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return None;
    }
    Some(PayloadSlot {
        version: s.version,
        channel: normalize_channel(Some(&s.channel)).to_string(),
        min_shell_version: s.min_shell_version,
        tree_sha256: s.tree_sha256,
        archive_sha256: s.archive_sha256.filter(|h| {
            h.len() == 64
                && h.chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        }),
        installed_at: s.installed_at,
        boot_attempts: s.boot_attempts,
        confirmed: s.confirmed,
    })
}

/// Validates and normalizes an [`UpdateState`].
pub fn normalize_update_state(state: UpdateState) -> UpdateState {
    // Discard any pending installer entry whose version, path, or SHA-256 is malformed.
    let pending = state.pending_installer.and_then(|p| {
        if parse_version(&p.version).is_none() || p.path.is_empty() || p.kind.is_empty() {
            return None;
        }
        if p.sha256.len() != 64
            || !p
                .sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return None;
        }
        Some(PendingInstaller {
            version: p.version,
            channel: normalize_channel(Some(&p.channel)).to_string(),
            kind: p.kind,
            path: p.path,
            sha256: p.sha256,
            size: p.size,
            downloaded_at: p.downloaded_at,
        })
    });

    UpdateState {
        schema: STATE_SCHEMA,
        channel: normalize_channel(Some(&state.channel)).to_string(),
        auto_check: state.auto_check,
        active: normalize_slot(state.active),
        previous: normalize_slot(state.previous),
        pending_installer: pending,
        last_check_at: state.last_check_at,
        last_available_version: state.last_available_version,
        last_error: state.last_error,
        rolled_back_version: state.rolled_back_version,
    }
}

/// Reads and normalizes `<state_file>` (`update-state.json`), returning
/// [`UpdateState::default`] if missing or corrupt.
///
/// Mirrors `readState` in `electron/update/state.js`.
pub fn read_update_state(state_file: &Path) -> UpdateState {
    let Ok(raw) = fs::read_to_string(state_file) else {
        return UpdateState::default();
    };
    let Ok(parsed) = serde_json::from_str::<UpdateState>(&raw) else {
        return UpdateState::default();
    };
    normalize_update_state(parsed)
}

/// Atomically writes `state` to `state_file` via a PID-tagged `.tmp` sibling file.
///
/// Mirrors `writeState` in `electron/update/state.js`.
pub fn write_update_state(state_file: &Path, state: &UpdateState) -> Result<UpdateState, String> {
    let normalized = normalize_update_state(state.clone());
    if let Some(parent) = state_file.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = state_file.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut bytes = serde_json::to_vec_pretty(&normalized).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, state_file).map_err(|e| e.to_string())?;
    Ok(normalized)
}

/// Resolves the payload slot directory (`<payloads_dir>/<version>`) for `version`,
/// rejecting invalid versions or path traversal.
pub fn payload_slot_dir(paths: &UpdatePaths, version: &str) -> Result<PathBuf, String> {
    let parsed = parse_version(version)
        .ok_or_else(|| format!("Invalid payload slot version: {}", version))?;
    let slot_dir = paths.payloads_dir.join(&parsed.raw);
    if !slot_dir.starts_with(&paths.payloads_dir) || slot_dir == paths.payloads_dir {
        return Err(format!("Refusing unsafe payload slot path for {}", version));
    }
    Ok(slot_dir)
}

/// Activates a newly installed [`PayloadSlot`] in `update-state.json`, moving any
/// previous `active` slot of a different version into `previous`.
pub fn activate_payload_slot(
    paths: &UpdatePaths,
    new_slot: PayloadSlot,
) -> Result<UpdateState, String> {
    let normalized_new =
        normalize_slot(Some(new_slot)).ok_or_else(|| "Invalid payload slot metadata.".to_string())?;
    let mut current = read_update_state(&paths.state_file);

    // Keep the prior active slot as `previous` so we can roll back in one step if needed.
    let previous = match &current.active {
        Some(act) if act.version != normalized_new.version => Some(act.clone()),
        _ => current.previous.clone(),
    };

    current.channel = normalized_new.channel.clone();
    current.active = Some(PayloadSlot {
        boot_attempts: 0,
        confirmed: false,
        ..normalized_new
    });
    current.previous = previous;
    current.last_error = None;
    current.rolled_back_version = None;

    write_update_state(&paths.state_file, &current)
}

/// Marks the currently active payload slot as confirmed healthy (`confirmed: true, boot_attempts: 0`).
///
/// Mirrors `recordHealthy` in `electron/update/state.js`.
pub fn mark_active_payload_boot_succeeded(paths: &UpdatePaths) -> Result<UpdateState, String> {
    let mut current = read_update_state(&paths.state_file);
    let Some(mut active) = current.active.take() else {
        return Ok(current);
    };
    active.boot_attempts = 0;
    active.confirmed = true;
    current.active = Some(active);
    write_update_state(&paths.state_file, &current)
}

/// Removes payload directories under `paths.payloads_dir` that are not referenced
/// by `state.active` or `state.previous`.
///
/// Mirrors the unused-version cleanup in `collect` in `electron/update/state.js`.
pub fn prune_payload_slots(paths: &UpdatePaths, state: Option<&UpdateState>) -> Vec<String> {
    let owned_state;
    let current = match state {
        Some(s) => s,
        None => {
            owned_state = read_update_state(&paths.state_file);
            &owned_state
        }
    };
    let mut keep = BTreeSet::new();
    if let Some(act) = &current.active {
        keep.insert(act.version.clone());
    }
    if let Some(prev) = &current.previous {
        keep.insert(prev.version.clone());
    }
    let Ok(entries) = fs::read_dir(&paths.payloads_dir) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
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
        let candidate = paths.payloads_dir.join(&name);
        if fs::remove_dir_all(&candidate).is_ok() {
            removed.push(name);
        }
    }
    removed
}

/// Result returned by [`resolve_active_payload_for_launch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPayloadResolution {
    /// Verified payload directory to boot from, or `None` to boot from the base shell.
    pub payload_dir: Option<PathBuf>,
    /// Active payload version when `payload_dir` is `Some`.
    pub payload_version: Option<String>,
    /// Updated [`UpdateState`].
    pub state: UpdateState,
    /// True if a broken or crash-looping active slot was rolled back during resolution.
    pub rolled_back: bool,
}

/// Evaluates `update-state.json` at startup to decide whether to launch from an
/// installed payload slot or fall back to the bundled shell.
///
/// Enforces:
/// 1. `active.version > shell_version` and `shell_version >= active.min_shell_version`
///    (otherwise clears stale payload slots superseded by a newly installed shell).
/// 2. Crash-loop rollback when `!active.confirmed && active.boot_attempts >= MAX_FAILED_BOOTS`
///    (matching `chooseLaunch` and `MAX_ATTEMPTS` in `electron/update/state.js`).
/// 3. Directory integrity via [`verify_payload_directory`].
/// 4. Increments `boot_attempts` before launching an unconfirmed slot.
pub fn resolve_active_payload_for_launch(
    paths: &UpdatePaths,
    shell_version: &str,
    verify_hash: bool,
) -> Result<LaunchPayloadResolution, String> {
    let mut state = read_update_state(&paths.state_file);
    let Some(active) = state.active.clone() else {
        return Ok(LaunchPayloadResolution {
            payload_dir: None,
            payload_version: None,
            state,
            rolled_back: false,
        });
    };

    // Check if the native shell binary was upgraded to or past the active payload version,
    // or if the payload requires a newer shell version than the running binary.
    let newer_than_shell = compare_versions(&active.version, shell_version) == Some(1);
    let shell_compatible =
        compare_versions(shell_version, &active.min_shell_version).unwrap_or(-1) >= 0;
    if !newer_than_shell || !shell_compatible {
        state.active = None;
        state.previous = None;
        let cleared = write_update_state(&paths.state_file, &state)?;
        prune_payload_slots(paths, Some(&cleared));
        return Ok(LaunchPayloadResolution {
            payload_dir: None,
            payload_version: None,
            state: cleared,
            rolled_back: false,
        });
    }

    // Helper closure to promote `state.previous` if healthy, or clear back to the base shell.
    let promote_previous_or_clear =
        |mut st: UpdateState, failed_version: String, reason: String| -> Result<LaunchPayloadResolution, String> {
            let fallback = st.previous.take();
            if let Some(prev) = fallback {
                let prev_newer = compare_versions(&prev.version, shell_version) == Some(1);
                let prev_compat =
                    compare_versions(shell_version, &prev.min_shell_version).unwrap_or(-1) >= 0;
                if prev_newer && prev_compat {
                    if let Ok(prev_dir) = payload_slot_dir(paths, &prev.version) {
                        let check = verify_payload_directory(
                            &prev_dir,
                            Some(&prev.version),
                            Some(&prev.tree_sha256),
                            verify_hash,
                        );
                        if check.ok {
                            st.active = Some(PayloadSlot {
                                boot_attempts: 1,
                                ..prev.clone()
                            });
                            st.previous = None;
                            st.rolled_back_version = Some(failed_version);
                            st.last_error = Some(reason);
                            let next = write_update_state(&paths.state_file, &st)?;
                            prune_payload_slots(paths, Some(&next));
                            return Ok(LaunchPayloadResolution {
                                payload_dir: Some(prev_dir),
                                payload_version: Some(prev.version),
                                state: next,
                                rolled_back: true,
                            });
                        }
                    }
                }
            }
            st.active = None;
            st.previous = None;
            st.rolled_back_version = Some(failed_version);
            st.last_error = Some(reason);
            let next = write_update_state(&paths.state_file, &st)?;
            prune_payload_slots(paths, Some(&next));
            Ok(LaunchPayloadResolution {
                payload_dir: None,
                payload_version: None,
                state: next,
                rolled_back: true,
            })
        };

    // Roll back unconfirmed slots that have already failed `MAX_FAILED_BOOTS` startups.
    if !active.confirmed && active.boot_attempts >= MAX_FAILED_BOOTS {
        let ver = active.version.clone();
        let reason = format!(
            "Update {} failed to start {} times and was rolled back.",
            ver, active.boot_attempts
        );
        return promote_previous_or_clear(state, ver, reason);
    }

    let slot_dir = match payload_slot_dir(paths, &active.version) {
        Ok(d) => d,
        Err(err) => {
            let ver = active.version.clone();
            return promote_previous_or_clear(state, ver, err);
        }
    };

    let check = verify_payload_directory(
        &slot_dir,
        Some(&active.version),
        Some(&active.tree_sha256),
        verify_hash,
    );
    if !check.ok {
        let ver = active.version.clone();
        let reason = check
            .reason
            .unwrap_or_else(|| format!("Update {} failed integrity verification.", ver));
        return promote_previous_or_clear(state, ver, reason);
    }

    state.active = Some(PayloadSlot {
        boot_attempts: active.boot_attempts + 1,
        ..active.clone()
    });
    let updated = write_update_state(&paths.state_file, &state)?;
    Ok(LaunchPayloadResolution {
        payload_dir: Some(slot_dir),
        payload_version: Some(active.version),
        state: updated,
        rolled_back: false,
    })
}
