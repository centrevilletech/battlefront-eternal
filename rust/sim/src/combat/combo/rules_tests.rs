use super::*;
use serde_json::{json, Value};

fn attr(key: &str, values: Value) -> Value {
    json!({"key": key, "values": values})
}

fn state(name: &str, children: Vec<Value>) -> Value {
    json!({"name": name, "children": children})
}

fn edge(target: &str, conditions: Vec<Value>) -> Value {
    let mut edge = attr("Transition", json!([target]));
    if !conditions.is_empty() {
        edge["children"] = json!([{"key":"If", "children":conditions}]);
    }
    edge
}

fn wait(name: &str) -> Value {
    state(name, vec![attr("Duration", json!([0]))])
}

fn load(states: Vec<Value>) -> Result<ComboMachine, ComboLoadError> {
    ComboMachine::from_json_value(
        &json!({"nameHash":"a1600321","states":states}),
        &WeaponMeleeSpec::default(),
        COMBO_ANIM_FPS,
        |name| match name {
            "fold" => Some(11.0 / 30.0),
            "reel" => Some(0.7),
            "unspool" => Some(16.75),
            _ => None,
        },
    )
}

fn tick(m: &ComboMachine, s: &mut ComboState, i: ComboInput) -> StepOutput {
    let tuning = super::tests::test_move_tuning();
    life_tick(m, s, i, &mut move2::State::new(&tuning), &tuning)
}

#[test]
fn finite_clips_return_to_the_named_neutral_state() {
    let m = load(vec![
        state("CATCH_SABRE", vec![attr("Animation", json!(["fold"]))]),
        wait("IDLE"),
        state("coil", vec![attr("Animation", json!(["unspool"]))]),
    ])
    .unwrap();
    let caught = m.state(m.catch_state.unwrap());
    assert_eq!(
        m.animation(caught.animation.unwrap()).duration_secs,
        11.0 / 30.0
    );
    assert_eq!(m.state_end_time(caught), Some(0.36));
    assert_eq!(m.state_end_time(m.state(StateId(2))), Some(10.22));
    let mut s = ComboState::new(&m);
    assert!(s.enter_catch(&m).is_some());
    for _ in 0..22 {
        tick(&m, &mut s, ComboInput::default());
    }
    assert!(s.is_idle(&m));
}

#[test]
fn seconds_are_encoded_in_single_precision_and_names_are_unambiguous() {
    for secs in [0.47_f32, 0.59, 0.83] {
        let m = load(vec![state(
            "IDLE",
            vec![attr("Duration", json!([secs as f64]))],
        )])
        .unwrap();
        let cs = (secs * 100.0) as u16;
        assert_eq!(
            m.state_end_time(m.state(StateId(0))),
            Some(f64::from(cs) / 100.0)
        );
    }
    assert!(load(vec![wait("IDLE"), wait("bobbin"), wait("BOBBIN")]).is_err());
}

#[test]
fn eligible_transitions_compare_their_times_in_authored_order() {
    let early = edge("chalk", vec![attr("TimeStart", json!([0.002]))]);
    let late = edge("ink", vec![attr("TimeStart", json!([0.013]))]);
    for order in [[late.clone(), early.clone()], [early, late]] {
        let mut start = wait("IDLE");
        start["children"].as_array_mut().unwrap().extend(order);
        let m = load(vec![start, wait("chalk"), wait("ink")]).unwrap();
        let mut s = ComboState::new(&m);
        tick(&m, &mut s, ComboInput::default());
        assert_eq!(m.state_name(s.state), "chalk");
    }
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0.07])),
                edge("dusk", vec![]),
                edge("dawn", vec![attr("TimeStart", json!([0.07]))]),
            ],
        ),
        wait("dusk"),
        wait("dawn"),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    for _ in 0..5 {
        tick(&m, &mut s, ComboInput::default());
    }
    assert_eq!(m.state_name(s.state), "dusk");
}

#[test]
fn an_entry_lock_and_a_delayed_button_take_effect_on_their_own_ticks() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("wick", vec![attr("Button", json!(["Reload", "Press"]))]),
            ],
        ),
        state("wick", vec![edge("shuttle", vec![])]),
        state(
            "shuttle",
            vec![
                attr("Duration", json!([0])),
                attr("InputLock", json!(["Thrust", "Jump"])),
                edge(
                    "spindle",
                    vec![
                        attr("Break", json!([0.37])),
                        attr("Button", json!(["Crouch", "Press"])),
                    ],
                ),
            ],
        ),
        wait("spindle"),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    let out = tick(&m, &mut s, ComboInput::default().press(ComboButton::Reload));
    assert_eq!(m.state_name(s.state), "shuttle");
    assert_eq!(
        out.input_lock,
        InputLockMask::THRUST
            .union(InputLockMask::STRAFE)
            .union(InputLockMask::JUMP)
    );
    for n in 1..=23 {
        let input = if n == 17 {
            ComboInput::default().press(ComboButton::Crouch)
        } else {
            Default::default()
        };
        tick(&m, &mut s, input);
        assert_eq!(
            m.state_name(s.state),
            if n < 23 { "shuttle" } else { "spindle" }
        );
    }
}

#[test]
fn a_closed_window_cannot_supply_input_when_a_break_opens() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge(
                    "reed",
                    vec![
                        attr("Break", json!([0.31])),
                        attr("TimeEnd", json!([0.12])),
                        attr("Button", json!(["Fire", "Press"])),
                    ],
                ),
            ],
        ),
        wait("reed"),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    for n in 0..26 {
        let i = if n == 3 {
            ComboInput::default().press(ComboButton::Fire)
        } else {
            Default::default()
        };
        tick(&m, &mut s, i);
    }
    assert!(s.is_idle(&m));
}

#[test]
fn a_hold_and_its_release_leave_the_original_press_in_history() {
    let m = load(vec![wait("IDLE")]).unwrap();
    let mut s = ComboState::new(&m);
    for _ in 0..25 {
        tick(&m, &mut s, ComboInput::default().hold(ComboButton::Reload));
    }
    tick(&m, &mut s, ComboInput::default());
    let history = &s.button_history;
    for mode in [ButtonMode::Press, ButtonMode::Hold, ButtonMode::HoldRelease] {
        assert!(history
            .earliest_match(ComboButton::Reload, mode, 0.0, history.now, history.now)
            .is_some());
    }
    let mut corrected = ComboState::new(&m);
    let mut cp = s.checkpoint();
    corrected.try_restore(&m, cp).unwrap();
    cp.button_history = ButtonEventHistory::from_parts(
        0.0,
        0,
        0,
        &[ButtonEvent {
            button: ComboButton::Fire,
            flags: 0,
            event_time: 0.2,
        }],
    )
    .unwrap();
    assert!(corrected.try_restore(&m, cp).is_err());
    assert_eq!(corrected, s);
}

