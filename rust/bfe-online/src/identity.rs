// Persistent Ed25519 player identity, master HTTP request signing, and online
// notice acknowledgement storage.
//
// Ports `loadIdentity`, `getIdentity`, `signedHeaders`, `readOnlineConsent`,
// and `writeOnlineConsent` from `electron/network.js`, plus `verifySignature`
// and `requireNotice` from `battlefront-eternal-master/src/auth.js` and
// `identity` / `default_identity` from `rust/bfe-client/src/online/info.rs`.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use bfe_transport::crypto::Identity as TransportIdentity;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Online notice version required in the `x-bfe-notice` header by
// `battlefront-eternal-master/src/auth.js` (`NOTICE_VERSION`) and stored by
// `electron/network.js` (`ONLINE_NOTICE_VERSION`).
pub const ONLINE_NOTICE_VERSION: &str = "2026-10-11";

// Fixed ASN.1 SubjectPublicKeyInfo (SPKI) DER prefix for a 32-byte raw Ed25519
// public key (`ED_PREFIX` in `rust/bfe-transport/src/crypto.rs`).
pub const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

// Fixed ASN.1 PKCS#8 v1 DER prefix for a 32-byte raw Ed25519 private seed.
// Allows reading Electron's `player-identity.json` (`privateKey` base64 DER)
// as well as the native 32-byte `identity.key` seed file.
const ED25519_PKCS8_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];

// Resolves the default native identity key path, matching
// `rust/bfe-client/src/online/info.rs::default_identity`.
pub fn default_identity_path() -> Option<PathBuf> {
    default_data_dir().map(|dir| dir.join("identity.key"))
}

// Resolves the default online notice consent path in the user's application
// data directory, matching `electron/network.js::consentPath`.
pub fn default_consent_path() -> Option<PathBuf> {
    default_data_dir().map(|dir| dir.join("online-consent.json"))
}

fn default_data_dir() -> Option<PathBuf> {
    if let Some(dir) = env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir).join("battlefront-eternal"));
    }
    if let Some(dir) = env::var_os("APPDATA").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir).join("battlefront-eternal"));
    }
    let home = env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("battlefront-eternal"),
    )
}

#[derive(Clone)]
pub struct PlayerIdentity {
    seed: [u8; 32],
    signing: SigningKey,
    public_key_b64: String,
    id_hex: String,
}

impl std::fmt::Debug for PlayerIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlayerIdentity")
            .field("id_hex", &self.id_hex)
            .field("public_key_b64", &self.public_key_b64)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedMasterHeaders {
    pub key: String,
    pub time: String,
    pub nonce: String,
    pub signature: String,
    pub notice: &'static str,
}

