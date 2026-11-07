//! Navigation on canonical unit rows. Only helper, route and controller state
//! persists here; movement facts are read from the slab at each observation.
use super::super::control::world::{ControlObs, HelperObs};
use super::super::nav::{
    goto::{FlyGoto, GotoMode, GotoWorld, GroundGoto, Resolved},
    math::Vec3,
    units::{Air, ClassDef, JetView, NavUnit, Posture, SizeType, SoldierView, UnitRef},
    Nav,
};
use super::super::phase::RandomState;
use crate::gen::{AiLifeHandle, SeatLease, SeatUnit, Store, Unit, UnitBody};
use crate::seats::lease::control_key;
use crate::store::{Ref, Slab};
use crate::vehicles::f2_seats::{HullLife, Seats};

/// Native actor identity. The registry generation prevents a retired control
/// from following a different actor when either registry or unit slots recycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Actor(pub u64);
impl Actor {
    fn of(r: Ref<Binding>) -> Self {
        Self((u64::from(r.gen) << 32) | u64::from(r.slot))
    }
    fn reference(self) -> Ref<Binding> {
        Ref::new(self.0 as u32, (self.0 >> 32) as u32)
    }
}
struct Binding {
    target: AiLifeHandle,
    identity: Option<u64>,
    class: usize,
    control: usize,
    seat: Option<Seat>,
    helper:Option<HelperKey>,
    retiring:bool,
    passive:Option<FollowTarget>,
}
#[derive(Clone, Copy)]
struct Seat {
    turret:Option<Ref<crate::gen::Turret>>,
    lease: Ref<SeatLease>,
    unit: Ref<SeatUnit>,
    index: usize,
    kind: f64,
    speed: f64,
    strafe: f64,
    turn: f64,
    pitch: f64,
}
/// Passive targets keep owner handles, never a second copy of their pose.
#[derive(Clone)]
pub enum FollowSource { Hull(Ref<crate::gen::Vehicle>), Building(crate::gen::F4Handle), Trap(crate::gen::F4Handle,Vec3) }
#[derive(Clone)]
pub struct FollowTarget {pub source:FollowSource,pub kind:f64}
#[derive(Clone,Copy)]
pub struct FollowView {pub position:Vec3,pub velocity:Vec3,pub generation:f64,pub alive:bool}
impl Binding {
    fn life(&self) -> f64 {
        if self.seat.is_some() {
            1.
        } else {
            self.target.life
        }
    }
    fn kind(&self, store: &Store) -> Option<f64> {
        if self.retiring {return None;}
        if let Some(target)=&self.passive {return Some(target.kind);}
        if let Some(seat) = self.seat {
            if seat.turret.is_some() {valid_turret_lease(store,&self.target,seat)?;}else{valid_seat(store, &self.target, seat)?;}
            Some(seat.kind)
        } else {
            Some(self.target.unit(store)?.kind)
        }
    }
}
enum Helper {
    Ground(GroundGoto),
    Fly(FlyGoto),
    Stationary(crate::ai::vehicles::service::StationaryHelper),
}
impl Helper {
    fn new(kind:f64,control:usize)->Self {
        if kind==7. {Self::Stationary(Default::default())}
        else if kind==5. {Self::Fly(FlyGoto::new(control))}
        else {Self::Ground(GroundGoto::new(control))}
    }
}
/// A helper is an object distinct from its controller and current agent.
/// Retained agent stacks and jet jobs pin this qualified identity across a
/// replacement; each still writes the controller's shared movement request.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct HelperKey(Ref<HelperRow>);
struct HelperRow {actor:Actor,value:Helper,owners:usize}
const NO_CONTROL:usize=usize::MAX;

/// Queries outside navigation are native services supplied by the world. The
/// clear-above query occurs only at the original jet-attempt observation point.
pub trait Environment {
    fn time(&self) -> f64;
    fn building_target(&mut self,_target:&crate::gen::F4Handle)->Option<FollowView> {None}
    fn trap_target(&mut self,_target:&crate::gen::F4Handle,_position:Vec3)->Option<FollowView> {None}
    /// Unit.origin/forward switches to its lease immediately, even before the
    /// next AI sync replaces the infantry controller with a seat controller.
    fn unit_frame(&mut self,store:&mut Store,seats:&mut Seats,unit:Ref<Unit>)->Option<(Vec3,Vec3)> {
        let body=store.unit.get(unit)?;
        let source=body.seat.and_then(|r|store.seat_lease.get(r));
        if let Some(lease)=source {
            if let Some(hull)=lease.vehicle {
                let life=store.vehicle.get(hull)?.target_generation;
                let index=lease.index as usize;
                return self.seat_frame(store,seats,HullLife {vehicle:hull,life},index);
            }
            if let Some(turret)=lease.turret {
                let side=seats.crews.turrets.iter_mut().find(|side|side.turret==turret)?;
                let p=side.origin();let d=side.view_direction(side.yaw,side.pitch);
                return Some((Vec3::new(p.x,p.y,p.z),Vec3::new(d.x,d.y,d.z)));
            }
        }
        let p=body.origin_on_foot();let d=body.forward_on_foot();
        Some((Vec3::new(p.x,p.y,p.z),Vec3::new(d.x,d.y,d.z)))
    }
    fn remembered(&mut self, target: Actor) -> Option<Vec3>;
    fn clear_above(&mut self,store:&mut Store,seats:&mut Seats,from:Vec3,unit:&AiLifeHandle)->bool;
    fn follow_far(&mut self) -> f64;
    /// SeatLease.origin/forward on the current simulation rig.
    fn seat_frame(&mut self,store:&mut Store,seats:&mut Seats,hull: HullLife,index:usize) -> Option<(Vec3, Vec3)>;
}

