//! A network human's seat (src/runtime/network-seat.js cycleNetworkSeat,
//! src/runtime/network-vehicle.js networkVehicleInput, and the seat switch
//! they share with the player's keys, src/boarding.js switchSeat): his
//! mouse and keys in a hull's seat, and his move to another seat.

use super::boarding::{seat_control, seat_look_with, LookAccess, LookField, NativeLook, LookScales, SeatControl};
use super::lease::person_lease;
use super::hulls::{clamp_aim, rest_aim, HullSide, Hulls};
use super::SeatWorld;
use crate::gen::{SeatLease, Store, Vehicle};
use crate::store::Ref;

/// What a command carries for his seat (networkVehicleInput reads it): his
/// mouse counts this tick, their scale and invert, and his flight settings'
/// sensitivity and invert (undefined: None).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SeatCommand {
    pub look: Vec<(f64, f64)>,
    pub mouse_scale: Option<f64>,
    pub invert_mouse: bool,
    pub flight_sensitivity: Option<f64>,
    pub flight_invert: bool,
}

/// The keys the driver's seat reads (boarding.js seatInput's drive branch):
/// the forward, right and turn axes and whether Space is down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeatKeys {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub space: bool,
}

/// What a seat's keys are to the hull (boarding.js seatInput): a craft's
/// flight keys (the flight package's flightInput, from the command's
/// bindings, without steering assistance), the driving keys (Space brakes),
/// or none for a gunner's seat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SeatKeysOut {
    Flight,
    Drive { forward: f64, right: f64, turn: f64, brake: bool },
    Turret,
}

/// boarding.js seatInput, but for the flight keys (the flight package's).
pub fn seat_input(v: &Vehicle, index: usize, keys: &SeatKeys) -> Option<SeatKeysOut> {
    Some(match seat_control(v, index)? {
        SeatControl::Flight => SeatKeysOut::Flight,
        SeatControl::Drive => SeatKeysOut::Drive { forward: keys.forward, right: keys.right, turn: keys.turn, brake: keys.space },
        SeatControl::Turret => SeatKeysOut::Turret,
    })
}

/// networkVehicleInput (network-vehicle.js): his mouse on the seat (a
/// craft's pilot's virtual stick, else the seat's look, which is its aim at
/// once), then his keys as the hull takes them.
pub fn network_vehicle_input(v: &mut Vehicle, index: usize, keys: &SeatKeys, command: &SeatCommand) -> Option<SeatKeysOut> {
    let mut access = NativeInput { look: NativeLook { vehicle: v, index }, keys, command, cursor: 0 };
    match network_input_with(&mut access).unwrap() {
        Input::Flight(_) => Some(SeatKeysOut::Flight),
        Input::Drive { forward, right, turn, brake } => Some(SeatKeysOut::Drive {
            forward: forward.value, right: right.value, turn: turn.value, brake: brake.truthy(),
        }),
        Input::Turret => if seat_control(access.look.vehicle, index).is_some() { Some(SeatKeysOut::Turret) } else { None },
    }
}

/// A scalar crossing a callback. References are invocation-local tokens, not
/// Store handles; tags preserve undefined, null, booleans and numeric bits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value { pub kind: u8, pub value: f64, pub truth: bool }
impl Value {
    pub fn number(value: f64) -> Self { Self { kind: 4, value, truth: value != 0.0 && !value.is_nan() } }
    pub fn boolean(value: bool) -> Self { Self { kind: if value { 3 } else { 2 }, value: 0.0, truth: value } }
    pub fn truthy(self) -> bool { self.truth }
}
impl crate::flight::controls::ActionValue for Value {
    fn boolean(value: bool) -> Self { Self::boolean(value) }
    fn truthy(&self) -> bool { self.truth }
}
#[derive(Clone, Debug, PartialEq)]
pub struct FlightKeys { pub keys: crate::flight::controls::Keys<Value>, pub curve: Value }
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Flight(Option<FlightKeys>),
    Drive { forward: Value, right: Value, turn: Value, brake: Value },
    Turret,
}

pub fn look_scales(mouse: f64, invert: bool, flight: f64, flight_invert: bool) -> LookScales {
    LookScales { scale: v8math::min(2.0, mouse), invert,
        flight_scale: v8math::max(0.05, v8math::min(10.0, flight)), flight_invert }
}

pub trait InputAccess: LookAccess {
    fn scales(&mut self) -> Result<LookScales, Self::Error>;
    fn next_look(&mut self) -> Result<Option<(f64, f64)>, Self::Error>;
    fn aim_from_look(&mut self) -> Result<(), Self::Error>;
    fn flight_input(&mut self) -> Result<Option<FlightKeys>, Self::Error>;
    fn drive_axis(&mut self, name: &str) -> Result<Value, Self::Error>;
    fn space(&mut self) -> Result<Value, Self::Error>;
}

