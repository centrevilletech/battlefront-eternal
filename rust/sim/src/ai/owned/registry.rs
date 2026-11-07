//! AI registry queries use F3's ordered broadphase and lazy native spheres.
//! Scratch buffers are reserved at construction and returned across reentry.
use crate::{gen::{Member,Store,Unit},store::DefId,units::world::UnitsWorld};
use super::{scratch::{Pool,List},population::Recruitment};
use crate::ai::{nav::math::Vec3,control::world::TeamTable};

pub trait TargetTeams {fn team(&self,member:Member)->f64;}
pub struct Queries {
    members:Pool<Member>,
    objects:Pool<crate::ai::combat::world::Obj>,
    targets:Vec<Option<(Member,crate::ai::combat::world::Obj)>>,
    masks:Vec<Option<f64>>,
    reverse:Vec<Option<(crate::ai::combat::world::Obj,Member)>>,
}
impl Queries {
    pub fn new(capacity:usize,depth:usize)->Self {Self {members:Pool::new(depth,capacity),objects:Pool::new(depth,capacity),targets:vec![None;capacity],masks:Vec::new(),reverse:vec![None;capacity]}}
    /// Bind F2/F4 target identities at construction. Registry ids and hull
    /// slab slots are independent; retaining the full member rejects reuse.
    pub fn target(&mut self,member:Member,obj:crate::ai::combat::world::Obj) {
        assert!(!matches!(member,Member::Unit(_)),"unit identities resolve directly");
        let id=crate::units::registry::target_id(member) as usize;
        if self.targets.len()<=id {self.targets.resize(id+1,None);}
        self.targets[id]=Some((member,obj));
        let slot=obj as u32 as usize;
        if self.reverse.len()<=slot {self.reverse.resize(slot+1,None);}
        self.reverse[slot]=Some((obj,member));
    }
    pub fn object(&self,entities:&super::entities::Entities,member:Member)->Option<crate::ai::combat::world::Obj> {
        match member {
            Member::Unit(r)=>entities.find(super::entities::Entity::Unit(r)),
            _=>self.targets.get(crate::units::registry::target_id(member) as usize).copied().flatten()
                .filter(|(current,obj)|*current==member&&entities.get(*obj).is_some()).map(|(_,obj)|obj),
        }
    }
    /// Reverse the prepared registry binding for native LOS and target locks.
    /// The full object token and current member both reject replacement, even
    /// when a stale holder still has the registry id's earlier object token.
    pub fn member(&self,entities:&super::entities::Entities,obj:crate::ai::combat::world::Obj)->Option<Member> {
        if let super::entities::Entity::Unit(r)=entities.get(obj)? {return Some(Member::Unit(r));}
        let (current,member)=self.reverse.get(obj as u32 as usize).copied().flatten()?;
        let id=crate::units::registry::target_id(member) as usize;
        (current==obj&&self.targets.get(id).copied().flatten()==Some((member,obj))).then_some(member)
    }
    pub fn objects(&self)->List<crate::ai::combat::world::Obj> {self.objects.take()}
    /// Combat and danger scans share the registry's original lazy sphere
    /// point and ordering. Relation filtering happens after the sphere query.
    pub fn query(&self,store:&mut Store,world:&mut dyn UnitsWorld,targets:&dyn TargetTeams,
        entities:&super::entities::Entities,center:&three::Vector3,radius:f64,
        filter:&mut dyn FnMut(crate::ai::combat::world::Obj,f64)->bool)->List<crate::ai::combat::world::Obj> {
        self.query_world(&mut Borrowed {store,world,targets},entities,center,radius,filter)
    }
    pub fn query_world(&self,world:&mut impl World,entities:&super::entities::Entities,
        center:&three::Vector3,radius:f64,filter:&mut dyn FnMut(crate::ai::combat::world::Obj,f64)->bool)->List<crate::ai::combat::world::Obj> {
        let mut out=self.objects();let mut at=0;
        while at<world.len() {
            let member=world.member(at);at+=1;
            if !world.alive(member) {continue;}
            let sphere=world.sphere(member);let reach=radius+sphere.radius;
            if crate::units::shapes::v3(sphere.center).distance_to_squared(center)>reach*reach {continue;}
            let obj=self.object(entities,member).expect("prepared native AI registry identity");
            if filter(obj,world.team(member)) {out.push_reserved(obj);}
        }
        out
    }
    pub fn living(&self,store:&Store,world:&mut dyn UnitsWorld,entities:&super::entities::Entities)->List<crate::ai::combat::world::Obj> {
        let mut out=self.objects();
        for &member in &store.units.list {
            let alive=match member {Member::Unit(r)=>store.unit.get(r).is_some_and(|u|u.alive),_=>world.target_alive(crate::units::registry::target_id(member))};
            if alive {out.push_reserved(self.object(entities,member).expect("prepared living AI identity"));}
        }
        out
    }
    pub fn living_world(&self,world:&mut impl World,entities:&super::entities::Entities)->List<crate::ai::combat::world::Obj> {
        let mut out=self.objects();let mut at=0;
        while at<world.len() {
            let member=world.member(at);at+=1;
            if world.alive(member) {out.push_reserved(self.object(entities,member).expect("prepared living AI identity"));}
        }
        out
    }

