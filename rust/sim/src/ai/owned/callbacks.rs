//! The native unit and seat phases carry an explicit AI callback loan. Each
//! invocation receives the active cross-owner resources; it cannot acquire
//! another Core or move a lifecycle action past its original source slot.
use crate::{battle::live::Queries,gen::{F3UnitKey,F4AIHook,SeatLease,SeatUnit,Store,Unit},recon::Authority,store::Ref,vehicles::f2_seats::Seats};
pub trait Callbacks {
    fn character_reassigned(&mut self,reference:Ref<crate::gen::Character>,handle:&crate::gen::F4Handle);
    fn hull_born(&mut self,hull:crate::store::Ref<crate::gen::Vehicle>,member:crate::gen::Member);
    fn grenade_contact(&mut self,grenade:crate::store::Ref<crate::gen::Grenade>);
    fn aim(&mut self,key:&F3UnitKey,dt:f64,recon:&mut Authority,queries:&mut Queries);
    fn lifecycle(&mut self,hook:&F4AIHook,recon:&mut Authority,queries:&mut Queries);
    fn controller_state(&self,store:&Store,unit:Option<Ref<Unit>>)->Option<f64>;
    fn owns_intent(&self,store:&Store,unit:crate::store::Ref<Unit>)->bool;
    fn weapon_hint(&self,store:&Store,unit:crate::store::Ref<Unit>,control:Option<crate::store::Ref<SeatUnit>>)->crate::gen::F2TurretHint;
    fn lease_created(&mut self,lease:Ref<SeatLease>);
    fn controllable(&mut self,store:&mut Store,seats:&mut Seats,lease:Ref<SeatLease>)->Option<Ref<SeatUnit>>;
}

