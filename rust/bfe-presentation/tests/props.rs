//! The page's prop frame: doors, triggers, a creature following adopted
//! world state and hanging whom it holds from its node, a log trap, the record counts at and past their limits,
//! and no allocation once registered.
use bfe_presentation::{
    layout::{
        self, prop_body as pb, prop_held as hs, prop_bone as bo, prop_event as pe,
        prop_frame as fr, prop_header as ph, prop_input as pi, prop_limits as lim,
        prop_output as po, trap_input as ti, trap_output as to, trap_phase as tp,
    },
    props::{Props, PropInput, RiderInput, TrapDesc, TriggerInput},
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
        scene: Default::default(),
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
        grab_nodes: vec![0],
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
    p.prop_input[pi::WORLD] = 1.0;
    p.prop_input[pi::SLOT] = 1.0;
    p.prop_input[pi::STARTED] = 10.0;
    p.prop_input[pi::START_TIME] = 0.0;
    frame(&mut p, 0.0, 25.0, &[], &[]);
    assert_eq!(p.prop_output[po::POSED], 1.0);
    let a = (16.0 / 60.0) * std::f64::consts::FRAC_PI_4;
    assert!((p.bones[bo::STRIDE + bo::QUATERNION + 1] - a.sin()).abs() < 1e-6);
    // Following with no time passing poses nothing short of a full weight.
    frame(&mut p, 0.0, 10.5, &[], &[]);
    assert_eq!(p.prop_output[po::POSED], 0.0);
}

#[test]
fn a_trap_rests_by_its_phase_and_keeps_unweighted_bones() {
    let mut p = Props::new();
    let c = clip(&mut p);
    let mut initial = bones();
    initial.extend_from_slice(&[0.0, 0.0, 0.0, 1.0, 5.0, 5.0, 5.0]);
    p.add_trap(TrapDesc { scene: Default::default(), root: 0, initial, trigger_clip: Some(c), build_clip: None, trigger_length: 31.0 / 30.0 });
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
    q.prop_input[pi::WORLD] = 1.0;
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
    p.add_trap(TrapDesc { scene: Default::default(), root: 0, initial: bones(), trigger_clip: Some(c), build_clip: Some(c), trigger_length: 1.0 });
    let (rider, held) = soldier(&mut p);
    hold(&mut p, 2, rider, held, 100.0);
    let feet = vec![[0.0; 3]; lim::FEET];
    let bodies = vec![[0.0, 0.0, 0.0, 1.0]; lim::BODIES];
    TRACK.with(|t| t.set(true));
    for k in 0..240 {
        p.prop_input[2 * pi::STRIDE + pi::WORLD] = if k < 60 { 0.0 } else { 1.0 };
        p.prop_input[2 * pi::STRIDE + pi::STARTED] = (k / 90 * 90) as f64;
        p.trap_input[ti::PHASE] = (k / 60 % 4) as f64;
        frame(&mut p, 1.0 / 60.0, k as f64, &feet, &bodies);
    }
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
}

#[test]
fn creature_matrices_publish_the_eased_float_pose_under_its_authored_root() {
    use bfe_presentation::{scene::{Input,Node},layout::scene_node as n};
    use three::{Matrix4,Quaternion,Vector3};
    let mut p=Props::new();let c=clip(&mut p);let mut desc=prop(false,c,vec![],true);
    let identity=Matrix4::default().elements;
    let mut placed=Matrix4::default();placed.make_translation(10.0,3.0,-7.0);
    let node=|parent,auto,skinned|Node {parent,pose:[0.0,0.0,0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0],matrix:identity,auto,skinned};
    let mut root=node(-1,false,false);root.matrix=placed.elements;
    // Creature record order is not hierarchy order: the rancor's bone_root
    // follows its spine in the skeleton table. Explicit bone handles keep
    // that late parent above the sampled spine in the retained scene.
    desc.initial.extend_from_slice(&[0.0,0.0,0.0,1.0,4.0,0.0,0.0]);
    desc.bind=desc.initial.clone();
    desc.scene=Input {nodes:vec![root,node(0,true,false),node(1,true,false),node(2,true,false),node(0,true,true)],bones:vec![1,3,2],parent:Some(identity)};
    p.add_prop(desc);p.prop_input[pi::WORLD]=1.0;
    p.prop_input[pi::SLOT]=0.0;p.prop_input[pi::STARTED]=1.0;
    frame(&mut p,1.0/60.0,25.0,&[],&[]);
    // A newly adopted action eases from the old idle. Publishing its raw
    // target would jump to a different pose, especially on a large creature.
    p.prop_input[pi::SLOT]=1.0;p.prop_input[pi::STARTED]=25.0;
    frame(&mut p,1.0/60.0,26.0,&[],&[]);
    let b=&p.bones[bo::STRIDE..];
    let mut local=Matrix4::default();local.compose(&Vector3::new(b[4],b[5],b[6]),&Quaternion::new(b[0],b[1],b[2],b[3]),&Vector3::new(1.0,1.0,1.0));
    let at=3*n::STRIDE;
    assert_eq!(&p.nodes[at+n::MATRIX..at+n::MATRIX+16],&local.elements);
    assert_eq!(&p.nodes[n::WORLD..n::WORLD+16],&placed.elements,"matrix-authored root survives publication");
    assert_eq!(p.nodes[at+n::WORLD+12],14.0,"a late skeleton parent still carries its child");
    assert!(b[1]>0.25,"the eased pose still retains most of the preceding idle");
    let mesh=4*n::STRIDE;let mut world=Matrix4::default();let mut inverse=Matrix4::default();
    world.from_array(&p.nodes[mesh+n::WORLD..],0);inverse.from_array(&p.nodes[mesh+n::BIND_INVERSE..],0);
    world.multiply(&inverse);assert_eq!(world.elements,identity,"attached mesh binding is native too");
}

