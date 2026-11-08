//! Signed release feed parsing, Ed25519 envelope/signature verification,
//! canonical JSON serialization, and update target selection (`payload` vs `full`).
//!
//! Mirrors `electron/update/feed.js` (`FEED`, `keyId`, `openEnvelope`) and
//! provides structured release feed validation and target selection.
//!
//! ## Feed Verification & Selection Pipeline
//! 1. **Key ID derivation (`key_id`)**:
//!    `sha256(spki_der)` truncated to the first 8 bytes (`16` lowercase hex characters).
//! 2. **Envelope verification (`open_envelope`)**:
//!    Verifies a `{ key, payload, signature }` envelope signed with Ed25519 over the
//!    raw decoded `payload` bytes, then validates `schema == 1`, `channel`, `version`,
//!    `release_id`, and the `<platform>-<arch>` manifest entries.
//! 3. **Structured feed verification (`parse_and_verify_feed`)**:
//!    Computes [`feed_signing_payload`] by stripping `"signature"` and serializing the
//!    remaining object via [`canonical_json`], verifies the Ed25519 signature against
//!    the trusted keyring, and validates every release's `payload` and `installers`.
//! 4. **Target selection (`select_update_target`)**:
//!    Sorts releases in descending SemVer/Debian order, compares the newest release
//!    against `current_version`, and selects `"payload"` when `shell_version >= minShellVersion`
//!    or `"full"` when a full native installer is required.

use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::update::keys::{default_update_public_keys, UPDATE_CHANNELS};
use crate::update::tree::safe_relative;
use crate::update::version::{compare_versions, is_newer_version, parse_version};

/// Default base URL for the release feed and content-addressed blob store.
///
/// Mirrors `FEED` in `electron/update/feed.js`.
pub const FEED: &str = "https://get.battlefronteternal.com/downloads/";

/// Maximum byte size accepted for a signed channel envelope response (`1 MiB`).
///
/// Mirrors `MAX_ENVELOPE` in `electron/update/feed.js`.
pub const MAX_ENVELOPE: usize = 1 << 20;

/// Expected `format` string in a structured update feed document.
pub const FEED_FORMAT: &str = "battlefront-eternal-update-feed";

/// Expected `schema` integer in a signed release or feed document (`1`).
pub const FEED_SCHEMA: u32 = 1;

/// Computes the 16-character hex key identifier for a DER-encoded
/// SubjectPublicKeyInfo (`sha256(spki).hex()[0..16]`).
///
/// Mirrors `keyId` in `electron/update/feed.js`.
pub fn key_id(spki_der: &[u8]) -> String {
    // Hash the raw SPKI DER bytes with SHA-256 and take the first 8 bytes (16 hex chars).
    let mut hasher = Sha256::new();
    hasher.update(spki_der);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(16);
    for b in &digest[..8] {
        use std::fmt::Write;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

/// Signed release envelope (`{ key, payload, signature }`) verified by [`open_envelope`].
///
/// Mirrors the envelope argument to `openEnvelope` in `electron/update/feed.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedEnvelope {
    /// 16-character hex signing key ID (`keyId(spki)`).
    pub key: String,
    /// Base64-encoded UTF-8 JSON release payload.
    pub payload: String,
    /// Base64-encoded 64-byte Ed25519 signature over the decoded `payload` bytes.
    pub signature: String,
}

/// Per-platform manifest reference inside an [`EnvelopeRelease`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopePlatformEntry {
    /// Lowercase 64-character hex SHA-256 of the compressed manifest file.
    pub sha256: String,
    /// Relative path to the zstd-compressed file manifest under [`FEED`].
    pub manifest: String,
}

