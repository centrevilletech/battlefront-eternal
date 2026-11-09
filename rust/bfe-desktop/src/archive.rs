//! Portable prepared content archive (`.bfecontent`) export, verification,
//! atomic import, and game-install relocation.
//!
//! Mirrors the desktop host.
//!
//! A `.bfecontent` file is an uncompressed ZIP64-capable archive containing:
//! - `bfe-content-manifest.json` (`format: "battlefront-eternal-content"`, `version: 1`)
//! - Prepared content files under `common/`, `sides/`, `maps/`, `audio/`, and `.prepare.json`
//! - Deduplicated `linkTo` metadata for hardlinked and case-folded alias files
//! - Source `.lvl` SHA-256 fingerprints (`source.files`) so `relocate_preparation`
//!   can verify that a local Star Wars Battlefront (2004) install matches the
//!   files the archive was prepared from.
//!
//! ## Binary Archive Layout
//! Because prepared content already consists of compressed audio, textures, and
//! packed asset blobs, `.bfecontent` archives store entries with ZIP compression
//! method `0` (`STORE`). When any individual entry or the archive as a whole
//! reaches `0xffff_ffff` bytes (4 GiB) or `0xffff` entries, standard ZIP64
//! extended information extra blocks (`0x0001`), the 56-byte ZIP64 End of Central
//! Directory record (`PK\x06\x06`), and the 20-byte ZIP64 EOCD Locator (`PK\x06\x07`)
//! are emitted automatically.
//!
//! ## Safety Invariants
//! Every archive entry path and manifest path is validated through [`safe_path`]
//! before touching the host filesystem:
//! - No empty segments, `.` or `..` traversal components, backslashes, or drive colons.
//! - No ASCII control characters (`<= 0x1f`) or trailing dots/spaces that Windows strips.
//! - No Windows reserved device stems (`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`).
//! - Content entries must live under `common/`, `sides/`, `maps/`, `audio/`, or `.prepare.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::discover::{inspect_content, inspect_install, walk_ignore_case};
use crate::paths::resolve_from;
use crate::prepare::{
    checkout_content_version, prepared_content_current, read_content_version, read_prepare_state,
    ContentVersion,
};

/// Manifest entry filename inside `.bfecontent` ZIP archives.
///
/// Mirrors `MANIFEST` in the desktop host.
pub const MANIFEST_NAME: &str = "bfe-content-manifest.json";

/// Receipt filename written into imported content directories.
///
/// Mirrors `RECEIPT` in the desktop host.
pub const RECEIPT_NAME: &str = ".bfe-import.json";

/// Archive format identifier stored in `bfe-content-manifest.json`.
///
/// Mirrors `FORMAT` in the desktop host.
pub const ARCHIVE_FORMAT: &str = "battlefront-eternal-content";

/// Archive manifest schema version.
///
/// Mirrors `VERSION` in the desktop host.
pub const ARCHIVE_VERSION: u32 = 1;

/// Allowed top-level subdirectories inside a `.bfecontent` archive.
///
/// Mirrors `CONTENT_ROOTS` in the desktop host.
const CONTENT_ROOTS: &[&str] = &["common", "sides", "maps", "audio"];

/// Maximum allowed file count in an imported `.bfecontent` archive (`300_000`).
///
/// Mirrors `MAX_FILES` in the desktop host.
pub const MAX_FILES: usize = 300_000;

/// Maximum allowed total uncompressed bytes in an imported archive (`256 GiB`).
///
/// Mirrors `MAX_BYTES` in the desktop host.
pub const MAX_BYTES: u64 = 256 * 1024 * 1024 * 1024;

/// Maximum allowed size of `bfe-content-manifest.json` (`64 MiB`).
///
/// Mirrors `MAX_MANIFEST` in the desktop host.
pub const MAX_MANIFEST: u64 = 64 * 1024 * 1024;

/// Single file entry inside `bfe-content-manifest.json`.
///
/// Each entry records the forward-slash path, uncompressed byte size, and
/// lowercase SHA-256 hex digest. When a file is a hardlink or case-folded
/// duplicate of an earlier entry, `link_to` names the canonical entry and
/// `hardlink` distinguishes hardlinks (`Some(true)`) from case-folded copies (`Some(false)`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFileEntry {
    /// Forward-slash relative path inside the content directory.
    pub path: String,
    /// Uncompressed file size in bytes.
    pub size: u64,
    /// Lowercase hex SHA-256 digest of the file payload.
    pub sha256: String,
    /// Canonical relative path when this entry is a hardlink or case-folded alias.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_to: Option<String>,
    /// Whether this alias was a filesystem hardlink (`Some(true)`) or a case-folded copy (`Some(false)`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardlink: Option<bool>,
}

/// Source `.lvl` fingerprint record stored under `manifest.source`.
///
/// Used by [`relocate_preparation`] to verify that a local `GameData` folder
/// contains the exact same `.lvl` files the archive was originally built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceReceipt {
    /// Base subdirectory inside the game install (`"GameData"`).
    pub base: String,
    /// Fingerprinted `.lvl` files relative to `GameData`.
    pub files: Vec<ManifestFileEntry>,
}

/// Deserialized representation of `bfe-content-manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveManifest {
    /// Must equal [`ARCHIVE_FORMAT`] (`"battlefront-eternal-content"`).
    pub format: String,
    /// Must equal [`ARCHIVE_VERSION`] (`1`).
    pub version: u32,
    /// Copy of `tools/content-version.json` active when the archive was exported.
    pub content_version: ContentVersion,
    /// ISO-8601 timestamp or epoch string of export creation.
    pub created_at: String,
    /// Sorted list of map directory names included in the archive.
    pub maps: Vec<String>,
    /// Sorted list of add-on IDs recorded in `.prepare.json`.
    pub add_ons: Vec<String>,
    /// Optional source `.lvl` fingerprints for relocating against a local install.
    pub source: Option<SourceReceipt>,
    /// Total uncompressed bytes across all files in `files`.
    pub total_bytes: u64,
    /// Manifest file entries (both stored entries and `linkTo` aliases).
    pub files: Vec<ManifestFileEntry>,
}

/// Validates a relative archive entry path against traversal, control characters,
/// Windows reserved device names, and (when `content_only` is true) allowed top-level roots.
///
/// Returns the normalized path segments on success.
/// Mirrors `safePath` in the desktop host.
pub fn safe_path(name: &str, content_only: bool) -> Result<Vec<String>, String> {
    // Reject empty or overly long entry names (> 1024 bytes).
    if name.is_empty() || name.len() > 1024 {
        return Err("Archive contains an unsafe file path.".to_string());
    }
    // Reject ASCII control characters, Windows backslashes, and drive colons.
    if name
        .chars()
        .any(|c| (c as u32) <= 0x1f || c == '\\' || c == ':')
    {
        return Err(format!("Archive contains an unsafe file path: {}", name));
    }
    let parts: Vec<&str> = name.split('/').collect();
    if parts.is_empty() {
        return Err(format!("Archive contains an unsafe file path: {}", name));
    }
    for part in &parts {
        // Reject empty path components, `.`, and `..` directory traversal segments.
        if part.is_empty() || *part == "." || *part == ".." {
            return Err(format!("Archive contains an unsafe file path: {}", name));
        }
        // Reject trailing dots or spaces that Windows filesystems strip silently.
        if part.ends_with('.') || part.ends_with(' ') {
            return Err(format!("Archive contains an unsafe file path: {}", name));
        }
        // Check Windows reserved device names (CON, PRN, AUX, NUL, COM1..9, LPT1..9)
        // before any dot extension.
        let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        let reserved = matches!(
            stem.as_str(),
            "con"
                | "prn"
                | "aux"
                | "nul"
                | "com1"
                | "com2"
                | "com3"
                | "com4"
                | "com5"
                | "com6"
                | "com7"
                | "com8"
                | "com9"
                | "lpt1"
                | "lpt2"
                | "lpt3"
                | "lpt4"
                | "lpt5"
                | "lpt6"
                | "lpt7"
                | "lpt8"
                | "lpt9"
        );
        if reserved {
            return Err(format!("Archive contains an unsafe file path: {}", name));
        }
    }
    // When validating prepared content entries, only allow `.prepare.json` at the
    // root or files nested inside `common/`, `sides/`, `maps/`, and `audio/`.
    if content_only {
        if name == ".prepare.json" {
            return Ok(parts.into_iter().map(|s| s.to_string()).collect());
        }
        if parts.len() < 2 || !CONTENT_ROOTS.contains(&parts[0]) {
            return Err(format!(
                "Archive contains an unexpected file path: {}",
                name
            ));
        }
    }
    Ok(parts.into_iter().map(|s| s.to_string()).collect())
}

