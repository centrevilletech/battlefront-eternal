//! crypto.js's standard primitives and domain-separated traffic keys.
//! Handshake admission may allocate; established packet protection uses the
//! caller's bounded byte slice and detached tags, including key rotation.
use aes_gcm::{Aes256Gcm, Nonce, Tag, aead::{AeadInPlace,KeyInit}};
use base64::{Engine,engine::general_purpose::STANDARD};
use ed25519_dalek::{Signer,SigningKey,VerifyingKey,Signature};
use hkdf::Hkdf;
use sha2::{Sha256,Digest};
use crate::{Error,Result};
use crate::layout::limits as lim;
const ED_PREFIX:[u8;12]=[0x30,0x2a,0x30,0x05,0x06,0x03,0x2b,0x65,0x70,0x03,0x21,0x00];
// Entropy failure is a failed connection, never a deterministic replacement
// identity or a reused packet secret. All calls occur at cold admission.
pub fn random<const N:usize>()->Result<[u8;N]>{let mut bytes=[0;N];getrandom::getrandom(&mut bytes).map_err(|_|Error("system randomness unavailable"))?;Ok(bytes)}
// Transcript parts concatenate without a separator. Labels and fixed wire
// spans provide the domain and boundaries that both implementations sign.
pub fn hash(parts:&[&[u8]])->[u8;32]{let mut h=Sha256::new();for part in parts{h.update(part);}h.finalize().into()}
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
pub struct Identity{key:SigningKey,pub der:[u8;44],pub id:[u8;32]}
impl Identity{
    // Identity ids hash the base64 SPKI text, rather than the raw point.
    // Keeping that convention lets one saved seed reconnect as the same player.
    pub fn from_seed(seed:[u8;32])->Self{
        let key=SigningKey::from_bytes(&seed);let mut der=[0;44];der[..12].copy_from_slice(&ED_PREFIX);der[12..].copy_from_slice(key.verifying_key().as_bytes());
        let text=STANDARD.encode(der);let id=hash(&[text.as_bytes()]);Self{key,der,id}
    }
    pub fn generate()->Result<Self>{Ok(Self::from_seed(random()?))}
    pub fn seed(&self)->[u8;32]{self.key.to_bytes()}
    // Sign a domain-separated transcript, never caller-controlled JSON alone.
    // The server and client use distinct labels for their identity proofs.
    pub fn sign(&self,label:&[u8],digest:&[u8;32])->Result<[u8;64]>{
        let mut text=[0;96];if label.len()+33>text.len(){return Err(Error("invalid proof label"));}
        text[..label.len()].copy_from_slice(label);text[label.len()+1..label.len()+33].copy_from_slice(digest);
        Ok(self.key.sign(&text[..label.len()+33]).to_bytes())
    }
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
