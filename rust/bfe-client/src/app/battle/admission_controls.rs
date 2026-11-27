use super::next_online_launch;
use crate::{
    generated_input::ClientAction,
    input::{BattleInput, Input},
    online::{info::Server, rotation::NextMatch, selection::Selection},
};
use bfe_family::GameFamily;
use bfe_interface::{hotkeys::ControlContext, Settings};
use bfe_online::browser::{RotationItem, ServerEntry};
use bfe_shell::{
    launch::{parse_rotation, BattleRequest},
    screens::{direct_connect_entry, resolve_server_join, MultiplayerKind, MultiplayerLaunch},
};
use winit::{event::MouseButton, keyboard::KeyCode};

fn selection(query: &str, summary: serde_json::Value) -> Selection {
    let body = serde_json::json!({
        "id": "fixture", "protocol": 1, "profile": "test", "simulation": "test",
        "content": "test", "query": query, "battle": summary
    });
    Server::parse(&serde_json::to_vec(&body).unwrap())
        .unwrap()
        .selection()
        .unwrap()
}

fn check_released_input(family: GameFamily) {
    assert_eq!(family, GameFamily::Swbf1);
    let mut settings = Settings::new();
    ControlContext::Infantry.rebind(&mut settings, "holds.sprint", Some("KeyJ"));
    let mut actual = BattleInput::default();
    actual.set_profile(family, &settings);
    let mut released = Input::default();
    for key in [
        KeyCode::KeyJ,
        KeyCode::Space,
        KeyCode::KeyW,
        KeyCode::ShiftLeft,
    ] {
        actual.key(key, true);
        released.key(key, true);
    }
    actual.mouse(MouseButton::Middle, true);
    released.mouse(MouseButton::Middle, true);
    for (tick, active) in [true, true, false, true].into_iter().enumerate() {
        let tick = tick as u32;
        let (commands, extra) = actual.sample_battle(tick, 1, active, 1.5, 1., false);
        assert_eq!(commands, released.sample(tick, 1, active, 1.5, 1., false));
        assert_eq!(extra, Default::default());
    }
}

fn start(launch: &MultiplayerLaunch, selection: &Selection) {
    let (map, _) = selection.display(&launch.map, &launch.era);
    assert_eq!(map.value(), selection.map);
    check_released_input(selection.family);
}

#[test]
fn local_and_network_starts_keep_the_released_controls() {
    let request = BattleRequest::from_rotation(
        None,
        None,
        parse_rotation(Some("sample:rep,second:all")),
        0,
        false,
    )
    .unwrap();
    check_released_input(request.battle_at(0).family);
    check_released_input(request.next(|| 0.5).battle_at(1).family);

    let announced = format!(
        "?map=s%65cond&side=all&family=swbf2&match={}",
        "a1".repeat(16)
    );
    let current = selection(
        "?map=s%61mple&family=swbf2",
        serde_json::json!({"mode":"night"}),
    );
    assert_eq!((current.family, current.map.as_str()), (GameFamily::Swbf1, "sample"));
    assert!(!current.named);
    let address = std::net::Ipv4Addr::LOCALHOST.to_string();
    let direct = direct_connect_entry(&address).unwrap();
    let listing = ServerEntry {
        rotation: vec![RotationItem::Slug("swbf2/earlier:gcw:hunt".into())],
        ..Default::default()
    };
    let lan = ServerEntry {
        address: Some(address),
        ..listing.clone()
    };
    for (row, kind) in [
        (&direct, MultiplayerKind::Lan),
        (&listing, MultiplayerKind::Online),
        (&lan, MultiplayerKind::Lan),
    ] {
        let launch = resolve_server_join(row, kind, "Pilot");
        assert!(launch.refused.is_none());
        start(&launch, &current);

        let rejoin = next_online_launch(&launch, None);
        assert!(rejoin.map.is_empty());
        start(&rejoin, &current);

        let next = NextMatch::new(announced.clone(), Some(0.), 0.).unwrap();
        let handover = next_online_launch(&launch, Some(&next));
        assert_eq!(
            (handover.map.as_str(), handover.side.as_str()),
            ("second", "all")
        );
        let fresh = selection(&announced, serde_json::json!({"family":"swbf1"}));
        assert!(fresh.named);
        assert_eq!(fresh.mode, "conquest");
        start(&handover, &fresh);
    }
}

