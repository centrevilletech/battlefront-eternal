//! Cryptographic handshake state machines (`ClientHandshake` and `ServerHandshakes`)
//! matching `src/net/handshake.js`.
//!
//! Implements a 3-message SIGMA-I (`INITIATION` -> `RESPONSE` -> `CONFIRM`) key
//! exchange over X25519 ephemeral Diffie-Hellman, Ed25519 identity signatures,
//! SHA-256 transcript hashing, and ChaCha20-Poly1305 AEAD protection:
//! - `INITIATION` is padded to 1,200 bytes (`lim::INITIATION_BYTES`) so clients
//!   demonstrate path MTU and cannot be used as amplification vectors.
//! - Stateless `RETRY` cookies bind the client's source path, session ID, and
//!   ephemeral public key before the server allocates a `Pending` slot.
//! - Transcript hashes `t1`, `t2`, and `t3` bind every header field, ephemeral
//!   key, identity DER, and application hello JSON byte in both directions.
use std::collections::VecDeque;
use base64::{Engine,engine::general_purpose::STANDARD};
use crate::{Error,Result,crypto::{self,Aead,Identity,RetryTokens}};
use crate::layout::{common as prefix,initiation as init,response as resp,confirm as conf,retry,proof,packet,limits as lim,VERSION};
use x25519_dalek::{StaticSecret,PublicKey};
// Domain-separation labels matching `INIT_LABEL`, `SERVER_LABEL`, and `CLIENT_LABEL`
// in `src/net/handshake.js`.
const INIT_LABEL:&[u8]=b"BFE-UDP/1 initiation";
const SERVER_LABEL:&[u8]=b"BFE-UDP/1 server";
const CLIENT_LABEL:&[u8]=b"BFE-UDP/1 client";
// Traffic keys move into the established packet owner. The saved response
// and confirmation remain independent, solely to answer handshake repeats.
#[derive(Clone)]
pub struct Keys{pub send:Aead,pub receive:Aead,pub local:u32,pub remote:u32}
// Outcome of feeding a server datagram into `ClientHandshake::receive`.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Event{Ignore,Retry,Version(u8),IdentityMismatch,Established,Duplicate}
// Client-side BFE-UDP/1 handshake state (`src/net/handshake.js` `ClientHandshake`).
// Retains the sealed `CONFIRM` packet and matched `RESPONSE` bytes after key derivation
// so lost confirmations can be retransmitted without regenerating AEAD proofs.
pub struct ClientHandshake{
    identity:Identity,expected:[u8;32],ephemeral:Option<StaticSecret>,public:[u8;32],random:[u8;16],pub session:u32,
    hello:Vec<u8>,transcript:[u8;32],response:[u8;lim::MAX_DATAGRAM],response_len:usize,
    confirm:[u8;lim::MAX_DATAGRAM],confirm_len:usize,token:[u8;lim::MAX_TOKEN],token_len:usize,
    pub server_hello:Vec<u8>,keys:Option<Keys>,
}
// Callers verify packet length against the generated header size constants before reading fields.
fn u16_at(bytes:&[u8],at:usize)->usize{u16::from_le_bytes([bytes[at],bytes[at+1]])as usize}
fn u32_at(bytes:&[u8],at:usize)->u32{u32::from_le_bytes([bytes[at],bytes[at+1],bytes[at+2],bytes[at+3]])}
impl ClientHandshake{
    // Parse a bounded object before generating keys. Arrays and scalars are
    // not application hellos, even though they are valid standalone JSON.
    pub fn new(identity:Identity,expected:[u8;32],hello:&[u8])->Result<Self>{
        if hello.len()>lim::MAX_HELLO{return Err(Error("handshake hello too large"));}
        let value:serde_json::Value=serde_json::from_slice(hello).map_err(|_|Error("invalid client hello"))?;
        if !value.is_object(){return Err(Error("invalid client hello"));}
        let ephemeral=StaticSecret::from(crypto::random::<32>()?);let public=PublicKey::from(&ephemeral).to_bytes();
        let mut session=u32::from_le_bytes(crypto::random()?);while session==0{session=u32::from_le_bytes(crypto::random()?);}
        let mut this=Self{identity,expected,ephemeral:Some(ephemeral),public,random:crypto::random()?,session,
            hello:hello.to_vec(),transcript:[0;32],response:[0;lim::MAX_DATAGRAM],response_len:0,confirm:[0;lim::MAX_DATAGRAM],confirm_len:0,
            token:[0;lim::MAX_TOKEN],token_len:0,server_hello:Vec::with_capacity(lim::MAX_HELLO),keys:None};
        let mut bytes=[0;lim::INITIATION_BYTES];this.initiation(&[],&mut bytes)?;
        this.transcript=crypto::hash(&[INIT_LABEL,&bytes[..lim::INITIATION_FIELDS]]);Ok(this)
    }
    // A candidate endpoint keeps its own retry token. Token and padding do not
    // enter the transcript, so another path cannot invalidate an earlier reply.
    pub fn initiation(&self,token:&[u8],out:&mut[u8;lim::INITIATION_BYTES])->Result<()>{
        if token.len()>lim::MAX_TOKEN{return Err(Error("invalid retry token"));}out.fill(0);
        out[init::TYPE]=packet::INITIATION as u8;out[init::VERSION]=VERSION as u8;
        out[init::SESSION..init::SESSION+4].copy_from_slice(&self.session.to_le_bytes());
        out[init::EPHEMERAL..init::EPHEMERAL+32].copy_from_slice(&self.public);out[init::RANDOM..init::RANDOM+16].copy_from_slice(&self.random);
        out[init::TOKEN_LENGTH..init::TOKEN_LENGTH+2].copy_from_slice(&(token.len()as u16).to_le_bytes());out[init::BYTES..init::BYTES+token.len()].copy_from_slice(token);Ok(())
    }
    // Returns the most recently received stateless `RETRY` token slice.
    pub fn token(&self)->&[u8]{&self.token[..self.token_len]}
    // Returns the sealed `CONFIRM` datagram once `Event::Established` has fired.
    pub fn confirm(&self)->&[u8]{&self.confirm[..self.confirm_len]}
    // Moves the derived application traffic keys out of the handshake into a `Connection`.
    pub fn take_keys(&mut self)->Option<Keys>{self.keys.take()}
    // Unauthentic or malformed traffic is ignored. A proven different
    // identity or version instead rejects just the candidate that replied.
    pub fn receive(&mut self,bytes:&[u8])->Event{self.read(bytes).unwrap_or(Event::Ignore)}
    fn read(&mut self,bytes:&[u8])->Result<Event>{
        if bytes.len()<prefix::BYTES||bytes.len()>lim::MAX_DATAGRAM||u32_at(bytes,prefix::SESSION)!=self.session{return Ok(Event::Ignore);}
        let kind=bytes[prefix::TYPE]as usize;
        // Stateless server retry cookie: store the opaque token so the caller can
        // re-send `INITIATION` on the same candidate path with the token attached.
        if kind==packet::RETRY{
            if self.confirm_len!=0||bytes.len()<retry::BYTES{return Ok(Event::Ignore);}
            let n=u16_at(bytes,retry::TOKEN_LENGTH);if n==0||n>lim::MAX_TOKEN||bytes.len()!=retry::BYTES+n{return Ok(Event::Ignore);}
            self.token[..n].copy_from_slice(&bytes[retry::BYTES..]);self.token_len=n;return Ok(Event::Retry);
        }
        // Explicit protocol version mismatch notice for this candidate session.
        if kind==packet::VERSION{return Ok(if self.confirm_len!=0||bytes.len()!=prefix::BYTES||bytes[prefix::VERSION]==VERSION as u8{Event::Ignore}else{Event::Version(bytes[prefix::VERSION])});}
        if kind!=packet::RESPONSE||bytes.len()<resp::BYTES+proof::BYTES+lim::TAG||bytes[resp::VERSION]!=VERSION as u8{return Ok(Event::Ignore);}
        // Re-send the original ciphertext after a lost confirmation. Producing
        // another proof under the same handshake nonce would reuse an AEAD nonce.
        if self.confirm_len!=0{return Ok(if bytes==&self.response[..self.response_len]{Event::Duplicate}else{Event::Ignore});}
        if resp::BYTES+u16_at(bytes,resp::SEALED_LENGTH)!=bytes.len(){return Ok(Event::Ignore);}
        let Some(private)=&self.ephemeral else{return Ok(Event::Ignore);};
        let peer=PublicKey::from(<[u8;32]>::try_from(&bytes[resp::EPHEMERAL..resp::EPHEMERAL+32]).map_err(|_|Error("invalid ephemeral key"))?);
        let shared=private.diffie_hellman(&peer);if shared.as_bytes().iter().all(|&n|n==0){return Ok(Event::Ignore);}
        // The response length itself joins the next transcript, but not t2.
        // Keeping those two spans distinct is required for JS confirmation.
        let t2=crypto::hash(&[&self.transcript,&bytes[..resp::SEALED_LENGTH]]);let secret=crypto::extract(&t2,shared.as_bytes());
        let server=Aead::new(crypto::expand(&secret,b"s hs")?)?;let client=Aead::new(crypto::expand(&secret,b"c hs")?)?;
        let mut clear=[0;lim::MAX_DATAGRAM];let length=bytes.len()-resp::BYTES-lim::TAG;clear[..length].copy_from_slice(&bytes[resp::BYTES..bytes.len()-lim::TAG]);
        if !server.open(&bytes[..resp::BYTES],0,&mut clear[..length],&bytes[bytes.len()-lim::TAG..]){return Ok(Event::Ignore);}
        let clear=&clear[..length];let n=u16_at(clear,proof::HELLO_LENGTH);
        if n>lim::MAX_HELLO||proof::BYTES+n!=length{return Ok(Event::Ignore);}
        // Verify the server's Ed25519 SPKI DER matches the expected 32-byte server node ID
        // and that its signature covers `H(t2 || server_der || server_hello)`.
        let (id,key)=crypto::public_identity(&clear[proof::IDENTITY..proof::SIGNATURE])?;
        if id!=self.expected{return Ok(Event::IdentityMismatch);}
        let hello=&clear[proof::BYTES..];let digest=crypto::hash(&[&t2,&clear[..proof::SIGNATURE],hello]);
        if !crypto::verify(&key,SERVER_LABEL,&digest,&clear[proof::SIGNATURE..proof::HELLO_LENGTH]){return Ok(Event::Ignore);}
        let value:serde_json::Value=serde_json::from_slice(hello).map_err(|_|Error("invalid server hello"))?;
        if !value.is_object(){return Ok(Event::Ignore);}
        let remote=u32_at(bytes,resp::SERVER_SESSION);if remote==0{return Ok(Event::Ignore);}
        let t3=crypto::hash(&[&t2,&bytes[resp::SEALED_LENGTH..]]);
        // Sign the exact hello bytes, including property order. The server
        // authenticates these bytes before interpreting admission policy.
        let signature=self.identity.sign(CLIENT_LABEL,&crypto::hash(&[&t3,&self.identity.der,&self.hello]))?;
        let out=&mut self.confirm;out[conf::TYPE]=packet::CONFIRM as u8;out[conf::VERSION]=VERSION as u8;
        out[conf::SESSION..conf::SESSION+4].copy_from_slice(&remote.to_le_bytes());let count=proof::BYTES+self.hello.len();
        out[conf::SEALED_LENGTH..conf::SEALED_LENGTH+2].copy_from_slice(&((count+lim::TAG)as u16).to_le_bytes());
        let body=&mut out[conf::BYTES..conf::BYTES+count];body[..proof::SIGNATURE].copy_from_slice(&self.identity.der);
        body[proof::SIGNATURE..proof::HELLO_LENGTH].copy_from_slice(&signature);body[proof::HELLO_LENGTH..proof::BYTES].copy_from_slice(&(self.hello.len()as u16).to_le_bytes());body[proof::BYTES..].copy_from_slice(&self.hello);
        let (header,tail)=out.split_at_mut(conf::BYTES);let tag=client.seal(header,0,&mut tail[..count])?;tail[count..count+lim::TAG].copy_from_slice(&tag);
        self.confirm_len=conf::BYTES+count+lim::TAG;let master=crypto::extract(&crypto::hash(&[&t3,&self.confirm[..self.confirm_len]]),&secret);
        // Drop the ephemeral secret only after every proof and derived key
        // is valid. A forged earlier response must leave a real reply usable.
        self.keys=Some(Keys{send:Aead::new(crypto::expand(&master,b"c ap")?)?,receive:Aead::new(crypto::expand(&master,b"s ap")?)?,local:self.session,remote});
        self.server_hello.extend_from_slice(hello);self.response[..bytes.len()].copy_from_slice(bytes);self.response_len=bytes.len();self.ephemeral=None;Ok(Event::Established)
    }
}

