//! Persistent unit ownership. State lives in the generated store, not in this
//! sidecar. The sidecar retains only factory ordering, free lists and changed
//! references. Construction imports a body once; commands validate both the
//! slab generation and the gameplay life before touching it.
use crate::gen::{F3DroidekaMotion, F3ShieldView, F3SoldierMotion, F3UnitKey, F3UnitMirror, Member, Store, Unit, UnitBody};
use crate::store::Ref;
use super::{factory::Pools, world::UnitsWorld};

#[derive(Default)]
pub struct Ownership {
    built: Vec<Ref<Unit>>,
    revision: u64,
    // The world's viewpoint shares registry geometry but is not constructed,
    // ticked or returned by UnitFactory. Its life still uses the same slab.
    passive: Vec<Ref<Unit>>,
    references: Vec<Option<Ref<Unit>>>,
    free: Pools,
    // A narrow membership mirror avoids rescanning class stacks at every
    // publication and lets an already-returned removal callback be a no-op.
    pooled: Vec<bool>,
    changed: Vec<Ref<Unit>>,
    marked: Vec<Option<u32>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stale { Slot, Life, State }

impl F3UnitKey {
    pub fn reference(&self) -> Ref<Unit> { Ref::new(self.slot, self.generation) }
    pub fn of(reference: Ref<Unit>, unit: &Unit) -> Self {
        Self { slot: reference.slot, generation: reference.gen, life: unit.generation }
    }
    pub fn validate<'a>(&self, store: &'a Store) -> Result<&'a Unit, Stale> {
        let unit = store.unit.get(self.reference()).ok_or(Stale::Slot)?;
        if unit.generation != self.life { return Err(Stale::Life); }
        Ok(unit)
    }
    pub fn validate_mut<'a>(&self, store: &'a mut Store) -> Result<&'a mut Unit, Stale> {
        let unit = store.unit.get_mut(self.reference()).ok_or(Stale::Slot)?;
        if unit.generation != self.life { return Err(Stale::Life); }
        Ok(unit)
    }
}

impl Ownership {
    /// Cold future capacity includes passive rows and all mixed-registry ids.
    /// Birth can extend these lengths without changing their backing storage.
    pub fn reserve(&mut self, capacity: usize) {
        self.built.reserve(capacity.saturating_sub(self.built.len()));
        self.passive.reserve(capacity.saturating_sub(self.passive.len()));
        self.references.reserve(capacity.saturating_sub(self.references.len()));
        self.pooled.reserve(capacity.saturating_sub(self.pooled.len()));
        self.changed.reserve(capacity.saturating_sub(self.changed.len()));
        self.marked.reserve(capacity.saturating_sub(self.marked.len()));
        for (_,free) in &mut self.free.free {
            free.reserve(capacity.saturating_sub(free.len()));
        }
    }
    /// UnitFactory classes reserve their free stacks before the first birth.
    pub fn reserve_class(&mut self, class: crate::store::StrId, capacity: usize) {
        if !self.free.free.iter().any(|(name, _)| *name == class) {
            self.free.free.push((class, Vec::with_capacity(capacity)));
        }
        let free = &mut self.free.free.iter_mut().find(|(name, _)| *name == class).unwrap().1;
        free.reserve(capacity.saturating_sub(free.len()));
    }

