//! Vehicles' seat phase and TurretSeats on canonical store rows. The ordered
//! hull and turret sidecars retain construction facts; leases belong to the
//! store slab. Commands qualify both a slab handle and its gameplay life.
use crate::gen::{Post, SeatLease, SeatUnit, Store, Turret, Unit, Vehicle};
use crate::seats::hulls::{lease_views, HullSide, Hulls, Vacate};
use crate::seats::lease::{control_key, SeatControl};
use crate::seats::network::{self, SeatCommand, SeatKeys, SeatKeysOut};
use crate::seats::turrets::{CrewWorld, Crews, Let, TurretSide};
use crate::seats::{Reason, SeatWorld};
use crate::store::Ref;

/// Vehicle.target.generation is separate from the slab generation. A pooled
/// hull can respawn without making an old command address its next life.
#[derive(Clone, Copy, Debug)]
pub struct HullLife {
    pub vehicle: Ref<Vehicle>,
    pub life: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct UnitLife {
    pub unit: Ref<Unit>,
    pub life: f64,
}

/// The rows to publish after a command. Hull readers need the changed hull,
/// its occupants and their leases, including a rider who just left it.
#[derive(Default)]
pub struct Changed {
    pub hulls: Vec<Ref<Vehicle>>,
    pub turrets: Vec<Ref<Turret>>,
    pub leases: Vec<Ref<SeatLease>>,
    pub units: Vec<Ref<Unit>>,
}

impl Changed {
    pub fn clear(&mut self) {
        self.hulls.clear(); self.turrets.clear(); self.leases.clear(); self.units.clear();
    }
    fn lease(&mut self, st: &Store, r: Ref<SeatLease>) {
        if !self.leases.contains(&r) { self.leases.push(r); }
        if let Some(unit) = st.seat_lease.get(r).and_then(|l| l.unit) {
            if !self.units.contains(&unit) { self.units.push(unit); }
        }
    }
}

/// Vehicles.board/requestExit/unboard/move and network seat commands share
/// this owner with latchCrews. The store never receives JS lease snapshots.
#[derive(Default)]
pub struct Seats {
    pub hulls: Hulls,
    pub crews: Crews,
    pub changed: Changed,
    retired: Vec<Ref<SeatLease>>,
}

impl Seats {
    /// Vehicle construction registers fixed seat facts once, before a tick.
    pub fn add_hull(&mut self, st: &Store, side: HullSide) {
        let seats = st.vehicle.get(side.vehicle).expect("registered hull").seats.len();
        assert!(!self.hulls.hulls.iter().any(|h| h.vehicle == side.vehicle));
        self.hulls.hulls.push(side);
        let mut capacity = seats;
        for side in &self.hulls.hulls[..self.hulls.hulls.len() - 1] {
            capacity += st.vehicle.get(side.vehicle).expect("registered hull").seats.len();
        }
        self.hulls.pending_exits.reserve(capacity);
        self.hulls.pending_deaths.reserve(capacity);
        self.hulls.latched.reserve(capacity);
        self.changed.hulls.reserve(self.hulls.hulls.len());
        let occupants = capacity + self.crews.turrets.len();
        self.changed.leases.reserve(occupants);
        self.changed.units.reserve(occupants);
        self.retired.reserve(occupants);
    }

    /// Turret construction retains the rig and aim. Rebuilding a building
    /// changes its lifecycle facts, not the identity of its turret sidecar.
    pub fn add_turret(&mut self, side: TurretSide) {
        assert!(!self.crews.turrets.iter().any(|t| t.turret == side.turret));
        self.crews.turrets.push(side);
        self.crews.updating.reserve(self.crews.turrets.len());
        self.crews.latched.reserve(self.crews.turrets.len());
        self.changed.turrets.reserve(1);
        self.changed.leases.reserve(1);
        self.changed.units.reserve(1);
        self.retired.reserve(1);
    }

    fn hull(&self, st: &Store, target: HullLife) -> Option<usize> {
        let v = st.vehicle.get(target.vehicle)?;
        if v.target_generation != target.life { return None; }
        self.hulls.hulls.iter().position(|h| h.vehicle == target.vehicle)
    }

    fn unit(&self, st: &Store, target: UnitLife) -> Option<u32> {
        let u = st.unit.get(target.unit)?;
        (u.generation == target.life).then_some(target.unit.slot)
    }

