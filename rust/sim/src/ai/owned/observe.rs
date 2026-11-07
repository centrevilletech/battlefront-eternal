//! Unit/SeatUnit/VehicleTarget/TurretTarget geometry (src/units.js and
//! src/seats.js). Each method observes only its source geometry. In
//! particular, an aim point reads the lazy sphere, never the gun origin.
#[cfg(not(target_arch="wasm32"))]
use crate::{gen::Store,vehicles::f2_seats::Seats};
use super::rows::{row_body,row_buildings,row_hull,row_seats,row_seats_mut,row_store,row_store_mut,row_traps};
use super::{arsenals::Mounted,entities::Entity,rows::Rows,weapons::Changes};
use crate::{ai::combat::world::{Obj,Sphere},gen::{RepairTarget,SeatLease,Vehicle},
    store::Ref,units::world::UnitsWorld};
use three::Vector3;


#[derive(Clone,Copy)]
struct ForwardSample {storage:u32,life:u64,yaw:u64,pitch:u64,value:Vector3}
/// Unit.forward is pure in its two current angles. Keep exact bits between
/// observations, including -0, and invalidate on either storage or game life.
/// Overflow remains an uncached native calculation, never a phase allocation.
pub struct ForwardCache {rows:Vec<Option<ForwardSample>>}
impl ForwardCache {
    pub fn new(capacity:usize)->Self {Self {rows:vec![None;capacity]}}
    pub fn read(&mut self,reference:Ref<crate::gen::Unit>,unit:&crate::gen::Unit)->Vector3 {
        let yaw=unit.aim_yaw();let pitch=unit.aim_pitch();
        let Some(slot)=self.rows.get_mut(reference.slot as usize) else{return unit.forward_on_foot()};
        if !yaw.is_finite()||!pitch.is_finite() {return unit.forward_on_foot();}
        let (life,yaw,pitch)=(unit.generation.to_bits(),yaw.to_bits(),pitch.to_bits());
        if let Some(old)=slot {if old.storage==reference.gen&&old.life==life&&old.yaw==yaw&&old.pitch==pitch {return old.value;}}
        let value=unit.forward_on_foot();*slot=Some(ForwardSample {storage:reference.gen,life,yaw,pitch,value});value
    }
}

