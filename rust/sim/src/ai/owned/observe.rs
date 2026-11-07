//! Unit/SeatUnit/VehicleTarget/TurretTarget geometry (src/units.js and
//! src/seats.js). Each method observes only its source geometry. In
//! particular, an aim point reads the lazy sphere, never the gun origin.
use super::{arsenals::Mounted,entities::Entity,rows::Rows,weapons::Changes};
use crate::{ai::combat::world::{Obj,Sphere},gen::{RepairTarget,SeatLease,Store,Vehicle},
    store::Ref,units::world::UnitsWorld};
use three::Vector3;
use crate::vehicles::f2_seats::Seats;

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
    fn sphere(&mut self,store:&mut Store,seats:&mut Seats,unit:Ref<crate::gen::Unit>)->Option<Sphere>;
}
/// Isolated native worlds supply their own pose sampler, without a host.
pub struct Shapes<'a>(pub &'a mut dyn UnitsWorld);
impl Bodies for Shapes<'_> {
    fn sphere(&mut self,store:&mut Store,_seats:&mut Seats,unit:Ref<crate::gen::Unit>)->Option<Sphere> {
        let key=crate::gen::F3UnitKey::of(unit,store.unit.get(unit)?);
        super::geometry::sphere(store,self.0,&key).ok()
    }
}
pub trait Hulls {
    fn origin(&mut self,store:&mut Store,seats:&mut Seats,hull:Ref<Vehicle>,seat:usize)->Option<Vector3>;
    fn forward(&mut self,store:&mut Store,seats:&mut Seats,hull:Ref<Vehicle>,seat:usize)->Option<Vector3>;
}
impl<C:Changes+?Sized,M:Mounted> Rows<'_,C,M> {
    fn control_lease(&self,reference:Ref<crate::gen::SeatUnit>)->Option<Ref<SeatLease>> {
        self.state.controls.lease_reference(self.store,reference).or_else(||
            self.state.turrets.lease_reference(self.store,self.seats,self.buildings.rows(),reference))
    }
    pub(crate) fn lease_origin(&mut self,reference:Ref<SeatLease>,hulls:&mut impl Hulls)->Option<Vector3> {
        let lease=self.store.seat_lease.get(reference)?;
        if let Some(hull)=lease.vehicle {
            let index=lease.index as usize;
            hulls.origin(self.store,self.seats,hull,index)
        }else{
            let turret=lease.turret?;
            self.state.turrets.origin(self.seats,self.buildings.rows(),turret)
        }
    }
    fn lease_forward(&mut self,reference:Ref<SeatLease>,hulls:&mut impl Hulls)->Option<Vector3> {
        let lease=self.store.seat_lease.get(reference)?;
        if let Some(hull)=lease.vehicle {
            let index=lease.index as usize;
            hulls.forward(self.store,self.seats,hull,index)
        }else{
            self.state.turrets.forward(self.seats,self.buildings.rows(),lease.turret?)
        }
    }
    pub fn origin(&mut self,obj:Obj,hulls:&mut impl Hulls)->Option<Vector3> {
        match self.state.entities.get(obj)? {
            Entity::Unit(reference)=>{
                let unit=self.store.unit.get(reference)?;
                if let Some(lease)=unit.seat {self.lease_origin(lease,hulls)}else{Some(unit.origin_on_foot())}
            },
            Entity::Control(reference)=>self.lease_origin(self.control_lease(reference)?,hulls),
            Entity::HullEntry(reference)=>hulls.origin(self.store,self.seats,reference,0),
            Entity::TurretEntry(reference)=>self.state.turrets.origin(self.seats,self.buildings.rows(),reference),
            Entity::BuildingService(reference)|Entity::ObjectiveEntry(reference)=>
                Some(self.state.buildings.sphere(self.buildings.rows(),self.buildings.traps(),RepairTarget::Building(reference))?.center),
            Entity::TurretService(reference)=>Some(self.state.buildings.sphere(self.buildings.rows(),self.buildings.traps(),RepairTarget::Turret(reference))?.center),
            _=>None,
        }
    }
    pub fn forward(&mut self,obj:Obj,hulls:&mut impl Hulls)->Option<Vector3> {
        match self.state.entities.get(obj)? {
            Entity::Unit(reference)=>{
                let unit=self.store.unit.get(reference)?;
                if let Some(lease)=unit.seat {self.lease_forward(lease,hulls)}else{Some(self.state.forward_cache.read(reference,unit))}
            },
            Entity::Control(reference)=>self.lease_forward(self.control_lease(reference)?,hulls),
            Entity::HullEntry(reference)=>hulls.forward(self.store,self.seats,reference,0),
            Entity::TurretEntry(reference)=>self.state.turrets.forward(self.seats,self.buildings.rows(),reference),
            Entity::BuildingService(reference)|Entity::ObjectiveEntry(reference)=>self.state.buildings.forward(self.buildings.rows(),self.buildings.traps(),RepairTarget::Building(reference)),
            Entity::TurretService(reference)=>self.state.buildings.forward(self.buildings.rows(),self.buildings.traps(),RepairTarget::Turret(reference)),
            _=>None,
        }
    }
    pub fn sphere(&mut self,obj:Obj,bodies:&mut impl Bodies)->Option<Sphere> {
        match self.state.entities.get(obj)? {
            Entity::Unit(reference)=>bodies.sphere(self.store,self.seats,reference),
            Entity::Control(reference)=>{
                let lease=self.store.seat_lease.get(self.control_lease(reference)?)?;
                if let Some(hull)=lease.vehicle {self.state.mounts.sphere(self.store,hull)}else{
                    self.state.turrets.sphere(self.seats,self.buildings.rows(),lease.turret?)
                }
            },
            Entity::HullEntry(reference)=>self.state.mounts.sphere(self.store,reference),
            Entity::TurretEntry(reference)=>self.state.turrets.sphere(self.seats,self.buildings.rows(),reference),
            Entity::BuildingService(reference)|Entity::ObjectiveEntry(reference)=>self.state.buildings.sphere(self.buildings.rows(),self.buildings.traps(),RepairTarget::Building(reference)),
            Entity::TurretService(reference)=>self.state.buildings.sphere(self.buildings.rows(),self.buildings.traps(),RepairTarget::Turret(reference)),
            _=>None,
        }
    }
    pub fn aim_point(&mut self,obj:Obj,bodies:&mut impl Bodies)->Option<Vector3> {Some(self.sphere(obj,bodies)?.center)}
}
#[cfg(target_arch="wasm32")]
impl Hulls for super::geometry::F2 {
    fn origin(&mut self,store:&mut Store,_seats:&mut Seats,hull:Ref<Vehicle>,seat:usize)->Option<Vector3> {
        let life=store.vehicle.get(hull)?.target_generation;
        crate::vehicles::module::f2::seat_origin_owned(store,crate::vehicles::f2_seats::HullLife {vehicle:hull,life},seat)
    }
    fn forward(&mut self,store:&mut Store,_seats:&mut Seats,hull:Ref<Vehicle>,seat:usize)->Option<Vector3> {
        let life=store.vehicle.get(hull)?.target_generation;
        crate::vehicles::module::f2::seat_forward_owned(store,crate::vehicles::f2_seats::HullLife {vehicle:hull,life},seat)
    }
}

