//! The front end's music policy, independent of AudioContext and PCM I/O.
//! Authored memberships load once; commands publish one start/stop decision.
use crate::shell_music_layout::{shell_music_op as op, shell_music_phase as phase, shell_music_row as r};

pub struct Music {
    pub row: [f64; r::STRIDE],
    groups: Vec<Vec<usize>>,
    current: Option<usize>,
    cycle: usize,
    fresh: bool,
    enabled: bool,
}
impl Music {
    // Membership vectors are immutable authored content. No frame builds
    // a filtered track list or allocates a playback-policy result object.
    pub fn new(groups: Vec<Vec<usize>>) -> Self {
        let mut row = [0.0; r::STRIDE];
        row[r::CYCLE] = -1.0;
        Self { row, groups, current: None, cycle: 0, fresh: true, enabled: false }
    }
    // Ports ShellMusic.next: one shared cycle continues across named music
    // groups. Missing or empty groups remain set and retry after the gap.
    fn next(&mut self) {
        let list = self.current.and_then(|i| self.groups.get(i));
        let Some(list) = list.filter(|list| !list.is_empty()) else {
            self.row[r::PHASE] = phase::GAP as f64;
            self.row[r::SILENT] = 0.0;
            return;
        };
        self.cycle = if self.fresh { 0 } else { (self.cycle + 1) % list.len() };
        self.fresh = false;
        self.row[r::CYCLE] = self.cycle as f64;
        self.row[r::PHASE] = phase::PLAYING as f64;
        self.row[r::START] = (list[self.cycle] + 1) as f64;
    }
    // Stopping does not rewind the shared cycle. Returning to soundtrack
    // resumes with the next entry, just as switching named groups does.
    fn halt(&mut self) {
        self.row[r::STOP] = 1.0;
        self.row[r::PHASE] = phase::STOPPED as f64;
        self.current = None;
    }
    pub fn frame(&mut self) {
        self.row[r::STOP] = 0.0;
        self.row[r::START] = 0.0;
        // One command has at most one stop and one start. The browser
        // applies those actions to its independently owned playback handles.
        match self.row[r::OP] as u32 {
            op::SET => {
                self.halt();
                self.current = Some(self.row[r::GROUP] as usize);
                // An empty requested name may select an authored group but
                // is false for periodic retries in the original controller.
                self.enabled = self.row[r::ENABLED] != 0.0;
                self.next();
            }
            op::HALT => self.halt(),
            op::END => {
                // The browser qualifies the ended playback handle before
                // submitting this command. A stale phase cannot end a gap.
                if self.row[r::PHASE] == phase::PLAYING as f64 {
                    self.row[r::PHASE] = phase::GAP as f64;
                    self.row[r::SILENT] = 0.0;
                }
            }
            op::NEXT => self.next(),
            op::TICK
                // Ports ShellMusic.update, including shortened intervals
                // taking effect immediately and only one start per frame.
                if self.row[r::PHASE] == phase::GAP as f64 && self.current.is_some() && self.enabled =>
            {
                self.row[r::SILENT] += self.row[r::DT];
                if self.row[r::SILENT] >= self.row[r::INTERVAL] { self.next(); }
            }
            _ => {}
        }
    }
}
