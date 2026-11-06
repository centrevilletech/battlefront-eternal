//! LiveObjects.update (src/props.js): supply, regeneration and idle turret
//! pose, then animated collision/grabs and log-trap crush in insertion order.
//! Objects and prepared settings persist across phases. Unit and hull services
//! provide native reader views; this scheduler never calls a JS object method.
use super::{animated, ownership::{Publish, Rows}, props, stats::Text, traps};
use crate::gen::{F4AnimatedView, PaVictim, F4BuildingView, F4Handle, F4ObjectKind, F4TrapView, TrPhase, TrState};
use three::Vector3;
use crate::store::Ref;
use crate::gen::{Attacker, Vehicle};

pub struct Building {
    pub state: props::Station,
    pub settings: props::Settings,
    pub supply: Option<props::Supply>,
    pub attacker: Attacker,
}
impl Publish for Building {
    const WORDS: usize = F4BuildingView::WORDS;
    fn write(&self, row: &mut [f64], heap: &mut Vec<f64>) {
        F4BuildingView { state: (&self.state).into(), attacker: self.attacker }.put(row, heap);
    }
}
pub struct Trap {
    pub state: traps::Trap, pub settings: traps::Settings,
    pub attacker: Attacker,
    pub(super) victims: Vec<Ref<Vehicle>>,
}
impl Trap {
    pub fn new(state: traps::Trap, settings: traps::Settings, hull_capacity: usize) -> Self {
        Self { state, settings, attacker: Attacker::Undefined, victims: Vec::with_capacity(hull_capacity) }
    }
}
impl Publish for Trap {
    const WORDS: usize = F4TrapView::WORDS;
    fn write(&self, row: &mut [f64], heap: &mut Vec<f64>) {
        let s = &self.state;
        F4TrapView { state: TrState { built: s.built, health: s.health,
            phase: match s.phase { traps::Phase::Down => TrPhase::Down, traps::Phase::Reset => TrPhase::Reset,
                traps::Phase::Armed => TrPhase::Armed, traps::Phase::Swing => TrPhase::Swing },
            started: s.started, time: s.time, sprung: s.sprung, tick: s.tick, crushed: s.crushed }, attacker: self.attacker }.put(row, heap);
    }
}

/// F2 retains all turret aim/crew/weapon authority. Its idle pass runs once
/// after every building's regeneration, including a rebuild earlier here.
pub trait World: props::World + traps::World<Vehicle = Ref<Vehicle>> + animated::World {
    fn idle_turrets(&mut self, dt: f64);
}
pub struct SupplyTargets<'a> {
    pub targets: &'a mut [props::Target],
    pub soldiers: &'a [usize],
    pub vehicles: &'a [usize],
}

