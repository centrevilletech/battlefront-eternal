//! Battlefield and battle rules enter once after items. Canonical Store rows
//! remain native through capture, deaths, queues, AI hooks and human choices.
use crate::world::owner::World as Root;
use super::owner::BattleOwner;
use super::{
    battlefield::Battlefield, human::Command, human_posts::Choices, human_world,
    native_actions::Actions, native_lifecycle::Native, owned_world::Owner,
};
use crate::{ gen::*, store::StrId, units::f3};

pub(super) struct State {
    owner: Owner,
    human: super::native_human::Driver,
    shell: Option<Store>,
    commands: Vec<Command>,
    input: Vec<f64>,
    random: u32,
    gameplay: u32,
    effects: [StrId; 3],
}
fn state(battle: &mut BattleOwner) -> &mut State {
    {
        battle.tail
            .as_mut()
            .expect("native battle tail initialized")
    }
}
pub(crate) fn reset(battle: &mut BattleOwner) {
    {
        battle.tail = None;
    }
}
/// captureWorldSummaries reads the same prepared human roster without
/// running a spawn choice, advancing a queue or consuming a random draw.
pub(crate) fn wire_roster_in(battle: &BattleOwner, team: usize) -> &[StrId] {
    battle.tail.as_ref().expect("prepared battle tail").human.human.prepared.rosters.get(team).map_or(&[], Vec::as_slice)
}
// The pre-AI boundary also replaces CommandPosts.all's former JS refresh.
// State is lent out inside a tail; its World refreshes before its own hooks.
pub(crate) fn refresh_mobile_views_in(w: &mut Root){
    let Some(state)=({w.battle.tail.as_mut()}) else{return};
    super::fleet_posts::refresh_posts(&mut w.sim.store,&mut state.owner.prepared.pads);
    super::f4_core::mark_mobile(&mut w.battle);
}
/// Cold native roster construction follows tail content preparation. Bind
/// its new human before any lifecycle or human-spawn phase can observe it.
pub(crate) fn initial_human_in(battle: &mut BattleOwner, character:Option<crate::store::Ref<Character>>) {
    let state=state(battle);state.owner.prepared.local_character=character;
    state.human.human.prepared.character=character;
}
use super::stats::WorldText as Text;
pub(crate) struct Random(pub u32);
impl<W: super::spawning::RandomWorld + ?Sized> super::spawning::ContextDraws<W> for Random {
    fn next_in(&mut self, world: &mut W) -> f64 { world.random_next(self.0) }
}
pub fn f4_tail_init_in(w: &mut Root) -> Option<&[f64]> {
    assert!(
        f3::active_in(&w.units) && super::f4_core::active_in(&mut w.battle),
        "canonical unit and battle owners required"
    );
    let (row, heap) = w.sim.calls.args();
    let cfg = F4TailSetup::take(row, heap);
    let edges: Vec<_> = cfg.boundary.polygons.into_iter().map(|p| p.edges).collect();
    let mut owner = Owner::new(
        &mut w.sim.store,
        Battlefield {
            boundary: crate::ai::nav::boundary::Boundary::from_edges(&edges),
        },
        cfg.local,
        cfg.classic,
        cfg.registry_capacity as usize,
        cfg.victim_capacity as usize,
    );
    owner.prepared.enhanced = cfg.enhanced;
    owner.prepared.local_character = cfg.character;
    owner.tail.reserve_spawn_points(cfg.point_capacity as usize);
    let posts = w.sim.store.command_posts.posts.len() + w.sim.store.mobile_posts.posts.len();
    let choices = Choices::new(
        posts,
        cfg.victim_capacity as usize,
        cfg.point_capacity as usize,
    );
    let human = human_world::Human::new(
        human_world::Prepared {
            character: cfg.character,
            classic: cfg.classic,
            rosters: cfg.rosters.into_iter().map(|r| r.classes).collect(),
            classes: cfg.classes,
        },
        choices,
        cfg.command_capacity as usize,
        posts,
        cfg.point_capacity as usize,
    );
    let effects = [
        w.sim.strings.intern_str("explosion"),
        w.sim.strings.intern_str("med_explosion"),
        w.sim.strings.intern_str("small_explosion"),
    ];
    super::f4_core::configure_tail(&mut w.battle, cfg.classic, cfg.local);
    {
        w.battle.tail = Some(State {
            owner,
            human: super::native_human::Driver {
                human,
                classic: cfg.classic,
                random: cfg.random,
                gameplay: cfg.gameplay,
                effects,
                spawn_index: cfg.spawn_index,
                fallback_path: cfg.fallback_path,
            },
            shell: Some(Store::default()),
            commands: Vec::with_capacity(cfg.command_capacity as usize),
            input: vec![0.0; cfg.command_capacity as usize * HsCommand::WORDS],
            random: cfg.random,
            gameplay: cfg.gameplay,
            effects,
        });
    }
    super::f4_live::descriptor_words(w)
}
/// HumanSpawn's commands share the tail owner's retained bank. Native hosts
/// publish a count and no address; Chromium keeps its existing typed view.
pub fn f4_tail_write_commands_in(w: &mut Root, commands: &[HsCommand]) -> bool {
    let Some(tail) = w.battle.tail.as_mut() else { return false };
    if commands.len() > tail.commands.capacity() || commands.len() > tail.input.len() / HsCommand::WORDS { return false; }
    // Check both owners before changing the command bank or its header.
    if w.battle.io.is_none() { return false; }
    for (row, command) in tail.input.chunks_exact_mut(HsCommand::WORDS).zip(commands) {
        command.put(row, &mut Vec::new());
    }
    let address = 0;
    #[cfg(target_arch = "wasm32")]
    let address = tail.input.as_ptr() as u32;
    let input = super::f4_live::tail_input_bank_mut_in(w).expect("checked tail input");
    input[F4TailInput::HUMAN_COMMANDS] = address as f64;
    input[F4TailInput::HUMAN_COUNT] = commands.len() as f64;
    true
}
pub fn f4_tail_commands_in(w: &mut Root) -> Option<&mut [f64]> {
    w.battle.tail.as_mut().map(|tail| &mut tail.input[..])
}
pub fn f4_tail_unit_class_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let c = F4UnitClass::take(row, heap);
    let p = &mut state(&mut w.battle).owner.prepared;
    p.registry.class(c.def, c.captures);
    p.observations.class(F4StatsClass {
        def: c.def,
        label: c.label,
    });
}
pub fn f4_tail_weapon_class_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let weapon = F4WeaponClass::take(row, heap);
    state(&mut w.battle).owner.prepared.observations.weapon(F4StatsWeapon {
        def: weapon.def,
        primary: weapon.primary,
        key: weapon.key,
        labels: weapon.labels,
    });
}
pub fn f4_tail_hull_class_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let h = F4HullClass::take(row, heap);
    state(&mut w.battle).owner.prepared.observations.hull(F4StatsHull {
        hull: h.hull,
        label: h.label,
    });
    let name = w.sim
        .store
        .vehicle
        .get(h.hull)
        .expect("prepared human hull")
        .name;
    state(&mut w.battle).human.human.choices.hull(h.hull, name);
}
pub fn f4_tail_post_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let p = F4PostSetup::take(row, heap);
    state(&mut w.battle)
        .human
        .human
        .choices
        .raw_post(p.post, p.raw_path);
}
pub fn f4_tail_pad_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let p = F4PadSetup::take(row, heap);
    let prepared = &mut state(&mut w.battle).owner.prepared;
    prepared.pads.adopt(p.index as usize, p.pad);
    for &r in &w.sim.store.mobile_posts.posts {
        let post = w.sim.store.mobile_post.get_mut(r).unwrap();
        if post.index == p.index as f64 {
            prepared.pads.reserve_post(p.index as usize, post);
        }
    }
}
pub fn f4_tail_hero_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let h = F4HeroSetup::take(row, heap);
    let names = &mut state(&mut w.battle).owner.prepared.hero_names;
    names.resize(names.len().max(h.class_name.0 as usize + 1), None);
    names[h.class_name.0 as usize] = h.name;
    super::f4_core::prepare_hero(&mut w.battle, h.class_name,h.name);
}
pub fn f4_tail_capture_team_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let c = F4CaptureTeam::take(row, heap);
    let table = &mut state(&mut w.battle).owner.prepared.captures;
    table.resize(table.len().max(c.team as usize + 1), None);
    table[c.team as usize] = Some(c.value);
}
pub fn f4_tail_ready_in(w: &mut Root) -> Option<&[f64]> {
    super::f4_core::prepare_tail_publications(w);
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);
    f3::publish_changed_in(w);
    super::f4_live::descriptor_words(w)
}
pub fn f4_tail_in(w: &mut Root) {
    let input = super::f4_live::tail_input(&mut w.battle);
    let before = crate::allocations::snapshot();
    let mut state = {
        w.battle.tail
            .take()
            .expect("battle tail cannot reenter")
    };
    state.decode_commands(&input, &w.sim.strings);
    let outer = f3::external_in(w);
    f3::with_services_in(w, |w, services| {
        let mut services = services.expect("native battle body services");
        let mut shell = state.shell.take().unwrap();
        std::mem::swap(&mut shell, &mut w.sim.store);
        let mut lifecycle = Native {
            local: state.owner.local,
            // A server can reuse the dormant local Character's roster slot
            // for a network player. That player belongs to his connection's
            // body, even when the retained Character reference is identical.
            character: if input.server { None } else { state.owner.prepared.local_character },
        };
        let mut actions = Actions { w,
            services:services.reborrow(),
            lifecycle: &mut lifecycle,
            human: &mut state.human,
            local: state.owner.local,
            effects: state.effects,
        };
        state.owner.step(
            &input,
            &mut shell,
            &Text,
            &state.commands,
            &mut actions,
            &mut Random(state.random),
            &mut Random(state.gameplay),
        );
        super::f4_core::mark_tail_in(&mut w.battle, &shell);
        std::mem::swap(&mut shell, &mut w.sim.store);
        state.shell = Some(shell);
    });
    {
        w.battle.tail = Some(state);
    }
    outer.finish(w, true);
    super::f4_live::count_tail(w, input.human_count as usize, before);
}

