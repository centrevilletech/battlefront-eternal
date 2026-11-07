//! Twin checks for the AI control core's pure logic: the expected
//! values were dumped from the JavaScript itself
//! (difficulty.js, controller.js, scheduler.js, agents.js under Node
//! 24.22) and are compared bit for bit, so a 1-ulp or one-draw slip
//! in the float32 arithmetic or the queue rules fails here. The
//! stateful behaviors are proven by the per-phase replay
//! (replay_ai.rs).

use bfe_sim::ai::control::agents::{random_cube, rotate_y, table_sin_cos};
use bfe_sim::ai::control::classes::{health_type, service_alive};
use bfe_sim::ai::control::controller::{goal_state, role_of, CommandQueue, MoveRequest, SourceGoal};
use bfe_sim::ai::control::difficulty::{clamp_skill, draw_range, skill_curves, Difficulty, DifficultyOptions, MissionCall};
use bfe_sim::ai::control::scheduler::{camera_accepts, candidate_budget, fallback_analogs};
use bfe_sim::ai::control::world::{AiUnit, CameraView, ClassSnap, Draws, LiveDraws, SoldierSnap};
use bfe_sim::ai::nav::math::Vec3;
use bfe_sim::ai::random::ClassicRandom;

#[test]
fn mounted_service_effects_preserve_publication_and_saved_post_semantics() {
    use bfe_sim::ai::control::{Core, Options};
    use bfe_sim::ai::control::controller::Controller;
    use bfe_sim::ai::control::world::{CharacterSnap, CoreEffect, TeamTable,Characters};
    struct Roster(Vec<CharacterSnap>);
    impl Characters for Roster {
        fn character(&self,index:usize)->CharacterSnap {self.0[index].clone()}
        fn character_set_objective(&mut self,index:usize,post:Option<usize>) {self.0[index].objective=post;}
    }

    let mut roster=Roster([3,5].into_iter().enumerate().map(|(i,post)|CharacterSnap {
        index:i,team:1.,human:false,unit:Some(i as u64),objective:Some(post),
    }).collect());
    let mut core=Core::new(Options::default(),TeamTable::default(),Difficulty::classic(DifficultyOptions::default()),roster.0.len());
    core.controllers = vec![Controller::new(0), Controller::new(1)];
    core.by_character = vec![Some(0), Some(1)];
    core.controllers[0].fire_a = true;
    // Node 24.22: ClassicObjectives.publish(6) for both crew members,
    // publish(null) for the driver, VehicleAI.disembarkPassengers(7),
    // then abandonPad at time 50. A saved post and a null publication
    // retain the earlier queued goal; both crew cooldowns become 65.
    core.apply_service_effects(&mut roster,vec![
        CoreEffect::Publish { character: 0, before: Some(3), after: Some(6) },
        CoreEffect::Publish { character: 1, before: Some(5), after: Some(6) },
        CoreEffect::Publish { character: 0, before: Some(6), after: None },
        CoreEffect::Objective { character: 1, before: Some(6), after: Some(7) },
        CoreEffect::Reboard { controller: 1, before: f64::NEG_INFINITY, after: 65.0 },
        CoreEffect::Reboard { controller: 0, before: f64::NEG_INFINITY, after: 65.0 },
    ]);
    assert_eq!(roster.0[0].objective, None);
    assert_eq!(roster.0[1].objective, Some(7));
    for c in &core.controllers {
        assert_eq!(&*c.commands.queue, &[SourceGoal::objective(Some(6))]);
        assert!(c.commands.latch && c.published);
        assert_eq!(c.reboard_at.to_bits(), 65.0f64.to_bits());
    }
    assert!(core.controllers[0].fire_a);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| core.apply_service_effects(&mut roster,vec![
        CoreEffect::Objective { character: 0, before: Some(3), after: Some(6) },
    ]))).is_err(), "a wrong incoming objective must not be hidden by the service result");
    core.controllers[1].reboard_at = -0.0;
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| core.apply_service_effects(&mut roster,vec![
        CoreEffect::Reboard { controller: 1, before: 0.0, after: 65.0 },
    ]))).is_err(), "the cooldown precondition uses Object.is semantics");
}

