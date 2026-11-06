use bfe_presentation::{
    layout::{self, command as c, frame as f, header as h},
    Limits, Presentation,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
struct Count;
// Per thread: tests run in parallel, and only the tracked frame counts.
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn tracking() -> bool {
    TRACK.try_with(Cell::get).unwrap_or(false)
}
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if tracking() {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if tracking() {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static ALLOCATOR: Count = Count;
#[test]
fn frames_include_births_and_expiry_without_allocating() {
    let content =
        serde_json::from_str(include_str!("../../../tests/fixtures/presentation.json")).unwrap();
    let mut p = Presentation::new(
        content,
        Limits {
            effects: 8,
            emitters: 16,
            particles: 64,
            commands: 16,
            events: 128,
        },
    );
    p.input[f::VERSION] = layout::VERSION as f64;
    p.input[f::PRESENT] = 1.0;
    p.input[f::DT] = 1.0 / 60.0;
    p.input[f::COMMANDS] = 1.0;
    let at = f::STRIDE;
    p.input[at + c::SEQUENCE] = 1.0;
    p.input[at + c::ID] = 1.0;
    p.input[at + c::GENERATION] = 1.0;
    p.input[at + c::CONTENT] = 1.0;
    p.input[at + c::SEED] = 1234.0;
    p.input[at + c::QUATERNION + 3] = 1.0;
    p.input[at + c::TINT] = 1.0;
    p.input[at + c::SCALE] = 1.0;
    TRACK.with(|t| t.set(true));
    p.frame();
    TRACK.with(|t| t.set(false));
    assert_eq!(p.header[h::STATUS], 0.0);
    assert_eq!(p.header[h::PARTICLES], 4.0);
    assert_eq!(ALLOCS.with(Cell::get), 0);
    p.input[f::COMMANDS] = 0.0;
    TRACK.with(|t| t.set(true));
    for frame in 1..240 {
        p.input[f::FRAME] = frame as f64;
        p.frame();
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(p.header[h::STATUS], 0.0);
    assert_eq!(p.header[h::PARTICLES], 0.0);
    assert_eq!(ALLOCS.with(Cell::get), 0);
}

// Every store tiny, births without limit, children per particle, an effect
// wider than the emitter store, and every kind of malformed command. The
// frame never fails, occupancy stays within the stores, and each death
// event pairs with an earlier birth of the same effect or particle.
#[test]
fn full_stores_refuse_births_and_events_still_pair() {
    use bfe_presentation::layout::{command_kind as ck, effect as e, event as ev, event_kind as ek};
    use std::collections::HashSet;
    let mut content: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/presentation.json")).unwrap();
    let mut particle = content["definitions"][0].clone();
    particle["maxParticles"] = serde_json::json!([-1, -1]);
    particle["burstCount"] = serde_json::json!([128, 128]);
    particle["burstDelay"] = serde_json::json!([0.01, 0.01]);
    let mut parent = particle.clone();
    parent["geometry"]["type"] = serde_json::json!("emitter");
    parent["child"] = serde_json::json!(1);
    content["definitions"] = serde_json::json!([particle, parent]);
    content["groups"] = serde_json::json!([[], [0], [1], [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]]);
    let limits = Limits {
        effects: 6,
        emitters: 10,
        particles: 50,
        commands: 8,
        events: 40,
    };
    let mut p = Presentation::new(serde_json::from_value(content).unwrap(), limits);
    let mut sequence = 0.0;
    let mut born: HashSet<(u32, u64)> = HashSet::new();
    let mut live: Vec<(f64, f64)> = Vec::new();
    let mut generation = [0.0f64; 6];
    p.input[f::VERSION] = layout::VERSION as f64;
    p.input[f::PRESENT] = 1.0;
    for frame in 0..400 {
        let mut n = 0;
        let mut command = |p: &mut Presentation, kind: u32, id: f64, generation: f64, killed: f64| {
            if n == 8 {
                return;
            }
            let at = f::STRIDE + n * c::STRIDE;
            p.input[at..at + c::STRIDE].fill(0.0);
            sequence += 1.0;
            p.input[at + c::SEQUENCE] = sequence;
            p.input[at + c::KIND] = kind as f64;
            p.input[at + c::ID] = id;
            p.input[at + c::GENERATION] = generation;
            p.input[at + c::CONTENT] = (2 + (id as usize % 2)) as f64;
            p.input[at + c::QUATERNION + 3] = 1.0;
            p.input[at + c::SCALE] = 1.0;
            p.input[at + c::KILLED] = killed;
            p.input[at + c::STOPPED] = killed;
            p.input[at + c::SECONDS] = f64::NAN;
            p.input[at + c::EMITTER] = 1e18;
            n += 1;
        };
        if frame < 300 {
            // A burst of births past the effect store, on every free id.
            for id in 1..6 {
                if !live.iter().any(|&(i, _)| i == id as f64) {
                    generation[id] += 1.0;
                    command(&mut p, ck::SPAWN, id as f64, generation[id], 0.0);
                }
            }
            // Stale, unknown and malformed commands, all skipped.
            command(&mut p, ck::SYNC, 1.0, generation[1] - 1.0, 1.0);
            command(&mut p, ck::NOZZLE, 2.0, generation[2], 0.0);
            command(&mut p, ck::PREWARM, 3.0, generation[3], 0.0);
            command(&mut p, 99, 1e9, 1.0, 0.0);
            if frame % 50 == 49 {
                // Recycle: kill every live effect so ids come back.
                for &(id, g) in &live {
                    command(&mut p, ck::SYNC, id, g, 1.0);
                }
            }
        } else {
            for &(id, g) in &live {
                command(&mut p, ck::SYNC, id, g, 1.0);
            }
        }
        p.input[f::COMMANDS] = if frame == 7 { 1e12 } else { n as f64 };
        p.input[f::FRAME] = frame as f64;
        // A long stall, a broken clock, and ordinary frames.
        p.input[f::DT] = match frame % 97 {
            5 => 30.0,
            11 => f64::NAN,
            12 => -1.0,
            _ => 1.0 / 60.0,
        };
        if frame == 50 {
            // Commands replayed out of order are skipped.
            sequence -= 20.0;
        }
        p.frame();
        assert_eq!(p.header[h::STATUS], 0.0, "frame {frame}");
        assert!(p.header[h::PARTICLES] <= 50.0 && p.header[h::EMITTERS] <= 10.0);
        assert!(p.header[h::EFFECTS] <= (6 + 8) as f64);
        assert!(p.header[h::EVENTS] <= 40.0);
        for k in 0..p.header[h::EVENTS] as usize {
            let row = &p.events[k * ev::STRIDE..(k + 1) * ev::STRIDE];
            let key = match row[ev::KIND] as u32 {
                ek::EFFECT_BIRTH | ek::EFFECT_DEATH => (0, row[ev::BIRTH_ORDER] as u64),
                _ => (1, row[ev::PARTICLE] as u64),
            };
            match row[ev::KIND] as u32 {
                ek::EFFECT_BIRTH | ek::PARTICLE_BIRTH => assert!(born.insert(key)),
                _ => assert!(born.remove(&key), "death without birth {key:?}"),
            }
        }
        live.clear();
        for k in 0..p.header[h::EFFECTS] as usize {
            let row = &p.effect_output[k * e::STRIDE..(k + 1) * e::STRIDE];
            if row[e::ID] != 0.0 && row[e::DONE] == 0.0 {
                live.push((row[e::ID], row[e::GENERATION]));
            }
        }
    }
    assert!(live.is_empty() && born.is_empty(), "every birth has its death");
    assert!(p.header[h::REFUSED_EFFECTS] > 0.0 && p.header[h::REFUSED_PARTICLES] > 0.0);
    assert!(p.header[h::SKIPPED_COMMANDS] > 0.0);
}
