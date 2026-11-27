//! Compare every supported device code, on both sides of a seat change, with the released sampler.
use bfe_client::{
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    input::{BattleInput as Input, InputTrigger},
};
use bfe_family::GameFamily;
use bfe_interface::{
    hotkeys::{default_flight_bindings, flat_bindings, INPUT_CODES},
    SettingValue, Settings,
};
use sha2::{Digest, Sha256};
use winit::event::MouseButton;

fn device_event(input: &mut Input, code: &str, down: bool, chord: bool) {
    match InputTrigger::from_dom_code(code).unwrap() {
        InputTrigger::Key(key) => input.key_with_modifiers(key, down, chord),
        InputTrigger::Mouse(index) => {
            let buttons =
                [MouseButton::Left, MouseButton::Middle, MouseButton::Right, MouseButton::Back, MouseButton::Forward];
            input.mouse(buttons[index as usize], down);
        }
        InputTrigger::WheelUp if down => input.wheel(3.5),
        InputTrigger::WheelDown if down => input.wheel(-0.125),
        _ => {}
    }
}

fn sample(input: &mut Input, tick: &mut u32, digest: &mut Sha256) {
    *tick += 1;
    let t = *tick;
    let ((command, look, flight, view), extra) =
        input.sample_battle(t, 1 + t / 157, true, 1.125, 0.55 + (t % 23) as f64 / 16., t % 313 < 177);
    assert_eq!(extra, Default::default());
    let mut a = [0.; ClientCommand::WORDS];
    let mut b = [0.; ClientLook::WORDS];
    let mut c = [0.; ClientFlightCommand::WORDS];
    let mut heap = Vec::new();
    command.put(&mut a, &mut heap);
    look.put(&mut b, &mut heap);
    flight.put(&mut c, &mut heap);
    for word in a.iter().chain(&b).chain(&c).chain(&heap) {
        digest.update(word.to_le_bytes());
    }
    digest.update([view.perspective as u8, view.map as u8, view.stats as u8]);
}

#[test]
fn released_physical_codes_survive_a_mid_hold_seat_change() {
    let mut input = Input::default();
    let mut settings = Settings::new();
    let mut digest = Sha256::new();
    let mut tick = 0;
    input.set_profile(GameFamily::Swbf1, &settings);
    for pass in 0..4 {
        let flight = pass % 2 == 1;
        let actions = if flight { default_flight_bindings() } else { flat_bindings() };
        for (n, code) in INPUT_CODES.iter().enumerate() {
            if pass >= 2 {
                let overrides = actions
                    .keys()
                    .enumerate()
                    .map(|(j, action)| {
                        let key = INPUT_CODES[(n + j) % INPUT_CODES.len()];
                        let alias = INPUT_CODES[(n + j + 3) % INPUT_CODES.len()];
                        (action.clone(), vec![key.into(), key.into(), alias.into(), "Unsupported".into()])
                    })
                    .collect();
                settings.set(if flight { "flightBindings" } else { "keyBindings" }, SettingValue::Bindings(overrides));
                input.set_profile(GameFamily::Swbf1, &settings);
            }
            input.device.flight_mode = flight;
            device_event(&mut input, code, true, true);
            sample(&mut input, &mut tick, &mut digest);
            device_event(&mut input, code, true, false);
            sample(&mut input, &mut tick, &mut digest);
            input.device.flight_mode = !flight;
            sample(&mut input, &mut tick, &mut digest);
            device_event(&mut input, code, false, false);
            sample(&mut input, &mut tick, &mut digest);
        }
    }
    assert_eq!(tick, 1664);
    assert_eq!(format!("{:x}", digest.finalize()), include_str!("fixtures/controls/physical-sweep.sha256").trim());
}
