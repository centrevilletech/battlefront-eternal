//! Zero-allocation hot-path and cross-architecture replay determinism test for
//! the Battlefront II melee combo state machine, adaptive blade sampling,
//! deflection, and Force powers.

use bfe_wide::Wide;
use bfe_family::GameFamily;
use bfe_sim::combat::combo::{
    build_damage_samples, execute_force_push, execute_force_push_validated,
    resolve_melee_hit_samples, step, try_deflect_with_spread, BodyContext, ComboButton, ComboInput,
    ComboMachine, ComboPosture, ComboState, DeflectAimMode, DeflectDef, DeflectableKind,
    DefenderPose, ForcePushCandidate, ForcePushDef, IncomingProjectile,
    SaberThrowDef, SaberThrowState, ValidatedForcePushDef, ValidatedSaberThrowDef, WeaponMeleeSpec,
    COMBO_ANIM_FPS, COMBO_FIXED_DT,
};
use bfe_sim::combat::impacts::HealthScale;
use bfe_sim::gen::{Half, HitShape, ShapePart, ShapeType};
use bfe_sim::movement::soldier::swbf2 as move2;
use bfe_sim::store::DefId;
use serde_json::json;
#[global_allocator]
static GLOBAL: bfe_test_alloc::Counting = bfe_test_alloc::Counting::new();

fn invented_replay_machine() -> ComboMachine {
    let doc = json!({
        "nameHash": "00cc4401",
        "animations": [
            { "values": ["arc_cut_a"], "duration": 0.40 },
            { "values": ["arc_cut_b"], "duration": 0.45 },
            { "values": ["guard_stance"], "duration": 0.80, "children": [{ "key": "Loop", "values": [] }] }
        ],
        "states": [
            {
                "name": "IDLE",
                "children": [
                    { "key": "Duration", "values": [0.0] },
                    { "key": "ThrustFactor", "values": [0.97] },
                    { "key": "StrafeFactor", "values": [0.93] },
                    {
                        "key": "Transition",
                        "values": ["CUT_ALPHA"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "Break", "values": [0.02] },
                                { "key": "Button", "values": ["Fire", "Press"] }
                            ]
                        }]
                    },
                    {
                        "key": "Transition",
                        "values": ["WARD_HOLD"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "Break", "values": [0.02] },
                                { "key": "Energy", "values": [">=", 7.5] },
                                { "key": "Button", "values": ["FireSecondary", "Press"] }
                            ]
                        }]
                    }
                ]
            },
            {
                "name": "CUT_ALPHA",
                "children": [
                    { "key": "Animation", "values": ["arc_cut_a"] },
                    { "key": "Duration", "values": [0.41] },
                    { "key": "TurnFactor", "values": [0.73] },
                    {
                        "key": "Attack",
                        "children": [
                            { "key": "AttackId", "values": ["ArcAlpha"] },
                            { "key": "DamageTime", "values": [0.10, 0.30, "Seconds"] },
                            { "key": "DamageLength", "values": [1.7] },
                            { "key": "DamageWidth", "values": [0.5] },
                            { "key": "Damage", "values": [140.0] },
                            { "key": "Push", "values": [2.5] }
                        ]
                    },
                    {
                        "key": "Transition",
                        "values": ["CUT_BETA"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "TimeStart", "values": [0.10, "Seconds"] },
                                { "key": "TimeEnd", "values": [0.35, "Seconds"] },
                                { "key": "Button", "values": ["Fire", "Press"] }
                            ]
                        }]
                    },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            },
            {
                "name": "CUT_BETA",
                "children": [
                    { "key": "Animation", "values": ["arc_cut_b"] },
                    { "key": "Duration", "values": [0.47] },
                    { "key": "TurnFactor", "values": [0.69] },
                    {
                        "key": "Attack",
                        "children": [
                            { "key": "AttackId", "values": ["ArcBeta"] },
                            { "key": "DamageTime", "values": [0.12, 0.34, "Seconds"] },
                            { "key": "DamageLength", "values": [1.9] },
                            { "key": "DamageWidth", "values": [0.6] },
                            { "key": "Damage", "values": [180.0] },
                            { "key": "Push", "values": [3.5] }
                        ]
                    },
                    { "key": "Transition", "values": ["IDLE"] }
                ]
            },
            {
                "name": "WARD_HOLD",
                "children": [
                    { "key": "Animation", "values": ["guard_stance"] },
                    { "key": "EnergyRestoreRate", "values": [-8.0] },
                    { "key": "TurnFactor", "values": [0.83] },
                    {
                        "key": "Deflect",
                        "children": [
                            { "key": "DeflectAngle", "values": [-105.0, 105.0] },
                            { "key": "EnergyCost", "values": [2.0] }
                        ]
                    },
                    {
                        "key": "Transition",
                        "values": ["IDLE"],
                        "children": [{
                            "key": "If",
                            "children": [
                                { "key": "Break", "values": [0.25] },
                                { "key": "Button", "values": ["FireSecondary", "Press"] }
                            ]
                        }]
                    }
                ]
            }
        ]
    });
    let weapon = WeaponMeleeSpec {
        combo_hash: 0x00cc_4401,
        ..WeaponMeleeSpec::default()
    };
    ComboMachine::from_json_value(&doc, &weapon, COMBO_ANIM_FPS, |_| None)
        .expect("invented machine loads")
}

