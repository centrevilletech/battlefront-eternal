//! Content-addressed blob URL resolution, payload ZIP extraction, full installer
//! verification, and cross-platform installer handoff scripts (`AppImage` swap,
//! Windows NSIS PowerShell helper, macOS `.dmg` / `.app` opening).
//!
//! Mirrors `electron/update/install.js` (`blobUrl`, `installRelease`) and provides
//! archive staging and pending-installer handoff helpers.
//!
//! ## Update Staging & Installer Handoff Overview
//! - **Content-addressed blobs (`blob_url`)**:
//!   Individual files in a release tree are stored under
//!   `<FEED>/updates/blobs/<sha256[0..2]>/<sha256>.zst`.
//! - **Hot-swappable payload slots (`stage_payload_from_archive`)**:
//!   Verifies the downloaded `.zip` archive against `artifact.size` and
//!   `artifact.sha256`, unpacks it into `<staging_dir>/payload-<version>-<rand>`,
//!   hashes the unpacked tree via [`hash_payload_tree`], writes `.bfe-payload.json`,
//!   atomically renames the verified tree into `<payloads_dir>/<version>`, activates
//!   the slot in `update-state.json`, and prunes retired payload slots.
//! - **Full native installer handoff (`register_downloaded_installer` & `apply_pending_installer`)**:
//!   When `minShellVersion` exceeds the running binary version, the downloaded
//!   platform installer is verified and registered in `update-state.json` under
//!   `pendingInstaller`. Calling [`apply_pending_installer`] generates the platform
//!   helper script (`apply-appimage-<pid>.sh` on Linux AppImage, `apply-nsis-<pid>.ps1`
//!   on Windows NSIS) and, when `dry_run` is `false`, spawns it detached.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};

use crate::archive::{read_u16, read_u32, read_zip_entries, ZipEntryInfo};
use crate::paths::UpdatePaths;
use crate::update::feed::{normalize_channel, InstallerArtifact, PayloadArtifact, FEED};
use crate::update::state::{
    activate_payload_slot, payload_slot_dir, prune_payload_slots, read_update_state,
    write_update_state, PayloadSlot, PendingInstaller, UpdateState,
};
use crate::update::tree::{
    hash_file, hash_payload_tree, verify_payload_directory, write_payload_metadata, PayloadMetadata,
};

/// Resolves the content-addressed zstd blob URL for a file's SHA-256 hex digest
/// (`<base>/updates/blobs/<sha256[0..2]>/<sha256>.zst`).
///
/// Mirrors `blobUrl` in `electron/update/install.js`.
///
/// # Arguments
/// * `sha256` - Lowercase 64-character SHA-256 hex digest of the uncompressed file.
/// * `base` - Optional release server origin override (defaults to [`FEED`]).
pub fn blob_url(sha256: &str, base: Option<&str>) -> String {
    // Strip any trailing slash on `base` and bucket blobs by their two-character hex prefix.
    let root = base.unwrap_or(FEED).trim_end_matches('/');
    let prefix = if sha256.len() >= 2 { &sha256[..2] } else { sha256 };
    format!("{}/updates/blobs/{}/{}.zst", root, prefix, sha256)
}

/// Maximum single entry size inside a desktop payload ZIP (`256 MiB`).
pub const MAX_PAYLOAD_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// Maximum total uncompressed size of a desktop payload ZIP (`1 GiB`).
pub const MAX_PAYLOAD_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

