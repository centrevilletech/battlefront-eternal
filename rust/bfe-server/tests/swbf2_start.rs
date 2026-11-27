//! The start of a 2005 battle, in a test binary of its own: no other test in
//! this process computes a content digest while this one counts them.
mod common;

use bfe_content::files::DirFiles;
use bfe_family::GameFamily;
use bfe_server::{config::ServerConfig, identity::ServerIdentity, local_server::LocalDedicatedServer};
use bfe_test_gate::OPEN;
use std::fs;

/// A 2005 battle is started and advertised only after one verified
/// admission: one content digest per start, its world's, which the service
/// lists. A world whose admission is refused starts nothing (no socket, no
/// signed match, no listing, no welcome); the start's error is the refusal.
#[test]
fn a_2005_battle_starts_only_after_one_verified_admission() {
    use bfe_content_native::fingerprint::{family_digests, hash_for_family};
    let bytes = bfe_transport::crypto::random::<8>().unwrap_or([0x22; 8]);
    let dir = std::env::temp_dir().join(format!("bfe-server-test-start-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    fs::create_dir_all(&dir).unwrap();
    let root = common::minimal_content_dir(&dir);
    let folder = GameFamily::Swbf2.content_dir(&root);
    let identity = ServerIdentity::load(&dir).unwrap();
    let config = ServerConfig { data: root.to_string_lossy().into_owned(), rotation: vec!["swbf2/yard:cw:duel".into()], game_port: Some(0),
        discovery: false, gate: OPEN, ..Default::default() };
    let start = || LocalDedicatedServer::start(&config, &identity, |_| false, |_, _, _, _, _, _| {});

    common::invented_battle_rolling(&folder, 25, false);
    let before = family_digests();
    assert_eq!(start().err().as_deref(), Some("Session: the Battlefront II class blue_trooper has no movement numbers: missing ControlSpeed"));
    assert_eq!(family_digests() - before, 1, "the refused world's one digest");

    common::invented_battle_rolling(&folder, 25, true);
    let before = family_digests();
    let server = start().unwrap();
    assert_eq!(family_digests() - before, 1, "one digest for the whole start");
    assert!(server.ready.load(std::sync::atomic::Ordering::SeqCst));
    let query = server.query.clone();
    let content_query = bfe_family::swbf2::content_query(&query, "yard").unwrap();
    let digest = hash_for_family(&DirFiles::from_resolved_root(&folder, GameFamily::Swbf2), GameFamily::Swbf2, &content_query).unwrap();
    assert_eq!((server.fingerprint.as_str(), server.info()["fingerprint"].as_str()), (digest.as_str(), Some(digest.as_str())));
    drop(server);

    // A scripted battle starts the same way, from one digest.
    common::invented_mission(&folder, &["cw"]);
    let config = ServerConfig { rotation: vec!["swbf2/yard:cw:eli".into()], ..config };
    let before = family_digests();
    let server = LocalDedicatedServer::start(&config, &identity, |_| false, |_, _, _, _, _, _| {}).unwrap();
    assert_eq!(family_digests() - before, 1, "one digest for a scripted battle's whole start");
    assert!(server.ready.load(std::sync::atomic::Ordering::SeqCst));
    let content_query = bfe_family::swbf2::content_query(&server.query, "yard").unwrap();
    let digest = hash_for_family(&DirFiles::from_resolved_root(&folder, GameFamily::Swbf2), GameFamily::Swbf2, &content_query).unwrap();
    assert_eq!(server.fingerprint, digest);
    drop(server);
    let _ = fs::remove_dir_all(&dir);
}
