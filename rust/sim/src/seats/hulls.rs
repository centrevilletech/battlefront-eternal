//! The hulls' seats (src/vehicles.js Vehicles: board, eligibleSeat,
//! requestExit, unboard, vacate, promote, move, displace, destroyedCrew,
//! latchCrews, seatOf; Vehicle: restYaw, restAim, clampAim, holdYaw,
//! seatFrameForward, sphere).
//!
//! A hull's seats and pose are the store's (`gen::Vehicle`, the hulls'
//! schema); what its seats read beyond them (its class's base and health
//! type, its seats' turret nodes and weapons, which the content gives, and
//! its last attacker, which the battle keeps) is its `HullSide`. Units are the store's; what a seat
//! sets of a unit's combat beyond the units' schema is its `CombatFlags`.

use super::bodies::{drop_body, place_on_foot, seat_body, unseat_body, CombatFlags};
use super::boarding::{boarding_seat, can_board, claim_seat, crewed, in_flight, may_leave, seat_free};
use super::exits::{classic_seat_exit, person_exit_point, ExitFrame, ExitSpot, HullFrame};
use super::lease::{latch_lease, live, network_human, new_lease, person_lease, seat_role, Controls};
use super::{unit_ref, Attacker, Holder, Reason, SeatWorld};
use crate::gen::{ExitRequest, Post, SeatLease, SeatLeaseView, Store, Unit, Vehicle};
use crate::rig::math;
use crate::store::{Nullable, Ref};
use crate::vehicles::hull::angle;
use std::collections::BTreeMap;
use three::{clamp, Matrix4, Vector3};

/// A hull as its seats read it beyond the store's fields.
#[derive(Clone, Debug, Default)]
pub struct HullSide {
    /// Its place in the battle's list (Vehicles.vehicles).
    pub index: usize,
    /// The store's hull (its team, flight state and virtual stick are the
    /// flight package's fields of it).
    pub vehicle: Ref<Vehicle>,
    /// Who last damaged it (for its crew's kill credit).
    pub last_attacker: Attacker,
    /// Its class's base is the hover's (`(def.base || 'hover') === 'hover'`).
    pub hover: bool,
    /// Its health type is an animal's (it does not shield its riders).
    pub animal: bool,
    /// Each seat's section names a turret node (TurretNodeName).
    pub turret_nodes: Vec<bool>,
    /// Each seat's number of weapons.
    pub weapons: Vec<usize>,
}

/// What a unit brings to a seat beyond the store's unit: the combat flags a
/// seat sets, and its class's NoEnterVehicles (Number.parseFloat of it, 0
/// where the class has none).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RiderFacts {
    pub combat: CombatFlags,
    pub no_enter: f64,
}

/// The battle's hulls as their seats see them (src/vehicles.js Vehicles):
/// the seconds of vehicle updates and the input batches so far, each hull
/// (in the battle's order; the methods name a hull by its place here), and
/// the riders' facts and seat controls the seats read.
#[derive(Clone, Debug, Default)]
pub struct Hulls {
    pub time: f64,
    pub ticks: f64,
    pub hulls: Vec<HullSide>,
    pub riders: BTreeMap<u32, RiderFacts>,
    pub controls: Controls,
    /// Leases made since this was last emptied, in the order made (a
    /// replay names them as the capture does).
    pub made: Vec<Ref<SeatLease>>,
}

/// Vehicles.vacate's options.
#[derive(Clone, Copy, Debug)]
pub struct Vacate {
    pub placement: bool,
    /// None: undefined (the default: as the original game, unless the unit
    /// is boarding something else).
    pub promote: Option<bool>,
    pub post: Option<Ref<Post>>,
    /// policy 'safe' (a person's search) rather than 'classic'.
    pub safe: bool,
    pub force: bool,
    pub reason: Reason,
}

impl Default for Vacate {
    fn default() -> Self {
        Vacate { placement: true, promote: None, post: None, safe: false, force: false, reason: Reason::Exit }
    }
}

