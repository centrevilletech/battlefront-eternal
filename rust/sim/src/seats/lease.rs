//! A seat's lease (src/seats.js SeatLease) and the intent its seat freezes
//! once a batch (SeatInput: latch, take, takeReload).
//!
//! The lease is the store's (`gen::SeatLease`, rust/sim/schema/seats.mjs);
//! the intent it plays is its unit's (`gen::Intent`, the units' schema) or,
//! once the AI drives the seat, the seat's own (`SeatIntent`: SeatUnit's,
//! seats.js blankSeatIntent).

use super::{or_zero, truthy};
use crate::gen::{Intent, SeatInput, SeatLease, SeatRole, SeatUnit, Turret, Unit, Vehicle};
use crate::store::{Ref, Slab};
use three::Vector3;

/// seats.js seatRole: seat 0 drives; a seat with weapons guns; any other
/// carries. `weapons`: each seat's number of weapons.
pub fn seat_role(weapons: &[usize], index: usize) -> SeatRole {
    if index == 0 {
        return SeatRole::Pilot;
    }
    if weapons.get(index).copied().unwrap_or(0) > 0 {
        SeatRole::Gunner
    } else {
        SeatRole::Passenger
    }
}

/// A fresh SeatInput (seats.js:181): idle until the first batch, its aim
/// straight ahead.
pub fn idle_input() -> SeatInput {
    SeatInput {
        forward: 0.0,
        right: 0.0,
        turn: 0.0,
        pitch: 0.0,
        brake: false,
        up: 0.0,
        down: 0.0,
        aim: [0.0, 0.0, 1.0],
        aiming: false,
        held: vec![false, false],
        pressed: vec![false, false],
        reload: false,
        exit: false,
        trick: None,
        immelmann: None,
        boost: None,
    }
}

/// `new SeatLease({ unit, human, team, vehicle, turret, index, role, time,
/// tick })` (seats.js:87): `unit` with the generation it is granted to.
#[allow(clippy::too_many_arguments)]
pub fn new_lease(unit: Option<(u32, f64)>, human: bool, team: f64, vehicle: Option<Ref<Vehicle>>, turret: Option<Ref<Turret>>, index: f64, role: SeatRole, time: f64, tick: f64) -> SeatLease {
    SeatLease {
        unit: unit.map(|(id, _)| Ref::new(id, 0)),
        generation: unit.map_or(0.0, |(_, generation)| generation),
        human,
        team,
        vehicle,
        turret,
        index,
        role,
        since: time,
        tick,
        was: [0.0, 0.0],
        exit_request: None,
        released: false,
        input: idle_input(),
        controllable: None,
    }
}

/// SeatLease.live (seats.js:132): not let go, and (for a bot) the same life
/// of its unit, alive, still sitting here.
pub fn live(lease: &SeatLease, me: Ref<SeatLease>, units: &Slab<Unit>) -> bool {
    if lease.released {
        return false;
    }
    if lease.human {
        return true;
    }
    let Some(u) = lease.unit.and_then(|r| units.get(r)) else { return false };
    u.alive && !u.dead && u.generation == lease.generation && u.seat == Some(me)
}

/// A unit's `networkHuman` (undefined for a bot: false).
pub fn network_human(u: &Unit) -> bool {
    u.network_human == Some(true)
}

/// boarding.js personLease: a person's lease (the player's, or a network
/// human's) rather than a bot's.
pub fn person_lease(lease: &SeatLease, units: &Slab<Unit>) -> bool {
    lease.human || lease.unit.and_then(|r| units.get(r)).is_some_and(network_human)
}

/// The seat as the AI drives it (SeatUnit) as its lease plays it: whether
/// a controller drives it, and its own intent (seats.js blankSeatIntent
/// and what the AI writes there).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeatControl {
    pub driven: bool,
    pub intent: SeatIntent,
}

