//! Frame times by phase (loading screen, spawn screen, the first minute of
//! play), one stderr line when a phase ends, so a slow PC's log shows where
//! the window stalled.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Other,
    Loading,
    Spawn,
    Play,
}

const PLAY_WINDOW: Duration = Duration::from_secs(30);
const PLAY_WINDOWS: u32 = 2;
const SPAWN_WINDOW: Duration = Duration::from_secs(10);

pub struct FrameLog {
    phase: Phase,
    started: Instant,
    last: Option<Instant>,
    ms: Vec<f32>,
    play_windows: u32,
    spawn_windows: u32,
}

impl Default for FrameLog {
    fn default() -> Self {
        Self { phase: Phase::Other, started: Instant::now(), last: None, ms: Vec::with_capacity(4096), play_windows: 0, spawn_windows: 0 }
    }
}

impl FrameLog {
    /// Records the frame that just ended at `now`, drawn in `phase`.
    pub fn frame(&mut self, now: Instant, phase: Phase) {
        let age = now.duration_since(self.started);
        let window_full = phase == self.phase
            && match phase {
                Phase::Play => age >= PLAY_WINDOW,
                Phase::Spawn => age >= SPAWN_WINDOW,
                _ => false,
            };
        if phase != self.phase || window_full {
            self.flush(now);
            if phase == Phase::Loading {
                self.play_windows = 0;
            }
            if phase != Phase::Spawn {
                self.spawn_windows = 0;
            }
            self.phase = phase;
            self.started = now;
        }
        if let Some(last) = self.last {
            self.ms.push(now.duration_since(last).as_secs_f32() * 1000.);
        }
        self.last = Some(now);
    }

    fn flush(&mut self, now: Instant) {
        let name = match self.phase {
            Phase::Other => None,
            Phase::Loading => Some("loading".to_string()),
            Phase::Spawn => {
                self.spawn_windows += 1;
                Some(format!("spawn screen part {}", self.spawn_windows))
            }
            Phase::Play if self.play_windows < PLAY_WINDOWS => {
                self.play_windows += 1;
                Some(format!("play part {} of {PLAY_WINDOW:?}", self.play_windows))
            }
            Phase::Play => None,
        };
        if let (Some(name), Some(line)) = (name, summary(&mut self.ms)) {
            eprintln!("frame times: {name} {:.1} s {line}{}", now.duration_since(self.started).as_secs_f64(), memory());
        }
        self.ms.clear();
    }
}

/// Peak resident memory and swap of this process, where the system tells.
fn memory() -> String {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else { return String::new() };
    let field = |name: &str| {
        status.lines().find_map(|l| l.strip_prefix(name)).and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
    };
    match (field("VmHWM:"), field("VmSwap:")) {
        (Some(peak), Some(swap)) => format!(", peak RSS {} MiB, swap {} MiB", peak / 1024, swap / 1024),
        _ => String::new(),
    }
}

fn summary(ms: &mut [f32]) -> Option<String> {
    if ms.is_empty() {
        return None;
    }
    ms.sort_by(f32::total_cmp);
    let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
    let over = |limit: f32| ms.iter().filter(|&&v| v > limit).count();
    Some(format!(
        "frames={} p50={:.1} p95={:.1} p99={:.1} max={:.1} ms, over 50 ms={} over 100 ms={}",
        ms.len(),
        at(0.5),
        at(0.95),
        at(0.99),
        ms[ms.len() - 1],
        over(50.),
        over(100.)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentiles_and_spikes() {
        let mut ms = vec![10.; 98];
        ms.extend([60., 120.]);
        let line = summary(&mut ms).unwrap();
        assert!(line.starts_with("frames=100 p50=10.0 p95=10.0 p99=60.0 max=120.0"), "{line}");
        assert!(line.ends_with("over 50 ms=2 over 100 ms=1"), "{line}");
    }
}
