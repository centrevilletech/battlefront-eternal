//! The native unit and seat phases carry an explicit AI callback loan. Each
//! invocation receives the active cross-owner resources; it cannot acquire
//! another Core or move a lifecycle action past its original source slot.
use crate::{battle::live::Queries,gen::{F3UnitKey,F4AIHook,SeatLease,SeatUnit,Store,Unit},recon::Authority,store::Ref};
pub trait Callbacks {
    fn unit_born(&mut self,unit:Ref<Unit>)->bool;
    fn character_reassigned(&mut self,reference:Ref<crate::gen::Character>,handle:&crate::gen::F4Handle);
    fn hull_born(&mut self,hull:crate::store::Ref<crate::gen::Vehicle>,member:crate::gen::Member);
    fn grenade_contact(&mut self,grenade:crate::store::Ref<crate::gen::Grenade>);
    fn aim_in(&mut self,w:&mut crate::world::owner::World,key:&F3UnitKey,dt:f64,recon:&mut Authority,queries:&mut Queries);
    fn lifecycle_in(&mut self,w:&mut crate::world::owner::World,hook:&F4AIHook,recon:&mut Authority,queries:&mut Queries);
    fn controller_state(&self,store:&Store,unit:Option<Ref<Unit>>)->Option<f64>;
    fn owns_intent(&self,store:&Store,unit:crate::store::Ref<Unit>)->bool;
    fn weapon_hint_in(&self,w:&mut crate::world::owner::World,unit:crate::store::Ref<Unit>,control:Option<crate::store::Ref<SeatUnit>>)->crate::gen::F2TurretHint;
    fn lease_created(&mut self,lease:Ref<SeatLease>);
    fn controllable(&mut self,w:&mut crate::world::owner::World,lease:Ref<SeatLease>)->Option<Ref<SeatUnit>>;
    /// One tick of the goal bots, for event rules with a goal table only.
    fn goal_pass(&mut self,_w:&mut crate::world::owner::World,_recon:&mut Authority,_queries:&mut Queries,
        _bots:&mut crate::ai::goals::GoalController,_table:&dyn crate::ai::goals::GoalTable,_dt:f64) {}
    fn soldier_inputs(&mut self,_w:&mut crate::world::owner::World,_recon:&mut Authority,_queries:&mut Queries,
        _inputs:&mut crate::ai::goals::SoldierInputs,_dt:f64) {}

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
    fn unit_born(&mut self,unit:Ref<Unit>)->bool {self.state.entities.try_register(super::entities::Entity::Unit(unit)).is_some()}
    fn character_reassigned(&mut self,reference:Ref<crate::gen::Character>,handle:&crate::gen::F4Handle) {
        // Human admission has no later bot attach hook. Refresh its command
        // life at the F4 reassignment, before another controller reads crew,
        // followers or Enhanced population from the same Character object.
        self.state.bind_character(reference,handle);
        let count=self.state.roster.handles().len();
        assert!(count<=self.core.by_character.len(),"admit native AI Character capacity before a roster birth");
        self.core.character_count=count;
    }
    fn hull_born(&mut self,hull:crate::store::Ref<crate::gen::Vehicle>,member:crate::gen::Member) {
        if let Some(entry)=self.state.mounts.activate_target(&self.state.entities,hull,member) {self.state.queries.target(member,entry);}
    }
    fn grenade_contact(&mut self,grenade:crate::store::Ref<crate::gen::Grenade>) {self.state.dangers.contact(grenade,self.facts.time);}
    fn aim_in(&mut self,w:&mut crate::world::owner::World,key:&F3UnitKey,dt:f64,recon:&mut Authority,queries:&mut Queries) {
        let controller={
            let Ok(unit)=key.validate(&w.sim.store) else{return};
            unit.controller.and_then(|r|self.state.controllers.core_reference(r))
        };
        // Preserve identity admission even for a body with no bot controller.
        // Its canonical binding avoids a second per-unit Core map lookup.
        let Some(object)=self.state.entities.try_register(super::entities::Entity::Unit(key.reference())) else{return};
        let Some(controller)=controller else{return};
        if !self.core.controllers.get(controller).is_some_and(|c|c.unit==Some(object)) {return;}
        let mut services=self.factory.services(recon,queries);let mut random=self.random.share();
        super::live::with_rows(w,self.state,|rows| {
            let mut world=super::world::World {rows,random:&mut random,combat:self.combat,services:&mut services,settings:self.settings,facts:self.facts};
            super::lifecycle::Callback {core:self.core,random:self.random}.aim_controller(&mut world,controller,dt);
        });

    }
    fn lifecycle_in(&mut self,w:&mut crate::world::owner::World,hook:&F4AIHook,recon:&mut Authority,queries:&mut Queries) {
        if let Some(handle)=hook.character_handle.as_ref() {
            if let Some(reference)=crate::battle::f4_core::resolve_character_ref(&w.battle,&w.sim.store,handle) {
                // Roster replacement advances F4 command life on the same
                // Character object. Its AI index and unit link stay stable.
                self.character_reassigned(reference,handle);
                if let Some(unit)=w.sim.store.character.get(reference).and_then(|c|c.unit) {self.state.entities.register(super::entities::Entity::Unit(unit));}
            }
        }
        let mut services=self.factory.services(recon,queries);let mut random=self.random.share();
        super::live::with_rows(w,self.state,|rows| {
            let mut world=super::world::World {rows,random:&mut random,combat:self.combat,services:&mut services,settings:self.settings,facts:self.facts};
            super::lifecycle::Callback {core:self.core,random:self.random}.battle_hook(&mut world,hook).expect("current native AI lifecycle handles");
        });

    }
    fn controller_state(&self,store:&Store,unit:Option<Ref<Unit>>)->Option<f64> {
        let reference=store.unit.get(unit?)?.controller?;
        let index=self.state.controllers.core_reference(reference)?;
        Some(self.core.controllers.get(index)?.fsm.state)
    }
    fn owns_intent(&self,store:&Store,unit:crate::store::Ref<Unit>)->bool {
        let Some(body)=store.unit.get(unit) else{return false};
        // The unit's qualified binding reaches the native row directly. The
        // Core's bound object and gameplay life still grant ownership; a
        // foreign or recycled controller cannot acquire a missing input row.
        let Some(controller)=body.controller.and_then(|r|self.state.controllers.core_reference(r)) else{return false};
        self.core.controllers.get(controller).is_some_and(|controller|controller.generation==body.generation&&
            controller.unit.and_then(|object|self.state.entities.get(object))==Some(super::entities::Entity::Unit(unit)))
    }
    fn weapon_hint_in(&self,w:&mut crate::world::owner::World,unit:crate::store::Ref<Unit>,control:Option<crate::store::Ref<SeatUnit>>)->crate::gen::F2TurretHint {
        self.state.weapon_hint_in(w,unit,control)
    }