/// Verified release object returned by [`open_envelope`].
///
/// Mirrors the release object validated and returned by `openEnvelope` in
/// `electron/update/feed.js`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvelopeRelease {
    /// Release schema version (`1`).
    pub schema: u32,
    /// Release channel (`"stable"`, `"preview"`, etc.).
    pub channel: String,
    /// Release version string (`"0.1.0-preview.262"`).
    pub version: String,
    /// Release identifier (`1..=64` ASCII `[A-Za-z0-9._-]`).
    pub release_id: String,
    /// Per-platform manifest entries keyed by `<platform>-<arch>`.
    pub platforms: BTreeMap<String, EnvelopePlatformEntry>,
}

/// Opens and verifies a [`SignedEnvelope`] against `keys` (`key_id -> base64 SPKI DER`),
/// returning the parsed [`EnvelopeRelease`] if the Ed25519 signature and release
/// fields are valid.
///
/// Mirrors `openEnvelope` in `electron/update/feed.js`.
///
/// Verification steps:
/// 1. Look up `envelope.key` in `keys` and reject unknown key IDs.
/// 2. Base64-decode `envelope.payload` and `envelope.signature` (requiring 64 signature bytes).
/// 3. Verify the Ed25519 signature over `payload_bytes` using the SPKI public key.
/// 4. Parse `payload_bytes` as JSON and validate `schema == 1`, `channel`, `version`,
///    `release_id`, and every `<platform>-<arch>` manifest entry.
pub fn open_envelope(
    envelope: &SignedEnvelope,
    keys: &BTreeMap<String, String>,
) -> Result<EnvelopeRelease, String> {
    // Look up the trusted SPKI public key by `envelope.key` before decoding the payload.
    let spki_b64 = keys
        .get(&envelope.key)
        .ok_or_else(|| "The update feed is not signed by a trusted key".to_string())?;
    let payload_bytes = BASE64_STANDARD
        .decode(envelope.payload.as_bytes())
        .map_err(|_| "The update feed signature is invalid".to_string())?;
    let sig_bytes = BASE64_STANDARD
        .decode(envelope.signature.as_bytes())
        .map_err(|_| "The update feed signature is invalid".to_string())?;
    if sig_bytes.len() != 64 {
        return Err("The update feed signature is invalid".to_string());
    }
    // Verify the 64-byte Ed25519 signature over the decoded payload bytes.
    let verifying_key = parse_ed25519_public_key_pem(spki_b64)
        .map_err(|_| "The update feed is not signed by a trusted key".to_string())?;
    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|_| "The update feed signature is invalid".to_string())?;
    verifying_key
        .verify(&payload_bytes, &signature)
        .map_err(|_| "The update feed signature is invalid".to_string())?;

    // Parse and validate the inner release JSON schema, version, release_id, and platform map.
    let release: EnvelopeRelease = serde_json::from_slice(&payload_bytes)
        .map_err(|_| "The update feed is malformed".to_string())?;
    if release.schema != 1
        || release.channel.is_empty()
        || parse_version(&release.version).is_none()
        || release.release_id.is_empty()
        || release.release_id.len() > 64
        || !release
            .release_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("The update feed is malformed".to_string());
    }
    // Verify that every `<platform>-<arch>` key is lowercase alphanumeric and points to
    // a safe relative manifest path with a valid 64-char hex SHA-256.
    for (plat, entry) in &release.platforms {
        let valid_plat = plat
            .split_once('-')
            .map(|(a, b)| {
                !a.is_empty()
                    && !b.is_empty()
                    && a.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    && b.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
            .unwrap_or(false);
        if !valid_plat || !is_sha256_hex(&entry.sha256) || !safe_relative(&entry.manifest) {
            return Err("The update feed is malformed".to_string());
        }
    }
    Ok(release)
}

/// Returns the platform/architecture key (`"linux-x64"`, `"win32-x64"`, `"darwin-arm64"`)
/// used to index platform artifacts in an update feed.
///
/// Mirrors `platformKey` in `electron/update/state.js`.
pub fn platform_arch_key(platform: &str, arch: &str) -> String {
    // Normalize Rust or OS aliases (`windows` -> `win32`, `macos` -> `darwin`, `x86_64` -> `x64`).
    let p = match platform {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    let a = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{}-{}", p, a)
}

/// Normalizes a channel string (`"stable"` or `"nightly"`), falling back to `"stable"`.
pub fn normalize_channel(channel: Option<&str>) -> &'static str {
    // Any channel other than `"nightly"` defaults to `"stable"`.
    match channel.map(|s| s.trim().to_ascii_lowercase()) {
        Some(ref s) if s == "nightly" => "nightly",
        _ => "stable",
    }
}

/// Validates that `url_str` is an `https:` URL (or `http://localhost` / `http://127.0.0.1`
/// when `allow_insecure_local` is true).
pub fn is_safe_feed_url(url_str: &str, allow_insecure_local: bool) -> bool {
    // Require `https://` in production; allow loopback `http://` only when explicitly enabled for tests.
    let trimmed = url_str.trim();
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return !rest.is_empty() && !rest.starts_with('/');
    }
    if allow_insecure_local {
        if let Some(rest) = trimmed.strip_prefix("http://") {
            let host_port = rest.split('/').next().unwrap_or("");
            let host = host_port.split(':').next().unwrap_or("");
            return host == "127.0.0.1" || host == "localhost";
        }
    }
    false
}

