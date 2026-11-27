//! Cold mission names and qualified native object operations for rules.
use super::events::ObjectHandle;
use crate::gen::{F4ObjectKind, SpawnPost, Store};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleError {
    NotAvailable,
    UnknownName,
    AmbiguousName,
    StaleHandle,
    InvalidValue,
    LimitReached,
}
pub type RuleResult<T> = Result<T, RuleError>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ObjectProperty {
    Health(f64),
    MaxHealth(f64),
    Team(f64),
    Collidable(bool),
    Visible(bool),
    PhysicsActive(bool),
    Locked(bool),
    CurrentShield(f64),
    MaxShield(f64),
    AddShield(f64),
    LockOn(bool),
    AiDamageThreshold(f64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ObjectState {
    pub alive: bool,
    pub team: Option<f64>,
    pub health: Option<(f64, f64)>,
}

pub trait ObjectServices {
    fn add_landing_region(&mut self, _name: &str) -> RuleResult<()> { Err(RuleError::NotAvailable) }
    fn passage_barrier(&mut self, _name: &str, _enabled: bool) -> bool { false }
    /// Follow a named instance across lives. Commands always require the exact life.
    fn current(&self, store: &Store, object: &ObjectHandle) -> RuleResult<ObjectHandle>;
    fn state(&self, store: &Store, object: &ObjectHandle) -> RuleResult<ObjectState>;
    fn goal_alive(&self, store: &Store, object: &ObjectHandle) -> RuleResult<bool> {
        self.state(store, object).map(|state| state.alive)
    }
    fn change(&mut self, store: &mut Store, object: &ObjectHandle, property: ObjectProperty) -> RuleResult<()>;
    fn shields(&self, _store: &Store, _object: &ObjectHandle) -> RuleResult<(f64, f64, f64)> { Err(RuleError::NotAvailable) }
    fn text(&self, id: crate::store::StrId) -> RuleResult<&[u16]>;
}

#[derive(Debug)]
pub struct NamedObject {
    pub object: ObjectHandle,
    pub name: Option<String>,
    pub class: Option<String>,
}

/// Sorted once at admission. Name comparisons fold ASCII in place and never
/// allocate. The reverse index includes slab generations and spans rebuilds.
#[derive(Default)]
pub struct MissionObjects {
    rows: Vec<NamedObject>,
    names: Vec<usize>,
}

pub(crate) fn key(object: &ObjectHandle) -> (u8, u32, u32) {
    match object {
        ObjectHandle::Door(instance) => (u8::MAX - 1, *instance, 0),
        ObjectHandle::Prop(h) => (h.kind as u8, h.slot, h.generation),
        ObjectHandle::Vehicle { vehicle, .. } => (u8::MAX, vehicle.slot, vehicle.gen),
        ObjectHandle::Post(SpawnPost::Post(r)) => (F4ObjectKind::Post as u8, r.slot, r.gen),
        ObjectHandle::Post(SpawnPost::MobilePost(r)) => (F4ObjectKind::MobilePost as u8, r.slot, r.gen),
    }
}

/// The order of names in the index: bytes, ASCII letters folded to lower
/// case. Every lookup of mission names (objects, regions) uses this order.
pub(crate) fn name_order(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    a.iter().map(u8::to_ascii_lowercase).cmp(b.iter().map(u8::to_ascii_lowercase))
}

fn name_cmp(a: &str, b: &str) -> std::cmp::Ordering { name_order(a.as_bytes(), b.as_bytes()) }

impl MissionObjects {
    pub fn new(mut rows: Vec<NamedObject>) -> Self {
        rows.sort_by_key(|row| key(&row.object));
        let mut names: Vec<_> = (0..rows.len()).filter(|&i| rows[i].name.is_some()).collect();
        names.sort_by(|&a, &b| name_cmp(rows[a].name.as_ref().unwrap(), rows[b].name.as_ref().unwrap()));
        Self { rows, names }
    }

    pub(crate) fn admitted(w: &crate::world::owner::World, rows: Vec<NamedObject>) -> Self {
        let mut index = Self::new(rows);
        // A destructible post also exposes its physical building. Keep the
        // post handle for callbacks but resolve the shared name to the building.
        let rows = &index.rows;
        index.names.retain(|&i| {
            let ObjectHandle::Post(SpawnPost::Post(post)) = rows[i].object else { return true };
            let Some(building) = w.sim.store.post.get(post).and_then(|p| p.building) else { return true };
            let Some(handle) = crate::units::f3_props::building_owner(&w.units, &w.sim.store, building) else { return true };
            let prop = ObjectHandle::Prop(handle.clone());
            let Ok(j) = rows.binary_search_by_key(&key(&prop), |row| key(&row.object)) else { return true };
            !rows[j].name.as_ref().is_some_and(|n| n.eq_ignore_ascii_case(rows[i].name.as_ref().unwrap()))
        });
        index
    }

    pub fn iter(&self) -> impl Iterator<Item=&NamedObject> { self.rows.iter() }

    pub fn find(&self, name: &str) -> RuleResult<&ObjectHandle> { self.find_bytes(name.as_bytes()) }

    /// A name as exact bytes (a script string), in the same order as `find`.
    pub(crate) fn find_bytes(&self, name: &[u8]) -> RuleResult<&ObjectHandle> {
        let cmp = |&i: &usize| name_order(self.rows[i].name.as_ref().unwrap().as_bytes(), name);
        let start = self.names.partition_point(|i| cmp(i).is_lt());
        let index = self.names.get(start).filter(|i| cmp(i).is_eq()).ok_or(RuleError::UnknownName)?;
        if self.names.get(start + 1).is_some_and(|i| cmp(i).is_eq()) {
            return Err(RuleError::AmbiguousName);
        }
        Ok(&self.rows[*index].object)
    }

    pub fn row(&self, object: &ObjectHandle) -> RuleResult<&NamedObject> {
        let index = self.rows.binary_search_by_key(&key(object), |row| key(&row.object)).map_err(|_| RuleError::NotAvailable)?;
        Ok(&self.rows[index])
    }
}
