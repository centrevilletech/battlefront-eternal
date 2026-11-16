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
fn virtual_time_and_stale_handles_keep_voice_lifetimes_separate() {
    let mut runtime = Runtime::new(7); bank(&mut runtime, 0.4); let p = &mut runtime.phase; p.started = true;
    p.frame[frame::RUNNING] = 1.0; p.frame[frame::RATE] = 48000.0; p.frame[frame::MAX_VOICES] = 4.0; p.frame[frame::VIRTUAL_VOICES] = 8.0;
    p.frame[frame::TEAM] = 1.0; p.frame[frame::OWN] = -1.0; p.frame[frame::FZ] = -1.0; p.frame[frame::UY] = 1.0; p.engine.running = true;
    let one = p.play(audio_control::engine::Ask::new(0, 0)); assert_ne!(one, SILENT);
    for n in 0..27 { p.frame[frame::DT] = 1.0/60.0; p.frame[frame::NOW] = n as f64/60.0; p.frame[frame::WALL_NOW] = n as f64/60.0; p.step(&mut Room, &[]); }
    assert!(!p.playing(one)); let two = p.play(audio_control::engine::Ask::new(0, 0)); assert!(p.playing(two)); p.stop(one, 0.0); assert!(p.playing(two), "stale handle cannot stop the replacement");
}

#[test]
fn displayed_hull_feet_keep_placed_sounds_and_vehicle_gates() {
    let mut runtime = Runtime::new(7); bank(&mut runtime, 0.4);
    let p = &mut runtime.phase; p.engine.running = true;
    p.classes[0][class::STEP2] = 0.;
    p.classes[0][class::HYDRAULIC] = 0.;
    p.emitters[1][emitter::KIND] = 3.;
    p.emitters[1][emitter::ALIVE] = 1.;
    p.emitters[1][emitter::CLASS] = 0.;
    let mut row = [0.; event::STRIDE];
    row[event::KIND] = event_kind::HULL_FOOT as f64;
    row[event::OWNER] = 1.; row[event::VALUE] = 2.; row[event::PLACED] = 1.;
    row[event::X..event::X + 3].copy_from_slice(&[3., 1.5, 4.]);
    p.event(&row);
    let active = |p: &audio_control::phase::Phase| p.engine.voices.iter().filter(|v| v.sound != usize::MAX && !v.done).count();
    assert_eq!(active(p), 1);
    assert_eq!(p.engine.voices.iter().find(|v| v.sound != usize::MAX && !v.done).unwrap().position, Some([3., 1.5, 4.]));
    row[event::VALUE] = 6.; p.event(&row);
    assert_eq!(active(p), 2, "hydraulics are a separate placed one-shot");
    p.emitters[1][emitter::X] = 1000.; p.event(&row);
    p.emitters[1][emitter::X] = 0.; p.emitters[1][emitter::ALIVE] = 0.; p.event(&row);
    p.emitters[1][emitter::ALIVE] = 1.; row[event::VALUE] = 7.; p.event(&row);
    assert_eq!(active(p), 2, "distant, dead and invalid feet stay silent");
}