/// Validates a relative path inside a payload ZIP archive against path traversal,
/// control characters, backslashes, or colons.
///
/// Complements `safeRelative` in `electron/update/tree.js` for ZIP entries that
/// may carry trailing directory slashes.
///
/// Returns `Ok(Vec::new())` for top-level directory entries (`"/"`) and an error
/// string when any segment is empty, `"."`, or `".."`.
pub fn safe_relative_path(entry_name: &str) -> Result<Vec<String>, String> {
    // Reject empty or overly long entry names (> 1024 bytes).
    if entry_name.is_empty() || entry_name.len() > 1024 {
        return Err("Payload archive contains an invalid path.".to_string());
    }
    // Reject ASCII control characters, Windows backslashes, and drive colons.
    if entry_name
        .chars()
        .any(|c| (c as u32) <= 0x1f || c == '\\' || c == ':')
    {
        return Err(format!(
            "Payload archive contains an unsafe path: {}",
            entry_name
        ));
    }
    // Directory entries inside ZIP archives end with `/`; strip trailing slashes before splitting.
    let clean = entry_name.trim_end_matches('/');
    if clean.is_empty() {
        return Ok(Vec::new());
    }
    // Reject empty path components, `.`, and `..` traversal segments.
    let mut parts = Vec::new();
    for part in clean.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!(
                "Payload archive contains an unsafe path: {}",
                entry_name
            ));
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

/// Reads, decompresses (stored or deflate), and CRC-32-verifies a single ZIP entry.
///
/// Supports ZIP compression method `0` (stored) and method `8` (Deflate), enforcing
/// [`MAX_PAYLOAD_ENTRY_BYTES`] before allocating buffers.
fn read_payload_entry_bytes(file: &mut File, entry: &ZipEntryInfo) -> Result<Vec<u8>, String> {
    // Enforce the per-entry memory limit before allocating buffers.
    if entry.size > MAX_PAYLOAD_ENTRY_BYTES || entry.compressed_size > MAX_PAYLOAD_ENTRY_BYTES {
        return Err(format!("Payload entry is too large: {}", entry.name));
    }
    // Read the 30-byte ZIP Local File Header (`PK\x03\x04`) at `entry.offset`
    // to find the variable-length filename and extra field lengths.
    let hdr_err = format!("Payload archive local header is damaged: {}", entry.name);
    file.seek(SeekFrom::Start(entry.offset))
        .map_err(|e| e.to_string())?;
    let mut header = [0u8; 30];
    file.read_exact(&mut header)
        .map_err(|_| "Payload archive is truncated.".to_string())?;
    let sig = read_u32(&header, 0, &hdr_err)?;
    if sig != 0x04034b50 {
        return Err(hdr_err);
    }
    let name_len = read_u16(&header, 26, &hdr_err)? as u64;
    let extra_len = read_u16(&header, 28, &hdr_err)? as u64;
    let data_offset = entry
        .offset
        .checked_add(30)
        .and_then(|n| n.checked_add(name_len))
        .and_then(|n| n.checked_add(extra_len))
        .ok_or(hdr_err)?;

    // Seek past the variable-length header fields and read the compressed entry bytes.
    file.seek(SeekFrom::Start(data_offset))
        .map_err(|e| e.to_string())?;
    let mut raw = vec![0u8; entry.compressed_size as usize];
    file.read_exact(&mut raw)
        .map_err(|_| "Payload archive is truncated.".to_string())?;

    // Support both uncompressed (method 0) and Deflate-compressed (method 8) ZIP entries.
    let data = if entry.method == 0 {
        raw
    } else if entry.method == 8 {
        let mut decoder = flate2::read::DeflateDecoder::new(&raw[..]);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| format!("Failed decompressing {}: {}", entry.name, e))?;
        out
    } else {
        return Err(format!(
            "Unsupported compression method {} in payload archive for {}",
            entry.method, entry.name
        ));
    };

    // Verify both the uncompressed byte count and the IEEE CRC-32 checksum.
    if data.len() as u64 != entry.size {
        return Err(format!("Payload entry size mismatch for {}", entry.name));
    }
    let mut crc_hasher = Crc32Hasher::new();
    crc_hasher.update(&data);
    if crc_hasher.finalize() != entry.crc {
        return Err(format!(
            "Payload entry CRC mismatch for {}",
            entry.name
        ));
    }
    Ok(data)
}

