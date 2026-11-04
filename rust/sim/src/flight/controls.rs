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

/// Values returned by an action callback retain their source type. Ordinary
/// command controls return booleans; a live callback may return another value.
pub trait ActionValue: Clone {
    fn boolean(value: bool) -> Self;
    fn truthy(&self) -> bool;
}
impl ActionValue for bool {
    fn boolean(value: bool) -> Self { value }
    fn truthy(&self) -> bool { *self }
}

/// Read inputs at the point the source reads them. A binding keeps its array
/// and starting length, but each element and each pad value can change during
/// a controls callback. No borrowed array crosses one of these calls.
pub trait InputReader {
    type Error;
    type Binding: Copy;
    type Code;
    type Value: ActionValue;
    fn binding(&mut self, name: &str) -> Result<(Self::Binding, usize), Self::Error>;
    fn code(&mut self, binding: Self::Binding, index: usize) -> Result<Option<Self::Code>, Self::Error>;
    fn down(&mut self, code: Self::Code) -> Result<bool, Self::Error>;
    fn take(&mut self, action: &str) -> Result<Self::Value, Self::Error>;
    fn axis(&mut self, index: usize) -> Result<Option<f64>, Self::Error>;
    fn button(&mut self, index: usize) -> Result<bool, Self::Error>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keys<V> {
    pub forward: f64,
    pub right: f64,
    pub stick: Option<(f64, f64)>,
    pub boost: bool,
    pub roll_lock: bool,
    pub takeoff: V,
    pub trick: V,
    pub immelmann: V,
    pub target_lock: V,
}

fn held<R: InputReader>(r: &mut R, name: &str) -> Result<bool, R::Error> {
    let (binding, length) = r.binding(name)?;
    for index in 0..length {
        if let Some(code) = r.code(binding, index)? {
            if r.down(code)? { return Ok(true); }
        }
    }
    Ok(false)
}

/// The flightInput algorithm shared by native snapshots and live readers.
pub fn read_input<R: InputReader>(r: &mut R) -> Result<Keys<R::Value>, R::Error> {
    let b = |x: bool| if x { 1.0 } else { 0.0 };
    let keys = b(held(r, "throttle")?) - b(held(r, "brake")?);
    let forward = if keys != 0.0 { keys } else { -dead(r.axis(3)?) };
    let keys = b(held(r, "rollRight")?) - b(held(r, "rollLeft")?);
    let right = if keys != 0.0 { keys } else { dead(r.axis(2)?) };
    // The condition short-circuits, and the object fields read the axes again.
    let stick = if dead(r.axis(0)?) != 0.0 || dead(r.axis(1)?) != 0.0 {
        Some((dead(r.axis(0)?), -dead(r.axis(1)?)))
    } else { None };
    let boost = held(r, "boost")? || r.button(11)?;
    let roll_lock = held(r, "rollLock")?;
    let takeoff = r.take("takeoff")?;
    let trick = r.take("trick")?;
    let trick = if trick.truthy() { trick } else { R::Value::boolean(r.button(0)?) };
    let immelmann = r.take("immelmann")?;
    let target_lock = r.take("targetLock")?;
    Ok(Keys { forward, right, stick, boost, roll_lock, takeoff, trick, immelmann, target_lock })
}

struct Fixed<'a, C, B> { controls: &'a mut C, bindings: &'a B, pad: Option<&'a Pad>, names: Vec<String> }
impl<C: Controls, B: Bindings> InputReader for Fixed<'_, C, B> {
    type Error = std::convert::Infallible;
    type Binding = usize;
    type Code = String;
    type Value = bool;
    fn binding(&mut self, name: &str) -> Result<(usize, usize), Self::Error> {
        let index = self.names.len(); self.names.push(name.into());
        Ok((index, self.bindings.codes(name).len()))
    }
    fn code(&mut self, binding: usize, index: usize) -> Result<Option<String>, Self::Error> {
        Ok(self.bindings.codes(&self.names[binding]).get(index).cloned())
    }
    fn down(&mut self, code: String) -> Result<bool, Self::Error> { Ok(self.controls.down(&code)) }
    fn take(&mut self, action: &str) -> Result<bool, Self::Error> { Ok(self.controls.take(action)) }
    fn axis(&mut self, index: usize) -> Result<Option<f64>, Self::Error> { Ok(self.pad.and_then(|p| p.axes.get(index).copied().flatten())) }
    fn button(&mut self, index: usize) -> Result<bool, Self::Error> { Ok(self.pad.is_some_and(|p| p.buttons.get(index).copied().unwrap_or(false))) }
}

/// flightInput(controls, bindings, gamepad), then seatInput's
/// `responseCurve` (the command's flight curve, NaN for none) and, for a
/// network pilot (`assist` false), `steeringAssist = false`.
pub fn flight_input<C: Controls, B: Bindings>(controls: &mut C, bindings: &B, pad: Option<&Pad>, response_curve: f64, assist: bool) -> FlightInput {
    let keys = read_input(&mut Fixed { controls, bindings, pad, names: Vec::new() }).unwrap();
    let b = |x: bool| if x { 1.0 } else { 0.0 };
    FlightInput {
        forward: keys.forward,
        right: keys.right,
        stick_x: keys.stick.map(|s| s.0),
        stick_y: keys.stick.map(|s| s.1),
        boost: b(keys.boost),
        roll_lock: b(keys.roll_lock),
        takeoff: b(keys.takeoff),
        trick: b(keys.trick),
        immelmann: b(keys.immelmann),
        target_lock: b(keys.target_lock),
        response_curve,
        no_assist: !assist,
        ..FlightInput::default()
    }
}
