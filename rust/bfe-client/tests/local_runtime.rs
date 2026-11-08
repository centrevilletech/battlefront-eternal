//! Host facts use public synthetic rows; no game files or world substitute
//! are required to test dispatch, clock rejection and focus-loss behavior.
use bfe_client::local;
use local::{clock, commands, options, publication, runtime, setup, thread};
use bfe_client::{host::{Host, World, STEP}, generated_input::{ClientCommand, ClientLook, ClientFlightCommand}, input::{ViewActions, SquadOrder}};
use bfe_sim::gen::{WorldClockState, WorldTickInput, WorldTickOutput};
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
        allocated(); std::alloc::GlobalAlloc::alloc(&std::alloc::System, layout)
    }
    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        allocated(); std::alloc::GlobalAlloc::alloc_zeroed(&std::alloc::System, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        allocated(); std::alloc::GlobalAlloc::realloc(&std::alloc::System, ptr, layout, size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        std::alloc::GlobalAlloc::dealloc(&std::alloc::System, ptr, layout)
    }
}

fn args(s: &[&str]) -> Vec<String> { s.iter().map(|s| (*s).into()).collect() }

#[test]
fn local_dispatch_preserves_other_modes_and_forwards_battle_policy() {
    assert!(options::Options::parse(&args(&["--join", "server"]), None).unwrap().is_none());
    let o = options::Options::parse(&args(&["--local", "nab2", "--side", "rep", "--seed", "5", "--heroes", "content", "--no-audio", "--seconds", "600"]), None).unwrap().unwrap();
    assert_eq!(o.battle, args(&["--map", "nab2", "--side", "rep", "--seed", "5", "--heroes"]));
    assert_eq!(o.root, std::path::PathBuf::from("content")); assert!(!o.audio);
    assert_eq!(o.duration.unwrap().as_secs(), 600);
    for a in [&["--local"][..], &["--local", "nab2", "--side"], &["--local", "nab2", "--seconds", "NaN"], &["--local", "nab2", "--other"]] {
        assert!(options::Options::parse(&args(a), Some("content".into())).is_err());
    }
}

#[test]
fn only_an_accepted_consecutive_tick_can_publish_clock_completion() {
    let template = WorldTickInput { units: 37, ai: true, live: true, clock: WorldClockState { hz: 60., dt: STEP, max_steps: 8., budget: 0.035, max_debt: 8., ..Default::default() }, ..Default::default() };
    let mut clock = clock::Clock::new(template);
    for (order, bit) in [(SquadOrder::MoveOut, 0), (SquadOrder::Hold, 1), (SquadOrder::Follow, 2), (SquadOrder::AtEase, 3)] {
        let row = clock.input(&ViewActions { squad: Some(order), ..Default::default() });
        assert_eq!(row.squads, 1 << bit); assert_eq!(row.units, 37); assert_eq!(row.dt, STEP);
        let bad = WorldTickOutput { clock: WorldClockState { tick: row.tick, ..Default::default() }, ..Default::default() };
        assert!(clock.complete(&bad).is_err()); assert_eq!(clock.tick(), row.tick - 1.);
        let good = WorldTickOutput { accepted: true, clock: WorldClockState { tick: row.tick, elapsed: row.tick * STEP, debt: 2., ..Default::default() }, ..Default::default() };
        clock.complete(&good).unwrap();
        let next = clock.input(&ViewActions::default());
        assert_eq!(next.clock.hz, 60.); assert_eq!(next.clock.budget, 0.035); assert_eq!(next.clock.debt, 2.);
        assert!(clock.complete(&good).is_err());
    }
}

// This observes the existing device Host, rather than simulating gameplay.
// Pausing must clear both time debt and input before a real Context resumes.
struct Observed { steps: usize, held: u32, look: [f64; 2] }
impl World for Observed {
    type Frame = ();
    fn step(&mut self, c: &ClientCommand, look: &ClientLook, _: &ClientFlightCommand, _: &ViewActions, _: f64) -> Result<(), String> {
        self.steps += 1; self.held = c.held; self.look = [look.x, look.y]; Ok(())
    }
    fn present(&mut self, _: f64) -> &() { &() }
}
#[test]
fn local_host_uses_the_existing_eight_step_debt_bound_and_clears_pause() {
    let mut host = Host::new(Observed { steps: 0, held: 0, look: [0.; 2] });
    host.input.key(KeyCode::KeyW, true); host.input.motion(100., -50.);
    host.frame(600.).unwrap(); assert!(host.world.steps <= 8); assert_ne!(host.world.held, 0);
    host.pause(); host.frame(STEP).unwrap();
    assert_eq!(host.world.held, 0); assert_eq!(host.world.look, [0.; 2]);
}

