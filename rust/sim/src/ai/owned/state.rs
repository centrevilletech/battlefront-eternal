//! Battle-local AI authorities. Bodies, Characters, buildings, hulls and
//! leases stay in their owners; this state retains identities and AI memory.
use super::{agents::Agents,battle::Bindings,buildings::Buildings,buffers::Buffers,classes::Classes,
    combat::Units as CombatUnits,controllers::Controllers,controls::HullControls,entities::{Entities,Entity},
    events::{Journal,Reader},hints::Hints,items::Items,lod::Lod,mounts::Mounts,navigation::{Actor,Navigation},
    population::Population,posts::Posts,registry::Queries,roster::{CharacterOwner,Roster},scene::Scene,
    seats::Controls,services::Candidates,sight::Sight,units::Units,weapons::Definitions};
use crate::{ai::{combat::world::Obj,nav::Nav},battle::{live_phase::Building,ownership::Rows},
    gen::Store,vehicles::f2_seats::Seats};

#[derive(Clone,Copy)]
pub struct Capacity {
    pub registry:usize,pub characters:usize,pub posts:usize,pub hulls:usize,
    pub buildings:usize,pub controls:usize,pub scopes:usize,pub cameras:usize,
}
pub struct State {
    pub entities:Entities,
    pub units:Units,
    pub combat_units:CombatUnits,
    pub forward_cache:super::observe::ForwardCache,
    pub controllers:Controllers,
    pub agents:Agents,
    pub roster:Roster,
    pub population:Population,
    pub posts:Posts,
    pub bindings:Bindings,
    pub controls:HullControls,
    pub births:Controls,
    pub control_views:super::control_views::Publication,
    pub mounts:Mounts,
    pub turrets:super::turrets::Turrets,
    pub buildings:Buildings,
    pub navigation:Navigation,
    /// Physical controller slots, not monotonically increasing Core numbers.
    pub actors:Vec<Option<Actor>>,
    pub hints:Option<Hints>,
    pub jets:super::jets::Jets,
    pub lod:Lod,
    pub reticle:crate::gen::AiPhaseReticle,
    pub buffers:Buffers,
    pub queries:Queries,
    pub candidates:Candidates,
    pub weapons:Definitions,
    pub classes:Classes,
    pub items:Items,
    pub sight:Sight,
    pub journal:Journal,
    pub reader:Reader,
    pub dangers:super::danger::Dangers,
    object_actors:Vec<Option<(Obj,Actor)>>,
    nav_classes:Vec<Option<usize>>,
    hull_controls:Vec<Option<(crate::store::Ref<crate::gen::Vehicle>,Vec<usize>)>>,
    scopes:usize,
    stationary_navigation:usize,
}
// A body's lease view needs only these two retained control owners, rather
// than building a complete scene with unrelated navigation/prop references.
impl super::combat::ControlOwners for State {
    fn controller(&self,control:crate::store::Ref<crate::gen::SeatUnit>)->Option<Obj> {
        self.controls.controller(control).or_else(||self.turrets.controller(control))
    }
}
impl State {
    pub fn new(nav:Nav,enhanced:bool,capacity:Capacity)->Self {
        let c=capacity;let hints=nav.hints.list.len();
        let identities=c.registry+c.characters*3+c.controls*2+c.posts+c.buildings*3+hints+c.scopes;
        let journal=Journal::new(4096);let reader=journal.reader();
        let mut navigation=Navigation::new(nav,c.registry+c.controls+c.characters);
        let stationary_navigation=navigation.class(crate::ai::nav::units::ClassDef {ai_size_type:crate::ai::nav::units::SizeType::Number(1.),max_speed:Some(0.),forward_speed:Some(0.),max_strafe_speed:Some(0.),..Default::default()});
        Self {
            entities:Entities::new(identities),units:Units::new(),combat_units:CombatUnits::new(),forward_cache:super::observe::ForwardCache::new(identities),
            controllers:Controllers::new(c.characters,c.scopes),agents:Agents::with_scopes(c.characters,c.scopes),
            roster:Roster::new(c.characters),population:Population::new(c.posts,c.characters,enhanced),posts:Posts::new(c.posts),
            bindings:Bindings::new(),controls:HullControls::with_capacity(c.controls),births:Controls::new(c.controls),control_views:super::control_views::Publication::new(c.controls),
            mounts:Mounts::new(c.hulls,c.scopes),turrets:super::turrets::Turrets::new(c.buildings,c.controls),buildings:Buildings::new(c.buildings),
            navigation,actors:vec![None;c.characters],hints:None,jets:super::jets::Jets::new(identities),
            lod:Lod::new(c.cameras),reticle:Default::default(),buffers:Buffers::new(c.registry,c.posts,c.characters,hints,c.scopes),
            queries:Queries::new(c.registry,c.scopes),candidates:Candidates::new(c.registry+c.buildings,c.buildings,c.scopes),
            weapons:Definitions::new(),classes:Classes::new(),items:Items::new(),sight:Sight::new(identities),journal,reader,dangers:Default::default(),
            object_actors:vec![None;identities],nav_classes:Vec::new(),hull_controls:Vec::new(),scopes:c.scopes,stationary_navigation,
        }
    }
    /// First-use publication tables reserve one entry per native identity,
    /// including future controller, agent, control and hint births.
    pub fn identity_capacity(&self)->usize {self.object_actors.len()}
    /// Capture camera/reticle facts once at the AI phase boundary. Current
    /// canonical scalar and geometry reads remain with their entity owners.
    pub fn phase_facts(&mut self,facts:&crate::gen::AiPhaseFacts,cameras:&[crate::gen::AiPhaseCamera]) {
        self.lod.update(cameras,facts.network_views,facts.lod_disabled);self.reticle=facts.reticle.clone();
    }
    pub fn unit_class(&mut self,row:&crate::gen::AiUnitClass,strings:&crate::store::Strings)->usize {
        let class=super::content::UnitClass::read(row,strings);
        self.queries.class(row.def,crate::ai::control::goals::travel_mask_of(&class.policy.ai_size_type));
        let index=class.install(&mut self.units,&mut self.combat_units,&mut self.navigation);
        let at=row.def.0 as usize;if self.nav_classes.len()<=at {self.nav_classes.resize(at+1,None);}
        self.nav_classes[at]=Some(index);index
    }
    /// Cooked SeatUnit classes differ from their hull's registry class. The
    /// loader resolves both policies once, before any controller can board.
    pub fn control_class(&mut self,row:&crate::gen::AiUnitClass,strings:&crate::store::Strings,kind:f64)->usize {
        let class=super::content::UnitClass::read(row,strings);
        let navigation=self.navigation.class(class.navigation);
        let index=self.controls.class(kind,class.policy,class.combat);
        self.controls.prepare_navigation(index,navigation);
        self.controls.prepare_factory(index,class.vehicle,class.name);index
    }
    /// Hull construction prepares one cooked class per physical seat. A
    /// promoted pilot reads its new index, while a gunner keeps zero speed.
    pub fn prepare_hull_controls(&mut self,hull:crate::store::Ref<crate::gen::Vehicle>,classes:Vec<usize>) {
        let slot=hull.slot as usize;if self.hull_controls.len()<=slot {self.hull_controls.resize_with(slot+1,||None);}
        self.hull_controls[slot]=Some((hull,classes));
    }
    /// F2 has committed the lease and holder before requesting controllable.
    /// Install that same reference before the cooked views validate it; the
    /// caller's following assignment writes the identical reference again.
    pub fn construct_control(&mut self,store:&mut Store,seats:&mut Seats,buildings:&Rows<Building>,lease:crate::store::Ref<crate::gen::SeatLease>)->Option<crate::store::Ref<crate::gen::SeatUnit>> {
        let row=store.seat_lease.get(lease)?;let previous=row.controllable;
        let occupant=super::seats::Occupant::read(store,lease)?;
        let (class,turret_life)=if let Some(hull)=row.vehicle {
            let (prepared,classes)=self.hull_controls.get(hull.slot as usize)?.as_ref()?;
            if *prepared!=hull||row.index<0.||row.index.fract()!=0. {return None;}
            (Some(*classes.get(row.index as usize)?),None)
        }else{(None,Some(self.turrets.life(seats,buildings,row.turret?)?))};
        let control=self.births.construct(store,seats,&mut self.entities,lease,turret_life)?;
        store.seat_lease.get_mut(lease).unwrap().controllable=Some(control);
        let valid=if let Some(class)=class {self.controls.construct(store,&mut self.entities,control,lease,class,occupant)}else{
            let turret=store.seat_lease.get(lease).unwrap().turret.unwrap();
            self.turrets.bind_control(store,seats,buildings,&mut self.entities,control,lease,turret,None,occupant)
        };
        if valid {
            #[cfg(target_arch="wasm32")]
            crate::gen::AiControlBirth {slot:control.slot,generation:control.gen}.emit(&mut crate::abi::sim().events);
            Some(control)
        }else{
            store.seat_lease.get_mut(lease).unwrap().controllable=previous;
            self.births.retire(seats,&mut self.entities,control);None
        }
    }
    pub fn controller_control(&mut self,store:&Store,seats:&Seats,buildings:&Rows<Building>,controller:usize,obj:Obj)->Option<Actor> {
        let Entity::Control(reference)=self.entities.get(obj)? else{return None};
        if self.controls.navigation(store,reference).is_some() {return self.controller_hull(store,controller,obj);}
        let (target,lease,view)=self.turrets.navigation(store,seats,buildings,reference)?;
        let class=self.stationary_navigation;
        let actor=if let Some(actor)=self.actor(controller) {actor}else{
            let actor=self.navigation.register_seat(store,target.clone(),lease,reference,7.,class,&view)?;
            self.set_actor(controller,Some(actor));actor
        };
        self.navigation.attach_seat(store,actor,target,lease,reference,7.,class,&view,obj).then_some(actor)
    }
    pub fn controller_hull(&mut self,store:&Store,controller:usize,obj:Obj)->Option<Actor> {
        let Entity::Control(reference)=self.entities.get(obj)? else{return None};
        let (target,lease,kind,class,view)=self.controls.navigation(store,reference)?;
        let actor=if let Some(actor)=self.actor(controller) {actor}else{
            let actor=self.navigation.register_seat(store,target.clone(),lease,reference,kind,class,&view)?;
            self.set_actor(controller,Some(actor));actor
        };
        self.navigation.attach_seat(store,actor,target,lease,reference,kind,class,&view,obj).then_some(actor)
    }
    /// Static repair/registry targets prepare their class once. Their live
    /// position, health and target generation stay in F2/F4 at follow time.
    pub fn prepare_follow_target(&mut self,obj:Obj,source:super::navigation::FollowSource,policy:&crate::ai::control::world::AiUnit) {
        let class=self.navigation.class(policy.nav_unit().class_def.unwrap_or_default());
        let actor=self.navigation.register_follow_target(super::navigation::FollowTarget {source,kind:policy.kind},class,policy.generation);
        self.object_actors[obj as u32 as usize]=Some((obj,actor));
    }
    pub fn follow_actor(&mut self,store:&Store,seats:&Seats,buildings:&Rows<Building>,obj:Obj)->Option<Actor> {
        if matches!(self.entities.get(obj),Some(Entity::Unit(_))) {return self.body_actor(store,obj,false);}
        let at=obj as u32 as usize;
        if let Some((current,actor))=self.object_actors.get(at).copied().flatten() {
            if current==obj {return Some(actor);}
            self.navigation.retire(actor);
        }
        let Entity::Control(reference)=self.entities.get(obj)? else{return None};
        let actor=if let Some((target,lease,kind,class,view))=self.controls.navigation(store,reference) {
            self.navigation.register_seat_target(store,target,lease,reference,kind,class,&view)?
        }else{
            let (target,lease,view)=self.turrets.navigation(store,seats,buildings,reference)?;
            self.navigation.register_seat_target(store,target,lease,reference,7.,self.stationary_navigation,&view)?
        };
        self.object_actors[at]=Some((obj,actor));Some(actor)
    }
    /// A followed human needs only a qualified target. A controller can
    /// later activate that same identity without changing saved follow refs.
    pub fn body_actor(&mut self,store:&Store,obj:Obj,active:bool)->Option<Actor> {
        let Entity::Unit(reference)=self.entities.get(obj)? else {return None};
        let unit=store.unit.get(reference)?;
        let class=self.nav_classes.get(unit.class_def.0 as usize).copied().flatten()?;
        let target=crate::gen::AiLifeHandle {kind:crate::gen::KIND_UNIT.into(),slot:reference.slot,generation:reference.gen,life:unit.generation};
        let at=obj as u32 as usize;
        if let Some((current,actor))=self.object_actors[at] {
            if current==obj&&self.navigation.refresh(store,actor,target.clone(),class) {
                if active&&!self.navigation.activate(store,actor) {return None;}
                return Some(actor);
            }
            self.navigation.retire(actor);
        }
        let actor=if active {self.navigation.register(store,target,class)?}else{self.navigation.register_target(store,target,class)?};
        self.object_actors[at]=Some((obj,actor));Some(actor)
    }
    /// A controller owns one navigation control through respawn and class
    /// changes. Its passive target identity belongs to the body instead.
    pub fn controller_body(&mut self,store:&Store,controller:usize,obj:Obj)->Option<Actor> {
        let Entity::Unit(reference)=self.entities.get(obj)? else {return None};
        let unit=store.unit.get(reference)?;
        let class=self.nav_classes.get(unit.class_def.0 as usize).copied().flatten()?;
        let target=crate::gen::AiLifeHandle {kind:crate::gen::KIND_UNIT.into(),slot:reference.slot,generation:reference.gen,life:unit.generation};
        if let Some(actor)=self.actor(controller) {
            return self.navigation.attach_body(store,actor,target,class,obj).then_some(actor);
        }
        let actor=self.navigation.register_body(store,target,class,obj)?;
        self.set_actor(controller,Some(actor));Some(actor)
    }
    /// Resolve authored hints after posts have acquired their native keys.
    /// All hints, including siblings outside a cover query's cap, get keys.
    pub fn prepare_hints(&mut self,post:impl Fn(u64)->Option<usize>) {
        assert!(self.hints.is_none(),"prepare native hints once per battle");
        for i in 0..self.navigation.nav.hints.list.len() {self.entities.register(Entity::Hint(i as u32));}
        let entities=&self.entities;
        self.hints=Some(Hints::new(&self.navigation.nav,self.scopes,post,
            |i|entities.find(Entity::Hint(i as u32)).unwrap()));
    }
    pub fn scene<'a>(&'a self,store:&'a Store,seats:&'a Seats,buildings:&'a Rows<Building>,characters:&'a dyn CharacterOwner)->Scene<'a> {
        Scene {store,seats,buildings,traps:None,characters,roster:&self.roster,entities:&self.entities,
            units:&self.units,combat_units:&self.combat_units,controls:&self.controls,mounts:&self.mounts,
            turrets:&self.turrets,props:&self.buildings}
    }
    pub fn intent(&self,store:&mut Store,seats:&mut Seats,buildings:&Rows<Building>,unit:Obj,command:super::intent::Command)->bool {
        match self.entities.get(unit) {
            Some(Entity::Unit(_))=>self.units.command(store,&self.entities,unit,command),
            Some(Entity::Control(r))=>self.controls.command(store,seats,r,command)
                ||self.turrets.command(store,seats,buildings,r,command),
            _=>false,
        }
    }
    /// Vehicle steering reads AI state from the same retained controller as
    /// combat. Prepared physical controls do not imply JS navControl presence.
    pub fn vehicle_controller(&self,store:&Store,key:crate::ai::combat::world::Obj)->Option<crate::ai::vehicles::world::ControllerView> {
        let core=self.controllers.core(&self.entities,key)?;
        let row=self.controllers.read(&self.entities,key)?;
        let (kind,has_path)=self.controllers.helper(&self.entities,key)?;
        // VehicleAI.analogs reads helper.hasPath, which GroundGotoHelper and
        // FlyGotoHelper omit. Their executor is observed separately above.
        let has_path=has_path&&matches!(kind,Some(crate::ai::control::world::HelperKind::Direct|crate::ai::control::world::HelperKind::Stationary|crate::ai::control::world::HelperKind::Vehicle));
        let actor=self.actor(core);
        let nav=actor.and_then(|a|self.navigation.observation(store,a)).filter(|_|row.nav_stance.is_some());
        let detail=actor.and_then(|a|self.navigation.helper_detail(store,a)).filter(|_|matches!(kind,Some(crate::ai::control::world::HelperKind::Ground|crate::ai::control::world::HelperKind::Fly|crate::ai::control::world::HelperKind::Vehicle)));
        Some(crate::ai::vehicles::world::ControllerView {obj:key,unit:row.unit,
            nav_control:row.nav_stance.is_some(),executor:nav.is_some_and(|n|n.has_executor),
            agent:self.agents.key(core),executor_status:nav.map_or(0.,|n|n.status),target:row.target.is_some(),
            helper:kind.is_some(),helper_has_path:has_path,
            helper_state:detail.and_then(|d|d.0),helper_destination:detail.and_then(|d|d.1).map(|p|three::Vector3::new(p.x,p.y,p.z)),
            is_unit_controller:true,fire_channel:row.perception.as_ref().map(|p|p.fire.channel)})
    }
    /// First bind prepares the controller before any combat/perception hook.
    /// Body publication is sparse; a duplicate bind does not dirty its row.
    pub fn link_controller<C:super::weapons::Changes+?Sized>(&mut self,store:&mut Store,owner:&mut C,unit:Obj,controller:usize,bound:bool)->bool {
        let key=self.controllers.adopt(controller,&mut self.entities);
        match self.entities.get(unit) {
            Some(Entity::Unit(reference))=>{
                let Some(changed)=super::controllers::link_unit(store,&self.entities,unit,key,bound) else {return false};
                if changed {owner.native_intent(reference,store.unit.get(reference).unwrap().controller.is_some());}
                true
            },
            Some(Entity::Control(reference))=>self.controls.link_controller(reference,key,bound)
                .or_else(||self.turrets.link_controller(reference,key,bound)).is_some(),
            _=>false,
        }
    }
    pub fn link_character(&mut self,store:&mut Store,owner:&mut dyn CharacterOwner,index:usize,controller:usize,bound:bool)->bool {
        let key=if bound {self.controllers.adopt(controller,&mut self.entities)}else{
            let Some(key)=self.controllers.key(controller) else{return false};key
        };
        let found=super::controllers::link_character(store,owner,&self.roster,&self.entities,index,key,bound).is_some();
        if !bound {if let Some(actor)=self.actor(controller) {self.navigation.retire(actor);}self.set_actor(controller,None);self.controllers.retire(&mut self.entities,key);}
        found
    }
    pub fn actor(&self,core:usize)->Option<Actor> {
        let Entity::Controller(reference)=self.entities.get(self.controllers.key(core)?)? else{return None};
        self.actors.get(reference.slot as usize).copied().flatten()
    }
    pub fn set_actor(&mut self,core:usize,actor:Option<Actor>) {
        let Entity::Controller(reference)=self.entities.get(self.controllers.key(core).expect("current native controller")).unwrap() else{unreachable!()};
        self.actors[reference.slot as usize]=actor;
    }
    /// Agent construction takes ownership of the exact helper object. A
    /// replacement may leave the old row pinned by a running combat stack.
    pub fn bind_agent_helper(&mut self,agent:crate::ai::combat::world::Obj,controller:usize) {
        let kind=self.agents.get(&self.entities,agent).expect("new native helper owner").helper.kind;
        let helper=if matches!(kind,Some(crate::ai::control::world::HelperKind::Ground|crate::ai::control::world::HelperKind::Fly|crate::ai::control::world::HelperKind::Vehicle)) {self.actor(controller).and_then(|a|self.navigation.helper_key(a))}else{None};
        let row=self.agents.get_mut(&self.entities,agent).expect("new native helper owner");
        if row.navigation==helper {return;}
        if let Some(old)=row.navigation.take() {self.navigation.release_helper(old);}
        if let Some(helper)=helper {assert!(self.navigation.retain_helper(helper));row.navigation=Some(helper);}
    }
    pub fn reap_helpers(&mut self) {
        while let Some(helper)=self.agents.released_helper() {self.navigation.release_helper(helper);}
    }
    pub fn agent_disposed(&mut self,controller:usize) {
        if let Some(key)=self.agents.key(controller) {self.agents.retire(&mut self.entities,key);}self.reap_helpers();
    }
}

