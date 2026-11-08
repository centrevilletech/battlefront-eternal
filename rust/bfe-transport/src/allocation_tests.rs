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
// Thread-local flag and counter isolate steady-state allocation measurement to the current test thread
// without counting allocations made by parallel test runners or cold handshake setup before TRACK is set.
thread_local!{static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
// Global allocator wrapper that intercepts heap allocations only while `TRACK` is active on the current thread.
struct Counting;
// SAFETY: `Counting` delegates all memory allocation, deallocation, and reallocation
// directly to `std::alloc::System` with identical `Layout` and pointer arguments, only
// incrementing a thread-local `Cell<usize>` counter when `TRACK` is enabled.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        // SAFETY: Forwarded with the caller-validated `Layout` `l`.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: Forwarded with the pointer `p` and `Layout` `l` allocated by `System`.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        // SAFETY: Forwarded with the pointer `p`, `Layout` `l`, and new size `n`.
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]static ALLOCATOR:Counting=Counting;
// Preallocated ring of 512 maximum-sized UDP datagrams so the test harness itself never allocates
// when capturing transmitted packets or verifying delivered payload bytes during a tracked tick.
struct Wire{bytes:Vec<[u8;lim::MAX_DATAGRAM]>,lengths:Vec<usize>,count:usize,messages:usize}
impl Wire{fn new()->Self{Self{bytes:vec![[0;lim::MAX_DATAGRAM];512],lengths:vec![0;512],count:0,messages:0}}}
// Synchronous Sink implementation that copies outgoing wire bytes into preallocated slots
// and validates incoming message bytes in place without growing any heap collection.
impl Sink for Wire{
 fn packet(&mut self,bytes:&[u8],_:SocketAddr){assert!(self.count<self.bytes.len());self.bytes[self.count][..bytes.len()].copy_from_slice(bytes);self.lengths[self.count]=bytes.len();self.count+=1;}
 fn message(&mut self,_:Lane,_:bool,bytes:&[u8]){assert!(bytes.iter().all(|&n|n==73));self.messages+=1;}
}
// Preallocated server-side test harness pairing `Wire` packet storage with per-slot destination addresses
// so `ServerEndpoint::receive` and `ServerEndpoint::poll` can be measured with zero observer overhead.
struct ServerWire {
    wire: Wire,
    addrs: Vec<SocketAddr>,
    session_id: u32,
}
// ServerSink callbacks used during cold admission (`on_hello`, `on_admit`, `on_session`) and
// steady-state packet routing (`send_packet`, `on_message`) without allocating on the hot path.
impl ServerSink for ServerWire {
    // Record the outgoing datagram and its target socket address in the preallocated slot.
    fn send_packet(&mut self, bytes: &[u8], to: SocketAddr) {
        let idx = self.wire.count;
        self.wire.packet(bytes, to);
        self.addrs[idx] = to;
    }
    // Cold handshake hello callback invoked only during `INITIATION` processing.
    fn on_hello(&mut self) -> serde_json::Value {
        serde_json::json!({"protocol": 3})
    }
    // Cold admission callback invoked only during `CONFIRM` processing.
    fn on_admit(&mut self, _client: &ClientInfo, _remote: SocketAddr) -> AdmitDecision {
        AdmitDecision::accept()
    }
    // Record the newly assigned server session id when handshake confirmation completes.
    fn on_session(
        &mut self,
        session_id: u32,
        _client: &ClientInfo,
        _remote: SocketAddr,
        _rtt: f64,
    ) {
        self.session_id = session_id;
    }
    // Validate and count delivered application messages on the steady-state receive path.
    fn on_message(&mut self, _session_id: u32, lane: Lane, text: bool, bytes: &[u8]) {
        self.wire.message(lane, text, bytes);
    }
    // Path migration and close callbacks are no-ops in steady-state allocation fixtures.
    fn on_path(&mut self, _session_id: u32, _new_path: SocketAddr, _old_path: SocketAddr) {}
    fn on_close(&mut self, _session_id: u32, _closed: Closed) {}
}
// Pump up to four synchronous round-trip passes between peers `a` and `b`, optionally dropping
// deterministic sequence slots (`% 17` and `% 19`) to force ACK range gaps and loss recovery.
fn transfer(a:&mut Connection,b:&mut Connection,up:&mut Wire,down:&mut Wire,at:f64,drop:bool){
 let path=SocketAddr::from(([127,0,0,1],18451));
 for pass in 0..4{
  let n=up.count;for i in 0..n{if !drop||(i+pass)%17!=0{b.receive(&up.bytes[i][..up.lengths[i]],path,at,down);}}up.count=0;
  let n=down.count;for i in 0..n{if !drop||(i+pass)%19!=0{a.receive(&down.bytes[i][..down.lengths[i]],path,at,up);}}down.count=0;
  if up.count==0{break;}
 }
}
// Verifies that after `Connection::new` preallocates stream rings, state datagram queues, fragment
// assembly slots, and sent-packet metadata, 1,400 ticks of lossy traffic and AEAD key rotations
// (`fixture_key_interval(64)`) execute with zero heap allocations.
#[test]
fn first_packet_streams_loss_and_rekey_use_only_reserved_storage()->std::result::Result<(),Box<dyn std::error::Error>>{
 let path=SocketAddr::from(([127,0,0,1],18451));let c=Aead::new([7;32])?;let s=Aead::new([9;32])?;
 let mut a=Connection::new(Keys{send:c.clone(),receive:s.clone(),local:1,remote:2},path,0.,100.);
 let mut b=Connection::new(Keys{send:s,receive:c,local:2,remote:1},path,0.,100.);a.fixture_key_interval(64);b.fixture_key_interval(64);
 let mut up=Wire::new();let mut down=Wire::new();let payload=[73;8000];
 // Reset and enable the thread-local allocation counter only after both connections are constructed.
 ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
 // Run 1,000 ticks with periodic packet drops (`tick % 5 == 0`) to exercise fragmentation,
 // out-of-order stream reassembly, RACK loss detection, PTO probes, and key phase updates.
 for tick in 0..1000{
  let now=tick as f64*17.;a.queue(Lane::State,false,&payload[..80])?;
  if tick%3==0{a.queue(Lane::Events,false,&payload[..2048])?;}
  if tick%10==0{b.queue(Lane::Bulk,false,&payload)?;}
  a.poll(now,&mut up)?;b.poll(now,&mut down)?;transfer(&mut a,&mut b,&mut up,&mut down,now,tick%5==0);
 }
 // Run 400 loss-free recovery ticks so all in-flight reliable stream spans drain completely.
 for tick in 1000..1400{let now=tick as f64*17.;a.poll(now,&mut up)?;b.poll(now,&mut down)?;transfer(&mut a,&mut b,&mut up,&mut down,now,false);}
 TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);assert_eq!(a.backlog()+b.backlog(),0);
 assert!(a.closed.is_none()&&b.closed.is_none());assert!(up.messages>0&&down.messages>0);Ok(())
}
// Verifies that flooding the replaceable `Lane::State` queue evicts old datagrams in place, and that
// overflowing `MAX_STREAM_BUFFER` on `Lane::Bulk` closes the connection with `close_code::PROTOCOL` (2)
// without growing any buffer on the heap.
#[test]
fn paused_state_queue_and_reliable_byte_limit_refuse_without_growth()->std::result::Result<(),Box<dyn std::error::Error>>{
 let path=SocketAddr::from(([127,0,0,1],18451));let mut c=Connection::new(Keys{send:Aead::new([7;32])?,receive:Aead::new([9;32])?,local:1,remote:2},path,0.,100.);
 let mut sink=Wire::new();let payload=[73;65536];ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
 for _ in 0..5000{c.queue(Lane::State,false,&payload[..1100])?;}c.flush(0.,&mut sink)?;sink.count=0;
 let mut refused=false;for _ in 0..100{if c.queue(Lane::Bulk,false,&payload).is_err(){refused=true;break;}}
 c.flush(1.,&mut sink)?;TRACK.with(|n|n.set(false));assert!(refused);assert_eq!(c.closed.ok_or("expected closed")?.code,2);assert_eq!(ALLOCS.with(Cell::get),0);Ok(())
}
// Verifies that once a client completes the 3-way BFE-UDP/1 handshake (`INITIATION` -> `RESPONSE` ->
// `CONFIRM` -> `HANDSHAKE_DONE`) with `ServerEndpoint`, 200 bidirectional ticks of `Lane::State` and
// `Lane::Events` messages allocate zero bytes on the heap.
#[test]
fn server_endpoint_steady_state_receive_and_flush_allocate_zero_bytes(
) -> std::result::Result<(), Box<dyn std::error::Error>> {
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
    // Perform cold client/server handshake establishment before enabling allocation tracking.
    let mut hs = ClientHandshake::new(client_id, server_id.id, br#"{"protocol":3}"#)?;
    let mut init_pkt = [0u8; lim::INITIATION_BYTES];
    hs.initiation(&[], &mut init_pkt)?;
    server.receive(&init_pkt, client_addr, 0.0, &mut server_sink);
    let resp_len = server_sink.wire.lengths[0];
    let resp_pkt = server_sink.wire.bytes[0][..resp_len].to_vec();
    server_sink.wire.count = 0;
    assert_eq!(hs.receive(&resp_pkt), Event::Established);
    // Construct the established client Connection from the derived session traffic keys.
    let keys = hs.take_keys().ok_or("expected client keys")?;
    let mut client = Connection::new(keys, server_addr, 5.0, 10.0);
    let mut client_wire = Wire::new();
    // Deliver CONFIRM to the server and drain its initial HANDSHAKE_DONE packet into the client.
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

    // Cold handshake admission is complete; measure steady-state client/server frame exchange.
    let payload = [73u8; 256];
    ALLOCS.with(|n| n.set(0));
    TRACK.with(|n| n.set(true));
    for tick in 1..=200 {
        let now = 20.0 + tick as f64 * 16.0;
        client.queue(Lane::State, false, &payload[..64])?;
        if tick % 4 == 0 {
            client.queue(Lane::Events, false, &payload[..128])?;
        }
        client.poll(now, &mut client_wire)?;
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
        server.queue(session_id, Lane::State, false, &payload[..96])?;
        if tick % 5 == 0 {
            server.queue(session_id, Lane::Events, false, &payload)?;
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
    Ok(())
}

// Verifies the Phase 2 exit gate: 16 concurrent clients across 4 distinct `/24` subnets exchanging
// 30 Hz `Lane::State` and periodic `Lane::Events` frames over a 30-minute simulated soak (54,000 ticks)
// perform zero heap allocations after session establishment.
#[test]
fn sixteen_synthetic_clients_thirty_minute_simulated_soak_allocates_zero_bytes_per_tick(
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let server_addr = SocketAddr::from(([127, 0, 0, 1], 18451));
    let server_id = Identity::from_seed([31u8; 32]);
    let retry = RetryTokens::from_secret([32u8; 32], 10_000.0);
    let mut server = ServerEndpoint::new(server_id.clone(), retry, 18451, 16, Some(4), 10.0);
    let mut server_sink = ServerWire {
        wire: Wire::new(),
        addrs: vec![server_addr; 512],
        session_id: 0,
    };

    // Each synthetic client holds its distinct source address, assigned server session id,
    // and preallocated client Connection state machine.
    struct SyntheticClient {
        addr: SocketAddr,
        session_id: u32,
        conn: Connection,
    }

    // Admit all 16 synthetic clients before enabling the zero-allocation tracker, spreading them
    // across 4 subnets (4 clients per subnet) to stay within `max_sessions_per_prefix = Some(4)`.
    let mut clients = Vec::with_capacity(16);
    for idx in 0..16usize {
        let subnet = (idx / 4) as u8 + 1;
        let host = (idx % 4) as u8 + 10;
        let client_addr = SocketAddr::from(([10, 0, subnet, host], 49100 + idx as u16));
        let client_id = Identity::from_seed([40 + idx as u8; 32]);
        // Drive the 3-way handshake for client `idx` and capture its established Connection.
        // First, build the 1,200-byte padded INITIATION datagram and pass it to the server.
        // Next, feed the server's signed RESPONSE back into `ClientHandshake::receive` to derive
        // directional application keys (`c ap` and `s ap`) and construct the client `Connection`.
        let mut hs = ClientHandshake::new(client_id, server_id.id, br#"{"protocol":3}"#)?;
        let mut init_pkt = [0u8; lim::INITIATION_BYTES];
        hs.initiation(&[], &mut init_pkt)?;
        server.receive(&init_pkt, client_addr, 0.0, &mut server_sink);
        let resp_len = server_sink.wire.lengths[0];
        let resp_pkt = server_sink.wire.bytes[0][..resp_len].to_vec();
        server_sink.wire.count = 0;
        assert_eq!(hs.receive(&resp_pkt), Event::Established);
        let keys = hs.take_keys().ok_or("expected client keys")?;
        let mut conn = Connection::new(keys, server_addr, 5.0, 10.0);
        let mut cwire = Wire::new();
        // Confirm the session on the server and deliver the server's HANDSHAKE_DONE frame
        // so the client marks `handshake_confirmed = true` and enables 1-RTT key updates.
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
    // Reset the thread-local allocation counter and arm the global allocator hook before entering
    // the steady-state tick loop so any dynamic Vec, Box, or String growth is caught.
    let total_ticks = 54_000usize;
    ALLOCS.with(|n| n.set(0));
    TRACK.with(|n| n.set(true));

    // Advance the simulated clock at 30 Hz (33.333 ms per tick) across 54,000 ticks, exercising
    // packet number growth, ACK range trimming, replay window advancement, and periodic key updates.
    for tick in 1..=total_ticks {
        let now = 100.0 + (tick as f64) * (1000.0 / 30.0);
        // Each client queues an unreliable state input every tick (30 Hz) and a reliable ordered
        // event once per simulated second (1 Hz), then polls its `Connection` to emit UDP datagrams
        // directly into `ServerEndpoint::receive` without touching kernel sockets or heap buffers.
        for c in &mut clients {
            c.conn.queue(Lane::State, false, &payload[..48])?;
            if tick % 30 == 0 {
                c.conn.queue(Lane::Events, false, &payload[..96])?;
            }
            c.conn.poll(now, &mut client_wire)?;
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

        // Server broadcasts a world snapshot delta on `Lane::State` every tick and a reliable event
        // every 60 ticks (2 simulated seconds) to all 16 established sessions, then flushes due work.
        for c in &clients {
            server.queue(c.session_id, Lane::State, false, &payload[..80])?;
            if tick % 60 == 0 {
                server.queue(c.session_id, Lane::Events, false, &payload[..128])?;
            }
        }
        // Poll all sessions for due ACKs, keepalives, loss recovery, and coalesced packet emission.
        // Over 30 simulated minutes, packet numbers exceed multiple `KEY_UPDATE_INTERVAL` (5,000)
        // boundaries, verifying that `CryptoPhase::maybe_advance` and `Connection::on_authenticated`
        // derive next-generation HKDF keys and rotate AEAD phases in place with zero heap allocations.
        server.poll(now, &mut server_sink);
        // Route each emitted server datagram back to the matching synthetic client by destination address.
        // Each client decrypts the server packet, updates its 128-packet replay window and ACK ranges,
        // and reassembles reliable `Lane::Events` messages using its fixed-capacity receive slots.
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

    // Disable allocation tracking before running post-soak assertions so any failure formatting can allocate.
    // Every single tick across all 16 sessions must have reused preallocated ring and packet storage.
    // Confirm all 16 sessions remain open and both directions delivered the expected message volume.
    TRACK.with(|n| n.set(false));
    let total_allocs = ALLOCS.with(Cell::get);
    assert_eq!(
        total_allocs, 0,
        "expected 0 heap allocations over 54,000 ticks (30m @ 30Hz) with 16 clients"
    );
    assert_eq!(server.live_sessions(), 16);
    assert!(server_sink.wire.messages >= 16 * (total_ticks / 30));
    assert!(client_wire.messages >= 16 * (total_ticks / 60));
    Ok(())
}