/// Half-open state for a single client `INITIATION` awaiting `CONFIRM`
/// (`src/net/handshake.js` `ServerHandshakes` `pending` map entry).
struct Pending {
    // Server-chosen 32-bit session identifier echoed in `RESPONSE` and `CONFIRM`.
    // Client-chosen 32-bit session identifier from `INITIATION`.
    // Client's X25519 ephemeral public key used to detect duplicate initiations.
    // Normalized path string (`direct|<local_port>|<ip>|<port>`) binding the handshake.
    // Network prefix (`/32` IPv4 or `/64` IPv6) used for per-subnet half-open caps.
    // Timestamp (`ms`) when the pending entry was created (for `pending_ms` expiry).
    // Timestamp (`ms`) when `RESPONSE` was sent (used to compute initial handshake RTT).
    // HKDF-Extracted handshake secret `HKDF-Extract(t2, X25519(e_s, e_c))`.
    // Handshake AEAD cipher keyed by `HKDF-Expand(secret, "c hs")` to open `CONFIRM`.
    // Transcript hash `H(t2 || response_tail)` covering the sealed server proof.
    // Cached wire bytes of the `RESPONSE` datagram for idempotent retransmission.
    server_session: u32,
    client_session: u32,
    client_ephemeral: [u8; 32],
    source: String,
    prefix: String,
    created: f64,
    sent_at: f64,
    secret: [u8; 32],
    client_hs: Aead,
    t3: [u8; 32],
    response: Vec<u8>,
}

