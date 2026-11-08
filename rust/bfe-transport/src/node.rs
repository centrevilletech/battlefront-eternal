//! In-process interoperability host. Only the fixture feature exposes these
//! allocating observers; the native device client uses Connection directly.
use napi::{bindgen_prelude::*,Error as NodeError};
use napi_derive::napi;
use crate::{connection::{Connection,Sink,Lane},crypto::Identity,handshake::{ClientHandshake,Event},layout::limits as lim};
use std::net::SocketAddr;
// Convert transport protocol errors into Node N-API exceptions for test assertions.
fn error(e:crate::Error)->NodeError{NodeError::from_reason(e.0)}
// Default synthetic loopback socket address for in-memory packet exchange fixtures.
fn address()->SocketAddr{SocketAddr::from(([127,0,0,1],18451))}
// Message and packet copies below belong solely to explicit test observers.
// They make ownership visible to JS assertions without lending live scratch.
#[napi(object)]
pub struct Delivery{pub lane:String,pub text:bool,pub bytes:Buffer}
#[napi(object)]
pub struct Packet{pub bytes:Buffer,pub path:String}
#[derive(Default)]
struct Observations{packets:Vec<Packet>,messages:Vec<Delivery>}
impl Sink for Observations{
    fn packet(&mut self,bytes:&[u8],path:SocketAddr){self.packets.push(Packet{bytes:bytes.to_vec().into(),path:path.to_string()});}
    fn message(&mut self,lane:Lane,text:bool,bytes:&[u8]){self.messages.push(Delivery{lane:match lane{Lane::State=>"state",Lane::Events=>"events",Lane::Bulk=>"bulk"}.into(),text,bytes:bytes.to_vec().into()});}
}
// In-memory client handshake and connection peer driven deterministically by Node tests.
#[napi]
pub struct NativePeer{handshake:ClientHandshake,connection:Option<Connection>,observed:Observations,id:String}
#[napi]
impl NativePeer{
    #[napi(constructor)]
    pub fn new(seed:Buffer,expected:String,hello:String)->napi::Result<Self>{
        let seed:[u8;32]=seed.as_ref().try_into().map_err(|_|NodeError::from_reason("identity seed must contain 32 bytes"))?;
        if expected.len()!=64||!expected.bytes().all(|b|b.is_ascii_hexdigit()){return Err(NodeError::from_reason("invalid server identity"));}
        let mut id=[0;32];for (i,n)in id.iter_mut().enumerate(){*n=u8::from_str_radix(&expected[i*2..i*2+2],16).map_err(|_|NodeError::from_reason("invalid server identity"))?;}
        let identity=Identity::from_seed(seed);let text=identity.id.iter().map(|n|format!("{n:02x}")).collect();
        Ok(Self{handshake:ClientHandshake::new(identity,id,hello.as_bytes()).map_err(error)?,connection:None,observed:Observations::default(),id:text})
    }
    #[napi]
    pub fn identity(&self)->String{self.id.clone()}
    #[napi]
    // The fixture controls retry delivery while the production handshake
    // owner retains session randomness and the original transcript bytes.
    pub fn initiation(&self,token:Option<Buffer>)->napi::Result<Buffer>{let mut bytes=[0;lim::INITIATION_BYTES];self.handshake.initiation(token.as_ref().map_or(&[],|b|b.as_ref()),&mut bytes).map_err(error)?;Ok(bytes.to_vec().into())}
    // Feed a handshake datagram and instantiate the Connection when RESPONSE succeeds.
    #[napi]
    pub fn handshake(&mut self,bytes:Buffer,now:f64,rtt:f64)->String{
        let event=self.handshake.receive(&bytes);if event==Event::Established{if let Some(keys)=self.handshake.take_keys(){self.connection=Some(Connection::new(keys,address(),now,rtt));}}
        match event{Event::Ignore=>"ignore",Event::Retry=>"retry",Event::Version(_)=>"version",Event::IdentityMismatch=>"identity",Event::Established=>"established",Event::Duplicate=>"duplicate"}.into()
    }
    #[napi]
    pub fn token(&self)->Buffer{self.handshake.token().to_vec().into()}
    #[napi]
    pub fn confirmation(&self)->Buffer{self.handshake.confirm().to_vec().into()}
    #[napi]
    pub fn server_hello(&self)->String{String::from_utf8_lossy(&self.handshake.server_hello).into_owned()}
    #[napi]
    // Tests can supply another path to verify source binding and migration.
    // A normal production endpoint obtains it directly from recv_from.
    pub fn feed(&mut self,bytes:Buffer,now:f64,path:Option<String>)->napi::Result<bool>{
        let path=match path{Some(p)=>p.parse().map_err(|_|NodeError::from_reason("invalid path"))?,None=>address()};
        let c=self.connection.as_mut().ok_or_else(||NodeError::from_reason("handshake is incomplete"))?;Ok(c.receive(&bytes,path,now,&mut self.observed))
    }
    // Queue an application message onto the named transport lane before flushing.
    #[napi]
    pub fn send(&mut self,lane:String,text:bool,bytes:Buffer)->napi::Result<bool>{
        let lane=match lane.as_str(){"state"=>Lane::State,"events"=>Lane::Events,"bulk"=>Lane::Bulk,_=>return Err(NodeError::from_reason("invalid lane"))};
        self.connection.as_mut().ok_or_else(||NodeError::from_reason("handshake is incomplete"))?.queue(lane,text,&bytes).map_err(error)
    }
    #[napi]
    pub fn poll(&mut self,now:f64)->napi::Result<()>{self.connection.as_mut().ok_or_else(||NodeError::from_reason("handshake is incomplete"))?.poll(now,&mut self.observed).map_err(error)}
    #[napi]
    pub fn flush(&mut self,now:f64)->napi::Result<()>{self.connection.as_mut().ok_or_else(||NodeError::from_reason("handshake is incomplete"))?.flush(now,&mut self.observed).map_err(error)}
    #[napi]
    pub fn close(&mut self,code:u32,reason:String,now:f64)->napi::Result<()>{let c=self.connection.as_mut().ok_or_else(||NodeError::from_reason("handshake is incomplete"))?;c.close(code as u64,reason.as_bytes());c.flush(now,&mut self.observed).map_err(error)}
    #[napi]
    // Draining observations cannot alter connection recovery bookkeeping.
    // Lost packet tests decide which captured bytes the peer will receive.
    pub fn packets(&mut self)->Vec<Packet>{std::mem::take(&mut self.observed.packets)}
    #[napi]
    pub fn messages(&mut self)->Vec<Delivery>{std::mem::take(&mut self.observed.messages)}
    #[napi]
    // A shortened fixture interval exercises rekeying without millions of
    // packets. The executable always uses the protocol interval.
    pub fn key_interval(&mut self,n:u32){if let Some(c)=&mut self.connection{c.fixture_key_interval(n);}}
    #[napi]
    // Diagnostics serialize only when requested by an assertion. Live packet
    // counters are scalars and never build this object on each delivery.
    pub fn stats(&self)->String{if let Some(c)=&self.connection{serde_json::json!({"sent":c.stats.sent,"received":c.stats.received,"lost":c.stats.lost,"retransmitted":c.stats.retransmitted,"duplicates":c.stats.duplicates,"authFailures":c.stats.auth_failures,"backlog":c.backlog(),"handshakeConfirmed":c.handshake_confirmed,"path":c.path.to_string(),"closed":c.closed.map(|v|serde_json::json!({"code":v.code,"reason":String::from_utf8_lossy(v.reason()),"remote":v.remote}))}).to_string()}else{"{}".into()}}
}