fn bits(v: f64) -> u64 {
    v.to_bits()
}

fn same(a: f64, b: f64) -> bool {
    bits(a) == bits(b)
}

#[test]
fn curves_match_the_javascript() {
    const CURVES: [[f64; 4]; 20] = [
        [0.800000011920929, 0.800000011920929, 3.0, 2.0],
        [0.884615421295166, 0.884615421295166, 2.892307758331299, 1.923076868057251],
        [0.9692307710647583, 0.9692307710647583, 2.7846152782440186, 1.8461538553237915],
        [1.0538461208343506, 1.0538461208343506, 2.6769230365753174, 1.7692307233810425],
        [1.1384615898132324, 1.1384615898132324, 2.569230794906616, 1.692307710647583],
        [1.2230769395828247, 1.2230769395828247, 2.461538314819336, 1.615384578704834],
        [1.307692289352417, 1.307692289352417, 2.3538460731506348, 1.5384615659713745],
        [1.3923077583312988, 1.3923077583312988, 2.2461538314819336, 1.4615384340286255],
        [1.4769229888916016, 1.4769229888916016, 2.1384615898132324, 1.384615421295166],
        [1.5615384578704834, 1.5615384578704834, 2.0307693481445312, 1.307692289352417],
        [1.6461538076400757, 1.6461538076400757, 1.923076868057251, 1.230769157409668],
        [1.730769157409668, 1.730769157409668, 1.8153846263885498, 1.1538461446762085],
        [1.8153846263885498, 1.8153846263885498, 1.707692265510559, 1.076923131942749],
        [1.899999976158142, 1.899999976158142, 1.600000023841858, 1.0],
        [4.9166669845581055, 1.9833333492279053, 1.5, 0.8333333134651184],
        [7.933333873748779, 2.066666603088379, 1.399999976158142, 0.6666666269302368],
        [10.949999809265137, 2.1500000953674316, 1.2999999523162842, 0.5],
        [13.966667175292969, 2.2333333492279053, 1.2000000476837158, 0.3333333134651184],
        [16.983333587646484, 2.316666603088379, 1.100000023841858, 0.1666666865348816],
        [20.0, 2.4000000953674316, 1.0, 0.0],
    ];
    for (i, row) in CURVES.iter().enumerate() {
        let c = skill_curves((i + 1) as f64);
        assert!(same(c.a, row[0]) && same(c.d, row[1]) && same(c.b, row[2]) && same(c.s, row[3]), "skill {}", i + 1);
    }
}

#[test]
fn clamp_and_draw_range_match() {
    let cases = [
        (0.0, 1.0), (1.0, 1.0), (20.0, 20.0), (21.0, 20.0), (-5.0, 1.0), (13.9, 13.0),
        (f64::NAN, 1.0), (f64::INFINITY, 1.0), (f64::NEG_INFINITY, 1.0), (2147483648.0, 1.0), (1e21, 1.0),
    ];
    for (v, want) in cases {
        assert!(same(clamp_skill(v), want), "clamp {v}");
    }
    let mut rng = ClassicRandom::new(12345.0);
    let draws = [
        (0.0, 100.5, 1.6623469591140747),
        (1.1, 2.1, 1.7349112033843994),
        (0.6, 1.3, 0.678729772567749),
        (0.0, 0.8, 0.4386778771877289),
        (-3.0, 3.0, 1.7007379531860352),
        (5.0, 5.0, 5.0),
    ];
    for (lo, hi, want) in draws {
        let got = draw_range(&mut || rng.uniform(), lo, hi);
        assert!(same(got, want), "drawRange {lo} {hi}: got {got} want {want}");
    }
}

