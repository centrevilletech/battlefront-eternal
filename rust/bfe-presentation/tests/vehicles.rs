//! The page's hull frame: a ground hull, a walker asking for its leg rays,
//! a dying walker, records at and past their limits, forged offsets, and no
//! allocation once its rigs are registered; a craft's gear clock following
//! its replicated flight state.
use bfe_presentation::{
    layout::{
        self, cast_ray as cr, vehicle_aimer_output as ao, vehicle_entry as ve, vehicle_entry_output as eo,
        vehicle_flight_state as fs, vehicle_frame as vf, vehicle_header as vh, vehicle_mode as mode, vehicle_node as vn, vehicle_root as vr,
        vehicle_seat_input as si,
    },
    vehicles::Vehicles,
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

// A two-bone clip: bone 1 turns about z, a rising to pi/4 over 31 frames.
fn clip(v: &mut Vehicles) -> u32 {
    let frames = 31;
    let mut data = Vec::new();
    for bone in 0..2 {
        for f in 0..frames {
            let a = if bone == 1 { f as f64 / 30.0 * std::f64::consts::FRAC_PI_4 } else { 0.0 };
            data.extend_from_slice(&[0.0, 0.0, a.sin() as f32, a.cos() as f32, 0.0, bone as f32, 0.0]);
        }
    }
    v.load_clip(frames, 30.0, false, vec![0, 1], data)
}
// A hull of three bones: a body, a turret on it (an aimer's bone, seat 1's
// turret) and a barrel on the turret.
fn rig(walker: bool, c: u32) -> serde_json::Value {
    let bone = |parent: i32, y: f64| {
        serde_json::json!({ "parent": parent, "position": [0.0, y, 0.0], "quaternion": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0], "bind": [0.0, 0.0, 0.0, 1.0, 0.0, y, 0.0] })
    };
    let clip = serde_json::json!({ "handle": c, "duration": 1.0 });
    let aimer = serde_json::json!({ "bone": 1, "owner": -1, "yawRate": 2.0, "pitchRate": 1.0, "yawLimits": [-1.0, 1.0], "pitchLimits": [-0.5, 0.5], "wrapYaw": false, "yaw": 0.0, "pitch": 0.0 });
    serde_json::json!({
        "bones": [bone(-1, 0.0), bone(0, 1.0), bone(1, 0.5)],
        "yxz": true, "hasPose": walker, "poseRoot": -1, "flying": false, "creature": false,
        "idle": if walker { clip.clone() } else { serde_json::Value::Null },
        "left": if walker { clip.clone() } else { serde_json::Value::Null },
        "right": if walker { clip.clone() } else { serde_json::Value::Null },
        "liftLeft": if walker { clip.clone() } else { serde_json::Value::Null },
        "liftRight": if walker { clip.clone() } else { serde_json::Value::Null },
        "death": if walker { clip.clone() } else { serde_json::Value::Null },
        "takeoff": null, "gearRoot": -1,
        "legs": if walker { serde_json::json!({ "rayLength": 4.0, "at": [[[1.0, 0.0, 1.0], [-1.0, 0.0, 1.0]]], "bones": [[1, 2]] }) } else { serde_json::Value::Null },
        "seats": [
            { "turret": -1, "up": [0.0, 1.0, 0.0], "yawFactor": 1.0, "pitchFactor": 1.0, "weapons": [[aimer]], "muzzles": [{ "barrel": 2, "rest": [0.0, 0.5, 0.0], "recoil": 0.3 }] },
            { "turret": 1, "up": [0.0, 1.0, 0.0], "yawFactor": 1.0, "pitchFactor": 1.0, "weapons": [], "muzzles": [] }
        ],
        "wheels": [[2, [1.0, 0.0, 0.0]]],
        // A marker on the barrel and a skinned mesh on the root.
        "nodes": [{ "parent": 3, "position": [0.0, 0.0, 2.0], "quaternion": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0], "skinned": false },
                  { "parent": 0, "position": [0.0, 0.0, 0.0], "quaternion": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0], "skinned": true }],
        "deathSpan": if walker { 2.0 } else { 0.0 }
    })
}
fn add(v: &mut Vehicles, walker: bool, c: u32) -> usize {
    v.add_rig(serde_json::from_value(rig(walker, c)).unwrap())
}
// An entry for `rig`'s slot 0, alive at the tick (alpha 1), dt seconds on.
fn entry(v: &mut Vehicles, e: usize, rig: usize, m: u32, dt: f64) -> usize {
    let (input, output) = (v.slot_input(rig, 0), v.slot_output(rig, 0));
    let at = vf::STRIDE + e * ve::STRIDE;
    let x = &mut v.input[at..at + ve::STRIDE];
    x.fill(0.0);
    x[ve::RIG] = rig as f64;
    x[ve::MODE] = m as f64;
    x[ve::ALPHA] = 1.0;
    x[ve::DT] = dt;
    x[ve::FRAME_DT] = dt;
    x[ve::POSITION..ve::POSITION + 3].copy_from_slice(&[5.0, 1.0, -2.0]);
    x[ve::QUATERNION + 3] = 1.0;
    x[ve::TICK_QUATERNION + 3] = 1.0;
    x[ve::TICK_FROM..ve::TICK_FROM + 3].copy_from_slice(&[5.0, 1.0, -2.0]);
    x[ve::YAW] = 0.5;
    x[ve::TICK_YAW] = 0.5;
    x[ve::WALK_FROM_PHASE] = f64::NAN;
    x[ve::GEAR] = f64::NAN;
    x[ve::GEAR_FOLLOW] = -1.0;
    x[ve::BODY] = 7.0;
    x[ve::DYING] = 1.0;
    x[ve::INPUT] = input as f64;
    x[ve::OUTPUT] = output as f64;
    output
}
fn frame(v: &mut Vehicles, entries: usize) {
    v.input[vf::VERSION] = layout::VERSION as f64;
    v.input[vf::ENTRIES] = entries as f64;
    v.place();
    v.pose();
}

