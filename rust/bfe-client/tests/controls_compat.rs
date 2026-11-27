//! Synthetic profiles and device events compared with the released client.
use bfe_client::{
    app::settings::AppSettingsStore,
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    input::BattleInput as Input,
};
use bfe_desktop::paths::{resolve_desktop_paths, DesktopPaths, DesktopPathsOptions};
use bfe_interface::{
    widgets::{Widget, WidgetAction}, MenuInputEvent, OptionsContext, OptionsController, Settings,
};
use std::{collections::BTreeMap, fmt::Write, fs, path::Path};
use winit::{event::MouseButton, keyboard::KeyCode};

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/controls").join(name)).unwrap()
}

fn compare(name: &str, actual: &str) {
    assert_eq!(actual, fixture(name), "{name}");
}

fn profile_paths(case: &str) -> DesktopPaths {
    let dir = std::env::temp_dir().join(format!("bfe-controls-{}-{case}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    resolve_desktop_paths(&DesktopPathsOptions {
        home: Some(dir.clone()),
        env: BTreeMap::from([("BFE_USER_DATA_DIR".into(), dir.to_str().unwrap().into())]),
        ..Default::default()
    })
}

#[test]
fn native_profile_bytes_match_the_released_writer() {
    let default = fixture("default.json");
    let rebound = default
        .replace(r#""keyBindings": {}"#, r#""keyBindings": {"presses.jump":["KeyJ","Mouse3"],"axes.right-":[]}"#)
        .replace(r#""flightBindings": {}"#, r#""flightBindings": {"brake":["WheelDown"],"takeoff":["KeyU","Space"]}"#)
        .replace(r#""iaPlaylist": []"#, r#""iaPlaylist": [{"family":"swbf1","map":"test","era":"gcw","mode":"conquest"}]"#);
    let irregular = r#"{"complete":true,"values":{
        "keyBindings":{"unknown":["Mouse0"],"presses.jump":["KeyA"],"presses.jump":["KeyK","KeyK","Tab","Unsupported",5],
                       "presses.crouch":["KeyK"],"axes.turn+":false},
        "flightBindings":{"mystery":[],"rollLock":["KeyK","KeyL"]},"unrecognized":5},
        "pauseKv":{"synthetic":"retain"},"otherTopLevel":2}"#;
    let partial = r#"{"complete":false,"profileName":"Test Pilot","values":{"keyBindings":{"presses.reload":["KeyI"]}}}"#;
    for (name, source) in [
        ("default.json", default.as_str()),
        ("rebound.json", rebound.as_str()),
        ("irregular.json", irregular),
        ("partial.json", partial),
        ("default.json", "{}"),
    ] {
        let paths = profile_paths(name);
        fs::write(&paths.player_settings_file, source).unwrap();
        AppSettingsStore::load(paths.clone()).save_player().unwrap();
        compare(name, &fs::read_to_string(&paths.player_settings_file).unwrap());
        fs::remove_dir_all(paths.user_data_dir).unwrap();
    }
}

fn controls_action(ctrl: &mut OptionsController, action: WidgetAction) {
    ctrl.apply_event(MenuInputEvent::Action(action));
}

fn rebind_row(ctrl: &mut OptionsController, id: &str, code: &str) {
    let Some(Widget::List(list)) = ctrl.stack.top().unwrap().widget("bindings") else { panic!() };
    let row = list.rows.iter().find(|r| r.id == id).unwrap();
    controls_action(ctrl, row.action.clone().unwrap());
    ctrl.on_key_down(code, false);
}

#[test]
fn controls_page_matches_the_released_tabs_rows_and_conflicts() {
    let mut context = OptionsContext::default();
    context.strings.insert("ifs.Controls.SoldierTab".into(), "Test tab".into());
    context.strings.insert("ifs.controls.General.Jump".into(), "Test jump".into());
    let mut ctrl = OptionsController::new(Settings::new(), context);
    ctrl.open_page("controls");
    let mut actual = String::new();
    let Some(Widget::Tabs(tabs)) = ctrl.stack.top().unwrap().widget("tabs") else { panic!() };
    let ids: Vec<_> = tabs.tabs.iter().map(|t| (t.id.clone(), t.label.clone())).collect();
    for (tab, title) in ids {
        controls_action(&mut ctrl, WidgetAction::SelectTab(tab.clone()));
        let page = ctrl.stack.top().unwrap();
        writeln!(actual, "{}\t{tab}\t{title}", page.id).unwrap();
        let Some(Widget::Options(opts)) = page.widget("rows") else { panic!() };
        for row in &opts.items {
            if row.id == "controlsFamily" && bfe_family::GameFamily::Swbf2.is_available() {
                continue;
            }
            writeln!(actual, "{}\t{}\t{}", row.id, row.label, row.value_text).unwrap();
        }
        let Some(Widget::List(list)) = page.widget("bindings") else { panic!() };
        for row in &list.rows {
            writeln!(actual, "{}\t{}\t{}\t{}", row.id, row.cells[0], row.cells[1], row.disabled).unwrap();
        }
    }
    controls_action(&mut ctrl, WidgetAction::SelectTab("infantry".into()));
    rebind_row(&mut ctrl, "presses.jump", "KeyW");
    rebind_row(&mut ctrl, "presses.crouch", "KeyW");
    controls_action(&mut ctrl, WidgetAction::SelectTab("starfighter".into()));
    rebind_row(&mut ctrl, "takeoff", "KeyW");
    for key in ["keyBindings", "flightBindings"] {
        writeln!(actual, "{key}\t{}", ctrl.settings.get(key).to_json()).unwrap();
    }
    // macOS displays the physical Alt key as Option.
    compare("page.tsv", &actual.replace("OPTION,", "ALT,").replace("OPTION\t", "ALT\t"));
}

fn sample(input: &mut Input, tick: u32, active: bool, stream: &mut String) {
    let ((cmd, look, flight, view), extra) = input.sample_battle(tick, 4, active, 1.5, 0.75, true);
    assert_eq!(extra, Default::default());
    let mut ground = [0.; ClientCommand::WORDS];
    let mut motion = [0.; ClientLook::WORDS];
    let mut pilot = [0.; ClientFlightCommand::WORDS];
    let mut heap = Vec::new();
    cmd.put(&mut ground, &mut heap);
    look.put(&mut motion, &mut heap);
    flight.put(&mut pilot, &mut heap);
    for word in ground.iter().chain(&motion).chain(&pilot).chain(&heap) {
        write!(stream, "{word} ").unwrap();
    }
    writeln!(stream, "{} {} {}", view.perspective, view.map, view.stats).unwrap();
}

#[test]
fn device_events_match_the_released_command_stream() {
    use KeyCode::*;
    let mut settings = Settings::new();
    let mut input = Input::default();
    input.set_profile(bfe_family::GameFamily::Swbf1, &settings);
    let mut stream = String::new();
    for key in [KeyW, ArrowUp, KeyS, KeyA, KeyD, Space, Space, KeyC, KeyX, KeyQ, KeyM] {
        input.key(key, true);
    }
    input.key_with_modifiers(Backquote, true, true);
    input.device.motion(3.5, -4.25);
    input.mouse(MouseButton::Left, true);
    input.wheel(-4.);
    sample(&mut input, 1, true, &mut stream);
    input.key(KeyW, false);
    input.key(Backquote, false);
    input.key(Backquote, true);
    input.mouse(MouseButton::Middle, true);
    input.mouse(MouseButton::Right, true);
    input.key(AltLeft, true);
    input.mouse(MouseButton::Right, false);
    sample(&mut input, 2, true, &mut stream);
    settings.set("keyBindings", bfe_interface::SettingValue::Bindings(BTreeMap::from([
            ("axes.forward+".into(), vec!["KeyI".into(), "Unsupported".into()]),
            ("holds.firePrimary".into(), vec!["WheelDown".into()]),
            ("presses.jump".into(), vec!["KeyI".into(), "Mouse3".into()]),
            ("presses.reload".into(), vec!["Tab".into()]),
        ])));
    settings.set("flightBindings", bfe_interface::SettingValue::Bindings(BTreeMap::from([
        ("takeoff".into(), vec!["Mouse3".into()]),
    ])));
    input.set_profile(bfe_family::GameFamily::Swbf1, &settings);
    input.key(KeyI, true);
    input.key(Tab, true);
    input.wheel(-1.);
    input.wheel(-1.);
    input.mouse(MouseButton::Back, true);
    sample(&mut input, 3, true, &mut stream);
    sample(&mut input, 4, true, &mut stream);
    input.wheel(-1.);
    input.device.motion(19., 8.);
    sample(&mut input, 5, false, &mut stream);
    sample(&mut input, 6, true, &mut stream);
    input.device.flight_mode = true;
    for key in [KeyW, KeyS, KeyA, KeyD, ShiftRight, KeyQ, CapsLock] {
        input.key(key, true);
    }
    input.mouse(MouseButton::Back, true);
    input.mouse(MouseButton::Right, true);
    input.wheel(1.);
    sample(&mut input, 7, true, &mut stream);
    input.clear();
    sample(&mut input, 8, true, &mut stream);
    compare("commands.txt", &stream);
}

// Frozen native reader types. The released reader first deserializes values
// into a JSON map, then admits only recognized settings from that map.
#[derive(serde::Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct ReleasedPlayer {
    profile_name: String,
    privacy_notice_accepted: bool,
    native_settings_version: u32,
    complete: bool,
    values: serde_json::Map<String, serde_json::Value>,
    shell: ReleasedShell,
    pause_kv: BTreeMap<String, String>,
    ia_playlist: Vec<ReleasedEntry>,
    ia_random: bool,
}

