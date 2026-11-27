//! Straight-line trials measure body displacement, including coast after an action.

use super::*;
use serde_json::Value;

const SPRINT: Actions = Actions { sprint: true, roll: false, ai: false };

struct Timing {
    roll_lead: u32,
    roll_hold: u32,
    speed_lead: u32,
    sprint_lead: u32,
    speed_window: [u32; 2],
    jump_lead: u32,
    jump_sprint: u32,
    jump_hold: u32,
}

fn forward() -> Input {
    Input { forward: 1.0, ..Input::default() }
}

fn walk(rig: &mut Rig, ticks: u32, actions: Actions) {
    for _ in 0..ticks {
        rig.tick(forward(), actions);
    }
}

fn roll(mut rig: Rig, timing: &Timing) -> (f64, f64) {
    walk(&mut rig, timing.roll_lead, Actions::default());
    let start = rig.mover().position.z;
    let mut end = None;
    for tick in 1..=300 {
        let input = if tick <= timing.roll_hold { forward() } else { Input::default() };
        let before = rig.mover().position.z;
        rig.tick(input, Actions { roll: tick == 1, sprint: false, ai: false });
        if tick == 1 {
            assert_eq!(rig.state.roll.unwrap().direction, RollDirection::Forward);
            near(rig.state.energy, rig.tuning.energy_capacity - rig.tuning.energy_cost_roll);
        }
        if rig.state.roll.is_none() {
            let exit_tick = *end.get_or_insert(tick);
            if rig.mover().position.z != before {
                continue;
            }
            let lock_time = f64::from(exit_tick) * DT;
            assert!((lock_time - rig.tuning.roll_seconds()).abs() <= DT + 1e-12);
            println!("roll action ended at {lock_time:.6} s");
            return (before - start, f64::from(tick - 1) * DT);
        }
    }
    panic!("forward roll did not settle");
}

fn sprint_speed(mut rig: Rig, timing: &Timing) -> f64 {
    walk(&mut rig, timing.speed_lead, Actions::default());
    let [from, to] = timing.speed_window;
    assert!(from < to);
    walk(&mut rig, from, SPRINT);
    let start = rig.mover().position.z;
    walk(&mut rig, to - from, SPRINT);
    assert!(rig.state.sprint_time.is_some());
    (rig.mover().position.z - start) / (f64::from(to - from) * DT)
}

fn held_sprint(mut rig: Rig, timing: &Timing) -> (f64, f64, f64) {
    walk(&mut rig, timing.sprint_lead, Actions::default());
    let mut first_debit = None;
    let mut empty = None;
    for tick in 1..=1800 {
        let before = rig.state.energy;
        rig.tick(forward(), SPRINT);
        if rig.state.energy < before {
            let start = *first_debit.get_or_insert(tick);
            if let Some(floor_tick) = empty {
                assert!(rig.state.sprint_time.is_some() && !rig.state.exhausted);
                let drain_time = f64::from(floor_tick + 1 - start) * DT;
                let restart_time = f64::from(tick - floor_tick) * DT;
                return (drain_time, 100.0 * before / rig.tuning.energy_capacity, restart_time);
            }
        }
        if rig.state.energy <= 0.0 {
            empty.get_or_insert(tick);
        }
    }
    panic!("held sprint did not empty and restart");
}

fn sprint_jump(mut rig: Rig, timing: &Timing) -> f64 {
    walk(&mut rig, timing.jump_lead, Actions::default());
    walk(&mut rig, timing.jump_sprint, SPRINT);
    assert!(rig.state.sprint_time.is_some());
    let start = rig.mover().position.z;
    for tick in 1..=300 {
        let held = tick <= timing.jump_hold;
        let mut input = if held { forward() } else { Input::default() };
        input.jump = tick == 1;
        let before = rig.mover().position.z;
        let airborne = !rig.mover().grounded;
        rig.tick(input, Actions { sprint: held, roll: false, ai: false });
        if tick == 1 {
            assert_eq!(rig.mover().air, Air::Jump);
        }
        if !rig.mover().grounded {
            continue;
        }
        if airborne {
            println!("jump landed at {:.6} m after {:.6} s", rig.mover().position.z - start, f64::from(tick) * DT);
        }
        if before == rig.mover().position.z {
            return before - start;
        }
    }
    panic!("sprint jump did not settle");
}

