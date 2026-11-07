//! One native identity dispatch for the three AI service worlds. This frame
//! borrows canonical owners for scalar observations only. Geometry and commands
//! use separate mutable scopes, so no observation borrow survives a callback.
use super::{
    buildings::Buildings,
    combat::{ControlOwners, Units as CombatUnits},
    controls::HullControls,
    entities::{Entities, Entity},
    mounts::Mounts,
    roster::{CharacterOwner, Roster},
    turrets::Turrets,
    units::Units,
};
use crate::ai::{
    combat::world::{Obj, UnitView},
    control::world::AiUnit,
    vehicles::world::UnitView as VehicleUnit,
};
use crate::{
    battle::{live_phase::Building, ownership::Rows},
    gen::{RepairTarget, SeatUnit, Store},
    store::Ref,
    vehicles::f2_seats::Seats,
};

pub struct Scene<'a> {
    pub store: &'a Store,
    pub seats: &'a Seats,
    pub buildings: &'a Rows<Building>,
    pub traps: Option<&'a Rows<crate::battle::live_phase::Trap>>,
    pub characters: &'a dyn CharacterOwner,
    pub roster: &'a Roster,
    pub entities: &'a Entities,
    pub units: &'a Units,
    pub combat_units: &'a CombatUnits,
    pub controls: &'a HullControls,
    pub mounts: &'a Mounts,
    pub turrets: &'a Turrets,
    pub props: &'a Buildings,
}
impl Scene<'_> {
    pub fn motion(&self,key:Obj)->crate::ai::control::world::UnitMotion {
        use crate::ai::control::world::UnitMotion;
        if let Some(Entity::Unit(reference))=self.entities.get(key) {
            return super::units::motion(self.store,reference).unwrap_or_default();
        }
        if let Some(Entity::Control(reference))=self.entities.get(key) {
            return self.controls.motion(self.store,reference)
                .or_else(||self.turrets.motion(self.store,self.seats,self.buildings,reference)).unwrap_or_default();
        }
        self.policy(key).as_ref().map(UnitMotion::of).unwrap_or_default()
    }
    /// Combat's scalar point uses three's vector type at the same read slot.
    /// Other kinds retain their holder-specific validation and fallback view.
    pub fn combat_scalars(&self,key:Obj)->crate::ai::combat::world::UnitScalars {
        use crate::ai::combat::world::UnitScalars;
        if let Some(Entity::Unit(reference))=self.entities.get(key) {
            return super::combat::scalars(self.store,reference).unwrap_or_default();
        }
        if let Some(Entity::Control(reference))=self.entities.get(key) {
            return self.controls.scalars(self.store,reference)
                .or_else(||self.turrets.scalars(self.store,self.seats,self.buildings,reference,false))
                .map(|u|UnitScalars {generation:u.generation,kind:u.kind,team:u.team,alive:u.alive,
                    position:three::Vector3::new(u.position.x,u.position.y,u.position.z),hero:u.hero,human:u.human,
                    intent:true,intent_buttons:true,targetable_false:true}).unwrap_or_default();
        }
        self.combat(key).as_ref().map(UnitScalars::of).unwrap_or_default()
    }
    /// Scalar reads preserve the current qualified life and source read
    /// point, without constructing a class/intent snapshot for an F3 body.
    pub fn scalars(&self,key:Obj)->crate::ai::control::world::UnitScalars {
        use crate::ai::control::world::UnitScalars;
        if let Some(Entity::Unit(reference))=self.entities.get(key) {
            return super::units::scalars(self.store,reference).unwrap_or_default();
        }
        if let Some(Entity::Control(reference))=self.entities.get(key) {
            return self.controls.scalars(self.store,reference)
                .or_else(||self.turrets.scalars(self.store,self.seats,self.buildings,reference,true)).unwrap_or_default();
        }
        self.policy(key).as_ref().map(UnitScalars::of).unwrap_or_default()
    }
    pub fn policy(&self, key: Obj) -> Option<AiUnit> {
        match self.entities.get(key)? {
            Entity::Unit(_) => self.units.read(self.store, self.entities, key),
            Entity::Control(r) => {
                let character = |r| {
                    self.roster
                        .index(self.store, self.characters, r)
                        .expect("current native rider Character")
                };
                self.controls
                    .policy_unit(self.store, self.entities, r, &character)
                    .or_else(|| {
                        self.turrets.policy_control(
                            self.store,
                            self.seats,
                            self.buildings,
                            self.entities,
                            r,
                            &character,
                        )
                    })
            }
            Entity::HullEntry(r) => self.mounts.policy_target(self.store, self.entities, r),
            Entity::TurretEntry(r) => {
                self.turrets
                    .policy_target(self.store, self.seats, self.buildings, r)
            }
            Entity::BuildingService(r) => {
                self.props
                    .policy(self.buildings, self.traps, RepairTarget::Building(r), false)
            }
            Entity::TurretService(r) => {
                self.props
                    .policy(self.buildings, self.traps, RepairTarget::Turret(r), false)
            }
            Entity::ObjectiveEntry(r) => {
                self.props
                    .policy(self.buildings, self.traps, RepairTarget::Building(r), true)
            }
            _ => None,
        }
    }
    pub fn combat(&self, key: Obj) -> Option<UnitView> {
        match self.entities.get(key)? {
            Entity::Unit(_) => self.combat_units.read(self.store, self.entities, self, key),
            Entity::Control(r) => self
                .controls
                .combat_unit(self.store, self.entities, r)
                .or_else(|| {
                    self.turrets.combat_control(
                        self.store,
                        self.seats,
                        self.buildings,
                        self.entities,
                        r,
                    )
                }),
            Entity::HullEntry(r) => self.mounts.combat_target(self.store, self.entities, r),
            Entity::TurretEntry(r) => {
                self.turrets
                    .combat_target(self.store, self.seats, self.buildings, self.entities, r)
            }
            Entity::BuildingService(r) => {
                self.props
                    .combat(self.buildings, self.traps, RepairTarget::Building(r), false)
            }
            Entity::TurretService(r) => {
                self.props
                    .combat(self.buildings, self.traps, RepairTarget::Turret(r), false)
            }
            Entity::ObjectiveEntry(r) => {
                self.props
                    .combat(self.buildings, self.traps, RepairTarget::Building(r), true)
            }
            _ => None,
        }
    }
    pub fn vehicle(&self, key: Obj) -> Option<VehicleUnit> {
        match self.entities.get(key)? {
            Entity::Unit(_) => self.units.vehicle(self.store, self.entities, key),
            Entity::Control(r) => self
                .controls
                .unit(self.store, self.entities, r)
                .or_else(|| {
                    self.turrets.vehicle_control(
                        self.store,
                        self.seats,
                        self.buildings,
                        self.entities,
                        r,
                    )
                }),
            Entity::HullEntry(r) => self.mounts.vehicle_target(self.store, self.entities, r),
            Entity::TurretEntry(r) => self.turrets.vehicle_target(
                self.store,
                self.seats,
                self.buildings,
                self.entities,
                r,
            ),
            Entity::BuildingService(r) => {
                self.props
                    .vehicle(self.buildings, self.traps, RepairTarget::Building(r), false)
            }
            Entity::TurretService(r) => {
                self.props
                    .vehicle(self.buildings, self.traps, RepairTarget::Turret(r), false)
            }
            Entity::ObjectiveEntry(r) => {
                self.props
                    .vehicle(self.buildings, self.traps, RepairTarget::Building(r), true)
            }
            _ => None,
        }
    }
}
impl ControlOwners for Scene<'_> {
    fn controller(&self, control: Ref<SeatUnit>) -> Option<Obj> {
        self.controls
            .controller(control)
            .or_else(|| self.turrets.controller(control))
    }
}
