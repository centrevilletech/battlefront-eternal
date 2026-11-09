//! Browser hostDialog and directConnect behavior over synthetic shell data.
use bfe_shell::{
    data::{MapEraInfo, ShellData, ShellMap},
    launch::{GameFamily, PlaylistEntry},
    screens::{HostControl, ScreenId, ShellAction, ShellApp},
};
fn app() -> ShellApp {
    let map = |id: &str| ShellMap {
        family: GameFamily::Swbf1,
        id: id.into(),
        name: id.into(),
        name_key: None,
        extracted: Some(true),
        eras: [
            ("cw".into(), MapEraInfo::default()),
            ("gcw".into(), MapEraInfo::default()),
        ]
        .into(),
        fly: None,
        load: None,
    };
    let mut app = ShellApp::new(
        ShellData {
            maps: vec![map("nab2"), map("bes2")],
            ..ShellData::default()
        },
        None,
    );
    app.profile.name = "PLAYER TWO".into();
    app.unlock_gate();
    app.push_screen(ScreenId::Multiplayer);
    app.request_host();
    app
}
#[test]
fn host_dialog_edits_one_map_without_using_instant_action() {
    let mut app = app();
    app.ia_entries.push(PlaylistEntry::new("tat2", Some("gcw")));
    assert_eq!(app.host_config.server_name, "My Battlefront server");
    app.host_control(HostControl::Select(2));
    app.host_control(HostControl::Choice(1));
    app.host_control(HostControl::Select(3));
    app.host_control(HostControl::Choice(3));
    app.host_control(HostControl::Select(5));
    app.host_control(HostControl::Choice(2));
    app.host_config.server_name = "Edited battle".into();
    app.host_form.players = "12".into();
    app.host_control(HostControl::Start);
    assert!(app.host_form.error.is_none());
    assert_eq!(app.host_config.server_name, "Edited battle");
    assert_eq!(app.host_config.max_players, 12);
    assert_eq!(app.host_config.relay_policy, "never");
    let Some(ShellAction::LaunchMultiplayer(launch)) = app.take_action() else {
        panic!("host launch")
    };
    assert_eq!(launch.map, "bes2");
    assert_eq!(launch.side, "imp");
    assert_eq!(launch.era, "gcw");
    assert_eq!(app.ia_entries[0].map, "tat2");
}
#[test]
fn invalid_players_do_not_launch_and_public_notice_can_be_reopened() {
    let mut app = app();
    for value in ["", "1", "65", "abc"] {
        app.host_form.players = value.into();
        app.host_control(HostControl::Start);
        assert!(app.host_form.error.is_some());
        assert!(app.take_action().is_none());
    }
    app.host_form.players = "8".into();
    app.mp_needs_notice = true;
    app.host_control(HostControl::Select(1));
    app.host_control(HostControl::Choice(1));
    app.host_control(HostControl::Start);
    assert!(app.popup.is_some());
    app.back();
    assert_eq!(app.current_screen(), ScreenId::HostDialog);
    assert!(app.mp_needs_notice);
    app.host_control(HostControl::Start);
    assert!(app.popup.is_some());
    assert!(app.take_action().is_none());
    if let Some(bfe_shell::screens::PopupState::OnlineNotice {
        selected_accept, ..
    }) = app.popup.as_mut()
    {
        *selected_accept = true;
    }
    app.accept(&mut bfe_online::browser::ServerBrowser::default());
    assert!(matches!(
        app.take_action(),
        Some(ShellAction::LaunchMultiplayer(_))
    ));
}
#[test]
fn direct_connect_uses_address_and_port_and_brackets_ipv6() {
    for address in ["127.0.0.1", "::1"] {
        let mut app = app();
        app.back();
        app.push_screen(ScreenId::DirectConnect);
        app.direct_connect_draft = address.into();
        app.direct_port_draft = "19471".into();
        app.accept(&mut bfe_online::browser::ServerBrowser::default());
        let Some(ShellAction::LaunchMultiplayer(launch)) = app.take_action() else {
            panic!("direct launch")
        };
        assert_eq!(launch.port, Some(19471));
    }
}
