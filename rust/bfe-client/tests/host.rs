use std::collections::BTreeMap;

use bfe_client::generated_input::{ClientAction as Action, ClientCommand, ClientFlightCommand, ClientHeld as Held, ClientLook};
use bfe_client::{
    host::{Host, World, STEP},
    input::{dom_code_to_keycode, keycode_to_dom_code, Input, InputTrigger},
};
use winit::{event::MouseButton, keyboard::KeyCode};

#[test]
fn generated_command_maps_defaults_and_consumes_edges_once() {
    let mut input = Input::default();
    input.key(KeyCode::KeyW, true);
    input.key(KeyCode::ArrowUp, true);
    input.key(KeyCode::KeyW, false); // The alias still holds forward.
    input.key(KeyCode::KeyD, true);
    input.key(KeyCode::Space, true);
    input.key(KeyCode::Space, true); // OS repeat is not a second press.
    input.mouse(MouseButton::Left, true);
    input.mouse(MouseButton::Middle, true);
    input.mouse(MouseButton::Right, true);
    input.wheel(-1.0);
    input.motion(4.0, -3.0);
    input.motion(2.0, 1.0);
    let (command, look) = input.take(7, 3);
    assert_eq!(command.tick, 7.0);
    assert_eq!(command.generation, 3.0);
    assert_eq!(
        command.held,
        (1 << Held::Forward as u8)
            | (1 << Held::Right as u8)
            | (1 << Held::Space as u8)
            | (1 << Held::Primary as u8)
            | (1 << Held::Secondary as u8)
    );
    assert_eq!(
        command.actions,
        (1 << Action::Jump as u8)
            | (1 << Action::FirePrimary as u8)
            | (1 << Action::FireSecondary as u8)
            | (1 << Action::Zoom as u8)
            | (1 << Action::PreviousPrimary as u8)
    );
    assert_eq!((look.x, look.y), (6.0, -2.0));
    assert_eq!((command.primary_count, command.secondary_count), (1, 1));
    // Round-trip the real generated layout, including the block-relative look.
    let mut words = [0.0; ClientCommand::WORDS + ClientLook::WORDS];
    let mut heap = Vec::new();
    command.put(&mut words, &mut heap);
    look.put(&mut words[ClientCommand::WORDS..], &mut heap);
    assert_eq!(ClientCommand::take(&words, &[]), command);
    assert_eq!(ClientLook::take(&words[command.look_at as usize..], &[]), look);
    let (next, look) = input.take(8, 3);
    assert_eq!(next.actions, 0);
    assert_eq!(next.held, command.held);
    assert_eq!((next.primary_count, next.secondary_count), (0, 0));
    assert_eq!((look.x, look.y), (0.0, 0.0));
    input.clear();
    assert_eq!(input.take(9, 3).0.held, 0);
}

#[test]
fn stalled_input_is_bounded_and_releases_fire_aliases() {
    let mut input = Input::default();
    for _ in 0..100_000 {
        input.mouse(MouseButton::Left, true);
        input.mouse(MouseButton::Left, false);
        input.key(KeyCode::Space, true);
        input.key(KeyCode::Space, false);
        input.motion(f64::MAX, f64::NAN);
    }
    let (command, look) = input.take(1, 1);
    assert_eq!(command.primary_count, 32);
    assert_eq!(command.held, 0);
    assert_eq!(look.x, 1e6);
    assert_eq!(look.y, 0.0);
    input.key(KeyCode::AltLeft, true);
    input.key(KeyCode::AltRight, true);
    input.key(KeyCode::AltLeft, false);
    let (command, _) = input.take(2, 1);
    assert_eq!(command.held, 1 << Held::Secondary as u8);
    assert_eq!(command.secondary_count, 1);
}

