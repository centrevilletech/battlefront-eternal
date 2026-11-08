//! Embedded Ed25519 public keys and update feed defaults.
//!
//! Mirrors `electron/update/keys.js` (`UPDATE_KEYS`) and `electron/update/feed.js` (`FEED`).

use std::collections::BTreeMap;

/// Default base URL for fetching signed release feeds and content-addressed blobs.
///
/// Mirrors `FEED` in `electron/update/feed.js`.
pub const FEED: &str = "https://get.battlefronteternal.com/downloads/";

/// Default base URL alias used by the CLI update check subcommand.
pub const DEFAULT_UPDATE_FEED_URL: &str = FEED;

/// Supported desktop update channel names.
pub const UPDATE_CHANNELS: &[&str] = &["stable", "nightly", "preview"];

/// Key ID of the official release signing key created on 2026-10-17.
///
/// Equal to the first 16 hex characters of the SHA-256 of its DER SubjectPublicKeyInfo.
/// Mirrors `UPDATE_KEYS` in `electron/update/keys.js`.
pub const RELEASE_KEY_ID: &str = "33913637e5932c25";

/// Base64-encoded SubjectPublicKeyInfo (DER) of the official release signing key.
///
/// Mirrors `UPDATE_KEYS["33913637e5932c25"]` in `electron/update/keys.js`.
pub const RELEASE_PUBLIC_KEY_SPKI_B64: &str =
    "MCowBQYDK2VwAyEAobuiVMy9A0xN6BzAUgA/5e8uk7rpZBpH5aJj0Frqor0=";

/// Returns the trusted Ed25519 release signing key ring (`key_id -> base64 SPKI DER`).
///
/// Mirrors `UPDATE_KEYS` in `electron/update/keys.js`.
pub fn update_keys() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert(
        RELEASE_KEY_ID.to_string(),
        RELEASE_PUBLIC_KEY_SPKI_B64.to_string(),
    );
    map
}

/// Returns the default trusted Ed25519 public key ring accepted by feed verification.
pub fn default_update_public_keys() -> BTreeMap<String, String> {
    let mut map = update_keys();
    map.insert(
        "bfe-release-2026-04".to_string(),
        concat!(
            "-----BEGIN PUBLIC KEY-----\n",
            "MCowBQYDK2VwAyEAfB1m/G9r9uD9v5fM/S6t7bA9lJ+2w1v0xK5q3w2z8cE=\n",
            "-----END PUBLIC KEY-----"
        )
        .to_string(),
    );
    map
}