/// Resolves the full URL to `<channel>.json` from a base URL or explicit `.json` URL.
///
/// Returns an `Err` if `base_url` is empty or resolves to a non-HTTPS URL when
/// `allow_insecure_local` is `false`.
pub fn resolve_channel_feed_url(
    base_url: &str,
    channel: Option<&str>,
    allow_insecure_local: bool,
) -> Result<String, String> {
    let clean_channel = normalize_channel(channel);
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("Update feed URL is not configured.".to_string());
    }
    // Preserve explicit `.json` URLs; otherwise append `/<channel>.json` to the base URL.
    let resolved = if trimmed.ends_with(".json") {
        trimmed.to_string()
    } else {
        format!(
            "{}/{}.json",
            trimmed.trim_end_matches('/'),
            clean_channel
        )
    };
    if !is_safe_feed_url(&resolved, allow_insecure_local) {
        return Err(format!("Refusing unsafe update feed URL: {}", resolved));
    }
    Ok(resolved)
}

/// Serializes a [`serde_json::Value`] into deterministic canonical JSON with
/// lexicographically sorted object keys and no whitespace.
///
/// Used by [`feed_signing_payload`] so the signed byte representation of a release
/// feed is independent of key insertion order or pretty-printing whitespace.
pub fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
        }
        Value::Array(items) => {
            // Canonicalize array elements in order with no spaces around commas.
            let parts: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        Value::Object(map) => {
            // Sort object keys explicitly to guarantee lexicographical UTF-8 order.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .filter_map(|k| {
                    let v = map.get(k)?;
                    let k_json = serde_json::to_string(k).ok()?;
                    Some(format!("{}:{}", k_json, canonical_json(v)))
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

/// Strips the `signature` field from a feed JSON object and returns the canonical
/// UTF-8 byte payload that is signed by the release key.
pub fn feed_signing_payload(feed: &Value) -> Vec<u8> {
    // Copy all top-level keys except `signature` and serialize via `canonical_json`.
    let Some(obj) = feed.as_object() else {
        return Vec::new();
    };
    let mut unsigned = Map::new();
    for (k, v) in obj {
        if k != "signature" {
            unsigned.insert(k.clone(), v.clone());
        }
    }
    canonical_json(&Value::Object(unsigned)).into_bytes()
}

/// Extracts the 32-byte raw Ed25519 public key from a PEM-encoded or raw base64
/// SubjectPublicKeyInfo (`MCowBQYDK2VwAyEA...`) string.
fn parse_ed25519_public_key_pem(pem: &str) -> Result<VerifyingKey, String> {
    // Strip optional `-----BEGIN PUBLIC KEY-----` / `-----END PUBLIC KEY-----` armor lines.
    let body: String = pem
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("-----"))
        .collect();
    let der = BASE64_STANDARD
        .decode(body.as_bytes())
        .map_err(|e| format!("Invalid base64 in public key: {}", e))?;

    // Standard RFC 8410 Ed25519 SubjectPublicKeyInfo DER is 44 bytes:
    // 12-byte ASN.1 header `30 2a 30 05 06 03 2b 65 70 03 21 00` followed by 32 key bytes.
    let key_bytes: &[u8; 32] = if der.len() == 44
        && der[..12]
            == [
                0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
            ]
    {
        der[12..44].try_into().unwrap()
    } else if der.len() == 32 {
        der[..32].try_into().unwrap()
    } else {
        return Err(format!("Unsupported Ed25519 SPKI DER length: {}", der.len()));
    };

    VerifyingKey::from_bytes(key_bytes)
        .map_err(|e| format!("Invalid Ed25519 public key bytes: {}", e))
}

/// Verifies the Ed25519 signature of a feed JSON value against `public_keys`
/// (defaulting to [`default_update_public_keys`] when `None`).
pub fn verify_feed_signature(
    feed: &Value,
    public_keys: Option<&BTreeMap<String, String>>,
) -> Result<bool, String> {
    // Fall back to the pinned release public keys when no custom key ring is supplied.
    let default_keys;
    let keys = match public_keys {
        Some(k) => k,
        None => {
            default_keys = default_update_public_keys();
            &default_keys
        }
    };

    // Extract and validate `signature.algorithm`, `signature.keyId`, and `signature.value`.
    let sig_obj = feed
        .get("signature")
        .and_then(|v| v.as_object())
        .ok_or_else(|| "Update feed is missing an Ed25519 signature.".to_string())?;

    let alg = sig_obj
        .get("algorithm")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if alg != "ed25519" {
        return Err("Update feed is missing an Ed25519 signature.".to_string());
    }
    let key_id = sig_obj
        .get("keyId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Update feed signature is missing a keyId.".to_string())?;
    let value_b64 = sig_obj
        .get("value")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Update feed signature is missing a value.".to_string())?;

    let pem = keys
        .get(key_id)
        .ok_or_else(|| format!("Update feed is signed by untrusted keyId: {}", key_id))?;

    // Decode the 64-byte Ed25519 signature and verify it against `feed_signing_payload(feed)`.
    let verifying_key = parse_ed25519_public_key_pem(pem)?;
    let sig_bytes = BASE64_STANDARD
        .decode(value_b64.as_bytes())
        .map_err(|_| "Update feed signature verification failed.".to_string())?;
    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|_| "Update feed signature verification failed.".to_string())?;

    let payload = feed_signing_payload(feed);
    verifying_key
        .verify(&payload, &signature)
        .map_err(|_| "Update feed signature verification failed.".to_string())?;

    Ok(true)
}