    pub fn pooled(&self, reference: Ref<Unit>) -> bool {
        self.reference(reference.slot) == Some(reference) && self.pooled[reference.slot as usize]
    }
    /// A capacity check must not pop the LIFO pool before removal succeeds.
    pub fn available(&self, class: crate::store::StrId) -> bool {
        self.free.free.iter().any(|(name, free)| *name == class && !free.is_empty())
    }
    /// A prepared row has completed adoption but is not a live factory body.
    /// Withdrawing it neither returns a corpse nor changes registry order.
    pub(crate) fn withdraw(&mut self, reference: Ref<Unit>) {
        self.revision = self.revision.wrapping_add(1);
        self.built.retain(|&r| r != reference);
        self.passive.retain(|&r| r != reference);
        self.changed.retain(|&r| r != reference);
        for (_, free) in &mut self.free.free { free.retain(|&id| id != reference.slot); }
        self.references[reference.slot as usize] = None;
        self.marked[reference.slot as usize] = None;
        self.pooled[reference.slot as usize] = false;
    }
    pub fn built(&self) -> &[Ref<Unit>] { &self.built }
    /// Structural admission only: gameplay life and ordinary movement retain
    /// their existing body slot, so dependent phases need no membership scan.
    pub fn revision(&self) -> u64 { self.revision }
    /// Seat solvers lend the Store shell across hooks. Resolve a qualified
    /// body identity even while its canonical row is in that local shell.
    pub fn reference(&self, slot: u32) -> Option<Ref<Unit>> {
        self.references.get(slot as usize).copied().flatten()
    }
    pub fn changed(&self) -> &[Ref<Unit>] { &self.changed }
    /// A movement-only barrier may retain combat only if no earlier writer
    /// has touched this life. Damage and lifecycle changes still publish all.
    pub fn is_marked(&self, reference: Ref<Unit>) -> bool {
        self.marked[reference.slot as usize] == Some(reference.gen)
    }
    pub fn clear_changed(&mut self) {
        for reference in &self.changed { self.marked[reference.slot as usize]=None; }
        self.changed.clear();
    }
    pub fn unmark(&mut self, reference: Ref<Unit>) {
        self.marked[reference.slot as usize]=None;
        // A normal tick most recently marked its own row. Removing that tail
        // is constant time; rare nested damage preserves other victims' order.
        if self.changed.last() == Some(&reference) { self.changed.pop(); }
        else { self.changed.retain(|&r| r != reference); }
    }

    /// Adopt a construction row already loaded into the generated slab. The
    /// caller has registered mixed targets in their original construction order.
    /// Adoption does not write any existing body, cache or combat value.
    pub fn adopt(&mut self, store: &Store, reference: Ref<Unit>) -> Result<F3UnitKey, Stale> {
        self.adopt_as(store, reference, true)
    }

    /// The original viewpoint joins Units without joining UnitFactory.built.
    /// Its geometry and explicit commands remain canonical in the same slab.
    pub fn adopt_passive(&mut self, store: &Store, reference: Ref<Unit>) -> Result<F3UnitKey, Stale> {
        self.adopt_as(store, reference, false)
    }
    fn adopt_as(&mut self, store: &Store, reference: Ref<Unit>, factory: bool) -> Result<F3UnitKey, Stale> {
        let unit = store.unit.get(reference).ok_or(Stale::Slot)?;
        self.revision = self.revision.wrapping_add(1);
        self.references.resize(self.references.len().max(reference.slot as usize + 1), None);
        self.references[reference.slot as usize] = Some(reference);
        self.pooled.resize(self.references.len(),false);
        self.marked.resize(self.references.len(),None);
        if !self.built.contains(&reference) && !self.passive.contains(&reference) {
            if factory {
                self.built.push(reference);
                self.free.release(unit.class_name, reference.slot);
                let free = &mut self.free.free.iter_mut().find(|(class, _)| *class == unit.class_name).unwrap().1;
                free.retain(|&id| id != reference.slot);
                free.reserve(self.built.len());
            } else {
                self.passive.push(reference);
            }
            // Damage may touch a passive body while every factory body also
            // changed. Reserve for both before that first native phase runs.
            self.changed.reserve((self.built.len() + self.passive.len()).saturating_sub(self.changed.len()));
        }
        self.mark(reference);
        Ok(F3UnitKey::of(reference, unit))
    }

