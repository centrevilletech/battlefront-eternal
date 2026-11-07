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
    shots:Pool<crate::ai::control::world::ShotAlert>,
    intake:Option<List<crate::ai::control::world::ShotAlert>>,
    shot_marks:Vec<Option<(u32,usize)>>,
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
            shots:Pool::new(depth,capacity),intake:None,shot_marks:vec![None;capacity],
        }
    }

    pub(crate) fn adopt_in(&mut self,w:&mut crate::world::owner::World,core:usize,entities:&mut Entities)->Obj {
        let born=self.key(core).is_none();
        let object=self.adopt(core,entities);
        if born {
            let Entity::Controller(r)=entities.get(object).expect("admitted AI controller") else{unreachable!()};
            crate::gen::AiControllerBirth {slot:r.slot,generation:r.gen,index:core as u32}.emit(&mut w.sim.events);
        }
        object
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
    /// Core callers already have the prepared index. Its stored slab
    /// reference still validates generation, without another entity lookup.
    pub fn read_core(&self,core:usize)->Option<&Ctl> {
        Some(&self.rows.get(self.by_core.get(core)?)?.value)
    }
    pub fn write_core(&mut self,core:usize)->Option<&mut Ctl> {
        Some(&mut self.rows.get_mut(self.by_core.get(core)?)?.value)
    }
    pub fn key(&self, core: usize) -> Option<Obj> {
        Some(self.rows.get(self.by_core.get(core)?)?.value.obj)
    }
    /// A canonical Unit already carries its qualified controller reference.
    /// Post-movement aim can resolve that slab directly without an entity map.
    pub fn core_reference(&self,reference:Ref<crate::gen::Controller>)->Option<usize> {
        Some(self.rows.get(Ref::new(reference.slot,reference.gen))?.core)
    }
    pub fn read_reference(&self,reference:Ref<crate::gen::Controller>)->Option<&Ctl> {
        Some(&self.rows.get(Ref::new(reference.slot,reference.gen))?.value)
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
        Self::feed_row(row,input,objects)
    }
    pub fn feed_core(&mut self,core:usize,input:&CombatInput<'_>,objects:&impl Objects)->bool {
        let Some(reference)=self.by_core.get(core) else{return false};
        let Some(row)=self.rows.get_mut(reference) else{return false};
        Self::feed_row(row,input,objects)
    }
    fn feed_row(row:&mut Row,input:&CombatInput<'_>,objects:&impl Objects)->bool {
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
    /// Core numbers can outlive many joins. Refresh only retained native
    /// bindings, so a long session never scans every retired controller.
    pub fn shot_targets(&mut self,core:&crate::ai::control::Core) {
        for reference in self.by_core.values() {
            let row=self.rows.get_mut(reference).expect("retained controller binding");
            if let Some(source)=core.controllers.get(row.core) {
                row.value.target=source.target.map(|r|crate::ai::combat::world::Ref {obj:r.unit,generation:r.generation});
            }
        }
    }
    pub fn begin_shots(&mut self) {self.shot_marks.fill(None);self.intake=Some(self.shots.take());}
    pub fn finish_shots(&mut self)->List<crate::ai::control::world::ShotAlert> {self.intake.take().expect("active shot intake")}
    pub fn capturing_shots(&self)->bool {self.intake.is_some()}
    /// notifyShot always replaces source, while only a known point replaces
    /// the old known point. One slot per live controller bounds even a paused
    /// journal burst; ingestion creates no controllers or recursive callbacks.
    pub fn shot(&mut self,entities:&Entities,key:Obj,source:crate::ai::control::world::AlertSource,known:bool,point:Option<three::Vector3>) {
        let reference=Self::reference(entities,key).expect("current alerted controller");
        let core=self.rows.get(reference).expect("current alerted controller").core;
        let notes=self.intake.as_mut().expect("active shot intake");
        let mark=&mut self.shot_marks[reference.slot as usize];
        let point=if known {point.map(|p|crate::ai::nav::math::Vec3::new(p.x,p.y,p.z))}else{None};
        if let Some((generation,index))=*mark {if generation==reference.gen {
            notes[index].source=source;if point.is_some() {notes[index].point=point;}return;
        }}
        *mark=Some((reference.gen,notes.len()));
        notes.push_reserved(crate::ai::control::world::ShotAlert {controller:core,source,point});
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
pub fn link_character(context:super::rows::WriteContext<'_>,owner:&mut dyn super::roster::CharacterOwner,
    roster:&super::roster::Roster,entities:&Entities,index:usize,controller:Obj,bound:bool)->Option<bool> {
    let reference=roster.reference(&*context,owner,index)?;
    let Entity::Controller(controller)=entities.get(controller)? else {return None};
    let character=super::rows::context_store_mut!(context).character.get_mut(reference)?;let before=character.controller;
    if bound {character.controller=Some(controller);}else if before==Some(controller) {character.controller=None;}
    let changed=before!=character.controller;
    if changed {
         owner.changed_in(context,reference);

    }
    Some(changed)
}

#[cfg(test)]
mod los_epochs {
    use super::*;
    #[test]
    fn shot_bursts_coalesce_without_losing_the_last_known_point_or_source() {
        use crate::ai::control::{controller::Controller,world::AlertSource};
        use crate::ai::nav::math::Vec3;
        let mut entities=Entities::new(2);let mut rows=Controllers::new(2,1);
        let first=rows.adopt(7,&mut entities);let second=rows.adopt(99,&mut entities);
        let mut expected=[Controller::new(0),Controller::new(1)];
        rows.begin_shots();
        for index in 0..4096 {
            let at=index%2;let source=AlertSource::Unit(index as u64);
            let known=index%3==0;let point=(index%5!=0).then_some(Vec3::new(index as f64,-0.,4.));
            expected[at].notify_shot(Some(source),known,point);
            rows.shot(&entities,if at==0 {first}else{second},source,known,point.map(|p|three::Vector3::new(p.x,p.y,p.z)));
        }
        let notes=rows.finish_shots();assert_eq!(notes.len(),2);
        for note in &notes {
            let at=if note.controller==7 {0}else{1};let mut actual=Controller::new(at);
            actual.notify_shot(Some(note.source),note.point.is_some(),note.point);
            assert_eq!(actual.shot_pending,expected[at].shot_pending);
            assert_eq!(actual.shot_known,expected[at].shot_known);
            assert_eq!(actual.shot_point,expected[at].shot_point);
            assert_eq!(actual.shot_source,expected[at].shot_source);
        }
        drop(notes);assert!(rows.retire(&mut entities,first));
        let replacement=rows.adopt(1000,&mut entities);assert_ne!(replacement,first);
        rows.begin_shots();rows.shot(&entities,replacement,AlertSource::Character(1),false,None);
        let notes=rows.finish_shots();assert_eq!(notes.len(),1);assert_eq!(notes[0].controller,1000);
        assert_eq!(notes[0].source,AlertSource::Character(1));assert!(notes[0].point.is_none());
    }
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
        rows.write_core(7).unwrap().fire_a=true;
        assert!(rows.read_core(7).unwrap().fire_a);
        assert!(rows.retire(&mut entities,old));assert_eq!(rows.core_reference(reference),None);
        assert!(rows.read_core(7).is_none());assert!(rows.write_core(7).is_none());
        let new=rows.adopt(7,&mut entities);
        assert_eq!(rows.read_core(7).unwrap().obj,new);assert!(!rows.read_core(7).unwrap().fire_a);
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
