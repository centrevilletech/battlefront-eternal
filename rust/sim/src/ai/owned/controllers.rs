//! Combat state belongs to native controllers across unit lives. Binding a
//! new life renews perception values while retaining its heap allocations.
use super::{
    entities::{Entities, Entity},
    scratch::{List, Pool},
};
use crate::ai::{
    combat::{
        control::Objects,
        perception::Perception,
        world::{Ctl, Obj},
    },
    control::world::{AlertState, CombatInput, CombatWrites},
};
use crate::store::{Ref, Slab};
use std::cell::Cell;
struct Row {
    core: usize,
    character_index:Option<usize>,
    los_seen:Cell<u32>,
    helper_kind:Option<crate::ai::control::world::HelperKind>,
    helper_has_path:bool,
    policy: crate::ai::control::policy::Input,
    memory: crate::ai::control::controller::EnhancedMemory,
    value: Ctl,
    perception: Option<Perception>,
}
pub struct Controllers {
    rows: Slab<Row>,
    by_core: super::index::Index<Row>,
    spare: Vec<Row>,
    alerts: Pool<AlertState>,
    los_epoch:Cell<u32>,
}
impl Controllers {
    pub fn new(capacity: usize, depth: usize) -> Self {
        let spare = (0..capacity)
            .map(|_| Row {
                core: 0,character_index:None,los_seen:Cell::new(0),helper_kind:None,helper_has_path:false,
                policy: crate::ai::control::policy::Input::default(),
                memory: {
                    let mut memory = crate::ai::control::controller::EnhancedMemory::new();
                    memory.reserve(capacity.saturating_mul(4));
                    memory
                },
                value: Ctl {
                    acoustic: Vec::with_capacity(20),
                    ..Ctl::default()
                },
                perception: Some(Perception::default()),
            })
            .collect();
        Self {
            rows: Slab::with_capacity(capacity),
            by_core: super::index::Index::new(capacity),
            spare,
            alerts: Pool::new(depth, capacity),los_epoch:Cell::new(0),
        }
    }
    pub fn adopt(&mut self, core: usize, entities: &mut Entities) -> Obj {
        if let Some(r) = self.by_core.get(core) {
            return self.rows.get(r).unwrap().value.obj;
        }
        let mut row = self.spare.pop().expect("prepared AI controller capacity");
        row.core = core;row.character_index=None;
        // A spare row can outlive a complete sight-epoch cycle. Its new
        // storage generation must never inherit that old visited mark.
        row.los_seen.set(0);
        let r = self.rows.insert(row);
        let obj = entities.register(Entity::Controller(Ref::new(r.slot, r.gen)));
        self.rows.get_mut(r).unwrap().value.obj = obj;
        self.by_core.insert(core,r);
        // Character and body publications can reference the private AI slab.
        // Establish only its facade identity before either record is decoded.
        #[cfg(target_arch="wasm32")]
        crate::gen::AiControllerBirth {slot:r.slot,generation:r.gen,index:core as u32}.emit(&mut crate::abi::sim().events);
        obj
    }
    fn reference(entities: &Entities, key: Obj) -> Option<Ref<Row>> {
        let Entity::Controller(r) = entities.get(key)? else {
            return None;
        };
        Some(Ref::new(r.slot, r.gen))
    }
    pub fn read(&self, entities: &Entities, key: Obj) -> Option<&Ctl> {
        Some(&self.rows.get(Self::reference(entities, key)?)?.value)
    }
    pub fn write(&mut self, entities: &Entities, key: Obj) -> Option<&mut Ctl> {
        Some(&mut self.rows.get_mut(Self::reference(entities, key)?)?.value)
    }
    pub fn key(&self, core: usize) -> Option<Obj> {
        Some(self.rows.get(self.by_core.get(core)?)?.value.obj)
    }
    /// A canonical Unit already carries its qualified controller reference.
    /// Post-movement aim can resolve that slab directly without an entity map.
    pub fn core_reference(&self,reference:Ref<crate::gen::Controller>)->Option<usize> {
        Some(self.rows.get(Ref::new(reference.slot,reference.gen))?.core)
    }
    pub fn core(&self,entities:&Entities,key:Obj)->Option<usize> {Some(self.rows.get(Self::reference(entities,key)?)?.core)}
    pub fn helper_created(&mut self,core:usize,helper:crate::ai::control::agents::Helper) {
        let reference=self.by_core.get(core).expect("prepared native helper controller");
        let row=self.rows.get_mut(reference).unwrap();row.helper_kind=helper.kind;
        row.helper_has_path=helper.has_path_property();
    }
    pub fn helper(&self,entities:&Entities,key:Obj)->Option<(Option<crate::ai::control::world::HelperKind>,bool)> {
        let row=self.rows.get(Self::reference(entities,key)?)?;Some((row.helper_kind,row.helper_has_path))
    }
    pub fn feed(
        &mut self,
        entities: &Entities,
        key: Obj,
        input: &CombatInput<'_>,
        objects: &impl Objects,
    ) -> bool {
        let Some(row) = Self::reference(entities, key).and_then(|r| self.rows.get_mut(r)) else {
            return false;
        };
        row.helper_kind=input.helper_kind;row.helper_has_path=input.helper_has_path;
        row.policy = input.policy;
        if row.character_index!=Some(input.source.character) {
            row.value.character=Some(objects.character(input.source.character));
            row.character_index=Some(input.source.character);
        }
        row.value.feed_requests(input, objects);
        true
    }
    /// Only queued owners lend requests to deferred sight. Resolve storage
    /// identities before deduplication, but leave gameplay-life checks until
    /// service sees the freshly fed controller. A reused slab cannot inherit
    /// an old queued owner, and a full output slice degrades without growth.
    pub fn los_clients(&self,entities:&Entities,los:&crate::ai::combat::perception::DeferredLos,out:&mut [usize])->usize {
        let mut epoch=self.los_epoch.get().wrapping_add(1);
        if epoch==0 {
            // Zero is never an active scan. A wrap must not let a long-idle
            // controller inherit a visit from the previous epoch cycle.
            for (_,row) in self.rows.iter() {row.los_seen.set(0);}
            epoch=1;
        }
        self.los_epoch.set(epoch);
        let mut count=0;
        for request in los.active.items.iter().skip(1).take(los.active.count).flatten() {
            let Some(row)=Self::reference(entities,request.controller).and_then(|r|self.rows.get(r)) else{continue};
            if row.los_seen.get()!=epoch&&count<out.len() {
                row.los_seen.set(epoch);out[count]=row.core;count+=1;
            }
        }
        count
    }
    pub fn policy(
        &self,
        entities: &Entities,
        key: Obj,
    ) -> Option<crate::ai::control::policy::Input> {
        Some(self.rows.get(Self::reference(entities, key)?)?.policy)
    }
    pub fn state_changed(&mut self, core: usize, state: f64) {
        if let Some(r) = self.by_core.get(core) {
            self.rows.get_mut(r).unwrap().value.controller_state = state;
        }
    }
    pub fn fighting(&self, entities: &Entities, key: Obj) -> bool {
        Self::reference(entities, key)
            .and_then(|r| self.rows.get(r))
            .is_some_and(|row| row.value.controller_state == 2.)
    }
    pub fn new_memory(
        &mut self,
        entities: &Entities,
        key: Obj,
    ) -> Option<crate::ai::control::controller::EnhancedMemory> {
        let row = self.rows.get_mut(Self::reference(entities, key)?)?;
        row.memory.renew();
        Some(row.memory.share())
    }
    pub fn writes(
        &self,
        entities: &Entities,
        key: Obj,
        objects: &impl Objects,
    ) -> Option<CombatWrites> {
        Some(self.read(entities, key)?.control_writes(objects))
    }
    pub fn fresh_perception(&mut self, entities: &Entities, key: Obj) -> Option<Perception> {
        let row = self.rows.get_mut(Self::reference(entities, key)?)?;
        let mut p = row
            .value
            .perception
            .take()
            .or_else(|| row.perception.take())
            .expect("prepared native perception");
        p.renew();
        Some(p)
    }
    pub fn ingest(&self, objects: &impl Objects) -> List<AlertState> {
        let mut out = self.alerts.take();
        for r in self.by_core.values() {
            let row = self.rows.get(r).unwrap();
            out.push_reserved(row.value.control_alerts(row.core, objects));
        }
        out
    }
    pub fn retire(&mut self, entities: &mut Entities, key: Obj) -> bool {
        let Some(r) = Self::reference(entities, key) else {
            return false;
        };
        let Some(mut row) = self.rows.remove(r) else {
            return false;
        };
        entities.retire(key);
        self.by_core.remove(row.core);
        let mut acoustic = std::mem::take(&mut row.value.acoustic);
        acoustic.clear();
        row.perception = row.value.perception.take().or(row.perception);
        row.helper_kind=None;row.helper_has_path=false;
        row.policy = crate::ai::control::policy::Input::default();
        row.memory.renew();
        row.value = Ctl {
            acoustic,
            ..Ctl::default()
        };
        self.spare.push(row);
        true
    }
}