/// Cold service facts can be shared through a recursive callback, while the
/// mutable recon/query owners are reborrowed for just that invocation.
pub trait Factory {
    type Services<'a>:super::world::Services<super::weapons::F3,super::arsenals::Live> where Self:'a;
    fn services<'a>(&'a self,recon:&'a mut Authority,queries:&'a mut Queries)->Self::Services<'a>;
}
pub struct Scope<'a,F:Factory> {
    pub core:&'a mut crate::ai::control::Core,
    pub state:&'a mut super::state::State,
    pub random:&'a mut crate::ai::phase::RandomState,
    pub combat:&'a mut crate::ai::combat::binding::CombatAi,
    pub settings:&'a super::frame::Settings,
    pub facts:&'a super::world::Facts,
    pub factory:&'a F,
}
impl<F:Factory> Callbacks for Scope<'_,F> {
    fn character_reassigned(&mut self,reference:Ref<crate::gen::Character>,handle:&crate::gen::F4Handle) {
        // Human admission has no later bot attach hook. Refresh its command
        // life at the F4 reassignment, before another controller reads crew,
        // followers or Enhanced population from the same Character object.
        if let Some(index)=self.state.roster.storage_index(reference) {self.state.roster.replace(index,handle.clone());}
    }
    fn hull_born(&mut self,hull:crate::store::Ref<crate::gen::Vehicle>,member:crate::gen::Member) {
        if let Some(entry)=self.state.mounts.activate_target(&self.state.entities,hull,member) {self.state.queries.target(member,entry);}
    }
    fn grenade_contact(&mut self,grenade:crate::store::Ref<crate::gen::Grenade>) {self.state.dangers.contact(grenade,self.facts.time);}
    fn aim(&mut self,key:&F3UnitKey,dt:f64,recon:&mut Authority,queries:&mut Queries) {
        let controller={
            let Ok(unit)=key.validate(&crate::abi::sim().store) else{return};
            unit.controller.and_then(|r|self.state.controllers.core_reference(r))
        };
        // Preserve identity admission even for a body with no bot controller.
        // Its canonical binding avoids a second per-unit Core map lookup.
        let object=self.state.entities.register(super::entities::Entity::Unit(key.reference()));
        let Some(controller)=controller else{return};
        if !self.core.controllers.get(controller).is_some_and(|c|c.unit==Some(object)) {return;}
        let mut store=std::mem::take(&mut crate::abi::sim().store);
        let mut services=self.factory.services(recon,queries);let mut random=self.random.share();
        super::live::with_rows(self.state,&mut store,|rows| {
            let mut world=super::world::World {rows,random:&mut random,combat:self.combat,services:&mut services,settings:self.settings,facts:self.facts};
            super::lifecycle::Callback {core:self.core,random:self.random}.aim_controller(&mut world,controller,dt);
        });
        crate::abi::sim().store=store;
    }
    fn lifecycle(&mut self,hook:&F4AIHook,recon:&mut Authority,queries:&mut Queries) {
        let mut store=std::mem::take(&mut crate::abi::sim().store);
        if let Some(handle)=hook.character_handle.as_ref() {
            if let Some(reference)=crate::battle::f4_core::resolve_character(&store,handle) {
                // Roster replacement advances F4 command life on the same
                // Character object. Its AI index and unit link stay stable.
                if let Some(index)=self.state.roster.storage_index(reference) {self.state.roster.replace(index,handle.clone());}else{
                    let index=self.state.roster.add(handle.clone());self.state.units.character(reference,index);
                }
                self.state.entities.register(super::entities::Entity::Character(reference));
                if let Some(unit)=store.character.get(reference).and_then(|c|c.unit) {self.state.entities.register(super::entities::Entity::Unit(unit));}
                let count=self.state.roster.handles().len();
                assert!(count<=self.core.by_character.len(),"admit native AI Character capacity before a roster birth");
                self.core.character_count=count;
            }
        }
        let mut services=self.factory.services(recon,queries);let mut random=self.random.share();
        super::live::with_rows(self.state,&mut store,|rows| {
            let mut world=super::world::World {rows,random:&mut random,combat:self.combat,services:&mut services,settings:self.settings,facts:self.facts};
            super::lifecycle::Callback {core:self.core,random:self.random}.battle_hook(&mut world,hook).expect("current native AI lifecycle handles");
        });
        crate::abi::sim().store=store;
    }
    fn controller_state(&self,store:&Store,unit:Option<Ref<Unit>>)->Option<f64> {
        let reference=store.unit.get(unit?)?.controller?;
        let object=self.state.entities.find(super::entities::Entity::Controller(reference))?;
        let index=self.state.controllers.core(&self.state.entities,object)?;
        Some(self.core.controllers.get(index)?.fsm.state)
    }
    fn owns_intent(&self,store:&Store,unit:crate::store::Ref<Unit>)->bool {
        let Some(body)=store.unit.get(unit) else{return false};
        let Some(object)=self.state.entities.find(super::entities::Entity::Unit(unit)) else{return false};
        self.core.controller_of_unit(object).is_some_and(|controller|self.core.controllers[controller].generation==body.generation)
    }
    fn weapon_hint(&self,store:&Store,unit:crate::store::Ref<Unit>,control:Option<crate::store::Ref<SeatUnit>>)->crate::gen::F2TurretHint {
        self.state.weapon_hint(store,unit,control,|member|crate::units::f3::target(crate::units::registry::target_id(member)).map(|target|target.life))
    }

    fn lease_created(&mut self,lease:Ref<SeatLease>) {
        // Human leases have no SeatUnit construction callback. They still
        // participate in crew, takeover and shared-target identity queries.
        self.state.entities.register(super::entities::Entity::Lease(lease));
    }
    fn controllable(&mut self,store:&mut Store,seats:&mut Seats,lease:Ref<SeatLease>)->Option<Ref<SeatUnit>> {
        self.state.construct_control(store,seats,&crate::battle::f4::live().buildings,lease)
    }
}

/// An AI action temporarily returns the physical owners to their native
/// modules. Recursive seat/death hooks then borrow this same Core and RNGs;
/// no Store, lease or AI row borrow spans the physical operation.
pub fn with_services<F:Factory,R>(
    factory:&F,core:&mut crate::ai::control::Core,
    rows:&mut super::rows::Rows<'_,super::weapons::F3,super::arsenals::Live>,
    random:&mut crate::ai::phase::RandomState,combat:&mut crate::ai::combat::binding::CombatAi,
    settings:&super::frame::Settings,facts:&super::world::Facts,
    recon:&mut Authority,queries:&mut Queries,
    run:impl FnOnce(crate::units::f3::WorldServices<'_>)->R)->R {
    let mut scope=Scope {core,state:rows.state,random,combat,settings,facts,factory};
    crate::vehicles::module::f2::live_seats::loan_seats(rows.seats,||
        crate::battle::f4_live::with_store(rows.store,||
            run(crate::units::f3::WorldServices {recon,queries,ai:Some(&mut scope)})))
}
