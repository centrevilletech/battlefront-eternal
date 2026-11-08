//! handshake.js::ClientHandshake and ServerHandshakes, including path-bound
//! retry and duplicate response handling.
use std::collections::VecDeque;
use base64::{Engine,engine::general_purpose::STANDARD};
use crate::{Error,Result,crypto::{self,Aead,Identity,RetryTokens}};
use crate::layout::{common as prefix,initiation as init,response as resp,confirm as conf,retry,proof,packet,limits as lim,VERSION};
use x25519_dalek::{StaticSecret,PublicKey};
const INIT_LABEL:&[u8]=b"BFE-UDP/1 initiation";
const SERVER_LABEL:&[u8]=b"BFE-UDP/1 server";
const CLIENT_LABEL:&[u8]=b"BFE-UDP/1 client";
// Traffic keys move into the established packet owner. The saved response
// and confirmation remain independent, solely to answer handshake repeats.
#[derive(Clone)]
pub struct Keys{pub send:Aead,pub receive:Aead,pub local:u32,pub remote:u32}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Event{Ignore,Retry,Version(u8),IdentityMismatch,Established,Duplicate}
pub struct ClientHandshake{
    identity:Identity,expected:[u8;32],ephemeral:Option<StaticSecret>,public:[u8;32],random:[u8;16],pub session:u32,
    hello:Vec<u8>,transcript:[u8;32],response:[u8;lim::MAX_DATAGRAM],response_len:usize,
    confirm:[u8;lim::MAX_DATAGRAM],confirm_len:usize,token:[u8;lim::MAX_TOKEN],token_len:usize,
    pub server_hello:Vec<u8>,keys:Option<Keys>,
}
fn u16_at(bytes:&[u8],at:usize)->usize{u16::from_le_bytes([bytes[at],bytes[at+1]])as usize}
fn u32_at(bytes:&[u8],at:usize)->u32{u32::from_le_bytes(bytes[at..at+4].try_into().unwrap())}
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
    pub fn token(&self)->&[u8]{&self.token[..self.token_len]}
    pub fn confirm(&self)->&[u8]{&self.confirm[..self.confirm_len]}
    pub fn take_keys(&mut self)->Option<Keys>{self.keys.take()}
    // Unauthentic or malformed traffic is ignored. A proven different
    // identity or version instead rejects just the candidate that replied.
    pub fn receive(&mut self,bytes:&[u8])->Event{self.read(bytes).unwrap_or(Event::Ignore)}
    fn read(&mut self,bytes:&[u8])->Result<Event>{
        if bytes.len()<prefix::BYTES||bytes.len()>lim::MAX_DATAGRAM||u32_at(bytes,prefix::SESSION)!=self.session{return Ok(Event::Ignore);}
        let kind=bytes[prefix::TYPE]as usize;
        if kind==packet::RETRY{
            if self.confirm_len!=0||bytes.len()<retry::BYTES{return Ok(Event::Ignore);}
            let n=u16_at(bytes,retry::TOKEN_LENGTH);if n==0||n>lim::MAX_TOKEN||bytes.len()!=retry::BYTES+n{return Ok(Event::Ignore);}
            self.token[..n].copy_from_slice(&bytes[retry::BYTES..]);self.token_len=n;return Ok(Event::Retry);
        }
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
    pub identity: Identity,
    pub retry: RetryTokens,
    pub max_pending: usize,
    pub max_pending_per_prefix: usize,
    pub pending_ms: f64,
    pub retry_above: usize,
    pending: VecDeque<Pending>,
}

impl ServerHandshakes {
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

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn expire(&mut self, now: f64) {
        let window = self.pending_ms;
        self.pending.retain(|p| now - p.created <= window);
    }

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
        if let Some(existing) = self.pending.iter().find(|p| {
            p.client_session == client_session
                && p.source == source
                && p.client_ephemeral == client_ephemeral
        }) {
            return Some(existing.response.clone());
        }
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
        let prefix_key = if prefix.is_empty() { source } else { prefix };
        if self.prefix_count(prefix_key) >= self.max_pending_per_prefix {
            return None;
        }
        while self.pending.len() >= self.max_pending {
            self.pending.pop_front();
        }
        let ephemeral = StaticSecret::from(crypto::random::<32>().ok()?);
        let server_public = PublicKey::from(&ephemeral).to_bytes();
        let random_bytes = crypto::random::<16>().ok()?;
        let shared = ephemeral.diffie_hellman(&PublicKey::from(client_ephemeral));
        if shared.as_bytes().iter().all(|&b| b == 0) {
            return None;
        }
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
        let t1 = crypto::hash(&[INIT_LABEL, &bytes[..lim::INITIATION_FIELDS]]);
        let t2 = crypto::hash(&[&t1, &out[..resp::SEALED_LENGTH]]);
        let secret = crypto::extract(&t2, shared.as_bytes());
        let server_hs = Aead::new(crypto::expand(&secret, b"s hs").ok()?).ok()?;
        let client_hs = Aead::new(crypto::expand(&secret, b"c hs").ok()?).ok()?;
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
        let server_session = u32_at(bytes, conf::SESSION);
        let pos = self
            .pending
            .iter()
            .position(|p| p.server_session == server_session)?;
        if self.pending[pos].source != source {
            return None;
        }
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

    #[test]
    fn client_and_server_handshake_round_trip_with_retry_and_replays() {
        let server_id = Identity::from_seed([17u8; 32]);
        let client_id = Identity::from_seed([9u8; 32]);
        let retry = RetryTokens::from_secret([31u8; 32], 10_000.0);
        let mut server = ServerHandshakes::new(server_id.clone(), retry);
        let hello = br#"{"protocol":3,"profile":"snapshot-delta-v1","simulation":"bfe-sim-28","content":"abc","name":"Player"}"#;
        let mut client = ClientHandshake::new(client_id.clone(), server_id.id, hello).unwrap();
        let mut init_bytes = [0u8; lim::INITIATION_BYTES];
        client.initiation(&[], &mut init_bytes).unwrap();
        let server_hello = serde_json::json!({
            "protocol": 3,
            "profile": "snapshot-delta-v1",
            "simulation": "bfe-sim-28",
            "content": "abc"
        });
        let source = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 40000).to_string();
        let prefix = Ipv4Addr::LOCALHOST.to_string();
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
            .unwrap();
        assert_eq!(client.receive(&retry_pkt), Event::Retry);
        client.initiation(client.token(), &mut init_bytes).unwrap();
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
            .unwrap();
        assert_eq!(client.receive(&resp_pkt), Event::Established);
        assert_eq!(client.receive(&resp_pkt), Event::Duplicate);
        let completed = server
            .confirm(client.confirm(), &source, 120.0)
            .unwrap();
        assert_eq!(completed.client_id, client_id.id);
        let client_keys = client.take_keys().unwrap();
        assert_eq!(client_keys.local, completed.keys.remote);
        assert_eq!(client_keys.remote, completed.keys.local);
        let mut msg = *b"hello from server";
        let hdr = [0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let tag = completed.keys.send.seal(&hdr, 0, &mut msg).unwrap();
        assert!(client_keys.receive.open(&hdr, 0, &mut msg, &tag));
        assert_eq!(&msg, b"hello from server");
    }
}
