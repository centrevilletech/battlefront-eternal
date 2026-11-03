//! The rules every boarder follows (src/boarding.js): who may get into
//! anything, which seat of a hull he takes, whether he may get out, and a
//! person's seat switch and mouse in a seat.

use super::hulls::{HullSide, Hulls};
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
pub fn in_flight(v: &Vehicle, _side: &HullSide) -> bool {
    v.flying && v.alive && v.flight_state != Some(crate::gen::FlightState::Landed)
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
pub fn controlling_team(v: &Vehicle, _side: &HullSide, leases: &Slab<SeatLease>) -> Option<f64> {
    for seat in &v.seats {
        if let Some(r) = seat.lease {
            return Some(leases.get(r).expect("a seat's lease").team);
        }
    }
    v.team
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

/// What a seat's keys and mouse do to the hull (boarding.js seatControl):
/// the pilot of a craft flies it, the driver of anything else drives it,
/// and every other seat only aims and fires its own guns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeatControl {
    Flight,
    Drive,
    Turret,
}

/// boarding.js seatControl (None: no such seat).
pub fn seat_control(v: &Vehicle, index: usize) -> Option<SeatControl> {
    v.seats.get(index)?;
    Some(if index > 0 {
        SeatControl::Turret
    } else if v.flying {
        SeatControl::Flight
    } else {
        SeatControl::Drive
    })
}

/// A seat's mouse settings (boarding.js seatLook's last argument).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookScales {
    pub scale: f64,
    pub invert: bool,
    pub flight_scale: f64,
    pub flight_invert: bool,
}

/// boarding.js seatLook: mouse counts on a seat. A craft's pilot moves its
/// virtual stick (stickX, stickY, kept inside the unit circle); any other
/// seat turns its look, held inside the seat's limits.
pub fn seat_look(v: &mut Vehicle, index: usize, dx: f64, dy: f64, s: &LookScales) {
    if seat_control(v, index) == Some(SeatControl::Flight) {
        let x = three::clamp(v.stick_x.unwrap_or(f64::NAN) + (dx * s.flight_scale) / 120.0, -1.0, 1.0);
        v.stick_x = Some(x);
        let y = three::clamp(v.stick_y.unwrap_or(f64::NAN) + ((if s.flight_invert { -dy } else { dy }) * s.flight_scale) / 120.0, -1.0, 1.0);
        v.stick_y = Some(y);
        let radius = v8math::max(1.0, v8math::hypot(x, y));
        v.stick_x = Some(x / radius);
        v.stick_y = Some(y / radius);
        return;
    }
    let seat = &mut v.seats[index];
    seat.look_yaw = Some(seat.look_yaw.unwrap_or(seat.aim_yaw) - (dx * s.scale) / 2048.0);
    seat.look_pitch = Some(seat.look_pitch.unwrap_or(seat.pitch) + ((if s.invert { dy } else { -dy }) * s.scale) / 2048.0);
    super::hulls::clamp_aim(v, index);
}

/// boarding.js turretTakeover: whether a person may take this turret from
/// whoever mans it: unmanned, or manned by a bot's lease.
pub fn turret_takeover(gunner: bool, lease: Option<&SeatLease>, units: &Slab<Unit>) -> bool {
    if !gunner {
        return true;
    }
    lease.is_some_and(|l| !l.released && !person_lease(l, units))
}

/// Vehicles.enterable (vehicles.js:2519): the nearest hull in reach ahead of
/// an eye that `may` takes (by default one the page's player is not
/// driving): its box's centre within its radius and 3.5 m, ahead of the
/// aim, and nothing but its own body in the way. Its place, or None.
pub fn enterable_hull<W: SeatWorld + ?Sized>(w: &mut W, st: &Store, hulls: &[super::hulls::HullSide], eye: &three::Vector3, direction: &three::Vector3, may: &mut dyn FnMut(&mut W, usize) -> bool) -> Option<usize> {
    let mut best = None;
    let mut distance = f64::INFINITY;
    for (i, h) in hulls.iter().enumerate() {
        let v = st.vehicle.get(h.vehicle).unwrap();
        if !v.alive || !may(w, i) {
            continue;
        }
        let v = st.vehicle.get(h.vehicle).unwrap();
        let mut m = three::Matrix4::default();
        m.from_array(&v.matrix, 0);
        let mut center = super::v3(&v.center);
        center.apply_matrix4(&m);
        let mut toward = three::Vector3::default();
        toward.sub_vectors(&center, eye);
        let span = toward.length();
        let radius = v8math::max(v.size[0], v.size[2]) * 0.5;
        if span > radius + 3.5 || toward.dot(direction) < 0.0 {
            continue;
        }
        toward.normalize();
        if let Some(hit) = w.cast(eye, &toward, span, None) {
            if hit.body != Some(v.body) && hit.distance < span {
                continue;
            }
        }
        if span < distance {
            distance = span;
            best = Some(i);
        }
    }
    best
}