/// Roster commands run outside the phase and borrow its prepared post/class
/// readers. Mutating services still install the canonical Store at AI hooks.
pub(crate) fn spawning<T>(w: &mut Root,
    s: &mut Store,
    mut services: crate::units::f3::WorldServices<'_>,
    run: impl FnOnce(
        &mut Store,
        &mut (dyn SpawnContext + '_),
        &mut dyn for<'w> super::spawning::ContextDraws<dyn SpawnContext + 'w>,
    ) -> T,
) -> T {
    let mut state = w.battle.tail.take().expect("idle battle tail");
    let local = state.owner.local;
    let mut lifecycle = Native {
        local,
        character: state.owner.prepared.local_character,
    };
    let mut actions = Actions { w,
        services:services.reborrow(),
        lifecycle: &mut lifecycle,
        human: &mut state.human,
        local,
        effects: state.effects,
    };
    let mut world = super::owned_world::World {
        prepared: &mut state.owner.prepared,
        services: &mut actions,
        local,
        classic: state.owner.classic,
    };
    let result = run(s, &mut world, &mut Random(state.random));
    w.battle.tail = Some(state);
    result
}

/// Game.onRespawn is a stateful combat callback, before the live/tail slots.
/// It cannot be delayed with visual body notices to the end of a phase.
pub(crate) fn owns_player_in(w: &mut Root, unit:crate::store::Ref<Unit>)->bool {
    super::f4_core::owns_lifecycle_in(&mut w.battle) && state(&mut w.battle).owner.local==Some(unit)
}
pub(crate) fn auto_respawn_in(w: &mut Root, services:f3::WorldServices<'_>) {
    let mut state = w.battle.tail.take().expect("idle battle tail");
    let mut lifecycle=Native{local:state.owner.local,character:state.owner.prepared.local_character};
    let mut shell = state.shell.take().expect("retained tail shell");
    std::mem::swap(&mut shell, &mut w.sim.store);
    state.human.auto_respawn(w,&mut shell,&mut state.owner.prepared,services,&mut lifecycle,state.owner.local);
    std::mem::swap(&mut shell, &mut w.sim.store);
    state.shell = Some(shell);
    w.battle.tail = Some(state);
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);
}

