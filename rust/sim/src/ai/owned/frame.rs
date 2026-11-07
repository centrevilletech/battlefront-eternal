//! The combat world borrows canonical rows for its entire service scope.
//! Geometry and cross-owner actions receive that same scope explicitly; no
//! controller operation crosses the host or recreates a body service bag.
use super::{arsenals::Mounted,rows::Rows,weapons::Changes};
use crate::ai::{combat::{world::*},control::difficulty::Difficulty,phase::RandomState};
use three::Vector3;
use super::scratch::List;

pub struct Settings {
    /// Retarget updates the same native view while service frames are active.
    /// Borrows below contain only scalar arithmetic and end before callbacks.
    pub difficulty:std::cell::RefCell<Difficulty>,
    pub mission_stay_in_turrets:bool,
    pub mission_no_jet_obstacle_test:bool,
    pub ai_stay_in_turrets:bool,
    pub vehicles:bool,
    pub dense:bool,
    pub navigation:bool,
}
/// A response observes Core policy state at the original interrupt slot.
/// Enhanced memory shares its authority; this never clones a perception table.
pub struct ResponseScope {
    pub state:crate::ai::control::world::ResponseState,
    pub shared:crate::ai::combat::weapons_ai::Shared,
}
impl ResponseScope {
    pub fn take(state:&mut crate::ai::control::world::ResponseState,shared:crate::ai::combat::weapons_ai::Shared)->Self {
        Self {state:std::mem::take(state),shared}
    }
    pub fn finish(self,state:&mut crate::ai::control::world::ResponseState) {*state=self.state;}
}
pub struct Frame<'a,'b,C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> {
    pub rows:&'a mut Rows<'b,C,M>,
    pub hooks:&'a mut H,
    pub random:&'a mut RandomState,
    pub settings:&'a Settings,
    pub time:f64,
    pub response:Option<ResponseScope>,
}
/// These actions belong to geometry, navigation, vehicles or the active Core.
/// Implementations loan the canonical owners and return before another query;
/// they never retain a row borrow across a lifecycle callback.
pub trait Hooks<C:Changes+?Sized,M:Mounted>:super::navigation::Environment {
    fn vehicle_binding(&self)->super::vehicles::Binding;
    fn fire_primary(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,controller:Obj);
    fn flight_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent:Obj,points:&[[f64;3]],over:bool,altitude:bool);
    fn mount_origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,origin:crate::ai::vehicles::world::Origin)->Option<Vector3>;
    fn origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3;
    fn forward(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3;
    fn aim_point(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Vector3;
    fn sphere(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj)-> Sphere;
    fn use_classic_random(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,u: Obj);
    fn query(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,center: &Vector3,radius: f64,filter: QueryFilter)-> List<Obj>;
    fn relation(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,a: f64,b: f64)-> f64;
    fn living(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState)-> List<Obj>;
    fn los(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,from: &Vector3,to: &Vector3,a: Option<Obj>,b: Option<Obj>)-> bool;
    fn set_stance(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,c:Obj,stance:f64);
    fn human_target(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,hull:Obj)->Option<Obj>;
    fn interrupts_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,combat:&mut crate::ai::combat::binding::CombatAi)-> bool;
    fn has_helper(&self,rows:&Rows<'_,C,M>,agent: Obj)-> bool;
    fn helper_arrived(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj)-> bool;
    fn helper_flying(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj)-> bool;
    fn helper_has_follow(&self,rows:&Rows<'_,C,M>,agent: Obj)-> bool;
    fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,point: &Vector3,mode: Option<GotoMode>);
    fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj);
    fn helper_tick(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,dt: f64);
    fn helper_follow(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj,target: Obj);
    fn helper_terminate(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,agent: Obj);
    fn request_jet(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,controller: Obj,vector: &Vector3);
    fn emit_vo(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,unit: Obj,event: &str,property: &str);
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> Draws for Frame<'_, '_,C,M,H> {
    fn uniform_a(&mut self)->f64 {Draws::uniform_a(self.random)}
    fn uniform_b(&mut self)->f64 {Draws::uniform_b(self.random)}
    fn uniform_c(&mut self)->f64 {Draws::uniform_c(self.random)}
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> World for Frame<'_, '_,C,M,H> {
    fn time(&self)-> f64 { self.time }
    fn difficulty(&self)-> DifficultyView { {let d=self.settings.difficulty.borrow();let row=d.danger_row();DifficultyView {present:true,enemy_skill:Some(d.enemy_skill),hard:Some(d.hard()),tier:Some(d.tier),danger_row:Some((row[0],row[1]))}} }
    fn mission_stay_in_turrets(&self)-> bool { self.settings.mission_stay_in_turrets }
    fn mission_no_jet_obstacle_test(&self)-> bool { self.settings.mission_no_jet_obstacle_test }
    fn ai_stay_in_turrets(&mut self)-> bool { self.settings.ai_stay_in_turrets }
    fn unit(&self, u: Obj)-> UnitView { self.rows.unit(u) }
    fn unit_sensor(&self,u:Obj)->crate::ai::combat::world::UnitSensor {self.rows.unit_sensor(u)}
    fn unit_weapon_motion(&self,u:Obj)->(f64,f64) {self.rows.unit_weapon_motion(u)}
    fn visit_weapon_levels(&self,u:Obj,index:usize,visit:&mut dyn FnMut(&crate::combat::defs::JsObj)) {self.rows.visit_weapon_levels(u,index,visit)}
    fn unit_bound_posture(&self,u:Obj)->Option<crate::ai::combat::world::BoundPosture> {self.rows.unit_bound_posture(u)}
    fn unit_scalars(&self,u:Obj)->UnitScalars {self.rows.combat_scalars(u)}
    fn unit_velocity(&self,u:Obj)->Option<Vector3> {self.rows.unit_velocity(u)}
    fn unit_controller(&self,u:Obj)->Option<Obj> {self.rows.unit_controller(u)}
    fn unit_life(&self,u:Obj)->(bool,f64) {self.rows.unit_life(u)}
    fn unit_priority(&self,u:Obj)->PriorityView {self.rows.unit_priority(u)}
    fn unit_target_status(&self,u:Obj)->TargetStatus {self.rows.unit_target_status(u)}
    fn unit_has_origin(&self,u:Obj)->bool {self.rows.unit_has_origin(u)}
    fn unit_has_buttons(&self,u:Obj)->bool {self.rows.unit_has_buttons(u)}
    fn has_arsenal(&self,u:Obj)->bool { self.rows.has_arsenal(u) }
    fn weapon_count(&self,u:Obj)->usize { self.rows.weapon_count(u) }
    fn current_weapon(&self,u:Obj,channel:f64)->Option<usize> { self.rows.current_weapon(u,channel) }
    fn weapon(&self,u:Obj,index:usize)->Option<WeaponView> { self.rows.weapon(u,index) }
    fn weapon_for_burst(&self,u:Obj,index:usize,active:bool)->Option<WeaponView> {
        if !active {None}
        else {Some(self.rows.weapon(u,index).expect("arsenal member"))}
    }
    fn weapon_chargeable(&self,u:Obj,index:usize)->bool {self.rows.weapon_chargeable(u,index)}
    fn weapon_definition(&self,u:Obj,index:usize)->Option<std::rc::Rc<crate::combat::defs::JsObj>> {self.rows.weapon_definition(u,index)}
    fn body_lease_count(&self,u:Obj)->usize { self.rows.body_lease_count(u) }
    fn body_lease(&self,u:Obj,index:usize)->Option<LeaseView> { self.rows.body_lease(u,index) }
    fn set_weapon_yaw(&mut self,u:Obj,index:usize,value:Option<f64>) {self.rows.set_weapon_yaw(u,index,value);}
    fn weapon_settings(&self,u:Obj,index:usize)->WeaponSettings { self.rows.weapon_settings(u,index) }
    fn edit_weapon<R>(&mut self,u:Obj,index:usize,edit:impl FnOnce(&mut WeaponSettings)->R)->R { self.rows.edit_weapon(u,index,edit) }
    fn origin(&mut self, u: Obj)-> Vector3 { self.hooks.origin(self.rows,self.random,u) }
    fn forward(&mut self, u: Obj)-> Vector3 { self.hooks.forward(self.rows,self.random,u) }
    fn aim_point(&mut self, u: Obj)-> Vector3 { self.hooks.aim_point(self.rows,self.random,u) }
    fn sphere(&mut self, u: Obj)-> Sphere { self.hooks.sphere(self.rows,self.random,u) }
    fn set_intent_buttons(&mut self, u: Obj, b0: f64, b1: f64) { self.rows.buttons(u,b0,b1) }
    fn set_intent_aim(&mut self, u: Obj, aim: Option<Vector3>) { self.rows.aim(u,aim) }
    fn arsenal_select(&mut self, u: Obj, index: usize)-> bool { self.rows.select(u,index) }
    fn use_classic_random(&mut self, u: Obj) { self.hooks.use_classic_random(self.rows,self.random,u) }
    fn object_buffer(&self)-> List<Obj> { self.rows.state.buffers.units.take() }
    fn query(&mut self, center: &Vector3, radius: f64, filter: QueryFilter)-> List<Obj> { self.hooks.query(self.rows,self.random,center,radius,filter) }
    fn relation(&mut self, a: f64, b: f64)-> f64 { self.hooks.relation(self.rows,self.random,a,b) }
    fn living(&mut self)-> List<Obj> { self.hooks.living(self.rows,self.random) }
    fn los(&mut self, from: &Vector3, to: &Vector3, a: Option<Obj>, b: Option<Obj>)-> bool { self.hooks.los(self.rows,self.random,from,to,a,b) }
    fn curves_for(&mut self, team: f64)-> Option<Curves> { {let c=self.settings.difficulty.borrow().curves(team);Some(Curves {a:c.a,d:c.d,b:Some(c.b),s:Some(c.s)})} }
    fn character_team(&self, character: Obj)-> f64 { self.rows.character_team(character) }
    fn fresh_perception(&mut self,c:Obj)->crate::ai::combat::perception::Perception { self.rows.state.controllers.fresh_perception(&self.rows.state.entities,c).expect("prepared native perception") }
    /// targetPriority reads Character.objective only for a post target.
    /// No duplicated objective travels through unrelated weapon/aim feeds.
    fn controller_objective(&self,c:Obj)->Option<Obj> {
        use super::entities::Entity;
        let Entity::Character(r)=self.rows.state.entities.get(self.ctl(c).character?)? else{return None};
        let entity=match self.rows.store.character.get(r)?.objective {
            crate::gen::PostRef::Post(r)=>Entity::Post(r),
            crate::gen::PostRef::MobilePost(r)=>Entity::MobilePost(r),
            _=>return None,
        };
        self.rows.state.entities.find(entity)
    }
    fn ctl(&self, c: Obj)-> &Ctl { self.rows.state.controllers.read(&self.rows.state.entities,c).expect("current native controller") }
    fn ctl_mut(&mut self, c: Obj)-> &mut Ctl { self.rows.state.controllers.write(&self.rows.state.entities,c).expect("current native controller") }
    fn notify_shot(&mut self,c:Obj,source:Option<Obj>,known:bool,point:Option<Vector3>) {
        if self.rows.state.controllers.capturing_shots() {
            let Some(source)=source else{return};
            use crate::ai::combat::control::Objects as _;
            let objects=super::objects::Objects {store:self.rows.store,commands:self.rows.characters,entities:&self.rows.state.entities,roster:&self.rows.state.roster,population:&self.rows.state.population};
            let source=objects.alert_source(source);
            self.rows.state.controllers.shot(&self.rows.state.entities,c,source,known,point);
        }else{self.ctl_mut(c).notify_shot(source,known,point);}
    }
    fn set_stance(&mut self,c:Obj,stance:f64) { self.hooks.set_stance(self.rows,self.random,c,stance) }
    fn agent_controller(&self, agent: Obj)-> Obj { self.rows.state.agents.get(&self.rows.state.entities,agent).expect("current native agent").controller }
    fn tracker(&mut self, agent: Obj)-> Option<&mut crate::ai::combat::perception::TargetTracker> { Some(self.rows.state.agents.get_mut(&self.rows.state.entities,agent)?.tracker_mut()) }
    fn is_controller(&self, c: Obj)-> bool { self.rows.state.controllers.read(&self.rows.state.entities,c).is_some() }
    fn agent_accepts_alert(&mut self, agent: Obj, known: bool, point: &Vector3)-> bool {
        let mut scope=self.response.take().expect("native Core response scope");
        let behavior=self.rows.state.agents.get(&self.rows.state.entities,agent).expect("current response agent").behavior.clone();
        let result=match behavior {
            super::agents::Behavior::Base=>self.base_accepts_alert(&scope.shared,agent,&mut scope.state,known,crate::ai::nav::math::Vec3::new(point.x,point.y,point.z)),
            super::agents::Behavior::Combat(machine)=>machine.accepts_alert(self,known,point),
            super::agents::Behavior::Board(board)=>self.board_contested(agent,board),
            super::agents::Behavior::Unboard(_)|super::agents::Behavior::Fly=>false,
        };
        self.response=Some(scope);result
    }
    fn agent_accepts_hazard(&mut self, agent: Obj)-> bool {
        let mut scope=self.response.take().expect("native Core response scope");
        let behavior=self.rows.state.agents.get(&self.rows.state.entities,agent).expect("current response agent").behavior.clone();
        let result=match behavior {
            super::agents::Behavior::Base=>self.base_accepts_hazard(&scope.shared,agent,&mut scope.state),
            super::agents::Behavior::Combat(machine)=>machine.accepts_hazard(self),
            super::agents::Behavior::Board(board)=>self.board_contested(agent,board),
            super::agents::Behavior::Unboard(_)|super::agents::Behavior::Fly=>false,
        };
        self.response=Some(scope);result
    }
    fn agent_accepts_acoustic(&mut self, agent: Obj, source: Obj)-> bool {
        let mut scope=self.response.take().expect("native Core response scope");
        let behavior=self.rows.state.agents.get(&self.rows.state.entities,agent).expect("current response agent").behavior.clone();
        let result=match behavior {
            super::agents::Behavior::Base=>self.base_accepts_acoustic(&scope.shared,agent,&mut scope.state,source),
            super::agents::Behavior::Combat(machine)=>machine.accepts_acoustic_shared(self,&mut scope.shared,Some(source)),
            super::agents::Behavior::Board(board)=>{
                let controller=self.agent_controller(agent);
                self.board_contested(agent,board)&&crate::ai::combat::agent::can_engage_shared(self,&mut scope.shared,controller,Some(source))
            },
            super::agents::Behavior::Unboard(_)|super::agents::Behavior::Fly=>false,
        };
        self.response=Some(scope);result
    }
    fn shared_target(&mut self,controller:Obj)->Option<Obj> {if self.settings.vehicles {crate::ai::vehicles::service::shared_target(self,Some(controller))}else{None}}
    fn has_vehicle_dispatch(&self)->bool { self.settings.vehicles }
    fn vehicle_dispatch(&mut self,shared:&mut crate::ai::combat::weapons_ai::Shared,controller:Obj,unit:Obj,target:Obj,switch_allowed:bool)->Engagement {
        let unit=self.rows.scene().vehicle(unit).expect("current dispatch control");
        let target=self.rows.scene().vehicle(target).expect("current dispatch target");
        let dense=self.settings.dense;
        let mut out=crate::ai::vehicles::patterns::Dispatch::default();
        crate::ai::vehicles::patterns::vehicle_dispatch(&mut VehicleDispatch {frame:self,shared},controller,&unit,&target,switch_allowed,dense,&mut out);
        Engagement {pattern:out.pattern,probability:out.probability,aux:out.aux,action:out.action,in_band:out.in_band,weapon:out.weapon.map(|index|index as usize)}
    }
    fn has_mount_reach(&self)->bool {self.settings.vehicles}
    fn mount_reach(&mut self,unit:Obj,point:&Vector3)->bool {crate::ai::vehicles::service::mount_reach(self,Some(unit),point)}
    fn current_controllable(&mut self, character: Obj)-> (bool, Option<Obj>) { if self.settings.vehicles {(true,self.rows.current_controllable(character))}else{(false,None)} }
    // Character has no remote property; RemoteDroids retains unit owners.
    fn character_remote(&self, _character: Obj)-> Option<Obj> { None }
    fn character_unit(&self, character: Obj)-> Option<Obj> { self.rows.character_unit(character) }
    fn is_unit(&self, o: Obj)-> bool { self.rows.scene().combat(o).is_some() }
    fn enter_pattern(&mut self, agent: Obj, state: f64) { self.mounted_pattern(agent,state,super::patterns::Operation::Enter) }
    fn exit_pattern(&mut self, agent: Obj, state: f64) { self.mounted_pattern(agent,state,super::patterns::Operation::Exit) }
    fn tick_pattern(&mut self, agent: Obj, state: f64, dt: f64) { {let _=dt;self.mounted_pattern(agent,state,super::patterns::Operation::Tick);} }
    fn sidestep(&mut self, agent: Obj) { self.mounted_pattern(agent,0.,super::patterns::Operation::Sidestep) }
    fn cover_hints(&mut self,center:&Vector3,half:f64)->List<HintView> {
        let Some(hints)=self.rows.state.hints.as_ref() else{return List::default()};
        hints.cover(&self.rows.state.navigation.nav,crate::ai::nav::math::Vec3::new(center.x,center.y,center.z),half,self.time,
            |index|self.rows.state.entities.find(super::entities::Entity::Hint(index as u32)).expect("prepared native cover hint"))
    }
    fn has_jet_hints(&self)-> bool { self.settings.navigation }
    fn jet_hints_excluded(&mut self, controller: Obj)-> bool { let _=controller;false }
    fn jet_hints_find(&mut self, controller: Obj)-> Option<Obj> { let unit=self.ctl(controller).unit?;let p=self.rows.unit(unit).position;
        let hints=self.rows.state.hints.as_ref().expect("prepared native jet hints");
        let hint=self.rows.state.jets.find(&self.rows.state.navigation,hints,unit,crate::ai::nav::math::Vec3::new(p.x,p.y,p.z),self.time,self.random)?;
        self.rows.state.entities.find(super::entities::Entity::Hint(hint as u32)) }
    fn jet_hints_enter(&mut self, controller: Obj, hint: Option<Obj>) { let core=self.rows.state.controllers.core(&self.rows.state.entities,controller).expect("current native jet controller");
        let present=self.rows.state.controllers.helper(&self.rows.state.entities,controller).is_some_and(|(kind,_)|kind.is_some());
        let actor=if present {self.rows.state.actor(core)}else{None};
        let hint=hint.map(|key|match self.rows.state.entities.get(key).expect("current native jet hint") {super::entities::Entity::Hint(index)=>index as usize,_=>panic!("native jet hint identity")});
        self.rows.state.jets.enter(controller,actor,hint,&mut self.rows.state.navigation,self.rows.store,self.rows.seats,self.random,self.hooks); }
    fn jet_hints_exit(&mut self, controller: Obj, hint: Option<Obj>) { let _=hint;self.rows.state.jets.exit(controller,&mut self.rows.state.navigation); }
    fn jet_hints_tick(&mut self, controller: Obj, hint: Option<Obj>, dt: f64)-> bool { let _=(hint,dt);self.rows.state.jets.tick(controller,&mut self.rows.state.navigation,self.rows.store,self.rows.seats,self.random,self.hooks) }
    // ClassicNav exposes barriers internally, not these optional extensions.
    fn nav_point_clear(&mut self, _point: &Vector3, _unit: Obj)-> Option<bool> { None }
    fn nav_segment_blocked(&mut self, _from: &Vector3, _to: &Vector3, _unit: Obj)-> Option<bool> { None }
    // ClassicNav has neither optional service. Presence affects combat's
    // fallback policy, so an empty region table must not create one here.
    fn nav_direct_clear(&mut self, _unit: Obj, _point: &Vector3)-> Option<bool> { None }
    fn nav_in_danger_region(&mut self, _unit: Obj)-> Option<bool> { None }
    fn agent_sync(&mut self, agent: &crate::ai::combat::agent::CombatAgent) { {assert!(self.rows.state.agents.sync(&self.rows.state.entities,agent));} }
    fn agent_resync(&mut self, agent: &mut crate::ai::combat::agent::CombatAgent) { {*agent=self.rows.state.agents.combat(&self.rows.state.entities,agent.obj).expect("current native combat agent");} }
    fn interrupts_tick(&mut self, agent: Obj,combat:&mut crate::ai::combat::binding::CombatAi)-> bool { self.hooks.interrupts_tick(self.rows,self.random,agent,combat) }
    fn agent_board_policy(&mut self, agent: Obj, candidate: Obj)-> bool {
        if !self.settings.vehicles {return crate::ai::control::agents::nominal_speed(self.rows.scene().policy(candidate).as_ref())>0.;}
        let controller=self.agent_controller(agent);
        crate::ai::vehicles::service::board_policy(self,agent,controller,candidate)
    }
    fn has_helper(&self, agent: Obj)-> bool { self.hooks.has_helper(self.rows,agent) }
    fn helper_arrived(&mut self, agent: Obj)-> bool { self.hooks.helper_arrived(self.rows,self.random,agent) }
    fn helper_flying(&mut self, agent: Obj)-> bool { self.hooks.helper_flying(self.rows,self.random,agent) }
    fn helper_has_follow(&self, agent: Obj)-> bool { self.hooks.helper_has_follow(self.rows,agent) }
    fn helper_goto(&mut self, agent: Obj, point: &Vector3, mode: Option<GotoMode>) { self.hooks.helper_goto(self.rows,self.random,agent,point,mode) }
    fn helper_stop(&mut self, agent: Obj) { self.hooks.helper_stop(self.rows,self.random,agent) }
    fn helper_tick(&mut self, agent: Obj, dt: f64) { self.hooks.helper_tick(self.rows,self.random,agent,dt) }
    fn helper_follow(&mut self, agent: Obj, target: Obj) { self.hooks.helper_follow(self.rows,self.random,agent,target) }
    fn helper_terminate(&mut self, agent: Obj) { self.hooks.helper_terminate(self.rows,self.random,agent) }
    fn request_jet(&mut self, controller: Obj, vector: &Vector3) { self.hooks.request_jet(self.rows,self.random,controller,vector) }
    fn emit_vo(&mut self, unit: Obj, event: &str, property: &str) { self.hooks.emit_vo(self.rows,self.random,unit,event,property) }
}

impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> super::policy::Facts for Frame<'_,'_,C,M,H> {
    fn policy_post_contains(&self,index:usize,position:crate::ai::nav::math::Vec3)->bool {
        let facts=self.rows.state.bindings.facts(self.rows.buildings.rows());
        let post=self.rows.state.posts.read(self.rows.store,&self.rows.state.population,index,&facts).expect("current native AI post");
        crate::ai::control::goals::post_contains(&post,position)
    }
    fn policy_hint_forward(&self,index:usize)->crate::ai::nav::math::Vec3 {self.rows.state.navigation.nav.hints.list[index].forward}
    fn policy_memory(&mut self,controller:Obj)->crate::ai::control::controller::EnhancedMemory {
        self.rows.state.controllers.new_memory(&self.rows.state.entities,controller).expect("current native controller memory")
    }
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> Frame<'_,'_,C,M,H> {
    fn policy_input(&self,agent:Obj)->(Obj,crate::ai::control::policy::Input) {
        let row=self.rows.state.agents.get(&self.rows.state.entities,agent).expect("current native agent");
        assert!(matches!(row.behavior,super::agents::Behavior::Base),"control-owned agent policy");
        let controller=row.controller;
        (controller,self.rows.state.controllers.policy(&self.rows.state.entities,controller).expect("current native policy"))
    }
    /// Control-owned agents use the Core's explicit response scope. Combat
    /// and mounted agents retain their specialized acceptance methods.
    /// Combat never
    /// reacquires the active Core while its interrupt helper is borrowed.
    pub fn base_accepts_alert(&mut self,shared:&crate::ai::combat::weapons_ai::Shared,agent:Obj,state:&mut crate::ai::control::world::ResponseState,known:bool,point:crate::ai::nav::math::Vec3)->bool {
        let (controller,input)=self.policy_input(agent);
        crate::ai::control::policy::accepts_alert(input,state,&mut super::policy::Scene::new(self,shared,controller,agent),known,point)
    }
    pub fn base_accepts_hazard(&mut self,shared:&crate::ai::combat::weapons_ai::Shared,agent:Obj,state:&mut crate::ai::control::world::ResponseState)->bool {
        let (controller,input)=self.policy_input(agent);
        crate::ai::control::policy::accepts_hazard(input,state,&mut super::policy::Scene::new(self,shared,controller,agent))
    }
    pub fn base_accepts_acoustic(&mut self,shared:&crate::ai::combat::weapons_ai::Shared,agent:Obj,state:&mut crate::ai::control::world::ResponseState,source:Obj)->bool {
        let (controller,input)=self.policy_input(agent);
        crate::ai::control::policy::accepts_acoustic(input,state,&mut super::policy::Scene::new(self,shared,controller,agent),source)
    }
    pub fn base_accepts_board(&mut self,shared:&crate::ai::combat::weapons_ai::Shared,agent:Obj,candidate:Obj)->bool {
        let (controller,input)=self.policy_input(agent);
        crate::ai::control::policy::accepts_board(input,&mut super::policy::Scene::new(self,shared,controller,agent),candidate)
    }
}

impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> Frame<'_,'_,C,M,H> {
    /// BoardAgent and FlyAgent override Agent.acceptsTarget. Observe their
    /// own live machine before CanEngage, preserving early query gates.
    pub fn target_override(&mut self,shared:&mut crate::ai::combat::weapons_ai::Shared,agent:Obj,target:Obj)->Option<bool> {
        let row=self.rows.state.agents.get(&self.rows.state.entities,agent)?;
        let controller=row.controller;
        match row.behavior.clone() {
            super::agents::Behavior::Base=>None,
            super::agents::Behavior::Combat(_)|super::agents::Behavior::Unboard(_)=>Some(false),
            super::agents::Behavior::Board(board)=>Some(self.board_contested(agent,board)
                &&crate::ai::combat::agent::can_engage_shared(self,shared,controller,Some(target))),
            super::agents::Behavior::Fly=>{
                let machine=row.vehicle.clone();let state=machine.state().state;
                if state==2. || state==5. {return Some(false);}
                let transport=machine.unit().and_then(|unit|self.rows.scene().vehicle(unit))
                    .and_then(|unit|unit.class).and_then(|class|class.vehicle_type.clone())
                    .is_some_and(|kind|kind.eq_ignore_ascii_case("transport"));
                Some(!transport&&crate::ai::combat::agent::can_engage_shared(self,shared,controller,Some(target)))
            },
        }
    }
    /// UnitCombatAgent.acceptsBoard checks the eight-metre hostile query
    /// before the inherited vehicle policy, including its short-circuit.
    pub fn board_override(&mut self,agent:Obj,candidate:Obj)->Option<bool> {
        let behavior=self.rows.state.agents.get(&self.rows.state.entities,agent)?.behavior.clone();
        match behavior {
            super::agents::Behavior::Base=>None,
            super::agents::Behavior::Combat(machine)=>Some(machine.accepts_board(self,candidate)),
            _=>Some(false),
        }
    }
    fn board_contested(&mut self,agent:Obj,board:crate::ai::vehicles::agents::BoardAgent)->bool {
        let Some(reference)=board.vehicle else{return true};
        let unit=self.rows.scene().vehicle(reference.obj);
        if !unit.is_some_and(|u|u.alive&&u.generation==reference.generation) {return true;}
        let controller=self.agent_controller(agent);
        !crate::ai::vehicles::service::board_path_clear(self,Some(controller),reference.obj)
    }
    /// UnitCombatAgent's subclass constructor follows helper creation and
    /// initAgent. The tracker already belongs to that newly created agent.
    pub fn create_combat(&mut self,agent:Obj,goal:crate::ai::combat::agent::Goal) {
        let row=self.rows.state.agents.get(&self.rows.state.entities,agent).expect("new native combat agent");
        let controller=row.controller;let tracker=row.tracker().clone();
        let machine=crate::ai::combat::agent::CombatAgent::new(self,agent,controller,tracker,goal);
        assert!(self.rows.state.agents.sync(&self.rows.state.entities,&machine));
    }
    /// A callback can retire this agent and create its successor. Keep the
    /// running object's state reachable until its last native call returns,
    /// without holding a row borrow through the callback or touching its heir.
    pub fn run_combat<R>(&mut self,agent:Obj,combat:&mut crate::ai::combat::binding::CombatAi,
        run:impl FnOnce(&mut Self,&mut crate::ai::combat::agent::CombatAgent,&mut crate::ai::combat::binding::CombatAi)->R)->R {
        let mut machine=self.rows.state.agents.combat(&self.rows.state.entities,agent).expect("current native combat machine");
        assert!(self.rows.state.agents.enter(&self.rows.state.entities,agent));
        let result=run(self,&mut machine,combat);
        assert!(self.rows.state.agents.sync(&self.rows.state.entities,&machine));
        self.rows.state.agents.leave(&mut self.rows.state.entities,agent);
        self.rows.state.reap_helpers();
        result
    }
}

/// vehicles.dispatch shares the caller's weapon policy. Its hull reads use
/// the same generation-qualified F2 owner as the full vehicle frame.
struct VehicleDispatch<'a,'b,'c,'d,C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> {
    frame:&'a mut Frame<'b,'c,C,M,H>,
    shared:&'d mut crate::ai::combat::weapons_ai::Shared,
}
impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> crate::ai::vehicles::patterns::DispatchWorld for VehicleDispatch<'_,'_,'_,'_,C,M,H> {
    fn dispatch_hull(&self,hull:Obj)->crate::ai::vehicles::world::HullView {
        let Some(super::entities::Entity::Hull(reference))=self.frame.rows.state.entities.get(hull) else {panic!("current dispatch hull")};
        self.frame.rows.state.mounts.hull(self.frame.rows.store,self.frame.rows.seats,&self.frame.rows.state.entities,reference).expect("current dispatch hull")
    }
    fn dispatch_choice(&mut self,controller:Obj,unit:Obj,target:Obj,switch_allowed:bool)->crate::ai::vehicles::world::Choice {
        let choice=crate::ai::combat::weapons_ai::select_weapon(self.frame,self.shared,Some(controller),unit,Some(target),switch_allowed);
        crate::ai::vehicles::world::Choice {ok:choice.ok,action:choice.action,in_band:choice.in_band,weapon:choice.index.map(|index|index as u64)}
    }
}

impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> crate::ai::vehicles::service::MountWorld for Frame<'_,'_,C,M,H> {
    fn mount_unit(&self,unit:Obj)->crate::ai::vehicles::world::UnitView {self.rows.scene().vehicle(unit).unwrap_or_else(||crate::ai::vehicles::world::UnitView {obj:unit,..Default::default()})}
    fn mount_lease(&self,lease:Obj)->crate::ai::vehicles::world::LeaseView {
        let Some(super::entities::Entity::Lease(reference))=self.rows.state.entities.get(lease) else {panic!("current native mount lease")};
        self.rows.state.mounts.lease(self.rows.store,&self.rows.state.entities,reference).expect("current native mount lease")
    }
    fn mount_hull(&self,hull:Obj)->crate::ai::vehicles::world::HullView {
        let Some(super::entities::Entity::Hull(reference))=self.rows.state.entities.get(hull) else {panic!("current native mount hull")};
        self.rows.state.mounts.hull(self.rows.store,self.rows.seats,&self.rows.state.entities,reference).expect("current native mount hull")
    }
    fn mount_turret(&self,turret:Obj)->crate::ai::vehicles::world::TurretView {
        let Some(super::entities::Entity::Turret(reference))=self.rows.state.entities.get(turret) else {panic!("current native mount turret")};
        self.rows.state.turrets.view(self.rows.seats,self.rows.buildings.rows(),&self.rows.state.entities,reference).expect("current native mount turret")
    }
    fn mount_seat_count(&self,hull:Obj)->usize {
        let Some(super::entities::Entity::Hull(reference))=self.rows.state.entities.get(hull) else{return 0};
        self.rows.state.mounts.seat_count(self.rows.store,reference).unwrap_or(0)
    }
    fn mount_seat(&self,hull:Obj,index:usize)->Option<crate::ai::vehicles::world::SeatView> {
        let super::entities::Entity::Hull(reference)=self.rows.state.entities.get(hull)? else{return None};
        self.rows.state.mounts.seat(self.rows.store,&self.rows.state.entities,reference,index)
    }
    fn mount_eye(&mut self,origin:crate::ai::vehicles::world::Origin)->Option<Vector3> {self.hooks.mount_origin(self.rows,self.random,origin)}
    fn mount_hold_yaw(&mut self,turret:Obj,yaw:f64)->f64 {
        let Some(super::entities::Entity::Turret(reference))=self.rows.state.entities.get(turret) else {panic!("current native mount turret")};
        self.rows.state.turrets.hold_yaw(self.rows.seats,self.rows.buildings.rows(),reference,yaw).expect("current native mount pivot")
    }
}

impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> crate::ai::vehicles::service::BoardWorld for Frame<'_,'_,C,M,H> {
    fn board_unit(&self,unit:Obj)->crate::ai::vehicles::world::UnitView {self.rows.scene().vehicle(unit).unwrap_or_else(||crate::ai::vehicles::world::UnitView {obj:unit,..Default::default()})}
    fn board_controller_unit(&self,controller:Obj)->Option<Obj> {self.ctl(controller).unit}
    fn board_enemies(&mut self,center:&Vector3,unit:Obj,team:f64)->List<Obj> {
        let mut found=self.query(center,8.,QueryFilter::Hostile {except:unit,team});
        found.retain(|key|self.rows.scene().vehicle(*key).is_some_and(|unit|unit.targetable));found
    }
}

impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> crate::ai::vehicles::service::SharedTargetWorld for Frame<'_,'_,C,M,H> {
    fn shared_unit(&self,controller:Obj)->Option<Obj> {self.ctl(controller).unit}
    fn shared_human(&mut self,hull:Obj)->Option<Obj> {self.hooks.human_target(self.rows,self.random,hull)}
    fn shared_relation(&mut self,a:f64,b:f64)->f64 {self.relation(a,b)}
}

impl<C:Changes+?Sized,M:Mounted,H:Hooks<C,M>> crate::ai::vehicles::service::BoardPolicyWorld for Frame<'_,'_,C,M,H> {
    fn policy_controller_unit(&self,controller:Obj)->Option<Obj> {self.ctl(controller).unit}
    fn policy_stay_in_turrets(&self)->bool {self.hooks.vehicle_binding().settings.stay_in_turrets}
    fn policy_memory_point(&mut self,agent:Obj)->Option<Vector3> {
        let tracker=*self.tracker(agent)?;
        tracker.target(self).map(|_|tracker.point)
    }
}
