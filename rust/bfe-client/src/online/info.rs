//! Direct joins read bounded public metadata before signing a game hello.
//! Gameplay policy remains in bfeserver; the client checks its own content and
//! the engine's generated contract independently of the advertised digest.
use std::{io::{Read,Write},net::{TcpStream,ToSocketAddrs},path::{Path,PathBuf},time::Duration};
use serde::Deserialize;
use bfe_transport::{crypto::{Identity,random},layout::network_contract as contract};
#[derive(Deserialize)]
pub struct Server{
    pub id:String,pub protocol:usize,pub profile:String,pub simulation:String,pub content:String,pub query:String,
    #[serde(default)]pub name:String,
}
impl Server{
    // Discovery is connection-time HTTP only. Bound the body before parsing
    // so an endpoint cannot make the native admission buffer unbounded.
    pub fn read(address:&str)->Result<Self,String>{
        if address.len()>255||address.bytes().any(|b|b.is_ascii_control()){return Err("invalid server address".into());}
        let addresses=address.to_socket_addrs().map_err(|e|format!("cannot resolve server: {e}"))?;let mut socket=None;
        for target in addresses.take(16){if let Ok(s)=TcpStream::connect_timeout(&target,Duration::from_secs(3)){socket=Some(s);break;}}
        let mut socket=socket.ok_or("cannot reach server information endpoint")?;socket.set_read_timeout(Some(Duration::from_secs(5))).map_err(|e|e.to_string())?;socket.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e|e.to_string())?;
        write!(socket,"GET /info HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAccept: application/json\r\n\r\n").map_err(|e|e.to_string())?;
        let mut bytes=Vec::with_capacity(8192);socket.take(131073).read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        if bytes.len()>131072{return Err("server information exceeds bounds".into());}
        let body=response(&bytes)?;let info:Server=serde_json::from_slice(&body).map_err(|e|format!("invalid server information: {e}"))?;
        info.validate()?;Ok(info)
    }
    pub fn validate(&self)->Result<(),String>{
        self.identity()?;
        if self.protocol!=contract::PROTOCOL||self.profile!=contract::PROFILE||self.simulation!=contract::SIMULATION{return Err("Update required: native client and server must use the same engine contract".into());}
        if self.query.len()>2048||self.content.len()>128||self.name.len()>400{return Err("invalid server information".into());}Ok(())
    }
    // The identity announced here pins the subsequent signed UDP handshake.
    // It is not inferred from the address or the server display name.
    pub fn identity(&self)->Result<[u8;32],String>{
        if self.id.len()!=64||!self.id.bytes().all(|b|b.is_ascii_hexdigit()){return Err("invalid server identity".into());}
        let mut id=[0;32];for (i,value)in id.iter_mut().enumerate(){*value=u8::from_str_radix(&self.id[i*2..i*2+2],16).map_err(|_|"invalid server identity")?;}Ok(id)
    }
    // The advertised content is only a comparison target. Calling code must
    // compute the local fingerprint and send that value, including mismatches.
    pub fn hello(content:&str,name:&str)->Vec<u8>{
        let name=name.chars().filter(|c|!c.is_control()).take(32).collect::<String>();
        serde_json::to_vec(&serde_json::json!({"protocol":contract::PROTOCOL,"profile":contract::PROFILE,"simulation":contract::SIMULATION,"content":content,"name":name})).expect("bounded client hello")
    }
}
// The signed handshake is authoritative. HTTP discovery metadata may have
// changed between loading the map and opening the authenticated connection.
pub fn authenticated(bytes:&[u8])->Result<(),String>{
    #[derive(Deserialize)]struct Hello<'a>{protocol:usize,profile:&'a str,simulation:&'a str}
    let hello:Hello=serde_json::from_slice(bytes).map_err(|_|"invalid authenticated server contract")?;
    if hello.protocol!=contract::PROTOCOL||hello.profile!=contract::PROFILE||hello.simulation!=contract::SIMULATION{return Err("native client and server contract mismatch".into());}Ok(())
}
fn response(bytes:&[u8])->Result<Vec<u8>,String>{
    let at=bytes.windows(4).position(|b|b==b"\r\n\r\n").ok_or("invalid HTTP information response")?;
    if at>16384{return Err("HTTP information headers exceed bounds".into());}
    let header=std::str::from_utf8(&bytes[..at]).map_err(|_|"invalid HTTP information headers")?;
    let status=header.lines().next().unwrap_or("").split_whitespace().nth(1);if status!=Some("200"){return Err("server information request failed".into());}
    let body=&bytes[at+4..];let chunked=header.lines().any(|line|line.split_once(':').is_some_and(|(k,v)|k.eq_ignore_ascii_case("transfer-encoding")&&v.trim().eq_ignore_ascii_case("chunked")));
    if !chunked{return Ok(body.to_vec());}
    // Node may stream /info rather than declaring Content-Length. Decode only
    // bounded chunks, and never treat trailing HTTP bytes as another JSON body.
    let mut out=Vec::with_capacity(body.len());let mut cursor=0;
    loop{
        let line=body[cursor..].windows(2).position(|v|v==b"\r\n").ok_or("truncated HTTP chunk")?;
        let text=std::str::from_utf8(&body[cursor..cursor+line]).map_err(|_|"invalid HTTP chunk size")?;
        let size=usize::from_str_radix(text.split(';').next().unwrap_or(""),16).map_err(|_|"invalid HTTP chunk size")?;cursor+=line+2;if size==0{break;}
        let end=cursor.checked_add(size).filter(|end|*end<=body.len().saturating_sub(2)).ok_or("truncated HTTP chunk")?;
        if &body[end..end+2]!=b"\r\n"{return Err("invalid HTTP chunk terminator".into());}out.extend_from_slice(&body[cursor..end]);cursor=end+2;
    }Ok(out)
}
// Profiles survive process restarts. Platform application data, rather than
// the executable directory, gives ordinary launches the same durable key.
pub fn default_identity()->Result<PathBuf,String>{
    let base=std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(||std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(||std::env::var_os("HOME").map(|v|PathBuf::from(v).join(".local/share"))).ok_or("identity path required")?;
    Ok(base.join("battlefront-eternal").join("identity.key"))
}
// Key creation is exclusive so simultaneous windows cannot overwrite a
// profile. A partial or invalid key requires an explicit user-side repair.
pub fn identity(path:&Path)->Result<Identity,String>{
    match std::fs::read(path){
        Ok(bytes)=>{let seed=bytes.as_slice().try_into().map_err(|_|"invalid identity seed file")?;return Ok(Identity::from_seed(seed));},
        Err(e)if e.kind()==std::io::ErrorKind::NotFound=>{},Err(e)=>return Err(format!("cannot read identity: {e}")),
    }
    if let Some(parent)=path.parent().filter(|p|!p.as_os_str().is_empty()){std::fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
    let seed=random::<32>().map_err(|e|e.to_string())?;let mut options=std::fs::OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)]{use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    // A simultaneous window may have created the same profile. Read that key;
    // neither overwrite it nor silently replace an invalid identity file.
    match options.open(path){Ok(mut file)=>{file.write_all(&seed).map_err(|e|e.to_string())?;file.sync_all().map_err(|e|e.to_string())?;Ok(Identity::from_seed(seed))},Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>identity(path),Err(e)=>Err(format!("cannot save identity: {e}"))}
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn bounded_http_body_handles_content_length_and_chunks(){
        assert_eq!(response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap(),b"{}");
        assert_eq!(response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n{\r\n1\r\n}\r\n0\r\n\r\n").unwrap(),b"{}");
        assert!(response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffff\r\n{}").is_err());
        assert!(response(b"HTTP/1.1 404 Not Found\r\n\r\n{}").is_err());
    }
}

// Resolve discovery, content identity and transport candidates before creating
// a visible window. The endpoint remains nonblocking during window admission.
pub fn prepare(address:&str,root:&Path,path:&Path,name:&str)->Result<(Server,Box<bfe_transport::endpoint::Endpoint>),String>{
    let server=Server::read(address)?;
    let files=bfe_content::files::DirFiles::new(root);
    let hash=bfe_content_native::fingerprint::hash(&files,&server.query)?;
    let endpoint=Box::new(bfe_transport::endpoint::Endpoint::connect(address,identity(path)?,server.identity()?,&Server::hello(&hash,name)).map_err(|e|e.to_string())?);
    Ok((server,endpoint))
}