pub fn network_input_with<A: InputAccess>(a: &mut A) -> Result<Input, A::Error> {
    let scales = a.scales()?;
    while let Some((dx, dy)) = a.next_look()? { seat_look_with(a, dx, dy, &scales)?; }
    if a.control()? != Some(SeatControl::Flight) {
        a.aim_from_look()?;
        a.clamp()?;
    }
    // clampAim can change the seat before its keys are read.
    Ok(match a.control()? {
        Some(SeatControl::Flight) => Input::Flight(a.flight_input()?),
        Some(SeatControl::Drive) => Input::Drive { forward: a.drive_axis("forward")?, right: a.drive_axis("right")?,
            turn: a.drive_axis("turn")?, brake: a.space()? },
        _ => Input::Turret,
    })
}

struct NativeInput<'a> { look: NativeLook<'a>, keys: &'a SeatKeys, command: &'a SeatCommand, cursor: usize }
impl LookAccess for NativeInput<'_> {
    type Error = std::convert::Infallible;
    fn control(&mut self) -> Result<Option<SeatControl>, Self::Error> { self.look.control() }
    fn look_read(&mut self, field: LookField) -> Result<f64, Self::Error> { self.look.look_read(field) }
    fn look_write(&mut self, field: LookField, value: f64) -> Result<(), Self::Error> { self.look.look_write(field, value) }
    fn clamp(&mut self) -> Result<(), Self::Error> { self.look.clamp() }
}
impl InputAccess for NativeInput<'_> {
    fn scales(&mut self) -> Result<LookScales, Self::Error> {
        let c = self.command;
        Ok(look_scales(c.mouse_scale.unwrap_or(1.0), c.invert_mouse, c.flight_sensitivity.unwrap_or(1.0), c.flight_invert))
    }
    fn next_look(&mut self) -> Result<Option<(f64, f64)>, Self::Error> {
        let value = self.command.look.get(self.cursor).copied(); self.cursor += 1; Ok(value)
    }
    fn aim_from_look(&mut self) -> Result<(), Self::Error> {
        let s = &mut self.look.vehicle.seats[self.look.index];
        s.aim_yaw = s.look_yaw.unwrap_or(s.aim_yaw); s.pitch = s.look_pitch.unwrap_or(s.pitch); Ok(())
    }
    fn flight_input(&mut self) -> Result<Option<FlightKeys>, Self::Error> { Ok(None) }
    fn drive_axis(&mut self, name: &str) -> Result<Value, Self::Error> {
        Ok(Value::number(match name { "forward" => self.keys.forward, "right" => self.keys.right, _ => self.keys.turn }))
    }
    fn space(&mut self) -> Result<Value, Self::Error> { Ok(Value::boolean(self.keys.space)) }
}

/// boarding.js switchSeat: a person's seat switch, on to the next seat that
/// way that no other person holds; a bot there trades places with him. Into
/// a gunner's seat his view goes with him, inside its arc; back at the
/// controls he aims along the hull. Leaving the pilot's seat lets go of
/// the stick. Returns the new seat, or -1.
pub trait CycleAccess {
    type Error;
    type Lease: Copy + PartialEq;
    type Hull: Copy;
    fn rider(&mut self, unit: u32) -> Result<Option<(Self::Hull, Self::Lease, f64)>, Self::Error>;
    fn direction(&mut self, value: f64) -> Result<f64, Self::Error> { Ok(value) }
    fn seat_count(&mut self, hull: Self::Hull) -> Result<usize, Self::Error>;
    fn candidate(&mut self, hull: Self::Hull, lease: Option<Self::Lease>, to: f64) -> Result<(bool, Option<Self::Lease>), Self::Error>;
    fn copy_aim(&mut self, hull: Self::Hull, from: f64, to: f64) -> Result<(), Self::Error>;
    fn move_lease(&mut self, lease: Self::Lease, to: f64) -> Result<(), Self::Error>;
    fn aim(&mut self, hull: Self::Hull, to: f64) -> Result<(), Self::Error>;
    fn clear_sticks(&mut self, hull: Self::Hull) -> Result<(), Self::Error>;
}

pub fn switch_with<A: CycleAccess>(a: &mut A, hull: A::Hull, lease: Option<A::Lease>, from: f64, direction: f64) -> Result<f64, A::Error> {
    let n = a.seat_count(hull)?;
    if n < 2 { return Ok(-1.0); }
    for k in 1..n {
        let to = ((from + a.direction(direction)? * k as f64) % n as f64 + n as f64) % n as f64;
        let (blocked, occupant) = a.candidate(hull, lease, to)?;
        if blocked { continue; }
        if to > 0.0 { a.copy_aim(hull, from, to)?; }
        if let Some(lease) = lease { a.move_lease(lease, to)?; }
        // Keep the selected occupant, even if the first move recursively
        // changes this hull's seats or moves this same rider again.
        if let Some(other) = occupant { if Some(other) != lease { a.move_lease(other, from)?; } }
        a.aim(hull, to)?;
        if from == 0.0 { a.clear_sticks(hull)?; }
        return Ok(to);
    }
    Ok(-1.0)
}