/// A seat's own intent (seats.js blankSeatIntent): the units' intent
/// without the jet's direction, with the vehicle fields brake, up and down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SeatIntent {
    pub forward: f64,
    pub right: f64,
    pub turn: f64,
    pub pitch: f64,
    pub jump: bool,
    pub crouch: bool,
    pub prone: bool,
    pub r#use: bool,
    pub exit: bool,
    pub buttons: [f64; 2],
    pub reload: bool,
    pub next_primary: bool,
    pub previous_primary: bool,
    pub next_secondary: bool,
    pub zoom: bool,
    pub aim: Option<Vector3>,
    pub brake: bool,
    pub up: f64,
    pub down: f64,
    /// What the AI's craft steering adds (src/ai/classic/vehicles/steering.js).
    pub trick: bool,
    pub immelmann: bool,
    pub boost: bool,
}

/// What SeatInput.latch reads of an intent and clears in it: a unit's or a
/// seat's (`lease.intent`, seats.js:125).
pub trait PlayIntent {
    fn forward(&self) -> f64;
    fn right(&self) -> f64;
    fn turn(&self) -> f64;
    fn pitch(&self) -> f64;
    /// `Boolean(intent.brake)`.
    fn brake(&self) -> bool;
    /// `intent.up ? 1 : 0`'s test.
    fn up(&self) -> bool;
    fn down(&self) -> bool;
    fn aim(&self) -> Option<Vector3>;
    fn buttons(&self) -> [f64; 2];
    fn reload(&self) -> bool;
    /// `Boolean(intent.trick)`, `immelmann`, `boost`: a craft's actions.
    fn trick(&self) -> bool;
    fn immelmann(&self) -> bool;
    fn boost(&self) -> bool;
    /// `Boolean(intent.use || intent.exit)`.
    fn exit_edge(&self) -> bool;
    /// The edges a seat takes or drops (seats.js:224-226).
    fn clear_edges(&mut self);
}

impl PlayIntent for Intent {
    fn forward(&self) -> f64 {
        self.forward
    }
    fn right(&self) -> f64 {
        self.right
    }
    fn turn(&self) -> f64 {
        self.turn
    }
    fn pitch(&self) -> f64 {
        self.pitch
    }
    fn brake(&self) -> bool {
        self.brake == Some(true)
    }
    fn up(&self) -> bool {
        self.up.is_some_and(truthy)
    }
    fn down(&self) -> bool {
        self.down.is_some_and(truthy)
    }
    fn aim(&self) -> Option<Vector3> {
        self.aim.as_ref().map(|a| Vector3::new(a[0], a[1], a[2]))
    }
    fn buttons(&self) -> [f64; 2] {
        self.buttons
    }
    fn reload(&self) -> bool {
        self.reload
    }
    // A unit's own intent has none of a craft's actions (the units' schema
    // declares none; the AI writes them on a seat's own intent).
    fn trick(&self) -> bool {
        false
    }
    fn immelmann(&self) -> bool {
        false
    }
    fn boost(&self) -> bool {
        false
    }
    fn exit_edge(&self) -> bool {
        self.r#use || self.exit
    }
    fn clear_edges(&mut self) {
        self.reload = false;
        self.r#use = false;
        self.exit = false;
        self.jump = false;
        self.crouch = false;
        self.prone = false;
        self.next_primary = false;
        self.previous_primary = false;
        self.next_secondary = false;
        self.zoom = false;
    }
}

impl PlayIntent for SeatIntent {
    fn forward(&self) -> f64 {
        self.forward
    }
    fn right(&self) -> f64 {
        self.right
    }
    fn turn(&self) -> f64 {
        self.turn
    }
    fn pitch(&self) -> f64 {
        self.pitch
    }
    fn brake(&self) -> bool {
        self.brake
    }
    fn up(&self) -> bool {
        truthy(self.up)
    }
    fn down(&self) -> bool {
        truthy(self.down)
    }
    fn aim(&self) -> Option<Vector3> {
        self.aim
    }
    fn buttons(&self) -> [f64; 2] {
        self.buttons
    }
    fn reload(&self) -> bool {
        self.reload
    }
    fn trick(&self) -> bool {
        self.trick
    }
    fn immelmann(&self) -> bool {
        self.immelmann
    }
    fn boost(&self) -> bool {
        self.boost
    }
    fn exit_edge(&self) -> bool {
        self.r#use || self.exit
    }
    fn clear_edges(&mut self) {
        self.reload = false;
        self.r#use = false;
        self.exit = false;
        self.jump = false;
        self.crouch = false;
        self.prone = false;
        self.next_primary = false;
        self.previous_primary = false;
        self.next_secondary = false;
        self.zoom = false;
    }
}

