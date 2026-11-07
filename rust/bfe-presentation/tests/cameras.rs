use bfe_presentation::{
    cameras::{math::V8, Cast, Content, Phase},
    layout::{
        self, camera_command as c, camera_command_kind as k, camera_frame as f,
        camera_limits as limits, camera_output as o,
    },
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use three::Vector3;
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

struct World {
    casts: usize,
    random: usize,
    wall: bool,
}
impl Cast for World {
    fn cast(&mut self, _p: Vector3, _d: Vector3, far: f64, _static_only: bool) -> Option<f64> {
        self.casts += 1;
        if self.wall {
            Some(far * 0.5)
        } else {
            None
        }
    }
    fn random(&mut self) -> f64 {
        self.random += 1;
        0.25
    }
}
fn fixture() -> Phase {
    let mut p = Phase::new(Content::default());
    let a = &mut p.input;
    a[f::VERSION] = layout::VERSION as f64;
    a[f::DT] = 1.0 / 60.0;
    a[f::ZOOM] = 1.0;
    a[f::RESPONSIVE] = 1.0;
    a[f::CAMERA_QUATERNION + 3] = 1.0;
    a[f::CAMERA_UP + 1] = 1.0;
    a[f::CAMERA_FOV] = 65.0;
    a[f::CAMERA_ASPECT] = 16.0 / 9.0;
    a[f::CAMERA_NEAR] = 0.1;
    a[f::CAMERA_FAR] = 1000.0;
    a[f::VEHICLE_ID] = 1.0;
    a[f::VEHICLE_HEALTH] = 100.0;
    a[f::VEHICLE_DRAWN + 3] = 1.0;
    a[f::VEHICLE_FLIGHT_FRAME + 3] = 1.0;
    a[f::VEHICLE_DIRECTION + 2] = 1.0;
    a[f::VEHICLE_MAX_SPEED] = 20.0;
    for i in 0..4 {
        a[f::VEHICLE_FRAME + i * 5] = 1.0;
        a[f::VEHICLE_HULL + i * 5] = 1.0;
        a[f::TURRET_ROOT + i * 5] = 1.0;
    }
    a[f::SEAT_OFFSET + 2] = 4.0;
    a[f::SEAT_FIRST] = 1.0;
    a[f::SEAT_THIRD] = 1.0;
    a[f::SEAT_AIM] = 30.0;
    a[f::SEAT_RATES..f::SEAT_RATES + 6].fill(30.0);
    a[f::TURRET_UP + 1] = 1.0;
    a[f::SEAT_UP + 1] = 1.0;
    a[f::TURRET_CANONICAL_DIRECTION] = 1.0;
    a[f::CANONICAL_DIRECTION] = 1.0;
    a[f::TURRET_DIRECTION + 2] = 1.0;
    a[f::TURRET_OFFSET + 2] = 4.0;
    a[f::TURRET_FIRST] = 1.0;
    a[f::TURRET_THIRD] = 1.0;
    a[f::TURRET_RATES..f::TURRET_RATES + 6].fill(30.0);
    a[f::REMOTE_ZOOM] = 1.0;
    a[f::DEATH_HFOV] = 1.0;
    p
}
#[test]
fn camera_priority_is_exclusive_and_spawn_overrides_after_the_pre_shake_record() {
    let mut p = fixture();
    let mut world = World {
        casts: 0,
        random: 0,
        wall: false,
    };
    p.input[f::VEHICLE_ACTIVE] = 1.0;
    p.input[f::TURRET_ACTIVE] = 1.0;
    p.frame::<V8, _>(&mut world);
    assert_eq!(p.output[o::MODE], 3.0);
    p.input[f::REMOTE_ACTIVE] = 1.0;
    p.frame::<V8, _>(&mut world);
    assert_eq!(p.output[o::MODE], 2.0);
    p.input[f::DEATH_ACTIVE] = 1.0;
    p.frame::<V8, _>(&mut world);
    assert_eq!(p.output[o::MODE], 1.0);
    p.input[f::HAS_SHOT] = 1.0;
    p.input[f::SHOT_POSITION] = 99.0;
    p.input[f::SHOT_QUATERNION + 3] = 1.0;
    p.frame::<V8, _>(&mut world);
    assert_eq!(p.output[o::POSITION], 99.0);
    assert_ne!(p.output[o::PRE_POSITION], 99.0);
    // A death still decays shakes, but never asks the cosmetic stream for
    // turns. A spawn shot also leaves the underlying camera handoff intact.
    p.input[f::COMMANDS] = 1.0;
    p.commands[c::KIND] = k::ADD_SHAKE as f64;
    p.commands[c::AMOUNT] = 1.0;
    p.commands[c::LENGTH] = 0.5;
    p.frame::<V8, _>(&mut world);
    assert_eq!(world.random, 0);
    assert_eq!(p.output[o::SHAKE_TOTAL], 1.0);
}
#[test]
fn paused_bursts_and_forged_counts_remain_bounded_and_frames_allocate_nothing() {
    let mut p = fixture();
    let mut world = World {
        casts: 0,
        random: 0,
        wall: true,
    };
    p.input[f::SOLDIER_DEF] = 1e30;
    p.input[f::REMOTE_DEF] = 1e30;
    for row in 0..limits::COMMANDS {
        let at = row * c::STRIDE;
        p.commands[at + c::KIND] = k::ADD_SHAKE as f64;
        p.commands[at + c::AMOUNT] = 1.0;
        p.commands[at + c::LENGTH] = 1.0;
    }
    p.input[f::COMMANDS] = 1e30;
    p.input[f::FROZEN] = 1.0;
    // Every mode is exercised within the same tracked steady-state phase,
    // including the four-shake limit and a long pause with no intervening step.
    ALLOCS.with(|n| n.set(0));
    TRACK.with(|t| t.set(true));
    for frame in 0..1200 {
        let mode = frame % 5;
        p.input[f::DEATH_ACTIVE] = (mode == 1) as u8 as f64;
        p.input[f::REMOTE_ACTIVE] = (mode == 2) as u8 as f64;
        p.input[f::VEHICLE_ACTIVE] = (mode == 3) as u8 as f64;
        p.input[f::TURRET_ACTIVE] = (mode == 4) as u8 as f64;
        p.input[f::FIRST_PERSON] = (frame % 2 == 0) as u8 as f64;
        p.input[f::VEHICLE_FLYING] = (frame % 3 == 0) as u8 as f64;
        p.input[f::DT] = if frame == 100 { 30.0 } else { 1.0 / 60.0 };
        p.input[f::FROZEN] = (frame < 60) as u8 as f64;
        p.frame::<V8, _>(&mut world);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(p.output[o::STATUS], 0.0);
    assert!(p.output[o::REFUSED] > 0.0);
    assert!(p.output[o::SKIPPED] > 0.0);
    assert!(world.casts > 0);
    assert!(world.random > 0);
}

#[test]
fn excessive_cold_tables_preserve_admitted_ids_and_a_usable_frame() {
    use bfe_presentation::cameras::{soldier, spawn};
    let content = Content {
        definitions: (0..limits::DEFINITIONS + 64)
            .map(|_| soldier::Def::default())
            .collect(),
        spawn_shots: (0..limits::SPAWN_SHOTS + 64)
            .map(|_| spawn::Shot {
                position: [0.0; 3],
                quaternion: [0.0, 0.0, 0.0, 1.0],
            })
            .collect(),
        ..Content::default()
    };
    let mut p = Phase::new(content);
    assert!(p.add_definition(soldier::Def::default()).is_none());
    let index = p.show_shot(1e30);
    assert!(index >= 0 && (index as usize) < limits::SPAWN_SHOTS);
    let revision = p.director.revision();
    assert_eq!(p.show_shot(f64::NAN), -1);
    let initial = fixture();
    p.input.copy_from_slice(&initial.input);
    p.input[f::SOLDIER_DEF] = -1.0;
    p.input[f::HAS_SHOT] = 1.0;
    p.input[f::DIRECTOR] = 1.0;
    p.input[f::DIRECTOR_REVISION] = revision as f64;
    let mut world = World {
        casts: 0,
        random: 0,
        wall: false,
    };
    p.frame::<V8, _>(&mut world);
    assert_eq!(p.output[o::STATUS], 0.0);
    assert!(p.output[o::SKIPPED] >= 129.0);
    assert!(p.output[o::MATRIX..o::MATRIX + 16]
        .iter()
        .all(|x| x.is_finite()));
}

#[test]
fn death_radius_keeps_the_original_hypot_argument_count() {
    use bfe_presentation::cameras::math::Math;
    // DeathShot.begin's distant incoming view exposes the compensation left
    // by the second squared component. A third zero changes the last bit.
    let x = -5.69339564426776;
    let z = 119.59428479214932;
    assert_eq!(V8::hypot2(x, z).to_bits(), 119.7297277575953f64.to_bits());
    assert_ne!(V8::hypot2(x, z).to_bits(), V8::hypot(x, z, 0.0).to_bits());
}
