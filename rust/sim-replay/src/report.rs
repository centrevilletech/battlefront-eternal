//! A replay run's tally: frames replayed the same, frames the port could not
//! take (and why), and the first differences.

use crate::frame::{Capture, Corpus, Error, Frame};
use std::collections::BTreeMap;

/// How one frame's replay went.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Every call, draw and the end state as captured.
    Same,
    /// Not replayed: the port does not take frames like this one yet (why).
    Skipped(String),
    /// Parted from the capture (how; the first difference).
    Failed(String),
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub name: String,
    pub captures: usize,
    pub same: usize,
    pub skipped: BTreeMap<String, usize>,
    pub failures: Vec<String>,
}

impl Report {
    pub fn new(name: &str) -> Report {
        Report { name: name.into(), ..Report::default() }
    }

    pub fn add(&mut self, capture: &Capture, frame: &Frame, outcome: Outcome) {
        match outcome {
            Outcome::Same => self.same += 1,
            Outcome::Skipped(why) => *self.skipped.entry(why).or_default() += 1,
            Outcome::Failed(what) => self.failures.push(format!("{} {}: {what}", capture.label(), frame.place())),
        }
    }

    pub fn replayed(&self) -> usize {
        self.same + self.failures.len()
    }

    pub fn summary(&self) -> String {
        let skipped: usize = self.skipped.values().sum();
        let mut s = format!(
            "{}: {} frames from {} captures, {} the same, {} different, {} not replayed",
            self.name,
            self.replayed() + skipped,
            self.captures,
            self.same,
            self.failures.len(),
            skipped
        );
        for (why, n) in &self.skipped {
            s.push_str(&format!("\n  not replayed ({n}): {why}"));
        }
        for f in self.failures.iter().take(10) {
            s.push_str(&format!("\n  DIFFERENT {f}"));
        }
        if self.failures.len() > 10 {
            s.push_str(&format!("\n  ... and {} more", self.failures.len() - 10));
        }
        s
    }

    /// Panics, with the summary, unless every replayed frame was the same.
    pub fn assert_clean(&self) {
        assert!(self.failures.is_empty(), "{}", self.summary());
    }
}

/// Replays every frame of `name` in every capture of the corpus through `port`.
pub fn replay_corpus(corpus: &Corpus, name: &str, mut port: impl FnMut(&Capture, &Frame) -> Outcome) -> Result<Report, Error> {
    let mut report = Report::new(name);
    for capture in corpus.captures() {
        let capture = capture?;
        report.captures += 1;
        for frame in capture.frames(name) {
            let frame = frame?;
            let outcome = port(&capture, &frame);
            report.add(&capture, &frame, outcome);
        }
    }
    Ok(report)
}
