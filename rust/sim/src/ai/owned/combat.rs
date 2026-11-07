//! Combat's scalar unit observations read F3 rows directly. Class roles and
//! categories are cooked once; leases remain qualified native F2 references.
use super::entities::{Entities, Entity};
use crate::ai::combat::{
    agent::role_of,
    weapons_ai::category_of,
    world::{ClassView, LeaseView, Obj, ShieldView, SoldierView, UnitView},
};
use crate::combat::defs::{Js, JsObj};
use crate::{
    gen::{Posture, SeatLease, SeatUnit, Store, UnitBody},
    store::{DefId, Ref},
};
use std::rc::Rc;

/// Aim and perception flags borrow no seat, building or Character owner.
pub fn scalars(store:&Store,reference:Ref<crate::gen::Unit>)->Option<crate::ai::combat::world::UnitScalars> {
    let u=store.unit.get(reference)?;
    Some(crate::ai::combat::world::UnitScalars {generation:u.generation,kind:u.kind,team:u.team,alive:u.alive,
        position:u.position(),hero:u.hero,human:u.human,intent:true,intent_buttons:true,targetable_false:u.targetable==Some(false)})
}

pub trait ControlOwners {
    fn controller(&self, control: Ref<SeatUnit>) -> Option<Obj>;
}
struct Definition {
    view: ClassView,
    categories: [f64; 10],
    other_category: f64,
    forward_speed:f64,
}
pub struct Units {
    classes: Vec<Option<Definition>>,
    postures: [Rc<str>; 3],
}
impl Units {
    pub fn new() -> Self {
        Self {
            classes: Vec::new(),
            postures: ["stand", "crouch", "prone"].map(Rc::from),
        }
    }
    pub fn class(&mut self, id: DefId, raw: &JsObj) {
        let view = ClassView {
            fields:Rc::new(crate::ai::combat::world::ClassFields {
            forward_speed: raw.get("ForwardSpeed").clone(),
            max_speed: raw.get("MaxSpeed").clone(),
            max_health: raw.get("MaxHealth").clone(),
            health_type: raw.get("HealthType").clone(),
            jet_jump: raw.get("JetJump").clone(),
            jet_fuel_min_border: raw.get("JetFuelMinBorder").clone(),
            unit_type: raw.get("UnitType").clone(),
            role: raw.get("role").clone(),
            ai_role: raw.get("ai").get("role").clone(),
            }),
            prepared_role: None,
            prepared_category: None,
        };
        self.class_view(id,view);
    }
    pub fn class_view(&mut self,id:DefId,mut view:ClassView) {
        view.prepared_role = Some(role_of(Some(&view)));
        let categories = std::array::from_fn(|kind| {
            category_of(&UnitView {
                kind: kind as f64,
                class: Some(view.clone()),
                ..UnitView::default()
            })
        });
        let other_category = category_of(&UnitView {
            kind: f64::NAN,
            class: Some(view.clone()),
            ..UnitView::default()
        });
        let forward_speed=crate::ai::combat::weapons_ai::forward_speed(&UnitView {class:Some(view.clone()),..Default::default()});
        let i = id.0 as usize;
        if self.classes.len() <= i {
            self.classes.resize_with(i + 1, || None);
        }
        self.classes[i] = Some(Definition {
            view,
            categories,
            other_category,
            forward_speed,
        });
    }
    /// A field observation keeps the full reader's missing-class and weak
    /// storage-generation gates without building unrelated object bindings.
    pub fn observed_body<'a>(&self,store:&'a Store,reference:Ref<crate::gen::Unit>)->Option<&'a crate::gen::Unit> {
        let unit=store.unit.get(reference)?;self.class_prepared(unit.class_def).then_some(unit)
    }
    pub fn life(&self,store:&Store,reference:Ref<crate::gen::Unit>)->Option<(bool,f64)> {
        let u=self.observed_body(store,reference)?;Some((u.alive,u.generation))
    }
    pub fn priority(&self,store:&Store,reference:Ref<crate::gen::Unit>)->Option<crate::ai::combat::world::PriorityView> {
        let u=self.observed_body(store,reference)?;Some(crate::ai::combat::world::PriorityView {kind:u.kind,..Default::default()})
    }
    pub fn target_status(&self,store:&Store,reference:Ref<crate::gen::Unit>)->Option<crate::ai::combat::world::TargetStatus> {
        let u=self.observed_body(store,reference)?;
        let class=&self.classes[u.class_def.0 as usize].as_ref().unwrap().view;
        Some(crate::ai::combat::world::TargetStatus {alive:u.alive,targetable_false:u.targetable==Some(false),
            combat_max_health:Js::Num(u.combat.max_health),class_max_health:class.max_health.clone()})
    }
    /// scan, recordTarget and hearing borrow just the observed body fields.
    /// Preserve read's absent-class and storage-generation gates before any
    /// controller binding resolves; unrelated Character/lease views stay cold.
    pub fn sensor(&self,store:&Store,entities:&Entities,reference:Ref<crate::gen::Unit>)->Option<crate::ai::combat::world::UnitSensor> {
        let u=self.observed_body(store,reference)?;
        Some(crate::ai::combat::world::UnitSensor {generation:u.generation,kind:u.kind,team:u.team,human:u.human,
            position:u.position(),velocity:Some(u.velocity()),
            controller:u.controller.map(|r|entities.find(Entity::Controller(r)).expect("prepared AI controller")),
            collision_category:None})
    }
    /// expandedRange needs no live Character, posture or mount view.
    pub fn weapon_motion(&self,store:&Store,reference:Ref<crate::gen::Unit>)->Option<(f64,f64)> {
        let u=self.observed_body(store,reference)?;
        Some((u.kind,self.classes[u.class_def.0 as usize].as_ref().unwrap().forward_speed))
    }
    pub fn bound_posture(&self,store:&Store,reference:Ref<crate::gen::Unit>)->Option<crate::ai::combat::world::BoundPosture> {
        use crate::ai::combat::world::BoundPosture;
        let u=self.observed_body(store,reference)?;if u.kind!=0. {return None;}
        Some(match &u.soldier {
            UnitBody::Soldier(s)=>if s.air.is_some() {BoundPosture::Stand}else{match s.posture {Posture::Crouch=>BoundPosture::Crouch,Posture::Prone=>BoundPosture::Prone,_=>BoundPosture::Stand}},
            UnitBody::Droideka(d)=>if d.mode>=11. {BoundPosture::Crouch}else{BoundPosture::Stand},
        })
    }
    pub fn class_prepared(&self,id:DefId)->bool {self.classes.get(id.0 as usize).is_some_and(Option::is_some)}
    pub fn read(
        &self,
        store: &Store,
        entities: &Entities,
        controls: &dyn ControlOwners,
        key: Obj,
    ) -> Option<UnitView> {
        let Entity::Unit(reference) = entities.get(key)? else {
            return None;
        };
        let u = store.unit.get(reference)?;
        let definition = self.classes.get(u.class_def.0 as usize)?.as_ref()?;
        let mut class = definition.view.clone();
        class.prepared_category = Some(if u.kind >= 0. && u.kind < 10. && u.kind.fract() == 0. {
            definition.categories[u.kind as usize]
        } else {
            definition.other_category
        });
        let soldier = match &u.soldier {
            UnitBody::Soldier(s) => SoldierView {
                air: s.air.is_some(),
                posture: Js::Str(
                    self.postures[match s.posture {
                        Posture::Stand => 0,
                        Posture::Crouch => 1,
                        Posture::Prone => 2,
                    }]
                    .clone(),
                ),
                shield: None,
                jet: true,
                jet_jump: Js::Num(s.jet.jump),
                jet_min_border: Js::Num(s.jet.min_border),
                fuel: Js::Num(s.fuel),
                max_speed: Js::Num(s.max_speed),
            },
            UnitBody::Droideka(d) => SoldierView {
                posture: Js::Str(self.postures[if d.mode >= 11. { 1 } else { 0 }].clone()),
                shield: d.shield.as_ref().map(|s| ShieldView {
                    on: s.on,
                    max: Some(s.max),
                    value: Some(s.value),
                }),
                ..SoldierView::default()
            },
        };
        Some(UnitView {
            obj: key,
            id: Some(reference.slot as f64),
            generation: u.generation,
            alive: u.alive,
            team: u.team,
            kind: u.kind,
            human: u.human,
            hero: u.hero,
            targetable_false: u.targetable == Some(false),
            position: u.position(),
            velocity: Some(u.velocity()),
            class: Some(class),
            combat_max_health: Js::Num(u.combat.max_health),
            soldier: Some(soldier),
            has_origin: true,
            controller: u.controller.map(|r| {
                entities
                    .find(Entity::Controller(r))
                    .expect("prepared AI controller")
            }),
            character: u.character.map(|r| {
                entities
                    .find(Entity::Character(r))
                    .expect("prepared AI Character")
            }),
            seat: u
                .seat
                .and_then(|r| self.lease(store, entities, controls, r)),
            intent: true,
            intent_buttons: true,
            ..UnitView::default()
        })
    }
    pub fn lease(
        &self,
        store: &Store,
        entities: &Entities,
        controls: &dyn ControlOwners,
        reference: Ref<SeatLease>,
    ) -> Option<LeaseView> {
        let l = store.seat_lease.get(reference)?;
        Some(LeaseView {
            obj: entities
                .find(Entity::Lease(reference))
                .expect("prepared AI lease"),
            human: l.human,
            released: l.released,
            live_false: !crate::seats::lease::live(l, reference, &store.unit),
            controllable_controller: l.controllable.and_then(|r| controls.controller(r)),
            unit_controller: l
                .unit
                .and_then(|r| store.unit.get(r))
                .and_then(|u| u.controller)
                .map(|r| {
                    entities
                        .find(Entity::Controller(r))
                        .expect("prepared AI rider controller")
                }),
        })
    }
}