    fn lease_created(&mut self,lease:Ref<SeatLease>) {
        // Human leases have no SeatUnit construction callback. They still
        // participate in crew, takeover and shared-target identity queries.
        self.state.entities.register(super::entities::Entity::Lease(lease));
    }
    fn controllable(&mut self,w:&mut crate::world::owner::World,lease:Ref<SeatLease>)->Option<Ref<SeatUnit>> {
        self.state.construct_control_in(w,lease)
    }
    fn goal_pass(&mut self,w:&mut crate::world::owner::World,recon:&mut Authority,queries:&mut Queries,
        bots:&mut crate::ai::goals::GoalController,table:&dyn crate::ai::goals::GoalTable,dt:f64) {
        self.event_bots(w, recon, queries, EventBots::Goals(bots, table), dt);
    }
    fn soldier_inputs(&mut self,w:&mut crate::world::owner::World,recon:&mut Authority,queries:&mut Queries,
        inputs:&mut crate::ai::goals::SoldierInputs,dt:f64) {
        self.event_bots(w, recon, queries, EventBots::Soldiers(inputs), dt);
    }
}

enum EventBots<'a> {
    Goals(&'a mut crate::ai::goals::GoalController, &'a dyn crate::ai::goals::GoalTable),
    Soldiers(&'a mut crate::ai::goals::SoldierInputs),
}

impl<F:Factory> Scope<'_,F> {
    fn event_bots(&mut self,w:&mut crate::world::owner::World,recon:&mut Authority,queries:&mut Queries,
        bots:EventBots<'_>,dt:f64) {
        let mut services=self.factory.services(recon,queries);let mut random=self.random.share();
        super::live::with_rows(w,self.state,|rows| {
            let mut world=super::world::World {rows,random:&mut random,combat:self.combat,services:&mut services,settings:self.settings,facts:self.facts};
            let core = crate::ai::control::goal_orders::GoalCore::new(self.core);
            match bots {
                EventBots::Goals(bots, table) => bots.pass(core, &mut world, table, dt),
                EventBots::Soldiers(inputs) => inputs.pass(core, &mut world, dt),
            }
        });
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
    run:impl FnOnce(&mut crate::world::owner::World,crate::units::f3::WorldServices<'_>)->R)->R {
    let mut scope=Scope {core,state:rows.state,random,combat,settings,facts,factory};
    run(rows.w,crate::units::f3::WorldServices {recon,queries,ai:Some(&mut scope)})
}
