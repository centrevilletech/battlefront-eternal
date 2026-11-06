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
fn unit_frame_has_no_allocation_even_during_lifecycle_changes() {
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
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    for row in 0..32 {
        assert_eq!(units.actor(content, &[identity; 4]), row);
        let at = f::STRIDE + row * i::STRIDE;
        let input = &mut units.input[at..at + i::STRIDE];
        input[i::ACTOR] = row as f64;
        input[i::ID] = row as f64;
        input[i::GENERATION] = 1.0;
        input[i::PRESENT] = 1.0;
        input[i::OWN] = if row == 0 { 1.0 } else { 0.0 };
        input[i::ALPHA] = 0.75;
        input[i::VELOCITY + 2] = 2.0;
        input[i::PREVIOUS_VELOCITY + 2] = 2.0;
        input[i::HELD_CLIP] = -1.0;
        input[i::DEATH_CLIP] = 0.0;
        input[i::ROOT_QUATERNION + 3] = 1.0;
        input[i::DIVE_TIME] = 0.6;
        input[i::POSE_PARENT..i::POSE_PARENT + 16].copy_from_slice(&identity);
        input[i::ATTACHMENT_PARENT..i::ATTACHMENT_PARENT + 16].copy_from_slice(&identity);
    }
    let view = units.view(FirstPersonInput {
        bones: bones(),
        clips: vec![clip()],
        sets: vec![[Some(0); 9]],
        forced_set: None,
        offset: identity,
    });
    units.input[f::FIRST_PERSON_CAMERA..f::FIRST_PERSON_CAMERA + 16].copy_from_slice(&identity);
    units.input[f::VERSION] = VERSION as f64;
    units.input[f::ACTORS] = 32.0;
    units.input[f::DT] = 1.0 / 60.0;
    units.input[f::FIRST_PERSON] = view as f64;
    units.input[f::FIRST_PERSON_ACTIVE] = 1.0;
    units.input[f::FIRST_PERSON_SHOWN] = 0.0;
    let mut valid = true;
    let mut posed = false;
    TRACK.with(|t| t.set(true));
    for frame in 0..600 {
        units.input[f::FRAME] = frame as f64;
        units.input[f::COMMANDS] = 1.0;
        units.commands[c::ACTOR] = (frame % 32) as f64;
        units.commands[c::KIND] = if frame % 30 == 0 { k::RESET } else { k::FIRE } as f64;
        for row in 0..32 {
            let at = f::STRIDE + row * i::STRIDE;
            // Exercise first attach, detach and a parent move between the
            // two phases as interface layout does, without growing children.
            units.input[at + i::HAS_POSE_PARENT] = (frame % 2) as f64;
            units.input[at + i::HAS_ATTACHMENT_PARENT] = ((frame + 1) % 2) as f64;
            units.input[at + i::POSE_PARENT + 12] = frame as f64 * 0.01;
            units.input[at + i::ATTACHMENT_PARENT + 12] = frame as f64 * 0.01 + 1.0;
            units.input[at + i::YAW] = frame as f64 * 0.01;
            units.input[at + i::POSITION] = row as f64 + frame as f64 * 0.01;
            units.input[at + i::PREVIOUS_POSITION] = row as f64 + (frame as f64 - 1.0) * 0.01;
            units.input[at + i::POSTURE] = ((frame / 40) % 3) as f64;
            units.input[at + i::AIR] = if frame % 100 < 30 { 1.0 } else { 0.0 };
            units.input[at + i::DIVE] = if frame % 80 < 10 { 1.0 } else { 0.0 };
            units.input[at + i::GENERATION] = if frame < 300 { 1.0 } else { 2.0 };
        }
        units.pose_frame();
        posed |= units.header[h::CHANGED] > 0.0;
        units.attachment_frame();
        valid &= units.header[h::STATUS] == 0.0;
        // The scene update then places the drawn remote soldiers.
        for row in 1..32 {
            units.places[row - 1] = row as u32;
        }
        units.place_actors(31);
        valid &= units.header[h::STATUS] == 0.0 && units.header[h::CHANGED] > 0.0;
    }
    TRACK.with(|t| t.set(false));
    assert!(valid);
    assert_eq!(ALLOCS.with(Cell::get), 0, "frame grew a native collection");
    assert!(posed, "frame must publish actual transformed nodes");
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
}