#[test]
fn a_ground_hull_is_placed_and_its_held_seat_turned() {
    let mut v = Vehicles::new();
    let c = clip(&mut v);
    let r = add(&mut v, false, c);
    let o = entry(&mut v, 0, r, mode::ALIVE, 1.0 / 60.0);
    // Seat 0 held, aiming at a point to the hull's left.
    let seats = v.slot_input(r, 0); // no legs: the seats come first
    v.stream_in[seats + si::HELD] = 1.0;
    v.stream_in[seats + si::POSE] = 1.0;
    v.stream_in[seats + si::TARGET] = 1.0;
    v.stream_in[seats + si::TARGET_POINT..seats + si::TARGET_POINT + 3].copy_from_slice(&[50.0, 2.0, -2.0]);
    frame(&mut v, 1);
    assert_eq!(v.header[vh::STATUS], 0.0);
    assert_eq!(v.stream_out[o + eo::ROOT_MODE], vr::EULER as f64);
    assert_eq!(&v.stream_out[o + eo::POSITION..o + eo::POSITION + 3], &[5.0, 1.0, -2.0]);
    assert_eq!(v.stream_out[o + eo::ROTATION + 1], 0.5, "the heading between equal ticks");
    // Its aimer turned toward the target by its rate over the frame.
    let a = o + eo::STRIDE;
    assert_eq!(v.stream_out[a + ao::WRITTEN], 1.0);
    assert_eq!(v.stream_out[a + ao::POSED], 1.0);
    // Turned 2/60 rad off the hull's heading (0.5) about its up.
    let (fx, fz) = (v.stream_out[a + ao::FACING], v.stream_out[a + ao::FACING + 2]);
    assert!(((fx.atan2(fz) - 0.5).abs() - 2.0 / 60.0).abs() < 1e-9, "{}", fx.atan2(fz));
    // The nodes: the root's world matrix at its place, the marker 2 m along
    // the barrel, the mesh's bind inverse undoing the root's.
    let n = o + eo::STRIDE + ao::STRIDE;
    assert_eq!(&v.stream_out[n + vn::WORLD + 12..n + vn::WORLD + 15], &[5.0, 1.0, -2.0]);
    let marker = n + 4 * vn::STRIDE;
    assert!((v.stream_out[marker + vn::WORLD + 13] - 2.5).abs() < 1e-12);
    let skin = n + 6 * vn::STRIDE;
    let w = &v.stream_out[n + vn::WORLD..n + vn::WORLD + 16];
    assert!((v.stream_out[skin + 12] + (w[0] * w[12] + w[1] * w[13] + w[2] * w[14])).abs() < 1e-9);
}

