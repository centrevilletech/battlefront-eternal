//! Seat reservations, as the 2004 game keeps them
//! (src/ai/classic/vehicles/reservations.js).
//!
//! A reservation is { candidate, time, owner, type }: the vehicle (a weak
//! reference), when it was made or last refreshed, the reserving unit (a
//! weak reference) and an integer type (1 for seats, 2 a flyer's target).
//! It holds while now < time + 3 and both references still resolve
//! (alive, and of the generation it was made with); expired or broken ones
//! are pruned as the table is read. Reserving again refreshes the matching
//! record instead of adding one.

use super::world::{Obj, Ref, World};

/// reservations.js:125.
pub const RESERVATION_LIFE: f64 = 3.0;
/// reservations.js:126.
pub const SEAT: f64 = 1.0;

/// One record (reservations.js:179).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reservation {
    pub candidate: Ref,
    pub owner: Ref,
    pub time: f64,
    pub kind: f64,
}

/// Reservations (reservations.js:133): the records in the order made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reservations {
    pub records: Vec<Reservation>,
}

/// resolve(ref) (reservations.js:128): the unit, when it lives and is of
/// the reference's generation.
fn resolve<W: World + ?Sized>(w: &W, r: &Ref) -> Option<Obj> {
    let u = w.unit(r.obj);
    if u.alive && u.generation == r.generation {
        Some(r.obj)
    } else {
        None
    }
}

impl Reservations {
    /// live() (reservations.js:144): prune, keeping the live records.
    pub fn live<W: World + ?Sized>(&mut self, w: &W) -> &[Reservation] {
        let now = w.now();
        self.records.retain(|r| now < r.time + RESERVATION_LIFE && resolve(w, &r.candidate).is_some() && resolve(w, &r.owner).is_some());
        &self.records
    }

    /// find(candidate, owner, type) (reservations.js:150): the index of the record.
    pub fn find<W: World + ?Sized>(&mut self, w: &W, candidate: Obj, owner: Obj, kind: f64) -> Option<usize> {
        self.live(w);
        self.records
            .iter()
            .position(|r| r.kind == kind && resolve(w, &r.candidate) == Some(candidate) && resolve(w, &r.owner) == Some(owner))
    }

    /// has(candidate, owner, type) (reservations.js:157).
    pub fn has<W: World + ?Sized>(&mut self, w: &W, candidate: Obj, owner: Obj, kind: f64) -> bool {
        self.find(w, candidate, owner, kind).is_some()
    }

    /// count(candidate, team, type) (reservations.js:162): live reservations
    /// of `kind` on `candidate` by units of `team`.
    pub fn count<W: World + ?Sized>(&mut self, w: &W, candidate: Obj, team: f64, kind: f64) -> f64 {
        self.live(w);
        let mut n = 0.0;
        for r in &self.records {
            if r.kind != kind || resolve(w, &r.candidate) != Some(candidate) {
                continue;
            }
            if resolve(w, &r.owner).map(|o| w.unit(o).team) == Some(team) {
                n += 1.0;
            }
        }
        n
    }

    /// reserve(candidate, owner, type) (reservations.js:172): make, or
    /// refresh the matching record.
    pub fn reserve<W: World + ?Sized>(&mut self, w: &W, candidate: Obj, owner: Obj, kind: f64) {
        if let Some(i) = self.find(w, candidate, owner, kind) {
            self.records[i].time = w.now();
            return;
        }
        let record = Reservation {
            candidate: Ref { obj: candidate, generation: w.unit(candidate).generation },
            owner: Ref { obj: owner, generation: w.unit(owner).generation },
            time: w.now(),
            kind,
        };
        self.records.push(record);
    }

    /// release(candidate, owner, type) (reservations.js:189): by identity
    /// alone, whatever the generations.
    pub fn release(&mut self, candidate: Obj, owner: Obj, kind: f64) {
        self.records.retain(|r| !(r.kind == kind && r.candidate.obj == candidate && r.owner.obj == owner));
    }

    /// clear() (reservations.js:193).
    pub fn clear(&mut self) {
        self.records.clear();
    }
}

/// freeCapacity(reservations, candidate, team, type, seats, humans)
/// (reservations.js:200).
pub fn free_capacity<W: World + ?Sized>(reservations: &mut Reservations, w: &W, candidate: Obj, team: f64, kind: f64, seats: f64, humans: f64) -> f64 {
    let reserved = reservations.count(w, candidate, team, kind);
    let players = if kind == SEAT { humans } else { 0.0 };
    if seats < players + reserved {
        return 0.0;
    }
    seats - players - reserved
}
