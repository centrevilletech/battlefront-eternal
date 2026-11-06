//! PowerupStation targets over canonical unit and hull rows. LiveObjects freezes
//! membership before supply, then each station observes current positions and
//! writes health/ammunition directly. Definition facts are prepared at load.
use super::props::{self, Station, Supplies, Supply, SupplyCandidate};
use crate::gen::{Store, Unit, Vehicle};
use crate::store::{DefId, Nullable, Ref};
use three::Vector3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Unit { reference: Ref<Unit>, life: f64, player: bool },
    Hull { reference: Ref<Vehicle>, life: f64 },
}
pub struct Prepared { classes: Vec<Option<bool>>, hulls:Vec<Option<(Ref<Vehicle>,bool)>>, ammunition: Vec<Vec<f64>> }
impl Default for Prepared {
    fn default() -> Self { Self { classes: Vec::new(), hulls:Vec::new(), ammunition: Vec::new() } }
}
impl Prepared {
    pub fn class(&mut self, def: DefId, vehicle: bool) {
        self.classes.resize(self.classes.len().max(def.0 as usize + 1), None);
        self.classes[def.0 as usize] = Some(vehicle);
    }
    // PowerupStation.serve classifies by healthType, not by the presence of
    // seats: an animal hull keeps soldier rates even in the vehicle traversal.
    pub fn hull(&mut self,r:Ref<Vehicle>,vehicle:bool){
        self.hulls.resize(self.hulls.len().max(r.slot as usize+1),None);
        self.hulls[r.slot as usize]=Some((r,vehicle));
    }
    pub fn weapon(&mut self, def: DefId, ammunition: Vec<f64>) {
        self.ammunition.resize_with(self.ammunition.len().max(def.0 as usize + 1), Vec::new);
        self.ammunition[def.0 as usize] = ammunition;
    }
}
/// Source order is the local viewpoint, then UnitFactory.built. Hulls remain
/// in the fleet's construction order, including its inactive variants.
pub struct Targets { units: Vec<Key>, hulls: Vec<Key> }
impl Targets {
    pub fn new(units: usize, hulls: usize) -> Self {
        Self { units: Vec::with_capacity(units + 1), hulls: Vec::with_capacity(hulls) }
    }
    pub fn reserve(&mut self, units: usize, hulls: usize) {
        self.units.reserve((units + 1).saturating_sub(self.units.len()));
        self.hulls.reserve(hulls.saturating_sub(self.hulls.len()));
    }
    pub fn begin(&mut self, s: &Store, local: Option<Ref<Unit>>, driving: bool, built: &[Ref<Unit>], hulls: &[Ref<Vehicle>]) {
        self.units.clear(); self.hulls.clear();
        // The viewpoint is supplied while held or manning a stationary gun.
        // Its supply facade has no healthType, even for vehicle-health classes.
        if let Some(r) = local.filter(|_| !driving) {
            let u = s.unit.get(r).expect("local supply body");
            if !u.combat.dead { self.units.push(Key::Unit { reference: r, life: u.generation, player: true }); }
        }
        for &r in built {
            let u = s.unit.get(r).expect("ordered factory body");
            if u.alive && u.seat.is_none() && !matches!(u.held, Nullable::Value(_)) {
                self.units.push(Key::Unit { reference: r, life: u.generation, player: false });
            }
        }
        for &r in hulls {
            let v = s.vehicle.get(r).expect("ordered fleet hull");
            self.hulls.push(Key::Hull { reference: r, life: v.target_generation });
        }
    }
    pub fn pulse<W: World>(&self, station: &mut Station, dt: f64, supply: &Supply, s: &mut Store, prepared: &Prepared, world: &mut W) {
        station.pulse(dt, supply, &mut Pulse { s, targets: self, prepared, world });
    }
}
/// Hull health and ammunition belong to F2. Notices capture immutable unit
/// facts at this slot; publishing them later must not follow a pooled body.
pub trait World {
    fn hull_health(&mut self, s: &mut Store, reference: Ref<Vehicle>, life: f64, health: f64);
    fn hull_ammo(&mut self, s: &mut Store, reference: Ref<Vehicle>, life: f64, amount: f64) -> bool;
    fn changed(&mut self, s: &mut Store, key: Key);
    fn served(&mut self, station: &Station, key: Key, class: DefId, position: Vector3, gave: u8);
    fn sound(&mut self, station: &Station, first: Option<Key>);
}
struct Pulse<'a, W> { s: &'a mut Store, targets: &'a Targets, prepared: &'a Prepared, world: &'a mut W }
impl<W: World> Supplies for Pulse<'_, W> {
    type Key = Key;
    fn soldiers(&self) -> usize { self.targets.units.len() }
    fn soldier(&mut self, i: usize) -> SupplyCandidate<Key> {
        let key = self.targets.units[i];
        let Key::Unit { reference, life, player } = key else { unreachable!() };
        let u = self.s.unit.get(reference).filter(|u| u.generation == life);
        SupplyCandidate { key, position: u.map_or(Vector3::new(f64::NAN, 0.0, 0.0), Unit::position),
            alive: u.is_some_and(|u| u.alive), player, radius: 0.0 }
    }
    fn vehicles(&self) -> usize { self.targets.hulls.len() }
    fn vehicle(&mut self, i: usize) -> SupplyCandidate<Key> {
        let key = self.targets.hulls[i];
        let Key::Hull { reference, life } = key else { unreachable!() };
        let v = self.s.vehicle.get(reference).filter(|v| v.target_generation == life);
        let p = v.map_or([f64::NAN, 0.0, 0.0], |v| v.position);
        SupplyCandidate { key, position: Vector3::new(p[0], p[1], p[2]), alive: v.is_some_and(|v| v.alive), player: false, radius: v.map_or(0.0, |v| v.radius) }
    }
    fn serve(&mut self, key: Key, supply: &Supply) -> u8 {
        match key {
            Key::Unit { reference, life, player } => {
                let Some(u) = self.s.unit.get_mut(reference).filter(|u| u.generation == life) else { return 0 };
                let vehicle = !player && self.prepared.classes[u.class_def.0 as usize].expect("prepared supply class");
                let (health, ammo) = if vehicle { (supply.vehicle_health, supply.vehicle_ammo) } else { (supply.health, supply.ammo) };
                let mut gave = 0;
                if let Some(next) = props::supply_health(u.combat.health, u.combat.max_health, health) { u.combat.health = next; gave |= 1; }
                if ammo > 0.0 { for w in &mut u.combat.weapons {
                    let cap = self.prepared.ammunition[w.def.0 as usize][w.level as usize];
                    if let Some(next) = props::supply_reserve(w.bottomless, cap, w.clip, w.per_shot, w.reserve, ammo) { w.reserve = next; gave |= 2; }
                }}
                gave
            }
            Key::Hull { reference, life } => {
                let Some(v) = self.s.vehicle.get(reference).filter(|v| v.target_generation == life) else { return 0 };
                let vehicle=self.prepared.hulls[reference.slot as usize].filter(|(r,_)|*r==reference).expect("prepared hull supply class").1;
                let (health,ammo)=if vehicle{(supply.vehicle_health,supply.vehicle_ammo)}else{(supply.health,supply.ammo)};
                let next = props::supply_health(v.health, v.max_health, health);
                let mut gave = 0;
                if let Some(next) = next { self.world.hull_health(self.s, reference, life, next); gave |= 1; }
                if ammo > 0.0 && self.world.hull_ammo(self.s, reference, life, ammo) { gave |= 2; }
                gave
            }
        }
    }
    fn changed(&mut self, key: Key) { self.world.changed(self.s, key); }
    fn served(&mut self, station: &Station, key: Key, gave: u8) {
        let Key::Unit { reference, life, .. } = key else { unreachable!() };
        let u = self.s.unit.get(reference).filter(|u| u.generation == life).expect("served supply life");
        self.world.served(station, key, u.class_def, u.position(), gave);
    }
    fn sound(&mut self, station: &Station, first: Option<Key>) { self.world.sound(station, first); }
}
