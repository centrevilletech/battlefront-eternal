//! Host facts use public synthetic rows; no game files or world substitute
//! are required to test dispatch, clock rejection and focus-loss behavior.
use bfe_client::local;
use bfe_client::{
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    host::{Host, World, STEP},
    input::{SquadOrder, ViewActions},
};
use bfe_sim::gen::{WorldClockState, WorldTickInput, WorldTickOutput};
use local::{clock, commands, options, publication, runtime, setup, thread};
use winit::keyboard::KeyCode;

// Count this thread's actual adapter allocations, including reallocations.
// Other concurrently running tests and audio/device threads do not enter the
// window. This proves input packing only, never an unadmitted world's tick.
struct Allocator;
thread_local! {
    static MEASURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn allocated() {
    if MEASURE.try_with(|enabled| enabled.get()).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }
}
unsafe impl std::alloc::GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        allocated();
        std::alloc::GlobalAlloc::alloc(&std::alloc::System, layout)
    }
    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        allocated();
        std::alloc::GlobalAlloc::alloc_zeroed(&std::alloc::System, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        allocated();
        std::alloc::GlobalAlloc::realloc(&std::alloc::System, ptr, layout, size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        std::alloc::GlobalAlloc::dealloc(&std::alloc::System, ptr, layout)
    }
}

fn args(s: &[&str]) -> Vec<String> {
    s.iter().map(|s| (*s).into()).collect()
}

#[test]
/// Verifies CLI option parsing for `--local`:
/// - Non-local CLI modes (`--join`) return `Ok(None)`.
/// - `--local <map>` forwards `--map`, `--side`, `--seed`, and `--heroes` into `Options.battle`,
///   extracts the content root, `--no-audio`, and `--seconds`, and rejects malformed flags.
fn local_dispatch_preserves_other_modes_and_forwards_battle_policy() {
    assert!(options::Options::parse(&args(&["--join", "server"]), None)
        .unwrap()
        .is_none());
    let o = options::Options::parse(
        &args(&[
            "--local",
            "nab2",
            "--side",
            "rep",
            "--seed",
            "5",
            "--heroes",
            "content",
            "--no-audio",
            "--seconds",
            "600",
        ]),
        None,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        o.battle,
        args(&["--map", "nab2", "--side", "rep", "--seed", "5", "--heroes"])
    );
    assert_eq!(o.root, std::path::PathBuf::from("content"));
    assert!(!o.audio);
    assert_eq!(o.duration.unwrap().as_secs(), 600);
    for a in [
        &["--local"][..],
        &["--local", "nab2", "--side"],
        &["--local", "nab2", "--seconds", "NaN"],
        &["--local", "nab2", "--other"],
    ] {
        assert!(options::Options::parse(&args(a), Some("content".into())).is_err());
    }
}

