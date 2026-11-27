//! Direct joins read bounded public metadata before signing a game hello.
//! Gameplay policy remains in bfeserver; the client checks its own content and
//! the engine's generated contract independently of the advertised digest.
use std::{io::{Read,Write},net::{TcpStream,ToSocketAddrs},path::{Path,PathBuf},time::{Duration,Instant}};
use serde::Deserialize;
use bfe_content::GameFamily;
use bfe_transport::{crypto::{Identity,random},layout::network_contract as contract};
use bfe_online::declared::Statements;
#[derive(Deserialize)]
pub struct Server{
    pub id:String,pub protocol:usize,pub profile:String,pub simulation:String,pub content:String,pub query:String,
    #[serde(default)]pub name:String,
    /// Set while the battle is over and the next one is announced (the end table).
    #[serde(default)]pub next:Option<NextBattle>,
    /// What the reply names beside its query, read by a pass of its own over the reply's
    /// bytes (`Server::parse`; see `selection`), never by this reader.
    #[serde(skip)]pub statements:Statements,
}
pub use bfe_online::declared::NextBattle;
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
        let body=response(&bytes)?;let info=Server::parse(&body)?;
        info.validate()?;Ok(info)
    }
    /// A reply's bytes as the released reader reads them, and what the reply names beside its
    /// query, read from the same bytes by a pass of its own that cannot fail.
    pub fn parse(body:&[u8])->Result<Self,String>{
        let mut info:Server=serde_json::from_slice(body).map_err(|e|format!("invalid server information: {e}"))?;
        info.statements=Statements::read(body);Ok(info)
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
    /// Hash the client's prepared files for the active battle advertised by this session.
    pub fn local_fingerprint(&self, root: &Path, family: GameFamily) -> Result<String, String> {
        if family == GameFamily::Swbf2 {
            let selected = bfe_content::mission::Query::parse(&self.query);
            if selected.get("family") != family.as_str() || selected.get("map").is_empty()
                || selected.get("mission").is_empty() {
                return Err("the server did not select a prepared 2005 mission".into());
            }
        }
        let files = bfe_content::files::DirFiles::from_resolved_root(root, family);
        bfe_content_native::fingerprint::hash_for_family(&files, family, &self.query)
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
    #[test]
    fn scripted_session_requires_its_explicit_family_map_and_mission() {
        let mut server = super::Server {
            id: String::new(), protocol: 0, profile: String::new(), simulation: String::new(),
            content: "advertised".into(), query: String::new(), name: String::new(), next: None,
            statements: Default::default(),
        };
        for query in ["?map=vale&mission=dawn", "?family=swbf2&mission=dawn", "?family=swbf2&map=vale",
            "?family=swbf1&family=swbf2&map=vale&mission=dawn"] {
            server.query = query.into();
            assert_eq!(server.local_fingerprint(std::path::Path::new(""), bfe_content::GameFamily::Swbf2),
                Err("the server did not select a prepared 2005 mission".into()));
        }
    }

    use super::*;
    fn answer(from:&str,kind:&str,payload:serde_json::Value)->String{serde_json::json!({"from":from,"type":kind,"payload":payload.to_string()}).to_string()}
    #[test]fn a_listed_join_loads_the_battle_the_server_names_not_a_stale_listing(){
        let id="ab".repeat(32);let nonces=vec!["1".repeat(32)];
        let stale="?map=nab2&side=all&match=00000000000000000000000000000000";
        let running="?map=tat1&side=rep&match=11111111111111111111111111111111";
        let text=answer(&id,"answer",serde_json::json!({"udp":1,"nonce":nonces[0],"candidates":[],"battle":running}));
        let named=answered_battle(&text,&id,&nonces).unwrap();
        assert_eq!(battle_to_load(stale,&named).as_deref(),Some(running));
        // Between battles: wait for the announced one.
        let text=answer(&id,"candidate",serde_json::json!({"udp":1,"nonce":nonces[0],"candidates":[],"battle":stale,"next":running}));
        assert_eq!(battle_to_load(stale,&answered_battle(&text,&id,&nonces).unwrap()),None);
        // An older server names no battle: the listing's stays.
        let text=answer(&id,"answer",serde_json::json!({"udp":1,"nonce":nonces[0],"candidates":[]}));
        assert_eq!(battle_to_load(stale,&answered_battle(&text,&id,&nonces).unwrap()).as_deref(),Some(stale));
        // Not an answer to our offer, or not from this server.
        assert!(answered_battle(&answer(&id,"answer",serde_json::json!({"nonce":"2".repeat(32),"battle":running})),&id,&nonces).is_none());
        assert!(answered_battle(&answer(&"cd".repeat(32),"answer",serde_json::json!({"nonce":nonces[0],"battle":running})),&id,&nonces).is_none());
    }
    #[test]fn a_refusal_is_judged_by_the_battle_the_server_names(){
        // The client loaded the stale battle the list named and was refused. The listing still
        // names that battle, the server names another: that is a battle change to follow
        // (the rejoin probe's rule is "changed when the server's query differs from the loaded").
        let loaded="?map=nab2&side=all&match=00000000000000000000000000000000";
        let mut server:Server=serde_json::from_value(serde_json::json!({"id":"ab".repeat(32),"protocol":3,"profile":"p","simulation":"s","content":"c","query":loaded})).unwrap();
        server.take_answer(AnsweredBattle{battle:Some("?map=tat1&match=11111111111111111111111111111111".into()),next:None});
        assert_ne!(server.query,loaded);
        // The server names the loaded battle itself: a real content mismatch.
        server.take_answer(AnsweredBattle{battle:Some(loaded.into()),next:None});
        assert_eq!(server.query,loaded);
    }
    // Between battles a signalling answer names two battles; both are this build's to join
    // (a 2005 battle is played or sat out), or the join is refused before the wait.
    #[test]fn both_battles_of_an_answer_are_checked(){
        let listing="?map=tat2&side=all&match=00000000000000000000000000000000";
        let server:Server=serde_json::from_value(serde_json::json!({"id":"ab".repeat(32),"protocol":3,"profile":"p","simulation":"s","content":"c","query":listing})).unwrap();
        let supported="?map=tat2&side=all&match=11111111111111111111111111111111";
        let qualified="?map=swbf2/tat2&side=all&match=11111111111111111111111111111111";
        let answer=|battle:&str,next:&str|AnsweredBattle{battle:Some(battle.into()),next:Some(next.into())};
        assert_eq!(server.check_answer(&answer(listing,supported)),Ok(()));
        assert_eq!(server.check_answer(&answer(qualified,supported)),Ok(()));
        let padded = format!("?map={}swbf2%2Fmoon", "+".repeat(40));
        assert_eq!(server.check_answer(&answer(&padded,supported)),Ok(()));
        for current in ["?map=mods/one", "?map=swbf1/one"] {
            assert!(server.check_answer(&answer(current,supported)).is_ok());
        }
        assert_eq!(server.check_answer(&answer(supported,qualified)),Ok(()));
        assert!(server.check_answer(&answer(supported,"?map=swbf9/tat2&match=11111111111111111111111111111111")).is_err());
        // The listing's statements stand for the listing's own battle only.
        let mut listed=server;listed.statements=Statements::read(br#"{"battle":{"family":"swbf9"}}"#);
        assert!(listed.check_answer(&answer(listing,supported)).is_err());
        assert_eq!(listed.check_answer(&answer(supported,supported)),Ok(()));
    }
    // A listed server is ready for the next battle only once a listing has named this
    // build's version: a server that names the match while its listing is missing or
    // unreadable is never ready, and another version ends the wait at once.
    #[test]fn listed_readiness_needs_a_listing_that_names_this_version(){
        use super::super::rotation::Answer;
        // The server names the match at every probe; the listing answers in turn.
        let wait=|answers:Vec<Listed>|{
            let stop=std::sync::atomic::AtomicBool::new(false);
            let mut answers=answers.into_iter();
            listed_match_wait(&stop,&mut |_|answers.next(),&mut ||Some(true),Duration::from_millis(300))
        };
        // No listing, or none that can be read, throughout: never ready.
        assert_eq!(wait(vec![Listed::Unknown;8]),Answer::Decided(false));
        assert_eq!(wait(Vec::new()),Answer::Decided(false));
        // A listing with this build's version, before or after the server named the match: ready.
        assert_eq!(wait(vec![Listed::Unknown,Listed::Same]),Answer::Decided(true));
        assert_eq!(wait(vec![Listed::Same]),Answer::Decided(true));
        // Another version: final.
        let other=version_mismatch("bfe-sim-1");
        assert_eq!(wait(vec![Listed::Other(other.clone())]),Answer::Mismatch(other));
        // A server that does not name the match is not ready, whatever its listing says.
        let stop=std::sync::atomic::AtomicBool::new(false);
        assert_eq!(listed_match_wait(&stop,&mut |_|Some(Listed::Same),&mut ||Some(false),Duration::from_millis(300)),Answer::Decided(false));
    }
    #[test]fn the_next_battle_is_read_from_the_end_table_information(){
        let base=r#""id":"x","protocol":3,"profile":"p","simulation":"s","content":"c","query":"?map=nab2""#;
        let during:Server=serde_json::from_str(&format!(r#"{{{base},"next":{{"query":"?map=hot1&side=imp","delayMs":4200}}}}"#)).unwrap();
        assert_eq!(during.next.map(|n|n.query).as_deref(),Some("?map=hot1&side=imp"));
        let playing:Server=serde_json::from_str(&format!("{{{base}}}")).unwrap();
        assert!(playing.next.is_none());
    }
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
        let (server,_,mut endpoint)=super::prepare_listed(&id,Path::new(&content),&key,"PROBE",bfe_family::Gate::BUILT,&mut |_|{}).expect("prepare");
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
// A battle the reply names that this build will not join is a `Refused` error: final for
// the match, as another address of the same server names the same battle. `gate` decides
// whether a 2005 battle is played or sat out.
pub fn prepare(address:&str,root:&Path,path:&Path,name:&str,expected:Option<&str>,gate:bfe_family::Gate)->Result<(Server,super::selection::Selection,Box<bfe_transport::endpoint::Endpoint>),PrepareError>{
    let server=Server::read(address)?;
    if expected.is_some_and(|id|!id.eq_ignore_ascii_case(&server.id)){return Err("The server at the listed address is not the listed server. Refresh the list and try again.".to_string().into());}
    // The battle's game, and the next one's while it is announced, before any content.
    let mut selection=server.selection().map_err(PrepareError::Refused)?;
    let hello=join_hello(root,&server,&mut selection,name,gate)?;
    let endpoint=Box::new(bfe_transport::endpoint::Endpoint::connect(address,identity(path)?,server.identity()?,&hello).map_err(|e|e.to_string())?);
    Ok((server,selection,endpoint))
}

/// The hello of a join, for the battle the server's fresh reply names. A 2004
/// battle sends the released hello with the released fingerprint of its query.
/// A 2005 match sends the hello that names the match (its mode, qualified map
/// and token), the record revisions this build reads and the fingerprint of its
/// 2005 content folder, so that the server gives the battle role, or no
/// fingerprint, for the sit-out role: a client whose gate does not open the
/// 2005 game, or that lacks the map, reads no 2005 content and sits the battle
/// out. The selection keeps that match for the check of the signed answer.
pub fn join_hello(root:&Path,server:&Server,selection:&mut super::selection::Selection,name:&str,gate:bfe_family::Gate)->Result<Vec<u8>,String>{
    use bfe_family::GameFamily;
    if selection.family==GameFamily::Swbf1{
        let files=bfe_content::files::DirFiles::new(root,GameFamily::Swbf1);
        let hash=bfe_content_native::fingerprint::hash(&files,&server.query)?;
        return Ok(Server::hello(&hash,name));
    }
    let query=bfe_family::swbf2::content_query(&server.query,&selection.map).ok_or("invalid Battlefront II battle query")?;
    let folder=GameFamily::Swbf2.content_dir(root);
    // The digest offered is computed once, from files read once: the scene is
    // later loaded from those same bytes (`Selection::pinned`).
    let content=match gate.plays(GameFamily::Swbf2)&&folder.join("maps").join(&selection.map).join("map.json").is_file(){
        true=>{
            let disk=bfe_content::files::DirFiles::from_resolved_root(&folder,GameFamily::Swbf2);
            let recorder=bfe_content::files::Recorder::new(&disk);
            let digest=bfe_content_native::fingerprint::hash_for_family(&recorder,GameFamily::Swbf2,&query)?;
            selection.pinned=Some(std::sync::Arc::new(recorder.pinned()));
            Some(digest)
        }
        false=>None,
    };
    let token=bfe_content::mission::Query::parse(&server.query).get("match").to_string();
    let expected=super::match2005::Expected{mode:selection.mode.clone(),map:GameFamily::Swbf2.qualify(&selection.map),token,content};
    let hello=super::match2005::hello(&expected,name);
    selection.join=Some(expected);
    Ok(hello)
}

/// Why `prepare` stopped.
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum PrepareError{
    /// The reply names a battle this build will not join: a game it cannot play or does not
    /// know, two games, or a next battle it cannot load. Final for the match.
    Refused(String),
    /// Anything else (`transient` and `unreachable` tell what): another address may answer.
    Failed(String),
}
impl From<String> for PrepareError{fn from(error:String)->Self{Self::Failed(error)}}
impl std::fmt::Display for PrepareError{
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{match self{Self::Refused(why)|Self::Failed(why)=>f.write_str(why)}}
}
// A listed server that names no direct address (relay policy "auto" or
// "always"): its information comes from the master's listing, and the paths to
// it from the master's signalling (hole punching, then the relayed address the
// server adds). `target` for later probes is "listed:<id>".
pub const LISTED:&str="listed:";
pub fn listed_server(master:&bfe_online::master::MasterClient,server_id:&str)->Result<(Server,bfe_online::browser::ServerEntry),String>{
    let listing=master.server_info(&server_id.to_ascii_lowercase()).map_err(|e|format!("The online service no longer lists this server ({e}). Refresh the list."))?;
    if !listing.id.eq_ignore_ascii_case(server_id){return Err("The online service listed another server under this identity. Refresh the list.".into());}
    let statements=listing.battle.as_ref().map(|battle|Statements{
        stated:battle.family.clone(),mode:Some(battle.mode.clone())
    }).unwrap_or_default();
    let server=Server{id:listing.id.clone(),protocol:listing.protocol.unwrap_or(0) as usize,profile:contract::PROFILE.to_string(),simulation:listing.simulation.clone().unwrap_or_default(),
        content:listing.content.clone().unwrap_or_default(),query:listing.query.clone().unwrap_or_default(),name:listing.name.clone(),next:None,statements};
    server.validate()?;Ok((server,listing))
}
// The battle a server's answer to our offer names ("battle"), and the next one while it is
// announced ("next"), when the answer is to one of our offers. A server that names none (an
// older one) leaves the listing's battle in force.
#[derive(Debug,Default,PartialEq)]
pub struct AnsweredBattle{pub battle:Option<String>,pub next:Option<String>}
pub fn answered_battle(text:&str,server_id:&str,nonces:&[String])->Option<AnsweredBattle>{
    #[derive(Deserialize)]struct Envelope{#[serde(default)]from:String,#[serde(rename="type",default)]kind:String,#[serde(default)]payload:String}
    #[derive(Deserialize)]struct Payload{#[serde(default)]nonce:String,#[serde(default)]battle:Option<String>,#[serde(default)]next:Option<String>}
    let envelope:Envelope=serde_json::from_str(text).ok()?;
    if !envelope.from.eq_ignore_ascii_case(server_id)||!matches!(envelope.kind.as_str(),"answer"|"candidate"){return None;}
    let payload:Payload=serde_json::from_str(&envelope.payload).ok()?;
    nonces.contains(&payload.nonce).then_some(AnsweredBattle{battle:payload.battle.filter(|q|!q.is_empty()),next:payload.next.filter(|q|!q.is_empty())})
}
// What a listed join loads: the battle the server names, never the listing's when they differ
// (the listing lags a battle change by a minute or more); None while the next battle is
// announced, to wait for it.
pub fn battle_to_load(listing:&str,answer:&AnsweredBattle)->Option<String>{
    if answer.next.is_some(){return None;}
    Some(answer.battle.clone().unwrap_or_else(||listing.to_string()))
}
// Asks a listed server through the master's signalling which battle it runs: one offer
// without candidates, its answer read for "battle". None when the server names none.
fn listed_battle(master:&bfe_online::master::MasterClient,server_id:&str)->Result<Option<AnsweredBattle>,String>{
    use bfe_online::rendezvous::{self as rv,Signalling};
    let id=server_id.to_ascii_lowercase();
    let mut signalling=Signalling::open(master)?;
    let nonce=rv::new_nonce()?;signalling.send(&id,"offer",&rv::offer_payload(&nonce,&[]))?;
    let nonces=[nonce];let by=Instant::now()+Duration::from_secs(3);let mut answer=None;
    while answer.is_none()&&Instant::now()<by{
        match signalling.receive(Duration::from_millis(200)){
            Ok(Some(text))=>answer=answered_battle(&text,&id,&nonces),
            Ok(None)=>{},
            Err(_)=>break,
        }
    }
    signalling.close();
    Ok(answer.filter(|a|a.battle.is_some()))
}
// Rotation and rejoin probes: /info over HTTP for an address; for a listed server its
// listing, with the battle the server itself names through the master's signalling (the
// listing lags a battle change, so a refusal must not be judged by it).
pub fn read_target(target:&str)->Result<Server,String>{
    let Some(id)=target.strip_prefix(LISTED)else{return Server::read(target);};
    let player=bfe_online::identity::PlayerIdentity::load_or_create(&default_identity()?)?;
    let master=bfe_online::master::MasterClient::new(bfe_online::master::resolve_master_url(),player);
    let (mut server,_)=listed_server(&master,id)?;
    if let Some(answer)=listed_battle(&master,id)?{server.take_answer(answer);}
    Ok(server)
}
impl Server{
    // The battle the server itself named replaces the listing's. The listing's summary and
    // rotation then describe another battle, so they no longer name its game.
    fn take_answer(&mut self,answer:AnsweredBattle){
        if let Some(battle)=answer.battle{
            if battle!=self.query{self.forget_listing();}
            self.query=battle;
        }
        self.next=answer.next.map(|query|NextBattle{query});
    }
    fn forget_listing(&mut self){
        self.statements=Statements::default();
    }
    /// The battle this reply names, resolved once before any content is read; refused for a
    /// game this build does not know, two games, or an announced next battle it cannot load.
    pub fn selection(&self)->Result<super::selection::Selection,String>{
        super::selection::select_reply(&self.query,&self.statements,self.next.as_ref().map(|n|n.query.as_str()))
    }
    // Both battles a signalling answer names while the server is between battles, checked as
    // `selection` checks a reply: the one it runs (with the listing's statements only when it
    // is the listing's battle) and the next.
    fn check_answer(&self,answer:&AnsweredBattle)->Result<(),String>{
        let query = answer.battle.as_deref().unwrap_or(&self.query);
        let empty = Statements::default();
        let statements = if query == self.query { &self.statements } else { &empty };
        statements.family(query, answer.next.as_deref()).map(drop)
    }
}
/// What a server's listing says of its version: this build's (the listing
/// read and validated), another (final), or nothing to go by: no listing, or
/// one that cannot be read.
#[derive(Clone,Debug,PartialEq,Eq)]
pub enum Listed{Same,Other(String),Unknown}
fn listed_version(master:&bfe_online::master::MasterClient,id:&str)->Listed{
    match listed_server(master,id){Ok(_)=>Listed::Same,Err(text)if text.starts_with("This server runs ")=>Listed::Other(text),Err(_)=>Listed::Unknown}
}
// Asks a listed server through the master's signalling, about once a second for up to two
// minutes, whether it already runs the match `token` names. Its listing may not carry the battle
// query (the master drops queries it does not know), so the listing cannot tell which battle;
// the signalling answer carries no version, so the listing's version contract is compared with
// this build's every five seconds: another version is final at once, and the server is ready
// only once a listing has named this build's version. Without one it is never ready.
pub fn listed_match_ready(server_id:&str,token:&str,stop:&std::sync::atomic::AtomicBool)->super::rotation::Answer{
    use super::rotation::Answer;
    let id=server_id.to_ascii_lowercase();
    let Ok(path)=default_identity()else{return Answer::Decided(false);};
    let _=identity(&path);
    let Ok(player)=bfe_online::identity::PlayerIdentity::load_or_create(&path)else{return Answer::Decided(false);};
    let master=bfe_online::master::MasterClient::new(bfe_online::master::resolve_master_url(),player);
    let mut listed:Option<Instant>=None;
    listed_match_poll(server_id,token,stop,&master,&mut |now|{
        if !now&&listed.is_some_and(|at|at.elapsed()<Duration::from_secs(5)){return None;}
        listed=Some(Instant::now());Some(listed_version(&master,&id))
    })
}
// `version(now)` reads the listing's version when it is due, or at once when `now`; None when
// it is not due.
fn listed_match_poll(server_id:&str,token:&str,stop:&std::sync::atomic::AtomicBool,master:&bfe_online::master::MasterClient,
    version:&mut dyn FnMut(bool)->Option<Listed>)->super::rotation::Answer{
    use bfe_online::rendezvous::{self as rv,Signalling};
    let id=server_id.to_ascii_lowercase();
    let mut signalling:Option<Signalling>=None;
    let answer=listed_match_wait(stop,version,&mut ||{
        if signalling.is_none(){signalling=Signalling::open(master).ok();}
        let Some(socket)=signalling.as_mut()else{std::thread::sleep(Duration::from_secs(1));return Some(false);};
        let Ok(nonce)=rv::new_nonce()else{return None;};
        if socket.send(&id,"matchProbe",&rv::match_probe_payload(token,&nonce)).is_err(){signalling=None;return Some(false);}
        let answer_by=Instant::now()+Duration::from_secs(1);let (mut ready,mut lost)=(false,false);
        while Instant::now()<answer_by&&!ready{
            match socket.receive(Duration::from_millis(100)){
                Ok(Some(text))=>ready=rv::is_match_ready(&text,&id,token,&nonce),
                Ok(None)=>{},
                Err(_)=>{lost=true;break;},
            }
        }
        if lost{signalling=None;}
        Some(ready)
    },Duration::from_secs(120));
    if let Some(s)=signalling.take(){s.close();}
    answer
}
// The wait for a listed server's next battle, until `timeout`: `version(now)` as above, and
// `named()` asks the server once whether it runs the match (None when it cannot be asked). The
// server is ready once it named the match and a listing named this build's version.
fn listed_match_wait(stop:&std::sync::atomic::AtomicBool,version:&mut dyn FnMut(bool)->Option<Listed>,
    named:&mut dyn FnMut()->Option<bool>,timeout:Duration)->super::rotation::Answer{
    use super::rotation::Answer;use std::sync::atomic::Ordering;
    let deadline=Instant::now()+timeout;
    let (mut same,mut ready)=(false,false);
    while Instant::now()<deadline&&!stop.load(Ordering::Acquire){
        match version(ready&&!same){
            Some(Listed::Other(text))=>return Answer::Mismatch(text),
            Some(Listed::Same)=>same=true,
            Some(Listed::Unknown)|None=>{}
        }
        if ready&&same{eprintln!("online join: the server runs the next battle");return Answer::Decided(true);}
        if ready{std::thread::sleep(Duration::from_millis(200));continue;}
        match named(){Some(answer)=>ready=answer,None=>return Answer::Decided(false)}
    }
    Answer::Decided(false)
}
// `on_battle` hears the battle the server names (the next one while it is announced), for the
// loading screen to show.
pub fn prepare_listed(server_id:&str,root:&Path,path:&Path,name:&str,gate:bfe_family::Gate,on_battle:&mut dyn FnMut(&str))->Result<(Server,super::selection::Selection,Box<bfe_transport::endpoint::Endpoint>),String>{
    use bfe_online::rendezvous::{self as rv,Candidate,Signalling};
    use std::net::{SocketAddr,UdpSocket};
    // The game identity first: it creates the key file in the game's format.
    let transport=identity(path)?;
    let player=bfe_online::identity::PlayerIdentity::load_or_create(path)?;
    let master=bfe_online::master::MasterClient::new(bfe_online::master::resolve_master_url(),player);
    let (mut server,listing)=listed_server(&master,server_id)?;
    let id=listing.id.to_ascii_lowercase();
    if listing.relay_policy.as_deref()==Some("never"){return Err("This server is listed without an address. Refresh the list.".into());}
    // A rotation or query in the directory can predate the current battle. Only a game this
    // build does not know, or two games, end the join here: a 2005 battle is played or sat
    // out once the server names it, as the gate decides.
    if !matches!(listing.family_reading(),bfe_online::browser::FamilyReading::Known(_)){listing.declared_family()?;}
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
    // The offer is sent again every 2.5 s until an answer: a server whose signalling socket is
    // reconnecting (as at a battle change) drops what arrives in between.
    // Between two battles the server is briefly unregistered and the master closes a socket that
    // offers to it: open a new one and offer again.
    // The answer also names the battle the server runs: that one is loaded, not the listing's.
    // While it names a next battle (end table), offer again until the server runs that one.
    let mut deadline=Instant::now()+Duration::from_secs(10);let mut first=None;let mut resend=Instant::now()+Duration::from_millis(2500);
    let mut between:Option<Instant>=None;
    while first.is_none()&&Instant::now()<deadline{
        let mut lost=false;
        if Instant::now()>=resend{
            resend=Instant::now()+Duration::from_millis(2500);
            let nonce=rv::new_nonce()?;lost=signalling.send(&id,"offer",&rv::offer_payload(&nonce,&offered)).is_err();nonces.push(nonce);
            eprintln!("online join: no answer yet; offer sent again");
        }
        match signalling.receive(Duration::from_millis(200)){
            Ok(Some(text))=>{
                first=rv::parse_answer(&text,&id,&nonces);
                if first.is_some(){
                    let answer=answered_battle(&text,&id,&nonces).unwrap_or_default();
                    match battle_to_load(&server.query,&answer){
                        Some(battle)=>{
                            if battle!=server.query{eprintln!("online join: the list names another battle; loading the one the server runs");server.forget_listing();}
                            server.query=battle;
                        }
                        None=>{
                            // A battle this build will not join is refused now, not after the wait:
                            // the one the server runs as well as the next.
                            if let Err(error)=server.check_answer(&answer){signalling.close();return Err(error);}
                            // The battle is over and the next announced: wait for it (up to 90 s).
                            if between.is_none(){if let Some(next)=&answer.next{on_battle(next);}}
                            let since=*between.get_or_insert_with(||{eprintln!("online join: the server is between battles; waiting for the next one");Instant::now()});
                            if since.elapsed()<Duration::from_secs(90){
                                first=None;deadline=Instant::now()+Duration::from_secs(10);resend=Instant::now()+Duration::from_millis(1000);
                            }else if let Some(next)=answer.next{server.forget_listing();server.query=next;}
                        }
                    }
                }
            },
            Ok(None)=>{},
            Err(error)=>{eprintln!("online join: {error}");lost=true;},
        }
        if lost{
            std::thread::sleep(Duration::from_millis(500));
            if let Ok(again)=Signalling::open(&master){signalling=again;resend=Instant::now();}
        }
    }
    let Some(first)=first else{signalling.close();return Err("The server did not answer through the online service. It may have stopped or be busy; refresh the list and try again.".into());};
    // The battle the server answered with, before the keepalive, the content or the endpoint.
    let mut selection=match server.selection(){Ok(selection)=>selection,Err(error)=>{signalling.close();return Err(error);}};
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
    let hello=join_hello(root,&server,&mut selection,name,gate)?;
    let endpoint=bfe_transport::endpoint::Endpoint::rendezvous(sockets,transport,server.identity()?,&hello,
        bfe_transport::endpoint::Rendezvous{candidates,start:Some(start),alive}).map_err(|e|e.to_string())?;
    Ok((server,selection,Box::new(endpoint)))
}