/// Xor a whole word into the hash, then multiply by the FNV prime.
fn mix_word(hash: &mut Wide, v: Wide) {
    *hash = hash.xor(v).wrapping_mul(Wide::new(0x0000_0100, 0x0000_01B3));
}

/// A count or a flag as a whole word.
fn small(v: usize) -> Wide {
    Wide::from_u32(u32::try_from(v).unwrap_or(u32::MAX))
}

#[test]
fn hot_path_zero_alloc_and_deterministic_replay_digest() {
    let machine = invented_replay_machine();
    let weapon = WeaponMeleeSpec {
        combo_hash: 0x00cc_4401,
        ..WeaponMeleeSpec::default()
    };
    let speed = move2::ControlSpeed { forward: 1.0, strafe: 1.0, turn: 1.0 };
    let tuning = move2::Tuning {
        stand: speed,
        crouch: speed,
        sprint: speed,
        jump: speed,
        jet: speed,
        jetpack: None,
        roll: speed,
        tumble: speed,
        sprint_accelerate_time: 0.2,
        energy_capacity: 100.0,
        energy_floor: 0.0,
        energy_restore: 12.0,
        energy_restore_idle: 22.0,
        energy_drain_sprint: 0.0,
        energy_min_sprint: 10.0,
        energy_cost_jump: 0.0,
        energy_cost_jump_sprint: None,
        energy_cost_roll: 0.0,
        jump_height: 1.0,
        jump_forward_speed_factor: 1.0,
        jump_strafe_speed_factor: 1.0,
        roll_speed_factor: 1.0,
        roll_frames: 1,
        recover_from_tumble: false,
        getup_front_seconds: move2::DEFAULT_GETUP_FRONT_SECONDS,
        getup_back_seconds: move2::DEFAULT_GETUP_BACK_SECONDS,
    };
    let body = BodyContext {
        posture: ComboPosture::Stand,
        aim_yaw: 0.25,
        velocity_y: 0.0,
        grounded: true,
        moving: false,
    };
    let push_def = ForcePushDef {
        energy_drain: 15.0,
        cone_length: 14.0,
        cone_angle_deg: 85.0,
        max_targets: 4,
        push: 12.0,
        damage: 10.0,
        affect_friends: false,
    };
    let validated_push = ValidatedForcePushDef::try_new(push_def).expect("valid push");
    let invalid_push = ForcePushDef {
        cone_length: -2.0,
        ..push_def
    };
    let throw_def = SaberThrowDef {
        energy_drain: 20.0,
        velocity: 22.0,
        life_span: 0.45,
        return_duration: 0.35,
        rotation_rate: 3.5,
        damage: 160.0,
        damage_length: 1.3,
        damage_width: 0.5,
        push: 1.5,
        damage_scales: [1.0; 5],
        shield_scale: 1.0,
    };
    let validated_throw = ValidatedSaberThrowDef::try_new(throw_def).expect("valid throw");
    let bad_throw = SaberThrowDef {
        velocity: -10.0,
        ..throw_def
    };
    let candidates = [
        ForcePushCandidate {
            target_id: bfe_sim::combat::combo::TargetId::Life(11),
            position: [0.5, 0.0, 6.0],
            team: 2.0,
            alive: true,
            los_clear: true,
        },
        ForcePushCandidate {
            target_id: bfe_sim::combat::combo::TargetId::Life(12),
            position: [-0.8, 0.0, 9.0],
            team: 2.0,
            alive: true,
            los_clear: true,
        },
    ];
    let target_shapes = [HitShape {
        r#type: ShapeType::Cylinder,
        part: ShapePart::Body,
        center: [0.0, 1.0, 1.4],
        radius: 0.45,
        half_size: [0.45, 0.85, 0.45],
        half: Half::F64(0.85),
        yaw: 0.0,
    }];
    let target = bfe_sim::combat::combo::MeleeTarget {
        target_id: bfe_sim::combat::combo::TargetId::Life(700),
        position: [0.0, 0.0, 1.4],
        aim_yaw: std::f64::consts::PI,
        health_scale: HealthScale::Person,
        shapes: &target_shapes,
        deflect: None,
    };
    let deflect_def = DeflectDef {
        min_angle_deg: -105.0,
        max_angle_deg: 105.0,
        energy_cost: 2.0,
        deflect_anim_start: 0,
        deflect_anim_len: 0,
        reaction_ids: [u16::MAX;6],
    };
    let pose = DefenderPose {
        position: [0.0, 0.0, 0.0],
        aim_yaw: 0.0,
        aim_dir: [0.0, 0.0, 1.0],
    };
    let incoming = IncomingProjectile {
        kind: DeflectableKind::Bolt,
        impact_point: [0.0, 1.0, 0.2],
        incoming_dir: [0.0, 0.0, -1.0],
        shooter_pos: Some([0.0, 1.0, 12.0]),
        ordnance_def: DefId(0),
    };

    let run_once = || -> (Wide, u32) {
        let mut st = ComboState::new(&machine);
        let mut throw_st = SaberThrowState::new();
        let mut energy = move2::State::new(&tuning);
        let mut digest = Wide::new(0xCBF2_9CE4, 0x8422_2325);

        bfe_test_alloc::start_all();

        for tick in 0..1200u32 {
            let phase = tick % 90;
            let input = match phase {
                0 | 12 => ComboInput::default().press(ComboButton::Fire),
                45 | 68 => ComboInput::default().press(ComboButton::FireSecondary),
                _ => ComboInput::default(),
            };
            let out = step(
                GameFamily::Swbf2,
                &machine,
                &mut st,
                &input,
                &body,
                &mut energy,
                &tuning,
                true,
                COMBO_FIXED_DT,
            );
            mix_word(&mut digest, Wide::from_u32(u32::from(st.state.0)));
            mix_word(&mut digest, Wide::from_f64_bits(st.eval_time));
            mix_word(&mut digest, Wide::from_f64_bits(st.anim_time));
            mix_word(&mut digest, Wide::from_f64_bits(energy.energy));

            for active in out.active_attacks.as_slice() {
                let atk_def = &machine.state(active.state_id).attacks[active.attack_index as usize];
                let samples = build_damage_samples(0.0, COMBO_FIXED_DT, atk_def.damage_length, |t| {
                    let frac = t / COMBO_FIXED_DT;
                    let ang = -0.9 + 1.8 * frac;
                    let (s, c) = v8math::sin_cos(ang);
                    ([0.0, 1.0, 0.0], [s, 0.0, c])
                });
                let res = resolve_melee_hit_samples(
                    [0.0; 3],
                    atk_def,
                    &weapon,
                    samples.as_slice(),
                    &mut st.hit_history,
                    &target,
                    None,
                );
                mix_word(&mut digest, small(samples.len()));
                if let bfe_sim::combat::combo::MeleeHitOutcome::Hit { damage, .. } = res {
                    mix_word(&mut digest, Wide::from_f64_bits(damage));
                }
            }

            if phase == 50 {
                let mut de = 50.0;
                if let bfe_sim::combat::combo::DeflectOutcome::Deflected { reflected_dir, .. } =
                    try_deflect_with_spread(
                        Some(&deflect_def),
                        &pose,
                        &mut de,
                        &incoming,
                        DeflectAimMode::AimDirection,
                        4.0,
                        Some(Wide::from_u32(tick)),
                    )
                {
                    mix_word(&mut digest, Wide::from_f64_bits(reflected_dir[0]));
                    mix_word(&mut digest, Wide::from_f64_bits(reflected_dir[2]));
                }
            }

            if phase == 75 {
                let mut pe = 80.0;
                if let Some(pr) = execute_force_push_validated(
                    &validated_push,
                    [0.0; 3],
                    [0.0, 0.0, 1.0],
                    1.0,
                    &mut pe,
                    &candidates,
                ) {
                    mix_word(&mut digest, small(pr.hits().len()));
                    mix_word(&mut digest, Wide::from_f64_bits(pe));
                }
            }

            if phase == 30 && !throw_st.is_active() {
                let mut te = 60.0;
                let launched = throw_st.launch_validated(
                    &validated_throw,
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    &mut te,
                );
                mix_word(&mut digest, Wide::from_u32(u32::from(launched)));
            }
            if throw_st.is_active() {
                let ts = throw_st.step_validated(
                    &validated_throw,
                    [0.0, 1.0, 0.0],
                    true,
                    COMBO_FIXED_DT,
                );
                if let Some(sw) = ts.sweep {
                    let hit = throw_st.resolve_hit_validated(&validated_throw, &sw, &target, None);
                    if let bfe_sim::combat::combo::MeleeHitOutcome::Hit { damage, .. } = hit {
                        mix_word(&mut digest, Wide::from_f64_bits(damage));
                    }
                }
                mix_word(&mut digest, Wide::from_f64_bits(throw_st.position[2]));
            }

            if tick == 600 {
                let cp = st.checkpoint();
                let mut restored = ComboState::new(&machine);
                restored.try_restore(&machine, cp).expect("valid mid-swing correction");
                mix_word(&mut digest, Wide::from_u32(1));
                st = restored;
            }
        }

        // 100 rejected invalid launches + 100 rejected invalid pushes must also allocate 0 bytes.
        let mut rej_energy = 100.0;
        let mut rej_throw = SaberThrowState::new();
        for _ in 0..100 {
            let ok_t = rej_throw.launch(&bad_throw, [0.0; 3], [0.0, 0.0, 1.0], &mut rej_energy);
            let ok_p = execute_force_push(
                &invalid_push,
                [0.0; 3],
                [0.0, 0.0, 1.0],
                1.0,
                &mut rej_energy,
                &candidates,
            )
            .is_some();
            mix_word(&mut digest, Wide::from_u32(u32::from(ok_t || ok_p)));
        }

        (digest, bfe_test_alloc::finish_all().allocations)
    };

    let (d1, allocs1) = run_once();
    let (d2, allocs2) = run_once();
    assert_eq!(allocs1, 0, "hot path (1200 valid ticks + 200 rejected invalid calls) must allocate 0 bytes");
    assert_eq!(allocs2, 0, "second hot-path run must allocate 0 bytes");
    assert_eq!(d1, Wide::new(0xe892_fca7, 0xc060_2347), "fixed synthetic combat replay");
    eprintln!("COMBO_REPLAY {d1:016x}");
    assert_eq!(d1, d2, "two deterministic replay runs must agree bit for bit");
}