#[test]
fn turret_frames_retain_storage_and_reject_nonfinite_aim() {
    use bfe_presentation::{scene::{Input,Node},turrets::Descriptor,layout::{turret_presentation_input as t,scene_node as n}};
    use three::Matrix4;
    let matrix=Matrix4::default().elements;
    let node=|parent,z|Node {parent,pose:[0.0,0.0,z,0.0,0.0,0.0,1.0,1.0,1.0,1.0],matrix,auto:true,skinned:false};
    let mut p=Props::new();
    p.add_turret(Descriptor {seat:0,source_shown:false,turn_rate:0.0,scene:Input {nodes:vec![node(-1,0.0),node(0,0.0),node(1,-0.5)],bones:vec![],parent:None},
        drum:1,gun:-1,parent_turn:Some([0.0,0.0,0.0,1.0]),gun_bind:None,up:[0.0,1.0,0.0],yaw_factor:1.0,pitch_factor:0.0});
    p.input[fr::VERSION]=layout::VERSION as f64;
    // A full stream of new angles uses the registered storage. Corrupt
    // aim retains the last valid frame instead of poisoning drawn matrices.
    TRACK.with(|v|v.set(true));
    for tick in 0..1000 {p.turret_input[t::YAW]=tick as f64*0.01;p.frame();}
    let before=p.nodes[2*n::STRIDE+n::WORLD+12];
    p.turret_input[t::YAW]=f64::INFINITY;p.frame();
    TRACK.with(|v|v.set(false));
    assert_eq!(ALLOCS.with(Cell::get),0);
    assert_eq!(p.nodes[2*n::STRIDE+n::WORLD+12],before);
}

// A soldier's skeleton as riderPoints reads it (DummyRoot, bone_root and
// the joints the points hang from, in a chain), and his grab clip: bone_root
// 1 m up, turned a quarter about y. Returns the rider and clip handles.
fn soldier(p: &mut Props) -> (f64, f64) {
    let names = ["DummyRoot", "bone_root", "bone_pelvis", "bone_neck", "bone_head", "bone_l_thigh", "bone_l_calf", "bone_l_foot", "bone_r_thigh", "bone_r_calf", "bone_r_foot"];
    let rider = p.add_rider(RiderInput {
        names: names.iter().map(|n| n.to_string()).collect(),
        parents: (0..names.len() as i32).map(|i| i - 1).collect(),
        poses: vec![[0.0, 0.1, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]; names.len()],
        binds: vec![],
        root_bind: Some([0.0; 3]),
    });
    let (s, c) = (std::f64::consts::FRAC_PI_4.sin() as f32, std::f64::consts::FRAC_PI_4.cos() as f32);
    let data: Vec<f32> = (0..2).flat_map(|_| [0.0, s, 0.0, c, 0.0, 1.0, 0.0]).collect();
    let clip = p.load_clip(2, 30.0, false, vec![1], data);
    (rider as f64, clip as f64)
}
// Prop `i` holding that soldier since `since`.
fn hold(p: &mut Props, i: usize, rider: f64, clip: f64, since: f64) {
    let x = &mut p.prop_input[i * pi::STRIDE..(i + 1) * pi::STRIDE];
    x[pi::VICTIM] = 1.0;
    x[pi::GRAB] = 0.0;
    x[pi::SINCE] = since;
    x[pi::RIDER] = rider;
    x[pi::CLIP] = clip;
}

