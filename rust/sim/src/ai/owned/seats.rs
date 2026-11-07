//! SeatUnit construction identities. F2 requests a fresh control on every
//! acquisition or promotion. Capture its occupant facts at that exact point;
//! later controller binding must not resample hero status or the lease index.
use super::entities::{Entities, Entity};
use crate::{
    gen::{SeatLease, SeatUnit, Store, Turret, Vehicle},
    seats::lease::{control_key, SeatControl},
    store::{Ref, Slab},
    vehicles::f2_seats::Seats,
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Occupant {
    pub id: f64,
    pub hero: bool,
}
impl Occupant {
    pub fn read(store: &Store, lease: Ref<SeatLease>) -> Option<Self> {
        let l = store.seat_lease.get(lease)?;
        Some(Self {
            id: l.unit.map_or(0., |r| r.slot as f64),
            hero: l
                .unit
                .and_then(|r| store.unit.get(r))
                .is_some_and(|u| u.hero),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Source {
    Hull(Ref<Vehicle>, f64),
    Turret(Ref<Turret>, f64),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Birth {
    pub lease: Ref<SeatLease>,
    pub source: Source,
    pub index: usize,
    pub occupant: Occupant,
}
pub struct Controls {
    rows: Slab<Birth>,
    capacity: usize,
}
impl Controls {
    pub fn new(capacity: usize) -> Self {
        Self {
            rows: Slab::with_capacity(capacity),
            capacity,
        }
    }
    /// The lease and holder have been committed before F2 calls controllable.
    /// F2 installs the returned reference on the lease after this call returns.
    pub fn construct(
        &mut self,
        store: &Store,
        seats: &mut Seats,
        entities: &mut Entities,
        lease: Ref<SeatLease>,
        turret_life: Option<f64>,
    ) -> Option<Ref<SeatUnit>> {
        if self.rows.len() >= self.capacity {
            return None;
        }
        let l = store.seat_lease.get(lease)?;
        if !(l.index >= 0. && l.index.is_finite() && l.index.fract() == 0.) {
            return None;
        }
        // A construction callback runs before Unit.seat is committed. Validate
        // the occupant's storage and gameplay lives without requiring that link.
        if l.released { return None; }
        if let Some(unit) = l.unit {
            if store.unit.get(unit)?.generation != l.generation { return None; }
        }
        let source = if let Some(hull) = l.vehicle {
            let vehicle = store.vehicle.get(hull)?;
            if l.turret.is_some() || vehicle.seats.get(l.index as usize)?.lease != Some(lease) { return None; }
            Source::Hull(hull, vehicle.target_generation)
        } else {
            let turret = l.turret?;
            if l.index != 0. || !seats.crews.turrets.iter().any(|t| t.turret == turret && t.lease == Some(lease)) { return None; }
            Source::Turret(turret, turret_life?)
        };
        let birth = Birth {
            lease,
            source,
            index: l.index as usize,
            occupant: Occupant::read(store, lease)?,
        };
        let r = self.rows.insert(birth);
        let control = Ref::new(r.slot, r.gen);
        entities.register(Entity::Control(control));
        entities.register(Entity::Lease(lease));
        seats.control(
            control,
            SeatControl::default(),
            matches!(source, Source::Turret(..)),
        );
        Some(control)
    }
    pub fn slots(&self)->u32 {self.rows.slots()}
    pub fn at(&self,slot:u32)->Option<(Ref<SeatUnit>,Birth)> {self.rows.at(slot).map(|(reference,birth)|(Ref::new(reference.slot,reference.gen),*birth))}
    pub fn read(&self, control: Ref<SeatUnit>) -> Option<Birth> {
        self.rows.get(Ref::new(control.slot, control.gen)).copied()
    }
    /// Call after the old agent exits. Saved targets can keep the old token,
    /// but a reused pool slot cannot resolve that token to its replacement.
    pub fn retire(
        &mut self,
        seats: &mut Seats,
        entities: &mut Entities,
        control: Ref<SeatUnit>,
    ) -> bool {
        if self
            .rows
            .remove(Ref::new(control.slot, control.gen))
            .is_none()
        {
            return false;
        }
        let key = control_key(control);
        seats.hulls.controls.remove(&key);
        seats.crews.controls.remove(&key);
        if let Some(obj) = entities.find(Entity::Control(control)) {
            entities.retire(obj);
        }
        true
    }
}
