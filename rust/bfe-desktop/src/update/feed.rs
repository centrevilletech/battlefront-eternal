//! Signed channel envelope verification (`updates/<channel>.json`), Zstandard
//! platform manifest decompression (`<release_id>/update-<platform>.json.zst`),
//! and HTTP/HTTPS fetching.
//!
//! Mirrors `electron/update/feed.js` (`openEnvelope`, `validateTree`,
//! `readBody`, `fetchRelease`, `fetchTree`, `MAX_ENVELOPE`, `MAX_TREE`,
//! `MAX_TREE_JSON`).

use std::collections::BTreeMap;
use std::ffi::{c_char, c_void, CStr};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};

use crate::update::keys::parse_verifying_key;
use crate::update::tree::{is_valid_sha256, safe_relative_path, sha256_bytes};
use crate::update::version::parse_version;

/// Maximum byte size of a signed `updates/<channel>.json` response (`1 MiB`).
///
/// Mirrors `MAX_ENVELOPE` in `electron/update/feed.js`.
pub const MAX_CHANNEL_BYTES: usize = 1024 * 1024;

/// Maximum compressed byte size of `<release_id>/update-<platform>.json.zst` (`32 MiB`).
///
/// Mirrors `MAX_TREE` in `electron/update/feed.js`.
pub const MAX_MANIFEST_BYTES: usize = 32 * 1024 * 1024;

/// Maximum decompressed JSON byte size of `update-<platform>.json` (`256 MiB`).
///
/// Mirrors `MAX_TREE_JSON` in `electron/update/feed.js`.
pub const MAX_TREE_JSON: usize = 256 * 1024 * 1024;

/// Default timeout in milliseconds (`30_000`) for channel and manifest HTTP requests.
///
/// Mirrors `DEFAULT_TIMEOUT_MS` in `electron/update/feed.js`.
pub const DEFAULT_TIMEOUT_MS: u64 = 30 * 1000;

/// Default release channel name (`"development-preview"`).
pub const DEFAULT_CHANNEL: &str = "development-preview";

#[repr(C)]
struct ZstdInBuffer {
    src: *const u8,
    size: usize,
    pos: usize,
}

#[repr(C)]
struct ZstdOutBuffer {
    dst: *mut u8,
    size: usize,
    pos: usize,
}

extern "C" {
    fn ZSTD_createDStream() -> *mut c_void;
    fn ZSTD_freeDStream(zds: *mut c_void) -> usize;
    fn ZSTD_initDStream(zds: *mut c_void) -> usize;
    fn ZSTD_decompressStream(
        zds: *mut c_void,
        output: *mut ZstdOutBuffer,
        input: *mut ZstdInBuffer,
    ) -> usize;
    fn ZSTD_isError(code: usize) -> u32;
    fn ZSTD_getErrorName(code: usize) -> *const c_char;
}

struct ZstdDStreamGuard(*mut c_void);

impl ZstdDStreamGuard {
    fn new() -> Result<Self, String> {
        let ptr = unsafe { ZSTD_createDStream() };
        if ptr.is_null() {
            return Err("Failed to allocate Zstandard DStream".to_string());
        }
        let init_rc = unsafe { ZSTD_initDStream(ptr) };
        if unsafe { ZSTD_isError(init_rc) } != 0 {
            unsafe {
                ZSTD_freeDStream(ptr);
            }
            return Err(format!("ZSTD_initDStream failed: {}", zstd_error_string(init_rc)));
        }
        Ok(Self(ptr))
    }
}

impl Drop for ZstdDStreamGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                ZSTD_freeDStream(self.0);
            }
        }
    }
}