#[test]
fn dedicated_block_keeps_short_stock_taps_and_releases_held_guard() {
    let button = attr("Button", json!(["FireSecondary", "Press"]));
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("shutter", vec![button.clone()]),
            ],
        ),
        state(
            "shutter",
            vec![
                attr("Duration", json!([0])),
                json!({"key":"Deflect","children":[{"key":"DeflectAngle","values":[-61,48]}]}),
                edge("IDLE", vec![attr("Break", json!([0.28])), button]),
            ],
        ),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    tick(
        &m,
        &mut s,
        ComboInput::default().with_block_button(true, false),
    );
    for _ in 0..9 {
        tick(&m, &mut s, ComboInput::default());
    }
    for n in 0..9 {
        tick(
            &m,
            &mut s,
            ComboInput::default().with_block_button(n == 0, true),
        );
    }
    assert!(s.is_idle(&m));
    tick(
        &m,
        &mut s,
        ComboInput::default().with_hold_to_block(true, false),
    );
    assert!(s.deflecting);
    tick(
        &m,
        &mut s,
        ComboInput::default().with_hold_to_block(false, false),
    );
    assert!(s.is_idle(&m));
}

fn life_tick(
    machine: &ComboMachine,
    combo: &mut ComboState,
    input: ComboInput,
    life: &mut move2::State,
    tuning: &move2::Tuning,
) -> StepOutput {
    step(
        GameFamily::Swbf2,
        machine,
        combo,
        &input,
        &super::tests::stand_body(),
        life,
        tuning,
        false,
        COMBO_FIXED_DT,
    )
}

#[test]
fn matched_cost_uses_the_same_overdraw_and_rearm_as_locomotion() {
    let mut cut = edge("notch", vec![attr("Button", json!(["Reload", "Press"]))]);
    cut["children"]
        .as_array_mut()
        .unwrap()
        .push(attr("EnergyCost", json!([6.75])));
    let mut repeat = cut.clone();
    repeat["values"] = json!(["IDLE"]);
    let m = load(vec![
        state("IDLE", vec![attr("Duration", json!([0])), cut]),
        state("notch", vec![attr("Duration", json!([0])), repeat]),
    ])
    .unwrap();
    let mut tuning = super::tests::test_move_tuning();
    tuning.energy_floor = -1.5;
    let input = ComboInput::default().press(ComboButton::Reload);
    for initial in [11.0, 6.75, 2.0, 0.0] {
        let mut life = move2::State::new(&tuning);
        life.energy = initial;
        let mut baseline = life;
        let mut combo = ComboState::new(&m);
        for attempt in 0..3 {
            let allowed = baseline.charge(6.75, &tuning);
            let out = life_tick(&m, &mut combo, input, &mut life, &tuning);
            assert_eq!(
                out.entered_state.is_some(),
                allowed,
                "balance {initial}, press {attempt}"
            );
            assert_eq!(
                (life.energy, life.exhausted),
                (baseline.energy, baseline.exhausted)
            );
            assert_eq!(
                out.transition_energy_spent,
                if allowed { 6.75 } else { 0.0 }
            );
            life_tick(&m, &mut combo, Default::default(), &mut life, &tuning);
        }
        baseline.recover(tuning.energy_capacity, &tuning);
        life.recover(tuning.energy_capacity, &tuning);
        assert!(baseline.charge(6.75, &tuning));
        assert!(life_tick(&m, &mut combo, input, &mut life, &tuning)
            .entered_state
            .is_some());
        assert_eq!(
            (life.energy, life.exhausted),
            (baseline.energy, baseline.exhausted)
        );
    }
}

#[test]
fn expiry_uses_the_last_destination_without_buying_an_unmatched_action() {
    let mut last = edge("selvedge", vec![attr("Button", json!(["Crouch", "Press"]))]);
    last["children"]
        .as_array_mut()
        .unwrap()
        .push(attr("EnergyCost", json!([18.5])));
    let m = load(vec![
        wait("selvedge"),
        state("IDLE", vec![attr("Duration", json!([0.04])), last]),
    ])
    .unwrap();
    let tuning = super::tests::test_move_tuning();
    for (initial, exhausted) in [(46.0, false), (10.0, false), (2.0, true)] {
        let mut life = move2::State::new(&tuning);
        life.energy = initial;
        life.exhausted = exhausted;
        let mut combo = ComboState::new(&m);
        for _ in 0..3 {
            let out = life_tick(&m, &mut combo, Default::default(), &mut life, &tuning);
            assert_eq!(out.transition_energy_spent, 0.0);
        }
        assert_eq!(m.state_name(combo.state), "selvedge");
        assert_eq!((life.energy, life.exhausted), (initial, exhausted));
    }
}

#[test]
fn finite_interception_finishes_with_hold_controls_and_with_exhausted_stamina() {
    let deflect = json!({"key":"Deflect","children":[{"key":"DeflectAngle","values":[-44,37]}]});
    let tuning = super::tests::test_move_tuning();
    for (rate, hold_option) in [(0.0, true), (-28.5, false), (-28.5, true)] {
        let m = load(vec![
            state(
                "hem",
                vec![
                    attr("Duration", json!([0.43])),
                    attr("EnergyRestoreRate", json!([rate])),
                    deflect.clone(),
                    contact(json!([0.38, 0.40]), "sel"),
                    edge("IDLE", vec![]),
                ],
            ),
            state(
                "IDLE",
                vec![
                    attr("Duration", json!([0])),
                    edge("hem", vec![attr("Button", json!(["Fire", "Press"]))]),
                ],
            ),
        ])
        .unwrap();
        let mut life = move2::State::new(&tuning);
        life.energy = 3.0;
        let mut combo = ComboState::new(&m);
        let controls = ComboInput {
            hold_to_block: hold_option,
            ..Default::default()
        };
        let mut out = life_tick(
            &m,
            &mut combo,
            controls.press(ComboButton::Fire),
            &mut life,
            &tuning,
        );
        let mut reached_window = false;
        for _ in 0..26 {
            assert!(out.deflect.is_some());
            assert!(!combo.is_guarding(&m));
            out = life_tick(&m, &mut combo, controls, &mut life, &tuning);
            reached_window |= !out.active_attacks.is_empty();
        }
        assert!(combo.is_idle(&m) && reached_window);
        assert_eq!(life.exhausted, rate < 0.0);
    }
}

#[test]
fn latched_stamina_controls_guard_through_its_authored_energy_predicates() {
    let button = attr("Button", json!(["FireSecondary", "Press"]));
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("porch", vec![button, attr("Energy", json!([">", 1.25]))]),
            ],
        ),
        state(
            "porch",
            vec![
                attr("Duration", json!([0])),
                json!({"key":"Deflect","children":[{"key":"DeflectAngle","values":[-38,67]}]}),
                edge("IDLE", vec![attr("Energy", json!(["<=", 0]))]),
            ],
        ),
    ])
    .unwrap();
    let tuning = super::tests::test_move_tuning();
    let mut combo = ComboState::new(&m);
    let mut life = move2::State::new(&tuning);
    let press = ComboInput::default().with_block_button(true, false);
    assert!(life_tick(&m, &mut combo, press, &mut life, &tuning)
        .deflect
        .is_some());
    assert!(life.charge(tuning.energy_capacity + 2.0, &tuning));
    life.recover(3.75, &tuning);
    assert!(life.exhausted);
    assert!(
        life_tick(&m, &mut combo, Default::default(), &mut life, &tuning)
            .deflect
            .is_none()
    );
    assert!(life_tick(&m, &mut combo, press, &mut life, &tuning)
        .deflect
        .is_none());
}