/// seats.js clamp: `v < lo ? lo : v > hi ? hi : v`.
#[inline(always)]
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// SeatInput.latch (seats.js:201): freezes the intent for this batch. Held
/// analogs and buttons are copied; edges are taken from the intent (cleared
/// there) so they happen once; the edges a seat has no use for are dropped.
/// `aim_limit`: a network human at a turret turns at up to 16, anyone else
/// at 1.
pub fn latch(input: &mut SeatInput, was: &mut [f64; 2], intent: &mut dyn PlayIntent, aim_limit: f64) {
    input.forward = clamp(or_zero(intent.forward()), -1.0, 1.0);
    input.right = clamp(or_zero(intent.right()), -1.0, 1.0);
    input.turn = clamp(or_zero(intent.turn()), -aim_limit, aim_limit);
    input.pitch = clamp(or_zero(intent.pitch()), -aim_limit, aim_limit);
    input.brake = intent.brake();
    input.trick = Some(intent.trick());
    input.immelmann = Some(intent.immelmann());
    input.boost = Some(intent.boost());
    input.up = if intent.up() { 1.0 } else { 0.0 };
    input.down = if intent.down() { 1.0 } else { 0.0 };
    let aim = intent.aim();
    input.aiming = aim.is_some_and(|a| a.length_sq() > 1e-12);
    if input.aiming {
        let mut a = aim.unwrap();
        a.normalize();
        input.aim = [a.x, a.y, a.z];
    }
    let buttons = intent.buttons();
    for c in 0..2 {
        let held = buttons[c] > 0.0;
        input.held[c] = held;
        input.pressed[c] = held && !(was[c] > 0.0);
        was[c] = buttons[c];
    }
    input.reload = intent.reload();
    input.exit = intent.exit_edge();
    intent.clear_edges();
}

/// SeatInput.take (seats.js:231): a trigger press, once.
pub fn take(input: &mut SeatInput, channel: usize) -> bool {
    let pressed = input.pressed[channel];
    input.pressed[channel] = false;
    pressed
}

/// SeatInput.takeReload (seats.js:237).
pub fn take_reload(input: &mut SeatInput) -> bool {
    let reload = input.reload;
    input.reload = false;
    reload
}

/// The intent a lease plays (seats.js:125): its seat's own when a controller
/// drives the seat, else its unit's.
pub enum Played<'a> {
    Seat(&'a mut SeatIntent),
    Unit(&'a mut Intent),
}

/// Where the seat controls live: SeatUnit by its handle.
pub type Controls = super::rows::Rows<(u32,u32),SeatControl>;

/// The key of a seat unit's handle in `Controls`.
pub fn control_key(r: Ref<SeatUnit>) -> (u32, u32) {
    (r.slot, r.gen)
}

/// Latches `lease`'s input from the intent it plays (`lease.input.latch(lease)`).
/// `turret`: the lease holds a turret (a network human's turn and pitch
/// may then reach 16). Returns the seat control whose intent it played
/// (its edges taken), or None for the unit's own.
pub fn latch_lease(leases: &mut Slab<SeatLease>, units: &mut Slab<Unit>, controls: &mut Controls, r: Ref<SeatLease>) -> Option<Ref<SeatUnit>> {
    let lease = leases.get_mut(r).expect("a live lease");
    let unit = lease.unit;
    let aim_limit = if lease.turret.is_some() && unit.and_then(|u| units.get(u)).is_some_and(network_human) { 16.0 } else { 1.0 };
    let seat = lease.controllable;
    let driven = seat.and_then(|c| controls.get_mut(&control_key(c))).filter(|c| c.driven);
    let mut was = lease.was;
    let played = match driven {
        Some(control) => {
            latch(&mut lease.input, &mut was, &mut control.intent, aim_limit);
            seat
        }
        None => {
            let u = units.get_mut(unit.expect("a bot's lease names its unit")).expect("the lease's unit");
            latch(&mut lease.input, &mut was, &mut u.intent, aim_limit);
            None
        }
    };
    lease.was = was;
    played
}
