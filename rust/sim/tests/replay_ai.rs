// The Classic AI control core (src/ai/classic plus the Enhanced
// objectives), replayed against the ai.* frames of the corpus
// BFE_REPLAY_CORPUS names (tools/sim-capture.mjs --specs ai). Each
// frame starts from the captured core state (tools/sim-capture/ai.mjs
// aiState); its calls to the rest of the battle (the combat, vehicle
// and navigation service bags, the movement helpers, the foreign
// agents, the registry query) are served from the capture with their
// arguments checked, its A/B draws served in order; the state it ends
// with is compared field for field. Without a corpus the tests pass
// without replaying.
#![cfg(feature = "replay")]

use bfe_sim::ai::control::replay::replay_frame;
use sim_replay::{Corpus, Event, Frame, Outcome, Report};

fn port(frame: &Frame) -> Outcome {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| replay_frame(frame))) {
        Ok(Ok(())) => Outcome::Same,
        Ok(Err(e)) => Outcome::Failed(e),
        Err(panic) => {
            let message = panic.downcast_ref::<String>().map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied()).unwrap_or("unknown panic");
            Outcome::Failed(format!("{}: replay panic: {message}", frame.place()))
        }
    }
}

#[test]
fn ai_phases_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let only = std::env::var("BFE_REPLAY_ONLY").ok();
    let tick = std::env::var("BFE_REPLAY_TICK").ok().map(|s| s.parse::<u32>().unwrap());
    let fail_fast = std::env::var_os("BFE_REPLAY_FAIL_FAST").is_some();
    let mut reports: Vec<_> = ["ai.update", "controller.bind", "controller.unbind", "objectives.onOwnerChanged", "objectives.onDeactivated", "objectives.tick"]
        .into_iter()
        .filter(|name| only.as_ref().is_none_or(|only| name.contains(only)))
        .map(Report::new)
        .collect();
    // Keep one capture in memory while checking all of its entry points.
    for capture in corpus.captures() {
        let capture = capture.unwrap();
        for report in &mut reports {
            report.captures += 1;
            let name = report.name.clone();
            for frame in capture.frames(&name) {
                let frame = frame.unwrap();
                if tick.is_some_and(|tick| frame.tick != tick) {
                    continue;
                }
                let outcome = port(&frame);
                report.add(&capture, &frame, outcome);
                if fail_fast {
                    report.assert_clean();
                }
            }
        }
    }
    assert!(reports.iter().any(|r| r.replayed() > 0), "no matching AI frames in the corpus");
    for report in &reports {
        eprintln!("{}", report.summary());
    }
    if let Some(path) = std::env::var_os("BFE_REPLAY_FAILURES") {
        let failures: Vec<_> = reports.iter().flat_map(|report| report.failures.iter()).map(String::as_str).collect();
        std::fs::write(path, failures.join("\n\n")).unwrap();
    }
    for report in &reports {
        report.assert_clean();
    }
}

/// The gates have teeth: one flipped draw, two swapped calls and one
/// end-state float moved by a single ulp each part the replay from
/// the capture. Nested state also distinguishes the two zero signs.
#[test]
fn ai_replay_mutations_are_caught() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    // A frame with draws, calls and a controller roll to move.
    let mut found = None;
    for capture in corpus.captures() {
        let capture = capture.unwrap();
        for frame in capture.frames("ai.update") {
            let frame = frame.unwrap();
            let draws = frame.events.iter().filter(|e| matches!(e, Event::Draw(_))).count();
            let calls = frame.events.iter().filter(|e| matches!(e, Event::Call(_))).count();
            if draws > 0 && calls > 1 && replay_frame(&frame).is_ok() {
                found = Some(frame);
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let frame = found.expect("the corpus has no clean AI update with draws and calls for mutation checks");

    // A branch-only draw can change while staying on the same side of
    // its threshold. Find a draw whose changed value is observable.
    let caught_value = frame.events.iter().enumerate().any(|(i, event)| {
        if !matches!(event, Event::Draw(_)) {
            return false;
        }
        let mut mutated = frame.clone();
        if let Event::Draw(d) = &mut mutated.events[i] {
            d.value = ((d.value as u32) ^ 0x8000_0000) as f64;
        }
        replay_frame(&mutated).is_err()
    });
    assert!(caught_value, "no changed draw value affected the frame");

    // An extra draw must always fail, even if its value is unobserved.
    let mut mutated = frame.clone();
    let at = mutated.events.iter().position(|event| matches!(event, Event::Draw(_))).unwrap();
    mutated.events.insert(at, mutated.events[at].clone());
    assert!(replay_frame(&mutated).is_err(), "an extra draw replayed clean");

    // Two calls swapped.
    let mut mutated = frame.clone();
    let idx: Vec<usize> = mutated.events.iter().enumerate().filter_map(|(i, e)| matches!(e, Event::Call(_)).then_some(i)).take(2).collect();
    mutated.events.swap(idx[0], idx[1]);
    assert!(replay_frame(&mutated).is_err(), "swapped calls replayed clean");

    // One end-state float (a controller's roll) by one ulp.
    let mut mutated = frame.clone();
    let mut bumped = false;
    for node in &mut mutated.exit.arena.nodes {
        if bumped {
            break;
        }
        if let sim_replay::Node::Object { fields, .. } = node {
            for (key, value) in fields.iter_mut() {
                if key == "roll" {
                    if let sim_replay::Value::Num(n) = value {
                        *n = f64::from_bits(n.to_bits() + 1);
                        bumped = true;
                        break;
                    }
                }
            }
        }
    }
    assert!(bumped, "no roll in the end state to move");
    assert!(replay_frame(&mutated).is_err(), "a one-ulp end state replayed clean");

    // A zero in a movement request must retain its sign even though
    // ordinary PartialEq would treat the two requests as equal.
    let mut mutated = frame.clone();
    let mut flipped = false;
    for node in &mut mutated.exit.arena.nodes {
        if let sim_replay::Node::Object { fields, .. } = node {
            if !fields.iter().any(|(key, _)| key == "steering") {
                continue;
            }
            for (key, value) in fields.iter_mut() {
                if key == "mode" {
                    if let sim_replay::Value::Num(n) = value {
                        if *n == 0.0 {
                            *n = -*n;
                            flipped = true;
                            break;
                        }
                    }
                }
            }
        }
        if flipped {
            break;
        }
    }
    assert!(flipped, "no zero movement mode in the end state to flip");
    assert!(replay_frame(&mutated).is_err(), "a signed-zero request replayed clean");
}