struct Probe {
    frame: u32,
    ticks: usize,
    actions: usize,
    last_sequence: f64,
    alpha: f64,
}
impl World for Probe {
    type Frame = u32;
    fn step(
        &mut self,
        command: &ClientCommand,
        _look: &ClientLook,
        _flight: &ClientFlightCommand,
        _view: &bfe_client::input::ViewActions,
        dt: f64,
    ) -> Result<(), String> {
        assert_eq!(dt, STEP);
        assert!(command.sequence > self.last_sequence);
        self.last_sequence = command.sequence;
        self.actions += usize::from(command.actions != 0);
        self.ticks += 1;
        Ok(())
    }
    fn present(&mut self, alpha: f64) -> &u32 {
        self.alpha = alpha;
        &self.frame
    }
}
#[test]
fn host_steps_before_presentation_and_drops_pause_backlog() {
    let probe = Probe { frame: 42, ticks: 0, actions: 0, last_sequence: 0.0, alpha: 0.0 };
    let mut host = Host::new(probe);
    host.input.key(KeyCode::Space, true);
    host.frame(STEP * 2.5).unwrap();
    assert_eq!(host.world.ticks, 2);
    assert_eq!(host.world.actions, 1);
    assert!((host.world.alpha - 0.5).abs() < 1e-12);
    host.pause();
    host.frame(300.0).unwrap();
    assert!(host.world.ticks <= 10);
    host.frame(f64::NAN).unwrap();
    assert!(host.world.alpha.is_finite());
}

#[test]
fn flyer_and_view_intentions_use_their_own_input_records() {
    use bfe_client::{
        generated_input::{ClientFlightAction as A, ClientFlightHeld as F},
        input::SquadOrder,
    };
    let mut input = Input::default();
    input.flight_mode = true;
    for key in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::ShiftRight, KeyCode::Space, KeyCode::CapsLock] {
        input.key(key, true);
    }
    input.mouse(MouseButton::Middle, true);
    input.key(KeyCode::KeyQ, true);
    input.key(KeyCode::F3, true);
    let flight = input.take_flight();
    assert_eq!(flight.held, (1 << F::Throttle as u8) | (1 << F::RollLeft as u8) | (1 << F::Boost as u8) | (1 << F::RollLock as u8));
    assert_eq!(flight.actions, (1 << A::Takeoff as u8) | (1 << A::Immelmann as u8) | (1 << A::TargetLock as u8));
    let view = input.take_view();
    assert!(!view.perspective);
    assert_eq!(view.squad, None);
    input.clear();
    input.flight_mode = false;
    input.key(KeyCode::KeyQ, true);
    input.key(KeyCode::F3, true);
    let view = input.take_view();
    assert!(view.perspective);
    assert_eq!(view.squad, Some(SquadOrder::MoveOut));
    assert_eq!(input.take_flight().actions, 0);
    assert!(input.take_view().squad.is_none());
    input.clear();
    assert_eq!(input.take_flight().held, 0);
}

#[test]
fn physical_mouse_buttons_match_the_shared_browser_profile() {
    let mut input = Input::default();
    input.mouse(MouseButton::Right, true);
    let (command, _) = input.take(1, 1);
    assert_eq!(command.secondary_count, 1);
    assert_eq!(command.held, 1 << Held::Secondary as u8);
    assert_eq!(command.actions, 1 << Action::FireSecondary as u8);
    assert_eq!(input.take_flight().held, 0);
    input.mouse(MouseButton::Right, false);
    input.mouse(MouseButton::Middle, true);
    let (command, _) = input.take(2, 1);
    assert_eq!(command.held, 0);
    assert_eq!(command.secondary_count, 0);
    assert_eq!(command.actions, 1 << Action::Zoom as u8);
    assert_eq!(input.take_flight().held, 1 << bfe_client::generated_input::ClientFlightHeld::RollLock as u8);
}

#[test]

fn covered_battle_keeps_valid_device_settings_and_clears_input() {
    let mut device = Input::default();
    device.key(KeyCode::KeyW, true);
    device.motion(160., 40.);
    let (command, look, flight, view) = device.sample(1, 2, false, 16./9., 0.75, true);
    assert_eq!(command.held, 0);
    assert_eq!(look.x, 0.);
    assert_eq!(command.aspect, 16./9.);
    assert!(command.has_aspect);
    assert_eq!(command.mouse_scale, 0.75);
    assert_eq!(command.invert, Some(true));
    assert!(flight.scale > 0. && flight.curve >= 0.1);
    assert_eq!(view, Default::default());
}

