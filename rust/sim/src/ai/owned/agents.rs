//! AI agent data is retained independently of canonical bodies. Replacement
//! advances the agent identity even when the controller and unit stay alive.
use super::entities::{Entities, Entity};
use crate::{
    ai::{
        combat::{agent::CombatAgent, perception::TargetTracker, world::Obj},
        vehicles::agents::{BoardAgent, UnBoardAgent},
    },
    store::{Ref, Slab},
};
/// Typed identity only; the corresponding row belongs to Agents.
pub struct Identity;
#[derive(Clone, Debug)]
pub enum Behavior {
    Base,
    Combat(CombatAgent),
    Board(BoardAgent),
    Unboard(UnBoardAgent),
    Fly,
}
pub struct Agent {
    pub controller: Obj,
    pub core: usize,
    pub obj: Obj,
    pub behavior: Behavior,
    pub helper:crate::ai::control::agents::Helper,
    pub navigation:Option<super::navigation::HelperKey>,
    pub vehicle:super::vehicle_agent::VehicleAgent,
    tracker: TargetTracker,
    active:usize,
    retiring:bool,
}
impl Agent {
    pub fn tracker(&self) -> &TargetTracker {
        if let Behavior::Combat(a) = &self.behavior {
            &a.tracker
        } else {
            &self.tracker
        }
    }
    pub fn tracker_mut(&mut self) -> &mut TargetTracker {
        if let Behavior::Combat(a) = &mut self.behavior {
            &mut a.tracker
        } else {
            &mut self.tracker
        }
    }
}
pub struct Agents {
    rows: Slab<Agent>,
    by_core: super::index::Index<Agent>,
    released:Vec<super::navigation::HelperKey>,
    vehicles:Vec<super::vehicle_agent::VehicleAgent>,
}
impl Agents {
    pub fn new(controllers: usize) -> Self {
        Self::with_scopes(controllers,1)
    }
    pub fn with_scopes(controllers:usize,scopes:usize)->Self {
        Self {rows:Slab::with_capacity(controllers+scopes),by_core:super::index::Index::new(controllers),released:Vec::with_capacity(controllers+scopes),vehicles:(0..controllers+scopes).map(|_|super::vehicle_agent::VehicleAgent::new()).collect()}
    }
    /// initAgent runs after the prior agent has exited. Every kind receives a
    /// fresh tracker, including agents that never enter the combat machine.
    pub fn begin(&mut self, entities: &mut Entities, core: usize, controller: Obj) -> Obj {
        assert!(
            self.by_core.get(core).is_none(),
            "exit the previous native agent before construction"
        );
        let mut vehicle=self.vehicles.pop().expect("prepared native vehicle agent capacity");vehicle.reset();
        let r = self.rows.insert(Agent {
            controller,
            core,
            obj: 0,
            behavior: Behavior::Base,
            helper:Default::default(),navigation:None,vehicle,
            tracker: TargetTracker::new(controller),
            active:0,retiring:false,
        });
        let obj = entities.register(Entity::Agent(Ref::new(r.slot, r.gen)));
        self.rows.get_mut(r).unwrap().obj = obj;
        self.by_core.insert(core,r);
        obj
    }
    fn reference(entities: &Entities, key: Obj) -> Option<Ref<Agent>> {
        let Entity::Agent(r) = entities.get(key)? else {
            return None;
        };
        Some(Ref::new(r.slot, r.gen))
    }
    pub fn get(&self, entities: &Entities, key: Obj) -> Option<&Agent> {
        self.rows.get(Self::reference(entities, key)?)
    }
    pub fn get_mut(&mut self, entities: &Entities, key: Obj) -> Option<&mut Agent> {
        self.rows.get_mut(Self::reference(entities, key)?)
    }
    pub fn key(&self, core: usize) -> Option<Obj> {
        Some(self.rows.get(self.by_core.get(core)?)?.obj)
    }
    /// Callback-visible state is AI data only. Hint definitions are shared and
    /// trackers are inline, so synchronization allocates no body snapshots.
    pub fn sync(&mut self, entities: &Entities, agent: &CombatAgent) -> bool {
        let Some(row) = self.get_mut(entities, agent.obj) else {
            return false;
        };
        assert_eq!(
            row.controller, agent.controller,
            "native combat controller identity"
        );
        row.behavior = Behavior::Combat(agent.clone());
        true
    }
    pub fn combat(&self, entities: &Entities, key: Obj) -> Option<CombatAgent> {
        let Behavior::Combat(a) = &self.get(entities, key)?.behavior else {
            return None;
        };
        Some(a.clone())
    }
    /// A running JS agent remains an object after a callback replaces the
    /// controller's agent. Pin that native row until the running call returns;
    /// replacement already removes it from the controller's current binding.
    pub fn enter(&mut self,entities:&Entities,key:Obj)->bool {
        let Some(row)=self.get_mut(entities,key) else{return false};
        if row.retiring&&row.active==0 {return false;}
        row.active+=1;true
    }
    pub fn leave(&mut self,entities:&mut Entities,key:Obj) {
        let reference=Self::reference(entities,key).expect("running native agent identity");
        let row=self.rows.get_mut(reference).expect("running native agent row");
        assert!(row.active>0,"native agent scope balance");row.active-=1;
        if row.active==0&&row.retiring {self.release(reference);entities.retire(key);}
    }
    fn release(&mut self,reference:Ref<Agent>) {
        let row=self.rows.remove(reference).unwrap();
        if let Some(helper)=row.navigation {self.released.push(helper);}
        self.vehicles.push(row.vehicle);
    }
    pub fn released_helper(&mut self)->Option<super::navigation::HelperKey> {self.released.pop()}
    pub fn retire(&mut self, entities: &mut Entities, key: Obj) -> bool {
        let Some(r) = Self::reference(entities, key) else {return false};
        let Some(row)=self.rows.get_mut(r) else {return false};
        if row.retiring {return false;}
        row.retiring=true;
        self.by_core.remove(row.core);
        if row.active==0 {self.release(r);entities.retire(key);}
        true
    }
}