// The socket fixture drives the actual endpoint while the JS server's event
// loop remains free to answer. Blocking establishment belongs to a separate
// native application process, never to this in-process observer.
#[napi]
pub struct NativeSocket{endpoint:Option<crate::endpoint::Endpoint>,observed:Observations}
#[napi]
impl NativeSocket{
    #[napi(constructor)]
    pub fn new(address:String,seed:Buffer,expected:String,hello:String)->napi::Result<Self>{
        let seed:[u8;32]=seed.as_ref().try_into().map_err(|_|NodeError::from_reason("identity seed must contain 32 bytes"))?;
        if expected.len()!=64||!expected.bytes().all(|b|b.is_ascii_hexdigit()){return Err(NodeError::from_reason("invalid server identity"));}
        let mut id=[0;32];for (i,n)in id.iter_mut().enumerate(){*n=u8::from_str_radix(&expected[i*2..i*2+2],16).map_err(|_|NodeError::from_reason("invalid server identity"))?;}
        let endpoint=crate::endpoint::Endpoint::connect(&address,Identity::from_seed(seed),id,hello.as_bytes()).map_err(error)?;
        Ok(Self{endpoint:Some(endpoint),observed:Observations::default()})
    }
    // Non-blocking poll turn that drains OS UDP sockets and collects delivered messages.
    #[napi]
    pub fn poll(&mut self)->napi::Result<bool>{
        let endpoint=self.endpoint.as_mut().ok_or_else(||NodeError::from_reason("socket is closed"))?;let observed=&mut self.observed;
        endpoint.pump(&mut|lane,text,bytes,_|observed.message(lane,text,bytes)).map_err(error)?;Ok(endpoint.ready())
    }
    #[napi]
    pub fn send(&mut self,lane:String,text:bool,bytes:Buffer)->napi::Result<bool>{
        let lane=match lane.as_str(){"state"=>Lane::State,"events"=>Lane::Events,"bulk"=>Lane::Bulk,_=>return Err(NodeError::from_reason("invalid lane"))};
        self.endpoint.as_mut().ok_or_else(||NodeError::from_reason("socket is closed"))?.send(lane,text,&bytes).map_err(error)
    }
    #[napi]
    pub fn messages(&mut self)->Vec<Delivery>{std::mem::take(&mut self.observed.messages)}
    #[napi]
    // Closing the observer still emits the real protected close packet.
    // Dropping test storage never substitutes for protocol shutdown.
    pub fn dispose(&mut self){if let Some(mut e)=self.endpoint.take(){e.close(0,b"disconnected");let _=e.pump(&mut|_,_,_,_|{});}}
}

