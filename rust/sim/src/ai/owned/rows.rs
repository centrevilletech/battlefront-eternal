//! A combat service scope over the canonical owners. Mutable borrows end
//! before geometry, boarding or lifecycle services loan the owners onward.
use super::{arsenals::{Mounted,Owner},entities::Entity,intent::Command,roster::CharacterOwner,
    scene::Scene,state::State,weapons::Changes};
use crate::{ai::combat::world::{LeaseView,Obj,UnitView,WeaponSettings,WeaponView},
    battle::{live_phase::Building,ownership::Rows as Buildings},combat::defs::WeaponDefs,
    gen::{Character,Store},store::Ref,vehicles::f2_seats::Seats};
use three::Vector3;

/// A live reader lends F4's rows only during an observation. Holding its
/// zero-sized reader across a damage hook does not hold the building table.
pub trait BuildingOwner {
    fn rows(&self)->&Buildings<Building>;
    fn traps(&self)->Option<&Buildings<crate::battle::live_phase::Trap>> {None}
    fn trap_geometry(&self,_:crate::gen::RepairTarget)->Option<(crate::ai::combat::world::Sphere,three::Matrix4)> {None}
}
impl BuildingOwner for Buildings<Building> {fn rows(&self)->&Buildings<Building> {self}}

pub struct Rows<'a,C:Changes+?Sized,M:Mounted> {
    pub store:&'a mut Store,
    pub seats:&'a mut Seats,
    pub buildings:&'a dyn BuildingOwner,
    pub characters:&'a mut dyn CharacterOwner,
    pub state:&'a mut State,
    pub changes:&'a mut C,
    pub mounted:&'a mut M,
    pub definitions:&'a WeaponDefs,
}
impl<C:Changes+?Sized,M:Mounted> Rows<'_,C,M> {
    pub fn scene(&self)->Scene<'_> {Scene {traps:self.buildings.traps(),..self.state.scene(self.store,self.seats,self.buildings.rows(),self.characters)}}
    /// Vehicles.humanTarget reads only the driver's current reticle. A
    /// stale pooled hull, target life or registry generation cannot redirect
    /// this boundary input to another object while native decisions run.
    pub fn human_reticle_target(&self,hull:Obj)->Option<Obj> {
        let Entity::Hull(reference)=self.state.entities.get(hull)? else{return None};
        let input=&self.state.reticle;
        if input.pilot!=Some(reference)||self.store.vehicle.get(reference)?.target_generation!=input.pilot_life {return None;}
        let target=self.state.queries.object(&self.state.entities,input.target?)?;
        (self.scene().combat(target)?.generation==input.target_life).then_some(target)
    }
    /// An invalid object token is a dead weak target. It cannot be used to
    /// index a replacement row, even while a deferred sight request drains.
    pub fn unit(&self,obj:Obj)->UnitView {
        let view=if matches!(self.state.entities.get(obj),Some(Entity::Unit(_))) {
            self.state.combat_units.read(self.store,&self.state.entities,&*self.state,obj)
        }else{self.scene().combat(obj)};
        view.unwrap_or_else(||UnitView {obj,..Default::default()})
    }
    pub fn unit_sensor(&self,obj:Obj)->crate::ai::combat::world::UnitSensor {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.combat_units.sensor(self.store,&self.state.entities,r).unwrap_or_default()}
        else if let Some(Entity::Control(reference))=self.state.entities.get(obj) {
            self.state.controls.sensor(self.store,reference)
                .or_else(||self.state.turrets.sensor(self.store,self.seats,self.buildings.rows(),reference)).unwrap_or_default()
        }else {crate::ai::combat::world::UnitSensor::of(&self.unit(obj))}
    }
    pub fn unit_weapon_motion(&self,obj:Obj)->(f64,f64) {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.combat_units.weapon_motion(self.store,r).unwrap_or_default()}
        else {let u=self.unit(obj);(u.kind,crate::ai::combat::weapons_ai::forward_speed(&u))}
    }
    pub fn unit_bound_posture(&self,obj:Obj)->Option<crate::ai::combat::world::BoundPosture> {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.combat_units.bound_posture(self.store,r)}
        else {crate::ai::combat::world::BoundPosture::of(&self.unit(obj))}
    }
    /// Ordinary bodies use their owner directly. A multi-owner scene is
    /// needed only for a mounted control or a registry/service wrapper.
    pub fn policy(&self,obj:Obj)->Option<crate::ai::control::world::AiUnit> {
        if matches!(self.state.entities.get(obj),Some(Entity::Unit(_))) {self.state.units.read(self.store,&self.state.entities,obj)}else{self.scene().policy(obj)}
    }
    pub fn policy_scalars(&self,obj:Obj)->crate::ai::control::world::UnitScalars {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {super::units::scalars(self.store,r).unwrap_or_default()}else{self.scene().scalars(obj)}
    }
    pub fn combat_scalars(&self,obj:Obj)->crate::ai::combat::world::UnitScalars {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {super::combat::scalars(self.store,r).unwrap_or_default()}else{self.scene().combat_scalars(obj)}
    }
    /// Lead and weapon-channel observations need no posture, class view or
    /// lease frame. Keep the full reader's absent-class and weak-token rules.
    pub fn unit_life(&self,obj:Obj)->(bool,f64) {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.combat_units.life(self.store,r).unwrap_or_default()}
        else if matches!(self.state.entities.get(obj),Some(Entity::Control(_))) {let u=self.combat_scalars(obj);(u.alive,u.generation)}
        else {let u=self.unit(obj);(u.alive,u.generation)}
    }
    pub fn unit_priority(&self,obj:Obj)->crate::ai::combat::world::PriorityView {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.combat_units.priority(self.store,r).unwrap_or_default()}
        else {crate::ai::combat::world::PriorityView::of(&self.unit(obj))}
    }
    pub fn unit_target_status(&self,obj:Obj)->crate::ai::combat::world::TargetStatus {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.combat_units.target_status(self.store,r).unwrap_or_default()}
        else {crate::ai::combat::world::TargetStatus::of(&self.unit(obj))}
    }
    pub fn unit_has_origin(&self,obj:Obj)->bool {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.combat_units.observed_body(self.store,r).is_some()}
        else if matches!(self.state.entities.get(obj),Some(Entity::Control(_))) {self.combat_scalars(obj).intent}
        else {self.unit(obj).has_origin}
    }
    /// Native bodies always own the button pair; absent class rows retain
    /// the full observation's empty-unit behavior (consumeFire).
    pub fn unit_has_buttons(&self,obj:Obj)->bool {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.combat_units.observed_body(self.store,r).is_some()}
        else if matches!(self.state.entities.get(obj),Some(Entity::Control(_))) {let u=self.combat_scalars(obj);u.intent&&u.intent_buttons}
        else {let u=self.unit(obj);u.intent&&u.intent_buttons}
    }
    pub fn unit_velocity(&self,obj:Obj)->Option<Vector3> {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {
            let u=self.store.unit.get(r)?;
            self.state.combat_units.class_prepared(u.class_def).then(||u.velocity())
        }else if matches!(self.state.entities.get(obj),Some(Entity::Control(_))) {self.unit_sensor(obj).velocity}
        else{self.unit(obj).velocity}
    }
    pub fn unit_controller(&self,obj:Obj)->Option<Obj> {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {
            let u=self.store.unit.get(r)?;
            if !self.state.combat_units.class_prepared(u.class_def) {return None;}
            u.controller.map(|r|self.state.entities.find(Entity::Controller(r)).expect("prepared AI controller"))
        }else if matches!(self.state.entities.get(obj),Some(Entity::Control(_))) {self.unit_sensor(obj).controller}
        else{self.unit(obj).controller}
    }
    pub fn motion(&self,obj:Obj)->crate::ai::control::world::UnitMotion {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {super::units::motion(self.store,r).unwrap_or_default()}else{self.scene().motion(obj)}
    }
    fn arsenal(&self,obj:Obj)->Option<Owner> {Owner::resolve(self.store,&self.state.entities,&self.state.births,obj)}
    pub fn has_arsenal(&self,obj:Obj)->bool {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.store.unit.contains(r)}else{self.arsenal(obj).is_some()}
    }
    pub fn repair_weapon(&self,obj:Obj,index:usize)->bool {
        self.arsenal(obj).and_then(|owner|owner.inspect(self.store,self.mounted,index,
            |weapon|self.state.classes.is_repair(weapon)))==Some(true)
    }
    pub fn needs_ammo(&self,obj:Obj)->bool {
        let Some(owner)=self.arsenal(obj) else{return false};
        (0..owner.count(self.store,self.mounted)).any(|index|
            owner.inspect(self.store,self.mounted,index,|weapon|self.state.classes.weapon_needs_ammo(weapon))==Some(true))
    }
    pub fn dispenser_available(&self,obj:Obj,index:usize,count:&impl Fn(Obj,&crate::gen::Weapon)->usize)->bool {
        self.arsenal(obj).and_then(|owner|owner.inspect(self.store,self.mounted,index,|weapon|
            self.state.classes.available(weapon,||count(obj,weapon))))==Some(true)
    }
    /// mineWeapon and serviceWeapon keep arsenal order for both F3 bodies
    /// and F2 holders. Only a qualifying dispenser observes its owner key.
    pub fn mine_weapon(&self,obj:Obj,count:&impl Fn(Obj,&crate::gen::Weapon)->usize)->Option<usize> {
        let owner=self.arsenal(obj)?;
        (0..owner.count(self.store,self.mounted)).find(|&index|owner.inspect(self.store,self.mounted,index,|weapon|
            self.state.classes.persistent_mine(weapon)&&self.state.classes.available(weapon,||count(obj,weapon)))==Some(true))
    }
    pub fn service_weapon(&self,obj:Obj,health_type:f64,ammo:bool,count:&impl Fn(Obj,&crate::gen::Weapon)->usize)->Option<usize> {
        let owner=self.arsenal(obj)?;
        (0..owner.count(self.store,self.mounted)).find(|&index|owner.inspect(self.store,self.mounted,index,|weapon| {
            let classes=&self.state.classes;
            if classes.dispenser(weapon)&&!classes.available(weapon,||count(obj,weapon)) {return false;}
            classes.service_quantity(weapon,health_type,ammo)>0.
        })==Some(true))
    }
    pub fn weapon_count(&self,obj:Obj)->usize {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.store.unit.get(r).map_or(0,|u|u.combat.weapons.len())}else{self.arsenal(obj).map_or(0,|o|o.count(self.store,self.mounted))}
    }
    pub fn current_weapon(&self,obj:Obj,channel:f64)->Option<usize> {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {self.state.weapons.current_combat(&self.store.unit.get(r)?.combat,channel)}else{self.arsenal(obj)?.current(self.store,self.mounted,&self.state.weapons,channel)}
    }
    pub fn weapon(&self,obj:Obj,index:usize)->Option<WeaponView> {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {Some(self.state.weapons.view_weapon(self.store.unit.get(r)?.combat.weapons.get(index)?))}else{self.arsenal(obj)?.view(self.store,self.mounted,&self.state.weapons,index)}
    }
    /// chargeWeapon needs only the immutable level-zero classification.
    pub fn weapon_chargeable(&self,obj:Obj,index:usize)->bool {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {
            self.store.unit.get(r).and_then(|u|u.combat.weapons.get(index)).map(|w|self.state.weapons.chargeable(w)).expect("arsenal member")
        }else{self.arsenal(obj).and_then(|owner|owner.inspect(self.store,self.mounted,index,|w|self.state.weapons.chargeable(w))).expect("arsenal member")}
    }
    /// This observation ends before any service can replace the arsenal, so
    /// its current canonical row needs no second saved-life validation.
    pub fn weapon_definition(&self,obj:Obj,index:usize)->Option<std::rc::Rc<crate::combat::defs::JsObj>> {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {
            Some(self.state.weapons.definition(self.store.unit.get(r)?.combat.weapons.get(index)?))
        }else{self.arsenal(obj)?.inspect(self.store,self.mounted,index,|w|self.state.weapons.definition(w))}
    }
    pub fn visit_weapon_levels(&self,obj:Obj,index:usize,visit:&mut dyn FnMut(&crate::combat::defs::JsObj)) {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {
            self.state.weapons.visit_levels(self.store.unit.get(r).and_then(|u|u.combat.weapons.get(index)).expect("arsenal member"),visit);
        }else{self.arsenal(obj).and_then(|owner|owner.inspect(self.store,self.mounted,index,|w|self.state.weapons.visit_levels(w,visit))).expect("arsenal member");}
    }
    pub fn weapon_settings(&self,obj:Obj,index:usize)->WeaponSettings {
        self.arsenal(obj).and_then(|o|o.settings(self.store,self.mounted,index)).expect("current native AI weapon")
    }
    /// Foot weapons need no lease resolution or copy of unrelated tuning.
    /// Mounted writes keep F2's loan and sparse-publication bookkeeping.
    pub fn set_weapon_yaw(&mut self,obj:Obj,index:usize,value:Option<f64>) {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {
            let weapon=self.store.unit.get_mut(r).and_then(|u|u.combat.weapons.get_mut(index)).expect("current native AI weapon");
            if super::weapons::set_yaw(weapon,value) {self.changes.weapon(r);}
        }else{self.edit_weapon(obj,index,|w|w.yaw_spread_override=value);}
    }
    pub fn edit_weapon<R>(&mut self,obj:Obj,index:usize,edit:impl FnOnce(&mut WeaponSettings)->R)->R {
        if let Some(Entity::Unit(r))=self.state.entities.get(obj) {
            let weapon=self.store.unit.get_mut(r).and_then(|u|u.combat.weapons.get_mut(index)).expect("current native AI weapon");
            let (result,changed)=super::weapons::edit_state(weapon,edit);
            if changed {self.changes.weapon(r);}return result;
        }
        let owner=self.arsenal(obj).expect("current native AI arsenal");
        // Mounted spread edits also dirty F2's publication. Its seat owner
        // belongs to this scope; marking the empty module shell loses those
        // touches and allocates a temporary changed list on every aim edit.
        #[cfg(target_arch="wasm32")]
        if matches!(owner,super::arsenals::Owner::Mount(..)) {
            return crate::vehicles::module::f2::live_seats::loan_seats(self.seats,||
                owner.edit(self.store,self.changes,self.mounted,index,edit).expect("current native AI weapon"));
        }
        owner.edit(self.store,self.changes,self.mounted,index,edit).expect("current native AI weapon")
    }
    pub fn select(&mut self,obj:Obj,index:usize)->bool {
        self.arsenal(obj).is_some_and(|o|o.select(self.store,self.changes,self.mounted,self.definitions,index))
    }
    /// Publish only bodies whose native intent was touched. F2 controls retain
    /// their separate latching/publication owner and never dirty the rider.
    pub fn intent(&mut self,obj:Obj,command:Command)->bool {
        let unit=match self.state.entities.get(obj) {Some(Entity::Unit(r))=>Some(r),_=>None};
        let changed=unit.and_then(|r|self.store.unit.get(r)).is_some_and(|u|command.changes_unit(&u.intent));
        if !self.state.intent(self.store,self.seats,self.buildings.rows(),obj,command) {return false;}
        if changed {self.changes.intent(unit.unwrap());}
        true
    }
    pub fn buttons(&mut self,obj:Obj,first:f64,second:f64) {
        self.intent(obj,Command::Buttons([first,second]));
    }
    pub fn aim(&mut self,obj:Obj,value:Option<Vector3>) {
        self.intent(obj,Command::Aim(value.map(|p|[p.x,p.y,p.z])));
    }
    pub fn body_lease_count(&self,obj:Obj)->usize {
        match self.state.entities.get(obj) {
            Some(Entity::HullEntry(r))=>self.store.vehicle.get(r).map_or(0,|h|h.seats.len()),
            Some(Entity::TurretEntry(r))=>self.state.turrets.lease_count(self.seats,self.buildings.rows(),r).unwrap_or(0),
            _=>0,
        }
    }
    pub fn body_lease(&self,obj:Obj,index:usize)->Option<LeaseView> {
        match self.state.entities.get(obj)? {
            Entity::HullEntry(r)=>{
                let lease=self.store.vehicle.get(r)?.seats.get(index)?.lease?;
                self.state.combat_units.lease(self.store,&self.state.entities,&self.scene(),lease)
            },
            Entity::TurretEntry(r) if index==0=>self.state.turrets.lease(self.store,self.seats,self.buildings.rows(),
                &self.state.entities,&self.scene(),r).flatten(),
            _=>None,
        }
    }
    pub fn character(&self,obj:Obj)->Option<Ref<Character>> {
        let Entity::Character(r)=self.state.entities.get(obj)? else {return None};
        self.state.roster.index(self.store,self.characters,r)?;
        Some(r)
    }
    pub fn character_team(&self,obj:Obj)->f64 {
        self.character(obj).and_then(|r|self.store.character.get(r)).map_or(f64::NAN,|c|c.team)
    }
    pub fn character_unit(&self,obj:Obj)->Option<Obj> {
        let r=self.store.character.get(self.character(obj)?)?.unit?;
        self.state.entities.find(Entity::Unit(r))
    }
    pub fn current_controllable(&self,obj:Obj)->Option<Obj> {
        let character=self.store.character.get(self.character(obj)?)?;
        let body=self.store.unit.get(character.unit?)?;
        let lease=self.store.seat_lease.get(body.seat?)?;
        if lease.human||lease.released {return None;}
        let key=self.state.entities.find(Entity::Control(lease.controllable?))?;
        self.unit(key).alive.then_some(key)
    }
    /// PlayerCombat.repairTarget names the raw gameplay object, whereas an AI
    /// job usually names its registry or service wrapper.
    pub fn repair_target(&self,obj:Obj)->Option<Obj> {
        let Entity::Unit(reference)=self.state.entities.get(obj)? else{return None};
        let target=self.store.unit.get(reference)?.combat.repair_target?;
        self.state.entities.find(match target {
            crate::gen::RepairTarget::Vehicle(r)=>Entity::Hull(r),
            crate::gen::RepairTarget::Building(r)=>Entity::Building(r),
            crate::gen::RepairTarget::Turret(r)=>Entity::Turret(r),
        })
    }
    pub fn vehicle_steer<R>(&mut self,obj:Obj,edit:impl FnOnce(&mut Option<crate::ai::vehicles::world::Steer>)->R)->Option<R> {
        let Entity::Control(reference)=self.state.entities.get(obj)? else{return None};
        // Validate the holder life before lending the control's own AI state.
        self.scene().vehicle(obj)?;
        if self.state.controls.lease_reference(self.store,reference).is_some() {
            self.state.controls.edit_steer(reference,edit)
        }else{self.state.turrets.edit_steer(reference,edit)}
    }
    pub fn vehicle_fields(&mut self,obj:Obj,value:crate::ai::vehicles::world::SeatIntent)->bool {
        let Some(Entity::Control(reference))=self.state.entities.get(obj) else{return false};
        if !self.unit(obj).alive {return false;}
        let hull=self.state.controls.lease_reference(self.store,reference).is_some();
        let intent=if hull {self.state.controls.intent(self.store,self.seats,reference)}else{
            if self.state.turrets.lease_reference(self.store,self.seats,self.buildings.rows(),reference).is_none() {return false;}
            self.seats.crews.controls.get_mut(&crate::seats::lease::control_key(reference)).map(|c|&mut c.intent)
        };
        let Some(intent)=intent else{return false};
        intent.up=value.up;intent.down=value.down;intent.brake=value.brake;
        intent.trick=value.trick;intent.immelmann=value.immelmann;intent.boost=value.boost;true
    }
    pub fn tow_weapon(&self,obj:Obj)->Option<(usize,f64)> {
        let owner=self.arsenal(obj)?;
        for index in 0..owner.count(self.store,self.mounted) {
            let weapon=owner.view(self.store,self.mounted,&self.state.weapons,index)?;
            if weapon.def.get("base").is_str("towcableweapon") {return Some((index,weapon.channel.num()));}
        }
        None
    }
}

