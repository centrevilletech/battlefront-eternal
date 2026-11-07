//! The page's prop frame: doors, triggers, a creature following adopted
//! world state and a log trap, the record counts at and past their limits,
//! and no allocation once registered.
use bfe_presentation::{
    layout::{
        self, prop_adopted as pa, prop_body as pb, prop_bone as bo, prop_event as pe,
        prop_frame as fr, prop_header as ph, prop_input as pi, prop_limits as lim,
        prop_output as po, trap_input as ti, trap_output as to, trap_phase as tp,
    },
    props::{Props, PropInput, TrapDesc, TriggerInput},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
struct Count;
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

// A two-bone clip whose bone 1 quaternion reads (0, sin a, 0, cos a), a
// rising to pi/4 over 31 frames at 30 a
// second (bone 0 still).
fn clip(props: &mut Props) -> u32 {
    let frames = 31;
    let mut data = Vec::new();
    for bone in 0..2 {
        for f in 0..frames {
            let a = if bone == 1 { f as f64 / 30.0 * std::f64::consts::FRAC_PI_4 } else { 0.0 };
            data.extend_from_slice(&[0.0, a.sin() as f32, 0.0, a.cos() as f32, 0.0, bone as f32, 0.0]);
        }
    }
    props.load_clip(frames, 30.0, false, vec![0, 1], data)
}
fn bones() -> Vec<f64> {
    vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0]
}
fn prop(door: bool, clip: u32, triggers: Vec<TriggerInput>, grabs: bool) -> PropInput {
    PropInput {
        root: 0,
        initial: bones(),
        bind: bones(),
        door,
        door_clip: door.then_some(clip),
        door_duration: 1.0,
        slots: vec![Some(clip), Some(clip)],
        open_slots: vec![false, true],
        triggers,
        opens: !door,
        body: true,
        idle_delay: 0.0,
        grabs,
        time: 0.25,
    }
}
fn frame(p: &mut Props, dt: f64, tick: f64, feet: &[[f64; 3]], bodies: &[[f64; 4]]) {
    p.input[fr::VERSION] = layout::VERSION as f64;
    p.input[fr::DT] = dt;
    p.input[fr::TICK] = tick;
    // The host writes only the records it has; counts past them clamp.
    for (i, f) in feet.iter().take(lim::FEET).enumerate() {
        p.input[Props::feet_at() + i * 3..][..3].copy_from_slice(f);
    }
    for (i, b) in bodies.iter().take(lim::BODIES).enumerate() {
        p.input[Props::bodies_at() + i * pb::STRIDE..][..4].copy_from_slice(b);
    }
    p.input[fr::FEET] = feet.len() as f64;
    p.input[fr::BODIES] = bodies.len() as f64;
    p.frame();
}

#[test]
fn a_door_opens_while_feet_are_near_and_drops_its_collision() {
    let mut p = Props::new();
    let c = clip(&mut p);
    let point = [10.0, 0.0, 0.0];
    p.add_prop(prop(true, c, vec![TriggerInput { slot: -1, point, radius: 2.0, sound: false }], false));
    frame(&mut p, 0.25, 1.0, &[[11.0, 0.0, 0.0]], &[]);
    assert_eq!(p.prop_output[po::POSED], 1.0);
    assert_eq!(p.prop_output[po::BODY_ENABLED], 1.0, "a quarter open still blocks");
    frame(&mut p, 0.5, 2.0, &[[11.0, 0.0, 0.0]], &[]);
    assert_eq!(p.prop_output[po::BODY_ENABLED], 0.0, "three quarters open lets through");
    // Bone 1 at three quarters of the clip: a turn of 3/16 pi about y.
    let a = 0.75 * std::f64::consts::FRAC_PI_4;
    assert!((p.bones[bo::STRIDE + bo::QUATERNION + 1] - a.sin()).abs() < 1e-6);
    frame(&mut p, 1.0, 3.0, &[[20.0, 0.0, 0.0]], &[]);
    assert_eq!(p.prop_output[po::BODY_ENABLED], 1.0, "shut once no one is near");
}

#[test]
fn a_trigger_plays_its_action_once_with_its_sound() {
    let mut p = Props::new();
    let c = clip(&mut p);
    let point = [0.0, 1.0, 0.0];
    p.add_prop(prop(false, c, vec![TriggerInput { slot: 1, point, radius: 1.0, sound: true }], false));
    let on_foot = [[0.0, 0.0, 0.0, 1.0]];
    frame(&mut p, 1.0 / 60.0, 1.0, &[], &on_foot);
    assert_eq!(p.header[ph::EVENTS], 1.0);
    assert_eq!(p.events[pe::TRIGGER], 0.0);
    assert_eq!(p.prop_output[po::SLOT], 1.0);
    assert_eq!(p.prop_output[po::BODY_ENABLED], 0.0, "an open action lets through");
    frame(&mut p, 1.0 / 60.0, 2.0, &[], &on_foot);
    assert_eq!(p.header[ph::EVENTS], 0.0, "the action plays on");
    // A body not on foot sets nothing off.
    let mut q = Props::new();
    let c = clip(&mut q);
    q.add_prop(prop(false, c, vec![TriggerInput { slot: 1, point, radius: 1.0, sound: true }], false));
    frame(&mut q, 1.0 / 60.0, 1.0, &[], &[[0.0, 0.0, 0.0, 0.0]]);
    assert_eq!(q.header[ph::EVENTS], 0.0);
    assert_eq!(q.prop_output[po::SLOT], 0.0);
}