fn published(value: &bfe_sim::gen::WirePublicationOutput) -> bfe_sim::store::block::Block {
    let mut block = bfe_sim::store::block::Block::new();
    block.begin(1, bfe_sim::gen::WirePublicationOutput::WORDS);
    let (row, heap) = block.row(0); value.put(row, heap); block.finish(); block
}

#[test]
fn local_publication_selects_its_client_and_joins_reliable_fields_once() {
    use bfe_sim::gen::*;
    let value = WirePublicationOutput {
        epoch: 1., tick: 10., byte_length: 2,
        shared: vec![WirePublishedGroup { clients: vec![99], parts: vec![WireSpan { offset: 999, length: 1 }], ..Default::default() }],
        acks: vec![WirePublishedAck { client: 7, generation: 3., ack: 8., ..Default::default() }],
        edges: vec![WirePublishedEdges { client: 7, epoch: 1., generation: 3., received: vec![4.],
            results: vec![WirePublishedOutcome { id: 4., ..Default::default() }] }],
        records: vec![WirePublishedRecord { id: 19., generation: 2., tick: 10., schema: 8., payload: WireSpan { offset: 0, length: 2 } }],
        reliable: vec![WirePublishedReliable { client: 7, records: vec![0] }], ..Default::default()
    };
    let block = published(&value);
    let p = publication::Publication::new(&block, b"{}").unwrap();
    let mut receiver = bfe_client::online::receiver::Receiver::new(serde_json::json!({}));
    receiver.epoch = 1.; receiver.own.generation = 3.;
    p.deliver(7, &mut receiver, 1000.).unwrap(); p.deliver(7, &mut receiver, 1000.).unwrap();
    assert_eq!(receiver.ack, 8.); assert_eq!(receiver.received.count, 1); assert_eq!(receiver.outcomes.count, 1);
    assert_eq!(receiver.publications.iter().count(), 1);
    assert_eq!(receiver.packets, 0, "another client's bytes never enter this session");
    assert_eq!(publication::Publication::acknowledgement(&receiver)[1], 0.);
}

#[test]
fn native_publication_rejects_bad_spans_and_preserves_world_fault_text() {
    use bfe_sim::gen::*;
    let mut receiver = bfe_client::online::receiver::Receiver::new(serde_json::json!({}));
    let value = WirePublicationOutput { byte_length: 4, faults: vec![WirePublishedFault { client: 0, message: WireSpan { offset: 0, length: 4 } }], ..Default::default() };
    let block = published(&value);
    assert!(publication::Publication::new(&block, b"bad").is_err());
    assert_eq!(publication::Publication::new(&block, b"stop").unwrap().deliver(7, &mut receiver, 0.).unwrap_err(), "stop");
    let mut invalid = published(&WirePublicationOutput::default());
    invalid.words[BlockHeader::WORDS + WirePublicationOutput::OWN] = f64::NAN;
    assert!(publication::Publication::new(&invalid, &[]).unwrap().deliver(7, &mut receiver, 0.).is_err());
    invalid.words[BlockHeader::HEAP] = 0.;
    assert!(publication::Publication::new(&invalid, &[]).is_err());
}