/// Result of a verified client `CONFIRM` packet (`src/net/handshake.js`
/// `ServerHandshakes.confirm`).
pub struct CompletedHandshake {
    /// Directional 1-RTT application traffic keys (`s ap` send, `c ap` receive).
    /// Canonical source path key on which the handshake completed.
    /// Authenticated 32-byte client node ID (`SHA-256(SPKI DER)`).
    /// Lowercase 64-character hex encoding of `client_id`.
    /// Standard Base64 encoding of the client's 44-byte Ed25519 SPKI DER public key.
    /// Parsed JSON object from the client's signed hello payload.
    /// Measured round-trip time (`ms`) between `RESPONSE` transmission and `CONFIRM` receipt.
    /// Raw `CONFIRM` datagram bytes retained by the session to recognize duplicate confirms.
    /// Raw `RESPONSE` datagram bytes produced during initiation.
    pub keys: Keys,
    pub source: String,
    pub client_id: [u8; 32],
    pub client_id_hex: String,
    pub client_public_key: String,
    pub client_hello: serde_json::Value,
    pub rtt: f64,
    pub confirm: Vec<u8>,
    pub response: Vec<u8>,
}

/// Server-side BFE-UDP/1 handshake state machine (`src/net/handshake.js`
/// `ServerHandshakes`). Bounds half-open sessions globally and per network
/// prefix, issues stateless retry cookies when requested or above
/// `retry_above`, and verifies the client's signed confirmation.
pub struct ServerHandshakes {
    /// Server's static Ed25519 identity used to sign `RESPONSE` proofs.
    /// Stateless HMAC-SHA256 retry token generator and verifier.
    /// Global maximum number of simultaneous half-open handshakes (`1024`).
    /// Maximum number of half-open handshakes allowed from a single network prefix (`32`).
    /// Lifetime in milliseconds (`5000.0`) before an unconfirmed pending entry expires.
    /// Pending-table threshold (`256`) above which stateless `RETRY` cookies are required.
    pub identity: Identity,
    pub retry: RetryTokens,
    pub max_pending: usize,
    pub max_pending_per_prefix: usize,
    pub pending_ms: f64,
    pub retry_above: usize,
    pending: VecDeque<Pending>,
}