fn zstd_error_string(code: usize) -> String {
    unsafe {
        let ptr = ZSTD_getErrorName(code);
        if ptr.is_null() {
            format!("zstd error {code}")
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

/// Decompresses a Zstandard stream from `reader` into `writer`, enforcing an
/// upper bound of `max_decompressed_bytes` on the decompressed output.
///
/// Returns the total number of decompressed bytes written.
pub fn decompress_zstd_stream<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    max_decompressed_bytes: u64,
    limit_error_msg: &str,
) -> Result<u64, String> {
    let dstream = ZstdDStreamGuard::new()?;
    let mut in_chunk = vec![0u8; 64 * 1024];
    let mut out_chunk = vec![0u8; 64 * 1024];
    let mut total_written = 0u64;
    let mut last_ret = 1usize;
    let mut saw_any_input = false;

    loop {
        let n = reader
            .read(&mut in_chunk)
            .map_err(|e| format!("Zstandard stream read error: {e}"))?;
        if n == 0 {
            break;
        }
        saw_any_input = true;
        let mut input = ZstdInBuffer {
            src: in_chunk.as_ptr(),
            size: n,
            pos: 0,
        };
        while input.pos < input.size {
            let mut output = ZstdOutBuffer {
                dst: out_chunk.as_mut_ptr(),
                size: out_chunk.len(),
                pos: 0,
            };
            let ret = unsafe { ZSTD_decompressStream(dstream.0, &mut output, &mut input) };
            if unsafe { ZSTD_isError(ret) } != 0 {
                return Err(format!(
                    "Zstandard decompression failed: {}",
                    zstd_error_string(ret)
                ));
            }
            last_ret = ret;
            if output.pos > 0 {
                total_written = total_written
                    .checked_add(output.pos as u64)
                    .ok_or_else(|| limit_error_msg.to_string())?;
                if total_written > max_decompressed_bytes {
                    return Err(limit_error_msg.to_string());
                }
                writer
                    .write_all(&out_chunk[..output.pos])
                    .map_err(|e| format!("Zstandard stream write error: {e}"))?;
            }
        }
    }

    if !saw_any_input || last_ret != 0 {
        return Err("Truncated or empty Zstandard frame".to_string());
    }
    Ok(total_written)
}

/// Decompresses an in-memory Zstandard frame up to `max_bytes`.
pub fn decompress_zstd_bytes(
    compressed: &[u8],
    max_bytes: usize,
    limit_error_msg: &str,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    decompress_zstd_stream(
        compressed,
        &mut out,
        max_bytes as u64,
        limit_error_msg,
    )?;
    Ok(out)
}

/// Encodes `raw` into a valid RFC 8878 Zstandard frame using uncompressed (`Raw_Block`)
/// blocks (maximum 128 KiB per block).
///
/// Decodable by `ZSTD_decompressStream`, `zstd -d`, and Node's `zlib.zstdDecompressSync`.
pub fn encode_zstd_store_frame(raw: &[u8]) -> Vec<u8> {
    const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
    const MAX_BLOCK: usize = 128 * 1024;
    let mut out = Vec::with_capacity(raw.len() + 16 + (raw.len() / MAX_BLOCK) * 3);
    out.extend_from_slice(&ZSTD_MAGIC);
    // Frame_Header_Descriptor: Frame_Content_Size_Flag=2 (4 bytes), Single_Segment_Flag=1.
    out.push(0xA0);
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());

    if raw.is_empty() {
        // Single empty Raw_Block with Last_Block = 1.
        out.extend_from_slice(&[0x01, 0x00, 0x00]);
        return out;
    }

    let chunks: Vec<&[u8]> = raw.chunks(MAX_BLOCK).collect();
    for (idx, chunk) in chunks.iter().enumerate() {
        let is_last = idx + 1 == chunks.len();
        let header: u32 = ((chunk.len() as u32) << 3) | u32::from(is_last);
        out.push((header & 0xFF) as u8);
        out.push(((header >> 8) & 0xFF) as u8);
        out.push(((header >> 16) & 0xFF) as u8);
        out.extend_from_slice(chunk);
    }
    out
}

/// Returns true when `name` matches `/^[a-z0-9][a-z0-9-]{0,62}$/`.
///
/// Mirrors the channel validation regex in `electron/update/feed.js`.
pub fn is_valid_channel_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    if !matches!(bytes[0], b'a'..=b'z' | b'0'..=b'9') {
        return false;
    }
    bytes
        .iter()
        .all(|b| matches!(*b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// Normalizes an optional channel name, falling back to [`DEFAULT_CHANNEL`]
/// (`"development-preview"`) when empty or invalid.
pub fn normalize_channel(channel: Option<&str>) -> &str {
    let Some(raw) = channel.map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        return DEFAULT_CHANNEL;
    };
    if is_valid_channel_name(raw) {
        raw
    } else {
        DEFAULT_CHANNEL
    }
}

/// Outer signed JSON envelope stored at `updates/<channel>.json`.
///
/// Mirrors the `{ key, payload, signature }` envelope in `electron/update/feed.js`
/// and `build/publish-distribution.py`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedChannelEnvelope {
    /// 16-character hex key identifier (`"33913637e5932c25"`).
    pub key: String,
    /// Base64-encoded UTF-8 JSON bytes of [`ChannelPayload`].
    pub payload: String,
    /// Base64-encoded 64-byte Ed25519 signature over the raw decoded `payload` bytes.
    pub signature: String,
}

/// Decoded and signature-verified payload inside `updates/<channel>.json`.
///
/// Mirrors the payload returned by `openEnvelope` in `electron/update/feed.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelPayload {
    /// Channel name (`"development-preview"`).
    pub channel: String,
    /// ISO-8601 timestamp or date when the channel pointer was published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Release metadata pointed to by this channel.
    pub release: ChannelRelease,
}