/// Payload archive artifact metadata inside a feed release entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadArtifact {
    /// Download URL (`https://...`).
    pub url: String,
    /// Compressed archive byte size.
    pub size: u64,
    /// Lowercase hex SHA-256 of the archive file.
    pub sha256: String,
    /// Lowercase hex SHA-256 of the unpacked payload tree (`hash_payload_tree`).
    pub tree_sha256: String,
    /// Minimum shell/binary version compatible with this payload.
    pub min_shell_version: String,
    /// Archive format (defaults to `"zip"`).
    #[serde(default = "default_zip_format")]
    pub format: String,
}

/// Default archive format (`"zip"`) for [`PayloadArtifact::format`].
fn default_zip_format() -> String {
    "zip".to_string()
}

/// Full native installer artifact metadata inside a feed release entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallerArtifact {
    /// Download URL (`https://...`).
    pub url: String,
    /// Installer file byte size.
    pub size: u64,
    /// Lowercase hex SHA-256 of the installer file.
    pub sha256: String,
    /// Package format (`"appimage"`, `"nsis"`, `"dmg"`, `"zip"`, `"tar.gz"`).
    pub kind: String,
    /// Optional filename override.
    #[serde(default)]
    pub file_name: Option<String>,
}

/// Validated release entry inside an update feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseEntry {
    /// Semantic version string.
    pub version: String,
    /// Release channel (`"stable"` or `"nightly"`).
    pub channel: String,
    /// Optional publication timestamp.
    pub published_at: Option<String>,
    /// Optional Markdown/plain-text release notes.
    pub notes: String,
    /// Minimum shell version required by this release.
    pub min_shell_version: Option<String>,
    /// Optional self-contained payload artifact for fast in-place updates.
    pub payload: Option<PayloadArtifact>,
    /// Platform-specific full installer artifacts keyed by [`platform_arch_key`].
    pub installers: BTreeMap<String, InstallerArtifact>,
}

