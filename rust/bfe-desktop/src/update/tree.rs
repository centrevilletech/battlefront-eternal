//! Installed build file manifest validation, relative link resolution, and
//! deterministic payload tree hashing (`sha256` over file paths, modes, and contents).
//!
//! Mirrors `electron/update/tree.js` (`TREE_INDEX`, `safeRelative`, `linkTarget`,
//! `validateTree`, `hashFile`, `walkTree`, `readTreeIndex`).
//!
//! ## Payload Tree Hashing Format
//! - Every regular file in a payload directory (excluding `.DS_Store`, `__pycache__`,
//!   `*.pyc`, and `.bfe-payload.json`) is visited in lexicographical path order.
//! - Each file contributes a single NUL-separated record line to the master SHA-256:
//!   `"file\0<path>\0<mode>\0<size>\0<sha256>\n"`.
//! - The resulting digest (`tree_sha256`) is stored inside `.bfe-payload.json` and
//!   signed inside the release feed's `payload.treeSha256` field.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::paths::resolve_from;
use crate::update::version::parse_version;

/// Relative path to a packaged build's embedded file hash index.
///
/// Every packaged build carries the hashes of its own files at `resources/bfe-tree.json`
/// so an update can reuse unchanged files already on disk without re-reading them.
///
/// Mirrors `TREE_INDEX` in `electron/update/tree.js`.
pub const TREE_INDEX: &str = "resources/bfe-tree.json";

/// Maximum file count allowed in an update manifest (`100_000`).
///
/// Mirrors `MAX_FILES` in `electron/update/tree.js`.
pub const MAX_TREE_FILES: usize = 100_000;

/// Maximum total uncompressed byte size allowed in an update manifest (`8 GiB`).
///
/// Mirrors `MAX_TOTAL` in `electron/update/tree.js`.
pub const MAX_TREE_TOTAL_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Checks that `value` is a safe `/`-separated relative path (`1..=512` chars)
/// with no `.` or `..` segments, no Windows-forbidden characters (`\:*?"<>|`),
/// and no ASCII control characters (`0x00..=0x1f`).
///
/// Mirrors `safeRelative` in `electron/update/tree.js`.
pub fn safe_relative(value: &str) -> bool {
    // Reject empty paths or paths longer than 512 bytes.
    if value.is_empty() || value.len() > 512 {
        return false;
    }
    // Every `/`-separated segment must be non-empty, not `.` or `..`, and free of
    // control characters and Windows-reserved path punctuation.
    value.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && !part
                .chars()
                .any(|c| (c as u32) <= 0x1f || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    })
}

/// Resolves a symbolic link's relative `target` against `link_path` and verifies
/// that the resulting normalized path remains inside the version folder.
///
/// Returns `None` if `target` is absolute, contains backslashes, or escapes the root.
/// Mirrors `linkTarget` in `electron/update/tree.js`.
pub fn link_target(link_path: &str, target: &str) -> Option<String> {
    // Reject absolute symlink targets or Windows backslashes.
    if target.is_empty() || target.starts_with('/') || target.contains('\\') {
        return None;
    }
    // Resolve `target` relative to the parent directory of `link_path`.
    let parent = match link_path.rfind('/') {
        Some(idx) => &link_path[..idx],
        None => "",
    };
    let joined = if parent.is_empty() {
        target.to_string()
    } else {
        format!("{}/{}", parent, target)
    };
    // Collapse `.` and `..` segments; fail immediately if `..` pops past the root.
    let mut stack: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            stack.pop()?;
        } else {
            stack.push(part);
        }
    }
    let resolved = stack.join("/");
    if safe_relative(&resolved) {
        Some(resolved)
    } else {
        None
    }
}

/// Top-level file and directory names excluded when computing a payload tree hash.
pub const IGNORED_NAMES: &[&str] = &[".DS_Store", "__pycache__", ".bfe-payload.json"];

/// Single file record hashed inside a payload tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayloadTreeFile {
    /// Forward-slash relative path inside the payload root.
    pub path: String,
    /// Permission mode string (`"755"` for executable files, `"644"` otherwise).
    pub mode: String,
    /// File byte size.
    pub size: u64,
    /// Lowercase hex SHA-256 of the file contents.
    pub sha256: String,
}

/// Complete hash summary of a desktop update payload directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadTreeSummary {
    /// Overall tree SHA-256 digest (over `"file\0<path>\0<mode>\0<size>\0<sha256>\n"` records).
    pub tree_sha256: String,
    /// Number of files included in the payload tree.
    pub file_count: usize,
    /// Total byte size across all files in the payload tree.
    pub total_bytes: u64,
    /// Sorted list of hashed payload files.
    pub files: Vec<PayloadTreeFile>,
}