/// LiveObjects.enterable (props.js:1731): the built turret an eye faces, the
/// one nearest its line (by 16 x miss squared + depth) whose drum is within
/// reach; a manned one only where `may` takes it (a person's takeover).
pub fn enterable_turret(turrets: &[super::turrets::TurretSide], eye: &three::Vector3, dir: &three::Vector3, may: Option<&dyn Fn(usize) -> bool>) -> Option<usize> {
    let mut best = None;
    let mut best_score = f64::INFINITY;
    for (i, t) in turrets.iter().enumerate() {
        if !t.alive || t.gunner.is_some() && !may.is_some_and(|f| f(i)) {
            continue;
        }
        let mut m = three::Matrix4::default();
        m.from_array(&t.desc.matrix, 0);
        let mut c = three::Vector3::new(0.0, 1.65, 0.32);
        c.apply_matrix4(&m);
        let reach = 2.25;
        let mut to_c = c;
        to_c.sub(eye);
        let along = to_c.dot(dir);
        if eye.distance_to(&t.desc.position) > reach + 3.5 {
            continue;
        }
        if along < 0.0 || along > reach + 5.0 {
            continue;
        }
        let miss = to_c.add_scaled_vector(dir, -along).length();
        if miss > reach + along {
            continue;
        }
        let score = 16.0 * miss * miss + along;
        if score < best_score {
            best_score = score;
            best = Some(i);
        }
    }
    best
}

/// What a soldier's use key gets him into (boarding.js mountInReach).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mount {
    Hull(usize),
    Turret(usize),
}

/// boarding.js mountInReach: the nearest hull in reach ahead of him that he
/// may board, else the turret he faces (a person may take a bot's over).
/// `hulls`: the battle's hulls; `crews`: its turret crews.
pub fn mount_in_reach<W: SeatWorld + ?Sized>(w: &mut W, st: &Store, hulls: &super::hulls::Hulls, crews: &super::turrets::Crews, unit: u32, person: bool) -> Option<Mount> {
    let u = st.unit.get(super::unit_ref(unit))?;
    let no_enter = hulls.riders.get(&unit).or_else(|| crews.riders.get(&unit)).map_or(0.0, |f| f.no_enter);
    if !can_board(u, person, no_enter) {
        return None;
    }
    let UnitBody::Soldier(s) = &u.soldier else { return None };
    let eye = three::Vector3::new(s.position[0], s.position[1] + 1.7, s.position[2]);
    let aim = three::Vector3::new(math_sin(s.aim_yaw) * math_cos(s.aim_pitch), math_sin(s.aim_pitch), math_cos(s.aim_yaw) * math_cos(s.aim_pitch));
    let team = u.team;
    let sides = &hulls.hulls;
    let vehicle = enterable_hull(w, st, sides, &eye, &aim, &mut |w: &mut W, i: usize| {
        let side = &sides[i];
        let v = st.vehicle.get(side.vehicle).unwrap();
        boarding_seat(w, st, v, side, team, person, -1) >= 0
    });
    if let Some(i) = vehicle {
        return Some(Mount::Hull(sides[i].index));
    }
    let takeover = |i: usize| {
        let t = &crews.turrets[i];
        turret_takeover(t.gunner.is_some(), t.lease.and_then(|r| st.seat_lease.get(r)), &st.unit)
    };
    let turret = enterable_turret(&crews.turrets, &eye, &aim, if person { Some(&takeover) } else { None });
    turret.map(|i| Mount::Turret(crews.turrets[i].index))
}

fn math_sin(x: f64) -> f64 {
    crate::rig::math::sin(x)
}

fn math_cos(x: f64) -> f64 {
    crate::rig::math::cos(x)
}

/// boarding.js leaveKilled: a rider killed on his seat leaves it at once,
/// his body falling from where it rode (the seat is handed on as after any
/// exit); a turret's gunner the same. Whether he held a seat. (The page's
/// own player leaves through his VehicleDriver or Gunner, not here.)
pub fn leave_killed<W: super::turrets::CrewWorld + ?Sized>(hulls: &mut super::hulls::Hulls, crews: &mut super::turrets::Crews, st: &mut Store, w: &mut W, unit: u32) -> bool {
    let Some(r) = st.unit.get(super::unit_ref(unit)).and_then(|u| u.seat) else { return false };
    let lease = st.seat_lease.get(r).unwrap();
    if lease.released {
        return false;
    }
    if lease.vehicle.is_some() {
        debug_assert!(!lease.human, "the page's own player leaves through his VehicleDriver");
        hulls.vacate(st, w, r, super::hulls::Vacate { placement: false, reason: super::Reason::Killed, ..super::hulls::Vacate::default() });
    } else if lease.turret.is_some() {
        debug_assert!(!lease.human, "the page's own player leaves through his Gunner");
        crews.vacate(st, w, r, super::turrets::Let { placement: false, reason: super::Reason::Killed, ..super::turrets::Let::default() });
    } else {
        return false;
    }
    true
}
