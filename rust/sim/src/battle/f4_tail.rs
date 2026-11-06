//! Battlefield and battle rules enter once after items. Canonical Store rows
//! remain native through capture, deaths, queues, AI hooks and human choices.
use super::{
    battlefield::Battlefield, human::Command, human_posts::Choices, human_world,
    native_actions::Actions, native_lifecycle::Native, owned_world::Owner,
};
use crate::{abi::sim, gen::*, store::StrId, units::f3};
use std::ptr::addr_of_mut;
struct State {
    owner: Owner,
    human: super::native_human::Driver,
    shell: Option<Store>,
    commands: Vec<Command>,
    input: Vec<f64>,
    random: u32,
    gameplay: u32,
    effects: [StrId; 3],
}
static mut STATE: Option<State> = None;
fn state() -> &'static mut State {
    unsafe {
        (*addr_of_mut!(STATE))
            .as_mut()
            .expect("native battle tail initialized")
    }
}
pub(crate) fn reset() {
    unsafe {
        *addr_of_mut!(STATE) = None;
    }
}
// The pre-AI boundary also replaces CommandPosts.all's former JS refresh.
// State is lent out inside a tail; its World refreshes before its own hooks.
pub(crate) fn refresh_mobile_views(){
    let Some(state)=(unsafe{(*addr_of_mut!(STATE)).as_mut()}) else{return};
    super::fleet_posts::refresh_posts(&mut sim().store,&mut state.owner.prepared.pads);
    super::f4_core::mark_mobile();
}
struct Text;
impl super::stats::Text for Text {
    fn nonempty(&self, id: StrId) -> bool {
        !sim().strings.units(id).is_empty()
    }
}
pub(crate) struct Random(pub u32);
impl super::spawning::Draws for Random {
    fn next(&mut self) -> f64 {
        f3::random_next(self.0) as f64
    }
}
#[no_mangle]
pub extern "C" fn f4_tail_init() -> *const f64 {
    assert!(
        f3::active() && super::f4_core::active(),
        "canonical unit and battle owners required"
    );
    let (row, heap) = sim().calls.args();
    let cfg = F4TailSetup::take(row, heap);
    let edges: Vec<_> = cfg.boundary.polygons.into_iter().map(|p| p.edges).collect();
    let mut owner = Owner::new(
        &mut sim().store,
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
    let posts = sim().store.command_posts.posts.len() + sim().store.mobile_posts.posts.len();
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
        sim().strings.intern_str("explosion"),
        sim().strings.intern_str("med_explosion"),
        sim().strings.intern_str("small_explosion"),
    ];
    super::f4_core::configure_tail(cfg.classic, cfg.local);
    unsafe {
        *addr_of_mut!(STATE) = Some(State {
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
    super::f4_live::descriptor()
}
#[no_mangle]
pub extern "C" fn f4_tail_commands() -> *mut f64 {
    state().input.as_mut_ptr()
}
#[no_mangle]
pub extern "C" fn f4_tail_unit_class() {
    let (row, heap) = sim().calls.args();
    let c = F4UnitClass::take(row, heap);
    let p = &mut state().owner.prepared;
    p.registry.class(c.def, c.captures);
    p.observations.class(F4StatsClass {
        def: c.def,
        label: c.label,
    });
}
#[no_mangle]
pub extern "C" fn f4_tail_weapon_class() {
    let (row, heap) = sim().calls.args();
    let w = F4WeaponClass::take(row, heap);
    state().owner.prepared.observations.weapon(F4StatsWeapon {
        def: w.def,
        primary: w.primary,
        key: w.key,
        labels: w.labels,
    });
}
#[no_mangle]
pub extern "C" fn f4_tail_hull_class() {
    let (row, heap) = sim().calls.args();
    let h = F4HullClass::take(row, heap);
    state().owner.prepared.observations.hull(F4StatsHull {
        hull: h.hull,
        label: h.label,
    });
    let name = sim()
        .store
        .vehicle
        .get(h.hull)
        .expect("prepared human hull")
        .name;
    state().human.human.choices.hull(h.hull, name);
}
#[no_mangle]
pub extern "C" fn f4_tail_post() {
    let (row, heap) = sim().calls.args();
    let p = F4PostSetup::take(row, heap);
    state()
        .human
        .human
        .choices
        .raw_post(p.post, p.raw_path);
}
#[no_mangle]
pub extern "C" fn f4_tail_pad() {
    let (row, heap) = sim().calls.args();
    let p = F4PadSetup::take(row, heap);
    let prepared = &mut state().owner.prepared;
    prepared.pads.adopt(p.index as usize, p.pad);
    for &r in &sim().store.mobile_posts.posts {
        let post = sim().store.mobile_post.get_mut(r).unwrap();
        if post.index == p.index as f64 {
            prepared.pads.reserve_post(p.index as usize, post);
        }
    }
}
#[no_mangle]
pub extern "C" fn f4_tail_hero() {
    let (row, heap) = sim().calls.args();
    let h = F4HeroSetup::take(row, heap);
    let names = &mut state().owner.prepared.hero_names;
    names.resize(names.len().max(h.class_name.0 as usize + 1), None);
    names[h.class_name.0 as usize] = h.name;
    super::f4_core::prepare_hero(h.class_name,h.name);
}
#[no_mangle]
pub extern "C" fn f4_tail_capture_team() {
    let (row, heap) = sim().calls.args();
    let c = F4CaptureTeam::take(row, heap);
    let table = &mut state().owner.prepared.captures;
    table.resize(table.len().max(c.team as usize + 1), None);
    table[c.team as usize] = Some(c.value);
}
#[no_mangle]
pub extern "C" fn f4_tail_ready() -> *const f64 {
    super::f4_core::prepare_tail_publications();
    super::f4_core::mark_tail(&sim().store);
    f3::publish_changed();
    super::f4_live::descriptor()
}
#[no_mangle]
pub extern "C" fn f4_tail() {
    let input = super::f4_live::tail_input();
    let before = crate::allocations::snapshot();
    let mut state = unsafe {
        (*addr_of_mut!(STATE))
            .take()
            .expect("battle tail cannot reenter")
    };
    // Clamp at the owner as well as the JS writer. A stale or oversized
    // count must never index the adjacent phase input or grow the command list.
    let count = (input.human_count as usize).min(state.commands.capacity()).min(state.input.len() / HsCommand::WORDS);
    state.commands.clear();
    for i in 0..count {
        let c = HsCommand::take(&state.input[i * HsCommand::WORDS..], &[]);
        let command = match c.kind {
            HsCommandKind::Team => Command::team(c.team),
            HsCommandKind::Spawn => c
                .class_name
                .and_then(|class| Command::spawn(class, c.post, &sim().strings)),
            HsCommandKind::Suicide => Some(Command::Suicide),
            HsCommandKind::Cancel => Some(Command::Cancel),
        };
        if let Some(command) = command {
            state.commands.push(command);
        }
    }
    let outer = f3::external();
    f3::with_services(|services| {
        let services = services.expect("native battle body services");
        let mut shell = state.shell.take().unwrap();
        std::mem::swap(&mut shell, &mut sim().store);
        let mut lifecycle = Native {
            local: state.owner.local,
            // A server can reuse the dormant local Character's roster slot
            // for a network player. That player belongs to his connection's
            // body, even when the retained Character reference is identical.
            character: if input.server { None } else { state.owner.prepared.local_character },
        };
        let mut actions = Actions {
            services,
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
        super::f4_core::mark_tail(&shell);
        std::mem::swap(&mut shell, &mut sim().store);
        state.shell = Some(shell);
    });
    unsafe {
        *addr_of_mut!(STATE) = Some(state);
    }
    drop(outer);
    super::f4_live::count_tail(input.human_count as usize, before);
}

/// Roster commands run outside the phase and borrow its prepared post/class
/// readers. Mutating services still install the canonical Store at AI hooks.
pub(crate) fn spawning<T>(
    s: &mut Store,
    services: crate::units::f3::WorldServices<'_>,
    run: impl FnOnce(
        &mut Store,
        &mut dyn super::world::SpawningWorld,
        &mut dyn super::spawning::Draws,
    ) -> T,
) -> T {
    let state = state();
    let local = state.owner.local;
    let mut lifecycle = Native {
        local,
        character: state.owner.prepared.local_character,
    };
    let mut actions = Actions {
        services,
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
    run(s, &mut world, &mut Random(state.random))
}

/// Game.onRespawn is a stateful combat callback, before the live/tail slots.
/// It cannot be delayed with visual body notices to the end of a phase.
pub(crate) fn owns_player(unit:crate::store::Ref<Unit>)->bool {
    super::f4_core::owns_lifecycle() && state().owner.local==Some(unit)
}
pub(crate) fn auto_respawn(services:f3::WorldServices<'_>) {
    let state=state();
    let mut lifecycle=Native{local:state.owner.local,character:state.owner.prepared.local_character};
    state.human.auto_respawn(&mut sim().store,&mut state.owner.prepared,services,&mut lifecycle,state.owner.local);
    super::f4_core::mark_tail(&sim().store);
}

/// MultiplayerWorld.spawn uses the same prepared post facts as local human
/// choices. Network spawn points use the connection ordinal's fixed uniform;
/// choosing a fallback post uses one half, never a gameplay RNG draw.
pub(crate) fn connection_spawn(character:crate::store::Ref<Character>, previous:Option<crate::store::Ref<Unit>>,
    class:StrId, post:Option<StrId>,ordinal:f64,services:f3::WorldServices<'_>)->Option<(crate::store::Ref<Unit>,StrId)> {
    let state=state();let team=sim().store.character.get(character)?.team;
    if !state.human.human.prepared.rosters.get(team as usize).is_some_and(|r|r.contains(&class)) {return None;}
    let mut lifecycle=Native{local:None,character:None};let mut human=super::native_human::NoHuman;
    let mut actions=Actions{services,lifecycle:&mut lifecycle,human:&mut human,local:None,effects:state.effects};
    let mut world=super::owned_world::World{prepared:&mut state.owner.prepared,services:&mut actions,local:None,classic:state.owner.classic};
    let choices=&mut state.human.human.choices;
    // Landed transports use the original ground-normal admission test.
    choices.begin(&mut sim().store,&mut world,|at,body| {
        crate::units::native::ordnance_cast(f3::collision_world(),[at.x,at.y,at.z],[0.,-1.,0.],12.,Some(body),true,false)
            .map(|hit|(three::Vector3::new(hit.point[0],hit.point[1],hit.point[2]),three::Vector3::new(hit.normal[0],hit.normal[1],hit.normal[2])))
    });
    let index=choices.choose(team,post,f3::relation,||0.5)?;
    let point=choices.point(&sim().store,index,team,&mut world,f3::relation,||(ordinal*0.381966)%1.0)?;
    let name=choices.name(index);
    let was_alive=previous.and_then(|r|sim().store.unit.get(r)).is_some_and(|u|u.alive);
    let unit=super::f4_factory::player(&F4PlayerRequest{character:Some(character),body:previous,class_name:class,
        post:Some(name),position:[point[0],point[1],point[2]],yaw:point[3],local:false},world.services.services.reborrow())?;
    let c=sim().store.character.get_mut(character).unwrap();c.unit=Some(unit);c.class_name=Some(class);
    if let Some(spawn)=&mut c.spawn {spawn.ready=false;spawn.timer=f64::INFINITY;}
    if !was_alive {if let Some(team)=sim().store.spawning.teams.iter_mut().find(|row|row.index==team) {team.living+=1.0;}}
    super::f4_core::mark_character(character);
    let u=sim().store.unit.get_mut(unit).unwrap();u.combat.player=true;
    let p=u.position();
    F4SpawnNotice{key:F3UnitKey::of(unit,u),unit,character:Some(character),class_name:class,
        post:Some(name),team:u.team,position:[p.x,p.y,p.z],local:false}.emit(&mut sim().events);
    Some((unit,name))
}

/// StubAI's body request is an explicit command outside the owned AI span.
/// Its native lifecycle shares waves' credit, body and spawn-notice ordering.
#[no_mangle]
pub extern "C" fn f4_unit_spawn()->i32 {
    use super::native_actions::Lifecycle;
    let (row,heap)=sim().calls.args();let input=F4UnitSpawn::take(row,heap);
    let Some(character)=super::f4_core::resolve(&sim().store,&input.target) else{return -1};
    if input.post.life!=1. {return -1;}
    let post=match input.post.kind {
        F4ObjectKind::Post=>{let r=crate::store::Ref::new(input.post.slot,input.post.generation);if !sim().store.post.contains(r){return -1};SpawnPost::Post(r)},
        F4ObjectKind::MobilePost=>{let r=crate::store::Ref::new(input.post.slot,input.post.generation);if !sim().store.mobile_post.contains(r){return -1};SpawnPost::MobilePost(r)},
        _=>return -1,
    };
    super::f4_live::count_external(F4UnitSpawn::WORDS);
    let outer=f3::external();
    let state=state();let mut lifecycle=Native{local:state.owner.local,character:state.owner.prepared.local_character};
    let unit=f3::with_services(|services|lifecycle.spawn(character,Some(input.class_name),post,input.position,Some(input.yaw),services.expect("native spawn body services")));
    super::f4_core::mark_tail(&sim().store);drop(outer);
    unit.map_or(-1,|r|r.slot as i32)
}