pub enum Command<'a> {
    Goto(Vec3, GotoMode),
    Attach,
    Update(f64),
    FlightPath(&'a [[f64;3]],bool,bool),
    Tick(f64),
    Stop,
    Follow(UnitRef, f64, bool),
    Install(&'a [Vec3]),
    Hold(bool),
    Trail(bool),
    Leader(Option<UnitRef>),
    Stance(f64),
    Jet(Vec3),
    Aim(Vec3),
    AimTarget(Option<UnitRef>),
    AimPath,
}

pub struct Navigation {
    pub nav: Nav,
    classes: Vec<ClassDef>,
    actors: Slab<Binding>,
    helpers: Slab<HelperRow>,
    free_controls:Vec<usize>,
    dirty: Vec<AiLifeHandle>,
    input_body_changed: bool,
}
impl Navigation {
    pub fn new(mut nav: Nav, capacity: usize) -> Self {
        nav.publications = None;
        nav.prepare_owned();
        nav.controls.reserve(capacity);
        nav.store.reserve_paths(capacity.saturating_mul(3).saturating_add(8));
        Self {
            nav,
            classes: Vec::new(),
            actors: Slab::with_capacity(capacity),
            helpers: Slab::with_capacity(capacity.saturating_mul(2).saturating_add(8)),
            free_controls:Vec::with_capacity(capacity),
            dirty: Vec::with_capacity(capacity),
            input_body_changed: false,
        }
    }
    /// Resolve textual size names once at class load. Every projected class
    /// thereafter consists solely of numeric fields and never allocates.
    pub fn class(&mut self, def: ClassDef) -> usize {
        let mut probe = NavUnit::new(0, 0., Vec3::default());
        probe.class_def = Some(def);
        let mask = super::super::nav::units::traversal_mask(&probe);
        let mut def = probe.class_def.take().unwrap();
        def.ai_size_type = SizeType::Number(mask as f64);
        self.classes.push(def);
        self.classes.len() - 1
    }
    pub fn register(&mut self, store: &Store, target: AiLifeHandle, class: usize) -> Option<Actor> {
        let unit = target.unit(store)?;
        self.classes.get(class)?;
        let control = self.acquire_control();
        let helper = Helper::new(unit.kind,control);
        let r = self.actors.insert(Binding {
            target,
            identity: None,
            class,
            control,
            seat: None,helper:None,retiring:false,passive:None,
        });
        let key=HelperKey(self.helpers.insert(HelperRow {actor:Actor::of(r),value:helper,owners:1}));
        self.actors.get_mut(r).unwrap().helper=Some(key);
        Some(Actor::of(r))
    }
    pub fn register_body(&mut self,store:&Store,target:AiLifeHandle,class:usize,identity:u64)->Option<Actor> {
        let actor=self.register(store,target,class)?;
        self.actors.get_mut(actor.reference()).unwrap().identity=Some(identity);Some(actor)
    }
    /// Humans and other followed bodies have a navigation identity without
    /// owning a bot's movement helper or consuming a control slot.
    pub fn register_follow_target(&mut self,target:FollowTarget,class:usize,generation:f64)->Actor {
        let r=self.actors.insert(Binding {target:AiLifeHandle {life:generation,..Default::default()},identity:None,class,control:NO_CONTROL,seat:None,helper:None,retiring:false,passive:Some(target)});
        Actor::of(r)
    }
    pub fn register_target(&mut self,store:&Store,target:AiLifeHandle,class:usize)->Option<Actor> {
        target.unit(store)?;self.classes.get(class)?;
        let r=self.actors.insert(Binding {target,identity:None,class,control:NO_CONTROL,seat:None,helper:None,retiring:false,passive:None});
        Some(Actor::of(r))
    }
    pub fn activate(&mut self,store:&Store,actor:Actor)->bool {
        let Some(binding)=self.actors.get(actor.reference()) else {return false};
        if binding.control!=NO_CONTROL {return true;}
        let Some(kind)=binding.kind(store) else {return false};
        let control=self.acquire_control();self.actors.get_mut(actor.reference()).unwrap().control=control;
        self.replace_helper(actor,kind,control);
        true
    }
    pub fn register_seat(
        &mut self,
        store: &Store,
        target: AiLifeHandle,
        lease: Ref<SeatLease>,
        unit: Ref<SeatUnit>,
        kind: f64,
        class: usize,
        view: &crate::ai::vehicles::world::ClassView,
    ) -> Option<Actor> {
        self.seat_actor(store,target,lease,unit,kind,class,view,true)
    }
    pub fn register_seat_target(&mut self,store:&Store,target:AiLifeHandle,lease:Ref<SeatLease>,unit:Ref<SeatUnit>,kind:f64,class:usize,view:&crate::ai::vehicles::world::ClassView)->Option<Actor> {
        self.seat_actor(store,target,lease,unit,kind,class,view,false)
    }
    fn seat_actor(&mut self,store:&Store,target:AiLifeHandle,lease:Ref<SeatLease>,unit:Ref<SeatUnit>,kind:f64,class:usize,view:&crate::ai::vehicles::world::ClassView,active:bool)->Option<Actor> {
        self.classes.get(class)?;
        let l = store.seat_lease.get(lease)?;
        if l.index < 0. || l.index.fract() != 0. {
            return None;
        }
        let pilot = l.vehicle.is_some()&&l.index == 0.;
        let seat = Seat {
            turret:l.turret,
            lease,
            unit,
            index: l.index as usize,
            kind,
            speed: if pilot {
                view.forward_speed.or(view.max_speed).unwrap_or(0.)
            } else {
                0.
            },
            strafe: if pilot {
                view.strafe_speed.unwrap_or(0.)
            } else {
                0.
            },
            turn: view.turn_rate.unwrap_or(2.),
            pitch: view.pitch_rate.unwrap_or(1.),
        };
        if seat.turret.is_some() {valid_turret_lease(store,&target,seat)?;}else{valid_seat(store, &target, seat)?;}
        let control = if active {self.acquire_control()}else{NO_CONTROL};
        let r = self.actors.insert(Binding {
            target,
            identity: None,
            class,
            control,
            seat: Some(seat),helper:None,retiring:false,passive:None,
        });
        if active {self.replace_helper(Actor::of(r),kind,control);}
        Some(Actor::of(r))
    }
    /// A pooled life keeps its actor identity but invalidates old follow refs.
    /// A different slab generation requires retirement and fresh registration.
    pub fn refresh(
        &mut self,
        store: &Store,
        actor: Actor,
        target: AiLifeHandle,
        class: usize,
    ) -> bool {
        if target.unit(store).is_none() || self.classes.get(class).is_none() {
            return false;
        }
        let Some(binding) = self.actors.get_mut(actor.reference()) else {
            return false;
        };
        if binding.seat.is_some() {
            return false;
        }
        let old = &binding.target;
        if old.kind != target.kind || old.slot != target.slot || old.generation != target.generation
        {
            return false;
        }
        let kind=target.unit(store).unwrap().kind;
        let renew=old.life!=target.life&&binding.control!=NO_CONTROL;
        let control=binding.control;
        binding.target=target;binding.class=class;
        if renew {self.nav.control_reset(control);self.replace_helper(actor,kind,control);}
        true
    }
    /// NavControl.attach follows its controller across body changes. Passive
    /// follow targets keep a separate actor, so rebinding one controller can
    /// never move another bot's saved reference to a replacement body.
    pub fn attach_body(&mut self,store:&Store,actor:Actor,target:AiLifeHandle,class:usize,identity:u64)->bool {
        if target.unit(store).is_none()||self.classes.get(class).is_none() {return false;}
        let Some(binding)=self.actors.get_mut(actor.reference()) else {return false};
        if binding.retiring||binding.control==NO_CONTROL {return false;}
        binding.target=target;binding.identity=Some(identity);binding.class=class;binding.seat=None;true
    }
    /// NavControl.attach follows the same controller onto its new SeatUnit.
    /// The helper class still belongs to its agent; only a later constructor
    /// renews it, preserving an old running helper through a boarding hook.
    pub fn attach_seat(&mut self,store:&Store,actor:Actor,target:AiLifeHandle,lease:Ref<SeatLease>,unit:Ref<SeatUnit>,kind:f64,class:usize,view:&crate::ai::vehicles::world::ClassView,identity:u64)->bool {
        if self.classes.get(class).is_none() {return false;}
        let Some(value)=store.seat_lease.get(lease) else{return false};
        if value.index<0.||value.index.fract()!=0. {return false;}
        let pilot=value.vehicle.is_some()&&value.index==0.;
        let seat=Seat {turret:value.turret,lease,unit,index:value.index as usize,kind,
            speed:if pilot {view.forward_speed.or(view.max_speed).unwrap_or(0.)}else{0.},
            strafe:if pilot {view.strafe_speed.unwrap_or(0.)}else{0.},turn:view.turn_rate.unwrap_or(2.),pitch:view.pitch_rate.unwrap_or(1.)};
        if seat.turret.is_some() {if valid_turret_lease(store,&target,seat).is_none() {return false;}}else if valid_seat(store,&target,seat).is_none() {return false;}
        let Some(binding)=self.actors.get_mut(actor.reference()) else{return false};
        if binding.retiring||binding.control==NO_CONTROL {return false;}
        binding.target=target;binding.identity=Some(identity);binding.class=class;binding.seat=Some(seat);true
    }
    pub fn retire(&mut self, actor: Actor) -> bool {
        let Some(binding)=self.actors.get_mut(actor.reference()) else{return false};
        if binding.retiring {return false;}
        binding.retiring=true;
        let helper=binding.helper.take();
        if binding.control!=NO_CONTROL {self.nav.control_dispose(binding.control);}
        if let Some(helper)=helper {self.release_helper(helper);}else{self.reap_actor(actor);}
        true
    }
    fn reap_actor(&mut self,actor:Actor) {
        let Some(binding)=self.actors.get(actor.reference()) else{return};
        if !binding.retiring||self.helpers.iter().any(|(_,h)|h.actor==actor) {return;}
        let binding=self.actors.remove(actor.reference()).unwrap();
        if binding.control!=NO_CONTROL {self.free_controls.push(binding.control);}
    }
    pub fn helper_actor(&self,key:HelperKey)->Option<Actor> {Some(self.helpers.get(key.0)?.actor)}
    pub fn helper_key(&self,actor:Actor)->Option<HelperKey> {self.actors.get(actor.reference())?.helper}
    pub fn retain_helper(&mut self,key:HelperKey)->bool {
        let Some(row)=self.helpers.get_mut(key.0) else{return false};row.owners+=1;true
    }
    pub fn release_helper(&mut self,key:HelperKey) {
        let Some(row)=self.helpers.get_mut(key.0) else{return};
        assert!(row.owners>0,"native helper ownership balance");row.owners-=1;
        if row.owners==0 {let actor=self.helpers.remove(key.0).unwrap().actor;self.reap_actor(actor);}
    }
    fn replace_helper(&mut self,actor:Actor,kind:f64,control:usize) {
        if let Some(old)=self.actors.get_mut(actor.reference()).unwrap().helper.take() {self.release_helper(old);}
        let key=HelperKey(self.helpers.insert(HelperRow {actor,value:Helper::new(kind,control),owners:1}));
        self.actors.get_mut(actor.reference()).unwrap().helper=Some(key);
    }
    fn acquire_control(&mut self)->usize {
        if let Some(index)=self.free_controls.pop() {
            // Retirement cancelled the planner and released its paths. Keep
            // the serial monotonic so an old answer cannot latch a new actor.
            let serial=self.nav.controls[index].serial;
            self.nav.controls[index]=super::super::nav::control::NavControl::new();
            self.nav.controls[index].serial=serial;
            index
        }else{self.nav.new_control()}
    }
    pub fn follow_ref(&self, actor: Actor) -> Option<UnitRef> {
        let binding = self.actors.get(actor.reference())?;
        if binding.retiring {return None;}
        Some(UnitRef {
            unit: actor.0,
            generation: Some(binding.life()),
        })
    }
    pub fn resolve<'a>(&self, store: &'a Store, reference: UnitRef) -> Option<&'a Unit> {
        resolve(store, &self.actors, reference)
    }
    pub fn control(&self, actor: Actor) -> Option<&super::super::nav::control::NavControl> {
        let binding = self.actors.get(actor.reference())?;
        self.nav.controls.get(binding.control)
    }
    pub fn control_mut(
        &mut self,
        actor: Actor,
    ) -> Option<&mut super::super::nav::control::NavControl> {
        let binding = self.actors.get(actor.reference())?;
        self.nav.controls.get_mut(binding.control)
    }
    pub fn observation(&self, store: &Store, actor: Actor) -> Option<ControlObs> {
        let binding = self.actors.get(actor.reference())?;
        let kind = binding.kind(store)?;
        self.observe_control(actor,Some(kind))
    }
    /// UnitController.unbind resets its retained control after the target has
    /// died or respawned. Reading that owned control does not resolve an old
    /// weak target or authorize geometry/movement on the replacement life.
    pub fn retained_observation(&self,store:&Store,actor:Actor)->Option<ControlObs> {
        let binding=self.actors.get(actor.reference())?;
        let kind=if let Some(seat)=binding.seat {Some(seat.kind)}else{
            store.unit.get(Ref::new(binding.target.slot,binding.target.generation)).map(|u|u.kind)
        };
        self.observe_control(actor,kind)
    }
    fn observe_control(&self,actor:Actor,kind:Option<f64>)->Option<ControlObs> {
        let binding=self.actors.get(actor.reference())?;
        let c = self.nav.controls.get(binding.control)?;
        let view = kind.map(|kind|NavUnit::new(actor.0, kind, Vec3::default()));
        let status = self.nav.control_status(binding.control, view.as_ref());
        let r = c.request;
        Some(ControlObs {
            status,
            has_executor: c.executor.is_some(),
            avoid_side: c.avoid_side,
            stance: c.stance,
            jet_pending: c.jet.pending,
            jet_vector: c.jet.vector,
            request: super::super::control::controller::MoveRequest {
                valid: r.valid,
                steering_target: r.steering_target,
                waypoint: r.waypoint,
                segment_direction: r.segment_direction,
                mode: r.mode,
                lower: r.lower,
                upper: r.upper,
            },
        })
    }
    /// A new agent receives a new helper over the controller's existing
    /// navigation control. Control.reset belongs to bind/exit, not this ctor.
    pub fn reset(&mut self,actor:Actor)->bool {
        let Some(binding)=self.actors.get(actor.reference()) else{return false};
        if binding.control==NO_CONTROL {return false;}
        self.nav.control_reset(binding.control);true
    }
    pub fn renew_helper(&mut self,store:&Store,actor:Actor)->Option<HelperObs> {
        let binding=self.actors.get(actor.reference())?;let kind=binding.kind(store)?;
        if binding.control==NO_CONTROL {return None;}
        self.replace_helper(actor,kind,binding.control);
        self.helper_observation(store,actor)
    }
    pub fn helper_observation(&self,store:&Store,actor:Actor)->Option<HelperObs> {
        self.actors.get(actor.reference())?.kind(store)?;
        self.helper_observation_for(store,self.helper_key(actor)?)
    }
    pub fn helper_observation_for(&self,store:&Store,key:HelperKey)->Option<HelperObs> {
        let row=self.helpers.get(key.0)?;let actor=row.actor;
        let binding=self.actors.get(actor.reference())?;
        let helper=&row.value;
        if let Helper::Stationary(h)=helper {return Some(HelperObs {arrived:h.arrived,failed:h.failed,status:h.status,has_path:h.has_path});}
        let (arrived,failed)=match helper {
            Helper::Ground(h)=>(self.nav.ground_arrived(h),h.failed),
            Helper::Fly(h)=>(self.nav.fly_arrived(h),h.failed),
            Helper::Stationary(_)=>unreachable!(),
        };
        Some(HelperObs {arrived,failed,status:self.retained_observation(store,actor)?.status,has_path:self.nav.controls[binding.control].executor.is_some()})
    }
    /// Only ground/fly helpers expose state and destination properties.
    /// StationaryHelper has destinationPoint(), but neither property.
    /// `typeof helper.follow` observes the helper class, even without a path.
    pub fn helper_has_follow(&self,actor:Actor)->Option<bool> {
        self.actors.get(actor.reference())?;
        self.helper_has_follow_for(self.helper_key(actor)?)
    }
    pub fn helper_has_follow_for(&self,key:HelperKey)->Option<bool> {
        Some(matches!(&self.helpers.get(key.0)?.value,Helper::Ground(_)|Helper::Stationary(_)))
    }
    pub fn helper_detail(&self,store:&Store,actor:Actor)->Option<(Option<f64>,Option<Vec3>)> {
        self.actors.get(actor.reference())?.kind(store)?;
        Some(match &self.helpers.get(self.helper_key(actor)?.0)?.value {
            Helper::Ground(h)=>(Some(h.state),Some(h.destination)),
            Helper::Fly(h)=>(Some(h.state),Some(h.destination)),
            Helper::Stationary(_)=>(None,None),
        })
    }
    pub fn destination(&self,store:&Store,actor:Actor)->Option<Vec3> {
        self.actors.get(actor.reference())?.kind(store)?;
        self.destination_for(store,self.helper_key(actor)?)
    }
    pub fn destination_for(&self,store:&Store,key:HelperKey)->Option<Vec3> {
        let row=self.helpers.get(key.0)?;
        Some(match &row.value {Helper::Ground(h)=>h.destination,Helper::Fly(h)=>h.destination,Helper::Stationary(_)=>self.position(store,row.actor)?})
    }
    pub fn position(&self,store:&Store,actor:Actor)->Option<Vec3> {
        let binding=self.actors.get(actor.reference())?;
        if let Some(seat)=binding.seat {
            let h=valid_seat(store,&binding.target,seat)?;
            Some(Vec3::new(h.position[0],h.position[1],h.position[2]))
        }else{
            let p=binding.target.unit(store)?.position();Some(Vec3::new(p.x,p.y,p.z))
        }
    }
    pub fn command(
        &mut self,
        store: &mut Store,
        seats: &mut Seats,
        random: &mut RandomState,
        env: &mut dyn Environment,
        actor: Actor,
        command: Command<'_>,
    ) -> Option<HelperObs> {
        let key=self.helper_key(actor)?;
        self.command_for(store,seats,random,env,key,command)
    }
    pub fn command_for(&mut self,store:&mut Store,seats:&mut Seats,random:&mut RandomState,env:&mut dyn Environment,key:HelperKey,command:Command<'_>)->Option<HelperObs> {
        let actor=self.helpers.get(key.0)?.actor;
        let binding = self.actors.get(actor.reference())?;
        if binding.control==NO_CONTROL {return None;}
        if binding.retiring {
            // Ground/Fly.stop still drops the shared executor after unbind;
            // their goto/tick methods return when controller.unit is null.
            // Never project the detached body or reuse a new controller's row.
            let control=binding.control;
            match command {
                Command::Stop=>{
                    let helper=&mut self.helpers.get_mut(key.0)?.value;
                    match helper {
                        Helper::Ground(h)=>{h.cursor=None;h.pending=0.;h.state=7.;self.nav.control_strand(control,false,None);self.nav.control_stop(control,None);},
                        Helper::Fly(h)=>{h.pending=0.;h.state=5.;self.nav.control_stop(control,None);},
                        Helper::Stationary(_)=>self.nav.controls[control].request.valid=false,
                    }
                },
                Command::Leader(leader)=>self.nav.control_set_leader(control,leader),
                Command::Hold(on)=>{if let Some(executor)=self.nav.controls[control].executor.as_mut() {executor.hold=on;}},
                _=>{},
            }
            return self.helper_observation_for(store,key);
        }
        let kind=binding.kind(store)?;
        if matches!(command,Command::Attach) {
            // control.js attach reads only object identity and generation.
            // In particular it must not pose a mounted gun before steering.
            let mut unit=NavUnit::new(actor.0,kind,Vec3::default());
            unit.generation=Some(binding.life());unit.control_identity=binding.identity;
            self.nav.control_attach(binding.control,Some(&unit));return self.helper_observation_for(store,key);
        }
        if matches!(command,Command::Update(_)) {
            let control=&self.nav.controls[binding.control];
            // With no executor, trail or stranded state, control.update has
            // no unit observation or elapsed clock to advance.
            if control.executor.is_none()&&control.leader.is_none()&&!control.stranded&&control.stranded_for==0. {return self.helper_observation_for(store,key);}
        }
        if matches!(&self.helpers.get(key.0)?.value,Helper::Stationary(_)) {
            // StationaryHelper.goto/stop invalidate only the request. Follow,
            // tick, stance and aim are present no-ops, without rig queries.
            let control=binding.control;
            match command {
                Command::Goto(..)|Command::Stop=>self.nav.controls[control].request.valid=false,
                Command::Leader(r)=>self.nav.control_set_leader(control,r),
                _=>{},
            }
            return self.helper_observation_for(store,key);
        }
        let mut unit = project(store,seats, &self.classes, &self.actors, actor, env)?;
        let control = binding.control;
        // VehicleAI runs attach/update separately from seat steering. These
        // operations share the controller's control, without latching input.
        match command {
            Command::Attach=>{self.nav.control_attach(control,Some(&unit));return self.helper_observation_for(store,key);},
            Command::Update(dt)=>{
                let mut world=World {store,seats,classes:&self.classes,actors:&self.actors,random,env,jet:false};
                self.nav.control_update(control,dt,&mut unit,&mut world);
                if self.actors.get(actor.reference())?.seat.is_none()&&write_avoidance(world.store,&binding.target,&unit) {mark(&mut self.dirty,&binding.target);}
                return self.helper_observation_for(world.store,key);
            },
            Command::FlightPath(points,over,altitude)=>{
                if points.is_empty() {return self.helper_observation_for(store,key);}
                let Helper::Fly(helper)=&mut self.helpers.get_mut(key.0)?.value else{return None};
                self.nav.control_attach(control,Some(&unit));self.nav.planner_cancel(control);
                let path=self.nav.path_new();
                for &[x,y,z] in points.iter().rev() {if !self.nav.store.prepend(path,x,y,z) {self.nav.store.release(path);return self.helper_observation_for(store,key);}}
                let last=points.last().unwrap();helper.destination=Vec3::new(last[0],last[1],last[2]);
                self.nav.control_install(control,path,over,altitude,&mut unit);
                helper.failed=false;helper.state=4.;helper.pending=0.;
                return self.helper_observation_for(store,key);
            },
            _=>{},
        }
        let helper = &mut self.helpers.get_mut(key.0)?.value;
        let mut world = World {
            store,seats,
            classes: &self.classes,
            actors: &self.actors,
            random,
            env,
            jet: true,
        };
        match helper {
            Helper::Ground(h) => match command {
                Command::Attach|Command::Update(_)|Command::FlightPath(..)=>unreachable!(),
                Command::Goto(p, m) => self.nav.ground_goto(h, &mut unit, &mut world, p, m),
                Command::Tick(dt) => self.nav.ground_tick(h, &mut unit, &mut world, dt),
                Command::Stop => self.nav.ground_stop(h, &mut unit, &mut world),
                Command::Follow(r, d, m) => {
                    self.nav
                        .ground_follow(h, &mut unit, &mut world, Some(r), d, m)
                }
                Command::Install(points) => self.nav.ground_install_path(h, &mut unit, points),
                Command::Hold(on) => self.nav.ground_hold_path(h, on),
                Command::Trail(on) => self.nav.control_record_trail(control, on, &mut unit),
                Command::Leader(r) => self.nav.control_set_leader(control, r),
                Command::Stance(v) => self.nav.ground_stance(h, &mut unit, &mut world, v),
                Command::Jet(v) => self.nav.ground_jet(h, &mut unit, &mut world, v),
                Command::Aim(v) => self.nav.ground_aim_at(h, &mut unit, &mut world, v),
                Command::AimTarget(r) => self.nav.ground_aim_at_target(h, &mut unit, &mut world, r),
                Command::AimPath => self.nav.ground_aim_along_path(h, &mut unit, &mut world),
            },
            Helper::Stationary(_)=>unreachable!(),
            Helper::Fly(h) => match command {
                Command::Goto(p, m) => self.nav.fly_goto(h, &mut unit, p, m),
                Command::Tick(_) => self.nav.fly_tick(h, &mut unit),
                Command::Stop => self.nav.fly_stop(h, &mut unit),
                Command::Hold(on) => {
                    if let Some(e) = self.nav.controls[control].executor.as_mut() {
                        e.hold = on;
                    }
                }
                Command::Leader(r) => self.nav.control_set_leader(control, r),
                Command::Trail(on) => self.nav.control_record_trail(control, on, &mut unit),
                // Flyer formation follows its vehicle helper rather than ground goto.
                _ => return None,
            },
        }
        let target = &self.actors.get(actor.reference())?.target;
        if self.actors.get(actor.reference())?.seat.is_none()
            && write_avoidance(store, target, &unit)
        {
            mark(&mut self.dirty, target);
        }
        self.helper_observation_for(store,key)
    }
    /// Called within the AI input phase. The generated intent row is written
    /// directly, preserving unrelated weapon edges and held buttons.
    pub fn input(
        &mut self,store:&mut Store,seats:&mut Seats,random:&mut RandomState,
        env:&mut dyn Environment,actor:Actor,dt:f64,aim:Option<Vec3>,facing:bool,
    )->Option<bool> {
        self.input_body_changed=false;
        let binding = self.actors.get(actor.reference())?;
        if binding.control==NO_CONTROL {return None;}
        let mut unit = project(store,seats, &self.classes, &self.actors, actor, env)?;
        input_frame(store,seats,binding,env,&mut unit)?;
        let mut world = World {
            store,seats,
            classes: &self.classes,
            actors: &self.actors,
            random,
            env,
            jet: false,
        };
        let intent =
            self.nav
                .navigation_input(binding.control, &mut unit, dt, aim, facing, &mut world);
        let changed = if let Some(seat) = binding.seat {
            let row = seats.hulls.controls.get_mut(&control_key(seat.unit))?;
            write_seat_intent(&mut row.intent, intent)
        } else {
            let changed = write_avoidance(store, &binding.target, &unit);
            self.input_body_changed=changed;
            let row = store
                .unit
                .get_mut(Ref::new(binding.target.slot, binding.target.generation))?;
            let changed = write_intent(&mut row.intent, intent) || changed;
            if changed {
                mark(&mut self.dirty, &binding.target);
            }
            changed
        };
        Some(changed)
    }
    /// navigationInput also owns avoidance. Its actual body edit needs the
    /// ordinary publication; unchanged avoidance leaves only intent to emit.
    pub fn input_body_changed(&self)->bool {self.input_body_changed}
    /// nav/analogs.js plain conversion does not step a control, consume a
    /// jet or toggle posture. The scheduler owns those edges in this path.
    pub fn analogs(&mut self,store:&mut Store,seats:&mut Seats,env:&mut dyn Environment,actor:Actor,aim:Option<Vec3>,dt:f64,facing:bool)->Option<[f64;4]> {
        let binding=self.actors.get(actor.reference())?;
        let mut unit=project(store,seats,&self.classes,&self.actors,actor,env)?;
        input_frame(store,seats,binding,env,&mut unit)?;
        let control=(binding.control!=NO_CONTROL).then(||&self.nav.controls[binding.control]);
        let intent=crate::ai::nav::analogs::analogs(&unit,control.map(|control|&control.request),aim,dt,facing,control.and_then(|control|control.executor.as_ref().map(|executor|executor.hold)));
        Some([intent.forward,intent.right,intent.turn,intent.pitch])
    }
    pub fn dirty(&self) -> &[AiLifeHandle] {
        &self.dirty
    }
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }
}
fn mark(dirty: &mut Vec<AiLifeHandle>, target: &AiLifeHandle) {
    if !dirty.iter().any(|h| {
        h.kind == target.kind
            && h.slot == target.slot
            && h.generation == target.generation
            && h.life == target.life
    }) {
        dirty.push(AiLifeHandle {
            kind: target.kind,
            slot: target.slot,
            generation: target.generation,
            life: target.life,
        });
    }
}
fn project(
    store: &mut Store,
    seats: &mut Seats,
    classes: &[ClassDef],
    actors: &Slab<Binding>,
    actor: Actor,
    env: &mut dyn Environment,
) -> Option<NavUnit> {
    let binding = actors.get(actor.reference())?;
    if binding.retiring {return None;}
    let class = classes.get(binding.class)?;
    if let Some(target)=&binding.passive {
        let view=match &target.source {
            FollowSource::Hull(reference)=>{let hull=store.vehicle.get(*reference)?;FollowView {position:Vec3::new(hull.position[0],hull.position[1],hull.position[2]),velocity:Vec3::new(hull.velocity[0],hull.velocity[1],hull.velocity[2]),generation:hull.target_generation,alive:hull.alive}},
            FollowSource::Building(handle)=>env.building_target(handle)?,
            FollowSource::Trap(handle,position)=>env.trap_target(handle,*position)?,
        };
        let mut out=NavUnit::new(actor.0,target.kind,view.position);out.generation=Some(view.generation);out.velocity=view.velocity;out.alive_false=!view.alive;out.class_def=Some(class.clone());return Some(out);
    }
    if let Some(seat) = binding.seat {
        return project_seat(store,seats, binding, class, actor, seat, env);
    }
    let unit = binding.target.unit(store)?;
    let p = unit.position();
    let v = unit.velocity();
    let mut out = NavUnit::new(actor.0, unit.kind, Vec3::new(p.x, p.y, p.z));
    out.velocity = Vec3::new(v.x, v.y, v.z);
    out.generation = Some(unit.generation);
    out.control_identity = binding.identity;
    out.alive_false = !unit.alive;
    out.nav_radius = unit.nav_radius;
    // All fields are numeric after class registration, including the mask.
    let SizeType::Number(mask) = class.ai_size_type else {
        unreachable!("resolved navigation class")
    };
    out.class_def = Some(ClassDef {
        ai_size_type: SizeType::Number(mask),
        bounds: class.bounds,
        max_speed: class.max_speed,
        max_strafe_speed: class.max_strafe_speed,
        max_turn_speed: class.max_turn_speed,
        max_pitch_speed: class.max_pitch_speed,
        forward_speed: class.forward_speed,
        turn_rate: class.turn_rate,
        jet_fuel_min_border: class.jet_fuel_min_border,
        jet_fuel_recharge_rate: class.jet_fuel_recharge_rate,
    });
    out.soldier = Some(match &unit.soldier {
        UnitBody::Soldier(s) => SoldierView {
            aim_yaw: Some(s.aim_yaw),
            aim_pitch: Some(s.aim_pitch),
            max_speed: Some(s.max_speed),
            max_strafe: Some(s.max_strafe),
            max_turn: Some(s.max_turn),
            max_pitch: Some(s.max_pitch),
            avoidance: Some(Vec3::new(s.avoidance[0], s.avoidance[1], s.avoidance[2])),
            fuel: s.fuel,
            jet: Some(JetView {
                min_border: Some(s.jet.min_border),
                recharge: Some(s.jet.recharge),
            }),
            air: match s.air {
                None => Air::None,
                Some(crate::gen::Air::Jump) => Air::Jump,
                Some(crate::gen::Air::Jet) => Air::Jet,
                Some(_) => Air::Other,
            },
            posture: match s.posture {
                crate::gen::Posture::Stand => Posture::Stand,
                crate::gen::Posture::Crouch => Posture::Crouch,
                crate::gen::Posture::Prone => Posture::Prone,
            },
            lock: s.lock,
            dive: crate::units::unit::truthy(s.dive),
        },
        UnitBody::Droideka(d) => SoldierView {
            aim_yaw: Some(d.aim_yaw),
            aim_pitch: Some(d.aim_pitch),
            max_turn: Some(d.max_turn),
            jet: Some(JetView {
                min_border: Some(0.),
                recharge: Some(0.),
            }),
            avoidance: Some(Vec3::new(d.avoidance[0], d.avoidance[1], d.avoidance[2])),
            posture: if d.mode >= 11. {
                Posture::Crouch
            } else {
                Posture::Stand
            },
            ..SoldierView::default()
        },
    });
    let p=unit.forward_on_foot();out.forward=Some(Vec3::new(p.x,p.y,p.z));
    let p=unit.origin_on_foot();out.origin=Some(Vec3::new(p.x,p.y,p.z));
    Some(out)
}
// Navigation only observes a newly mounted infantry body's lease frame at
// input conversion. A follow-position query must not warm its rig early.
fn input_frame(store:&mut Store,seats:&mut Seats,binding:&Binding,env:&mut dyn Environment,unit:&mut NavUnit)->Option<()> {
    if binding.seat.is_none()&&binding.passive.is_none() {
        let reference=Ref::new(binding.target.slot,binding.target.generation);
        if store.unit.get(reference)?.seat.is_some() {
            let (origin,forward)=env.unit_frame(store,seats,reference)?;
            unit.origin=Some(origin);unit.forward=Some(forward);
        }
    }
    Some(())
}
fn write_avoidance(store: &mut Store, target: &AiLifeHandle, view: &NavUnit) -> bool {
    let Some(value) = view.soldier.and_then(|s| s.avoidance) else {
        return false;
    };
    let row = store
        .unit
        .get_mut(Ref::new(target.slot, target.generation))
        .unwrap();
    let out = match &mut row.soldier {
        UnitBody::Soldier(s) => &mut s.avoidance,
        UnitBody::Droideka(d) => &mut d.avoidance,
    };
    let value = [value.x, value.y, value.z];
    let changed = (0..3).any(|i| out[i].to_bits() != value[i].to_bits());
    if changed {
        *out = value;
    }
    changed
}
fn write_intent(out: &mut crate::gen::Intent, value: super::super::nav::analogs::Intent) -> bool {
    let old = (
        out.forward.to_bits(),
        out.right.to_bits(),
        out.turn.to_bits(),
        out.pitch.to_bits(),
        out.jump,
        out.crouch,
        out.prone,
        out.jet_direction.map(|p| p.map(f64::to_bits)),
    );
    out.forward = value.forward;
    out.right = value.right;
    out.turn = value.turn;
    out.pitch = value.pitch;
    out.jump |= value.jump;
    out.crouch |= value.crouch;
    out.prone |= value.prone;
    if let Some(p) = value.jet_direction {
        out.jet_direction = p.map(|p| [p.x, p.y, p.z]);
    }
    old != (
        out.forward.to_bits(),
        out.right.to_bits(),
        out.turn.to_bits(),
        out.pitch.to_bits(),
        out.jump,
        out.crouch,
        out.prone,
        out.jet_direction.map(|p| p.map(f64::to_bits)),
    )
}
struct World<'a> {
    store: &'a mut Store,
    seats: &'a mut Seats,
    classes: &'a [ClassDef],
    actors: &'a Slab<Binding>,
    random: &'a mut RandomState,
    env: &'a mut dyn Environment,
    jet: bool,
}
impl GotoWorld for World<'_> {
    fn time(&self) -> f64 {
        self.env.time()
    }
    fn random_a(&mut self) -> &mut dyn super::super::random::Random {
        self.random.a_mut()
    }
    fn random_b(&mut self) -> &mut dyn super::super::random::Random {
        if self.jet {
            self.random.nav_game_mut()
        } else {
            self.random.b_mut()
        }
    }
    fn resolve(&mut self, r: UnitRef) -> Option<Resolved> {
        let unit = project(
            self.store,self.seats,
            self.classes,
            self.actors,
            Actor(r.unit),
            self.env,
        )?;
        if unit.alive_false || r.generation.is_some_and(|g| unit.generation != Some(g)) {
            return None;
        }
        Some(Resolved {
            id: r.unit,
            position: unit.position,
            velocity: Some(unit.velocity),
        })
    }
    fn leader(&mut self, r: UnitRef) -> Option<NavUnit> {
        let unit = project(
            self.store,self.seats,
            self.classes,
            self.actors,
            Actor(r.unit),
            self.env,
        )?;
        if unit.alive_false || r.generation.is_some_and(|g| unit.generation != Some(g)) {
            None
        } else {
            Some(unit)
        }
    }
    fn remembered(&mut self, target: u64) -> Option<Vec3> {
        self.env.remembered(Actor(target))
    }
    fn clear_above(&mut self, from: Vec3, unit: &NavUnit) -> bool {
        let Some(binding) = self.actors.get(Actor(unit.id).reference()) else {
            return false;
        };
        if binding.target.unit(self.store).is_none() {return false;}
        self.env.clear_above(self.store,self.seats,from,&binding.target)
    }
    fn follow_far(&mut self) -> f64 {
        self.env.follow_far()
    }
}