#[test]
fn a_fresh_selection_changes_the_bindings_on_the_same_map() {
    let current = selection("?map=sample&family=swbf2", serde_json::json!({}));
    let mut launch = resolve_server_join(&ServerEntry::default(), MultiplayerKind::Online, "Pilot");
    launch.family = GameFamily::Swbf2;
    let mut settings = Settings::new();
    ControlContext::Infantry.rebind(&mut settings, "holds.sprint", Some("KeyJ"));
    let mut input = BattleInput::default();
    assert_ne!(launch.family, current.family);
    input.set_profile(current.family, &settings);
    input.key(KeyCode::KeyJ, true);

    let next = NextMatch::new(format!("?map=sample&match={}", "b2".repeat(16)), None, 0.).unwrap();
    let mut launch = next_online_launch(&launch, Some(&next));
    assert_eq!(
        input.sample_battle(1, 1, true, 1., 1., false).1,
        Default::default()
    );
    launch.family = GameFamily::Swbf1;
    // Supply an admitted battle directly while public joins for this game are unavailable.
    let admitted = Selection {
        family: GameFamily::Swbf2,
        named: true,
        map: "sample".into(),
        side: String::new(),
        era: String::new(),
        mode: "night".into(),
        query: next.query,
        join: None,
        pinned: None,
    };
    assert_eq!(admitted.family.qualify(&admitted.map), "swbf2/sample");
    let (map, _) = admitted.display(&launch.map, &launch.era);
    assert_eq!((map.value(), admitted.mode.as_str()), ("sample", "night"));
    input.set_profile(admitted.family, &settings);
    assert!(!input.sample_battle(2, 1, true, 1., 1., false).1.sprint);
    input.key(KeyCode::KeyJ, true);
    input.mouse(MouseButton::Middle, true);
    let ((ground, _, _, _), extra) = input.sample_battle(3, 1, true, 1., 1., false);
    assert!(extra.sprint);
    assert_eq!(ground.actions, 1 << ClientAction::Zoom as u8);
    input.set_profile(current.family, &settings);
    assert_eq!(
        input.sample_battle(4, 1, true, 1., 1., false).1,
        Default::default()
    );
}

#[test]
fn announcements_are_checked_before_showing_a_map_and_rejoin_keeps_the_fallbacks() {
    let mut launch = resolve_server_join(&ServerEntry::default(), MultiplayerKind::Online, "Pilot");
    for map in ["other/sample", "swbf1/sample"] {
        let query = format!("?map={map}&match={}", "c3".repeat(16));
        assert!(NextMatch::new(query, Some(0.), 0.).is_err());
    }
    // A 2005 next battle is loadable in principle: the join that follows it
    // plays it or sits it out, so its announcement is accepted here.
    let next = NextMatch::new(format!("?map=swbf2%2Fsample&match={}", "c3".repeat(16)), Some(0.), 0.).unwrap();
    let announced = next_online_launch(&launch, Some(&next));
    assert_eq!((announced.family, announced.map.as_str()), (GameFamily::Swbf2, "sample"));
    let no_map = selection("?family=swbf2&side=all", serde_json::json!({}));
    launch.map = "fallback".into();
    launch.era = "gcw".into();
    let (map, era) = no_map.display(&launch.map, &launch.era);
    assert_eq!((map.value(), era.value()), ("fallback", "gcw"));
    let with_era = selection("?era=cw&side=all", serde_json::json!({}));
    assert_eq!(with_era.display(&launch.map, &launch.era).1.value(), "cw");
    launch.map.clear();
    launch.era.clear();
    let (map, era) = no_map.display(&launch.map, &launch.era);
    assert_eq!((map.value(), era.value()), ("nab2", "cw"));
}