fn measure(make_rig: impl Fn() -> Rig, timing: &Timing) -> [(&'static str, f64); 7] {
    let (distance, duration) = roll(make_rig(), timing);
    let speed = sprint_speed(make_rig(), timing);
    let (drain, restart, delay) = held_sprint(make_rig(), timing);
    let jump = sprint_jump(make_rig(), timing);
    [
        ("roll_distance", distance),
        ("roll_motion_seconds", duration),
        ("sprint_speed", speed),
        ("drain_seconds", drain),
        ("restart_percent", restart),
        ("restart_seconds", delay),
        ("jump_distance", jump),
    ]
}

fn flat() -> Rig {
    Rig::on(terrain(512.0, 0.0, None))
}

fn within(name: &str, value: f64, [low, high]: [f64; 2]) {
    println!("{name} {value:.9} [{low}, {high}]");
    assert!((low..=high).contains(&value), "{name}: {value} outside [{low}, {high}]");
}

#[test]
fn flat_ground_action_distances_and_held_sprint() {
    let make_rig = || {
        let mut rig = flat();
        rig.mover().max_speed = 5.5;
        rig.mover().accel = 48.0;
        rig
    };
    let timing = Timing {
        roll_lead: 14,
        roll_hold: 4,
        speed_lead: 38,
        sprint_lead: 32,
        speed_window: [42, 70],
        jump_lead: 35,
        jump_sprint: 51,
        jump_hold: 7,
    };
    let results = measure(make_rig, &timing);
    let bounds = [
        [6.4, 6.6],
        [0.68, 0.72],
        [9.62499, 9.62501],
        [3.75, 3.75 + DT],
        [13.33, 13.51],
        [1.33, 1.38],
        [11.3, 11.9],
    ];
    for ((name, value), range) in results.into_iter().zip(bounds) {
        within(name, value, range);
    }
}

/// The external file supplies resolved class properties, input timings in ticks
/// and independent measurement bounds. Fixtures used by default are synthetic.
#[test]
#[ignore = "requires class settings and independent observations in BFE_INFANTRY_TRIAL"]
fn flat_ground_actions_with_external_class_and_observations() {
    let Some(path) = std::env::var_os("BFE_INFANTRY_TRIAL") else { return };
    let spec: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let class = &spec["class"];
    let number = |key: &str| class[key].as_f64().unwrap_or_else(|| panic!("missing {key}"));
    let row = |key: &str| {
        let [forward, strafe, turn]: [f64; 3] = serde_json::from_value(class[key].clone()).unwrap();
        ControlSpeed { forward, strafe, turn }
    };
    let make_rig = || {
        let mut rig = flat();
        let m = rig.mover();
        m.max_speed = number("max_speed");
        m.max_strafe = number("max_strafe");
        m.max_turn = number("max_turn");
        m.accel = number("acceleration");
        rig.tuning = Tuning {
            stand: row("stand"),
            crouch: row("crouch"),
            sprint: row("sprint"),
            jump: row("jump"),
            jet: row("jet"),
            roll: row("roll"),
            tumble: row("tumble"),
            jetpack: None,
            sprint_accelerate_time: number("sprint_accelerate_time"),
            energy_capacity: number("energy_capacity"),
            energy_floor: number("energy_floor"),
            energy_restore: number("energy_restore"),
            energy_restore_idle: number("energy_restore_idle"),
            energy_drain_sprint: number("energy_drain_sprint"),
            energy_min_sprint: number("energy_min_sprint"),
            energy_cost_roll: number("energy_cost_roll"),
            energy_cost_jump: number("energy_cost_jump"),
            energy_cost_jump_sprint: class["energy_cost_jump_sprint"].as_f64(),
            jump_height: number("jump_height"),
            jump_forward_speed_factor: number("jump_forward_speed_factor"),
            jump_strafe_speed_factor: number("jump_strafe_speed_factor"),
            roll_speed_factor: number("roll_speed_factor"),
            roll_frames: serde_json::from_value(class["roll_frames"].clone()).unwrap(),
            recover_from_tumble: false,
            getup_front_seconds: DEFAULT_GETUP_FRONT_SECONDS,
            getup_back_seconds: DEFAULT_GETUP_BACK_SECONDS,
        };
        rig.state = State::new(&rig.tuning);
        rig
    };
    let times = &spec["ticks"];
    let ticks = |key: &str| serde_json::from_value(times[key].clone()).unwrap();
    let timing = Timing {
        roll_lead: ticks("roll_lead"),
        roll_hold: ticks("roll_hold"),
        speed_lead: ticks("speed_lead"),
        sprint_lead: ticks("sprint_lead"),
        speed_window: serde_json::from_value(times["speed_window"].clone()).unwrap(),
        jump_lead: ticks("jump_lead"),
        jump_sprint: ticks("jump_sprint"),
        jump_hold: ticks("jump_hold"),
    };
    for (name, value) in measure(make_rig, &timing) {
        within(name, value, serde_json::from_value(spec["bounds"][name].clone()).unwrap());
    }
}