/// MultiplayerWorld.spawn uses the same prepared post facts as local human
/// choices. Network spawn points use the connection ordinal's fixed uniform;
/// choosing a fallback post uses one half, never a gameplay RNG draw.
pub(crate) fn connection_spawn(w: &mut Root, character:crate::store::Ref<Character>, previous:Option<crate::store::Ref<Unit>>,
    class:StrId, post:Option<StrId>,ordinal:f64,mut services:f3::WorldServices<'_>)->Option<(crate::store::Ref<Unit>,StrId)> {
    let team=w.sim.store.character.get(character)?.team;
    let mut state=w.battle.tail.take().expect("idle battle tail");
    let result=(|| {
    if !state.human.human.prepared.rosters.get(team as usize).is_some_and(|r|r.contains(&class)) {return None;}
    let mut lifecycle=Native{local:None,character:None};let mut human=super::native_human::NoHuman;
    let mut actions=Actions{w,services:services.reborrow(),lifecycle:&mut lifecycle,human:&mut human,local:None,effects:state.effects};
    let mut world=super::owned_world::World{prepared:&mut state.owner.prepared,services:&mut actions,local:None,classic:state.owner.classic};
    let choices=&mut state.human.human.choices;
    let mut shell=state.shell.take().expect("retained tail shell");
    std::mem::swap(&mut shell,&mut world.services.w.sim.store);
    let choice=(|| {
    // Landed transports use the original ground-normal admission test.
    choices.begin_with(&mut shell,&mut world,|world,at,body| {
        let w = &mut *world.services.w;
        let world_id = f3::collision_world_in(&w.units);
        crate::units::native::ordnance_cast_in(w, world_id,[at.x,at.y,at.z],[0.,-1.,0.],12.,Some(body),true,false)
            .map(|hit|(three::Vector3::new(hit.point[0],hit.point[1],hit.point[2]),three::Vector3::new(hit.normal[0],hit.normal[1],hit.normal[2])))
    });
    let index=choices.choose(team,post,|a,b|f3::relation_in(&world.services.w.units,a,b),||0.5)?;
    let point=choices.point_with(&shell,index,team,&mut world,|world,a,b|f3::relation_in(&world.services.w.units,a,b),||(ordinal*0.381966)%1.0)?;
    let name=choices.name(index);
    Some((point, name))
    })();
    std::mem::swap(&mut shell,&mut world.services.w.sim.store);
    state.shell=Some(shell);
    let (point,name)=choice?;
    let w=&mut *world.services.w;
    let was_alive=previous.and_then(|r|w.sim.store.unit.get(r)).is_some_and(|u|u.alive);
    let unit=super::f4_factory::player(w,&F4PlayerRequest{character:Some(character),body:previous,class_name:class,
        post:Some(name),position:[point[0],point[1],point[2]],yaw:point[3],local:false},world.services.services.reborrow())?;
    let c=w.sim.store.character.get_mut(character).unwrap();c.unit=Some(unit);c.class_name=Some(class);
    if let Some(spawn)=&mut c.spawn {spawn.ready=false;spawn.timer=f64::INFINITY;}
    if !was_alive {if let Some(team)=w.sim.store.spawning.teams.iter_mut().find(|row|row.index==team) {team.living+=1.0;}}
    super::f4_core::mark_character_in(&mut w.battle, character);
    let u=w.sim.store.unit.get_mut(unit).unwrap();u.combat.player=true;
    let p=u.position();
    F4SpawnNotice{key:F3UnitKey::of(unit,u),unit,character:Some(character),class_name:class,
        post:Some(name),team:u.team,position:[p.x,p.y,p.z],local:false}.emit(&mut w.sim.events);
    Some((unit,name))
    })();
    w.battle.tail=Some(state);
    result
}

