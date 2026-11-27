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

// A droideka's chase camera trails its place behind a ball rolling at 13 m/s
// and, when the ball stops dead (it unfolds), glides to rest over a few
// frames instead of stopping with it; sideways it never trails.
#[test]
fn a_droideka_camera_glides_to_rest_when_its_ball_stops_dead() {
    use bfe_presentation::cameras::{motion::Motion, soldier, Camera};
    let (def, motion) = (soldier::Def::default(), Motion::default());
    let mut world = World { casts: 0, random: 0, wall: false };
    let mut cam = Camera { p: Vector3::new(0., 0., 0.), q: three::Quaternion::default(), up: Vector3::new(0., 1., 0.), fov: 65., aspect: 1.5, near: 0.3, far: 1000. };
    let mut camera = soldier::Soldier::new();
    let input = |z: f64| soldier::Input {
        feet: Vector3::new(0., 0., z), yaw: 0., pitch: 0., posture: 0, zoom: 1., first: false, responsive: true, eye: None, crouch: 0., follow: 27.,
    };
    let dt = 1. / 60.;
    let mut z = 0.;
    for _ in 0..120 {
        z += 13. * dt;
        camera.frame::<V8, _>(dt, &input(z), &def, &motion, &mut cam, &mut world);
    }
    let rest = |cam: &Camera| cam.p.z - z;
    let mut gaps = vec![rest(&cam)];
    for _ in 0..12 {
        camera.frame::<V8, _>(dt, &input(z), &def, &motion, &mut cam, &mut world);
        gaps.push(rest(&cam));
        assert!(cam.p.x.abs() < 1e-9, "sideways {}", cam.p.x);
    }
    for _ in 0..60 {
        camera.frame::<V8, _>(dt, &input(z), &def, &motion, &mut cam, &mut world);
    }
    let settled = rest(&cam);
    // Over a third of a metre further back while rolling; then every frame closes part of the gap, less each time.
    assert!(gaps[0] < settled - 0.3, "it trails while rolling: {gaps:?}, settled {settled}");
    let steps: Vec<f64> = gaps.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(steps.iter().all(|step| *step > 0.0) && steps.windows(2).all(|pair| pair[1] < pair[0]), "{steps:?}");
    assert!((gaps[12] - settled).abs() < 0.01, "at rest behind the body within 12 frames: {gaps:?}, settled {settled}");
}

struct BoxWorld {
    boxes: Vec<([f64; 3], [f64; 3])>,
}
impl BoxWorld {
    fn segment_hits_before(&self, a: Vector3, b: Vector3, eps: f64) -> bool {
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let dz = b.z - a.z;
        let len = (dx * dx + dy * dy + dz * dz).sqrt();
        if len <= eps {
            return false;
        }
        let d = Vector3::new(dx / len, dy / len, dz / len);
        self.cast_const(a, d, len - eps).is_some()
    }
    fn cast_const(&self, o: Vector3, d: Vector3, far: f64) -> Option<f64> {
        let mut best: Option<f64> = None;
        for (lo, hi) in &self.boxes {
            let mut tmin = 0.0_f64;
            let mut tmax = far;
            let orig = [o.x, o.y, o.z];
            let dir = [d.x, d.y, d.z];
            let mut hit = true;
            for axis in 0..3 {
                if dir[axis].abs() < 1e-12 {
                    if orig[axis] < lo[axis] || orig[axis] > hi[axis] {
                        hit = false;
                        break;
                    }
                } else {
                    let inv = 1.0 / dir[axis];
                    let mut t1 = (lo[axis] - orig[axis]) * inv;
                    let mut t2 = (hi[axis] - orig[axis]) * inv;
                    if t1 > t2 {
                        std::mem::swap(&mut t1, &mut t2);
                    }
                    tmin = tmin.max(t1);
                    tmax = tmax.min(t2);
                    if tmin > tmax {
                        hit = false;
                        break;
                    }
                }
            }
            if hit && tmax >= 1e-6 && tmin <= far {
                let t = if tmin > 1e-6 { tmin } else { tmax };
                if t <= far && best.is_none_or(|b| t < b) {
                    best = Some(t);
                }
            }
        }
        best
    }
}
impl Cast for BoxWorld {
    fn cast(&mut self, p: Vector3, d: Vector3, far: f64, _static_only: bool) -> Option<f64> {
        self.cast_const(p, d, far)
    }
}

