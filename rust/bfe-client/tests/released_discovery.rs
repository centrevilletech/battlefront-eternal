//! A 2004 server's information reply and signed hello, as the released
//! client reads them, on a generated corpus of extensions the released reader
//! ignores (another game's keys among them). The summary was captured from
//! the released build with this same file.
use bfe_wide::{Fnv, Wide};
use bfe_client::online::info::{authenticated, Server};
use bfe_transport::layout::network_contract as contract;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn reply() -> Value {
    json!({"id": "ab".repeat(32), "protocol": contract::PROTOCOL, "profile": contract::PROFILE,
        "simulation": contract::SIMULATION, "content": "bfe-content-sha256-00", "query": "?map=nab2&side=rep",
        "name": "A server", "players": 3, "maxPlayers": 16})
}

fn read(value: &Value) -> String {
    match serde_json::from_slice::<Server>(&serde_json::to_vec(value).unwrap()) {
        Ok(server) => match server.validate() { Ok(()) => "ok".into(), Err(e) => e },
        Err(e) => format!("parse {}", e.classify() as u8),
    }
}

fn hello(value: &Value) -> String {
    match authenticated(&serde_json::to_vec(value).unwrap()) { Ok(()) => "ok".into(), Err(e) => e }
}

#[test]
fn extensions_a_2004_reply_ignores() {
    // The replies a released client accepted with keys it never read.
    for (key, value) in [("family", json!("swbf1")), ("family", json!("unrelated")), ("mode", json!("conquest")),
        ("families", json!(["swbf1", 3])), ("families", json!(["swbf2", null]))]
    {
        let mut info = reply();
        info[key] = value.clone();
        assert_eq!(read(&info), "ok", "{key}: {value}");
        let mut signed = json!({"protocol": contract::PROTOCOL, "profile": contract::PROFILE, "simulation": contract::SIMULATION,
            "content": "c", "fingerprint": "f"});
        signed[key] = value;
        assert_eq!(hello(&signed), "ok");
    }
}

#[test]
fn the_released_discovery_readers() {
    let keys = ["family", "mode", "families", "match", "records", "battle", "next", "name", "content", "query", "rotation"];
    let values = [json!(null), json!("swbf1"), json!("swbf2"), json!("swbf2/tat2"), json!("eli"), json!(["swbf1", "swbf2"]),
        json!([1, null, "x"]), json!({"query": "?map=kas2"}), json!({"family": "swbf2"}), json!(7), json!(-1), json!(true),
        json!("x".repeat(500)), json!([]), json!({})];
    let mut state = Wide::new(0x1d15, 0xc0de_2004);
    let mut next = || {
        state = state.xor(state.shl(13));
        state = state.xor(state.shr(7));
        state = state.xor(state.shl(17));
        state
    };
    let mut outcomes = BTreeMap::<String, u32>::new();
    let mut digest = Fnv::new();
    for case in 0..4_096 {
        let mut info = reply();
        let mut signed = json!({"protocol": contract::PROTOCOL, "profile": contract::PROFILE, "simulation": contract::SIMULATION,
            "content": "c", "fingerprint": "f"});
        for _ in 0..1 + next().rem(3) {
            let key = keys[next().rem(keys.len() as u32) as usize];
            let value = values[next().rem(values.len() as u32) as usize].clone();
            info[key] = value.clone();
            signed[key] = value;
        }
        if case % 7 == 0 { info["protocol"] = json!(contract::PROTOCOL - 1); signed["simulation"] = json!("bfe-sim-1"); }
        // The version texts name this build's own simulation number, the
        // one value that differs from the released build by design.
        let ours = contract::SIMULATION.trim_start_matches("bfe-sim-");
        let mask = |s: String| s.replace(ours, "N");
        for outcome in [mask(format!("info {}", read(&info))), mask(format!("hello {}", hello(&signed)))] {
            digest.bytes(outcome.as_bytes());
            *outcomes.entry(outcome.chars().take(40).collect()).or_default() += 1;
        }
    }
    let mut text: Vec<String> = outcomes.iter().map(|(k, n)| format!("{k}={n}")).collect();
    text.push(format!("digest={:016x}", digest.finish()));
    let text = text.join(";");
    println!("released discovery {text}");
    assert_eq!(text, RELEASED);
}

const RELEASED: &str = "hello This server runs an older version =586;hello ok=3510;info This server runs another version of=347;info invalid server information=53;info ok=1931;info parse 1=51;info parse 2=1714;digest=9443c35e306e1b5f";
