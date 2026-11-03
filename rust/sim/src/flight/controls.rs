//! src/flight/controls.js flightInput and src/boarding.js seatInput's
//! flight part: a pilot's flight keys from his held keys, his queued
//! actions and his game pad, as the world reads a network pilot's
//! (src/runtime/network-vehicle.js networkVehicleInput, every tick in the
//! players' phase). The keys a binding names are his command's flight
//! bindings over FLIGHT_BINDINGS (`flightBindings`).

use super::model::FlightInput;
use v8math::abs;

/// What the flight keys read of a pilot's controls: whether a key code is
/// held (`controls.down.has`), and an action taken from his queue
/// (`controls.take`, true once a queued press of it is consumed).
pub trait Controls {
    fn down(&self, code: &str) -> bool;
    fn take(&mut self, action: &str) -> bool;
}

/// A binding's key codes (`bindings[name] || []`).
pub trait Bindings {
    fn codes(&self, name: &str) -> &[String];
}

/// A game pad as flightInput reads it: its axes (undefined past their
/// length: None) and whether each button is pressed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pad {
    pub axes: Vec<Option<f64>>,
    pub buttons: Vec<bool>,
}

/// `dead(x)`: `Math.abs(x || 0) > .12 ? x : 0` (an undefined axis is 0).
fn dead(x: Option<f64>) -> f64 {
    let v = match x {
        Some(v) if !(v == 0.0 || v.is_nan()) => v,
        _ => 0.0,
    };
    if abs(v) > 0.12 {
        // `? x`: the axis itself (here never undefined: undefined is 0).
        x.unwrap_or(0.0)
    } else {
        0.0
    }
}

/// flightInput(controls, bindings, gamepad), then seatInput's
/// `responseCurve` (the command's flight curve, NaN for none) and, for a
/// network pilot (`assist` false), `steeringAssist = false`.
pub fn flight_input<C: Controls, B: Bindings>(controls: &mut C, bindings: &B, pad: Option<&Pad>, response_curve: f64, assist: bool) -> FlightInput {
    let held = |c: &C, name: &str| bindings.codes(name).iter().any(|k| c.down(k));
    let axis = |i: usize| pad.and_then(|p| p.axes.get(i).copied().flatten());
    let button = |i: usize| pad.is_some_and(|p| p.buttons.get(i).copied().unwrap_or(false));
    let b = |x: bool| if x { 1.0 } else { 0.0 };
    // `held(a) - held(b) || -dead(axis)`: a key pair's -1, 0 or 1, else the
    // axis (negated for the throttle: the pad's stick is down-positive).
    let keys = b(held(controls, "throttle")) - b(held(controls, "brake"));
    let forward = if keys != 0.0 { keys } else { -dead(axis(3)) };
    let keys = b(held(controls, "rollRight")) - b(held(controls, "rollLeft"));
    let right = if keys != 0.0 { keys } else { dead(axis(2)) };
    let (sx, sy) = (dead(axis(0)), dead(axis(1)));
    let stick = sx != 0.0 || sy != 0.0;
    let boost = held(controls, "boost") || button(11);
    let roll_lock = held(controls, "rollLock");
    // The edges in their order: each consumes its own action.
    let takeoff = controls.take("takeoff");
    let trick = controls.take("trick") || button(0);
    let immelmann = controls.take("immelmann");
    let target_lock = controls.take("targetLock");
    FlightInput {
        forward,
        right,
        stick_x: if stick { Some(sx) } else { None },
        stick_y: if stick { Some(-sy) } else { None },
        boost: b(boost),
        roll_lock: b(roll_lock),
        takeoff: b(takeoff),
        trick: b(trick),
        immelmann: b(immelmann),
        target_lock: b(target_lock),
        response_curve,
        no_assist: !assist,
        ..FlightInput::default()
    }
}