/// StubAI's body request is an explicit command outside the owned AI span.
/// Its native lifecycle shares waves' credit, body and spawn-notice ordering.
pub fn f4_unit_spawn_in(w: &mut Root)->i32 {
    use super::native_actions::Lifecycle;
    let (row,heap)=w.sim.calls.args();let input=F4UnitSpawn::take(row,heap);
    let Some(character)=super::f4_core::resolve(&mut w.battle, &w.sim.store,&input.target) else{return -1};
    if input.post.life!=1. {return -1;}
    let post=match input.post.kind {
        F4ObjectKind::Post=>{let r=crate::store::Ref::new(input.post.slot,input.post.generation);if !w.sim.store.post.contains(r){return -1};SpawnPost::Post(r)},
        F4ObjectKind::MobilePost=>{let r=crate::store::Ref::new(input.post.slot,input.post.generation);if !w.sim.store.mobile_post.contains(r){return -1};SpawnPost::MobilePost(r)},
        _=>return -1,
    };
    super::f4_live::count_external(&mut w.battle, F4UnitSpawn::WORDS);
    let outer=f3::external_in(w);
    let state=state(&mut w.battle);let mut lifecycle=Native{local:state.owner.local,character:state.owner.prepared.local_character};
    let unit=f3::with_services_in(w, |w, services|lifecycle.spawn(w,character,Some(input.class_name),post,input.position,Some(input.yaw),services.expect("native spawn body services")));
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);outer.finish(w, true);
    unit.map_or(-1,|r|r.slot as i32)
}