#[test]
fn difficulty_tiers_match() {
    let make = |tier: f64| {
        Difficulty::classic(DifficultyOptions { tier, profile_tier: tier, player_team: Some(1.0), ..Default::default() })
    };
    let d = make(1.0);
    assert!(same(d.player_skill, 6.0) && same(d.enemy_skill, 4.0) && same(d.enemy.a, 1.0538461208343506));
    assert!(same(d.cycle(), 15.0) && same(d.los_budget(), 6.0) && d.danger_row() == [1.1, 2.1]);
    assert!(same(d.curves(1.0).a, 1.2230769395828247) && same(d.curves(2.0).a, 1.0538461208343506));
    let d = make(2.0);
    assert!(same(d.player_skill, 4.0) && same(d.enemy_skill, 14.0) && same(d.curves(1.0).a, 1.0538461208343506));
    let d = make(3.0);
    assert!(same(d.enemy_skill, 17.0) && same(d.cycle(), 3.0) && same(d.los_budget(), 12.0) && d.danger_row() == [0.0, 0.8]);
    let mut d = make(2.0);
    d.apply(&[
        MissionCall { name: "SetPlayerTeamDifficulty".into(), args: vec![9.0], str_args: vec![] },
        MissionCall { name: "SetEnemyTeamDifficultyHard".into(), args: vec![3.0], str_args: vec![] },
        MissionCall { name: "SetEnemyTeamDifficultyMedium".into(), args: vec![2.0], str_args: vec![] },
    ]);
    assert!(same(d.player_skill, 9.0) && same(d.enemy_skill, 16.0), "applied {} {}", d.player_skill, d.enemy_skill);
}

#[test]
fn candidate_budgets_match() {
    // (count, cycle, step, budget); dumped whole from the JavaScript.
    const BUDGET: [(f64, f64, f64, f64); 15] = [
        (64.0, 15.0, 1.0, 4.0), (64.0, 15.0, 11.0, 4.0), (64.0, 15.0, 12.0, 5.0), (64.0, 15.0, 15.0, 64.0),
        (64.0, 3.0, 1.0, 21.0), (64.0, 3.0, 3.0, 64.0), (10.0, 15.0, 5.0, 0.0), (10.0, 15.0, 6.0, 1.0),
        (10.0, 15.0, 15.0, 10.0), (0.0, 15.0, 1.0, 0.0), (0.0, 15.0, 15.0, 0.0),
        (17.0, 6.0, 1.0, 2.0), (17.0, 6.0, 2.0, 3.0), (17.0, 6.0, 6.0, 17.0), (17.0, 6.0, 4.0, 3.0),
    ];
    for (count, cycle, step, want) in BUDGET {
        assert!(same(candidate_budget(count, cycle, step), want), "budget {count} {cycle} {step}");
    }
}

#[test]
fn runtime_health_type_matches_repair_wrappers() {
    // Node's healthType uses the runtime field before ClassDef.HealthType.
    // A destroyed building's wrapper becomes BuildingRebuild at runtime.
    let mut unit = AiUnit {
        class_def: Some(std::rc::Rc::new(ClassSnap { health_type_name: Some("Vehicle".into()), ..Default::default() })),
        ..Default::default()
    };
    assert!(same(health_type(&unit), 3.0));
    for (runtime, want) in [(0.0, 0.0), (5.0, 5.0)] {
        unit.health_type_num = Some(runtime);
        assert!(same(health_type(&unit), want));
    }
    unit.health_type_num = None;
    for (runtime, want) in [("buildingrebuild", 5.0), ("BUILDINGBUILD", 6.0), ("unknown", 0.0)] {
        unit.health_type_name = Some(runtime.into());
        assert!(same(health_type(&unit), want));
    }
    unit.alive = false;
    unit.health_type_name = Some("BuildingRebuild".into());
    assert!(service_alive(&unit));
}

