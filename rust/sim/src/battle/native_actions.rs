//! Battle-tail actions into the existing canonical unit, fleet and recon
//! owners. Prepared readers use Store directly; only factory/player construction
//! and the remaining foreign AI operations need the lifecycle interface.
use crate::world::owner::World as Root;
use super::{f4_live::with_store_in, mobile::MobileEvent, observation::{Mount,Mounts},
    owned_world::Services, registry::Targets, rules::events::EventQueue, world::Member as BattleMember};
use crate::{ gen::*, store::{Ref, StrId}, units::f3::{self,WorldServices}};
use three::Vector3;

/// These hooks run with the canonical Store installed in Sim. Implementations
/// release all native value frames before an AI import; returned objective or
/// readiness commands qualify their Character life. Unit spawning itself is a
/// native factory operation and finishes before returning its reference.
pub trait Lifecycle {
    fn spawn(&mut self, w:&mut Root, character:Ref<Character>, class:Option<StrId>, post:SpawnPost, at:[f64;3], yaw:Option<f64>, services:WorldServices<'_>) -> Option<Ref<Unit>>;
    fn request_objective(&mut self, w:&mut Root, character:Ref<Character>, services:WorldServices<'_>);
    fn republish(&mut self, w:&mut Root, character:Ref<Character>, services:WorldServices<'_>);
    fn post_event(&mut self, w:&mut Root, event:&PostEvent, services:WorldServices<'_>);
    fn mobile_event(&mut self, w:&mut Root, event:&MobileEvent, services:WorldServices<'_>);
    fn battle_over(&mut self, w:&mut Root, loser:f64, services:WorldServices<'_>);
}
/// HumanSpawn retains its own event/selection buffers. It reborrows native
/// services only at the tail slot, after post/rule/Character work finishes.
pub trait HumanDriver {
    fn step<F:Lifecycle>(&mut self,w:&mut Root,s:&mut Store,dt:f64,commands:&[super::human::Command],
        prepared:&mut super::owned_world::Prepared,services:WorldServices<'_>,lifecycle:&mut F,local:Option<Ref<Unit>>);
}
pub struct Actions<'w, 'a,F:Lifecycle,H:HumanDriver> { pub w: &'w mut Root,
    pub services:WorldServices<'a>,
    pub lifecycle:&'a mut F,
    pub human:&'a mut H,
    pub local:Option<Ref<Unit>>,
    pub effects:[StrId;3],
}
fn array(p:Vector3)->[f64;3] {[p.x,p.y,p.z]}
pub(super) fn character(s:&Store, local:Option<Ref<Unit>>, owner:Attacker)->Option<Ref<Character>> {
    match owner {
        Attacker::Unit(r)=>s.unit.get(r).and_then(|u|u.character),
        Attacker::Player=>local.and_then(|r|s.unit.get(r)).and_then(|u|u.character),
        Attacker::OwnerSnapshot(r)=>s.owner_snapshot.get(r).and_then(|o|o.character),
        _=>None,
    }
}
impl<F:Lifecycle,H:HumanDriver> Mounts for Actions<'_, '_,F,H> {
    fn mount(&mut self,s:&Store,r:Ref<SeatLease>)->Option<Mount> {
        let l=s.seat_lease.get(r)?;
        if let Some(r)=l.vehicle {
            let v=s.vehicle.get(r)?;
            let weapon=if l.index>=0.0 && l.index.is_finite() && l.index.fract()==0.0 {
                crate::vehicles::module::f2::firing::first_weapon_in(self.w, r,l.index as usize)
            } else {None};
            return Some(Mount{position:v.position,weapon,vehicle:Some((Some(v.name),super::f4_live::hull_label(&mut self.w.battle, r)))});
        }
        let (position,weapon)=crate::vehicles::module::f2::live_seats::battle_mount_in(self.w, l.turret?)?;
        Some(Mount{position,weapon,vehicle:None})
    }
}
impl<F:Lifecycle,H:HumanDriver> Targets for Actions<'_, '_,F,H> {
    fn member(&mut self,s:&Store,key:Member)->BattleMember {
        if let Some(m)=crate::units::f3_hulls::battle_member_in(&mut self.w.units, s,key) {return m;}
        if let Some(m)=crate::units::f3_props::battle_member_in(self.w, s,key) {return m;}
        let f=f3::prepared_target_in(&self.w.units, crate::units::registry::target_id(key)).expect("prepared battle target");
        BattleMember{alive:f.alive,team:f.team,position:f.position,..BattleMember::default()}
    }
}
impl<F:Lifecycle,H:HumanDriver> Services for Actions<'_, '_,F,H> {
    fn changed_battle(&mut self,_:&Store) {super::f4_core::changed_battle(&mut self.w.battle);}
    fn building_alive(&mut self,r:Ref<Building>)->bool {crate::units::f3_props::battle_building_in(self.w, r).0}
    fn building_attacker(&mut self,s:&Store,r:Ref<Building>)->Option<Ref<Character>> {
        character(s,self.local,crate::units::f3_props::battle_building_in(self.w, r).1)
    }
    fn vehicle_attacker(&mut self,s:&Store,r:Ref<Vehicle>)->Option<Ref<Character>> {
        let owner=s.vehicle.get(r)?.last_attacker;
        let owner=match owner {
            OrdnanceOwner::Player=>Attacker::Player,OrdnanceOwner::Unit(r)=>Attacker::Unit(r),
            OrdnanceOwner::OwnerSnapshot(r)=>Attacker::OwnerSnapshot(r),_=>Attacker::Null,
        }; character(s,self.local,owner)
    }
    fn piloted(&mut self,s:&Store,r:Ref<Unit>)->Option<[f64;3]> {
        s.recon_droid.get(crate::recon::module::piloted_in(&self.w.units,s,r)?).map(|d|d.position)
    }
    fn relation(&mut self,a:f64,b:f64)->f64 {f3::relation_in(&self.w.units, a,b)}
    fn prepare_record(&mut self,s:&mut Store,r:Ref<Character>) {super::f4_core::prepare_record(&mut self.w.battle, s,r);}
    fn point(&mut self,s:&mut Store,r:Option<Ref<Character>>) {super::f4_core::point(&mut self.w.battle, s,r);}
    fn kill(&mut self,s:&mut Store,r:Ref<Unit>) {
        with_store_in(self.w,s,|w|crate::combat::live::damage_in(w, r,f64::INFINITY,Attacker::Null,self.services.reborrow()));
    }
    fn kill_droid(&mut self,s:&mut Store,r:Ref<Unit>) {
        with_store_in(self.w,s,|w|crate::recon::module::kill_piloted_in(w, r,self.services.reborrow()));
    }
    fn wreck(&mut self,s:&mut Store,r:Ref<Vehicle>) {
        s.vehicle.get_mut(r).expect("boundary hull").last_attacker=OrdnanceOwner::Null;
        crate::vehicles::module::f2::mark_changed_in(&mut self.w.fleet, r);
        with_store_in(self.w,s,|w|crate::units::f3_hulls::mutate_in(w, r,f64::INFINITY,Some(OrdnanceOwner::Null),self.services.reborrow()));
    }
    fn set_hull_health(&mut self,s:&mut Store,r:Ref<Vehicle>,value:f64) {
        if value == 0.0 && self.w.battle.rule_events.is_some() {
            // A script or a kill region sets health directly. Reload the pooled
            // hull from this row before its crew and destruction callbacks run.
            s.vehicle.get_mut(r).expect("current fleet hull").last_attacker = OrdnanceOwner::Null;
            crate::vehicles::module::f2::mark_changed_in(&mut self.w.fleet, r);
        }
        with_store_in(self.w,s,|w|crate::units::f3_hulls::mutate_in(w, r,value,None,self.services.reborrow()));
    }
    fn spawn(&mut self,s:&mut Store,r:Ref<Character>,class:Option<StrId>,post:SpawnPost,at:[f64;3],yaw:Option<f64>)->Option<Ref<Unit>> {
        with_store_in(self.w,s,|w|self.lifecycle.spawn(w,r,class,post,at,yaw,self.services.reborrow()))
    }
    fn request_objective(&mut self,s:&mut Store,r:Ref<Character>) {
        with_store_in(self.w,s,|w|{self.lifecycle.request_objective(w,r,self.services.reborrow());});
    }
    fn republish(&mut self,s:&mut Store,r:Ref<Character>) {
        with_store_in(self.w,s,|w|{self.lifecycle.republish(w,r,self.services.reborrow());});
    }
    fn cast_spawn(&mut self,from:Vector3,direction:Vector3,span:f64,body:Ref<Body>)->Option<Vector3> {
        crate::units::native::ordnance_cast_in(self.w, f3::collision_world_in(&self.w.units),array(from),array(direction),span,Some(body),true,false)
            .map(|h|Vector3::new(h.point[0],h.point[1],h.point[2]))
    }
    fn effect(&mut self,_:&mut Store,name:&str,position:[f64;3]) {
        let name=match name {
            "explosion"=>self.effects[0],"med_explosion"=>self.effects[1],"small_explosion"=>self.effects[2],
            _=>panic!("prepared kill-region effect"),
        }; F4Effect{name,position}.emit(&mut self.w.sim.events);
    }
    fn post_event(&mut self,s:&mut Store,event:&PostEvent) {
        with_store_in(self.w,s,|w|{self.lifecycle.post_event(w,event,self.services.reborrow());});
    }
    fn mobile_event(&mut self,s:&mut Store,event:&MobileEvent) {
        with_store_in(self.w,s,|w|{self.lifecycle.mobile_event(w,event,self.services.reborrow());});
    }
    fn battle_over(&mut self,s:&mut Store,loser:f64) {
        with_store_in(self.w,s,|w|{self.lifecycle.battle_over(w,loser,self.services.reborrow());});
    }
    fn rule_objects(&mut self) -> Option<&mut dyn super::rules::objects::ObjectServices> { Some(self) }
    fn rule_events(&mut self) -> Option<&mut EventQueue> {
        self.w.battle.rule_events.as_mut()
    }
    fn ai_goal_pass(&mut self, s: &mut Store, bots: &mut crate::ai::goals::GoalController, table: &dyn crate::ai::goals::GoalTable, dt: f64) {
        with_store_in(self.w, s, |w| {
            if let Some(ai) = self.services.ai.as_deref_mut() {
                ai.goal_pass(w, self.services.recon, self.services.queries, bots, table, dt);
            }
        });
    }
    fn ai_soldier_inputs(&mut self, s: &mut Store, inputs: &mut crate::ai::goals::SoldierInputs, dt: f64) {
        with_store_in(self.w, s, |w| {
            if let Some(ai) = self.services.ai.as_deref_mut() {
                ai.soldier_inputs(w, self.services.recon, self.services.queries, inputs, dt);
            }
        });
    }
    fn human_tail(&mut self,s:&mut Store,dt:f64,commands:&[super::human::Command],prepared:&mut super::owned_world::Prepared) {
        self.human.step(self.w,s,dt,commands,prepared,self.services.reborrow(),self.lifecycle,self.local);
    }
    fn corpse_tail(&mut self,s:&mut Store) {
        let Some(r)=self.local else{return};
        let u=s.unit.get(r).expect("viewpoint corpse");
        if u.combat.dead && !u.removed && u.combat.death_time>=crate::units::unit::DEATH_LINGER {
            with_store_in(self.w,s,|w|f3::remove_native_in(w, r,self.services.reborrow()));
        }
    }
}

impl<F:Lifecycle,H:HumanDriver> super::stats::TextWorld for Actions<'_, '_, F,H> { fn text_nonempty(&self,id:StrId)->bool { !self.w.sim.strings.units(id).is_empty() } }

impl<F:Lifecycle,H:HumanDriver> super::spawning::RandomWorld for Actions<'_, '_, F,H> { fn random_next(&mut self, stream:u32)->f64 { f3::random_next_in(&mut self.w.units,stream) as f64 } }

use super::rules::events::{ObjectHandle, ObjectProperty, ObjectState, RuleError, RuleResult};
use super::rules::objects::ObjectServices;

impl<F: Lifecycle, H: HumanDriver> ObjectServices for Actions<'_, '_, F, H> {
    fn add_landing_region(&mut self, name: &str) -> RuleResult<()> {
        self.w.space_add_landing_region(name).map(|_| ()).ok_or(RuleError::NotAvailable)
    }
    fn passage_barrier(&mut self, name: &str, enabled: bool) -> bool {
        self.w.space_set_passage_barrier(name, enabled)
    }
    fn text(&self, id: StrId) -> RuleResult<&[u16]> {
        self.w.sim.strings.list.get(id.0 as usize).map(Vec::as_slice).ok_or(RuleError::NotAvailable)
    }

    fn goal_alive(&self, s: &Store, object: &ObjectHandle) -> RuleResult<bool> {
        if let ObjectHandle::Prop(h) = object {
            let live = super::f4::live_ref(&self.w.battle);
            return match h.kind {
                F4ObjectKind::Building => live.buildings.get(h).map(|b| b.state.building.alive() && b.state.building.health > 0.0).ok_or(RuleError::StaleHandle),
                F4ObjectKind::Trap => live.traps.get(h).map(|t| t.state.built && t.state.health > 0.0).ok_or(RuleError::StaleHandle),
                F4ObjectKind::Animated => Ok(live.animated.get(h).is_some()),
                _ => Err(RuleError::NotAvailable),
            };
        }
        self.state(s, object).map(|state| state.alive)
    }

    fn shields(&self,s:&Store,object:&ObjectHandle)->RuleResult<(f64,f64,f64)> { shields(self.w,s,object) }
    fn current(&self,s:&Store,object:&ObjectHandle)->RuleResult<ObjectHandle> { current(self.w,s,object) }
    fn state(&self,s:&Store,object:&ObjectHandle)->RuleResult<ObjectState> { object_state(self.w,s,object) }

    fn change(&mut self, s: &mut Store, object: &ObjectHandle, property: ObjectProperty) -> RuleResult<()> {
        if self.current(s, object)? != *object { return Err(RuleError::StaleHandle); }
        match property {
            ObjectProperty::MaxShield(_) | ObjectProperty::AddShield(_) | ObjectProperty::CurrentShield(_)
                if !self.w.battle.rule_events.as_ref().is_some_and(|q| q.mission_events()) => return Err(RuleError::NotAvailable),
            ObjectProperty::Health(v) if !v.is_finite() || v < 0.0 => return Err(RuleError::InvalidValue),
            ObjectProperty::MaxHealth(v) if !v.is_finite() || v <= 0.0 => return Err(RuleError::InvalidValue),
            ObjectProperty::Team(t) if t.fract() != 0.0 || !(0.0..super::spawning::TEAM_COUNT as f64).contains(&t) => return Err(RuleError::InvalidValue),
            _ => {}
        }
        match object {
            ObjectHandle::Prop(h) => {
                let index = crate::units::f3_props::building_of_in(&self.w.units, h).ok_or(RuleError::NotAvailable)?;
                with_store_in(self.w, s, |w| {
                    if let ObjectProperty::Health(value) = property {
                        if value == 0.0 { super::f4::set_attacker_in(&mut w.battle, h, Attacker::Null); }
                        crate::units::f3_props::command_hit_in(w, index, value, None, None, self.services.reborrow());
                        Ok(())
                    } else {
                        crate::units::f3_props::rule_property(w, index, property)
                    }
                })
            }
            ObjectHandle::Vehicle { vehicle, .. } => {
                let v = s.vehicle.get_mut(*vehicle).unwrap();
                match property {
                    ObjectProperty::Health(value) => {
                        if !s.vehicles.vehicles.contains(vehicle) { return Err(RuleError::NotAvailable); }
                        self.set_hull_health(s, *vehicle, value);
                        return Ok(());
                    }
                    ObjectProperty::MaxHealth(max) => v.max_health = max,
                    ObjectProperty::Team(team) => v.team = Some(team),
                    ObjectProperty::Visible(visible) => v.visible = visible,
                    _ => return Err(RuleError::NotAvailable),
                }
                crate::vehicles::module::f2::mark_changed_in(&mut self.w.fleet, *vehicle);
                Ok(())
            }
            ObjectHandle::Door(instance) => {
                let ObjectProperty::Locked(locked) = property else { return Err(RuleError::NotAvailable) };
                let id = self.w.space().and_then(|s| s.doors.iter().find(|d| d.instance == *instance as usize))
                    .ok_or(RuleError::StaleHandle)?.id;
                self.w.space_set_door_locked(id, locked).then_some(()).ok_or(RuleError::NotAvailable)
            }
            ObjectHandle::Post(SpawnPost::Post(r)) => match property {
                ObjectProperty::Team(team) => with_store_in(self.w, s, |w| {
                    crate::units::f3_props::rule_post_team(w, *r, team)
                }),
                _ => Err(RuleError::NotAvailable),
            },
            ObjectHandle::Post(SpawnPost::MobilePost(_)) => Err(RuleError::NotAvailable),
        }
    }
}

pub(super) fn shields(w:&Root, s: &Store, object: &ObjectHandle) -> RuleResult<(f64, f64, f64)> {
        if !w.battle.rule_events.as_ref().is_some_and(|q| q.mission_events()) { return Err(RuleError::NotAvailable); }
        if current(w,s,object)? != *object { return Err(RuleError::StaleHandle); }
        let ObjectHandle::Prop(h) = object else { return Err(RuleError::NotAvailable); };
        let i = crate::units::f3_props::building_of_in(&w.units, h).ok_or(RuleError::NotAvailable)?;
        let shield = crate::units::f3_props::mission_shields(&w.units, i);
        Ok((shield.current, shield.maximum, shield.recharge))
    }

pub(super) fn current(w:&Root, s: &Store, object: &ObjectHandle) -> RuleResult<ObjectHandle> {
        let stale = RuleError::StaleHandle;
        match object {
            ObjectHandle::Prop(h) => {
                let live = super::f4::live_if(&w.battle).ok_or(stale)?;
                let current = match h.kind {
                    F4ObjectKind::Building => live.buildings.current(h),
                    F4ObjectKind::Trap => live.traps.current(h),
                    F4ObjectKind::Animated => live.animated.current(h),
                    _ => None,
                };
                current.map(ObjectHandle::Prop).ok_or(stale)
            }
            ObjectHandle::Vehicle { vehicle, .. } => Ok(ObjectHandle::Vehicle {
                vehicle: *vehicle, life: s.vehicle.get(*vehicle).ok_or(stale)?.target_generation,
            }),
            ObjectHandle::Door(i) if w.space().is_some_and(|s| s.doors.iter().any(|d| d.instance == *i as usize)) => Ok(object.clone()),
            ObjectHandle::Post(SpawnPost::Post(r)) if s.post.contains(*r) => Ok(object.clone()),
            ObjectHandle::Post(SpawnPost::MobilePost(r)) if s.mobile_post.contains(*r) => Ok(object.clone()),
            _ => Err(stale),
        }
    }

pub(super) fn object_state(w:&Root, s: &Store, object: &ObjectHandle) -> RuleResult<ObjectState> {
        if current(w,s,object)? != *object { return Err(RuleError::StaleHandle); }
        let (alive, team, health) = match object {
            ObjectHandle::Prop(h) => {
                let live = super::f4::live_ref(&w.battle);
                let (alive, health, max) = match h.kind {
                    F4ObjectKind::Trap => {
                        let t = live.traps.get(h).ok_or(RuleError::StaleHandle)?;
                        (t.state.built && t.state.health > 0.0, t.state.health, t.settings.max_health)
                    }
                    F4ObjectKind::Building => {
                        let b = live.buildings.get(h).ok_or(RuleError::StaleHandle)?;
                        (b.state.building.alive() && b.state.building.health > 0.0, b.state.building.health, b.settings.max_health)
                    }
                    _ => return Err(RuleError::NotAvailable),
                };
                let team = crate::units::f3_props::building_of_in(&w.units, h)
                    .map(|i| crate::units::f3_props::rule_team(&w.units, i));
                (alive, team, Some((health, max)))
            }
            ObjectHandle::Vehicle { vehicle, .. } => {
                let v = s.vehicle.get(*vehicle).unwrap();
                (v.alive, v.team, Some((v.health, v.max_health)))
            }
            ObjectHandle::Door(_) => (true, None, None),
            ObjectHandle::Post(SpawnPost::Post(r)) => {
                let p = s.post.get(*r).unwrap();
                (p.active, Some(p.team), None)
            }
            ObjectHandle::Post(SpawnPost::MobilePost(r)) => {
                let p = s.mobile_post.get(*r).unwrap();
                (p.active, Some(p.team), None)
            }
        };
        Ok(ObjectState { alive, team, health })
    }