// The recorded-session observer uses the same Session as the native display
// client. These explicit captures allocate only in the replay harness; neither
// live packet delivery nor a displayed frame reconstructs JSON entity rows.
#[napi]
pub struct NativeSnapshot{owner:bfe_presentation::network::Session}
#[napi]
impl NativeSnapshot{
    #[napi(constructor)]
    pub fn new()->Self{let mut owner=bfe_presentation::network::Session::new();owner.reserve_receive();owner.reset(0.);Self{owner}}
    // Decode a binary snapshot/delta datagram against the caller's world bounds.
    #[napi]
    pub fn receive(&mut self,bytes:Buffer,bounds:Float64Array)->napi::Result<Float64Array>{
        let bounds:[f64;6]=bounds.as_ref().try_into().map_err(|_|NodeError::from_reason("invalid snapshot bounds"))?;
        self.owner.receive(&bytes,&bounds).map_err(NodeError::from_reason)?;Ok(self.owner.header.clone().into())
    }
    #[napi]
    // Capture retains baseline insertion order and opaque payload bytes.
    // The recorded corpus hashes those facts independently of interpolation.
    pub fn capture(&self,tick:f64)->String{self.owner.capture(tick).to_string()}
    #[napi]
    // Publications inspect only the current changed-record lease. Taking a
    // cold snapshot above neither consumes that lease nor samples its timeline.
    pub fn publication(&self)->String{self.owner.capture_publication().to_string()}
}
// Timeline rows cross this fixture boundary in generated layout order. The
// production client instead borrows Session.timeline directly, with no copy.
#[napi]
pub struct NativeTimeline{owner:bfe_pose::timeline::Timeline,hz:f64,extrapolate:f64}
#[napi]
impl NativeTimeline{
    #[napi(constructor)]
    pub fn new(hz:f64,extrapolate:f64)->Self{Self{owner:bfe_pose::timeline::Timeline::new(),hz,extrapolate}}
    #[napi]
    pub fn reset(&mut self,epoch:f64){self.owner.reset(epoch);}
    #[napi]
    pub fn remove(&mut self,id:f64){self.owner.remove(id);}
    // Copy a packed timeline frame into the native pose ring and advance its cursor.
    #[napi]
    pub fn push(&mut self,rows:Float64Array)->napi::Result<Float64Array>{
        if rows.len()!=self.owner.input.len(){return Err(NodeError::from_reason("invalid timeline rows"));}
        self.owner.input.copy_from_slice(&rows);self.owner.push();Ok(self.owner.input.clone().into())
    }
    #[napi]
    // Generated output rows expose one batch to the replay harness. Live
    // presentation consumes the identical batch without this test-only copy.
    pub fn sample(&mut self,tick:f64)->Float64Array{
        use bfe_pose::timeline_layout::{timeline_frame as f,VERSION};
        self.owner.input[f::VERSION]=VERSION as f64;self.owner.input[f::TICK]=tick;self.owner.input[f::HZ]=self.hz;self.owner.input[f::EXTRAPOLATE]=self.extrapolate;self.owner.present();self.owner.output.clone().into()
    }
}
