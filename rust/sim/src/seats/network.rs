//! A network human's seat (src/runtime/network-seat.js cycleNetworkSeat,
//! src/runtime/network-vehicle.js networkVehicleInput, and the seat switch
//! they share with the player's keys, src/boarding.js switchSeat): his
//! mouse and keys in a hull's seat, and his move to another seat.

use super::boarding::{seat_control, seat_look, LookScales, SeatControl};
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
    let look = LookScales {
        scale: v8math::min(2.0, command.mouse_scale.unwrap_or(1.0)),
        invert: command.invert_mouse,
        flight_scale: v8math::max(0.05, v8math::min(10.0, command.flight_sensitivity.unwrap_or(1.0))),
        flight_invert: command.flight_invert,
    };
    for &(dx, dy) in &command.look {
        seat_look(v, index, dx, dy, &look);
    }
    if seat_control(v, index) != Some(SeatControl::Flight) {
        let seat = &mut v.seats[index];
        seat.aim_yaw = seat.look_yaw.unwrap_or(seat.aim_yaw);
        seat.pitch = seat.look_pitch.unwrap_or(seat.pitch);
        clamp_aim(v, index);
    }
    seat_input(v, index, keys)
}

/// boarding.js switchSeat: a person's seat switch, on to the next seat that
/// way that no other person holds; a bot there trades places with him. Into
/// a gunner's seat his view goes with him, inside its arc; back at the
/// controls he aims along the hull. Leaving the pilot's seat lets go of
/// the stick. Returns the new seat, or -1.
pub fn switch_seat<W: SeatWorld + ?Sized>(hulls: &mut Hulls, st: &mut Store, w: &mut W, hull: usize, lease: Option<Ref<SeatLease>>, from: usize, direction: i32) -> i32 {
    let vref = hulls.hulls[hull].vehicle;
    let v = st.vehicle.get(vref).unwrap();
    let n = v.seats.len();
    if !v.alive || n < 2 {
        return -1;
    }
    let human = lease.and_then(|r| st.seat_lease.get(r)).is_some_and(|l| l.human);
    for k in 1..n as i32 {
        let to = (((from as i32 + direction * k) % n as i32) + n as i32) % n as i32;
        let to = to as usize;
        let v = st.vehicle.get(vref).unwrap();
        let occupant = v.seats[to].lease;
        if let Some(o) = occupant {
            if Some(o) != lease && person_lease(st.seat_lease.get(o).unwrap(), &st.unit) {
                continue;
            }
        }
        // The player's seat, seen from a network human's switch.
        if v.driver.is_some() && v.seat_index == to as f64 && !human {
            continue;
        }
        let v = st.vehicle.get_mut(vref).unwrap();
        if to > 0 {
            let a = v.seats[from].clone();
            let b = &mut v.seats[to];
            b.aim_yaw = a.aim_yaw;
            b.pitch = a.pitch;
            b.look_yaw = a.look_yaw;
            b.look_pitch = a.look_pitch;
        }
        if let Some(r) = lease {
            hulls.move_seat(st, w, r, to);
        }
        if let Some(o) = occupant {
            if Some(o) != lease {
                hulls.move_seat(st, w, o, from);
            }
        }
        let v = st.vehicle.get_mut(vref).unwrap();
        if to == 0 {
            rest_aim(v, 0);
        } else {
            clamp_aim(v, to);
        }
        if from == 0 {
            let v = st.vehicle.get_mut(vref).unwrap();
            v.stick_x = Some(0.0);
            v.stick_y = Some(0.0);
        }
        return to as i32;
    }
    -1
}

/// network-seat.js cycleNetworkSeat: an authenticated occupant through the
/// seats available, by the player's seat switch. Whether he moved.
pub fn cycle_network_seat<W: SeatWorld + ?Sized>(hulls: &mut Hulls, st: &mut Store, w: &mut W, unit: u32, direction: i32) -> bool {
    let Some(u) = st.unit.get(super::unit_ref(unit)) else { return false };
    if u.network_human != Some(true) {
        return false;
    }
    let Some(r) = u.seat else { return false };
    let lease = st.seat_lease.get(r).unwrap();
    let Some(vref) = lease.vehicle else { return false };
    if !st.vehicle.get(vref).is_some_and(|v| v.alive) || lease.released {
        return false;
    }
    let from = lease.index as usize;
    let hull: Option<usize> = hulls.hulls.iter().position(|h: &HullSide| h.vehicle == vref);
    let Some(hull) = hull else { return false };
    switch_seat(hulls, st, w, hull, Some(r), from, direction) >= 0
}
