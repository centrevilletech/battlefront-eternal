//! Scalar controller observations from F3 rows. Definitions are shared loaded
//! facts; weapon policy and lazy geometry use their own native observation
//! methods rather than copying an arsenal or evaluating shapes here.
use std::rc::Rc;
use crate::{gen::{Store,UnitBody,Character},store::{DefId,Ref}};
use crate::ai::{control::{world::{AiUnit,ClassSnap,SoldierSnap,UKey},goals},nav::{math::Vec3}};
use super::entities::{Entities,Entity};

/// UnitController's current scalars need only the qualified canonical row.
pub fn scalars(store:&Store,reference:Ref<crate::gen::Unit>)->Option<crate::ai::control::world::UnitScalars> {
    let u=store.unit.get(reference)?;let p=u.position();
    Some(crate::ai::control::world::UnitScalars {generation:u.generation,kind:u.kind,team:u.team,alive:u.alive,
        position:Vec3::new(p.x,p.y,p.z),has_lease:false,lease_is_self:false,hero:u.hero,human:u.human})
}
pub fn motion(store:&Store,reference:Ref<crate::gen::Unit>)->Option<crate::ai::control::world::UnitMotion> {
    let u=store.unit.get(reference)?;let p=u.position();let v=u.velocity();
    Some(crate::ai::control::world::UnitMotion {id:reference.slot as f64,position:Vec3::new(p.x,p.y,p.z),velocity:Some(Vec3::new(v.x,v.y,v.z))})
}

