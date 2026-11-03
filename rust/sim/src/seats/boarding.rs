//! The rules every boarder follows (src/boarding.js): who may get into
//! anything, which seat of a hull he takes, whether he may get out, and a
//! person's seat switch and mouse in a seat.

use super::hulls::{FlightState, HullSide, Hulls};
use super::lease::{network_human, person_lease};
use super::{truthy, SeatWorld};
use crate::gen::{SeatLease, Store, Unit, UnitBody, Vehicle};
use crate::store::{Ref, Slab};

/// boarding.js personUnit: a person (the player, a network human) rather
/// than a bot.
pub fn person_unit(u: &Unit) -> bool {
    u.human || network_human(u)
}

/// boarding.js inFlight: a craft off the ground (taking off, flying or
/// landing).
pub fn in_flight(v: &Vehicle, side: &HullSide) -> bool {
    v.flying && v.alive && side.flight_state != Some(FlightState::Landed)
}

/// boarding.js teamRelation: 1 friend, 0 neutral, -1 enemy, without
/// alliances.
pub fn team_relation(a: f64, b: f64) -> f64 {
    if a == b {
        if truthy(a) {
            1.0
        } else {
            0.0
        }
    } else if !truthy(a) || !truthy(b) {
        0.0
    } else {
        -1.0
    }
}

/// boarding.js canBoard: alive, a soldier on foot in no seat (not a
/// droideka), of a class that may enter vehicles; a person not in a jump,
/// fall or jet. `no_enter`: Number.parseFloat of the class's
/// NoEnterVehicles (0 when it has none).
pub fn can_board(u: &Unit, person: bool, no_enter: f64) -> bool {
    if !u.alive || u.dead || u.seat.is_some() || u.walker {
        return false;
    }
    let UnitBody::Soldier(s) = &u.soldier else { return false };
    if no_enter != 0.0 {
        return false;
    }
    !person || s.air.is_none()
}

/// Vehicle.seatFree (vehicles.js:696): the seat is there, nobody's lease
/// holds it, and the page's player is not in it.
pub fn seat_free(v: &Vehicle, i: usize) -> bool {
    match v.seats.get(i) {
        Some(seat) => seat.lease.is_none() && !(v.driver.is_some() && i as f64 == v.seat_index),
        None => false,
    }
}

/// Vehicle.firstFreeSeat.
pub fn first_free_seat(v: &Vehicle) -> i32 {
    (0..v.seats.len()).find(|&i| seat_free(v, i)).map_or(-1, |i| i as i32)
}

/// Vehicle.crewed: a bot holds seat i.
pub fn crewed(v: &Vehicle, leases: &Slab<SeatLease>, i: usize) -> bool {
    v.seats.get(i).and_then(|s| s.lease).and_then(|r| leases.get(r)).is_some_and(|l| !l.human)
}

/// Vehicle.controllingTeam: its first occupant's (pilot first), else its
/// own (undefined before it spawned: None).
pub fn controlling_team(v: &Vehicle, side: &HullSide, leases: &Slab<SeatLease>) -> Option<f64> {
    for seat in &v.seats {
        if let Some(r) = seat.lease {
            return Some(leases.get(r).expect("a seat's lease").team);
        }
    }
    side.team
}

/// boarding.js boardingSeat: the seat a soldier of `team` boarding `v`
/// takes, or -1 when he may not. A hull anyone holds takes only that
/// crew's friends; a bot boards only its own team's hulls (a hull of no
/// team, anyone's) and only a free seat; a person takes `preferred` when it
/// is free, else the first seat that is free or a bot holds.
pub fn boarding_seat<W: SeatWorld + ?Sized>(w: &mut W, st: &Store, v: &Vehicle, side: &HullSide, team: f64, person: bool, preferred: i32) -> i32 {
    if !v.alive || v.seats.is_empty() {
        return -1;
    }
    for seat in &v.seats {
        let Some(r) = seat.lease else { continue };
        let lease = st.seat_lease.get(r).expect("a seat's lease");
        if !lease.released && !(w.relation(team, lease.team) > 0.0) {
            return -1;
        }
    }
    if !person {
        let own = controlling_team(v, side, &st.seat_lease);
        if let Some(own) = own {
            if own > 0.0 && own != team {
                return -1;
            }
        }
        if preferred >= 0 {
            return if seat_free(v, preferred as usize) { preferred } else { -1 };
        }
        return first_free_seat(v);
    }
    if preferred >= 0 && seat_free(v, preferred as usize) {
        return preferred;
    }
    for i in 0..v.seats.len() {
        let free = seat_free(v, i);
        if free {
            return i as i32;
        }
        if let Some(r) = v.seats[i].lease {
            if !person_lease(st.seat_lease.get(r).expect("a seat's lease"), &st.unit) {
                return i as i32;
            }
        }
    }
    -1
}

/// boarding.js mayLeave: a person may get out at any time; a bot stays
/// aboard a craft in flight.
pub fn may_leave(v: &Vehicle, side: &HullSide, lease: &SeatLease, units: &Slab<Unit>) -> bool {
    person_lease(lease, units) || !in_flight(v, side)
}

/// boarding.js claimSeat: makes seat `index` his; a bot holding it moves
/// on to the next free seat or gets out (Vehicles.displace). Whether the
/// seat is free now.
pub fn claim_seat<W: SeatWorld + ?Sized>(hulls: &mut Hulls, st: &mut Store, w: &mut W, hull: usize, index: usize) -> bool {
    let v = st.vehicle.get(hulls.hulls[hull].vehicle).expect("the hull");
    if let Some(r) = v.seats.get(index).and_then(|s| s.lease) {
        let lease = st.seat_lease.get(r).expect("a seat's lease");
        if !lease.released && !person_lease(lease, &st.unit) {
            hulls.displace(st, w, r);
        }
    }
    let v = st.vehicle.get(hulls.hulls[hull].vehicle).expect("the hull");
    seat_free(v, index)
}

/// A lease's hull's place among the battle's hulls.
pub fn hull_of(hulls: &Hulls, lease: &SeatLease) -> usize {
    let v: Ref<Vehicle> = lease.vehicle.expect("a hull's lease");
    hulls.hulls.iter().position(|h| h.vehicle == v).expect("the lease's hull")
}