#[test]
fn paid_entries_use_the_movers_latch_and_retry_atomically() {
    use crate::collision::spheres::{GatherCache, Gathered};
    use crate::movement::scene::GroundCache;
    use crate::movement::soldier::{Posture, PART_CACHES};
    let mut select = edge("tilt", vec![attr("Button", json!(["Sprint", "Press"]))]);
    select["children"]
        .as_array_mut()
        .unwrap()
        .push(attr("EnergyCost", json!([3.75])));
    let m = load(vec![
        state("IDLE", vec![attr("Duration", json!([0])), select]),
        state(
            "tilt",
            vec![
                attr("Duration", json!([0.08])),
                attr("Posture", json!(["Stand"])),
                attr("InputLock", json!(["All"])),
                attr("AlignedToThrust", json!(["Initial"])),
                json!({"key":"AnimatedMove","children":[{"key":"VelocityZ","values":[8.25]}]}),
                edge("nest", vec![]),
            ],
        ),
        state(
            "nest",
            vec![
                attr("Duration", json!([0])),
                attr("Posture", json!(["Crouch"])),
                attr("InputLock", json!(["All"])),
            ],
        ),
    ])
    .unwrap();
    let mut tuning = super::tests::test_move_tuning();
    let (world, mut col) = super::tests::flat_move_world();
    let mut mover = Mover::for_family(world as usize, GameFamily::Swbf2);
    mover.grounded = true;
    mover.posture = Posture::Crouch;
    let mut life = move2::State::new(&tuning);
    let mut s = ComboState::new(&m);
    let mut gathered = Gathered::default();
    let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
    let mut ground = GroundCache::default();
    let mut scene = Scene {
        s: &mut col,
        world: world as usize,
        flags: &[],
        gathered: &mut gathered,
        caches: &mut caches,
        ground_cache: &mut ground,
    };
    let mut run = |s: &mut ComboState,
                   life: &mut move2::State,
                   mover: &mut Mover,
                   tuning: &move2::Tuning,
                   i| {
        step_soldier_combo_and_move(
            &m,
            s,
            &i,
            mover,
            life,
            tuning,
            &MoveInput::default(),
            Default::default(),
            &mut scene,
            COMBO_FIXED_DT,
        )
    };
    let press = ComboInput::default()
        .press(ComboButton::Sprint)
        .stick(0.0, 1.0);
    assert!(life.charge(tuning.energy_capacity + 1.0, &tuning));
    life.recover(5.0 - life.energy, &tuning);
    assert!(life.exhausted);
    run(&mut s, &mut life, &mut mover, &tuning, press).unwrap();
    assert!(s.is_idle(&m));
    assert!(life.energy >= 5.0);
    life.recover(tuning.energy_capacity, &tuning);
    run(
        &mut s,
        &mut life,
        &mut mover,
        &tuning,
        ComboInput::default(),
    )
    .unwrap();
    assert!(
        s.is_idle(&m),
        "recovery cannot commit an earlier rejected action"
    );
    let before = (s, life.energy, life.exhausted, mover.position.arr());
    let ok = tuning;
    tuning.energy_capacity = -1.0;
    assert!(run(&mut s, &mut life, &mut mover, &tuning, press).is_err());
    assert_eq!(
        (s, life.energy, life.exhausted, mover.position.arr()),
        before
    );
    let (o, _) = run(&mut s, &mut life, &mut mover, &ok, press).unwrap();
    assert_eq!(o.transition_energy_spent, 3.75);
    assert_eq!(mover.posture, Posture::Stand);
    assert!(
        mover.position.x < 0.0
            && mover.position.z > 0.0
            && (mover.position.x + mover.position.z).abs() < 1e-9
    );
    let facing = mover.aim_yaw;
    for _ in 0..6 {
        run(
            &mut s,
            &mut life,
            &mut mover,
            &ok,
            ComboInput::default().stick(0.0, 1.0),
        )
        .unwrap();
        assert_eq!(mover.aim_yaw, facing);
    }
    assert_eq!(mover.posture, Posture::Crouch);
}

fn contact(window: Value, group: &str) -> Value {
    json!({"key":"Attack","children":[{"key":"AttackId","values":[group]},
        {"key":"DamageTime","values":window},{"key":"Damage","values":[19.25]}]})
}

#[test]
fn load_checks_fixed_step_windows_and_retained_hit_lists() {
    let bank = |waves: &[(usize, f64)], shared: bool| {
        let mut children = vec![attr("Duration", json!([0.75]))];
        for &(count, point) in waves {
            for _ in 0..count {
                let name = if shared {
                    "seam".into()
                } else {
                    format!("spool{}", children.len())
                };
                children.push(contact(json!([point, point]), &name));
            }
        }
        state("IDLE", children)
    };
    let crowded = bank(&[(5, 0.02), (4, 0.03)], true);
    let err = load(vec![crowded.clone()]).unwrap_err();
    assert!(err.to_string().contains("8 attack windows"));
    let mut disabled = crowded;
    disabled["children"]
        .as_array_mut()
        .unwrap()
        .push(attr("TurnOffLightsaber", json!([])));
    assert!(load(vec![disabled]).is_ok());
    assert!(load(vec![bank(&[(8, 0.74), (5, 0.75)], false)]).is_ok());

    let m = load(vec![bank(&[(8, 0.01), (1, 0.02)], true)]).unwrap();
    let tuning = super::tests::test_move_tuning();
    let out = step(
        GameFamily::Swbf2,
        &m,
        &mut ComboState::new(&m),
        &Default::default(),
        &super::tests::stand_body(),
        &mut move2::State::new(&tuning),
        &tuning,
        false,
        2.0 * COMBO_FIXED_DT,
    );
    assert_eq!(
        out.first_omitted_attack,
        Some(m.state(m.idle_state).attacks[8].attack_key)
    );

    let separate_steps = [(8, 0.01), (8, 0.02), (1, 0.19)];
    let err = load(vec![bank(&separate_steps, false)]).unwrap_err();
    assert!(err.to_string().contains("16 live attack hit lists"));
    assert!(load(vec![bank(&separate_steps, true)]).is_ok());
    assert!(load(vec![bank(&[(8, 0.01), (8, 0.02), (1, 0.32)], false)]).is_ok());
}

