//! Native building repair wrappers and ObjectiveTarget observations. F4 owns
//! health, rebuild state and geometry. The wrappers keep stable equality with
//! the underlying prop while their weak life follows the current target life.
use super::entities::{Entities, Entity};
use crate::{
    ai::{
        combat::world::{ClassView, Obj, Sphere, UnitView},
        control::world::{AiUnit, ClassSnap},
        nav::math::Vec3,
    },
    battle::{
        live_phase::Building,
        ownership::Rows,
        props::{Kind, State},
    },
    combat::defs::Js,
    gen::{F4Handle, Member, RepairTarget},
};
use std::rc::Rc;
use three::Vector3;

pub struct Definition {
    pub name: Option<Rc<str>>,
    pub policy: Rc<ClassSnap>,
    pub combat: ClassView,
    pub team: f64,
    pub armed: bool,
    pub post: Option<Obj>,
}
struct Binding {
    target: RepairTarget,
    owner: F4Handle,
    wrapper: Obj,
    entry: Option<(Member, Obj)>,
    definition: Definition,
    vehicle_class: crate::ai::vehicles::world::ClassView,
    trap: Option<(Sphere,three::Matrix4)>,
}
pub struct Buildings {
    rows: Vec<Binding>,
    buildings: Vec<Option<usize>>,
    turrets: Vec<Option<usize>>,
}
impl Buildings {
    pub fn new(capacity: usize) -> Self {
        Self {
            rows: Vec::with_capacity(capacity),
            buildings: vec![None; capacity],
            turrets: vec![None; capacity],
        }
    }
    /// Both the real object and its repair wrapper have qualified identities.
    /// Objective registry entries are another object, just as in JavaScript.
    pub fn register(
        &mut self,
        rows: &Rows<Building>,
        entities: &mut Entities,
        target: RepairTarget,
        owner: F4Handle,
        definition: Definition,
    ) -> Obj {
        let native = rows.get(&owner).expect("prepared native building");
        assert!(native.settings.kind != Kind::Turret || matches!(target, RepairTarget::Turret(_)));
        self.bind(entities,target,owner,definition,None)
    }
    /// RepairServices includes log traps as building wrappers. Their health
    /// and built state stay on F4's trap row; only pad geometry is immutable.
    pub fn register_trap(&mut self, rows:&Rows<crate::battle::live_phase::Trap>, entities:&mut Entities,
        target:RepairTarget, owner:F4Handle, definition:Definition, geometry:(Sphere,three::Matrix4))->Obj {
        assert!(rows.get(&owner).is_some(),"prepared native trap");
        self.bind(entities,target,owner,definition,Some(geometry))
    }
    fn bind(&mut self,entities:&mut Entities,target:RepairTarget,owner:F4Handle,definition:Definition,
        trap:Option<(Sphere,three::Matrix4)>)->Obj {
        let index = self.rows.len();
        let (slot, slots, object, service) = match target {
            RepairTarget::Building(r) => (
                r.slot,
                &mut self.buildings,
                Entity::Building(r),
                Entity::BuildingService(r),
            ),
            RepairTarget::Turret(r) => (
                r.slot,
                &mut self.turrets,
                Entity::Turret(r),
                Entity::TurretService(r),
            ),
            RepairTarget::Vehicle(_) => panic!("fleet targets use the canonical hull binding"),
        };
        if slots.len() <= slot as usize {
            slots.resize(slot as usize + 1, None);
        }
        slots[slot as usize] = Some(index);
        entities.register(object);
        let wrapper = entities.register(service);
        let vehicle_class=super::content::vehicle_class(&definition.policy,0.);
        self.rows.push(Binding {
            target,
            owner,
            wrapper,
            entry: None,
            definition,vehicle_class,trap,
        });
        wrapper
    }
    fn binding(&self, target: RepairTarget) -> Option<&Binding> {
        let index = match target {
            RepairTarget::Building(r) => self.buildings.get(r.slot as usize)?,
            RepairTarget::Turret(r) => self.turrets.get(r.slot as usize)?,
            RepairTarget::Vehicle(_) => return None,
        };
        self.rows.get((*index)?).filter(|b| b.target == target)
    }
    fn current<'a>(&'a self, rows:&'a Rows<Building>, traps:Option<&'a Rows<crate::battle::live_phase::Trap>>,
        target:RepairTarget)->Option<(&'a Binding,View<'a>)> {
        let binding=self.binding(target)?;
        let view=if let Some(geometry)=binding.trap.as_ref() {
            let traps=traps?;let current=traps.current(&binding.owner)?;let trap=traps.get(&current)?;
            View {state:crate::battle::props::Building {state:if trap.state.built {State::Built}else{State::Unbuilt},health:trap.state.health,
                add_health:0.,last_attacker:Default::default(),target_generation:None},source:Source::Trap(trap,geometry)}
        }else {
            let current=rows.current(&binding.owner)?;let building=rows.get(&current)?;
            View {state:building.state.building,source:Source::Building(building)}
        };
        Some((binding,view))
    }
    pub fn objective(
        &mut self,
        entities: &mut Entities,
        target: RepairTarget,
        member: Member,
    ) -> Option<Obj> {
        let RepairTarget::Building(r) = target else {
            return None;
        };
        if !matches!(member, Member::ObjectiveTarget(_)) {
            return None;
        }
        let index = (*self.buildings.get(r.slot as usize)?)?;
        let binding = self.rows.get_mut(index).filter(|b| b.target == target)?;
        let object = entities.register(Entity::ObjectiveEntry(r));
        binding.entry = Some((member, object));
        Some(object)
    }
    pub fn policy(
        &self,
        rows: &Rows<Building>,
        traps: Option<&Rows<crate::battle::live_phase::Trap>>,
        target: RepairTarget,
        registry: bool,
    ) -> Option<AiUnit> {
        let (binding, building) = self.current(rows, traps, target)?;
        let definition = &binding.definition;
        let state = &building.state;
        let position = building.position();
        let (key, id, kind) = if registry {
            let (member, entry) = binding.entry?;
            (entry, crate::units::registry::target_id(member) as f64, 9.)
        } else {
            (
                binding.wrapper,
                f64::NAN,
                if definition.armed { 8. } else { 9. },
            )
        };
        Some(AiUnit {
            key,
            id,
            generation: state.target_generation.unwrap_or(1.),
            kind,
            team: definition.team,
            alive: state.alive(),
            position: Vec3::new(position.x, position.y, position.z),
            velocity: Some(Vec3::default()),
            class_def: Some(definition.policy.clone()),
            class_name: definition.name.clone(),
            health: Some(state.health),
            max_health: Some(building.max_health()),
            combat_health: registry.then_some(state.health),
            combat_max_health: registry.then_some(building.max_health()),
            health_type_num: (!registry).then_some(match state.state {
                State::Built => 4.,
                State::Destroyed if !building.objective() => 5.,
                State::Unbuilt if !building.objective() => 6.,
                _ => 4.,
            }),
            building_alive: Some(state.alive()),
            ..Default::default()
        })
    }
    pub fn combat(
        &self,
        rows: &Rows<Building>,
        traps: Option<&Rows<crate::battle::live_phase::Trap>>,
        target: RepairTarget,
        registry: bool,
    ) -> Option<UnitView> {
        let (binding, building) = self.current(rows, traps, target)?;
        let definition = &binding.definition;
        let state = &building.state;
        let (obj, id, kind) = if registry {
            let (member, entry) = binding.entry?;
            (
                entry,
                Some(crate::units::registry::target_id(member) as f64),
                9.,
            )
        } else {
            (
                binding.wrapper,
                None,
                if definition.armed { 8. } else { 9. },
            )
        };
        Some(UnitView {
            obj,
            id,
            generation: state.target_generation.unwrap_or(1.),
            kind,
            category: Some(4.),
            team: definition.team,
            alive: state.alive(),
            position: building.position(),
            velocity: Some(Vector3::default()),
            class: Some(definition.combat.clone()),
            combat_max_health: if registry {
                Js::Num(building.max_health())
            } else {
                Js::Undefined
            },
            max_health: Js::Num(building.max_health()),
            building_post: if registry { definition.post } else { None },
            has_origin: true,
            ..Default::default()
        })
    }
    pub fn vehicle(&self,rows:&Rows<Building>,traps:Option<&Rows<crate::battle::live_phase::Trap>>,target:RepairTarget,registry:bool)->Option<crate::ai::vehicles::world::UnitView> {
        let (binding,building)=self.current(rows,traps,target)?;
        let (obj,id,kind)=if registry {let (member,entry)=binding.entry?;(entry,crate::units::registry::target_id(member) as f64,9.)}
            else{(binding.wrapper,f64::NAN,if binding.definition.armed {8.}else{9.})};
        Some(crate::ai::vehicles::world::UnitView {obj,id,generation:building.state.target_generation.unwrap_or(1.),kind,team:binding.definition.team,
            alive:building.state.alive(),position:building.position(),class:Some(binding.vehicle_class.clone()),class_name:binding.definition.name.clone(),targetable:true,..Default::default()})
    }
    pub fn sphere(&self, rows: &Rows<Building>, traps:Option<&Rows<crate::battle::live_phase::Trap>>, target: RepairTarget) -> Option<Sphere> {
        let (_, building) = self.current(rows, traps, target)?;
        Some(building.sphere())
    }
    pub fn forward(&self, rows: &Rows<Building>, traps:Option<&Rows<crate::battle::live_phase::Trap>>, target: RepairTarget) -> Option<Vector3> {
        let (_, building) = self.current(rows, traps, target)?;
        Some(building.forward())
    }
    pub fn matches(&self, entities: &Entities, wrapper: Obj, current: Obj) -> bool {
        let target = match entities.get(wrapper) {
            Some(Entity::BuildingService(r)) => Entity::Building(r),
            Some(Entity::TurretService(r)) => Entity::Turret(r),
            _ => return false,
        };
        entities.get(current) == Some(target)
    }
}
pub(crate) fn position(building: &Building) -> Vector3 {
    let mut position = Vector3::default();
    position.set_from_matrix_position(&building.settings.matrix);
    position
}

// A scalar observation borrows one canonical row. Copying the small lifecycle
// frame keeps no owner loan alive across a later native geometry/death call.
struct View<'a> {state:crate::battle::props::Building,source:Source<'a>}
enum Source<'a> {Building(&'a Building),Trap(&'a crate::battle::live_phase::Trap,&'a (Sphere,three::Matrix4))}
impl View<'_> {
    fn max_health(&self)->f64 {match self.source {Source::Building(b)=>b.settings.max_health,Source::Trap(t,_)=>t.settings.max_health}}
    fn objective(&self)->bool {matches!(self.source,Source::Building(b) if b.settings.kind==Kind::Objective)}
    fn position(&self)->Vector3 {match self.source {Source::Building(b)=>position(b),Source::Trap(_,g)=>g.0.center}}
    fn sphere(&self)->Sphere {match self.source {Source::Building(b)=>{let(center,radius)=self.state.sphere(&b.settings);Sphere {center,radius}},Source::Trap(_,g)=>g.0}}
    fn forward(&self)->Vector3 {let matrix=match self.source {Source::Building(b)=>&b.settings.matrix,Source::Trap(_,g)=>&g.1};let mut out=Vector3::new(0.,0.,1.);out.transform_direction(matrix);out}
}
