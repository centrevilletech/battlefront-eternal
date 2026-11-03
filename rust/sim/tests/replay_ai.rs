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
use sim_replay::{replay_corpus, Corpus, Event, Frame, Outcome};

fn port(frame: &Frame) -> Outcome {
    match replay_frame(frame) {
        Ok(()) => Outcome::Same,
        Err(e) => Outcome::Failed(e),
    }
}

#[test]
fn ai_phases_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    for name in ["ai.update", "controller.bind", "controller.unbind", "objectives.onOwnerChanged", "objectives.onDeactivated", "objectives.tick"] {
        let report = replay_corpus(&corpus, name, |_, frame| port(frame)).unwrap();
        eprintln!("{}", report.summary());
        report.assert_clean();
    }
}

/// The gates have teeth: one flipped draw, two swapped calls and one
/// end-state float moved by a single ulp each part the replay from
/// the capture.
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
    let Some(frame) = found else { return };

    // One raw draw flipped in its top bit.
    let mut mutated = frame.clone();
    for event in &mut mutated.events {
        if let Event::Draw(d) = event {
            d.value = ((d.value as u32) ^ 0x8000_0000) as f64;
            break;
        }
    }
    assert!(replay_frame(&mutated).is_err(), "a flipped draw replayed clean");

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
}
