//! Pinned Ed25519 public keys and default distribution origin for desktop auto-updates.
//!
//! Mirrors the desktop host (`UPDATE_KEYS`) and the desktop host (`FEED`).
//!
//! ## Key Format
//! Channel envelopes (`updates/<channel>.json`) carry `{ key, payload, signature }`
//! where `key` is a 16-character hex key ID (`"33913637e5932c25"`) and `KEYS` maps
//! each trusted key ID to its Ed25519 SubjectPublicKeyInfo public key.

use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use ed25519_dalek::VerifyingKey;
use sha2::{Digest, Sha256};

/// Default HTTPS origin for desktop update channels, manifests, and blobs.
///
/// Mirrors `FEED` in the desktop host.
pub const FEED: &str = "https://get.battlefronteternal.com/downloads/";

/// Alias for [`FEED`].
pub const DEFAULT_FEED_BASE_URL: &str = FEED;

/// Pinned production Ed25519 public key ID (`"33913637e5932c25"`).
pub const PRODUCTION_KEY_ID: &str = "33913637e5932c25";

/// Pinned production Ed25519 public key in SubjectPublicKeyInfo PEM format.
///
/// Mirrors `UPDATE_KEYS["33913637e5932c25"]` in the desktop host.
pub const PRODUCTION_PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAobuiVMy9A0xN6BzAUgA/5e8uk7rpZBpH5aJj0Frqor0=\n-----END PUBLIC KEY-----\n";

/// ASN.1 DER SubjectPublicKeyInfo prefix for a 32-byte Ed25519 public key (RFC 8410).
const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

/// Pinned Ed25519 public keys table (`(key_id, public_key_pem)`).
///
/// Mirrors `UPDATE_KEYS` in the desktop host.
pub const KEYS: &[(&str, &str)] = &[(PRODUCTION_KEY_ID, PRODUCTION_PUBLIC_KEY_PEM)];

/// Returns the default trusted [`KEYS`] table as a `BTreeMap<String, String>`.
pub fn default_keys() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (id, pem) in KEYS {
        map.insert((*id).to_string(), (*pem).to_string());
    }
    map
}

/// Encodes a 32-byte Ed25519 verifying key as 44-byte SubjectPublicKeyInfo DER.
pub fn encode_spki_der(verifying_key: &VerifyingKey) -> [u8; 44] {
    let mut der = [0u8; 44];
    der[..12].copy_from_slice(&ED25519_SPKI_PREFIX);
    der[12..].copy_from_slice(verifying_key.as_bytes());
    der
}

/// Encodes a 32-byte Ed25519 verifying key as a SubjectPublicKeyInfo PEM string
/// matching the format in the desktop host.
pub fn encode_spki_pem(verifying_key: &VerifyingKey) -> String {
    let der = encode_spki_der(verifying_key);
    let b64 = BASE64_STANDARD.encode(der);
    format!("-----BEGIN PUBLIC KEY-----\n{b64}\n-----END PUBLIC KEY-----\n")
}

/// Computes the 16-character lowercase hex key ID (`sha256(spki_der)[:16]`) matching
/// `keyId` in the desktop host.
pub fn key_id_from_verifying_key(verifying_key: &VerifyingKey) -> String {
    let der = encode_spki_der(verifying_key);
    let mut hasher = Sha256::new();
    hasher.update(der);
    let digest = hasher.finalize();
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Alias for [`key_id_from_verifying_key`].
pub fn sha256_key_id_for_test(verifying_key: &VerifyingKey) -> String {
    key_id_from_verifying_key(verifying_key)
}

/// Parses an Ed25519 [`VerifyingKey`] from a SubjectPublicKeyInfo PEM string,
/// raw 44-byte SPKI Base64, raw 32-byte Base64, or 64-character hex string.
pub fn parse_verifying_key(encoded: &str) -> Result<VerifyingKey, String> {
    let trimmed = encoded.trim();
    if trimmed.is_empty() {
        return Err("Empty Ed25519 public key".to_string());
    }

    // Strip optional PEM header/footer lines and whitespace.
    let body: String = trimmed
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .flat_map(|line| line.chars().filter(|c| !c.is_whitespace()))
        .collect();

    if body.len() == 64 && body.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut raw = [0u8; 32];
        for (i, chunk) in body.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            let s = std::str::from_utf8(chunk).map_err(|e| e.to_string())?;
            raw[i] = u8::from_str_radix(s, 16).map_err(|e| e.to_string())?;
        }
        return VerifyingKey::from_bytes(&raw)
            .map_err(|e| format!("Invalid Ed25519 public key: {e}"));
    }

    let decoded = BASE64_STANDARD
        .decode(&body)
        .map_err(|e| format!("Invalid base64 in Ed25519 public key: {e}"))?;

    let raw_bytes: &[u8] = if decoded.len() == 44 && decoded.starts_with(&ED25519_SPKI_PREFIX) {
        &decoded[12..]
    } else if decoded.len() == 32 {
        &decoded[..]
    } else {
        return Err(format!(
            "Unsupported Ed25519 public key length: {} bytes",
            decoded.len()
        ));
    };

    let arr: [u8; 32] = raw_bytes
        .try_into()
        .map_err(|_| "Invalid Ed25519 key slice".to_string())?;
    VerifyingKey::from_bytes(&arr).map_err(|e| format!("Invalid Ed25519 public key: {e}"))
}
