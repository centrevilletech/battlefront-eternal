//! handshake.js::ClientHandshake, including path-bound retry and duplicate
//! response handling. Server admission and retry-token policy stay at the peer.
use crate::{Error,Result,crypto::{self,Aead,Identity}};
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
