//! Portable prepared content archive export, verification, atomic import and
//! game-install relocation.
//!
//! Ports `electron/content-archive.js` of the browser build: the archives players
//! already hold (`.bfepack`, or `.zip`) are the standard, so this module reads and
//! writes that one dialect and nothing else.
//!
//! An archive is a ZIP64-capable file holding:
//! - `bfe-content-manifest.json` (`format: "battlefront-eternal-content"`,
//!   `version: 1`, `created`, `source`, `files`), the first entry the exporter writes;
//! - prepared content files under `common/`, `sides/`, `maps/`, `audio/`, and `.prepare.json`;
//! - `linkTo` manifest records (no payload) for hardlinks and case-variant duplicates;
//! - `source`, SHA-256 fingerprints of the original `.lvl` files, so
//!   [`relocate_preparation`] can check that a local install matches what the
//!   content was prepared from.
//!
//! Each `files` record is `{path, size, mtime, [linkTo, hardlink,] sha256}` exactly as
//! the browser exporter writes it. The importer accepts deflated entries (the browser
//! compresses at level 3) as well as stored ones, and streams both; the exporter stores
//! its entries, which every ZIP reader accepts, because prepared content is already
//! compressed audio, textures and packed blobs.
//!
//! ## Safety Invariants
//! Every archive entry path and manifest path is validated through [`safe_path`]
//! before touching the host filesystem:
//! - No empty segments, `.` or `..` traversal components, backslashes, or drive colons.
//! - No ASCII control characters (`<= 0x1f`) or trailing dots/spaces that Windows strips.
//! - No Windows reserved device stems (`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`).
//! - Content entries must live under `common/`, `sides/`, `maps/`, `audio/`, or `.prepare.json`.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crc32fast::Hasher as Crc32Hasher;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::discover::{inspect_install, walk_ignore_case};
use crate::paths::resolve_from;
use crate::prepare::{
    checkout_content_version, prepared_content_current, read_content_version, read_prepare_state,
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

/// What `yauzl` says for a file that has no end-of-central-directory record, which the
/// browser build shows as the failure of such an import.
const EOCD_NOT_FOUND: &str =
    "End of central directory record signature not found. Either not a zip file, or file is truncated.";

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

/// Single file record inside `bfe-content-manifest.json`.
///
/// The field order is the browser exporter's: `path`, `size`, `mtime`, then
/// `linkTo` and `hardlink` for an alias, and `sha256` last (`exportContent` adds the
/// hash after the stat fields). A source fingerprint record carries only
/// `path`, `size` and `sha256`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFileEntry {
    /// Forward-slash relative path inside the content directory.
    pub path: String,
    /// Uncompressed file size in bytes.
    pub size: u64,
    /// Modification time in milliseconds since the epoch (`stat.mtimeMs`); restored on import.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<f64>,
    /// Canonical relative path when this entry is a hardlink or case-variant alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_to: Option<String>,
    /// Whether this alias was a filesystem hardlink (`true`) or a case variant (`false`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardlink: Option<bool>,
    /// Lowercase hex SHA-256 digest of the file payload.
    pub sha256: String,
}

/// Source `.lvl` fingerprint record stored under `manifest.source`.
///
/// Used by [`relocate_preparation`] to verify that a local `GameData` folder
/// contains the exact same `.lvl` files the archive was originally built from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceReceipt {
    /// Base subdirectory inside the game install (`"GameData"`).
    pub base: String,
    /// Fingerprinted `.lvl` files relative to `GameData`.
    pub files: Vec<ManifestFileEntry>,
}

/// Deserialized representation of `bfe-content-manifest.json`: the browser build's
/// manifest, field for field. Unknown fields (an older native dialect wrote several)
/// are ignored, like the browser importer ignores them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchiveManifest {
    /// Must equal [`ARCHIVE_FORMAT`] (`"battlefront-eternal-content"`).
    pub format: String,
    /// Must equal [`ARCHIVE_VERSION`] (`1`).
    pub version: u32,
    /// ISO-8601 timestamp of the export (`new Date().toISOString()`).
    #[serde(default, alias = "createdAt", skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Source `.lvl` fingerprints for relocating against a local install, or `null`.
    #[serde(default)]
    pub source: Option<SourceReceipt>,
    /// Manifest file records (stored entries and `linkTo` aliases).
    pub files: Vec<ManifestFileEntry>,
}