/// Bind only controller metadata, preserving every solver and intent field.
/// The caller marks the F3 body only when the returned bit says it changed.
pub fn link_unit(store:&mut crate::gen::Store,entities:&Entities,unit:Obj,controller:Obj,bound:bool)->Option<bool> {
    let Entity::Unit(unit)=entities.get(unit)? else{return None};
    let Entity::Controller(controller)=entities.get(controller)? else{return None};
    let row=store.unit.get_mut(unit)?;let before=row.controller;
    if bound {row.controller=Some(controller);}else if row.controller==Some(controller) {row.controller=None;}
    Some(before!=row.controller)
}

/// ClassicAI.attach/detach links the Character even while it has no body.
/// Its owning F4 life resolves before the link changes or is published.
pub fn link_character(store:&mut crate::gen::Store,owner:&mut dyn super::roster::CharacterOwner,
    roster:&super::roster::Roster,entities:&Entities,index:usize,controller:Obj,bound:bool)->Option<bool> {
    let reference=roster.reference(store,owner,index)?;
    let Entity::Controller(controller)=entities.get(controller)? else {return None};
    let character=store.character.get_mut(reference)?;let before=character.controller;
    if bound {character.controller=Some(controller);}else if before==Some(controller) {character.controller=None;}
    let changed=before!=character.controller;
    if changed {owner.changed(reference);}
    Some(changed)
}

