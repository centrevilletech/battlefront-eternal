//! Vehicle policy observations and commands over F2's canonical owners. Class
//! facts are registered once; leases, occupancy and motion are read when asked.
use std::rc::Rc;
use crate::{gen::{Store, Vehicle, SeatLease, SeatUnit, AiLifeHandle}, store::Ref,
    seats::{SeatWorld, lease::{SeatControl, control_key}},
    vehicles::f2_seats::{Seats, HullLife, UnitLife}};
use crate::ai::vehicles::world::{HullView, LeaseView, SeatView, Stick, Obj};
use super::{entities::{Entities,Entity}, scratch::{List,Pool}};
use three::{Vector3,Quaternion,Matrix4};

#[derive(Clone,Debug,Default)]
pub struct HullClass {
    pub name:Rc<str>, pub label:Option<Rc<str>>,
    pub max_speed:f64, pub forward_speed:f64,
    pub slot_post:bool, pub decay_time:Option<f64>,
    pub seats:Vec<SeatView>,
}
pub struct Target {
    pub member:crate::gen::Member,pub kind:f64,pub category:f64,
    pub policy:Rc<crate::ai::control::world::ClassSnap>,pub combat:crate::ai::combat::world::ClassView,
}
struct Hull { reference:Ref<Vehicle>, class:HullClass,decay:Option<crate::ai::vehicles::service::Decay>,target:Option<Target>,pending:Option<Target>,target_class:Option<crate::ai::vehicles::world::ClassView> }
pub struct Mounts {
    hulls:Vec<Option<Hull>>,
    objects:Pool<Obj>,
    flight_names:[Rc<str>;5],
}
impl Mounts {
    pub fn new(capacity:usize, nested_queries:usize) -> Self {
        Self {hulls:Vec::new(), objects:Pool::new(nested_queries,capacity),
            flight_names:crate::gen::FlightState::NAMES.map(Rc::from)}
    }
    pub fn register(&mut self,store:&Store,entities:&mut Entities,reference:Ref<Vehicle>,mut class:HullClass) {
        let hull=store.vehicle.get(reference).expect("registered AI hull");
        assert_eq!(hull.seats.len(),class.seats.len());
        assert!(class.seats.iter().all(|seat|seat.lease.is_none()),"class seats contain no occupancy");
        for seat in &mut class.seats {seat.prepared_limits=Some(crate::ai::vehicles::service::prepare_mount_limits(seat.yaw_limits.as_deref(),seat.pitch_limits.as_deref(),true));}
        entities.register(Entity::Hull(reference));entities.register(Entity::HullEntry(reference));
        if self.hulls.len()<=reference.slot as usize {self.hulls.resize_with(reference.slot as usize+1,||None);}
        self.hulls[reference.slot as usize]=Some(Hull {reference,class,decay:None,target:None,pending:None,target_class:None});
    }
    pub fn target(&mut self,entities:&Entities,reference:Ref<Vehicle>,mut target:Target)->Option<Obj> {
        assert!(matches!(target.member,crate::gen::Member::VehicleTarget(_)),"hull registry identity");
        let row=self.hulls.get_mut(reference.slot as usize)?.as_mut().filter(|h|h.reference==reference)?;
        target.combat.prepared_role=Some(crate::ai::combat::agent::role_of(Some(&target.combat)));
        target.combat.prepared_category=Some(target.category);if row.target_class.is_none() {row.target_class=Some(super::content::vehicle_class(&target.policy,0.));}row.target=Some(target);
        entities.find(Entity::HullEntry(reference))
    }
    /// A prepared pad variant has policy but no mixed-registry id yet. Its
    /// first activation adopts that id without copying or cooking class data.
    pub fn stage_target(&mut self,entities:&Entities,reference:Ref<Vehicle>,mut target:Target,member:Option<crate::gen::Member>)->Option<Obj> {
        if let Some(member)=member {target.member=member;return self.target(entities,reference,target);}
        target.combat.prepared_role=Some(crate::ai::combat::agent::role_of(Some(&target.combat)));target.combat.prepared_category=Some(target.category);
        let row=self.hulls.get_mut(reference.slot as usize)?.as_mut().filter(|row|row.reference==reference)?;row.pending=Some(target);None
    }
    pub fn activate_target(&mut self,entities:&Entities,reference:Ref<Vehicle>,member:crate::gen::Member)->Option<Obj> {
        let mut target=self.hulls.get_mut(reference.slot as usize)?.as_mut().filter(|row|row.reference==reference)?.pending.take()?;
        target.member=member;self.target(entities,reference,target)
    }
    /// Authored vehicle-only fields are richer than the common control policy.
    pub fn target_vehicle_class(&mut self,reference:Ref<Vehicle>,class:crate::ai::vehicles::world::ClassView) {
        if let Some(row)=self.hulls.get_mut(reference.slot as usize).and_then(Option::as_mut).filter(|row|row.reference==reference) {row.target_class=Some(class);}
    }
    pub fn policy_target(&self,store:&Store,entities:&Entities,reference:Ref<Vehicle>)->Option<crate::ai::control::world::AiUnit> {
        let row=self.hulls.get(reference.slot as usize)?.as_ref().filter(|h|h.reference==reference)?;
        let target=row.target.as_ref()?;let h=store.vehicle.get(reference)?;
        Some(crate::ai::control::world::AiUnit {key:entities.find(Entity::HullEntry(reference))?,
            id:crate::units::registry::target_id(target.member) as f64,generation:h.target_generation,kind:target.kind,
            team:team(store,h),alive:h.alive,position:crate::ai::nav::math::Vec3::new(h.position[0],h.position[1],h.position[2]),
            velocity:Some(crate::ai::nav::math::Vec3::new(h.velocity[0],h.velocity[1],h.velocity[2])),class_def:Some(target.policy.clone()),class_name:Some(row.class.name.clone()),
            combat_health:Some(h.health),combat_max_health:Some(h.max_health),sphere_radius:Some(h.bound_radius),..Default::default()})
    }
    pub fn combat_target(&self,store:&Store,entities:&Entities,reference:Ref<Vehicle>)->Option<crate::ai::combat::world::UnitView> {
        let row=self.hulls.get(reference.slot as usize)?.as_ref().filter(|h|h.reference==reference)?;
        let target=row.target.as_ref()?;let h=store.vehicle.get(reference)?;let (tow_on,towed)=tow(store,entities,h);
        Some(crate::ai::combat::world::UnitView {obj:entities.find(Entity::HullEntry(reference))?,
            id:Some(crate::units::registry::target_id(target.member) as f64),generation:h.target_generation,kind:target.kind,category:Some(target.category),
            team:team(store,h),alive:h.alive,position:v3(h.position),velocity:Some(v3(h.velocity)),class:Some(target.combat.clone()),
            combat_max_health:crate::combat::defs::Js::Num(h.max_health),vehicle:entities.find(Entity::Hull(reference)),tow_on,towed,has_origin:true,..Default::default()})
    }
    pub fn vehicle_target(&self,store:&Store,entities:&Entities,reference:Ref<Vehicle>)->Option<crate::ai::vehicles::world::UnitView> {
        let row=self.hulls.get(reference.slot as usize)?.as_ref().filter(|h|h.reference==reference)?;
        let target=row.target.as_ref()?;let h=store.vehicle.get(reference)?;
        Some(crate::ai::vehicles::world::UnitView {obj:entities.find(Entity::HullEntry(reference))?,id:crate::units::registry::target_id(target.member) as f64,
            generation:h.target_generation,kind:target.kind,team:team(store,h),alive:h.alive,position:v3(h.position),velocity:v3(h.velocity),
            class:Some(row.target_class.as_ref()?.clone()),class_name:Some(row.class.name.clone()),vehicle:entities.find(Entity::Hull(reference)),targetable:true,
            own_label:row.class.label.clone(),..Default::default()})
    }
    pub fn sphere(&self,store:&Store,reference:Ref<Vehicle>)->Option<crate::ai::combat::world::Sphere> {
        self.class(reference)?;let h=store.vehicle.get(reference)?;let mut center=v3(h.center);center.apply_matrix4(&Matrix4 {elements:h.matrix});
        Some(crate::ai::combat::world::Sphere {center,radius:h.bound_radius})
    }
    fn class(&self,reference:Ref<Vehicle>)->Option<&HullClass> {
        self.hulls.get(reference.slot as usize)?.as_ref().filter(|h|h.reference==reference).map(|h|&h.class)
    }
    /// Vehicle.decay belongs to the hull object, so a pooled respawn keeps
    /// its clock; a new slab generation cannot inherit the previous object's.
    pub fn decay(&self,reference:Ref<Vehicle>)->Option<Option<crate::ai::vehicles::service::Decay>> {
        let row=self.hulls.get(reference.slot as usize)?.as_ref().filter(|h|h.reference==reference)?;Some(row.decay)
    }
    pub fn write_decay(&mut self,reference:Ref<Vehicle>,value:crate::ai::vehicles::service::Decay)->bool {
        let Some(row)=self.hulls.get_mut(reference.slot as usize).and_then(Option::as_mut).filter(|h|h.reference==reference) else{return false};row.decay=Some(value);true
    }
    pub fn objects(&self)->List<Obj> {self.objects.take()}
    pub fn hulls(&self,store:&Store,seats:&Seats,entities:&Entities)->List<Obj> {
        let mut out=self.objects();
        for h in &seats.hulls.hulls {
            if store.vehicle.get(h.vehicle).is_some() {
                out.push_reserved(entities.find(Entity::Hull(h.vehicle)).expect("registered AI hull identity"));
            }
        }
        out
    }
    pub fn seat_count(&self,store:&Store,reference:Ref<Vehicle>)->Option<usize> {Some(store.vehicle.get(reference)?.seats.len())}
    pub fn seat(&self,store:&Store,entities:&Entities,reference:Ref<Vehicle>,index:usize)->Option<SeatView> {
        let seat=store.vehicle.get(reference)?.seats.get(index)?;
        let mut view=self.class(reference)?.seats.get(index)?.clone();
        view.lease=seat.lease.map(|r|entities.find(Entity::Lease(r)).expect("registered AI lease identity"));
        Some(view)
    }
    pub fn hull(&self,store:&Store,seats:&Seats,entities:&Entities,reference:Ref<Vehicle>)->Option<HullView> {
        let h=store.vehicle.get(reference)?; let class=self.class(reference)?;
        let side=seats.hulls.hulls.iter().find(|s|s.vehicle==reference)?;
        let occupants=h.seats.iter().filter(|s|s.lease.is_some()).count() as f64;
        let free_seats=(0..h.seats.len()).filter(|&i|crate::seats::boarding::seat_free(h,i)).count() as f64;
        Some(HullView {
            obj:entities.find(Entity::Hull(reference))?,entry:entities.find(Entity::HullEntry(reference))?,
            alive:h.alive,name:class.name.clone(),flying:h.flying,walking:h.walking,grounded:h.grounded,
            flight_state:h.flight_state.map(|state|self.flight_names[state as usize].clone()),
            gear:h.gear.unwrap_or(f64::NAN),speed:h.speed,max_speed_setting:class.max_speed,
            forward_speed_setting:class.forward_speed,label:class.label.clone(),
            position:v3(h.position),velocity:v3(h.velocity),yaw:h.yaw,
            quaternion:Quaternion::new(h.quaternion[0],h.quaternion[1],h.quaternion[2],h.quaternion[3]),
            matrix:Matrix4 {elements:h.matrix},bounds_min:v3(h.bounds_min),bounds_max:v3(h.bounds_max),
            bound_radius:h.bound_radius,size:v3(h.size),body:Some(h.body.slot),
            // Live algorithms use seat()/seat_count(), so no dynamic table is
            // copied into this scalar observation.
            seats:Vec::new(),occupants,driver:h.driver.is_some(),free_seats,
            occupied:h.driver.is_some()||occupants!=0.,
            controlling_team:crate::seats::boarding::controlling_team(h,side,&store.seat_lease).unwrap_or(f64::NAN),
            tow_target:h.tow_target,slot_post:class.slot_post,decay_time:class.decay_time,
            max_health:h.max_health,own_system:true,
        })
    }
    pub fn lease(&self,store:&Store,entities:&Entities,reference:Ref<SeatLease>)->Option<LeaseView> {
        let l=store.seat_lease.get(reference)?;
        Some(LeaseView { obj:entities.find(Entity::Lease(reference))?,released:l.released,human:l.human,
            live:crate::seats::lease::live(l,reference,&store.unit),
            unit:l.unit.map(|r|entities.find(Entity::Unit(r)).expect("registered AI rider")),
            controllable:l.controllable.map(|r|entities.find(Entity::Control(r)).expect("registered AI control")),
            vehicle:l.vehicle.map(|r|entities.find(Entity::Hull(r)).expect("registered AI hull")),
            turret:l.turret.map(|r|entities.find(Entity::Turret(r)).expect("registered AI turret")),index:l.index,team:l.team })
    }
    /// Write the current native seat intent, preserving its fire/aim edges.
    pub fn analogs(&self,store:&Store,seats:&mut Seats,lease:Ref<SeatLease>,control:Ref<SeatUnit>,stick:&Stick)->bool {
        let Some(l)=store.seat_lease.get(lease) else {return false};
        if l.controllable!=Some(control)||!crate::seats::lease::live(l,lease,&store.unit) {return false;}
        let controls=if l.turret.is_some() {&mut seats.crews.controls}else{&mut seats.hulls.controls};
        let Some(value)=controls.get_mut(&control_key(control)) else {return false};
        let i=&mut value.intent;
        i.forward=stick.forward;i.right=stick.right;i.turn=stick.turn;i.pitch=stick.pitch;
        i.up=stick.up;i.down=stick.down;i.brake=stick.brake;i.trick=stick.trick;
        i.immelmann=stick.immelmann;i.boost=stick.boost;
        true
    }
    pub fn board<W:SeatWorld+?Sized>(&self,store:&mut Store,seats:&mut Seats,world:&mut W,
        unit:AiLifeHandle,hull:AiLifeHandle)->bool {
        if unit.unit(store).is_none()||hull.vehicle(store).is_none() {return false;}
        seats.board(store,world,UnitLife {unit:Ref::new(unit.slot,unit.generation),life:unit.life},
            HullLife {vehicle:Ref::new(hull.slot,hull.generation),life:hull.life},-1)
    }
    pub fn request_exit(&self,store:&mut Store,seats:&mut Seats,unit:AiLifeHandle,
        post:Option<Ref<crate::gen::Post>>,promote:Option<bool>,placement:bool)->bool {
        if unit.unit(store).is_none() {return false;}
        seats.request_exit(store,UnitLife {unit:Ref::new(unit.slot,unit.generation),life:unit.life},post,promote,placement)
    }
    /// Register a lease's control once at acquisition. Its input is retained by
    /// F2, including after latch consumes button edges.
    pub fn control(&self,seats:&mut Seats,control:Ref<SeatUnit>,turret:bool) {
        seats.control(control,SeatControl {driven:true,..SeatControl::default()},turret);
    }
}
fn v3(v:[f64;3])->Vector3 {Vector3::new(v[0],v[1],v[2])}

fn team(store:&Store,hull:&Vehicle)->f64 {
    for seat in &hull.seats {if let Some(r)=seat.lease {return store.seat_lease.get(r).expect("current AI hull lease").team;}}
    hull.team.unwrap_or(f64::NAN)
}
pub(super) fn tow(store:&Store,entities:&Entities,hull:&Vehicle)->(bool,Option<Obj>) {
    let on=hull.tow_target>=0.;
    let towed=crate::ai::combat::world::index_of(hull.tow_target).and_then(|index|store.vehicles.vehicles.get(index))
        .map(|&r|entities.find(Entity::Hull(r)).expect("prepared tow hull identity"));
    (on,towed)
}
