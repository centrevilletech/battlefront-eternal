//! SeatUnit's live facts come from its lease and hull. Only construction-time
//! class facts and identity are retained here; no hull or intent is mirrored.
use super::entities::{Entities, Entity};
use crate::ai::vehicles::world::{ClassView, Obj, UnitView};
use crate::seats::lease::{control_key, SeatIntent};
use crate::vehicles::f2_seats::Seats;
use crate::{
    gen::{AiLifeHandle, SeatLease, SeatUnit, Store},
    store::Ref,
};
use std::rc::Rc;

struct Binding {
    control: Ref<SeatUnit>,
    lease: Ref<SeatLease>,
    hull: AiLifeHandle,
    index: usize,
    kind: f64,
    class: ClassView,
    name: Rc<str>,
    controller: Option<Obj>,
    id: f64,
    hero: bool,
    policy: Option<usize>,
    steer:Option<crate::ai::vehicles::world::Steer>,
}
struct Policy {
    factory:Option<(ClassView,Rc<str>)>,
    navigation:Option<usize>,
    kind: f64,
    policy: Rc<crate::ai::control::world::ClassSnap>,
    combat: crate::ai::combat::world::ClassView,
}
pub struct HullControls {
    rows: Vec<Option<Binding>>,
    classes: Vec<Policy>,
    stand: Rc<str>,
}
impl HullControls {
    pub fn new() -> Self {
        Self::with_capacity(0)
    }
    pub fn with_capacity(controls: usize) -> Self {
        Self { rows: Vec::with_capacity(controls), classes: Vec::new(), stand: Rc::from("stand") }
    }
    /// SeatUnit captures these immutable facts at acquisition. A fresh lease
    /// must register a fresh control rather than adopting an earlier binding.
    pub fn register(
        &mut self,
        store: &Store,
        entities: &mut Entities,
        control: Ref<SeatUnit>,
        lease: Ref<SeatLease>,
        hull: AiLifeHandle,
        kind: f64,
        class: ClassView,
        name: Rc<str>,
        controller: Option<Obj>,
        occupant: super::seats::Occupant,
    ) -> bool {
        let Some(body) = hull.vehicle(store) else {
            return false;
        };
        let Some(l) = store.seat_lease.get(lease) else {
            return false;
        };
        if l.vehicle != Some(Ref::new(hull.slot, hull.generation))
            || l.controllable != Some(control)
            || l.index < 0.
            || l.index.fract() != 0.
        {
            return false;
        }
        let index = l.index as usize;
        if body.seats.get(index).and_then(|s| s.lease) != Some(lease) {
            return false;
        }
        entities.register(Entity::Control(control));
        entities.register(Entity::Lease(lease));
        let i = control.slot as usize;
        if self.rows.len() <= i {
            self.rows.resize_with(i + 1, || None);
        }
        self.rows[i] = Some(Binding {
            control,
            lease,
            hull,
            index,
            kind,
            class,
            name,
            controller,
            id: occupant.id,
            hero: occupant.hero,
            policy: None,
            steer:None,
        });
        true
    }
    fn binding(&self, control: Ref<SeatUnit>) -> Option<&Binding> {
        self.rows
            .get(control.slot as usize)?
            .as_ref()
            .filter(|r| r.control == control)
    }
    fn live<'a>(
        &'a self,
        store: &'a Store,
        control: Ref<SeatUnit>,
    ) -> Option<(
        &'a Binding,
        &'a crate::gen::Vehicle,
        &'a crate::gen::SeatLease,
    )> {
        let b = self.binding(control)?;
        let h = b.hull.vehicle(store)?;
        let l = store.seat_lease.get(b.lease)?;
        if l.controllable != Some(control)
            || l.vehicle != Some(Ref::new(b.hull.slot, b.hull.generation))
            || l.index != b.index as f64
            || h.seats.get(b.index)?.lease != Some(b.lease)
        {
            return None;
        }
        Some((b, h, l))
    }
    pub fn unit(
        &self,
        store: &Store,
        entities: &Entities,
        control: Ref<SeatUnit>,
    ) -> Option<UnitView> {
        let (b, h, l) = self.live(store, control)?;
        let rider = l.unit.and_then(|r| store.unit.get(r));
        let lease = entities
            .find(Entity::Lease(b.lease))
            .expect("prepared AI lease");
        Some(UnitView {
            obj: entities.find(Entity::Control(control))?,
            kind: b.kind,
            id: b.id,
            generation: 1.,
            alive: h.alive && crate::seats::lease::live(l, b.lease, &store.unit),
            team: l.team,
            position: three::Vector3::new(h.position[0], h.position[1], h.position[2]),
            velocity: three::Vector3::new(h.velocity[0], h.velocity[1], h.velocity[2]),
            body_yaw: h.yaw,
            class: Some(b.class.clone()),
            class_name: Some(b.name.clone()),
            soldier: true,
            soldier_max_speed: Some(if b.index == 0 {
                b.class.forward_speed.or(b.class.max_speed).unwrap_or(0.)
            } else {
                0.
            }),
            human: false,
            network_human: false,
            targetable: false,
            walker: false,
            seat: Some(lease),
            lease: Some(lease),
            vehicle: entities.find(Entity::Hull(Ref::new(b.hull.slot, b.hull.generation))),
            controller: b.controller,
            character: rider.and_then(|u| u.character).map(|r| {
                entities
                    .find(Entity::Character(r))
                    .expect("prepared AI rider Character")
            }),
            ..UnitView::default()
        })
    }
    /// Prepare immutable class policy when content loads. Boarding only binds
    /// the prepared index, keeping class parsing and allocation out of play.
    pub fn class(
        &mut self,
        kind: f64,
        mut policy: crate::ai::control::world::ClassSnap,
        mut combat: crate::ai::combat::world::ClassView,
    ) -> usize {
        policy.prepared_travel_mask = Some(crate::ai::control::goals::travel_mask_of(
            &policy.ai_size_type,
        ));
        policy.prepared_role = Some(crate::ai::control::controller::role_of(
            policy.role,
            policy.unit_type.as_deref(),
        ));
        policy.prepared_health_type = Some(crate::ai::control::classes::health_type(
            &crate::ai::control::world::AiUnit {
                class_def: Some(Rc::new(policy.clone())),
                ..Default::default()
            },
        ));
        combat.prepared_role = Some(crate::ai::combat::agent::role_of(Some(&combat)));
        combat.prepared_category = Some(crate::ai::combat::weapons_ai::category_of(
            &crate::ai::combat::world::UnitView {
                kind,
                class: Some(combat.clone()),
                ..Default::default()
            },
        ));
        let index = self.classes.len();
        self.classes.push(Policy { factory:None,navigation:None,kind, policy: Rc::new(policy), combat });
        index
    }
    pub fn prepare_factory(&mut self,index:usize,class:ClassView,name:Rc<str>) {self.classes[index].factory=Some((class,name));}
    /// SeatUnit construction uses prepared cooked values. Acquisition must
    /// never parse or clone a raw class table on the boarding stack.
    pub fn construct(&mut self,store:&Store,entities:&mut Entities,control:Ref<SeatUnit>,lease:Ref<SeatLease>,class:usize,occupant:super::seats::Occupant)->bool {
        let Some(policy)=self.classes.get(class) else{return false};
        let Some((view,name))=policy.factory.as_ref() else{return false};
        let Some(reference)=store.seat_lease.get(lease).and_then(|l|l.vehicle) else{return false};
        let Some(hull)=store.vehicle.get(reference) else{return false};
        let target=AiLifeHandle {kind:crate::gen::KIND_VEHICLE.into(),slot:reference.slot,generation:reference.gen,life:hull.target_generation};
        let (kind,view,name)=(policy.kind,view.clone(),name.clone());
        self.register(store,entities,control,lease,target,kind,view,name,None,occupant)&&self.set_class(control,class)
    }
    /// The cooked class and its native navigation layout load together.
    /// Controls retain that resolved index across acquisitions and promotions.
    pub fn prepare_navigation(&mut self,index:usize,navigation:usize) {
        self.classes[index].navigation=Some(navigation);
    }
    pub fn navigation(&self,store:&Store,control:Ref<SeatUnit>)->Option<(AiLifeHandle,Ref<SeatLease>,f64,usize,ClassView)> {
        let (binding,_,lease)=self.live(store,control)?;
        if lease.released {return None;}
        let navigation=self.classes.get(binding.policy?)?.navigation?;
        Some((binding.hull.clone(),binding.lease,binding.kind,navigation,binding.class.clone()))
    }
    pub fn set_class(&mut self, control: Ref<SeatUnit>, index: usize) -> bool {
        let Some(class) = self.classes.get(index) else { return false };
        let Some(b) = self.rows.get_mut(control.slot as usize).and_then(Option::as_mut)
            .filter(|b| b.control == control && b.kind == class.kind) else { return false };
        b.policy = Some(index);
        true
    }
    pub fn policy_unit(
        &self,
        store: &Store,
        entities: &Entities,
        control: Ref<SeatUnit>,
        character: &impl Fn(Ref<crate::gen::Character>) -> usize,
    ) -> Option<crate::ai::control::world::AiUnit> {
        use crate::ai::control::world::{AiUnit, SoldierSnap};
        use crate::ai::nav::math::Vec3;
        let (b, h, l) = self.live(store, control)?;
        let seat = h.seats.get(b.index)?;
        Some(AiUnit {
            key: entities.find(Entity::Control(control))?,
            id: b.id,
            generation: 1.,
            kind: b.kind,
            team: l.team,
            alive: h.alive && crate::seats::lease::live(l, b.lease, &store.unit),
            hero: b.hero,
            targetable: Some(false),
            position: Vec3::new(h.position[0], h.position[1], h.position[2]),
            velocity: Some(Vec3::new(h.velocity[0], h.velocity[1], h.velocity[2])),
            soldier: Some(SoldierSnap {
                aim_yaw: Some(seat.aim_yaw),
                aim_pitch: Some(seat.pitch),
                posture: Some(crate::gen::Posture::Stand),
                max_speed: Some(if b.index == 0 {
                    b.class.forward_speed.or(b.class.max_speed).unwrap_or(0.)
                } else {
                    0.
                }),
                max_strafe: Some(if b.index == 0 {
                    b.class.strafe_speed.unwrap_or(0.)
                } else {
                    0.
                }),
                max_turn: Some(b.class.turn_rate.unwrap_or(2.)),
                max_pitch: Some(b.class.pitch_rate.unwrap_or(1.)),
                ..Default::default()
            }),
            class_def: Some(self.classes.get(b.policy?)?.policy.clone()),
            class_name: Some(b.name.clone()),
            combat_health: Some(h.health),
            combat_max_health: Some(h.max_health),
            nav_radius: Some(
                v8math::hypot(h.size[0], h.size[2]) * if b.kind == 6. { 0.5 } else { 0.25 },
            ),
            seat: true,
            seat_vehicle_pos: Some(Vec3::new(h.position[0], h.position[1], h.position[2])),
            has_lease: true,
            lease_is_self: true,
            is_seat_unit: true,
            character: l
                .unit
                .and_then(|r| store.unit.get(r))
                .and_then(|u| u.character)
                .map(character),
            ..AiUnit::default()
        })
    }
    /// SeatUnit's scalar reads validate its current holder and policy, but
    /// need neither class cloning nor the full view's navigation radius.
    pub fn scalars(&self,store:&Store,control:Ref<SeatUnit>)->Option<crate::ai::control::world::UnitScalars> {
        let (b,h,l)=self.live(store,control)?;self.classes.get(b.policy?)?;
        Some(crate::ai::control::world::UnitScalars {generation:1.,kind:b.kind,team:l.team,
            alive:h.alive&&crate::seats::lease::live(l,b.lease,&store.unit),
            position:crate::ai::nav::math::Vec3::new(h.position[0],h.position[1],h.position[2]),has_lease:true,lease_is_self:true,hero:b.hero,human:false})
    }
    /// Keep the acquired body's id for Classic parity, not the control slot.
    pub fn motion(&self,store:&Store,control:Ref<SeatUnit>)->Option<crate::ai::control::world::UnitMotion> {
        let (b,h,_)=self.live(store,control)?;self.classes.get(b.policy?)?;
        let point=|p:[f64;3]|crate::ai::nav::math::Vec3::new(p[0],p[1],p[2]);
        Some(crate::ai::control::world::UnitMotion {id:b.id,position:point(h.position),velocity:Some(point(h.velocity))})
    }
    /// scan and leadPoint observe neither rider Character nor tow/class
    /// snapshots. Keep combat_unit's qualified holder and prepared-class gates.
    pub fn sensor(&self,store:&Store,control:Ref<SeatUnit>)->Option<crate::ai::combat::world::UnitSensor> {
        let (binding,hull,lease)=self.live(store,control)?;self.classes.get(binding.policy?)?;
        Some(crate::ai::combat::world::UnitSensor {generation:1.,kind:binding.kind,team:lease.team,human:false,
            position:three::Vector3::new(hull.position[0],hull.position[1],hull.position[2]),
            velocity:Some(three::Vector3::new(hull.velocity[0],hull.velocity[1],hull.velocity[2])),
            controller:binding.controller,collision_category:None})
    }
    pub fn combat_unit(
        &self,
        store: &Store,
        entities: &Entities,
        control: Ref<SeatUnit>,
    ) -> Option<crate::ai::combat::world::UnitView> {
        use crate::ai::combat::world::{LeaseView, SoldierView, UnitView};
        use crate::combat::defs::Js;
        let (b, h, l) = self.live(store, control)?;
        let unit = l.unit.and_then(|r| store.unit.get(r));
        let controller = unit.and_then(|u| u.controller).map(|r| {
            entities
                .find(Entity::Controller(r))
                .expect("prepared rider controller")
        });
        let (tow_on,towed)=super::mounts::tow(store,entities,h);
        Some(UnitView {
            obj: entities.find(Entity::Control(control))?,
            id: Some(b.id),
            generation: 1.,
            kind: b.kind,
            team: l.team,
            alive: h.alive && crate::seats::lease::live(l, b.lease, &store.unit),
            hero: b.hero,
            targetable_false: true,
            position: three::Vector3::new(h.position[0], h.position[1], h.position[2]),
            velocity: Some(three::Vector3::new(
                h.velocity[0],
                h.velocity[1],
                h.velocity[2],
            )),
            class: Some(self.classes.get(b.policy?)?.combat.clone()),
            combat_max_health: Js::Num(h.max_health),
            soldier: Some(SoldierView {
                posture: Js::Str(self.stand.clone()),
                max_speed: Js::Num(if b.index == 0 {
                    b.class.forward_speed.or(b.class.max_speed).unwrap_or(0.)
                } else {
                    0.
                }),
                ..Default::default()
            }),
            controller: b.controller,
            character: unit.and_then(|u| u.character).map(|r| {
                entities
                    .find(Entity::Character(r))
                    .expect("prepared rider Character")
            }),
            pilot: b.index == 0,
            tow_on,towed,
            vehicle: entities.find(Entity::Hull(Ref::new(b.hull.slot, b.hull.generation))),
            seat: Some(LeaseView {
                obj: entities.find(Entity::Lease(b.lease))?,
                human: l.human,
                released: l.released,
                live_false: !crate::seats::lease::live(l, b.lease, &store.unit),
                controllable_controller: b.controller,
                unit_controller: controller,
            }),
            has_lease: true,
            has_origin: true,
            intent: true,
            intent_buttons: true,
            ..UnitView::default()
        })
    }
    pub fn lease_reference(&self, store: &Store, control: Ref<SeatUnit>) -> Option<Ref<SeatLease>> {
        Some(self.live(store, control)?.0.lease)
    }

    /// The same retained intent F2 latches, including held fire and reload.
    /// Reject released or reassigned leases before allowing a controller write.
    pub fn intent<'a>(
        &self,
        store: &Store,
        seats: &'a mut Seats,
        control: Ref<SeatUnit>,
    ) -> Option<&'a mut SeatIntent> {
        let (b, h, l) = self.live(store, control)?;
        if !h.alive || !crate::seats::lease::live(l, b.lease, &store.unit) {
            return None;
        }
        Some(&mut seats.hulls.controls.get_mut(&control_key(control))?.intent)
    }
    pub fn command(
        &self,
        store: &Store,
        seats: &mut Seats,
        control: Ref<SeatUnit>,
        command: super::intent::Command,
    ) -> bool {
        let Some(intent) = self.intent(store, seats, control) else {
            return false;
        };
        command.seat(intent);
        true
    }
    pub fn steer(&self,control:Ref<SeatUnit>)->Option<Option<crate::ai::vehicles::world::Steer>> {Some(self.binding(control)?.steer)}
    /// The scoped edit is pure. No world borrow survives a steering callback.
    pub fn edit_steer<R>(&mut self,control:Ref<SeatUnit>,edit:impl FnOnce(&mut Option<crate::ai::vehicles::world::Steer>)->R)->Option<R> {
        let row=self.rows.get_mut(control.slot as usize)?.as_mut().filter(|b|b.control==control)?;
        Some(edit(&mut row.steer))
    }
    pub fn link_controller(&mut self,control:Ref<SeatUnit>,controller:Obj,bound:bool)->Option<bool> {
        let row=self.rows.get_mut(control.slot as usize)?.as_mut().filter(|b|b.control==control)?;
        let before=row.controller;
        if bound {row.controller=Some(controller);}else if row.controller==Some(controller) {row.controller=None;}
        Some(before!=row.controller)
    }

    pub fn controller_key(&self,control:Ref<SeatUnit>)->Option<Obj> {self.binding(control)?.controller}
    pub fn retire(&mut self, control: Ref<SeatUnit>) -> bool {
        if self.binding(control).is_none() {
            return false;
        }
        self.rows[control.slot as usize] = None;
        true
    }
}
impl super::combat::ControlOwners for HullControls {
    fn controller(&self, control: Ref<SeatUnit>) -> Option<Obj> {
        self.binding(control)?.controller
    }
}