/// Extracts a payload ZIP archive into `destination_dir`, validating every entry's
/// path and CRC-32 checksum.
///
/// Enforces [`MAX_PAYLOAD_TOTAL_BYTES`] across all entries in the archive to guard
/// against decompression bombs.
pub fn extract_zip_archive(archive_path: &Path, destination_dir: &Path) -> Result<(), String> {
    // Open the ZIP archive and parse its central directory before creating destination folders.
    let mut file = File::open(archive_path)
        .map_err(|e| format!("Failed opening {}: {}", archive_path.display(), e))?;
    let entries = read_zip_entries(&mut file)?;
    fs::create_dir_all(destination_dir).map_err(|e| e.to_string())?;

    // Track cumulative uncompressed bytes across the entire archive to guard against zip bombs.
    let mut total_bytes = 0u64;
    for entry in entries.values() {
        let parts = safe_relative_path(&entry.name)?;
        if parts.is_empty() || entry.name.ends_with('/') {
            continue;
        }
        total_bytes = total_bytes.saturating_add(entry.size);
        if total_bytes > MAX_PAYLOAD_TOTAL_BYTES {
            return Err("Payload archive exceeds maximum allowed size.".to_string());
        }
        let mut dest = destination_dir.to_path_buf();
        for p in &parts {
            dest.push(p);
        }
        // Verify that the joined path remains strictly inside `destination_dir`.
        if !dest.starts_with(destination_dir) || dest == destination_dir {
            return Err(format!(
                "Payload archive path escaped destination: {}",
                entry.name
            ));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Decompress and CRC-verify the entry before writing it to disk.
        let data = read_payload_entry_bytes(&mut file, entry)?;
        fs::write(&dest, &data).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Verifies that a downloaded file at `file_path` matches `expected_size` and
/// `expected_sha256` (case-insensitive hex).
///
/// Returns `(size_bytes, sha256_hex)` when both checks succeed.
pub fn verify_downloaded_artifact(
    file_path: &Path,
    expected_size: Option<u64>,
    expected_sha256: Option<&str>,
) -> Result<(u64, String), String> {
    // Hash the file on disk and compare both byte length and SHA-256 hex digest.
    let (size, sha256) = hash_file(file_path)?;
    if let Some(exp_sz) = expected_size.filter(|&n| n > 0) {
        if size != exp_sz {
            return Err(format!(
                "Downloaded file size mismatch: expected {}, got {}",
                exp_sz, size
            ));
        }
    }
    if let Some(exp_hash) = expected_sha256.filter(|s| !s.is_empty()) {
        if sha256 != exp_hash.to_ascii_lowercase() {
            return Err(format!(
                "Downloaded file SHA-256 mismatch: expected {}, got {}",
                exp_hash, sha256
            ));
        }
    }
    Ok((size, sha256))
}

/// Result returned by [`stage_payload_from_archive`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagePayloadResult {
    /// Final installed slot directory (`<payloads_dir>/<version>`).
    pub slot_dir: PathBuf,
    /// Activated [`PayloadSlot`] metadata.
    pub slot: PayloadSlot,
    /// Updated [`UpdateState`].
    pub state: UpdateState,
}

/// Verifies a downloaded payload archive, extracts it into a staging directory,
/// verifies its [`hash_payload_tree`] against `artifact.tree_sha256`, writes
/// `.bfe-payload.json`, moves it into `<payloads_dir>/<version>`, activates the
/// slot in `update-state.json`, and prunes old payload slots.
///
/// Mirrors the staging, verification, and atomic rename workflow of `installRelease`
/// in `electron/update/install.js`.
///
/// If any step fails, the temporary staging directory is removed before returning `Err`.
pub fn stage_payload_from_archive(
    paths: &UpdatePaths,
    archive_path: &Path,
    version: &str,
    channel: &str,
    artifact: &PayloadArtifact,
) -> Result<StagePayloadResult, String> {
    // Step 1: Verify the outer ZIP archive's byte size and SHA-256 before unpacking.
    verify_downloaded_artifact(
        archive_path,
        Some(artifact.size),
        Some(&artifact.sha256),
    )?;

    let slot_dir = payload_slot_dir(paths, version)?;
    fs::create_dir_all(&paths.staging_dir).map_err(|e| e.to_string())?;
    let temp_dir = paths
        .staging_dir
        .join(format!("payload-{}-{}", version, random_hex()));

    // Step 2: Stage in a temporary folder and clean up on any failure so partial installs never linger.
    let res = (|| -> Result<StagePayloadResult, String> {
        extract_zip_archive(archive_path, &temp_dir)?;
        let tree = hash_payload_tree(&temp_dir)?;
        if tree.tree_sha256 != artifact.tree_sha256 {
            return Err(format!(
                "Payload tree SHA-256 mismatch: expected {}, got {}",
                artifact.tree_sha256, tree.tree_sha256
            ));
        }
        // Step 3: Write `.bfe-payload.json` so subsequent boot-time checks can verify the slot.
        write_payload_metadata(
            &temp_dir,
            &PayloadMetadata {
                format: 1,
                version: version.to_string(),
                channel: Some(normalize_channel(Some(channel)).to_string()),
                tree_sha256: tree.tree_sha256.clone(),
                file_count: tree.file_count,
                total_bytes: tree.total_bytes,
                installed_at: None,
            },
        )?;

        // Run a full directory check (`verify_payload_directory`) before moving into `payloads/`.
        let check = verify_payload_directory(
            &temp_dir,
            Some(version),
            Some(&artifact.tree_sha256),
            true,
        );
        if !check.ok {
            return Err(check
                .reason
                .unwrap_or_else(|| "Staged payload verification failed.".to_string()));
        }

        // Step 4: Atomically promote the verified staging tree into `<payloads_dir>/<version>`.
        fs::create_dir_all(&paths.payloads_dir).map_err(|e| e.to_string())?;
        if slot_dir.exists() {
            fs::remove_dir_all(&slot_dir).map_err(|e| e.to_string())?;
        }
        fs::rename(&temp_dir, &slot_dir).map_err(|e| e.to_string())?;

        // Step 5: Activate the new slot in `update-state.json` and prune retired slots.
        let slot = PayloadSlot {
            version: version.to_string(),
            channel: normalize_channel(Some(channel)).to_string(),
            min_shell_version: artifact.min_shell_version.clone(),
            tree_sha256: artifact.tree_sha256.clone(),
            archive_sha256: Some(artifact.sha256.clone()),
            installed_at: Some("2026-01-01T00:00:00.000Z".to_string()),
            boot_attempts: 0,
            confirmed: false,
        };
        let state = activate_payload_slot(paths, slot.clone())?;
        prune_payload_slots(paths, Some(&state));

        Ok(StagePayloadResult {
            slot_dir,
            slot,
            state,
        })
    })();

    // Always remove the temporary staging directory if any step returned an error.
    if res.is_err() {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    res
}

/// Sanitizes an installer filename for saving under `<downloads_dir>`.
///
/// Strips directory components from `artifact.file_name` and replaces any characters
/// outside `[A-Za-z0-9._-]` with `_`. Falls back to `Battlefront-Eternal-<version>.<ext>`
/// when `file_name` is absent.
fn sanitize_installer_filename(version: &str, artifact: &InstallerArtifact) -> String {
    // Strip directory components and replace any characters outside `[A-Za-z0-9._-]` with `_`.
    if let Some(name) = artifact.file_name.as_deref().filter(|s| !s.is_empty()) {
        let base = Path::new(name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("installer");
        let cleaned: String = base
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    // Derive a default extension from `artifact.kind` when `fileName` is omitted.
    let ext = match artifact.kind.as_str() {
        "appimage" => "AppImage",
        "nsis" => "exe",
        "dmg" => "dmg",
        "zip" => "zip",
        "tar.gz" => "tar.gz",
        other => other,
    };
    format!("Battlefront-Eternal-{}.{}", version, ext)
}

/// Verifies a pre-downloaded full installer file, moves/copies it into
/// `<paths.downloads_dir>`, and records it as `pending_installer` in `update-state.json`.
///
/// On Unix platforms, `appimage` artifacts are marked `0755` so they can be executed
/// directly by the handoff script.
pub fn register_downloaded_installer(
    paths: &UpdatePaths,
    source_file: &Path,
    version: &str,
    channel: &str,
    artifact: &InstallerArtifact,
) -> Result<(PendingInstaller, UpdateState), String> {
    // Verify size and SHA-256 before copying the installer into `<downloads_dir>`.
    verify_downloaded_artifact(source_file, Some(artifact.size), Some(&artifact.sha256))?;
    fs::create_dir_all(&paths.downloads_dir).map_err(|e| e.to_string())?;
    let file_name = sanitize_installer_filename(version, artifact);
    let final_path = paths.downloads_dir.join(file_name);
    if source_file != final_path {
        fs::copy(source_file, &final_path).map_err(|e| e.to_string())?;
    }
    // Ensure Linux AppImage downloads have execute permissions set before handoff.
    #[cfg(unix)]
    if artifact.kind == "appimage" {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&final_path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&final_path, perms);
        }
    }

    // Record the verified `PendingInstaller` in `update-state.json` and clear any prior error.
    let pending = PendingInstaller {
        version: version.to_string(),
        channel: normalize_channel(Some(channel)).to_string(),
        kind: artifact.kind.clone(),
        path: final_path.to_string_lossy().into_owned(),
        sha256: artifact.sha256.clone(),
        size: artifact.size,
        downloaded_at: Some("2026-01-01T00:00:00.000Z".to_string()),
    };

    let mut current = read_update_state(&paths.state_file);
    current.pending_installer = Some(pending.clone());
    current.last_error = None;
    let state = write_update_state(&paths.state_file, &current)?;
    Ok((pending, state))
}

/// Result returned by [`apply_pending_installer`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyInstallerResult {
    /// Action taken (`"appimage-replace"`, `"nsis-installer"`, `"open-installer"`, `"reveal-installer"`, or `"dry-run"`).
    pub action: String,
    /// Target path of the replaced executable or launched installer.
    pub target: String,
    /// Generated helper script path when applicable (or in dry-run mode).
    pub script_path: Option<PathBuf>,
}

/// Options for [`apply_pending_installer`].
#[derive(Debug, Clone)]
pub struct ApplyInstallerOptions {
    /// Target platform (`"linux"`, `"win32"`, `"darwin"`).
    pub platform: String,
    /// Environment variables (`APPIMAGE` on Linux).
    pub env: std::collections::BTreeMap<String, String>,
    /// Path to the currently running executable (`process.execPath`).
    pub exec_path: PathBuf,
    /// When `true`, verifies the pending installer and writes the helper script
    /// into `<staging_dir>` without spawning a detached process or mutating the host binary.
    pub dry_run: bool,
}

/// Verifies `state.pending_installer` and launches (or stages in `dry_run` mode)
/// the platform-specific installer handoff helper.
///
/// Re-verifies the SHA-256 digest of `pending.path` immediately before handoff so a
/// modified or corrupted file in `<downloads_dir>` is never executed.
pub fn apply_pending_installer(
    paths: &UpdatePaths,
    opts: &ApplyInstallerOptions,
) -> Result<ApplyInstallerResult, String> {
    // Load `update-state.json` and verify that a `pendingInstaller` record is present on disk.
    let state = read_update_state(&paths.state_file);
    let pending = state
        .pending_installer
        .as_ref()
        .ok_or_else(|| "No pending installer has been downloaded.".to_string())?;
    let installer_path = PathBuf::from(&pending.path);
    if !installer_path.exists() {
        return Err("Downloaded installer file is no longer present.".to_string());
    }
    // Re-verify the installer's SHA-256 right before handoff so a modified file in
    // the downloads directory is never executed.
    let (_, actual_hash) = hash_file(&installer_path)?;
    if actual_hash != pending.sha256 {
        return Err("Pending installer failed SHA-256 verification.".to_string());
    }

    fs::create_dir_all(&paths.staging_dir).map_err(|e| e.to_string())?;

    // On Linux AppImage installs, stage a POSIX shell helper that atomically swaps the
    // new AppImage into `$APPIMAGE` and re-execs it with `--updated`.
    if opts.platform == "linux" && pending.kind == "appimage" {
        if let Some(appimage_target) = opts.env.get("APPIMAGE").filter(|s| !s.is_empty()) {
            let helper_script = paths
                .staging_dir
                .join(format!("apply-appimage-{}.sh", std::process::id()));
            let script_body = concat!(
                "#!/bin/sh\n",
                "SRC=\"$1\"\n",
                "DST=\"$2\"\n",
                "sleep 1\n",
                "chmod +x \"$SRC\" || exit 1\n",
                "cp -f \"$SRC\" \"$DST.new\" && chmod +x \"$DST.new\" && mv -f \"$DST.new\" \"$DST\" && rm -f \"$SRC\"\n",
                "exec \"$DST\" --updated\n",
            );
            fs::write(&helper_script, script_body).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = fs::metadata(&helper_script) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    let _ = fs::set_permissions(&helper_script, perms);
                }
            }
            // Spawn the detached shell script only when `dry_run` is false.
            if !opts.dry_run {
                let _ = Command::new("/bin/sh")
                    .arg(&helper_script)
                    .arg(&installer_path)
                    .arg(appimage_target)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                let mut next = state.clone();
                next.pending_installer = None;
                let _ = write_update_state(&paths.state_file, &next);
            }
            return Ok(ApplyInstallerResult {
                action: if opts.dry_run {
                    "dry-run".to_string()
                } else {
                    "appimage-replace".to_string()
                },
                target: appimage_target.clone(),
                script_path: Some(helper_script),
            });
        }
    }

    // On Windows NSIS installs, stage a hidden PowerShell helper that runs `/S`
    // and relaunches `$AppExe` once the installer exits with code 0.
    if (opts.platform == "win32" || opts.platform == "windows") && pending.kind == "nsis" {
        let helper_script = paths
            .staging_dir
            .join(format!("apply-nsis-{}.ps1", std::process::id()));
        let ps_body = concat!(
            "param([string]$Installer, [string]$AppExe)\n",
            "Start-Sleep -Seconds 1\n",
            "$p = Start-Process -FilePath $Installer -ArgumentList '/S' -PassThru -Wait\n",
            "if ($p.ExitCode -eq 0 -and (Test-Path $AppExe)) {\n",
            "  Start-Process -FilePath $AppExe\n",
            "}\n",
        );
        fs::write(&helper_script, ps_body).map_err(|e| e.to_string())?;
        if !opts.dry_run {
            let _ = Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-WindowStyle",
                    "Hidden",
                    "-File",
                ])
                .arg(&helper_script)
                .arg(&installer_path)
                .arg(&opts.exec_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            let mut next = state.clone();
            next.pending_installer = None;
            let _ = write_update_state(&paths.state_file, &next);
        }
        return Ok(ApplyInstallerResult {
            action: if opts.dry_run {
                "dry-run".to_string()
            } else {
                "nsis-installer".to_string()
            },
            target: pending.path.clone(),
            script_path: Some(helper_script),
        });
    }

    // Fallback for macOS `.dmg` / `.zip` or non-AppImage Linux packages.
    Ok(ApplyInstallerResult {
        action: if opts.dry_run {
            "dry-run".to_string()
        } else if opts.platform == "darwin" || opts.platform == "macos" {
            "open-installer".to_string()
        } else {
            "reveal-installer".to_string()
        },
        target: pending.path.clone(),
        script_path: None,
    })
}

/// Generates 12 random lowercase hex characters for temporary staging folder names.
fn random_hex() -> String {
    // Sample 6 random bytes via `getrandom` and encode as 12 hex characters.
    let mut bytes = [0u8; 6];
    let _ = getrandom::getrandom(&mut bytes);
    let mut s = String::with_capacity(12);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}