#[test]
fn a_creature_idles_until_adopted_then_follows_the_world() {
    let mut p = Props::new();
    let c = clip(&mut p);
    p.add_prop(prop(false, c, vec![TriggerInput { slot: 1, point: [0.0; 3], radius: 9.0, sound: true }], true));
    // Before the world's state: it idles and feels no bodies.
    frame(&mut p, 1.0 / 60.0, 5.5, &[], &[[0.0, 0.0, 0.0, 1.0]]);
    assert_eq!(p.header[ph::EVENTS], 0.0);
    assert_eq!((p.prop_output[po::SLOT], p.prop_output[po::STARTED]), (0.0, 5.0));
    // Adopted: the action from tick 10, stood at once on the first state.
    p.prop_input[pi::ADOPTED] = pa::FIRST as f64;
    p.prop_input[pi::SLOT] = 1.0;
    p.prop_input[pi::STARTED] = 10.0;
    p.prop_input[pi::START_TIME] = 0.0;
    frame(&mut p, 0.0, 25.0, &[], &[]);
    assert_eq!(p.prop_output[po::POSED], 1.0);
    let a = (16.0 / 60.0) * std::f64::consts::FRAC_PI_4;
    assert!((p.bones[bo::STRIDE + bo::QUATERNION + 1] - a.sin()).abs() < 1e-6);
    // Following with no time passing poses nothing short of a full weight.
    p.prop_input[pi::ADOPTED] = pa::FOLLOWING as f64;
    frame(&mut p, 0.0, 10.5, &[], &[]);
    assert_eq!(p.prop_output[po::POSED], 0.0);
}

#[test]
fn a_trap_rests_by_its_phase_and_keeps_unweighted_bones() {
    let mut p = Props::new();
    let c = clip(&mut p);
    let mut initial = bones();
    initial.extend_from_slice(&[0.0, 0.0, 0.0, 1.0, 5.0, 5.0, 5.0]);
    p.add_trap(TrapDesc { root: 0, initial, trigger_clip: Some(c), build_clip: None, trigger_length: 31.0 / 30.0 });
    let start = p.bone_start(true, 0);
    p.trap_input[ti::PHASE] = tp::DOWN as f64;
    p.trap_input[ti::STARTED] = f64::NAN;
    frame(&mut p, 0.0, 3.0, &[], &[]);
    assert_eq!(p.trap_output[to::POSED], 1.0);
    let q = &p.bones[(start + 1) * bo::STRIDE..];
    assert!((q[1] - std::f64::consts::FRAC_PI_4.sin()).abs() < 1e-6, "down: the swing's end");
    assert_eq!(&p.bones[(start + 2) * bo::STRIDE + 4..][..3], &[5.0, 5.0, 5.0]);
    p.trap_input[ti::PHASE] = tp::ARMED as f64;
    frame(&mut p, 0.0, 4.0, &[], &[]);
    assert!(p.bones[(start + 1) * bo::STRIDE + 1].abs() < 1e-9, "armed: the swing's start");
}

#[test]
fn records_past_their_limits_clamp_and_a_stale_layout_poses_nothing() {
    let mut p = Props::new();
    let c = clip(&mut p);
    let point = [0.0; 3];
    p.add_prop(prop(true, c, vec![TriggerInput { slot: -1, point, radius: 1.0, sound: false }], false));
    p.add_prop(prop(false, c, vec![TriggerInput { slot: 1, point, radius: 1.0, sound: true }], false));
    // Every prop's trigger at once, with counts far past the records: the
    // frame reads the records it has, at most one event per prop.
    let feet = vec![[50.0, 0.0, 0.0]; lim::FEET + 7];
    let bodies = vec![[50.0, 0.0, 0.0, 1.0]; lim::BODIES + 7];
    frame(&mut p, 1.0 / 60.0, 1.0, &feet, &bodies);
    assert_eq!(p.header[ph::EVENTS], 0.0);
    p.input[fr::FEET] = 1e12;
    p.input[fr::BODIES] = f64::INFINITY;
    p.frame();
    p.input[fr::FEET] = -3.0;
    p.input[fr::BODIES] = f64::NAN;
    p.frame();
    p.input[fr::VERSION] = 0.0;
    p.frame();
    assert_eq!(p.header[ph::STATUS], layout::status::VERSION_MISMATCH as f64);
    assert_eq!(p.header[ph::EVENTS], 0.0);
    // Prop input naming a slot past its clips plays nothing, without panic.
    let mut q = Props::new();
    let c = clip(&mut q);
    q.add_prop(prop(false, c, vec![], true));
    q.prop_input[pi::ADOPTED] = pa::FIRST as f64;
    q.prop_input[pi::SLOT] = 99.0;
    frame(&mut q, 1.0 / 60.0, 1.0, &[], &[]);
    assert_eq!(q.prop_output[po::POSED], 1.0);
}

#[test]
fn frames_do_not_allocate() {
    let mut p = Props::new();
    let c = clip(&mut p);
    let point = [0.0; 3];
    p.add_prop(prop(true, c, vec![TriggerInput { slot: -1, point, radius: 1.0, sound: false }], false));
    p.add_prop(prop(false, c, vec![TriggerInput { slot: 1, point, radius: 1.0, sound: true }], false));
    p.add_prop(prop(false, c, vec![], true));
    p.add_trap(TrapDesc { root: 0, initial: bones(), trigger_clip: Some(c), build_clip: Some(c), trigger_length: 1.0 });
    let feet = vec![[0.0; 3]; lim::FEET];
    let bodies = vec![[0.0, 0.0, 0.0, 1.0]; lim::BODIES];
    TRACK.with(|t| t.set(true));
    for k in 0..240 {
        p.prop_input[2 * pi::STRIDE + pi::ADOPTED] = if k < 60 { pa::NONE } else { pa::FOLLOWING } as f64;
        p.trap_input[ti::PHASE] = (k / 60 % 4) as f64;
        frame(&mut p, 1.0 / 60.0, k as f64, &feet, &bodies);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
}