/// Deserialized representation of `<payload_dir>/.bfe-payload.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadMetadata {
    /// Payload schema format version (`1`).
    pub format: u32,
    /// Semantic version of the payload (`"0.2.1"`).
    pub version: String,
    /// Release channel (`"stable"` or `"nightly"`).
    #[serde(default)]
    pub channel: Option<String>,
    /// Expected tree SHA-256 computed by [`hash_payload_tree`].
    pub tree_sha256: String,
    /// Expected file count in the payload.
    pub file_count: usize,
    /// Expected total bytes across all files in the payload.
    pub total_bytes: u64,
    /// Optional timestamp when the payload was installed.
    #[serde(default)]
    pub installed_at: Option<String>,
}

/// Checks whether a filename or path segment should be ignored during payload tree hashing.
fn is_ignored(name: &str) -> bool {
    IGNORED_NAMES.contains(&name) || name.ends_with(".pyc")
}

/// Computes the size and lowercase hex SHA-256 digest of `file_path`.
///
/// Mirrors `hashFile` in `electron/update/tree.js`.
pub fn hash_file(file_path: &Path) -> Result<(u64, String), String> {
    // Stream the file in 64 KiB chunks to compute both byte length and SHA-256 digest.
    let mut file = File::open(file_path)
        .map_err(|e| format!("Failed opening {}: {}", file_path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed reading {}: {}", file_path.display(), e))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((size, hex_encode(&hasher.finalize())))
}

/// Encodes a byte slice as lowercase hexadecimal.
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

/// Returns `"755"` when any executable bit (`0o111`) is set on Unix, or `"644"` otherwise.
#[cfg(unix)]
fn file_mode_str(meta: &fs::Metadata) -> &'static str {
    use std::os::unix::fs::PermissionsExt;
    if (meta.permissions().mode() & 0o111) != 0 {
        "755"
    } else {
        "644"
    }
}

/// Returns `"644"` on non-Unix platforms where POSIX execute bits are not tracked.
#[cfg(not(unix))]
fn file_mode_str(_meta: &fs::Metadata) -> &'static str {
    "644"
}

/// Recursively walks `root` in lexicographical entry order and collects regular files.
fn walk_payload_files(
    root: &Path,
    rel: &str,
    out: &mut Vec<(String, PathBuf, String)>,
) -> Result<(), String> {
    let current = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    // Sort directory entries by filename so the resulting tree digest is platform-independent.
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(&current)
        .map_err(|e| format!("Failed reading {}: {}", current.display(), e))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|a| a.file_name());

    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Payload contains non-UTF-8 filename.".to_string())?;
        if is_ignored(&name) {
            continue;
        }
        let rel_path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", rel, name)
        };
        let full_path = root.join(&rel_path);
        let meta = fs::symlink_metadata(&full_path)
            .map_err(|e| format!("Failed stating {}: {}", full_path.display(), e))?;
        // Reject symbolic links inside a payload directory.
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Payload cannot contain symbolic links: {}",
                rel_path
            ));
        }
        if meta.is_dir() {
            walk_payload_files(root, &rel_path, out)?;
        } else if meta.is_file() {
            out.push((rel_path, full_path, file_mode_str(&meta).to_string()));
        }
    }
    Ok(())
}

/// Computes the deterministic SHA-256 tree hash and file manifest of `root_dir`.
///
/// Rejects symbolic links, skips [`IGNORED_NAMES`] and `*.pyc` files, sorts
/// entries lexicographically by relative path, and feeds each file record into
/// a master SHA-256 hasher as `"file\0<path>\0<mode>\0<size>\0<sha256>\n"`.
pub fn hash_payload_tree(root_dir: &Path) -> Result<PayloadTreeSummary, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = resolve_from(&cwd, root_dir);
    let mut files = Vec::new();
    walk_payload_files(&root, "", &mut files)?;

    let mut tree_hasher = Sha256::new();
    let mut total_bytes = 0u64;
    let mut hashed_files = Vec::with_capacity(files.len());

    // Hash each file and feed its NUL-delimited manifest line into the master tree SHA-256.
    for (path, full, mode) in files {
        let (size, sha256) = hash_file(&full)?;
        total_bytes += size;
        let line = format!("file\0{}\0{}\0{}\0{}\n", path, mode, size, sha256);
        tree_hasher.update(line.as_bytes());
        hashed_files.push(PayloadTreeFile {
            path,
            mode,
            size,
            sha256,
        });
    }

    Ok(PayloadTreeSummary {
        tree_sha256: hex_encode(&tree_hasher.finalize()),
        file_count: hashed_files.len(),
        total_bytes,
        files: hashed_files,
    })
}

/// Reads and validates `<payload_dir>/.bfe-payload.json`.
///
/// Returns `None` if the metadata file is missing, unparsable, or has an invalid
/// `format`, `version`, or `treeSha256`.
pub fn read_payload_metadata(payload_dir: &Path) -> Option<PayloadMetadata> {
    // Parse `.bfe-payload.json` and check schema format, SemVer version, and 64-char hex digest.
    let raw = fs::read_to_string(payload_dir.join(".bfe-payload.json")).ok()?;
    let data: PayloadMetadata = serde_json::from_str(&raw).ok()?;
    if data.format != 1 {
        return None;
    }
    parse_version(&data.version)?;
    if data.tree_sha256.len() != 64
        || !data
            .tree_sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return None;
    }
    Some(data)
}