#[test]
fn a_creature_hangs_whom_it_holds_from_its_node() {
    use bfe_presentation::scene::{Input, Node};
    use three::{Matrix4, Quaternion, Vector3};
    let mut p = Props::new();
    let c = clip(&mut p);
    let mut desc = prop(false, c, vec![], true);
    let mut placed = Matrix4::default();
    placed.make_translation(10.0, 3.0, -7.0);
    let node = |parent, auto| Node { parent, pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], matrix: Matrix4::default().elements, auto, skinned: false };
    let mut root = node(-1, false);
    root.matrix = placed.elements;
    desc.scene = Input { nodes: vec![root, node(0, true), node(1, true)], bones: vec![1, 2], parent: None };
    p.add_prop(desc);
    let (rider, held) = soldier(&mut p);
    hold(&mut p, 0, rider, held, 30.0);
    // Not adopted yet: no hold drawn.
    frame(&mut p, 1.0 / 60.0, 20.0, &[], &[]);
    assert_eq!(p.prop_output[po::HELD], hs::NONE as f64);
    p.prop_input[pi::WORLD] = 1.0;
    p.prop_input[pi::SLOT] = 1.0;
    p.prop_input[pi::STARTED] = 30.0;
    // Before the tick it took him, he is drawn from his own record.
    frame(&mut p, 1.0 / 60.0, 29.5, &[], &[]);
    assert_eq!(p.prop_output[po::HELD], hs::WAITING as f64);
    // Then his root on the node: the node's frame times his inverse root.
    frame(&mut p, 1.0 / 60.0, 36.0, &[], &[]);
    assert_eq!(p.prop_output[po::HELD], hs::TIMED as f64);
    assert!((p.prop_output[po::HELD_TIME] - 0.1).abs() < 1e-12);
    assert_eq!(p.prop_output[po::HELD_PLACED], 1.0);
    let mut root = Matrix4::default();
    let mut turn = Quaternion::new(0.0, std::f64::consts::FRAC_PI_4.sin() as f32 as f64, 0.0, std::f64::consts::FRAC_PI_4.cos() as f32 as f64);
    turn.normalize();
    root.compose(&Vector3::new(0.0, 1.0, 0.0), &turn, &Vector3::new(1.0, 1.0, 1.0)).invert();
    let mut want = Matrix4::default();
    want.multiply_matrices(&placed, &root);
    let got = &p.prop_output[po::HELD_FRAME..po::HELD_FRAME + 16];
    for k in 0..16 {
        assert!((got[k] - want.elements[k]).abs() < 1e-9, "{k}: {} {}", got[k], want.elements[k]);
    }
    // Let go, a stale skeleton or clip, or a grab past its nodes: none or
    // not placed, never a panic.
    p.prop_input[pi::RIDER] = 9.0;
    frame(&mut p, 1.0 / 60.0, 37.0, &[], &[]);
    assert_eq!(p.prop_output[po::HELD_PLACED], 0.0);
    p.prop_input[pi::RIDER] = rider;
    p.prop_input[pi::GRAB] = f64::NAN;
    frame(&mut p, 1.0 / 60.0, 38.0, &[], &[]);
    assert_eq!(p.prop_output[po::HELD_PLACED], 0.0);
    p.prop_input[pi::VICTIM] = 0.0;
    frame(&mut p, 1.0 / 60.0, 39.0, &[], &[]);
    assert_eq!(p.prop_output[po::HELD], hs::NONE as f64);
}

#[test]
fn held_unkeyed_joints_retain_the_native_unit_pose_at_seizure(){
    use bfe_pose::{display::DisplayRig,rig::skeleton::Skeleton};
    use bfe_presentation::scene::{Input,Node};
    use three::{Matrix4,Quaternion,Vector3};
    let names=["DummyRoot","bone_root","bone_pelvis","bone_neck","bone_head","bone_l_thigh","bone_l_calf","bone_l_foot","bone_r_thigh","bone_r_calf","bone_r_foot"];
    let binds=vec![(Vector3::default(),Quaternion::default(),Vector3::new(1.0,1.0,1.0));names.len()];
    let skeleton=Skeleton::from_bind(names.iter().map(|s|s.to_string()).collect(),(0..names.len()as i32).map(|n|n-1).collect(),&binds,false);
    let mut rig=DisplayRig::new(&skeleton);rig.tree.node_mut(rig.bones[1]).position.y=2.0;
    let mut props=Props::new();let rider=props.add_unit_rider(&skeleton,&rig);
    // Later display poses must not replace this immutable held sample. A
    // grab clip without a root track keeps the pose at its first seizure.
    rig.tree.node_mut(rig.bones[1]).position.y=3.0;
    let clip=props.load_clip(2,30.0,false,vec![],vec![]);
    let mut descriptor=prop(false,clip,vec![],true);
    descriptor.scene=Input{nodes:vec![Node{parent:-1,pose:[0.0,0.0,0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0],matrix:Matrix4::default().elements,auto:false,skinned:false}],bones:vec![],parent:None};
    props.add_prop(descriptor);hold(&mut props,0,rider as f64,clip as f64,0.0);
    props.prop_input[pi::WORLD]=1.0;props.prop_input[pi::SLOT]=1.0;
    frame(&mut props,0.0,1.0,&[],&[]);
    assert_eq!(props.prop_output[po::HELD_PLACED],1.0);
    assert_eq!(props.prop_output[po::HELD_FRAME+13],-2.0);
}
