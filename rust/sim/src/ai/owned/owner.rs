//! The simulation instance owns one native AI battle. Content is prepared
//! before activation; phase scopes borrow this retained state and canonical
//! entity owners, rather than constructing another per-method AI service.
use super::{frame::Settings,process::Process,runtime::Runtime,state::{Capacity,State},vehicles::{Authority,Binding}};
use crate::{ai::{combat::{binding::CombatAi,enhanced::EnhancedCombat,weapons_ai::Registered},
    control::{self,difficulty::{Difficulty,DifficultyOptions,MissionCall},goals::mission_controls,world::{Bind,TeamTable}},
    nav::Nav,phase::RandomState},combat::defs::WeaponClass,gen::{AiColdSettings,AiPhaseCounters},store::{DefId,Strings}};

#[derive(Default)]
pub struct Owner {process:Process,battle:Option<Box<Battle>>}
pub struct Battle {
    pub state:State,
    pub runtime:Runtime,
    pub combat:CombatAi,
    pub settings:Settings,
    pub vehicles:Binding,
    pub counters:AiPhaseCounters,
    pub voices:super::publication::Voices,
    pub item_owners:super::publication::ItemOwners,
    pub facts:super::world::Facts,
    pub stream_a:u32,
    pub stream_b:u32,
    pub exchange:super::exchange::Exchange,
    pub navigation_posts:Vec<crate::gen::SpawnPost>,
    pub initial_naming:Option<crate::battle::spawning::Naming>,
    pub initial_character:[f64;crate::gen::AiCharacterBirth::WORDS],
    pub initial_prop:[f64;crate::gen::F4Handle::WORDS],
    ready:bool,
    registry:usize,characters:usize,support_keys:usize,hulls:usize,scopes:usize,
}
impl Owner {
    /// createClassicAI/createEnhancedAI starts after the previous battle's
    /// last callback. Process caches move across that boundary exactly once.
    pub fn begin(&mut self,nav:Nav,input:&AiColdSettings,strings:&mut Strings,teams:TeamTable)->&mut Battle {
        assert!(self.battle.is_none(),"finish the current native AI battle before loading another");
        let capacity=&input.capacity;
        assert!(capacity.scopes>0,"native AI needs a reserved service scope");
        let c=Capacity {registry:capacity.registry as usize,characters:capacity.characters as usize,posts:capacity.posts as usize,
            hulls:capacity.hulls as usize,buildings:capacity.buildings as usize,controls:capacity.controls as usize,
            scopes:capacity.scopes as usize,cameras:capacity.cameras as usize};
        let calls:Vec<MissionCall>=input.mission.iter().map(|call|MissionCall {
            name:String::from_utf16_lossy(strings.units(call.name)),args:call.args.clone(),
            str_args:call.strings.iter().map(|id|String::from_utf16_lossy(strings.units(*id))).collect(),
        }).collect();
        let controls=mission_controls(&calls);
        let mut difficulty=if input.enhanced {Difficulty::enhanced(input.tier,input.player_team,input.network)}else{
            Difficulty::classic(DifficultyOptions {tier:input.tier,profile_tier:input.profile_tier,player_team:input.player_team,
                second_player_team:input.second_player_team,network:input.network,override_:input.skill_override,skills:(input.player_skill,input.enemy_skill)})
        };
        difficulty.apply(&calls);
        let b=&input.capabilities;
        let bind=Bind {nav:b.navigation,combat:b.combat,combat_agent:b.combat_agent,population:b.population,vehicles:b.vehicles,
            classes:b.classes,random_b:b.random_b,enhanced:input.enhanced,mine_ground:b.mine_ground,vehicle_adapter:b.vehicle_adapter};
        let dense=if controls.dense_explicit {controls.dense_value}else{input.dense};
        let vehicle_settings=crate::ai::vehicles::world::Settings {
            min_fly_height:controls.fly_min.unwrap_or(-200.),max_fly_height:controls.fly_max.unwrap_or(200.),
            notify_radius:controls.vehicle_notify_radius,dense,stay_in_turrets:controls.stay_in_turrets,cycle:input.hard.map_or_else(||difficulty.cycle(),|hard|if hard {3.}else{15.}),
        };
        let settings=Settings {difficulty:std::cell::RefCell::new(difficulty.clone()),mission_stay_in_turrets:input.mission_stay_in_turrets,
            mission_no_jet_obstacle_test:input.no_jet_obstacle_test,ai_stay_in_turrets:controls.stay_in_turrets,
            vehicles:bind.vehicles,dense,navigation:bind.nav};
        let options=control::Options {bind,fallback_sensor_range:input.sensor_range,controls:Some(controls),
            follow_near:input.follow_near,follow_far:input.follow_far,hard:input.hard,lod_disabled:input.lod_disabled};
        let mut core=control::Core::new(options,teams,difficulty,c.characters);
        core.prepare(c.posts,c.registry,capacity.repair_jobs as usize);
        let (shared,vehicles)=self.process.begin();
        let random=match input.streams.as_ref() {Some(states)=>RandomState::from_stream_states(input.seed,[states.a,states.b,states.c],b.random_b),None=>RandomState::with_random_b(input.seed,b.random_b)};
        // classicBinding.consumeFire samples the spawn direction until the
        // first post-movement aim hook runs. Starting hooked skips that first
        // sample and changes the filter's direction on the very first body.
        let combat=CombatAi {shared,enhanced:input.enhanced.then(||EnhancedCombat::new(input.tier)),..Default::default()};
        let mut state=State::new(nav,input.enhanced,c);
        state.dangers.reserve(capacity.grenades as usize);
        let item_owners=super::publication::ItemOwners::new(state.identity_capacity());
        let voices=super::publication::Voices::new(strings);
        // Owner loans cross every physical/publication barrier. Keep the
        // retained battle at one cold address rather than moving its large
        // inline state through each empty module shell on every loan.
        self.battle=Some(Box::new(Battle {
            voices,item_owners,facts:Default::default(),stream_a:input.stream_a,stream_b:input.stream_b,
            exchange:super::exchange::Exchange::new(c.cameras,4096),navigation_posts:Vec::new(),initial_naming:None,initial_character:[0.;crate::gen::AiCharacterBirth::WORDS],initial_prop:[0.;crate::gen::F4Handle::WORDS],
            state,runtime:Runtime::new(core,random),combat,settings,
            vehicles:Binding {authority:Authority::new(vehicles),settings:vehicle_settings,navigation:bind.nav,population:bind.population},
            counters:Default::default(),ready:false,registry:c.registry,characters:c.characters,support_keys:capacity.support_keys as usize,hulls:c.hulls,scopes:c.scopes,
        }));
        self.battle.as_mut().unwrap()
    }
    pub fn battle(&self)->Option<&Battle> {self.battle.as_deref()}
    pub fn battle_mut(&mut self)->Option<&mut Battle> {self.battle.as_deref_mut()}
    /// Call only after the last active frame has returned. Dropping local
    /// agents first releases their shared weapon and vehicle references.
    pub fn finish(&mut self) {
        let Some(battle)=self.battle.take() else{return};
        let Battle {combat,vehicles,..}=*battle;
        self.process.end(combat.shared,vehicles.authority.finish());
    }
}
impl Battle {
    /// registerAIData retains insertion order across battles. A later class
    /// preparation uses the resulting policy rather than looking up names hot.
    pub fn weapon_records(&mut self,records:&[(String,Registered)]) {
        assert!(!self.ready,"register AI weapon records before phase activation");self.combat.shared.register(records);
    }
    pub fn weapon_json(&mut self,bytes:&[u8])->Result<(),String> {
        let records=super::load::weapon_records(bytes)?;self.weapon_records(&records);Ok(())
    }
    pub fn weapon_class(&mut self,id:DefId,class:&WeaponClass) {
        assert!(!self.ready,"prepare AI weapon classes before phase activation");
        self.state.weapons.register(id,class);self.state.classes.weapon(id,class);self.combat.shared.prepare_class(class);
    }
    /// Reserve all policy/query caches, including the first Enhanced skill
    /// observation. This must not draw C or create a Character's aimer early.
    pub fn activate(&mut self) {
        assert!(!self.ready,"activate native AI once per battle");
        // Reserved Character capacity can exceed the current roster. Sync
        // visits real rows only; future admissions extend this count cold.
        self.runtime.core.character_count=self.state.roster.handles().len();
        let hints=self.state.navigation.nav.hints.list.len();
        self.combat.shared.begin(self.registry+self.characters);
        self.combat.reservations.prepare(hints);
        if let Some(enhanced)=self.combat.enhanced.as_mut() {enhanced.skills.reserve(self.characters);}
        use crate::ai::vehicles::service::VehicleStorage;
        let posts=self.state.population.len();let hulls=self.hulls;
        // supportHubs scans authored navigation hubs, independently of the
        // mixed registry. Large maps must not grow this scratch on first use.
        let support_hubs=self.state.navigation.nav.graph.as_ref().map_or(0,|graph|graph.hubs.len());
        self.vehicles.authority.write(|vehicles| {
            vehicles.prepare_pads(hulls);
            // Reservations have two kinds per candidate/owner pair. Their
            // native weak-reference reads are scalar and cannot invoke Core;
            // two snapshots cover the retained result and its next refresh.
            vehicles.prepare_reservations(self.registry.saturating_mul(self.characters).saturating_mul(2),2);
            vehicles.prepare_support(posts,self.support_keys,support_hubs,self.scopes);
        });
        self.ready=true;
    }
    /// Scheduler work is gameplay-visible LOD, so report its real counters
    /// beside boundary traffic instead of inferring work from phase calls.
    fn phase_counters(&mut self,dt:f64) {
        self.counters.phases=self.counters.phases.wrapping_add(1);
        self.counters.live_controllers=self.runtime.core.live.len() as u32;
        // Whole-phase diagnostics prove that the owned perception/target/fire
        // lanes performed work without exporting per-controller snapshots.
        self.counters.targets=0;self.counters.combat_targets=0;self.counters.fire_requests=0;
        self.counters.candidates=0;self.counters.trackers=0;self.counters.scanned=0.;
        for &index in &self.runtime.core.live {
            let core=&self.runtime.core.controllers[index];
            self.counters.targets+=u32::from(core.target.is_some());
            self.counters.fire_requests+=u32::from(core.fire_a||core.fire_b);
            if let Some(row)=self.state.controllers.key(index).and_then(|key|self.state.controllers.read(&self.state.entities,key)) {
                self.counters.combat_targets+=u32::from(row.target.is_some());
                if let Some(perception)=&row.perception {
                    self.counters.candidates+=u32::from(perception.candidate.is_some());self.counters.scanned+=perception.last_scan_count;
                }
            }
            if let Some(agent)=self.state.agents.key(index).and_then(|key|self.state.agents.get(&self.state.entities,key)) {self.counters.trackers+=u32::from(agent.tracker().target.is_some());}
        }
        self.counters.considered+=self.runtime.core.scheduler.stats.considered;
        self.counters.serviced+=self.runtime.core.scheduler.stats.serviced;
        self.counters.skipped+=self.runtime.core.scheduler.stats.skipped;
        self.counters.scheduler_ticks=self.runtime.core.scheduler.counter;
        self.counters.step_dt=dt;self.counters.time=self.facts.time;
    }

