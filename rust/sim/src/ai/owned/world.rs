//! The control Core observes canonical owners through this native world.
//! Combat calls use one retained frame; mounted actions and lifecycle reentry
//! carry their active Core explicitly, without acquiring a second AI owner.
use crate::ai::vehicles::service::VehicleServices;
use super::{arsenals::Mounted,frame::{Frame,Settings},navigation::Environment,rows::Rows,weapons::Changes,scratch::List};
use crate::ai::{control::{self as ctl,world::*},combat::{binding::CombatAi,world::World as CombatWorld},phase::RandomState,nav::math::Vec3};
fn point(v:three::Vector3)->Vec3 {Vec3::new(v.x,v.y,v.z)}
use super::navigation::{Actor,Command};
fn body_actor<C:Changes+?Sized,M:Mounted>(rows:&Rows<'_,C,M>,controller:usize)->Actor {
    let key=rows.state.controllers.key(controller).expect("current native controller");
    rows.state.controllers.read(&rows.state.entities,key).unwrap().unit.expect("bound native body");
    rows.state.actor(controller).expect("prepared native body actor")
}
fn body_reference<C:Changes+?Sized,M:Mounted>(rows:&mut Rows<'_,C,M>,target:EntRef)->crate::ai::nav::units::UnitRef {
    let actor=rows.state.follow_actor(rows.store,rows.seats,rows.buildings.rows(),target.unit);
    // An invalid weak target resolves to null inside the helper, as the
    // source's resolve(ref) does; it cannot stop the remaining world phase.
    crate::ai::nav::units::UnitRef {unit:actor.map_or(u64::MAX,|actor|actor.0),generation:Some(target.generation)}
}
enum ResponseKind {Hazard,Alert,Acoustic}
#[derive(Clone,Copy,Debug,Default)]
pub struct Facts {pub time:f64,pub paused:bool,pub network_views:bool,pub lod_disabled:bool,pub network_secondary:bool}
/// Cross-owner actions run inside the native phase. Implementations do not
/// invoke host AI or publish mirrors between controller decisions.
pub trait Services<C:Changes+?Sized,M:Mounted>:super::vehicle_frame::Hooks<C,M>+Environment+Sized {
    /// Items.ownedBy is one shared first-use owner authority. Its native
    /// implementation must retain observation order even for empty lists.
    /// Active callbacks retain their Core through a physical F2 operation.
    /// Replay hosts use their own command service; live ownership overrides
    /// this with the canonical loan rather than entering a host callback.
    fn seat_action(&mut self,core:&mut ctl::Core,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,action:super::physical::SeatAction)->bool {
        let _=(core,facts,settings);
        use super::{physical::SeatAction,vehicle_frame::Hooks};
        match action {
            SeatAction::Board {unit,hull}=>Hooks::board(self,rows,random,combat,unit,hull),
            SeatAction::Man {unit,turret}=>Hooks::man(self,rows,random,combat,unit,turret),
            SeatAction::RequestVehicle {unit,options}=>Hooks::request_exit_vehicle(self,rows,random,combat,unit,options),
            SeatAction::RequestTurret {unit,options}=>Hooks::request_exit_turret(self,rows,random,combat,unit,options),
            SeatAction::Unboard {unit,options}=>Hooks::unboard(self,rows,random,combat,unit,options),
        }
    }
    fn hull_damage(&mut self,core:&mut ctl::Core,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,hull:UKey,amount:f64) {
        let _=(core,facts,settings);super::vehicle_frame::Hooks::damage(self,rows,random,combat,hull,amount);
    }
    fn owned_item_count(&self,rows:&Rows<'_,C,M>,unit:UKey,weapon:&crate::gen::Weapon)->usize;
    fn bind_actor(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller:usize,unit:UKey) {
        if rows.unit_priority(unit).kind==7. {
            if rows.state.actor(controller).is_some() {rows.state.controller_control(rows.store,rows.seats,rows.buildings.rows(),controller,unit).expect("prepared stationary controller binding");}
            return;
        }
        rows.state.controller_hull(rows.store,controller,unit).expect("prepared native hull navigation class and control");
    }
    fn mine_ground_height(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,point: Vec3) -> f64;
    fn emit_vo(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,unit: UKey,event: &str,property: &str,command: f64,context: Option<f64>,positional: Option<bool>,notice: Option<bool>);
    fn helper_create(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize) -> (HelperKind, HelperObs, Option<ControlObs>) {
        let key=rows.state.controllers.key(controller).unwrap();let unit=rows.state.controllers.read(&rows.state.entities,key).unwrap().unit.unwrap();
        // StationaryHelper never constructs NavControl, including a bot's
        // first life in a turret. Existing controller navigation is retained.
        if rows.unit_priority(unit).kind==7. {return (HelperKind::Stationary,HelperObs {arrived:true,..Default::default()},None);}
        let actor=body_actor(rows,controller);let obs=rows.state.navigation.renew_helper(rows.store,actor).expect("current native body helper");
        let kind=if rows.unit_priority(unit).kind==5. {HelperKind::Fly}else{HelperKind::Ground};
        (kind,obs,rows.state.navigation.observation(rows.store,actor))
    }
    fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize,point: Vec3,mode: GotoMode) -> HelperObs {
        let actor=body_actor(rows,controller);rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Goto(point,match mode {GotoMode::Route=>crate::ai::nav::goto::GotoMode::Route,GotoMode::Direct=>crate::ai::nav::goto::GotoMode::Direct,GotoMode::Straight=>crate::ai::nav::goto::GotoMode::Straight})).expect("current native body helper")
    }
    fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize) -> HelperObs {
        let actor=body_actor(rows,controller);rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Stop).expect("current native body helper")
    }
    fn helper_terminate(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize) -> HelperObs {
        let actor=body_actor(rows,controller);rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Stop).expect("current native body helper")
    }
    fn helper_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize,dt: f64) -> HelperObs {
        let actor=body_actor(rows,controller);rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Tick(dt)).expect("current native body helper")
    }
    fn helper_destination(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize) -> Vec3 {
        let actor=body_actor(rows,controller);rows.state.navigation.destination(rows.store,actor).expect("current native body helper")
    }
    fn helper_install_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize,points: &[Vec3]) -> HelperObs {
        let actor=body_actor(rows,controller);rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Install(points)).expect("current native body helper")
    }
    fn helper_follow(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize,target: EntRef) -> HelperObs {
        let actor=body_actor(rows,controller);let target=body_reference(rows,target);
        rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Follow(target,0.,false)).expect("current native body helper")
    }
    fn helper_follow_trail(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize,on: bool) -> HelperObs {
        let actor=body_actor(rows,controller);rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Trail(on)).expect("current native body helper")
    }
    fn helper_hold_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize,on: bool) -> HelperObs {
        let actor=body_actor(rows,controller);rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Hold(on)).expect("current native body helper")
    }
    fn helper_set_leader(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize,leader: Option<EntRef>) -> HelperObs {
        let actor=body_actor(rows,controller);let leader=leader.map(|target|body_reference(rows,target));
        rows.state.navigation.command(rows.store,rows.seats,random,self,actor,Command::Leader(leader)).expect("current native body helper")
    }
    fn helper_dispose(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize) {
        let _=body_actor(rows,controller);
    }
    fn nav_control_ensure(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize) -> ControlObs {
        let key=rows.state.controllers.key(controller).expect("current native input controller");
        let unit=rows.state.controllers.read(&rows.state.entities,key).unwrap().unit.expect("bound native input unit");
        if rows.unit_priority(unit).kind==7. {rows.state.controller_control(rows.store,rows.seats,rows.buildings.rows(),controller,unit).expect("prepared stationary input control");}
        let actor=body_actor(rows,controller);rows.state.navigation.observation(rows.store,actor).expect("current native body control")
    }
    fn nav_control_reset(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize) {
        let actor=body_actor(rows,controller);assert!(rows.state.navigation.reset(actor));
    }
    fn nav_control_obs(&mut self,rows:&mut Rows<'_,C,M>,_random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize) -> ControlObs {
        let actor=body_actor(rows,controller);rows.state.navigation.retained_observation(rows.store,actor).expect("current native body control")
    }
    fn nav_navigation_input(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,_combat:&mut CombatAi,_facts:&Facts,_settings:&Settings,controller: usize,step: f64,aim: Option<Vec3>,facing:bool) -> (Option<HelperObs>, Vec<CoreEffect>) {
        let actor=body_actor(rows,controller);let changed=rows.state.navigation.input(rows.store,rows.seats,random,self,actor,step,aim,facing).expect("current native body control");
        let controller=rows.state.controllers.key(controller).unwrap();
        let unit=rows.state.controllers.read(&rows.state.entities,controller).unwrap().unit.unwrap();
        if changed {if let Some(super::entities::Entity::Unit(reference))=rows.state.entities.get(unit) {
            if rows.state.navigation.input_body_changed() {rows.changes.mark(reference);}else{rows.changes.intent(reference);}
        }}
        (rows.state.navigation.helper_observation(rows.store,actor),Vec::new())
    }
    fn nav_analogs(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,facts:&Facts,settings:&Settings,controller: usize,aim: Option<Vec3>,step: f64,_facing:bool) -> ([f64; 4], Vec<CoreEffect>);
}
pub struct World<'a,'b,C:Changes+?Sized,M:Mounted,S:Services<C,M>> {
    pub rows:&'a mut Rows<'b,C,M>,pub random:&'a mut RandomState,pub combat:&'a mut CombatAi,
    pub services:&'a mut S,pub settings:&'a Settings,pub facts:&'a Facts,
}
impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> World<'_,'_,C,M,S> {
    fn controller(&self,core:usize)->u64 {self.rows.state.controllers.key(core).expect("current native AI controller")}
    fn agent(&self,core:usize)->u64 {self.rows.state.agents.key(core).expect("current native AI agent")}
    fn observed_helper(&self,controller:usize,mut helper:ctl::agents::Helper)->ctl::agents::Helper {
        if let Some(key)=self.rows.state.agents.key(controller).and_then(|key|self.rows.state.agents.get(&self.rows.state.entities,key)).and_then(|a|a.navigation) {
            if let Some(obs)=self.rows.state.navigation.helper_observation_for(self.rows.store,key) {helper.obs=obs;}
        }
        helper
    }
    fn vehicles<R>(&mut self,run:impl FnOnce(&mut super::vehicles::Authority,&mut super::vehicle_frame::Frame<'_,'_,C,M,S>)->R)->R {
        let mut binding=self.services.vehicle_binding();
        let mut frame=super::vehicle_frame::Frame {rows:self.rows,hooks:self.services,random:self.random,combat:self.combat,
            combat_settings:self.settings,settings:&binding.settings,time:self.facts.time,navigation:binding.navigation,population:binding.population};
        run(&mut binding.authority,&mut frame)
    }
    fn active_vehicles<R>(&mut self,core:&mut ctl::Core,run:impl FnOnce(&mut super::vehicles::Authority,&mut super::vehicle_frame::Frame<'_,'_,C,M,super::active::Active<'_,S>>)->R)->R {
        let mut binding=self.services.vehicle_binding();
        let mut hooks=super::active::Active {core,services:self.services,facts:self.facts,settings:self.settings};
        let mut frame=super::vehicle_frame::Frame {rows:self.rows,hooks:&mut hooks,random:self.random,combat:self.combat,
            combat_settings:self.settings,settings:&binding.settings,time:self.facts.time,navigation:binding.navigation,population:binding.population};
        run(&mut binding.authority,&mut frame)
    }
    fn mounted(&self,core:&ctl::Core,controller:usize)->bool {
        if !core.options.bind.vehicles {return false;}
        let Some(unit)=core.controllers[controller].unit else{return false};
        let view=self.rows.policy_scalars(unit);view.has_lease&&view.lease_is_self
    }
    /// bindVehicleAI wraps both scheduler input paths. The navigation path
    /// attaches and updates first; the plain conversion does so only when a
    /// control exists. No soldier posture or jet conversion runs for a seat.
    fn seat_input(&mut self,core:&mut ctl::Core,controller:usize,dt:f64,aim:Option<Vec3>,navigation:bool)->[f64;4] {
        use crate::ai::vehicles::{world::World as VehicleWorld,steering::{self,Env}};
        let unit=core.controllers[controller].unit.expect("mounted native controller");
        let key=self.controller(controller);let aim=aim.map(|point|three::Vector3::new(point.x,point.y,point.z));
        self.active_vehicles(core,|vehicles,frame| {
            if navigation||frame.hooks.core.controllers[controller].nav_control.is_some() {
                VehicleWorld::nav_attach(frame,Some(key),unit);VehicleWorld::nav_update(frame,Some(key),dt);
            }
            let value=&frame.hooks.core.controllers[controller];let request=value.request;
            let request=crate::ai::vehicles::world::Request {valid:request.valid,steering_target:three::Vector3::new(request.steering_target.x,request.steering_target.y,request.steering_target.z),
                waypoint:three::Vector3::new(request.waypoint.x,request.waypoint.y,request.waypoint.z),segment_direction:three::Vector3::new(request.segment_direction.x,request.segment_direction.y,request.segment_direction.z),
                mode:request.mode,lower:request.lower,upper:request.upper,..Default::default()};
            let helper_path=value.agent.as_ref().is_some_and(|agent|agent.helper.has_path_property());
            let env=Env {now:frame.time,has_path:value.nav_control.is_some_and(|control|control.has_executor)||helper_path,
                status:value.executor_status(),min_fly_height:frame.settings.min_fly_height,max_fly_height:frame.settings.max_fly_height,controller:Some(key)};
            let view=VehicleWorld::unit(frame,unit);
            // VehicleAI.analogs renews yielding contact holds immediately
            // before steering. A direct seat_analogs call alone misses them.
            if view.kind==crate::ai::vehicles::world::kind::WALKER {
                crate::ai::vehicles::service::VehicleServices::walker_contacts(vehicles,frame,&view);
            }
            let stick=steering::seat_analogs(frame,&view,Some(&request),aim.as_ref(),dt,&env);
            let out=[stick.forward,stick.right,stick.turn,stick.pitch];
            frame.rows.intent(unit,super::intent::Command::Analogs(out));out
        })
    }
    fn response(&mut self,controller:usize,state:&mut ResponseState,kind:ResponseKind)->Option<Response> {
        let key=self.controller(controller);let agent=self.rows.state.agents.key(controller);
        let scope=super::frame::ResponseScope::take(state,self.combat.shared.share());
        let mut frame=Frame {rows:self.rows,hooks:self.services,random:self.random,settings:self.settings,time:self.facts.time,response:Some(scope)};
        let response=match kind {
            ResponseKind::Hazard=>self.combat.hazard_response(&mut frame,key,agent),
            ResponseKind::Alert=>self.combat.alert_response(&mut frame,key,agent),
            ResponseKind::Acoustic=>self.combat.acoustic_response(&mut frame,key,agent),
        };
        frame.response.take().unwrap().finish(state);
        response.map(|response|Response {mode:response.mode,point:Some(point(response.point)),target:None,exit:response.exit,alert:response.alert})
    }
    fn combat<R>(&mut self,run:impl FnOnce(&mut Frame<'_,'_,C,M,S>,&mut CombatAi)->R)->R {
        let mut frame=Frame {rows:self.rows,hooks:self.services,random:self.random,settings:self.settings,time:self.facts.time,response:None};
        run(&mut frame,self.combat)
    }
}
impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> Characters for World<'_,'_,C,M,S> {
    fn character(&self,index:usize)->CharacterSnap {self.rows.state.roster.read(self.rows.store,self.rows.characters,&self.rows.state.entities,&self.rows.state.population,index).expect("current native AI Character")}
    fn character_unit(&self,index:usize)->Option<UKey> {
        let reference=self.rows.state.roster.reference(self.rows.store,self.rows.characters,index).expect("current native AI Character");
        self.rows.store.character.get(reference).unwrap().unit.map(|unit|
            self.rows.state.entities.find(super::entities::Entity::Unit(unit)).expect("prepared AI Character body"))
    }
    fn character_objective(&self,index:usize)->Option<usize> {self.rows.state.roster.objective(self.rows.store,self.rows.characters,&self.rows.state.population,index).expect("current native AI objective")}
    fn character_set_objective(&mut self,index:usize,post:Option<usize>) {self.rows.state.roster.assign(self.rows.store,self.rows.characters,&self.rows.state.population,index,post).expect("current native objective publication");}
}
impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> AiWorld for World<'_,'_,C,M,S> {
    fn agent_target_override(&mut self,controller:usize,target:UKey)->Option<bool> {
        let agent=self.rows.state.agents.key(controller)?;
        self.combat(|frame,combat|frame.target_override(&mut combat.shared,agent,target))
    }
    fn agent_board_override(&mut self,controller:usize,candidate:UKey)->Option<bool> {
        let agent=self.rows.state.agents.key(controller)?;
        self.combat(|frame,_|frame.board_override(agent,candidate))
    }
    fn link_character(&mut self,character:usize,controller:usize,bound:bool) { assert!(self.rows.state.link_character(self.rows.store,self.rows.characters,character,controller,bound)); }
    fn link_controller(&mut self,key:UKey,controller:usize,bound:bool) { assert!(self.rows.state.link_controller(self.rows.store,self.rows.changes,key,controller,bound));
        self.rows.state.seat_driven(self.rows.seats,key);
        let native=self.rows.state.controllers.key(controller).expect("current native controller");
        let generation=self.unit(key).generation;
        let row=self.rows.state.controllers.write(&self.rows.state.entities,native).unwrap();
        if bound {row.unit=Some(key);row.generation=generation;}else if row.unit==Some(key) {row.unit=None;}
        if bound {if matches!(self.rows.state.entities.get(key),Some(super::entities::Entity::Unit(_))) {
            self.rows.state.controller_body(self.rows.store,controller,key).expect("prepared native controller body");
        }else{Services::bind_actor(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,key);}} }
    fn nav_stance_changed(&mut self,controller:usize,stance:f64) { if let Some(actor)=self.rows.state.actor(controller) {if let Some(c)=self.rows.state.navigation.control_mut(actor) {c.stance=stance;}} }
    fn nav_jet_requested(&mut self,controller:usize,vector:Vec3) { if let Some(actor)=self.rows.state.actor(controller) {if let Some(c)=self.rows.state.navigation.control_mut(actor) {c.jet.pending=true;c.jet.vector=vector;}} }
    fn time(&self) -> f64 { self.facts.time }
    fn paused(&self) -> bool { self.facts.paused }
    fn unit(&self, key: UKey) -> AiUnit { self.rows.policy(key).unwrap_or_else(||AiUnit {key,..Default::default()}) }
    fn unit_is_seat_unit(&self,key:UKey)->bool {
        if matches!(self.rows.state.entities.get(key),Some(super::entities::Entity::Unit(_))) {false}else{self.unit(key).is_seat_unit}
    }
    fn unit_live(&self,key:UKey,generation:f64)->bool {
        if let Some(super::entities::Entity::Unit(reference))=self.rows.state.entities.get(key) {
            self.rows.state.units.bound_live(self.rows.store,reference,generation)
        }else {self.rows.policy(key).is_some_and(|u|u.alive&&u.generation==generation)}
    }
    fn unit_scalars(&self,key:UKey)->UnitScalars {self.rows.policy_scalars(key)}
    fn unit_motion(&self,key:UKey)->UnitMotion {self.rows.motion(key)}
    fn intent_command(&mut self,key:UKey,command:crate::ai::owned::intent::Command) { assert!(self.rows.intent(key,command)); }
    fn units_len(&self) -> usize { self.rows.store.units.list.len() }
    fn unit_key(&self,index:usize)->UKey { self.rows.state.queries.object(&self.rows.state.entities,self.rows.store.units.list[index]).expect("prepared native AI registry identity") }
    fn unit_sphere_radius(&mut self,key:UKey)->Option<f64> { Some(self.combat(|f,_|CombatWorld::sphere(f,key).radius)) }
    fn unit_forward(&mut self,key:UKey)->Option<Vec3> { Some(point(self.combat(|f,_|CombatWorld::forward(f,key)))) }
    fn unit_aim_point(&mut self,key:UKey)->Option<Vec3> { Some(point(self.combat(|f,_|CombatWorld::aim_point(f,key)))) }
    fn unit_has_origin_forward(&self,key:UKey)->bool { self.rows.unit_has_origin(key) }
    fn post(&self, index: usize) -> PostSnap { self.rows.state.posts.read(self.rows.store,&self.rows.state.population,index,&self.rows.state.bindings.facts(self.rows.buildings.rows())).expect("current native AI post") }
    fn objective_post(&self,index:usize)->ctl::goals::ObjectivePost<'_> { self.rows.state.population.view(self.rows.store,index,&self.rows.state.bindings.facts(self.rows.buildings.rows())).expect("current native objective post") }
    fn posts_len(&self) -> usize { self.rows.state.population.len() }
    fn post_name_id(&self,index:usize)->u32 { self.rows.state.posts.name_id(index) }
    fn cameras(&self) -> &[CameraView] { self.rows.state.lod.cameras() }
    fn network_views(&self) -> bool { self.facts.network_views }
    fn lod_disabled_world(&self) -> bool { self.facts.lod_disabled }
    fn unit_buffer(&self)->List<UKey> { self.rows.state.buffers.units.take() }
    fn predicate_buffer(&self)->List<(UKey,bool)> { self.rows.state.buffers.predicates.take() }
    fn index_buffer(&self)->List<usize> { self.rows.state.buffers.indices.take() }
    fn weight_buffer(&self)->List<f64> { self.rows.state.buffers.weights.take() }
    fn objective_buffer(&self)->List<Option<usize>> { self.rows.state.buffers.objectives.take() }
    fn recruit_buffer(&self)->List<(f64,usize)> { self.rows.state.buffers.recruits.take() }
    fn sort_recruits(&mut self,rows:&mut Vec<(f64,usize)>) { self.rows.state.buffers.sort_recruits(rows); }
    fn units_query(&mut self,center:Vec3,radius:f64,filter:&mut dyn FnMut(&AiUnit)->bool)->List<UKey> {
        let mut out=self.rows.state.buffers.units.take();let mut index=0;
        // A lazy sphere precedes the filter for each registry member; pose
        // observations must not be pulled forward into a separate batch.
        while index<self.rows.store.units.list.len() {
            let member=self.rows.store.units.list[index];index+=1;
            let key=self.rows.state.queries.object(&self.rows.state.entities,member).expect("prepared native query identity");
            if !self.unit(key).alive {continue;}
            let sphere=self.combat(|frame,_|CombatWorld::sphere(frame,key));
            let reach=radius+sphere.radius;
            let distance=sphere.center.distance_to_squared(&three::Vector3::new(center.x,center.y,center.z));
            if distance>reach*reach {continue;}
            if filter(&self.unit(key)) {out.push_reserved(key);}
        }out
    }
    fn service_candidates(&mut self,center:Vec3,radius:f64)->List<UKey> {
        // Source repair jobs snapshot body, fleet and wreck order first.
        let mut out=self.rows.state.candidates.snapshot(self.rows.store,&self.rows.state.entities);
        out.retain(|key| {
            let position=self.unit(*key).position;
            let distance=ctl::vec::distance_sq(position,center);
            let r=self.combat(|frame,_|CombatWorld::sphere(frame,*key)).radius;
            let r=if r==0.||r.is_nan() {0.}else{r};
            distance<=(radius+r)*(radius+r)
        });out
    }
    fn items_len(&self)->usize { self.rows.state.items.len(self.rows.store) }
    fn item(&self,index:usize)->ItemSnap { self.rows.state.items.read(self.rows.store,&self.rows.state.entities,index) }
    fn crew(&mut self,character:usize)->Option<CrewSnap> {
        super::crew::read(self.rows.store,&self.rows.state.entities,&self.rows.state.roster,self.rows.characters,character)
    }
    fn crew_occupants(&self,character:usize)->List<usize> {
        super::crew::occupants(self.rows.store,self.rows.seats,&self.rows.state.turrets,&self.rows.state.roster,self.rows.characters,&self.rows.state.buffers.indices,character)
    }
    fn objective_vehicle_mask(&self,vehicle:UKey)->f64 {
        match self.rows.state.entities.get(vehicle) {
            Some(super::entities::Entity::Hull(reference))=>super::population::Facts::vehicle_mask(&self.rows.state.bindings.facts(self.rows.buildings.rows()),reference),
            _=>crate::ai::nav::units::traversal_mask(&self.unit(vehicle).nav_unit()) as f64,
        }
    }
    fn tickets(&self,team:f64)->Option<f64> {Some(crate::battle::spawning::tickets(self.rows.store,team))}
    fn reinforcements(&self,team:f64)->Option<f64> {self.rows.store.battle.reinforcements.at(team)}
    fn mine_ground_height(&mut self, point: Vec3) -> f64 { Services::mine_ground_height(self.services,self.rows,self.random,self.combat,self.facts,self.settings,point) }
    fn emit_vo(&mut self, unit: UKey, event: &str, property: &str, command: f64, context: Option<f64>, positional: Option<bool>, notice: Option<bool>) { Services::emit_vo(self.services,self.rows,self.random,self.combat,self.facts,self.settings,unit,event,property,command,context,positional,notice) }
    // Hearing notifies the control owner at each append. Its private combat
    // list is refreshed by the next ingest/response feed before it is read;
    // source-order caps and duplicates still belong to the canonical owner.
    fn combat_prepare_ingest(&mut self,core:&crate::ai::control::Core)->bool {
        if self.rows.state.reader.cursor>=self.rows.state.journal.sequence() {return false;}
        for index in 0..core.controllers.len() {core.feed_combat(self,index);}true
    }
    fn combat_prepare_retained(&mut self,_core:&crate::ai::control::Core,_controller:usize) {}
    fn combat_prepare_fire(&mut self,core:&ctl::Core,controller:usize)->bool {self.rows.combat_fire_feed(controller,&core.controllers[controller]);false}
    fn combat_fire_requests(&mut self,controller:usize)->(bool,bool) {
        let row=self.rows.state.controllers.read(&self.rows.state.entities,self.controller(controller)).expect("current native fire controller");
        (row.fire_a,row.fire_b)
    }
    fn combat_prepare_query(&mut self,core:&ctl::Core,controller:usize) {self.rows.combat_query_feed(controller,&core.controllers[controller],false);}
    fn combat_prepare_aim_query(&mut self,core:&ctl::Core,controller:usize)->bool {self.rows.combat_query_feed(controller,&core.controllers[controller],true);false}
    fn combat_prepare_sensors(&mut self, _core: &crate::ai::control::Core) {}
    fn combat_prepare_los(&mut self,core:&crate::ai::control::Core) {
        // Heap order and budgets stay with DeferredLos. Stage only qualified
        // client numbers so no heap reference survives a controller feed.
        let mut clients=[0usize;crate::ai::combat::perception::GLOBAL_CAPACITY];
        let count=self.rows.state.controllers.los_clients(&self.rows.state.entities,&self.combat.los,&mut clients);
        for &index in &clients[..count] {if index<core.controllers.len() {core.feed_combat(self,index);}}
    }
    fn combat_objective(&self,_character:usize)->Option<usize> {None}
    fn combat_feed(&mut self, _controller: usize, _input: &CombatInput) { self.rows.combat_feed(_controller,_input); }
    fn controller_state_changed(&mut self,controller:usize,state:f64) { self.rows.state.controllers.state_changed(controller,state); }
    fn combat_create_perception(&mut self, controller: usize) { let key=self.controller(controller);self.combat(|f,c|{c.create_perception(f,key);}); }
    fn combat_attach_controller(&mut self, controller: usize) { let key=self.controller(controller);self.combat(|f,c|{c.attach_controller(f,key);}); }
    fn combat_detach_controller(&mut self, controller: usize) { let key=self.controller(controller);self.combat(|f,c|{c.detach_controller(f,key);}); }
    fn combat_rebuild_protected(&mut self) { self.combat(|f,c|c.rebuild_protected(f)); }
    fn combat_ingest(&mut self) -> List<AlertState> {
        let journal=self.rows.state.journal.clone();let mut reader=std::mem::take(&mut self.rows.state.reader);
        self.combat(|frame,combat|reader.ingest(&journal,combat,frame));
        self.rows.state.reader=reader;self.rows.combat_alerts()
    }
    fn combat_init_agent(&mut self, controller: usize) { let key=self.rows.state.controllers.key(controller).expect("current native controller"); if self.rows.state.agents.key(controller).is_none() {self.rows.state.agents.begin(&mut self.rows.state.entities,controller,key);} }
    fn combat_scan(&mut self, controller: usize) { let key=self.controller(controller);self.combat(|f,c|{c.scan(f,key);}); }
    fn combat_drop_los_requests(&mut self, controller: usize) { let key=self.controller(controller);self.combat(|f,c|{c.drop_los_requests(f,key);}); }
    fn combat_service_los(&mut self, budget: f64) -> bool { self.combat(|f,c|c.service_los(f,budget)) }
    fn combat_clear_candidate(&mut self, controller: usize) { let key=self.controller(controller);self.combat(|f,c|{c.clear_candidate(f,key);}); }
    fn combat_rank_targets(&mut self, controller: usize) { let agent=self.agent(controller);self.combat(|f,c|{c.rank_targets(f,agent);}); }
    fn combat_update_target(&mut self, controller: usize) { let agent=self.agent(controller);self.combat(|f,c|{c.update_target(f,agent);}); }
    fn combat_update_target_selection(&mut self, controller: usize) { let agent=self.agent(controller);self.combat(|f,c|{c.update_target_selection(f,agent);}); }
    fn combat_gate_fire(&mut self, controller: usize) { let agent=self.agent(controller);self.combat(|f,c|{c.gate_fire(f,agent);}); }
    fn combat_current_target(&mut self, controller: usize) -> Option<UKey> { let agent=self.agent(controller);self.combat(|f,c|c.current_target(f,agent)) }
    fn combat_memory_point(&mut self, controller: usize) -> Option<Vec3> { let agent=self.agent(controller);self.combat(|f,c|c.memory_point(f,agent)).map(point) }
    fn combat_acquired_target(&mut self, controller: usize) -> Option<UKey> { let agent=self.agent(controller);self.combat(|f,c|c.acquired_target(f,agent)) }
    fn combat_can_engage(&mut self, controller: usize, target: UKey) -> bool { let key=self.controller(controller);self.combat(|f,c|c.can_engage(f,key,Some(target))) }
    fn combat_has_weapon(&mut self, controller: usize) -> bool { let key=self.controller(controller);self.combat(|f,c|c.has_weapon(f,key)) }
    fn combat_sensor_range(&mut self, controller: usize, target: UKey) -> f64 { let key=self.controller(controller);self.combat(|f,c|c.sensor_range(f,key,target)) }
    fn combat_acoustic_range(&mut self, controller: usize) -> f64 { let key=self.controller(controller);self.combat(|f,c|c.acoustic_range(f,key)) }
    fn combat_consume_fire(&mut self, controller: usize, step: f64) { let key=self.controller(controller);let unit=self.rows.state.controllers.read(&self.rows.state.entities,key).unwrap().unit.expect("controlled native unit");self.combat(|f,c|c.consume_fire(f,key,unit,step,true)); }
    fn combat_aim_point(&mut self, controller: usize, dt: f64) -> Option<Vec3> { let key=self.controller(controller);let unit=self.rows.state.controllers.read(&self.rows.state.entities,key).unwrap().unit.expect("controlled native unit");self.combat(|f,c|c.aim_point(f,key,unit,dt)).map(point) }
    fn combat_lead_point(&mut self, controller: usize, target: UKey, dt: f64) -> Option<Vec3> { let key=self.controller(controller);self.combat(|f,c|c.lead_point(f,key,target,dt)).map(point) }
    fn combat_mark_skip(&mut self, controller: usize, skipped: bool) { let _=(controller,skipped);self.combat.mark_skip(); }
    fn combat_broadcast_acoustic(&mut self,unit:UKey)->List<(usize,EntRef)> {
        let mut out=self.rows.state.buffers.references.take();
        self.combat(|frame,_| {
            crate::ai::combat::perception::broadcast_acoustic_with(frame,unit,&mut |frame,controller,note| {
                let core=frame.rows.state.controllers.core(&frame.rows.state.entities,controller).expect("current acoustic controller");
                out.push_reserved((core,EntRef {unit:note.obj,generation:note.generation}));
            });
        });out
    }
    fn combat_hazard_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> { self.response(controller,state,ResponseKind::Hazard) }
    fn combat_alert_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> { self.response(controller,state,ResponseKind::Alert) }
    fn combat_acoustic_response(&mut self, controller: usize, state: &mut ResponseState) -> Option<Response> { self.response(controller,state,ResponseKind::Acoustic) }
    fn weapons_selected(&mut self,controller:usize)->Option<usize> {
        let key=self.controller(controller);self.combat(|frame,_| {
            let unit=CombatWorld::ctl(frame,key).unit?;
            crate::ai::combat::weapons_ai::selected_weapon(frame,unit,Some(key))
        })
    }
    fn weapons_switch(&mut self,controller:usize,index:usize) {
        let key=self.controller(controller);self.combat(|frame,_| {
            if let Some(unit)=CombatWorld::ctl(frame,key).unit {crate::ai::combat::weapons_ai::switch_weapon(frame,key,unit,index);}
        });
    }
    fn weapons_select(&mut self,controller:usize,index:usize) {
        let key=self.controller(controller);self.combat(|frame,_| {
            if let Some(unit)=CombatWorld::ctl(frame,key).unit {CombatWorld::arsenal_select(frame,unit,index);}
        });
    }
    fn weapons_effective_range(&mut self,controller:usize,weapon:usize)->f64 {
        let key=self.controller(controller);self.combat(|frame,combat| {
            let unit=CombatWorld::ctl(frame,key).unit.expect("bound native weapon owner");
            let def=crate::ai::combat::weapons_ai::def_at(frame,unit,weapon);
            crate::ai::combat::weapons_ai::effective_range(&combat.shared.weapons,CombatWorld::unit_priority(frame,unit).kind,CombatWorld::ctl(frame,key).range_scale,&def)
        })
    }
    fn weapons_firing_cone(&mut self,controller:usize,p:Vec3)->bool {
        let key=self.controller(controller);self.combat(|frame,combat| {
            let Some(unit)=CombatWorld::ctl(frame,key).unit else{return false};
            crate::ai::combat::weapons_ai::firing_cone(frame,&combat.shared.weapons,key,unit,&three::Vector3::new(p.x,p.y,p.z))
        })
    }
    fn combat_repair_target(&mut self,unit:UKey)->Option<UKey> {self.rows.repair_target(unit)}
    fn repair_matches(&self,target:UKey,current:UKey)->bool {self.rows.state.entities.repair_matches(target,current)}
    fn classes_needs(&self,unit:UKey,ammo:bool)->bool {
        if ammo {return self.rows.needs_ammo(unit);}
        let unit=self.unit(unit);ctl::classes::unit_health(&unit)<ctl::classes::unit_maximum(&unit)
    }
    fn classes_dispenser_available(&self,unit:UKey,index:usize)->bool {self.rows.dispenser_available(unit,index,&|unit,weapon|self.services.owned_item_count(self.rows,unit,weapon))}
    fn classes_mine_weapon(&self,unit:UKey)->Option<usize> {self.rows.mine_weapon(unit,&|unit,weapon|self.services.owned_item_count(self.rows,unit,weapon))}
    fn classes_service_weapon(&self,unit:UKey,health_type:f64,ammo:bool)->Option<usize> {self.rows.service_weapon(unit,health_type,ammo,&|unit,weapon|self.services.owned_item_count(self.rows,unit,weapon))}
    fn classes_repair_weapon(&self,unit:UKey,index:usize)->bool {
        self.rows.repair_weapon(unit,index)
    }
    fn vehicles_notify(&mut self)->List<(usize,EntRef)> {
        use crate::ai::vehicles::notify::VehicleNotify;
        self.vehicles(|vehicles,frame| {
            let mut out=frame.rows.state.buffers.vehicle_notes.take();
            vehicles.notify_with(frame,&mut |frame,controller,note| {
                let core=frame.rows.state.controllers.core(&frame.rows.state.entities,controller).expect("current vehicle notice controller");
                out.push_reserved((core,EntRef {unit:note.obj,generation:note.generation}));
            });out
        })
    }
    fn vehicles_request_exit(&mut self,core:&mut ctl::Core,controller:usize) {let controller=self.controller(controller);self.active_vehicles(core,|v,frame|{v.request_exit(frame,Some(controller),None);});}
    fn vehicles_boarded(&mut self, controller: usize) -> bool { let controller=self.controller(controller);self.vehicles(|v,f|v.boarded(f,Some(controller))) }
    fn vehicles_boarded_vehicle(&mut self, controller: usize) -> Option<UKey> { let controller=self.controller(controller);self.vehicles(|v,f|v.boarded_vehicle(f,Some(controller))) }
    fn vehicles_boarded_vehicle_of(&mut self, unit: UKey) -> Option<UKey> { self.vehicles(|v,f|v.boarded_vehicle_of(f,Some(unit))) }
    fn vehicles_boarded_vehicle_of_character(&mut self, character: usize) -> Option<UKey> { let character=self.rows.state.entities.find(super::entities::Entity::Character(self.rows.state.roster.reference(self.rows.store,self.rows.characters,character).expect("current native Character"))).expect("prepared native Character identity");self.vehicles(|v,f|v.boarded_vehicle_of_character(f,Some(character))) }
    fn vehicles_current_controllable(&mut self, character: usize) -> Option<UKey> {
        let reference=self.rows.state.roster.reference(self.rows.store,self.rows.characters,character).expect("current native Character");
        // currentControllable stops at an absent body lease. Most roster
        // members are on foot, so do not build their full vehicle/class view
        // or loan every vehicle service merely to discover that absence.
        self.rows.store.character.get(reference)?.unit.and_then(|unit|self.rows.store.unit.get(unit))?.seat?;
        let character=self.rows.state.entities.find(super::entities::Entity::Character(reference)).expect("prepared native Character identity");
        self.vehicles(|v,f|v.current_controllable(f,Some(character)))
    }
    fn vehicles_is_mounted_turret(&mut self, unit: UKey) -> bool { self.vehicles(|v,f|v.is_mounted_turret(f,Some(unit))) }
    fn vehicles_empty_seats(&mut self, vehicle: UKey) -> f64 { self.vehicles(|v,f|v.empty_seats(f,Some(vehicle))) }
    fn vehicles_crew_controllers(&mut self,vehicle:UKey,except:usize)->List<usize> {
        let except=self.controller(except);
        let source=self.rows.state.controllers.read(&self.rows.state.entities,except).unwrap().character;
        self.vehicles(|vehicles,frame| {
            let found=vehicles.crew_controllers(frame,Some(vehicle),source);
            let mut out=frame.rows.state.buffers.indices.take();
            for controller in found {
                out.push_reserved(frame.rows.state.controllers.core(&frame.rows.state.entities,controller).expect("current native crew controller"));
            }out
        })
    }
    fn vehicles_seats_full(&mut self, vehicle: UKey) -> bool { self.vehicles(|v,f|v.seats_full(f,vehicle)) }
    fn vehicles_pending_reservations(&mut self, vehicle: UKey, team: f64) -> bool { self.vehicles(|v,f|v.pending_reservations(f,vehicle,team)) }
    fn vehicles_low_tickets(&mut self, team: f64) -> bool { self.vehicles(|v,f|v.low_tickets(f,team)) }
    fn vehicles_pilot_keeps_seat(&mut self, _controller: usize) -> bool { self.vehicles(|v,_|v.pilot_keeps_seat()) }
    fn vehicles_alerts_blocked(&mut self, _controller: usize) -> bool { self.vehicles(|v,_|v.alerts_blocked()) }
    fn vehicles_mount_reach(&mut self, unit: UKey, point: Vec3) -> bool { self.vehicles(|v,f|v.mount_reach(f,Some(unit),&three::Vector3::new(point.x,point.y,point.z))) }
    fn vehicles_refuses_hold(&mut self, _controller: usize) -> bool { self.vehicles(|v,_|v.refuses_hold()) }
    fn vehicles_can_enter(&mut self, controller: usize, candidate: UKey) -> Option<bool> { let controller=self.controller(controller);Some(self.vehicles(|v,f|v.can_enter(f,Some(controller),candidate))) }
    fn vehicles_free_seats(&mut self, candidate: UKey, team: f64, want: f64) -> f64 { self.vehicles(|v,f|v.free_seats(f,candidate,team,want)) }
    fn vehicles_reserve(&mut self, candidate: UKey, unit: UKey, want: f64) { self.vehicles(|v,f|v.reserve(f,candidate,unit,want)) }
    fn vehicles_reserve_seat(&mut self, controller: usize, vehicle: UKey) -> bool { let controller=self.controller(controller);self.vehicles(|v,f|v.reserve_seat(f,Some(controller),Some(vehicle))) }
    fn vehicles_target_reserved(&mut self, target: UKey, team: f64) -> bool { self.vehicles(|v,f|v.target_reserved(f,target,team)) }
    fn vehicles_reserve_target(&mut self, target: UKey, unit: UKey) { self.vehicles(|v,f|v.reserve_target(f,target,unit)) }
    fn vehicles_is_at_at(&mut self, unit: UKey) -> bool { self.vehicles(|v,f|v.is_at_at(f,unit)) }
    fn vehicles_support_point(&mut self, unit: UKey, post: usize) -> Option<Vec3> { let post=self.rows.state.population.post(post).expect("current native support post");
        let post=self.rows.state.entities.find(super::objects::post_entity(post)).expect("prepared native support post");
        let helper=self.rows.unit_controller(unit).and_then(|c|self.rows.state.controllers.core(&self.rows.state.entities,c)).and_then(|c|self.rows.state.agents.key(c));
        self.vehicles(|v,frame|v.support_point(frame,Some(unit),Some(post),helper)).map(point) }
    fn vehicles_board_policy(&mut self, controller: usize, candidate: UKey) -> Option<bool> { let key=self.controller(controller);let agent=self.agent(controller);Some(self.vehicles(|v,frame|v.board_policy(frame,agent,key,candidate))) }
    fn vehicles_board_tick(&mut self,core:&mut ctl::Core,controller:usize,_dt:f64) {
        // VehicleAI.boardTick is the optional stub-agent service. It reads
        // vehicle.unit without resolving its saved generation, as the source.
        let candidate=core.controllers[controller].agent.as_ref().and_then(|a|if let ctl::agents::AgentKind::Board {vehicle}=a.kind {vehicle.map(|r|r.unit)}else{None});
        let key=self.controller(controller);
        self.active_vehicles(core,|vehicles,frame| {
            let valid=candidate.is_some_and(|vehicle|vehicles.can_enter(frame,Some(key),vehicle)&&vehicles.hold_reservation(frame,Some(key),vehicle));
            if !valid {frame.hooks.core.request_state(ctl::Owner::Agent(controller),ctl::fsm::DONE);return;}
            let candidate=candidate.unwrap();let unit=frame.hooks.core.controllers[controller].unit.expect("boarding controller unit");
            let a=crate::ai::vehicles::world::World::sphere(frame,unit);let b=crate::ai::vehicles::world::World::sphere(frame,candidate);
            let reach=a.radius+b.radius+0.5;
            if a.center.distance_to_squared(&b.center)<reach*reach {vehicles.board_now(frame,Some(key),candidate);}
        });
    }
    fn vehicles_patrol_mounted(&mut self,core:&mut ctl::Core,controller:usize,patrol:ctl::patrol::Patrol,shared:&mut ForeignShared,_objectives:&[Option<usize>],_support_holds:Option<f64>)->Option<MountedPatrol> {
        let key=self.controller(controller);let agent=self.agent(controller);
        let post=patrol.post.and_then(|i|self.rows.state.population.post(i)).and_then(|p|self.rows.state.entities.find(super::objects::post_entity(p)));
        let objective=core.objective(controller).expect("mounted patrol objective");
        let view=crate::ai::vehicles::service::PatrolView {obj:agent,agent,controller:key,post,support:objective.support,helper:core.controllers[controller].agent.as_ref().is_some_and(|a|a.helper.kind.is_some())};
        let _kept=self.active_vehicles(core,|vehicles,frame|vehicles.patrol_mounted(frame,&view));
        // Native callbacks already publish objectives through this Core. A
        // refreshed shared view preserves those immediate effects, and no
        // delayed publication list or snapshot overwrites them on return.
        *shared=ForeignShared::read(&core.controllers[controller]);
        let objective=core.objective(controller).expect("mounted patrol survives its entry");
        Some(MountedPatrol {patrol:objective.patrol.clone(),support_holds:objective.support_holds,publications:Vec::new()})
    }
    fn vehicles_policy_of(&mut self, unit: UKey) -> Policy { self.vehicles(|v,f|match v.adapt_controller(f,Some(unit)).0 {None=>Policy::None,Some(crate::ai::vehicles::controllers::Policy::Mobile)=>Policy::Mobile,Some(crate::ai::vehicles::controllers::Policy::Stationary)=>Policy::Stationary,Some(crate::ai::vehicles::controllers::Policy::Fly)=>Policy::Fly}) }
    fn vehicles_policy_home_facing(&mut self, policy: Policy, unit: UKey) -> Option<bool> { match policy {Policy::Stationary=>Some(self.unit(unit).has_turret),_=>None} }
    fn vehicles_policy_agent(&mut self, policy: Policy, state: f64) -> bool { policy==Policy::Fly&&state==7. }
    fn foreign_agent_create(&mut self,core:&mut ctl::Core,controller:usize,kind:ForeignKind,_shared:&ForeignShared)->(ForeignTick,Option<ControlObs>) {
        // The source constructs the base helper before initAgent and the
        // combat subclass. Helper creation can adopt the navigation request.
        let helper=core.create_helper(self,controller);
        self.combat_init_agent(controller);let agent=self.agent(controller);
        self.rows.state.agents.get_mut(&self.rows.state.entities,agent).unwrap().helper=helper;
        self.rows.state.bind_agent_helper(agent,controller);
        if kind!=ForeignKind::Combat {
            let row=self.rows.state.agents.get_mut(&self.rows.state.entities,agent).unwrap();
            row.vehicle.initialize(kind,core.controllers[controller].unit);
            row.behavior=match kind {
                ForeignKind::Board=>super::agents::Behavior::Board(crate::ai::vehicles::agents::BoardAgent {vehicle:core.controllers[controller].commands.current.ref_a.map(|r|crate::ai::vehicles::world::Ref {obj:r.unit,generation:r.generation})}),
                ForeignKind::Unboard=>super::agents::Behavior::Unboard(Default::default()),
                ForeignKind::Fly=>super::agents::Behavior::Fly,
                ForeignKind::Combat=>unreachable!(),
            };
            let machine=row.vehicle.state();
            return (ForeignTick {state:machine.state,pending:machine.pending,helper,..Default::default()},core.controllers[controller].nav_control);
        }
        let goal=core.controllers[controller].commands.current;
        let goal=crate::ai::combat::agent::Goal {mode:Some(goal.mode),point:goal.point.map(|p|three::Vector3::new(p.x,p.y,p.z)),
            ref_a:goal.ref_a.map(|r|crate::ai::combat::world::Ref {obj:r.unit,generation:r.generation})};
        self.combat(|frame,_|frame.create_combat(agent,goal));
        let machine=self.rows.state.agents.combat(&self.rows.state.entities,agent).unwrap();
        (ForeignTick {state:machine.state,pending:machine.pending,helper,..Default::default()},core.controllers[controller].nav_control)
    }
    fn foreign_agent_tick(&mut self,core:&mut ctl::Core,controller:usize,dt:f64,phase:f64,_shared:&ForeignShared)->ForeignTick {
        let agent=self.agent(controller);
        if self.rows.state.agents.combat(&self.rows.state.entities,agent).is_none() {
            let combat_enabled=core.options.bind.combat;
            let (state,pending)=self.active_vehicles(core,|vehicles,frame|super::vehicle_driver::tick(frame,vehicles,agent,dt,phase,combat_enabled));
            let replaced=self.rows.state.agents.key(controller)!=Some(agent);
            let current=core.controllers[controller].agent.as_ref();
            return ForeignTick {state,pending,replaced,helper:self.observed_helper(controller,current.map(|a|a.helper).unwrap_or_default()),interrupts:current.map(|a|a.interrupts).unwrap_or_default(),shared:Some(ForeignShared::read(&core.controllers[controller])),effects:Vec::new()};
        }
        let mut hooks=super::active::Active {core,services:self.services,facts:self.facts,settings:self.settings};
        let mut frame=Frame {rows:self.rows,hooks:&mut hooks,random:self.random,settings:self.settings,time:self.facts.time,response:None};
        let (state,pending)=frame.run_combat(agent,self.combat,|frame,machine,combat| {
            machine.tick(frame,combat,dt,if phase==0. {crate::ai::combat::agent::Phase::Behave}else{crate::ai::combat::agent::Phase::Rank});
            (machine.state,machine.pending)
        });
        let replaced=frame.rows.state.agents.key(controller)!=Some(agent);
        let current=frame.hooks.core.controllers[controller].agent.as_ref();
        let mut helper=current.map(|a|a.helper).unwrap_or_default();
        if let Some(key)=frame.rows.state.agents.key(controller).and_then(|key|frame.rows.state.agents.get(&frame.rows.state.entities,key)).and_then(|a|a.navigation) {if let Some(obs)=frame.rows.state.navigation.helper_observation_for(frame.rows.store,key) {helper.obs=obs;}}
        let interrupts=current.map(|a|a.interrupts).unwrap_or_default();
        let mut shared=ForeignShared::read(&frame.hooks.core.controllers[controller]);
        // The running CombatAgent owns aimTarget, even when its old look
        // survives a weapon-target replacement during this decision pass.
        shared.aim_target=frame.rows.combat_aim_target(controller);
        ForeignTick {state,pending,replaced,helper,interrupts,shared:Some(shared),effects:Vec::new()}
    }
    fn foreign_agent_exit(&mut self,core:&mut ctl::Core,controller:usize,_shared:&ForeignShared)->(Option<ForeignShared>,Vec<CoreEffect>) {
        let agent=self.agent(controller);
        if self.rows.state.agents.combat(&self.rows.state.entities,agent).is_none() {
            self.active_vehicles(core,|_,frame|super::vehicle_driver::finish(frame,agent));
            return (Some(ForeignShared::read(&core.controllers[controller])),Vec::new());
        }
        let mut hooks=super::active::Active {core,services:self.services,facts:self.facts,settings:self.settings};
        let mut frame=Frame {rows:self.rows,hooks:&mut hooks,random:self.random,settings:self.settings,time:self.facts.time,response:None};
        frame.run_combat(agent,self.combat,|frame,machine,combat|machine.exit(frame,combat));
        (Some(ForeignShared::read(&frame.hooks.core.controllers[controller])),Vec::new())
    }
    fn helper_request_invalidated(&mut self,controller:usize) {
        if let Some(actor)=self.rows.state.actor(controller) {if let Some(control)=self.rows.state.navigation.control_mut(actor) {control.request.valid=false;}}
    }
    fn helper_create(&mut self, controller: usize) -> (HelperKind, HelperObs, Option<ControlObs>) { Services::helper_create(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller) }
    fn helper_goto(&mut self, controller: usize, point: Vec3, mode: GotoMode) -> HelperObs { Services::helper_goto(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,point,mode) }
    fn helper_stop(&mut self, controller: usize) -> HelperObs { Services::helper_stop(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller) }
    fn helper_terminate(&mut self, controller: usize) -> HelperObs { Services::helper_terminate(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller) }
    fn helper_tick(&mut self, controller: usize, dt: f64) -> HelperObs { Services::helper_tick(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,dt) }
    fn helper_destination(&mut self, controller: usize) -> Vec3 { Services::helper_destination(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller) }
    fn helper_install_path(&mut self, controller: usize, points: &[Vec3]) -> HelperObs { Services::helper_install_path(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,points) }
    fn helper_follow(&mut self, controller: usize, target: EntRef) -> HelperObs { Services::helper_follow(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,target) }
    fn helper_follow_trail(&mut self, controller: usize, on: bool) -> HelperObs { Services::helper_follow_trail(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,on) }
    fn helper_hold_path(&mut self, controller: usize, on: bool) -> HelperObs { Services::helper_hold_path(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,on) }
    fn helper_set_leader(&mut self, controller: usize, leader: Option<EntRef>) -> HelperObs { Services::helper_set_leader(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,leader) }
    fn helper_dispose(&mut self, controller: usize) { Services::helper_dispose(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller) }
    fn nav_control_ensure(&mut self, controller: usize) -> ControlObs { Services::nav_control_ensure(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller) }
    fn nav_control_reset(&mut self, controller: usize) { Services::nav_control_reset(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller) }
    fn nav_control_obs(&mut self, controller: usize) -> ControlObs { Services::nav_control_obs(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller) }
    fn nav_planner_idle(&mut self) -> bool { self.rows.state.navigation.nav.planner_idle() }
    fn nav_service(&mut self, budget: f64) { self.rows.state.navigation.nav.service(budget); }
    fn nav_point_clear(&mut self, point: Vec3, radius: f64, mask: f64) -> bool { self.rows.state.navigation.nav.barriers.point_clear(point,radius,v8math::to_uint32(mask)).0 }
    fn nav_clip_segment(&mut self, from: Vec3, to: Vec3, radius: f64, mask: f64) -> (bool, Vec3) { let mut end=to;let hit=self.rows.state.navigation.nav.barriers.obstruct(from,&mut end,radius,v8math::to_uint32(mask),None,None);(hit,end) }
    fn nav_in_danger_region(&mut self, point: Vec3, pad: f64) -> bool { let _=(point,pad);false }
    fn nav_query_hints(&mut self, mask: f64, center: Vec3, half_extent: f64, cap: f64, requester: Option<UKey>) -> List<usize> { self.rows.state.hints.as_ref().expect("prepared native hints").query(&self.rows.state.navigation.nav,v8math::to_uint32(mask),center,half_extent,cap,requester,self.facts.time) }
    fn nav_hints_len(&self) -> usize { self.rows.state.navigation.nav.hints.list.len() }
    fn nav_hint(&self, hint: usize) -> HintSnap { self.rows.state.hints.as_ref().expect("prepared native hints").read(&self.rows.state.navigation.nav,hint) }
    fn nav_hint_available(&mut self, hint: usize) -> bool { self.rows.state.navigation.nav.hint_available(hint,self.facts.time) }
    fn nav_mark_hint(&mut self, hint: usize, unit: Option<UKey>, prefer_world: bool) { let _=prefer_world;self.rows.state.navigation.nav.mark_hint(hint,unit,self.facts.time); }
    fn nav_hint_set_reserved(&mut self, hint: usize, at: f64) { self.rows.state.navigation.nav.hints.list[hint].reserved_at=at; }
    fn perception_flags(&mut self, controller: usize) -> (bool, bool) { let c=self.rows.state.controllers.read(&self.rows.state.entities,self.controller(controller)).unwrap();(c.perception.is_some(),c.perception.is_some()) }
    fn combat_writes(&mut self, controller: usize) -> CombatWrites { self.rows.combat_writes(controller).expect("current native combat writes") }
    fn network_secondary_active(&self) -> bool { self.facts.network_secondary }
    fn nav_navigation_input(&mut self, core:&mut ctl::Core, controller: usize, step: f64, aim: Option<Vec3>,facing:bool) -> (Option<HelperObs>, Vec<CoreEffect>) {
        if self.mounted(core,controller) {
            self.seat_input(core,controller,step,aim,true);
            let stationary=core.controllers[controller].agent.as_ref().is_some_and(|agent|agent.helper.kind==Some(HelperKind::Stationary));
            let obs=if stationary {None}else{self.rows.state.actor(controller).and_then(|actor|self.rows.state.navigation.helper_observation(self.rows.store,actor))};
            return (obs,Vec::new());
        }
        Services::nav_navigation_input(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,step,aim,facing)
    }
    fn nav_analogs(&mut self, core:&mut ctl::Core, controller: usize, aim: Option<Vec3>, step: f64,facing:bool) -> ([f64; 4], Vec<CoreEffect>) {
        if self.mounted(core,controller) {return (self.seat_input(core,controller,step,aim,false),Vec::new());}
        Services::nav_analogs(self.services,self.rows,self.random,self.combat,self.facts,self.settings,controller,aim,step,facing)
    }
    fn agent_created(&mut self,controller:usize,helper:ctl::agents::Helper) {
        let key=self.rows.state.agents.key(controller).unwrap_or_else(|| {
            let owner=self.controller(controller);self.rows.state.agents.begin(&mut self.rows.state.entities,controller,owner)
        });
        self.rows.state.agents.get_mut(&self.rows.state.entities,key).unwrap().helper=helper;
        self.rows.state.bind_agent_helper(key,controller);
        self.rows.state.controllers.helper_created(controller,helper);
    }
    fn agent_disposed(&mut self,controller:usize,helper:ctl::agents::Helper) {if let Some(key)=self.rows.state.agents.key(controller) {self.rows.state.agents.get_mut(&self.rows.state.entities,key).unwrap().helper=helper;}self.rows.state.agent_disposed(controller);}
}

impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> super::runtime::World for World<'_,'_,C,M,S> {
    fn difficulty_changed(&mut self,difficulty:&ctl::difficulty::Difficulty) {
        self.settings.difficulty.replace(difficulty.clone());
    }
    fn prepare_aimer(&mut self,core:&ctl::Core,controller:usize)->bool {self.rows.combat_aimer_feed(controller,&core.controllers[controller]);false}
    fn aim_controller(&mut self,controller:usize,dt:f64) {
        let key=self.controller(controller);self.combat(|frame,combat|{combat.aim(frame,key,dt);});
    }
    fn decay(&mut self,core:&mut ctl::Core,random:&mut RandomState,dt:f64) {
        let binding=self.services.vehicle_binding();
        let mut hooks=super::active::Active {core,services:self.services,facts:self.facts,settings:self.settings};
        let mut frame=super::vehicle_frame::Frame {rows:self.rows,hooks:&mut hooks,random,combat:self.combat,
            combat_settings:self.settings,settings:&binding.settings,time:self.facts.time,navigation:binding.navigation,population:binding.population};
        binding.authority.update_decay(&mut frame,dt);
    }
}
impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> super::lifecycle::Bindings for World<'_,'_,C,M,S> {
    fn character_index(&self,handle:&crate::gen::F4Handle)->Option<usize> {
        let reference=self.rows.characters.resolve(self.rows.store,handle)?;
        self.rows.state.roster.index(self.rows.store,self.rows.characters,reference)
    }
    fn post_index(&self,post:crate::gen::SpawnPost)->Option<usize> {
        let present=match post {crate::gen::SpawnPost::Post(r)=>self.rows.store.post.contains(r),crate::gen::SpawnPost::MobilePost(r)=>self.rows.store.mobile_post.contains(r)};
        present.then(||self.rows.state.population.index(crate::battle::posts::post_ref(Some(post)))).flatten()
    }
}

impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> World<'_,'_,C,M,S> {
    /// Game's post-battle grenade loop visits native projectile insertion
    /// order. Even an untouched airborne grenade draws its delay before the
    /// contact/time tests; moving those draws behind eligibility changes A.
    pub fn grenade_dangers(&mut self,core:&mut ctl::Core,mut viewpoint:impl FnMut()->Option<crate::store::Ref<crate::gen::Unit>>)->usize {
        use crate::gen::OrdnanceOwner;
        use super::entities::Entity;
        let mut visited=0;
        for index in 0..self.rows.store.ordnance.grenades.len() {
            let reference=self.rows.store.ordnance.grenades[index];
            let Some(grenade)=self.rows.store.grenade.get(reference) else{continue};
            if grenade.gone==Some(true) {continue;}
            let position=three::Vector3::new(grenade.position[0],grenade.position[1],grenade.position[2]);
            let owner=match grenade.owner {
                OrdnanceOwner::Unit(unit)=>self.rows.state.entities.find(Entity::Unit(unit)),
                OrdnanceOwner::Vehicle(hull)=>self.rows.state.entities.find(Entity::Hull(hull)),
                OrdnanceOwner::Player=>viewpoint().and_then(|unit|self.rows.state.entities.find(Entity::Unit(unit))),
                // A captured launch owner is never a registry member. Source
                // query exclusion therefore cannot exclude its later body.
                OrdnanceOwner::Null|OrdnanceOwner::OwnerSnapshot(_)=>None,
            };
            let (mut danger,contact)=self.rows.state.dangers.begin(reference);
            let mut frame=Frame {rows:self.rows,hooks:self.services,random:self.random,settings:self.settings,time:self.facts.time,response:None};
            self.combat.grenade_update_with(&mut frame,&mut danger,&position,owner,contact,&mut |frame,controller| {
                let index=frame.rows.state.controllers.core(&frame.rows.state.entities,controller).expect("current danger recipient");
                core.controllers[index].notify_danger(point(position));
            });
            frame.rows.state.dangers.finish(reference,danger);visited+=1;
        }
        visited
    }
}
