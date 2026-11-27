//! Navigation on canonical unit rows. Only helper, route and controller state
//! persists here; movement facts are read from the slab at each observation.
// Phase owners and independent scalar fixtures use the same navigation body.

macro_rules! nav_store {($w:expr,$s:expr)=>{&($w).sim.store};}

macro_rules! nav_store_mut {($w:expr,$s:expr)=>{&mut ($w).sim.store};}

macro_rules! nav_seats_mut {($w:expr,$s:expr)=>{($w).fleet.seats_owner()};}

macro_rules! nav_fn {($f:path,$w:expr,$s:expr,$seats:expr,$($a:expr),* $(,)?)=>{$f($w,$($a),*)};}

macro_rules! nav_method {($e:expr,$f:ident,$w:expr,$s:expr,$seats:expr,$($a:expr),* $(,)?)=>{($e).$f($w,$($a),*)};}

macro_rules! nav_building {($e:expr,$w:expr,$f:ident,$($a:expr),*)=>{($e).$f($w,$($a),*)};}

use super::rows::{row_store_mut};
use super::super::control::world::{ControlObs, HelperObs};
use super::super::nav::{
    goto::{FlyGoto, GotoMode, GotoWorld, GroundGoto, Resolved},
    math::{d2, Vec3},
    swbf2::{NavWalkStatus, ProbeKind, RouteQuery, Swbf2GotoWorld, Swbf2GroundGoto},
    units::{
        traversal_mask, Air, ClassDef, JetView, NavUnit, Posture, SizeType, SoldierView, UnitRef,
    },
    Nav,
};
use super::super::phase::RandomState;
use crate::gen::{AiLifeHandle, SeatLease, SeatUnit, Store, Unit, UnitBody};
use crate::seats::lease::control_key;
use crate::store::{Ref, Slab};
use crate::vehicles::f2_seats::HullLife;

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
    /// The native scope fixes the policy before entering controller code.
    fn uses_swbf2() -> bool where Self: Sized { false }
    fn line_of_walk(&mut self, w: &mut crate::world::owner::World, from: Vec3, to: Vec3) -> bool {
        super::live::crosses(w, from, to, 0)
    }
    fn try_line_of_walk(
        &mut self,
        w: &mut crate::world::owner::World,
        from: Vec3,
        to: Vec3,
        _kind: crate::ai::nav::swbf2::ProbeKind,
    ) -> Option<bool> {
        Some(self.line_of_walk(w, from, to))
    }
    fn time(&self) -> f64;
    fn building_target(&mut self, _w:&crate::world::owner::World,_target:&crate::gen::F4Handle)->Option<FollowView> {None}
    fn trap_target(&mut self, _w:&crate::world::owner::World,_target:&crate::gen::F4Handle,_position:Vec3)->Option<FollowView> {None}
    /// Unit.origin/forward switches to its lease immediately, even before the
    /// next AI sync replaces the infantry controller with a seat controller.
    fn unit_frame(&mut self, w:&mut crate::world::owner::World,unit:Ref<Unit>)->Option<(Vec3,Vec3)> {
        let seat=nav_store!(w,store).unit.get(unit)?.seat;
        let source=seat.and_then(|r|nav_store!(w,store).seat_lease.get(r));
        if let Some(lease)=source {
            if let Some(hull)=lease.vehicle {
                let life=nav_store!(w,store).vehicle.get(hull)?.target_generation;
                let index=lease.index as usize;
                return nav_method!(self,seat_frame,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats),HullLife {vehicle:hull,life},index);
            }
            if let Some(turret)=lease.turret {
                let side=nav_seats_mut!(w,seats).crews.turrets.iter_mut().find(|side|side.turret==turret)?;
                let p=side.origin();let d=side.view_direction(side.yaw,side.pitch);
                return Some((Vec3::new(p.x,p.y,p.z),Vec3::new(d.x,d.y,d.z)));
            }
        }
        let body=nav_store!(w,store).unit.get(unit)?;
        let p=body.origin_on_foot();let d=body.forward_on_foot();
        Some((Vec3::new(p.x,p.y,p.z),Vec3::new(d.x,d.y,d.z)))
    }
    fn remembered(&mut self, target: Actor) -> Option<Vec3>;
    fn clear_above(&mut self, w:&mut crate::world::owner::World,from:Vec3,unit:&AiLifeHandle)->bool;
    fn follow_far(&mut self) -> f64;
    /// SeatLease.origin/forward on the current simulation rig.
    fn seat_frame(&mut self, w:&mut crate::world::owner::World,hull: HullLife,index:usize) -> Option<(Vec3, Vec3)>;
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

struct ExternalWalk {
    control: usize,
    helper: Swbf2GroundGoto,
    unit: Ref<Unit>,
    life: f64,
    raw_target: Vec3,
    approach: Option<f64>,
    resumed_tick: Option<u32>,
    active: bool,
}