#[test]
fn overflow_from_prior_states_identifies_the_first_missing_record() {
    let wave = |name: &str, next: &str, n: usize| {
        let mut children = vec![attr("Duration", json!([0.01])), edge(next, vec![])];
        children.extend((0..n).map(|i| contact(json!([0.0, 0.0]), &format!("{name}{i}"))));
        state(name, children)
    };
    let m = load(vec![
        wave("IDLE", "warp", 8),
        wave("warp", "weft", 8),
        wave("weft", "selvedge", 1),
        wait("selvedge"),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    for _ in 0..2 {
        let out = tick(&m, &mut s, Default::default());
        assert_eq!(out.active_attacks.as_slice().len(), 8);
        assert_eq!(out.first_omitted_attack, None);
    }
    let attack = m.state(s.state).attacks[0];
    let out = tick(&m, &mut s, Default::default());
    assert_eq!(s.hit_history.len(), 16);
    assert!(out.active_attacks.is_empty());
    assert_eq!(out.first_omitted_attack, Some(attack.attack_key));
}

#[test]
fn contacts_clip_the_outgoing_interval_and_reactivate_after_expiry() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0.8])),
                attr("Animation", json!(["unspool"])),
                contact(json!([0.64, 0.95, "FromAnim"]), "thread"),
                contact(json!([0.10, 0.26, "FromAnim"]), "thread"),
                contact(json!([0.2, 0.2]), "needle"),
                contact(json!([0.8, 0.8]), "outside"),
                contact(json!([0.79, 0.81]), "rim"),
                edge("tray", vec![]),
            ],
        ),
        wait("tray"),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    let mut hits = Vec::new();
    let mut points = Vec::new();
    let mut last = None;
    for advance in 1..=48 {
        let out = tick(&m, &mut s, Default::default());
        for active in out.active_attacks.as_slice() {
            match active.attack_index {
                0 | 1 => {
                    if s.hit_history
                        .record(active.attack_key, active.shared_id, 0.0, TargetId::Life(17))
                    {
                        hits.push((advance, active.attack_index, active.sample_start));
                    }
                }
                2 => {
                    points.push(advance);
                    assert_eq!(active.sample_start, active.sample_end);
                }
                3 => panic!("point at the deadline has no following interval"),
                4 => last = Some(*active),
                _ => unreachable!(),
            }
        }
    }
    assert_eq!(points, [13]);
    assert_eq!(hits.len(), 2);
    assert_eq!((hits[0].0, hits[1].0), (5, 31));
    assert!((hits[1].2 - 0.512).abs() < 1e-12);
    let last = last.unwrap();
    assert_ne!(last.state_id, s.state);
    assert!((last.sample_start - 0.79).abs() < 1e-12 && (last.sample_end - 0.80).abs() < 1e-12);
    let samples = last.sample(&m, |t| ([t, 0.0, 0.0], [0.0, 1.0, 0.0]));
    assert!(samples
        .as_slice()
        .iter()
        .all(|p| p.position[0] >= 0.79 && p.position[0] <= 0.8));
}

#[test]
fn three_strokes_accept_only_their_own_keys_and_replay_from_mid_stroke() {
    let press = |name| attr("Button", json!([name, "Press"]));
    let m = load(vec![
        state(
            "weft",
            vec![
                attr("Duration", json!([0.08])),
                contact(json!([0.01, 0.05]), "green"),
                edge("selvedge", vec![press("Crouch")]),
                edge("IDLE", vec![]),
            ],
        ),
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("warp", vec![press("Fire")]),
            ],
        ),
        state(
            "selvedge",
            vec![
                attr("Duration", json!([0.11])),
                contact(json!([0.03, 0.09]), "red"),
            ],
        ),
        state(
            "warp",
            vec![
                attr("Duration", json!([0.12])),
                contact(json!([0.01, 0.07]), "blue"),
                edge(
                    "weft",
                    vec![attr("TimeStart", json!([0.04])), press("Jump")],
                ),
                edge("IDLE", vec![]),
            ],
        ),
    ])
    .unwrap();
    for (jump, expected) in [
        (
            Some(4),
            vec![(0, "warp"), (8, "weft"), (13, "selvedge"), (20, "IDLE")],
        ),
        (Some(1), vec![(0, "warp"), (8, "IDLE")]),
        (Some(10), vec![(0, "warp"), (8, "IDLE")]),
        (None, vec![(0, "warp"), (8, "IDLE")]),
    ] {
        let mut s = ComboState::new(&m);
        let mut timeline = Vec::new();
        for n in 0..24 {
            let input = if n == 0 {
                ComboInput::default().press(ComboButton::Fire)
            } else if Some(n) == jump {
                ComboInput::default().press(ComboButton::Jump)
            } else if n == 11 {
                ComboInput::default().press(ComboButton::Crouch)
            } else {
                Default::default()
            };
            let mut prediction = ComboState::new(&m);
            prediction.try_restore(&m, s.checkpoint()).unwrap();
            let out = tick(&m, &mut s, input);
            assert_eq!(out, tick(&m, &mut prediction, input));
            assert_eq!(s, prediction);
            if out.entered_state.is_some() {
                timeline.push((n, m.state_name(s.state)));
            }
        }
        assert_eq!(timeline, expected);
    }
}

#[test]
fn a_split_pose_keeps_each_layer_and_offhand_aim() {
    let m=ComboMachine::from_json_value(&json!({"nameHash":"df07b018",
        "offhandAnimations":[{"values":["weave","Fire"],"children":[{"key":"AimType","values":["FullBody"]}]}],
        "states":[{"name":"IDLE","children":[
            {"key":"Animation","values":["ribbon_upper"],"children":[{"key":"BlendInTime","values":[0.23]}]},
            {"key":"Animation","values":["ribbon_lower"],"children":[{"key":"BlendOutTime","values":[0.17]}]},
            {"key":"Duration","values":[0]}
        ]}]}),&WeaponMeleeSpec::default(),COMBO_ANIM_FPS,|_|Some(0.53)).unwrap();
    let pose = m.state(m.idle_state);
    assert_ne!(pose.animation, pose.lower_animation);
    assert_eq!(pose.anim_layer.as_ref().unwrap().blend_in, 0.23);
    assert_eq!(pose.lower_anim_layer.as_ref().unwrap().blend_out, 0.17);
    assert_eq!(m.offhand_animations[0].aim_type, Some(AimType::FullBody));
}

#[test]
fn corrections_reject_duplicate_targets_groups_and_inconsistent_button_levels() {
    let m = load(vec![state(
        "IDLE",
        vec![
            attr("Duration", json!([0])),
            contact(json!([0.12, 0.14]), "silver"),
            contact(json!([0.28, 0.37]), "silver"),
        ],
    )])
    .unwrap();
    let attack = m.state(m.idle_state).attacks[0];
    let record = |key, targets: &[TargetId]| {
        HitListRecord::from_parts(key, attack.shared_id, -0.16, targets).unwrap()
    };
    let mut state = ComboState::new(&m);
    let base = state;
    for lists in [
        vec![record(0, &[TargetId::Life(4), TargetId::Life(4)])],
        vec![record(0, &[TargetId::Life(6)]), record(1, &[TargetId::Life(7)])],
    ] {
        let mut cp = base.checkpoint();
        cp.hit_history = HitHistory::from_lists(&lists).unwrap();
        assert!(state.try_restore(&m, cp).is_err());
        assert_eq!(state, base);
    }
    let mut cp = base.checkpoint();
    cp.hit_history = HitHistory::from_lists(&[record(0, &[TargetId::Life(4)])]).unwrap();
    state.try_restore(&m, cp).unwrap();
    let mut cp = base.checkpoint();
    cp.button_history = ButtonEventHistory::from_parts(
        0.,
        0,
        0,
        &[ButtonEvent {
            button: ComboButton::Jump,
            flags: 0,
            event_time: 0.,
        }],
    )
    .unwrap();
    assert!(state.try_restore(&m, cp).is_err());
    let mut thrown = SaberThrowState::new();
    for lists in [
        vec![HitListRecord::from_parts(0, 0, 0., &[TargetId::Life(9), TargetId::Life(9)]).unwrap()],
        vec![HitListRecord::new(1, 0, 0.), HitListRecord::new(1, 0, 0.)],
    ] {
        let mut cp = thrown.checkpoint();
        cp.hit_history = ThrowHitHistory::from_lists(&lists).unwrap();
        assert!(thrown.try_restore(cp).is_err());
        assert_eq!(thrown, SaberThrowState::new());
    }
}