/// Validated update feed document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedFeed {
    /// Feed format identifier (`"battlefront-eternal-update-feed"`).
    pub format: String,
    /// Feed schema version (`1`).
    pub schema: u32,
    /// Feed channel (`"stable"` or `"nightly"`).
    pub channel: String,
    /// Feed generation timestamp.
    pub generated_at: Option<String>,
    /// Validated release entries.
    pub releases: Vec<ReleaseEntry>,
}

/// Checks that `s` is a 64-character lowercase hexadecimal SHA-256 digest.
fn is_sha256_hex(s: &str) -> bool {
    // Require exactly 64 lowercase ASCII hex digits (`0..=9` or `a..=f`).
    s.len() == 64
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Validates a `payload` object inside a release entry.
///
/// Checks `url`, `size > 0`, `sha256`, `treeSha256`, and `minShellVersion`.
fn validate_payload_artifact(
    payload: &Value,
    allow_insecure_local: bool,
) -> Result<PayloadArtifact, String> {
    // Validate download URL, positive archive size, archive SHA-256, tree SHA-256, and minShellVersion.
    let obj = payload
        .as_object()
        .ok_or_else(|| "Release payload must be an object.".to_string())?;
    let url = obj.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if !is_safe_feed_url(url, allow_insecure_local) {
        return Err(format!("Unsafe payload URL in update feed: {}", url));
    }
    let size = obj
        .get("size")
        .and_then(|v| v.as_u64())
        .filter(|&n| n > 0)
        .ok_or_else(|| "Release payload size must be a positive integer.".to_string())?;
    let sha256 = obj.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
    if !is_sha256_hex(sha256) {
        return Err("Release payload sha256 must be a 64-character hex digest.".to_string());
    }
    let tree_sha256 = obj.get("treeSha256").and_then(|v| v.as_str()).unwrap_or("");
    if !is_sha256_hex(tree_sha256) {
        return Err("Release payload treeSha256 must be a 64-character hex digest.".to_string());
    }
    let min_shell = obj
        .get("minShellVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if parse_version(min_shell).is_none() {
        return Err("Release payload minShellVersion must be a valid version.".to_string());
    }
    let format = obj
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("zip")
        .to_string();

    Ok(PayloadArtifact {
        url: url.to_string(),
        size,
        sha256: sha256.to_string(),
        tree_sha256: tree_sha256.to_string(),
        min_shell_version: min_shell.to_string(),
        format,
    })
}

/// Validates a platform `installer` object inside a release entry.
///
/// Checks `url`, `size > 0`, `sha256`, `kind`, and optional `fileName`.
fn validate_installer_artifact(
    installer: &Value,
    allow_insecure_local: bool,
) -> Result<InstallerArtifact, String> {
    // Validate installer download URL, positive byte size, SHA-256 digest, and package kind.
    let obj = installer
        .as_object()
        .ok_or_else(|| "Installer artifact must be an object.".to_string())?;
    let url = obj.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if !is_safe_feed_url(url, allow_insecure_local) {
        return Err(format!("Unsafe installer URL in update feed: {}", url));
    }
    let size = obj
        .get("size")
        .and_then(|v| v.as_u64())
        .filter(|&n| n > 0)
        .ok_or_else(|| "Installer size must be a positive integer.".to_string())?;
    let sha256 = obj.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
    if !is_sha256_hex(sha256) {
        return Err("Installer sha256 must be a 64-character hex digest.".to_string());
    }
    let kind = obj
        .get("kind")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Installer artifact is missing kind.".to_string())?
        .to_string();
    let file_name = obj
        .get("fileName")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Ok(InstallerArtifact {
        url: url.to_string(),
        size,
        sha256: sha256.to_string(),
        kind,
        file_name,
    })
}

/// Validates a single release object inside `feed.releases`.
fn validate_release_entry(
    entry: &Value,
    default_channel: &str,
    allow_insecure_local: bool,
) -> Result<ReleaseEntry, String> {
    // Require a valid SemVer `version` and validate optional `payload` and `installers` blocks.
    let obj = entry
        .as_object()
        .ok_or_else(|| "Release entry in update feed is not an object.".to_string())?;
    let version = obj.get("version").and_then(|v| v.as_str()).unwrap_or("");
    if parse_version(version).is_none() {
        return Err(format!(
            "Invalid release version in update feed: {}",
            version
        ));
    }
    let channel = normalize_channel(
        obj.get("channel")
            .and_then(|v| v.as_str())
            .or(Some(default_channel)),
    )
    .to_string();

    let payload = match obj.get("payload").filter(|v| !v.is_null()) {
        Some(p) => Some(validate_payload_artifact(p, allow_insecure_local)?),
        None => None,
    };

    // Validate each non-null platform installer entry in `installers`.
    let mut installers = BTreeMap::new();
    if let Some(inst_map) = obj.get("installers").and_then(|v| v.as_object()) {
        for (target, val) in inst_map {
            if !val.is_null() {
                installers.insert(
                    target.clone(),
                    validate_installer_artifact(val, allow_insecure_local)?,
                );
            }
        }
    }

    let min_shell_version = obj
        .get("minShellVersion")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| payload.as_ref().map(|p| p.min_shell_version.clone()));

    Ok(ReleaseEntry {
        version: version.to_string(),
        channel,
        published_at: obj
            .get("publishedAt")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        notes: obj
            .get("notes")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        min_shell_version,
        payload,
        installers,
    })
}