#[derive(serde::Deserialize, Debug, PartialEq)]
struct ReleasedShell {
    master_volume: f32,
    music_volume: f32,
    interface_volume: f32,
    effects_volume: f32,
    menu_theme: String,
    battle_size: String,
    difficulty: String,
    mouse_sensitivity: f32,
    invert_y: bool,
}

#[derive(serde::Deserialize, Debug, PartialEq)]
struct ReleasedEntry {
    family: bfe_family::GameFamily,
    map: String,
    era: Option<String>,
    mode: String,
}

#[test]
fn one_2005_edit_survives_reopen_and_a_released_resave() {
    use bfe_interface::hotkeys::ControlContext;
    use bfe_shell::{data::ShellData, screens::ShellApp};
    use serde_json::{json, Value};

    // Both released seeds contain the same effective settings. The released writer
    // read and resaved both edited files to these exact complete profile bytes.
    let released_resave = fixture("native-true.json");
    for complete in [false, true] {
        let paths = profile_paths(&format!("one-edit-{complete}"));
        fs::write(&paths.player_settings_file, fixture(&format!("native-{complete}.json"))).unwrap();
        let mut store = AppSettingsStore::load(paths.clone());
        let mut app = ShellApp::new(ShellData::default(), None);
        store.apply_to_shell_app(&mut app);
        let mut page = store.interface.clone();
        let binding = BTreeMap::from([("presses.roll".into(), vec!["Mouse4".into()])]);
        ControlContext::Infantry.rebind(&mut page, "presses.roll", Some("Mouse4"));
        assert!(store.capture_from_interface_settings(&page, &mut app).unwrap());
        let mut expected: Value = serde_json::from_str(&released_resave).unwrap();
        expected["swbf2ControlsVersion"] = json!(1);
        expected["values"]["keyBindingsSwbf2"] = json!({"infantry": binding});
        let encoded = fs::read(&paths.player_settings_file).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&encoded).unwrap(), expected);
        let legacy: ReleasedPlayer = serde_json::from_slice(&encoded).unwrap();
        assert!(legacy.complete);
        assert_eq!(legacy.values["keyBindings"]["presses.jump"], json!(["KeyI"]));
        assert_eq!(legacy.values["flightBindings"]["takeoff"], json!(["Mouse3"]));
        assert_eq!(encoded, fixture("edited-ground.json").as_bytes());
        let mut reopened = AppSettingsStore::load(paths.clone());
        assert_eq!(reopened.interface.swbf2_bindings[&ControlContext::Infantry], binding);
        reopened.save_player().unwrap();
        assert_eq!(fs::read(&paths.player_settings_file).unwrap(), encoded);
        let mut page = reopened.interface.clone();
        ControlContext::Starfighter.rebind(&mut page, "takeoff", None);
        assert!(reopened.capture_from_interface_settings(&page, &mut app).unwrap());
        compare("edited-ground-flight.json", &fs::read_to_string(&paths.player_settings_file).unwrap());
        // The released native writer resaved both of the above fixtures to native-true.json.
        fs::write(&paths.player_settings_file, &released_resave).unwrap();
        let mut downgraded = AppSettingsStore::load(paths.clone());
        assert!(downgraded.interface.swbf2_bindings.is_empty());
        assert_eq!(downgraded.interface.get_bindings_ref("keyBindings").unwrap()["presses.jump"], ["KeyI"]);
        assert_eq!(downgraded.interface.get_bindings_ref("flightBindings").unwrap()["takeoff"], ["Mouse3"]);
        downgraded.save_player().unwrap();
        assert_eq!(fs::read_to_string(&paths.player_settings_file).unwrap(), released_resave);
        fs::remove_dir_all(paths.user_data_dir).unwrap();
    }
}