#[test]
fn bridge_and_catch_entries_dispatch_their_own_sounds() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("relay", vec![attr("Button", json!(["Fire", "Press"]))]),
            ],
        ),
        state(
            "relay",
            vec![attr("Sound", json!(["clink"])), edge("coil", vec![])],
        ),
        state(
            "coil",
            vec![
                attr("Duration", json!([0.21])),
                attr("Sound", json!(["hum"])),
                attr("Sound", json!(["plink", 0.01])),
            ],
        ),
        state(
            "CATCH_SABRE",
            vec![
                attr("Animation", json!(["fold"])),
                attr("Sound", json!(["snap"])),
            ],
        ),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    let out = tick(&m, &mut s, ComboInput::default().press(ComboButton::Fire));
    let entries = out.entries.as_slice();
    assert_eq!(
        entries
            .iter()
            .map(|e| m.state_name(e.state))
            .collect::<Vec<_>>(),
        ["relay", "coil"]
    );
    let names = entries
        .iter()
        .flat_map(|e| e.sounds(&m))
        .map(|(_, id)| m.sound_name(id))
        .collect::<Vec<_>>();
    assert_eq!(names, ["clink", "hum"]);
    let timed = tick(&m, &mut s, Default::default()).timed_cues.unwrap();
    assert_eq!(
        timed
            .sounds(&m)
            .map(|(i, id)| (i, m.sound_name(id)))
            .collect::<Vec<_>>(),
        [(1, "plink")]
    );
    let catch = s.enter_catch(&m).unwrap();
    assert_eq!(
        catch
            .sounds(&m)
            .map(|(_, id)| m.sound_name(id))
            .collect::<Vec<_>>(),
        ["snap"]
    );
}

#[test]
fn pose_refinement_keeps_the_last_admissible_half_step() {
    let samples = build_damage_samples(0.0, COMBO_FIXED_DT, 2.0, |time| {
        let (sin, cos) = v8math::sin_cos(std::f64::consts::PI * time / COMBO_FIXED_DT);
        ([0., 0., 0.], [sin, 0., cos])
    });
    assert_eq!(samples.as_slice()[1].time, COMBO_FIXED_DT / 4.0);
    assert_eq!(samples.as_slice().last().unwrap().time, COMBO_FIXED_DT);
}

#[test]
fn aligned_to_thrust_and_guard_locks_preserve_camera_aim_yaw() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("ward", vec![attr("Button", json!(["FireSecondary", "Press"]))]),
                edge("sweep", vec![attr("Button", json!(["Fire", "Press"]))]),
                edge("stride", vec![attr("Button", json!(["Reload", "Press"]))]),
            ],
        ),
        state(
            "ward",
            vec![
                attr("Duration", json!([0])),
                attr("Posture", json!(["Stand"])),
                attr("InputLock", json!([0.0, "All", "!Thrust"])),
                json!({"key":"Deflect","children":[{"key":"DeflectAngle","values":[-90.0, 90.0]}]}),
                edge("sweep", vec![attr("Button", json!(["Fire", "Press"]))]),
                edge("IDLE", vec![attr("Button", json!(["FireSecondary", "Press"]))]),
            ],
        ),
        state(
            "sweep",
            vec![
                attr("Duration", json!([0.20])),
                attr("Posture", json!(["Stand"])),
                attr("InputLock", json!(["All", "!Thrust"])),
                attr("AlignedToThrust", json!([])),
                json!({"key":"AnimatedMove","children":[{"key":"VelocityZ","values":[7.25]}]}),
                edge("IDLE", vec![]),
            ],
        ),
        state(
            "stride",
            vec![
                attr("Duration", json!([0.15])),
                attr("Posture", json!(["Stand"])),
                attr("InputLock", json!(["All", "!Thrust"])),
                attr("AlignedToThrust", json!(["Initial"])),
                json!({"key":"AnimatedMove","children":[{"key":"VelocityZ","values":[6.5]}]}),
                edge("IDLE", vec![]),
            ],
        ),
    ])
    .unwrap();

    use crate::collision::spheres::{GatherCache, Gathered};
    use crate::movement::scene::GroundCache;
    use crate::movement::soldier::{Posture, PART_CACHES};
    let tuning = super::tests::test_move_tuning();
    let (world, mut col) = super::tests::flat_move_world();
    let mut mover = Mover::for_family(world as usize, GameFamily::Swbf2);
    mover.grounded = true;
    mover.posture = Posture::Stand;
    mover.aim_yaw = 0.42;
    let mut life = move2::State::new(&tuning);
    let mut s = ComboState::new(&m);
    let mut gathered = Gathered::default();
    let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
    let mut ground = GroundCache::default();
    let mut scene = Scene {
        s: &mut col,
        world: world as usize,
        flags: &[],
        gathered: &mut gathered,
        caches: &mut caches,
        ground_cache: &mut ground,
    };

    // 1. Entering guard with InputLock(0.0, All, !Thrust) keeps turning unlocked.
    let (out, _) = step_soldier_combo_and_move(
        &m,
        &mut s,
        &ComboInput::default().with_block_button(true, true).stick(0.0, 1.0),
        &mut mover,
        &mut life,
        &tuning,
        &MoveInput { right: 1.0, turn: 0.5, ..MoveInput::default() },
        Default::default(),
        &mut scene,
        COMBO_FIXED_DT,
    )
    .unwrap();
    assert!(!out.input_lock.contains(InputLockMask::TURN));
    assert!(mover.aim_yaw < 0.42, "turning must remain active while guarding");

    // 2. Attacking out of guard while strafing right must never spin aim_yaw,
    // while AnimatedMove still moves laterally.
    let yaw_before_attack = mover.aim_yaw;
    let start_pos = mover.position;
    let (_, _) = step_soldier_combo_and_move(
        &m,
        &mut s,
        &ComboInput::default().press(ComboButton::Fire).stick(0.0, 1.0),
        &mut mover,
        &mut life,
        &tuning,
        &MoveInput { right: 1.0, ..MoveInput::default() },
        Default::default(),
        &mut scene,
        COMBO_FIXED_DT,
    )
    .unwrap();
    assert_eq!(mover.aim_yaw, yaw_before_attack);
    for _ in 0..10 {
        step_soldier_combo_and_move(
            &m,
            &mut s,
            &ComboInput::default().stick(0.0, 1.0),
            &mut mover,
            &mut life,
            &tuning,
            &MoveInput { right: 1.0, ..MoveInput::default() },
            Default::default(),
            &mut scene,
            COMBO_FIXED_DT,
        )
        .unwrap();
        assert_eq!(mover.aim_yaw, yaw_before_attack);
    }
    let dx = mover.position.x - start_pos.x;
    let dz = mover.position.z - start_pos.z;
    assert!(dx * dx + dz * dz > 0.01, "AlignedToThrust lunge should still displace");

    // Return to IDLE, then attack holding back (S): must never flip 180 deg.
    for _ in 0..5 {
        step_soldier_combo_and_move(
            &m,
            &mut s,
            &ComboInput::default(),
            &mut mover,
            &mut life,
            &tuning,
            &MoveInput::default(),
            Default::default(),
            &mut scene,
            COMBO_FIXED_DT,
        )
        .unwrap();
    }
    assert!(s.is_idle(&m));
    step_soldier_combo_and_move(
        &m,
        &mut s,
        &ComboInput::default().press(ComboButton::Fire).stick(-1.0, 0.0),
        &mut mover,
        &mut life,
        &tuning,
        &MoveInput { forward: -1.0, ..MoveInput::default() },
        Default::default(),
        &mut scene,
        COMBO_FIXED_DT,
    )
    .unwrap();
    assert_eq!(mover.aim_yaw, yaw_before_attack);
    for _ in 0..10 {
        step_soldier_combo_and_move(
            &m,
            &mut s,
            &ComboInput::default().stick(-1.0, 0.0),
            &mut mover,
            &mut life,
            &tuning,
            &MoveInput { forward: -1.0, ..MoveInput::default() },
            Default::default(),
            &mut scene,
            COMBO_FIXED_DT,
        )
        .unwrap();
        assert_eq!(mover.aim_yaw, yaw_before_attack);
    }

    // 3. AlignedToThrust("Initial") on diagonal stick must also preserve aim_yaw.
    for _ in 0..5 {
        step_soldier_combo_and_move(
            &m,
            &mut s,
            &ComboInput::default(),
            &mut mover,
            &mut life,
            &tuning,
            &MoveInput::default(),
            Default::default(),
            &mut scene,
            COMBO_FIXED_DT,
        )
        .unwrap();
    }
    assert!(s.is_idle(&m));
    step_soldier_combo_and_move(
        &m,
        &mut s,
        &ComboInput::default().press(ComboButton::Reload).stick(0.7, -0.7),
        &mut mover,
        &mut life,
        &tuning,
        &MoveInput { forward: 0.7, right: -0.7, ..MoveInput::default() },
        Default::default(),
        &mut scene,
        COMBO_FIXED_DT,
    )
    .unwrap();
    assert_eq!(mover.aim_yaw, yaw_before_attack);
}

