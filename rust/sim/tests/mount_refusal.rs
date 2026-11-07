//! A retired mount must fail eligibility before any aim or hull observation.
use bfe_sim::ai::vehicles::{service::{MountWorld,mount_angles},world::*};
use three::Vector3;
struct Retired;
impl MountWorld for Retired {
    fn mount_unit(&self,_:Obj)->UnitView {panic!("unused unit")}
    fn mount_lease(&self,_:Obj)->LeaseView {panic!("unused lease")}
    fn mount_hull(&self,_:Obj)->HullView {panic!("retired hull must not be observed")}
    fn mount_turret(&self,_:Obj)->TurretView {panic!("unused turret")}
    fn mount_seat_count(&self,_:Obj)->usize {0}
    fn mount_seat(&self,_:Obj,_:usize)->Option<SeatView> {None}
    fn mount_eye(&mut self,_:Origin)->Option<Vector3> {None}
    fn mount_hold_yaw(&mut self,_:Obj,_:f64)->f64 {panic!("unused yaw")}
}
#[test]
fn retired_lease_or_seat_refuses_mount_angles() {
    for origin in [Origin::Lease(7),Origin::SeatOrigin(8,0)] {
        assert!(!mount_angles(&mut Retired,Some(8),None,0.,origin,&Vector3::new(0.,0.,10.)));
    }
    assert!(!mount_angles(&mut Retired,None,Some(8),0.,Origin::TurretPivot(8),&Vector3::new(0.,0.,10.)));
}