#[test]
fn marker_requires_an_effective_override_and_disappears_after_reset() {
    use bfe_interface::hotkeys::ControlContext::Jedi;
    use serde_json::json;
    let paths = profile_paths("marker");
    for (version, overrides) in [(json!(1), json!({"opaque": ["KeyG"], "holds.block":["KeyF","Mouse1"]})),
        (json!("1"), json!({"holds.block": ["KeyI"]}))] {
        let record = json!({"complete":true, "swbf2ControlsVersion":version, "values":{"keyBindingsSwbf2":{"jedi":overrides}}});
        fs::write(&paths.player_settings_file, record.to_string()).unwrap();
        let mut store = AppSettingsStore::load(paths.clone());
        assert!(store.interface.swbf2_bindings.is_empty());
        Jedi.rebind(&mut store.interface, "holds.block", Some("KeyI"));
        store.save_player().unwrap();
        assert_eq!(store.player.swbf2_controls_version, Some(json!(1)));
        store.interface.swbf2_bindings.clear();
        store.save_player().unwrap();
        assert_eq!(store.player.swbf2_controls_version, None);
        assert!(!store.player.values.contains_key("keyBindingsSwbf2"));
    }
    fs::remove_dir_all(paths.user_data_dir).unwrap();
}

#[test]
fn unmarked_desktop_extensions_match_the_released_readers_and_writer() {
    use bfe_desktop::settings::write_player_settings_value;
    use serde_json::json;

    for complete in [false, true] {
        let paths = profile_paths(&format!("unmarked-{complete}"));
        let golden = fixture(if complete { "unmarked-complete.json" } else { "unmarked-partial.json" });
        for value in [json!({}), json!(false), json!({"legacyAction":["KeyG"]}), json!({"presses.roll":["KeyB"]})] {
            let record = json!({"complete":complete, "values":{"keyBindingsSwbf2":value}});
            assert!(write_player_settings_value(&paths.user_data_dir, &record));
            let text = fs::read_to_string(&paths.player_settings_file).unwrap();
            for interface in [Settings::from_json_str(&text), Settings::try_from_json_str(&text, "profile").unwrap()] {
                assert!(interface.fresh);
                assert!(interface.swbf2_bindings.is_empty());
                assert!(!interface.to_desktop_snapshot().values.contains_key("keyBindingsSwbf2"));
            }
            let mut native = AppSettingsStore::load(paths.clone());
            assert!(native.interface.swbf2_bindings.is_empty());
            native.save_player().unwrap();
            assert_eq!(fs::read_to_string(&paths.player_settings_file).unwrap(), golden);
        }
        fs::remove_dir_all(paths.user_data_dir).unwrap();
    }
}