impl ServerHandshakes {
    /// Creates a server handshake manager with default capacity and retry thresholds
    /// matching `src/net/handshake.js` (`maxPending = 1024`, `maxPendingPerPrefix = 32`,
    /// `pendingMs = 5000`, `retryAbove = 256`).
    pub fn new(identity: Identity, retry: RetryTokens) -> Self {
        Self {
            identity,
            retry,
            max_pending: 1024,
            max_pending_per_prefix: 32,
            pending_ms: 5000.0,
            retry_above: 256,
            pending: VecDeque::with_capacity(64),
        }
    }

    /// Number of half-open handshakes currently awaiting a client `CONFIRM`.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Evicts half-open handshakes older than `pending_ms` (`5000.0` ms by default).
    pub fn expire(&mut self, now: f64) {
        let window = self.pending_ms;
        self.pending.retain(|p| now - p.created <= window);
    }

    // Counts active half-open entries originating from the same `/32` IPv4 or `/64` IPv6 prefix.
    fn prefix_count(&self, prefix: &str) -> usize {
        self.pending.iter().filter(|p| p.prefix == prefix).count()
    }

    /// `src/net/handshake.js` `ServerHandshakes.initiation`: validates the
    /// 1,200-byte UDP initiation, enforces retry tokens when `require_retry`
    /// is set or pending count reaches `retry_above`, and returns `VERSION`,
    /// `RETRY`, or a signed `RESPONSE` datagram.
    #[allow(clippy::too_many_arguments)]
    pub fn initiation(
        &mut self,
        bytes: &[u8],
        source: &str,
        prefix: &str,
        require_retry: bool,
        server_hello: &serde_json::Value,
        now: f64,
        session_taken: impl Fn(u32) -> bool,
    ) -> Option<Vec<u8>> {
        // Every valid client initiation must be padded to the full 1,200-byte minimum
        // (`lim::INITIATION_BYTES`) to prevent unverified source amplification.
        if bytes.len() != lim::INITIATION_BYTES || bytes[init::TYPE] != packet::INITIATION as u8 {
            return None;
        }
        let client_session = u32_at(bytes, init::SESSION);
        if client_session == 0 {
            return None;
        }
        let token_len = u16_at(bytes, init::TOKEN_LENGTH);
        if token_len > lim::MAX_TOKEN || init::BYTES + token_len > bytes.len() {
            return None;
        }
        // Reply with a 6-byte `VERSION` packet when the client speaks a different wire version.
        if bytes[init::VERSION] != VERSION as u8 {
            let mut out = vec![0u8; prefix::BYTES];
            out[prefix::TYPE] = packet::VERSION as u8;
            out[prefix::VERSION] = VERSION as u8;
            out[prefix::SESSION..prefix::SESSION + 4]
                .copy_from_slice(&client_session.to_le_bytes());
            return Some(out);
        }
        self.expire(now);
        let client_ephemeral: [u8; 32] =
            bytes[init::EPHEMERAL..init::EPHEMERAL + 32].try_into().ok()?;
        // Re-send the cached `RESPONSE` if the same client session, source path, and
        // ephemeral key retransmit `INITIATION` after a lost server reply.
        if let Some(existing) = self.pending.iter().find(|p| {
            p.client_session == client_session
                && p.source == source
                && p.client_ephemeral == client_ephemeral
        }) {
            return Some(existing.response.clone());
        }
        // Under load or when half-open state exceeds `retry_above`, demand a valid
        // path-bound `RETRY` token before performing X25519 or Ed25519 operations.
        let token = &bytes[init::BYTES..init::BYTES + token_len];
        let need_retry = require_retry || self.pending.len() >= self.retry_above;
        if need_retry
            && (token_len == 0
                || !self
                    .retry
                    .check(token, source, client_session, &client_ephemeral, now))
        {
            let issued = self
                .retry
                .mint(source, client_session, &client_ephemeral, now);
            let mut out = vec![0u8; retry::BYTES + issued.len()];
            out[retry::TYPE] = packet::RETRY as u8;
            out[retry::VERSION] = VERSION as u8;
            out[retry::SESSION..retry::SESSION + 4].copy_from_slice(&client_session.to_le_bytes());
            out[retry::TOKEN_LENGTH..retry::TOKEN_LENGTH + 2]
                .copy_from_slice(&(issued.len() as u16).to_le_bytes());
            out[retry::BYTES..].copy_from_slice(&issued);
            return Some(out);
        }
        // Enforce per-prefixhalf-open limits so a single subnet cannot monopolize
        // the pending table, and evict the oldest entry if the global cap is full.
        let prefix_key = if prefix.is_empty() { source } else { prefix };
        if self.prefix_count(prefix_key) >= self.max_pending_per_prefix {
            return None;
        }
        while self.pending.len() >= self.max_pending {
            self.pending.pop_front();
        }
        // Generate a fresh server X25519 ephemeral keypair and 16-byte server random,
        // and reject low-order peer points whose Diffie-Hellman output is all zeros.
        let ephemeral = StaticSecret::from(crypto::random::<32>().ok()?);
        let server_public = PublicKey::from(&ephemeral).to_bytes();
        let random_bytes = crypto::random::<16>().ok()?;
        let shared = ephemeral.diffie_hellman(&PublicKey::from(client_ephemeral));
        if shared.as_bytes().iter().all(|&b| b == 0) {
            return None;
        }
        // Allocate a non-zero local session ID that does not collide with any pending
        // handshake or already-established session on the server endpoint.
        let mut server_session = u32::from_le_bytes(crypto::random().ok()?);
        while server_session == 0
            || self.pending.iter().any(|p| p.server_session == server_session)
            || session_taken(server_session)
        {
            server_session = u32::from_le_bytes(crypto::random().ok()?);
        }
        if !server_hello.is_object() {
            return None;
        }
        let hello_bytes = serde_json::to_vec(server_hello).ok()?;
        if hello_bytes.len() > lim::MAX_HELLO {
            return None;
        }
        let count = proof::BYTES + hello_bytes.len();
        let total = resp::BYTES + count + lim::TAG;
        if total > lim::MAX_DATAGRAM {
            return None;
        }
        let mut out = vec![0u8; total];
        out[resp::TYPE] = packet::RESPONSE as u8;
        out[resp::VERSION] = VERSION as u8;
        out[resp::CLIENT_SESSION..resp::CLIENT_SESSION + 4]
            .copy_from_slice(&client_session.to_le_bytes());
        out[resp::SERVER_SESSION..resp::SERVER_SESSION + 4]
            .copy_from_slice(&server_session.to_le_bytes());
        out[resp::EPHEMERAL..resp::EPHEMERAL + 32].copy_from_slice(&server_public);
        out[resp::RANDOM..resp::RANDOM + 16].copy_from_slice(&random_bytes);
        out[resp::SEALED_LENGTH..resp::SEALED_LENGTH + 2]
            .copy_from_slice(&((count + lim::TAG) as u16).to_le_bytes());
        // Compute transcript hashes `t1` (fixed initiation header without token/padding)
        // and `t2` (`t1` plus response header up to `SEALED_LENGTH`), then derive
        // handshake AEAD keys `s hs` and `c hs` via HKDF-SHA256.
        let t1 = crypto::hash(&[INIT_LABEL, &bytes[..lim::INITIATION_FIELDS]]);
        let t2 = crypto::hash(&[&t1, &out[..resp::SEALED_LENGTH]]);
        let secret = crypto::extract(&t2, shared.as_bytes());
        let server_hs = Aead::new(crypto::expand(&secret, b"s hs").ok()?).ok()?;
        let client_hs = Aead::new(crypto::expand(&secret, b"c hs").ok()?).ok()?;
        // Sign `H(t2 || server_der || server_hello)` under the domain label
        // `"BFE-UDP/1 server"` so the client can verify server authenticity and hello integrity.
        let sig = self
            .identity
            .sign(
                SERVER_LABEL,
                &crypto::hash(&[&t2, &self.identity.der, &hello_bytes]),
            )
            .ok()?;
        {
            let body = &mut out[resp::BYTES..resp::BYTES + count];
            body[..proof::SIGNATURE].copy_from_slice(&self.identity.der);
            body[proof::SIGNATURE..proof::HELLO_LENGTH].copy_from_slice(&sig);
            body[proof::HELLO_LENGTH..proof::BYTES]
                .copy_from_slice(&(hello_bytes.len() as u16).to_le_bytes());
            body[proof::BYTES..].copy_from_slice(&hello_bytes);
        }
        // Seal the server's identity proof and hello JSON with `s hs` at nonce 0,
        // authenticating the unencrypted response header as AEAD AAD, and advance `t3`.
        let (header, tail) = out.split_at_mut(resp::BYTES);
        let tag = server_hs.seal(header, 0, &mut tail[..count]).ok()?;
        tail[count..count + lim::TAG].copy_from_slice(&tag);
        let t3 = crypto::hash(&[&t2, &out[resp::SEALED_LENGTH..]]);
        self.pending.push_back(Pending {
            server_session,
            client_session,
            client_ephemeral,
            source: source.to_string(),
            prefix: prefix_key.to_string(),
            created: now,
            sent_at: now,
            secret,
            client_hs,
            t3,
            response: out.clone(),
        });
        Some(out)
    }