/// The live sampler loans Store back to F3 before it observes held/rider
/// geometry. No local Store borrow spans those native owner reads.
#[cfg(target_arch="wasm32")]
pub struct F3;
#[cfg(target_arch="wasm32")]
impl Bodies for F3 {
    fn sphere(&mut self,store:&mut Store,seats:&mut Seats,unit:Ref<crate::gen::Unit>)->Option<Sphere> {
        let row=store.unit.get(unit)?;
        // Unit.sphere returns its already-observed bounds at this exact tick.
        // Borrow those scalars directly; only first observation may need the
        // native held/rider sampler and its cross-owner scope.
        if row.spheres_tick==row.tick_count {
            return Some(Sphere {center:Vector3::new(row.bounds.center[0],row.bounds.center[1],row.bounds.center[2]),radius:row.bounds.radius});
        }
        // Ordinary on-foot bounds use Unit.updateSpheres' local primitives.
        // Only an exposed seat or a held frame can ask its world for a pose;
        // keep that lazy sampler path, without loaning Store for plain bodies.
        let held=matches!(&row.held,crate::store::Nullable::Value(h) if !matches!(h.frame,crate::store::Nullable::Null|crate::store::Nullable::Undefined));
        if row.seat.is_none()&&!held {
            let key=crate::gen::F3UnitKey::of(unit,row);
            let mut world=crate::units::module::ModuleWorld::sensor(crate::units::f3::collision_world());
            return super::geometry::sphere(store,&mut world,&key).ok();
        }
        let (center,radius)=crate::vehicles::module::f2::live_seats::loan_seats(seats,||
            crate::battle::f4_live::with_store(store,||crate::units::f3_sensors::unit_sphere(unit)));
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