/// Validates a relative archive entry path against traversal, control characters,
/// Windows reserved device names, and (when `content_only` is true) allowed top-level roots.
///
/// Returns the normalized path segments on success.
/// Mirrors `safePath` in the desktop host.
pub fn safe_path(name: &str, content_only: bool) -> Result<Vec<String>, String> {
    // Ports `safePath`: a name over 1024 characters, or with a backslash, a colon or a control
    // character, is an invalid path before it is split.
    if name.len() > 1024
        || name
            .chars()
            .any(|c| (c as u32) <= 0x1f || c == '\\' || c == ':')
    {
        return Err("Invalid archive path".to_string());
    }
    let unsafe_path = || format!("Unsafe archive path: {}", name);
    let parts: Vec<&str> = name.split('/').collect();
    for part in &parts {
        // Empty components, `.` and `..`, trailing dots or spaces (Windows strips them), and
        // Windows reserved device names before any extension.
        if part.is_empty() || *part == "." || *part == ".." {
            return Err(unsafe_path());
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Err(unsafe_path());
        }
        let stem = part.split('.').next().unwrap_or("").to_ascii_lowercase();
        let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
            || ((stem.starts_with("com") || stem.starts_with("lpt"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit()
                && stem.as_bytes()[3] != b'0');
        if reserved {
            return Err(unsafe_path());
        }
    }
    // Prepared content lives under the four roots (with something inside) or is `.prepare.json`.
    if content_only
        && name != ".prepare.json"
        && !(CONTENT_ROOTS.contains(&parts[0]) && parts.len() > 1)
    {
        return Err(format!("Not prepared game content: {}", name));
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
fn hardlink_key(_path: &Path, meta: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    if meta.nlink() > 1 && meta.ino() > 0 {
        Some((meta.dev(), meta.ino()))
    } else {
        None
    }
}

/// The same identity on Windows, which Node's `stat` also reports on NTFS: the volume serial
/// number and the 64-bit file index of the open file, when the file has more than one link.
/// `std`'s `MetadataExt` offers these only on nightly, so ask `GetFileInformationByHandle`.
#[cfg(windows)]
fn hardlink_key(path: &Path, _meta: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
    };
    let file = File::open(path).ok()?;
    // SAFETY: `info` is plain data the call fills in, and the handle is open for the call.
    let info = unsafe {
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        if GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) == 0 {
            return None;
        }
        info
    };
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    (info.nNumberOfLinks > 1 && index > 0).then_some((u64::from(info.dwVolumeSerialNumber), index))
}

/// Other platforms report no link identity; every file is exported as its own payload.
#[cfg(not(any(unix, windows)))]
fn hardlink_key(_path: &Path, _meta: &fs::Metadata) -> Option<(u64, u64)> {
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
            let key = hardlink_key(&prep, &meta);
            out.push((".prepare.json".to_string(), prep, meta.len(), key));
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
            out.push((rel, full.clone(), meta.len(), hardlink_key(&full, &meta)));
        }
    }
    Ok(())
}

/// Resolves the `GameData` directory from the `game` path recorded in `.prepare.json`.
///
/// Ports the check at the top of `provenance` in `electron/content-archive.js`: the recorded
/// path must end in `_LVL_PC` (any case), and `GameData` is two levels above it.
fn game_data_root(state_game: Option<&str>) -> Option<PathBuf> {
    let game_str = state_game.filter(|s| !s.is_empty())?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let game = resolve_from(&cwd, Path::new(game_str));
    let is_lvl_pc = game
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("_lvl_pc"));
    if !is_lvl_pc {
        return None;
    }
    Some(game.parent()?.parent()?.to_path_buf())
}