    /// A lease command must still name the same hull life. A released lease
    /// may be read by its old emitter but cannot move another occupant.
    fn lease(&self, st: &Store, r: Ref<SeatLease>) -> Option<Ref<Vehicle>> {
        let lease = st.seat_lease.get(r)?;
        if lease.released { return None; }
        if let Some(unit) = lease.unit {
            let unit = st.unit.get(unit)?;
            if unit.generation != lease.generation || unit.seat != Some(r) { return None; }
        }
        let v = lease.vehicle?;
        let hull = st.vehicle.get(v)?;
        let seat = hull.seats.get(lease.index as usize)?;
        (seat.lease == Some(r)).then_some(v)
    }

    fn touch_hull(&mut self, st: &Store, v: Ref<Vehicle>) {
        if !self.changed.hulls.contains(&v) { self.changed.hulls.push(v); }
        if let Some(hull) = st.vehicle.get(v) {
            for seat in &hull.seats {
                if let Some(r) = seat.lease { self.changed.lease(st, r); }
            }
        }
    }

    fn touch_turret(&mut self, st: &Store, index: usize) {
        let t = &self.crews.turrets[index];
        if !self.changed.turrets.contains(&t.turret) { self.changed.turrets.push(t.turret); }
        if let Some(r) = t.lease { self.changed.lease(st, r); }
    }

    /// AI writes are imported once before Vehicles.latchCrews. Existing
    /// controls keep their map entries so the ordinary tick does not allocate.
    pub fn control(&mut self, seat: Ref<SeatUnit>, control: SeatControl, turret: bool) {
        let controls = if turret { &mut self.crews.controls } else { &mut self.hulls.controls };
        controls.insert(control_key(seat), control);
    }

    /// Vehicles.latchCrews: freeze AI inputs, release stale leases, apply exit
    /// requests and promotions, then expose fresh derived controls to physics.
    pub fn latch<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, time: f64) {
        self.hulls.time = time;
        self.hulls.latched.clear();
        self.hulls.made.clear();
        for i in 0..self.hulls.hulls.len() {
            let v = self.hulls.hulls[i].vehicle;
            if st.vehicle.get(v).is_some_and(|v| v.seats.iter().any(|s| s.lease.is_some())) {
                self.touch_hull(st, v);
            }
        }
        self.hulls.latch_crews(st, world);
    }