/// Options for [`parse_and_verify_feed`].
#[derive(Debug, Clone, Default)]
pub struct VerifyFeedOptions {
    /// Expected channel (`"stable"` or `"nightly"`).
    pub expected_channel: Option<String>,
    /// Custom Ed25519 public key map (`keyId -> PEM`); uses [`default_update_public_keys`] when `None`.
    pub public_keys: Option<BTreeMap<String, String>>,
    /// Allow `http://127.0.0.1` and `http://localhost` URLs for local tests.
    pub allow_insecure_local: bool,
    /// Skip Ed25519 signature verification (only for unit tests).
    pub allow_unsigned: bool,
}

/// Parses a raw JSON update feed (from string or [`Value`]), verifies its Ed25519
/// signature, and validates every release entry and artifact URL/digest.
///
/// Rejects feeds with unknown `format`, unsupported `schema`, mismatched `channel`,
/// or invalid Ed25519 signatures.
pub fn parse_and_verify_feed(
    raw: &Value,
    opts: &VerifyFeedOptions,
) -> Result<ValidatedFeed, String> {
    // Allow `raw` to be passed either as an unparsed JSON string or a pre-parsed `serde_json::Value`.
    let parsed_holder: Value;
    let feed = if let Some(s) = raw.as_str() {
        parsed_holder =
            serde_json::from_str(s).map_err(|_| "Update feed is not valid JSON.".to_string())?;
        &parsed_holder
    } else {
        raw
    };

    // Verify top-level feed format, schema version, and channel before checking the signature.
    let obj = feed
        .as_object()
        .ok_or_else(|| "Update feed must be a JSON object.".to_string())?;
    if obj.get("format").and_then(|v| v.as_str()) != Some(FEED_FORMAT) {
        return Err(format!(
            "Unsupported update feed format: {}",
            obj.get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
        ));
    }
    if obj.get("schema").and_then(|v| v.as_u64()) != Some(FEED_SCHEMA as u64) {
        return Err(format!(
            "Unsupported update feed schema: {}",
            obj.get("schema")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
        ));
    }
    let ch = obj.get("channel").and_then(|v| v.as_str()).unwrap_or("");
    if !UPDATE_CHANNELS.contains(&ch) {
        return Err(format!("Unsupported update feed channel: {}", ch));
    }
    if let Some(exp) = opts.expected_channel.as_deref() {
        if ch != normalize_channel(Some(exp)) {
            return Err(format!(
                "Update feed channel mismatch: expected {}, got {}",
                normalize_channel(Some(exp)),
                ch
            ));
        }
    }

    // Verify the Ed25519 signature unless `allow_unsigned` is explicitly set in tests.
    if !opts.allow_unsigned {
        verify_feed_signature(feed, opts.public_keys.as_ref())?;
    }

    // Validate each release entry in the `releases` array.
    let releases_arr = obj
        .get("releases")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Update feed releases must be an array.".to_string())?;

    let mut releases = Vec::with_capacity(releases_arr.len());
    for entry in releases_arr {
        releases.push(validate_release_entry(entry, ch, opts.allow_insecure_local)?);
    }

    Ok(ValidatedFeed {
        format: FEED_FORMAT.to_string(),
        schema: FEED_SCHEMA,
        channel: ch.to_string(),
        generated_at: obj
            .get("generatedAt")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        releases,
    })
}

