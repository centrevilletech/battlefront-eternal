//! Outer simulation entries take the complete AI instance out of Sim before
//! lending canonical owners onward. Nested calls carry explicit callback
//! scopes; they cannot reacquire a second Core from the instance slot.
use crate::{abi::sim,battle::live::Queries,recon::Authority,units::f3::WorldServices};

/// missionTeams/teamTable are cold readers for the page's relationship,
/// capture and side UI. Runtime bot decisions use the canonical roster.
#[no_mangle]
pub extern "C" fn ai_mission_teams(roster:u32)->*const f64 {
    use crate::{battle::spawning,gen::{AiMissionTeam,AiMissionTeams,AiMissionTeamsInput}};
    let (words,heap)=sim().calls.args();let input=AiMissionTeamsInput::take(words,heap);
    let calls=super::initial::calls(&input.calls,&sim().strings);
    let mut teams=if roster!=0 {Vec::new()}else{spawning::make_teams(200.)};
    if roster==0 {spawning::apply_mission(&mut teams,&calls,&|_|true,&mut sim().strings);}
    let source=if roster!=0 {&sim().store.spawning.teams}else{&teams};
    let teams=source.iter().map(|team| {
        let first=team.relations[1]>0;let second=team.relations[2]>0;
        let capture=if team.index<=2. {team.index}else if first {if second {0.}else{1.}}else if second {2.}else{team.index};
        AiMissionTeam {relations:std::array::from_fn(|i|team.relations[i] as f64),faction:team.faction,capture}
    }).collect();
    let names=input.names.iter().map(|id|spawning::faction_of(Some(&String::from_utf16_lossy(sim().strings.units(*id))))).collect();
    let out=AiMissionTeams {teams,names};
    sim().calls.result(AiMissionTeams::WORDS,|row,heap|out.put(row,heap));sim().calls.block.ptr()
}

pub fn with_callbacks<R>(recon:&mut Authority,queries:&mut Queries,run:impl FnOnce(WorldServices<'_>)->R)->R {
    if !sim().ai.battle().is_some_and(|battle|battle.ready()) {return run(WorldServices {ai:None,recon,queries});}
    let mut owner=std::mem::take(&mut sim().ai);
    let result=owner.battle_mut().unwrap().with_native_callbacks(recon,queries,run);
    sim().ai=owner;result
}

/// Map/class loading starts an inactive owner. Source createAI finishes the
/// initial roster before activation; callbacks cannot attach bots early.
#[no_mangle]
pub extern "C" fn ai_world_begin()->u32 {
    use crate::gen::AiWorldSetup;
    if sim().ai.battle().is_some() {return 0;}
    let (words,heap)=sim().calls.args();let input=AiWorldSetup::take(words,heap);
    let Ok(navigation)=super::load::Navigation::read(&input.navigation,&sim().strings) else{return 0};
    let (navigation,posts)=navigation.into_parts();
    let teams=super::load::teams(&sim().store,crate::units::f3::relation);
    let mut owner=std::mem::take(&mut sim().ai);
    let battle=owner.begin(navigation,&input.settings,&mut sim().strings,teams);battle.navigation_posts=posts;
    sim().ai=owner;1
}

/// The descriptor's generated pointers survive ordinary phases. JavaScript
/// recreates typed views after calls, just as for the unit and fleet owners.
#[no_mangle]
pub extern "C" fn ai_world_boundary()->*const f64 {
    let Some(battle)=sim().ai.battle_mut() else{return std::ptr::null()};
    battle.exchange.descriptor(&battle.counters)
}

/// ai.update crosses once. Decisions, mounted aim and recursive physical
/// effects all borrow one native Core and the original shared generators.
#[no_mangle]
pub extern "C" fn ai_world_phase()->u32 {
    if !sim().ai.battle().is_some_and(|battle|battle.ready()) {return 0;}
    let before=crate::allocations::snapshot();
    let outer=crate::units::f3::external();
    let event_count=sim().events.count();let event_words=sim().events.len();
    let version=crate::units::f3::publication_version();
    let control_version=sim().ai.battle().unwrap().state.control_views.version();
    let mut owner=std::mem::take(&mut sim().ai);
    let battle=owner.battle_mut().unwrap();
    let mut exchange=std::mem::take(&mut battle.exchange);
    let header=exchange.decode();
    let accepted=crate::units::f3::with_bare_services(|services| {
            let Some(services)=services else{return false};
            let mut store=std::mem::take(&mut sim().store);
            let facts=super::world::Facts {time:header.time,paused:header.paused,network_secondary:header.network_secondary,..Default::default()};
            let accepted=battle.native_phase(&mut store,services.recon,services.queries,&facts,&header.facts,&exchange.cameras,&exchange.journal).is_ok();
            sim().store=store;accepted
        });
    battle.exchange=exchange;sim().ai=owner;
    // All native loans end before sparse entity publication. Its allocations
    // belong to this crossing too, rather than disappearing from the census.
    if header.unit_phase_follows {outer.before_units();}else{drop(outer);}
    finish_boundary(before,version,control_version,event_count,event_words,accepted);
    u32::from(accepted)
}

/// Immutable authored policies are prepared once. Weapon ids refer to F3's
/// existing definitions; no second live arsenal or weapon table is imported.
#[no_mangle]
pub extern "C" fn ai_world_content()->u32 {
    let (words,heap)=sim().calls.args();let content=crate::gen::AiWorldContent::take(words,heap);
    if !sim().ai.battle().is_some_and(|battle|!battle.ready()) {return 0;}
    if content.hulls.iter().any(|row|!crate::units::f3_hulls::matches_binding(row.hull,row.member))
        ||content.props.iter().any(|row|!crate::units::f3_props::matches_binding(row.target,row.member,&row.owner)) {return 0;}
    let definitions=crate::combat::module::defs();
    if content.weapons.iter().any(|id|!definitions.weapons.has(*id)) {return 0;}
    let mut owner=std::mem::take(&mut sim().ai);let battle=owner.battle_mut().unwrap();
    let result=(|| {
        for data in &content.weapon_data {battle.weapon_json(&data.json)?;}
        for class in &content.classes {battle.state.unit_class(class,&sim().strings);}
        for item in &content.items {super::content::item(item,&sim().strings,&mut battle.state.items);}
        for &id in &content.weapons {battle.weapon_class(id,definitions.weapons.class(id));}
        for post in &content.posts {
            let definition=super::posts::Definition::read(post,&sim().strings);
            battle.state.posts.register(&sim().store,&mut battle.state.population,post.policy.clone(),definition).map_err(|_|String::from("invalid AI post"))?;
        }
        let mut store=std::mem::take(&mut sim().store);
        let prepared=super::live::with_rows(&mut battle.state,&mut store,|rows|rows.prepare_members(&content.hulls,&content.props,&sim().strings));
        sim().store=store;
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
            if let Some(body)=crate::units::f3_props::built_body(row.target) {
                let object=battle.state.entities.find(entity).expect("prepared AI sight identity");
                battle.state.sight.prepare(object,body);
            }
        }
        Ok::<(),String>(())
    })().is_ok();
    sim().ai=owner;u32::from(result)
}

