//! Synthetic coverage of the complete exchange path, including command
//! dispatch, interpolation, LOD, native matrices, skins, and first person.
use bfe_presentation::{
    layout::{
        unit_command as c, unit_command_kind as k, unit_frame as f, unit_header as h,
        unit_input as i, VERSION,
    },
    unit_content::{Bone, ClipInput, FirstPersonInput, Input, TableInput},
    units::Units,
};
use bfe_presentation::layout::unit_collision_input as ci;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
thread_local! {static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
struct Counting;
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|n| n.set(n.get() + 1));
        }
        System.realloc(p, l, n)
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
fn bone(name: &str, parent: i32) -> Bone {
    Bone {
        name: name.into(),
        parent,
        pose: [0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    }
}
fn clip() -> ClipInput {
    ClipInput {
        frames: 2,
        fps: 30.0,
        root_motion: true,
        tracks: vec![0, 1, 2, 3],
        data: (0..8)
            .flat_map(|_| [0.0, 0.0, 0.0, 1.0, 0.0, 0.1, 0.0])
            .collect(),
        duration: 1.0 / 30.0,
        speed: 2.0,
        idle: false,
    }
}
fn bones() -> Vec<Bone> {
    vec![
        bone("DummyRoot", -1),
        bone("bone_pelvis", 0),
        bone("bone_a_spine", 1),
        bone("bone_ribcage", 2),
    ]
}
#[test]
fn overfull_counts_and_unknown_ids_never_fail_a_frame() {
    // A menu-sized kernel: one actor and 64 commands. The host claims far
    // more rows, commands and placements than fit, and names actors that
    // were never registered; every phase clamps or skips instead of failing.
    let mut units = Units::preview();
    let content = units.load(Input {
        bones: bones(),
        clips: vec![clip()],
        tables: TableInput {
            sets: [[Some(0); 9]; 3],
            alert: [Some(0); 9],
            transitions: [[Some(0); 3]; 3],
            jump: Some(0),
            fall: Some(0),
            land_soft: Some(0),
            land_hard: Some(0),
            dive: Some(0),
            jet: Some(0),
            thrown: [[Some(0); 4]; 2],
            upper: vec![vec![0]],
        },
        walker: None,
    });
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    assert_eq!(units.actor(content, &[identity; 4]), 0);
    let input = &mut units.input[f::STRIDE..f::STRIDE + i::STRIDE];
    input[i::ACTOR] = 7.0;
    input[i::PRESENT] = 1.0;
    input[i::GENERATION] = 1.0;
    input[i::ROOT_QUATERNION + 3] = 1.0;
    for at in 0..units.commands.len() / c::STRIDE {
        units.commands[at * c::STRIDE + c::ACTOR] = (at % 3) as f64 * 1e6;
        units.commands[at * c::STRIDE + c::KIND] = if at % 2 == 0 { k::FIRE } else { 999 } as f64;
    }
    units.input[f::VERSION] = VERSION as f64;
    units.input[f::DT] = 1.0 / 60.0;
    units.input[f::FIRST_PERSON] = -1.0;
    for frame in 0..4 {
        units.input[f::FRAME] = frame as f64;
        units.input[f::ACTORS] = 1e9;
        units.input[f::COMMANDS] = 1e12;
        units.input[f::STRIDE + i::ACTOR] = if frame < 2 { 7.0 } else { 0.0 };
        units.pose_frame();
        assert_eq!(units.header[h::STATUS], 0.0);
        units.attachment_frame();
        assert_eq!(units.header[h::STATUS], 0.0);
        units.places[0] = if frame % 2 == 0 { 0 } else { u32::MAX };
        units.place_actors(usize::MAX);
        units.refresh_actor(usize::MAX);
        assert_eq!(units.header[h::STATUS], 0.0);
    }
    // A query prefix clamps its count and skips unknown resource handles.
    // Ordinary frame() must leave even an enabled, stale collision row alone.
    use bfe_presentation::layout::unit_collision_output as co;
    units.collision.input[ci::ENABLED]=1.0;
    units.collision.input[ci::SCALE]=1.0;
    units.collision.input[ci::SOURCE_PROP] = -1.0;
    units.frame();assert_eq!(units.collision.output[co::COUNT],0.0);
    units.collision.actors[0]=u32::MAX;
    units.collision_frame(usize::MAX);assert_eq!(units.collision.output[co::COUNT],0.0);
    units.collision.actors[0]=0;
    units.collision_frame(usize::MAX);assert_eq!(units.collision.output[co::COUNT],2.0);

}
#[test]
fn leaving_a_seat_gives_the_body_its_own_clips_back() {
    let mut units = Units::new();
    let content = units.load(Input {
        bones: bones(),
        clips: vec![clip()],
        tables: TableInput {
            sets: [[Some(0); 9]; 3],
            alert: [Some(0); 9],
            transitions: [[Some(0); 3]; 3],
            jump: Some(0),
            fall: Some(0),
            land_soft: Some(0),
            land_hard: Some(0),
            dive: Some(0),
            jet: Some(0),
            thrown: [[Some(0); 4]; 2],
            upper: vec![vec![0]],
        },
        walker: None,
    });
    let identity = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
    assert_eq!(units.actor(content, &[identity; 4]), 0);
    units.input[f::VERSION] = VERSION as f64;
    units.input[f::ACTORS] = 1.0;
    units.input[f::DT] = 1.0 / 60.0;
    units.input[f::FIRST_PERSON] = -1.0;
    let at = f::STRIDE;
    let input = &mut units.input[at..at + i::STRIDE];
    input[i::ID] = 7.0;
    input[i::GENERATION] = 1.0;
    input[i::PRESENT] = 1.0;
    input[i::ALPHA] = 1.0;
    input[i::HELD_CLIP] = -1.0;
    input[i::ROOT_QUATERNION + 3] = 1.0;
    input[i::DIVE_TIME] = 0.6;
    // A remote gunner at a turret.
    input[i::MOUNT_PRESENT] = 1.0;
    input[i::MOUNT_ALIVE] = 1.0;
    input[i::MOUNT_ID] = 37.0;
    input[i::SEAT] = 1.0;
    input[i::RIDER_KIND] = 2.0;
    input[i::RIDER_CLIP] = 0.0;
    input[i::RIDER_MATRIX..i::RIDER_MATRIX + 16].copy_from_slice(&identity);
    for _ in 0..10 {
        units.pose_frame();
    }
    assert!(units.actor_animation(0).unwrap().gunning.is_some());
    // He gets out and runs.
    let input = &mut units.input[at..at + i::STRIDE];
    input[i::MOUNT_PRESENT] = 0.0;
    input[i::SEAT] = 0.0;
    input[i::RIDER_KIND] = 0.0;
    input[i::VELOCITY + 2] = 6.0;
    input[i::PREVIOUS_VELOCITY + 2] = 6.0;
    for _ in 0..10 {
        units.pose_frame();
    }
    let animation = units.actor_animation(0).unwrap();
    assert!(animation.gunning.is_none(), "the turret pose outlived the seat");
    assert!(animation.base_layers().count() > 0);
}
