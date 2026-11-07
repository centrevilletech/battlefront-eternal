//! Movement helpers need navigation and current scalar motion, without a
//! second combat owner. Full AI worlds provide these same observations.
use super::world::*;
use crate::ai::nav::math::Vec3;
pub trait World {
    fn unit(&self, key: UKey) -> AiUnit;
    fn unit_scalars(&self,key:UKey)->UnitScalars {UnitScalars::of(&self.unit(key))}
    fn helper_request_invalidated(&mut self, _controller:usize) {}
    fn helper_goto(&mut self, controller: usize, point: Vec3, mode: GotoMode) -> HelperObs;
    fn helper_stop(&mut self, controller: usize) -> HelperObs;
    fn helper_terminate(&mut self, controller: usize) -> HelperObs;
    fn helper_tick(&mut self, controller: usize, dt: f64) -> HelperObs;
    fn helper_destination(&mut self, controller: usize) -> Vec3;
    fn helper_install_path(&mut self, controller: usize, points: &[Vec3]) -> HelperObs;
    fn helper_follow(&mut self, controller: usize, target: EntRef) -> HelperObs;
    fn helper_follow_trail(&mut self, controller: usize, on: bool) -> HelperObs;
    fn helper_hold_path(&mut self, controller: usize, on: bool) -> HelperObs;
    fn helper_set_leader(&mut self, controller: usize, leader: Option<EntRef>) -> HelperObs;
    fn nav_control_obs(&mut self, controller: usize) -> ControlObs;
}
impl<T:AiWorld+?Sized> World for T {
    fn unit(&self, key: UKey) -> AiUnit {AiWorld::unit(self,key)}
    fn unit_scalars(&self,key:UKey)->UnitScalars {AiWorld::unit_scalars(self,key)}
    fn helper_request_invalidated(&mut self,controller:usize) {AiWorld::helper_request_invalidated(self,controller)}
    fn helper_goto(&mut self, controller: usize, point: Vec3, mode: GotoMode) -> HelperObs {AiWorld::helper_goto(self,controller,point,mode)}
    fn helper_stop(&mut self, controller: usize) -> HelperObs {AiWorld::helper_stop(self,controller)}
    fn helper_terminate(&mut self, controller: usize) -> HelperObs {AiWorld::helper_terminate(self,controller)}
    fn helper_tick(&mut self, controller: usize, dt: f64) -> HelperObs {AiWorld::helper_tick(self,controller,dt)}
    fn helper_destination(&mut self, controller: usize) -> Vec3 {AiWorld::helper_destination(self,controller)}
    fn helper_install_path(&mut self, controller: usize, points: &[Vec3]) -> HelperObs {AiWorld::helper_install_path(self,controller,points)}
    fn helper_follow(&mut self, controller: usize, target: EntRef) -> HelperObs {AiWorld::helper_follow(self,controller,target)}
    fn helper_follow_trail(&mut self, controller: usize, on: bool) -> HelperObs {AiWorld::helper_follow_trail(self,controller,on)}
    fn helper_hold_path(&mut self, controller: usize, on: bool) -> HelperObs {AiWorld::helper_hold_path(self,controller,on)}
    fn helper_set_leader(&mut self, controller: usize, leader: Option<EntRef>) -> HelperObs {AiWorld::helper_set_leader(self,controller,leader)}
    fn nav_control_obs(&mut self, controller: usize) -> ControlObs {AiWorld::nav_control_obs(self,controller)}
}

/// A helper owns its state; movement requests and navigation control belong
/// to its controller. A retired agent can therefore finish a stack frame
/// without selecting or overwriting the successor's helper.
pub struct Controller<'a> {
    pub index:usize,
    pub unit:Option<UKey>,
    pub request:&'a mut super::controller::MoveRequest,
    pub control:&'a mut Option<ControlObs>,
    pub leader:&'a mut Option<EntRef>,
}
impl Controller<'_> {
    fn invalidate(&mut self,world:&mut (impl World+?Sized)) {
        self.request.valid=false;
        if let Some(control)=self.control.as_mut() {control.request.valid=false;}
        world.helper_request_invalidated(self.index);
    }
    fn sync(&mut self,world:&mut (impl World+?Sized)) {
        if self.control.is_some() {
            let control=world.nav_control_obs(self.index);
            *self.request=control.request;*self.control=Some(control);
        }
    }
}
use super::{agents::Helper,world::HelperKind};

