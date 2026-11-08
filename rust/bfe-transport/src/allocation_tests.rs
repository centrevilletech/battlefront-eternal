//! First-use allocation accounting includes protection, receive, stream
//! assembly, ACKs, loss recovery and key rotation. No fixture observer copies
//! a message or grows a queue while the protocol reservation is measured.
use crate::{
    connection::{Closed, Connection, Lane, Sink},
    crypto::{Aead, Identity, RetryTokens},
    handshake::{ClientHandshake, Event, Keys},
    layout::limits as lim,
    server::{AdmitDecision, ClientInfo, ServerEndpoint, ServerSink},
};
use std::{alloc::{GlobalAlloc,Layout,System},cell::Cell,net::SocketAddr};
thread_local!{static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
struct Counting;
unsafe impl GlobalAlloc for Counting{
 unsafe fn alloc(&self,l:Layout)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|n|n.set(n.get()+1));}System.alloc(l)}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|n|n.set(n.get()+1));}System.realloc(p,l,n)}
}
#[global_allocator]static ALLOCATOR:Counting=Counting;
struct Wire{bytes:Vec<[u8;lim::MAX_DATAGRAM]>,lengths:Vec<usize>,count:usize,messages:usize}
impl Wire{fn new()->Self{Self{bytes:vec![[0;lim::MAX_DATAGRAM];512],lengths:vec![0;512],count:0,messages:0}}}
impl Sink for Wire{
 fn packet(&mut self,bytes:&[u8],_:SocketAddr){assert!(self.count<self.bytes.len());self.bytes[self.count][..bytes.len()].copy_from_slice(bytes);self.lengths[self.count]=bytes.len();self.count+=1;}
 fn message(&mut self,_:Lane,_:bool,bytes:&[u8]){assert!(bytes.iter().all(|&n|n==73));self.messages+=1;}
}
struct ServerWire {
    wire: Wire,
    addrs: Vec<SocketAddr>,
    session_id: u32,
}
impl ServerSink for ServerWire {
    fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
        let idx = self.wire.count;
        self.wire.packet(bytes, to);
        self.addrs[idx] = to;
    }
    fn on_hello(&mut self) -> serde_json::Value {
        serde_json::json!({"protocol": 3})
    }
    fn on_admit(&mut self, _client: &ClientInfo, _remote: SocketAddr) -> AdmitDecision {
        AdmitDecision::accept()
    }
    fn on_session(
        &mut self,
        session_id: u32,
        _client: &ClientInfo,
        _remote: SocketAddr,
        _rtt: f64,
    ) {
        self.session_id = session_id;
    }
    fn on_message(&mut self, _session_id: u32, lane: Lane, text: bool, bytes: &[u8]) {
        self.wire.message(lane, text, bytes);
    }
    fn on_path(&mut self, _session_id: u32, _new_path: SocketAddr, _old_path: SocketAddr) {}
    fn on_close(&mut self, _session_id: u32, _closed: Closed) {}
}
fn transfer(a:&mut Connection,b:&mut Connection,up:&mut Wire,down:&mut Wire,at:f64,drop:bool){
 let path=SocketAddr::from(([127,0,0,1],18451));
 for pass in 0..4{
  let n=up.count;for i in 0..n{if !drop||(i+pass)%17!=0{b.receive(&up.bytes[i][..up.lengths[i]],path,at,down);}}up.count=0;
  let n=down.count;for i in 0..n{if !drop||(i+pass)%19!=0{a.receive(&down.bytes[i][..down.lengths[i]],path,at,up);}}down.count=0;
  if up.count==0{break;}
 }
}
#[test]
fn first_packet_streams_loss_and_rekey_use_only_reserved_storage(){
 let path=SocketAddr::from(([127,0,0,1],18451));let c=Aead::new([7;32]).unwrap();let s=Aead::new([9;32]).unwrap();
 let mut a=Connection::new(Keys{send:c.clone(),receive:s.clone(),local:1,remote:2},path,0.,100.);
 let mut b=Connection::new(Keys{send:s,receive:c,local:2,remote:1},path,0.,100.);a.fixture_key_interval(64);b.fixture_key_interval(64);
 let mut up=Wire::new();let mut down=Wire::new();let payload=[73;8000];
 ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
 for tick in 0..1000{
  let now=tick as f64*17.;a.queue(Lane::State,false,&payload[..80]).unwrap();
  if tick%3==0{a.queue(Lane::Events,false,&payload[..2048]).unwrap();}
  if tick%10==0{b.queue(Lane::Bulk,false,&payload).unwrap();}
  a.poll(now,&mut up).unwrap();b.poll(now,&mut down).unwrap();transfer(&mut a,&mut b,&mut up,&mut down,now,tick%5==0);
 }
 for tick in 1000..1400{let now=tick as f64*17.;a.poll(now,&mut up).unwrap();b.poll(now,&mut down).unwrap();transfer(&mut a,&mut b,&mut up,&mut down,now,false);}
 TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);assert_eq!(a.backlog()+b.backlog(),0);
 assert!(a.closed.is_none()&&b.closed.is_none());assert!(up.messages>0&&down.messages>0);
}
#[test]
fn paused_state_queue_and_reliable_byte_limit_refuse_without_growth(){
 let path=SocketAddr::from(([127,0,0,1],18451));let mut c=Connection::new(Keys{send:Aead::new([7;32]).unwrap(),receive:Aead::new([9;32]).unwrap(),local:1,remote:2},path,0.,100.);
 let mut sink=Wire::new();let payload=[73;65536];ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
 for _ in 0..5000{c.queue(Lane::State,false,&payload[..1100]).unwrap();}c.flush(0.,&mut sink).unwrap();sink.count=0;
 let mut refused=false;for _ in 0..100{if c.queue(Lane::Bulk,false,&payload).is_err(){refused=true;break;}}
 c.flush(1.,&mut sink).unwrap();TRACK.with(|n|n.set(false));assert!(refused);assert_eq!(c.closed.unwrap().code,2);assert_eq!(ALLOCS.with(Cell::get),0);
}
#[test]
fn server_endpoint_steady_state_receive_and_flush_allocate_zero_bytes() {
    let server_addr = SocketAddr::from(([127, 0, 0, 1], 18451));
    let client_addr = SocketAddr::from(([127, 0, 0, 1], 49100));
    let server_id = Identity::from_seed([21u8; 32]);
    let client_id = Identity::from_seed([22u8; 32]);
    let retry = RetryTokens::from_secret([23u8; 32], 10_000.0);
    let mut server = ServerEndpoint::new(server_id.clone(), retry, 18451, 32, None, 10.0);
    let mut server_sink = ServerWire {
        wire: Wire::new(),
        addrs: vec![server_addr; 512],
        session_id: 0,
    };
    let mut hs = ClientHandshake::new(client_id, server_id.id, br#"{"protocol":3}"#).unwrap();
    let mut init_pkt = [0u8; lim::INITIATION_BYTES];
    hs.initiation(&[], &mut init_pkt).unwrap();
    server.receive(&init_pkt, client_addr, 0.0, &mut server_sink);
    let resp_len = server_sink.wire.lengths[0];
    let resp_pkt = server_sink.wire.bytes[0][..resp_len].to_vec();
    server_sink.wire.count = 0;
    assert_eq!(hs.receive(&resp_pkt), Event::Established);
    let keys = hs.take_keys().unwrap();
    let mut client = Connection::new(keys, server_addr, 5.0, 10.0);
    let mut client_wire = Wire::new();
    server.receive(hs.confirm(), client_addr, 10.0, &mut server_sink);
    let session_id = server_sink.session_id;
    assert_ne!(session_id, 0);
    for i in 0..server_sink.wire.count {
        let n = server_sink.wire.lengths[i];
        client.receive(
            &server_sink.wire.bytes[i][..n],
            server_addr,
            12.0,
            &mut client_wire,
        );
    }
    server_sink.wire.count = 0;
    client_wire.count = 0;

    let payload = [73u8; 256];
    ALLOCS.with(|n| n.set(0));
    TRACK.with(|n| n.set(true));
    for tick in 1..=200 {
        let now = 20.0 + tick as f64 * 16.0;
        client.queue(Lane::State, false, &payload[..64]).unwrap();
        if tick % 4 == 0 {
            client.queue(Lane::Events, false, &payload[..128]).unwrap();
        }
        client.poll(now, &mut client_wire).unwrap();
        for i in 0..client_wire.count {
            let n = client_wire.lengths[i];
            server.receive(
                &client_wire.bytes[i][..n],
                client_addr,
                now,
                &mut server_sink,
            );
        }
        client_wire.count = 0;
        server
            .queue(session_id, Lane::State, false, &payload[..96])
            .unwrap();
        if tick % 5 == 0 {
            server
                .queue(session_id, Lane::Events, false, &payload)
                .unwrap();
        }
        server.poll(now, &mut server_sink);
        for i in 0..server_sink.wire.count {
            let n = server_sink.wire.lengths[i];
            client.receive(
                &server_sink.wire.bytes[i][..n],
                server_addr,
                now,
                &mut client_wire,
            );
        }
        server_sink.wire.count = 0;
    }
    TRACK.with(|n| n.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert!(server_sink.wire.messages > 0 && client_wire.messages > 0);
}

#[test]
fn sixteen_synthetic_clients_thirty_minute_simulated_soak_allocates_zero_bytes_per_tick() {
    let server_addr = SocketAddr::from(([127, 0, 0, 1], 18451));
    let server_id = Identity::from_seed([31u8; 32]);
    let retry = RetryTokens::from_secret([32u8; 32], 10_000.0);
    let mut server = ServerEndpoint::new(server_id.clone(), retry, 18451, 16, Some(4), 10.0);
    let mut server_sink = ServerWire {
        wire: Wire::new(),
        addrs: vec![server_addr; 512],
        session_id: 0,
    };

    struct SyntheticClient {
        addr: SocketAddr,
        session_id: u32,
        conn: Connection,
    }

    let mut clients = Vec::with_capacity(16);
    for idx in 0..16usize {
        let subnet = (idx / 4) as u8 + 1;
        let host = (idx % 4) as u8 + 10;
        let client_addr = SocketAddr::from(([10, 0, subnet, host], 49100 + idx as u16));
        let client_id = Identity::from_seed([40 + idx as u8; 32]);
        let mut hs = ClientHandshake::new(client_id, server_id.id, br#"{"protocol":3}"#).unwrap();
        let mut init_pkt = [0u8; lim::INITIATION_BYTES];
        hs.initiation(&[], &mut init_pkt).unwrap();
        server.receive(&init_pkt, client_addr, 0.0, &mut server_sink);
        let resp_len = server_sink.wire.lengths[0];
        let resp_pkt = server_sink.wire.bytes[0][..resp_len].to_vec();
        server_sink.wire.count = 0;
        assert_eq!(hs.receive(&resp_pkt), Event::Established);
        let keys = hs.take_keys().unwrap();
        let mut conn = Connection::new(keys, server_addr, 5.0, 10.0);
        let mut cwire = Wire::new();
        server.receive(hs.confirm(), client_addr, 10.0, &mut server_sink);
        let session_id = server_sink.session_id;
        assert_ne!(session_id, 0);
        for i in 0..server_sink.wire.count {
            let n = server_sink.wire.lengths[i];
            conn.receive(&server_sink.wire.bytes[i][..n], server_addr, 12.0, &mut cwire);
        }
        server_sink.wire.count = 0;
        clients.push(SyntheticClient {
            addr: client_addr,
            session_id,
            conn,
        });
    }

    let mut client_wire = Wire::new();
    let payload = [73u8; 192];
    // 30 minutes at 30 Hz = 54,000 ticks (dt = 33.333 ms, total simulated duration = 1,800,000 ms).
    let total_ticks = 54_000usize;
    ALLOCS.with(|n| n.set(0));
    TRACK.with(|n| n.set(true));

    for tick in 1..=total_ticks {
        let now = 100.0 + (tick as f64) * (1000.0 / 30.0);
        for c in &mut clients {
            c.conn.queue(Lane::State, false, &payload[..48]).unwrap();
            if tick % 30 == 0 {
                c.conn.queue(Lane::Events, false, &payload[..96]).unwrap();
            }
            c.conn.poll(now, &mut client_wire).unwrap();
            for i in 0..client_wire.count {
                let n = client_wire.lengths[i];
                server.receive(
                    &client_wire.bytes[i][..n],
                    c.addr,
                    now,
                    &mut server_sink,
                );
            }
            client_wire.count = 0;
        }

        for c in &clients {
            server
                .queue(c.session_id, Lane::State, false, &payload[..80])
                .unwrap();
            if tick % 60 == 0 {
                server
                    .queue(c.session_id, Lane::Events, false, &payload[..128])
                    .unwrap();
            }
        }
        server.poll(now, &mut server_sink);
        for i in 0..server_sink.wire.count {
            let n = server_sink.wire.lengths[i];
            let dst = server_sink.addrs[i];
            if let Some(c) = clients.iter_mut().find(|c| c.addr == dst) {
                c.conn.receive(
                    &server_sink.wire.bytes[i][..n],
                    server_addr,
                    now,
                    &mut client_wire,
                );
            }
        }
        server_sink.wire.count = 0;
        client_wire.count = 0;
    }

    TRACK.with(|n| n.set(false));
    let total_allocs = ALLOCS.with(Cell::get);
    assert_eq!(
        total_allocs, 0,
        "expected 0 heap allocations over 54,000 ticks (30m @ 30Hz) with 16 clients"
    );
    assert_eq!(server.live_sessions(), 16);
    assert!(server_sink.wire.messages >= 16 * (total_ticks / 30));
    assert!(client_wire.messages >= 16 * (total_ticks / 60));
}