#[test]
fn table_trig_and_cube_match() {
    const SINCOS: [(f64, f64, f64); 7] = [
        (0.0, 0.0, 1.0),
        (0.5, 0.47942543029785156, 0.8775824904441833),
        (1.3089969, 0.9659247398376465, 0.2588188946247101),
        (-0.5235988, -0.5000025033950806, 0.8660300374031067),
        (std::f64::consts::PI, 9.983097726262713e-08, -1.0),
        (-7.25, -0.8230812549591064, 0.5679248571395874),
        (100.125, -0.3949111998081207, 0.918718159198761),
    ];
    for (a, s, c) in SINCOS {
        let (gs, gc) = table_sin_cos(a);
        assert!(same(gs, s) && same(gc, c), "sinCos {a}: got {gs} {gc} want {s} {c}");
    }
    let rv = rotate_y(Vec3::new(1.0, 2.0, 3.0), 0.7);
    assert!(same(rv.x, -1.167810082435608) && same(rv.y, 2.0) && same(rv.z, 2.9387422800064087), "rotate {rv:?}");
    let mut rc = ClassicRandom::new(999.0);
    let cube = random_cube(&mut || rc.uniform());
    assert!(same(cube.x, 0.16992735862731934) && same(cube.y, 0.29442858695983887) && same(cube.z, 0.783252477645874), "cube {cube:?}");
    assert_eq!(rc.state, 2512399989);
}

#[test]
fn scheduler_camera_boundaries_match_the_javascript() {
    let camera = CameraView {
        elements: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
        fov: 70.0, aspect: 1.6, near: 0.1, far: 1000.0, zoom: 1.75,
    };
    // Values on the inclusive planes and one ulp beyond, from Node24.22.
    for (point, want) in [
        ([0.0, 0.0, -0.1], true), ([0.0, 0.0, -0.09999999999999999], false),
        ([0.0, 0.0, -84.0], true), ([0.0, 0.0, -84.00000000000001], false),
        ([6.401897492203061, 0.0, -10.0], true), ([6.401897492203062, 0.0, -10.0], false),
        ([-6.401897492203061, 0.0, -10.0], true), ([-6.401897492203062, 0.0, -10.0], false),
        ([0.0, 4.001185932626913, -10.0], true), ([0.0, 4.001185932626914, -10.0], false),
        ([0.0, 0.0, 10.0], false),
    ] {
        let point = Vec3::new(point[0], point[1], point[2]);
        assert_eq!(camera_accepts(&camera, point), want, "camera {point:?}");
    }
}

#[test]
fn fallback_analogs_match_the_javascript() {
    let unit = AiUnit {
        position: Vec3::new(1.0, 2.0, 3.0),
        soldier: Some(SoldierSnap {
            aim_yaw: Some(0.7), aim_pitch: Some(0.11), max_speed: Some(7.0), max_strafe: Some(4.5), max_turn: Some(4.0),
            ..Default::default()
        }),
        class_def: Some(std::rc::Rc::new(ClassSnap { max_pitch_speed: Some(0.8), ..Default::default() })),
        ..Default::default()
    };
    let request = MoveRequest { valid: true, steering_target: Vec3::new(1.4, 2.0, 3.3), ..Default::default() };
    // Exact Node24.22 bits cover slowdown, steering-only aim, unsaturated
    // turn/pitch, facing's doubled turn limit and the vertical-only case.
    for (aim, dt, facing, want) in [
        (None, 1.0 / 60.0, false, [0x3fdffffffffffffc, 0xbfc706c300161fca, 0xbff0000000000000, 0xbff0000000000000]),
        (Some(Vec3::new(9.0, 4.0, 20.0)), 0.25, false, [0x3fdffffffffffffc, 0xbfc706c300161fca, 0x3fd0768009bfdde9, 0xbfdc59debb239484]),
        (Some(Vec3::new(-15.0, 10.0, 4.0)), 0.05, true, [0x3fdffffffffffffc, 0xbfc706c300161fca, 0x4000000000000000, 0x3ff0000000000000]),
        (Some(Vec3::new(1.0, 4.0, 3.0)), 0.0, false, [0x3fdffffffffffffc, 0xbfc706c300161fca, 0, 0]),
    ] {
        let got = fallback_analogs(&unit, &request, aim, dt, facing).map(f64::to_bits);
        assert_eq!(got, want, "fallback {aim:?}, dt {dt}, facing {facing}");
    }
}