pub struct Navigation {
    pub nav: crate::ai::nav::route::MapNavigation,
    ground_helpers: Vec<Option<(u32, Swbf2GroundGoto)>>,
    external_walks: Vec<ExternalWalk>,
    classes: Vec<ClassDef>,
    actors: Slab<Binding>,
    helpers: Slab<HelperRow>,
    free_controls:Vec<usize>,
    dirty: Vec<AiLifeHandle>,
    input_body_changed: bool,
}
impl Navigation {
    pub(crate) fn set_swbf2(
        &mut self,
        state: Box<crate::ai::nav::swbf2::Swbf2Nav>,
        capacity: usize,
        external: usize,
    ) {
        self.nav.swbf2 = Some(state);
        self.ground_helpers.resize_with(capacity * 2 + 8, || None);
        self.nav.controls.reserve(external);
        self.nav.store.reserve_paths(external.saturating_mul(3));
        self.external_walks.reserve_exact(external);
        for _ in 0..external {
            let control = self.nav.new_control();
            self.external_walks.push(ExternalWalk {
                control,
                helper: Swbf2GroundGoto::new(control),
                unit: Ref::new(0, 0),
                life: 0.,
                raw_target: Vec3::default(),
                approach: None,
                resumed_tick: None,
                active: false,
            });
        }
    }
    #[cfg(test)]
    pub(crate) fn external_walk_capacity(&self) -> usize {
        self.external_walks.capacity()
    }
    pub fn new(mut nav: Nav, capacity: usize) -> Self {
        nav.publications = None;
        nav.prepare_owned();
        nav.controls.reserve(capacity);
        nav.store.reserve_paths(capacity.saturating_mul(3).saturating_add(8));
        Self {
            nav: nav.into(),
            ground_helpers: Vec::new(),
            external_walks: Vec::new(),
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
    pub fn phase_command<E: Environment>(&mut self, w: &mut crate::world::owner::World,
        random: &mut RandomState, env: &mut E, actor: Actor, command: Command<'_>) -> Option<HelperObs> {
        let key = self.helper_key(actor)?;
        self.phase_command_for(w, random, env, key, command)
    }

    pub fn phase_command_for<E: Environment>(&mut self, w: &mut crate::world::owner::World,
        random: &mut RandomState, env: &mut E, key: HelperKey, command: Command<'_>) -> Option<HelperObs> {
        if !E::uses_swbf2() { return self.command_for(w, random, env, key, command); }
        self.swbf2_command(w, random, env, key, command, false)
    }

    /// What ran out of a tick's work goes on before this tick's new orders.
    pub(super) fn resume_walks<E: Environment>(
        &mut self,
        w: &mut crate::world::owner::World,
        nav_classes: &[Option<usize>],
        random: &mut RandomState,
        env: &mut E,
    ) {
        let mut last = None;
        loop {
            let helper_next = self.ground_helpers.iter().enumerate().filter_map(|(slot, entry)| {
                let (life, h) = entry.as_ref()?;
                let (place, kind) = self.nav.swbf2_ground_place(slot, h)?;
                let order = (place.0, place.1, false, place.2);
                (last < Some(order) && w.ai.has_probe_work(kind)).then_some((order, Some(*life)))
            });
            let external_next = self.external_walks.iter().enumerate().filter_map(|(slot, entry)| {
                if !entry.active {
                    return None;
                }
                let (place, kind) = self.nav.swbf2_ground_place(slot, &entry.helper)?;
                let order = (place.0, place.1, true, place.2);
                (last < Some(order) && w.ai.has_probe_work(kind)).then_some((order, None))
            });
            let Some((order, life)) = helper_next.chain(external_next).min() else {
                return;
            };
            last = Some(order);
            if let Some(life) = life {
                let key = HelperKey(Ref::new(order.3 as u32, life));
                self.swbf2_helper(w, random, env, key, None);
            } else {
                self.resume_external_walk(w, nav_classes, order.3);
            }
        }
    }

    fn resume_external_walk(
        &mut self,
        w: &mut crate::world::owner::World,
        nav_classes: &[Option<usize>],
        slot: usize,
    ) {
        let Some(entry) = self.external_walks.get(slot) else {
            return;
        };
        if !entry.active {
            return;
        }
        let unit_ref = entry.unit;
        let life = entry.life;
        let live = w
            .sim
            .store
            .unit
            .get(unit_ref)
            .is_some_and(|u| u.alive && u.generation == life && u.seat.is_none());
        if !live {
            self.release_external_walk(slot);
            return;
        }
        let Some(mut nav_unit) =
            project_external_unit(&w.sim.store, nav_classes, &self.classes, unit_ref)
        else {
            self.release_external_walk(slot);
            return;
        };
        let tick = self.nav.swbf2.as_ref().and_then(|s| s.query_tick());
        let entry = &mut self.external_walks[slot];
        let approach = entry.approach;
        let nav = &mut self.nav;
        crate::space::hulls_meet(w, crate::space::flight::HullMeets::Soldiers, |w| {
            let mut ext_world = ExternalWorld {
                w,
                unit: unit_ref,
                life,
                approach,
                random: crate::ai::random::ClassicRandom::new(0.),
            };
            nav.swbf2_ground_resume(&mut entry.helper, &mut nav_unit, &mut ext_world);
        });
        entry.resumed_tick = tick;
        if entry.helper.failed {
            self.release_external_walk(slot);
        }
    }

    fn release_external_walk(&mut self, slot: usize) {
        let Some(entry) = self.external_walks.get_mut(slot) else {
            return;
        };
        self.nav.swbf2_ground_cancel(&mut entry.helper);
        entry.active = false;
        entry.approach = None;
        entry.resumed_tick = None;
    }

    fn external_status(&self, slot: usize) -> NavWalkStatus {
        let Some(entry) = self.external_walks.get(slot) else {
            return NavWalkStatus::Failed;
        };
        if !entry.active || entry.helper.failed {
            return NavWalkStatus::Failed;
        }
        if self.nav.swbf2_ground_arrived(&entry.helper) {
            return NavWalkStatus::Arrived;
        }
        let check_body = self
            .nav
            .swbf2
            .as_ref()
            .and_then(|s| s.legs.get(entry.control))
            .and_then(Option::as_ref)
            .is_some_and(|leg| leg.check_body);
        let has_executor = self.nav.controls[entry.control].executor.is_some();
        if check_body || (self.nav.swbf2_ground_waiting(&entry.helper) && !has_executor) {
            return NavWalkStatus::Waiting;
        }
        if has_executor || entry.helper.cursor.is_some() {
            NavWalkStatus::Moving
        } else {
            NavWalkStatus::Failed
        }
    }

    pub(crate) fn nav_walk_begin(
        &mut self,
        w: &mut crate::world::owner::World,
        nav_classes: &[Option<usize>],
        unit_ref: Ref<Unit>,
        target: [f64; 3],
        approach: f64,
    ) -> NavWalkStatus {
        if self.nav.swbf2.is_none()
            || self.external_walks.is_empty()
            || !w.ai.has_ground_probe()
            || !target.iter().all(|v| v.is_finite())
        {
            return NavWalkStatus::Failed;
        }
        let approach = if approach.is_finite() && approach > 0.0 {
            approach
        } else {
            0.0
        };
        let slot = unit_ref.slot as usize;
        if slot >= self.external_walks.len() {
            return NavWalkStatus::Failed;
        }
        let Some(row) = w.sim.store.unit.get(unit_ref) else {
            if self.external_walks[slot].active && self.external_walks[slot].unit == unit_ref {
                self.release_external_walk(slot);
            }
            return NavWalkStatus::Failed;
        };
        if !row.alive || row.seat.is_some() {
            if self.external_walks[slot].active {
                self.release_external_walk(slot);
            }
            return NavWalkStatus::Failed;
        }
        let life = row.generation;
        let raw = Vec3::new(target[0], target[1], target[2]);
        if self.nav.swbf2.as_ref().unwrap().query_tick().is_none() {
            self.nav.swbf2.as_mut().unwrap().begin_tick(0);
            w.ai.begin_probe_tick(0);
        }
        let keep = {
            let entry = &self.external_walks[slot];
            entry.active
                && entry.unit == unit_ref
                && entry.life == life
                && d2(entry.raw_target, raw) < 0.01
                && entry.approach.unwrap_or(0.0).to_bits() == approach.to_bits()
        };
        if keep {
            let status = self.external_status(slot);
            if matches!(status, NavWalkStatus::Arrived | NavWalkStatus::Failed) {
                self.release_external_walk(slot);
            }
            return status;
        }
        if self.external_walks[slot].active {
            self.release_external_walk(slot);
        }
        let Some(mut nav_unit) =
            project_external_unit(&w.sim.store, nav_classes, &self.classes, unit_ref)
        else {
            return NavWalkStatus::Failed;
        };
        let mask = traversal_mask(&nav_unit);
        if mask == 0 {
            return NavWalkStatus::Failed;
        }
        // Reject disconnected floors before spending a per-tick start credit or
        // running any physical floor cast or mover check.
        if !self.nav.swbf2.as_ref().unwrap().may_connect(
            nav_unit.position,
            raw,
            v8math::max(8.0, approach),
            mask,
        ) {
            return NavWalkStatus::Failed;
        }
        if !self.nav.swbf2.as_mut().unwrap().take_external_start() {
            return NavWalkStatus::Failed;
        }
        let snapped = match super::walk::support(w, raw, 0.32, 1.25)
            .or_else(|| super::walk::support(w, raw, 8.0, 8.0))
        {
            Some(p) => p,
            None if approach > 0.0 => raw,
            None => return NavWalkStatus::Failed,
        };
        if !self
            .nav
            .swbf2
            .as_ref()
            .unwrap()
            .may_connect(nav_unit.position, snapped, approach, mask)
        {
            return NavWalkStatus::Failed;
        }
        let approach = (approach > 0.0).then_some(approach);
        let entry = &mut self.external_walks[slot];
        entry.unit = unit_ref;
        entry.life = life;
        entry.raw_target = raw;
        entry.approach = approach;
        entry.resumed_tick = None;
        entry.active = true;
        let mut ext_world = ExternalWorld {
            w,
            unit: unit_ref,
            life,
            approach,
            random: crate::ai::random::ClassicRandom::new(0.),
        };
        self.nav.ground_approach(
            &mut entry.helper,
            &mut nav_unit,
            &mut ext_world,
            snapped,
            GotoMode::Route,
            false,
        );
        let status = self.external_status(slot);
        if matches!(status, NavWalkStatus::Arrived | NavWalkStatus::Failed) {
            self.release_external_walk(slot);
        }
        status
    }

    pub(crate) fn nav_walk_step(
        &mut self,
        w: &mut crate::world::owner::World,
        nav_classes: &[Option<usize>],
        unit_ref: Ref<Unit>,
        dt: f64,
    ) -> NavWalkStatus {
        if self.nav.swbf2.is_none()
            || self.external_walks.is_empty()
            || !w.ai.has_ground_probe()
            || !dt.is_finite()
            || dt < 0.0
        {
            return NavWalkStatus::Failed;
        }
        let slot = unit_ref.slot as usize;
        let Some(entry) = self.external_walks.get(slot) else {
            return NavWalkStatus::Failed;
        };
        if !entry.active {
            return NavWalkStatus::Failed;
        }
        if entry.unit != unit_ref {
            return NavWalkStatus::Failed;
        }
        let life = entry.life;
        let live = w
            .sim
            .store
            .unit
            .get(unit_ref)
            .is_some_and(|u| u.alive && u.generation == life && u.seat.is_none());
        if !live {
            self.release_external_walk(slot);
            return NavWalkStatus::Failed;
        }
        if self.nav.swbf2.as_ref().unwrap().query_tick().is_none() {
            self.nav.swbf2.as_mut().unwrap().begin_tick(0);
            w.ai.begin_probe_tick(0);
        }
        let tick = self.nav.swbf2.as_ref().unwrap().query_tick();
        if self.external_walks[slot].helper.failed {
            self.release_external_walk(slot);
            return NavWalkStatus::Failed;
        }
        if self
            .nav
            .swbf2_ground_arrived(&self.external_walks[slot].helper)
        {
            if let Some(row) = w.sim.store.unit.get_mut(unit_ref) {
                overwrite_intent(&mut row.intent, Default::default());
            }
            self.release_external_walk(slot);
            return NavWalkStatus::Arrived;
        }
        let Some(mut nav_unit) =
            project_external_unit(&w.sim.store, nav_classes, &self.classes, unit_ref)
        else {
            self.release_external_walk(slot);
            return NavWalkStatus::Failed;
        };
        let control = self.external_walks[slot].control;
        let already_resumed_waiting = self.external_walks[slot].resumed_tick == tick
            && (self.nav.swbf2_ground_waiting(&self.external_walks[slot].helper)
                || self
                    .nav
                    .swbf2
                    .as_ref()
                    .unwrap()
                    .legs
                    .get(control)
                    .and_then(Option::as_ref)
                    .is_some_and(|leg| leg.check_body));
        if !already_resumed_waiting {
            let entry = &mut self.external_walks[slot];
            let mut ext_world = ExternalWorld {
                w,
                unit: unit_ref,
                life,
                approach: entry.approach,
                random: crate::ai::random::ClassicRandom::new(0.),
            };
            self.nav
                .swbf2_ground_tick(&mut entry.helper, &mut nav_unit, &mut ext_world, dt);
        }
        if self.external_walks[slot].helper.failed {
            self.release_external_walk(slot);
            return NavWalkStatus::Failed;
        }
        if self
            .nav
            .swbf2_ground_arrived(&self.external_walks[slot].helper)
        {
            if let Some(row) = w.sim.store.unit.get_mut(unit_ref) {
                overwrite_intent(&mut row.intent, Default::default());
            }
            self.release_external_walk(slot);
            return NavWalkStatus::Arrived;
        }
        let approach = self.external_walks[slot].approach;
        let intent = {
            let mut ext_world = ExternalWorld {
                w,
                unit: unit_ref,
                life,
                approach,
                random: crate::ai::random::ClassicRandom::new(0.),
            };
            self.nav
                .swbf2_navigation_input(control, &mut nav_unit, dt, None, false, &mut ext_world)
        };
        if let Some(row) = w.sim.store.unit.get_mut(unit_ref) {
            write_external_avoidance(row, &nav_unit);
            overwrite_intent(&mut row.intent, intent);
        }
        let status = self.external_status(slot);
        if matches!(status, NavWalkStatus::Arrived | NavWalkStatus::Failed) {
            self.release_external_walk(slot);
        }
        status
    }

    pub(crate) fn nav_walk_stop(&mut self, unit_ref: Ref<Unit>) {
        let slot = unit_ref.slot as usize;
        if self
            .external_walks
            .get(slot)
            .is_some_and(|e| e.active && e.unit == unit_ref)
        {
            self.release_external_walk(slot);
        }
    }

    pub(super) fn combat_approach<E: Environment>(&mut self, w: &mut crate::world::owner::World,
        random: &mut RandomState, env: &mut E, key: HelperKey, point: Vec3) -> Option<HelperObs> {
        self.swbf2_command(w, random, env, key, Command::Goto(point, GotoMode::Route), true)
    }

    fn swbf2_command<E: Environment>(&mut self, w: &mut crate::world::owner::World,
        random: &mut RandomState, env: &mut E, key: HelperKey, command: Command<'_>, combat: bool) -> Option<HelperObs> {
        let helper = self.helpers.get(key.0)?;
        let binding = self.actors.get(helper.actor.reference())?;
        if binding.retiring || binding.control == NO_CONTROL || !matches!(helper.value, Helper::Ground(_))
            || matches!(command, Command::Attach | Command::FlightPath(..)) {
            return self.command_for(w, random, env, key, command);
        }
        self.swbf2_helper(w, random, env, key, Some((command, combat)))
    }

    /// Carry out a command, or go on with a waiting order.
    fn swbf2_helper<E: Environment>(&mut self, w: &mut crate::world::owner::World,
        random: &mut RandomState, env: &mut E, key: HelperKey, command: Option<(Command<'_>, bool)>) -> Option<HelperObs> {
        let helper = self.helpers.get(key.0)?;
        let actor = helper.actor;
        let binding = self.actors.get(actor.reference())?;
        let control = binding.control;
        if binding.retiring || control == NO_CONTROL { return None; }
        let mut unit = project(w, &self.classes, &self.actors, actor, env)?;
        let mut world = World { w, classes: &self.classes, actors: &self.actors, random, env, jet: true };
        let slot = self.ground_helpers.get_mut(key.0.slot as usize)?;
        if !slot.as_ref().is_some_and(|(life, _)| *life == key.0.gen) {
            *slot = Some((key.0.gen, Swbf2GroundGoto::new(control)));
        }
        let (_, h) = slot.as_mut()?;
        let Some((command, combat)) = command else {
            if unit.alive_false || binding.seat.is_some() {
                self.nav.swbf2_ground_abandon(h);
                return None;
            }
            self.nav.swbf2_ground_resume(h, &mut unit, &mut world);
            return self.swbf2_observation(w, actor, key, &unit);
        };
        match command {
                Command::Attach|Command::FlightPath(..)=>unreachable!(),
                Command::Update(dt) => self.nav.swbf2_control_update(control, dt, &mut unit, &mut world),
                Command::Goto(p, m) => self.nav.ground_approach(h, &mut unit, &mut world, p, m, combat),
                Command::Tick(dt) => self.nav.swbf2_ground_tick(h, &mut unit, &mut world, dt),
                Command::Stop => self.nav.swbf2_ground_stop(h, &mut unit, &mut world),
                Command::Follow(r, d, m) => {
                    self.nav
                        .swbf2_ground_follow(h, &mut unit, &mut world, Some(r), d, m)
                }
                Command::Install(points) => self.nav.swbf2_ground_install_path(h,&mut unit,&mut world,points),
                Command::Hold(on) => self.nav.swbf2_ground_hold_path(h, on),
                Command::Trail(on) => self.nav.control_record_trail(control, on, &mut unit),
                Command::Leader(r) => self.nav.control_set_leader(control, r),
                Command::Stance(v) => self.nav.swbf2_ground_stance(h, &mut unit, &mut world, v),
                Command::Jet(v) => self.nav.swbf2_ground_jet(h, &mut unit, &mut world, v),
                Command::Aim(v) => self.nav.swbf2_ground_aim_at(h, &mut unit, &mut world, v),
                Command::AimTarget(r) => self.nav.swbf2_ground_aim_at_target(h, &mut unit, &mut world, r),
                Command::AimPath => self.nav.swbf2_ground_aim_along_path(h, &mut unit, &mut world),
            }
        self.swbf2_observation(w, actor, key, &unit)
    }

    fn swbf2_observation(&mut self, w: &mut crate::world::owner::World, actor: Actor, key: HelperKey, unit: &NavUnit) -> Option<HelperObs> {
        let (_, h) = self.ground_helpers.get(key.0.slot as usize)?.as_ref()?;
        // The retained controller observes the same destination and completion
        // fields for both families, including after a helper identity is renewed.
        let arrived = self.nav.swbf2_ground_arrived(h);
        if let Helper::Ground(view) = &mut self.helpers.get_mut(key.0)?.value {
            // On 2005 maps, a failed ground walk reports arrived alongside failed,
            // matching 2004 GroundGoto so melee, snipe, and mine consumers can retry.
            view.state = if arrived || h.failed { 7. } else { 2. };
            view.destination = h.destination;
            view.failed = h.failed;
        }
        let binding = self.actors.get(actor.reference())?;
        if binding.seat.is_none() && write_avoidance(&mut w.sim.store, &binding.target, unit) {
            mark(&mut self.dirty, &binding.target);
        }
        self.helper_observation_for(&w.sim.store, key)
    }
    pub fn command(
        &mut self,
         w:&mut crate::world::owner::World,
        random: &mut RandomState,
        env: &mut dyn Environment,
        actor: Actor,
        command: Command<'_>,
    ) -> Option<HelperObs> {
        let key=self.helper_key(actor)?;
        nav_method!(self,command_for,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats),random,env,key,command)
    }
    pub fn command_for(&mut self, w:&mut crate::world::owner::World,random:&mut RandomState,env:&mut dyn Environment,key:HelperKey,command:Command<'_>)->Option<HelperObs> {
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
            return self.helper_observation_for(nav_store_mut!(w,store),key);
        }
        let kind=binding.kind(nav_store_mut!(w,store))?;
        if matches!(command,Command::Attach) {
            // attach reads only object identity and generation.
            // In particular it must not pose a mounted gun before steering.
            let mut unit=NavUnit::new(actor.0,kind,Vec3::default());
            unit.generation=Some(binding.life());unit.control_identity=binding.identity;
            self.nav.control_attach(binding.control,Some(&unit));return self.helper_observation_for(nav_store_mut!(w,store),key);
        }
        if matches!(command,Command::Update(_)) {
            let control=&self.nav.controls[binding.control];
            // With no executor, trail or stranded state, control.update has
            // no unit observation or elapsed clock to advance.
            if control.executor.is_none()&&control.leader.is_none()&&!control.stranded&&control.stranded_for==0. {return self.helper_observation_for(nav_store_mut!(w,store),key);}
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
            return self.helper_observation_for(nav_store_mut!(w,store),key);
        }
        let mut unit = nav_fn!(project,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats), &self.classes, &self.actors, actor, env)?;
        let control = binding.control;
        // VehicleAI runs attach/update separately from seat steering. These
        // operations share the controller's control, without latching input.
        match command {
            Command::Attach=>{self.nav.control_attach(control,Some(&unit));return self.helper_observation_for(nav_store_mut!(w,store),key);},
            Command::Update(dt)=>{
                let mut world=World { w:&mut *w,classes:&self.classes,actors:&self.actors,random,env,jet:false};
                self.nav.control_update(control,dt,&mut unit,&mut world);
                if self.actors.get(actor.reference())?.seat.is_none()&&write_avoidance(row_store_mut!(world),&binding.target,&unit) {mark(&mut self.dirty,&binding.target);}
                return self.helper_observation_for(row_store_mut!(world),key);
            },
            Command::FlightPath(points,over,altitude)=>{
                if points.is_empty() {return self.helper_observation_for(nav_store_mut!(w,store),key);}
                let Helper::Fly(helper)=&mut self.helpers.get_mut(key.0)?.value else{return None};
                self.nav.control_attach(control,Some(&unit));self.nav.planner_cancel(control);
                let path=self.nav.path_new();
                for &[x,y,z] in points.iter().rev() {if !self.nav.store.prepend(path,x,y,z) {self.nav.store.release(path);return self.helper_observation_for(nav_store_mut!(w,store),key);}}
                let last=points.last().unwrap();helper.destination=Vec3::new(last[0],last[1],last[2]);
                self.nav.control_install(control,path,over,altitude,&mut unit);
                helper.failed=false;helper.state=4.;helper.pending=0.;
                return self.helper_observation_for(nav_store_mut!(w,store),key);
            },
            _=>{},
        }
        let helper = &mut self.helpers.get_mut(key.0)?.value;
        let mut world = World { w:&mut *w,
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
            && write_avoidance(nav_store_mut!(w,store), target, &unit)
        {
            mark(&mut self.dirty, target);
        }
        self.helper_observation_for(nav_store_mut!(w,store),key)
    }
    /// Called within the AI input phase. The generated intent row is written
    /// directly, preserving unrelated weapon edges and held buttons.
    #[allow(clippy::too_many_arguments)]
    pub fn phase_input<E: Environment>(&mut self, w: &mut crate::world::owner::World,
        random: &mut RandomState, env: &mut E, actor: Actor, dt: f64, aim: Option<Vec3>, facing: bool) -> Option<bool> {
        if E::uses_swbf2() { self.input_swbf2(w, random, env, actor, dt, aim, facing) }
        else { self.input(w, random, env, actor, dt, aim, facing) }
    }
    #[allow(clippy::too_many_arguments)]
    fn input_swbf2(
        &mut self, w:&mut crate::world::owner::World,random:&mut RandomState,
        env:&mut dyn Environment,actor:Actor,dt:f64,aim:Option<Vec3>,facing:bool,
    )->Option<bool> {
        self.input_body_changed=false;
        let binding = self.actors.get(actor.reference())?;
        if binding.control==NO_CONTROL {return None;}
        let mut unit = nav_fn!(project,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats), &self.classes, &self.actors, actor, env)?;
        nav_fn!(input_frame,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats),binding,env,&mut unit)?;
        let mut world = World { w:&mut *w,
            classes: &self.classes,
            actors: &self.actors,
            random,
            env,
            jet: false,
        };
        let intent =
            self.nav
                .swbf2_navigation_input(binding.control, &mut unit, dt, aim, facing, &mut world);
        let changed = if let Some(seat) = binding.seat {
            let row = nav_seats_mut!(w,seats).hulls.controls.get_mut(&control_key(seat.unit))?;
            write_seat_intent(&mut row.intent, intent)
        } else {
            let changed = write_avoidance(nav_store_mut!(w,store), &binding.target, &unit);
            self.input_body_changed=changed;
            let row = nav_store_mut!(w,store)
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
    #[allow(clippy::too_many_arguments)]
    pub fn input(
        &mut self, w:&mut crate::world::owner::World,random:&mut RandomState,
        env:&mut dyn Environment,actor:Actor,dt:f64,aim:Option<Vec3>,facing:bool,
    )->Option<bool> {
        self.input_body_changed=false;
        let binding = self.actors.get(actor.reference())?;
        if binding.control==NO_CONTROL {return None;}
        let mut unit = nav_fn!(project,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats), &self.classes, &self.actors, actor, env)?;
        nav_fn!(input_frame,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats),binding,env,&mut unit)?;
        let mut world = World { w:&mut *w,
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
            let row = nav_seats_mut!(w,seats).hulls.controls.get_mut(&control_key(seat.unit))?;
            write_seat_intent(&mut row.intent, intent)
        } else {
            let changed = write_avoidance(nav_store_mut!(w,store), &binding.target, &unit);
            self.input_body_changed=changed;
            let row = nav_store_mut!(w,store)
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
    /// plain conversion does not step a control, consume a
    /// jet or toggle posture. The scheduler owns those edges in this path.
    pub fn analogs(&mut self, w:&mut crate::world::owner::World,env:&mut dyn Environment,actor:Actor,aim:Option<Vec3>,dt:f64,facing:bool)->Option<[f64;4]> {
        let binding=self.actors.get(actor.reference())?;
        let mut unit=nav_fn!(project,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats),&self.classes,&self.actors,actor,env)?;
        nav_fn!(input_frame,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats),binding,env,&mut unit)?;
        let control=(binding.control!=NO_CONTROL).then(||&self.nav.controls[binding.control]);
        let turn=if self.nav.swbf2.is_some() {crate::ai::nav::swbf2::goto::bot_turn(&unit)} else {1.0};
        let intent=crate::ai::nav::analogs::analogs_with_turn(&unit,control.map(|control|&control.request),aim,dt,facing,control.and_then(|control|control.executor.as_ref().map(|executor|executor.hold)),turn);
        Some([intent.forward,intent.right,intent.turn,intent.pitch])
    }
    pub fn dirty(&self) -> &[AiLifeHandle] {
        &self.dirty
    }
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }
    /// Deterministic, zero-allocation reachability and walkable route distance
    /// between two world positions across 2005 graphs, 2004 graphs, and
    /// graphless barrier/collision worlds.
    #[allow(clippy::too_many_arguments)]
    pub fn route_distance(
        &mut self,
        w: &mut crate::world::owner::World,
        env: &mut dyn Environment,
        from: Vec3,
        to: Vec3,
        radius: f64,
        mask: u32,
        can_jet: bool,
    ) -> RouteQuery {
        if self.nav.swbf2.as_ref().is_some_and(|s| !s.has_query_credit() || w.ai.ground_probe.is_none()) { return RouteQuery::UNKNOWN; }
        let cached=super::live::observe_navigation(w,&mut self.nav);
        self.nav.try_distance_with_clearance(from,to,radius,mask,can_jet,cached,|a,b,m|if m==0 {env.try_line_of_walk(w,a,b,crate::ai::nav::swbf2::ProbeKind::Question)} else {super::live::try_crosses(w,a,b,m,crate::ai::nav::swbf2::ProbeKind::Question)})
    }
    /// O(1) hub-to-hub route distance lookup on a 2005 planning graph.
    pub fn hub_route_distance(
        &self,
        from_hub: usize,
        to_hub: usize,
        mask: u32,
        can_jet: bool,
    ) -> RouteQuery {
        self.nav
            .swbf2
            .as_ref()
            .and_then(|s| s.graph.as_ref())
            .map_or(RouteQuery::UNREACHABLE, |g| {
                g.hub_route_distance(from_hub, to_hub, mask, can_jet)
            })
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
     w:&mut crate::world::owner::World,
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
            FollowSource::Hull(reference)=>{let hull=nav_store_mut!(w,store).vehicle.get(*reference)?;FollowView {position:Vec3::new(hull.position[0],hull.position[1],hull.position[2]),velocity:Vec3::new(hull.velocity[0],hull.velocity[1],hull.velocity[2]),generation:hull.target_generation,alive:hull.alive}},
            FollowSource::Building(handle)=>nav_building!(env,w,building_target,handle)?,
            FollowSource::Trap(handle,position)=>nav_building!(env,w,trap_target,handle,*position)?,
        };
        let mut out=NavUnit::new(actor.0,target.kind,view.position);out.generation=Some(view.generation);out.velocity=view.velocity;out.alive_false=!view.alive;out.class_def=Some(class.clone());return Some(out);
    }
    if let Some(seat) = binding.seat {
        return nav_fn!(project_seat,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats), binding, class, actor, seat, env);
    }
    let unit = binding.target.unit(nav_store_mut!(w,store))?;
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
fn input_frame( w:&mut crate::world::owner::World,binding:&Binding,env:&mut dyn Environment,unit:&mut NavUnit)->Option<()> {
    if binding.seat.is_none()&&binding.passive.is_none() {
        let reference=Ref::new(binding.target.slot,binding.target.generation);
        if nav_store_mut!(w,store).unit.get(reference)?.seat.is_some() {
            let (origin,forward)=nav_method!(env,unit_frame,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats),reference)?;
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
fn project_external_unit(
    store: &Store,
    nav_classes: &[Option<usize>],
    classes: &[ClassDef],
    reference: Ref<Unit>,
) -> Option<NavUnit> {
    let unit = store.unit.get(reference)?;
    let p = unit.position();
    let v = unit.velocity();
    let id = (u64::from(reference.gen) << 32) | u64::from(reference.slot);
    let mut out = NavUnit::new(id, unit.kind, Vec3::new(p.x, p.y, p.z));
    out.velocity = Vec3::new(v.x, v.y, v.z);
    out.generation = Some(unit.generation);
    out.alive_false = !unit.alive;
    out.nav_radius = unit.nav_radius;
    out.class_def = Some(
        nav_classes
            .get(unit.class_def.0 as usize)
            .copied()
            .flatten()
            .and_then(|idx| classes.get(idx))
            .map_or_else(
                || ClassDef {
                    ai_size_type: SizeType::Number(1.),
                    ..ClassDef::default()
                },
                |class| {
                    let SizeType::Number(mask) = class.ai_size_type else {
                        unreachable!("resolved navigation class")
                    };
                    ClassDef {
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
                    }
                },
            ),
    );
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
    let f = unit.forward_on_foot();
    out.forward = Some(Vec3::new(f.x, f.y, f.z));
    let o = unit.origin_on_foot();
    out.origin = Some(Vec3::new(o.x, o.y, o.z));
    Some(out)
}

fn write_external_avoidance(row: &mut Unit, view: &NavUnit) {
    let Some(value) = view.soldier.and_then(|s| s.avoidance) else {
        return;
    };
    let out = match &mut row.soldier {
        UnitBody::Soldier(s) => &mut s.avoidance,
        UnitBody::Droideka(d) => &mut d.avoidance,
    };
    *out = [value.x, value.y, value.z];
}

fn overwrite_intent(out: &mut crate::gen::Intent, value: super::super::nav::analogs::Intent) {
    out.forward = value.forward;
    out.right = value.right;
    out.turn = value.turn;
    out.pitch = value.pitch;
    out.jump = value.jump;
    out.crouch = value.crouch;
    out.prone = value.prone;
    out.jet_direction = value
        .jet_direction
        .flatten()
        .map(|p| [p.x, p.y, p.z]);
}

struct ExternalWorld<'a> {
    w: &'a mut crate::world::owner::World,
    unit: Ref<Unit>,
    life: f64,
    approach: Option<f64>,
    random: crate::ai::random::ClassicRandom,
}

impl GotoWorld for ExternalWorld<'_> {
    fn time(&self) -> f64 {
        self.w.sim.store.battle.stats.time
    }
    fn random_a(&mut self) -> &mut dyn super::super::random::Random {
        &mut self.random
    }
    fn random_b(&mut self) -> &mut dyn super::super::random::Random {
        &mut self.random
    }
    fn resolve(&mut self, _r: UnitRef) -> Option<Resolved> {
        None
    }
    fn leader(&mut self, _r: UnitRef) -> Option<NavUnit> {
        None
    }
    fn remembered(&mut self, _target: u64) -> Option<Vec3> {
        None
    }
    fn clear_above(&mut self, from: Vec3, _unit: &NavUnit) -> bool {
        let handle = AiLifeHandle {
            kind: crate::gen::KIND_UNIT.into(),
            slot: self.unit.slot,
            generation: self.unit.gen,
            life: self.life,
        };
        super::live::clear_above(self.w, from, &handle)
    }
    fn follow_far(&mut self) -> f64 {
        15.0
    }
}

