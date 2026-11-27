//! Joins a running server with this build's own hello and prints what the
//! server says: its advertised contract, then the handshake's result. Used to
//! check that two builds of different protocols refuse each other cleanly.
//! `BFE_PROBE_ADDRESS=host:port cargo test -p bfe-server --test version_probe -- --ignored --nocapture`
use bfe_transport::{crypto::Identity, endpoint::Endpoint, layout::network_contract as contract};
use serde_json::Value;
use std::io::{Read, Write};
use std::time::Duration;

fn info(address: &str) -> Value {
    let mut stream = std::net::TcpStream::connect(address).expect("server information endpoint");
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(format!("GET /info HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let at = bytes.windows(4).position(|b| b == b"\r\n\r\n").expect("HTTP response") + 4;
    serde_json::from_slice(&bytes[at..]).expect("information JSON")
}

#[test]
#[ignore = "needs a running server"]
fn probe_a_running_server() {
    let Ok(address) = std::env::var("BFE_PROBE_ADDRESS") else { return };
    let info = info(&address);
    println!("probe ours protocol={} simulation={}", contract::PROTOCOL, contract::SIMULATION);
    println!("probe server protocol={} simulation={}", info["protocol"], info["simulation"]);
    let id = info["id"].as_str().expect("server identity");
    let mut server_id = [0u8; 32];
    for (i, byte) in server_id.iter_mut().enumerate() { *byte = u8::from_str_radix(&id[i * 2..i * 2 + 2], 16).unwrap(); }
    let hello = serde_json::to_vec(&serde_json::json!({"protocol": contract::PROTOCOL, "profile": contract::PROFILE,
        "simulation": contract::SIMULATION, "content": info["fingerprint"], "name": "Probe"})).unwrap();
    let mut endpoint = Endpoint::connect(&address, Identity::generate().unwrap(), server_id, &hello).expect("connect");
    match endpoint.establish(Duration::from_secs(5), &mut |_, _, _, _| {}) {
        Ok(()) => println!("probe result admitted"),
        Err(error) => println!("probe result refused: {}", endpoint.failure(error)),
    }
}