#[test]
fn a_walker_asks_for_its_leg_rays_and_a_dying_one_plays_its_death() {
    let mut v = Vehicles::new();
    let c = clip(&mut v);
    let r = add(&mut v, true, c);
    let o = entry(&mut v, 0, r, mode::ALIVE, 1.0 / 60.0);
    v.input[vf::VERSION] = layout::VERSION as f64;
    v.input[vf::ENTRIES] = 1.0;
    v.place();
    assert_eq!(v.header[vh::RAYS], 2.0);
    assert_eq!(v.stream_out[o + eo::RAYS], 2.0);
    // Each foot's ray from L (4 m) above it, 2L down, past the hull's body.
    let ray = &v.rays[..cr::STRIDE];
    assert_eq!(&ray[cr::DIRECTION..cr::DIRECTION + 3], &[0.0, -1.0, 0.0]);
    assert_eq!((ray[cr::FAR], ray[cr::HYPOT], ray[cr::IGNORE]), (8.0, 0.0, 7.0));
    assert!((ray[cr::ORIGIN + 1] - 5.0).abs() < 1e-12);
    // The host's casts: the left foot's ground 1 m higher.
    for (k, d) in [3.0, 4.0].into_iter().enumerate() {
        v.rays[k * cr::STRIDE + cr::HIT] = 1.0;
        v.rays[k * cr::STRIDE + cr::DISTANCE] = d;
    }
    v.pose();
    let i = v.slot_input(r, 0);
    assert_eq!((v.stream_in[i + 1], v.stream_in[i + 2]), (1.0, 0.0), "L - distance");
    assert_eq!(v.stream_out[o + eo::LEG_SIDES], 1.0);
    assert!(v.stream_out[o + eo::STRIDE] < 0.0, "the left foot lifted");
    entry(&mut v, 0, r, mode::DYING, 0.0);
    frame(&mut v, 1);
    assert_eq!(v.stream_out[o + eo::ROOT_MODE], vr::EULER as f64);
}

#[test]
fn records_past_their_limits_and_forged_offsets_pose_nothing() {
    let mut v = Vehicles::new();
    let c = clip(&mut v);
    let a = add(&mut v, true, c);
    let b = add(&mut v, false, c);
    // More entries than the records hold: the count clamps.
    for e in 0..v.entries_capacity() {
        entry(&mut v, e, if e % 2 == 0 { a } else { b }, mode::ALIVE, 1.0 / 60.0);
    }
    v.input[vf::VERSION] = layout::VERSION as f64;
    v.input[vf::ENTRIES] = 1e9;
    v.place();
    v.pose();
    v.input[vf::ENTRIES] = f64::NAN;
    v.place();
    v.pose();
    // A rig past the registered ones, a negative one, and offsets that are
    // not a slot's: skipped and counted.
    entry(&mut v, 0, a, mode::ALIVE, 0.0);
    v.input[vf::STRIDE + ve::RIG] = 99.0;
    entry(&mut v, 1, b, mode::ALIVE, 0.0);
    v.input[vf::STRIDE + ve::STRIDE + ve::OUTPUT] += 3.0;
    entry(&mut v, 2, b, mode::ALIVE, 0.0);
    v.input[vf::STRIDE + 2 * ve::STRIDE + ve::RIG] = -1.0;
    frame(&mut v, 3);
    assert_eq!(v.header[vh::SKIPPED], 3.0);
    // NaN state and an unknown mode: no panic.
    let o = entry(&mut v, 0, a, mode::ALIVE, f64::NAN);
    v.input[vf::STRIDE + ve::ALPHA] = f64::NAN;
    v.input[vf::STRIDE + ve::WALK_PHASE] = f64::INFINITY;
    entry(&mut v, 1, b, 77, 0.0);
    frame(&mut v, 2);
    let _ = o;
    v.input[vf::VERSION] = 0.0;
    v.place();
    v.pose();
    assert_eq!(v.header[vh::STATUS], layout::status::VERSION_MISMATCH as f64);
}