#[test]
fn hard_landing_lock_blocks_primary_secondary_and_reload_until_expired() {
    use crate::collision::spheres::{GatherCache, Gathered};
    use crate::movement::scene::GroundCache;
    use crate::movement::soldier::{Air, V3 as MoveV3, PART_CACHES};
    let press = |name| attr("Button", json!([name, "Press"]));
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("slash", vec![attr("Posture", json!(["Stand"])), press("Fire")]),
                edge("guard", vec![attr("Posture", json!(["Stand"])), press("FireSecondary")]),
                edge("vent", vec![attr("Posture", json!(["Stand"])), press("Reload")]),
            ],
        ),
        state("slash", vec![attr("Duration", json!([0.25])), edge("IDLE", vec![])]),
        state("guard", vec![attr("Duration", json!([0.25])), edge("IDLE", vec![])]),
        state("vent", vec![attr("Duration", json!([0.25])), edge("IDLE", vec![])]),
    ])
    .unwrap();
    let tuning = super::tests::test_move_tuning();
    let (world, mut col) = super::tests::flat_move_world();
    let mut mover = Mover::for_family(world as usize, GameFamily::Swbf2);
    mover.air = Air::Fall;
    mover.grounded = false;
    mover.state_time = 1.6;
    mover.position.y = 0.05;
    mover.settled = mover.position;
    mover.velocity = MoveV3::new(0.0, -8.0, 0.0);
    let mut life = move2::State::new(&tuning);
    let mut s = ComboState::new(&m);
    let mut gathered = Gathered::default();
    let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
    let mut ground = GroundCache::default();
    let mut scene = Scene {
        s: &mut col,
        world: world as usize,
        flags: &[],
        gathered: &mut gathered,
        caches: &mut caches,
        ground_cache: &mut ground,
    };
    let mut run = |s: &mut ComboState,
                   life: &mut move2::State,
                   mover: &mut Mover,
                   input: ComboInput| {
        step_soldier_combo_and_move(
            &m,
            s,
            &input,
            mover,
            life,
            &tuning,
            &MoveInput::default(),
            Default::default(),
            &mut scene,
            COMBO_FIXED_DT,
        )
        .unwrap()
    };

    // Touchdown tick sets the 0.5 s hard-landing lock and denies fire permission immediately.
    let (_, perm) = run(&mut s, &mut life, &mut mover, ComboInput::default().press(ComboButton::Fire));
    assert!(!perm);
    assert!(mover.lock > 0.49);
    assert!(s.is_idle(&m));

    // Across all 30 locked ticks, Fire, FireSecondary and Reload are suppressed.
    for tick in 0..30 {
        let button = match tick % 3 {
            0 => ComboButton::Fire,
            1 => ComboButton::FireSecondary,
            _ => ComboButton::Reload,
        };
        let (_, perm) = run(&mut s, &mut life, &mut mover, ComboInput::default().press(button));
        assert!(!perm, "tick {tick} should still deny fire/reload");
        assert!(s.is_idle(&m), "tick {tick} should block combo entry during hard lock");
    }
    assert_eq!(mover.lock, 0.0);

    // Once the 0.5 s lock expires, Fire immediately starts the melee state.
    let (_, perm) = run(&mut s, &mut life, &mut mover, ComboInput::default().press(ComboButton::Fire));
    assert!(perm);
    assert_eq!(m.state_name(s.state), "slash");
}

#[test]
fn enter_idle_exits_guard_and_preserves_checkpoint_invariants() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge(
                    "guard_stance",
                    vec![attr("Button", json!(["FireSecondary", "Press"]))],
                ),
            ],
        ),
        state(
            "guard_stance",
            vec![
                attr("Animation", json!(["fold"])),
                attr("Duration", json!([0])),
                json!({"key": "Deflect", "children": [{"key": "DeflectAngle", "values": [-61, 48]}]}),
            ],
        ),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    tick(
        &m,
        &mut s,
        ComboInput::default().press(ComboButton::FireSecondary),
    );
    assert!(s.is_guarding(&m));
    assert!(s.deflecting);

    let entry = s.enter_idle(&m);
    assert_eq!(entry.state, m.idle_state);
    assert!(s.is_idle(&m));
    assert!(!s.is_guarding(&m));
    assert!(!s.deflecting);
    assert_eq!(s.blades_off_mask, 0);

    let cp = s.checkpoint();
    let mut restored = ComboState::new(&m);
    assert!(restored.try_restore(&m, cp).is_ok());
    assert_eq!(restored, s);
}