#[test]

fn all_104_input_codes_map_to_triggers_and_round_trip_keycodes() {
    for code in bfe_interface::hotkeys::INPUT_CODES {
        let trigger = InputTrigger::from_dom_code(code)
            .unwrap_or_else(|| panic!("INPUT_CODES entry {code} missing from InputTrigger::from_dom_code"));
        if let InputTrigger::Key(kc) = trigger {
            assert_eq!(dom_code_to_keycode(code), Some(kc));
            assert_eq!(keycode_to_dom_code(kc), Some(code));
        }
    }
}

#[test]
fn audited_default_bindings_and_custom_rebindings_dispatch_correctly() {
    use bfe_client::generated_input::ClientFlightHeld as FH;

    let mut input = Input::default();

    // Digit1/2 are flight bindings, not additional infantry shortcuts.
    input.flight_mode = true;
    input.key(KeyCode::Digit1, true);
    input.key(KeyCode::Digit1, false);
    assert_eq!(input.take(1, 1).0.actions, 1 << Action::NextPrimary as u8);

    input.key(KeyCode::Digit2, true);
    input.key(KeyCode::Digit2, false);
    assert_eq!(input.take(2, 1).0.actions, 1 << Action::PreviousPrimary as u8);

    input.flight_mode = false;
    input.key(KeyCode::KeyG, true);
    input.key(KeyCode::KeyG, false);
    assert_eq!(input.take(3, 1).0.actions, 1 << Action::NextSecondary as u8);

    input.key(KeyCode::Numpad0, true);
    input.key(KeyCode::Numpad0, false);
    assert_eq!(input.take(4, 1).0.actions, 1 << Action::Use as u8);

    input.key(KeyCode::Enter, true);
    input.key(KeyCode::Enter, false);
    assert_eq!(input.take(5, 1).0.actions, 1 << Action::Reload as u8);

    // KeyM -> map, Backquote -> stats (unless Shift or Ctrl is held)
    input.key(KeyCode::KeyM, true);
    input.key(KeyCode::KeyM, false);
    input.key_with_modifiers(KeyCode::Backquote, true, true);
    input.key_with_modifiers(KeyCode::Backquote, false, true);
    let view = input.take_view();
    assert!(view.map);
    assert!(!view.stats);

    input.key_with_modifiers(KeyCode::Backquote, true, false);
    input.key_with_modifiers(KeyCode::Backquote, false, false);
    assert!(input.take_view().stats);

    // Mouse3 (Back) -> perspective + flight TargetLock
    input.mouse(MouseButton::Back, true);
    input.mouse(MouseButton::Back, false);
    assert!(input.take_view().perspective);
    assert_eq!(input.take_flight().actions, 0);

    // AltLeft and ControlRight -> flight Trick + ground FireSecondary
    input.key(KeyCode::AltLeft, true);
    input.key(KeyCode::AltLeft, false);
    assert_eq!(input.take_flight().actions, 0);
    let _ = input.take(6, 1);

    // Custom keyBindings and flightBindings override defaults
    let mut custom_ground = BTreeMap::new();
    custom_ground.insert("presses.jump".to_string(), vec!["KeyV".to_string()]);
    custom_ground.insert("axes.forward+".to_string(), vec!["KeyI".to_string()]);
    custom_ground.insert("holds.firePrimary".to_string(), vec!["KeyK".to_string()]);

    let mut custom_flight = BTreeMap::new();
    custom_flight.insert("throttle".to_string(), vec!["KeyI".to_string()]);
    custom_flight.insert("takeoff".to_string(), vec!["KeyV".to_string()]);

    input.set_bindings(&custom_ground, &custom_flight);

    // Old default Space no longer triggers ground Jump or flight Takeoff
    input.key(KeyCode::Space, true);
    input.key(KeyCode::Space, false);
    assert_eq!(input.take(7, 1).0.actions, 0);
    assert_eq!(input.take_flight().actions, 0);

    // New bindings KeyV, KeyI, KeyK work
    input.key(KeyCode::KeyV, true);
    input.key(KeyCode::KeyI, true);
    input.key(KeyCode::KeyK, true);
    let (cmd, _) = input.take(8, 1);
    assert_eq!(cmd.actions, (1 << Action::Jump as u8) | (1 << Action::FirePrimary as u8));
    assert_eq!(cmd.held, (1 << Held::Forward as u8) | (1 << Held::Primary as u8));
    assert_eq!(cmd.primary_count, 1);

    let flight = input.take_flight();
    assert_eq!(flight.actions, 0);
    assert_eq!(flight.held, 1 << FH::Throttle as u8);

}

