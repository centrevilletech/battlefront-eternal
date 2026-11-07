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
pub(crate) fn wire_roster_in(battle: &mut BattleOwner, team: usize) -> &[StrId] {
    state(battle).human.human.prepared.rosters.get(team).map_or(&[], Vec::as_slice)
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
struct Text;
impl Text { fn nonempty_in(&self, w: &Root, id: StrId) -> bool { !w.sim.strings.units(id).is_empty() } }
pub(crate) struct Random(pub u32);
impl Random {
    fn next_in(&mut self, _w: &mut Root) -> f64 { f3::random_next(self.0) as f64 }
}
pub fn f4_tail_init_in(w: &mut Root) -> *const f64 {
    assert!(
        f3::active() && super::f4_core::active_in(&mut w.battle),
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
    super::f4_live::descriptor(w)
}
pub fn f4_tail_commands_in(w: &mut Root) -> *mut f64 {
    state(&mut w.battle).input.as_mut_ptr()
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
pub fn f4_tail_ready_in(w: &mut Root) -> *const f64 {
    super::f4_core::prepare_tail_publications(w);
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);
    f3::publish_changed();
    super::f4_live::descriptor(w)
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
    let outer = f3::external();
    f3::with_services(|services| {
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
    drop(outer);
    super::f4_live::count_tail(w, input.human_count as usize, before);
}

/// Roster commands run outside the phase and borrow its prepared post/class
/// readers. Mutating services still install the canonical Store at AI hooks.
pub(crate) fn spawning<T>(w: &mut Root,
    s: &mut Store,
    mut services: crate::units::f3::WorldServices<'_>,
    run: impl FnOnce(
        &mut Store,
        &mut dyn super::world::SpawningWorld,
        &mut dyn super::spawning::Draws,
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
    choices.begin(&mut shell,&mut world,|at,body| {
        crate::units::native::ordnance_cast(f3::collision_world(),[at.x,at.y,at.z],[0.,-1.,0.],12.,Some(body),true,false)
            .map(|hit|(three::Vector3::new(hit.point[0],hit.point[1],hit.point[2]),three::Vector3::new(hit.normal[0],hit.normal[1],hit.normal[2])))
    });
    let index=choices.choose(team,post,f3::relation,||0.5)?;
    let point=choices.point(&shell,index,team,&mut world,f3::relation,||(ordinal*0.381966)%1.0)?;
    let name=choices.name(index);
    Some((point, name))
    })();
    std::mem::swap(&mut shell,&mut world.services.w.sim.store);
    state.shell=Some(shell);
    let (point,name)=choice?;
    let w=&mut *world.services.w;
    let was_alive=previous.and_then(|r|w.sim.store.unit.get(r)).is_some_and(|u|u.alive);
    let unit=super::f4_factory::player(&F4PlayerRequest{character:Some(character),body:previous,class_name:class,
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
    let outer=f3::external();
    let state=state(&mut w.battle);let mut lifecycle=Native{local:state.owner.local,character:state.owner.prepared.local_character};
    let unit=f3::with_services(|services|lifecycle.spawn(w,character,Some(input.class_name),post,input.position,Some(input.yaw),services.expect("native spawn body services")));
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);drop(outer);
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

// facade shims
#[cfg(target_arch = "wasm32")]
pub(crate) fn wire_roster(team: usize) -> &'static [StrId] { wire_roster_in(&mut crate::abi::cold().battle, team) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn refresh_mobile_views() { refresh_mobile_views_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn initial_human(character:Option<crate::store::Ref<Character>>) { initial_human_in(&mut crate::abi::cold().battle, character) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_init() -> *const f64 { f4_tail_init_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_commands() -> *mut f64 { f4_tail_commands_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_unit_class() { f4_tail_unit_class_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_weapon_class() { f4_tail_weapon_class_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_hull_class() { f4_tail_hull_class_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_post() { f4_tail_post_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_pad() { f4_tail_pad_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_hero() { f4_tail_hero_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_capture_team() { f4_tail_capture_team_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail_ready() -> *const f64 { f4_tail_ready_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_tail() { f4_tail_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn owns_player(unit:crate::store::Ref<Unit>) ->bool { owns_player_in(crate::abi::world(), unit) }
#[cfg(target_arch = "wasm32")]
pub(crate) fn auto_respawn(services:f3::WorldServices<'_>) { auto_respawn_in(crate::abi::world(), services) }
#[cfg(target_arch = "wasm32")] #[no_mangle]
pub extern "C" fn f4_unit_spawn() ->i32 { f4_unit_spawn_in(crate::abi::world()) }
#[cfg(target_arch = "wasm32")]
impl super::stats::Text for Text { fn nonempty(&self, id: StrId) -> bool { self.nonempty_in(crate::abi::world(), id) } }
#[cfg(target_arch = "wasm32")]
impl super::spawning::Draws for Random { fn next(&mut self) -> f64 { self.next_in(crate::abi::world()) } }