/// Collects SHA-256 fingerprints of `.lvl` files inside `GameData`, or falls back
/// to an existing `.bfe-import.json` receipt if the original game install is absent.
///
/// Mirrors `provenance` and `sourceFingerprint` in the desktop host.
pub fn source_manifest(root: &Path, state_game: Option<&str>) -> Option<SourceReceipt> {
    // A live `GameData` directory decides the answer, as `provenance` returns
    // `sourceFingerprint(...)` outright: its `.lvl` hashes, or `null` when it has none.
    if let Some(game_data) = game_data_root(state_game).filter(|d| d.is_dir()) {
        let mut files = Vec::new();
        if walk_lvl_files(&game_data, "", &mut files).is_ok() {
            files.sort_by(|a, b| a.path.cmp(&b.path));
            return (!files.is_empty()).then_some(SourceReceipt {
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
                mtime: None,
                link_to: None,
                hardlink: None,
                sha256,
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
    /// Unix file mode kept in the high half of the external attributes (`0o100644`).
    mode: u32,
}

/// Streaming uncompressed (`method = 0`) ZIP / ZIP64 archive writer.
///
/// Produces store-mode ZIP/ZIP64 archives the browser build's `yauzl` importer reads: regular-file
/// modes in the external attributes, UTF-8 names, ZIP64 records past 4 GiB.
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
    fn add_bytes(&mut self, name: &str, data: &[u8], mode: u32) -> Result<(), String> {
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
            mode,
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
        mut progress: impl FnMut(u64) -> Result<(), String>,
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
            // Reports each chunk so progress moves through a multi-gigabyte file and a
            // cancel is seen within one chunk.
            progress(n as u64)?;
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
            mode: 0o100644,
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
            rec.extend_from_slice(&(entry.mode << 16).to_le_bytes()); // external attrs: unix mode
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
    /// General-purpose bit flags (bit 0 marks an encrypted entry).
    pub flags: u16,
    /// External attributes; the high half holds the Unix file type and mode.
    pub external_attrs: u32,
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
        return Err(EOCD_NOT_FOUND.to_string());
    }
    // Read at most the trailing 65,557 bytes (22-byte EOCD + 65,535-byte max comment).
    let tail_size = total_size.min(65557) as usize;
    let tail_offset = total_size - tail_size as u64;
    let tail = read_exact_at(file, tail_offset, tail_size)?;

    // Scan backwards from the end of the file to locate the End of Central Directory (EOCD) record.
    let mut eocd_pos: Option<usize> = None;
    if let Some(max_start) = tail.len().checked_sub(22) {
        for i in (0..=max_start).rev() {
            let sig = read_u32(&tail, i, EOCD_NOT_FOUND)?;
            if sig == ZIP_EOCD_SIG {
                let comment_len =
                    read_u16(&tail, i + 20, EOCD_NOT_FOUND)? as usize;
                if i.checked_add(22).and_then(|n| n.checked_add(comment_len)) == Some(tail.len()) {
                    eocd_pos = Some(i);
                    break;
                }
            }
        }
    }
    let Some(eocd) = eocd_pos else {
        return Err(EOCD_NOT_FOUND.to_string());
    };

    let mut count = read_u16(
        &tail,
        eocd + 10,
        EOCD_NOT_FOUND,
    )? as u64;
    let mut central_size = read_u32(
        &tail,
        eocd + 12,
        EOCD_NOT_FOUND,
    )? as u64;
    let mut central_offset = read_u32(
        &tail,
        eocd + 16,
        EOCD_NOT_FOUND,
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
        let flags = read_u16(&central, pos + 8, "Archive central directory is damaged.")?;
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
        let external_attrs = read_u32(&central, pos + 38, "Archive central directory is damaged.")?;
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
                flags,
                external_attrs,
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

/// Reads and verifies the whole payload of a small ZIP entry (the manifest), stored or deflated.
///
/// Ports the manifest read of `importContent`: the entry may not exceed `max_bytes`
/// uncompressed, and its recorded size and CRC-32 must match what was read.
fn read_entry_bytes(
    file: &mut File,
    entry: &ZipEntryInfo,
    max_bytes: u64,
) -> Result<Vec<u8>, String> {
    // Enforce the caller's upper bound (`MAX_MANIFEST`) before allocating a buffer.
    if entry.size > max_bytes || entry.compressed_size > max_bytes {
        return Err("Invalid content manifest".to_string());
    }
    let data_offset = entry_data_offset(file, entry)?;
    let raw = read_exact_at(file, data_offset, entry.compressed_size as usize)?;
    let data = match entry.method {
        0 => raw,
        8 => {
            let mut out = Vec::new();
            // One byte past the recorded size catches an entry that inflates larger.
            flate2::read::DeflateDecoder::new(&raw[..])
                .take(entry.size + 1)
                .read_to_end(&mut out)
                .map_err(|e| format!("Failed decompressing {}: {}", entry.name, e))?;
            out
        }
        _ => {
            return Err(format!(
                "Unsupported archive compression method for {}",
                entry.name
            ))
        }
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

/// Streams one ZIP entry (stored or deflated) to `dest`, verifying its CRC-32 and its
/// manifest SHA-256 as the bytes pass, and calls `on_bytes` for every chunk.
///
/// Ports the per-entry loop of `importContent`: the target is created exclusively
/// (`wx`), the hash is checked after the last byte (`Checksum mismatch: ...`), and the
/// manifest's `mtime` is applied when it is finite. The callback runs every 64 KiB at
/// most, so progress follows the bytes and an `Err` (a cancel) stops the copy at once.
fn extract_entry_to_file(
    file: &mut File,
    entry: &ZipEntryInfo,
    dest: &Path,
    record: &ManifestFileEntry,
    mut on_bytes: impl FnMut(u64) -> Result<(), String>,
) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let data_offset = entry_data_offset(file, entry)?;
    file.seek(SeekFrom::Start(data_offset))
        .map_err(|e| format!("Seek failed: {}", e))?;
    let limited = (&mut *file).take(entry.compressed_size);
    let mut reader: Box<dyn Read + '_> = match entry.method {
        0 => Box::new(limited),
        8 => Box::new(flate2::read::DeflateDecoder::new(
            std::io::BufReader::with_capacity(65536, limited),
        )),
        _ => {
            return Err(format!(
                "Unsupported archive compression method for {}",
                entry.name
            ))
        }
    };
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| format!("Failed to create {}: {}", dest.display(), e))?;

    let mut crc_hasher = Crc32Hasher::new();
    let mut sha_hasher = Sha256::new();
    let mut written = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("Read error extracting {}: {}", entry.name, e))?;
        if n == 0 {
            break;
        }
        written += n as u64;
        // An entry that inflates past its recorded size is a damaged or hostile archive.
        if written > entry.size {
            return Err(format!("Archive entry size mismatch: {}", entry.name));
        }
        crc_hasher.update(&buf[..n]);
        sha_hasher.update(&buf[..n]);
        out.write_all(&buf[..n])
            .map_err(|e| format!("Write error for {}: {}", dest.display(), e))?;
        on_bytes(n as u64)?;
    }
    if written != entry.size {
        return Err(format!("Archive is truncated at {}", entry.name));
    }
    if crc_hasher.finalize() != entry.crc || hex_encode(&sha_hasher.finalize()) != record.sha256 {
        return Err(format!(
            "Checksum mismatch: {}. The export is damaged or incomplete.",
            record.path
        ));
    }
    if let Some(mtime) = record.mtime.filter(|m| m.is_finite() && *m >= 0.0) {
        let _ = out.set_modified(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(mtime / 1000.0),
        );
    }
    out.flush()
        .map_err(|e| format!("Flush error for {}: {}", dest.display(), e))?;
    Ok(())
}

/// True for `maps/<id>/map.json`, the file the browser importer looks for as proof of a
/// playable prepared map (`/^maps\/[^/]+\/map\.json$/`).
fn is_map_record(path: &str) -> bool {
    let mut parts = path.split('/');
    parts.next() == Some("maps")
        && parts.next().is_some_and(|id| !id.is_empty())
        && parts.next() == Some("map.json")
        && parts.next().is_none()
}

/// Map folder names a manifest carries, sorted.
fn manifest_maps(manifest: &ArchiveManifest) -> Vec<String> {
    let mut maps: Vec<String> = manifest
        .files
        .iter()
        .filter(|f| is_map_record(&f.path))
        .filter_map(|f| f.path.split('/').nth(1).map(str::to_string))
        .collect();
    maps.sort();
    maps.dedup();
    maps
}

/// Checks a sha256 string the way the browser does (`/^[a-f0-9]{64}$/`).
fn valid_sha256(sha: &str) -> bool {
    sha.len() == 64
        && sha
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Validates a parsed [`ArchiveManifest`] and returns the total byte count of its records.
///
/// Ports `validateManifest` in `electron/content-archive.js`, rule for rule and in its order:
/// 1. `format`, `version` and a non-empty file list within `MAX_FILES`.
/// 2. Per record: a content-only safe path, no duplicate path, no case variant that is not a
///    `linkTo` alias with the same hash and size, a safe-integer size and a lowercase SHA-256,
///    and a running total within `MAX_BYTES`.
/// 3. Every `linkTo` names a stored (non-alias) record with the same hash and size.
/// 4. `common/common.json` and at least one `maps/<id>/map.json` are present.
/// 5. `source`, when present, is a `GameData` fingerprint of at most `MAX_FILES` safe records.
pub fn validate_manifest(manifest: &ArchiveManifest) -> Result<u64, String> {
    if manifest.format != ARCHIVE_FORMAT
        || manifest.version != ARCHIVE_VERSION
        || manifest.files.is_empty()
        || manifest.files.len() > MAX_FILES
    {
        return Err("Unsupported or invalid content export".to_string());
    }
    let mut bytes = 0u64;
    let mut seen: BTreeMap<&str, &ManifestFileEntry> = BTreeMap::new();
    let mut folded: BTreeMap<String, &ManifestFileEntry> = BTreeMap::new();
    for file in &manifest.files {
        safe_path(&file.path, true)?;
        // The browser folds with NFC then lower case; content names are ASCII, so lower case alone.
        let key = file.path.to_lowercase();
        if seen.contains_key(file.path.as_str()) {
            return Err(format!("Duplicate archive path: {}", file.path));
        }
        seen.insert(&file.path, file);
        let previous = folded.get(&key).copied();
        if let Some(prev) = previous {
            let alias = file.link_to.as_deref() == Some(prev.path.as_str())
                && file.sha256 == prev.sha256
                && file.size == prev.size;
            if !alias {
                return Err(format!("Conflicting case variants: {}", file.path));
            }
        } else {
            folded.insert(key, file);
        }
        if !valid_sha256(&file.sha256) {
            return Err(format!("Invalid checksum record: {}", file.path));
        }
        bytes = bytes.saturating_add(file.size);
        if bytes > MAX_BYTES {
            return Err("Content export is too large".to_string());
        }
    }
    for file in &manifest.files {
        let Some(link) = &file.link_to else {
            continue;
        };
        safe_path(link, true)?;
        let valid = seen.get(link.as_str()).is_some_and(|target| {
            target.link_to.is_none()
                && !std::ptr::eq(*target, file)
                && target.sha256 == file.sha256
                && target.size == file.size
        });
        if !valid {
            return Err(format!("Invalid content alias: {}", file.path));
        }
    }
    if !seen.contains_key("common/common.json")
        || !manifest.files.iter().any(|f| is_map_record(&f.path))
    {
        return Err("Export has no playable prepared maps".to_string());
    }
    if let Some(source) = &manifest.source {
        if source.base != "GameData" || source.files.len() > MAX_FILES {
            return Err("Invalid source fingerprint".to_string());
        }
        for file in &source.files {
            safe_path(&file.path, false)?;
            if !valid_sha256(&file.sha256) {
                return Err("Invalid source checksum".to_string());
            }
        }
    }
    Ok(bytes)
}

/// Refuses an import that cannot fit: free space on the destination volume must cover the
/// content plus 32 MiB. Ports `diskSpace`; only Unix reports free space without extra
/// dependencies, so elsewhere the check is skipped.
#[cfg(unix)]
fn check_disk_space(dir: &Path, needed: u64) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = CString::new(dir.as_os_str().as_bytes()) else {
        return Ok(());
    };
    // SAFETY: `path` is a valid NUL-terminated string and `stat` is written by the call.
    let free = unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut stat) != 0 {
            return Ok(());
        }
        (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64)
    };
    if free < needed.saturating_add(32 * 1024 * 1024) {
        return Err(format!(
            "Not enough free disk space. About {} GB is needed.",
            needed.div_ceil(1024 * 1024 * 1024)
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_disk_space(_dir: &Path, _needed: u64) -> Result<(), String> {
    Ok(())
}

/// Formats a time as `YYYY-MM-DDTHH:MM:SS.mmmZ`, the shape of JavaScript's `toISOString`.
fn iso_timestamp(time: std::time::SystemTime) -> String {
    let since = time
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        since.subsec_millis()
    )
}

/// The modification time of a file in milliseconds since the epoch, like `stat.mtimeMs`.
fn mtime_ms(meta: &fs::Metadata) -> Option<f64> {
    let since = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(since.as_secs_f64() * 1000.0)
}

/// Summary returned when exporting an archive via [`export_content`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    /// Output archive path.
    pub archive_path: PathBuf,
    /// Number of manifest file records written.
    pub files: usize,
    /// Total logical uncompressed bytes across all manifest entries.
    pub bytes: u64,
    /// Final archive file size on disk.
    pub archive_bytes: u64,
    /// Sorted list of exported map IDs.
    pub maps: Vec<String>,
}

/// Exports a prepared content directory to an archive the browser build imports.
///
/// Writes to a temporary sibling file first and renames it on completion.
pub fn export_content(
    content_dir: &Path,
    archive_path: &Path,
    tools_dir: Option<&Path>,
) -> Result<ExportSummary, String> {
    export_content_with_progress(content_dir, archive_path, tools_dir, |_, _, _, _| Ok(()))
}

/// Like [`export_content`], reporting `(phase, file, done, total)` in bytes as the browser
/// does (`Checking content`, `Recording original source hashes`, `Exporting content`).
/// An `Err` from the callback cancels the export and removes the partial file.
///
/// Ports `exportContent` in `electron/content-archive.js`: hash every file first, build the
/// manifest, then write the manifest entry followed by the stored files in sorted order.
pub fn export_content_with_progress(
    content_dir: &Path,
    archive_path: &Path,
    tools_dir: Option<&Path>,
    mut progress: impl FnMut(&str, &str, u64, u64) -> Result<(), String>,
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
        return Err("Save the export outside the game data folder".to_string());
    }
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
    let total: u64 = files.iter().map(|f| f.2).sum();

    // Pass 1: size, mtime and hashes of every file, aliasing hardlinks and case variants.
    let mut records: Vec<ManifestFileEntry> = Vec::new();
    let mut crcs: BTreeMap<String, u32> = BTreeMap::new();
    let mut inodes: BTreeMap<(u64, u64), usize> = BTreeMap::new();
    let mut cases: BTreeMap<String, usize> = BTreeMap::new();
    let mut done = 0u64;
    for (rel, full, size, hl_key) in &files {
        let meta = fs::symlink_metadata(full)
            .map_err(|e| format!("Content changed while exporting: {} ({})", rel, e))?;
        if !meta.is_file() || meta.len() != *size {
            return Err(format!("Content changed while exporting: {}", rel));
        }
        let linked = hl_key.and_then(|k| inodes.get(&k).copied());
        let variant = cases.get(&rel.to_lowercase()).copied();
        let original = linked.or(variant);
        let (sha256, crc) = if let Some(index) = linked {
            // A hardlink shares its first path's bytes, so its hash is copied, not re-read.
            done += *size;
            progress("Checking content", rel, done, total)?;
            (records[index].sha256.clone(), 0)
        } else {
            let (sha, crc) = hash_and_crc_file(full, |n| {
                done += n;
                progress("Checking content", rel, done, total)
            })?;
            (sha, crc)
        };
        let mut record = ManifestFileEntry {
            path: rel.clone(),
            size: *size,
            mtime: mtime_ms(&meta),
            link_to: None,
            hardlink: None,
            sha256,
        };
        if let Some(index) = original {
            // `file.linkTo = original.linkTo || original.path; file.hardlink = !!linked`
            let target = &records[index];
            record.link_to = Some(
                target
                    .link_to
                    .clone()
                    .unwrap_or_else(|| target.path.clone()),
            );
            record.hardlink = Some(linked.is_some());
        } else {
            if let Some(key) = hl_key {
                inodes.insert(*key, records.len());
            }
            cases.insert(rel.to_lowercase(), records.len());
            crcs.insert(rel.clone(), crc);
        }
        records.push(record);
    }

    progress("Recording original source hashes", "", done, total)?;
    let state_game = state.as_ref().and_then(|s| s.game.as_deref());
    let manifest = ArchiveManifest {
        format: ARCHIVE_FORMAT.to_string(),
        version: ARCHIVE_VERSION,
        created: Some(iso_timestamp(std::time::SystemTime::now())),
        source: source_manifest(&root, state_game),
        files: records,
    };
    // `validateManifest(manifest)` runs before anything is written, so a bad tree fails early.
    validate_manifest(&manifest)?;
    let manifest_bytes =
        serde_json::to_vec(&manifest).map_err(|e| format!("Failed serializing manifest: {}", e))?;
    if manifest_bytes.len() as u64 > MAX_MANIFEST {
        return Err("Content manifest is too large".to_string());
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed creating {}: {}", parent.display(), e))?;
        check_disk_space(parent, total)?;
    }

    // Write into a hidden `.part` sibling first so an interrupted export never leaves a
    // half-written archive at the target path.
    let temp_path = out.with_file_name(format!(
        ".{}.{}.part",
        out.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("archive"),
        random_hex_suffix()
    ));
    let result = (|| -> Result<ExportSummary, String> {
        let mut writer = ZipStoreWriter::create(&temp_path)?;
        // The manifest is the first entry, as `zip.addBuffer(encoded, MANIFEST)` comes first.
        writer.add_bytes(MANIFEST_NAME, &manifest_bytes, 0o100664)?;
        let mut written = 0u64;
        let by_path: BTreeMap<&str, &ContentFileRecord> =
            files.iter().map(|f| (f.0.as_str(), f)).collect();
        for record in &manifest.files {
            if record.link_to.is_some() {
                written += record.size;
                continue;
            }
            let (_, full, size, _) = by_path
                .get(record.path.as_str())
                .copied()
                .ok_or_else(|| format!("Content changed while exporting: {}", record.path))?;
            let crc = crcs.get(&record.path).copied().unwrap_or(0);
            writer.add_file(&record.path, full, *size, crc, |n| {
                written += n;
                progress("Exporting content", &record.path, written, total)
            })?;
        }
        let archive_bytes = writer.finish()?;
        fs::rename(&temp_path, &out)
            .map_err(|e| format!("Failed renaming archive into place: {}", e))?;
        Ok(ExportSummary {
            archive_path: out.clone(),
            files: manifest.files.len(),
            bytes: total,
            archive_bytes,
            maps: manifest_maps(&manifest),
        })
    })();

    // Clean up the temporary `.part` file if any step failed before the rename.
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Computes `(sha256_hex, crc32)` of `path` in a single streaming read, reporting each chunk.
///
/// Reading each exported content file once for both the manifest SHA-256 and the ZIP
/// CRC-32 halves the disk I/O of an export. Ports `hashFile` with its `onBytes` callback.
fn hash_and_crc_file(
    path: &Path,
    mut on_bytes: impl FnMut(u64) -> Result<(), String>,
) -> Result<(String, u32), String> {
    let mut f =
        File::open(path).map_err(|e| format!("Failed opening {}: {}", path.display(), e))?;
    let mut crc = Crc32Hasher::new();
    let mut sha = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("Failed reading {}: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        crc.update(&buf[..n]);
        sha.update(&buf[..n]);
        on_bytes(n as u64)?;
    }
    Ok((hex_encode(&sha.finalize()), crc.finalize()))
}

/// Summary returned when inspecting an archive via [`inspect_archive`].
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
    /// Archive creation timestamp string.
    pub created: Option<String>,
}

/// What the central directory and manifest of an archive say, checked the way the import
/// checks them before it writes anything: entry kinds, the manifest, and that the stored
/// entries and the stored manifest records are the same set.
fn read_archive(
    file: &mut File,
) -> Result<(BTreeMap<String, ZipEntryInfo>, ArchiveManifest, u64), String> {
    let entries = read_zip_entries(file)?;
    for entry in entries.values() {
        // Only regular files: the high half of the external attributes is the Unix mode.
        let kind = (entry.external_attrs >> 16) & 0o170000;
        if kind != 0 && kind != 0o100000 {
            return Err("Only regular files are allowed in content exports".to_string());
        }
        if entry.flags & 1 != 0 {
            return Err("Encrypted content exports are not supported".to_string());
        }
        if entry.name != MANIFEST_NAME {
            safe_path(&entry.name, true)?;
        }
    }
    let manifest_entry = entries
        .get(MANIFEST_NAME)
        .ok_or_else(|| "This file is not a Battlefront Eternal content export".to_string())?;
    let raw = read_entry_bytes(file, manifest_entry, MAX_MANIFEST)?;
    let manifest: ArchiveManifest = serde_json::from_slice(&raw)
        .map_err(|_| "Unsupported or invalid content export".to_string())?;
    let total = validate_manifest(&manifest)?;
    let stored = manifest
        .files
        .iter()
        .filter(|f| f.link_to.is_none())
        .count();
    // The manifest entry is not a content file, so it is not counted among `entries`.
    if entries.len() - 1 != stored {
        return Err("Archive files do not match the manifest".to_string());
    }
    Ok((entries, manifest, total))
}

/// Validates an archive's central directory and manifest without extracting files.
///
/// Mirrors the manifest inspection step of `importContent` in `electron/content-archive.js`.
pub fn inspect_archive(archive_path: &Path) -> Result<InspectArchiveResult, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let resolved = resolve_from(&cwd, archive_path);
    let mut file = File::open(&resolved)
        .map_err(|e| format!("Failed to open archive {}: {}", resolved.display(), e))?;
    let (_, manifest, total) = read_archive(&mut file)?;
    Ok(InspectArchiveResult {
        archive_path: resolved,
        files: manifest.files.len(),
        total_bytes: total,
        maps: manifest_maps(&manifest),
        created: manifest.created,
    })
}