#[cfg(test)]
mod los_epochs {
    use super::*;
    #[test]
    fn character_bindings_resolve_once_and_reset_on_storage_reuse() {
        struct Ids(Cell<usize>);
        impl Objects for Ids {
            fn unit(&self,key:crate::ai::control::world::UKey)->Obj {key}
            fn character(&self,key:usize)->Obj {self.0.set(self.0.get()+1);100+key as Obj}
            fn post(&self,key:usize)->Obj {200+key as Obj}
            fn unit_key(&self,key:Obj)->crate::ai::control::world::UKey {key}
            fn alert_source(&self,key:Obj)->crate::ai::control::world::AlertSource {crate::ai::control::world::AlertSource::Unit(key)}
        }
        let mut entities=Entities::new(1);let mut rows=Controllers::new(1,1);let ids=Ids(Cell::new(0));
        let old=rows.adopt(7,&mut entities);let c=crate::ai::control::controller::Controller::new(2);
        let input=CombatInput::read(&c,None);
        let Entity::Controller(reference)=entities.get(old).unwrap() else{panic!("controller identity")};
        assert_eq!(rows.core_reference(reference),Some(7));
        assert!(rows.feed(&entities,old,&input,&ids));assert!(rows.feed(&entities,old,&input,&ids));
        assert_eq!(ids.0.get(),1);assert_eq!(rows.read(&entities,old).unwrap().character,Some(102));
        assert!(rows.retire(&mut entities,old));assert_eq!(rows.core_reference(reference),None);
        let new=rows.adopt(7,&mut entities);
        assert_ne!(new,old);assert!(rows.feed(&entities,new,&input,&ids));assert_eq!(ids.0.get(),2);
    }
    #[test]
    fn a_reused_spare_cannot_inherit_a_sight_mark_after_wrap() {
        let mut entities=Entities::new(1);let mut rows=Controllers::new(1,1);
        let old=rows.adopt(7,&mut entities);
        let mut los=crate::ai::combat::perception::DeferredLos::default();
        los.submit(old,1.,None,10.);let mut out=[0;1];
        assert_eq!(rows.los_clients(&entities,&los,&mut out),1);
        assert_eq!(rows.los_clients(&entities,&los,&mut out),1);
        assert!(rows.retire(&mut entities,old));
        rows.los_epoch.set(u32::MAX);
        assert_eq!(rows.los_clients(&entities,&los,&mut out),0);
        let new=rows.adopt(100,&mut entities);assert_ne!(new,old);
        los.submit(new,1.,None,20.);
        assert_eq!(rows.los_clients(&entities,&los,&mut out),1);
        assert_eq!(out,[100]);assert_eq!(rows.los_epoch.get(),2);
    }
    #[test]
    fn a_wrapped_sight_epoch_keeps_every_qualified_client() {
        let mut entities=Entities::new(2);let mut rows=Controllers::new(2,1);
        let first=rows.adopt(7,&mut entities);let second=rows.adopt(100,&mut entities);
        let mut los=crate::ai::combat::perception::DeferredLos::default();
        los.submit(first,1.,None,10.);los.submit(second,1.,None,20.);
        let mut out=[0;2];assert_eq!(rows.los_clients(&entities,&los,&mut out),2);
        rows.los_epoch.set(u32::MAX);
        assert_eq!(rows.los_clients(&entities,&los,&mut out),2);
        assert!(out.contains(&7)&&out.contains(&100));
        assert_eq!(rows.los_epoch.get(),1);
        assert_eq!(rows.los_clients(&entities,&los,&mut out),2);
    }
}