    /// `src/net/handshake.js` `ServerHandshakes.confirm`: verifies the
    /// client's signed confirmation from the same source path and derives
    /// server traffic keys (`s ap` for send, `c ap` for receive).
    pub fn confirm(&mut self, bytes: &[u8], source: &str, now: f64) -> Option<CompletedHandshake> {
        if bytes.len() < conf::BYTES + proof::BYTES + lim::TAG || bytes.len() > lim::MAX_DATAGRAM {
            return None;
        }
        if bytes[conf::TYPE] != packet::CONFIRM as u8 || bytes[conf::VERSION] != VERSION as u8 {
            return None;
        }
        // Look up the half-open entry by `server_session` and verify the `CONFIRM`
        // arrived on the exact same source path that sent `INITIATION`.
        let server_session = u32_at(bytes, conf::SESSION);
        let pos = self
            .pending
            .iter()
            .position(|p| p.server_session == server_session)?;
        if self.pending[pos].source != source {
            return None;
        }
        // Verify the length-prefixed AEAD payload size matches the UDP datagram length
        // and decrypt the client proof using `c hs` at nonce 0 with the header as AAD.
        let sealed_len = u16_at(bytes, conf::SEALED_LENGTH);
        if conf::BYTES + sealed_len != bytes.len() {
            return None;
        }
        let length = sealed_len - lim::TAG;
        let mut clear = vec![0u8; length];
        clear.copy_from_slice(&bytes[conf::BYTES..bytes.len() - lim::TAG]);
        if !self.pending[pos].client_hs.open(
            &bytes[..conf::BYTES],
            0,
            &mut clear,
            &bytes[bytes.len() - lim::TAG..],
        ) {
            return None;
        }
        let hello_len = u16_at(&clear, proof::HELLO_LENGTH);
        if hello_len > lim::MAX_HELLO || proof::BYTES + hello_len != clear.len() {
            return None;
        }
        // Extract the client's Ed25519 public key from the 44-byte SPKI DER prefix
        // and verify its signature over `H(t3 || client_der || client_hello)`.
        let der = &clear[proof::IDENTITY..proof::SIGNATURE];
        let (client_id, client_vk) = crypto::public_identity(der).ok()?;
        let hello = &clear[proof::BYTES..];
        let digest = crypto::hash(&[&self.pending[pos].t3, der, hello]);
        if !crypto::verify(
            &client_vk,
            CLIENT_LABEL,
            &digest,
            &clear[proof::SIGNATURE..proof::HELLO_LENGTH],
        ) {
            return None;
        }
        let client_hello: serde_json::Value = serde_json::from_slice(hello).ok()?;
        if !client_hello.is_object() {
            return None;
        }
        // Promote the half-open entry and derive 1-RTT application traffic keys:
        // server sends under `s ap` and receives client traffic under `c ap`.
        let pending = self.pending.remove(pos)?;
        let master = crypto::extract(&crypto::hash(&[&pending.t3, bytes]), &pending.secret);
        let send = Aead::new(crypto::expand(&master, b"s ap").ok()?).ok()?;
        let receive = Aead::new(crypto::expand(&master, b"c ap").ok()?).ok()?;
        Some(CompletedHandshake {
            keys: Keys {
                send,
                receive,
                local: pending.server_session,
                remote: pending.client_session,
            },
            source: pending.source,
            client_id,
            client_id_hex: crypto::hex_lower(&client_id),
            client_public_key: STANDARD.encode(der),
            client_hello,
            rtt: now - pending.sent_at,
            confirm: bytes.to_vec(),
            response: pending.response,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    // Verifies the full 3-way handshake with mandatory stateless retry cookie,
    // duplicate `RESPONSE` detection, and opposite-direction AEAD key symmetry.
    #[test]
    fn client_and_server_handshake_round_trip_with_retry_and_replays(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let server_id = Identity::from_seed([17u8; 32]);
        let client_id = Identity::from_seed([9u8; 32]);
        let retry = RetryTokens::from_secret([31u8; 32], 10_000.0);
        let mut server = ServerHandshakes::new(server_id.clone(), retry);
        let hello = br#"{"protocol":3,"profile":"snapshot-delta-v1","simulation":"bfe-sim-28","content":"abc","name":"Player"}"#;
        let mut client = ClientHandshake::new(client_id.clone(), server_id.id, hello)?;
        let mut init_bytes = [0u8; lim::INITIATION_BYTES];
        client.initiation(&[], &mut init_bytes)?;
        let server_hello = serde_json::json!({
            "protocol": 3,
            "profile": "snapshot-delta-v1",
            "simulation": "bfe-sim-28",
            "content": "abc"
        });
        let source = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000).to_string();
        let prefix = Ipv4Addr::LOCALHOST.to_string();
        // Step 1: When `require_retry` is true and the client has no token, the server
        // responds with a stateless `RETRY` packet carrying an HMAC cookie.
        let retry_pkt = server
            .initiation(
                &init_bytes,
                &source,
                &prefix,
                true,
                &server_hello,
                100.0,
                |_| false,
            )
            .ok_or("expected retry packet")?;
        assert_eq!(client.receive(&retry_pkt), Event::Retry);
        client.initiation(client.token(), &mut init_bytes)?;
        // Step 2: Re-sending `INITIATION` with the retry token produces a signed `RESPONSE`,
        // and re-feeding that exact `RESPONSE` to the client yields `Event::Duplicate`.
        let resp_pkt = server
            .initiation(
                &init_bytes,
                &source,
                &prefix,
                true,
                &server_hello,
                110.0,
                |_| false,
            )
            .ok_or("expected response packet")?;
        assert_eq!(client.receive(&resp_pkt), Event::Established);
        assert_eq!(client.receive(&resp_pkt), Event::Duplicate);
        // Step 3: Feeding the client's `CONFIRM` into the server completes the handshake
        // and produces matching opposite-direction ChaCha20-Poly1305 traffic keys.
        let completed = server
            .confirm(client.confirm(), &source, 120.0)
            .ok_or("expected completed handshake")?;
        assert_eq!(completed.client_id, client_id.id);
        let client_keys = client.take_keys().ok_or("expected client keys")?;
        assert_eq!(client_keys.local, completed.keys.remote);
        assert_eq!(client_keys.remote, completed.keys.local);
        let mut msg = *b"hello from server";
        let hdr = [0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let tag = completed.keys.send.seal(&hdr, 0, &mut msg)?;
        assert!(client_keys.receive.open(&hdr, 0, &mut msg, &tag));
        assert_eq!(&msg, b"hello from server");
        Ok(())
    }
}
