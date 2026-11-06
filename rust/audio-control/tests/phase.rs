use audio_control::{Collision, Hit, Point};
use audio_control::runtime::Runtime;
use audio_control::layout::*;
use audio_control::engine::{Entry, Sound, Sample, SILENT};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { COUNT.with(|c| if let Some(n) = c.get() { c.set(Some(n + 1)); }); System.alloc(layout) }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 { COUNT.with(|c| if let Some(n) = c.get() { c.set(Some(n + 1)); }); System.alloc_zeroed(layout) }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 { COUNT.with(|c| if let Some(n) = c.get() { c.set(Some(n + 1)); }); System.realloc(pointer, layout, size) }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) { System.dealloc(pointer, layout); }
}
#[global_allocator] static ALLOCATOR: Allocator = Allocator;
struct Room;
impl Collision for Room {
    fn cast(&mut self, from: Point, direction: Point, reach: f64) -> Option<Hit> {
        let mut distance = f64::INFINITY;
        if direction[1] > 0.0 { distance = distance.min((5.0 - from[1]) / direction[1]); }
        if direction[0] > 0.0 { distance = distance.min((12.0 - from[0]) / direction[0]); }
        if distance < 0.0 || distance > reach { return None; }
        Some(Hit { distance, point: [from[0] + direction[0] * distance, from[1] + direction[1] * distance, from[2] + direction[2] * distance], terrain: false })
    }
}
fn packet(runtime: &mut Runtime, at: &mut usize, kind: usize, row: &[f64]) -> usize {
    runtime.input[*at + packet::KIND] = kind as f64; runtime.input[*at + packet::COUNT] = 1.0; *at += packet::STRIDE;
    let start = *at; runtime.input[start..start + row.len()].copy_from_slice(row); *at += row.len(); start
}
fn bank(runtime: &mut Runtime, duration: f64) {
    let mut row = [f64::NAN; sound::STRIDE]; row[sound::KIND] = 1.0; row[sound::BUS] = -1.0; row[sound::STREAM] = -1.0;
    row[sound::MODE3D] = 1.0; row[sound::MIN_DISTANCE] = 3.0; row[sound::MAX_DISTANCE] = 90.0; row[sound::MUTE_DISTANCE] = 80.0; row[sound::GAIN] = 0.6;
    runtime.phase.engine.sounds.push(Sound::load(&row, vec![Entry { id: 0, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }]));
    row[sound::KIND] = 4.0; row[sound::STREAM] = 0.0;
    runtime.phase.engine.sounds.push(Sound::load(&row, vec![Entry { id: 0, weight: 1.0, interval: 0.0, dev: 0.0, next: f64::NEG_INFINITY }]));
    runtime.phase.engine.samples.push(Sample { render_id: 0, duration, rate: 48000.0, scale: 1.0, available: true });
    runtime.phase.classes.push([0.0; class::STRIDE]); runtime.phase.reaches.push(f64::NAN); runtime.phase.foot_definitions.push(Vec::new()); runtime.phase.class_properties.push(vec![(17, 1)]);
    runtime.phase.layers.definitions.resize_with(2, || audio_control::layers::Definition { layers: Vec::new() });
    let mut parameter = row; parameter[sound::KIND] = 3.0;
    runtime.phase.engine.sounds.push(Sound::load(&parameter,Vec::new()));
    runtime.phase.layers.definitions.push(audio_control::layers::Definition { layers: vec![audio_control::layers::Layer { sound: 0, delay: 0.0, deviation: 0.0, bindings: vec![audio_control::layers::Binding { input: 0, pitch: false, points: vec![[0.0,0.2],[1.0,1.0]] }] }] });
    runtime.phase.classes[0][class::ENGINE] = 2.0;
    runtime.phase.foot_definitions[0].push(audio_control::feet::Foot::new([0.0,0.0,0.0], &[0.0,1.0,0.0,-1.0], 1.0, false, 0.15, 1.0, false, 1.0));
    runtime.phase.streams.definitions.push(audio_control::streams::Stream { slots: 1, channels: 1 }); runtime.phase.streams.segments.push(audio_control::streams::Segment {stream:0,duration:0.1,channels:1,variants:[usize::MAX;4]}); runtime.phase.streams.configure();
}
#[test]
fn complete_frame_and_asset_completion_allocate_nothing_from_the_first_call() {
    let mut runtime = Runtime::new(123); bank(&mut runtime, 0.5); let mut end = 0;
    let mut frame = [0.0; frame::STRIDE]; frame[frame::DT] = 1.0 / 60.0; frame[frame::RATE] = 48000.0; frame[frame::RUNNING] = 1.0; frame[frame::ENHANCED] = 1.0; frame[frame::LFE] = 1.0; frame[frame::MAX_VOICES] = 4.0; frame[frame::VIRTUAL_VOICES] = 8.0; frame[frame::FY] = 0.0; frame[frame::FZ] = -1.0; frame[frame::UY] = 1.0; frame[frame::TEAM] = 1.0; frame[frame::HUMAN] = 1.0; frame[frame::COLLISION] = 1.0;
    for key in [frame::EFFECTS_VOLUME, frame::AMBIENCE_VOLUME, frame::MUSIC_VOLUME, frame::VOICE_VOLUME, frame::INTERFACE_VOLUME] { frame[key] = 1.0; }
    let header = packet(&mut runtime, &mut end, input::FRAME, &frame);
    for (id, kind) in [1.0, 1.0, 3.0, 5.0, 4.0].into_iter().enumerate() {
        let mut row = [0.0; emitter::STRIDE]; row[emitter::ID] = id as f64; row[emitter::KIND] = kind; row[emitter::ALIVE] = 1.0; row[emitter::HEALTH] = 100.0; row[emitter::MAX_HEALTH] = 100.0; row[emitter::Z] = -3.0 - id as f64 * 2.0; row[emitter::GROUNDED] = 1.0; row[emitter::PILOTED] = 1.0; row[emitter::LIFE] = 2.0;
        row[emitter::FOOT0] = f64::NAN; row[emitter::FOOT1] = f64::NAN; row[emitter::FOOT2] = f64::NAN; packet(&mut runtime, &mut end, input::EMITTER, &row);
    }
    for id in 0..2 { let mut row = [0.0; weapon::STRIDE]; row[weapon::ID] = id as f64; row[weapon::OWNER] = id as f64; row[weapon::SELECTED] = 1.0; row[weapon::RELOADING] = 1.0; row[weapon::OVERHEATED] = 1.0; row[weapon::CHARGING] = 1.0; row[weapon::HEAT] = 2.0; row[weapon::THRESHOLD] = 1.0; row[weapon::MAX_PRESSED] = 1.0; packet(&mut runtime, &mut end, input::WEAPON, &row); }
    let mut post = [-1.0; post::STRIDE]; post[post::ID] = 0.0; post[post::CLASS] = 0.0; post[post::PRESENT] = 1.0; post[post::ACTIVE] = 1.0; post[post::TEAM] = 0.0; post[post::CONTESTED] = 0.0; post[post::PROGRESS] = 0.5; packet(&mut runtime, &mut end, input::POST, &post);
    let count = [0.0, 1.0, 1.0]; packet(&mut runtime, &mut end, input::COUNT, &count);
    let mut foot = [0.0; foot::STRIDE]; foot[foot::OWNER] = 2.0; foot[foot::PRESENT] = 1.0;
    for axis in 0..4 { foot[foot::M0 + axis * 5] = 1.0; } packet(&mut runtime, &mut end, input::FOOT, &foot);
    let mut item = [0.0; item::STRIDE]; item[item::MINE] = 1.0; item[item::MINE_BASE] = 1.0; item[item::LANDING_SOURCE] = 1.0; item[item::LANDED] = 1.0; item[item::PRESENT] = 1.0; item[item::SPAN] = 2.0; item[item::LIFE] = 1.9; packet(&mut runtime, &mut end, input::ITEM, &item);
    let mut event = [0.0; event::STRIDE]; event[event::KIND] = event_kind::START as f64; let start = packet(&mut runtime, &mut end, input::EVENT, &event);
    event[event::KIND] = event_kind::FIRE as f64; packet(&mut runtime, &mut end, input::EVENT, &event);
    event[event::KIND] = event_kind::CHATTER as f64; event[event::SOUND] = 17.0; event[event::POSITIONAL] = 1.0; packet(&mut runtime, &mut end, input::EVENT, &event);
    event[event::KIND] = event_kind::TEST_CHANNEL as f64; event[event::AMOUNT] = 0.1; event[event::SECONDS] = 0.2; event[event::HEALTH] = 0.35; event[event::SOUND] = 0.0;
    let diagnostic = packet(&mut runtime, &mut end, input::EVENT, &event);
    let mut commands_emitted = false;
    COUNT.with(|c| c.set(Some(0)));
    for frame in 0..1000 {
        runtime.clear_output(); runtime.input[header + frame::NOW] = frame as f64 / 60.0; runtime.input[header + frame::WALL_NOW] = frame as f64 / 60.0; runtime.feed(end); runtime.step(&mut Room); commands_emitted |= !runtime.phase.engine.commands.is_empty();
        runtime.input[start + event::KIND] = event_kind::FIRE as f64; runtime.input[diagnostic + event::KIND] = event_kind::FIRE as f64;
    }
    let allocations = COUNT.with(|c| c.replace(None).unwrap());
    assert_eq!(allocations, 0); assert!(runtime.phase.engine.stats[2] > 10); assert!(runtime.phase.engine.stats[3] > 0); assert!(commands_emitted);
    let mut ready = [0.0; ready::STRIDE]; ready[ready::VOICE] = 0.0; ready[ready::GENERATION] = 1.0; ready[ready::DURATION] = 0.1; ready[ready::CHANNELS] = 1.0; ready[ready::TIME] = 17.0; ready[ready::WALL_NOW] = 17.0; runtime.input[..ready.len()].copy_from_slice(&ready);
    COUNT.with(|c| c.set(Some(0))); runtime.ready(); let allocations = COUNT.with(|c| c.replace(None).unwrap()); assert_eq!(allocations, 0);
}
#[test]
fn virtual_time_and_stale_handles_keep_voice_lifetimes_separate() {
    let mut runtime = Runtime::new(7); bank(&mut runtime, 0.4); let p = &mut runtime.phase; p.started = true;
    p.frame[frame::RUNNING] = 1.0; p.frame[frame::RATE] = 48000.0; p.frame[frame::MAX_VOICES] = 4.0; p.frame[frame::VIRTUAL_VOICES] = 8.0;
    p.frame[frame::TEAM] = 1.0; p.frame[frame::OWN] = -1.0; p.frame[frame::FZ] = -1.0; p.frame[frame::UY] = 1.0; p.engine.running = true;
    let one = p.play(audio_control::engine::Ask::new(0, 0)); assert_ne!(one, SILENT);
    for n in 0..27 { p.frame[frame::DT] = 1.0/60.0; p.frame[frame::NOW] = n as f64/60.0; p.frame[frame::WALL_NOW] = n as f64/60.0; p.step(&mut Room, &[]); }
    assert!(!p.playing(one)); let two = p.play(audio_control::engine::Ask::new(0, 0)); assert!(p.playing(two)); p.stop(one, 0.0); assert!(p.playing(two), "stale handle cannot stop the replacement");
}
