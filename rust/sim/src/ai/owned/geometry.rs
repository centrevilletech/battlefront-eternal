//! Geometry is observed only when an AI algorithm asks for it. Unit scalar
//! reads never refresh lazy hit shapes or ask a seat to pose its gun.
use crate::{
    gen::{F3UnitKey, SeatLease, Store},
    store::Ref,
    units::{ownership::Stale, world::UnitsWorld},
};
use three::Vector3;
pub trait Seats {
    fn origin(&mut self, lease: Ref<SeatLease>) -> Option<Vector3>;
    fn forward(&mut self, lease: Ref<SeatLease>) -> Option<Vector3>;
}
pub fn origin(store: &Store, seats: &mut dyn Seats, key: &F3UnitKey) -> Result<Vector3, Stale> {
    let unit = key.validate(store)?;
    if let Some(lease) = unit.seat {
        Ok(seats.origin(lease).expect("current native AI seat origin"))
    } else {
        Ok(unit.origin_on_foot())
    }
}
pub fn forward(store: &Store, seats: &mut dyn Seats, key: &F3UnitKey) -> Result<Vector3, Stale> {
    let unit = key.validate(store)?;
    if let Some(lease) = unit.seat {
        Ok(seats
            .forward(lease)
            .expect("current native AI seat forward"))
    } else {
        Ok(unit.forward_on_foot())
    }
}
pub fn sphere(
    store: &mut Store,
    world: &mut dyn UnitsWorld,
    key: &F3UnitKey,
) -> Result<crate::ai::combat::world::Sphere, Stale> {
    let unit = key.validate_mut(store)?;
    let sphere = unit.sphere(key.slot, world);
    Ok(crate::ai::combat::world::Sphere {
        center: Vector3::new(sphere.center[0], sphere.center[1], sphere.center[2]),
        radius: sphere.radius,
    })
}
#[cfg(target_arch = "wasm32")]
pub struct F2;
#[cfg(target_arch = "wasm32")]
impl Seats for F2 {
    fn origin(&mut self, lease: Ref<SeatLease>) -> Option<Vector3> {
        crate::vehicles::module::f2::live_seats::lease_origin(lease)
    }
    fn forward(&mut self, lease: Ref<SeatLease>) -> Option<Vector3> {
        crate::vehicles::module::f2::live_seats::lease_forward(lease)
    }
}