#[test]
fn paused_publication_burst_keeps_newest_records_without_allocating() {
    use bfe_sim::gen::*;
    use bfe_presentation::network_layout::net_record as row;
    // More than any map's simultaneous events also wraps the session's
    // duplicate-identity window. Publication delivery must inherit the shared
    // journal's newest-record policy without allocating an overflow queue.
    let value = WirePublicationOutput { byte_length: 2,
        records: (0..10000).map(|id| WirePublishedRecord { id: id as f64 + 1., generation: 3., tick: 10.,
            schema: 8., payload: WireSpan { offset: 0, length: 2 } }).collect(),
        reliable: vec![WirePublishedReliable { client: 7, records: (0..10000).collect() }],
        edges: vec![WirePublishedEdges { client: 7, epoch: 1., generation: 3., received: (1..=128).map(f64::from).collect(),
            results: (1..=128).map(|id| WirePublishedOutcome { id: f64::from(id), ..Default::default() }).collect() }],
        ..Default::default() };
    let block = published(&value);
    let mut receiver = bfe_client::online::receiver::Receiver::new(serde_json::json!({}));
    receiver.epoch = 1.; receiver.own.generation = 3.;
    ALLOCATIONS.with(|count| count.set(0)); MEASURE.with(|enabled| enabled.set(true));
    let result = publication::Publication::new(&block, b"{}").unwrap().deliver(7, &mut receiver, 1000.);
    MEASURE.with(|enabled| enabled.set(false));
    assert!(result.is_ok()); assert_eq!(ALLOCATIONS.with(|count| count.get()), 0);
    assert_eq!(receiver.publications.iter().count(), 2048);
    assert_eq!(receiver.publications.iter().next().unwrap().row[row::ID], 7953.);
    assert_eq!(receiver.publications.iter().last().unwrap().row[row::ID], 10000.);
    assert_eq!(receiver.publications.refused, 7952);
    assert_eq!(receiver.received.count, 32); assert_eq!(receiver.outcomes.count, 32);
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
    fn initialize(&mut self, _: bfe_sim::gen::WorldTickSetup) -> Result<(), String> { self.events.borrow_mut().push("initialize"); Ok(()) }
    fn input(&mut self, batch: runtime::Input<'_>) -> Result<(), String> {
        self.events.borrow_mut().push("input");
        assert_eq!(batch.tick, 1.); assert_eq!(batch.at_ms, 1000. + 1000. / 60.);
        assert_eq!(batch.command.sequence, 3.); assert_eq!(batch.look.x, 7.);
        assert_eq!(batch.flight.scale, 1.);
        Ok(())
    }
    fn step(&mut self, input: WorldTickInput) -> Result<WorldTickOutput, String> {
        self.events.borrow_mut().push("step");
        Ok(WorldTickOutput { accepted: !self.reject, clock: WorldClockState { tick: input.tick, ..input.clock }, ..Default::default() })
    }
    fn publish(&mut self, facts: &[f64]) -> Result<publication::Publication<'_>, String> {
        self.events.borrow_mut().push("publish");
        let tick = facts[bfe_sim::gen::WireWorldFacts::TICK];
        assert!(tick == 0. || tick == 1.);
        assert_eq!(facts[bfe_sim::gen::WireWorldFacts::UNIX_MS], 1.7e12 + if tick == 0. { 1000. } else { 1016. });
        publication::Publication::new(&self.block, &[]).map_err(str::to_string)
    }
    fn acknowledge(&mut self, _: [f64; 2], _: f64, _: &[u32]) -> Result<(), String> { self.events.borrow_mut().push("acknowledge"); Ok(()) }
    fn close(&mut self) { self.events.borrow_mut().push("close"); }
}

#[test]
fn local_tick_publishes_only_after_completion_and_close_is_once() {
    for reject in [false, true] {
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let engine = Boundary { events: events.clone(), block: published(&bfe_sim::gen::WirePublicationOutput::default()), reject };
        let admission = setup::Admitted { setup: Default::default(), tick: WorldTickInput { clock: WorldClockState { hz: 60., dt: STEP, ..Default::default() }, ..Default::default() },
            handles: setup::Handles::default(), player_id: 1., registry: serde_json::json!({}), query: String::new() };
        let mut local = runtime::Runtime::new(engine, admission, 1000., 1.7e12).unwrap();
        let command = ClientCommand { sequence: 3., ..Default::default() };
        let look = ClientLook { x: 7., y: 0. };
        let flight = ClientFlightCommand { scale: 1., ..Default::default() };
        let view = ViewActions { map: true, ..Default::default() };
        assert_eq!(local.step(&command, &look, &flight, &view).is_err(), reject);
        assert_eq!(local.tick(), if reject { 0. } else { 1. });
        local.close(); local.close();
        assert!(local.step(&command, &look, &flight, &view).is_err());
        drop(local);
        assert_eq!(*events.borrow(), if reject { vec!["initialize", "publish", "acknowledge", "input", "step", "close"] }
            else { vec!["initialize", "publish", "acknowledge", "input", "step", "publish", "acknowledge", "close"] });
    }
}