    pub fn class(&mut self,class:DefId,mask:f64) {
        let i=class.0 as usize;if self.masks.len()<=i {self.masks.resize(i+1,None);}
        self.masks[i]=Some(mask);
    }
    pub fn bind<'a>(&'a self,world:&'a mut dyn UnitsWorld,targets:&'a dyn TargetTeams)->Registry<'a> {Registry {queries:self,world,targets}}
}
pub struct Registry<'a> {queries:&'a Queries,world:&'a mut dyn UnitsWorld,targets:&'a dyn TargetTeams}
impl Recruitment for Registry<'_> {
    fn query(&mut self,store:&mut Store,center:Vec3,radius:f64,team:f64,teams:&TeamTable)->List<Member> {
        let mut out=self.queries.members.take();
        assert!(out.capacity()>=store.units.list.len(),"registry growth must reserve AI query capacity");
        let targets=self.targets;
        store.units.query_with(&mut store.unit,&three::Vector3::new(center.x,center.y,center.z),radius,&mut out,
            &mut |member,unit,_|teams.relation(team,unit.map(|u|u.team).unwrap_or_else(||targets.team(member)))>0.,self.world);
        out
    }
    fn mask(&self,unit:&Unit)->f64 {self.queries.masks[unit.class_def.0 as usize].expect("prepared AI unit traversal")}
}
#[cfg(target_arch="wasm32")]
pub struct F3Teams;
#[cfg(target_arch="wasm32")]
impl TargetTeams for F3Teams {
    fn team(&self,member:Member)->f64 {
        crate::units::f3::target(crate::units::registry::target_id(member)).expect("current native registry target").team
    }
}

/// Short observations permit native pose/target readers to loan their owners
/// without retaining a mutable Store reference across the geometry call.
pub trait World {
    fn len(&self)->usize;
    fn member(&self,index:usize)->Member;
    fn alive(&mut self,member:Member)->bool;
    fn sphere(&mut self,member:Member)->crate::gen::Bounds;
    fn team(&self,member:Member)->f64;
}
struct Borrowed<'a> {store:&'a mut Store,world:&'a mut dyn UnitsWorld,targets:&'a dyn TargetTeams}
impl World for Borrowed<'_> {
    fn len(&self)->usize {self.store.units.list.len()}
    fn member(&self,index:usize)->Member {self.store.units.list[index]}
    fn alive(&mut self,member:Member)->bool {match member {
        Member::Unit(r)=>self.store.unit.get(r).is_some_and(|u|u.alive),
        _=>self.world.target_alive(crate::units::registry::target_id(member)),
    }}
    fn sphere(&mut self,member:Member)->crate::gen::Bounds {match member {
        Member::Unit(r)=>self.store.unit.get_mut(r).unwrap().sphere(r.slot,self.world),
        _=>self.world.target_sphere(crate::units::registry::target_id(member)),
    }}
    fn team(&self,member:Member)->f64 {match member {Member::Unit(r)=>self.store.unit.get(r).unwrap().team,_=>self.targets.team(member)}}
}
#[cfg(target_arch="wasm32")]
pub struct F3;
#[cfg(target_arch="wasm32")]
impl World for F3 {
    fn len(&self)->usize {crate::abi::sim().store.units.list.len()}
    fn member(&self,index:usize)->Member {crate::abi::sim().store.units.list[index]}
    fn alive(&mut self,member:Member)->bool {match member {
        Member::Unit(r)=>crate::abi::sim().store.unit.get(r).is_some_and(|u|u.alive),
        _=>crate::units::f3::target(crate::units::registry::target_id(member)).is_some_and(|t|t.alive),
    }}
    fn sphere(&mut self,member:Member)->crate::gen::Bounds {match member {
        Member::Unit(r)=>{let (center,radius)=crate::units::f3_sensors::unit_sphere(r);crate::gen::Bounds {center,radius}},
        _=>{let t=crate::units::f3::target(crate::units::registry::target_id(member)).expect("current native registry target");crate::gen::Bounds {center:t.bounds.center,radius:t.bounds.radius}},
    }}
    fn team(&self,member:Member)->f64 {match member {
        Member::Unit(r)=>crate::abi::sim().store.unit.get(r).unwrap().team,
        _=>crate::units::f3::target(crate::units::registry::target_id(member)).expect("current native registry target").team,
    }}
}
#[cfg(target_arch="wasm32")]
impl Queries {
    pub fn query_live(&self,store:&mut Store,seats:&mut crate::vehicles::f2_seats::Seats,entities:&super::entities::Entities,center:&three::Vector3,radius:f64,
        filter:crate::ai::combat::world::QueryFilter)->List<crate::ai::combat::world::Obj> {
        crate::vehicles::module::f2::live_seats::loan_seats(seats,||
            crate::battle::f4_live::with_store(store,||self.query_world(&mut F3,entities,center,radius,
                &mut |obj,team|filter.keeps(obj,team,&mut crate::units::f3::relation))))
    }
    pub fn living_live(&self,store:&mut Store,seats:&mut crate::vehicles::f2_seats::Seats,entities:&super::entities::Entities)->List<crate::ai::combat::world::Obj> {
        crate::vehicles::module::f2::live_seats::loan_seats(seats,||
            crate::battle::f4_live::with_store(store,||self.living_world(&mut F3,entities)))
    }
}