const UP: Vector3 = Vector3::new(0.0, 1.0, 0.0);
const UNIT_SCALE: Vector3 = Vector3::new(1.0, 1.0, 1.0);

/// Vehicle.holdYaw (vehicles.js:774) with arcYaw folded in: a yaw held
/// inside a seat's limited arc, measured from the hull's heading.
fn hold_yaw(hull_yaw: f64, limits: [f64; 2], wrap_yaw: bool, yaw: f64) -> f64 {
    crate::vehicles::hull::hold_yaw(hull_yaw, limits, wrap_yaw, yaw)
}

/// Vehicle.clampAim (vehicles.js:763): a seat's aim held inside its limits
/// (its view's: `seatViews`).
pub fn clamp_aim(v: &mut Vehicle, i: usize) {
    if v.flying && i == 0 {
        return;
    }
    let yaw = v.yaw;
    let view = v.seat_views[i].clone();
    let seat = &mut v.seats[i];
    let (low, high) = (view.pitch_limits[0], view.pitch_limits[1]);
    seat.pitch = clamp(seat.pitch, low, high);
    if let Some(look_pitch) = seat.look_pitch {
        seat.look_pitch = Some(clamp(look_pitch, low, high));
    }
    let Some(limits) = view.yaw_limits else { return };
    seat.aim_yaw = hold_yaw(yaw, limits, view.wrap_yaw, seat.aim_yaw);
    if let Some(look_yaw) = seat.look_yaw {
        seat.look_yaw = Some(seat.aim_yaw + angle(hold_yaw(yaw, limits, view.wrap_yaw, look_yaw) - seat.aim_yaw));
    }
}

/// Vehicle.restYaw (vehicles.js:790): the hull's heading, or the middle of
/// an arc that leaves it out.
pub fn rest_yaw(v: &Vehicle, i: usize) -> f64 {
    let arc = v.seat_views.get(i).and_then(|s| s.yaw_limits);
    v.yaw + match arc {
        Some(arc) if arc[0] > 0.0 || arc[1] < 0.0 => (arc[0] + arc[1]) / 2.0,
        _ => 0.0,
    }
}

/// Vehicle.restAim (vehicles.js:780): a seat's aim as it is taken afresh.
pub fn rest_aim(v: &mut Vehicle, i: usize) {
    let yaw = rest_yaw(v, i);
    let flying = v.flying;
    let seat = &mut v.seats[i];
    seat.aim_yaw = yaw;
    seat.pitch = if flying { 0.0 } else { -0.12 };
    seat.look_yaw = None;
    seat.look_pitch = None;
    clamp_aim(v, i);
}

/// Vehicle.seatFrameForward (vehicles.js:824): the frame a seat lets its
/// occupant out in: a turret seat's own heading, otherwise the hull's.
fn seat_frame_forward(v: &Vehicle, side: &HullSide, i: usize) -> Vector3 {
    let seat = v.seats.get(i).unwrap_or(&v.seats[0]);
    let turret_node = side.turret_nodes.get(if v.seats.get(i).is_some() { i } else { 0 }).copied().unwrap_or(false);
    let yaw = if i > 0 && turret_node { seat.aim_yaw } else { v.yaw };
    Vector3::new(math::sin(yaw), 0.0, math::cos(yaw))
}

/// Vehicle.sphere(out) (vehicles.js:661): its box's centre in the world.
fn sphere_center(v: &Vehicle) -> Vector3 {
    let mut m = Matrix4::default();
    m.from_array(&v.matrix, 0);
    let mut c = super::v3(&v.center);
    c.apply_matrix4(&m);
    c
}

impl Hulls {
    /// The leased hull of a lease.
    fn side_of(&self, v: Ref<Vehicle>) -> usize {
        self.hulls.iter().position(|h| h.vehicle == v).expect("the lease's hull")
    }

    fn facts(&mut self, unit: u32) -> &mut RiderFacts {
        self.riders.entry(unit).or_default()
    }

