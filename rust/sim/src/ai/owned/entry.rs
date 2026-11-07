//! Outer simulation entries take the AI battle out of its owner before
//! lending canonical owners onward. Nested calls carry explicit callback
//! scopes; they cannot reacquire a second Core from the instance slot.
use super::rows::{row_buildings,row_context,row_store,row_store_mut};
use crate::{world::owner::World,battle::live::Queries,recon::Authority,units::f3::WorldServices};

/// missionTeams/teamTable are cold readers for the page's relationship,
/// capture and side UI. Runtime bot decisions use the canonical roster.
pub fn ai_mission_teams_in(w:&mut World,roster:u32)->&crate::store::block::Block {
    use crate::{battle::spawning,gen::{AiMissionTeam,AiMissionTeams,AiMissionTeamsInput}};
    let (words,heap)=w.sim.calls.args();let input=AiMissionTeamsInput::take(words,heap);
    let calls=super::initial::calls(&input.calls,&w.sim.strings);
    let mut teams=if roster!=0 {Vec::new()}else{spawning::make_teams(200.)};
    if roster==0 {spawning::apply_mission(&mut teams,&calls,&|_|true,&mut w.sim.strings);}
    let source=if roster!=0 {&w.sim.store.spawning.teams}else{&teams};
    let teams=source.iter().map(|team| {
        let first=team.relations[1]>0;let second=team.relations[2]>0;
        let capture=if team.index<=2. {team.index}else if first {if second {0.}else{1.}}else if second {2.}else{team.index};
        AiMissionTeam {relations:std::array::from_fn(|i|team.relations[i] as f64),faction:team.faction,capture}
    }).collect();
    let names=input.names.iter().map(|id|spawning::faction_of(Some(&String::from_utf16_lossy(w.sim.strings.units(*id))))).collect();
    let out=AiMissionTeams {teams,names};
    w.sim.calls.result(AiMissionTeams::WORDS,|row,heap|out.put(row,heap));&w.sim.calls.block
}