fn resolve<'a>(store: &'a Store, actors: &Slab<Binding>, reference: UnitRef) -> Option<&'a Unit> {
    let binding = actors.get(Actor(reference.unit).reference())?;
    if binding.retiring||binding.seat.is_some() {
        return None;
    }
    let unit = binding.target.unit(store)?;
    if !unit.alive
        || reference
            .generation
            .is_some_and(|life| life != unit.generation)
    {
        None
    } else {
        Some(unit)
    }
}

fn valid_turret_lease(store:&Store,target:&AiLifeHandle,seat:Seat)->Option<()> {
    target.unit(store)?;
    let lease=store.seat_lease.get(seat.lease)?;
    (lease.turret==seat.turret&&seat.turret.is_some()&&lease.index==seat.index as f64
        &&lease.controllable==Some(seat.unit)&&crate::seats::lease::live(lease,seat.lease,&store.unit)).then_some(())
}
fn valid_seat<'a>(
    store: &'a Store,
    target: &AiLifeHandle,
    seat: Seat,
) -> Option<&'a crate::gen::Vehicle> {
    let h = target.vehicle(store)?;
    let l = store.seat_lease.get(seat.lease)?;
    if !h.alive
        || !crate::seats::lease::live(l, seat.lease, &store.unit)
        || l.controllable != Some(seat.unit)
        || l.vehicle != Some(Ref::new(target.slot, target.generation))
        || l.index != seat.index as f64
        || h.seats.get(seat.index)?.lease != Some(seat.lease)
    {
        return None;
    }
    Some(h)
}
fn project_seat(
    store: &mut Store,
    seats: &mut Seats,
    binding: &Binding,
    class: &ClassDef,
    actor: Actor,
    seat: Seat,
    env: &mut dyn Environment,
) -> Option<NavUnit> {
    if let Some(turret)=seat.turret {
        valid_turret_lease(store,&binding.target,seat)?;
        let turret=seats.crews.turrets.iter_mut().find(|side|side.turret==turret&&side.lease==Some(seat.lease)&&side.alive)?;
        let mut out=NavUnit::new(actor.0,7.,Vec3::new(turret.desc.position.x,turret.desc.position.y,turret.desc.position.z));
        out.generation=Some(1.);out.control_identity=binding.identity;out.class_def=Some(class.clone());
        out.soldier=Some(SoldierView {aim_yaw:Some(turret.desc.facing+turret.yaw),aim_pitch:Some(turret.pitch),max_speed:Some(0.),max_strafe:Some(0.),max_turn:Some(seat.turn),max_pitch:Some(seat.pitch),posture:Posture::Stand,air:Air::None,..Default::default()});
        let origin=turret.origin();let forward=turret.view_direction(turret.yaw,turret.pitch);
        out.origin=Some(Vec3::new(origin.x,origin.y,origin.z));out.forward=Some(Vec3::new(forward.x,forward.y,forward.z));return Some(out);
    }
    valid_seat(store,&binding.target,seat)?;
    let frame = env.seat_frame(store,seats,
        HullLife {
            vehicle: Ref::new(binding.target.slot, binding.target.generation),
            life: binding.target.life,
        },
        seat.index,
    )?;
    let h=valid_seat(store,&binding.target,seat)?;
    let mut out = NavUnit::new(
        actor.0,
        seat.kind,
        Vec3::new(h.position[0], h.position[1], h.position[2]),
    );
    out.velocity = Vec3::new(h.velocity[0], h.velocity[1], h.velocity[2]);
    out.generation = Some(1.);
    out.control_identity = binding.identity;
    out.class_def = Some(class.clone());
    out.nav_radius =
        Some(v8math::hypot(h.size[0], h.size[2]) * if seat.kind == 6. { 0.5 } else { 0.25 });
    out.soldier = Some(SoldierView {
        aim_yaw: Some(h.seats[seat.index].aim_yaw),
        aim_pitch: Some(h.seats[seat.index].pitch),
        max_speed: Some(seat.speed),
        max_strafe: Some(seat.strafe),
        max_turn: Some(seat.turn),
        max_pitch: Some(seat.pitch),
        posture: Posture::Stand,
        air: Air::None,
        ..SoldierView::default()
    });
    out.origin = Some(frame.0);
    out.forward = Some(frame.1);
    Some(out)
}
fn write_seat_intent(
    out: &mut crate::seats::lease::SeatIntent,
    value: super::super::nav::analogs::Intent,
) -> bool {
    let old = (
        out.forward.to_bits(),
        out.right.to_bits(),
        out.turn.to_bits(),
        out.pitch.to_bits(),
        out.jump,
        out.crouch,
        out.prone,
    );
    out.forward = value.forward;
    out.right = value.right;
    out.turn = value.turn;
    out.pitch = value.pitch;
    out.jump |= value.jump;
    out.crouch |= value.crouch;
    out.prone |= value.prone;
    old != (
        out.forward.to_bits(),
        out.right.to_bits(),
        out.turn.to_bits(),
        out.pitch.to_bits(),
        out.jump,
        out.crouch,
        out.prone,
    )
}
