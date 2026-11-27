//! The inbound limit of a server hosting a 2005 battle, through real sockets
//! and the real signed handshake: a joined player who floods the server is
//! dropped undecoded and closed after five seconds over the limit, a player
//! beside it keeps playing, and a 2004 battle's server never applies it.
use bfe_server::udp_server::UdpGameServer;
use bfe_sim::wire::revisions::{Descriptor, Request, Supports, Table, SUPPORTS, WRITES};
use bfe_transport::crypto::Identity;
use bfe_transport::endpoint::{Endpoint, Rendezvous};
use bfe_transport::layout::network_contract;
use serde_json::{json, Value};
use std::net::{SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TOKEN: &str = "5eed0000000000000000000000000000";
const FINGERPRINT: &str = "inbound-limit-test";

fn hello(name: &str) -> Vec<u8> {
    let battle = Request { mode: "eli".into(), map: "swbf2/kam9".into(), token: TOKEN.into(), supports: Supports::of(SUPPORTS) };
    serde_json::to_vec(&json!({
        "name": name,
        "protocol": network_contract::PROTOCOL,
        "profile": network_contract::PROFILE,
        "simulation": network_contract::SIMULATION,
        "content": FINGERPRINT,
        "battle": battle.json(),
    }))
    .unwrap()
}

fn id_hex(id: &[u8; 32]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

struct Loopback {
    server: UdpGameServer,
    address: SocketAddr,
    server_id: [u8; 32],
}

impl Loopback {
    fn new(battle: Option<Descriptor>) -> Self {
        let identity = Identity::from_seed([41; 32]);
        let server_id = identity.id;
        let mut server = UdpGameServer::bind(0, Some("127.0.0.1"), identity, 8, 8, 100_000, FINGERPRINT.into(), FINGERPRINT.into())
            .expect("bind");
        server.set_match(battle);
        let address = SocketAddr::from(([127, 0, 0, 1], server.port));
        Self { server, address, server_id }
    }

    fn turn(&mut self, clients: &mut [&mut Endpoint]) {
        let _ = self.server.poll(|_| false);
        self.server.flush();
        for client in clients.iter_mut() {
            let _ = client.pump(&mut |_, _, _, _| {});
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    // A client whose socket the test keeps a second handle on, to send from
    // the same address outside the session.
    fn join(&mut self, name: &str, seed: u8) -> (Endpoint, UdpSocket) {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let raw = socket.try_clone().unwrap();
        let (candidates, receiver) = mpsc::channel();
        candidates.send(self.address).unwrap();
        let alive = std::sync::Arc::new(());
        let rendezvous = Rendezvous { candidates: receiver, start: None, alive: alive.clone() };
        let identity = Identity::from_seed([seed; 32]);
        let id = id_hex(&identity.id);
        let mut client = Endpoint::rendezvous(vec![socket], identity, self.server_id, &hello(name), rendezvous).unwrap();
        let started = Instant::now();
        while !self.server.players().contains(&id) {
            assert!(started.elapsed() < Duration::from_secs(5), "{name} admitted");
            self.turn(&mut [&mut client]);
        }
        drop(alive);
        (client, raw)
    }
}

fn metrics(net: &mut Loopback) -> Value {
    net.server.transport_metrics(false)
}

#[test]
fn a_flooding_player_is_closed_and_the_player_beside_it_keeps_playing() {
    use bfe_server::inbound::{BURST, OVER_SECONDS, RATE};
    let mut net = Loopback::new(Some(Descriptor { mode: "eli".into(), map: "swbf2/kam9".into(), token: TOKEN.into(), records: Table::of(WRITES) }));
    let (mut pilot, _) = net.join("Pilot", 1);
    let (mut flooder, raw) = net.join("Flooder", 2);
    let (pilot_id, flooder_id) = (id_hex(&pilot.identity), id_hex(&flooder.identity));
    let junk = [0x5a_u8; 1200];
    let started = Instant::now();
    let mut closed_at = None;
    // What the test itself sent, when it first saw the limit drop a datagram, and how long its
    // turns took: the bounds below follow these, not the machine's speed.
    let (mut junk_sent, mut limit_seen, mut last_turn, mut longest_turn, mut count_from) =
        (0u32, None, started, Duration::ZERO, Duration::ZERO);
    while started.elapsed() < Duration::from_secs(9) {
        // Two junk datagrams a turn from the flooder's own address, far over the limit.
        for _ in 0..2 {
            if raw.send_to(&junk, net.address).is_ok() {
                junk_sent += 1;
            }
        }
        net.turn(&mut [&mut pilot, &mut flooder]);
        let now = Instant::now();
        // A turn of a second or more leaves a counted second without a drop, which restarts
        // the rule's count.
        if closed_at.is_none() && now - last_turn >= Duration::from_secs(1) {
            count_from = now - started;
        }
        longest_turn = longest_turn.max(now - last_turn);
        last_turn = now;
        if limit_seen.is_none() && net.server.inbound.dropped > 0 {
            limit_seen = Some((now - started, junk_sent, net.server.inbound.dropped));
        }
        let players = net.server.players();
        assert!(players.contains(&pilot_id), "the pilot was dropped at {:?}", started.elapsed());
        if closed_at.is_none() && !players.contains(&flooder_id) {
            closed_at = Some(started.elapsed());
        }
        if closed_at.is_some_and(|at| started.elapsed() > at + Duration::from_millis(500)) {
            break;
        }
    }
    let flooded = started.elapsed();
    let closed_at = closed_at.expect("the flooder was never closed");
    let m = metrics(&mut net);
    println!("flooder closed after {closed_at:?} (longest turn {longest_turn:?}); sent {junk_sent} junk datagrams; metrics {m}");
    // The rule counts whole seconds from the flooder's first datagram, its handshake, a moment
    // before the flood, and closes it on a drop in the fifth second in a row with drops:
    // more than three seconds into the flood and at most five, one turn later at the latest.
    let (fewest, most) = (Duration::from_secs_f64(f64::from(OVER_SECONDS - 2)), Duration::from_secs_f64(f64::from(OVER_SECONDS)));
    assert!(closed_at > fewest && closed_at <= count_from + most + longest_turn, "{closed_at:?}, longest turn {longest_turn:?}");
    let (dropped, bytes) = (net.server.inbound.dropped, net.server.inbound.dropped_bytes);
    assert_eq!((&m["inboundDropped"], &m["inboundDroppedBytes"]), (&json!(dropped), &json!(bytes)), "{m}");
    // From the moment the test saw a drop to the end of the run the rule lets no more through than
    // its burst (a second time after the close, when the address is forgotten) and its rate; the
    // rest of what the test sent in that time is dropped undecoded.
    let (seen_at, junk_then, dropped_then) = limit_seen.expect("the limit never dropped a datagram");
    let allowance = ((2. * BURST + RATE * (flooded - seen_at).as_secs_f64()) / junk.len() as f64).ceil();
    let (sent_after, dropped_after) = (f64::from(junk_sent - junk_then), f64::from(dropped - dropped_then));
    assert!(sent_after > 2. * allowance, "the flood stayed over the limit: {sent_after} sent, allowance {allowance}");
    assert!(dropped_after >= sent_after - allowance, "{dropped_after} dropped of {sent_after} sent after the limit, allowance {allowance}; {m}");
    assert!(bytes >= dropped * 1_000, "{m}");
    assert_eq!(net.server.players(), vec![pilot_id]);
}

#[test]
fn a_2004_battle_never_applies_the_limit() {
    let mut net = Loopback::new(None);
    let (mut player, raw) = net.join("Player", 3);
    let id = id_hex(&player.identity);
    let junk = [0x5a_u8; 1200];
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(1_500) {
        for _ in 0..2 {
            let _ = raw.send_to(&junk, net.address);
        }
        net.turn(&mut [&mut player]);
    }
    let m = metrics(&mut net);
    assert!(m.get("inboundDropped").is_none() && m.get("inboundDroppedBytes").is_none(), "{m}");
    assert_eq!(net.server.inbound.dropped, 0);
    assert!(net.server.players().contains(&id));
}