#[test]
fn goal_state_and_roles_match() {
    let table = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0), (5.0, 5.0), (6.0, 6.0), (7.0, 0.0), (8.0, 9.0), (9.0, 10.0), (10.0, 11.0), (-1.0, 0.0), (f64::NAN, 0.0)];
    for (g, want) in table {
        assert!(same(goal_state(g), want), "goal {g}");
    }
    assert!(same(role_of(Some(2.0), None), 2.0));
    assert!(same(role_of(Some(2.5), Some("Scout")), 0.0));
    assert!(same(role_of(None, Some(" Anti-Armor ")), 1.0));
    assert!(same(role_of(None, Some("repair")), 3.0));
    assert!(same(role_of(None, Some("Pilot")), 4.0));
    assert!(same(role_of(None, None), 6.0));
    assert!(same(role_of(None, Some("x")), 6.0));
}

fn kinds(q: &CommandQueue) -> Vec<f64> {
    q.queue.iter().map(|g| g.kind).collect()
}

#[test]
fn command_queue_matches_the_javascript() {
    let mut q = CommandQueue::new();
    assert_eq!((q.count(), q.needs_next_goal(), q.current.kind, q.latch, q.current_done), (0, false, 0.0, false, false));
    assert!(q.append(SourceGoal::new(1.0)));
    assert_eq!(kinds(&q), vec![1.0]);
    assert!(q.append(SourceGoal::combat(1.0, None, None)));
    for _ in 0..5 {
        q.append(SourceGoal::objective(None));
    }
    assert_eq!(kinds(&q), vec![1.0, 2.0, 3.0, 3.0, 3.0]);
    assert!(!q.append(SourceGoal::new(1.0)));
    assert!(q.prepend_one(SourceGoal::follow(None, None)));
    assert_eq!(kinds(&q), vec![9.0, 1.0, 2.0, 3.0, 3.0]);
    assert!(q.latch && !q.prepend_one(SourceGoal::new(1.0)));
    q.pop_front();
    assert_eq!((q.current.kind, kinds(&q)), (9.0, vec![1.0, 2.0, 3.0, 3.0]));
    q.remove_kind(3.0);
    assert_eq!(kinds(&q), vec![1.0, 2.0]);
    q.append(SourceGoal::new(1.0));
    q.append(SourceGoal::new(0.0));
    q.append(SourceGoal::new(1.0));
    assert_eq!(kinds(&q), vec![1.0, 2.0, 1.0, 0.0, 1.0]);
    q.remove_kind(1.0);
    // The kind-0 record ends the scan: the trailing wait goes too.
    assert_eq!(kinds(&q), vec![2.0]);
    q.overwrite(SourceGoal::new(10.0));
    assert_eq!(kinds(&q), vec![10.0]);
    assert!(q.latch);
}

#[test]
fn live_draws_int_does_not_draw_when_degenerate() {
    let mut d = LiveDraws { a: ClassicRandom::new(7.0), b: ClassicRandom::new(9.0) };
    let before = d.a.state;
    assert!(same(d.a_int(4.0, 4.0), 4.0));
    assert_eq!(d.a.state, before);
    let _ = d.a_int(0.0, 1.0);
    assert_ne!(d.a.state, before);
}