#[test]
fn swbf2_soldier_camera_round_robin_rays_and_frame_filter() {
    use bfe_presentation::cameras::{
        math::{rotate, DEG},
        motion::Motion,
        soldier, Camera,
    };

    struct ScriptedCast {
        calls: Vec<(Vector3, Vector3, f64)>,
        reply: Option<f64>,
    }
    impl Cast for ScriptedCast {
        fn cast(&mut self, origin: Vector3, direction: Vector3, far: f64, _static_only: bool) -> Option<f64> {
            self.calls.push((origin, direction, far));
            self.reply
        }
    }

    let mut def = soldier::Def::default();
    def.sections[0] = soldier::Section {
        eye: [0.0, 2.0, 0.0],
        center: [0.0, 1.55, 0.0],
        offset: [0.0, 0.0, 4.6],
        tilt: 8.0 * DEG,
    };
    let motion = Motion::default();
    let mut cam = Camera {
        p: Vector3::new(0.0, 0.0, 0.0),
        q: three::Quaternion::default(),
        up: Vector3::new(0.0, 1.0, 0.0),
        fov: 65.0,
        aspect: 16.0 / 9.0,
        near: 0.3,
        far: 1000.0,
    };
    let mut camera = soldier::Soldier::new();
    camera.near_clearance = true;
    let input = soldier::Input {
        feet: Vector3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
        posture: 0,
        zoom: 1.0,
        first: false,
        responsive: true,
        eye: None,
        crouch: 0.0,
        follow: 0.0,
    };
    let dt = 1.0 / 60.0;
    let pivot = Vector3::new(0.0, 1.55, 0.0);
    let boom_len = 4.6;
    let seg = boom_len - cam.near;
    let mut world = ScriptedCast {
        calls: Vec::new(),
        reply: None,
    };

    // Five frames cast exactly one parallel ray per frame in round-robin order
    // over (0, 0), (+0.4, +0.4), (-0.4, +0.4), (+0.4, -0.4), (-0.4, -0.4),
    // each of length L - near.
    let expected_offsets = [
        (0.0, 0.0),
        (0.4, 0.4),
        (-0.4, 0.4),
        (0.4, -0.4),
        (-0.4, -0.4),
    ];
    for (frame, (ex, ey)) in expected_offsets.iter().enumerate() {
        world.calls.clear();
        camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
        assert_eq!(world.calls.len(), 1, "frame {frame}: expected 1 ray per frame");
        let (orig, _dir, far) = world.calls[0];
        assert!((far - seg).abs() < 1e-9, "frame {frame}: cast length {far} != {seg}");
        let right = rotate(Vector3::new(1.0, 0.0, 0.0), cam.q);
        let up = rotate(Vector3::new(0.0, 1.0, 0.0), cam.q);
        let dx = (orig.x - pivot.x) * right.x + (orig.y - pivot.y) * right.y + (orig.z - pivot.z) * right.z;
        let dy = (orig.x - pivot.x) * up.x + (orig.y - pivot.y) * up.y + (orig.z - pivot.z) * up.z;
        assert!(
            (dx - ex).abs() < 1e-9 && (dy - ey).abs() < 1e-9,
            "frame {frame}: offset ({dx:.3}, {dy:.3}) != ({ex}, {ey})"
        );
    }

    // Instant pull-in: a hit at fraction 0.20 pulls s straight to 0.20 and
    // places the eye at near + 0.20 * (L - near) from TrackCenter.
    world.reply = Some(0.20 * seg);
    camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
    let d_pull = cam.p.distance_to(&pivot);
    assert!(
        (d_pull - (cam.near + 0.20 * seg)).abs() < 1e-9,
        "instant pull-in distance {d_pull} != {}",
        cam.near + 0.20 * seg
    );

    // Zero-distance hit (wall touching TrackCenter) keeps the eye at cam.near.
    world.reply = Some(0.0);
    camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
    let d_min = cam.p.distance_to(&pivot);
    assert!(
        (d_min - cam.near).abs() < 1e-9,
        "minimum eye distance from TrackCenter {d_min} != near {}",
        cam.near
    );

    // Fill all 5 slots with 0.30 so min_frac rises to 0.30 (> s=0.0 + 0.15):
    // must ease out at 0.75/s (not snap), then hold once within 0.15 of s.
    world.reply = Some(0.30 * seg);
    // First 4 frames still have a 0.0 slot in the ring, so min_frac stays 0.0.
    for _ in 0..4 {
        camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
        assert!((cam.p.distance_to(&pivot) - cam.near).abs() < 1e-9);
    }
    // 5th frame replaces the last 0.0 slot -> min_frac = 0.30 > 0.0 + 0.15 -> eases by 0.75 * dt.
    camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
    let s1 = (cam.p.distance_to(&pivot) - cam.near) / seg;
    assert!(
        (s1 - 0.75 * dt).abs() < 1e-9,
        "ease-out step {s1} != {}",
        0.75 * dt
    );

    // Advance until s reaches >= 0.15 (within 0.15 of min_frac=0.30): hold must freeze s.
    let mut prev_s = s1;
    for _ in 0..30 {
        camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
        prev_s = (cam.p.distance_to(&pivot) - cam.near) / seg;
    }
    assert!(
        prev_s >= 0.15 - 1e-9 && prev_s <= 0.15 + 0.75 * dt + 1e-9,
        "0.15 hold band should stop ease-out near 0.15, got s={prev_s}"
    );
    let held_s = prev_s;
    for _ in 0..10 {
        camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
        let s = (cam.p.distance_to(&pivot) - cam.near) / seg;
        assert!(
            (s - held_s).abs() < 1e-12,
            "hold band must keep s={held_s}, moved to {s}"
        );
    }

    // Clear all 5 slots to 1.0 (unobstructed): eases out at 0.75/s all the way
    // to 1.0 even once 1.0 - s <= 0.15.
    world.reply = None;
    for _ in 0..4 {
        camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
    }
    camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
    let s_clear_step = (cam.p.distance_to(&pivot) - cam.near) / seg;
    assert!(
        (s_clear_step - (held_s + 0.75 * dt)).abs() < 1e-9,
        "unobstructed recovery should resume at 0.75/s: {s_clear_step}"
    );
    for _ in 0..100 {
        camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
    }
    assert!(
        (cam.p.distance_to(&pivot) - boom_len).abs() < 1e-9,
        "unobstructed camera must ease all the way back to full boom"
    );

    // Fresh reset discards stale obstructed slots and restores full reach.
    world.reply = Some(0.10 * seg);
    camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
    assert!(cam.p.distance_to(&pivot) < 1.0);
    camera.reset(&def);
    world.reply = None;
    camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut world);
    assert!((cam.p.distance_to(&pivot) - boom_len).abs() < 1e-9);
}