    /// A new pooled object, with a registry id shared by all member kinds.
    /// Free corpses are acquired rather than rebuilt; a new object starts with
    /// its own slab generation, independently of its zero gameplay life.
    pub fn build(&mut self, store: &mut Store, unit: Unit) -> F3UnitKey {
        let id = store.units.add(|id| Member::Unit(Ref::new(id, 1)));
        let reference = store.unit.insert_at(id, 1, unit);
        self.adopt(store, reference).expect("new construction row")
    }

    pub fn mark(&mut self, reference: Ref<Unit>) {
        // First-touch order is observable during nested death explosions.
        // The generation stamp rejects duplicates in constant time without
        // scanning every earlier victim of that same blast.
        let stamp=&mut self.marked[reference.slot as usize];
        if *stamp==Some(reference.gen) {return;}
        *stamp=Some(reference.gen);
        self.changed.push(reference);
    }

    /// One publication per changed object, in first-change order. Building a
    /// mirror never evaluates hit shapes, which must remain lazy across death.
    pub fn publish(&mut self, store: &Store, out: &mut Vec<F3UnitMirror>) {
        out.clear();
        for reference in self.changed.drain(..) {
            self.marked[reference.slot as usize]=None;
            if let Some(unit) = store.unit.get(reference) { out.push(mirror(reference, unit)); }
        }
    }

    pub fn intent(&mut self, store: &mut Store, command: crate::gen::F3IntentCommand) -> Result<(), Stale> {
        command.key.validate_mut(store)?.intent = command.intent;
        Ok(())
    }

    pub fn team(&mut self, store: &mut Store, key: &F3UnitKey, team: f64) -> Result<(), Stale> {
        // HumanSpawn sets Unit.team before resetting combat. CombatWorld's
        // viewer uses that same team even when combat is temporarily borrowed.
        let unit = key.validate_mut(store)?;
        unit.team = team;
        unit.combat.team = team;
        self.mark(key.reference());
        Ok(())
    }

    pub fn look(&mut self, store: &mut Store, command: &crate::gen::F3LookCommand) -> Result<(), Stale> {
        let unit = command.key.validate_mut(store)?;
        match &mut unit.soldier {
            UnitBody::Soldier(s) => {
                if command.raw { s.look_raw(command.dx, command.dy, command.sensitivity, command.invert); }
                else { s.look(command.dx, command.dy, command.sensitivity, command.invert); }
            }
            UnitBody::Droideka(d) => d.look(command.dx, command.dy, command.sensitivity, command.invert),
        }
        // Mouse look does not bump tickCount in the original game.
        self.mark(command.key.reference());
        Ok(())
    }

    pub fn spawn(&mut self, store: &mut Store, command: &crate::gen::F3SpawnCommand, world: &mut dyn UnitsWorld) -> Result<F3UnitKey, Stale> {
        let reference = command.key.reference();
        let unit = command.key.validate_mut(store)?;
        let [x, y, z] = command.position;
        unit.spawn(reference.slot, x, y, z, command.yaw, world);
        self.mark(reference);
        Ok(F3UnitKey::of(reference, unit))
    }

    /// Corpse removal returns the same pooled object without retiring its
    /// slab handle or removing it from the mixed registry.
    pub fn remove(&mut self, store: &mut Store, key: &F3UnitKey, world: &mut dyn UnitsWorld) -> Result<(), Stale> {
        let unit = key.validate_mut(store)?;
        unit.remove(key.slot, world);
        self.removed(store, key)
    }

    /// The tick path already ran Unit::remove and its ordered callbacks.
    pub fn removed(&mut self, store: &Store, key: &F3UnitKey) -> Result<(), Stale> {
        let unit = key.validate(store)?;
        if !unit.removed {return Err(Stale::State); }
        if unit.network_human != Some(true) && !self.passive.contains(&key.reference()) {
            self.free.release(unit.class_name, key.slot);
            self.pooled[key.slot as usize] = true;
        }
        self.mark(key.reference());
        Ok(())
    }

