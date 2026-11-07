//! Core's movement helpers run in a narrow native navigation scope.
//! No combat state is reacquired while an active combat machine calls back.
use super::{arsenals::Mounted,rows::Rows,weapons::Changes,navigation::{Actor,Command,Environment}};
use crate::ai::{control::world::*,nav::{math::Vec3,units::UnitRef},phase::RandomState};
pub struct Scope<'a,'b,C:Changes+?Sized,M:Mounted,E:Environment> {
    pub helper:Option<super::navigation::HelperKey>,
    pub rows:&'a mut Rows<'b,C,M>,pub random:&'a mut RandomState,pub environment:&'a mut E,
}
impl<C:Changes+?Sized,M:Mounted,E:Environment> Scope<'_,'_,C,M,E> {
    fn actor(&self,controller:usize)->Actor {self.helper.and_then(|key|self.rows.state.navigation.helper_actor(key)).or_else(||self.rows.state.actor(controller)).expect("prepared native helper actor")}
    fn reference(&mut self,target:EntRef)->UnitRef {
        let actor=self.rows.state.follow_actor(self.rows.store,self.rows.seats,self.rows.buildings.rows(),target.unit);
        UnitRef {unit:actor.map_or(u64::MAX,|actor|actor.0),generation:Some(target.generation)}
    }
    fn command(&mut self,controller:usize,command:Command<'_>)->HelperObs {
        let key=self.helper.or_else(||self.rows.state.navigation.helper_key(self.actor(controller))).expect("prepared native helper identity");self.rows.state.navigation.command_for(self.rows.store,self.rows.seats,self.random,self.environment,key,command).expect("retained native helper")
    }
}
impl<C:Changes+?Sized,M:Mounted,E:Environment> crate::ai::control::helpers::World for Scope<'_,'_,C,M,E> {
    fn unit(&self, key: UKey) -> AiUnit {self.rows.policy(key).unwrap_or_else(||AiUnit {key,..Default::default()})}
    fn unit_scalars(&self,key:UKey)->UnitScalars {self.rows.policy_scalars(key)}
    fn helper_request_invalidated(&mut self,controller:usize) {
        let actor=self.helper.and_then(|key|self.rows.state.navigation.helper_actor(key)).or_else(||self.rows.state.actor(controller));
        if let Some(actor)=actor {if let Some(control)=self.rows.state.navigation.control_mut(actor) {control.request.valid=false;}}
    }
    fn helper_goto(&mut self, controller: usize, point: Vec3, mode: GotoMode) -> HelperObs {self.command(controller,Command::Goto(point,match mode {GotoMode::Route=>crate::ai::nav::goto::GotoMode::Route,GotoMode::Direct=>crate::ai::nav::goto::GotoMode::Direct,GotoMode::Straight=>crate::ai::nav::goto::GotoMode::Straight}))}
    fn helper_stop(&mut self, controller: usize) -> HelperObs {self.command(controller,Command::Stop)}
    fn helper_terminate(&mut self, controller: usize) -> HelperObs {self.command(controller,Command::Stop)}
    fn helper_tick(&mut self, controller: usize, dt: f64) -> HelperObs {self.command(controller,Command::Tick(dt))}
    fn helper_destination(&mut self, controller: usize) -> Vec3 {let key=self.helper.or_else(||self.rows.state.navigation.helper_key(self.actor(controller))).expect("prepared native helper identity");self.rows.state.navigation.destination_for(self.rows.store,key).expect("retained native helper destination")}
    fn helper_install_path(&mut self, controller: usize, points: &[Vec3]) -> HelperObs {self.command(controller,Command::Install(points))}
    fn helper_follow(&mut self, controller: usize, target: EntRef) -> HelperObs {let target=self.reference(target);self.command(controller,Command::Follow(target,0.,false))}
    fn helper_follow_trail(&mut self, controller: usize, on: bool) -> HelperObs {self.command(controller,Command::Trail(on))}
    fn helper_hold_path(&mut self, controller: usize, on: bool) -> HelperObs {self.command(controller,Command::Hold(on))}
    fn helper_set_leader(&mut self, controller: usize, leader: Option<EntRef>) -> HelperObs {let leader=leader.map(|target|self.reference(target));self.command(controller,Command::Leader(leader))}
    fn nav_control_obs(&mut self, controller: usize) -> ControlObs {let actor=self.actor(controller);self.rows.state.navigation.retained_observation(self.rows.store,actor).expect("retained native helper control")}
}