pub fn cycle_with<A: CycleAccess>(a: &mut A, unit: u32, direction: f64) -> Result<bool, A::Error> {
    let Some((hull, lease, from)) = a.rider(unit)? else { return Ok(false) };
    Ok(switch_with(a, hull, Some(lease), from, direction)? >= 0.0)
}

struct NativeCycle<'a, W: ?Sized> { hulls: &'a mut Hulls, st: &'a mut Store, world: &'a mut W }
impl<W: SeatWorld + ?Sized> CycleAccess for NativeCycle<'_, W> {
    type Error = std::convert::Infallible;
    type Lease = Ref<SeatLease>;
    type Hull = usize;
    fn rider(&mut self, unit: u32) -> Result<Option<(usize, Self::Lease, f64)>, Self::Error> {
        let Some(u) = self.st.unit.get(super::unit_ref(self.st, unit)) else { return Ok(None) };
        if u.network_human != Some(true) { return Ok(None); }
        let Some(r) = u.seat else { return Ok(None) };
        let lease = self.st.seat_lease.get(r).unwrap();
        let Some(vref) = lease.vehicle else { return Ok(None) };
        if !self.st.vehicle.get(vref).is_some_and(|v| v.alive) || lease.released { return Ok(None); }
        Ok(self.hulls.hulls.iter().position(|h: &HullSide| h.vehicle == vref).map(|h| (h, r, lease.index)))
    }
    fn seat_count(&mut self, hull: usize) -> Result<usize, Self::Error> {
        let v = self.st.vehicle.get(self.hulls.hulls[hull].vehicle).unwrap();
        Ok(if v.alive { v.seats.len() } else { 0 })
    }
    fn candidate(&mut self, hull: usize, lease: Option<Self::Lease>, to: f64) -> Result<(bool, Option<Self::Lease>), Self::Error> {
        let v = self.st.vehicle.get(self.hulls.hulls[hull].vehicle).unwrap();
        let occupant = v.seats[to as usize].lease;
        if occupant.is_some_and(|o| Some(o) != lease && person_lease(self.st.seat_lease.get(o).unwrap(), &self.st.unit)) { return Ok((true, occupant)); }
        let human = lease.and_then(|r| self.st.seat_lease.get(r)).is_some_and(|l| l.human);
        Ok((v.driver.is_some() && v.seat_index == to && !human, occupant))
    }
    fn copy_aim(&mut self, hull: usize, from: f64, to: f64) -> Result<(), Self::Error> {
        let v = self.st.vehicle.get_mut(self.hulls.hulls[hull].vehicle).unwrap();
        let a = &v.seats[from as usize];
        let (yaw, pitch, look_yaw, look_pitch) = (a.aim_yaw, a.pitch, a.look_yaw, a.look_pitch);
        let b = &mut v.seats[to as usize];
        b.aim_yaw = yaw; b.pitch = pitch; b.look_yaw = look_yaw; b.look_pitch = look_pitch; Ok(())
    }
    fn move_lease(&mut self, lease: Self::Lease, to: f64) -> Result<(), Self::Error> { self.hulls.move_seat(self.st, self.world, lease, to as usize); Ok(()) }
    fn aim(&mut self, hull: usize, to: f64) -> Result<(), Self::Error> {
        let v = self.st.vehicle.get_mut(self.hulls.hulls[hull].vehicle).unwrap();
        if to == 0.0 { rest_aim(v, 0); } else { clamp_aim(v, to as usize); } Ok(())
    }
    fn clear_sticks(&mut self, hull: usize) -> Result<(), Self::Error> {
        let v = self.st.vehicle.get_mut(self.hulls.hulls[hull].vehicle).unwrap(); v.stick_x = Some(0.0); v.stick_y = Some(0.0); Ok(())
    }
}

pub fn switch_seat<W: SeatWorld + ?Sized>(hulls: &mut Hulls, st: &mut Store, w: &mut W, hull: usize, lease: Option<Ref<SeatLease>>, from: usize, direction: i32) -> i32 {
    switch_with(&mut NativeCycle { hulls, st, world: w }, hull, lease, from as f64, direction as f64).unwrap() as i32
}

/// A network human's current lease, then the ordinary seat switch.
pub fn cycle_network_seat<W: SeatWorld + ?Sized>(hulls: &mut Hulls, st: &mut Store, w: &mut W, unit: u32, direction: i32) -> bool {
    cycle_with(&mut NativeCycle { hulls, st, world: w }, unit, direction as f64).unwrap()
}
