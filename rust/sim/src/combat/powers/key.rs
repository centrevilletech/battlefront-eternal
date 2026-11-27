//! A copyable name for one soldier life.

use crate::gen::{F3UnitKey, Store, Unit};
use crate::store::Ref;

/// One soldier life: unit slot, slab lease and gameplay life. A respawned
/// body in the same slot is a different life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LifeKey {
    pub slot: u32,
    pub generation: u32,
    pub life: f64,
}

impl LifeKey {
    pub fn of(reference: Ref<Unit>, unit: &Unit) -> Self {
        Self { slot: reference.slot, generation: reference.gen, life: unit.generation }
    }

    pub fn reference(&self) -> Ref<Unit> {
        Ref::new(self.slot, self.generation)
    }

    /// The unit while it still holds this life.
    pub fn unit<'a>(&self, store: &'a Store) -> Option<&'a Unit> {
        store.unit.get(self.reference()).filter(|u| u.generation == self.life)
    }

    pub fn unit_mut<'a>(&self, store: &'a mut Store) -> Option<&'a mut Unit> {
        store.unit.get_mut(self.reference()).filter(|u| u.generation == self.life)
    }
}

impl From<&F3UnitKey> for LifeKey {
    fn from(key: &F3UnitKey) -> Self {
        Self { slot: key.slot, generation: key.generation, life: key.life }
    }
}

impl From<LifeKey> for F3UnitKey {
    fn from(key: LifeKey) -> Self {
        F3UnitKey { slot: key.slot, generation: key.generation, life: key.life }
    }
}