/// createAI's explicit attach loop follows the completed native roster.
/// Stream binding shares authorities with later unit/seat fire and spawn work;
/// adopting the existing state here consumes none of A, B or private C.
#[no_mangle]
pub extern "C" fn ai_world_activate()->u32 {
    if !sim().ai.battle().is_some_and(|battle|!battle.ready()) {return 0;}
    let outer=crate::units::f3::external();
    let mut owner=std::mem::take(&mut sim().ai);let battle=owner.battle_mut().unwrap();
    // UnitFactory finishes inactive hero/player stock after the initial
    // roster. Resolve the completed native weapon table now, including ids
    // absent from the host's earlier snapshot and gaps from failed classes.
    for (index,class) in crate::combat::module::defs().weapons.classes.iter().enumerate() {
        let id=crate::store::DefId(index as u32);
        if let Some(class)=class.as_ref().filter(|_|!battle.state.weapons.contains(id)) {battle.weapon_class(id,class);}
    }
    let accepted=crate::units::f3::with_bare_services(|services| {
        let Some(services)=services else{return false};
        let mut store=std::mem::take(&mut sim().store);
        let handles:Option<Vec<_>>=store.battle.characters.iter().map(|reference|crate::battle::f4_core::character_handle(*reference)).collect();
        let accepted=if let Some(handles)=handles {
            let bodies:Vec<_>=store.unit.iter().map(|(reference,_)|reference).collect();
            let prepared=super::live::with_rows(&mut battle.state,&mut store,|rows| {
                rows.state.prepare_roster(rows.store,rows.characters,&handles)?;
                rows.state.prepare_bodies(rows.store,&bodies)?;
                // CombatInput maps objectives through the same post identities
                // as patrol and population, including before the first scan.
                super::objects::Objects::prepare(rows.store,rows.characters,&mut rows.state.entities,&rows.state.roster,&rows.state.population);
                Ok::<(),super::load::Stale>(())
            }).is_ok();
            if prepared {
                let posts=super::load::Navigation::hint_posts(&battle.navigation_posts,&battle.state.population);
                battle.state.prepare_hints(|index|posts.get(index as usize).copied().flatten());
                crate::units::f3::bind_ai_stream(battle.stream_a,battle.runtime.random.weapon_stream());
                if battle.runtime.core.options.bind.random_b {crate::units::f3::bind_ai_stream(battle.stream_b,battle.runtime.random.command_stream());}
                battle.activate();battle.attach_initial(&mut store,services.recon,services.queries);
            }
            prepared
        }else{false};
        sim().store=store;accepted
    });
    sim().ai=owner;drop(outer);u32::from(accepted)
}

