//! Game.los: collision first, then the ordered unit registry. Prepared body
//! identities belong to authored props; a rider's current holder is resolved
//! from the seat slab at the query, rather than cached with the controller.
#[cfg(not(target_arch="wasm32"))]
use crate::vehicles::f2_seats::Seats;
use super::{entities::{Entities,Entity},registry::Queries,seats::{Controls,Source}};
use crate::{gen::{Body,Member,SeatLease,Store},store::Ref};
use crate::ai::combat::world::Obj;
use three::Vector3;


#[cfg(target_arch="wasm32")]
macro_rules! sight_store {($w:expr,$s:expr)=>{&($w).sim.store};}
#[cfg(not(target_arch="wasm32"))]
macro_rules! sight_store {($w:expr,$s:expr)=>{&*($s)};}
#[cfg(target_arch="wasm32")]
macro_rules! sight_seats {($w:expr,$s:expr)=>{crate::vehicles::module::f2::live_seats::seats_ref(&($w).fleet)};}
#[cfg(not(target_arch="wasm32"))]
macro_rules! sight_seats {($w:expr,$s:expr)=>{&*($s)};}
#[cfg(target_arch="wasm32")]
macro_rules! sight_call {($o:expr,$method:ident,$w:expr,$store:expr,$seats:expr,$($a:expr),* $(,)?)=>{($o).$method($w,$($a),*)};}
#[cfg(not(target_arch="wasm32"))]
macro_rules! sight_call {($o:expr,$method:ident,$w:expr,$store:expr,$seats:expr,$($a:expr),* $(,)?)=>{($o).$method($store,$seats,$($a),*)};}