#[test]
fn aligned_to_thrust_lunge_stays_in_forward_hemisphere_within_45_degrees() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("lunge", vec![attr("Button", json!(["Fire", "Press"]))]),
            ],
        ),
        state(
            "lunge",
            vec![
                attr("Duration", json!([0.30])),
                attr("Posture", json!(["Stand"])),
                attr("AlignedToThrust", json!([])),
                json!({"key":"AnimatedMove","children":[{"key":"VelocityZ","values":[8.0]}]}),
                edge("IDLE", vec![]),
            ],
        ),
    ])
    .unwrap();

    let tuning = super::tests::test_move_tuning();
    let (world, mut col) = super::tests::flat_move_world();
    let base_yaw = 0.35;
    let mut lunge_angle = |fwd: f64, right: f64| -> f64 {
        let mut mover = Mover::for_family(world as usize, GameFamily::Swbf2);
        mover.grounded = true;
        mover.posture = crate::movement::soldier::Posture::Stand;
        mover.aim_yaw = base_yaw;
        let mut life = move2::State::new(&tuning);
        let mut s = ComboState::new(&m);
        let mut gathered = crate::collision::spheres::Gathered::default();
        let mut caches = std::array::from_fn::<_, { crate::movement::soldier::PART_CACHES }, _>(|_| {
            crate::collision::spheres::GatherCache::new(0.25)
        });
        let mut ground = crate::movement::scene::GroundCache::default();
        let mut scene = Scene {
            s: &mut col,
            world: world as usize,
            flags: &[],
            gathered: &mut gathered,
            caches: &mut caches,
            ground_cache: &mut ground,
        };
        let (out, _) = step_soldier_combo_and_move(
            &m,
            &mut s,
            &ComboInput::default().press(ComboButton::Fire).stick(fwd, right),
            &mut mover,
            &mut life,
            &tuning,
            &MoveInput { forward: fwd, right, ..MoveInput::default() },
            Default::default(),
            &mut scene,
            COMBO_FIXED_DT,
        )
        .unwrap();
        assert_eq!(mover.aim_yaw, base_yaw);
        let v = out.movement.animated_move.expect("lunge emits AnimatedMove");
        let vel_yaw = v8math::atan2(v[0], v[2]);
        let mut rel = vel_yaw - base_yaw;
        while rel > std::f64::consts::PI {
            rel -= 2.0 * std::f64::consts::PI;
        }
        while rel < -std::f64::consts::PI {
            rel += 2.0 * std::f64::consts::PI;
        }
        rel
    };

    let qtr = std::f64::consts::FRAC_PI_4;
    // Dead zone (< 0.3 magnitude): straight forward.
    assert!(lunge_angle(0.2, 0.1).abs() < 1e-9);
    // Pure W and pure S (mirrored into forward hemisphere): straight forward.
    assert!(lunge_angle(1.0, 0.0).abs() < 1e-9);
    assert!(lunge_angle(-1.0, 0.0).abs() < 1e-9);
    // Pure D clamps to 45 deg forward-right; pure A clamps to 45 deg forward-left.
    assert!((lunge_angle(0.0, 1.0) + qtr).abs() < 1e-9);
    assert!((lunge_angle(0.0, -1.0) - qtr).abs() < 1e-9);
    // Shallow forward diagonal keeps exact stick angle inside +-45 deg.
    let shallow = -v8math::atan2(0.25, 1.0);
    assert!((lunge_angle(1.0, 0.25) - shallow).abs() < 1e-9);
    // Backward diagonals mirror into the forward hemisphere and clamp to +-45 deg.
    assert!((lunge_angle(-1.0, 1.0) - qtr).abs() < 1e-9);
    assert!((lunge_angle(-1.0, -1.0) + qtr).abs() < 1e-9);
}

#[test]
fn thrust_lock_token_covers_the_whole_stick() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("drift", vec![attr("Button", json!(["Fire", "Press"]))]),
                edge("pinned", vec![attr("Button", json!(["Reload", "Press"]))]),
                edge("skid", vec![attr("Button", json!(["Crouch", "Press"]))]),
            ],
        ),
        state(
            "drift",
            vec![attr("Duration", json!([0])), attr("InputLock", json!(["All", "!Thrust"]))],
        ),
        state("pinned", vec![attr("Duration", json!([0])), attr("InputLock", json!(["All"]))]),
        state(
            "skid",
            vec![attr("Duration", json!([0])), attr("InputLock", json!(["All", "!strafe"]))],
        ),
    ])
    .unwrap();

    use crate::collision::spheres::{GatherCache, Gathered};
    use crate::movement::scene::GroundCache;
    use crate::movement::soldier::{Posture, PART_CACHES};
    let tuning = super::tests::test_move_tuning();
    let sideways = |button: ComboButton| {
        let (world, mut col) = super::tests::flat_move_world();
        let mut mover = Mover::for_family(world as usize, GameFamily::Swbf2);
        mover.grounded = true;
        mover.posture = Posture::Stand;
        mover.aim_yaw = 0.0;
        let mut life = move2::State::new(&tuning);
        let mut s = ComboState::new(&m);
        let mut gathered = Gathered::default();
        let mut caches = std::array::from_fn::<_, PART_CACHES, _>(|_| GatherCache::new(0.25));
        let mut ground = GroundCache::default();
        let mut scene = Scene {
            s: &mut col,
            world: world as usize,
            flags: &[],
            gathered: &mut gathered,
            caches: &mut caches,
            ground_cache: &mut ground,
        };
        let start = mover.position;
        let mut lock = InputLockMask::NONE;
        for n in 0..30 {
            let mut input = ComboInput::default().stick(0.0, 1.0);
            if n == 0 {
                input = input.press(button);
            }
            let (out, _) = step_soldier_combo_and_move(
                &m,
                &mut s,
                &input,
                &mut mover,
                &mut life,
                &tuning,
                &MoveInput { right: 1.0, ..MoveInput::default() },
                Default::default(),
                &mut scene,
                COMBO_FIXED_DT,
            )
            .unwrap();
            lock = out.input_lock;
        }
        let travel = (mover.position.x - start.x).hypot(mover.position.z - start.z);
        (m.state_name(s.state).to_string(), lock, travel)
    };

    let (name, lock, travel) = sideways(ComboButton::Fire);
    assert_eq!(name, "drift");
    assert!(!lock.contains(InputLockMask::THRUST) && !lock.contains(InputLockMask::STRAFE));
    assert!(travel > 1.0, "sideways stick must keep moving under !Thrust, moved {travel}");

    for button in [ComboButton::Reload, ComboButton::Crouch] {
        let (name, lock, travel) = sideways(button);
        assert!(name == "pinned" || name == "skid");
        assert!(lock.contains(InputLockMask::THRUST.union(InputLockMask::STRAFE)), "{name}");
        assert!(travel < 0.05, "{name} must hold the body still, moved {travel}");
    }
}

#[test]
fn break_waits_on_the_clock_a_shared_clip_carries() {
    let m = load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("whirl", vec![attr("Button", json!(["Fire", "Press"]))]),
            ],
        ),
        state(
            "whirl",
            vec![
                attr("Animation", json!(["reel"])),
                attr("Duration", json!([0.25])),
                edge("eddy", vec![]),
            ],
        ),
        state(
            "eddy",
            vec![
                attr("Animation", json!(["reel"])),
                edge(
                    "gust",
                    vec![
                        attr("Break", json!([0.35])),
                        attr("TimeStart", json!([0.1])),
                        attr("TimeEnd", json!([0.45])),
                        attr("Button", json!(["Fire", "Press"])),
                    ],
                ),
                edge("IDLE", vec![]),
            ],
        ),
        wait("gust"),
    ])
    .unwrap();
    let mut s = ComboState::new(&m);
    let mut entered = Vec::new();
    for n in 0..60 {
        let input = if n == 0 || n == 18 {
            ComboInput::default().press(ComboButton::Fire)
        } else {
            ComboInput::default()
        };
        let before = s.state;
        tick(&m, &mut s, input);
        if s.state != before {
            entered.push((n, m.state_name(s.state).to_string(), s.eval_time));
        }
    }
    let names: Vec<&str> = entered.iter().map(|(_, name, _)| name.as_str()).collect();
    assert_eq!(names, ["whirl", "eddy", "gust"], "{entered:?}");
    // eddy starts at clock 0.25 (the clip goes on), so the 0.35 s Break opens 6 ticks later.
    let (at_eddy, _, clock) = &entered[1];
    assert!((clock - 0.25).abs() < 1e-9, "{entered:?}");
    assert_eq!(entered[2].0, at_eddy + 6, "{entered:?}");
}