    /// Vehicles.seatOf (2189): the seat a unit holds in one of these hulls.
    pub fn seat_of(&self, st: &Store, unit: u32) -> Option<Ref<SeatLease>> {
        let r = st.unit.get(unit_ref(unit))?.seat?;
        let lease = st.seat_lease.get(r)?;
        (!lease.released && lease.vehicle.is_some_and(|v| self.hulls.iter().any(|h| h.vehicle == v))).then_some(r)
    }

    /// Vehicles.eligibleSeat (2198): the seat a bot or a network human
    /// would take by the shared rules, or -1.
    pub fn eligible_seat<W: SeatWorld + ?Sized>(&mut self, st: &Store, w: &mut W, unit: u32, hull: usize, seat_index: i32) -> i32 {
        let Some(u) = st.unit.get(unit_ref(unit)) else { return -1 };
        if u.human && !network_human(u) {
            return -1;
        }
        let person = network_human(u);
        let no_enter = self.riders.get(&unit).map_or(0.0, |f| f.no_enter);
        if !can_board(u, person, no_enter) {
            return -1;
        }
        let side = &self.hulls[hull];
        let v = st.vehicle.get(side.vehicle).expect("the hull");
        boarding_seat(w, st, v, side, u.team, person, seat_index)
    }

    /// Vehicles.board (2210): seats a bot or a network human now; -1 takes
    /// the seat the rules give. A person takes a seat a bot holds; the bot
    /// moves on. Whether it boarded.
    pub fn board<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, unit: u32, hull: usize, seat_index: i32) -> bool {
        let index = self.eligible_seat(st, w, unit, hull, seat_index);
        if index < 0 || seat_index >= 0 && index != seat_index || !claim_seat(self, st, w, hull, index as usize) {
            return false;
        }
        let index = index as usize;
        let side = &self.hulls[hull];
        let vref = side.vehicle;
        let role = seat_role(&side.weapons, index);
        let hover = side.hover;
        let u = st.unit.get(unit_ref(unit)).expect("the unit");
        let network = network_human(u);
        let lease = new_lease(Some((unit, u.generation)), false, u.team, Some(vref), None, index as f64, role, self.time, self.ticks);
        let r = st.seat_lease.insert(lease);
        st.seats.leases.push(r);
        self.made.push(r);
        let v = st.vehicle.get_mut(vref).expect("the hull");
        let rest = rest_yaw(v, index);
        let flying = v.flying;
        let seat = &mut v.seats[index];
        seat.lease = Some(r);
        seat.aim_yaw = rest;
        seat.pitch = if index == 0 && !flying && (!hover || network) { -0.12 } else { 0.0 };
        seat.look_yaw = None;
        seat.look_pitch = None;
        // (setSeatPolicy: the seat's weapons, the combat package's records.)
        if !network {
            let c = w.controllable(r);
            st.seat_lease.get_mut(r).unwrap().controllable = Some(c);
        }
        let mut combat = self.facts(unit).combat;
        let u = st.unit.get_mut(unit_ref(unit)).expect("the unit");
        u.seat = Some(r);
        seat_body(u, &mut combat);
        self.facts(unit).combat = combat;
        st.vehicle.get_mut(vref).unwrap().sleeping = Some(false);
        w.sync_rider(r, st.unit.get_mut(unit_ref(unit)).unwrap());
        w.boarded(unit, Holder::Hull(self.hulls[hull].index), index);
        true
    }

    /// Vehicles.requestExit (2242): his exit at the next batch.
    pub fn request_exit(&mut self, st: &mut Store, unit: u32, post: Option<Ref<Post>>, promote: Option<bool>, placement: bool) -> bool {
        let Some(r) = self.seat_of(st, unit) else { return false };
        st.seat_lease.get_mut(r).unwrap().exit_request = Some(ExitRequest { post, promote, placement });
        true
    }

    /// Vehicles.unboard (2258): out now, by vacate's rules.
    pub fn unboard<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, unit: u32, how: Vacate) -> bool {
        let Some(r) = self.seat_of(st, unit) else { return false };
        self.vacate(st, w, r, how)
    }

    /// Vehicles.vacate (2266): out now. A bot is set down by the original
    /// game's placement (always somewhere); a person by his own search,
    /// which refuses when boxed in on the ground unless forced; out of a
    /// craft in flight, beside it in the air. reason Killed: he died on the
    /// seat, his body falls from where it rode.
    pub fn vacate<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, r: Ref<SeatLease>, how: Vacate) -> bool {
        let lease = st.seat_lease.get(r).expect("the lease");
        if lease.released {
            return true;
        }
        let vref = lease.vehicle.expect("a hull's lease");
        let hull = self.side_of(vref);
        let index = lease.index as usize;
        let unit = lease.unit.map(|u| u.slot);
        let human = lease.human;
        let person = person_lease(lease, &st.unit);
        let v = st.vehicle.get(vref).expect("the hull");
        let side = &self.hulls[hull];
        if how.reason == Reason::Exit && !how.force && !may_leave(v, side, lease, &st.unit) {
            return false;
        }
        let airborne = in_flight(v, side);
        // `unit?.soldier`: every unit has a body.
        let has_body = unit.and_then(|u| st.unit.get(unit_ref(u))).is_some();
        let fall = if how.reason == Reason::Killed && has_body { w.rider_drop(Holder::Hull(self.hulls[hull].index), index, unit.unwrap()) } else { None };
        let mut spot: Option<Vector3> = None;
        let mut yaw = 0.0;
        let alive = unit.and_then(|u| st.unit.get(unit_ref(u))).is_some_and(|u| u.alive && !u.dead);
        if how.placement && !human && has_body && alive {
            let v = st.vehicle.get(vref).expect("the hull");
            let side = &self.hulls[hull];
            if how.safe || person {
                let frame = HullFrame {
                    position: super::v3(&v.position),
                    yaw: v.yaw,
                    radius: v8math::max(v.size[0], v.size[2]) * 0.5,
                    height: v.size[1],
                    body: Some(v.body),
                    airborne,
                };
                let mut out = Vector3::default();
                if !person_exit_point(w, &frame, &mut out, airborne || how.force) {
                    return false;
                }
                spot = Some(out);
                // `v.seats[index]?.aimYaw ?? v.aimYaw`.
                yaw = match v.seats.get(index) {
                    Some(seat) => seat.aim_yaw,
                    None => v.seats[v.seat_index as usize].aim_yaw,
                };
            } else {
                let forward = seat_frame_forward(v, side, index);
                let center = sphere_center(v);
                let frame = ExitFrame { origin: super::v3(&v.position), forward, up: UP, center, radius: v.bound_radius, body: Some(v.body) };
                let mut out = ExitSpot::default();
                classic_seat_exit(w, &frame, &mut out);
                spot = Some(out.position);
                yaw = math::atan2(out.forward.x, out.forward.z);
            }
        }
        {
            let lease = st.seat_lease.get_mut(r).unwrap();
            lease.released = true;
            lease.exit_request = None;
        }
        let v = st.vehicle.get_mut(vref).unwrap();
        if v.seats.get(index).is_some_and(|s| s.lease == Some(r)) {
            v.seats[index].lease = None;
        }
        if let Some(id) = unit {
            let mut combat = self.facts(id).combat;
            let u = st.unit.get_mut(unit_ref(id)).expect("the lease's unit");
            if u.seat == Some(r) {
                u.seat = None;
                unseat_body(u, &mut combat);
            }
            self.facts(id).combat = combat;
        }
        if let Some(fall) = fall {
            let mount = if self.hulls[hull].animal { Some(st.vehicle.get(vref).unwrap().body) } else { None };
            drop_body(st.unit.get_mut(unit_ref(unit.unwrap())).unwrap(), &fall, mount);
        }
        let promote = match how.reason {
            Reason::Killed => how.promote.unwrap_or(true),
            Reason::Exit => match how.promote {
                Some(p) => p,
                None => w.controller_state(unit) != Some(5.0),
            },
            _ => false,
        };
        if promote {
            self.promote(st, w, hull, index);
        }
        if let Some(spot) = spot {
            let u = st.unit.get_mut(unit_ref(unit.unwrap())).unwrap();
            place_on_foot(w, u, &spot, yaw, airborne && person);
        }
        w.unboarded(unit, Holder::Hull(self.hulls[hull].index), index, how.post, how.reason);
        true
    }

    /// Vehicles.promote (2311): the last bot aboard in a later seat moves
    /// into the vacated one; a person aboard is never moved.
    pub fn promote<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, hull: usize, index: usize) -> Option<Ref<SeatLease>> {
        let vref = self.hulls[hull].vehicle;
        let v = st.vehicle.get(vref).unwrap();
        if !v.alive || !seat_free(v, index) {
            return None;
        }
        let mut from: Option<usize> = None;
        for i in index + 1..v.seats.len() {
            if !crewed(v, &st.seat_lease, i) {
                continue;
            }
            let r = v.seats[i].lease.unwrap();
            let lease = st.seat_lease.get(r).unwrap();
            if !person_lease(lease, &st.unit) && live(lease, r, &st.unit) {
                from = Some(i);
            }
        }
        let from = from?;
        let r = v.seats[from].lease.unwrap();
        Some(self.move_seat(st, w, r, index))
    }

    /// Vehicles.move (2319): a lease to another seat of its hull; its batch's
    /// edges are dropped, held values carry on, and a bot's seat gets a new
    /// control for the AI.
    pub fn move_seat<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, r: Ref<SeatLease>, index: usize) -> Ref<SeatLease> {
        let lease = st.seat_lease.get(r).unwrap();
        let vref = lease.vehicle.unwrap();
        let hull = self.side_of(vref);
        let from = lease.index as usize;
        let human = lease.human;
        let network = lease.unit.and_then(|u| st.unit.get(u)).is_some_and(network_human);
        let unit = lease.unit.map(|u| u.slot);
        let v = st.vehicle.get_mut(vref).unwrap();
        if v.seats.get(from).is_some_and(|s| s.lease == Some(r)) {
            v.seats[from].lease = None;
        }
        v.seats[index].lease = Some(r);
        let role = seat_role(&self.hulls[hull].weapons, index);
        let lease = st.seat_lease.get_mut(r).unwrap();
        lease.index = index as f64;
        lease.role = role;
        lease.input.pressed[0] = false;
        lease.input.pressed[1] = false;
        lease.input.reload = false;
        lease.input.exit = false;
        // (setSeatPolicy: the seat's weapons, the combat package's records.)
        if !human && !network {
            let c = w.controllable(r);
            st.seat_lease.get_mut(r).unwrap().controllable = Some(c);
        }
        w.moved(unit, Holder::Hull(self.hulls[hull].index), index);
        r
    }

    /// Vehicles.displace (2339): the player takes a seat a bot holds: the
    /// bot moves to the next free seat after its own, or, with none, gets
    /// out (no promotion).
    pub fn displace<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, r: Ref<SeatLease>) -> Option<Ref<SeatLease>> {
        let lease = st.seat_lease.get(r).unwrap();
        let vref = lease.vehicle.unwrap();
        let index = lease.index as usize;
        let v = st.vehicle.get(vref).unwrap();
        let n = v.seats.len();
        for k in 1..n {
            let i = (index + k) % n;
            if seat_free(st.vehicle.get(vref).unwrap(), i) {
                return Some(self.move_seat(st, w, r, i));
            }
        }
        self.vacate(st, w, r, Vacate { placement: true, promote: Some(false), reason: Reason::Displaced, ..Vacate::default() });
        None
    }

    /// Vehicles.destroyedCrew (2351): a hull destroyed: every bot aboard is
    /// let go of its seat, all at once, then dies, credited to whoever
    /// destroyed it.
    pub fn destroyed_crew<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W, hull: usize) {
        let vref = self.hulls[hull].vehicle;
        let attacker = self.hulls[hull].last_attacker;
        let mut dying = Vec::new();
        let n = st.vehicle.get(vref).unwrap().seats.len();
        for i in 0..n {
            let Some(r) = st.vehicle.get(vref).unwrap().seats[i].lease else { continue };
            if st.seat_lease.get(r).unwrap().human {
                continue;
            }
            self.vacate(st, w, r, Vacate { placement: false, promote: Some(false), reason: Reason::Destroyed, ..Vacate::default() });
            if let Some(u) = st.seat_lease.get(r).unwrap().unit {
                let unit = st.unit.get(u).unwrap();
                if unit.alive && !unit.combat_dead {
                    dying.push(u.slot);
                }
            }
        }
        for unit in dying {
            w.kill(st, super::Seating::Hulls(self), unit, attacker);
        }
    }

    /// Vehicles.latchCrews (2085): the seat contract's input batch: stale
    /// leases are let go, every live bot's intent is frozen, and exit
    /// requests are carried out.
    pub fn latch_crews<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, w: &mut W) {
        self.ticks += 1.0;
        let mut exits = Vec::new();
        for hull in 0..self.hulls.len() {
            let vref = self.hulls[hull].vehicle;
            let n = st.vehicle.get(vref).unwrap().seats.len();
            for i in 0..n {
                let v = st.vehicle.get(vref).unwrap();
                let Some(r) = v.seats[i].lease else { continue };
                let lease = st.seat_lease.get(r).unwrap();
                if lease.human {
                    continue;
                }
                if !v.alive || !live(lease, r, &st.unit) {
                    self.vacate(st, w, r, Vacate { placement: false, promote: Some(false), reason: Reason::Stale, ..Vacate::default() });
                    continue;
                }
                latch_lease(&mut st.seat_lease, &mut st.unit, &mut self.controls, r);
                let lease = st.seat_lease.get(r).unwrap();
                if lease.input.exit || lease.exit_request.is_some() {
                    exits.push(r);
                }
            }
        }
        for r in exits {
            let lease = st.seat_lease.get(r).unwrap();
            if lease.released {
                continue;
            }
            let (placement, promote, post) = match &lease.exit_request {
                Some(q) => (q.placement, q.promote, q.post),
                None => (true, None, None),
            };
            self.vacate(st, w, r, Vacate { placement, promote, post, ..Vacate::default() });
        }
    }
}