/// Chosen update artifact (either a fast self-contained `"payload"` or a `"full"` installer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SelectedUpdateArtifact {
    /// Fast in-place payload archive update.
    Payload {
        /// Selected payload artifact.
        artifact: PayloadArtifact,
    },
    /// Full native shell installer update.
    Full {
        /// Selected installer artifact.
        artifact: InstallerArtifact,
    },
}

/// Result returned by [`select_update_target`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTargetSelection {
    /// Whether a newer update is available.
    pub available: bool,
    /// Update mechanism: `"none"`, `"payload"`, `"full"`, or `"unavailable"`.
    pub kind: String,
    /// Effective running version compared against.
    pub current_version: String,
    /// Running native binary shell version.
    pub shell_version: String,
    /// Target version when `available` is true (`None` otherwise).
    pub target_version: Option<String>,
    /// Release channel (`"stable"` or `"nightly"`).
    pub channel: String,
    /// Release publication timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    /// Release notes.
    #[serde(default)]
    pub notes: String,
    /// Selected payload or installer artifact when `available` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Value>,
    /// Optional fallback reason (e.g. `"shell-upgrade-required"` or `"no-artifact-for-linux-x64"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Options for [`select_update_target`].
#[derive(Debug, Clone)]
pub struct SelectTargetOptions {
    /// Effective version currently running (payload version if active, else shell version).
    pub current_version: String,
    /// Base native shell binary version.
    pub shell_version: String,
    /// Target platform (`"linux"`, `"win32"`, `"darwin"`).
    pub platform: String,
    /// Target CPU architecture (`"x64"`, `"arm64"`).
    pub arch: String,
    /// Whether the app is allowed to use fast payload updates (`true` when packaged).
    pub allow_payload: bool,
}