/// Release pointer inside [`ChannelPayload`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRelease {
    /// Release identifier (`"preview-332"`).
    pub id: String,
    /// Semantic version string (`"0.1.0-preview.332"`).
    pub version: String,
    /// Source git commit SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Per-platform compressed manifest references keyed by platform (`"linux-x64"`, `"win32-x64"`).
    pub platforms: BTreeMap<String, ChannelPlatformEntry>,
}

/// Per-platform manifest descriptor inside [`ChannelRelease::platforms`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelPlatformEntry {
    /// Relative path to `<release_id>/update-<platform>.json.zst`.
    pub manifest: String,
    /// Lowercase hex SHA-256 of the compressed `.json.zst` file.
    pub sha256: String,
    /// Byte size (`size` in `publish-distribution.py`).
    #[serde(default)]
    pub size: u64,
    /// Optional total uncompressed byte size of all files in the release tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_size: Option<u64>,
}

/// Symbolic link entry inside [`PlatformManifest::links`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestLinkEntry {
    /// Relative POSIX path of the symlink inside the version directory.
    pub path: String,
    /// Relative target path of the symlink.
    pub target: String,
}

/// Decompressed platform update manifest (`<release_id>/update-<platform>.json.zst`).
///
/// Mirrors the manifest validated by `validateTree` in `electron/update/tree.js`
/// and generated by `build/native/update-payload.py`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformManifest {
    /// Semantic version string (`"0.1.0-preview.332"`).
    pub version: String,
    /// Release identifier (`"preview-332"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    /// Source git commit SHA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Target platform key (`"linux-x64"`, `"linux-arm64"`, `"win32-x64"`).
    pub platform: String,
    /// Relative path to the main executable (`"bfe-client"` or `"Battlefront Eternal.exe"`).
    pub executable: String,
    /// Optional relative path to the dedicated server executable (`"bfeserver"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_executable: Option<String>,
    /// Complete file list for the release.
    pub files: Vec<ManifestFileEntry>,
    /// Relative symbolic links inside the release tree (Linux).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<ManifestLinkEntry>,
    /// Empty directories to create inside the release tree.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dirs: Vec<String>,
}

/// Single file entry inside [`PlatformManifest::files`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFileEntry {
    /// Relative POSIX path inside the version directory.
    pub path: String,
    /// Uncompressed byte size of the file.
    pub size: u64,
    /// Lowercase hex SHA-256 of the uncompressed file.
    pub sha256: String,
    /// POSIX file mode (`0o755` for executables, `0o644` for regular files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
    /// Relative path to the compressed blob (`"updates/blobs/<aa>/<sha256>.zst"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    /// Byte size of the `.zst` blob (`zsize` in `build/native/update-payload.py`).
    #[serde(
        default,
        alias = "zsize",
        skip_serializing_if = "Option::is_none"
    )]
    pub compressed_size: Option<u64>,
}