/// Computes the size and lowercase hex SHA-256 digest of a file on disk.
///
/// Mirrors `hashFile` in the desktop host.
pub fn hash_file(file_path: &Path) -> Result<(u64, String), String> {
    // Stream the file in 64 KiB chunks to bound peak memory usage on large audio or map files.
    let mut file = File::open(file_path)
        .map_err(|e| format!("Failed to open {} for hashing: {}", file_path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Failed to read {}: {}", file_path.display(), e))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((total, hex_encode(&hasher.finalize())))
}

/// Encodes a byte slice as lowercase hexadecimal.
///
/// Pre-allocates `bytes.len() * 2` ASCII characters and formats each byte as
/// two lowercase hexadecimal digits (`00..=ff`).
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

/// Returns a portable `(dev, ino)` key for hardlink detection on Unix when `nlink > 1`.
///
/// Mirrors the `stat.nlink > 1 && stat.ino > 0` check in `walk` in the desktop host.
#[cfg(unix)]
fn hardlink_key(meta: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    if meta.nlink() > 1 && meta.ino() > 0 {
        Some((meta.dev(), meta.ino()))
    } else {
        None
    }
}

/// Stub for non-Unix platforms where `(dev, ino)` metadata is not exposed on `fs::Metadata`.
#[cfg(not(unix))]
fn hardlink_key(_meta: &fs::Metadata) -> Option<(u64, u64)> {
    None
}

/// Exportable content file tuple `(relative_path, full_path, size_bytes, hardlink_key)`.
pub type ContentFileRecord = (String, PathBuf, u64, Option<(u64, u64)>);

/// Walks a prepared content directory and collects all exportable files under
/// `common/`, `sides/`, `maps/`, `audio/`, and `.prepare.json`.
///
/// Rejects symlinks and verifies that every path passes [`safe_path`].
/// Mirrors `walk` in the desktop host.
pub fn list_content_files(root: &Path) -> Result<Vec<ContentFileRecord>, String> {
    let mut out = Vec::new();
    // Traverse each of the four allowed content subdirectories (`common`, `sides`, `maps`, `audio`).
    for top in CONTENT_ROOTS {
        let dir = root.join(top);
        if dir.exists() {
            walk_content_dir(&dir, top, &mut out)?;
        }
    }
    // Include the root `.prepare.json` marker so the imported folder carries its preparation state.
    let prep = root.join(".prepare.json");
    if prep.exists() {
        let meta = fs::symlink_metadata(&prep)
            .map_err(|e| format!("Failed to stat .prepare.json: {}", e))?;
        if meta.file_type().is_symlink() {
            return Err(
                "Prepared content cannot contain symbolic links: .prepare.json".to_string(),
            );
        }
        if meta.is_file() {
            out.push((
                ".prepare.json".to_string(),
                prep,
                meta.len(),
                hardlink_key(&meta),
            ));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Recursively walks a content subdirectory (`common`, `sides`, `maps`, or `audio`)
/// in sorted filename order and appends regular files to `out`.
fn walk_content_dir(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<ContentFileRecord>,
) -> Result<(), String> {
    // Sort directory entries deterministically so archive manifests are reproducible across runs.
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|a| a.file_name());

    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Content directory contains non-UTF-8 filename.".to_string())?;
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        let full = dir.join(&name);
        let meta = fs::symlink_metadata(&full)
            .map_err(|e| format!("Failed to stat {}: {}", full.display(), e))?;
        // Never follow or archive symbolic links inside a content folder.
        if meta.file_type().is_symlink() {
            return Err(format!(
                "Prepared content cannot contain symbolic links: {}",
                rel
            ));
        }
        if meta.is_dir() {
            walk_content_dir(&full, &rel, out)?;
        } else if meta.is_file() {
            safe_path(&rel, true)?;
            out.push((rel, full, meta.len(), hardlink_key(&meta)));
        }
    }
    Ok(())
}

/// Resolves the `GameData` directory from the `game` path recorded in `.prepare.json`.
///
/// Walks up to 4 parent directories from `<game>/GameData/Data/_LVL_PC` looking
/// for the directory named `GameData` (case-insensitively).
/// Mirrors the `GameData` ancestor lookup inside `sourceFingerprint` in the desktop host.
fn game_data_root(state_game: Option<&str>) -> Option<PathBuf> {
    let game_str = state_game.filter(|s| !s.is_empty())?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = resolve_from(&cwd, Path::new(game_str));
    // Walk up at most four parent levels (`_LVL_PC` -> `Data` -> `GameData`).
    for _ in 0..4 {
        if dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.eq_ignore_ascii_case("gamedata"))
            .unwrap_or(false)
        {
            return Some(dir);
        }
        let Some(parent) = dir.parent().map(|p| p.to_path_buf()) else {
            break;
        };
        if parent == dir {
            break;
        }
        dir = parent;
    }
    None
}

/// Collects SHA-256 fingerprints of `.lvl` files inside `GameData`, or falls back
/// to an existing `.bfe-import.json` receipt if the original game install is absent.
///
/// Mirrors `provenance` and `sourceFingerprint` in the desktop host.
pub fn source_manifest(root: &Path, state_game: Option<&str>) -> Option<SourceReceipt> {
    // Prefer hashing `.lvl` files from the live `GameData` directory when it exists on disk.
    if let Some(game_data) = game_data_root(state_game).filter(|d| d.exists()) {
        let mut files = Vec::new();
        if walk_lvl_files(&game_data, "", &mut files).is_ok() && !files.is_empty() {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            return Some(SourceReceipt {
                base: "GameData".to_string(),
                files,
            });
        }
    }
    // Otherwise preserve the `source` fingerprint block from `.bfe-import.json` when re-exporting.
    let receipt_raw = fs::read_to_string(root.join(RECEIPT_NAME)).ok()?;
    let receipt_val: serde_json::Value = serde_json::from_str(&receipt_raw).ok()?;
    let src = receipt_val.get("source")?;
    serde_json::from_value::<SourceReceipt>(src.clone()).ok()
}

/// Recursively hashes `.lvl` files under `GameData` for [`source_manifest`].
///
/// Sorts directory entries lexicographically, skips symbolic links, validates
/// relative paths through [`safe_path`], and records the byte length and
/// SHA-256 digest of every `.lvl` file encountered.
fn walk_lvl_files(
    dir: &Path,
    prefix: &str,
    files: &mut Vec<ManifestFileEntry>,
) -> Result<(), String> {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|a| a.file_name());
    for entry in entries {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        // Skip symlinks when fingerprinting the source `GameData` tree.
        if ft.is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", prefix, name)
        };
        let full = dir.join(&name);
        if ft.is_dir() {
            walk_lvl_files(&full, &rel, files)?;
        } else if ft.is_file() && name.to_ascii_lowercase().ends_with(".lvl") {
            safe_path(&rel, false)?;
            let (size, sha256) = hash_file(&full)?;
            files.push(ManifestFileEntry {
                path: rel,
                size,
                sha256,
                link_to: None,
                hardlink: None,
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// ZIP / ZIP64 Store Reader & Writer (compatible with yazl / yauzl in content-archive.js)
// ---------------------------------------------------------------------------

/// ZIP local file header signature (`PK\x03\x04`).
const ZIP_LOCAL_SIG: u32 = 0x04034b50;
/// ZIP central directory file header signature (`PK\x01\x02`).
const ZIP_CENTRAL_SIG: u32 = 0x02014b50;
/// ZIP end-of-central-directory signature (`PK\x05\x06`).
const ZIP_EOCD_SIG: u32 = 0x06054b50;
/// ZIP64 end-of-central-directory record signature (`PK\x06\x06`).
const ZIP64_EOCD_SIG: u32 = 0x06064b50;
/// ZIP64 end-of-central-directory locator signature (`PK\x06\x07`).
const ZIP64_LOCATOR_SIG: u32 = 0x07064b50;
/// Extra field header ID for ZIP64 extended information (`0x0001`).
const ZIP64_EXTRA_ID: u16 = 0x0001;
/// General-purpose bit flag 11 indicating UTF-8 filename encoding (`0x0800`).
const UTF8_FLAG: u16 = 0x0800;

/// In-memory record of a written entry used to emit the ZIP central directory at the end.
struct CentralRecord {
    /// Raw UTF-8 entry path bytes.
    name: Vec<u8>,
    /// CRC-32 checksum of the entry payload.
    crc: u32,
    /// Uncompressed (and stored) payload length in bytes.
    size: u64,
    /// Byte offset of the local file header from the start of the archive.
    offset: u64,
}

/// Streaming uncompressed (`method = 0`) ZIP / ZIP64 archive writer.
///
/// Produces uncompressed store-mode ZIP/ZIP64 archives identical to `yazl`
/// with `{ compress: false }` in `exportContent` in the desktop host.
struct ZipStoreWriter {
    /// Underlying output file handle.
    file: File,
    /// Current write cursor byte offset from the start of `file`.
    offset: u64,
    /// Accumulated entry records for the trailing central directory.
    entries: Vec<CentralRecord>,
}

impl ZipStoreWriter {
    /// Opens a new archive file for writing.
    fn create(path: &Path) -> Result<Self, String> {
        let file = File::create(path)
            .map_err(|e| format!("Failed to create archive {}: {}", path.display(), e))?;
        Ok(Self {
            file,
            offset: 0,
            entries: Vec::new(),
        })
    }

    /// Writes `buf` to the underlying file and advances `self.offset`.
    fn write_all_bytes(&mut self, buf: &[u8]) -> Result<(), String> {
        self.file
            .write_all(buf)
            .map_err(|e| format!("Failed writing archive bytes: {}", e))?;
        self.offset += buf.len() as u64;
        Ok(())
    }

    /// Emits a 30-byte ZIP local file header (plus a 20-byte ZIP64 extra block
    /// when `size` or `offset` reaches `0xffffffff`) and returns the header offset.
    ///
    /// Local file header binary layout (little-endian):
    /// - `0..4`: signature `0x04034b50` (`PK\x03\x04`)
    /// - `4..6`: version needed to extract (`45` for ZIP64, `20` for standard)
    /// - `6..8`: general purpose bit flag (`0x0800` for UTF-8 names)
    /// - `8..10`: compression method (`0` = store)
    /// - `10..14`: MS-DOS modification time (`0`) and date (`0x0021` = 1980-01-01)
    /// - `14..18`: CRC-32 checksum
    /// - `18..22`: compressed size (`0xffffffff` when ZIP64)
    /// - `22..26`: uncompressed size (`0xffffffff` when ZIP64)
    /// - `26..28`: filename length in bytes
    /// - `28..30`: extra field length (`20` when ZIP64, `0` otherwise)
    fn write_local_header(&mut self, name: &[u8], crc: u32, size: u64) -> Result<u64, String> {
        let offset = self.offset;
        let zip64 = size >= 0xffff_ffff || offset >= 0xffff_ffff;
        let extra_len: u16 = if zip64 { 20 } else { 0 };
        let mut header = Vec::with_capacity(30 + name.len() + extra_len as usize);
        header.extend_from_slice(&ZIP_LOCAL_SIG.to_le_bytes());
        header.extend_from_slice(&(if zip64 { 45u16 } else { 20u16 }).to_le_bytes());
        header.extend_from_slice(&UTF8_FLAG.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes()); // store method = 0
        header.extend_from_slice(&0u16.to_le_bytes()); // mod time
        header.extend_from_slice(&0x21u16.to_le_bytes()); // mod date
        header.extend_from_slice(&crc.to_le_bytes());
        let size32 = if zip64 { 0xffff_ffffu32 } else { size as u32 };
        header.extend_from_slice(&size32.to_le_bytes());
        header.extend_from_slice(&size32.to_le_bytes());
        header.extend_from_slice(&(name.len() as u16).to_le_bytes());
        header.extend_from_slice(&extra_len.to_le_bytes());
        header.extend_from_slice(name);
        if zip64 {
            // 20-byte ZIP64 extended information extra block: header ID (2B) + data size (2B)
            // + uncompressed size (8B) + compressed size (8B).
            header.extend_from_slice(&ZIP64_EXTRA_ID.to_le_bytes());
            header.extend_from_slice(&16u16.to_le_bytes());
            header.extend_from_slice(&size.to_le_bytes());
            header.extend_from_slice(&size.to_le_bytes());
        }
        self.write_all_bytes(&header)?;
        Ok(offset)
    }

    /// Appends an in-memory byte buffer as a stored ZIP entry.
    ///
    /// Computes the IEEE CRC-32 of `data`, writes the local file header and raw bytes,
    /// and records a [`CentralRecord`] for the trailing central directory.
    fn add_bytes(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        let mut crc_hasher = Crc32Hasher::new();
        crc_hasher.update(data);
        let crc = crc_hasher.finalize();
        let name_bytes = name.as_bytes().to_vec();
        let size = data.len() as u64;
        let offset = self.write_local_header(&name_bytes, crc, size)?;
        self.write_all_bytes(data)?;
        self.entries.push(CentralRecord {
            name: name_bytes,
            crc,
            size,
            offset,
        });
        Ok(())
    }

    /// Streams a file from disk into the archive as a stored ZIP entry.
    ///
    /// Verifies that the number of bytes read from `file_path` matches `expected_size`
    /// so concurrent truncation or modification during export fails immediately.
    fn add_file(
        &mut self,
        name: &str,
        file_path: &Path,
        expected_size: u64,
        crc: u32,
    ) -> Result<(), String> {
        let name_bytes = name.as_bytes().to_vec();
        let offset = self.write_local_header(&name_bytes, crc, expected_size)?;
        let mut src = File::open(file_path)
            .map_err(|e| format!("Failed to open {}: {}", file_path.display(), e))?;
        let mut written = 0u64;
        let mut buf = [0u8; 65536];
        loop {
            let n = src
                .read(&mut buf)
                .map_err(|e| format!("Failed reading {}: {}", file_path.display(), e))?;
            if n == 0 {
                break;
            }
            self.write_all_bytes(&buf[..n])?;
            written += n as u64;
        }
        // Fail fast if a concurrent process modified or truncated the file during export.
        if written != expected_size {
            return Err(format!("File size changed while exporting: {}", name));
        }
        self.entries.push(CentralRecord {
            name: name_bytes,
            crc,
            size: expected_size,
            offset,
        });
        Ok(())
    }

    /// Writes the central directory, optional ZIP64 EOCD + Locator, and standard EOCD.
    ///
    /// Central directory header binary layout (46 bytes + filename + optional 28-byte ZIP64 extra):
    /// - `0..4`: signature `0x02014b50` (`PK\x01\x02`)
    /// - `4..6`: version made by (`45`)
    /// - `6..8`: version needed to extract (`45` for ZIP64, `20` for standard)
    /// - `8..10`: general purpose bit flag (`0x0800` UTF-8)
    /// - `10..12`: compression method (`0` = store)
    /// - `12..16`: MS-DOS modification time and date
    /// - `16..20`: CRC-32 checksum
    /// - `20..24`: compressed size (`0xffffffff` when ZIP64)
    /// - `24..28`: uncompressed size (`0xffffffff` when ZIP64)
    /// - `28..34`: filename, extra, and comment lengths
    /// - `34..42`: disk number, internal attributes, external attributes
    /// - `42..46`: local header relative offset (`0xffffffff` when ZIP64)
    fn finish(mut self) -> Result<u64, String> {
        let central_offset = self.offset;
        let entries = std::mem::take(&mut self.entries);
        // Emit one 46-byte Central Directory File Header per stored entry, plus
        // a 28-byte ZIP64 extra field whenever `size` or `offset` overflows 32 bits.
        for entry in &entries {
            let zip64 = entry.size >= 0xffff_ffff || entry.offset >= 0xffff_ffff;
            let extra_len: u16 = if zip64 { 28 } else { 0 };
            let mut rec = Vec::with_capacity(46 + entry.name.len() + extra_len as usize);
            rec.extend_from_slice(&ZIP_CENTRAL_SIG.to_le_bytes());
            rec.extend_from_slice(&45u16.to_le_bytes());
            rec.extend_from_slice(&(if zip64 { 45u16 } else { 20u16 }).to_le_bytes());
            rec.extend_from_slice(&UTF8_FLAG.to_le_bytes());
            rec.extend_from_slice(&0u16.to_le_bytes());
            rec.extend_from_slice(&0u16.to_le_bytes());
            rec.extend_from_slice(&0x21u16.to_le_bytes());
            rec.extend_from_slice(&entry.crc.to_le_bytes());
            let size32 = if zip64 {
                0xffff_ffffu32
            } else {
                entry.size as u32
            };
            let off32 = if zip64 {
                0xffff_ffffu32
            } else {
                entry.offset as u32
            };
            rec.extend_from_slice(&size32.to_le_bytes());
            rec.extend_from_slice(&size32.to_le_bytes());
            rec.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
            rec.extend_from_slice(&extra_len.to_le_bytes());
            rec.extend_from_slice(&0u16.to_le_bytes()); // comment len
            rec.extend_from_slice(&0u16.to_le_bytes()); // disk number
            rec.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            rec.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            rec.extend_from_slice(&off32.to_le_bytes());
            rec.extend_from_slice(&entry.name);
            if zip64 {
                rec.extend_from_slice(&ZIP64_EXTRA_ID.to_le_bytes());
                rec.extend_from_slice(&24u16.to_le_bytes());
                rec.extend_from_slice(&entry.size.to_le_bytes());
                rec.extend_from_slice(&entry.size.to_le_bytes());
                rec.extend_from_slice(&entry.offset.to_le_bytes());
            }
            self.write_all_bytes(&rec)?;
        }
        let central_size = self.offset - central_offset;
        let count = entries.len() as u64;
        let zip64 = count >= 0xffff || central_size >= 0xffff_ffff || central_offset >= 0xffff_ffff;
        // When the archive exceeds 16-bit entry count or 32-bit offsets, write the
        // 56-byte ZIP64 EOCD record followed by the 20-byte ZIP64 EOCD Locator.
        if zip64 {
            let eocd64_offset = self.offset;
            let mut eocd64 = Vec::with_capacity(56);
            eocd64.extend_from_slice(&ZIP64_EOCD_SIG.to_le_bytes());
            eocd64.extend_from_slice(&44u64.to_le_bytes());
            eocd64.extend_from_slice(&45u16.to_le_bytes());
            eocd64.extend_from_slice(&45u16.to_le_bytes());
            eocd64.extend_from_slice(&0u32.to_le_bytes());
            eocd64.extend_from_slice(&0u32.to_le_bytes());
            eocd64.extend_from_slice(&count.to_le_bytes());
            eocd64.extend_from_slice(&count.to_le_bytes());
            eocd64.extend_from_slice(&central_size.to_le_bytes());
            eocd64.extend_from_slice(&central_offset.to_le_bytes());
            self.write_all_bytes(&eocd64)?;

            let mut locator = Vec::with_capacity(20);
            locator.extend_from_slice(&ZIP64_LOCATOR_SIG.to_le_bytes());
            locator.extend_from_slice(&0u32.to_le_bytes());
            locator.extend_from_slice(&eocd64_offset.to_le_bytes());
            locator.extend_from_slice(&1u32.to_le_bytes());
            self.write_all_bytes(&locator)?;
        }

        // Write the trailing 22-byte standard End of Central Directory (EOCD) record.
        let mut eocd = Vec::with_capacity(22);
        eocd.extend_from_slice(&ZIP_EOCD_SIG.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        let count16 = if zip64 { 0xffffu16 } else { count as u16 };
        let csize32 = if zip64 {
            0xffff_ffffu32
        } else {
            central_size as u32
        };
        let coff32 = if zip64 {
            0xffff_ffffu32
        } else {
            central_offset as u32
        };
        eocd.extend_from_slice(&count16.to_le_bytes());
        eocd.extend_from_slice(&count16.to_le_bytes());
        eocd.extend_from_slice(&csize32.to_le_bytes());
        eocd.extend_from_slice(&coff32.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes());
        self.write_all_bytes(&eocd)?;
        self.file
            .flush()
            .map_err(|e| format!("Failed flushing archive: {}", e))?;
        Ok(self.offset)
    }
}

/// Metadata for a single entry in the central directory of a `.bfecontent` ZIP file.
#[derive(Debug, Clone)]
pub struct ZipEntryInfo {
    /// Entry filename inside the ZIP.
    pub name: String,
    /// Compression method (`0` = store, `8` = deflate).
    pub method: u16,
    /// CRC-32 checksum recorded in the central directory.
    pub crc: u32,
    /// Compressed byte size.
    pub compressed_size: u64,
    /// Uncompressed byte size.
    pub size: u64,
    /// Byte offset of the local file header.
    pub offset: u64,
}

/// Returns a bounds-checked subslice of `buf` of length `len` starting at `offset`.
///
/// Uses checked addition (`offset.checked_add(len)`) and `buf.get(offset..end)` so
/// truncated, malformed, or hostile archive offsets return `Err(err)` and never panic.
fn slice_at<'a>(buf: &'a [u8], offset: usize, len: usize, err: &str) -> Result<&'a [u8], String> {
    let end = offset.checked_add(len).ok_or_else(|| err.to_string())?;
    buf.get(offset..end).ok_or_else(|| err.to_string())
}

/// Reads a little-endian `u16` at `offset` in `buf`, returning `Err(err)` when out of bounds.
pub(crate) fn read_u16(buf: &[u8], offset: usize, err: &str) -> Result<u16, String> {
    let bytes: [u8; 2] = slice_at(buf, offset, 2, err)?
        .try_into()
        .map_err(|_| err.to_string())?;
    Ok(u16::from_le_bytes(bytes))
}

/// Reads a little-endian `u32` at `offset` in `buf`, returning `Err(err)` when out of bounds.
pub(crate) fn read_u32(buf: &[u8], offset: usize, err: &str) -> Result<u32, String> {
    let bytes: [u8; 4] = slice_at(buf, offset, 4, err)?
        .try_into()
        .map_err(|_| err.to_string())?;
    Ok(u32::from_le_bytes(bytes))
}

/// Reads a little-endian `u64` at `offset` in `buf`, returning `Err(err)` when out of bounds.
pub(crate) fn read_u64(buf: &[u8], offset: usize, err: &str) -> Result<u64, String> {
    let bytes: [u8; 8] = slice_at(buf, offset, 8, err)?
        .try_into()
        .map_err(|_| err.to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

/// Reads an exact slice of `len` bytes starting at `offset` in `file`.
///
/// Verifies that `offset + len` does not exceed the file's total length before
/// seeking or allocating `buf`, so hostile offsets in untrusted archives fail
/// cleanly without panicking or allocating oversized buffers.
fn read_exact_at(file: &mut File, offset: u64, len: usize) -> Result<Vec<u8>, String> {
    let file_len = file
        .metadata()
        .map_err(|e| format!("Failed to stat archive: {}", e))?
        .len();
    let end = offset
        .checked_add(len as u64)
        .ok_or_else(|| "Archive is truncated.".to_string())?;
    if end > file_len {
        return Err("Archive is truncated.".to_string());
    }
    // Seek to the target file offset and fill an exact buffer of `len` bytes.
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("Archive seek failed: {}", e))?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf)
        .map_err(|_| "Archive is truncated.".to_string())?;
    Ok(buf)
}

/// Parses the ZIP / ZIP64 central directory of `file`.
///
/// Mirrors the `yauzl.open` central directory reader in `importContent` in the desktop host.
///
/// Scans backwards from the end of `file` to find the 22-byte End of Central Directory
/// (EOCD) record, checks for a preceding 20-byte ZIP64 EOCD Locator (`PK\x06\x07`) and
/// 56-byte ZIP64 EOCD record (`PK\x06\x06`), and parses every central directory entry
/// into a `BTreeMap` keyed by entry name.
pub fn read_zip_entries(file: &mut File) -> Result<BTreeMap<String, ZipEntryInfo>, String> {
    let total_size = file
        .metadata()
        .map_err(|e| format!("Failed to stat archive: {}", e))?
        .len();
    // Every valid ZIP file has at least a 22-byte End of Central Directory (EOCD) record.
    if total_size < 22 {
        return Err("That file is not a Battlefront Eternal content archive.".to_string());
    }
    // Read at most the trailing 65,557 bytes (22-byte EOCD + 65,535-byte max comment).
    let tail_size = total_size.min(65557) as usize;
    let tail_offset = total_size - tail_size as u64;
    let tail = read_exact_at(file, tail_offset, tail_size)?;

    // Scan backwards from the end of the file to locate the End of Central Directory (EOCD) record.
    let mut eocd_pos: Option<usize> = None;
    if let Some(max_start) = tail.len().checked_sub(22) {
        for i in (0..=max_start).rev() {
            let sig = read_u32(&tail, i, "That file is not a valid content archive.")?;
            if sig == ZIP_EOCD_SIG {
                let comment_len =
                    read_u16(&tail, i + 20, "That file is not a valid content archive.")? as usize;
                if i.checked_add(22).and_then(|n| n.checked_add(comment_len)) == Some(tail.len()) {
                    eocd_pos = Some(i);
                    break;
                }
            }
        }
    }
    let Some(eocd) = eocd_pos else {
        return Err("That file is not a valid content archive.".to_string());
    };

    let mut count = read_u16(
        &tail,
        eocd + 10,
        "That file is not a valid content archive.",
    )? as u64;
    let mut central_size = read_u32(
        &tail,
        eocd + 12,
        "That file is not a valid content archive.",
    )? as u64;
    let mut central_offset = read_u32(
        &tail,
        eocd + 16,
        "That file is not a valid content archive.",
    )? as u64;

    // Check for a preceding 20-byte ZIP64 EOCD Locator when the archive exceeds 32-bit limits.
    if let Some(loc_pos) = eocd.checked_sub(20) {
        let loc_sig = read_u32(&tail, loc_pos, "Archive ZIP64 directory is damaged.")?;
        if loc_sig == ZIP64_LOCATOR_SIG {
            let eocd64_offset =
                read_u64(&tail, loc_pos + 8, "Archive ZIP64 directory is damaged.")?;
            let eocd64 = read_exact_at(file, eocd64_offset, 56)?;
            let sig64 = read_u32(&eocd64, 0, "Archive ZIP64 directory is damaged.")?;
            if sig64 != ZIP64_EOCD_SIG {
                return Err("Archive ZIP64 directory is damaged.".to_string());
            }
            count = read_u64(&eocd64, 32, "Archive ZIP64 directory is damaged.")?;
            central_size = read_u64(&eocd64, 40, "Archive ZIP64 directory is damaged.")?;
            central_offset = read_u64(&eocd64, 48, "Archive ZIP64 directory is damaged.")?;
        }
    }

    // Reject archives whose entry count exceeds MAX_FILES or whose central directory
    // points past the end of the file.
    let central_end = central_offset
        .checked_add(central_size)
        .ok_or_else(|| "Archive directory is out of bounds.".to_string())?;
    let central_len = usize::try_from(central_size)
        .map_err(|_| "Archive directory is out of bounds.".to_string())?;
    if count > (MAX_FILES as u64) + 4 || central_end > total_size {
        return Err("Archive directory is out of bounds.".to_string());
    }

    let central = read_exact_at(file, central_offset, central_len)?;
    let mut entries = BTreeMap::new();
    let mut pos = 0usize;

    // Walk every 46-byte central directory header and its variable-length name/extra/comment tail.
    for _ in 0..count {
        if read_u32(&central, pos, "Archive central directory is damaged.")? != ZIP_CENTRAL_SIG {
            return Err("Archive central directory is damaged.".to_string());
        }
        let method = read_u16(&central, pos + 10, "Archive central directory is damaged.")?;
        let crc = read_u32(&central, pos + 16, "Archive central directory is damaged.")?;
        let mut compressed_size =
            read_u32(&central, pos + 20, "Archive central directory is damaged.")? as u64;
        let mut size =
            read_u32(&central, pos + 24, "Archive central directory is damaged.")? as u64;
        let name_len =
            read_u16(&central, pos + 28, "Archive central directory is damaged.")? as usize;
        let extra_len =
            read_u16(&central, pos + 30, "Archive central directory is damaged.")? as usize;
        let comment_len =
            read_u16(&central, pos + 32, "Archive central directory is damaged.")? as usize;
        let mut offset =
            read_u32(&central, pos + 42, "Archive central directory is damaged.")? as u64;

        let header_end = pos
            .checked_add(46)
            .ok_or_else(|| "Archive directory entry is truncated.".to_string())?;
        let name_bytes = slice_at(
            &central,
            header_end,
            name_len,
            "Archive directory entry is truncated.",
        )?;
        let name_end = header_end + name_len;
        let extra = slice_at(
            &central,
            name_end,
            extra_len,
            "Archive directory entry is truncated.",
        )?;
        let extra_end = name_end + extra_len;
        let _comment = slice_at(
            &central,
            extra_end,
            comment_len,
            "Archive directory entry is truncated.",
        )?;
        let entry_end = extra_end + comment_len;
        let name = String::from_utf8(name_bytes.to_vec())
            .map_err(|_| "Archive entry name is not valid UTF-8.".to_string())?;

        // Parse 64-bit size, compressed_size, and local header offset from the ZIP64 extra field.
        if size == 0xffff_ffff || compressed_size == 0xffff_ffff || offset == 0xffff_ffff {
            let mut ep = 0usize;
            while ep.checked_add(4).is_some_and(|end| end <= extra.len()) {
                let id = read_u16(extra, ep, "Archive directory entry is truncated.")?;
                let len =
                    read_u16(extra, ep + 2, "Archive directory entry is truncated.")? as usize;
                let block_start = ep + 4;
                let avail = extra.len().saturating_sub(block_start);
                let block = slice_at(
                    extra,
                    block_start,
                    len.min(avail),
                    "Archive directory entry is truncated.",
                )?;
                if id == ZIP64_EXTRA_ID {
                    let mut bp = 0usize;
                    if size == 0xffff_ffff && bp.checked_add(8).is_some_and(|e| e <= block.len()) {
                        size = read_u64(block, bp, "Archive directory entry is truncated.")?;
                        bp += 8;
                    }
                    if compressed_size == 0xffff_ffff
                        && bp.checked_add(8).is_some_and(|e| e <= block.len())
                    {
                        compressed_size =
                            read_u64(block, bp, "Archive directory entry is truncated.")?;
                        bp += 8;
                    }
                    if offset == 0xffff_ffff && bp.checked_add(8).is_some_and(|e| e <= block.len())
                    {
                        offset = read_u64(block, bp, "Archive directory entry is truncated.")?;
                    }
                    break;
                }
                ep = block_start.saturating_add(len);
            }
        }
        // Reject archives with duplicate central directory entry names to prevent ambiguous extraction.
        if entries.contains_key(&name) {
            return Err(format!("Archive contains duplicate entry: {}", name));
        }
        entries.insert(
            name.clone(),
            ZipEntryInfo {
                name,
                method,
                crc,
                compressed_size,
                size,
                offset,
            },
        );
        pos = entry_end;
    }

    Ok(entries)
}

/// Computes the byte offset where an entry's payload begins after its local file header.
fn entry_data_offset(file: &mut File, entry: &ZipEntryInfo) -> Result<u64, String> {
    // Read the 30-byte local file header (`PK\x03\x04`) and skip its filename and extra fields.
    let err = format!("Archive local header is damaged: {}", entry.name);
    let header = read_exact_at(file, entry.offset, 30)?;
    if read_u32(&header, 0, &err)? != ZIP_LOCAL_SIG {
        return Err(err);
    }
    let name_len = read_u16(&header, 26, &err)? as u64;
    let extra_len = read_u16(&header, 28, &err)? as u64;
    entry
        .offset
        .checked_add(30)
        .and_then(|n| n.checked_add(name_len))
        .and_then(|n| n.checked_add(extra_len))
        .ok_or(err)
}

/// Reads and verifies the full byte payload of a ZIP entry (supporting store `0` and deflate `8`).
///
/// Mirrors `readEntryBuffer` in the desktop host.
fn read_entry_bytes(
    file: &mut File,
    entry: &ZipEntryInfo,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    // Enforce the caller's upper bound (`MAX_MANIFEST`) before allocating a buffer.
    if entry.size > max_bytes || entry.compressed_size > max_bytes {
        return Err(format!("Archive entry is too large: {}", entry.name));
    }
    let data_offset = entry_data_offset(file, entry)?;
    let raw = read_exact_at(file, data_offset, entry.compressed_size as usize)?;
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
            "Unsupported archive compression method for {}",
            entry.name
        ));
    };

    // Verify both the uncompressed byte length and the ZIP CRC-32 checksum.
    if data.len() as u64 != entry.size {
        return Err(format!("Archive entry size mismatch: {}", entry.name));
    }
    let mut crc_hasher = Crc32Hasher::new();
    crc_hasher.update(&data);
    if crc_hasher.finalize() != entry.crc {
        return Err(format!("Archive checksum mismatch: {}", entry.name));
    }
    Ok(data)
}

/// Extracts a single ZIP entry to `dest` on disk while verifying both its CRC-32
/// and its expected SHA-256 digest.
///
/// Mirrors `extractEntry` in the desktop host.
fn extract_entry_to_file(
    file: &mut File,
    entry: &ZipEntryInfo,
    dest: &Path,
    expected_sha256: &str,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    // Handle Deflate-compressed entries (`method == 8`) via `read_entry_bytes`.
    if entry.method == 8 {
        let data = read_entry_bytes(file, entry, MAX_MANIFEST)?;
        let mut hasher = Sha256::new();
        hasher.update(&data);
        if hex_encode(&hasher.finalize()) != expected_sha256 {
            return Err(format!("Content hash mismatch: {}", entry.name));
        }
        fs::write(dest, &data).map_err(|e| format!("Failed to write {}: {}", dest.display(), e))?;
        return Ok(());
    }
    if entry.method != 0 {
        return Err(format!(
            "Unsupported archive compression method for {}",
            entry.name
        ));
    }

    // Stream uncompressed (`method == 0`) entries directly to disk while computing CRC-32 and SHA-256.
    let data_offset = entry_data_offset(file, entry)?;
    file.seek(SeekFrom::Start(data_offset))
        .map_err(|e| format!("Seek failed: {}", e))?;
    let mut out =
        File::create(dest).map_err(|e| format!("Failed to create {}: {}", dest.display(), e))?;

    let mut crc_hasher = Crc32Hasher::new();
    let mut sha_hasher = Sha256::new();
    let mut remaining = entry.size;
    let mut buf = [0u8; 65536];

    while remaining > 0 {
        let to_read = (remaining as usize).min(buf.len());
        let n = file
            .read(&mut buf[..to_read])
            .map_err(|e| format!("Read error extracting {}: {}", entry.name, e))?;
        if n == 0 {
            return Err(format!("Archive is truncated at {}", entry.name));
        }
        crc_hasher.update(&buf[..n]);
        sha_hasher.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| format!("Write error for {}: {}", dest.display(), e))?;
        remaining -= n as u64;
    }
    out.flush()
        .map_err(|e| format!("Flush error for {}: {}", dest.display(), e))?;

    // Reject the extracted entry if either the ZIP CRC-32 or the manifest SHA-256 fails.
    if crc_hasher.finalize() != entry.crc {
        return Err(format!("Archive checksum mismatch: {}", entry.name));
    }
    if hex_encode(&sha_hasher.finalize()) != expected_sha256 {
        return Err(format!("Content hash mismatch: {}", entry.name));
    }
    Ok(())
}

