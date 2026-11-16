//! A headless player that joins a listed server the way an internet player does, through master
//! signalling, and plays on through the server's relayed candidate only. Not part of the normal
//! test run: it needs a master, a TURN server and a bfeserver with relay policy auto or always.
//!
//! `BFE_RELAY_REHEARSAL="<master url> <server id> <stun host:port> <seconds> [relay|any]"`
//! cargo test -p bfe-server --test relay_rehearsal -- --ignored --nocapture
//!
//! It offers its reflexive address (from the STUN server), waits for the server's answer and its
//! relayed candidate, connects to that candidate (or to every candidate with `any`), takes a spawn
//! and keeps sending inputs and acknowledgements for the given time or until the battle ends,
//! then reports what arrived.
//! It fails if the session does not open or closes early.

use bfe_server::identity::ServerIdentity;
use bfe_server::master_client::{MasterClient, MasterEvent};
use bfe_server::udp_peer::ACK_MAGIC;
use bfe_transport::connection::Lane;
use bfe_transport::endpoint::Endpoint;
use bfe_transport::layout::network_contract as contract;
use bfe_transport::stun;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn reflexive(stun_server: &str) -> SocketAddr {
    let target = stun_server.to_socket_addrs().expect("STUN address").find(SocketAddr::is_ipv4).expect("IPv4 STUN address");
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind");
    socket.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");
    let tx = stun::new_transaction().expect("random");
    socket.send_to(&stun::encode_binding_request(&tx), target).expect("send");
    let mut buf = [0u8; 512];
    let (n, _) = socket.recv_from(&mut buf).expect("STUN answer");
    stun::decode_message(&buf[..n]).and_then(|m| m.mapped_address()).expect("mapped address")
}

