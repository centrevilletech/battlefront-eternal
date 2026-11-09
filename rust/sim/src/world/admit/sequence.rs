//! The owner construction sequence: game.js createGame headless, then
//! createLocalServer, as the ordered stages below. Each stage drives the
//! existing `_in` bodies with the rows made for it, exactly as the page's
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
    /// The collision kernel from content triangles (ray-kernel.js, collision.js).
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

/// One call against the world as the page makes it: the strings interned
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
    let battle = &content.battle;
    let defs = bfe_content::defs::Defs::build(content).map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    let mut cx = Cx::new(content, &defs, files, options, world.sim.strings.clone());
    rows.setup = rows::setup(battle);
    let mut admitted = Admitted { setup: rows.setup.clone(), ..Default::default() };
    run_stages(world, content, &mut cx, rows, &mut admitted, &ORDER)?;
    push_page_streams(world, &mut cx)?;
    admitted.units = super::spawn::units(world)?;
    Ok(admitted)
}

/// F3Random's hand-over before the first tick, which the page made by
/// writing the phase's memory rather than through a call:
/// `configure(gameplay, grenade-spin, gameplay, seat-weapons)` binds the
/// gameplay, spin, drop and seat globals to those streams' ids (a repeated
/// callback keeps its id), and `push` writes each entry: the gameplay
/// stream present with its state after the one draw that placed the player
/// (`points[Math.floor(gameplayRandom() * points.length)]`), the lazy
/// grenade-spin and seat-weapons streams absent at their mixed seeds
/// (RandomStreams' `initialState`) until the world first draws them.
fn push_page_streams(world: &mut World, cx: &mut Cx) -> Result<(), AdmitError> {
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

/// The stages named, in the order given, on a world whose earlier stages
/// are already done and a context the earlier owners already filled. The
/// whole admission is `run`; a partial proof (admit-replay's recorded
/// prefix, partial.rs) runs a lane's stages alone over the host's recorded
/// construction of the stages before them.
pub fn run_stages(world: &mut World, content: &Content, cx: &mut Cx, rows: &mut Rows, admitted: &mut Admitted, stages: &[Stage]) -> Result<(), AdmitError> {
    // game.js createGame: `new Units()` (registry.js) numbers members from 1.
    // The host hands the registry over whole before attachF3 (the KIND_UNITS
    // import); the owning world keeps it as members are added instead.
    if stages.first() == Some(&Stage::Content) && world.sim.store.units.list.is_empty() { world.sim.store.units.next_id = 1.0; }
    for &stage in stages {
        match stage {
            Stage::Content => {}
            // validateStartupClass runs on the page before loading, never in a
            // headless world (game.js: `!headless && !params.has("network")`);
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
                admitted.handles = session.handles;
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
            // game.js: `units.add(new Unit({... human: true}))` gives the
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
            // seatRandom)` registers the host's first streams (f3-random.js).
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
            // game.js after bindWeaponPolicyContent: the fielded classes'
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
                // ai-world-load.js 27: the AI's two bare generators register
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