#[test]
fn saved_binding_aliases_are_not_truncated_and_flight_does_not_add_ground_keys() {
    let mut input = Input::default();
    let mut ground = std::collections::BTreeMap::new();
    ground.insert("presses.jump".into(), ["KeyI", "KeyJ", "KeyK", "KeyL", "KeyU"].map(String::from).to_vec());
    input.set_bindings(&ground, &Default::default());
    input.key(KeyCode::KeyU, true);
    assert_ne!(input.take(1, 1).0.actions & (1 << Action::Jump as u8), 0);
    input.clear();
    input.key(KeyCode::KeyV, true);
    assert!(!input.take_view().perspective);
    input.clear();
    input.flight_mode = true;
    input.key(KeyCode::KeyQ, true);
    input.key(KeyCode::F3, true);
    assert!(!input.take_view().perspective);
    assert!(input.take_view().squad.is_none());
    assert_ne!(input.take_flight().actions, 0);
}

#[cfg(target_os = "linux")]
#[test]
fn xdotool_under_xvfb_and_nested_weston_delivers_audited_hotkeys() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    use winit::{
        application::ApplicationHandler,
        event::{ElementState, MouseScrollDelta, WindowEvent},
        event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
        keyboard::PhysicalKey,
        platform::x11::EventLoopBuilderExtX11,
        window::{Window, WindowAttributes, WindowId},
    };

    struct AuditHarness {
        window: Option<Window>,
        dom_keys: Vec<String>,
        mouse_buttons: Vec<u8>,
        wheels: Vec<i32>,
        phase: u8,
        display: String,
        deadline: Instant,
    }

    impl AuditHarness {
        fn spawn_xdotool(&self) {
            let disp = self.display.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                let script = r#"
                    WID=$(xdotool search --sync --name "BfeKeybindAudit" | head -n 1)
                    xdotool windowfocus "$WID" || true
                    xdotool mousemove --window "$WID" 200 200
                    for k in g Alt_L Caps_Lock KP_Insert grave KP_Multiply KP_Divide Delete BackSpace F5 F9 m t y Tab Escape; do
                        xdotool key --window "$WID" "$k"
                    done
                    for b in 1 2 3 4 5 8 9; do
                        xdotool click "$b"
                    done
                "#;
                let _ = Command::new("bash")
                    .arg("-c")
                    .arg(script)
                    .env("DISPLAY", &disp)
                    .status();
            });
        }

        fn verify_and_reset(&mut self, label: &str) {
            for expected in [
                "KeyG",
                "AltLeft",
                "CapsLock",
                "Numpad0",
                "Backquote",
                "NumpadMultiply",
                "NumpadDivide",
                "Delete",
                "Backspace",
                "F5",
                "F9",
                "KeyM",
                "KeyT",
                "KeyY",
                "Tab",
                "Escape",
            ] {
                assert!(
                    self.dom_keys.iter().any(|k| k == expected),
                    "{label}: missing expected DOM key {expected} in {:?}",
                    self.dom_keys
                );
            }
            for b in 0..=4u8 {
                assert!(
                    self.mouse_buttons.contains(&b),
                    "{label}: missing mouse button {b} in {:?}",
                    self.mouse_buttons
                );
            }
            assert!(
                self.wheels.contains(&-1) && self.wheels.contains(&1),
                "{label}: missing wheel up/down in {:?}",
                self.wheels
            );
            self.dom_keys.clear();
            self.mouse_buttons.clear();
            self.wheels.clear();
        }
    }

    impl ApplicationHandler for AuditHarness {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_none() {
                let attrs = WindowAttributes::default()
                    .with_title("BfeKeybindAudit")
                    .with_inner_size(winit::dpi::PhysicalSize::new(640, 480))
                    .with_visible(true);
                let win = event_loop.create_window(attrs).expect("create window");
                self.window = Some(win);
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::KeyboardInput { event, .. } => {
                    if event.state == ElementState::Pressed && !event.repeat {
                        if let PhysicalKey::Code(kc) = event.physical_key {
                            if let Some(code) = keycode_to_dom_code(kc) {
                                self.dom_keys.push(code.to_string());
                            }
                        }
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    if state == ElementState::Pressed {
                        let b = match button {
                            MouseButton::Left => Some(0),
                            MouseButton::Middle => Some(1),
                            MouseButton::Right => Some(2),
                            MouseButton::Back => Some(3),
                            MouseButton::Forward => Some(4),
                            _ => None,
                        };
                        if let Some(b) = b {
                            self.mouse_buttons.push(b);
                        }
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_, y) => -y as f64 * 100.0,
                        MouseScrollDelta::PixelDelta(p) => p.y,
                    };
                    if dy < 0.0 {
                        self.wheels.push(-1);
                    } else if dy > 0.0 {
                        self.wheels.push(1);
                    }
                }
                _ => {}
            }

            if self.dom_keys.len() >= 16 && self.mouse_buttons.len() >= 5 && self.wheels.len() >= 2 {
                if self.phase == 1 {
                    self.verify_and_reset("Xvfb");
                    self.phase = 2;
                    let xdg_dir = std::env::temp_dir().join(format!("bfe-weston-{}", std::process::id()));
                    let _ = std::fs::create_dir_all(&xdg_dir);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&xdg_dir, std::fs::Permissions::from_mode(0o700));
                    }
                    let mut weston = Command::new("weston")
                        .args([
                            "--backend=x11-backend.so",
                            "--socket=wayland-bfe-audit",
                            "--width=1024",
                            "--height=768",
                        ])
                        .env("DISPLAY", &self.display)
                        .env("XDG_RUNTIME_DIR", &xdg_dir)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .expect("spawn nested weston");
                    std::thread::sleep(Duration::from_millis(300));
                    self.spawn_xdotool();
                    let _ = weston.kill();
                    let _ = weston.wait();
                    let _ = std::fs::remove_dir_all(&xdg_dir);
                } else if self.phase == 2 {
                    self.verify_and_reset("Weston");
                    self.phase = 3;
                    event_loop.exit();
                }
            }
        }

        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(20),
            ));
            if Instant::now() >= self.deadline {
                event_loop.exit();
                return;
            }
            if self.phase == 0 && self.window.is_some() {
                self.phase = 1;
                self.spawn_xdotool();
            }
        }
    }

    if std::env::var("BFE_XDOTOOL_DISPLAY_AUDIT").as_deref() != Ok("1") {
        return;
    }

    let xvfb_display = format!(":{}", 90 + (std::process::id() % 10));
    let mut xvfb = Command::new("Xvfb")
        .args([xvfb_display.as_str(), "-screen", "0", "1024x768x24", "-ac"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn Xvfb");
    std::thread::sleep(Duration::from_millis(300));

    std::env::set_var("DISPLAY", &xvfb_display);
    std::env::remove_var("WAYLAND_DISPLAY");

    let x11_loop = EventLoop::builder()
        .with_x11()
        .with_any_thread(true)
        .build()
        .expect("build X11 event loop");
    let mut harness = AuditHarness {
        window: None,
        dom_keys: Vec::new(),
        mouse_buttons: Vec::new(),
        wheels: Vec::new(),
        phase: 0,
        display: xvfb_display,
        deadline: Instant::now() + Duration::from_secs(10),
    };
    x11_loop.run_app(&mut harness).expect("run display audit app");

    let _ = xvfb.kill();
    let _ = xvfb.wait();

    assert_eq!(
        harness.phase, 3,
        "display audit did not complete (phase={}, keys={:?}, mouse={:?}, wheels={:?})",
        harness.phase, harness.dom_keys, harness.mouse_buttons, harness.wheels
    );
}