    pub fn ready(&self)->bool {self.ready}
}

/// The phase borrows each canonical owner once. Geometry and lifecycle
/// services may loan them onward only after the current row read ends.
pub struct Owners<'a,C:super::weapons::Changes+?Sized,M:super::arsenals::Mounted> {
    pub store:&'a mut crate::gen::Store,
    pub seats:&'a mut crate::vehicles::f2_seats::Seats,
    pub buildings:&'a dyn super::rows::BuildingOwner,
    pub characters:&'a mut dyn super::roster::CharacterOwner,
    pub changes:&'a mut C,
    pub mounted:&'a mut M,
    pub definitions:&'a crate::combat::defs::WeaponDefs,
}
impl Battle {
    pub fn with_world<C:super::weapons::Changes+?Sized,M:super::arsenals::Mounted,S:super::world::Services<C,M>,R>(
        &mut self,owners:Owners<'_,C,M>,services:&mut S,facts:&super::world::Facts,
        run:impl FnOnce(&mut Runtime,&mut super::world::World<'_,'_,C,M,S>)->R)->R {
        assert!(self.ready,"activate native AI content before entering a world scope");
        let mut random=self.runtime.random.share();
        let mut rows=super::rows::Rows {store:owners.store,seats:owners.seats,buildings:owners.buildings,
            characters:owners.characters,state:&mut self.state,changes:owners.changes,mounted:owners.mounted,definitions:owners.definitions};
        let mut world=super::world::World {rows:&mut rows,random:&mut random,combat:&mut self.combat,
            services,settings:&self.settings,facts};
        run(&mut self.runtime,&mut world)
    }
    /// The single AI phase boundary observes cameras once, then runs the
    /// original decide/execute, mounted aim, decay and Enhanced tail order.
    pub fn phase<C:super::weapons::Changes+?Sized,M:super::arsenals::Mounted,S:super::world::Services<C,M>>(
        &mut self,owners:Owners<'_,C,M>,services:&mut S,facts:&super::world::Facts,
        input:&crate::gen::AiPhaseFacts,cameras:&[crate::gen::AiPhaseCamera]) {
        self.state.phase_facts(input,cameras);
        // LOD and controller input observe one boundary. Other callers may
        // retain a Facts value across phases; its old camera flags must not
        // override the freshly captured scheduler input.
        let facts=super::world::Facts {time:facts.time,paused:facts.paused,network_views:input.network_views,
            lod_disabled:input.lod_disabled,network_secondary:facts.network_secondary};
        self.facts=facts;
        self.with_world(owners,services,&facts,|runtime,world|runtime.phase(world,input.dt));
        self.phase_counters(input.dt);
        self.counters.input_words=self.counters.input_words.wrapping_add((crate::gen::AiPhaseFacts::WORDS+cameras.len()*crate::gen::AiPhaseCamera::WORDS) as u32);
        let draws=self.runtime.random.draw_counts();
        self.counters.draws_a=draws[0] as u32;self.counters.draws_b=draws[1] as u32;self.counters.draws_c=draws[2] as u32;
    }
}