impl<C:Changes+?Sized,M:Mounted> Rows<'_,C,M> {
    pub fn combat_feed(&mut self,core:usize,input:&crate::ai::control::world::CombatInput<'_>)->bool {
        let objects=super::objects::Objects {store:self.store,commands:self.characters,entities:&self.state.entities,roster:&self.state.roster,population:&self.state.population};
        self.state.controllers.feed_core(core,input,&objects)
    }
    pub fn combat_query_feed(&mut self,core:usize,source:&crate::ai::control::controller::Controller,aim:bool)->bool {
        let Some(row)=self.state.controllers.write_core(core) else{return false};
        let objects=super::objects::Objects {store:self.store,commands:self.characters,entities:&self.state.entities,roster:&self.state.roster,population:&self.state.population};
        if aim {row.feed_aim_query(source,&objects);}else{row.feed_query(source,&objects);}true
    }
    /// Deferred sight validates the bound owner and life, then changes only
    /// its retained candidate. Fire, navigation and alert requests are not
    /// inputs to that service and must not be recopied for each queued client.
    pub fn combat_los_feed(&mut self,core:usize,source:&crate::ai::control::controller::Controller)->bool {
        let Some(row)=self.state.controllers.write_core(core) else{return false};
        row.unit=source.unit;row.generation=source.generation;true
    }
    pub fn combat_aimer_feed(&mut self,core:usize,source:&crate::ai::control::controller::Controller)->bool {
        let Some(row)=self.state.controllers.write_core(core) else{return false};
        let objects=super::objects::Objects {store:self.store,commands:self.characters,entities:&self.state.entities,roster:&self.state.roster,population:&self.state.population};
        row.feed_aimer(source,&objects);true
    }
    pub fn combat_fire_feed(&mut self,core:usize,source:&crate::ai::control::controller::Controller)->bool {
        let Some(row)=self.state.controllers.write_core(core) else{return false};
        let objects=super::objects::Objects {store:self.store,commands:self.characters,entities:&self.state.entities,roster:&self.state.roster,population:&self.state.population};
        row.feed_fire(source,&objects);true
    }
    /// CombatAgent.face writes aimTarget separately from the fire target.
    /// Return that shared field before a nested Core call feeds combat again.
    pub fn combat_aim_target(&self,core:usize)->Option<crate::ai::control::world::EntRef> {
        let ctl=self.state.controllers.read_core(core)?;
        ctl.aim_target.map(|r|crate::ai::control::world::EntRef {unit:r.obj,generation:r.generation})
    }
    pub fn combat_writes(&self,core:usize)->Option<crate::ai::control::world::CombatWrites> {
        let objects=super::objects::Objects {store:self.store,commands:self.characters,entities:&self.state.entities,roster:&self.state.roster,population:&self.state.population};
        Some(self.state.controllers.read_core(core)?.control_writes(&objects))
    }
    pub fn combat_alerts(&self)->super::scratch::List<crate::ai::control::world::AlertState> {
        let objects=super::objects::Objects {store:self.store,commands:self.characters,entities:&self.state.entities,roster:&self.state.roster,population:&self.state.population};
        self.state.controllers.ingest(&objects)
    }
}