/// A hull's seats' leases as the hulls' tick reads them (`seatLeases`: the
/// lease, or for an empty seat an object with nothing in it), made again
/// from the leases the store holds.
pub fn lease_views(st: &mut Store, v: Ref<Vehicle>) {
    let Some(hull) = st.vehicle.get(v) else { return };
    let views: Vec<SeatLeaseView> = hull
        .seats
        .iter()
        .map(|seat| match seat.lease.and_then(|r| st.seat_lease.get(r)) {
            Some(lease) => SeatLeaseView {
                human: Some(lease.human),
                unit: match lease.unit {
                    Some(u) => Nullable::Value(u),
                    None => Nullable::Null,
                },
                network_human: lease.unit.and_then(|u| st.unit.get(u)).and_then(|u| u.network_human),
                forward: Some(lease.input.forward),
                right: Some(lease.input.right),
                turn: Some(lease.input.turn),
                pitch: Some(lease.input.pitch),
                brake: Some(lease.input.brake),
            },
            None => SeatLeaseView { unit: Nullable::Undefined, ..SeatLeaseView::default() },
        })
        .collect();
    st.vehicle.get_mut(v).unwrap().seat_leases = views;
}

/// The lease's unit, by id.
pub fn lease_unit(st: &Store, r: Ref<SeatLease>) -> Option<&Unit> {
    st.seat_lease.get(r)?.unit.and_then(|u| st.unit.get(u))
}

/// UNIT_SCALE, for frames composed as the JavaScript composes them.
pub const ONE: Vector3 = UNIT_SCALE;