/// DirectHelper.goto and the native Ground/Fly helper service.
pub fn goto(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper,point:Vec3,mode:GotoMode) {
    match helper.kind {
        None=>{},
        Some(HelperKind::Stationary)=>controller.invalidate(world),
        Some(HelperKind::Direct)=>{helper.direct.target=point;helper.direct.active=true;helper.direct.arrived=false;helper.direct.has_path=true;},
        Some(_)=>{helper.obs=world.helper_goto(controller.index,point,mode);controller.sync(world);},
    }
}
/// DirectHelper.stop invalidates the shared request. Navigation performs its
/// own source stop before the controller adopts the resulting request.
pub fn stop(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper) {
    match helper.kind {
        None=>{},
        Some(HelperKind::Stationary)=>controller.invalidate(world),
        Some(HelperKind::Direct)=>{helper.direct.active=false;helper.direct.has_path=false;controller.invalidate(world);},
        Some(_)=>{helper.obs=world.helper_stop(controller.index);controller.sync(world);},
    }
}
pub fn terminate(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper) {
    match helper.kind {
        None|Some(HelperKind::Stationary)=>{},Some(HelperKind::Direct)=>stop(world,controller,helper),
        Some(_)=>{helper.obs=world.helper_terminate(controller.index);controller.sync(world);},
    }
}
/// DirectHelper.tick reads the controller's current unit, including after a
/// body change; only its own arrival/path flags belong to the old helper.
pub fn tick(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper,dt:f64) {
    match helper.kind {
        None=>{},
        Some(HelperKind::Stationary)=>{},
        Some(HelperKind::Direct)=>{
            let Some(unit)=controller.unit else {controller.invalidate(world);return};
            let direct=&mut helper.direct;
            if !direct.active {controller.invalidate(world);return;}
            let position=world.unit_scalars(unit).position;
            if super::agents::horizontal_distance_sq(position,direct.target)<direct.radius*direct.radius {
                direct.arrived=true;direct.active=false;direct.has_path=false;controller.invalidate(world);return;
            }
            let mut dir=super::vec::sub(direct.target,position);dir.y=0.;dir=super::vec::normalize(dir);
            controller.request.valid=true;controller.request.steering_target=direct.target;
            controller.request.waypoint=direct.target;controller.request.segment_direction=dir;
        },
        Some(_)=>{helper.obs=world.helper_tick(controller.index,dt);controller.sync(world);},
    }
}
pub fn destination(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&Helper)->Option<Vec3> {
    match helper.kind {None=>None,Some(HelperKind::Direct)=>Some(helper.direct.target),Some(HelperKind::Stationary)=>controller.unit.map(|unit|world.unit_scalars(unit).position),Some(_)=>Some(world.helper_destination(controller.index))}
}
pub fn install(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper,points:&[Vec3])->bool {
    match helper.kind {
        None|Some(HelperKind::Stationary)=>false,
        Some(HelperKind::Direct)=>{if let Some(last)=points.last() {goto(world,controller,helper,*last,GotoMode::Route);}true},
        Some(_)=>{helper.obs=world.helper_install_path(controller.index,points);controller.sync(world);true},
    }
}
// FlyGotoHelper has no follow or formation methods. Optional source calls
// therefore leave its route alone instead of invoking a ground service.
fn following(helper:&Helper)->bool {matches!(helper.kind,Some(HelperKind::Ground)|Some(HelperKind::Vehicle))}
pub fn follow(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper,target:EntRef) {
    if following(helper) {helper.obs=world.helper_follow(controller.index,target);controller.sync(world);}
}
pub fn trail(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper,on:bool) {
    if following(helper) {helper.obs=world.helper_follow_trail(controller.index,on);controller.sync(world);}
}
pub fn hold(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper,on:bool) {
    if following(helper) {helper.obs=world.helper_hold_path(controller.index,on);controller.sync(world);}
}
pub fn leader(world:&mut (impl World+?Sized),controller:&mut Controller<'_>,helper:&mut Helper,leader:Option<EntRef>) {
    *controller.leader=leader;
    if following(helper) {helper.obs=world.helper_set_leader(controller.index,leader);controller.sync(world);}
}
