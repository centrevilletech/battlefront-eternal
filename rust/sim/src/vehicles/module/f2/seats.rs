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
    // Vehicle construction records fixed seat facts beside the canonical slab.
    // Publication capacity grows here, not during normal crew iteration.
    pub fn add_hull(&mut self, st: &Store, side: HullSide) {
        let seats = st.vehicle.get(side.vehicle).expect("registered hull").seats.len();
        assert!(!self.hulls.hulls.iter().any(|h| h.vehicle == side.vehicle));
        self.hulls.hulls.push(side);
        let mut capacity = seats;
        for side in &self.hulls.hulls[..self.hulls.hulls.len() - 1] {
            capacity += st.vehicle.get(side.vehicle).expect("registered hull").seats.len();
        }
        self.reserve_hulls(self.hulls.hulls.len(), capacity);
    }

    // Vehicles.variant can prepare class facts before the pad chooses a team.
    // Reserve the eventual active topology without inserting those hulls into
    // the crew iteration order. Activation later moves only its retained side.
    pub fn reserve_hulls(&mut self, count: usize, seats: usize) {
        self.hulls.hulls.reserve(count.saturating_sub(self.hulls.hulls.len()));
        self.hulls.pending_exits.reserve(seats.saturating_sub(self.hulls.pending_exits.len()));
        self.hulls.pending_deaths.reserve(seats.saturating_sub(self.hulls.pending_deaths.len()));
        self.hulls.latched.reserve(seats.saturating_sub(self.hulls.latched.len()));
        self.changed.hulls.reserve(count.saturating_sub(self.changed.hulls.len()));
        let occupants = seats + self.crews.turrets.len();
        self.changed.leases.reserve(occupants.saturating_sub(self.changed.leases.len()));
        self.changed.units.reserve(occupants.saturating_sub(self.changed.units.len()));
        self.retired.reserve(occupants.saturating_sub(self.retired.len()));
        self.hulls.controls.reserve(occupants.saturating_sub(self.hulls.controls.len()));
        self.crews.controls.reserve(occupants.saturating_sub(self.crews.controls.len()));
    }

    /// Turret construction retains the rig and aim. Rebuilding a building
    /// changes its lifecycle facts, not the identity of its turret sidecar.
    pub fn add_turret(&mut self, side: TurretSide) {
        assert!(!self.crews.turrets.iter().any(|t| t.turret == side.turret));
        self.crews.turrets.push(side);
        self.crews.updating.reserve(self.crews.turrets.len());
        self.crews.shots.reserve(32);
        self.crews.latched.reserve(self.crews.turrets.len());
        self.changed.turrets.reserve(self.crews.turrets.len());
        let occupants = self.crews.turrets.len() + self.hulls.pending_exits.capacity();
        self.changed.leases.reserve(occupants);
        self.changed.units.reserve(occupants);
        self.retired.reserve(occupants);
        self.hulls.controls.reserve(occupants.saturating_sub(self.hulls.controls.len()));
        self.crews.controls.reserve(occupants.saturating_sub(self.crews.controls.len()));
    }

    // A pooled Vehicle has two identities. Both the slab generation and the
    // registry target's gameplay life must match before a command can write it.
    fn hull(&self, st: &Store, target: HullLife) -> Option<usize> {
        let v = st.vehicle.get(target.vehicle)?;
        if v.target_generation != target.life { return None; }
        self.hulls.hulls.iter().position(|h| h.vehicle == target.vehicle)
    }

    // Unit bodies are pooled too. Reject a command from a previous soldier
    // life even when the opaque unit handle still names the same JS object.
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

    // Vehicle.seatLeases is derived, not a second ownership source. Refresh it
    // immediately after each seat mutation before a later intra-phase read.
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
    // Vehicles.latchCrews releases stale occupancies and processes requested
    // exits before physics. Reuse the solvers and their ordered exit scratch.
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
    // Vehicles.board can displace or promote another occupant. Let the solver
    // finish each mutation and refresh views before reporting the touched rows.
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
    // Network input names an occupancy, not merely a physical seat. An old
    // packet must not steer whoever later takes that seat after lease reuse.
    pub fn network_input(&mut self, st: &mut Store, r: Ref<SeatLease>, keys: &SeatKeys, command: &SeatCommand) -> Option<SeatKeysOut> {
        let v = self.lease(st, r)?;
        let lease = st.seat_lease.get(r)?;
        let index = lease.index as usize;
        let result = network::network_vehicle_input(st.vehicle.get_mut(v)?, index, keys, command);
        self.touch_hull(st, v);
        result
    }

    /// Vehicles.destroyedCrew releases all riders before any death hook runs.
    // Vehicles.destroyedCrew preserves the solver's kill-entry slot, including
    // promotion and rider drop reads performed by synchronous death hooks.
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
    // TurretSeats.update keeps per-crew aim, pose and fire interleaved. A
    // whole-phase fire batch would expose different bodies to later shots.
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
    // Retire released lease rows only after their final state can be read.
    // Their generation-qualified emitters remain valid historical identities.
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

/// Scalar commands release the instance before any synchronous foreign hook.
pub fn command(args: crate::gen::F2SeatCommand) -> crate::gen::F2SeatResult {
    #[cfg(target_arch = "wasm32")]
    { crate::vehicles::module::f2::live_seats::command(args) }
    #[cfg(not(target_arch = "wasm32"))]
    { let _ = args; unreachable!("F2 seat scalar commands require a WASM instance") }
}