/// Validates a parsed [`ArchiveManifest`] against the active [`ContentVersion`]
/// and the central directory entries of the ZIP file.
///
/// Mirrors `validateManifest` in the desktop host.
///
/// Checks performed:
/// 1. `manifest.format == "battlefront-eternal-content"` and `manifest.version == 1`.
/// 2. `manifest.content_version == expected_version` (from `tools/content-version.json`).
/// 3. `1 <= manifest.files.len() <= MAX_FILES` and total bytes `<= MAX_BYTES`.
/// 4. Every file path passes [`safe_path`], is unique, has a lowercase 64-hex SHA-256,
///    and (unless it is a `linkTo` alias) exists in the ZIP central directory with matching size.
/// 5. Every `linkTo` alias references a primary (non-alias) manifest entry with matching size and SHA-256.
/// 6. `common/common.json` is present and the ZIP contains no unlisted entries.
pub fn validate_manifest(
    manifest: &ArchiveManifest,
    entries: &BTreeMap<String, ZipEntryInfo>,
    expected_version: &ContentVersion,
) -> Result<(), String> {
    // Reject archives with unknown format identifiers or incompatible schema versions.
    if manifest.format != ARCHIVE_FORMAT || manifest.version != ARCHIVE_VERSION {
        return Err(
            "That file is not a compatible Battlefront Eternal content archive.".to_string(),
        );
    }
    // Reject archives whose `contentVersion` does not match `tools/content-version.json`.
    if &manifest.content_version != expected_version {
        return Err(
            "That content archive was made for a different content version and must be prepared again."
                .to_string(),
        );
    }
    // Enforce non-empty file count within MAX_FILES.
    if manifest.files.is_empty() || manifest.files.len() > MAX_FILES {
        return Err("Content archive file list is invalid.".to_string());
    }
    let mut total = 0u64;
    let mut seen = BTreeSet::new();
    let mut has_common = false;
    let mut by_path = BTreeMap::new();

    // First pass: check each manifest item's path safety, uniqueness, SHA-256 syntax,
    // cumulative byte budget, and presence in the ZIP central directory.
    for item in &manifest.files {
        safe_path(&item.path, true)?;
        if !seen.insert(item.path.clone()) {
            return Err(format!(
                "Content archive lists {} more than once.",
                item.path
            ));
        }
        if item.sha256.len() != 64
            || !item
                .sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(format!("Content archive entry is invalid: {}", item.path));
        }
        total = total.saturating_add(item.size);
        if total > MAX_BYTES {
            return Err("Content archive is larger than supported.".to_string());
        }
        by_path.insert(item.path.clone(), item);
        if item.link_to.is_none() {
            let Some(entry) = entries.get(&item.path) else {
                return Err(format!("Content archive is missing {}", item.path));
            };
            if entry.size != item.size {
                return Err(format!("Content archive size mismatch for {}", item.path));
            }
        }
        if item.path == "common/common.json" {
            has_common = true;
        }
    }

    // Second pass: verify that every `linkTo` alias points to a primary stored entry
    // (no chained `linkTo -> linkTo`) with matching byte size and SHA-256 digest.
    for item in &manifest.files {
        let Some(link_to) = &item.link_to else {
            continue;
        };
        safe_path(link_to, true)?;
        let Some(target) = by_path.get(link_to) else {
            return Err(format!(
                "Content archive link target is invalid: {}",
                item.path
            ));
        };
        if target.link_to.is_some() {
            return Err(format!(
                "Content archive link target is invalid: {}",
                item.path
            ));
        };
        if target.size != item.size || target.sha256 != item.sha256 {
            return Err(format!("Content archive link mismatch for {}", item.path));
        }
    }

    if !has_common {
        return Err("Content archive is missing common/common.json.".to_string());
    }
    // Reject archives that smuggle unlisted entries outside `bfe-content-manifest.json`.
    for name in entries.keys() {
        if name != MANIFEST_NAME && !seen.contains(name) {
            return Err(format!(
                "Content archive contains an unlisted file: {}",
                name
            ));
        }
    }
    Ok(())
}