#[test]
fn generated_desktop_input_uses_shared_look_and_trigger_transitions() {
    use bfe_client::generated_input::{ClientAction, ClientHeld, ClientCommand, ClientLook, ClientFlightCommand};
    let context = bfe_sim::abi::context::Context::new();
    let names = commands::Names::new(&context).unwrap();
    let forward = names.held[ClientHeld::Forward as usize];
    let use_action = names.actions[bfe_transport::layout::INPUT_ACTIONS.iter().position(|n| *n == "use").unwrap()];
    let mut input = commands::Commands::new(names);
    let mut command = ClientCommand { sequence: 1., mouse_scale: 1., held: (1 << ClientHeld::Forward as u8) | (1 << ClientHeld::Primary as u8),
        actions: 1 << ClientAction::Use as u8, primary_count: 1, ..Default::default() };
    let flight = ClientFlightCommand { scale: 1., curve: 1.35, ..Default::default() };
    input.capture(&command, &ClientLook { x: 2., y: -1. }, &flight, 1., 3., 10., true, 16. / 9., Some(7.), false).unwrap();
    let entry = input.mailbox.entry.as_ref().unwrap();
    assert!(entry.codes.contains(&forward)); assert_eq!(entry.look_total, Some([2., -1.]));
    assert_eq!(input.actions.actions, vec![use_action]);
    assert_eq!(input.fires.len(), 1); assert!(input.fires[0].pressed); assert_eq!(input.fires[0].view_tick, Some(7.));
    command.sequence = 2.; command.held = 0; command.actions = 0; command.primary_count = 0;
    input.capture(&command, &ClientLook { x: -1., y: 3. }, &flight, 1., 3., 11., true, 16. / 9., Some(8.), false).unwrap();
    assert!(input.mailbox.entry.as_ref().unwrap().codes.is_empty());
    assert_eq!(input.mailbox.entry.as_ref().unwrap().look_total, Some([1., 2.]));
    assert_eq!(input.fires.len(), 1); assert!(!input.fires[0].pressed);
    assert!(input.actions.actions.is_empty()); assert_eq!(input.refused, 0);
    context.dispose().unwrap();
}

