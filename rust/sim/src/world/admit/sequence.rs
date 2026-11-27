//! The owner construction sequence: game.js createGame headless, then
//! createLocalServer, as the ordered stages below. Each stage drives the
//! existing `_in` bodies with the rows made for it, exactly as the client's
//! `bridge.callArgs(...)` then export call does; nothing of an owner is
//! decided here, only the order and the handles kept for the host.
use super::rows::{self, Call, Cx};
use super::{Admitted, AdmitError, Options, Rows};
use crate::gen::Member;
use crate::store::Ref;
use crate::world::owner::World;
use bfe_content::content::Content;
use bfe_content::files::Files;

/// The stages in order. A failure names the stage it stopped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Loading the battle's files and making the rows.
    Content,
    /// validateStartupClass (f4_startup_class).
    Startup,
    /// The collision kernel from content triangles.
    Kernels,
    /// The aim cameras (camera_init, camera_add_definition).
    Cameras,
    /// missionTeams names (ai_mission_teams).
    Teams,
    /// The store's reservation, ordnance definitions and items (ord_def, items_def, items_pools).
    Store,
    /// attachF3Combat (cmb_new, cmb_class, cmb_weapon_set, cmb_explosion).
    Combat,
    /// attachF2 and the hull content (veh_pack, f2_init, veh_class, f2_register, seats, firing, colliders).
    Fleet,
    /// attachF3 (f3_init, registries, props, hulls, items, recon).
    Units,
    /// bindWeaponPolicyContent (ai_weapon_policies).
    Policies,
    /// createNativeAI (ai_world_content, begin, roster, activate) and the mission teams' calls.
    Ai,
    /// createWorldSim and the world clock/events/local owners.
    World,
    /// F4Live, then F4Battle inside F4Tail (game.js 3136-3143, after the AI
    /// roster): f4_live_init, f4_battle_init/capacity, f4_tail_init,
    /// connections, roster.
    LiveTail,
    /// createMultiplayerWorld and createSnapshotHost: players, mailbox, wire session, host and publication.
    Session,
    /// addPlayer, the body placed, the welcome packets.
    Player,
    /// Not a stage of the sequence: an export a host drives itself through
    /// `Context::invoke_export`.
    Host,
}
/// The order the host constructs them in.
pub const ORDER: [Stage; 15] = [Stage::Content, Stage::Startup, Stage::Kernels, Stage::Cameras, Stage::Teams, Stage::Store,
    Stage::Combat, Stage::Fleet, Stage::Units, Stage::Policies, Stage::Ai, Stage::World, Stage::LiveTail, Stage::Session, Stage::Player];

/// One call against the world as the client makes it: the strings interned
/// (the builder interned them through the context's copy of the table in
/// the same order, so the world gives the same ids), the scratch bytes
/// written, then the argument row and heap in the exchange the `_in` body
/// reads with `sim.calls.args()`.
pub(crate) fn args(world: &mut World, call: &Call) -> Result<(), AdmitError> {
    for units in &call.strings {
        let id = world.sim.strings.intern(units);
        // A builder that interned through another table would misplace
        // every id after this one: stop rather than admit a wrong world.
        if world.sim.strings.list.get(id.0 as usize).map(|s| s.as_slice()) != Some(units.as_slice()) {
            return Err(AdmitError::Content { stage: Stage::Content, message: "string table out of step with the builders".into() });
        }
    }
    if !call.scratch.is_empty() { world.sim.scratch_room(call.scratch.len()).copy_from_slice(&call.scratch); }
    if !call.row.is_empty() {
        world.sim.calls.result(call.row.len(), |row, heap| {
            row.copy_from_slice(&call.row);
            heap.extend_from_slice(&call.heap);
        });
    }
    Ok(())
}