impl Swbf2GotoWorld for ExternalWorld<'_> {
    fn crosses(&mut self, a: Vec3, b: Vec3, _unit: &NavUnit, m: u8) -> bool {
        super::live::crosses(self.w, a, b, m)
    }
    fn try_crosses(
        &mut self,
        a: Vec3,
        b: Vec3,
        _unit: &NavUnit,
        m: u8,
        kind: ProbeKind,
    ) -> Option<bool> {
        super::live::try_crosses(self.w, a, b, m, kind)
    }
    fn observe_navigation(&mut self, nav: &mut crate::ai::nav::route::MapNavigation) -> bool {
        super::live::observe_navigation(self.w, nav)
    }
    fn objective_approach_radius(&self, _unit: &NavUnit, _point: Vec3) -> Option<f64> {
        self.approach
    }
    fn step_floor(&mut self, point: Vec3) -> Option<Vec3> {
        super::walk::support(self.w, point, 0.65, 0.75)
    }
    fn combat_floor(&mut self, point: Vec3) -> Option<Vec3> {
        super::walk::support(self.w, point, 0.45, 2.5)
    }
    fn sees_approach(&mut self, at: Vec3, target: Vec3) -> bool {
        super::walk::sees_approach(self.w, at, target)
    }
    fn approach_floor(&mut self, point: Vec3) -> Option<Vec3> {
        if self.approach.is_some() {
            self.foot(point)
                .or_else(|| super::walk::support(self.w, point, 1.0, 2.0))
        } else {
            self.foot(point)
                .or_else(|| super::walk::support(self.w, point, 8., 8.))
        }
    }
    fn foot(&mut self, p: Vec3) -> Option<Vec3> {
        super::walk::support(self.w, p, 0.32, 1.25)
    }
    fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld {
        self
    }
    fn line_of_walk(&mut self, from: Vec3, to: Vec3, _unit: &NavUnit) -> bool {
        super::live::crosses(self.w, from, to, 0)
    }
}