#[derive(Deserialize)]
struct RawFlatChannelPayload {
    channel: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    release: Option<ChannelRelease>,
    #[serde(default)]
    release_id: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    revision: Option<String>,
    #[serde(default)]
    platforms: Option<BTreeMap<String, ChannelPlatformEntry>>,
}

/// Verifies a signed channel envelope (`{ key, payload, signature }`) against `keys`
/// using Ed25519 over the raw Base64-decoded `payload` bytes, then parses and validates
/// the inner [`ChannelPayload`].
///
/// Mirrors `openEnvelope` in `electron/update/feed.js`.
pub fn verify_envelope(
    envelope_val: &serde_json::Value,
    keys: &BTreeMap<String, String>,
) -> Result<ChannelPayload, String> {
    let obj = envelope_val
        .as_object()
        .ok_or_else(|| "Update channel envelope is not an object".to_string())?;
    let key = obj
        .get("key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let payload_b64 = obj
        .get("payload")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let signature_b64 = obj
        .get("signature")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let (Some(key_id), Some(payload_str), Some(sig_str)) = (key, payload_b64, signature_b64) else {
        return Err("Update channel envelope is missing key, payload, or signature".to_string());
    };

    let pem = keys
        .get(key_id)
        .ok_or_else(|| format!("Unknown update signing key: {key_id}"))?;
    let verifying_key = parse_verifying_key(pem)?;

    let raw_payload = BASE64_STANDARD
        .decode(payload_str)
        .map_err(|_| "Update channel payload is not valid base64".to_string())?;
    let raw_sig = BASE64_STANDARD
        .decode(sig_str)
        .map_err(|_| "Invalid update signature length".to_string())?;
    if raw_sig.len() != 64 {
        return Err("Invalid update signature length".to_string());
    }
    let sig_bytes: [u8; 64] = raw_sig
        .try_into()
        .map_err(|_| "Invalid update signature length".to_string())?;
    let sig = Signature::from_bytes(&sig_bytes);

    verifying_key
        .verify_strict(&raw_payload, &sig)
        .map_err(|_| "Update channel signature verification failed".to_string())?;

    let raw: RawFlatChannelPayload = serde_json::from_slice(&raw_payload)
        .map_err(|_| "Update channel payload is not valid JSON".to_string())?;

    if !is_valid_channel_name(&raw.channel) {
        return Err("Update channel payload is missing a valid channel name".to_string());
    }

    let release = if let Some(rel) = raw.release {
        rel
    } else {
        let id = raw
            .release_id
            .ok_or_else(|| "Update channel payload is missing release metadata".to_string())?;
        let version = raw
            .version
            .ok_or_else(|| "Update channel payload is missing release metadata".to_string())?;
        let platforms = raw
            .platforms
            .ok_or_else(|| "Update channel payload is missing release metadata".to_string())?;
        ChannelRelease {
            id,
            version,
            revision: raw.revision,
            platforms,
        }
    };

    if release.id.is_empty() || parse_version(&release.version).is_none() {
        return Err("Update channel payload is missing release metadata".to_string());
    }

    for entry in release.platforms.values() {
        if safe_relative_path(&entry.manifest).is_none() || !is_valid_sha256(&entry.sha256) {
            return Err("Update channel payload has a malformed platform entry".to_string());
        }
    }

    Ok(ChannelPayload {
        channel: raw.channel,
        published_at: raw.published_at.or(raw.date),
        release,
    })
}

/// Validates a relative symlink target (`linkTarget` in `electron/update/tree.js`),
/// ensuring the resolved path stays inside the version directory.
pub fn validate_link_target(link_path: &str, target: &str) -> Option<String> {
    let clean_link = safe_relative_path(link_path)?;
    if target.is_empty() || target.starts_with('/') || target.contains('\\') {
        return None;
    }
    let parent_parts: Vec<&str> = clean_link
        .rsplit_once('/')
        .map(|(dir, _)| dir.split('/').filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let mut stack = parent_parts;
    for part in target.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            stack.pop()?;
        } else {
            stack.push(part);
        }
    }
    if stack.is_empty() {
        return None;
    }
    safe_relative_path(&stack.join("/"))
}