impl SignedMasterHeaders {
    // Header name/value pairs sent on every master HTTP request, matching
    // `electron/network.js::signedHeaders`.
    pub fn as_pairs(&self) -> [(&'static str, &str); 5] {
        [
            ("x-bfe-key", self.key.as_str()),
            ("x-bfe-time", self.time.as_str()),
            ("x-bfe-nonce", self.nonce.as_str()),
            ("x-bfe-signature", self.signature.as_str()),
            ("x-bfe-notice", self.notice),
        ]
    }
}

#[derive(Deserialize)]
struct StoredJsonIdentity {
    #[serde(rename = "publicKey")]
    public_key: String,
    #[serde(rename = "privateKey")]
    private_key: String,
}

impl PlayerIdentity {
    // Constructs a player identity from a 32-byte Ed25519 secret seed.
    // Ports `generateIdentity` / `loadIdentity` in `electron/network.js`:
    // - `public_key_b64` is standard Base64 of the 44-byte SPKI DER public key.
    // - `id_hex` is lowercase hex SHA-256 of the Base64 `public_key_b64` string
    //   (matching `crypto.createHash("sha256").update(keys.publicKey).digest("hex")`
    //   in both `electron/network.js` and `battlefront-eternal-master/src/auth.js`).
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&seed);
        let verifying: VerifyingKey = signing.verifying_key();
        let mut spki = [0u8; 44];
        spki[..12].copy_from_slice(&ED25519_SPKI_PREFIX);
        spki[12..].copy_from_slice(&verifying.to_bytes());
        let public_key_b64 = BASE64.encode(spki);
        let id_digest = Sha256::digest(public_key_b64.as_bytes());
        let id_hex = hex_lower(&id_digest);
        Self {
            seed,
            signing,
            public_key_b64,
            id_hex,
        }
    }

    // Generates a new random Ed25519 player identity using OS CSPRNG bytes.
    pub fn generate() -> Result<Self, String> {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).map_err(|err| format!("random failed: {err}"))?;
        Ok(Self::from_seed(seed))
    }

    // Loads an existing identity from `path` or creates a new 32-byte seed file
    // when the file does not exist. Supports both the native 32-byte raw seed
    // format (`rust/bfe-client/src/online/info.rs::identity`) and
    // Electron's JSON PKCS#8 format (`electron/network.js::loadIdentity`).
    pub fn load_or_create(path: &Path) -> Result<Self, String> {
        match fs::read(path) {
            Ok(bytes) => Self::from_file_bytes(&bytes, path),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let identity = Self::generate()?;
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)
                            .map_err(|e| format!("{}: {e}", parent.display()))?;
                    }
                }
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                match options.open(path) {
                    Ok(mut file) => {
                        file.write_all(&identity.seed)
                            .map_err(|e| format!("{}: {e}", path.display()))?;
                        Ok(identity)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        let bytes =
                            fs::read(path).map_err(|re| format!("{}: {re}", path.display()))?;
                        Self::from_file_bytes(&bytes, path)
                    }
                    Err(e) => Err(format!("{}: {e}", path.display())),
                }
            }
            Err(err) => Err(format!("{}: {err}", path.display())),
        }
    }

    fn from_file_bytes(bytes: &[u8], path: &Path) -> Result<Self, String> {
        if bytes.len() == 32 {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(bytes);
            return Ok(Self::from_seed(seed));
        }
        if let Ok(stored) = serde_json::from_slice::<StoredJsonIdentity>(bytes) {
            let pkcs8 = BASE64
                .decode(stored.private_key.trim())
                .map_err(|e| format!("{}: invalid base64 privateKey: {e}", path.display()))?;
            if pkcs8.len() == 48 && pkcs8[..16] == ED25519_PKCS8_PREFIX {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&pkcs8[16..48]);
                let id = Self::from_seed(seed);
                if id.public_key_b64 != stored.public_key.trim() {
                    return Err(format!(
                        "{}: publicKey does not match privateKey seed",
                        path.display()
                    ));
                }
                return Ok(id);
            }
        }
        Err(format!(
            "{}: expected 32-byte Ed25519 seed, found {} bytes",
            path.display(),
            bytes.len()
        ))
    }

    pub fn seed(&self) -> &[u8; 32] {
        &self.seed
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    pub fn public_key_spki_der(&self) -> [u8; 44] {
        let mut out = [0u8; 44];
        out[..12].copy_from_slice(&ED25519_SPKI_PREFIX);
        out[12..].copy_from_slice(&self.public_key_bytes());
        out
    }

    // Standard Base64 of the 44-byte SPKI DER public key (`x-bfe-key`).
    pub fn public_key_base64(&self) -> &str {
        &self.public_key_b64
    }

    // 64-character lowercase hex SHA-256 of `public_key_base64()`.
    pub fn id_hex(&self) -> &str {
        &self.id_hex
    }

    // Colon-separated uppercase SHA-256 fingerprint of the 44-byte SPKI DER
    // public key (`sha-256 XX:XX:...`), matching `src/net/crypto.js::fingerprintOf`
    // and `battlefront-eternal-server/src/identity.js::fingerprintOf`.
    pub fn fingerprint(&self) -> String {
        let digest = Sha256::digest(self.public_key_spki_der());
        let mut out = String::with_capacity(8 + 32 * 3 - 1);
        out.push_str("sha-256 ");
        for (i, byte) in digest.iter().enumerate() {
            if i > 0 {
                out.push(':');
            }
            const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
            out.push(HEX_UPPER[(byte >> 4) as usize] as char);
            out.push(HEX_UPPER[(byte & 0x0f) as usize] as char);
        }
        out
    }

    // Converts this player identity into the `bfe-transport` handshake identity.
    pub fn transport_identity(&self) -> TransportIdentity {
        TransportIdentity::from_seed(self.seed)
    }

    // Signs a master HTTP request at a caller-supplied Unix timestamp (seconds)
    // and 32-char lowercase hex nonce. Ports `electron/network.js::signedHeaders`
    // and `battlefront-eternal-master/src/auth.js::verifySignature`:
    //   message = "{METHOD}\n{route}\n{time}\n{nonce}\n{sha256_hex(body)}"
    pub fn sign_master_request_at(
        &self,
        method: &str,
        route: &str,
        body: &[u8],
        time_secs: u64,
        nonce_hex: &str,
    ) -> SignedMasterHeaders {
        let method_upper = method.to_ascii_uppercase();
        let time = time_secs.to_string();
        let body_hash = hex_lower(&Sha256::digest(body));
        let message = format!("{method_upper}\n{route}\n{time}\n{nonce_hex}\n{body_hash}");
        let sig = self.signing.sign(message.as_bytes());
        SignedMasterHeaders {
            key: self.public_key_b64.clone(),
            time,
            nonce: nonce_hex.to_string(),
            signature: BASE64.encode(sig.to_bytes()),
            notice: ONLINE_NOTICE_VERSION,
        }
    }

    // Signs a master HTTP request using the current Unix clock and 16 random
    // nonce bytes (32 lowercase hex characters).
    pub fn sign_master_request(
        &self,
        method: &str,
        route: &str,
        body: &[u8],
    ) -> Result<SignedMasterHeaders, String> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("system clock before unix epoch: {e}"))?
            .as_secs();
        let mut nonce_bytes = [0u8; 16];
        getrandom::getrandom(&mut nonce_bytes).map_err(|e| format!("random failed: {e}"))?;
        let nonce_hex = hex_lower(&nonce_bytes);
        Ok(self.sign_master_request_at(method, route, body, now_secs, &nonce_hex))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ConsentRecord {
    version: String,
    #[serde(rename = "acceptedAt")]
    accepted_at: u64,
}