#[test]
/// Verifies `Clock::input` and `Clock::complete` invariants:
/// - Encodes `SquadOrder` into the expected protocol bit (`1 << 0..3`).
/// - Rejects `WorldTickOutput` when `accepted == false` or `clock.tick != self.tick() + 1.0`.
/// - Copies only the eight mutable clock fields upon consecutive acceptance.
fn only_an_accepted_consecutive_tick_can_publish_clock_completion() {
    let template = WorldTickInput {
        units: 37,
        ai: true,
        live: true,
        clock: WorldClockState {
            hz: 60.,
            dt: STEP,
            max_steps: 8.,
            budget: 0.035,
            max_debt: 8.,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut clock = clock::Clock::new(template);
    for (order, bit) in [
        (SquadOrder::MoveOut, 0),
        (SquadOrder::Hold, 1),
        (SquadOrder::Follow, 2),
        (SquadOrder::AtEase, 3),
    ] {
        let row = clock.input(&ViewActions {
            squad: Some(order),
            ..Default::default()
        });
        assert_eq!(row.squads, 1 << bit);
        assert_eq!(row.units, 37);
        assert_eq!(row.dt, STEP);
        let bad = WorldTickOutput {
            clock: WorldClockState {
                tick: row.tick,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(clock.complete(&bad).is_err());
        assert_eq!(clock.tick(), row.tick - 1.);
        let good = WorldTickOutput {
            accepted: true,
            clock: WorldClockState {
                tick: row.tick,
                elapsed: row.tick * STEP,
                debt: 2.,
                ..Default::default()
            },
            ..Default::default()
        };
        clock.complete(&good).unwrap();
        let next = clock.input(&ViewActions::default());
        assert_eq!(next.clock.hz, 60.);
        assert_eq!(next.clock.budget, 0.035);
        assert_eq!(next.clock.debt, 2.);
        assert!(clock.complete(&good).is_err());
    }
}

// This observes the existing device Host, rather than simulating gameplay.
// Pausing must clear both time debt and input before a real Context resumes.
struct Observed {
    steps: usize,
    held: u32,
    look: [f64; 2],
}
impl World for Observed {
    type Frame = ();
    fn step(
        &mut self,
        c: &ClientCommand,
        look: &ClientLook,
        _: &ClientFlightCommand,
        _: &ViewActions,
        _: f64,
    ) -> Result<(), String> {
        self.steps += 1;
        self.held = c.held;
        self.look = [look.x, look.y];
        Ok(())
    }
    fn present(&mut self, _: f64) -> &() {
        &()
    }
}
#[test]
/// Verifies `Host` fixed-step accumulator debt bounds:
/// - Caps catch-up stepping at `MAX_STEPS` (8 steps per frame) and drops excess accumulator debt.
/// - `clear_debt` zeroes accumulated time when focus is lost or pause is toggled.
fn local_host_uses_the_existing_eight_step_debt_bound_and_clears_pause() {
    let mut host = Host::new(Observed {
        steps: 0,
        held: 0,
        look: [0.; 2],
    });
    host.input.key(KeyCode::KeyW, true);
    host.input.motion(100., -50.);
    host.frame(600.).unwrap();
    assert!(host.world.steps <= 8);
    assert_ne!(host.world.held, 0);
    host.pause();
    host.frame(STEP).unwrap();
    assert_eq!(host.world.held, 0);
    assert_eq!(host.world.look, [0.; 2]);
}

fn published(value: &bfe_sim::gen::WirePublicationOutput) -> bfe_sim::store::block::Block {
    let mut block = bfe_sim::store::block::Block::new();
    block.begin(1, bfe_sim::gen::WirePublicationOutput::WORDS);
    let (row, heap) = block.row(0);
    value.put(row, heap);
    block.finish();
    block
}

#[test]
/// Verifies `Publication::deliver` filtering by client handle:
/// - Ignores acks, edge receipts, reliable records, shared groups, and own spans for other clients.
/// - Delivers reliable combat records into `receiver.publications` before shared and own datagrams.
/// - Counts schema-8 presentation records vs total combat records via `count_client_records`.
fn local_publication_selects_its_client_and_joins_reliable_fields_once() {
    use bfe_sim::gen::*;
    let value = WirePublicationOutput {
        epoch: 1.,
        tick: 10.,
        byte_length: 2,
        shared: vec![WirePublishedGroup {
            clients: vec![99],
            parts: vec![WireSpan {
                offset: 999,
                length: 1,
            }],
            ..Default::default()
        }],
        acks: vec![WirePublishedAck {
            client: 7,
            generation: 3.,
            ack: 8.,
            ..Default::default()
        }],
        edges: vec![WirePublishedEdges {
            client: 7,
            epoch: 1.,
            generation: 3.,
            received: vec![4.],
            results: vec![WirePublishedOutcome {
                id: 4.,
                ..Default::default()
            }],
        }],
        records: vec![WirePublishedRecord {
            id: 19.,
            generation: 2.,
            tick: 10.,
            schema: 8.,
            payload: WireSpan {
                offset: 0,
                length: 2,
            },
        }],
        reliable: vec![WirePublishedReliable {
            client: 7,
            records: vec![0],
        }],
        ..Default::default()
    };
    let mut block = bfe_sim::store::block::Block::new();
    let p = publication::Publication::from_welcome(&value, b"{}", &mut block).unwrap();
    assert_eq!(p.count_client_records(7), (1, 1));
    assert_eq!(p.count_client_records(99), (0, 0));
    let mut receiver = bfe_client::online::receiver::Receiver::new(serde_json::json!({}));
    receiver.epoch = 1.;
    receiver.own.generation = 3.;
    p.deliver(7, &mut receiver, 1000.).unwrap();
    p.deliver(7, &mut receiver, 1000.).unwrap();
    assert_eq!(receiver.ack, 8.);
    assert_eq!(receiver.received.count, 1);
    assert_eq!(receiver.outcomes.count, 1);
    assert_eq!(receiver.publications.iter().count(), 1);
    assert_eq!(
        receiver.packets, 0,
        "another client's bytes never enter this session"
    );
    assert_eq!(publication::Publication::acknowledgement(&receiver)[1], 0.);
}

#[test]
/// Verifies `Publication::new` and `Publication::deliver` bounds checking and fault reporting:
/// - Rejects blocks with invalid row counts, strides, heap bounds, or byte lengths.
/// - Surfaces world-wide (`client == 0`) and client-specific fault strings up to 240 chars.
fn native_publication_rejects_bad_spans_and_preserves_world_fault_text() {
    use bfe_sim::gen::*;
    let mut receiver = bfe_client::online::receiver::Receiver::new(serde_json::json!({}));
    let value = WirePublicationOutput {
        byte_length: 4,
        faults: vec![WirePublishedFault {
            client: 0,
            message: WireSpan {
                offset: 0,
                length: 4,
            },
        }],
        ..Default::default()
    };
    let block = published(&value);
    assert!(publication::Publication::new(&block, b"bad").is_err());
    assert_eq!(
        publication::Publication::new(&block, b"stop")
            .unwrap()
            .deliver(7, &mut receiver, 0.)
            .unwrap_err(),
        "stop"
    );
    let mut invalid = published(&WirePublicationOutput::default());
    invalid.words[BlockHeader::WORDS + WirePublicationOutput::OWN] = f64::NAN;
    assert!(publication::Publication::new(&invalid, &[])
        .unwrap()
        .deliver(7, &mut receiver, 0.)
        .is_err());
    invalid.words[BlockHeader::HEAP] = 0.;
    assert!(publication::Publication::new(&invalid, &[]).is_err());
}

#[test]
/// Verifies zero-allocation steady-state delivery across bursts of 80 reliable records:
/// - Pre-warmed `Receiver` and `Publication` deliver 20 bursts without heap allocations.
/// - Bounded ring retains the newest 64 records and preserves `acknowledgement` scalars.
fn paused_publication_burst_keeps_newest_records_without_allocating() {
    use bfe_presentation::network_layout::net_record as row;
    use bfe_sim::gen::*;
    // More than any map's simultaneous events also wraps the session's
    // duplicate-identity window. Publication delivery must inherit the shared
    // journal's newest-record policy without allocating an overflow queue.
    let value = WirePublicationOutput {
        byte_length: 2,
        records: (0..10000)
            .map(|id| WirePublishedRecord {
                id: id as f64 + 1.,
                generation: 3.,
                tick: 10.,
                schema: 8.,
                payload: WireSpan {
                    offset: 0,
                    length: 2,
                },
            })
            .collect(),
        reliable: vec![WirePublishedReliable {
            client: 7,
            records: (0..10000).collect(),
        }],
        edges: vec![WirePublishedEdges {
            client: 7,
            epoch: 1.,
            generation: 3.,
            received: (1..=128).map(f64::from).collect(),
            results: (1..=128)
                .map(|id| WirePublishedOutcome {
                    id: f64::from(id),
                    ..Default::default()
                })
                .collect(),
        }],
        ..Default::default()
    };
    let block = published(&value);
    let mut receiver = bfe_client::online::receiver::Receiver::new(serde_json::json!({}));
    receiver.epoch = 1.;
    receiver.own.generation = 3.;
    ALLOCATIONS.with(|count| count.set(0));
    MEASURE.with(|enabled| enabled.set(true));
    let result = publication::Publication::new(&block, b"{}")
        .unwrap()
        .deliver(7, &mut receiver, 1000.);
    MEASURE.with(|enabled| enabled.set(false));
    assert!(result.is_ok());
    assert_eq!(ALLOCATIONS.with(|count| count.get()), 0);
    assert_eq!(receiver.publications.iter().count(), 2048);
    assert_eq!(
        receiver.publications.iter().next().unwrap().row[row::ID],
        7953.
    );
    assert_eq!(
        receiver.publications.iter().last().unwrap().row[row::ID],
        10000.
    );
    assert_eq!(receiver.publications.refused, 7952);
    assert_eq!(receiver.received.count, 32);
    assert_eq!(receiver.outcomes.count, 32);
    assert!(receiver.error().is_none());
}

// Only call order is observed here. Actual gameplay and presentation require
// the installed-content Context gates in addition to these host assertions.
struct Boundary {
    events: std::rc::Rc<std::cell::RefCell<Vec<&'static str>>>,
    block: bfe_sim::store::block::Block,
    reject: bool,
}
impl runtime::Engine for Boundary {
    fn initialize(&mut self, _: bfe_sim::gen::WorldTickSetup) -> Result<(), String> {
        self.events.borrow_mut().push("initialize");
        Ok(())
    }
    fn input(&mut self, batch: runtime::Input<'_>) -> Result<(), String> {
        self.events.borrow_mut().push("input");
        assert_eq!(batch.tick, 1.);
        assert_eq!(batch.at_ms, 1000. + 1000. / 60.);
        assert_eq!(batch.command.sequence, 3.);
        assert_eq!(batch.look.x, 7.);
        assert_eq!(batch.flight.scale, 1.);
        Ok(())
    }
    fn step(&mut self, input: WorldTickInput) -> Result<WorldTickOutput, String> {
        self.events.borrow_mut().push("step");
        Ok(WorldTickOutput {
            accepted: !self.reject,
            clock: WorldClockState {
                tick: input.tick,
                ..input.clock
            },
            ..Default::default()
        })
    }
    fn publish(&mut self, facts: &[f64]) -> Result<publication::Publication<'_>, String> {
        self.events.borrow_mut().push("publish");
        let tick = facts[bfe_sim::gen::WireWorldFacts::TICK];
        assert!(tick == 0. || tick == 1.);
        assert_eq!(
            facts[bfe_sim::gen::WireWorldFacts::UNIX_MS],
            1.7e12 + if tick == 0. { 1000. } else { 1016. }
        );
        publication::Publication::new(&self.block, &[]).map_err(str::to_string)
    }
    fn acknowledge(&mut self, _: [f64; 2], _: f64, _: &[u32]) -> Result<(), String> {
        self.events.borrow_mut().push("acknowledge");
        Ok(())
    }
    fn close(&mut self) {
        self.events.borrow_mut().push("close");
    }
}

#[test]
/// Verifies `Runtime::new` and `Runtime::step` lifecycle ordering:
/// - `Engine::initialize` runs once during `Runtime::new`.
/// - Each step submits `Input`, calls `Engine::step`, publishes only after clock completion,
///   acknowledges received outcomes, and ensures `close` disposes the engine at most once.
fn local_tick_publishes_only_after_completion_and_close_is_once() {
    for reject in [false, true] {
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let engine = Boundary {
            events: events.clone(),
            block: published(&bfe_sim::gen::WirePublicationOutput::default()),
            reject,
        };
        let admission = setup::Admitted {
            setup: Default::default(),
            tick: WorldTickInput {
                clock: WorldClockState {
                    hz: 60.,
                    dt: STEP,
                    ..Default::default()
                },
                ..Default::default()
            },
            handles: setup::Handles::default(),
            player_id: 1.,
            registry: serde_json::json!({}),
            query: String::new(),
            publication: Vec::new(),
            publication_output: Default::default(),
            host_content: Default::default(),
            explosions: Vec::new(),
        };
        let mut local = runtime::Runtime::new(engine, admission, 1000., 1.7e12).unwrap();
        let command = ClientCommand {
            sequence: 3.,
            ..Default::default()
        };
        let look = ClientLook { x: 7., y: 0. };
        let flight = ClientFlightCommand {
            scale: 1.,
            ..Default::default()
        };
        let view = ViewActions {
            map: true,
            ..Default::default()
        };
        assert_eq!(local.step(&command, &look, &flight, &view).is_err(), reject);
        assert_eq!(local.tick(), if reject { 0. } else { 1. });
        local.close();
        local.close();
        assert!(local.step(&command, &look, &flight, &view).is_err());
        drop(local);
        assert_eq!(
            *events.borrow(),
            if reject {
                vec![
                    "initialize",
                    "publish",
                    "acknowledge",
                    "input",
                    "step",
                    "close",
                ]
            } else {
                vec![
                    "initialize",
                    "publish",
                    "acknowledge",
                    "input",
                    "step",
                    "publish",
                    "acknowledge",
                    "close",
                ]
            }
        );
    }
}

#[test]
/// Verifies `Commands::capture` translation from desktop `ClientCommand`, `ClientLook`,
/// and `ClientFlightCommand` into `InputMailboxCall`, `PlayerActions`, and `PlayerFire` edges:
/// - Accumulates yaw/pitch degrees, trigger press/release edges, weapon switches, and vehicle actions.
fn generated_desktop_input_uses_shared_look_and_trigger_transitions() {
    use bfe_client::generated_input::{
        ClientAction, ClientCommand, ClientFlightCommand, ClientHeld, ClientLook,
    };
    let context = bfe_sim::abi::context::Context::new();
    let names = commands::Names::new(&context).unwrap();
    let forward = names.held[ClientHeld::Forward as usize];
    let use_action = names.actions[bfe_transport::layout::INPUT_ACTIONS
        .iter()
        .position(|n| *n == "use")
        .unwrap()];
    let mut input = commands::Commands::new(names);
    let mut command = ClientCommand {
        sequence: 1.,
        mouse_scale: 1.,
        held: (1 << ClientHeld::Forward as u8) | (1 << ClientHeld::Primary as u8),
        actions: 1 << ClientAction::Use as u8,
        primary_count: 1,
        ..Default::default()
    };
    let flight = ClientFlightCommand {
        scale: 1.,
        curve: 1.35,
        ..Default::default()
    };
    input
        .capture(
            &command,
            &ClientLook { x: 2., y: -1. },
            &flight,
            1.,
            3.,
            10.,
            true,
            16. / 9.,
            Some(7.),
            false,
        )
        .unwrap();
    let entry = input.mailbox.entry.as_ref().unwrap();
    assert!(entry.codes.contains(&forward));
    assert_eq!(entry.look_total, Some([2., -1.]));
    assert_eq!(input.actions.actions, vec![use_action]);
    assert_eq!(input.fires.len(), 1);
    assert!(input.fires[0].pressed);
    assert_eq!(input.fires[0].view_tick, Some(7.));
    command.sequence = 2.;
    command.held = 0;
    command.actions = 0;
    command.primary_count = 0;
    input
        .capture(
            &command,
            &ClientLook { x: -1., y: 3. },
            &flight,
            1.,
            3.,
            11.,
            true,
            16. / 9.,
            Some(8.),
            false,
        )
        .unwrap();
    assert!(input.mailbox.entry.as_ref().unwrap().codes.is_empty());
    assert_eq!(
        input.mailbox.entry.as_ref().unwrap().look_total,
        Some([1., 2.])
    );
    assert_eq!(input.fires.len(), 1);
    assert!(!input.fires[0].pressed);
    assert!(input.actions.actions.is_empty());
    assert_eq!(input.refused, 0);
    context.dispose().unwrap();
}

#[test]
/// Verifies `Native::submit` zero-allocation invariant after warm-up:
/// - Repeatedly packs movement, action edges, and fire edges into retained `Arguments`
///   arenas without allocating on the heap.
fn full_input_bursts_remain_bounded_without_allocating_after_admission() {
    let context = bfe_sim::abi::context::Context::new();
    let mut input = commands::Commands::new(commands::Names::new(&context).unwrap());
    let mut command = ClientCommand {
        mouse_scale: 1.,
        primary_count: u32::MAX,
        secondary_count: u32::MAX,
        ..Default::default()
    };
    let mut flight = ClientFlightCommand {
        scale: 1.,
        curve: 1.35,
        ..Default::default()
    };
    ALLOCATIONS.with(|count| count.set(0));
    MEASURE.with(|enabled| enabled.set(true));
    let mut error = None;
    // Ten minutes' worth of alternating maximal device batches exercises
    // every held key, flight mode, wheel count and trigger transition. Even
    // the first capture must fit the capacities reserved at admission.
    for tick in 1..=36000 {
        command.sequence = tick as f64;
        command.held = if tick % 2 == 0 { 0 } else { (1 << 9) - 1 };
        command.actions = (1 << 11) - 1;
        flight.held = (1 << bfe_client::generated_input::ClientFlightHeld::NAMES.len()) - 1;
        flight.actions = (1 << bfe_client::generated_input::ClientFlightAction::NAMES.len()) - 1;
        if let Err(fault) = input.capture(
            &command,
            &ClientLook { x: 2., y: -1. },
            &flight,
            1.,
            3.,
            tick as f64,
            true,
            16. / 9.,
            Some(tick as f64 - 1.),
            tick % 3 == 0,
        ) {
            error = Some(fault);
            break;
        }
    }
    MEASURE.with(|enabled| enabled.set(false));
    assert_eq!(error, None);
    assert_eq!(ALLOCATIONS.with(|count| count.get()), 0);
    assert!(input.actions.actions.len() <= 32);
    assert!(input.fires.len() <= 128);
    assert!(input.mailbox.entry.as_ref().unwrap().codes.len() <= 11);
    context.dispose().unwrap();
}

fn battle_options() -> bfe_sim::world::admit::Options {
    bfe_sim::world::admit::Options {
        map: "nab2".into(),
        side: "rep".into(),
        enhanced: false,
        seed: 17,
        heroes: true,
        unit: Some("trooper".into()),
    }
}

fn fake_admission() -> bfe_sim::world::admit::Admitted {
    bfe_sim::world::admit::Admitted {
        handles: setup::Handles {
            mailbox: 19,
            player: 23,
            publication: 29,
            host: 31,
            // Pool<EdgeRuntime> uses tag 5 (`(5 << 26) | (index + 1)`).
            // Slot 2 (`335_544_322`) is a non-default tagged pool key.
            edges: (5 << 26) | 2,
            client: 41,
            combat: 43,
            presentation: 47,
            store_source: 53,
        },
        player_id: 0.,
        units: 73,
        registry: serde_json::json!({"protocol":1}),
        ..Default::default()
    }
}

#[test]
/// Verifies `Admitted::from_world` validation of pool handles and capture keys:
/// - Preserves non-zero `world`, `player`, `edges`, `mailbox`, `host`, `publication`, `client`,
///   `combat`, `presentation`, and `store_source` handles, plus `player_id == 0.0`.
/// - Rejects zero capture handles (`combat`, `presentation`, `store_source`) and non-finite `player_id`.
fn context_admission_handoff_preserves_actual_pool_keys_and_zero_player_id() {
    use bfe_sim::world::admit::Call;
    use local::runtime::Engine;
    let source = fake_admission();
    let handles = source.handles;
    let admitted = setup::Admitted::from_world(source, &battle_options()).unwrap();
    assert_eq!(admitted.handles, handles);
    assert_eq!(admitted.player_id, 0.);
    assert_eq!(admitted.tick.units, 73);
    assert_eq!(admitted.tick.clock.hz, 60.);
    assert!(admitted.tick.ai && admitted.tick.live);
    assert_eq!(admitted.registry, serde_json::json!({"protocol":1}));
    assert_eq!(
        admitted.query,
        "?map=nab2&side=rep&era=cw&ai=classic&seed=17&heroes=1&unit=trooper"
    );
    // Reserve the exact non-default `edges` pool key (`(5 << 26) | 2`) so
    // `Native::initialize` can invoke `world_host_start(Some(capture))` and
    // `world_host_client(player, edges)` against that handle, then verify teardown.
    let context = bfe_sim::abi::context::Context::new();
    for _ in 0..2 {
        context
            .invoke_export("wire_edges_create", &Call::default())
            .unwrap();
    }
    let alias = context.clone();
    let mut engine = local::context::Native::new(context, &admitted).unwrap();
    assert_eq!(engine.handles(), handles);
    engine.initialize(admitted.setup).unwrap();
    engine.close();
    engine.close();
    drop(engine);
    assert_eq!(
        alias.string_count().unwrap_err().kind(),
        bfe_sim::abi::context::ErrorKind::Disposed
    );
    assert_eq!(alias.world_replay_live_handles(), 0);
    for bad in [f64::NAN, -1., 0.5, u32::MAX as f64 + 1.] {
        let mut source = fake_admission();
        source.player_id = bad;
        assert!(setup::Admitted::from_world(source, &battle_options()).is_err());
    }
    for clear in [
        |h: &mut setup::Handles| h.publication = 0,
        |h: &mut setup::Handles| h.combat = 0,
        |h: &mut setup::Handles| h.presentation = 0,
        |h: &mut setup::Handles| h.store_source = 0,
    ] {
        let mut source = fake_admission();
        clear(&mut source.handles);
        assert!(setup::Admitted::from_world(source, &battle_options()).is_err());
    }
}

#[test]
/// Verifies `Native::initialize` and `Native::step` integration with `sim::host`:
/// - `Native::initialize` calls `Context::world_host_start(Some(Capture))` with the admitted
///   `combat`, `presentation`, and `store_source` handles, then `Context::world_host_client(player, edges)`.
/// - `Native::step` executes `Context::world_tick` so `Drain` populates `world_host_journal`
///   with AI hearing (`Hearing::Shot`, `Hearing::Damage`) and emits schema-8 presentation records
///   into `Capture` for wire publication.
fn local_engine_starts_host_with_admitted_capture_and_ticks_via_world_tick() {
    use bfe_sim::gen::{InputMailboxSetup, PlayerSetup, WeCall, WeEdge, WeResult, WireStoreSetup};
    use bfe_sim::world::admit::{Call, Returned};
    use local::runtime::Engine;

    let context = bfe_sim::abi::context::Context::new();
    let scalar = |name: &str, args: Vec<f64>| -> u32 {
        match context.invoke_export(
            name,
            &Call {
                args,
                ..Default::default()
            },
        ) {
            Ok(Returned::Scalar(v)) => v as u32,
            other => panic!("{name} returned {other:?}"),
        }
    };

    let mut mailbox_call = Call::default();
    let mailbox_setup = InputMailboxSetup {
        epoch: 1.0,
        capacity: 256.0,
        codes: Vec::new(),
    };
    mailbox_call.row.resize(InputMailboxSetup::WORDS, 0.0);
    mailbox_setup.put(&mut mailbox_call.row, &mut mailbox_call.heap);
    let Returned::Scalar(mailbox) = context
        .invoke_export("input_mailbox_create", &mailbox_call)
        .unwrap()
    else {
        unreachable!()
    };
    let mailbox = mailbox as u32;

    let player = context
        .players_create(&PlayerSetup {
            mailbox,
            ..Default::default()
        })
        .unwrap();
    let runtime = scalar("wire_create", vec![]);
    let presentation = scalar("wire_presentation_create", vec![]);
    let host = scalar(
        "wire_host_create",
        vec![1.0, f64::INFINITY, f64::INFINITY, 1.0],
    );
    let stagger = scalar("wire_stagger_create", vec![]);
    let shots = scalar("wire_shots_create", vec![256.0]);
    let combat = scalar("wire_combat_create", vec![]);

    let mut source_call = Call::default();
    let source_setup = WireStoreSetup {
        item_capacity: 75,
        remote_capacity: 64,
        entity_capacity: 64,
        record_capacity: 64,
        seat_capacity: 4,
        ..Default::default()
    };
    source_call.row.resize(WireStoreSetup::WORDS, 0.0);
    source_setup.put(&mut source_call.row, &mut source_call.heap);
    let Returned::Scalar(store_source) = context
        .invoke_export("wire_store_source_create", &source_call)
        .unwrap()
    else {
        unreachable!()
    };
    let store_source = store_source as u32;

    let publication = scalar(
        "wire_publish_create",
        vec![
            runtime as f64,
            store_source as f64,
            stagger as f64,
            shots as f64,
            host as f64,
            combat as f64,
        ],
    );
    let edges = scalar("wire_edges_create", vec![]);

    let world = bfe_sim::world::admit::Admitted {
        setup: Default::default(),
        handles: setup::Handles {
            mailbox,
            player,
            publication,
            host,
            edges,
            client: 1,
            combat,
            presentation,
            store_source,
        },
        player_id: 0.0,
        registry: serde_json::json!({"protocol": 1}),
        units: 1,
        ..Default::default()
    };
    let admitted = setup::Admitted::from_world(world, &battle_options()).unwrap();
    let alias = context.clone();
    let mut engine = local::context::Native::new(context, &admitted).unwrap();
    engine.initialize(admitted.setup).unwrap();

    // Queue an input edge on `edges` scheduled for tick 2. A bare `world_step`
    // never touches `edges`; `world_tick` advances the client registered by
    // `world_host_client(player, edges)` on each tick.
    let push = WeCall {
        op: 1.0,
        epoch: 0.0,
        generation: 0.0,
        tick: 0.0,
        capacity: 256.0,
        edges: vec![WeEdge {
            id: 1.0,
            epoch: 0.0,
            generation: 0.0,
            tick: 2.0,
            ..Default::default()
        }],
        ..Default::default()
    };
    let mut args = local::arguments::Arguments::new();
    args.record(&[edges as f64], WeCall::WORDS, |row, heap| {
        push.put(row, heap)
    });
    args.run(&alias, bfe_sim::gen::OPERATION_WIRE_EDGES_RUN)
        .unwrap();

    // Tick 1 runs `world_tick`: `advance_edges` advances `edges.tick` to 1.0,
    // then `before_step` writes `bank:F2_INPUT` (refused here because `f2_init`
    // is only prepared by full content admission). A bare `world_step` does neither.
    let clock = clock::Clock::new(admitted.tick);
    let mut step1 = clock.input(&ViewActions::default());
    step1.at = 1000.0 + 1000.0 / 60.0;
    step1.mouse_scale = 1.0;
    let err1 = engine.step(step1.clone()).unwrap_err();
    assert!(
        err1.contains("bank F2_INPUT"),
        "expected world_tick before_step bank write, got: {err1}"
    );
    assert!(!engine.host_counters().context_poisoned);

    // Inspect `edges` state (op 8) to confirm `world_tick` advanced `edges.tick` to 1.0.
    let read = WeCall {
        op: 8.0,
        capacity: 256.0,
        ..Default::default()
    };
    args.record(&[edges as f64], WeCall::WORDS, |row, heap| {
        read.put(row, heap)
    });
    args.run(&alias, bfe_sim::gen::OPERATION_WIRE_EDGES_RUN)
        .unwrap();
    let (row, heap) = args.answer.read(0);
    assert_eq!(WeResult::take(row, heap).tick, 1.0);

    // Tick 2 reaches the queued edge on `edges` inside `world_tick`'s `advance_edges`,
    // proving `Native::initialize` registered `(player, edges)` via `world_host_client`
    // and `Native::step` executes `Context::world_tick` rather than bare `world_step`.
    let mut step2 = step1;
    step2.tick = 2.0;
    let err2 = engine.step(step2).unwrap_err();
    assert!(
        err2.contains("bank F2_INPUT"),
        "expected native edge dispatch before the bank write, got: {err2}"
    );
    engine.close();
}

#[test]
/// Verifies that an invalid or unavailable battle option fails fast before reading
/// map assets from disk or opening audio/window devices.
fn unavailable_world_admission_refuses_before_reading_assets_or_opening_devices() {
    use setup::Source;
    struct Missing(std::cell::Cell<usize>);
    impl bfe_content::files::Files for Missing {
        fn read(&self, _: &str) -> Option<std::rc::Rc<[u8]>> {
            self.0.set(self.0.get() + 1);
            None
        }
    }
    let files = Missing(std::cell::Cell::<usize>::new(0));
    let source = setup::NativeSource {
        spawn_screen: false,
        files: &files,
        options: battle_options(),
    };
    let context = bfe_sim::abi::context::Context::new();
    let error = source
        .admit(&context)
        .err()
        .expect("missing content cannot admit a battle");
    if !bfe_sim::world::admit::provided() {
        assert_eq!(error, "native world API unavailable: world_admit");
        assert_eq!(files.0.get(), 0);
    } else {
        assert!(files.0.get() > 0);
        assert!(!error.contains("unavailable"));
    }
    context.dispose().unwrap();
}

#[test]
/// Verifies `Arguments::record` arena retention across consecutive calls:
/// - Writes scalar prefix words, record body words, and variable-length heap slices
///   while preserving the allocated capacity of `words` and `heap`.
fn operation_packing_uses_generated_records_and_retains_both_arenas() {
    use bfe_sim::gen::{BlockHeader, WeCall};
    let context = bfe_sim::abi::context::Context::new();
    let mut args = local::arguments::Arguments::new();
    let ack = WeCall {
        ids: (1..=32).map(f64::from).collect(),
        ..Default::default()
    };
    ALLOCATIONS.with(|count| count.set(0));
    MEASURE.with(|enabled| enabled.set(true));
    for _ in 0..1000 {
        args.record(&[37.], WeCall::WORDS, |row, heap| ack.put(row, heap));
    }
    MEASURE.with(|enabled| enabled.set(false));
    assert_eq!(ALLOCATIONS.with(|count| count.get()), 0);
    let actual = WeCall::take(
        &args.words[1..1 + WeCall::WORDS],
        &args.words[1 + WeCall::WORDS..],
    );
    assert_eq!(actual.ids, ack.ids);
    assert_eq!(args.words[0], 37.);
    let echo = bfe_sim::gen::Echo {
        ..Default::default()
    };
    args.record(&[], bfe_sim::gen::Echo::WORDS, |row, heap| {
        echo.put(row, heap)
    });
    args.run(&context, bfe_sim::gen::OPERATION_ECHO).unwrap();
    assert_eq!(args.answer.words[BlockHeader::ROWS], 1.);
    assert_eq!(
        local::arguments::required("echo").unwrap(),
        bfe_sim::gen::OPERATION_ECHO
    );
    assert!(local::arguments::required("missing_operation").is_err());
    context.dispose().unwrap();
}

struct ThreadedBoundary {
    block: bfe_sim::store::block::Block,
}
impl runtime::Engine for ThreadedBoundary {
    fn initialize(&mut self, _: bfe_sim::gen::WorldTickSetup) -> Result<(), String> {
        Ok(())
    }
    fn input(&mut self, _: runtime::Input<'_>) -> Result<(), String> {
        Ok(())
    }
    fn step(&mut self, input: WorldTickInput) -> Result<WorldTickOutput, String> {
        Ok(WorldTickOutput {
            accepted: true,
            clock: WorldClockState {
                tick: input.tick,
                elapsed: input.tick * STEP,
                ..input.clock
            },
            ..Default::default()
        })
    }
    fn publish(&mut self, _: &[f64]) -> Result<publication::Publication<'_>, String> {
        publication::Publication::new(&self.block, &[]).map_err(str::to_string)
    }
    fn acknowledge(&mut self, _: [f64; 2], _: f64, _: &[u32]) -> Result<(), String> {
        Ok(())
    }
    fn close(&mut self) {}
}

#[test]
/// Verifies `WorldThread` lock-free cross-thread communication:
/// - Runs the 60 Hz simulation loop on a dedicated background thread while the render
///   thread sends `WorldMessage::Step` and drains publications with zero heap allocations
///   and zero render-thread blocks.
fn threaded_world_steps_at_60hz_without_blocking_or_allocating_on_render_thread() {
    use bfe_client::generated_input::ClientHeld;
    use thread::{WorldMessage, WorldThread};

    let mut world = WorldThread::spawn_native(|| {
        let engine = ThreadedBoundary {
            block: published(&bfe_sim::gen::WirePublicationOutput::default()),
        };
        let admission = setup::Admitted {
            setup: Default::default(),
            tick: WorldTickInput {
                clock: WorldClockState {
                    hz: 60.0,
                    dt: STEP,
                    ..Default::default()
                },
                ..Default::default()
            },
            handles: setup::Handles::default(),
            player_id: 1.0,
            registry: serde_json::json!({}),
            query: String::new(),
            publication: Vec::new(),
            publication_output: Default::default(),
            host_content: Default::default(),
            explosions: Vec::new(),
        };
        runtime::Runtime::new(engine, admission, 1000.0, 1.7e12)
    });
    let mut receiver = bfe_client::online::receiver::Receiver::new(serde_json::json!({}));

    world.send(WorldMessage::Spawn {
        team: 1,
        class_name: "rep_inf_trooper".to_string(),
        post_name: "cp1".to_string(),
    });

    // Wait briefly for the 60 Hz world thread to publish a few ticks.
    std::thread::sleep(std::time::Duration::from_millis(60));
    let drained = world.poll_publications(&mut receiver, 100.0).unwrap();
    assert!(drained > 0);
    assert!(world.latest_tick() >= 1.0);
    assert!(world.tick_stats().count >= 1);

    // Measure render-thread allocations across 200 non-blocking send + poll cycles.
    ALLOCATIONS.with(|count| count.set(0));
    MEASURE.with(|enabled| enabled.set(true));
    for i in 0..200 {
        let cmd = ClientCommand {
            sequence: (i + 1) as f64,
            held: 1 << (ClientHeld::Forward as u8),
            mouse_scale: 1.0,
            aspect: 16.0 / 9.0,
            ..Default::default()
        };
        world.send(WorldMessage::Step {
            command: cmd,
            look: ClientLook { x: 1.0, y: 0.0 },
            flight: ClientFlightCommand::default(),
            actions: ViewActions::default(),
            ack: [1.0, receiver.latest],
        });
        let _ = world
            .poll_publications(&mut receiver, 200.0 + i as f64)
            .unwrap();
    }
    MEASURE.with(|enabled| enabled.set(false));
    assert_eq!(ALLOCATIONS.with(|count| count.get()), 0);
    assert_eq!(world.renderer_blocks(), 0);

    // Verify pause halts tick advancement and resume continues.
    world.set_paused(true);
    assert!(world.is_paused());
    world.set_paused(false);
    assert!(!world.is_paused());

    world.shutdown();
}

#[test]
/// End-to-end integration test for cold admission (`Prepared::new`) and `Runtime::new`
/// across `nab2`, `tat1`, and `hot1`:
/// - Verifies `Admitted.publication` is non-empty and `Publication::from_welcome`
///   completes the receiver baseline at tick 0 (`local.receiver.complete == true`).
/// - Steps 120 ticks via `Context::world_tick` without poisoning `Context` and confirms
///   `world_host_journal` records are emitted (`counters.journal_seq > 0`).
fn cold_admission_delivers_welcome_publication_and_steps_three_maps() {
    let Some(root) = std::env::var_os("BFE_CONTENT_DIR").map(std::path::PathBuf::from) else {
        return;
    };
    for (map, side) in [("nab2", "rep"), ("tat1", "imp"), ("hot1", "all")] {
        let opts = options::Options {
            root: root.clone(),
            battle: vec![
                "--map".into(),
                map.into(),
                "--side".into(),
                side.into(),
                "--seed".into(),
                "1".into(),
            ],
            audio: false,
            duration: None,
        };
        let prepared = local::launch::Prepared::new(&opts)
            .unwrap_or_else(|e| panic!("cold admission failed on {map}/{side}: {e}"));
        assert!(
            !prepared.admission.publication.is_empty(),
            "expected non-empty welcome publication on {map}"
        );
        let mut local = runtime::Runtime::new(
            *prepared.engine,
            *prepared.admission,
            1000.0,
            1_700_000_000_000.0,
        )
        .unwrap_or_else(|e| panic!("Runtime::new failed on {map}/{side}: {e}"));
        assert!(
            local.receiver.complete,
            "welcome publication must complete tick-0 baseline on {map}"
        );
        for seq in 1..=120 {
            let cmd = ClientCommand {
                sequence: seq as f64,
                mouse_scale: 1.0,
                aspect: 16.0 / 9.0,
                ..Default::default()
            };
            local
                .step(
                    &cmd,
                    &ClientLook::default(),
                    &ClientFlightCommand::default(),
                    &ViewActions::default(),
                )
                .unwrap_or_else(|e| panic!("step {seq} failed on {map}/{side}: {e}"));
        }
        let counters = local.host_counters();
        assert!(!counters.context_poisoned);
        assert!(
            counters.journal_seq > 0,
            "expected host journal records after 120 ticks on {map}"
        );
        local.close();
    }
}

#[test]
fn menu_admission_waits_for_the_players_class_and_post_before_spawning() {
    let Some(root) = std::env::var_os("BFE_CONTENT_DIR") else { return };
    let options = options::Options {
        root: root.into(),
        battle: vec!["--map".into(), "nab2".into(), "--side".into(), "rep".into()],
        audio: false,
        duration: None,
    };
    let prepared = local::launch::Prepared::with_spawn_screen(&options, true).unwrap();
    let mut world = runtime::Runtime::new(*prepared.engine, *prepared.admission, 0.0, 0.0).unwrap();
    assert!(world.receiver.complete, "menu receives the admitted welcome baseline");
    assert!(!world.receiver.own.alive, "explicit side must not bypass menu selection");
    let life = world.receiver.own.generation;
    world.spawn_human(world.receiver.own.team, "rep_inf_clone_trooper", "cp6").unwrap();
    for sequence in 1..=6 {
        world.step(&ClientCommand { sequence: sequence as f64, mouse_scale: 1.0,
            ..Default::default() }, &ClientLook::default(), &ClientFlightCommand::default(),
            &ViewActions::default()).unwrap();
    }
    assert!(world.receiver.own.alive, "selected spawn reaches the authoritative body");
    assert!(world.receiver.own.generation > life, "spawn creates a new living generation");
    world.close();
}
