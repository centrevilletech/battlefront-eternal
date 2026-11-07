//! Mounted combat patterns share canonical hulls and the running agent's
//! reserved pattern state. Helpers and fire requests stay native, with every
//! query and random draw kept at its source pattern operation.
use super::{arsenals::Mounted,frame::{Frame,Hooks},weapons::Changes};
use crate::ai::{combat::world::{Obj,World},vehicles::{patterns::{self,PatternWorld,PatternsView},world::{HullView,Steer,UnitView,Sphere}}};
use three::Vector3;
pub enum Operation {Enter,Tick,Exit,Sidestep}
struct Scope<'a,'b,'c,C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> {frame:&'a mut Frame<'b,'c,C,M,H>}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> crate::ai::vehicles::random::Draws for Scope<'_,'_,'_,C,M,H> {
    fn next_a(&mut self)->u32 {crate::ai::vehicles::random::Draws::next_a(self.frame.random)}
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> PatternWorld for Scope<'_,'_,'_,C,M,H> {
    fn pattern_now(&self)->f64 {self.frame.time}
    fn pattern_unit(&self,unit:Obj)->UnitView {crate::ai::vehicles::service::MountWorld::mount_unit(self.frame,unit)}
    fn pattern_hull(&self,hull:Obj)->HullView {crate::ai::vehicles::service::MountWorld::mount_hull(self.frame,hull)}
    fn pattern_forward(&mut self,unit:Obj)->Vector3 {self.frame.forward(unit)}
    fn pattern_sphere(&mut self,unit:Obj)->Sphere {let sphere=self.frame.sphere(unit);Sphere {center:sphere.center,radius:sphere.radius}}
    fn pattern_face(&mut self,agent:Obj,posture:f64) {
        // face changes only the controller; the tracker copy is inline and
        // avoids holding a native row borrow while stance observes helpers.
        let mut machine=self.frame.rows.state.agents.combat(&self.frame.rows.state.entities,agent).expect("native mounted combat machine");
        machine.face(self.frame,posture);
    }
    fn pattern_fire_a(&mut self,controller:Obj) {self.frame.hooks.fire_primary(self.frame.rows,self.frame.random,controller);}
    fn pattern_helper_goto(&mut self,agent:Obj,point:&Vector3,direct:bool) {self.frame.helper_goto(agent,point,if direct {Some(crate::ai::combat::world::GotoMode::Direct)}else{None});}
    fn pattern_helper_stop(&mut self,agent:Obj) {self.frame.helper_stop(agent);}
    fn pattern_helper_follow_path(&mut self,agent:Obj,points:&[[f64;3]],over:bool,altitude:bool) {self.frame.hooks.flight_path(self.frame.rows,self.frame.random,agent,points,over,altitude);}
    fn pattern_default_delay(&mut self,agent:Obj,now:f64) {self.frame.rows.state.agents.get_mut(&self.frame.rows.state.entities,agent).expect("native pattern tracker").tracker_mut().default_delay(now);}
    fn pattern_set_delay(&mut self,agent:Obj,seconds:f64,now:f64) {self.frame.rows.state.agents.get_mut(&self.frame.rows.state.entities,agent).expect("native pattern tracker").tracker_mut().set_delay(seconds,now);}
    fn pattern_edit_steer<R>(&mut self,unit:Obj,edit:impl FnOnce(&mut Option<Steer>)->R)->R {self.frame.rows.vehicle_steer(unit,edit).expect("native pattern steering")}
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> Frame<'_,'_,C,M,H> {
    /// PatternsView observes the same agent and tracker that sync_out just
    /// committed. No body or arsenal table is cloned into this scope.
    fn mounted_view(&mut self,agent:Obj)->PatternsView {
        let row=self.rows.state.agents.get(&self.rows.state.entities,agent).expect("running native pattern agent");
        let controller=row.controller;let tracker=row.tracker().clone();
        let helper=self.has_helper(agent);
        PatternsView {obj:agent,agent,controller,control:self.ctl(controller).unit.expect("native pattern control"),tracker_point:Some(tracker.point),fresh_target:tracker.fresh_target(self),target:tracker.target(self),helper,helper_arrived:helper&&self.helper_arrived(agent)}
    }
    pub(super) fn mounted_pattern(&mut self,agent:Obj,state:f64,operation:Operation) {
        if !self.settings.vehicles {return;}
        let view=self.mounted_view(agent);
        let mut value=self.rows.state.agents.get(&self.rows.state.entities,agent).unwrap().vehicle.clone();
        let mut binding=self.hooks.vehicle_binding();
        let mut scope=Scope {frame:self};
        match operation {
            Operation::Enter=>patterns::enter_pattern(&mut scope,&mut binding.authority,&binding.settings,&view,&mut value,state),
            Operation::Tick=>patterns::tick_pattern(&mut scope,&mut binding.authority,&binding.settings,&view,&mut value,state),
            Operation::Exit=>patterns::exit_pattern(&mut scope,&view,state),
            Operation::Sidestep=>{
                let cycles=v8math::trunc(30./binding.settings.cycle);let cycles=if cycles==0.||cycles.is_nan() {1.}else{cycles};
                patterns::sidestep(&mut scope,&view,cycles);
            },
        }
    }
}
