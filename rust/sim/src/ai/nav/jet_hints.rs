//! The jet-hint service's authored-hint helper (nav/jet-hints.js),
//! without the controller machinery around it: `find_hints` is the
//! service's random-order scan of the jetjump hints near a unit; the
//! enter/tick driving (helper.goto, helper.jet) runs through the
//! service's goto helper in the caller's hands, and is kept here as the
//! small state machine of JetHintService::tick over plain inputs.

use super::math::Vec3;
use crate::ai::random::ClassicRandom;

/// One active jet-hint job (jet-hints.js:39): the hint being used, the
/// state (2 going to it, 3 launched from it) and when the launch began.
/// The helper itself is the caller's, by the token it knows it by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetHintJob {
    pub helper: u64,
    pub hint: usize,
    pub state: f64,
    pub started: f64,
}

/// The pick among candidate hint positions (jet-hints.js:15-34, the part
/// after the hint query): random removal order, the least
/// 2.25 * horizontal distance squared wins, ties keep the earlier pick.
/// `hints` are indices into the service's hint list with their positions.
pub fn find_hint(hints: &[(usize, Vec3)], position: Vec3, random: &mut ClassicRandom) -> Option<usize> {
    let mut hints: Vec<(usize, Vec3)> = hints.to_vec();
    let mut best = None;
    let mut score = f64::INFINITY;
    while !hints.is_empty() {
        let i = random.int(0.0, hints.len() as f64 - 1.0) as usize;
        let hint = hints[i];
        let last = hints.len() - 1;
        hints[i] = hints[last];
        hints.pop();
        let dx = hint.1.x - position.x;
        let dz = hint.1.z - position.z;
        let next = 2.25 * (dx * dx + dz * dz);
        if next < score {
            best = Some(hint.0);
            score = next;
        }
    }
    best
}

/// What one JetHintService::tick decides (jet-hints.js:43), over the
/// job's inputs: whether it is done, and whether this tick launches
/// (the goto 80 m along the hint's forward and the jet request, which
/// the caller makes on its helper).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JetTick {
    pub done: bool,
    pub launch: bool,
}

/// jet-hints.js:43. `arrived`, `status_or_failed` and `jetting` are the
/// helper's arrived, status || failed, and the control's jetting.
pub fn jet_tick(job: &mut JetHintJob, time: f64, arrived: bool, status_or_failed: bool, jetting: bool) -> JetTick {
    let mut launch = false;
    if job.state == 2.0 && arrived {
        if status_or_failed {
            return JetTick { done: true, launch };
        }
        job.state = 3.0;
        job.started = time;
        launch = true;
    }
    JetTick { done: job.state == 3.0 && time > job.started + 1.0 && !jetting, launch }
}