/// Heading of `out.movement.animated_move` relative to `aim`, and its speed.
fn lunge_heading(out: &StepOutput, aim: f64) -> (f64, f64) {
    let v = out.movement.animated_move.expect("an AnimatedMove state");
    let mut rel = v8math::atan2(v[0], v[2]) - aim;
    while rel > std::f64::consts::PI {
        rel -= 2.0 * std::f64::consts::PI;
    }
    while rel < -std::f64::consts::PI {
        rel += 2.0 * std::f64::consts::PI;
    }
    (rel, v[0].hypot(v[2]))
}

fn aligned_machine() -> ComboMachine {
    let mover = |z: f64, thrust: f64, strafe: f64| {
        json!({"key":"AnimatedMove","children":[
            {"key":"VelocityZ","values":[z]},
            {"key":"VelocityFromThrust","values":[thrust]},
            {"key":"VelocityFromStrafe","values":[strafe]}]})
    };
    load(vec![
        state(
            "IDLE",
            vec![
                attr("Duration", json!([0])),
                edge("arc", vec![attr("Button", json!(["Fire", "Press"]))]),
                edge("hook", vec![attr("Button", json!(["Reload", "Press"]))]),
                edge("parry", vec![attr("Button", json!(["Crouch", "Press"]))]),
            ],
        ),
        state(
            "arc",
            vec![
                attr("Duration", json!([0.5])),
                attr("AlignedToThrust", json!([])),
                mover(0.0, 10.0, 2.0),
                edge("IDLE", vec![]),
            ],
        ),
        state(
            "hook",
            vec![
                attr("Duration", json!([0.5])),
                attr("AlignedToThrust", json!(["Initial"])),
                mover(6.0, 0.0, 0.0),
                edge("IDLE", vec![]),
            ],
        ),
        state(
            "parry",
            vec![
                attr("Duration", json!([0.5])),
                attr("AlignedToThrust", json!([])),
                json!({"key":"Deflect","children":[{"key":"DeflectAngle","values":[-60.0, 60.0]}]}),
                mover(4.0, 0.0, 0.0),
                edge("IDLE", vec![]),
            ],
        ),
    ])
    .unwrap()
}

fn aligned_tick(m: &ComboMachine, s: &mut ComboState, input: ComboInput) -> StepOutput {
    let tuning = super::tests::test_move_tuning();
    let mut life = move2::State::new(&tuning);
    let body = BodyContext {
        posture: ComboPosture::Stand,
        aim_yaw: 0.3,
        velocity_y: 0.0,
        grounded: true,
        moving: false,
    };
    step(GameFamily::Swbf2, m, s, &input, &body, &mut life, &tuning, false, COMBO_FIXED_DT)
}

#[test]
fn aligned_lunge_counts_the_stick_in_the_turned_frame() {
    let m = aligned_machine();
    // Stick fully right: the turn stops at 45 degrees right; the stick seen from the
    // turned frame is half forward, half right (0.7071 each), so the lunge is
    // 7.071 forward and 1.414 right of the turned heading.
    for (right, sign) in [(1.0, -1.0), (-1.0, 1.0)] {
        let mut s = ComboState::new(&m);
        let out = aligned_tick(&m, &mut s, ComboInput::default().press(ComboButton::Fire).stick(0.0, right));
        assert_eq!(m.state_name(s.state), "arc");
        let (heading, speed) = lunge_heading(&out, 0.3);
        let want = sign * (std::f64::consts::FRAC_PI_4 + v8math::atan2(2.0, 10.0));
        assert!((heading - want).abs() < 1e-9, "heading {heading} want {want}");
        assert!((speed - (50.0f64 + 2.0).sqrt()).abs() < 1e-9, "speed {speed}");
        assert!((out.swing_turn + sign * -std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    }
}

#[test]
fn initial_alignment_keeps_the_entry_stick_for_the_whole_state() {
    let m = aligned_machine();
    let mut s = ComboState::new(&m);
    let out = aligned_tick(&m, &mut s, ComboInput::default().press(ComboButton::Reload).stick(0.0, 1.0));
    assert_eq!(m.state_name(s.state), "hook");
    let quarter = std::f64::consts::FRAC_PI_4;
    assert!((lunge_heading(&out, 0.3).0 + quarter).abs() < 1e-9);
    // Releasing the stick, or pushing it the other way, keeps the entry tick's turn.
    for stick in [(0.0, 0.0), (0.0, -1.0), (0.4, -0.2)] {
        let out = aligned_tick(&m, &mut s, ComboInput::default().stick(stick.0, stick.1));
        assert_eq!(m.state_name(s.state), "hook");
        assert!((lunge_heading(&out, 0.3).0 + quarter).abs() < 1e-9, "{stick:?}");
        assert!((out.swing_turn + quarter).abs() < 1e-12, "{stick:?}");
    }
}

#[test]
fn a_guarding_state_turns_its_lunge_but_not_its_swing() {
    let m = aligned_machine();
    let mut s = ComboState::new(&m);
    let out = aligned_tick(&m, &mut s, ComboInput::default().press(ComboButton::Crouch).stick(0.0, -1.0));
    assert_eq!(m.state_name(s.state), "parry");
    assert!((out.movement.align_turn - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    assert!((lunge_heading(&out, 0.3).0 - std::f64::consts::FRAC_PI_4).abs() < 1e-9);
    assert_eq!(out.swing_turn, 0.0);
    // Back to the neutral state: no turn at all.
    for _ in 0..40 {
        aligned_tick(&m, &mut s, ComboInput::default().stick(0.0, -1.0));
    }
    let out = aligned_tick(&m, &mut s, ComboInput::default().stick(0.0, -1.0));
    assert_eq!(m.state_name(s.state), "IDLE");
    assert_eq!((out.movement.align_turn, out.swing_turn), (0.0, 0.0));
}

#[test]
fn alignment_turn_follows_the_games_comparisons() {
    let q = std::f64::consts::FRAC_PI_4;
    // Dead zone, mirrored back stick, clamp, exact angle.
    assert_eq!(aligned_turn([0.2, 0.2], 0.2), 0.0);
    assert_eq!(aligned_turn([-1.0, 0.0], -1.0), 0.0);
    assert_eq!(aligned_turn([0.0, 1.0], 0.0), -q);
    assert_eq!(aligned_turn([0.0, -1.0], 0.0), q);
    assert!((aligned_turn([1.0, 0.5], 1.0) + v8math::atan2(0.5, 1.0)).abs() < 1e-12);
    // A back-left stick turns the swing toward front-right (its mirror).
    assert!((aligned_turn([-0.7, -0.7], -0.7) + q).abs() < 1e-12);
    // A kept backward stick with the live stick released is not mirrored and
    // stops at the left limit, as the 2005 game's comparisons give.
    assert_eq!(aligned_turn([-1.0, 0.0], 0.0), q);
}