// Boundary traffic includes the final sparse publication after every owner
// loan ends. Shared helpers keep ordinary AI and later danger censuses equal.
fn finish_boundary(before:crate::allocations::Counts,version:u32,control_version:u32,event_count:usize,event_words:usize,accepted:bool) {
    let count=crate::allocations::snapshot().since(before);
    let emitted=sim().events.count().saturating_sub(event_count) as u32;
    let payload=if crate::units::f3::publication_version()!=version {crate::units::f3::output_words()}else{0};
    let output=(sim().events.len().saturating_sub(event_words) as u32).wrapping_add(payload).wrapping_add(crate::gen::AiPhaseCounters::WORDS as u32);
    let battle=sim().ai.battle_mut().unwrap();
    let control_words=if battle.state.control_views.version()!=control_version {battle.state.control_views.word_count()}else{0};
    battle.counters.events=battle.counters.events.wrapping_add(emitted);
    battle.counters.output_words=battle.counters.output_words.wrapping_add(output).wrapping_add(control_words);
    battle.counters.allocations=battle.counters.allocations.wrapping_add(count.calls as u32);
    battle.counters.reallocations=battle.counters.reallocations.wrapping_add(count.reallocs as u32);
    battle.counters.allocation_bytes=battle.counters.allocation_bytes.wrapping_add(count.bytes as u32);
    if !accepted {battle.counters.stale_commands=battle.counters.stale_commands.wrapping_add(1);}
    battle.exchange.descriptor(&battle.counters);
}

#[no_mangle]
pub extern "C" fn ai_world_dangers()->u32 {
    if !sim().ai.battle().is_some_and(|battle|battle.ready()) {return 0;}
    let before=crate::allocations::snapshot();let outer=crate::units::f3::external();
    let event_count=sim().events.count();let event_words=sim().events.len();let version=crate::units::f3::publication_version();
    let control_version=sim().ai.battle().unwrap().state.control_views.version();
    let mut owner=std::mem::take(&mut sim().ai);
    let accepted=crate::units::f3::with_bare_services(|services| {
        let Some(services)=services else{return false};
        let mut store=std::mem::take(&mut sim().store);
        owner.battle_mut().unwrap().native_dangers(&mut store,services.recon,services.queries);
        sim().store=store;true
    });
    sim().ai=owner;drop(outer);finish_boundary(before,version,control_version,event_count,event_words,accepted);u32::from(accepted)
}


/// A user order is a boundary command, never a foreign controller action.
/// Qualified Character life rejects a command retained across slot reuse.
#[no_mangle]
pub extern "C" fn ai_world_command()->u32 {
    if !sim().ai.battle().is_some_and(|battle|battle.ready()) {return 0;}
    let (words,heap)=sim().calls.args();let input=crate::gen::AiPlayerCommand::take(words,heap);
    if !input.time.is_finite() || !input.kind.is_finite() || input.kind<0. || input.kind>5. || input.kind.fract()!=0. {return 0;}
    let before=crate::allocations::snapshot();let outer=crate::units::f3::external();
    let event_count=sim().events.count();let event_words=sim().events.len();let version=crate::units::f3::publication_version();
    let control_version=sim().ai.battle().unwrap().state.control_views.version();
    let mut owner=std::mem::take(&mut sim().ai);
    let accepted=crate::units::f3::with_bare_services(|services| {
        let Some(services)=services else{return false};let mut store=std::mem::take(&mut sim().store);
        let accepted=owner.battle_mut().unwrap().native_command(&mut store,services.recon,services.queries,&input);
        sim().store=store;accepted
    });
    let battle=owner.battle_mut().unwrap();
    battle.counters.commands=battle.counters.commands.wrapping_add(1);
    battle.counters.input_words=battle.counters.input_words.wrapping_add(crate::gen::AiPlayerCommand::WORDS as u32);
    let draws=battle.runtime.random.draw_counts();battle.counters.draws_a=draws[0] as u32;battle.counters.draws_b=draws[1] as u32;battle.counters.draws_c=draws[2] as u32;
    sim().ai=owner;drop(outer);finish_boundary(before,version,control_version,event_count,event_words,accepted);u32::from(accepted)
}


