//! Direct joins read bounded public metadata before signing a game hello.
//! Gameplay policy remains in bfeserver; the client checks its own content and
//! the engine's generated contract independently of the advertised digest.
use std::{io::{Read,Write},net::{TcpStream,ToSocketAddrs},path::{Path,PathBuf},time::{Duration,Instant}};
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
        // One write: a request split over several segments can be answered and closed before its
        // end arrives, which resets the connection.
        let request=format!("GET /info HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAccept: application/json\r\n\r\n");
        socket.write_all(request.as_bytes()).map_err(|e|format!("{INTERRUPTED}: {e}"))?;
        let mut bytes=Vec::with_capacity(8192);socket.take(131073).read_to_end(&mut bytes).map_err(|e|format!("{INTERRUPTED}: {e}"))?;
        if bytes.len()>131072{return Err("server information exceeds bounds".into());}
        let body=response(&bytes)?;let info:Server=serde_json::from_slice(&body).map_err(|e|format!("invalid server information: {e}"))?;
        info.validate()?;Ok(info)
    }
    pub fn validate(&self)->Result<(),String>{
        self.identity()?;
        if self.protocol!=contract::PROTOCOL||self.profile!=contract::PROFILE||self.simulation!=contract::SIMULATION{return Err(version_mismatch(&self.simulation));}
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
// Which side is behind, in the player's words. Simulation names end in their revision.
pub fn version_mismatch(server_simulation:&str)->String{
    let number=|s:&str|s.rsplit('-').next().and_then(|n|n.parse::<u32>().ok());
    match (number(server_simulation),number(contract::SIMULATION)){
        (Some(theirs),Some(ours))if theirs<ours=>format!("This server runs an older version of Battlefront Eternal (simulation {theirs}, this game is {ours}). It can be joined once the server updates."),
        (Some(theirs),Some(ours))if theirs>ours=>format!("This server runs a newer version of Battlefront Eternal (simulation {theirs}, this game is {ours}). Update the game to join it."),
        _=>"This server runs another version of Battlefront Eternal. Update the game, or wait for the server to update.".into(),
    }
}
// The signed handshake is authoritative. HTTP discovery metadata may have
// changed between loading the map and opening the authenticated connection.
pub fn authenticated(bytes:&[u8])->Result<(),String>{
    #[derive(Deserialize)]struct Hello<'a>{protocol:usize,profile:&'a str,simulation:&'a str}
    let hello:Hello=serde_json::from_slice(bytes).map_err(|_|"invalid authenticated server contract")?;
    if hello.protocol!=contract::PROTOCOL||hello.profile!=contract::PROFILE||hello.simulation!=contract::SIMULATION{return Err(version_mismatch(hello.simulation));}Ok(())
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
// Errors of Server::read that mean nothing answered at that address.
pub fn unreachable(error:&str)->bool{error.starts_with("cannot resolve server")||error.starts_with("cannot reach server")}
const INTERRUPTED:&str="server information interrupted";
// Errors worth another try in a moment: nothing answered, or the connection broke off (a server
// changing battle or restarting). A server that answered with something else has said its piece.
pub fn transient(error:&str)->bool{unreachable(error)||error.starts_with(INTERRUPTED)||error=="invalid HTTP information response"}
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
    #[test]fn a_broken_or_refused_connection_is_worth_another_try(){
        assert!(transient("cannot reach server information endpoint"));
        assert!(transient(&format!("{INTERRUPTED}: Connection reset by peer (os error 104)")));
        assert!(transient("invalid HTTP information response"));
        assert!(!transient("server information request failed"));
        assert!(!transient(&version_mismatch("bfe-sim-1")));
    }
    #[test]fn version_refusal_says_which_side_is_behind(){
        let ours:u32=contract::SIMULATION.rsplit('-').next().unwrap().parse().unwrap();
        assert!(version_mismatch(&format!("bfe-sim-{}",ours-1)).contains("older version"));
        assert!(version_mismatch(&format!("bfe-sim-{}",ours+1)).contains("Update the game"));
        assert!(version_mismatch("unknown").contains("another version"));
    }
    // Rehearsal probe (not run by default): joins the listed server BFE_PROBE_SERVER through the
    // master at BFE_MASTER_URL with content BFE_PROBE_CONTENT, exactly as JOIN does, and reports the path.
    #[test]#[ignore]fn relay_probe(){
        let id=std::env::var("BFE_PROBE_SERVER").expect("BFE_PROBE_SERVER");let content=std::env::var("BFE_PROBE_CONTENT").expect("BFE_PROBE_CONTENT");
        let key=std::env::temp_dir().join(format!("bfe-probe-{}.key",std::process::id()));
        let started=std::time::Instant::now();
        let (server,mut endpoint)=super::prepare_listed(&id,Path::new(&content),&key,"PROBE").expect("prepare");
        eprintln!("PROBE prepared {} in {} ms",server.name,started.elapsed().as_millis());
        // The client pumps the endpoint only once the battle has loaded: BFE_PROBE_LOAD_MS stands for that load.
        let load=std::env::var("BFE_PROBE_LOAD_MS").ok().and_then(|v|v.parse::<u64>().ok()).unwrap_or(0);
        std::thread::sleep(Duration::from_millis(load));let started=std::time::Instant::now();
        while !endpoint.ready(){
            endpoint.pump(&mut |_,_,_,_|{}).expect("pump");
            assert!(started.elapsed()<Duration::from_secs(40),"PROBE FAILED: {:?}",endpoint.waiting_reason());
            std::thread::sleep(Duration::from_millis(2));
        }
        eprintln!("PROBE CONNECTED via {:?} after {} ms",endpoint.path(),started.elapsed().as_millis());
        let _=std::fs::remove_file(key);
    }
    #[test]fn bounded_http_body_handles_content_length_and_chunks(){
        assert_eq!(response(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}").unwrap(),b"{}");
        assert_eq!(response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\n{\r\n1\r\n}\r\n0\r\n\r\n").unwrap(),b"{}");
        assert!(response(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffff\r\n{}").is_err());
        assert!(response(b"HTTP/1.1 404 Not Found\r\n\r\n{}").is_err());
    }
}

// Resolve discovery, content identity and transport candidates before creating
// a visible window. The endpoint remains nonblocking during window admission.
// `expected` is the identity an internet listing named for this server: an
// endpoint that answers with another identity is not the listed server.
pub fn prepare(address:&str,root:&Path,path:&Path,name:&str,expected:Option<&str>)->Result<(Server,Box<bfe_transport::endpoint::Endpoint>),String>{
    let server=Server::read(address)?;
    if expected.is_some_and(|id|!id.eq_ignore_ascii_case(&server.id)){return Err("The server at the listed address is not the listed server. Refresh the list and try again.".into());}
    let files=bfe_content::files::DirFiles::new(root);
    let hash=bfe_content_native::fingerprint::hash(&files,&server.query)?;
    let endpoint=Box::new(bfe_transport::endpoint::Endpoint::connect(address,identity(path)?,server.identity()?,&Server::hello(&hash,name)).map_err(|e|e.to_string())?);
    Ok((server,endpoint))
}

// A listed server that names no direct address (relay policy "auto" or
// "always"): its information comes from the master's listing, and the paths to
// it from the master's signalling (hole punching, then the relayed address the
// server adds). `target` for later probes is "listed:<id>".
pub const LISTED:&str="listed:";
pub fn listed_server(master:&bfe_online::master::MasterClient,server_id:&str)->Result<(Server,bfe_online::browser::ServerEntry),String>{
    let listing=master.server_info(&server_id.to_ascii_lowercase()).map_err(|e|format!("The online service no longer lists this server ({e}). Refresh the list."))?;
    if !listing.id.eq_ignore_ascii_case(server_id){return Err("The online service listed another server under this identity. Refresh the list.".into());}
    let server=Server{id:listing.id.clone(),protocol:listing.protocol.unwrap_or(0) as usize,profile:contract::PROFILE.to_string(),simulation:listing.simulation.clone().unwrap_or_default(),
        content:listing.content.clone().unwrap_or_default(),query:listing.query.clone().unwrap_or_default(),name:listing.name.clone()};
    server.validate()?;Ok((server,listing))
}
// Rotation and rejoin probes: /info over HTTP for an address, the master's
// listing (renewed by the server's heartbeat every 20 s) for a listed server.
pub fn read_target(target:&str)->Result<Server,String>{
    let Some(id)=target.strip_prefix(LISTED)else{return Server::read(target);};
    let player=bfe_online::identity::PlayerIdentity::load_or_create(&default_identity()?)?;
    let master=bfe_online::master::MasterClient::new(bfe_online::master::resolve_master_url(),player);
    listed_server(&master,id).map(|(server,_)|server)
}
pub fn prepare_listed(server_id:&str,root:&Path,path:&Path,name:&str)->Result<(Server,Box<bfe_transport::endpoint::Endpoint>),String>{
    use bfe_online::rendezvous::{self as rv,Candidate,Signalling};
    use std::net::{SocketAddr,UdpSocket};
    // The game identity first: it creates the key file in the game's format.
    let transport=identity(path)?;
    let player=bfe_online::identity::PlayerIdentity::load_or_create(path)?;
    let master=bfe_online::master::MasterClient::new(bfe_online::master::resolve_master_url(),player);
    let (server,listing)=listed_server(&master,server_id)?;
    let id=listing.id.to_ascii_lowercase();
    if listing.relay_policy.as_deref()==Some("never"){return Err("This server is listed without an address. Refresh the list.".into());}
    // The relay the master names also answers STUN: this player's public address.
    let credentials=master.relay_credentials(Some(&id));
    if let Err(error)=&credentials{eprintln!("online join: no relay credentials ({error}); trying without them");}
    let stun_host=credentials.as_ref().ok().and_then(|c|rv::stun_from_turn_urls(&c.urls)).unwrap_or_else(||rv::DEFAULT_STUN.to_string());
    let stun=stun_host.to_socket_addrs().ok().and_then(|mut a|a.find(SocketAddr::is_ipv4));
    let v4=UdpSocket::bind("0.0.0.0:0").map_err(|e|format!("cannot open UDP socket: {e}"))?;
    let mut offered=Vec::with_capacity(4);
    let port=v4.local_addr().map_err(|e|e.to_string())?.port();
    if let Some(stun)=stun{
        if let Some(ip)=rv::local_address(stun){offered.push(Candidate{address:SocketAddr::new(ip,port),kind:"host"});}
        match rv::reflexive(&v4,stun,Duration::from_secs(3)){
            Some(mapped)=>{if !offered.iter().any(|c|c.address==mapped){offered.push(Candidate{address:mapped,kind:"srflx"});}},
            None=>eprintln!("online join: no STUN answer from {stun_host}"),
        }
    }else{eprintln!("online join: cannot resolve STUN server {stun_host}");}
    let mut sockets=vec![v4];
    // IPv6 when this machine has a global address (no NAT to open there).
    if let Ok(v6)=UdpSocket::bind("[::]:0"){
        let global=rv::local_address("[2001:4860:4860::8888]:53".parse().map_err(|_|"bad address")?).filter(|ip|matches!(ip,std::net::IpAddr::V6(v)if v.segments()[0]&0xe000==0x2000));
        if let (Some(ip),Ok(local))=(global,v6.local_addr()){offered.push(Candidate{address:SocketAddr::new(ip,local.port()),kind:"host"});}
        sockets.push(v6);
    }
    eprintln!("online join: offering {} candidates to {}",offered.len(),&id[..12]);
    let mut signalling=Signalling::open(&master).map_err(|e|format!("Cannot reach the online service to contact this server: {e}"))?;
    let nonce=rv::new_nonce()?;
    signalling.send(&id,"offer",&rv::offer_payload(&nonce,&offered))?;
    let mut nonces=vec![nonce];
    let deadline=Instant::now()+Duration::from_secs(10);let mut first=None;
    while first.is_none()&&Instant::now()<deadline{
        if let Some(text)=signalling.receive(Duration::from_millis(200))?{first=rv::parse_answer(&text,&id,&nonces);}
    }
    let Some(first)=first else{signalling.close();return Err("The server did not answer through the online service. It may have stopped or be busy; refresh the list and try again.".into());};
    eprintln!("online join: answer with {} candidates",first.len());
    let (found,candidates)=std::sync::mpsc::channel();let (start,started)=std::sync::mpsc::channel();
    for path in &first{let _=found.send(*path);}
    let alive=std::sync::Arc::new(());let watch=std::sync::Arc::downgrade(&alive);
    let keep=sockets[0].try_clone().map_err(|e|e.to_string())?;
    let hello_offered=offered.clone();
    std::thread::Builder::new().name("bfe-rendezvous".into()).spawn(move||{
        // Until the handshake starts (after the battle loads), keep this
        // player's router mapping open toward the server's addresses; then
        // offer again so the server punches again, and pass on what follows.
        let mut known:Vec<SocketAddr>=first;let begun=Instant::now();let mut next_keep=Instant::now();let mut renewed=false;
        while watch.upgrade().is_some()&&begun.elapsed()<Duration::from_secs(300){
            if !renewed&&started.try_recv().is_ok(){
                renewed=true;
                if let Ok(nonce)=rv::new_nonce(){if signalling.send(&id,"offer",&rv::offer_payload(&nonce,&hello_offered)).is_ok(){nonces.push(nonce);}}
            }
            if !renewed&&Instant::now()>=next_keep{
                next_keep=Instant::now()+Duration::from_secs(2);
                for path in known.iter().filter(|p|p.is_ipv4()){let _=keep.send_to(&[bfe_transport::layout::packet::PUNCH as u8,0,0,0,0,0,0,0,0],path);}
            }
            match signalling.receive(Duration::from_millis(200)){
                Ok(Some(text))=>if let Some(paths)=rv::parse_answer(&text,&id,&nonces){
                    for path in paths{if !known.contains(&path){eprintln!("online join: further candidate {path}");known.push(path);}if found.send(path).is_err(){break;}}
                },
                Ok(None)=>{},
                Err(error)=>{eprintln!("online join: {error}");break;},
            }
        }
        signalling.close();
    }).map_err(|e|e.to_string())?;
    let files=bfe_content::files::DirFiles::new(root);
    let hash=bfe_content_native::fingerprint::hash(&files,&server.query)?;
    let endpoint=bfe_transport::endpoint::Endpoint::rendezvous(sockets,transport,server.identity()?,&Server::hello(&hash,name),
        bfe_transport::endpoint::Rendezvous{candidates,start:Some(start),alive}).map_err(|e|e.to_string())?;
    Ok((server,Box::new(endpoint)))
}
