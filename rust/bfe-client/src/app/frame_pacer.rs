//! When the window presents a frame. The browser hands the page one chance
//! to draw every display refresh and its FramePacer
//! (src/runtime/frame-pacing.js) lets a chance pass while the frameLimit
//! setting's deadline is still ahead. Here the chances come from the display
//! as well: a swap that waits for the refresh, or the compositor's frame
//! callbacks. `Display` watches that they really do, and counts the refresh
//! out on a timer where they don't (a software surface, a driver with its
//! sync forced off), so no frame is ever drawn faster than it can be shown.

/// frame-pacing.js FramePacer. Times are seconds.
pub struct FramePacer {
    pub limit: f64,
    elapsed: f64,
    presented: f64,
    deadline: f64,
}

impl FramePacer {
    pub fn new(limit: f64) -> Self {
        let mut pacer = Self { limit: 0., elapsed: 0., presented: 0., deadline: 0. };
        pacer.set_limit(limit);
        pacer
    }
    /// A limit that is no positive number is none: every chance presents.
    pub fn set_limit(&mut self, limit: f64) {
        self.limit = if limit.is_finite() && limit > 0. { limit } else { 0. };
        self.reset();
    }
    pub fn reset(&mut self) {
        self.elapsed = 0.;
        self.presented = 0.;
        self.deadline = 0.;
    }
    // A chance's timestamp jitters even on a display that matches the cap:
    // missing a 60 Hz deadline by a tenth of a millisecond must not throw a
    // whole refresh away.
    fn tolerance(&self) -> f64 {
        if self.limit != 0. {
            0.0005f64.min(0.05 / self.limit)
        } else {
            0.
        }
    }
    /// A chance to present, `seconds` after the last one. None lets it pass;
    /// otherwise the time since the last frame presented.
    pub fn advance(&mut self, seconds: f64) -> Option<f64> {
        self.elapsed += seconds.max(0.);
        let tolerance = self.tolerance();
        if self.limit != 0. && self.elapsed + tolerance + 1e-8 < self.deadline {
            return None;
        }
        let dt = self.elapsed - self.presented;
        self.presented = self.elapsed;
        // The phase is kept on an irregular display, but a stall is not made
        // up with extra frames.
        if self.limit != 0. {
            self.deadline += (((self.elapsed + tolerance + 1e-8 - self.deadline) * self.limit).floor() + 1.).max(1.) / self.limit;
        }
        Some(dt)
    }
    /// Seconds from the last chance until the deadline can be met: how long
    /// the loop may sleep instead of asking again. The page has no such
    /// question, its next chance simply comes a refresh later.
    pub fn wait(&self) -> f64 {
        if self.limit == 0. {
            return 0.;
        }
        (self.deadline - self.tolerance() - 1e-8 - self.elapsed).max(0.)
    }
}

// Frames watched before deciding whether the display paces them.
const WINDOW: u32 = 30;

/// Whether something outside the loop holds frames to the display's refresh.
#[derive(Default)]
pub struct Display {
    /// Seconds a refresh lasts; 60 Hz until the monitor says otherwise.
    interval: f64,
    /// The loop counts refreshes itself: nothing else was seen to.
    pub timed: bool,
    frames: u32,
    time: f64,
}

impl Display {
    /// `millihertz` is the monitor's refresh rate where the system knows it.
    pub fn set_refresh(&mut self, millihertz: Option<u32>) {
        self.interval = match millihertz {
            Some(rate) if rate >= 1000 => 1000. / rate as f64,
            _ => 1. / 60.,
        };
    }
    pub fn interval(&self) -> f64 {
        if self.interval > 0. {
            self.interval
        } else {
            1. / 60.
        }
    }
    /// A frame was presented `seconds` after the one before. Thirty frames
    /// in less than half the time the display needs to show them means
    /// nothing is waiting for it.
    pub fn presented(&mut self, seconds: f64) {
        if self.timed {
            return;
        }
        self.frames += 1;
        self.time += seconds;
        if self.frames == WINDOW {
            self.timed = self.time < 0.5 * WINDOW as f64 * self.interval();
            self.frames = 0;
            self.time = 0.;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_limit_presents_every_chance() {
        let mut pacer = FramePacer::new(0.);
        assert_eq!(pacer.advance(1. / 144.), Some(1. / 144.));
        assert_eq!(pacer.advance(1. / 144.), Some(1. / 144.));
        assert_eq!(pacer.wait(), 0.);
    }

    #[test]
    fn thirty_on_a_sixty_hertz_display_presents_every_other_chance() {
        let mut pacer = FramePacer::new(30.);
        let mut presented = 0;
        for _ in 0..120 {
            if pacer.advance(1. / 60.).is_some() {
                presented += 1;
            }
        }
        // The first chance presents at once, then every other one.
        assert_eq!(presented, 61);
    }

    #[test]
    fn a_limit_above_the_display_presents_every_chance() {
        let mut pacer = FramePacer::new(240.);
        assert!((0..60).all(|_| pacer.advance(1. / 60.).is_some()));
    }

    #[test]
    fn a_late_chance_within_the_tolerance_still_presents() {
        let mut pacer = FramePacer::new(60.);
        assert!(pacer.advance(1. / 60.).is_some());
        assert!(pacer.advance(1. / 60. - 0.0001).is_some());
    }

    #[test]
    fn a_stall_is_not_made_up() {
        let mut pacer = FramePacer::new(60.);
        assert!(pacer.advance(1. / 60.).is_some());
        // One frame for the whole second (the sum of two times is not exactly 1).
        assert!(pacer.advance(1.).is_some_and(|dt| (dt - 1.).abs() < 1e-9));
        assert!(pacer.advance(0.001).is_none());
        assert!(pacer.wait() > 0.01);
    }

    #[test]
    fn unpaced_frames_turn_the_timer_on() {
        let mut display = Display::default();
        display.set_refresh(Some(60_000));
        for _ in 0..WINDOW {
            display.presented(0.002);
        }
        assert!(display.timed);
        let mut display = Display::default();
        display.set_refresh(None);
        for _ in 0..WINDOW * 3 {
            display.presented(1. / 60.);
        }
        assert!(!display.timed);
    }
}