/// Native hull rigs are retained separately from the canonical row. A read
/// loans that row's owner only for the duration of the requested seat pose.
pub trait Bodies {
    fn sphere(&mut self,#[cfg(target_arch="wasm32")] w:&mut crate::world::owner::World,
        #[cfg(not(target_arch="wasm32"))] store:&mut Store,#[cfg(not(target_arch="wasm32"))] seats:&mut Seats,unit:Ref<crate::gen::Unit>)->Option<Sphere>;
}
/// Isolated native worlds supply their own pose sampler, without a host.
pub struct Shapes<'a>(pub &'a mut dyn UnitsWorld);
impl Bodies for Shapes<'_> {
    fn sphere(&mut self,#[cfg(target_arch="wasm32")] w:&mut crate::world::owner::World,
        #[cfg(not(target_arch="wasm32"))] store:&mut Store,#[cfg(not(target_arch="wasm32"))] _seats:&mut Seats,unit:Ref<crate::gen::Unit>)->Option<Sphere> {
        #[cfg(target_arch="wasm32")] let store=&mut w.sim.store;
        let key=crate::gen::F3UnitKey::of(unit,store.unit.get(unit)?);
        super::geometry::sphere(store,self.0,&key).ok()
    }
}
pub trait Hulls {
    fn origin(&mut self,#[cfg(target_arch="wasm32")] w:&mut crate::world::owner::World,
        #[cfg(not(target_arch="wasm32"))] store:&mut Store,#[cfg(not(target_arch="wasm32"))] seats:&mut Seats,hull:Ref<Vehicle>,seat:usize)->Option<Vector3>;
    fn forward(&mut self,#[cfg(target_arch="wasm32")] w:&mut crate::world::owner::World,
        #[cfg(not(target_arch="wasm32"))] store:&mut Store,#[cfg(not(target_arch="wasm32"))] seats:&mut Seats,hull:Ref<Vehicle>,seat:usize)->Option<Vector3>;
}
impl<C:Changes+?Sized,M:Mounted> Rows<'_,C,M> {
    fn control_lease(&self,reference:Ref<crate::gen::SeatUnit>)->Option<Ref<SeatLease>> {
        self.state.controls.lease_reference(row_store!(self),reference).or_else(||
            self.state.turrets.lease_reference(row_store!(self),row_seats!(self),row_buildings!(self),reference))
    }
    pub(crate) fn lease_origin(&mut self,reference:Ref<SeatLease>,hulls:&mut impl Hulls)->Option<Vector3> {
        let lease=row_store_mut!(self).seat_lease.get(reference)?;
        if let Some(hull)=lease.vehicle {
            let index=lease.index as usize;
            row_hull!(self,hulls,origin,hull,index)
        }else{
            let turret=lease.turret?;
            self.state.turrets.origin(row_seats_mut!(self),row_buildings!(self),turret)
        }
    }
    fn lease_forward(&mut self,reference:Ref<SeatLease>,hulls:&mut impl Hulls)->Option<Vector3> {
        let lease=row_store_mut!(self).seat_lease.get(reference)?;
        if let Some(hull)=lease.vehicle {
            let index=lease.index as usize;
            row_hull!(self,hulls,forward,hull,index)
        }else{
            self.state.turrets.forward(row_seats_mut!(self),row_buildings!(self),lease.turret?)
        }
    }
    pub fn origin(&mut self,obj:Obj,hulls:&mut impl Hulls)->Option<Vector3> {
        match self.state.entities.get(obj)? {
            Entity::Unit(reference)=>{
                let unit=row_store_mut!(self).unit.get(reference)?;
                if let Some(lease)=unit.seat {self.lease_origin(lease,hulls)}else{Some(unit.origin_on_foot())}
            },
            Entity::Control(reference)=>self.lease_origin(self.control_lease(reference)?,hulls),
            Entity::HullEntry(reference)=>row_hull!(self,hulls,origin,reference,0),
            Entity::TurretEntry(reference)=>self.state.turrets.origin(row_seats_mut!(self),row_buildings!(self),reference),
            Entity::BuildingService(reference)|Entity::ObjectiveEntry(reference)=>
                Some(self.state.buildings.sphere(row_buildings!(self),row_traps!(self),RepairTarget::Building(reference))?.center),
            Entity::TurretService(reference)=>Some(self.state.buildings.sphere(row_buildings!(self),row_traps!(self),RepairTarget::Turret(reference))?.center),
            _=>None,
        }
    }
    pub fn forward(&mut self,obj:Obj,hulls:&mut impl Hulls)->Option<Vector3> {
        match self.state.entities.get(obj)? {
            Entity::Unit(reference)=>{
                let unit=row_store_mut!(self).unit.get(reference)?;
                if let Some(lease)=unit.seat {self.lease_forward(lease,hulls)}else{Some(self.state.forward_cache.read(reference,unit))}
            },
            Entity::Control(reference)=>self.lease_forward(self.control_lease(reference)?,hulls),
            Entity::HullEntry(reference)=>row_hull!(self,hulls,forward,reference,0),
            Entity::TurretEntry(reference)=>self.state.turrets.forward(row_seats_mut!(self),row_buildings!(self),reference),
            Entity::BuildingService(reference)|Entity::ObjectiveEntry(reference)=>self.state.buildings.forward(row_buildings!(self),row_traps!(self),RepairTarget::Building(reference)),
            Entity::TurretService(reference)=>self.state.buildings.forward(row_buildings!(self),row_traps!(self),RepairTarget::Turret(reference)),
            _=>None,
        }
    }
    pub fn sphere(&mut self,obj:Obj,bodies:&mut impl Bodies)->Option<Sphere> {
        match self.state.entities.get(obj)? {
            Entity::Unit(reference)=>row_body!(self,bodies,reference),
            Entity::Control(reference)=>{
                let key=self.control_lease(reference)?;
                let lease=row_store!(self).seat_lease.get(key)?;
                if let Some(hull)=lease.vehicle {self.state.mounts.sphere(row_store_mut!(self),hull)}else{
                    self.state.turrets.sphere(row_seats_mut!(self),row_buildings!(self),lease.turret?)
                }
            },
            Entity::HullEntry(reference)=>self.state.mounts.sphere(row_store_mut!(self),reference),
            Entity::TurretEntry(reference)=>self.state.turrets.sphere(row_seats_mut!(self),row_buildings!(self),reference),
            Entity::BuildingService(reference)|Entity::ObjectiveEntry(reference)=>self.state.buildings.sphere(row_buildings!(self),row_traps!(self),RepairTarget::Building(reference)),
            Entity::TurretService(reference)=>self.state.buildings.sphere(row_buildings!(self),row_traps!(self),RepairTarget::Turret(reference)),
            _=>None,
        }
    }
    pub fn aim_point(&mut self,obj:Obj,bodies:&mut impl Bodies)->Option<Vector3> {Some(self.sphere(obj,bodies)?.center)}
}
#[cfg(target_arch="wasm32")]
impl Hulls for super::geometry::F2 {
    fn origin(&mut self,w:&mut crate::world::owner::World,hull:Ref<Vehicle>,seat:usize)->Option<Vector3> {
        let life=w.sim.store.vehicle.get(hull)?.target_generation;
        crate::vehicles::module::f2::seat_origin_in(w,crate::vehicles::f2_seats::HullLife {vehicle:hull,life},seat)
    }
    fn forward(&mut self,w:&mut crate::world::owner::World,hull:Ref<Vehicle>,seat:usize)->Option<Vector3> {
        let life=w.sim.store.vehicle.get(hull)?.target_generation;
        crate::vehicles::module::f2::seat_forward_in(w,crate::vehicles::f2_seats::HullLife {vehicle:hull,life},seat)
    }
}

/// The live sampler borrows the root for canonical lazy geometry. No local
/// Store borrow spans held or rider observations.
#[cfg(target_arch="wasm32")]
pub struct F3;
#[cfg(target_arch="wasm32")]
impl Bodies for F3 {
    fn sphere(&mut self,w:&mut crate::world::owner::World,unit:Ref<crate::gen::Unit>)->Option<Sphere> {
        let row=w.sim.store.unit.get(unit)?;
        // Unit.sphere returns its already-observed bounds at this exact tick.
        // Borrow those scalars directly; only first observation may need the
        // native held/rider sampler and its cross-owner scope.
        if row.spheres_tick==row.tick_count {
            return Some(Sphere {center:Vector3::new(row.bounds.center[0],row.bounds.center[1],row.bounds.center[2]),radius:row.bounds.radius});
        }
        let (center,radius)=crate::units::f3_sensors::unit_sphere_in(w,unit);
        Some(Sphere {center:Vector3::new(center[0],center[1],center[2]),radius})
    }
}

#[cfg(test)]
mod forward_cache_tests {
    use super::*;
    #[test]
    fn changed_angles_lives_and_overflow_keep_exact_forward_bits() {
        let mut cache=ForwardCache::new(1);let mut unit=crate::gen::Unit::default();
        let r=Ref::new(0,1);
        for (life,yaw,pitch) in [(1.,0.,0.),(1.,-0.,0.),(1.,0.3,-0.2),(2.,-0.7,0.4),(2.,f64::INFINITY,0.)] {
            unit.generation=life;
            let crate::gen::UnitBody::Soldier(body)=&mut unit.soldier else{unreachable!()};
            body.aim_yaw=yaw;body.aim_pitch=pitch;
            let expected=unit.forward_on_foot();
            for key in [r,r,Ref::new(0,2),Ref::new(7,1)] {
                let actual=cache.read(key,&unit);
                assert_eq!([actual.x.to_bits(),actual.y.to_bits(),actual.z.to_bits()],
                    [expected.x.to_bits(),expected.y.to_bits(),expected.z.to_bits()]);
            }
        }
        assert_eq!(cache.rows.len(),1);
    }
}