struct Class {definition:Rc<ClassSnap>,name:Rc<str>,vehicle:Option<crate::ai::vehicles::world::ClassView>,label:Option<Rc<str>>}
pub struct Units {
    classes:Vec<Option<Class>>,
    characters:Vec<Option<(Ref<Character>,usize)>>,
}
impl Units {
    pub fn new()->Self {Self {classes:Vec::new(),characters:Vec::new()}}
    pub fn class(&mut self,id:DefId,mut definition:ClassSnap,name:Rc<str>) {
        let mask=goals::travel_mask_of(&definition.ai_size_type);
        definition.prepared_travel_mask=Some(mask);
        definition.prepared_role=Some(crate::ai::control::controller::role_of(definition.role,definition.unit_type.as_deref()));
        let i=id.0 as usize;if self.classes.len()<=i {self.classes.resize_with(i+1,||None);}
        let mut definition=Rc::new(definition);
        let health=crate::ai::control::classes::health_type(&AiUnit {class_def:Some(definition.clone()),..AiUnit::default()});
        Rc::get_mut(&mut definition).unwrap().prepared_health_type=Some(health);
        self.classes[i]=Some(Class {definition,name,vehicle:None,label:None});
    }
    pub fn vehicle_class(&mut self,id:DefId,class:crate::ai::vehicles::world::ClassView,label:Option<Rc<str>>) {
        let row=self.classes[id.0 as usize].as_mut().expect("prepared AI body class");row.vehicle=Some(class);row.label=label;
    }
    pub fn vehicle(&self,store:&Store,entities:&Entities,key:UKey)->Option<crate::ai::vehicles::world::UnitView> {
        use crate::ai::vehicles::world::UnitView;
        let Entity::Unit(reference)=entities.get(key)? else{return None};let u=store.unit.get(reference)?;
        let class=self.classes.get(u.class_def.0 as usize)?.as_ref()?;
        let position=u.position();let velocity=u.velocity();
        let (yaw,speed)=match &u.soldier {UnitBody::Soldier(s)=>(s.aim_yaw,Some(s.max_speed)),UnitBody::Droideka(d)=>(d.aim_yaw,None)};
        Some(UnitView {obj:key,id:reference.slot as f64,generation:u.generation,kind:u.kind,alive:u.alive,team:u.team,
            position,velocity,body_yaw:yaw,class:Some(class.vehicle.as_ref()?.clone()),class_name:Some(class.name.clone()),
            soldier:true,soldier_max_speed:speed,walker:u.walker,human:u.human,network_human:u.network_human==Some(true),targetable:u.targetable!=Some(false),
            seat:u.seat.map(|r|entities.find(Entity::Lease(r)).expect("prepared body lease")),
            controller:u.controller.map(|r|entities.find(Entity::Controller(r)).expect("prepared body controller")),
            character:u.character.map(|r|entities.find(Entity::Character(r)).expect("prepared body Character")),own_label:class.label.clone(),..Default::default()})
    }
    pub fn character(&mut self,reference:Ref<Character>,index:usize) {
        let i=reference.slot as usize;if self.characters.len()<=i {self.characters.resize(i+1,None);}
        self.characters[i]=Some((reference,index));
    }
    fn character_index(&self,reference:Ref<Character>)->usize {
        let &(registered,index)=self.characters.get(reference.slot as usize).and_then(Option::as_ref).expect("prepared AI Character identity");
        assert_eq!(registered,reference,"current AI Character generation");index
    }
    /// UnitController.live keeps read's class and storage-life gates, but
    /// never constructs intent, rider, posture or class policy snapshots.
    pub fn bound_live(&self,store:&Store,reference:Ref<crate::gen::Unit>,generation:f64)->bool {
        store.unit.get(reference).is_some_and(|u|u.alive&&u.generation==generation
            &&self.classes.get(u.class_def.0 as usize).is_some_and(Option::is_some))
    }
    pub fn read(&self,store:&Store,entities:&Entities,key:UKey)->Option<AiUnit> {
        let Entity::Unit(reference)=entities.get(key)? else {return None};
        let u=store.unit.get(reference)?;
        let class=self.classes.get(u.class_def.0 as usize)?.as_ref()?;
        let soldier=match &u.soldier {
            UnitBody::Soldier(s)=>SoldierSnap {aim_yaw:Some(s.aim_yaw),aim_pitch:Some(s.aim_pitch),
                max_speed:Some(s.max_speed),max_strafe:Some(s.max_strafe),max_turn:Some(s.max_turn),max_pitch:Some(s.max_pitch),
                posture:Some(s.posture),mode:None,rolled:None},
            UnitBody::Droideka(d)=>SoldierSnap {aim_yaw:Some(d.aim_yaw),aim_pitch:Some(d.aim_pitch),
                max_turn:Some(d.max_turn),mode:Some(d.mode),rolled:Some(d.mode>=11.),..SoldierSnap::default()},
        };
        let position=u.position();let velocity=u.velocity();
        let seat=u.seat.and_then(|r|store.seat_lease.get(r));
        Some(AiUnit {key,id:reference.slot as f64,generation:u.generation,kind:u.kind,team:u.team,
            alive:u.alive,removed:u.removed,human:u.human,hero:u.hero,targetable:u.targetable,
            position:Vec3::new(position.x,position.y,position.z),velocity:Some(Vec3::new(velocity.x,velocity.y,velocity.z)),
            soldier:Some(soldier),class_def:Some(class.definition.clone()),class_name:Some(class.name.clone()),
            combat_health:Some(u.combat.health),combat_max_health:Some(u.combat.max_health),nav_radius:u.nav_radius,
            seat:u.seat.is_some(),seat_vehicle_pos:seat.and_then(|l|l.vehicle).and_then(|r|store.vehicle.get(r))
                .map(|v|Vec3::new(v.position[0],v.position[1],v.position[2])),
            character:u.character.map(|r|self.character_index(r)),intent:u.intent.clone(),..AiUnit::default()})
    }
    pub fn command(&self,store:&mut Store,entities:&Entities,key:UKey,command:super::intent::Command)->bool {
        let Some(Entity::Unit(reference))=entities.get(key) else{return false};
        let Some(unit)=store.unit.get_mut(reference) else{return false};command.unit(&mut unit.intent);true
    }
}