#[test]
fn native_subclasses_keep_source_interrupt_and_crew_wait_overrides() {
    use bfe_sim::ai::control::{Core,Options,agents::{Agent,AgentKind},controller::Controller,world::{ForeignKind,TeamTable},policy};
    let mut core=Core::new(Options::default(),TeamTable::default(),Difficulty::classic(Default::default()),1);
    core.controllers.push(Controller::new(0));
    // Combat, boarding and unboarding discard the interrupted goal. Flyers
    // retain it and accept the crew wait; these methods need no world query.
    for (kind,keep,wait) in [(ForeignKind::Combat,false,false),(ForeignKind::Board,false,false),(ForeignKind::Unboard,false,false),(ForeignKind::Fly,true,true)] {
        core.controllers[0].agent=Some(Agent {fsm:Default::default(),unit:Some(1),goal:None,interrupts:Default::default(),helper:Default::default(),kind:AgentKind::Foreign(kind)});
        assert_eq!(core.saves_on_interrupt(0,0.),keep);assert_eq!(core.accepts_secondary_wait(0),wait);
        let input=policy::Input::read(&core.controllers[0]);
        if kind==ForeignKind::Combat {assert_eq!(input.kind,policy::Kind::Combat);}
        if kind==ForeignKind::Board {assert_eq!(input.kind,policy::Kind::Board);}
        if kind==ForeignKind::Unboard {assert_eq!(input.kind,policy::Kind::Unboard);}
    }
}

#[test]
fn patrol_keeps_a_built_capture_when_authored_capture_is_absent() {
    use bfe_sim::ai::control::{patrol,world::{PostSnap,RegionSnap,RegionKind,CaptureSnap,RawCapture}};
    let mut post=PostSnap {position:Vec3::new(4.,5.,6.),region:Some(RegionSnap {
        kind:RegionKind::Box,half:[7.,8.,9.],radius:0.,center:Vec3::new(40.,50.,60.),inverse:[0.;16],
    }),index:0,name:None,team:1.,active:true,building_alive:None,neutralize:0.,radius:0.,speed:0.,ai:Default::default(),
        desc_capture:None,desc_spawn_path:false,desc_spawn_points:0.,desc_ally_path:false};
    // captureRegion's built center is a Vector3, so its Array.isArray test
    // retains post.position while the built half extents still apply.
    assert_eq!(patrol::capture_region(&post),Some((post.position,7.,9.)));
    assert!(patrol::has_capture_region(&post));
    post.desc_capture=Some(std::rc::Rc::new(RawCapture {center:Some([1.,2.,3.]),half:Some([10.,11.,12.]),matrix:None,size:None}));
    assert_eq!(patrol::capture_region(&post),Some((Vec3::new(1.,2.,3.),10.,12.)));
    post.ai.capture=Some(CaptureSnap {center:Vec3::new(20.,21.,22.),half:[23.,24.,25.]});
    assert_eq!(patrol::capture_region(&post),Some((Vec3::new(20.,21.,22.),23.,25.)));
    post.ai.capture=None;post.desc_capture=None;post.region=None;
    assert!(!patrol::has_capture_region(&post));
}

#[test]
fn allied_patrols_keep_objectives_without_an_ally_spawn_path() {
    use bfe_sim::ai::control::{patrol,world::PostSnap};
    let mut post=PostSnap {index:0,name:None,team:1.,active:true,position:Vec3::default(),building_alive:None,
        neutralize:0.,radius:30.,speed:0.,ai:Default::default(),region:None,desc_capture:None,
        desc_spawn_path:false,desc_spawn_points:0.,desc_ally_path:false};
    post.ai.spawn_present=Some(true);post.ai.ally_present=Some(false);
    let allied=|_:f64,_:f64|1.;
    // classicBinding's predicate requires an ally path even when the post
    // has an ordinary spawn path. The flyer-radius predicate is separate.
    assert!(!patrol::friendly_spawn_eligible(&post,3.,true,&allied));
    assert!(patrol::friendly_spawn_eligible(&post,1.,true,&allied));
    post.ai.ally_present=Some(true);
    assert!(patrol::friendly_spawn_eligible(&post,3.,true,&allied));
    post.active=false;
    assert!(!patrol::friendly_spawn_eligible(&post,3.,true,&allied));
}
