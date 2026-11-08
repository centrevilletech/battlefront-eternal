//! Persistent Ed25519 server identity (`src/identity.js`).
//!
//! Every dedicated server has a long-lived Ed25519 keypair stored in `identity.json`
//! (`{"publicKey": "<SPKI DER base64>", "privateKey": "<PKCS#8 PEM>"}`) inside `BFE_STATE_DIR`
//! with mode `0600` (`loadIdentity` in `src/identity.js`).
//!
//! From `keys.publicKey` (the 60-character base64-encoded 44-byte Ed25519 SPKI DER), `identity.js`
//! derives two distinct identifiers:
//! 1. `keyId(keys.publicKey)` (`ServerIdentity.id`): the 64-character lowercase hex SHA-256 of the
//!    UTF-8 base64 string `keys.publicKey` (`createHash("sha256").update(key).digest("hex")`).
//!    This is the server's canonical 64-hex identity in `/info`, LAN discovery, and the master directory.
//! 2. `fingerprint(keys)` (`ServerIdentity.fingerprint`): `"sha-256 XX:XX:..."`, the uppercase
//!    colon-hex SHA-256 of the 44 raw SPKI DER bytes (`Buffer.from(keys.publicKey, "base64")`),
//!    which matches the transport key proved during the `BFE-UDP/1` Noise-style handshake.
//!
//! This module also ports:
//! - `headers(keys, method, path, body)`: signs `${method}\n${path}\n${time}\n${nonce}\n${sha256_hex(body)}`
//!   and attaches `"x-bfe-notice": "2026-10-11"`, `"x-bfe-key"`, `"x-bfe-time"`, `"x-bfe-nonce"`,
//!   and `"x-bfe-signature"` for master HTTP requests (`src/master-client.js`).
//! - `verifyProof(p, transcript)` / `verify`: verifies an Ed25519 base64 signature against a
//!   60-char base64 SPKI public key (`src/identity.js` and `src/bans.js`).

use bfe_transport::crypto::{verify_spki_base64, Identity};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// On-disk JSON representation of `identity.json` (`loadIdentity` in `src/identity.js`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredIdentity {
    public_key: String,
    private_key: String,
}

/// Loaded Ed25519 server identity and derived identifiers (`src/identity.js`).
#[derive(Clone)]
pub struct ServerIdentity {
    /// Underlying transport `Identity` used by `bfe_transport::server::ServerEndpoint` for the
    /// `BFE-UDP/1` handshake transcript signature.
    pub inner: Identity,
    /// 64-character lowercase hex SHA-256 of `public_key` (`export const keyId` in `src/identity.js`).
    pub id: String,
    /// `"sha-256 XX:XX:..."` colon-hex SHA-256 of the decoded SPKI DER bytes (`export const fingerprint` in `src/identity.js`).
    pub fingerprint: String,
    /// 60-character base64-encoded 44-byte SPKI DER public key (`keys.publicKey`).
    pub public_key: String,
    /// PKCS#8 PEM-encoded Ed25519 private key (`keys.privateKey`).
    pub private_key: String,
}

impl ServerIdentity {
    /// Wraps a `bfe_transport::crypto::Identity` and precomputes `id`, `fingerprint`,
    /// `public_key`, and `private_key` matching `src/identity.js`.
    pub fn from_identity(inner: Identity) -> Self {
        let id = inner.id_hex();
        let fingerprint = inner.fingerprint();
        let public_key = inner.public_key_base64();
        let private_key = inner.private_key_pem();
        Self {
            inner,
            id,
            fingerprint,
            public_key,
            private_key,
        }
    }

    /// Generates a fresh Ed25519 identity keypair (`generateKeyPairSync("ed25519")` in `src/identity.js`).
    pub fn generate() -> Result<Self, String> {
        let inner = Identity::generate().map_err(|e| e.to_string())?;
        Ok(Self::from_identity(inner))
    }

    /// Parses an existing SPKI base64 public key and PKCS#8 PEM private key pair from `identity.json`.
    pub fn from_keypair(public_key_b64: &str, private_key_pem: &str) -> Result<Self, String> {
        let inner =
            Identity::from_keypair(public_key_b64, private_key_pem).map_err(|e| e.to_string())?;
        Ok(Self::from_identity(inner))
    }

    /// Loads or creates `identity.json` in `dir` (`loadIdentity(join(directory, 'identity.json'))` in `src/main.js`).
    pub fn load(dir: &Path) -> Result<Self, String> {
        load_identity(&dir.join("identity.json"))
    }

    /// Signs `text` with the server's Ed25519 private key and returns base64 (`proof` in `src/identity.js`).
    pub fn sign(&self, text: &str) -> String {
        self.inner.sign_base64(text.as_bytes())
    }

    /// Generates master request authentication headers (`export function headers(keys, method, path, body = "")` in `src/identity.js`).
    ///
    /// Returns:
    /// - `x-bfe-notice: 2026-10-11`
    /// - `x-bfe-key: <SPKI base64>`
    /// - `x-bfe-time: <unix seconds>`
    /// - `x-bfe-nonce: <32-char random hex>`
    /// - `x-bfe-signature: <base64 Ed25519 signature over "${method}\n${path}\n${time}\n${nonce}\n${sha256(body)}">`
    pub fn headers(
        &self,
        method: &str,
        path: &str,
        body: &str,
    ) -> Result<Vec<(String, String)>, String> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.inner
            .headers(method, path, body.as_bytes(), now_secs)
            .map_err(|e| e.to_string())
    }
}

/// Loads `identity.json` at `file_path` if present, or generates a new Ed25519 keypair and writes
/// it with mode `0600` (`export function loadIdentity(file)` in `src/identity.js`).
pub fn load_identity(file_path: &Path) -> Result<ServerIdentity, String> {
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create state dir: {e}"))?;
    }
    let stored = if let Ok(raw) = fs::read_to_string(file_path) {
        serde_json::from_str::<StoredIdentity>(&raw)
            .map_err(|e| format!("invalid identity.json: {e}"))?
    } else {
        let generated = ServerIdentity::generate()?;
        let stored = StoredIdentity {
            public_key: generated.public_key.clone(),
            private_key: generated.private_key.clone(),
        };
        let json_bytes = serde_json::to_vec(&stored)
            .map_err(|e| format!("failed to serialize identity.json: {e}"))?;
        write_mode_600(file_path, &json_bytes)?;
        stored
    };
    ServerIdentity::from_keypair(&stored.public_key, &stored.private_key)
}

/// Validates that `value` is a 64-character lowercase hexadecimal SHA-256 identity (`isId` in `src/operator.js`).
pub fn is_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Verifies a base64 Ed25519 signature over `text` against a 60-character base64 SPKI public key
/// (`export function verifyProof(p, transcript)` in `src/identity.js` and `BanCache.refresh` in `src/bans.js`).
pub fn verify(public_key_b64: &str, text: &str, signature_b64: &str) -> bool {
    verify_spki_base64(public_key_b64, text.as_bytes(), signature_b64)
}

/// Writes `bytes` to `path` with owner-only `0600` permissions (`mode: 0o600` in `src/identity.js`,
/// `src/bans.js`, and `src/operator.js`).
pub(crate) fn write_mode_600(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .map_err(|e| format!("failed to open {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