#[test]
fn actions_follow_the_2005_actor_while_a_button_is_held() {
    use bfe_client::generated_input::{ClientAction as A, ClientHeld as H, ClientFlightAction as F, ClientFlightHeld as FH};
    use bfe_family::GameFamily::{Swbf1, Swbf2};
    use bfe_interface::hotkeys::ControlContext::*;
    use KeyCode::*;

    let mut input = Input::default();
    let mut settings = Settings::new();
    let tick = |i: &mut Input| i.sample_battle(1, 2, true, 1., 1., false);
    input.key(Space, true);
    input.device.motion(2., 3.);
    input.set_profile(Swbf2, &settings);
    let ((cmd, look, _, _), extra) = tick(&mut input);
    assert_eq!((cmd.held, cmd.actions, look.x), (0, 0, 0.));
    assert_eq!(extra, Default::default());

    input.mouse(MouseButton::Middle, true);
    input.key(AltLeft, true);
    input.key(ShiftRight, true);
    input.key(F4, true);
    let ((cmd, _, _, _), extra) = tick(&mut input);
    assert_eq!(cmd.actions, 1 << A::Zoom as u8);
    assert!(extra.roll && extra.sprint && extra.squad_command);
    assert!(!extra.block && !extra.target_lock);

    input.key(KeyQ, true); // An unsampled infantry lock is discarded on the transition.
    input.set_context(Jedi);
    let ((cmd, _, _, _), extra) = tick(&mut input);
    assert_eq!(cmd.actions, 0);
    assert!(extra.block && extra.sprint);
    assert!(!extra.roll && !extra.target_lock);
    input.mouse(MouseButton::Middle, false);
    input.key(KeyF, true);
    input.mouse(MouseButton::Right, true);
    input.wheel(1.);
    let ((cmd, _, _, _), extra) = tick(&mut input);
    assert!(extra.block);
    assert_eq!(cmd.actions, (1 << A::FireSecondary as u8) | (1 << A::NextSecondary as u8));
    assert_eq!(cmd.held, 1 << H::Secondary as u8);
    assert_eq!(cmd.secondary_count, 1);

    input.clear();
    input.set_context(Vehicle);
    input.key(KeyW, true);
    input.wheel(1.);
    let ((cmd, _, _, _), _) = tick(&mut input);
    assert_eq!(cmd.held, 1 << H::Forward as u8);
    assert_eq!(cmd.actions, 1 << A::NextPrimary as u8);
    input.set_context(Turret);
    input.key(KeyD, true);
    let ((cmd, _, _, _), extra) = tick(&mut input);
    assert_eq!(cmd.held, 1 << H::TurnRight as u8);
    assert_eq!(cmd.actions, 0);
    assert!(extra.zoom_in);

    input.set_context(Starfighter); // W and D remain held.
    for key in [Numpad0, KeyC, AltLeft, KeyQ, KeyM, End, ShiftLeft] { input.key(key, true); }
    input.mouse(MouseButton::Middle, true);
    let ((cmd, _, flight, view), extra) = tick(&mut input);
    assert_eq!(flight.actions, (1 << F::Takeoff as u8) | (1 << F::Immelmann as u8)
        | (1 << F::Trick as u8) | (1 << F::TargetLock as u8));
    assert_eq!(flight.held, (1 << FH::Throttle as u8) | (1 << FH::RollRight as u8)
        | (1 << FH::Boost as u8) | (1 << FH::RollLock as u8));
    assert_eq!(flight.held, flight.default_held);
    assert_eq!(cmd.actions, 0);
    assert!(view.map && view.perspective);
    assert_eq!(extra, Default::default());

    input.clear();
    Jedi.rebind(&mut settings, "holds.block", Some("WheelDown"));
    Starfighter.rebind(&mut settings, "targetLock", Some("KeyK"));
    input.set_profile(Swbf2, &settings);
    input.key(KeyQ, true);
    assert_eq!(tick(&mut input).0.2.actions, 0);
    input.key(KeyK, true);
    assert_eq!(tick(&mut input).0.2.actions, 1 << F::TargetLock as u8);
    input.set_context(Jedi);
    input.wheel(-1.);
    assert!(tick(&mut input).1.block);
    assert!(!tick(&mut input).1.block);
    input.set_context(Infantry);
    input.mouse(MouseButton::Middle, true);
    assert_eq!(tick(&mut input).0.0.actions, 1 << A::Zoom as u8);
    input.set_profile(Swbf1, &settings);
    let ((cmd, _, flight, _), extra) = tick(&mut input);
    assert_eq!((cmd.held, cmd.actions, flight.held), (0, 0, 0));
    assert_eq!(extra, Default::default());
}