// Ports `readOnlineConsent` in `electron/network.js`: returns true only when
// the consent file exists and records `version == ONLINE_NOTICE_VERSION`.
pub fn read_online_consent(path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<ConsentRecord>(&raw) else {
        return false;
    };
    parsed.version == ONLINE_NOTICE_VERSION
}

// Ports `writeOnlineConsent` in `electron/network.js`: writes the accepted
// notice version atomically via a `.tmp` sibling file with 0o600 permissions.
pub fn write_online_consent(path: &Path) -> Result<bool, String> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("system clock before unix epoch: {e}"))?
        .as_millis() as u64;
    write_online_consent_at(path, now_ms)
}

pub fn write_online_consent_at(path: &Path, accepted_at_ms: u64) -> Result<bool, String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
    }
    let tmp_path = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let record = ConsentRecord {
        version: ONLINE_NOTICE_VERSION.to_string(),
        accepted_at: accepted_at_ms,
    };
    let payload =
        serde_json::to_vec(&record).map_err(|e| format!("serialize consent failed: {e}"))?;
    {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp_path)
            .map_err(|e| format!("{}: {e}", tmp_path.display()))?;
        file.write_all(&payload)
            .map_err(|e| format!("{}: {e}", tmp_path.display()))?;
    }
    fs::rename(&tmp_path, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub(crate) fn is_lower_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_node_ed25519_spki_id_fingerprint_and_master_signature() {
        let mut seed = [0u8; 32];
        for (i, byte) in seed.iter_mut().enumerate() {
            *byte = (i + 1) as u8;
        }
        let id = PlayerIdentity::from_seed(seed);
        assert_eq!(
            id.public_key_base64(),
            "MCowBQYDK2VwAyEAebVWLo/mVPlAeLES6KmLp5AfhTrmlb7X4OORC60ElmQ="
        );
        assert_eq!(
            id.id_hex(),
            "25475f00ca333c44b9a0f87d1bc3f5dcdaa7d2aafc0fcf059b256350a42d7f4f"
        );
        assert_eq!(
            id.fingerprint(),
            "sha-256 64:6D:6B:E4:9D:9F:00:48:F9:4F:67:74:9E:CA:35:15:6E:ED:4F:7A:7B:E1:8E:4F:C4:A9:4B:FD:44:E3:00:B0"
        );

        let body = format!("{{\"serverId\":\"{}\"}}", "a".repeat(64));
        let signed = id.sign_master_request_at(
            "POST",
            "/v1/relay-credentials",
            body.as_bytes(),
            1_760_000_000,
            "000102030405060708090a0b0c0d0e0f",
        );
        assert_eq!(signed.key, id.public_key_base64());
        assert_eq!(signed.time, "1760000000");
        assert_eq!(signed.nonce, "000102030405060708090a0b0c0d0e0f");
        assert_eq!(signed.notice, ONLINE_NOTICE_VERSION);
        assert_eq!(
            signed.signature,
            "i4FIcYs3WubEFerHNMfp7jHatfMc8XlcOfbID9RHusxGWNcQPEBpqH2V656XXXgJrIt8QNtKF/2HgioVRMT1DQ=="
        );
    }

    #[test]
    fn loads_and_persists_32_byte_seed_and_json_pkcs8() {
        let dir = std::env::temp_dir().join(format!(
            "bfe-online-id-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let key_path = dir.join("identity.key");
        let created = PlayerIdentity::load_or_create(&key_path).expect("create identity");
        let raw = fs::read(&key_path).expect("read seed file");
        assert_eq!(raw.len(), 32);
        let loaded = PlayerIdentity::load_or_create(&key_path).expect("reload identity");
        assert_eq!(created.id_hex(), loaded.id_hex());
        assert_eq!(created.public_key_base64(), loaded.public_key_base64());

        // Also test loading Electron's JSON PKCS#8 identity format.
        let mut pkcs8 = Vec::with_capacity(48);
        pkcs8.extend_from_slice(&ED25519_PKCS8_PREFIX);
        pkcs8.extend_from_slice(created.seed());
        let json_path = dir.join("player-identity.json");
        let json = serde_json::json!({
            "publicKey": created.public_key_base64(),
            "privateKey": BASE64.encode(&pkcs8),
        });
        fs::write(&json_path, serde_json::to_vec(&json).unwrap()).unwrap();
        let from_json = PlayerIdentity::load_or_create(&json_path).expect("load json identity");
        assert_eq!(from_json.id_hex(), created.id_hex());

        // Online consent round-trip.
        let consent_path = dir.join("online-consent.json");
        assert!(!read_online_consent(&consent_path));
        write_online_consent_at(&consent_path, 1_760_000_000_000).expect("write consent");
        assert!(read_online_consent(&consent_path));

        let _ = fs::remove_dir_all(&dir);
    }
}