#[test]
fn swbf2_soldier_camera_keeps_near_plane_in_front_of_hit_and_body_visible() {
    use bfe_presentation::cameras::{
        math::{add, plus, rotate, DEG},
        motion::Motion,
        soldier, Camera,
    };
    let mut def = soldier::Def::default();
    def.sections[0] = soldier::Section {
        eye: [0.0, 2.0, 0.0],
        center: [0.0, 1.55, 0.0],
        offset: [0.0, 0.0, 4.6],
        tilt: 8.0 * DEG,
    };
    let motion = Motion::default();
    let assert_game_properties = |world: &BoxWorld, feet: Vector3, cam: &Camera, check_near_center: bool, label: &str| {
        let pivot = add(feet, Vector3::new(0.0, 1.55, 0.0));
        let torso = add(feet, Vector3::new(0.0, 1.4, 0.0));
        let dpiv = cam.p.distance_to(&pivot);
        assert!(
            dpiv >= cam.near - 1e-9,
            "{label}: eye closer than near to TrackCenter: dpiv={dpiv:.4} < near={:.3}",
            cam.near
        );
        let fwd = rotate(Vector3::new(0.0, 0.0, -1.0), cam.q);
        let depth = (torso.x - cam.p.x) * fwd.x
            + (torso.y - cam.p.y) * fwd.y
            + (torso.z - cam.p.z) * fwd.z;
        assert!(
            depth >= cam.near - 1e-6,
            "{label}: standing body culled by near plane: depth={depth:.4} < near={:.3}",
            cam.near
        );
        if check_near_center {
            let near_center = plus(cam.p, fwd, cam.near);
            assert!(
                !world.segment_hits_before(pivot, near_center, 1e-6),
                "{label}: near-plane center lies behind wall hit (pivot={:.3},{:.3},{:.3}, near_center={:.3},{:.3},{:.3})",
                pivot.x, pivot.y, pivot.z, near_center.x, near_center.y, near_center.z
            );
        }
    };

    // Case 1: strafe-jump into a street wall at x <= 0 (with a protruding street pillar),
    // land beside it at x=0.36, sweep grazing angles 5..90 deg and back against wall.
    let mut street = BoxWorld {
        boxes: vec![
            ([-2.0, 0.0, -12.0], [0.0, 6.0, 12.0]),
            ([-0.2, 0.0, -3.2], [0.15, 6.0, -2.4]),
        ],
    };
    let mut cam = Camera {
        p: Vector3::new(0.0, 0.0, 0.0),
        q: three::Quaternion::default(),
        up: Vector3::new(0.0, 1.0, 0.0),
        fov: 65.0,
        aspect: 16.0 / 9.0,
        near: 0.3,
        far: 1000.0,
    };
    let mut camera = soldier::Soldier::new();
    camera.near_clearance = true;
    let dt = 1.0 / 60.0;
    for step in 0..210 {
        let t = step as f64 / 60.0;
        let x = if step < 45 {
            1.4 - (step as f64 / 45.0) * 1.04
        } else {
            0.36
        };
        let y = if step < 60 {
            let u = step as f64 / 60.0;
            4.0 * u * (1.0 - u) * 3.0
        } else {
            0.0
        };
        let yaw_deg = if step < 90 {
            5.0 + (step as f64 / 90.0) * 15.0
        } else {
            20.0 + ((step - 90) as f64 / 120.0) * 70.0
        };
        let z = if step < 90 { -t * 1.2 } else { -1.8 };
        let feet = Vector3::new(x, y, z);
        let input = soldier::Input {
            feet,
            yaw: yaw_deg * DEG,
            pitch: 0.0,
            posture: 0,
            zoom: 1.0,
            first: false,
            responsive: true,
            eye: None,
            crouch: 0.0,
            follow: 0.0,
        };
        TRACK.with(|f| f.set(true));
        let before = ALLOCS.with(Cell::get);
        camera.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut street);
        let after = ALLOCS.with(Cell::get);
        TRACK.with(|f| f.set(false));
        assert_eq!(after, before, "2005 chase camera frame must not allocate");
        // On every center-ray frame (step % 5 == 0), the center ray has just
        // sampled the current view, so near_center is strictly in front of the
        // first wall hit along the boom.
        assert_game_properties(
            &street,
            feet,
            &cam,
            step % 5 == 0,
            &format!("street step={step} yaw={yaw_deg:.1}"),
        );
    }

    // 2004 boundary: default near_clearance=false keeps the original 5-ray fanned
    // 0.08 m skin solver unchanged.
    let mut wall_only = BoxWorld {
        boxes: vec![([-2.0, 0.0, -12.0], [0.0, 6.0, 12.0])],
    };
    let mut cam_2004 = Camera {
        p: Vector3::new(0.0, 0.0, 0.0),
        q: three::Quaternion::default(),
        up: Vector3::new(0.0, 1.0, 0.0),
        fov: 65.0,
        aspect: 16.0 / 9.0,
        near: 0.3,
        far: 1000.0,
    };
    let mut soldier_2004 = soldier::Soldier::new();
    let back_input = soldier::Input {
        feet: Vector3::new(0.42, 0.0, 0.0),
        yaw: 90.0 * DEG,
        pitch: 0.0,
        posture: 0,
        zoom: 1.0,
        first: false,
        responsive: true,
        eye: None,
        crouch: 0.0,
        follow: 0.0,
    };
    soldier_2004.frame::<V8, _>(dt, &back_input, &def, &motion, &mut cam_2004, &mut wall_only);
    assert!(
        cam_2004.p.x < 0.10,
        "2004 default keeps original 0.08 m skin, got x={:.3}",
        cam_2004.p.x
    );

    // Case 2: narrow doorway and corner with 360-degree pan.
    let mut doorway = BoxWorld {
        boxes: vec![
            ([-4.0, 0.0, -0.5], [-0.65, 4.0, 0.5]),
            ([0.65, 0.0, -0.5], [4.0, 4.0, 0.5]),
            ([-4.0, 2.10, -0.5], [4.0, 4.0, 0.5]),
        ],
    };
    let mut door_cam = soldier::Soldier::new();
    door_cam.near_clearance = true;
    for step in 0..120 {
        let z = if step < 40 {
            -1.5 + (step as f64 / 40.0) * 1.5
        } else {
            0.0
        };
        let yaw_deg = if step < 40 {
            0.0
        } else {
            ((step - 40) as f64 / 80.0) * 360.0
        };
        let feet = Vector3::new(0.0, 0.0, z);
        let input = soldier::Input {
            feet,
            yaw: yaw_deg * DEG,
            pitch: 0.0,
            posture: 0,
            zoom: 1.0,
            first: false,
            responsive: true,
            eye: None,
            crouch: 0.0,
            follow: 0.0,
        };
        door_cam.frame::<V8, _>(dt, &input, &def, &motion, &mut cam, &mut doorway);
        assert_game_properties(
            &doorway,
            feet,
            &cam,
            step % 5 == 0,
            &format!("doorway step={step} z={z:.2} yaw={yaw_deg:.1}"),
        );
    }

    // Case 3: soldier standing 0.50 m from a street wall facing 45 deg away
    // (back to angled wall). Even across a full 5-frame round-robin sweep,
    // adding cam.near back keeps the eye >= near from TrackCenter and the
    // standing body visible (never culled).
    let mut angled_cam = soldier::Soldier::new();
    angled_cam.near_clearance = true;
    let angled_input = soldier::Input {
        feet: Vector3::new(0.50, 0.0, 0.0),
        yaw: 45.0 * DEG,
        pitch: 0.0,
        posture: 0,
        zoom: 1.0,
        first: false,
        responsive: true,
        eye: None,
        crouch: 0.0,
        follow: 0.0,
    };
    for step in 0..10 {
        angled_cam.frame::<V8, _>(dt, &angled_input, &def, &motion, &mut cam, &mut wall_only);
        assert_game_properties(
            &wall_only,
            angled_input.feet,
            &cam,
            true,
            &format!("angled-wall step={step}"),
        );
    }
}