/// F3 calls this after its unit, seat and lifecycle loans have ended. The
/// sidecar never reads a JS mirror and never acquires another Core scope.
pub fn publish_controls() {
    if !sim().ai.battle().is_some_and(|battle|battle.ready()) {return;}
    let mut owner=std::mem::take(&mut sim().ai);let battle=owner.battle_mut().unwrap();
    let previous=battle.exchange.control_output;
    let mut output=std::mem::take(&mut battle.state.control_views);
    let descriptor=crate::vehicles::module::f2::live_seats::with_owned_seats(|seats|output.publish(&mut battle.state,&sim().store,seats,&crate::battle::f4::live().buildings));
    battle.state.control_views=output;battle.exchange.control_output=[descriptor[0],descriptor[1],descriptor[2],previous[3]];
    sim().ai=owner;
    // An unchanged control prefix leaves its descriptor and counters valid.
    // Consuming an unread prefix changes count, so it still publishes zero.
    if previous[..3]==descriptor[..3] {return;}
    // Inline acknowledgement/header addresses are valid only in their final
    // instance location, after returning the complete owner from its loan.
    let battle=sim().ai.battle_mut().unwrap();
    battle.exchange.control_output[3]=battle.state.control_views.ack_pointer();
    battle.exchange.descriptor(&battle.counters);
}


/// Initial Spawning.begin precedes Classic's attach loop. It uses the same
/// retained population and shared A as later objectives and native weapons.
/// Result is count+1 so the cold host can bind Character facades before sync.
#[no_mangle]
pub extern "C" fn ai_world_roster()->u32 {
    if !sim().ai.battle().is_some_and(|battle|!battle.ready()&&battle.initial_naming.is_none())
        || !sim().store.battle.characters.is_empty() {return 0;}
    let (words,heap)=sim().calls.args();let input=crate::gen::AiRosterSetup::take(words,heap);
    let mut program=super::initial::Program::read(&input,&mut sim().strings);
    let teams=crate::ai::control::world::TeamTable {
        relations:program.spawning.teams.iter().map(|team|Some(team.relations.iter().map(|value|*value as f64).collect())).collect(),
        factions:program.spawning.teams.iter().map(|team|team.faction).collect(),alliances:Vec::new(),
    };
    let outer=crate::units::f3::external();
    let mut owner=std::mem::take(&mut sim().ai);let battle=owner.battle_mut().unwrap();
    battle.runtime.core.teams=teams;
    crate::units::f3::bind_ai_stream(battle.stream_a,battle.runtime.random.weapon_stream());
    if battle.runtime.core.options.bind.random_b {crate::units::f3::bind_ai_stream(battle.stream_b,battle.runtime.random.command_stream());}
    let accepted=crate::units::f3::with_bare_services(|services| {
        let Some(_services)=services else{return false};
        let mut store=std::mem::take(&mut sim().store);
        super::live::with_rows(&mut battle.state,&mut store,|rows| {
            let facts=rows.state.bindings.facts(rows.buildings.rows());
            let mut world=super::initial::Native::new(rows.store,&input,&facts);
            super::initial::begin(rows.store,&mut program,&mut rows.state.population,&mut battle.runtime.random,
                &battle.runtime.core.teams,&facts,battle.runtime.core.options.bind.enhanced,&mut sim().strings,&mut world);
        });
        if crate::battle::f4_core::owns_lifecycle() {
            for index in 0..store.battle.characters.len() {
                let character=store.battle.characters[index];crate::battle::f4_core::adopt(&mut store,character);
            }
            let human=store.battle.characters.iter().copied().find(|reference|store.character.get(*reference).is_some_and(|character|character.human));
            crate::battle::f4_tail::initial_human(human);
            crate::battle::f4_core::roster_slots(&mut store);crate::battle::f4_core::mark_tail(&store);
        }
        sim().store=store;true
    });
    if accepted {battle.initial_naming=Some(program.naming);}
    sim().ai=owner;drop(outer);
    if accepted {sim().store.battle.characters.len() as u32+1}else{0}
}


/// This cold structural identity precedes F4 command/publication handles.
/// It names only the canonical Character slab and cannot issue a command.
#[no_mangle]
pub extern "C" fn ai_world_roster_character(index:u32)->*const f64 {
    let Some(reference)=sim().store.battle.characters.get(index as usize).copied() else{return std::ptr::null()};
    let battle=sim().ai.battle_mut().unwrap();
    crate::gen::AiCharacterBirth {slot:reference.slot,generation:reference.gen}.put(&mut battle.initial_character,&mut Vec::new());
    battle.initial_character.as_ptr()
}


/// F3 construction already owns these canonical F4 identities. The cold AI
/// loader can bind policies before the later live-phase host is constructed.
#[no_mangle]
pub extern "C" fn ai_world_prop_owner(index:u32)->*const f64 {
    if index as usize>=crate::units::f3_props::count() {return std::ptr::null()};
    let owner=crate::units::f3_props::owner(index as usize);
    let battle=sim().ai.battle_mut().unwrap();owner.put(&mut battle.initial_prop,&mut Vec::new());
    battle.initial_prop.as_ptr()
}
