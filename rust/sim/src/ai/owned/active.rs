//! Cross-owner combat callbacks carry the active Core as a Rust loan.
//! Recursive interrupts construct a narrow world over the same owners and
//! shared generators, without reacquiring an AI global or exporting mirrors.
use super::{arsenals::Mounted,frame::{Hooks,Settings},rows::Rows,weapons::Changes,world::{Facts,Services},scratch::List};
use crate::ai::{control::Core,combat::world::*,phase::RandomState};
use three::Vector3;
pub struct Active<'a,S> {
    pub core:&'a mut Core,pub services:&'a mut S,
    pub facts:&'a Facts,pub settings:&'a Settings,
}
impl<S> Active<'_,S> {
    pub(super) fn helper<C:Changes+?Sized,M:Mounted>(&self,rows:&Rows<'_,C,M>,agent:Obj)->crate::ai::control::agents::Helper {
        let row=rows.state.agents.get(&rows.state.entities,agent).expect("running native helper agent");
        if rows.state.agents.key(row.core)==Some(agent) {
            if let Some(current)=self.core.controllers[row.core].agent.as_ref() {let mut helper=current.helper;if let Some(key)=row.navigation {if let Some(obs)=rows.state.navigation.helper_observation_for(rows.store,key) {helper.obs=obs;}}return helper;}
        }
        let mut helper=row.helper;if let Some(key)=row.navigation {if let Some(obs)=rows.state.navigation.helper_observation_for(rows.store,key) {helper.obs=obs;}}helper
    }
    pub(super) fn with_helper<C:Changes+?Sized,M:Mounted,Srv:super::navigation::Environment>(
        core:&mut Core,rows:&mut Rows<'_,C,M>,random:&mut RandomState,services:&mut Srv,agent:Obj,
        run:impl FnOnce(&mut super::helpers::Scope<'_,'_,C,M,Srv>,&mut crate::ai::control::helpers::Controller<'_>,&mut crate::ai::control::agents::Helper)) {
        let row=rows.state.agents.get(&rows.state.entities,agent).expect("running native helper agent");
        let index=row.core;let navigation=row.navigation;
        let current=rows.state.agents.key(index)==Some(agent);
        let mut helper=if current {core.controllers[index].agent.as_ref().map(|a|a.helper).unwrap_or(row.helper)}else{row.helper};
        let controller=&mut core.controllers[index];
        let mut view=crate::ai::control::helpers::Controller {index,unit:controller.unit,request:&mut controller.request,control:&mut controller.nav_control,leader:&mut controller.leader};
        let mut scope=super::helpers::Scope {helper:navigation,rows,random,environment:services};
        run(&mut scope,&mut view,&mut helper);
        scope.rows.state.agents.get_mut(&scope.rows.state.entities,agent).expect("running native helper owner").helper=helper;
        if current {if let Some(current)=controller.agent.as_mut() {current.helper=helper;}}
    }

}
impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> Hooks<C,M> for Active<'_,S> {
    fn vehicle_binding(&self)->super::vehicles::Binding {self.services.vehicle_binding()}
    fn fire_primary(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,controller:Obj) {
        let index=rows.state.controllers.core(&rows.state.entities,controller).expect("current native firing controller");
        self.core.controllers[index].fire_a=true;rows.state.controllers.write(&rows.state.entities,controller).unwrap().fire_a=true;
    }
    fn flight_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent:Obj,points:&[[f64;3]],over:bool,altitude:bool) {
        if points.is_empty() {return;}
        let key=rows.state.agents.get(&rows.state.entities,agent).expect("running native flyer helper").navigation.expect("native flyer helper identity");
        let Some(actor)=rows.state.navigation.helper_actor(key) else{return};if rows.state.navigation.position(rows.store,actor).is_none() {return;}
        let obs=rows.state.navigation.command_for(rows.store,rows.seats,random,self.services,key,super::navigation::Command::FlightPath(points,over,altitude));
        self.flight_flags(rows,agent,over,altitude,None);
        if let Some(obs)=obs {
            let row=rows.state.agents.get_mut(&rows.state.entities,agent).unwrap();row.helper.obs=obs;
            let index=row.core;
            if rows.state.agents.key(index)==Some(agent) {
                if let Some(current)=self.core.controllers[index].agent.as_mut() {current.helper.obs=obs;}
                if let Some(control)=rows.state.navigation.observation(rows.store,actor) {self.core.controllers[index].request=control.request;self.core.controllers[index].nav_control=Some(control);}
            }
        }
    }
    fn mount_origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,origin:crate::ai::vehicles::world::Origin)->Vector3 {Hooks::mount_origin(self.services,rows,random,origin)}
    fn origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 { Hooks::origin(self.services,rows,random,u) }
    fn forward(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 { Hooks::forward(self.services,rows,random,u) }
    fn aim_point(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3 { Hooks::aim_point(self.services,rows,random,u) }
    fn sphere(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Sphere { Hooks::sphere(self.services,rows,random,u) }
    fn use_classic_random(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj) { Hooks::use_classic_random(self.services,rows,random,u) }
    fn query(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,center: &Vector3,radius: f64,filter: QueryFilter)-> List<Obj> { Hooks::query(self.services,rows,random,center,radius,filter) }
    fn relation(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,a: f64,b: f64)-> f64 { Hooks::relation(self.services,rows,random,a,b) }
    fn living(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState)-> List<Obj> { Hooks::living(self.services,rows,random) }
    fn los(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,from: &Vector3,to: &Vector3,a: Option<Obj>,b: Option<Obj>)-> bool { Hooks::los(self.services,rows,random,from,to,a,b) }
    fn set_stance(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,c:Obj,stance:f64) { let controller=rows.state.controllers.core(&rows.state.entities,c).expect("current native stance controller");
        self.core.controllers[controller].set_stance(stance);
        rows.state.controllers.write(&rows.state.entities,c).unwrap().set_stance(stance);
        if self.core.controllers[controller].nav_control.is_some() {if let Some(actor)=rows.state.actor(controller) {if let Some(control)=rows.state.navigation.control_mut(actor) {control.stance=stance;}}} }
    fn human_target(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,hull:Obj)->Option<Obj> {Hooks::human_target(self.services,rows,random,hull)}
    fn interrupts_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,combat:&mut crate::ai::combat::binding::CombatAi)-> bool { let controller=rows.state.agents.get(&rows.state.entities,agent).expect("running native agent").core;
        // CombatAgent.behave has already selected its weapon target and look.
        // Source calls its base on that same object. Feeding the old Core copy
        // here would erase the new target before validated fire can see it.
        self.core.apply_combat_writes(controller,rows.combat_writes(controller).expect("current native combat controller"));
        self.core.controllers[controller].aim_target=rows.combat_aim_target(controller);
        let mut draws=random.share();
        let mut world=super::world::World {rows,random,combat,services:self.services,settings:self.settings,facts:self.facts};
        self.core.interrupts_tick(&mut world,&mut draws,controller) }
    fn has_helper(&self,rows:&Rows<'_,C,M>,agent: Obj)-> bool { self.helper(rows,agent).kind.is_some() }
    fn helper_arrived(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,agent: Obj)-> bool { let helper=self.helper(rows,agent);match helper.kind {None=>false,Some(crate::ai::control::world::HelperKind::Direct)=>helper.direct.arrived,_=>helper.obs.arrived} }
    fn helper_flying(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj)-> bool { // None of the source helper classes defines a flying property.
        let _=(rows,random,agent);false }
    fn helper_has_follow(&self,rows:&Rows<'_,C,M>,agent: Obj)-> bool {
        if self.helper(rows,agent).kind==Some(crate::ai::control::world::HelperKind::Stationary) {return true;}
        let row=rows.state.agents.get(&rows.state.entities,agent).expect("running native helper agent");
        row.navigation.and_then(|key|rows.state.navigation.helper_has_follow_for(key)).unwrap_or(false)
    }
    fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,point: &Vector3,mode: Option<GotoMode>) {
        if self.helper(rows,agent).kind==Some(crate::ai::control::world::HelperKind::Fly) {self.flight_flags(rows,agent,false,false,None);}
        Self::with_helper(self.core,rows,random,self.services,agent,|world,controller,helper|crate::ai::control::helpers::goto(world,controller,helper,crate::ai::nav::math::Vec3::new(point.x,point.y,point.z),match mode {Some(GotoMode::Direct)=>crate::ai::control::world::GotoMode::Direct,_=>crate::ai::control::world::GotoMode::Route}));
    }
    fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj) {
        if self.helper(rows,agent).kind==Some(crate::ai::control::world::HelperKind::Fly) {self.flight_flags(rows,agent,false,false,None);}
        Self::with_helper(self.core,rows,random,self.services,agent,|world,controller,helper|crate::ai::control::helpers::stop(world,controller,helper));
    }
    fn helper_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,dt: f64) {
        Self::with_helper(self.core,rows,random,self.services,agent,|world,controller,helper|crate::ai::control::helpers::tick(world,controller,helper,dt));
    }
    fn helper_follow(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,target: Obj) {
        let target=rows.unit(target);let target=crate::ai::control::world::EntRef {unit:target.obj,generation:target.generation};
        Self::with_helper(self.core,rows,random,self.services,agent,|world,controller,helper|crate::ai::control::helpers::follow(world,controller,helper,target));
    }
    fn helper_terminate(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj) {
        if self.helper(rows,agent).kind==Some(crate::ai::control::world::HelperKind::Fly) {self.flight_flags(rows,agent,false,false,None);}
        Self::with_helper(self.core,rows,random,self.services,agent,|world,controller,helper|crate::ai::control::helpers::terminate(world,controller,helper));
    }
    fn request_jet(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,controller: Obj,vector: &Vector3) { let index=rows.state.controllers.core(&rows.state.entities,controller).expect("current native jet controller");
        let vector=crate::ai::nav::math::Vec3::new(vector.x,vector.y,vector.z);
        self.core.controllers[index].request_jet(vector);
        if self.core.controllers[index].nav_control.is_some() {if let Some(actor)=rows.state.actor(index) {if let Some(control)=rows.state.navigation.control_mut(actor) {control.jet.pending=true;control.jet.vector=vector;}}} }
    fn emit_vo(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,unit: Obj,event: &str,property: &str) { Hooks::emit_vo(self.services,rows,random,unit,event,property) }
}

impl<S:super::navigation::Environment> super::navigation::Environment for Active<'_,S> {
 fn time(&self)->f64 {self.services.time()}
 fn remembered(&mut self,target:super::navigation::Actor)->Option<crate::ai::nav::math::Vec3> {self.services.remembered(target)}
 fn clear_above(&mut self,store:&mut crate::gen::Store,seats:&mut crate::vehicles::f2_seats::Seats,from:crate::ai::nav::math::Vec3,unit:&crate::gen::AiLifeHandle)->bool {self.services.clear_above(store,seats,from,unit)}
 fn follow_far(&mut self)->f64 {self.services.follow_far()}
 fn seat_frame(&mut self,store:&mut crate::gen::Store,seats:&mut crate::vehicles::f2_seats::Seats,hull:crate::vehicles::f2_seats::HullLife,index:usize)->Option<(crate::ai::nav::math::Vec3,crate::ai::nav::math::Vec3)> {self.services.seat_frame(store,seats,hull,index)}
}