/// Validates a decoded [`PlatformManifest`] against `expected_version` and `expected_platform`.
///
/// Mirrors `validateTree` in `electron/update/tree.js`.
pub fn validate_manifest(
    manifest: &PlatformManifest,
    expected_version: &str,
    expected_platform: &str,
) -> Result<(), String> {
    if manifest.version != expected_version {
        return Err(format!(
            "Update manifest version mismatch: expected {expected_version}, got {}",
            manifest.version
        ));
    }
    if manifest.platform != expected_platform {
        return Err(format!(
            "Update manifest platform mismatch: expected {expected_platform}, got {}",
            manifest.platform
        ));
    }
    let exe_rel = safe_relative_path(&manifest.executable)
        .ok_or_else(|| "Update manifest is missing executable".to_string())?;
    if manifest.files.is_empty() {
        return Err("Update manifest contains no files".to_string());
    }
    let mut found_exe = false;
    for entry in &manifest.files {
        let Some(rel) = safe_relative_path(&entry.path) else {
            return Err(format!("Unsafe path in update manifest: {}", entry.path));
        };
        if !is_valid_sha256(&entry.sha256) {
            return Err(format!(
                "Invalid file hash in update manifest: {}",
                entry.path
            ));
        }
        if rel == exe_rel {
            found_exe = true;
        }
    }
    if !found_exe {
        return Err("Update manifest executable is not in file list".to_string());
    }
    for link in &manifest.links {
        if validate_link_target(&link.path, &link.target).is_none() {
            return Err(format!(
                "Unsafe symlink in update manifest: {} -> {}",
                link.path, link.target
            ));
        }
    }
    for dir in &manifest.dirs {
        if safe_relative_path(dir).is_none() {
            return Err(format!("Unsafe directory in update manifest: {dir}"));
        }
    }
    Ok(())
}

/// Verifies `compressed` against `expected_sha256`, decompresses it with Zstandard
/// (up to [`MAX_TREE_JSON`]), parses the JSON into [`PlatformManifest`], and runs
/// [`validate_manifest`].
///
/// Mirrors the manifest verification and decompression pipeline in `fetchTree`
/// (`electron/update/feed.js`).
pub fn decode_and_validate_manifest(
    compressed: &[u8],
    expected_sha256: &str,
    expected_version: &str,
    expected_platform: &str,
) -> Result<PlatformManifest, String> {
    if compressed.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "Update response exceeded {MAX_MANIFEST_BYTES} bytes"
        ));
    }
    let actual_hash = sha256_bytes(compressed);
    if actual_hash != expected_sha256.to_ascii_lowercase() {
        return Err(format!(
            "Update manifest hash mismatch: expected {expected_sha256}, got {actual_hash}"
        ));
    }
    let decompressed = decompress_zstd_bytes(
        compressed,
        MAX_TREE_JSON,
        "Update manifest exceeds size limit",
    )?;
    let manifest: PlatformManifest = serde_json::from_slice(&decompressed)
        .map_err(|e| format!("Update manifest is not valid JSON: {e}"))?;
    validate_manifest(&manifest, expected_version, expected_platform)?;
    Ok(manifest)
}

/// Safely joins a relative path (`updates/<channel>.json`, `<release_id>/update-<platform>.json.zst`,
/// or `updates/blobs/<aa>/<sha256>.zst`) onto `base`.
///
/// Rejects relative paths starting with `/` or containing `..` or `:`.
pub fn join_url(base: &str, rel: &str) -> Result<String, String> {
    if rel.is_empty() || rel.starts_with('/') || rel.contains("..") || rel.contains(':') {
        return Err(format!("Unsafe relative update URL: {rel}"));
    }
    let clean_base = base.trim_end_matches('/');
    Ok(format!("{clean_base}/{rel}"))
}