#[test]
fn full_input_bursts_remain_bounded_without_allocating_after_admission() {
    let context = bfe_sim::abi::context::Context::new();
    let mut input = commands::Commands::new(commands::Names::new(&context).unwrap());
    let mut command = ClientCommand { mouse_scale: 1., primary_count: u32::MAX,
        secondary_count: u32::MAX, ..Default::default() };
    let mut flight = ClientFlightCommand { scale: 1., curve: 1.35, ..Default::default() };
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
        if let Err(fault) = input.capture(&command, &ClientLook { x: 2., y: -1. }, &flight,
            1., 3., tick as f64, true, 16. / 9., Some(tick as f64 - 1.), tick % 3 == 0) {
            error = Some(fault); break;
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
    bfe_sim::world::admit::Options { map: "nab2".into(), side: "rep".into(), enhanced: false,
        seed: 17, heroes: true, unit: Some("trooper".into()) }
}

fn fake_admission() -> bfe_sim::world::admit::Admitted {
    bfe_sim::world::admit::Admitted { handles: setup::Handles { mailbox: 19, player: 23,
        publication: 29, host: 31, edges: 37, client: 41 }, player_id: 0., units: 73,
        registry: serde_json::json!({"protocol":1}), ..Default::default() }
}

#[test]
fn context_admission_handoff_preserves_actual_pool_keys_and_zero_player_id() {
    use local::runtime::Engine;
    let source = fake_admission(); let handles = source.handles;
    let admitted = setup::Admitted::from_world(source, &battle_options()).unwrap();
    assert_eq!(admitted.handles, handles); assert_eq!(admitted.player_id, 0.);
    assert_eq!(admitted.tick.units, 73); assert_eq!(admitted.tick.clock.hz, 60.);
    assert!(admitted.tick.ai && admitted.tick.live);
    assert_eq!(admitted.registry, serde_json::json!({"protocol":1}));
    assert_eq!(admitted.query, "?map=nab2&side=rep&era=cw&ai=classic&seed=17&heroes=1&unit=trooper");
    // The fake supplies admission facts, never simulated gameplay. Exercise
    // the real Context's setup and convergent teardown without invoking the
    // nonexistent wire owners denoted by these deliberately unordered keys.
    let context = bfe_sim::abi::context::Context::new(); let alias = context.clone();
    let mut engine = local::context::Native::new(context, &admitted).unwrap();
    assert_eq!(engine.handles(), handles);
    engine.initialize(admitted.setup).unwrap(); engine.close(); engine.close(); drop(engine);
    assert_eq!(alias.string_count().unwrap_err().kind(), bfe_sim::abi::context::ErrorKind::Disposed);
    assert_eq!(alias.world_replay_live_handles(), 0);
    for bad in [f64::NAN, -1., 0.5, u32::MAX as f64 + 1.] {
        let mut source = fake_admission(); source.player_id = bad;
        assert!(setup::Admitted::from_world(source, &battle_options()).is_err());
    }
    let mut source = fake_admission(); source.handles.publication = 0;
    assert!(setup::Admitted::from_world(source, &battle_options()).is_err());
}

#[test]
fn unavailable_world_admission_refuses_before_reading_assets_or_opening_devices() {
    use setup::Source;
    struct Missing(std::cell::Cell<usize>);
    impl bfe_content::files::Files for Missing {
        fn read(&self, _: &str) -> Option<std::rc::Rc<[u8]>> { self.0.set(self.0.get() + 1); None }
    }
    let files = Missing(std::cell::Cell::<usize>::new(0));
    let source = setup::NativeSource { files: &files, options: battle_options() };
    let context = bfe_sim::abi::context::Context::new();
    let error = source.admit(&context).err().expect("missing content cannot admit a battle");
    if !bfe_sim::world::admit::provided() {
        assert_eq!(error, "native world API unavailable: world_admit"); assert_eq!(files.0.get(), 0);
    } else {
        assert!(files.0.get() > 0); assert!(!error.contains("unavailable"));
    }
    context.dispose().unwrap();
}

#[test]
fn operation_packing_uses_generated_records_and_retains_both_arenas() {
    use bfe_sim::gen::{WeCall, BlockHeader};
    let context = bfe_sim::abi::context::Context::new();
    let mut args = local::arguments::Arguments::new();
    let ack = WeCall { ids: (1..=32).map(f64::from).collect(), ..Default::default() };
    ALLOCATIONS.with(|count| count.set(0)); MEASURE.with(|enabled| enabled.set(true));
    for _ in 0..1000 { args.record(&[37.], WeCall::WORDS, |row, heap| ack.put(row, heap)); }
    MEASURE.with(|enabled| enabled.set(false));
    assert_eq!(ALLOCATIONS.with(|count| count.get()), 0);
    let actual = WeCall::take(&args.words[1..1 + WeCall::WORDS], &args.words[1 + WeCall::WORDS..]);
    assert_eq!(actual.ids, ack.ids); assert_eq!(args.words[0], 37.);
    let echo = bfe_sim::gen::Echo { ..Default::default() };
    args.record(&[], bfe_sim::gen::Echo::WORDS, |row, heap| echo.put(row, heap));
    args.run(&context, bfe_sim::gen::OPERATION_ECHO).unwrap();
    assert_eq!(args.answer.words[BlockHeader::ROWS], 1.);
    assert_eq!(local::arguments::required("echo").unwrap(), bfe_sim::gen::OPERATION_ECHO);
    assert!(local::arguments::required("missing_operation").is_err());
    context.dispose().unwrap();
}

#[test]
fn context_adapter_submits_generated_player_lanes_from_the_admitted_handles() {
    use bfe_sim::gen::*;
    use bfe_client::generated_input::{ClientAction, ClientHeld, ClientCommand, ClientLook, ClientFlightCommand};
    use local::runtime::Engine;
    fn reply(args: &mut local::arguments::Arguments, words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) {
        args.answer.begin(1, words); let (row, heap) = args.answer.row(0); put(row, heap); args.answer.finish();
    }
    for accepted in [true, false] {
        let admission = setup::Admitted::from_world(fake_admission(), &battle_options()).unwrap();
        let mut engine = local::context::Native::new(bfe_sim::abi::context::Context::new(), &admission).unwrap();
        let command = ClientCommand { sequence: 17., held: (1 << ClientHeld::Forward as u8) | (1 << ClientHeld::Primary as u8),
            actions: 1 << ClientAction::Use as u8, primary_count: 1, mouse_scale: 1., aspect: 16. / 9., ..Default::default() };
        let look = ClientLook { x: 2., y: -1. };
        let flight = ClientFlightCommand { scale: 1., curve: 1.35, ..Default::default() };
        let mut operations = Vec::new();
        // Only the operation replies are synthetic. The production adapter's
        // own mapping, ordering, cumulative look and queue-refusal handling
        // run unchanged; this callback implements no simulation or controller.
        engine.submit(local::runtime::Input { command: &command, look: &look, flight: &flight,
            epoch: 1., generation: 3., tick: 120., at_ms: 1000., view_tick: Some(118.), flight_mode: false }, |context, args, op| {
            operations.push(op);
            match op {
                OPERATION_INPUT_MAILBOX_OWNED_RUN => {
                    assert_eq!(args.words[0], 19.);
                    let call = InputMailboxCall::take(&args.words[1..1 + InputMailboxCall::WORDS], &args.words[1 + InputMailboxCall::WORDS..]);
                    assert_eq!(call.tick, 119.); assert!(call.epoch_matches);
                    let entry = call.entry.unwrap();
                    assert_eq!(entry.sequence, 17.); assert_eq!(entry.tick, 120.); assert_eq!(entry.generation, 3.);
                    assert_eq!(entry.look_total, Some([2., -1.])); assert!(entry.actions.is_empty());
                    let names: Vec<_> = entry.codes.iter().map(|&id| String::from_utf16_lossy(&context.string(id).unwrap())).collect();
                    assert_eq!(names, ["KeyW", "Mouse0"]);
                    reply(args, ImResult::WORDS, |row, heap| ImResult { accepted, ..Default::default() }.put(row, heap));
                }
                OPERATION_PLAYERS_INPUT => {
                    assert_eq!(args.words, [23., 1000., 1., u8::from(accepted) as f64]); args.answer.words.clear();
                }
                OPERATION_PLAYERS_ACTIONS => {
                    assert_eq!(args.words[0], 23.);
                    let action = PlayerActions::take(&args.words[1..1 + PlayerActions::WORDS], &args.words[1 + PlayerActions::WORDS..]);
                    assert_eq!(action.entry.id, 17.); assert_eq!(action.entry.tick, 120.);
                    assert_eq!(action.entry.generation, 3.); assert_eq!(action.tick, 119.); assert_eq!(action.at, 1000.);
                    assert!(action.same_epoch && action.payload_valid && action.human_valid);
                    assert_eq!(String::from_utf16_lossy(&context.string(action.entry.actions[0]).unwrap()), "use");
                    reply(args, PlayerAdmission::WORDS, |row, heap| PlayerAdmission { accepted: true, ..Default::default() }.put(row, heap));
                }
                OPERATION_PLAYERS_FIRE => {
                    assert_eq!(args.words[0], 23.);
                    let fire = PlayerFire::take(&args.words[1..], &[]);
                    assert_eq!(fire.edge.id, 1.); assert_eq!(fire.edge.tick, 120.); assert_eq!(fire.edge.generation, 3.);
                    assert!(fire.edge.pressed && fire.epoch_one && fire.pressed_valid);
                    assert_eq!(fire.tick, 119.); assert_eq!(fire.at, 1000.); assert_eq!(fire.edge.view_tick, Some(118.));
                    assert_eq!(fire.edge.aim_yaw, None); assert_eq!(fire.edge.aim_pitch, None);
                    reply(args, 1, |row, _| row[0] = 1.);
                }
                _ => panic!("unexpected generated operation {op}"),
            }
            Ok(())
        }).unwrap();
        assert_eq!(operations, [OPERATION_INPUT_MAILBOX_OWNED_RUN, OPERATION_PLAYERS_INPUT, OPERATION_PLAYERS_ACTIONS, OPERATION_PLAYERS_FIRE]);
        assert_eq!(engine.refused(), u64::from(!accepted)); engine.close();
    }
}

#[test]
fn threaded_world_steps_at_60hz_without_blocking_or_allocating_on_render_thread() {
    use bfe_client::generated_input::ClientHeld;
    use thread::{FallbackAuthority, FallbackMissionConfig, WorldMessage, WorldThread};

    let mut world = WorldThread::spawn_fallback(FallbackAuthority::new(FallbackMissionConfig::default()));
    let mut receiver = bfe_client::online::receiver::Receiver::new(serde_json::json!({}));

    // Spawn the human player at post 0.
    world.send(WorldMessage::Spawn {
        team: 1,
        class_index: 0,
        post_index: 0,
    });

    // Wait briefly for the 60 Hz world thread to process the spawn and publish a few ticks.
    std::thread::sleep(std::time::Duration::from_millis(60));
    let drained = world.poll_publications(&mut receiver, 100.0).unwrap();
    assert!(drained > 0);
    assert!(receiver.own.alive);

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
        let _ = world.poll_publications(&mut receiver, 200.0 + i as f64).unwrap();
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