impl State {
    fn decode_commands(&mut self, input: &F4TailInput, strings: &crate::store::Strings) {
        // Clamp at the owner as well as the JS writer. A stale or oversized
        // count must never index the adjacent phase input or grow the command list.
        let count = (input.human_count as usize).min(self.commands.capacity()).min(self.input.len() / HsCommand::WORDS);
        self.commands.clear();
        for i in 0..count {
            let c = HsCommand::take(&self.input[i * HsCommand::WORDS..], &[]);
            let command = match c.kind {
                HsCommandKind::Team => Command::team(c.team),
                HsCommandKind::Spawn => c
                    .class_name
                    .and_then(|class| Command::spawn(class, c.post, strings)),
                HsCommandKind::Suicide => Some(Command::Suicide),
                HsCommandKind::Cancel => Some(Command::Cancel),
            };
            if let Some(command) = command {
                self.commands.push(command);
            }
        }
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn f4_tail_init() -> *const f64 { f4_tail_init_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_commands() -> *mut f64 { f4_tail_commands_in(crate::abi::facade::world()).map_or(std::ptr::null_mut(), |b| b.as_mut_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_unit_class() { f4_tail_unit_class_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_weapon_class() { f4_tail_weapon_class_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_hull_class() { f4_tail_hull_class_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_post() { f4_tail_post_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_pad() { f4_tail_pad_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_hero() { f4_tail_hero_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_capture_team() { f4_tail_capture_team_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_tail_ready() -> *const f64 { f4_tail_ready_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn f4_tail() { f4_tail_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_unit_spawn() ->i32 { f4_unit_spawn_in(crate::abi::facade::world()) }
}

pub(crate) trait SpawnContext: super::world::SpawningWorld + super::stats::TextWorld + super::spawning::RandomWorld {}
impl<T:super::world::SpawningWorld + super::stats::TextWorld + super::spawning::RandomWorld> SpawnContext for T {}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_commands {
    use super::*;
    #[test]
    fn tail_commands_are_owned_bounded_and_addressless() {
        let mut w = Root::new();
        assert!(!f4_tail_write_commands_in(&mut w, &[]));
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        w.sim.calls.result(F4BattleSetup::WORDS, |row, heap| F4BattleSetup::default().put(row, heap));
        super::super::f4_core::f4_battle_init_in(&mut w);
        w.sim.calls.result(F4LiveSetup::WORDS, |row, heap| F4LiveSetup::default().put(row, heap));
        super::super::f4_live::f4_live_init_in(&mut w);
        w.sim.calls.result(F4TailSetup::WORDS, |row, heap| F4TailSetup { command_capacity: 2, ..Default::default() }.put(row, heap));
        f4_tail_init_in(&mut w);
        let commands = [HsCommand { kind: HsCommandKind::Suicide, ..Default::default() }, HsCommand { kind: HsCommandKind::Cancel, ..Default::default() }];
        assert!(f4_tail_write_commands_in(&mut w, &commands));
        let input = super::super::f4_live::tail_input(&mut w.battle);
        assert_eq!((input.human_count, input.human_commands), (2, 0));
        let saved = f4_tail_commands_in(&mut w).unwrap().to_vec();
        assert!(!f4_tail_write_commands_in(&mut w, &[commands[0].clone(), commands[1].clone(), commands[0].clone()]));
        assert_eq!(f4_tail_commands_in(&mut w).unwrap(), saved);
        state(&mut w.battle).decode_commands(&input, &w.sim.strings);
        assert_eq!(state(&mut w.battle).commands.len(), 2);
        assert!(f4_tail_write_commands_in(&mut w, &[]));
        assert_eq!(super::super::f4_live::tail_input(&mut w.battle).human_count, 0);
    }
}