#[test]
fn local_overlay_shortcuts_follow_the_same_context_as_commands() {
    use bfe_client::input::InputTrigger;
    use bfe_interface::hotkeys::ControlContext::{Infantry, Jedi};
    let mut settings = Settings::new();
    Jedi.rebind(&mut settings, "ui.roster", Some("Mouse1"));
    Infantry.rebind(&mut settings, "presses.roll", Some("KeyT"));
    let mut device = Input::default();
    device.set_profile(bfe_family::GameFamily::Swbf2, &settings);
    let key = |code| InputTrigger::from_dom_code(code).unwrap();
    assert_eq!(device.overlay_event(key("Backslash"), true, true), Some("Tab"));
    assert_eq!(device.overlay_event(key("KeyT"), true, true), None);
    device.set_context(Jedi);
    device.clear(); // Opening the table clears gameplay input.
    assert_eq!(device.overlay_event(key("Backslash"), false, false), Some("Tab"));
    assert_eq!(device.overlay_event(key("Backslash"), true, true), None);
    assert_eq!(device.overlay_event(key("Mouse1"), true, true), Some("Tab"));
    assert_eq!(device.overlay_event(key("Mouse1"), false, true), Some("Tab"));
    assert_eq!(device.overlay_event(key("KeyT"), true, true), Some("KeyT"));
    assert_eq!(Jedi.bindings(&settings)["holds.block"], ["KeyF"]);

    // A physical release reaches gameplay even when its new context has an overlay on that key.
    device.set_context(Infantry);
    device.mouse(MouseButton::Middle, true);
    device.set_context(Jedi);
    assert_eq!(device.overlay_event(key("Mouse1"), false, true), None);
    device.set_context(Infantry);
    device.mouse(MouseButton::Middle, true);
    let ((cmd, _, _, _), _) = device.sample_battle(1, 1, true, 1., 1., false);
    assert_eq!(cmd.actions, 1 << bfe_client::generated_input::ClientAction::Zoom as u8);
}

#[test]
fn jump_sets_the_shared_hold_until_release_in_each_ground_context() {
    use bfe_client::generated_input::{ClientAction, ClientHeld};
    use bfe_interface::hotkeys::ControlContext::{Infantry, Jedi, Vehicle};
    let jump = 1 << ClientAction::Jump as u8;
    let space = 1 << ClientHeld::Space as u8;
    for context in [Infantry, Vehicle, Jedi] {
        let mut device = Input::default();
        device.set_profile(bfe_family::GameFamily::Swbf2, &Settings::new());
        device.set_context(context);
        let sample = |input: &mut Input| input.sample_battle(1, 1, true, 1., 1., false).0.0;
        for key in [KeyCode::Space, KeyCode::Numpad0] {
            device.key(key, true);
            let pressed = sample(&mut device);
            assert_eq!(pressed.held & space, space, "{context:?} {key:?}");
            assert_eq!(pressed.actions & jump, jump);
            let held = sample(&mut device);
            assert_eq!((held.held & space, held.actions & jump), (space, 0));
            device.key(key, false);
            assert_eq!(sample(&mut device).held & space, 0);
        }
    }
}
