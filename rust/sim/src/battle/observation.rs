//! battle-stats.js observe: read canonical bodies and native mount facts.
//! Display labels and weapon channels are prepared once with content handles.
use super::stats::Seen;
use crate::gen::{F4StatsClass, F4StatsHull, F4StatsWeapon, Vehicle, SeatLease, Store, Unit};
use crate::store::{DefId, Ref, StrId};
#[derive(Clone, Copy, Default)]
struct Class { label: Option<StrId> }
#[derive(Clone, Copy, Default)]
struct Hull { generation: u32, label: Option<StrId> }
#[derive(Clone, Default)]
struct Weapon { primary: bool, key: Option<StrId>, labels: Vec<Option<StrId>> }
#[derive(Clone, Copy, Default)]
pub struct Mount {
    pub position: [f64; 3],
    pub weapon: Option<(DefId, f64)>,
    pub vehicle: Option<(Option<StrId>, Option<StrId>)>,
}
/// F2 reads its retained mounted weapons and turret placement directly. A
/// released lease never reaches this service and uses the soldier's hand.
pub trait Mounts {
    fn mount(&mut self, s: &Store, lease: Ref<SeatLease>) -> Option<Mount>;
}
#[derive(Default)]
pub struct Observations { classes: Vec<Option<Class>>, hulls: Vec<Option<Hull>>, weapons: Vec<Option<Weapon>> }
impl Observations {
    pub fn class(&mut self, class: F4StatsClass) {
        self.classes.resize(self.classes.len().max(class.def.0 as usize + 1), None);
        self.classes[class.def.0 as usize] = Some(Class { label: class.label });
    }
    pub fn hull(&mut self, hull: F4StatsHull) {
        self.hulls.resize(self.hulls.len().max(hull.hull.slot as usize + 1), None);
        self.hulls[hull.hull.slot as usize] = Some(Hull { generation: hull.hull.gen, label: hull.label });
    }
    pub fn weapon(&mut self, weapon: F4StatsWeapon) {
        self.weapons.resize(self.weapons.len().max(weapon.def.0 as usize + 1), None);
        self.weapons[weapon.def.0 as usize] = Some(Weapon { primary: weapon.primary, key: weapon.key, labels: weapon.labels });
    }
    pub fn vehicle_favourite(&self, s: &Store, r: Ref<Vehicle>) -> Option<(Option<StrId>, Option<StrId>)> {
        let vehicle = s.vehicle.get(r)?;
        let facts = self.hulls.get(r.slot as usize).copied().flatten()?;
        (facts.generation == r.gen).then_some((Some(vehicle.name), facts.label))
    }
    fn favourite(&self, def: DefId, level: f64) -> (Option<StrId>, Option<StrId>) {
        let weapon = self.weapons.get(def.0 as usize).and_then(Option::as_ref).expect("prepared statistics weapon");
        let label = if level >= 0.0 && level.is_finite() && level.fract() == 0.0 {
            weapon.labels.get(level as usize).copied().flatten()
        } else { None };
        (weapon.key, label)
    }
    fn hand(&self, u: &Unit) -> Option<(DefId, f64)> {
        let selected = u.combat.selected[0];
        if selected < 0.0 || !selected.is_finite() || selected.fract() != 0.0 { return None; }
        let mut primary = 0;
        for weapon in &u.combat.weapons {
            let info = self.weapons.get(weapon.def.0 as usize).and_then(Option::as_ref).expect("prepared statistics weapon");
            if !info.primary { continue; }
            if primary as f64 == selected { return Some((weapon.def, weapon.level)); }
            primary += 1;
        }
        None
    }
    /// A corpse still exposes its selected weapon, class and place. Only its
    /// health observation is absent; a removed body has no observation at all.
    pub fn observe<M: Mounts + ?Sized>(&self, s: &Store, unit: Option<Ref<Unit>>, mounts: &mut M) -> Option<Seen> {
        let unit = s.unit.get(unit?)?; if unit.removed { return None; }
        let seat = unit.seat.filter(|r| s.seat_lease.get(*r).is_some_and(|s| !s.released));
        let mount = seat.and_then(|r| mounts.mount(s, r));
        let p = unit.position();
        let position = mount.map_or([p.x, p.y, p.z], |m| m.position);
        let weapon = match mount { Some(m) => m.weapon, None => self.hand(unit) };
        let max = unit.combat.max_health;
        let max = if max == 0.0 || max.is_nan() { 1.0 } else { max };
        Some(Seen { position: Some(position),
            health: (unit.alive && !unit.combat.dead).then(|| unit.combat.health / v8math::max(max, 1e-6)),
            weapon: weapon.map(|(def, level)| self.favourite(def, level)), vehicle: mount.and_then(|m| m.vehicle),
            class_label: self.classes.get(unit.class_def.0 as usize).copied().flatten().expect("prepared statistics class").label })
    }
}