/// Summary returned when exporting a `.bfecontent` archive via [`export_content`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    /// Output archive path.
    pub archive_path: PathBuf,
    /// Number of manifest file records written.
    pub files: usize,
    /// Total logical uncompressed bytes across all manifest entries.
    pub bytes: u64,
    /// Final `.bfecontent` file size on disk.
    pub archive_bytes: u64,
    /// Sorted list of exported map IDs.
    pub maps: Vec<String>,
}

/// Exports a prepared content directory (`content_dir`) to a `.bfecontent` archive
/// at `archive_path`.
///
/// Writes to a temporary sibling file first and atomically renames on completion.
/// Mirrors `exportContent` in the desktop host.
pub fn export_content(
    content_dir: &Path,
    archive_path: &Path,
    tools_dir: Option<&Path>,
) -> Result<ExportSummary, String> {
    // Load the expected content version from `tools/content-version.json`.
    let content_version = match tools_dir {
        Some(dir) => read_content_version(dir)?,
        None => checkout_content_version()?,
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = resolve_from(&cwd, content_dir);
    let out = resolve_from(&cwd, archive_path);
    // Prevent exporting an archive inside the very content directory being archived.
    if out.starts_with(&root) {
        return Err("Choose an archive location outside the prepared content folder.".to_string());
    }
    let inspected = inspect_content(&root)
        .ok_or_else(|| "That folder does not contain prepared game content.".to_string())?;
    let state = read_prepare_state(Some(&root));
    if !prepared_content_current(state.as_ref(), &content_version) {
        return Err(
            "Prepared content is incomplete or outdated; prepare it again before exporting."
                .to_string(),
        );
    }

    let files = list_content_files(&root)?;
    if files.is_empty() {
        return Err("No prepared content files were found to export.".to_string());
    }

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating {}: {}", parent.display(), e))?;
    }

    // Write into a hidden `.part` sibling file first so interrupted exports never leave
    // a half-written `.bfecontent` archive at the target path.
    let rand_suffix = random_hex_suffix();
    let temp_path = out.with_file_name(format!(
        ".{}.{}.part",
        out.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive"),
        rand_suffix
    ));

    let result = (|| -> Result<ExportSummary, String> {
        let mut writer = ZipStoreWriter::create(&temp_path)?;
        let mut manifest_files: Vec<ManifestFileEntry> = Vec::new();
        let mut hardlink_targets: BTreeMap<(u64, u64), ManifestFileEntry> = BTreeMap::new();
        let mut case_targets: BTreeMap<String, ManifestFileEntry> = BTreeMap::new();
        let mut total_bytes = 0u64;

        for (rel, full, size, hl_key) in &files {
            // Deduplicate Unix hardlinks by linking subsequent paths to the first stored inode.
            if let Some(key) = hl_key {
                if let Some(target) = hardlink_targets.get(key) {
                    manifest_files.push(ManifestFileEntry {
                        path: rel.clone(),
                        size: target.size,
                        sha256: target.sha256.clone(),
                        link_to: Some(target.path.clone()),
                        hardlink: Some(true),
                    });
                    total_bytes += target.size;
                    continue;
                }
            }

            // Compute both CRC32 and SHA-256 in a single pass.
            let (actual_size, crc, sha256) = hash_and_crc_file(full)?;
            if actual_size != *size {
                return Err(format!("File size changed while exporting: {}", rel));
            }
            let folded = rel.to_lowercase();
            // Deduplicate identical case-folded filenames so importing onto a
            // case-insensitive Windows/macOS filesystem never collides or fails.
            if let Some(case_target) = case_targets.get(&folded) {
                if case_target.size != actual_size || case_target.sha256 != sha256 {
                    return Err(format!(
                        "Prepared content has conflicting files that differ only by letter case: {} and {}",
                        case_target.path, rel
                    ));
                }
                manifest_files.push(ManifestFileEntry {
                    path: rel.clone(),
                    size: actual_size,
                    sha256: sha256.clone(),
                    link_to: Some(case_target.path.clone()),
                    hardlink: Some(false),
                });
                total_bytes += actual_size;
                continue;
            }

            writer.add_file(rel, full, actual_size, crc)?;
            let entry = ManifestFileEntry {
                path: rel.clone(),
                size: actual_size,
                sha256,
                link_to: None,
                hardlink: None,
            };
            if let Some(key) = hl_key {
                hardlink_targets.insert(*key, entry.clone());
            }
            case_targets.insert(folded, entry.clone());
            manifest_files.push(entry);
            total_bytes += actual_size;
        }

        // Capture source `.lvl` fingerprints from the live `GameData` install (or preserve
        // them from an earlier `.bfe-import.json` receipt) so `relocate_preparation` works.
        let state_game = state.as_ref().and_then(|s| s.game.as_deref());
        let source = source_manifest(&root, state_game);
        let add_ons = state.and_then(|s| s.add_ons).unwrap_or_default();

        // Build and append `bfe-content-manifest.json` as the final entry before the central directory.
        let manifest = ArchiveManifest {
            format: ARCHIVE_FORMAT.to_string(),
            version: ARCHIVE_VERSION,
            content_version,
            created_at: "2026-01-01T00:00:00.000Z".to_string(),
            maps: inspected.maps.clone(),
            add_ons,
            source,
            total_bytes,
            files: manifest_files.clone(),
        };

        let mut manifest_bytes = serde_json::to_vec_pretty(&manifest)
            .map_err(|e| format!("Failed serializing manifest: {}", e))?;
        manifest_bytes.push(b'\n');
        writer.add_bytes(MANIFEST_NAME, &manifest_bytes)?;
        let archive_bytes = writer.finish()?;

        // Atomically rename the finished `.part` file to the target `.bfecontent` path.
        fs::rename(&temp_path, &out)
            .map_err(|e| format!("Failed renaming archive into place: {}", e))?;

        Ok(ExportSummary {
            archive_path: out.clone(),
            files: manifest_files.len(),
            bytes: total_bytes,
            archive_bytes,
            maps: inspected.maps,
        })
    })();

    // Clean up the temporary `.part` file if any step failed before the atomic rename.
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Computes `(size, crc32, sha256_hex)` of `path` in a single streaming read.
///
/// Reading each exported content file once for both CRC-32 (needed by the ZIP
/// local and central headers) and SHA-256 (needed by `bfe-content-manifest.json`)
/// halves disk I/O during `.bfecontent` archive creation.
fn hash_and_crc_file(path: &Path) -> Result<(u64, u32, String), String> {
    // Stream the file in 64 KiB chunks and update both CRC-32 and SHA-256 hashers simultaneously.
    let mut f =
        File::open(path).map_err(|e| format!("Failed opening {}: {}", path.display(), e))?;
    let mut crc = Crc32Hasher::new();
    let mut sha = Sha256::new();
    let mut total = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("Failed reading {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        crc.update(&buf[..n]);
        sha.update(&buf[..n]);
    }
    Ok((total, crc.finalize(), hex_encode(&sha.finalize())))
}

/// Summary returned when inspecting a `.bfecontent` archive via [`inspect_archive`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectArchiveResult {
    /// Canonical archive path.
    pub archive_path: PathBuf,
    /// Number of manifest file records.
    pub files: usize,
    /// Total uncompressed bytes across all manifest entries.
    pub total_bytes: u64,
    /// Sorted list of map IDs inside the archive.
    pub maps: Vec<String>,
    /// Sorted list of add-on IDs inside the archive.
    pub add_ons: Vec<String>,
    /// Archive creation timestamp string.
    pub created_at: String,
}