/// Selects the highest-versioned release in `feed` and decides whether to apply
/// a fast `"payload"` update (when `shell_version >= minShellVersion`) or a `"full"`
/// native installer upgrade.
///
/// Decision rules:
/// - If `feed.releases` is empty or the highest release `<= current_version`, returns `kind: "none"`.
/// - If `allow_payload` is `true`, a `payload` artifact exists, and `shell_version >= minShellVersion`,
///   returns `kind: "payload"`.
/// - Otherwise looks up `installers["<platform>-<arch>"]` and returns `kind: "full"`
///   (or `kind: "unavailable"` when no installer exists for the host platform/arch).
pub fn select_update_target(
    feed: &ValidatedFeed,
    opts: &SelectTargetOptions,
) -> Result<UpdateTargetSelection, String> {
    // Resolve effective current and shell versions and validate both as SemVer/Debian strings.
    let effective_current = if !opts.current_version.is_empty() {
        opts.current_version.clone()
    } else {
        opts.shell_version.clone()
    };
    let effective_shell = if !opts.shell_version.is_empty() {
        opts.shell_version.clone()
    } else {
        effective_current.clone()
    };

    if parse_version(&effective_current).is_none() {
        return Err(format!(
            "Invalid current version: {}",
            effective_current
        ));
    }
    if parse_version(&effective_shell).is_none() {
        return Err(format!("Invalid shell version: {}", effective_shell));
    }

    // Sort candidate releases in descending SemVer order so the newest release is first.
    let mut candidates = feed.releases.clone();
    candidates.sort_by(|a, b| match compare_versions(&b.version, &a.version) {
        Some(1) => std::cmp::Ordering::Greater,
        Some(-1) => std::cmp::Ordering::Less,
        _ => std::cmp::Ordering::Equal,
    });

    let Some(latest) = candidates.into_iter().next() else {
        return Ok(UpdateTargetSelection {
            available: false,
            kind: "none".to_string(),
            current_version: effective_current,
            shell_version: effective_shell,
            target_version: None,
            channel: feed.channel.clone(),
            published_at: None,
            notes: String::new(),
            artifact: None,
            reason: None,
        });
    };

    // If the newest release in the feed is not newer than `effective_current`, no update is needed.
    if !is_newer_version(&latest.version, &effective_current) {
        return Ok(UpdateTargetSelection {
            available: false,
            kind: "none".to_string(),
            current_version: effective_current,
            shell_version: effective_shell,
            target_version: Some(latest.version),
            channel: feed.channel.clone(),
            published_at: latest.published_at,
            notes: latest.notes,
            artifact: None,
            reason: None,
        });
    }

    // Prefer a fast in-place payload update when `effective_shell >= minShellVersion`.
    let target_key = platform_arch_key(&opts.platform, &opts.arch);
    let min_shell = latest
        .payload
        .as_ref()
        .map(|p| p.min_shell_version.as_str())
        .or(latest.min_shell_version.as_deref());
    let shell_compatible = min_shell
        .map(|ms| compare_versions(&effective_shell, ms).unwrap_or(-1) >= 0)
        .unwrap_or(false);

    if opts.allow_payload && shell_compatible {
        if let Some(payload) = latest.payload {
            return Ok(UpdateTargetSelection {
            available: true,
            kind: "payload".to_string(),
            current_version: effective_current,
            shell_version: effective_shell,
            target_version: Some(latest.version),
            channel: latest.channel,
            published_at: latest.published_at,
            notes: latest.notes,
            artifact: serde_json::to_value(payload).ok(),
            reason: None,
        });
        }
    }

    // Otherwise fall back to the full native installer for `<platform>-<arch>`.
    if let Some(installer) = latest.installers.get(&target_key) {
        let reason = if latest.payload.is_some() && !shell_compatible {
            Some("shell-upgrade-required".to_string())
        } else {
            Some("full-installer".to_string())
        };
        return Ok(UpdateTargetSelection {
            available: true,
            kind: "full".to_string(),
            current_version: effective_current,
            shell_version: effective_shell,
            target_version: Some(latest.version),
            channel: latest.channel,
            published_at: latest.published_at,
            notes: latest.notes,
            artifact: serde_json::to_value(installer).ok(),
            reason,
        });
    }

    Ok(UpdateTargetSelection {
        available: false,
        kind: "unavailable".to_string(),
        current_version: effective_current,
        shell_version: effective_shell,
        target_version: Some(latest.version),
        channel: latest.channel,
        published_at: latest.published_at,
        notes: latest.notes,
        artifact: None,
        reason: Some(format!("no-artifact-for-{}", target_key)),
    })
}