#[cfg(target_arch="wasm32")]
impl Battle {
    /// One native scope spans the enclosing unit, seat or battle operation.
    /// Every recursive callback receives this same Core, memory and generators.
    pub fn with_native_callbacks<R>(&mut self,recon:&mut crate::recon::Authority,queries:&mut crate::battle::live::Queries,
        run:impl FnOnce(crate::units::f3::WorldServices<'_>)->R)->R {
        assert!(self.ready,"prepare native AI before its lifecycle callbacks");
        let context=super::host::Context {time:self.facts.time,follow_far:self.runtime.core.follow_radii().1,
            stream_a:self.stream_a,vehicles:self.vehicles.clone(),voice:&self.voices,items:&self.item_owners};
        let mut scope=super::callbacks::Scope {core:&mut self.runtime.core,state:&mut self.state,
            random:&mut self.runtime.random,combat:&mut self.combat,settings:&self.settings,facts:&self.facts,factory:&context};
        run(crate::units::f3::WorldServices {ai:Some(&mut scope),recon,queries})
    }
    /// createAI attaches the completed initial roster in insertion order,
    /// after spawning.begin and service binding. Early spawn hooks must not
    /// consume those draws while the initial roster is still being built.
    pub fn attach_initial(&mut self,store:&mut crate::gen::Store,recon:&mut crate::recon::Authority,queries:&mut crate::battle::live::Queries) {
        assert!(self.ready,"activate native AI before attaching its roster");
        use super::callbacks::Factory;
        let context=super::host::Context {time:self.facts.time,follow_far:self.runtime.core.follow_radii().1,
            stream_a:self.stream_a,vehicles:self.vehicles.clone(),voice:&self.voices,items:&self.item_owners};
        let mut services=context.services(recon,queries);let mut random=self.runtime.random.share();
        super::live::with_rows(&mut self.state,store,|rows| {
            let mut world=super::world::World {rows,random:&mut random,combat:&mut self.combat,services:&mut services,settings:&self.settings,facts:&self.facts};
            for index in 0..self.runtime.core.character_count {self.runtime.attach(&mut world,index);}
        });
    }
    pub fn native_phase(&mut self,store:&mut crate::gen::Store,recon:&mut crate::recon::Authority,queries:&mut crate::battle::live::Queries,
        facts:&super::world::Facts,input:&crate::gen::AiPhaseFacts,cameras:&[crate::gen::AiPhaseCamera],journal:&crate::gen::AiJournalInput)->Result<(),super::events::Invalid> {
        assert!(self.ready,"prepare native AI before its phase");
        self.state.import_journal(journal)?;self.state.phase_facts(input,cameras);
        self.facts=super::world::Facts {time:facts.time,paused:facts.paused,network_views:input.network_views,lod_disabled:input.lod_disabled,network_secondary:facts.network_secondary};
        use super::callbacks::Factory;
        let context=super::host::Context {time:self.facts.time,follow_far:self.runtime.core.follow_radii().1,
            stream_a:self.stream_a,vehicles:self.vehicles.clone(),voice:&self.voices,items:&self.item_owners};
        let mut services=context.services(recon,queries);let mut random=self.runtime.random.share();
        super::live::with_rows(&mut self.state,store,|rows| {
            let mut world=super::world::World {rows,random:&mut random,combat:&mut self.combat,services:&mut services,settings:&self.settings,facts:&self.facts};
            self.runtime.phase(&mut world,input.dt);
        });
        self.phase_counters(input.dt);
        self.counters.input_words=self.counters.input_words.wrapping_add((crate::gen::AiPhaseHeader::WORDS+cameras.len()*crate::gen::AiPhaseCamera::WORDS
            +journal.records.len()*crate::gen::AiJournalEvent::WORDS) as u32);
        let draws=self.runtime.random.draw_counts();
        self.counters.draws_a=draws[0] as u32;self.counters.draws_b=draws[1] as u32;self.counters.draws_c=draws[2] as u32;
        Ok(())
    }
}

#[cfg(target_arch="wasm32")]
impl Battle {
    /// Danger warnings stay after battle and targeting, rather than joining
    /// the earlier decide/execute boundary or the projectile flight callback.
    pub fn native_dangers(&mut self,store:&mut crate::gen::Store,recon:&mut crate::recon::Authority,queries:&mut crate::battle::live::Queries) {
        if !self.runtime.core.options.bind.combat {return;}
        use super::callbacks::Factory;
        let context=super::host::Context {time:self.facts.time,follow_far:self.runtime.core.follow_radii().1,
            stream_a:self.stream_a,vehicles:self.vehicles.clone(),voice:&self.voices,items:&self.item_owners};
        let mut services=context.services(recon,queries);let mut random=self.runtime.random.share();
        super::live::with_rows(&mut self.state,store,|rows| {
            let mut world=super::world::World {rows,random:&mut random,combat:&mut self.combat,services:&mut services,settings:&self.settings,facts:&self.facts};
            world.grenade_dangers(&mut self.runtime.core,||Some(crate::units::f3::viewpoint()));
        });
        self.counters.danger_phases=self.counters.danger_phases.wrapping_add(1);
        let draws=self.runtime.random.draw_counts();
        self.counters.draws_a=draws[0] as u32;self.counters.draws_b=draws[1] as u32;self.counters.draws_c=draws[2] as u32;
    }
}


#[cfg(target_arch="wasm32")]
impl Battle {
    /// playerCommand observes the current Character and sends one ordered
    /// command through the same squad, voice and shared-generator services.
    pub fn native_command(&mut self,store:&mut crate::gen::Store,recon:&mut crate::recon::Authority,queries:&mut crate::battle::live::Queries,input:&crate::gen::AiPlayerCommand)->bool {
        use super::callbacks::Factory;
        if crate::battle::f4_core::resolve_character(store,&input.character).is_none() {return false;}
        self.facts.time=input.time;
        let context=super::host::Context {time:self.facts.time,follow_far:self.runtime.core.follow_radii().1,
            stream_a:self.stream_a,vehicles:self.vehicles.clone(),voice:&self.voices,items:&self.item_owners};
        let mut services=context.services(recon,queries);let mut random=self.runtime.random.share();
        super::live::with_rows(&mut self.state,store,|rows| {
            let Some(reference)=rows.characters.resolve(rows.store,&input.character) else{return false};
            let Some(character)=rows.state.roster.index(rows.store,rows.characters,reference) else{return false};
            let mut world=super::world::World {rows,random:&mut random,combat:&mut self.combat,services:&mut services,settings:&self.settings,facts:&self.facts};
            self.runtime.core.player_command(&mut world,character,input.kind);true
        })
    }
}