struct World<'a> {

    w:&'a mut crate::world::owner::World,

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
        let unit = nav_fn!(project,self.w,row_store_mut!(self),row_seats_mut!(self),
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
        let unit = nav_fn!(project,self.w,row_store_mut!(self),row_seats_mut!(self),
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
        if binding.target.unit(row_store_mut!(self)).is_none() {return false;}
        nav_method!(self.env,clear_above,self.w,row_store_mut!(self),row_seats_mut!(self),from,&binding.target)
    }
    fn follow_far(&mut self) -> f64 {
        self.env.follow_far()
    }
}

impl Swbf2GotoWorld for World<'_> {
    fn crosses(&mut self, a:Vec3,b:Vec3,unit:&NavUnit,m:u8)->bool {
        if m==0 {self.line_of_walk(a,b,unit)} else {super::live::crosses(self.w,a,b,m)}
    }
    fn try_crosses(&mut self, a:Vec3,b:Vec3,_unit:&NavUnit,m:u8,kind:crate::ai::nav::swbf2::ProbeKind)->Option<bool> {
        if m==0 {self.env.try_line_of_walk(self.w,a,b,kind)} else {super::live::try_crosses(self.w,a,b,m,kind)}
    }
    fn observe_navigation(&mut self, nav:&mut crate::ai::nav::route::MapNavigation)->bool {super::live::observe_navigation(self.w,nav)}
    fn objective_approach_radius(&self, unit: &NavUnit, point: Vec3) -> Option<f64> {
        let store = &self.w.sim.store;
        let binding = self.actors.get(Actor(unit.id).reference())?;
        let character = store.character.get(binding.target.unit(store)?.character?)?;
        let crate::gen::PostRef::Post(reference) = character.objective else { return None };
        let post = store.post.get(reference)?;
        let (center, half) = post.capture.as_ref().map_or((post.position, [3.; 3]), |c| (c.center, c.half));
        if (point.y - center[1]).abs() > 0.01 || (point.x - center[0]).abs() > half[0] + 0.01
            || (point.z - center[2]).abs() > half[2] + 0.01 { return None; }
        if post.capture.is_some() {
            Some(0.5 * unit.soldier?.max_speed?)
        } else {
            Some(v8math::max(10., post.radius.unwrap_or(0.) + super::super::nav::units::traversal_radius(unit) + 5.))
        }
    }
    fn step_floor(&mut self, point: Vec3) -> Option<Vec3> {
        super::walk::support(self.w,point,0.65,0.75)
    }
    fn combat_floor(&mut self, point: Vec3) -> Option<Vec3> {
        super::walk::support(self.w, point, 0.45, 2.5)
    }
    fn sees_approach(&mut self, at: Vec3, target: Vec3) -> bool {
        super::walk::sees_approach(self.w, at, target)
    }
    fn approach_floor(&mut self, point: Vec3) -> Option<Vec3> {
        self.foot(point).or_else(|| super::walk::support(self.w, point, 8., 8.))
    }
    fn foot(&mut self, p: Vec3) -> Option<Vec3> { super::walk::support(self.w, p, 0.32, 1.25) }
    fn as_goto_world_mut(&mut self) -> &mut dyn GotoWorld {
        self
    }
    fn line_of_walk(&mut self, from: Vec3, to: Vec3, _unit: &NavUnit) -> bool {
        self.env.line_of_walk(self.w, from, to)
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
     w:&mut crate::world::owner::World,
    binding: &Binding,
    class: &ClassDef,
    actor: Actor,
    seat: Seat,
    env: &mut dyn Environment,
) -> Option<NavUnit> {
    if let Some(turret)=seat.turret {
        valid_turret_lease(nav_store_mut!(w,store),&binding.target,seat)?;
        let turret=nav_seats_mut!(w,seats).crews.turrets.iter_mut().find(|side|side.turret==turret&&side.lease==Some(seat.lease)&&side.alive)?;
        let mut out=NavUnit::new(actor.0,7.,Vec3::new(turret.desc.position.x,turret.desc.position.y,turret.desc.position.z));
        out.generation=Some(1.);out.control_identity=binding.identity;out.class_def=Some(class.clone());
        out.soldier=Some(SoldierView {aim_yaw:Some(turret.desc.facing+turret.yaw),aim_pitch:Some(turret.pitch),max_speed:Some(0.),max_strafe:Some(0.),max_turn:Some(seat.turn),max_pitch:Some(seat.pitch),posture:Posture::Stand,air:Air::None,..Default::default()});
        let origin=turret.origin();let forward=turret.view_direction(turret.yaw,turret.pitch);
        out.origin=Some(Vec3::new(origin.x,origin.y,origin.z));out.forward=Some(Vec3::new(forward.x,forward.y,forward.z));return Some(out);
    }
    valid_seat(nav_store_mut!(w,store),&binding.target,seat)?;
    let frame = nav_method!(env,seat_frame,w,nav_store_mut!(w,store),nav_seats_mut!(w,seats),
        HullLife {
            vehicle: Ref::new(binding.target.slot, binding.target.generation),
            life: binding.target.life,
        },
        seat.index,
    )?;
    let h=valid_seat(nav_store_mut!(w,store),&binding.target,seat)?;
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

#[cfg(all(test, target_pointer_width = "64"))]
#[test]
fn classic_navigation_storage() {
    assert_eq!(std::mem::size_of::<HelperRow>(), 416);
    assert_eq!(std::mem::size_of::<Nav>(), 1176);
}