/// One stage's calls against their bodies (dispatch.rs). Every answer is fed
/// back into the context for the owners after this stage.
pub(crate) fn replay(world: &mut World, stage: Stage, calls: &[(Stage, &'static str, Call)], cx: &mut Cx) -> Result<(), AdmitError> {
    for (_, name, call) in calls {
        args(world, call)?;
        let answer = super::dispatch::call(world, stage, name, call)?;
        super::dispatch::record(cx, stage, name, answer)?;
        // A body that interned on its own moved the world's table past the
        // context's copy; the next rows plan against the world's.
        cx.refresh_strings(&world.sim.strings);
    }
    Ok(())
}

/// Runs every stage on the owning world, in order: asks the stage's owners
/// for their calls through the shared context, dispatches them, feeds back
/// what they returned. Stops at the first stage without a body
/// (Unavailable, naming it). A stage that ran leaves its owner admitted.
pub fn run(world: &mut World, content: &Content, files: &dyn Files, options: &Options) -> Result<(Admitted, Rows), AdmitError> {
    let mut rows = Rows::default();
    run_into(world, content, files, options, &mut rows).map(|admitted| (admitted, rows))
}

/// `run` with the rows kept in the caller's buffer: a driver comparing
/// them with a recording keeps what was dispatched before a failure.
pub fn run_into(world: &mut World, content: &Content, files: &dyn Files, options: &Options, rows: &mut Rows) -> Result<Admitted, AdmitError> {
    let doors = doors_of(content)?;
    if world.units.map_world_bounds.is_some() || (content.battle.family == bfe_content::GameFamily::Swbf2 && crate::units::f3::active_in(&world.units)) {
        let mut fresh = World::new();
        world.battle.swap_rule_factory(&mut fresh.battle);
        let result = run_fresh(&mut fresh, content, files, options, rows, doors);
        if result.is_ok() {
            std::mem::swap(world, &mut fresh);
        } else {
            world.battle.swap_rule_factory(&mut fresh.battle);
        }
        return result;
    }
    run_fresh(world, content, files, options, rows, doors)
}

fn run_fresh(world: &mut World, content: &Content, files: &dyn Files, options: &Options, rows: &mut Rows, doors: Doors) -> Result<Admitted, AdmitError> {
    let defs = bfe_content::defs::Defs::build(content).map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    if options.family == bfe_family::GameFamily::Swbf2 {
        register_classes(world, content, &defs)?;
    }
    let mut cx = Cx::new(content, &defs, files, options, world.sim.strings.clone());
    construct(world, content, &defs, files, &mut cx, rows, doors)
}

pub(super) use crate::space::door_animation::DoorSetup as Doors;

/// What a 2005 battle's content must hold before its world is built, and what
/// its hangar doors are bound to; nothing of a 2004 battle.
pub(super) fn doors_of(content: &Content) -> Result<Doors, AdmitError> {
    let error = |message| AdmitError::Content { stage: Stage::Content, message };
    if content.battle.family == bfe_content::GameFamily::Swbf2 {
        crate::space::flight::validate_combat_content(content).map_err(error)?;
    }
    if !crate::space::SpaceBattlefield::should_attach(content) { return Ok(Doors::default()); }
    let doors = content.battle.swbf2.as_ref().map(|s| crate::space::door_animation::load_for_battle(&s.map_admission)).transpose();
    Ok(doors.map_err(error)?.unwrap_or_default())
}

/// The stages of an admission and everything that follows them before the
/// first tick, for a local battle and a server's alike: a 2005 map's bounds
/// before the stages; after them its space battlefield, the page's streams
/// (so each host draws from the same ones), the bodies and a 2005 battle's
/// infantry.
pub(super) fn construct(world: &mut World, content: &Content, defs: &bfe_content::defs::Defs, files: &dyn Files, cx: &mut Cx, rows: &mut Rows, doors: Doors)
    -> Result<Admitted, AdmitError>
{
    let family = cx.options.family;
    rows.setup = rows::setup(&content.battle);
    let mut admitted = Admitted { family, setup: rows.setup.clone(), ..Default::default() };
    world.units.map_world_bounds = (content.battle.family == bfe_content::GameFamily::Swbf2)
        .then(|| crate::units::damage::MapWorldBounds::from_swbf2_map(&content.battle.world));
    run_stages(world, content, cx, rows, &mut admitted, &ORDER)?;
    world.space = if crate::space::SpaceBattlefield::should_attach(content) {
        Some(Box::new(crate::space::SpaceBattlefield::build(world, content, cx, doors)))
    } else {
        None
    };
    push_page_streams(world, cx)?;
    admitted.units = super::spawn::units(world)?;
    if family == bfe_family::GameFamily::Swbf2 {
        crate::units::infantry::admit(world, content, defs, files, admitted.handles.mailbox, admitted.handles.player)
            .map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    }
    Ok(admitted)
}

/// F3Random's hand-over before the first tick, which the client made by
/// writing the phase's memory rather than through a call:
/// `configure(gameplay, grenade-spin, gameplay, seat-weapons)` binds the
/// gameplay, spin, drop and seat globals to those streams' ids (a repeated
/// callback keeps its id), and `push` writes each entry: the gameplay
/// stream present with its state after the one draw that placed the player
/// (`points[Math.floor(gameplayRandom() * points.length)]`), the lazy
/// grenade-spin and seat-weapons streams absent at their mixed seeds
/// (RandomStreams' `initialState`) until the world first draws them.
pub(crate) fn push_page_streams(world: &mut World, cx: &mut Cx) -> Result<(), AdmitError> {
    use crate::gen::{F3RandomGlobals, F3RandomState};
    use rows::battle::streams;
    let seed = cx.options.seed;
    let ids = [streams::GAMEPLAY, streams::GRENADE_SPIN, streams::SEAT_WEAPONS].map(|key| cx.streams.register(key));
    let globals = F3RandomGlobals { gameplay: ids[0], spin: ids[1], drop: ids[0], seat: ids[2] };
    let mut row = vec![0.0; F3RandomGlobals::WORDS];
    globals.put(&mut row, &mut Vec::new());
    let bytes = |words: &[f64]| -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() };
    let refused = || AdmitError::Refused { stage: Stage::Player, call: "F3Random.push".into() };
    if !matches!(super::banks::write(world, "F3_RANDOM_GLOBALS", &bytes(&row)), Some(super::Returned::None)) { return Err(refused()); }
    let mut words = match super::banks::read(world, "F3_RANDOM_STATES") {
        Some(super::Returned::Row(words)) => words,
        _ => return Err(refused()),
    };
    let mut gameplay = jsrt::rng::ClassicRandom::from_state(jsrt::hash::fnv1a_mix(seed, "gameplay"));
    gameplay.next_u32();
    let entries = [
        (ids[0], F3RandomState { state: gameplay.state, present: true, sequence: 0 }),
        (ids[1], F3RandomState { state: jsrt::hash::fnv1a_mix(seed, "grenade-spin"), present: false, sequence: 0 }),
        (ids[2], F3RandomState { state: jsrt::hash::fnv1a_mix(seed, "seat-weapons"), present: false, sequence: 0 }),
    ];
    for (id, entry) in entries {
        let at = id as usize * F3RandomState::WORDS;
        let slot = words.get_mut(at..at + F3RandomState::WORDS).ok_or_else(refused)?;
        entry.put(slot, &mut Vec::new());
    }
    if !matches!(super::banks::write(world, "F3_RANDOM_STATES", &bytes(&words)), Some(super::Returned::None)) { return Err(refused()); }
    Ok(())
}

/// A 2005 battle's soldier classes take their movement numbers from the
/// matched content (`movement::soldier::swbf2::classes`), in the class list's
/// order, before admission records them ([`selection_calls`]). Numbers
/// registered before are dropped: only the content the battle's digest covers
/// supplies them. A class whose numbers cannot be read stays without; the
/// session refuses the battle and says why.
pub(crate) fn register_classes(world: &mut World, content: &Content, defs: &bfe_content::defs::Defs) -> Result<(), AdmitError> {
    world.movement.swbf2.clear_classes();
    for (name, tuning) in crate::movement::soldier::swbf2::classes::of_battle(content, defs) {
        let Ok(tuning) = tuning else { continue };
        let class = world.sim.strings.intern_str(&name);
        world.movement.swbf2.set_class(class, tuning)
            .map_err(|e| AdmitError::Content { stage: Stage::Content, message: format!("{name}: {e}") })?;
    }
    Ok(())
}

/// The movement numbers a 2005 world holds, by class name, in the order they
/// were registered.
pub fn swbf2_classes(world: &World) -> Vec<(String, crate::movement::soldier::swbf2::Tuning)> {
    world.movement.swbf2.classes().iter()
        .map(|&(class, tuning)| (String::from_utf16_lossy(world.sim.strings.units(class)), tuning))
        .collect()
}

/// The selection a recorded admission opens with: the world's game by its
/// release year (`movement_family`, a number, so it interns no name), then one `movement_swbf2_class` call per class the
/// world holds numbers for, so a world admitted call by call selects its game
/// before any class's numbers are registered.
fn selection_calls(world: &World) -> Vec<(Stage, &'static str, Call)> {
    let family = Call { args: vec![crate::movement::family_code(world.movement.family())], ..Default::default() };
    std::iter::once((Stage::Content, "movement_family", family)).chain(world.movement.swbf2.classes().iter().map(|&(class, tuning)| {
        let mut args = vec![f64::from(class.0)];
        args.extend(crate::movement::soldier::swbf2::words::put(&tuning));
        (Stage::Content, "movement_swbf2_class", Call { args, strings: vec![world.sim.strings.units(class).to_vec()], ..Default::default() })
    })).collect()
}

/// The stages named, in the order given, on a world whose earlier stages
/// are already done and a context the earlier owners already filled. The
/// whole admission is `run`; a partial proof (admit-replay's recorded
/// prefix, partial.rs) runs a lane's stages alone over the host's recorded
/// construction of the stages before them.
pub fn run_stages(world: &mut World, content: &Content, cx: &mut Cx, rows: &mut Rows, admitted: &mut Admitted, stages: &[Stage]) -> Result<(), AdmitError> {
    // Both admission entries select the battle's movement profile here.
    if stages.first() == Some(&Stage::Content) {
        world.movement.select_family(cx.options.family)
            .map_err(|message| AdmitError::Content { stage: Stage::Content, message: message.into() })?;
    }
    // createGame: `new Units()` numbers members from 1.
    // The host hands the registry over whole before attachF3 (the KIND_UNITS
    // import); the owning world keeps it as members are added instead.
    if stages.first() == Some(&Stage::Content) && world.sim.store.units.list.is_empty() { world.sim.store.units.next_id = 1.0; }
    for &stage in stages {
        match stage {
            // A 2005 battle's recorded admission opens with its game, then its
            // classes' movement numbers in the order they were registered, so
            // a host that admits call by call selects the same game and
            // registers the same numbers before any other name is interned. A
            // 2004 battle records nothing here.
            Stage::Content => {
                if cx.options.family == bfe_family::GameFamily::Swbf2 {
                    let calls = selection_calls(world);
                    replay(world, stage, &calls, cx)?;
                    rows.calls.push((stage, calls));
                }
            }
            // validateStartupClass runs on the client before loading, never in a
            // headless world (`!headless && !params.has("network")`);
            // the row stays for the comparison tests.
            Stage::Startup => {}
            Stage::Kernels => { cx.ids.kernel = Some(super::kernels::install(world, content)?); }
            // PageCameras constructs across a return edge: camera_init
            // answers the owner handle camera_add_definition is given, so
            // the definition row is built only after the first call ran.
            Stage::Cameras => {
                let mut calls = rows::cameras::build_init(cx)?;
                replay(world, stage, &calls, cx)?;
                let definition = rows::cameras::build_definition(cx)?;
                replay(world, stage, &definition, cx)?;
                calls.extend(definition);
                rows.calls.push((stage, calls));
            }
            // createWorldSim only wraps the clock, event and local exports the
            // world already owns; headless game.js then adopts the viewpoint,
            // reserves the factory and prepares the stock (rows/battle/stock.rs).
            Stage::World => {
                let (mut calls, mut stages) = (Vec::new(), Vec::new());
                let result = rows::battle::stock::run(world, cx, &mut calls, &mut stages);
                rows.cold_stages.extend(stages);
                rows.calls.push((stage, calls));
                result?;
            }
            // F4Live, F4Battle and F4Tail construct across return edges
            // (handles, descriptors), then the AI activates and the viewpoint
            // is bound (rows/battle/live.rs).
            Stage::LiveTail => {
                let mut calls = Vec::new();
                let result = rows::battle::live::run(world, cx, &mut calls);
                rows.calls.push((stage, calls));
                result?;
            }
            Stage::Session => {
                let mut session = super::session::run(world, cx)?;
                // createLocalServer.welcome: the two text events, then the
                // first publication at the admission's clocks.
                let (at, unix_ms) = (cx.at, cx.unix_ms);
                let initial = super::session::publish_initial(world, cx, &mut session, at, unix_ms)?;
                admitted.welcome = session.packets.clone();
                admitted.publication = initial.bytes;
                admitted.publication_output = initial.output;
                admitted.handles = session.handles;
                admitted.wire_bounds = session.wire_bounds;
                admitted.player_id = session.player_id;
                admitted.registry = session.registry;
                rows.cold_stages.extend(session.cold_stages);
                // The connection's calls (Player) sit between the session's
                // roster set-up and its snapshot host: keep that order, one
                // chunk per run of consecutive calls of a stage.
                for entry in session.calls {
                    match rows.calls.last_mut() {
                        Some((stage, calls)) if *stage == entry.0 && matches!(*stage, Stage::Session | Stage::Player) => calls.push(entry),
                        _ => rows.calls.push((entry.0, vec![entry])),
                    }
                }
            }
            Stage::Player => {}
            // `units.add(new Unit({... human: true}))` gives the
            // dormant viewpoint the registry's first id before attachF3Combat
            // constructs its combat; Units.add keeps generation 0.
            // PlayerBodies constructs the player's page body before the
            // combat attaches and places it after (rows/battle/stock.rs).
            Stage::Combat => {
                let mut calls = Vec::new();
                rows::battle::stock::construct_viewpoint_body(world, cx, &mut calls)?;
                let id = world.sim.store.units.add(|id| Member::Unit(Ref::new(id, 0)));
                cx.ids.registry.push(("viewpoint".into(), id as f64));
                let owners = rows::build(stage, cx)?;
                replay(world, stage, &owners, cx)?;
                calls.extend(owners);
                rows::battle::stock::spawn_viewpoint_body(world, cx, &mut calls)?;
                rows.calls.push((stage, calls));
            }
            // The host-side fleet construction (fleet.rs): new Vehicles, attachF2.
            Stage::Fleet => super::fleet::construct(world, cx, rows)?,
            // attachF3, then game.js 1392 right after it: `unitsSim.random
            // .configure(gameplayRandom, grenade-spin, gameplayRandom,
            // seatRandom)` registers the host's first streams.
            Stage::Units => {
                // attachF3 reads collision.bodySerial as f3_init's bodySerial:
                // the fleet stage published it (a prepared variant's body
                // gives its serial back, so the count is the live list's).
                rows::battle::bindings::fill(world, cx)?;
                let calls = rows::build(stage, cx)?;
                replay(world, stage, &calls, cx)?;
                rows.calls.push((stage, calls));
                for key in [rows::battle::streams::GAMEPLAY, rows::battle::streams::GRENADE_SPIN,
                    rows::battle::streams::GAMEPLAY, rows::battle::streams::SEAT_WEAPONS] {
                    cx.streams.register(key);
                }
                // The gameplay binding every body's instrument reads from here
                // on (the stock's random bindings); the rest of the battle
                // facts follow the roster and the viewpoint's adoption.
                let gameplay = cx.streams.register(rows::battle::streams::GAMEPLAY);
                let facts = cx.ids.battle.take().unwrap_or_default();
                cx.ids.battle = Some(rows::battle::Facts { gameplay, ..facts });
            }
            // after bindWeaponPolicyContent: the fielded classes'
            // corpse stock (UnitFactory.build/release), then createNativeAI
            // and the roster's missionTeams, each piece against the string
            // table its predecessor left (stock.rs, rows/ai/drive.rs).
            Stage::Ai => {
                // The stock's instrumentation reads the battle facts (the
                // gameplay stream's binding) as the factory stands now.
                cx.ids.battle = Some(rows::battle::stock::facts(world, cx)?);
                rows.calls.push((stage, super::stock::build(world, cx)?));
                // createNativeAI imports the posts, the mobile posts and the
                // battle before its first call (rows/battle/posts.rs).
                let mut imports = Vec::new();
                let constructed = rows::battle::posts::construct(world, cx, &mut imports);
                rows.calls.push((stage, imports));
                constructed?;
                // 27: the AI's two bare generators register
                // at ai_world_begin (streamA, streamB), after the stock.
                let stream_a = cx.streams.register(rows::battle::streams::WORLD_RANDOM);
                let stream_b = cx.streams.register(rows::battle::streams::WORLD_RANDOM_B);
                // setupRows: the streams' ids and their generators' states, A
                // being game.js's `new ClassicRandom(SEED)` (nothing has drawn
                // it before createNativeAI), B createNativeAI's own
                // `new ClassicRandom(seed ^ 0x9e3779b9)`.
                let mut receipts = cx.ids.ai.take().unwrap_or_default();
                receipts.setup.stream_a = stream_a;
                receipts.setup.stream_b = stream_b;
                receipts.setup.state_a = cx.options.seed;
                receipts.setup.state_b = cx.options.seed ^ 0x9e37_79b9;
                receipts.setup.unit_scale = cx.options.bots.unit_scale();
                receipts.setup.tier = cx.options.bots.tier();
                cx.ids.ai = Some(receipts);
                let calls = super::rows::ai::run_stage(world, cx)?;
                rows.calls.push((stage, calls));
            }
            _ => {
                let calls = rows::build(stage, cx)?;
                replay(world, stage, &calls, cx)?;
                rows.calls.push((stage, calls));
            }
        }
    }
    Ok(())
}


#[cfg(test)]
mod class_number_tests {
    use super::*;
    use crate::movement::soldier::swbf2::{words, ControlSpeed, Tuning};

    fn tuning(capacity: f64, frames: u32) -> Tuning {
        let row = ControlSpeed { forward: 1.0, strafe: 0.8, turn: 0.6 };
        Tuning { stand: row, crouch: row, sprint: row, jump: row, jet: row, jetpack: None, roll: row, tumble: row,
            sprint_accelerate_time: 0.25, energy_capacity: capacity, energy_floor: 0.0, energy_restore: 3.0, energy_restore_idle: 5.0,
            energy_drain_sprint: 8.0, energy_min_sprint: 10.0, energy_cost_jump: 2.0, energy_cost_jump_sprint: Some(4.0),
            energy_cost_roll: 9.0, jump_height: 1.3, jump_forward_speed_factor: 1.1, jump_strafe_speed_factor: 0.9,
            roll_speed_factor: 1.5, roll_frames: frames, recover_from_tumble: false, getup_front_seconds: 1.0,
            getup_back_seconds: 1.0 }
    }

    // What one world holds, as names and words.
    fn held(world: &World) -> Vec<(Vec<u16>, Vec<[u8; 8]>)> {
        world.movement.swbf2.classes().iter()
            .map(|(id, t)| (world.sim.strings.units(*id).to_vec(), words::put(t).iter().map(|w| w.to_le_bytes()).collect()))
            .collect()
    }

    // A fresh world that replays the recorded selection is a 2005 world with
    // the same class numbers and names.
    #[test]
    fn recorded_selection_rebuilds_the_same_game_numbers_and_names_in_a_fresh_world() {
        let mut first = World::new();
        first.movement.select_family(bfe_family::GameFamily::Swbf2).unwrap();
        for (class, t) in [("blue_trooper", tuning(60.0, 25)), ("gold_scout", tuning(75.0, 31)), ("blue_trooper", tuning(65.0, 25))] {
            let id = first.sim.strings.intern_str(class);
            first.movement.swbf2.set_class(id, t).unwrap();
        }
        let calls = selection_calls(&first);
        assert_eq!(calls.iter().map(|(_, name, _)| *name).collect::<Vec<_>>(), ["movement_family", "movement_swbf2_class", "movement_swbf2_class"]);
        let mut again = World::new();
        assert_eq!(again.movement.family(), bfe_family::GameFamily::Swbf1);
        for (_, name, call) in &calls {
            args(&mut again, call).unwrap();
            assert_eq!(super::super::dispatch::call(&mut again, Stage::Host, name, call).unwrap(), super::super::Returned::None);
        }
        assert_eq!(again.movement.family(), bfe_family::GameFamily::Swbf2);
        assert_eq!(held(&again), held(&first));
        assert_eq!(again.sim.strings.list, first.sim.strings.list);
        // A family call naming no game this build has is refused.
        let unknown = Call { args: vec![2009.], ..Default::default() };
        assert_eq!(super::super::dispatch::call(&mut World::new(), Stage::Host, "movement_family", &unknown).unwrap(), super::super::Returned::Refused);
        // In a world whose table is in another order the recorded id names
        // another string, and changed words are no writer's: neither registers.
        let mut other = World::new();
        other.sim.strings.intern_str("gold_scout");
        let (_, name, call) = &calls[1];
        args(&mut other, call).unwrap();
        let dispatch = |world: &mut World, call: &Call| super::super::dispatch::call(world, Stage::Host, name, call).unwrap();
        assert_eq!(dispatch(&mut other, call), super::super::Returned::Refused);
        let mut moved = call.clone();
        moved.args[0] = f64::from(other.sim.strings.intern_str("blue_trooper").0);
        let mut bent = moved.clone();
        bent.args[1 + 47] = 0.5;
        assert_eq!(dispatch(&mut other, &bent), super::super::Returned::Refused);
        assert!(other.movement.swbf2.classes().is_empty());
        assert_eq!(dispatch(&mut other, &moved), super::super::Returned::None);
        assert_eq!(other.movement.swbf2.classes().len(), 1);
    }
}