pub struct Live {
    pub buildings: Rows<Building>,
    pub traps: Rows<Trap>,
    pub animated: Rows<Animated>,
    stations: Vec<F4Handle>,
    turrets: Vec<F4Handle>,
}
impl Live {
    pub fn new(buildings: usize, traps: usize, animated: usize) -> Self {
        Self { buildings: Rows::new(F4ObjectKind::Building, buildings), traps: Rows::new(F4ObjectKind::Trap, traps),
            animated: Rows::new(F4ObjectKind::Animated, animated),
            stations: Vec::with_capacity(buildings), turrets: Vec::with_capacity(buildings) }
    }
    pub fn adopt(&mut self, life: f64, mut object: Building) -> F4Handle {
        let station = object.supply.is_some(); let turret = object.settings.kind == props::Kind::Turret;
        object.state.building.last_attacker = identity(object.attacker);
        let h = self.buildings.adopt(life, object);
        if station { self.stations.push(h.clone()); }
        else if turret { self.turrets.push(h.clone()); }
        h
    }
    /// Fleet construction can follow trap construction. Reserve every trap's
    /// victim snapshot when a variant is born, before an authoritative tick.
    pub fn prepare_hulls(&mut self, capacity: usize) {
        for i in 0..self.traps.len() {
            let h = self.traps.handle(i);
            self.traps.command(&h, |entry| entry.victims.reserve(capacity.saturating_sub(entry.victims.len())));
        }
    }
    /// LiveObjects' station and turret span. Callers keep the target buffers
    /// reserved and current with their owning unit/weapon/hull stores.
    pub fn supply<W: World>(&mut self, dt: f64, targets: &mut SupplyTargets<'_>, world: &mut W) {
        for h in &self.stations {
            let Some(h) = self.buildings.current(h) else { continue };
            self.buildings.lifecycle_command(&h, |entry| {
                let before = entry.state.building.alive();
                entry.state.update(dt, &entry.settings, entry.supply.as_ref().expect("station supply"),
                    targets.targets, targets.soldiers, targets.vehicles, world);
                let rebuilt = !before && entry.state.building.alive();
                if rebuilt && entry.settings.kind == props::Kind::Turret { entry.attacker = Attacker::Null; }
                rebuilt
            });
        }
        for h in &self.turrets {
            let Some(h) = self.buildings.current(h) else { continue };
            self.buildings.lifecycle_command(&h, |entry| {
                let before = entry.state.building.alive();
                entry.state.building.regenerate(dt, &entry.settings, world);
                let rebuilt = !before && entry.state.building.alive();
                if rebuilt && entry.settings.kind == props::Kind::Turret { entry.attacker = Attacker::Null; }
                rebuilt
            });
        }
        world.idle_turrets(dt);
    }
    /// Animated props run between supply() and crush(). Their retained rigs
    /// and held-body writer belong to the unit service at that join.
    pub fn crush<W: World>(&mut self, dt: f64, tick: f64, strings: &dyn Text, vehicles: &[W::Vehicle], world: &mut W) {
        for i in 0..self.traps.len() {
            let h = self.traps.handle(i);
            self.traps.command(&h, |entry| entry.state.step_with_scratch(dt, tick, &entry.settings, strings, vehicles, world, &mut entry.victims));
        }
    }
    pub fn health<W: World>(&mut self, h: &F4Handle, value: f64, world: &mut W) -> bool {
        match h.kind {
            F4ObjectKind::Building => self.buildings.lifecycle_command(h, |e| {
                let before = e.state.building.alive();
                e.state.building.set_health(value, &e.settings, world);
                let rebuilt = !before && e.state.building.alive();
                if rebuilt && e.settings.kind == props::Kind::Turret { e.attacker = Attacker::Null; }
                rebuilt
            }),
            F4ObjectKind::Trap => self.traps.lifecycle_command(h, |e| {
                let before = e.state.built; e.state.set_health(value, &e.settings, world);
                !before && e.state.built
            }),
            _ => false,
        }
    }
    pub fn damage<W: World>(&mut self, h: &F4Handle, amount: f64, by: Attacker, hit: props::Identity, world: &mut W) -> bool {
        match h.kind {
            F4ObjectKind::Building => self.buildings.lifecycle_command(h, |e| {
                let before = e.state.building.alive();
                if before && amount > 0.0 { e.attacker = by; }
                e.state.building.damage(amount, identity(by), hit, &e.settings, world);
                let rebuilt = !before && e.state.building.alive();
                if rebuilt && e.settings.kind == props::Kind::Turret { e.attacker = Attacker::Null; }
                rebuilt
            }),
            // Trap inherits Building.damage, but its health setter springs the
            // still-built logs at zero HP instead of destroying a wreck.
            F4ObjectKind::Trap => self.traps.command(h, |e| {
                if e.state.built && amount > 0.0 {
                    e.attacker = by;
                    e.state.set_health(e.state.health - amount, &e.settings, world);
                }
            }),
            _ => false,
        }
    }
    pub fn play(&mut self, h: &F4Handle, slot: i32, looping: bool, tick: f64) -> bool {
        self.animated.command(h, |e| e.state.play(slot, looping, tick, &e.settings))
    }

    /// The whole LiveObjects.update span, entered once from the host tick.
    /// Ordnance and battle-tail phases remain separate scheduler slots.
    #[allow(clippy::too_many_arguments)]
    pub fn step<W: World>(&mut self, dt: f64, tick: f64, targets: &mut SupplyTargets<'_>,
        positions: &[Vector3], bodies: &mut [animated::Body], order: &[usize],
        strings: &dyn Text, vehicles: &[W::Vehicle], world: &mut W) -> usize {
        self.supply(dt, targets, world);
        for i in 0..self.animated.len() {
            let h = self.animated.handle(i);
            self.animated.command(&h, |e| e.step(dt, tick, positions, bodies, order, world));
        }
        self.crush(dt, tick, strings, vehicles, world);
        self.flush()
    }
    pub fn flush(&mut self) -> usize { self.buildings.flush() + self.animated.flush() + self.traps.flush() }
}

/// Animated props keep the native pose solver's state and prepared clip/rig
/// handles. Its body order is the canonical unit registry's order.
pub struct Animated { pub state: animated::Animated, pub settings: animated::Settings }
impl Animated {
    pub fn step(&mut self, dt: f64, tick: f64, positions: &[Vector3], bodies: &mut [animated::Body], order: &[usize], world: &mut impl animated::World) {
        self.state.step(dt, positions, bodies, order, tick, &self.settings, world);
    }
}

impl Publish for Animated {
    const WORDS: usize = F4AnimatedView::WORDS;
    fn write(&self, row: &mut [f64], heap: &mut Vec<f64>) {
        write_animated(&self.state, row, heap);
    }
}

pub fn write_animated(s: &animated::Animated, row: &mut [f64], heap: &mut Vec<f64>) {
        F4AnimatedView { slot: s.slot, time: s.time, weight: s.weight, looping: s.looping,
            hold: s.hold, started: s.started, start_time: s.start_time,
            has_victim: s.victim.is_some(),
            victim: s.victim.as_ref().map_or_else(PaVictim::default, |v| PaVictim {
                unit: v.unit as f64, generation: v.generation, grab: v.grab as f64, since: v.since }),
            openness: s.openness, body_enabled: s.body_enabled }.put(row, heap);
}

// The lifecycle kernel needs only absence/null/object distinctions. The
// qualified native attacker stays on its owning row, including snapshots of
// a pooled shooter's previous life; no JavaScript identity table owns credit.
fn identity(attacker: Attacker) -> props::Identity {
    match attacker { Attacker::Undefined => props::Identity::Undefined,
        Attacker::Null => props::Identity::Null, _ => props::Identity::Object(0) }
}
