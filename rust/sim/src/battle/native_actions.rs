//! Battle-tail actions into the existing canonical unit, fleet and recon
//! owners. Prepared readers use Store directly; only factory/player construction
//! and the remaining foreign AI operations need the lifecycle interface.
use super::{f4_live::with_store, mobile::MobileEvent, observation::{Mount,Mounts},
    owned_world::Services, registry::Targets, world::Member as BattleMember};
use crate::{abi::sim, gen::*, store::{Ref, StrId}, units::f3::{self,WorldServices}};
use three::Vector3;

/// These hooks run with the canonical Store installed in Sim. Implementations
/// release all native value frames before an AI import; returned objective or
/// readiness commands qualify their Character life. Unit spawning itself is a
/// native factory operation and finishes before returning its reference.
pub trait Lifecycle {
    fn spawn(&mut self, character:Ref<Character>, class:Option<StrId>, post:SpawnPost, at:[f64;3], yaw:Option<f64>, services:WorldServices<'_>) -> Option<Ref<Unit>>;
    fn request_objective(&mut self, character:Ref<Character>);
    fn republish(&mut self, character:Ref<Character>);
    fn post_event(&mut self, event:&PostEvent);
    fn mobile_event(&mut self, event:&MobileEvent);
    fn battle_over(&mut self, loser:f64);
}
/// HumanSpawn retains its own event/selection buffers. It reborrows native
/// services only at the tail slot, after post/rule/Character work finishes.
pub trait HumanDriver {
    fn step<F:Lifecycle>(&mut self,s:&mut Store,dt:f64,commands:&[super::human::Command],
        prepared:&mut super::owned_world::Prepared,services:WorldServices<'_>,lifecycle:&mut F,local:Option<Ref<Unit>>);
}
pub struct Actions<'a,F:Lifecycle,H:HumanDriver> {
    pub services:WorldServices<'a>,
    pub lifecycle:&'a mut F,
    pub human:&'a mut H,
    pub local:Option<Ref<Unit>>,
    pub effects:[StrId;3],
}
fn array(p:Vector3)->[f64;3] {[p.x,p.y,p.z]}
fn character(s:&Store, local:Option<Ref<Unit>>, owner:Attacker)->Option<Ref<Character>> {
    match owner {
        Attacker::Unit(r)=>s.unit.get(r).and_then(|u|u.character),
        Attacker::Player=>local.and_then(|r|s.unit.get(r)).and_then(|u|u.character),
        Attacker::OwnerSnapshot(r)=>s.owner_snapshot.get(r).and_then(|o|o.character),
        _=>None,
    }
}
impl<F:Lifecycle,H:HumanDriver> Mounts for Actions<'_,F,H> {
    fn mount(&mut self,s:&Store,r:Ref<SeatLease>)->Option<Mount> {
        let l=s.seat_lease.get(r)?;
        if let Some(r)=l.vehicle {
            let v=s.vehicle.get(r)?;
            let weapon=if l.index>=0.0 && l.index.is_finite() && l.index.fract()==0.0 {
                crate::vehicles::module::f2::firing::first_weapon(r,l.index as usize)
            } else {None};
            return Some(Mount{position:v.position,weapon,vehicle:Some((Some(v.name),super::f4_live::hull_label(r)))});
        }
        let (position,weapon)=crate::vehicles::module::f2::live_seats::battle_mount(l.turret?)?;
        Some(Mount{position,weapon,vehicle:None})
    }
}
impl<F:Lifecycle,H:HumanDriver> Targets for Actions<'_,F,H> {
    fn member(&mut self,s:&Store,key:Member)->BattleMember {
        if let Some(m)=crate::units::f3_hulls::battle_member(s,key) {return m;}
        if let Some(m)=crate::units::f3_props::battle_member(s,key) {return m;}
        let f=f3::prepared_target(crate::units::registry::target_id(key)).expect("prepared battle target");
        BattleMember{alive:f.alive,team:f.team,position:f.position,..BattleMember::default()}
    }
}
impl<F:Lifecycle,H:HumanDriver> Services for Actions<'_,F,H> {
    fn changed_battle(&mut self,_:&Store) {super::f4_core::changed_battle();}
    fn building_alive(&mut self,r:Ref<Building>)->bool {crate::units::f3_props::battle_building(r).0}
    fn building_attacker(&mut self,s:&Store,r:Ref<Building>)->Option<Ref<Character>> {
        character(s,self.local,crate::units::f3_props::battle_building(r).1)
    }
    fn vehicle_attacker(&mut self,s:&Store,r:Ref<Vehicle>)->Option<Ref<Character>> {
        let owner=s.vehicle.get(r)?.last_attacker;
        let owner=match owner {
            OrdnanceOwner::Player=>Attacker::Player,OrdnanceOwner::Unit(r)=>Attacker::Unit(r),
            OrdnanceOwner::OwnerSnapshot(r)=>Attacker::OwnerSnapshot(r),_=>Attacker::Null,
        }; character(s,self.local,owner)
    }
    fn piloted(&mut self,s:&Store,r:Ref<Unit>)->Option<[f64;3]> {
        s.recon_droid.get(crate::recon::module::piloted_in(s,r)?).map(|d|d.position)
    }
    fn relation(&mut self,a:f64,b:f64)->f64 {f3::relation(a,b)}
    fn prepare_record(&mut self,s:&mut Store,r:Ref<Character>) {super::f4_core::prepare_record(s,r);}
    fn point(&mut self,s:&mut Store,r:Option<Ref<Character>>) {super::f4_core::point(s,r);}
    fn kill(&mut self,s:&mut Store,r:Ref<Unit>) {
        with_store(s,||crate::combat::live::damage(r,f64::INFINITY,Attacker::Null,self.services.reborrow()));
    }
    fn kill_droid(&mut self,s:&mut Store,r:Ref<Unit>) {
        with_store(s,||crate::recon::module::kill_piloted(r,self.services.reborrow()));
    }
    fn wreck(&mut self,s:&mut Store,r:Ref<Vehicle>) {
        s.vehicle.get_mut(r).expect("boundary hull").last_attacker=OrdnanceOwner::Null;
        with_store(s,||crate::units::f3_hulls::mutate(r,f64::INFINITY,Some(OrdnanceOwner::Null),self.services.reborrow()));
    }
    fn set_hull_health(&mut self,s:&mut Store,r:Ref<Vehicle>,value:f64) {
        with_store(s,||crate::units::f3_hulls::mutate(r,value,None,self.services.reborrow()));
    }
    fn spawn(&mut self,s:&mut Store,r:Ref<Character>,class:Option<StrId>,post:SpawnPost,at:[f64;3],yaw:Option<f64>)->Option<Ref<Unit>> {
        with_store(s,||self.lifecycle.spawn(r,class,post,at,yaw,self.services.reborrow()))
    }
    fn request_objective(&mut self,s:&mut Store,r:Ref<Character>) {
        with_store(s,||{self.lifecycle.request_objective(r);});
    }
    fn republish(&mut self,s:&mut Store,r:Ref<Character>) {
        with_store(s,||{self.lifecycle.republish(r);});
    }
    fn cast_spawn(&mut self,from:Vector3,direction:Vector3,span:f64,body:Ref<Body>)->Option<Vector3> {
        crate::units::native::ordnance_cast(f3::collision_world(),array(from),array(direction),span,Some(body),true,false)
            .map(|h|Vector3::new(h.point[0],h.point[1],h.point[2]))
    }
    fn effect(&mut self,_:&mut Store,name:&str,position:[f64;3]) {
        let name=match name {
            "explosion"=>self.effects[0],"med_explosion"=>self.effects[1],"small_explosion"=>self.effects[2],
            _=>panic!("prepared kill-region effect"),
        }; F4Effect{name,position}.emit(&mut sim().events);
    }
    fn post_event(&mut self,s:&mut Store,event:&PostEvent) {
        with_store(s,||{self.lifecycle.post_event(event);});
    }
    fn mobile_event(&mut self,s:&mut Store,event:&MobileEvent) {
        with_store(s,||{self.lifecycle.mobile_event(event);});
    }
    fn battle_over(&mut self,s:&mut Store,loser:f64) {
        with_store(s,||{self.lifecycle.battle_over(loser);});
    }
    fn human_tail(&mut self,s:&mut Store,dt:f64,commands:&[super::human::Command],prepared:&mut super::owned_world::Prepared) {
        self.human.step(s,dt,commands,prepared,self.services.reborrow(),self.lifecycle,self.local);
    }
    fn corpse_tail(&mut self,s:&mut Store) {
        let Some(r)=self.local else{return};
        let u=s.unit.get(r).expect("viewpoint corpse");
        if u.combat.dead && !u.removed && u.combat.death_time>=crate::units::unit::DEATH_LINGER {
            with_store(s,||f3::remove_native(r,self.services.reborrow()));
        }
    }
}