    /// Vehicles.board, including a network human's takeover of a bot's seat.
    pub fn board<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, unit: UnitLife, hull: HullLife, index: i32) -> bool {
        let Some(id) = self.unit(st, unit) else { return false };
        let Some(h) = self.hull(st, hull) else { return false };
        self.touch_hull(st, hull.vehicle);
        let boarded = self.hulls.board(st, world, id, h, index);
        lease_views(st, hull.vehicle);
        self.touch_hull(st, hull.vehicle);
        boarded
    }

    /// Vehicles.requestExit records a request until latchCrews can honor it.
    pub fn request_exit(&mut self, st: &mut Store, unit: UnitLife, post: Option<Ref<Post>>, promote: Option<bool>, placement: bool) -> bool {
        let Some(id) = self.unit(st, unit) else { return false };
        let Some(r) = self.hulls.seat_of(st, id) else { return false };
        self.changed.lease(st, r);
        self.hulls.request_exit(st, id, post, promote, placement)
    }

    /// Vehicles.vacate handles exits, disconnects, death and displacement.
    pub fn vacate<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, r: Ref<SeatLease>, how: Vacate) -> bool {
        if st.seat_lease.get(r).is_some_and(|l| l.released) { return true; }
        let Some(v) = self.lease(st, r) else { return false };
        self.changed.lease(st, r);
        self.touch_hull(st, v);
        let left = self.hulls.vacate(st, world, r, how);
        lease_views(st, v);
        self.touch_hull(st, v);
        left
    }

    /// Vehicles.unboard, also used when a network connection is removed.
    pub fn unboard<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, unit: UnitLife, how: Vacate) -> bool {
        let Some(id) = self.unit(st, unit) else { return false };
        let Some(r) = self.hulls.seat_of(st, id) else { return false };
        self.vacate(st, world, r, how)
    }

    /// Vehicles.move transfers a live lease without allocating a new lease.
    pub fn move_seat<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, r: Ref<SeatLease>, index: usize) -> bool {
        let Some(v) = self.lease(st, r) else { return false };
        if !st.vehicle.get(v).is_some_and(|v| index < v.seats.len()) { return false; }
        self.touch_hull(st, v);
        self.hulls.move_seat(st, world, r, index);
        lease_views(st, v);
        self.touch_hull(st, v);
        true
    }

    /// networkSeat.cycle swaps a human with a bot, retaining both identities.
    pub fn cycle<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, unit: UnitLife, direction: i32) -> bool {
        let Some(id) = self.unit(st, unit) else { return false };
        let Some(r) = self.hulls.seat_of(st, id) else { return false };
        let Some(v) = self.lease(st, r) else { return false };
        self.touch_hull(st, v);
        let changed = network::cycle_network_seat(&mut self.hulls, st, world, id, direction);
        lease_views(st, v);
        self.touch_hull(st, v);
        changed
    }

    /// networkVehicleInput applies a packet's mouse samples in order. Flight
    /// bindings remain foreign input; drive keys are returned to their caller.
    pub fn network_input(&mut self, st: &mut Store, r: Ref<SeatLease>, keys: &SeatKeys, command: &SeatCommand) -> Option<SeatKeysOut> {
        let v = self.lease(st, r)?;
        let lease = st.seat_lease.get(r)?;
        let index = lease.index as usize;
        let result = network::network_vehicle_input(st.vehicle.get_mut(v)?, index, keys, command);
        self.touch_hull(st, v);
        result
    }

    /// Vehicles.destroyedCrew releases all riders before any death hook runs.
    pub fn destroyed<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, hull: HullLife) -> bool {
        let Some(h) = self.hull(st, hull) else { return false };
        self.touch_hull(st, hull.vehicle);
        self.hulls.destroyed_crew(st, world, h);
        lease_views(st, hull.vehicle);
        true
    }

    /// TurretSeats.man uses the same lease slab as hull boarding.
    pub fn man<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, unit: UnitLife, turret: Ref<Turret>) -> bool {
        let Some(id) = self.unit(st, unit) else { return false };
        let Some(t) = self.crews.turrets.iter().position(|t| t.turret == turret) else { return false };
        self.touch_turret(st, t);
        let manned = self.crews.man(st, world, id, t);
        self.touch_turret(st, t);
        manned
    }

    /// Building.destroyed/rebuild retain JS health until buildings move. The
    /// native turret keeps its angles and rig across that building lifecycle.
    pub fn turret_life<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, turret: Ref<Turret>, alive: bool) -> bool {
        let Some(t) = self.crews.turrets.iter().position(|t| t.turret == turret) else { return false };
        self.touch_turret(st, t);
        self.crews.turrets[t].alive = alive;
        if !alive { self.crews.destroyed(st, world, t); }
        true
    }

    /// TurretSeats.vacate, including killed and disconnected occupants.
    pub fn leave_turret<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, r: Ref<SeatLease>, how: Let) -> bool {
        let Some(lease) = st.seat_lease.get(r) else { return false };
        if lease.released { return false; }
        if let Some(unit) = lease.unit {
            let Some(unit) = st.unit.get(unit) else { return false };
            if unit.generation != lease.generation || unit.seat != Some(r) { return false; }
        }
        let Some(t) = self.crews.turrets.iter().position(|t| Some(t.turret) == lease.turret && t.lease == Some(r)) else { return false };
        self.changed.lease(st, r);
        self.touch_turret(st, t);
        self.crews.vacate(st, world, r, how);
        true
    }

    /// TurretSeats.update follows hull fire, tow and rider writeback. A server
    /// world's dormant local Gunner/Driver is deliberately absent here.
    pub fn turret_tick<W: CrewWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, dt: f64) {
        self.crews.latched.clear();
        self.crews.made.clear();
        for t in 0..self.crews.turrets.len() {
            if self.crews.turrets[t].lease.is_some() { self.touch_turret(st, t); }
        }
        self.crews.update(st, world, dt);
    }

    /// After publication, retire released leases and their AI controls. Old
    /// generation-qualified commands then fail even if the slab slot is reused.
    pub fn published(&mut self, st: &mut Store) {
        self.retired.clear();
        for &r in &st.seats.leases {
            if st.seat_lease.get(r).is_some_and(|l| l.released) { self.retired.push(r); }
        }
        for &r in &self.retired {
            if let Some(c) = st.seat_lease.get(r).and_then(|l| l.controllable) {
                self.hulls.controls.remove(&control_key(c));
                self.crews.controls.remove(&control_key(c));
            }
            st.seat_lease.retire(r);
        }
        st.seats.leases.retain(|r| st.seat_lease.contains(*r));
        self.changed.clear();
    }

    /// leaveKilled chooses the lease's owner; neither branch searches by slot.
    pub fn killed<W: SeatWorld + ?Sized>(&mut self, st: &mut Store, world: &mut W, r: Ref<SeatLease>) -> bool {
        if st.seat_lease.get(r).is_some_and(|l| l.vehicle.is_some()) {
            self.vacate(st, world, r, Vacate { placement: false, reason: Reason::Killed, ..Vacate::default() })
        } else {
            self.leave_turret(st, world, r, Let { placement: false, reason: Reason::Killed, ..Let::default() })
        }
    }
}
