//! Class jobs inspect the canonical ordered item list. Service quantities and
//! definition names are loaded once, while positions and ownership stay live.
use super::entities::{Entities, Entity};
use crate::ai::{
    control::world::{ItemSnap, QtySnap},
    nav::math::Vec3,
};
use crate::{
    gen::{ItemOwner, Store},
    store::DefId,
};
use std::rc::Rc;
struct Definition {
    base: Option<Rc<str>>,
    class: Option<Rc<str>>,
    quantity: QtySnap,
}
pub struct Items {
    definitions: Vec<Option<Definition>>,
}
impl Items {
    pub fn new() -> Self {
        Self {
            definitions: Vec::new(),
        }
    }
    pub fn class(
        &mut self,
        id: DefId,
        base: Option<Rc<str>>,
        class: Option<Rc<str>>,
        quantity: QtySnap,
    ) {
        let i = id.0 as usize;
        if self.definitions.len() <= i {
            self.definitions.resize_with(i + 1, || None);
        }
        self.definitions[i] = Some(Definition {
            base,
            class,
            quantity,
        });
    }
    pub fn len(&self, store: &Store) -> usize {
        store.items.items.len()
    }
    pub fn read(&self, store: &Store, entities: &Entities, index: usize) -> ItemSnap {
        let r = store.items.items[index];
        let item = store.item.get(r).expect("current ordered AI item");
        let class = self.definitions[item.def.0 as usize]
            .as_ref()
            .expect("prepared AI item definition");
        ItemSnap {
            gone: item.gone == Some(true),
            mine: item.mine,
            def_base: class.base.clone(),
            def_class: class.class.clone(),
            position: Vec3::new(item.position[0], item.position[1], item.position[2]),
            radius: Some(item.radius),
            team: item.team,
            owner: match item.owner {
                ItemOwner::Unit(unit) => entities.find(Entity::Unit(unit)),
                _ => None,
            },
            qty: class.quantity.clone(),
        }
    }
}
