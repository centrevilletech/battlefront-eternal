//! Control/combat identity mapping over native Character and post owners.
//! The numeric token always names an Entities row. Roster indices and slab
//! slots are independent and never become object identities by arithmetic.
use super::{
    entities::{Entities, Entity},
    population::Population,
    roster::{CharacterOwner, Roster},
};
use crate::{
    ai::{
        combat::{control::Objects as CombatObjects, world::Obj},
        control::world::{AlertSource, UKey},
    },
    gen::{SpawnPost, Store},
};

pub struct Objects<'a> {
    pub store: &'a Store,
    pub commands: &'a dyn CharacterOwner,
    pub entities: &'a Entities,
    pub roster: &'a Roster,
    pub population: &'a Population,
}
impl Objects<'_> {
    /// Cold construction or an authored-post insertion prepares equality keys.
    pub fn prepare(
        store: &Store,
        commands: &dyn CharacterOwner,
        entities: &mut Entities,
        roster: &Roster,
        population: &Population,
    ) {
        for index in 0..roster.handles().len() {
            entities.register(Entity::Character(
                roster
                    .reference(store, commands, index)
                    .expect("current AI roster life"),
            ));
        }
        let mut index = 0;
        while let Some(post) = population.post(index) {
            entities.register(post_entity(post));
            index += 1;
        }
    }
}
pub(super) fn post_entity(post: SpawnPost) -> Entity {
    match post {
        SpawnPost::Post(r) => Entity::Post(r),
        SpawnPost::MobilePost(r) => Entity::MobilePost(r),
    }
}
impl CombatObjects for Objects<'_> {
    fn unit(&self, key: UKey) -> Obj {
        // A controller can retain a retired SeatUnit until the next sync.
        // Keep its qualified token: scalar readers resolve it as dead, and
        // never alias a replacement that reuses the entity slot.
        key
    }
    fn character(&self, index: usize) -> Obj {
        let r = self
            .roster
            .reference(self.store, self.commands, index)
            .expect("current native AI Character");
        self.entities
            .find(Entity::Character(r))
            .expect("prepared AI Character identity")
    }
    fn post(&self, index: usize) -> Obj {
        let post = self.population.post(index).expect("authored AI post");
        self.entities
            .find(post_entity(post))
            .expect("prepared AI post identity")
    }
    fn unit_key(&self, obj: Obj) -> UKey {
        self.unit(obj)
    }
    fn alert_source(&self, obj: Obj) -> AlertSource {
        match self
            .entities
            .get(obj)
            .expect("current native AI alert source")
        {
            Entity::Character(r) => AlertSource::Character(
                self.roster
                    .index(self.store, self.commands, r)
                    .expect("current AI alert Character"),
            ),
            _ => AlertSource::Unit(obj),
        }
    }
}