pub fn with_callbacks_in<R>(w:&mut World,recon:&mut Authority,queries:&mut Queries,run:impl FnOnce(&mut World,WorldServices<'_>)->R)->R {
    if !w.ai.battle().is_some_and(|battle|battle.ready()) {return run(w,WorldServices {ai:None,recon,queries});}
    let mut owned=w.ai.take_battle().unwrap();
    let result=owned.with_native_callbacks(w,recon,queries,run);
    w.ai.return_battle(owned);result
}

/// Map/class loading starts an inactive owner. Source createAI finishes the
/// initial roster before activation; callbacks cannot attach bots early.
pub fn ai_world_begin_in(w:&mut World)->u32 {
    use crate::gen::AiWorldSetup;
    if w.ai.battle().is_some() {return 0;}
    let (words,heap)=w.sim.calls.args();let input=AiWorldSetup::take(words,heap);
    let Ok(navigation)=super::load::Navigation::read(&input.navigation,&w.sim.strings) else{return 0};
    let (navigation,posts)=navigation.into_parts();
    let teams=super::load::teams(&w.sim.store,|a,b|crate::units::f3::relation_in(&w.units,a,b));
    let battle=w.ai.begin(navigation,&input.settings,&mut w.sim.strings,teams);battle.navigation_posts=posts;1
}

/// The descriptor's generated pointers survive ordinary phases. JavaScript
/// recreates typed views after calls, just as for the unit and fleet owners.
pub fn ai_world_boundary_in(w:&mut World)->Option<&[f64]> {
    let battle=w.ai.battle_mut()?;
    Some(battle.exchange.descriptor(&battle.counters))
}

/// ai.update crosses once. Decisions, mounted aim and recursive physical
/// effects all borrow one native Core and the original shared generators.
pub fn ai_world_phase_in(w:&mut World)->u32 {
    if !w.ai.battle().is_some_and(|battle|battle.ready()) {return 0;}
    let before=crate::allocations::snapshot();
    let outer=crate::units::f3::external_in(w);
    let event_count=w.sim.events.count();let event_words=w.sim.events.len();
    let version=crate::units::f3::publication_version_in(&w.units);
    let control_version=w.ai.battle().unwrap().state.control_views.version();
    let mut owned=w.ai.take_battle().unwrap();
    let battle=&mut *owned;
    let mut exchange=std::mem::take(&mut battle.exchange);
    let header=exchange.decode();
    let accepted=crate::units::f3::with_bare_services_in(w,|w,services| {
            let Some(services)=services else{return false};
            let facts=super::world::Facts {time:header.time,paused:header.paused,network_secondary:header.network_secondary,..Default::default()};
            let accepted=battle.native_phase(w,services.recon,services.queries,&facts,&header.facts,&exchange.cameras,&exchange.journal).is_ok();
            accepted
        });
    battle.exchange=exchange;w.ai.return_battle(owned);
    // All native loans end before sparse entity publication. Its allocations
    // belong to this crossing too, rather than disappearing from the census.
    if header.unit_phase_follows {outer.before_units(w);}else{outer.finish(w,true);}
    finish_boundary(w,before,version,control_version,event_count,event_words,accepted);
    u32::from(accepted)
}

/// Immutable authored policies are prepared once. Weapon ids refer to F3's
/// existing definitions; no second live arsenal or weapon table is imported.
pub fn ai_world_content_in(w:&mut World)->u32 {
    let (words,heap)=w.sim.calls.args();let content=crate::gen::AiWorldContent::take(words,heap);
    if !w.ai.battle().is_some_and(|battle|!battle.ready()) {return 0;}
    if content.hulls.iter().any(|row|!crate::units::f3_hulls::matches_binding_in(w,row.hull,row.member))
        ||content.props.iter().any(|row|!crate::units::f3_props::matches_binding_in(w,row.target,row.member,&row.owner)) {return 0;}
    let definitions=crate::combat::module::defs_in(&mut w.combat);
    if content.weapons.iter().any(|id|!definitions.weapons.has(*id)) {return 0;}
    let mut owned=w.ai.take_battle().unwrap();let battle=&mut *owned;
    let result=(|| {
        for data in &content.weapon_data {battle.weapon_json(&data.json)?;}
        for class in &content.classes {battle.state.unit_class(class,&w.sim.strings);}
        for item in &content.items {super::content::item(item,&w.sim.strings,&mut battle.state.items);}
        for &id in &content.weapons {battle.weapon_class(id,definitions.weapons.class(id));}
        for post in &content.posts {
            let definition=super::posts::Definition::read(post,&w.sim.strings);
            battle.state.posts.register(&w.sim.store,&mut battle.state.population,post.policy.clone(),definition).map_err(|_|String::from("invalid AI post"))?;
        }
        let prepared=super::live::with_rows(w,&mut battle.state,|rows|rows.prepare_members(&content.hulls,&content.props));
        if !prepared {return Err(String::from("invalid AI registry content"));}
        // Game.los accepts a hit on the candidate's own built body. Keep
        // that authored identity once; current enabled looks stay with F4.
        for row in &content.props {
            use super::entities::Entity;
            let entity=match row.target {
                crate::gen::RepairTarget::Building(r)=>Entity::Building(r),
                crate::gen::RepairTarget::Turret(r)=>Entity::Turret(r),
                crate::gen::RepairTarget::Vehicle(_)=>unreachable!(),
            };
            if let Some(body)=crate::units::f3_props::built_body_in(w,row.target) {
                let object=battle.state.entities.find(entity).expect("prepared AI sight identity");
                battle.state.sight.prepare(object,body);
            }
        }
        Ok::<(),String>(())
    })().is_ok();
    w.ai.return_battle(owned);u32::from(result)
}

/// createAI's explicit attach loop follows the completed native roster.
/// Stream binding shares authorities with later unit/seat fire and spawn work;
/// adopting the existing state here consumes none of A, B or private C.
pub fn ai_world_activate_in(w:&mut World)->u32 {
    if !w.ai.battle().is_some_and(|battle|!battle.ready()) {return 0;}
    let outer=crate::units::f3::external_in(w);
    let mut owned=w.ai.take_battle().unwrap();let battle=&mut *owned;
    // UnitFactory finishes inactive hero/player stock after the initial
    // roster. Resolve the completed native weapon table now, including ids
    // absent from the host's earlier snapshot and gaps from failed classes.
    for (index,class) in crate::combat::module::defs_in(&mut w.combat).weapons.classes.iter().enumerate() {
        let id=crate::store::DefId(index as u32);
        if let Some(class)=class.as_ref().filter(|_|!battle.state.weapons.contains(id)) {battle.weapon_class(id,class);}
    }
    let accepted=crate::units::f3::with_bare_services_in(w,|w,services| {
        let Some(services)=services else{return false};
        let handles:Option<Vec<_>>=w.sim.store.battle.characters.iter().map(|reference|crate::battle::f4_core::character_handle_in(&mut w.battle,*reference)).collect();
        let accepted=if let Some(handles)=handles {
            let bodies:Vec<_>=w.sim.store.unit.iter().map(|(reference,_)|reference).collect();
            let prepared=super::live::with_rows(w,&mut battle.state,|rows| {
                rows.state.prepare_roster(row_context!(rows),rows.characters,&handles)?;
                rows.state.prepare_bodies(row_store!(rows),&bodies)?;
                // CombatInput maps objectives through the same post identities
                // as patrol and population, including before the first scan.
                super::objects::Objects::prepare(row_context!(rows),rows.characters,&mut rows.state.entities,&rows.state.roster,&rows.state.population);
                Ok::<(),super::load::Stale>(())
            }).is_ok();
            if prepared {
                let posts=super::load::Navigation::hint_posts(&battle.navigation_posts,&battle.state.population);
                battle.state.prepare_hints(|index|posts.get(index as usize).copied().flatten());
                crate::units::f3::bind_ai_stream_in(&mut w.units,battle.stream_a,battle.runtime.random.weapon_stream());
                if battle.runtime.core.options.bind.random_b {crate::units::f3::bind_ai_stream_in(&mut w.units,battle.stream_b,battle.runtime.random.command_stream());}
                battle.activate();

                battle.attach_initial(w,services.recon,services.queries);

            }
            prepared
        }else{false};
        accepted
    });
    w.ai.return_battle(owned);outer.finish(w,true);u32::from(accepted)
}

// Boundary traffic includes the final sparse publication after every owner
// loan ends. Shared helpers keep ordinary AI and later danger censuses equal.
fn finish_boundary(w:&mut World,before:crate::allocations::Counts,version:u32,control_version:u32,event_count:usize,event_words:usize,accepted:bool) {
    let count=crate::allocations::snapshot().since(before);
    let emitted=w.sim.events.count().saturating_sub(event_count) as u32;
    let payload=if crate::units::f3::publication_version_in(&w.units)!=version {crate::units::f3::output_words_in(&w.units)}else{0};
    let output=(w.sim.events.len().saturating_sub(event_words) as u32).wrapping_add(payload).wrapping_add(crate::gen::AiPhaseCounters::WORDS as u32);
    let battle=w.ai.battle_mut().unwrap();
    let control_words=if battle.state.control_views.version()!=control_version {battle.state.control_views.word_count()}else{0};
    battle.counters.events=battle.counters.events.wrapping_add(emitted);
    battle.counters.output_words=battle.counters.output_words.wrapping_add(output).wrapping_add(control_words);
    battle.counters.allocations=battle.counters.allocations.wrapping_add(count.calls as u32);
    battle.counters.reallocations=battle.counters.reallocations.wrapping_add(count.reallocs as u32);
    battle.counters.allocation_bytes=battle.counters.allocation_bytes.wrapping_add(count.bytes as u32);
    if !accepted {battle.counters.stale_commands=battle.counters.stale_commands.wrapping_add(1);}
    battle.exchange.descriptor(&battle.counters);
}

pub fn ai_world_dangers_in(w:&mut World)->u32 {
    if !w.ai.battle().is_some_and(|battle|battle.ready()) {return 0;}
    // Game.step has already drained battle and ordnance. With no live
    // grenade there are no warnings or RNG draws, and no owner needs a loan.
    // Unread structural work still takes the ordinary publication boundary.
    if crate::units::f3::settled_in(w) && !crate::vehicles::module::f2::has_pending_in(w)
        && !w.sim.store.ordnance.grenades.iter().any(|r|w.sim.store.grenade.get(*r).is_some_and(|g|g.gone!=Some(true))) {
        let before=crate::allocations::snapshot();let version=crate::units::f3::publication_version_in(&w.units);
        let control_version=w.ai.battle().unwrap().state.control_views.version();
        let count=w.sim.events.count();let words=w.sim.events.len();
        w.ai.battle_mut().unwrap().record_danger_phase();
        finish_boundary(w,before,version,control_version,count,words,true);
        return crate::gen::AiDangerResult::Unchanged as u32;
    }
    let before=crate::allocations::snapshot();let outer=crate::units::f3::external_in(w);
    let event_count=w.sim.events.count();let event_words=w.sim.events.len();let version=crate::units::f3::publication_version_in(&w.units);
    let control_version=w.ai.battle().unwrap().state.control_views.version();
    let mut owned=w.ai.take_battle().unwrap();
    let accepted=crate::units::f3::with_bare_services_in(w,|w,services| {
        let Some(services)=services else{return false};
        owned.native_dangers(w,services.recon,services.queries);true
    });
    w.ai.return_battle(owned);outer.finish(w,true);finish_boundary(w,before,version,control_version,event_count,event_words,accepted);u32::from(accepted)
}

/// A user order is a boundary command, never a foreign controller action.
/// Qualified Character life rejects a command retained across slot reuse.
pub fn ai_world_command_in(w:&mut World)->u32 {
    if !w.ai.battle().is_some_and(|battle|battle.ready()) {return 0;}
    let (words,heap)=w.sim.calls.args();let input=crate::gen::AiPlayerCommand::take(words,heap);
    if !input.time.is_finite() || !input.kind.is_finite() || input.kind<0. || input.kind>5. || input.kind.fract()!=0. {return 0;}
    let before=crate::allocations::snapshot();let outer=crate::units::f3::external_in(w);
    let event_count=w.sim.events.count();let event_words=w.sim.events.len();let version=crate::units::f3::publication_version_in(&w.units);
    let control_version=w.ai.battle().unwrap().state.control_views.version();
    let mut owned=w.ai.take_battle().unwrap();
    let accepted=crate::units::f3::with_bare_services_in(w,|w,services| {
        let Some(services)=services else{return false};
        owned.native_command(w,services.recon,services.queries,&input)
    });
    let battle=&mut *owned;
    battle.counters.commands=battle.counters.commands.wrapping_add(1);
    battle.counters.input_words=battle.counters.input_words.wrapping_add(crate::gen::AiPlayerCommand::WORDS as u32);
    let draws=battle.runtime.random.draw_counts();battle.counters.draws_a=draws[0] as u32;battle.counters.draws_b=draws[1] as u32;battle.counters.draws_c=draws[2] as u32;
    w.ai.return_battle(owned);outer.finish(w,true);finish_boundary(w,before,version,control_version,event_count,event_words,accepted);u32::from(accepted)
}

/// F3 calls this after its unit, seat and lifecycle loans have ended. The
/// sidecar never reads a JS mirror and never acquires another Core scope.
pub fn publish_controls_in(w:&mut World) {
    let deferred=crate::world::deferred_in(&w.tick);
    if !w.ai.battle().is_some_and(|battle|battle.ready()) {return;}
    let mut owned=w.ai.take_battle().unwrap();let battle=&mut *owned;
    let previous=battle.exchange.control_output;
    let mut output=std::mem::take(&mut battle.state.control_views);
    let descriptor=output.publish_at(&mut battle.state,&w.sim.store,w.fleet.seats_owner(),&crate::battle::f4::live_in(&mut w.battle).buildings,deferred);
    battle.state.control_views=output;battle.exchange.control_output=[descriptor[0],descriptor[1],descriptor[2],previous[3]];
    w.ai.return_battle(owned);
    // An unchanged control prefix leaves its descriptor and counters valid.
    // Consuming an unread prefix changes count, so it still publishes zero.
    if previous[..3]==descriptor[..3] {return;}
    // Inline acknowledgement/header addresses are valid only in their final
    // instance location, after returning the complete owner from its loan.
    let battle=w.ai.battle_mut().unwrap();
    #[cfg(target_arch = "wasm32")]
    { battle.exchange.control_output[3]=battle.state.control_views.ack_pointer(); }
    battle.exchange.descriptor(&battle.counters);
}

/// Initial Spawning.begin precedes Classic's attach loop. It uses the same
/// retained population and shared A as later objectives and native weapons.
/// Result is count+1 so the cold host can bind Character facades before sync.
pub fn ai_world_roster_in(w:&mut World)->u32 {
    if !w.ai.battle().is_some_and(|battle|!battle.ready()&&battle.initial_naming.is_none())
        || !w.sim.store.battle.characters.is_empty() {return 0;}
    let (words,heap)=w.sim.calls.args();let input=crate::gen::AiRosterSetup::take(words,heap);
    let mut program=super::initial::Program::read(&input,&mut w.sim.strings);
    let teams=crate::ai::control::world::TeamTable {
        relations:program.spawning.teams.iter().map(|team|Some(team.relations.iter().map(|value|*value as f64).collect())).collect(),
        factions:program.spawning.teams.iter().map(|team|team.faction).collect(),alliances:Vec::new(),
    };
    let outer=crate::units::f3::external_in(w);
    let mut owned=w.ai.take_battle().unwrap();let battle=&mut *owned;
    battle.runtime.core.teams=teams;
    crate::units::f3::bind_ai_stream_in(&mut w.units,battle.stream_a,battle.runtime.random.weapon_stream());
    if battle.runtime.core.options.bind.random_b {crate::units::f3::bind_ai_stream_in(&mut w.units,battle.stream_b,battle.runtime.random.command_stream());}
    let accepted=crate::units::f3::with_bare_services_in(w,|w,services| {
        let Some(_services)=services else{return false};
        super::live::with_rows(w,&mut battle.state,|rows| {
            let facts=rows.state.bindings.facts(row_buildings!(rows));
            let world_id = crate::units::f3::collision_world_in(&rows.w.units);
            let mut world=super::initial::Native::new(&mut rows.w.sim.store,&input,&facts, &mut rows.w.collision, &mut rows.w.movement, &rows.w.fleet, world_id);
            super::initial::begin(row_store_mut!(rows),&mut program,&mut rows.state.population,&mut battle.runtime.random,
                &battle.runtime.core.teams,&facts,battle.runtime.core.options.bind.enhanced,&mut rows.w.sim.strings,&mut world);
        });
        if crate::battle::f4_core::owns_lifecycle_in(&mut w.battle) {
            for index in 0..w.sim.store.battle.characters.len() {
                let character=w.sim.store.battle.characters[index];
                crate::battle::f4_core::adopt_character_in(&mut w.battle,&mut w.sim.store,&mut w.sim.events,character);
            }
            let human=w.sim.store.battle.characters.iter().copied().find(|reference|w.sim.store.character.get(*reference).is_some_and(|character|character.human));
            crate::battle::f4_tail::initial_human_in(&mut w.battle,human);
            crate::battle::f4_core::roster_slots_in(&mut w.battle,&mut w.sim.store);crate::battle::f4_core::mark_tail_in(&mut w.battle,&w.sim.store);
        }
        true
    });
    if accepted {battle.initial_naming=Some(program.naming);}
    w.ai.return_battle(owned);outer.finish(w,true);
    if accepted {w.sim.store.battle.characters.len() as u32+1}else{0}
}

/// This cold structural identity precedes F4 command/publication handles.
/// It names only the canonical Character slab and cannot issue a command.
pub fn ai_world_roster_character_in(w:&mut World,index:u32)->Option<&[f64]> {
    let battle=w.ai.battle_mut()?;
    let reference=w.sim.store.battle.characters.get(index as usize).copied()?;
    crate::gen::AiCharacterBirth {slot:reference.slot,generation:reference.gen}.put(&mut battle.initial_character,&mut Vec::new());
    Some(&battle.initial_character)
}

/// F3 construction already owns these canonical F4 identities. The cold AI
/// loader can bind policies before the later live-phase host is constructed.
pub fn ai_world_prop_owner_in(w:&mut World,index:u32)->Option<&[f64]> {
    // Cold and reentrant callers may have no AI owner or prepared unit props.
    if w.ai.battle().is_none() {return None;}
    if index as usize>=crate::units::f3_props::count_in(&w.units) {return None};
    let owner=crate::units::f3_props::owner_in(w,index as usize);
    let battle=w.ai.battle_mut()?;
    owner.put(&mut battle.initial_prop,&mut Vec::new());
    Some(&battle.initial_prop)
}

#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    // Facade shims for callers awaiting explicit owner context.
    #[no_mangle]
    pub extern "C" fn ai_mission_teams(roster:u32)->*const f64 { ai_mission_teams_in(crate::abi::facade::world(),roster).ptr() }
    #[no_mangle]
    pub extern "C" fn ai_world_begin()->u32 { ai_world_begin_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn ai_world_boundary()->*const f64 { ai_world_boundary_in(crate::abi::facade::world()).map_or(std::ptr::null(),|b|b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn ai_world_phase()->u32 { ai_world_phase_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn ai_world_content()->u32 { ai_world_content_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn ai_world_activate()->u32 { ai_world_activate_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn ai_world_dangers()->u32 { ai_world_dangers_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn ai_world_command()->u32 { ai_world_command_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn ai_world_roster()->u32 { ai_world_roster_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn ai_world_roster_character(index:u32)->*const f64 { ai_world_roster_character_in(crate::abi::facade::world(),index).map_or(std::ptr::null(),|b|b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn ai_world_prop_owner(index:u32)->*const f64 { ai_world_prop_owner_in(crate::abi::facade::world(),index).map_or(std::ptr::null(),|b|b.as_ptr()) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{ai_mission_teams, ai_world_begin, ai_world_boundary, ai_world_phase, ai_world_content, ai_world_activate, ai_world_dangers, ai_world_command, ai_world_roster, ai_world_roster_character, ai_world_prop_owner};
