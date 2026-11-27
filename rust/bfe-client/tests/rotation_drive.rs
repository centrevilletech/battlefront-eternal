//! One player on a mixed rotation, against a real server on one port: a 2005
//! battle sat out, a 2004 battle played, and a 2005 battle sat out again. Every
//! move between battles is the client's own (the sit-out session's, the battle
//! receiver's announcement, the readiness probe and the join), with no action
//! of the player's. The server hosts the 2005 battles under the test-only open
//! gate; the client sits them out because its gate is closed, though it has
//! the map, because it lacks the map, or because its copy of the map is not
//! the server's: then it offers its own digest, the server signs the sit-out
//! role, and the join's connection goes to the map-free session.
#[path = "../../bfe-server/tests/common/mod.rs"]
mod common;

use bfe_client::online::{
    info, lobby_client::LobbyClient, match2005, receiver::Receiver, rotation::Probe,
    sitout::{Next, SitOut, Why},
};
use bfe_family::{Gate, GameFamily};
use bfe_server::{config::ServerConfig, identity::ServerIdentity, local_server::LocalDedicatedServer};
use bfe_transport::connection::Lane;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let bytes = bfe_transport::crypto::random::<8>().unwrap_or([0x33; 8]);
    let dir = std::env::temp_dir().join(format!("bfe-client-rotation-{name}-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Steps `step` every few milliseconds until it gives a value, for at most `seconds`.
fn until<T>(seconds: u64, what: &str, mut step: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(value) = step() {
            return value;
        }
        assert!(start.elapsed() < Duration::from_secs(seconds), "{what}: not within {seconds} s");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Joins what the server runs and sits it out: the map-free connection, its
/// notice and score table. A client whose copy of the map is another than the
/// server's offers it; the server's signed answer gives the sit-out role, and
/// the connection, with what it held while that answer was checked (as the
/// battle client holds it), goes to the map-free session.
fn sit_out(address: &str, root: &Path, key: &Path, gate: Gate, why: Why, token: &str) -> SitOut {
    let (server, selection, mut endpoint) = info::prepare(address, root, key, "Pilot", None, gate).unwrap();
    assert_eq!(selection.family, GameFamily::Swbf2);
    assert!(server.query.contains(token), "{}", server.query);
    let expected = selection.join.clone().unwrap();
    let client = if why == Why::OtherData {
        assert!(!selection.sits_out() && expected.content.is_some(), "a client with a copy of the map offers it");
        let mut held = match2005::Held::default();
        until(10, "the signed answer", || {
            endpoint.pump(&mut |lane, text, bytes, now| if text || lane == Lane::State { assert!(held.hold(text, bytes, now)); }).unwrap();
            endpoint.ready().then_some(())
        });
        let joined = match2005::accept(&endpoint.server_hello, &expected).unwrap();
        assert_eq!(joined.role, match2005::Role::SitOut, "another digest is given the sit-out role");
        LobbyClient::resume(endpoint, expected, held)
    } else {
        assert!(selection.sits_out(), "a join that does not play the 2005 battle sits it out");
        LobbyClient::connect(endpoint, expected)
    };
    let mut sitting = SitOut::new(client, address.to_string(), server.query.clone(), why);
    until(10, "the sit-out notice and score table", || {
        assert_eq!(sitting.step(), Ok(None));
        (sitting.client.joined().is_some() && sitting.client.lobby.notice.is_some() && sitting.client.lobby.board.is_some()).then_some(())
    });
    sitting
}

/// A content root with the 2004 content of `real` (each of its entries
/// linked) or, without it, the invented 2004 map; and the invented 2005 battle
/// when asked. Returns the root and the 2004 map the rotation plays.
fn content_root(dir: &Path, real: Option<&Path>, with_2005: bool) -> (PathBuf, &'static str) {
    let (root, map) = match real {
        Some(real) => {
            let root = dir.join("content");
            std::fs::create_dir_all(&root).unwrap();
            for entry in std::fs::read_dir(real).unwrap().flatten() {
                if entry.file_name() != "swbf2" {
                    link(&entry.path(), &root.join(entry.file_name()));
                }
            }
            (root, "geo1")
        }
        None => (common::minimal_content_dir(dir), "nab2"),
    };
    if with_2005 {
        common::invented_battle(&GameFamily::Swbf2.content_dir(&root));
    }
    (root, map)
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) {
    std::os::unix::fs::symlink(from, to).unwrap();
}
#[cfg(not(unix))]
fn link(_: &Path, _: &Path) {
    unreachable!("prepared content is linked on Unix only");
}

/// The client's copy of the 2005 map: none, the server's, or another.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Copy2005 { Missing, Same, Other }

fn drive(name: &str, gate: Gate, copy: Copy2005) {
    let dir = scratch(name);
    // With the player's prepared content the 2004 battle is a real one, which
    // sends state; the invented 2004 map admits no world, so only its welcome.
    let real = std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from).filter(|p| cfg!(unix) && p.join("maps").join("geo1").is_dir());
    let (root, map_2004) = content_root(&dir.join("server"), real.as_deref(), true);
    // The client's own copy: the same 2004 content, and the 2005 map when asked,
    // the server's or one whose classes have other energy.
    let (client_root, _) = content_root(&dir.join("client"), real.as_deref(), copy != Copy2005::Missing);
    if copy == Copy2005::Other {
        let units = GameFamily::Swbf2.content_dir(&client_root).join("sides/rep/units.json");
        let other = std::fs::read_to_string(&units).unwrap().replace(r#"["EnergyBar","50"]"#, r#"["EnergyBar","83"]"#);
        std::fs::write(&units, other).unwrap();
    }
    let entry_2004 = format!("swbf1/{map_2004}:rep:conquest");
    let key = dir.join("player.key");
    let identity = ServerIdentity::load(&dir).unwrap();
    let config = |entry: &str| ServerConfig { data: root.to_string_lossy().into_owned(), rotation: vec![entry.into()], game_port: Some(0),
        discovery: false, gate: bfe_test_gate::OPEN, ..Default::default() };
    let (first_token, second_token, third_token) = ("a".repeat(32), "b".repeat(32), "c".repeat(32));
    // What the server announces: the exact query its next battle runs.
    let next_query = |entry: &str, token: &str| {
        let config = config(entry);
        let query = bfe_server::config::query_for_rotation(entry, &config.ai).unwrap();
        bfe_server::local_server::with_match_param(&bfe_server::config::with_friendly_fire(&query, config.friendly_fire), token)
    };
    let why = match (gate.plays(GameFamily::Swbf2), copy) {
        (false, _) => Why::Unavailable,
        (true, Copy2005::Other) => Why::OtherData,
        (true, _) => Why::NoMap,
    };

    // 1. A 2005 battle, sat out.
    let first = LocalDedicatedServer::start_with_match_token(&config("swbf2/yard:cw:duel"), &identity, |_| false, |_, _, _, _, _, _| {},
        Some(&first_token), None).unwrap();
    let port = first.port;
    let address = format!("127.0.0.1:{port}");
    let mut sitting = sit_out(&address, &client_root, &key, gate, why, &first_token);
    assert!(sitting.message().starts_with("SITTING OUT: "), "{}", sitting.message());
    if !gate.plays(GameFamily::Swbf2) {
        assert!(sitting.message().contains("not available in this build"), "{}", sitting.message());
    }
    if copy == Copy2005::Other {
        assert!(sitting.message().contains("other Battlefront II game data than yours"), "{}", sitting.message());
    }
    println!("{name}: {}", sitting.message());

    // 2. The server announces a 2004 battle and runs it on the same port: the
    //    session follows it once the server names it.
    first.match_rotate(&next_query(&entry_2004, &second_token), 200);
    until(10, "the announcement", || { sitting.step().unwrap(); sitting.client.lobby.next.is_some().then_some(()) });
    let handed = first.hand_over();
    let second = LocalDedicatedServer::start_with_match_token(&config(&entry_2004), &identity, |_| false, |_, _, _, _, _, _| {},
        Some(&second_token), Some(handed)).unwrap();
    let played = second.ready.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(played, real.is_some(), "a real 2004 battle admits its world");
    second.udp.lock().unwrap().pending_match = None;
    assert_eq!(second.port, port);
    let next = until(30, "the next battle", || sitting.step().unwrap());
    assert!(matches!(&next, Next::Announced(query) if query.contains(&second_token)), "{next:?}");
    drop(sitting);

    // 3. The 2004 battle, played: the released hello, the welcome and state.
    let (server, selection, mut endpoint) = info::prepare(&address, &client_root, &key, "Pilot", None, gate).unwrap();
    assert_eq!((selection.family, selection.join.is_none()), (GameFamily::Swbf1, true));
    assert!(server.query.contains(&second_token));
    // The battle's announcements go through the battle receiver.
    let mut receiver = Receiver::new(serde_json::json!({}));
    let (mut state, mut welcomed, mut started) = (0usize, false, false);
    let take = |receiver: &mut Receiver, welcomed: &mut bool, text: bool, bytes: &[u8], now: f64| {
        let Ok(event) = serde_json::from_slice::<serde_json::Value>(bytes) else { return };
        match event["type"].as_str() {
            Some("welcome") if text => *welcomed = true,
            Some("match") if text => receiver.message(true, bytes, now),
            _ => {}
        }
    };
    until(30, "the 2004 battle's welcome and state", || {
        endpoint.pump(&mut |lane, text, bytes, now| {
            if text { take(&mut receiver, &mut welcomed, text, bytes, now); } else if lane == Lane::State { state += 1; }
        }).unwrap();
        if welcomed && !started {
            started = endpoint.send(Lane::Events, true, br#"{"type":"call","id":1,"method":"start","args":{}}"#).unwrap();
            endpoint.flush().unwrap();
        }
        (welcomed && (!played || state >= 3)).then_some(())
    });
    println!("{name}: 2004 battle on {map_2004}: {state} state datagrams");

    // 4. The server announces a 2005 battle: the battle's receiver takes it as
    //    a current one, the readiness probe finds the server running it, and the
    //    join sits it out.
    second.match_rotate(&next_query("swbf2/yard:cw:duel", &third_token), 200);
    until(10, "the 2005 announcement", || {
        endpoint.pump(&mut |_, text, bytes, now| if text { take(&mut receiver, &mut welcomed, text, bytes, now); }).unwrap();
        (receiver.next_match.is_some() || receiver.error().is_some()).then_some(())
    });
    assert!(receiver.error().is_none(), "an announced 2005 battle is not refused: {:?}", receiver.error());
    let announced = receiver.next_match.as_ref().unwrap().query.clone();
    let handed = second.hand_over();
    let third = LocalDedicatedServer::start_with_match_token(&config("swbf2/yard:cw:duel"), &identity, |_| false, |_, _, _, _, _, _| {},
        Some(&third_token), Some(handed)).unwrap();
    third.udp.lock().unwrap().pending_match = None;
    assert_eq!(third.port, port);
    let probe = Probe::start(address.clone(), announced);
    until(30, "the 2005 battle's readiness", || probe.ready().then_some(()));
    drop(endpoint);
    let sitting = sit_out(&address, &client_root, &key, gate, why, &third_token);
    assert!(sitting.client.lobby.board.as_ref().is_some_and(|board| board.match_token == third_token));
    drop(sitting);
    drop(third);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The drive with the 2005 game open on the client, which lacks the 2005
/// map.
#[test]
fn a_mixed_rotation_moves_through_a_2005_sit_out_a_2004_battle_and_another_sit_out() {
    drive("open", bfe_test_gate::OPEN, Copy2005::Missing);
}

/// The drive with the 2005 game open on a client whose copy of the 2005 map
/// is not the server's: it offers its digest, is given the sit-out role and
/// sits the battle out with the content-difference notice.
#[test]
fn a_client_with_other_2005_data_sits_out_on_its_signed_role_and_follows_the_rotation() {
    drive("other", bfe_test_gate::OPEN, Copy2005::Other);
}

/// With the gate closed, a player who has the 2005 map still sits the 2005
/// battles out, and returns for the 2004 battle with no action of theirs.
#[test]
fn with_the_gate_closed_a_player_sits_out_2005_battles_and_returns_for_2004() {
    drive("closed", Gate::BUILT, Copy2005::Same);
}

/// A sit-out join whose match ended between discovery and the handshake: the
/// server refuses the old match's token, and the session looks for the
/// server's current battle and follows it, with no action of the player's.
#[test]
fn a_sit_out_join_for_a_match_that_ended_follows_the_current_one() {
    let dir = scratch("stale");
    let (root, _) = content_root(&dir.join("server"), None, true);
    let (client_root, _) = content_root(&dir.join("client"), None, true);
    let key = dir.join("player.key");
    let identity = ServerIdentity::load(&dir).unwrap();
    let config = ServerConfig { data: root.to_string_lossy().into_owned(), rotation: vec!["swbf2/yard:cw:duel".into()], game_port: Some(0),
        discovery: false, gate: bfe_test_gate::OPEN, ..Default::default() };
    let (old_token, new_token) = ("d".repeat(32), "e".repeat(32));
    let first = LocalDedicatedServer::start_with_match_token(&config, &identity, |_| false, |_, _, _, _, _, _| {}, Some(&old_token), None).unwrap();
    let address = format!("127.0.0.1:{}", first.port);
    // Discovery names the old match, which this closed build sits out ...
    let server = info::Server::read(&address).unwrap();
    let mut selection = server.selection().unwrap();
    let hello = info::join_hello(&client_root, &server, &mut selection, "Pilot", Gate::BUILT).unwrap();
    assert!(selection.sits_out() && server.query.contains(&old_token), "{}", server.query);
    // ... but that match ends before the handshake, and the next one runs on the same port.
    let second = LocalDedicatedServer::start_with_match_token(&config, &identity, |_| false, |_, _, _, _, _, _| {}, Some(&new_token),
        Some(first.hand_over())).unwrap();
    let endpoint = bfe_transport::endpoint::Endpoint::connect(&address, info::identity(&key).unwrap(), server.identity().unwrap(), &hello).unwrap();
    let client = LobbyClient::connect(Box::new(endpoint), selection.join.clone().unwrap());
    let mut sitting = SitOut::new(client, address.clone(), server.query.clone(), Why::Unavailable);
    let next = until(30, "the server's current battle", || sitting.step().unwrap());
    assert_eq!(next, Next::Current);
    assert!(sitting.client.changed() && sitting.client.joined().is_none());
    assert!(sitting.client.refusal().is_some_and(|refusal| refusal.contains("match-changed")), "{:?}", sitting.client.refusal());
    println!("stale: refused with {:?}; followed the current battle", sitting.client.refusal());
    drop(sitting);
    // The current battle, joined afresh: sat out under the new token.
    let sitting = sit_out(&address, &client_root, &key, Gate::BUILT, Why::Unavailable, &new_token);
    assert!(sitting.client.lobby.board.as_ref().is_some_and(|board| board.match_token == new_token));
    drop(sitting);
    drop(second);
    let _ = std::fs::remove_dir_all(&dir);
}