/// Validates a `.bfecontent` archive's central directory and manifest without extracting files.
///
/// Mirrors the manifest inspection step of `importContent` and `validateManifest` in the desktop host.
pub fn inspect_archive(
    archive_path: &Path,
    tools_dir: Option<&Path>,
) -> Result<InspectArchiveResult, String> {
    // Load the active `tools/content-version.json` and verify the archive's manifest against it.
    let content_version = match tools_dir {
        Some(dir) => read_content_version(dir)?,
        None => checkout_content_version()?,
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let resolved = resolve_from(&cwd, archive_path);
    let mut file = File::open(&resolved)
        .map_err(|e| format!("Failed to open archive {}: {}", resolved.display(), e))?;
    let entries = read_zip_entries(&mut file)?;
    let manifest_entry = entries
        .get(MANIFEST_NAME)
        .ok_or_else(|| "That file is not a Battlefront Eternal content archive.".to_string())?;
    let raw = read_entry_bytes(&mut file, manifest_entry, MAX_MANIFEST)?;
    let manifest: ArchiveManifest = serde_json::from_slice(&raw)
        .map_err(|_| "Content archive manifest is damaged.".to_string())?;
    validate_manifest(&manifest, &entries, &content_version)?;

    Ok(InspectArchiveResult {
        archive_path: resolved,
        files: manifest.files.len(),
        total_bytes: manifest.total_bytes,
        maps: manifest.maps,
        add_ons: manifest.add_ons,
        created_at: manifest.created_at,
    })
}

/// Summary returned when importing a `.bfecontent` archive via [`import_content`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    /// Final imported content directory path (`<destination_parent>/content-<id>`).
    pub content_dir: PathBuf,
    /// Number of manifest file records extracted/restored.
    pub files: usize,
    /// Total uncompressed bytes extracted/restored.
    pub bytes: u64,
    /// Sorted list of map IDs in the imported content directory.
    pub maps: Vec<String>,
}