    pub fn acquire(&mut self, store: &Store, class: crate::store::StrId) -> Option<F3UnitKey> {
        let slot = self.free.acquire(class)?;
        self.pooled[slot as usize] = false;
        let (reference, unit) = store.unit.at(slot).expect("pooled body has a live slab slot");
        Some(F3UnitKey::of(reference, unit))
    }

    /// Retirement is distinct from returning a corpse to its class pool.
    /// A stale command never creates a row or resolves through a new life.
    pub fn retire(&mut self, store: &mut Store, key: &F3UnitKey) -> Result<(), Stale> {
        key.validate(store)?;
        let reference = key.reference();
        for (_, free) in &mut self.free.free { free.retain(|&id| id != key.slot); }
        self.built.retain(|&r| r != reference);
        self.passive.retain(|&r| r != reference);
        self.changed.retain(|&r| r != reference);
        store.units.remove(Member::Unit(reference));
        assert!(store.unit.retire(reference));
        self.references[reference.slot as usize] = None;
        self.marked[reference.slot as usize] = None;
        self.pooled[reference.slot as usize] = false;
        Ok(())
    }
}

pub fn mirror(reference: Ref<Unit>, unit: &Unit) -> F3UnitMirror {
    let mut view = F3UnitMirror {
        key: F3UnitKey::of(reference, unit), hero:unit.hero,human:unit.human,network_human:unit.network_human,team: unit.team, alive: unit.alive, dead: unit.dead,
        dead_time: unit.dead_time, removed: unit.removed, character: unit.character, controller: unit.controller, seat: unit.seat,
        held: match unit.held { crate::store::Nullable::Undefined => crate::store::Nullable::Undefined,
            crate::store::Nullable::Null => crate::store::Nullable::Null, _ => crate::store::Nullable::Value(true) },
        visible: !unit.removed && !(unit.dead && unit.combat.blow_apart) && !matches!(&unit.soldier, UnitBody::Droideka(d) if d.mode == 4.0),
        targetable: unit.targetable, tick_count: unit.tick_count, remote_active: unit.remotes_active,
        water_contact: unit.water_contact.as_ref().map(|w| crate::gen::WaterContact {
            gun_under: w.gun_under, under: w.under, wake_clock: w.wake_clock, splash_clock: w.splash_clock,
        }), ..Default::default()
    };
    match &unit.soldier {
        UnitBody::Soldier(s) => {
            view.position = s.position; view.previous = s.previous; view.settled = s.settled;
            view.velocity = s.velocity; view.avoidance = s.avoidance;
            view.soldier = Some(F3SoldierMotion {
                normal: s.normal, aim_yaw: s.aim_yaw, aim_pitch: s.aim_pitch, posture: s.posture, air: s.air,
                dive: s.dive, dive_from: s.dive_from, state_time: s.state_time, lock: s.lock, grounded: s.grounded,
                look_x: s.look_x, look_y: s.look_y, tick_length: s.tick_length, ground_adjustment: s.ground_adjustment,
                fuel: s.fuel, pass_through: s.pass_through, dead: s.dead,
            });
        }
        UnitBody::Droideka(d) => {
            view.position = d.position; view.previous = d.previous; view.settled = d.settled;
            view.velocity = d.velocity; view.avoidance = d.avoidance;
            view.droideka = Some(F3DroidekaMotion {
                aim_yaw: d.aim_yaw, aim_pitch: d.aim_pitch, state_time: d.state_time, grounded: d.grounded,
                look_x: d.look_x, look_y: d.look_y, mode: d.mode, time: d.time, reverse: d.reverse,
                legs: d.legs, spin: d.spin, roll: d.roll, lean: d.lean, dead: d.dead,
                blast_clock: d.blast_clock, gun: d.gun, last_gun: d.last_gun.as_ref().map(|g| crate::gen::LastGun { index: g.index, barrel: g.barrel }),
                shield: d.shield.as_ref().map(|s| F3ShieldView { value: s.value, on: s.on }), water_clock: d.water_clock,
            });
        }
    }
    view
}