#[test]
fn frames_do_not_allocate() {
    let mut v = Vehicles::new();
    let c = clip(&mut v);
    let a = add(&mut v, true, c);
    let b = add(&mut v, false, c);
    TRACK.with(|t| t.set(true));
    for k in 0..240 {
        let m = if k % 60 < 50 { mode::ALIVE } else { mode::DYING };
        entry(&mut v, 0, a, m, 1.0 / 60.0);
        entry(&mut v, 1, b, mode::ALIVE, 1.0 / 60.0);
        let s = v.slot_input(b, 0);
        v.stream_in[s + si::HELD] = 1.0;
        v.stream_in[s + si::POSE] = 1.0;
        v.input[vf::STRIDE + ve::DYING] = 2.0 - (k % 60) as f64 / 30.0;
        frame(&mut v, 2);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
}

// A craft with the two-bone clip as its takeoff clip, landing over 2 s and
// stowing over 1 s once 3 m (2 + 1 landed) up.
fn craft(v: &mut Vehicles, c: u32) -> usize {
    let mut d = rig(false, c);
    d["flying"] = serde_json::json!(true);
    d["takeoff"] = serde_json::json!({ "handle": c, "duration": 1.0 });
    d["flight"] = serde_json::json!({ "takeoffTime": 1.0, "landingTime": 2.0, "takeoffHeight": 2.0, "landedHeight": 1.0 });
    v.add_rig(serde_json::from_value(d).unwrap())
}
// One frame of a craft's gear: its own gear, whose records it follows (and
// their life), its flight state and the ground height under it.
fn gear(v: &mut Vehicles, r: usize, dt: f64, own: f64, follow: f64, generation: f64, state: u32, height: f64) -> f64 {
    let o = entry(v, 0, r, mode::ALIVE, dt);
    let x = &mut v.input[vf::STRIDE..vf::STRIDE + ve::STRIDE];
    x[ve::GEAR] = own;
    x[ve::GEAR_FOLLOW] = follow;
    x[ve::GEAR_GENERATION] = generation;
    x[ve::FLIGHT_STATE] = state as f64;
    x[ve::GROUND_FROM..ve::GROUND_FROM + 3].copy_from_slice(&[0.0, 9.0, 0.0]);
    v.input[vf::VERSION] = layout::VERSION as f64;
    v.input[vf::ENTRIES] = 1.0;
    v.place();
    // The ground under a lifting craft is cast only while its gear follows
    // its records and the frame has time: from where it is, 1024 m down.
    let asked = v.header[vh::RAYS];
    if asked > 0.0 {
        assert_eq!(asked, 1.0);
        assert_eq!((v.rays[cr::ORIGIN + 1], v.rays[cr::FAR], v.rays[cr::IGNORE]), (9.0, 1024.0, 7.0));
        v.rays[cr::HIT] = (!height.is_nan()) as u8 as f64;
        v.rays[cr::DISTANCE] = height;
    } else {
        assert!(height.is_nan() || !(dt > 0.0) || state != fs::TAKING_OFF || follow <= 0.0, "a lifting craft asks for its ground");
    }
    v.pose();
    v.stream_out[o + eo::GEAR]
}

#[test]
fn a_craft_drawn_from_its_records_follows_its_flight_state() {
    let mut v = Vehicles::new();
    let c = clip(&mut v);
    let r = craft(&mut v, c);
    let dt = 1.0 / 60.0;
    // Without records this life: its own gear (none: down).
    assert_eq!(gear(&mut v, r, dt, 0.4, -1.0, 1.0, fs::FLYING, f64::NAN), 0.4);
    assert_eq!(gear(&mut v, r, dt, f64::NAN, -1.0, 1.0, fs::FLYING, f64::NAN), 1.0);
    // First seen in flight: stowed at once; a landing brings it down over
    // LandingTime, and it stays down landed.
    assert_eq!(gear(&mut v, r, dt, 1.0, 1.0, 1.0, fs::FLYING, f64::NAN), 0.0);
    // Stowed: the takeoff clip's last frame (bone 1 a quarter turn about z),
    // as the gear's bones are drawn.
    let bone1 = v.slot_output(r, 0) + eo::STRIDE + ao::STRIDE + 2 * vn::STRIDE;
    assert!((v.stream_out[bone1 + vn::MATRIX + 1] - 1.0).abs() < 1e-6);
    let mut g = 0.0;
    for _ in 0..60 {
        g = gear(&mut v, r, dt, 1.0, 1.0, 1.0, fs::LANDING, f64::NAN);
    }
    assert!((g - 0.5).abs() < 1e-9, "half way down half way through the landing ({g})");
    for _ in 0..90 {
        g = gear(&mut v, r, dt, 1.0, 1.0, 1.0, fs::LANDING, f64::NAN);
    }
    assert_eq!(g, 1.0);
    assert!(v.stream_out[bone1 + vn::MATRIX + 1].abs() < 1e-6, "down: the clip's first frame");
    // The lift: held down under TakeoffHeight, stowing once above it.
    for _ in 0..30 {
        g = gear(&mut v, r, dt, 1.0, 1.0, 1.0, fs::TAKING_OFF, 0.5);
    }
    assert_eq!(g, 1.0, "held down while the craft is under TakeoffHeight");
    for _ in 0..30 {
        g = gear(&mut v, r, dt, 1.0, 1.0, 1.0, fs::TAKING_OFF, 3.5);
    }
    assert!((g - (1.0 - 30.0 * dt)).abs() < 1e-9, "stows at 1/TakeoffTime once up ({g})");
    assert_eq!(gear(&mut v, r, 0.0, 1.0, 1.0, 1.0, fs::TAKING_OFF, f64::NAN), g, "no time, no height read, no change");
    // No ground within reach reads as 1024 m up.
    assert!(gear(&mut v, r, dt, 1.0, 1.0, 1.0, fs::TAKING_OFF, f64::NAN) < g);
    // A new life starts from its state; his own craft draws its replayed
    // gear; given back to its records it starts where his replay left it.
    assert_eq!(gear(&mut v, r, dt, 1.0, 1.0, 2.0, fs::LANDED, f64::NAN), 1.0);
    assert_eq!(gear(&mut v, r, dt, 0.3, 0.0, 2.0, fs::FLYING, f64::NAN), 0.3);
    assert_eq!(gear(&mut v, r, 0.0, 0.3, 1.0, 2.0, fs::FLYING, f64::NAN), 0.3);
    assert!(gear(&mut v, r, dt, 0.3, 1.0, 2.0, fs::FLYING, f64::NAN) < 0.3);
    // Unknown states hold; NaN clocks never panic.
    let held = gear(&mut v, r, dt, 0.3, 1.0, 2.0, fs::OTHER, f64::NAN);
    assert_eq!(gear(&mut v, r, dt, 0.3, 1.0, 2.0, fs::OTHER, f64::NAN), held);
    let _ = gear(&mut v, r, f64::NAN, f64::NAN, 1.0, f64::NAN, 99, f64::NAN);
}
