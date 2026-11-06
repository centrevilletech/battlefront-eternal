//! Building lifecycle and supply pulses (src/props.js). Constructor data
//! is cooked once by the loader. Collision visibility, crew deaths,
//! explosions and notifications cross World in their original order.

use three::{Matrix4, Vector3};
use crate::gen::{PbKind, PbSettings, PbSupply};
fn v3(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State { #[default] Unbuilt, Built, Destroyed }
impl State {
    pub fn name(self) -> &'static str { match self { Self::Unbuilt => "unbuilt", Self::Built => "built", Self::Destroyed => "destroyed" } }
    pub fn index(self) -> usize { match self { Self::Built => 0, Self::Destroyed => 1, Self::Unbuilt => 2 } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind { #[default] Building, Turret, Objective }

/// A host identity. Undefined is kept for buildings not damaged yet;
/// turrets begin with null and every damage call supplies null by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Identity { #[default] Undefined, Null, Object(usize) }

#[derive(Clone, Copy, Debug, Default)]
pub struct Model {
    pub sphere: Option<[f64; 4]>,
    pub bounds: Option<[Vector3; 2]>,
}

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub kind: Kind,
    pub max_health: f64,
    pub respawn_time: f64,
    pub built_add_health: f64,
    pub regenerates: bool,
    pub has_destroyed: bool,
    pub has_unbuilt: bool,
    pub has_crew: bool,
    pub has_explosion: bool,
    pub has_damaged: bool,
    pub matrix: Matrix4,
    pub models: [Option<Model>; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Building {
    pub state: State,
    pub health: f64,
    pub add_health: f64,
    pub last_attacker: Identity,
    pub target_generation: Option<f64>,
}

pub trait World {
    fn show(&mut self, building: &Building);
    fn crew_destroyed(&mut self, building: &mut Building);
    fn destroyed(&mut self, building: &mut Building);
    fn explode(&mut self, building: &mut Building, position: Vector3);
    fn damaged(&mut self, building: &mut Building, amount: f64, attacker: Identity, hit: Identity);
    fn served(&mut self, station: &Station, target: &mut Target, gave: u8);
    fn serve_sound(&mut self, station: &Station, first: Option<usize>);
    /// Publish foreign health/ammo writes before notifications can read
    /// them. Native callers already share the target table.
    fn target_changed(&mut self, _target: &Target) {}
}

pub fn min(a: f64, b: f64) -> f64 {
    let value = v8math::min(a, b);
    if value.is_nan() { f64::from_bits(0xfff8_0000_0000_0000) } else { value }
}
pub fn max(a: f64, b: f64) -> f64 {
    let value = v8math::max(a, b);
    if value.is_nan() { f64::from_bits(0xfff8_0000_0000_0000) } else { value }
}

impl Building {
    pub fn alive(&self) -> bool { self.state == State::Built }
    pub fn show(&mut self, state: State, world: &mut impl World) {
        self.state = state;
        world.show(self);
    }
    pub fn set_health(&mut self, value: f64, settings: &Settings, world: &mut impl World) {
        if value > 0.0 {
            self.health = min(value, settings.max_health);
            if self.health >= settings.max_health && !self.alive() { self.built(settings, world); }
        } else {
            self.health = 0.0;
            if self.alive() { self.destroyed(settings, world); }
        }
    }
    pub fn built(&mut self, settings: &Settings, world: &mut impl World) {
        self.show(State::Built, world);
        self.health = settings.max_health;
        self.add_health = settings.built_add_health;
        if settings.kind == Kind::Turret {
            if let Some(generation) = &mut self.target_generation { *generation += 1.0; }
            self.last_attacker = Identity::Null;
        }
    }
    pub fn destroyed(&mut self, settings: &Settings, world: &mut impl World) {
        if settings.has_crew { world.crew_destroyed(self); }
        world.destroyed(self);
        if settings.kind == Kind::Objective {
            // ObjectivePost measures its live model before showing the wreck.
            let position = self.sphere(settings).0;
            self.show(State::Destroyed, world);
            if settings.has_explosion { world.explode(self, position); }
            self.add_health = 0.0;
            return;
        }
        if settings.has_destroyed { self.show(State::Destroyed, world); }
        else if settings.has_unbuilt { self.show(State::Unbuilt, world); }
        if settings.has_explosion { world.explode(self, self.sphere(settings).0); }
        self.add_health = if settings.respawn_time.is_finite() && settings.respawn_time > 0.0 {
            settings.max_health / settings.respawn_time
        } else { 0.0 };
        if settings.kind == Kind::Turret && self.alive() { self.show(State::Destroyed, world); }
    }
    pub fn damage(&mut self, amount: f64, attacker: Identity, hit: Identity, settings: &Settings, world: &mut impl World) {
        if !self.alive() || !(amount > 0.0) { return; }
        self.damage_base(amount, attacker, settings, world);
        if settings.kind == Kind::Turret && settings.has_damaged { world.damaged(self, amount, attacker, hit); }
    }
    pub fn damage_base(&mut self, amount: f64, attacker: Identity, settings: &Settings, world: &mut impl World) {
        if !self.alive() || !(amount > 0.0) { return; }
        self.last_attacker = attacker;
        self.set_health(self.health - amount, settings, world);
    }
    pub fn regenerate(&mut self, dt: f64, settings: &Settings, world: &mut impl World) {
        if settings.regenerates && self.add_health > 0.0 && self.health < settings.max_health {
            self.set_health(self.health + self.add_health * dt, settings, world);
        }
    }
    pub fn sphere(&self, settings: &Settings) -> (Vector3, f64) {
        let model = settings.models[self.state.index()].as_ref();
        let mut radius = 1.0;
        let mut center = Vector3::new(0.0, 1.0, 0.0);
        if let Some(sphere) = model.and_then(|m| (self.state == State::Unbuilt).then_some(m.sphere).flatten()) {
            center.set(sphere[0], sphere[1], sphere[2]); radius = sphere[3];
        } else if let Some([lo, hi]) = model.and_then(|m| m.bounds) {
            center.set((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, (lo.z + hi.z) / 2.0);
            radius = v8math::hypot3(hi.x - lo.x, hi.y - lo.y, hi.z - lo.z) / 2.0;
        }
        center.apply_matrix4(&settings.matrix);
        (center, radius)
    }
    pub fn blast_sphere(&self, settings: &Settings) -> (Vector3, f64) {
        if let Some(sphere) = settings.models[self.state.index()].as_ref().and_then(|m| m.sphere) {
            let mut center = Vector3::new(sphere[0], sphere[1], sphere[2]);
            center.apply_matrix4(&settings.matrix);
            (center, sphere[3])
        } else { self.sphere(settings) }
    }
}

#[derive(Clone, Debug)]
pub struct Weapon {
    pub bottomless: bool,
    pub ammo: f64,
    pub clip: f64,
    pub per_shot: f64,
    pub reserve: f64,
}
#[derive(Clone, Debug)]
pub struct Target {
    pub id: usize,
    pub position: Vector3,
    pub vehicle: bool,
    pub alive: bool,
    pub player: bool,
    pub radius: f64,
    pub health: f64,
    pub max_health: f64,
    pub weapons: Vec<Weapon>,
}
#[derive(Clone, Copy, Debug)]
pub struct Supply {
    pub position: Vector3,
    pub radius: f64,
    pub delay: f64,
    pub health: f64,
    pub ammo: f64,
    pub vehicle_health: f64,
    pub vehicle_ammo: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    pub building: Building,
    pub active_mode: bool,
    pub active: f64,
    pub pulse: f64,
    pub served_player: bool,
    pub served_health: bool,
    pub served_ammo: bool,
}
impl Station {
    pub fn serve(supply: &Supply, target: &mut Target) -> u8 {
        let mut served = 0;
        let (health, ammo) = if target.vehicle { (supply.vehicle_health, supply.vehicle_ammo) } else { (supply.health, supply.ammo) };
        if let Some(next) = supply_health(target.health, target.max_health, health) {
            target.health = next; served |= 1;
        }
        if ammo > 0.0 {
            for weapon in &mut target.weapons {
                if let Some(next) = supply_reserve(weapon.bottomless, weapon.ammo, weapon.clip, weapon.per_shot, weapon.reserve, ammo) {
                    weapon.reserve = next; served |= 2;
                }
            }
        }
        served
    }
    pub fn update(&mut self, dt: f64, settings: &Settings, supply: &Supply, targets: &mut [Target], soldiers: &[usize], vehicles: &[usize], world: &mut impl World) {
        self.building.regenerate(dt, settings, world);
        self.pulse(dt, supply, &mut SliceSupplies { targets, soldiers, vehicles, world });
    }
    /// PowerupStation.update after regeneration. Native services visit live
    /// bodies in source order and write their canonical health/ammunition;
    /// no complete target or weapon table must be copied for a pulse.
    pub fn pulse<T: Supplies>(&mut self, dt: f64, supply: &Supply, targets: &mut T) {
        if !self.building.alive() { return; }
        if self.active_mode {
            // V8's += keeps a NaN right operand's payload. LLVM may
            // commute this addition independently of the pulse's below.
            self.active = if dt.is_nan() { dt } else { self.active + dt };
        }
        self.pulse += dt;
        if self.pulse >= supply.delay {
            self.pulse -= supply.delay;
            let mut served = 0;
            let mut first = None;
            self.served_player = false;
            for index in 0..targets.soldiers() {
                let target = targets.soldier(index);
                if target.position.distance_to(&supply.position) >= supply.radius { continue; }
                let gave = targets.serve(target.key, supply);
                if gave == 0 { continue; }
                if first.is_none() { first = Some(target.key); }
                served |= gave;
                if target.player { self.served_player = true; }
                targets.changed(target.key);
                targets.served(self, target.key, gave);
            }
            if truth(supply.vehicle_health) || truth(supply.vehicle_ammo) {
                for index in 0..targets.vehicles() {
                    let target = targets.vehicle(index);
                    if target.alive && target.position.distance_to(&supply.position) < supply.radius + target.radius * 0.5 {
                        let gave = targets.serve(target.key, supply);
                        served |= gave;
                        if gave != 0 { targets.changed(target.key); }
                    }
                }
            }
            self.served_health = served & 1 != 0;
            self.served_ammo = served & 2 != 0;
            if served != 0 {
                targets.sound(self, first);
                if !self.active_mode { self.active = 0.0; }
                self.active_mode = true;
            } else if self.active_mode { self.active_mode = false; }
        }
    }
}
fn truth(value: f64) -> bool { value != 0.0 && !value.is_nan() }

// Generated reader records are scalar snapshots of canonical state.
use crate::gen::{PbIdentity, PbState, PbStateName, PbStation, PbMode};
impl From<&PbIdentity> for Identity {
    fn from(v: &PbIdentity) -> Self { match v.kind as u32 { 0 => Self::Undefined, 1 => Self::Null, 2 => Self::Object(v.id as usize), _ => panic!("prop identity") } }
}
impl From<Identity> for PbIdentity {
    fn from(v: Identity) -> Self { match v { Identity::Undefined => Self { kind: 0.0, id: 0.0 }, Identity::Null => Self { kind: 1.0, id: 0.0 }, Identity::Object(id) => Self { kind: 2.0, id: id as f64 } } }
}
impl From<PbState> for Building {
    fn from(v: PbState) -> Self { Self { state: match v.state { PbStateName::Unbuilt => State::Unbuilt, PbStateName::Built => State::Built, PbStateName::Destroyed => State::Destroyed },
        health: v.health, add_health: v.add_health, last_attacker: (&v.last_attacker).into(), target_generation: v.target_generation } }
}
impl From<&Building> for PbState {
    fn from(v: &Building) -> Self { Self { state: match v.state { State::Unbuilt => PbStateName::Unbuilt, State::Built => PbStateName::Built, State::Destroyed => PbStateName::Destroyed },
        health: v.health, add_health: v.add_health, last_attacker: v.last_attacker.into(), target_generation: v.target_generation } }
}
impl From<PbStation> for Station {
    fn from(v: PbStation) -> Self { Self { building: v.building.into(), active_mode: v.mode == PbMode::Active, active: v.active, pulse: v.pulse,
        served_player: v.served_player, served_health: v.served_health, served_ammo: v.served_ammo } }
}
impl From<&Station> for PbStation {
    fn from(v: &Station) -> Self { Self { building: (&v.building).into(), mode: if v.active_mode { PbMode::Active } else { PbMode::Waiting }, active: v.active, pulse: v.pulse,
        served_player: v.served_player, served_health: v.served_health, served_ammo: v.served_ammo } }
}

// Generated construction records are decoded once, before the live phase.
impl From<PbSupply> for Supply {
    fn from(v: PbSupply) -> Self { Self { position: v3(v.position), radius: v.radius, delay: v.delay, health: v.health, ammo: v.ammo, vehicle_health: v.vehicle_health, vehicle_ammo: v.vehicle_ammo } }
}
impl From<PbSettings> for Settings {
fn from(v: PbSettings) -> Self {
    let mut matrix = Matrix4::default(); matrix.from_array(&v.matrix, 0);
    Settings { kind: match v.kind { PbKind::Building => Kind::Building, PbKind::Turret => Kind::Turret, PbKind::Objective => Kind::Objective },
        max_health: v.max_health, respawn_time: v.respawn_time, built_add_health: v.built_add_health, regenerates: v.regenerates,
        has_destroyed: v.has_destroyed, has_unbuilt: v.has_unbuilt, has_crew: false, has_explosion: v.has_explosion, has_damaged: false, matrix,
        models: v.models.into_iter().map(|v| v.map(|m| Model { sphere: m.sphere, bounds: m.low.zip(m.high).map(|(lo, hi)| [v3(lo), v3(hi)]) })).collect::<Vec<_>>().try_into().expect("three building looks") }
}
}

/// Health and reserve arithmetic shared with canonical Unit/hull services.
pub fn supply_health(current: f64, maximum: f64, amount: f64) -> Option<f64> {
    (amount > 0.0 && current < maximum).then(|| min(current + amount, maximum))
}
pub fn supply_reserve(bottomless: bool, ammo: f64, clip: f64, per_shot: f64, reserve: f64, amount: f64) -> Option<f64> {
    if !(amount > 0.0) || bottomless || !(ammo > 0.0) { return None; }
    let loaded = if clip >= per_shot - 1e-6 { 1.0 } else { 0.0 };
    let next = max(0.0, min(ammo - loaded, reserve + amount));
    ((next - reserve).abs() > 0.0001).then_some(next)
}
#[derive(Clone, Copy)]
pub struct SupplyCandidate<K> {
    pub key: K, pub position: Vector3, pub alive: bool, pub player: bool, pub radius: f64,
}
/// Native services lend one candidate's scalar facts at a time. Keys carry
/// its full native identity; a notice never resolves a later pooled body.
pub trait Supplies {
    type Key: Copy;
    fn soldiers(&self) -> usize;
    fn soldier(&mut self, index: usize) -> SupplyCandidate<Self::Key>;
    fn vehicles(&self) -> usize;
    fn vehicle(&mut self, index: usize) -> SupplyCandidate<Self::Key>;
    fn serve(&mut self, key: Self::Key, supply: &Supply) -> u8;
    fn changed(&mut self, key: Self::Key);
    fn served(&mut self, station: &Station, key: Self::Key, gave: u8);
    fn sound(&mut self, station: &Station, first: Option<Self::Key>);
}
struct SliceSupplies<'a, W> { targets: &'a mut [Target], soldiers: &'a [usize], vehicles: &'a [usize], world: &'a mut W }
impl<W> SliceSupplies<'_, W> {
    fn candidate(&self, key: usize) -> SupplyCandidate<usize> {
        let t = &self.targets[key]; SupplyCandidate { key, position: t.position, alive: t.alive, player: t.player, radius: t.radius }
    }
}
impl<W: World> Supplies for SliceSupplies<'_, W> {
    type Key = usize;
    fn soldiers(&self) -> usize { self.soldiers.len() }
    fn soldier(&mut self, i: usize) -> SupplyCandidate<usize> { self.candidate(self.soldiers[i]) }
    fn vehicles(&self) -> usize { self.vehicles.len() }
    fn vehicle(&mut self, i: usize) -> SupplyCandidate<usize> { self.candidate(self.vehicles[i]) }
    fn serve(&mut self, key: usize, supply: &Supply) -> u8 { Station::serve(supply, &mut self.targets[key]) }
    fn changed(&mut self, key: usize) { self.world.target_changed(&self.targets[key]); }
    fn served(&mut self, station: &Station, key: usize, gave: u8) { self.world.served(station, &mut self.targets[key], gave); }
    fn sound(&mut self, station: &Station, first: Option<usize>) { self.world.serve_sound(station, first.map(|key| self.targets[key].id)); }
}
