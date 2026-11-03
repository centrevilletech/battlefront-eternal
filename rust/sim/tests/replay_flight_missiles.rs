// A flyer missile's seeker (src/flight/targeting.js navigateMissile, as the
// rounds ask it through their world, src/ordnance-world.js), replayed
// against every flightMissile.navigateMissile frame of the corpus
// BFE_REPLAY_CORPUS names (tools/sim-capture.mjs --specs flight): the
// world's checks (the target found by id, alive and of the life fired at),
// then the seeker's turn of the velocity and whether it keeps the target.
// The velocity it ends with and the id it returns are compared with the
// capture's. Without a corpus the test passes without replaying.
#![cfg(feature = "replay")]

use bfe_sim::flight::config::{Tuning, Tunings, TUNING_KEYS};
use bfe_sim::flight::targeting::{navigate_missile, MissileTarget};
use bfe_sim::flight::FlightClass;
use sim_replay::{replay_corpus, same_number, Corpus, Outcome, Value, V};
use three::Vector3;

fn vec3(v: V) -> Vector3 {
    let a = v.vec3().unwrap_or_else(|| panic!("not a Vector3: {}", v.show()));
    Vector3::new(a[0], a[1], a[2])
}

fn tunings_of(tuning: V) -> Tunings {
    let mut t = Tunings::default();
    if !tuning.is_nullish() {
        // (The classes the frame reads: a craft's own, a missile's.)
        for (i, name) in FlightClass::NAMES.iter().enumerate() {
            let c = tuning.get(name);
            if c.is_nullish() {
                continue;
            }
            let mut row = Tuning::of(FlightClass::from_u8(i as u8).unwrap());
            for key in TUNING_KEYS {
                let x = c.get(key).num();
                if !row.retune(key, x) {
                    assert!(same_number(row.get(key).unwrap(), x), "a tuning the port cannot hold: {name}.{key} = {x}");
                }
            }
            t.0[i] = row;
        }
    }
    t
}

/// `x || 0` for a value that may be undefined.
fn or_zero(v: V) -> f64 {
    match v.value {
        Value::Num(x) if !(*x == 0.0 || x.is_nan()) => *x,
        _ => 0.0,
    }
}

#[test]
fn flyer_missiles_steer_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let (mut kept, mut lost, mut gone) = (0usize, 0usize, 0usize);
    let report = replay_corpus(&corpus, "flightMissile.navigateMissile", |_, frame| {
        let (before, after) = (frame.before(), frame.after());
        let position = vec3(frame.arg(0));
        let mut velocity = vec3(frame.arg(1));
        let class = frame.arg(2);
        let target_id = frame.arg(3).num();
        let target_generation = frame.arg(4).num();
        let dt = frame.arg(5).num();
        let t = before.get("target");
        // The world's own checks: the entry by id, alive, of the life fired at.
        let alive = !t.is_nullish() && t.get("alive").truthy() && t.get("generation").opt_num().unwrap_or(0.0) == target_generation;
        let result = if !alive {
            gone += 1;
            target_id
        } else {
            // FLIGHT_CLASSES[r.flightClass || 'fighter'].
            let name = match class.value {
                Value::Str(s) if !s.is_empty() => s.as_str(),
                _ => "fighter",
            };
            let Some(i) = FlightClass::NAMES.iter().position(|&n| n == name) else {
                return Outcome::Failed(format!("{}: no class {name}", frame.place()));
            };
            let tuning = tunings_of(before.get("tuning")).of(FlightClass::from_u8(i as u8).unwrap());
            let target = MissileTarget {
                position: vec3(t.get("position")),
                velocity: if t.get("velocity").is_nullish() { None } else { Some(vec3(t.get("velocity"))) },
                roll_rate: or_zero(t.get("rollRate")),
            };
            if navigate_missile(&position, &mut velocity, &tuning, &target, dt) {
                kept += 1;
                target_id
            } else {
                lost += 1;
                -1.0
            }
        };
        if !same_number(frame.result().num(), result) {
            return Outcome::Failed(format!("{}: returned {} | {result} (capture | port)", frame.place(), frame.result().show()));
        }
        let theirs = vec3(after.get("velocity"));
        if !(same_number(theirs.x, velocity.x) && same_number(theirs.y, velocity.y) && same_number(theirs.z, velocity.z)) {
            return Outcome::Failed(format!("{}: velocity {theirs:?} | {velocity:?} (capture | port)", frame.place()));
        }
        Outcome::Same
    })
    .unwrap();
    eprintln!("{}\n  seeker kept {kept}, lost {lost}, target gone or another life {gone}", report.summary());
    report.assert_clean();
}
