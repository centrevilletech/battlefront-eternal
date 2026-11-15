//! Signed master ban-list verification and caching (`src/bans.js`).

use crate::identity::{verify, write_mode_600};
use serde::{Deserialize, Serialize};
use serde_json::{value::RawValue, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Abstraction over `MasterClient.request(path)` in `src/master-client.js` so `BanCache` can be
/// exercised both against the live HTTPS master client and deterministic unit-test mocks.
pub trait BanRequester: Send + Sync {
    /// Performs an authenticated `GET` request to `path` on the master server and returns the
    /// response body exactly as received: the ban list's signature covers the payload's own bytes.
    fn request_text(&self, path: &str) -> Result<String, String>;
}

/// The signed document of `/v1/bans`, with the payload kept as the bytes the master sent.
#[derive(Deserialize)]
struct SignedBans<'a> {
    #[serde(borrow)]
    payload: &'a RawValue,
    signature: String,
    #[serde(rename = "publicKey", default)]
    public_key: String,
}

/// A single global ban entry inside `document.payload.bans` (`src/bans.js`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BanItem {
    /// 64-character lowercase hex SHA-256 SPKI identity of the banned player (`b.player`).
    pub player: String,
    /// Unix timestamp in milliseconds when this player's ban expires (`b.expires`).
    pub expires: u64,
    /// Optional human-readable ban reason recorded by the master directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Verified cache of the master server's signed global ban list (`export class BanCache` in `src/bans.js`).
///
/// Mirrors the fields of `BanCache` in `src/bans.js`:
/// `this.master`, `this.file`, `this.bans`, `this.expires`, and `this.key`.
pub struct BanCache<R: BanRequester> {
    master: Arc<R>,
    file: PathBuf,
    bans: Vec<BanItem>,
    expires: u64,
    key: Option<String>,
}

impl<R: BanRequester> BanCache<R> {
    /// Constructs a `BanCache` (`constructor(master, file)` in `src/bans.js`).
    ///
    /// If `file` (`master-ban-key`) already exists on disk, reads and trims the pinned base64 SPKI
    /// public key (`existsSync(file) ? readFileSync(file, 'utf8').trim() : null`).
    pub fn new(master: Arc<R>, file: PathBuf) -> Self {
        let key = fs::read_to_string(&file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        Self {
            master,
            file,
            bans: Vec::new(),
            expires: 0,
            key,
        }
    }

    /// Returns the currently pinned master ban-signing SPKI public key (base64), if any.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Returns the Unix timestamp in milliseconds at which the cached ban list expires (`this.expires`).
    pub fn expires(&self) -> u64 {
        self.expires
    }

    /// Fetches `/v1/bans` (and `/v1/info` on first run), verifies the Ed25519 signature over
    /// the payload's received bytes, validates lifetime bounds, and persists `master-ban-key`
    /// (`async refresh()` in `src/bans.js`).
    pub fn refresh(&mut self) -> Result<(), String> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.refresh_at(now_ms)
    }

    /// Deterministic implementation of `BanCache.refresh()` in `src/bans.js` parameterized by
    /// `now_ms` (`Date.now()`).
    pub fn refresh_at(&mut self, now_ms: u64) -> Result<(), String> {
        let text = self.master.request_text("/v1/bans")?;
        let document: SignedBans =
            serde_json::from_str(&text).map_err(|_| "Invalid ban list signature".to_string())?;
        // `if (!this.key) { const info = await this.master.request('/v1/info'); this.key = info.publicKey; }`
        if self.key.is_none() {
            let info: Value = serde_json::from_str(&self.master.request_text("/v1/info")?)
                .map_err(|e| e.to_string())?;
            let info_key = info
                .get("publicKey")
                .and_then(Value::as_str)
                .ok_or_else(|| "Master ban signing key missing".to_string())?;
            self.key = Some(info_key.to_string());
        }
        // `if (document.publicKey !== this.key) throw Error('Master ban signing key changed');`
        let doc_key = document.public_key.as_str();
        if Some(doc_key) != self.key.as_deref() {
            return Err("Master ban signing key changed".to_string());
        }
        // The master signs JSON.stringify(payload) and sends those same bytes. Verify them as
        // received: serializing the parsed payload again would reorder its keys.
        if !verify(doc_key, document.payload.get(), &document.signature) {
            return Err("Invalid ban list signature".to_string());
        }
        let payload: Value = serde_json::from_str(document.payload.get())
            .map_err(|_| "Invalid ban list signature".to_string())?;
        // Validate list size (`<= 10000`), issue skew (`<= now + 60000`), and expiry window
        // (`now <= expires <= now + 600000`) (`BanCache.refresh` in `src/bans.js`).
        let bans_arr = payload
            .get("bans")
            .and_then(Value::as_array)
            .ok_or_else(|| "Invalid ban list lifetime".to_string())?;
        let issued = payload
            .get("issued")
            .and_then(Value::as_u64)
            .ok_or_else(|| "Invalid ban list lifetime".to_string())?;
        let expires = payload
            .get("expires")
            .and_then(Value::as_u64)
            .ok_or_else(|| "Invalid ban list lifetime".to_string())?;
        if bans_arr.len() > 10_000
            || issued > now_ms + 60_000
            || expires < now_ms
            || expires > now_ms + 600_000
        {
            return Err("Invalid ban list lifetime".to_string());
        }
        // Persist pinned key on first successful verification (`writeFileSync(this.file, this.key + '\n', {mode: 0o600, flag: 'wx'})`).
        if !self.file.exists() {
            let content = format!("{doc_key}\n");
            let _ = write_mode_600(&self.file, content.as_bytes());
        }
        let mut parsed_bans = Vec::with_capacity(bans_arr.len());
        for entry in bans_arr {
            if let (Some(player), Some(exp)) = (
                entry.get("player").and_then(Value::as_str),
                entry.get("expires").and_then(Value::as_u64),
            ) {
                parsed_bans.push(BanItem {
                    player: player.to_string(),
                    expires: exp,
                    reason: entry
                        .get("reason")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });
            }
        }
        self.bans = parsed_bans;
        self.expires = expires;
        Ok(())
    }

    /// Returns `true` if `id` is currently banned in a non-expired ban list (`contains(id)` in `src/bans.js`).
    pub fn contains(&self, id: &str) -> bool {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.contains_at(id, now_ms)
    }

    /// Deterministic check for `BanCache.contains(id)` in `src/bans.js` at `now_ms`:
    /// `this.expires > now && this.bans.some(b => b.player === id && b.expires > now)`.
    pub fn contains_at(&self, id: &str, now_ms: u64) -> bool {
        self.expires > now_ms
            && self
                .bans
                .iter()
                .any(|b| b.player == id && b.expires > now_ms)
    }
}