impl State {
    /// botLockTarget and seatLockTarget observe the current controller's
    /// target and gameplay generation without eagerly querying shapes or life.
    /// Body/hull callers freeze this result before movement; turrets read it
    /// at their later lock point, after preceding synchronous death hooks.
    pub fn weapon_hint(&self,store:&Store,unit:crate::store::Ref<crate::gen::Unit>,control:Option<crate::store::Ref<crate::gen::SeatUnit>>,
        target_life:impl Fn(crate::gen::Member)->Option<f64>)->crate::gen::F2TurretHint {
        let Some(body)=store.unit.get(unit) else{return Default::default()};
        if body.human||body.network_human==Some(true)||body.combat.player {return Default::default();}
        let controller=control.and_then(|control| {
            let birth=self.births.read(control)?;let lease=store.seat_lease.get(birth.lease)?;
            if lease.released||lease.unit!=Some(unit)||lease.controllable!=Some(control)||lease.generation!=body.generation {return None;}
            self.controls.controller_key(control).or_else(||self.turrets.controller_key(control))
        });
        // Bodies already retain the controller slab's qualified reference.
        // A seat supplies its separate captured controller identity instead.
        let controller=match controller {
            Some(controller)=>self.controllers.read(&self.entities,controller),
            None=>body.controller.and_then(|reference|self.controllers.read_reference(reference)),
        };
        let target=controller.and_then(|controller|controller.perception.as_ref()).and_then(|perception|perception.fire.weapon_target);
        let mut hint=crate::gen::F2TurretHint {bot:true,..Default::default()};
        if let Some(target)=target {
            if let Some(member)=self.queries.member(&self.entities,target.obj) {
                let life=match member {
                    crate::gen::Member::Unit(reference)=>store.unit.get(reference).map(|body|body.generation),
                    _=>target_life(member),
                };
                if life==Some(target.generation) {hint.target=Some(member);hint.life=target.generation;}
            }
        }
        hint
    }
    /// UnitController.bind/unbind changes which retained SeatIntent supplies
    /// latchCrews. A human's physical body intent remains a separate source.
    pub fn seat_driven(&self,seats:&mut Seats,unit:Obj) {
        let Some(Entity::Control(control))=self.entities.get(unit) else{return};
        let driven=self.controls.controller_key(control).or_else(||self.turrets.controller_key(control)).is_some();
        let key=crate::seats::lease::control_key(control);
        if let Some(row)=seats.hulls.controls.get_mut(&key) {row.driven=driven;}
        if let Some(row)=seats.crews.controls.get_mut(&key) {row.driven=driven;}
    }
}
