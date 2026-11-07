use bfe_client::{host::{Host, World, STEP}, input::Input};
use bfe_native::Frame;
use bfe_client::generated_input::{ClientAction as Action, ClientCommand, ClientHeld as Held, ClientLook, ClientFlightCommand};
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
    assert_eq!(command.held, (1 << Held::Forward as u8) | (1 << Held::Right as u8)
        | (1 << Held::Space as u8) | (1 << Held::Primary as u8) | (1 << Held::Secondary as u8));
    assert_eq!(command.actions, (1 << Action::Jump as u8) | (1 << Action::FirePrimary as u8)
        | (1 << Action::FireSecondary as u8) | (1 << Action::Zoom as u8) | (1 << Action::PreviousPrimary as u8));
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
    frame: Frame,
    ticks: usize,
    actions: usize,
    last_sequence: f64,
    alpha: f64,
}
impl World for Probe {
    type Frame = Frame;
    fn step(&mut self, command: &ClientCommand, _look: &ClientLook, _flight: &ClientFlightCommand, _view: &bfe_client::input::ViewActions, dt: f64) -> Result<(), String> {
        assert_eq!(dt, STEP);
        assert!(command.sequence > self.last_sequence);
        self.last_sequence = command.sequence;
        self.actions += usize::from(command.actions != 0);
        self.ticks += 1;
        Ok(())
    }
    fn present(&mut self, alpha: f64) -> &Frame { self.alpha = alpha; &self.frame }
}
#[test]
fn host_steps_before_presentation_and_drops_pause_backlog() {
    let probe = Probe { frame: Frame { records: 0, input: vec![], bones: vec![] }, ticks: 0,
        actions: 0, last_sequence: 0.0, alpha: 0.0 };
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
    use bfe_client::{generated_input::{ClientFlightHeld as F, ClientFlightAction as A}, input::SquadOrder};
    let mut input = Input::default();
    for key in [KeyCode::KeyW, KeyCode::KeyA, KeyCode::ShiftRight, KeyCode::Space, KeyCode::CapsLock] {
        input.key(key, true);
    }
    input.mouse(MouseButton::Right, true);
    input.key(KeyCode::KeyQ, true);
    input.key(KeyCode::F3, true);
    let flight = input.take_flight();
    assert_eq!(flight.held, (1 << F::Throttle as u8) | (1 << F::RollLeft as u8)
        | (1 << F::Boost as u8) | (1 << F::RollLock as u8));
    assert_eq!(flight.actions, (1 << A::Takeoff as u8) | (1 << A::Immelmann as u8) | (1 << A::TargetLock as u8));
    let view = input.take_view();
    assert!(view.perspective);
    assert_eq!(view.squad, Some(SquadOrder::MoveOut));
    assert_eq!(input.take_flight().actions, 0);
    assert!(input.take_view().squad.is_none());
    input.clear();
    assert_eq!(input.take_flight().held, 0);
}
