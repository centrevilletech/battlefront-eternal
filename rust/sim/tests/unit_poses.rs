use bfe_sim::{gen::{Half, HitShape, ShapeType}, pose::Clip, rig::{rider::RiderSample, skeleton::{BoneDesc, Skeleton}}, units::poses::BodyPose};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use three::{Matrix4, Vector3};

struct Allocator;
thread_local! { static TRACK: Cell<bool> = const { Cell::new(false) }; static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|on| if on.get() { ALLOCATIONS.with(|count| count.set(count.get() + 1)); });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) { System.dealloc(pointer, layout); }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|on| if on.get() { ALLOCATIONS.with(|count| count.set(count.get() + 1)); });
        System.realloc(pointer, layout, size)
    }
}
#[global_allocator] static ALLOCATOR: Allocator = Allocator;

fn skeleton() -> Skeleton {
    let names = ["DummyRoot", "bone_root", "bone_pelvis", "bone_neck", "bone_head", "bone_l_thigh", "bone_l_calf", "bone_l_foot", "bone_r_thigh", "bone_r_calf", "bone_r_foot"];
    let mut bones = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let mut local = Matrix4::default();
        local.make_translation(i as f64 * 0.01, if i == 2 { 0.8 } else { 0.1 }, 0.0);
        bones.push(BoneDesc { name: name.to_string(), parent: i as i32 - 1, local: local.elements });
    }
    Skeleton::actor(&bones)
}
fn clip() -> Clip {
    // Missing tracks retain bind joints, just as a rider clip does in a world.
    Clip { frames: 1, fps: 30.0, root_motion: false, tracks: vec![-1; 11], data: vec![] }
}

#[test]
fn retained_pose_samples_riders_and_held_frames_without_tick_allocation() {
    let skeleton = skeleton(); let mut body = BodyPose::new(&skeleton); let clip = clip();
    let sample = RiderSample { clip: &clip, pitch: 0.2, turn: -0.3, time: None };
    let frame = Matrix4::default(); let mut shapes = vec![HitShape::default(); 10]; let mut held = Matrix4::default();
    assert_eq!(body.hit_shapes(&sample, &frame, 1.0, &mut shapes), 6);
    assert_eq!(shapes[0].r#type, ShapeType::Sphere); assert_eq!(shapes[1].r#type, ShapeType::Capsule);
    assert!(matches!(shapes[1].half, Half::Vec3(v) if v == shapes[1].half_size));
    assert!(body.held_frame(&clip, 0.0, &frame, &mut held));
    let mut expected = Matrix4::default();
    expected.make_translation(-skeleton.bind[1].position.x - skeleton.bind[0].position.x,
        -skeleton.bind[1].position.y - skeleton.bind[0].position.y, 0.0);
    assert_eq!(held, expected);
    ALLOCATIONS.with(|n| n.set(0)); TRACK.with(|on| on.set(true));
    for _ in 0..1200 {
        assert_eq!(body.hit_shapes(&sample, &frame, 1.0, &mut shapes), 6);
        assert!(body.held_frame(&clip, 0.0, &frame, &mut held));
    }
    TRACK.with(|on| on.set(false)); assert_eq!(ALLOCATIONS.with(Cell::get), 0);
}

#[test]
fn a_missing_joint_falls_back_without_touching_the_output() {
    let empty = Skeleton::actor(&[]); let mut body = BodyPose::new(&empty); let clip = clip();
    let sample = RiderSample { clip: &clip, pitch: 0.0, turn: 0.0, time: None };
    let mut shapes = vec![HitShape::default(); 10]; shapes[0].center = [9.0; 3];
    assert_eq!(body.hit_shapes(&sample, &Matrix4::default(), 1.0, &mut shapes), 0);
    assert_eq!(shapes[0].center, [9.0; 3]);
    let mut frame = Matrix4::default(); frame.make_translation(2.0, 3.0, 4.0);
    assert!(!body.held_frame(&clip, 0.0, &Matrix4::default(), &mut frame));
    assert_eq!(Vector3::new(frame.elements[12], frame.elements[13], frame.elements[14]), Vector3::new(2.0,3.0,4.0));
}

#[test]
fn droideka_saved_transition_pose_reuses_all_thirty_three_bones() {
    use bfe_sim::{gen::{BonePose, RigAnimator}, rig::droideka::DroidekaRig, units::droideka::take_animator};
    let bones: Vec<_> = (0..33).map(|i| BoneDesc {
        name: format!("bone_{i}"), parent: i as i32 - 1,
        local: Matrix4::default().elements,
    }).collect();
    let skeleton = Skeleton::actor(&bones);
    let mut rig = DroidekaRig::new(&skeleton, &[], Vector3::default());
    let mut animator = RigAnimator { from: vec![BonePose::default(); 33], ..Default::default() };
    let storage = animator.from.as_ptr();
    ALLOCATIONS.with(|n| n.set(0)); TRACK.with(|on| on.set(true));
    for tick in 0..1200 {
        rig.animator.from_p[32].y = tick as f64;
        take_animator(&mut animator, &rig);
        assert_eq!(animator.from[32].p[1], tick as f64);
    }
    TRACK.with(|on| on.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(animator.from.as_ptr(), storage);
}
