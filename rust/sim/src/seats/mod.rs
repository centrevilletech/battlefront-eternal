//! The seats (src/seats.js, src/boarding.js, the hulls' seat methods of
//! src/vehicles.js Vehicles, src/runtime/network-seat.js and
//! network-vehicle.js): who sits in which hull seat or turret, how he gets
//! in, out and from seat to seat, and what his seat does with his intent,
//! to the last bit.
//!
//! - `lease`: a seat's lease (`SeatLease`) and the intent it freezes once a
//!   batch (`SeatInput.latch`, `take`, `takeReload`);
//! - `exits`: where someone getting out is set down (the 2004 game's exit
//!   search for bots, `classicSeatExit`, and a person's, `safeExitPoint`);
//! - `bodies`: what taking and leaving a seat does to a body (`seatBody`,
//!   `unseatBody`, `placeOnFoot`, `dropBody`);
//! - `boarding`: the rules every boarder follows (`canBoard`,
//!   `boardingSeat`, `claimSeat`, `mayLeave`, `switchSeat`, `seatLook`);
//! - `hulls`: the hulls' seats (`Vehicles.board`, `vacate`, `promote`,
//!   `move`, `displace`, `destroyedCrew`, `latchCrews`, `requestExit`).
//!
//! A lease is a store handle (`Ref<SeatLease>`, rust/sim/schema/seats.mjs);
//! units and hulls are the store's (`gen::Unit`, `gen::Vehicle`). What the
//! seats ask of the rest of the battle goes through [`SeatWorld`], one
//! method for each call src/seats-world.js makes, in the order the
//! JavaScript makes them: the replays serve them from captures, checking
//! every argument (tests/replay_seats.rs).
//!
//! Transcendental functions come from `crate::rig::math` (V8's, from
//! rust/v8math); Math.min, max and hypot from rust/v8math; three.js's
//! vectors from rust/three, operation by operation.

// Comparisons are written as the JavaScript writes them (`!(a > b)` is not
// `a <= b` where NaN may meet it).
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod boarding;
pub mod bodies;
pub mod exits;
pub mod hulls;
pub mod lease;

use crate::gen::{Body, Post, SeatLease, SeatUnit, Unit};
use crate::store::Ref;
use three::Vector3;

pub use crate::vehicles::{BodyId, Hit};

/// Whose seat: a hull's (its place in the battle's list, Vehicles.vehicles)
/// or a turret's (its place in the battle's turrets, LiveObjects.turrets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Hull(usize),
    Turret(usize),
}

/// Who a blow is credited to (a hull's or turret's `lastAttacker`): nobody,
/// the page's own player, a unit (by id) or a hull (by its place).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Attacker {
    #[default]
    Nobody,
    Player,
    Unit(u32),
    Hull(usize),
}

/// Why someone left a seat (Vehicles.vacate's and TurretSeats.vacate's
/// `reason`, handed to the battle's unboard hook).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Exit,
    Stale,
    Killed,
    Destroyed,
    Displaced,
}

impl Reason {
    pub fn name(self) -> &'static str {
        match self {
            Reason::Exit => "exit",
            Reason::Stale => "stale",
            Reason::Killed => "killed",
            Reason::Destroyed => "destroyed",
            Reason::Displaced => "displaced",
        }
    }
}

/// What the seats ask of the rest of the battle (src/seats-world.js), each
/// call as the JavaScript makes it. Units are named by their registry id.
pub trait SeatWorld {
    /// CollisionWorld.cast: the first hit along a ray within `far`, leaving
    /// out `ignore` (a collision body, by the store's reference).
    fn cast(&mut self, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<Ref<Body>>) -> Option<Hit>;
    /// CollisionWorld.overlapsSphere: whether a sphere overlaps anything
    /// solid but `ignore`.
    fn overlaps_sphere(&mut self, center: &Vector3, radius: f64, ignore: Option<Ref<Body>>) -> bool;
    /// Units.raycast: the distance to the first unit along a ray within
    /// `far`, `exclude` left out.
    fn raycast_units(&mut self, origin: &Vector3, direction: &Vector3, far: f64, exclude: Option<u32>) -> Option<f64>;
    /// Two teams' relation (the registry's, with its alliances).
    fn relation(&mut self, a: f64, b: f64) -> f64;
    /// Soldier.spawn: the body stood up at a point, facing yaw.
    fn stand_up(&mut self, unit: &mut Unit, x: f64, y: f64, z: f64, yaw: f64);
    /// combat.damage(Infinity, Infinity, null, attacker): the unit killed
    /// outright. What it does to the units (the death's hooks run inside)
    /// comes back in `units`.
    fn kill(&mut self, units: &mut crate::store::Slab<Unit>, unit: u32, attacker: Attacker);
    /// The point a rider killed on seat `index` starts to fall from (his
    /// pose's drop point), or None where none shows.
    fn rider_drop(&mut self, holder: Holder, index: usize, unit: u32) -> Option<Vector3>;
    /// Vehicles.syncRider (the hulls' package): the body at its seat, posed
    /// as a rider. What it writes of the unit comes back in `unit`.
    fn sync_rider(&mut self, lease: Ref<SeatLease>, unit: &mut Unit);
    /// The AI controller's state of a unit, or None (undefined).
    fn controller_state(&mut self, unit: Option<u32>) -> Option<f64>;
    /// The seat as the AI drives it, made for a bot's lease (SeatUnit).
    fn controllable(&mut self, lease: Ref<SeatLease>) -> Ref<SeatUnit>;
    /// The battle's hooks: a seat taken, left, changed.
    fn boarded(&mut self, unit: u32, holder: Holder, index: usize);
    fn unboarded(&mut self, unit: Option<u32>, holder: Holder, index: usize, post: Option<Ref<Post>>, reason: Reason);
    fn moved(&mut self, unit: Option<u32>, holder: Holder, index: usize);
}

/// `x || 0` for a number: 0 (+0) for 0, -0 and NaN.
#[inline(always)]
pub fn or_zero(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        0.0
    } else {
        x
    }
}

/// JavaScript's truthiness of a number.
#[inline(always)]
pub fn truthy(x: f64) -> bool {
    !(x == 0.0 || x.is_nan())
}

/// A unit's store handle by its registry id (the kind's key).
#[inline(always)]
pub fn unit_ref(id: u32) -> Ref<Unit> {
    Ref::new(id, 0)
}

pub(crate) fn v3(a: &[f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

pub(crate) fn a3(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}
