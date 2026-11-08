//! crypto.js's standard primitives and domain-separated traffic keys.
//! Handshake admission may allocate; established packet protection uses the
//! caller's bounded byte slice and detached tags, including key rotation.
use aes_gcm::{Aes256Gcm, Nonce, Tag, aead::{AeadInPlace,KeyInit}};
use base64::{Engine,engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer,SigningKey,VerifyingKey,Signature};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Sha256,Digest};
use crate::{Error,Result};
use crate::layout::limits as lim;
const ED_PREFIX:[u8;12]=[0x30,0x2a,0x30,0x05,0x06,0x03,0x2b,0x65,0x70,0x03,0x21,0x00];
const PKCS8_PREFIX: [u8; 16] = [
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04, 0x20,
];
const NOTICE_VERSION: &str = "2026-10-11";
type HmacSha256 = Hmac<Sha256>;
// Entropy failure is a failed connection, never a deterministic replacement
// identity or a reused packet secret. All calls occur at cold admission.
pub fn random<const N:usize>()->Result<[u8;N]>{let mut bytes=[0;N];getrandom::getrandom(&mut bytes).map_err(|_|Error("system randomness unavailable"))?;Ok(bytes)}
// Transcript parts concatenate without a separator. Labels and fixed wire
// spans provide the domain and boundaries that both implementations sign.
pub fn hash(parts:&[&[u8]])->[u8;32]{let mut h=Sha256::new();for part in parts{h.update(part);}h.finalize().into()}
pub fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}
// crypto.js hkdfExtract separates transcript salt from shared key material.
// Fixed output arrays keep packet-key derivation independent of heap growth.
pub fn extract(salt:&[u8],ikm:&[u8])->[u8;32]{Hkdf::<Sha256>::extract(Some(salt),ikm).0.into()}
// The label prefix separates transport keys from the handshake extract.
// Stack storage serves every expansion, including a live traffic-key update.
pub fn expand<const N:usize>(secret:&[u8;32],label:&[u8])->Result<[u8;N]>{
    let mut info=[0;48];let prefix=b"bfe-udp1 ";if prefix.len()+label.len()>info.len(){return Err(Error("invalid key label"));}
    info[..prefix.len()].copy_from_slice(prefix);info[prefix.len()..prefix.len()+label.len()].copy_from_slice(label);
    let mut out=[0;N];Hkdf::<Sha256>::from_prk(secret).map_err(|_|Error("invalid traffic secret"))?
        .expand(&info[..prefix.len()+label.len()],&mut out).map_err(|_|Error("invalid key expansion"))?;Ok(out)
}
#[derive(Clone)]
pub struct Identity{key:SigningKey,pub der:[u8;44],pub id:[u8;32]}
impl Identity{
    // Identity ids hash the base64 SPKI text, rather than the raw point.
    // Keeping that convention lets one saved seed reconnect as the same player.
    pub fn from_seed(seed:[u8;32])->Self{
        let key=SigningKey::from_bytes(&seed);let mut der=[0;44];der[..12].copy_from_slice(&ED_PREFIX);der[12..].copy_from_slice(key.verifying_key().as_bytes());
        let text=STANDARD.encode(der);let id=hash(&[text.as_bytes()]);Self{key,der,id}
    }
    /// Parses `identity.json`'s base64 SPKI `publicKey` and PKCS#8 PEM
    /// `privateKey` (`identity.js` `loadIdentity` and `src/net/crypto.js`
    /// `identityKeys`). Rejects any keypair whose public key does not match
    /// the seed inside its PKCS#8 envelope.
    pub fn from_keypair(public_key_b64: &str, private_key_pem: &str) -> Result<Self> {
        let spki = STANDARD
            .decode(public_key_b64.trim())
            .map_err(|_| Error("invalid identity"))?;
        let (_, expected_vk) = public_identity(&spki)?;
        let mut body = String::with_capacity(private_key_pem.len());
        for line in private_key_pem.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with("-----") {
                body.push_str(trimmed);
            }
        }
        let pkcs8 = STANDARD
            .decode(body.as_bytes())
            .map_err(|_| Error("invalid identity"))?;
        if pkcs8.len() != 48 || pkcs8[..16] != PKCS8_PREFIX {
            return Err(Error("invalid identity"));
        }
        let seed: [u8; 32] = pkcs8[16..48]
            .try_into()
            .map_err(|_| Error("invalid identity"))?;
        let id = Self::from_seed(seed);
        if id.key.verifying_key() != expected_vk {
            return Err(Error("invalid identity"));
        }
        Ok(id)
    }
    pub fn generate()->Result<Self>{Ok(Self::from_seed(random()?))}
    pub fn seed(&self)->[u8;32]{self.key.to_bytes()}
    pub fn public_key_base64(&self) -> String {
        STANDARD.encode(self.der)
    }
    pub fn private_key_pem(&self) -> String {
        let mut pkcs8 = [0u8; 48];
        pkcs8[..16].copy_from_slice(&PKCS8_PREFIX);
        pkcs8[16..].copy_from_slice(&self.seed());
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            STANDARD.encode(pkcs8)
        )
    }
    pub fn id_hex(&self) -> String {
        hex_lower(&self.id)
    }
    /// `identity.js` `fingerprint(keys)`: uppercase colon-separated SHA-256 of
    /// the 44-byte SPKI DER, displayed on startup so operators can verify the
    /// server identity.
    pub fn fingerprint(&self) -> String {
        let digest = hash(&[&self.der]);
        let mut out = String::from("sha-256 ");
        for (i, b) in digest.iter().enumerate() {
            if i > 0 {
                out.push(':');
            }
            use std::fmt::Write;
            let _ = write!(out, "{b:02X}");
        }
        out
    }
    // Sign a domain-separated transcript, never caller-controlled JSON alone.
    // The server and client use distinct labels for their identity proofs.
    pub fn sign(&self,label:&[u8],digest:&[u8;32])->Result<[u8;64]>{
        let mut text=[0;96];if label.len()+33>text.len(){return Err(Error("invalid proof label"));}
        text[..label.len()].copy_from_slice(label);text[label.len()+1..label.len()+33].copy_from_slice(digest);
        Ok(self.key.sign(&text[..label.len()+33]).to_bytes())
    }
    /// `identity.js` `proof(keys, transcript)` raw Ed25519 signature over
    /// `message`, returned as base64.
    pub fn sign_base64(&self, message: &[u8]) -> String {
        STANDARD.encode(self.key.sign(message).to_bytes())
    }
    /// `identity.js` `headers(keys, method, path, body)`: signs master HTTP
    /// requests over `${method}\n${path}\n${time}\n${nonce}\n${sha256_hex(body)}`
    /// where `time` is Unix seconds (`Math.floor(Date.now() / 1000)`).
    pub fn headers(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        now_sec: u64,
    ) -> Result<Vec<(String, String)>> {
        let time = now_sec.to_string();
        let nonce = hex_lower(&random::<16>()?);
        let body_hash = hex_lower(&hash(&[body]));
        let payload = format!("{method}\n{path}\n{time}\n{nonce}\n{body_hash}");
        let sig = self.key.sign(payload.as_bytes()).to_bytes();
        Ok(vec![
            ("x-bfe-notice".into(), NOTICE_VERSION.into()),
            ("x-bfe-key".into(), self.public_key_base64()),
            ("x-bfe-time".into(), time),
            ("x-bfe-nonce".into(), nonce),
            ("x-bfe-signature".into(), STANDARD.encode(sig)),
        ])
    }
}
/// Verifies an Ed25519 signature (`signature_b64` base64) over `message`
/// under a base64 SPKI `public_key_b64` (`identity.js` `verifyProof` and
/// `bans.js` `BanCache.refresh`).
pub fn verify_spki_base64(public_key_b64: &str, message: &[u8], signature_b64: &str) -> bool {
    let Ok(der) = STANDARD.decode(public_key_b64.trim()) else {
        return false;
    };
    let Ok((_, vk)) = public_identity(&der) else {
        return false;
    };
    let Ok(sig_bytes) = STANDARD.decode(signature_b64.trim()) else {
        return false;
    };
    let Ok(sig_arr) = <[u8; 64]>::try_from(sig_bytes.as_slice()) else {
        return false;
    };
    vk.verify_strict(message, &Signature::from_bytes(&sig_arr))
        .is_ok()
}
pub fn public_identity(der:&[u8])->Result<([u8;32],VerifyingKey)>{
    if der.len()!=44||der[..12]!=ED_PREFIX{return Err(Error("invalid identity"));}
    let raw:[u8;32]=der[12..].try_into().map_err(|_|Error("invalid identity"))?;
    // verify_strict rejects small-order points. Check canonical y separately,
    // as publicIdentity does, before hashing a point's transport identity.
    let mut y=raw;y[31]&=127;let mut prime=[255;32];prime[0]=237;prime[31]=127;
    if y.iter().rev().cmp(prime.iter().rev())!=std::cmp::Ordering::Less{return Err(Error("invalid identity"));}
    let key=VerifyingKey::from_bytes(&raw).map_err(|_|Error("invalid identity"))?;
    if key.is_weak(){return Err(Error("invalid identity"));}
    let text=STANDARD.encode(der);Ok((hash(&[text.as_bytes()]),key))
}
// A proof signs label, NUL and transcript digest. Checking fixed lengths
// before constructing Signature keeps malformed proofs outside the verifier.
pub fn verify(key:&VerifyingKey,label:&[u8],digest:&[u8;32],signature:&[u8])->bool{
    let Ok(signature)=<&[u8;64]>::try_from(signature)else{return false;};
    let mut text=[0;96];if label.len()+33>text.len(){return false;}
    text[..label.len()].copy_from_slice(label);text[label.len()+1..label.len()+33].copy_from_slice(digest);
    key.verify_strict(&text[..label.len()+33],&Signature::from_bytes(signature)).is_ok()
}
/// `src/net/crypto.js` `RetryTokens`: stateless address-validation cookie for
/// `INITIATION` floods. Binds the timestamp, remote source key, `session` and
/// client ephemeral X25519 key under a 32-byte `Hmac<Sha256>` secret (8-byte
/// little-endian `f64` timestamp followed by a 16-byte truncated tag).
#[derive(Clone)]
pub struct RetryTokens {
    secret: [u8; 32],
    pub lifetime_ms: f64,
}
impl RetryTokens {
    pub fn new() -> Result<Self> {
        Ok(Self::from_secret(random()?, 10_000.0))
    }
    pub fn from_secret(secret: [u8; 32], lifetime_ms: f64) -> Self {
        Self { secret, lifetime_ms }
    }
    fn mac(&self, time: f64, source: &str, session: u32, ephemeral: &[u8; 32]) -> HmacSha256 {
        let time_text = if time == 0.0 {
            "0".to_string()
        } else if time.fract() == 0.0 && time.abs() < 9_007_199_254_740_991.0 {
            format!("{}", time as i64)
        } else {
            format!("{time}")
        };
        let prefix = format!("{time_text}|{source}|{session}");
        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(&self.secret).expect("32-byte HMAC key");
        mac.update(prefix.as_bytes());
        mac.update(ephemeral);
        mac
    }
    pub fn mint(
        &self,
        source: &str,
        session: u32,
        ephemeral: &[u8; 32],
        now: f64,
    ) -> [u8; 24] {
        let mut out = [0u8; 24];
        out[..8].copy_from_slice(&now.to_le_bytes());
        let digest = self
            .mac(now, source, session, ephemeral)
            .finalize()
            .into_bytes();
        out[8..24].copy_from_slice(&digest[..16]);
        out
    }
    pub fn check(
        &self,
        token: &[u8],
        source: &str,
        session: u32,
        ephemeral: &[u8; 32],
        now: f64,
    ) -> bool {
        if token.len() != 24 {
            return false;
        }
        let time = f64::from_le_bytes(token[..8].try_into().unwrap());
        if !time.is_finite() || time > now + 1000.0 || now - time > self.lifetime_ms {
            return false;
        }
        self.mac(time, source, session, ephemeral)
            .verify_truncated_left(&token[8..24])
            .is_ok()
    }
}
#[derive(Clone)]
pub struct Aead{secret:[u8;32],cipher:Aes256Gcm,iv:[u8;lim::IV_BYTES]}
impl Aead{
    pub fn new(secret:[u8;32])->Result<Self>{
        let key=expand::<32>(&secret,b"key")?;let iv=expand(&secret,b"iv")?;
        Ok(Self{secret,cipher:Aes256Gcm::new_from_slice(&key).map_err(|_|Error("invalid encryption key"))?,iv})
    }
    // Key updates derive from this direction's traffic secret. Packet numbers
    // keep advancing so a rotated key never reuses an earlier packet nonce.
    pub fn next(&self)->Result<Self>{Self::new(expand(&self.secret,b"ku")?)}
    // Only the last network-order word changes. Packet numbers never repeat
    // for one key, even when its plaintext is retransmitted in a newer packet.
    fn nonce(&self,number:u32)->[u8;lim::IV_BYTES]{
        let mut out=self.iv;let tail=u32::from_be_bytes(self.iv[8..].try_into().unwrap())^number;
        out[8..].copy_from_slice(&tail.to_be_bytes());out
    }
    pub fn seal(&self,header:&[u8],number:u32,body:&mut[u8])->Result<[u8;lim::TAG]>{
        self.cipher.encrypt_in_place_detached(Nonce::from_slice(&self.nonce(number)),header,body)
            .map(Into::into).map_err(|_|Error("packet encryption failed"))
    }
    // Detached verification works on caller-owned bytes. A failed key-phase
    // trial is discarded before the connection commits replay or key state.
    // Authentication mutates only packet scratch. The connection commits
    // replay-window and key-phase state after this operation succeeds.
    pub fn open(&self,header:&[u8],number:u32,body:&mut[u8],tag:&[u8])->bool{
        if tag.len()!=lim::TAG{return false;}
        self.cipher.decrypt_in_place_detached(Nonce::from_slice(&self.nonce(number)),header,body,Tag::from_slice(tag)).is_ok()
    }
}
