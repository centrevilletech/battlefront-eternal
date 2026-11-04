//! Every captured AI combat service and agent entry point, with exact
//! state, boundary arguments and random-stream order checks.
#![cfg(feature = "replay")]
use bfe_sim::ai::combat::replay::replay_frame;
use sim_replay::{Corpus, Outcome, Report};

#[test]
fn ai_combat_as_captured() {
    let Some(corpus) = Corpus::from_env().unwrap() else { return };
    let only = std::env::var("BFE_REPLAY_ONLY").ok();
    let tick = std::env::var("BFE_REPLAY_TICK").ok().map(|s| s.parse::<u32>().unwrap());
    let fail_fast = std::env::var_os("BFE_REPLAY_FAIL_FAST").is_some();
    let mut reports: Vec<Report> = Vec::new();
    for capture in corpus.captures() {
        let capture = capture.unwrap();
        for spec in capture.header.root().get("frames").items() {
            let name = spec.get("name").str().unwrap();
            if !(name.starts_with("aiCombat.") || name.starts_with("UnitCombatAgent.")) || only.as_ref().is_some_and(|only| !name.contains(only)) { continue; }
            let index = reports.iter().position(|r| r.name == name).unwrap_or_else(|| { reports.push(Report::new(name)); reports.len()-1 });
            let report = &mut reports[index]; report.captures += 1;
            for frame in capture.frames(name) {
                let frame = frame.unwrap();
                if tick.is_some_and(|t| t != frame.tick) { continue; }
                let outcome = match replay_frame(&capture,&frame) { Ok(()) => Outcome::Same, Err(e) => Outcome::Failed(e) };
                report.add(&capture,&frame,outcome);
                if fail_fast { report.assert_clean(); }
            }
        }
    }
    assert!(reports.iter().any(|r| r.replayed() > 0), "no matching AI combat frames");
    for report in &reports { eprintln!("{}",report.summary()); }
    if let Some(path) = std::env::var_os("BFE_REPLAY_FAILURES") {
        let failures: Vec<_> = reports.iter().flat_map(|r| r.failures.iter()).map(String::as_str).collect();
        std::fs::write(path,failures.join("\n\n")).unwrap();
    }
    for report in &reports { report.assert_clean(); }
}