/// Writes `<payload_dir>/.bfe-payload.json` with a trailing newline.
pub fn write_payload_metadata(
    payload_dir: &Path,
    metadata: &PayloadMetadata,
) -> Result<PayloadMetadata, String> {
    // Serialize `.bfe-payload.json` with `format: 1` and a trailing newline.
    let out = PayloadMetadata {
        format: 1,
        version: metadata.version.clone(),
        channel: metadata.channel.clone(),
        tree_sha256: metadata.tree_sha256.clone(),
        file_count: metadata.file_count,
        total_bytes: metadata.total_bytes,
        installed_at: Some(
            metadata
                .installed_at
                .clone()
                .unwrap_or_else(|| "2026-01-01T00:00:00.000Z".to_string()),
        ),
    };
    let mut bytes = serde_json::to_vec_pretty(&out).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    fs::write(payload_dir.join(".bfe-payload.json"), bytes).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Result of verifying a staged or active payload directory via [`verify_payload_directory`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyPayloadResult {
    /// True if all required files exist, package version matches, and optionally the full tree hash matches.
    pub ok: bool,
    /// Human-readable failure reason when `ok == false`.
    pub reason: Option<String>,
    /// Verified payload metadata when `ok == true`.
    pub metadata: Option<PayloadMetadata>,
}

/// Required files inside a self-contained desktop payload directory.
pub const REQUIRED_PAYLOAD_FILES: &[&str] = &[
    "package.json",
    "electron/boot.js",
    "src/index.html",
    "tools/prepare_content.py",
    "tools/content-version.json",
];

/// Verifies a staged or active payload directory's `.bfe-payload.json` metadata,
/// required files, `package.json` version, and (when `verify_hash` is true) full
/// [`hash_payload_tree`] digest.
pub fn verify_payload_directory(
    payload_dir: &Path,
    expected_version: Option<&str>,
    expected_tree_sha256: Option<&str>,
    verify_hash: bool,
) -> VerifyPayloadResult {
    // Check directory existence and `.bfe-payload.json` metadata validity first.
    if payload_dir.as_os_str().is_empty() || !payload_dir.exists() {
        return VerifyPayloadResult {
            ok: false,
            reason: Some("Payload directory does not exist.".to_string()),
            metadata: None,
        };
    }
    let Some(metadata) = read_payload_metadata(payload_dir) else {
        return VerifyPayloadResult {
            ok: false,
            reason: Some("Payload metadata is missing or invalid.".to_string()),
            metadata: None,
        };
    };
    // Verify that the metadata matches the caller's expected version and tree hash.
    if let Some(exp_ver) = expected_version {
        if metadata.version != exp_ver {
            return VerifyPayloadResult {
                ok: false,
                reason: Some(format!(
                    "Payload version {} does not match expected {}.",
                    metadata.version, exp_ver
                )),
                metadata: None,
            };
        }
    }
    if let Some(exp_tree) = expected_tree_sha256 {
        if metadata.tree_sha256 != exp_tree {
            return VerifyPayloadResult {
                ok: false,
                reason: Some("Payload tree hash does not match update state.".to_string()),
                metadata: None,
            };
        }
    }
    // Check that all required application entrypoints and tool scripts exist on disk.
    for rel in REQUIRED_PAYLOAD_FILES {
        if !payload_dir.join(rel).exists() {
            return VerifyPayloadResult {
                ok: false,
                reason: Some(format!("Payload is missing required file: {}", rel)),
                metadata: None,
            };
        }
    }
    let pkg_raw = match fs::read_to_string(payload_dir.join("package.json")) {
        Ok(r) => r,
        Err(_) => {
            return VerifyPayloadResult {
                ok: false,
                reason: Some("Payload package.json is unreadable.".to_string()),
                metadata: None,
            };
        }
    };
    let pkg_val: serde_json::Value = match serde_json::from_str(&pkg_raw) {
        Ok(v) => v,
        Err(_) => {
            return VerifyPayloadResult {
                ok: false,
                reason: Some("Payload package.json is unreadable.".to_string()),
                metadata: None,
            };
        }
    };
    if pkg_val.get("version").and_then(|v| v.as_str()) != Some(metadata.version.as_str()) {
        return VerifyPayloadResult {
            ok: false,
            reason: Some("Payload package.json version does not match metadata.".to_string()),
            metadata: None,
        };
    }
    // Optionally recompute the full payload tree hash to detect any post-install tampering.
    if verify_hash {
        let actual = match hash_payload_tree(payload_dir) {
            Ok(a) => a,
            Err(e) => {
                return VerifyPayloadResult {
                    ok: false,
                    reason: Some(e),
                    metadata: None,
                };
            }
        };
        if actual.tree_sha256 != metadata.tree_sha256 {
            return VerifyPayloadResult {
                ok: false,
                reason: Some("Payload tree hash verification failed.".to_string()),
                metadata: None,
            };
        }
    }

    VerifyPayloadResult {
        ok: true,
        reason: None,
        metadata: Some(metadata),
    }
}