/// Opens a streaming reader for `url` (`http://`, `https://`, or `file://`) with
/// timeout `timeout_ms`.
pub fn open_url_reader(url: &str, timeout_ms: u64) -> Result<Box<dyn Read + Send>, String> {
    if let Some(file_path) = url.strip_prefix("file://") {
        let f = fs::File::open(Path::new(file_path))
            .map_err(|e| format!("Update request failed ({url}): {e}"))?;
        return Ok(Box::new(f));
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(format!("Unsupported update URL protocol: {url}"));
    }

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_millis(timeout_ms)))
        .max_redirects(5)
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let response = agent
        .get(url)
        .header("User-Agent", "BattlefrontEternal-Updater/1")
        .call()
        .map_err(|e| format!("Update request failed ({url}): {e}"))?;

    let status = response.status().as_u16();
    if status != 200 {
        return Err(format!("Update request failed ({status}): {url}"));
    }
    Ok(Box::new(response.into_body().into_reader()))
}

/// Downloads `url` into memory, aborting if the body exceeds `max_bytes`.
///
/// Mirrors `readBody` in `electron/update/feed.js`.
pub fn fetch_buffer(url: &str, max_bytes: usize, timeout_ms: u64) -> Result<Vec<u8>, String> {
    let mut reader = open_url_reader(url, timeout_ms)?;
    let mut out = Vec::new();
    let mut chunk = [0u8; 32 * 1024];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("Update read failed ({url}): {e}"))?;
        if n == 0 {
            break;
        }
        if out.len() + n > max_bytes {
            return Err(format!("Update response exceeded {max_bytes} bytes: {url}"));
        }
        out.extend_from_slice(&chunk[..n]);
    }
    Ok(out)
}

/// Fetches and verifies `<base_url>/updates/<channel>.json`.
///
/// Mirrors `fetchRelease` in `electron/update/feed.js`.
pub fn fetch_channel(
    base_url: &str,
    channel: &str,
    keys: &BTreeMap<String, String>,
    timeout_ms: u64,
) -> Result<ChannelPayload, String> {
    if !is_valid_channel_name(channel) {
        return Err(format!("Invalid update channel: {channel}"));
    }
    let url = join_url(base_url, &format!("updates/{channel}.json"))?;
    let raw = fetch_buffer(&url, MAX_CHANNEL_BYTES, timeout_ms)?;
    let envelope: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|_| "Update channel response is not valid JSON".to_string())?;
    let payload = verify_envelope(&envelope, keys)?;
    if payload.channel != channel {
        return Err(format!(
            "Update channel mismatch: expected {channel}, got {}",
            payload.channel
        ));
    }
    Ok(payload)
}

/// Fetches, verifies by SHA-256, decompresses with Zstandard, and validates the
/// platform manifest (`<release_id>/update-<platform>.json.zst`) for `release`.
///
/// Mirrors `fetchTree` in `electron/update/feed.js`.
pub fn fetch_manifest(
    base_url: &str,
    release: &ChannelRelease,
    platform: &str,
    timeout_ms: u64,
) -> Result<PlatformManifest, String> {
    let entry = release
        .platforms
        .get(platform)
        .ok_or_else(|| format!("No update manifest for {platform}"))?;
    if entry.manifest.is_empty() || !is_valid_sha256(&entry.sha256) {
        return Err(format!(
            "Malformed platform manifest entry for {platform}"
        ));
    }
    let url = join_url(base_url, &entry.manifest)?;
    let compressed = fetch_buffer(&url, MAX_MANIFEST_BYTES, timeout_ms)?;
    let mut manifest =
        decode_and_validate_manifest(&compressed, &entry.sha256, &release.version, platform)?;
    if manifest.release_id.is_none() {
        manifest.release_id = Some(release.id.clone());
    }
    if manifest.revision.is_none() {
        manifest.revision = release.revision.clone();
    }
    Ok(manifest)
}