/// Many players offer and none joins: the server must stop at its relay limit and release
/// every allocation once its player has not joined for 30 s.
/// `BFE_RELAY_OFFERS="<master url> <server id> <stun host:port> <players>"`
#[test]
#[ignore = "needs a master, a TURN server and a running bfeserver"]
fn many_offers_stop_at_the_relay_limit() {
    let Ok(setting) = std::env::var("BFE_RELAY_OFFERS") else {
        return;
    };
    let args: Vec<&str> = setting.split_whitespace().collect();
    let (master_url, server_id, stun_server) = (args[0], args[1], args[2]);
    let players: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
    let mapped = reflexive(stun_server);
    let mut sessions = Vec::new();
    for _ in 0..players {
        let player = ServerIdentity::generate().expect("identity");
        let master = MasterClient::new(master_url, player, true).expect("master client");
        let (events_tx, events_rx) = mpsc::channel();
        master.connect(events_tx).expect("signalling");
        let nonce: String = stun::new_transaction().expect("random").iter().chain(&[0u8; 4]).map(|b| format!("{b:02x}")).collect();
        assert!(master.signal(server_id, &json!({"type": "offer", "udp": 1, "nonce": nonce, "candidates": [{"address": mapped.ip().to_string(), "port": mapped.port(), "kind": "srflx"}]})));
        sessions.push((master, events_rx));
        std::thread::sleep(Duration::from_millis(150));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut relayed = std::collections::BTreeSet::new();
    while Instant::now() < deadline {
        for (i, (_, events)) in sessions.iter().enumerate() {
            while let Ok(MasterEvent::Signal(signal)) = events.try_recv() {
                if signal.payload.contains("\"relay\"") {
                    relayed.insert(i);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("RESULT {players} offers, {} relayed candidates", relayed.len());
}

#[test]
#[ignore = "needs a master, a TURN server and a running bfeserver"]
fn join_and_play_through_the_relay() {
    let Ok(setting) = std::env::var("BFE_RELAY_REHEARSAL") else {
        return;
    };
    let args: Vec<&str> = setting.split_whitespace().collect();
    let (master_url, server_id, stun_server) = (args[0], args[1], args[2]);
    let seconds: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);
    let relay_only = args.get(4).copied() != Some("any");

    let player = ServerIdentity::generate().expect("identity");
    let master = MasterClient::new(master_url, player.clone(), true).expect("master client");
    let listing = master.request("/v1/servers", "GET", None, 0).expect("server list");
    let row = listing
        .get("servers")
        .and_then(Value::as_array)
        .and_then(|rows| rows.iter().find(|r| r.get("id").and_then(Value::as_str) == Some(server_id)))
        .cloned()
        .expect("server listed");
    println!("listed: relayPolicy={} endpoints={}", row["relayPolicy"], row["endpoints"]);

    let (events_tx, events_rx) = mpsc::channel();
    master.connect(events_tx).expect("signalling");
    let mapped = reflexive(stun_server);
    let nonce: String = stun::new_transaction().expect("random").iter().chain(&[0u8; 4]).map(|b| format!("{b:02x}")).collect();
    let started = Instant::now();
    assert!(master.signal(server_id, &json!({"type": "offer", "udp": 1, "nonce": nonce, "candidates": [{"address": mapped.ip().to_string(), "port": mapped.port(), "kind": "srflx"}]})));

    let mut candidates: Vec<(String, SocketAddr)> = Vec::new();
    while started.elapsed() < Duration::from_secs(15) && !candidates.iter().any(|(kind, _)| kind == "relay") {
        let Ok(MasterEvent::Signal(signal)) = events_rx.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<Value>(&signal.payload) else {
            continue;
        };
        if signal.from != server_id || payload["nonce"] != nonce.as_str() {
            continue;
        }
        println!("{:>6} ms signal {}: {}", started.elapsed().as_millis(), signal.msg_type, payload["candidates"]);
        for c in payload["candidates"].as_array().into_iter().flatten() {
            if let (Some(address), Some(port)) = (c["address"].as_str(), c["port"].as_u64()) {
                if let Ok(ip) = address.parse() {
                    candidates.push((c["kind"].as_str().unwrap_or("host").to_string(), SocketAddr::new(ip, port as u16)));
                }
            }
        }
    }
    let relayed = candidates.iter().find(|(kind, _)| kind == "relay").map(|(_, a)| *a).expect("relayed candidate signalled");
    let targets: Vec<SocketAddr> = if relay_only { vec![relayed] } else { candidates.iter().map(|(_, a)| *a).collect() };
    let address = targets.iter().map(SocketAddr::to_string).collect::<Vec<_>>().join(",");

    let mut key = [0u8; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&server_id[i * 2..i * 2 + 2], 16).expect("server id");
    }
    let hello = serde_json::to_vec(&json!({
        "name": "RelayRehearsal",
        "protocol": contract::PROTOCOL,
        "profile": contract::PROFILE,
        "simulation": contract::SIMULATION,
        "content": row["content"],
    }))
    .expect("hello");
    // `Endpoint::connect` resolves one address string; connect to each target in turn.
    let mut client = None;
    for target in &targets {
        let mut endpoint = Endpoint::connect(&target.to_string(), player.inner.clone(), key, &hello).expect("endpoint");
        match endpoint.establish(Duration::from_secs(8), &mut |_, _, _, _| {}) {
            Ok(()) => {
                println!("{:>6} ms connected through {target} ({address})", started.elapsed().as_millis());
                client = Some(endpoint);
                break;
            }
            Err(error) => println!("{target}: {}", endpoint.failure(error)),
        }
    }
    let mut client = client.expect("session through the relay");

    let mut types: BTreeMap<String, u64> = BTreeMap::new();
    let mut state_packets = 0u64;
    let mut state_bytes = 0u64;
    let mut asked_start = false;
    let play = Instant::now();
    let mut last_ack = Instant::now();
    let mut next_report = Instant::now() + Duration::from_secs(30);
    while play.elapsed() < Duration::from_secs(seconds) {
        let mut welcome = false;
        let pumped = client.pump(&mut |lane, text, bytes, _| {
            if text && lane == Lane::Events {
                if let Ok(v) = serde_json::from_slice::<Value>(bytes) {
                    let kind = v["type"].as_str().unwrap_or("?").to_string();
                    welcome |= kind == "welcome";
                    *types.entry(kind).or_default() += 1;
                }
            } else if lane == Lane::State {
                state_packets += 1;
                state_bytes += bytes.len() as u64;
            }
        });
        // The battle is over when the next one is announced.
        if types.contains_key("match") {
            println!("{:>4} s: the battle ended (next one announced)", play.elapsed().as_secs());
            break;
        }
        if let Err(error) = pumped {
            panic!("session ended after {} s: {}", play.elapsed().as_secs(), client.failure(error));
        }
        if welcome && !asked_start {
            asked_start = true;
            let call = serde_json::to_vec(&json!({"type": "call", "id": 1, "method": "start", "args": {"name": "RelayRehearsal"}})).expect("call");
            client.send(Lane::Events, true, &call).expect("start");
        }
        if last_ack.elapsed() >= Duration::from_millis(100) {
            last_ack = Instant::now();
            let mut ack = Vec::with_capacity(10);
            ack.extend_from_slice(&ACK_MAGIC.to_le_bytes());
            ack.extend_from_slice(&1u16.to_le_bytes());
            ack.extend_from_slice(&0u32.to_le_bytes());
            let _ = client.send(Lane::State, false, &ack);
            let _ = client.flush();
        }
        if Instant::now() >= next_report {
            next_report += Duration::from_secs(30);
            println!("{:>4} s: state packets {state_packets} ({state_bytes} bytes), events {types:?}", play.elapsed().as_secs());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    println!("RESULT {seconds} s through {relayed}: state packets {state_packets} ({state_bytes} bytes), events {types:?}");
    assert!(types.contains_key("welcome"), "welcome arrived");
    assert!(state_packets > play.elapsed().as_secs() * 10, "snapshots kept arriving");
    client.close(0, b"rehearsal over");
    let _ = client.flush();
}