/// Summary returned when importing an archive via [`import_content`].
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

/// Extracts and verifies an archive into a new `content-<id>` directory under
/// `destination_parent`.
pub fn import_content(
    archive_path: &Path,
    destination_parent: &Path,
) -> Result<ImportSummary, String> {
    import_content_with_progress(archive_path, destination_parent, |_, _, _, _| Ok(()))
}

/// Like [`import_content`], reporting `(phase, file, done, total)` in bytes while it works
/// (`Importing and verifying`, then `Restoring shared content`), as `onProgress` does in the
/// browser build. The callback runs at least every 64 KiB of data; an `Err` from it cancels
/// the import, removes the staging directory and is returned unchanged.
///
/// Ports `importContent`: stage into `.import-<id>`, extract and verify every stored file,
/// restore the `linkTo` aliases, keep the manifest as `.bfe-import.json`, then rename the
/// staging directory to `content-<id>`. Whether the content is current for this version is
/// decided afterwards by the startup check, exactly as for a content folder.
pub fn import_content_with_progress(
    archive_path: &Path,
    destination_parent: &Path,
    mut progress: impl FnMut(&str, &str, u64, u64) -> Result<(), String>,
) -> Result<ImportSummary, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let resolved_archive = resolve_from(&cwd, archive_path);
    let parent = resolve_from(&cwd, destination_parent);
    fs::create_dir_all(&parent)
        .map_err(|e| format!("Failed creating {}: {}", parent.display(), e))?;

    // Stage into `.import-<id>` first and rename to `content-<id>` only after every file
    // hash, alias and the receipt are in place.
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
        let (entries, manifest, total) = read_archive(&mut file)?;
        check_disk_space(&parent, total)?;
        fs::create_dir_all(&staging).map_err(|e| format!("Failed creating staging dir: {}", e))?;

        // Stored files first, in manifest order; aliases wait until their targets exist.
        let mut done = 0u64;
        progress("Starting…", "", 0, total)?;
        let mut linked = Vec::new();
        for item in &manifest.files {
            if item.link_to.is_some() {
                linked.push(item);
                continue;
            }
            let entry = entries
                .get(&item.path)
                .filter(|e| e.size == item.size)
                .ok_or_else(|| format!("Unexpected archive entry: {}", item.path))?;
            let mut dest = staging.clone();
            dest.extend(safe_path(&item.path, true)?);
            extract_entry_to_file(&mut file, entry, &dest, item, |n| {
                done += n;
                progress("Importing and verifying", &item.path, done, total)
            })?;
        }

        // Restore hardlinked and case-variant aliases from their stored targets.
        for item in linked {
            let Some(link_to) = item.link_to.as_ref() else {
                continue;
            };
            let mut src = staging.clone();
            src.extend(safe_path(link_to, true)?);
            let mut dst = staging.clone();
            dst.extend(safe_path(&item.path, true)?);
            if let Some(p) = dst.parent() {
                fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            if dst.exists() {
                // A case-insensitive filesystem already resolves a case variant to its target
                // file; accept it only after checking the bytes.
                let same = same_file::is_same_file(&src, &dst).unwrap_or(false)
                    || hash_file(&dst)
                        .map(|(_, sha)| sha == item.sha256)
                        .unwrap_or(false);
                if !same {
                    return Err(format!("Conflicting content alias: {}", item.path));
                }
            } else if item.hardlink == Some(true) {
                if fs::hard_link(&src, &dst).is_err() {
                    fs::copy(&src, &dst).map_err(|e| {
                        format!("Failed restoring linked file {}: {}", item.path, e)
                    })?;
                }
            } else {
                fs::copy(&src, &dst)
                    .map_err(|e| format!("Failed copying alias file {}: {}", item.path, e))?;
            }
            done += item.size;
            progress("Restoring shared content", &item.path, done, total)?;
        }

        // Retain provenance outside the playable roots, as the manifest itself: the browser's
        // `relocatePreparation` reads `source` from this receipt.
        let receipt = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
        fs::write(staging.join(RECEIPT_NAME), receipt)
            .map_err(|e| format!("Failed writing {}: {}", RECEIPT_NAME, e))?;
        fs::rename(&staging, &target)
            .map_err(|e| format!("Failed moving imported content into place: {}", e))?;
        Ok(ImportSummary {
            content_dir: target.clone(),
            files: manifest.files.len(),
            bytes: total,
            maps: manifest_maps(&manifest),
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