pub struct Sight {bodies:Vec<Option<(Obj,Ref<Body>)>>}
impl Sight {
    pub fn new(capacity:usize)->Self {Self {bodies:vec![None;capacity]}}
    /// Built-body identity is retained even while a prop shows its wreck.
    /// Collision enablement remains with F4 and is never copied here.
    pub fn prepare(&mut self,obj:Obj,body:Ref<Body>) {
        let slot=obj as u32 as usize;
        if self.bodies.len()<=slot {self.bodies.resize(slot+1,None);}
        self.bodies[slot]=Some((obj,body));
    }
    fn authored(&self,entities:&Entities,entity:Entity)->Option<Ref<Body>> {
        let obj=entities.find(entity)?;let (current,body)=self.bodies.get(obj as u32 as usize).copied().flatten()?;
        (current==obj).then_some(body)
    }
    fn lease(&self,store:&Store,entities:&Entities,lease:Ref<SeatLease>)->Option<Ref<Body>> {
        let l=store.seat_lease.get(lease)?;
        if let Some(hull)=l.vehicle {return Some(store.vehicle.get(hull)?.body);}
        self.authored(entities,Entity::Turret(l.turret?))
    }
    pub fn body(&self,store:&Store,entities:&Entities,controls:&Controls,obj:Obj)->Option<Ref<Body>> {
        match entities.get(obj)? {
            Entity::Unit(r)=>self.lease(store,entities,store.unit.get(r)?.seat?),
            Entity::Control(r)=>match controls.read(r)?.source {
                Source::Hull(r,_)=>Some(store.vehicle.get(r)?.body),
                Source::Turret(r,_)=>self.authored(entities,Entity::Turret(r)),
            },
            Entity::Hull(r)|Entity::HullEntry(r)=>Some(store.vehicle.get(r)?.body),
            Entity::Turret(r)|Entity::TurretEntry(r)|Entity::TurretService(r)=>self.authored(entities,Entity::Turret(r)),
            Entity::Building(r)|Entity::BuildingService(r)|Entity::ObjectiveEntry(r)=>self.authored(entities,Entity::Building(r)),
            _=>None,
        }
    }
    pub fn los(&self,#[cfg(target_arch="wasm32")] w:&mut crate::world::owner::World,#[cfg(not(target_arch="wasm32"))] store:&mut Store,#[cfg(not(target_arch="wasm32"))] seats:&mut Seats,entities:&Entities,controls:&Controls,queries:&Queries,
        world:&mut impl World,from:&Vector3,to:&Vector3,a:Option<Obj>,b:Option<Obj>)->bool {
        let mut direction=Vector3::default();direction.sub_vectors(to,from);let span=direction.length();
        if span<1e-6 {return true;}
        direction.divide_scalar(span);
        let first=a.and_then(|o|self.body(sight_store!(w,store),entities,controls,o));
        if let Some(hit)=sight_call!(world,cast,w,store,seats,from,&direction,span,first) {
            let second=b.and_then(|o|self.body(sight_store!(w,store),entities,controls,o));
            if second.is_none()||hit!=second {return false;}
        }
        let mut excluded=[Member::Unit(Ref::new(0,0));2];let mut count=0;
        for obj in [a,b].into_iter().flatten() {
            if let Some(member)=queries.member(entities,obj) {excluded[count]=member;count+=1;}
        }
        !sight_call!(world,raycast,w,store,seats,from,&direction,span,&excluded[..count])
    }
}
pub trait World {
    /// Some(None) is a terrain hit; None is a clear collision segment.
    fn cast(&mut self,#[cfg(target_arch="wasm32")] w:&mut crate::world::owner::World,#[cfg(not(target_arch="wasm32"))] store:&mut Store,#[cfg(not(target_arch="wasm32"))] seats:&mut Seats,from:&Vector3,direction:&Vector3,span:f64,ignore:Option<Ref<Body>>)->Option<Option<Ref<Body>>>;
    fn raycast(&mut self,#[cfg(target_arch="wasm32")] w:&mut crate::world::owner::World,#[cfg(not(target_arch="wasm32"))] store:&mut Store,#[cfg(not(target_arch="wasm32"))] seats:&mut Seats,from:&Vector3,direction:&Vector3,span:f64,exclude:&[Member])->bool;
}
/// goto.js clearAbove uses the game's LOS service: ignore the rider's hull
/// collision body and its own registry member, then test the full 40 m span.
/// Owners are lent explicitly so lazy F3 shape reads cannot reacquire them.
pub fn clear_above(#[cfg(target_arch="wasm32")] w:&mut crate::world::owner::World,#[cfg(not(target_arch="wasm32"))] store:&mut Store,#[cfg(not(target_arch="wasm32"))] seats:&mut Seats,world:&mut impl World,from:&Vector3,unit:&crate::gen::AiLifeHandle)->bool {
    let Some(row)=unit.unit(sight_store!(w,store)) else{return false};
    let lease=row.seat.and_then(|r|sight_store!(w,store).seat_lease.get(r));
    let ignore=lease.and_then(|l| {
        if let Some(hull)=l.vehicle {sight_store!(w,store).vehicle.get(hull).map(|h|h.body)}else{
            l.turret.and_then(|r|sight_seats!(w,seats).crews.turrets.iter().find(|t|t.turret==r)).and_then(|t|t.desc.body)
        }
    });
    let direction=Vector3::new(0.,1.,0.);
    if sight_call!(world,cast,w,store,seats,from,&direction,40.,ignore).is_some() {return false;}
    !sight_call!(world,raycast,w,store,seats,from,&direction,40.,&[Member::Unit(Ref::new(unit.slot,unit.generation))])
}
#[cfg(target_arch="wasm32")]
pub struct F3;
#[cfg(target_arch="wasm32")]
impl World for F3 {
    fn cast(&mut self,w:&mut crate::world::owner::World,from:&Vector3,direction:&Vector3,span:f64,ignore:Option<Ref<Body>>)->Option<Option<Ref<Body>>> {
        crate::units::native::ordnance_cast_in(w,crate::units::f3::collision_world_in(&w.units),
            [from.x,from.y,from.z],[direction.x,direction.y,direction.z],span,ignore,false,false).map(|hit|hit.body)
    }
    fn raycast(&mut self,w:&mut crate::world::owner::World,from:&Vector3,direction:&Vector3,span:f64,exclude:&[Member])->bool {
        crate::units::f3::raycast_in(w,[from.x,from.y,from.z],[direction.x,direction.y,direction.z],span,
                crate::units::registry::Exclude::Many(exclude),false,false).is_some()
    }
}