#[test]
fn swbf2_soldier_camera_boom_shorter_than_near_casts_back_past_track_center() {
    use bfe_presentation::cameras::{motion::Motion, soldier, Camera};

    struct OneHit {
        calls: Vec<(Vector3, f64)>,
        reply: Option<f64>,
    }
    impl Cast for OneHit {
        fn cast(&mut self, _origin: Vector3, direction: Vector3, far: f64, _static_only: bool) -> Option<f64> {
            self.calls.push((direction, far));
            self.reply
        }
    }
    let mut def = soldier::Def::default();
    def.sections[0] = soldier::Section { eye: [0.0, 1.1, 0.0], center: [0.0, 0.9, 0.0], offset: [0.0, 0.0, 0.25], tilt: 0.0 };
    let mut cam = Camera {
        p: Vector3::new(0.0, 0.0, 0.0),
        q: three::Quaternion::default(),
        up: Vector3::new(0.0, 1.0, 0.0),
        fov: 60.0,
        aspect: 1.5,
        near: 0.4,
        far: 500.0,
    };
    let input = soldier::Input {
        feet: Vector3::new(2.0, 0.0, -3.0),
        yaw: 0.0,
        pitch: 0.0,
        posture: 0,
        zoom: 1.0,
        first: false,
        responsive: true,
        eye: None,
        crouch: 0.0,
        follow: 0.0,
    };
    let mut camera = soldier::Soldier::new();
    camera.near_clearance = true;
    let mut world = OneHit { calls: Vec::new(), reply: Some(0.06) };
    camera.frame::<V8, _>(1.0 / 60.0, &input, &def, &Motion::default(), &mut cam, &mut world);
    // The boom is 0.25 m and the near distance 0.4 m: the ray runs 0.15 m the
    // other way, and a hit 0.06 m along it (fraction 0.4) puts the eye at
    // 0.4 - 0.4 * 0.15 = 0.34 m from TrackCenter.
    assert_eq!(world.calls.len(), 1);
    let (direction, far) = world.calls[0];
    assert!((far - 0.15).abs() < 1e-12, "ray length {far}");
    let pivot = Vector3::new(2.0, 0.9, -3.0);
    let boom = Vector3::new(cam.p.x - pivot.x, cam.p.y - pivot.y, cam.p.z - pivot.z);
    assert!(direction.dot(&boom) < 0.0, "the ray runs back past TrackCenter");
    assert!((cam.p.distance_to(&pivot) - 0.34).abs() < 1e-9, "eye {}", cam.p.distance_to(&pivot));
}