/// Extracts and verifies a `.bfecontent` archive into a new sibling directory under
/// `destination_parent`.
///
/// Extracts into a temporary `.import-<id>` staging directory, verifies every
/// file's CRC-32 and SHA-256, restores `linkTo` hardlinks/copies, validates
/// `.prepare.json` against `tools/content-version.json`, writes `.bfe-import.json`,
/// and renames the directory to `<destination_parent>/content-<id>`.
///
/// Mirrors `importContent` in the desktop host.
pub fn import_content(
    archive_path: &Path,
    destination_parent: &Path,
    tools_dir: Option<&Path>,
) -> Result<ImportSummary, String> {
    import_content_with_progress(
        archive_path,
        destination_parent,
        tools_dir,
        |_, _, _| Ok(()),
    )
}

/// Publishes verified-file progress and checks cancellation before committing
/// the staging directory. Callback failures use the same rollback as bad data.
pub fn import_content_with_progress(
    archive_path: &Path,
    destination_parent: &Path,
    tools_dir: Option<&Path>,
    mut progress: impl FnMut(&str, u64, u64) -> Result<(), String>,
) -> Result<ImportSummary, String> {
    // Load the active `tools/content-version.json` before reading the archive.
    let content_version = match tools_dir {
        Some(dir) => read_content_version(dir)?,
        None => checkout_content_version()?,
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let resolved_archive = resolve_from(&cwd, archive_path);
    let parent = resolve_from(&cwd, destination_parent);
    fs::create_dir_all(&parent)
        .map_err(|e| format!("Failed creating {}: {}", parent.display(), e))?;

    // Stage into `.import-<id>` first and atomically rename to `content-<id>` only after
    // every file hash, link target, and `.prepare.json` state record has been verified.
    let id = random_hex_suffix();
    let staging = parent.join(format!(".import-{}", id));
    let target = parent.join(format!("content-{}", id));

    let mut file = File::open(&resolved_archive).map_err(|e| {
        format!(
            "Failed to open archive {}: {}",
            resolved_archive.display(),
            e
        )
    })?;

    let result = (|| -> Result<ImportSummary, String> {
        let entries = read_zip_entries(&mut file)?;
        let manifest_entry = entries
            .get(MANIFEST_NAME)
            .ok_or_else(|| "That file is not a Battlefront Eternal content archive.".to_string())?;
        let raw = read_entry_bytes(&mut file, manifest_entry, MAX_MANIFEST)?;
        let manifest: ArchiveManifest = serde_json::from_slice(&raw)
            .map_err(|_| "Content archive manifest is damaged.".to_string())?;
        validate_manifest(&manifest, &entries, &content_version)?;

        fs::create_dir_all(&staging).map_err(|e| format!("Failed creating staging dir: {}", e))?;

        // Extract primary stored entries first and defer `linkTo` aliases until their targets exist.
        let mut linked = Vec::new();
        let total = manifest.files.len() as u64;
        let mut done = 0;
        progress("", 0, total)?;
        for item in &manifest.files {
            if item.link_to.is_some() {
                linked.push(item);
                continue;
            }
            let entry = entries
                .get(&item.path)
                .ok_or_else(|| format!("Content archive is missing {}", item.path))?;
            let parts = safe_path(&item.path, true)?;
            let mut dest = staging.clone();
            for p in parts {
                dest.push(p);
            }
            progress(&item.path, done, total)?;
            extract_entry_to_file(&mut file, entry, &dest, &item.sha256)?;
            done += 1;
            progress(&item.path, done, total)?;
        }

        // Restore hardlinked or case-folded alias entries after all primary files are on disk.
        for item in linked {
            progress(&item.path, done, total)?;
            let Some(link_to) = item.link_to.as_ref() else {
                continue;
            };
            let src_parts = safe_path(link_to, true)?;
            let dst_parts = safe_path(&item.path, true)?;
            let mut src = staging.clone();
            for p in src_parts {
                src.push(p);
            }
            let mut dst = staging.clone();
            for p in dst_parts {
                dst.push(p);
            }
            if let Some(p) = dst.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            if dst.exists() {
                // On case-insensitive filesystems (`APFS`, `NTFS`), a case-only alias resolves
                // to the same underlying inode as `src` and requires no copy.
                if same_file::is_same_file(&src, &dst).unwrap_or(false) {
                    done += 1;
                    progress(&item.path, done, total)?;
                    continue;
                }
                return Err(format!(
                    "Content archive link target collided: {}",
                    item.path
                ));
            }
            if item.hardlink != Some(false) {
                if fs::hard_link(&src, &dst).is_err() {
                    fs::copy(&src, &dst).map_err(|e| {
                        format!("Failed restoring linked file {}: {}", item.path, e)
                    })?;
                }
            } else {
                fs::copy(&src, &dst)
                    .map_err(|e| format!("Failed copying alias file {}: {}", item.path, e))?;
            }
            done += 1;
            progress(&item.path, done, total)?;
        }

        progress("", total, total)?;
        let inspected = inspect_content(&staging).ok_or_else(|| {
            "Imported archive did not produce a valid content folder.".to_string()
        })?;
        let state = read_prepare_state(Some(&staging));
        if !prepared_content_current(state.as_ref(), &content_version) {
            return Err("Imported archive is missing a current preparation record.".to_string());
        }

        // Write `.bfe-import.json` so subsequent exports or game-install relocations
        // can read the provenance and source `.lvl` hashes.
        let receipt = serde_json::json!({
            "importedAt": "2026-01-01T00:00:00.000Z",
            "archiveName": resolved_archive.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "createdAt": manifest.created_at,
            "contentVersion": manifest.content_version,
            "source": manifest.source,
            "files": manifest.files.len(),
            "bytes": manifest.total_bytes,
        });
        let mut receipt_bytes = serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?;
        receipt_bytes.push(b'\n');
        fs::write(staging.join(RECEIPT_NAME), receipt_bytes)
            .map_err(|e| format!("Failed writing {}: {}", RECEIPT_NAME, e))?;

        fs::rename(&staging, &target)
            .map_err(|e| format!("Failed moving imported content into place: {}", e))?;

        Ok(ImportSummary {
            content_dir: target,
            files: manifest.files.len(),
            bytes: manifest.total_bytes,
            maps: inspected.maps,
        })
    })();

    // Remove the temporary `.import-<id>` directory if any extraction or validation step failed.
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// Result returned by [`relocate_preparation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelocateResult {
    /// True if all fingerprinted `.lvl` files matched and `.prepare.json` was updated.
    pub ok: bool,
    /// Human-readable failure reason when `ok == false`.
    pub reason: Option<String>,
}

/// Verifies a local Star Wars Battlefront (2004) installation (`install_dir`)
/// against the `.bfe-import.json` source `.lvl` hashes of `content_dir`, and
/// updates `<content_dir>/.prepare.json`'s `game` field when they match.
///
/// Mirrors `relocatePreparation` in the desktop host.
pub fn relocate_preparation(
    content_dir: &Path,
    install_dir: &Path,
    tools_dir: Option<&Path>,
) -> Result<RelocateResult, String> {
    // Load the active `tools/content-version.json` to verify `.prepare.json` is current.
    let content_version = match tools_dir {
        Some(dir) => read_content_version(dir)?,
        None => checkout_content_version()?,
    };
    if content_dir.as_os_str().is_empty() || install_dir.as_os_str().is_empty() {
        return Ok(RelocateResult {
            ok: false,
            reason: Some("Missing content or game folder.".to_string()),
        });
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = resolve_from(&cwd, content_dir);
    // Validate that `install_dir` is a genuine Star Wars Battlefront (2004) installation.
    let Some(install) = inspect_install(install_dir) else {
        return Ok(RelocateResult {
            ok: false,
            reason: Some(
                "That folder does not look like Star Wars Battlefront (2004).".to_string(),
            ),
        });
    };
    let state = read_prepare_state(Some(&root));
    if !prepared_content_current(state.as_ref(), &content_version) {
        return Ok(RelocateResult {
            ok: false,
            reason: Some("Prepared content is not current.".to_string()),
        });
    }

    // Read `.bfe-import.json` to retrieve the `source.files` `.lvl` fingerprints.
    let receipt_raw = match fs::read_to_string(root.join(RECEIPT_NAME)) {
        Ok(r) => r,
        Err(_) => {
            return Ok(RelocateResult {
                ok: false,
                reason: Some("Content folder does not have an import receipt.".to_string()),
            });
        }
    };
    let receipt_val: serde_json::Value = match serde_json::from_str(&receipt_raw) {
        Ok(v) => v,
        Err(_) => {
            return Ok(RelocateResult {
                ok: false,
                reason: Some("Content folder does not have an import receipt.".to_string()),
            });
        }
    };
    let Some(source) = receipt_val
        .get("source")
        .and_then(|v| serde_json::from_value::<SourceReceipt>(v.clone()).ok())
    else {
        return Ok(RelocateResult {
            ok: false,
            reason: Some("Imported content archive did not record source game hashes.".to_string()),
        });
    };
    if source.base != "GameData" || source.files.is_empty() {
        return Ok(RelocateResult {
            ok: false,
            reason: Some("Imported content archive did not record source game hashes.".to_string()),
        });
    }

    // Verify every fingerprinted `.lvl` file in the candidate install directory.
    for item in &source.files {
        let parts = safe_path(&item.path, false)?;
        let mut walk_parts = vec!["GameData"];
        let part_refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
        walk_parts.extend(part_refs);
        let Some(candidate) = walk_ignore_case(&install.dir, &walk_parts) else {
            return Ok(RelocateResult {
                ok: false,
                reason: Some(format!("Game install is missing {}.", item.path)),
            });
        };
        let meta = match fs::metadata(&candidate) {
            Ok(m) if m.is_file() => m,
            _ => {
                return Ok(RelocateResult {
                    ok: false,
                    reason: Some(format!("Game install is missing {}.", item.path)),
                });
            }
        };
        if meta.len() != item.size {
            return Ok(RelocateResult {
                ok: false,
                reason: Some(format!("Game file size differs for {}.", item.path)),
            });
        }
        let (_, sha256) = hash_file(&candidate)?;
        if sha256 != item.sha256 {
            return Ok(RelocateResult {
                ok: false,
                reason: Some(format!("Game file hash differs for {}.", item.path)),
            });
        }
    }

    // Rewrite `.prepare.json` with the new install's `GameData/Data/_LVL_PC` path.
    let prep_file = root.join(".prepare.json");
    let mut prep_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&prep_file).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let new_game = install
        .dir
        .join("GameData")
        .join("Data")
        .join("_LVL_PC")
        .to_string_lossy()
        .into_owned();
    if let Some(obj) = prep_json.as_object_mut() {
        obj.insert("game".to_string(), serde_json::Value::String(new_game));
    }
    let mut out_bytes = serde_json::to_vec_pretty(&prep_json).map_err(|e| e.to_string())?;
    out_bytes.push(b'\n');
    fs::write(&prep_file, out_bytes).map_err(|e| e.to_string())?;

    Ok(RelocateResult {
        ok: true,
        reason: None,
    })
}

/// Generates a 16-character random hex suffix for temporary staging paths.
///
/// Used when naming `.part` export files, `.import-<id>` staging directories,
/// and `content-<id>` imported folders so concurrent operations never collide.
fn random_hex_suffix() -> String {
    // Sample 8 random bytes via `getrandom` and encode as 16 lowercase hex characters.
    let mut bytes = [0u8; 8];
    let _ = getrandom::getrandom(&mut bytes);
    hex_encode(&bytes)
}
