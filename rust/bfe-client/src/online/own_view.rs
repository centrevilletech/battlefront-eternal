//! SoldierPresentation samples the last prediction interval for every display
//! frame. Collided bodies and input history remain at their fixed tick poses.
use bfe_presentation::{soldier_view, visual_effects_layout::soldier_frame as f};
use bfe_sim::gen::Soldier;
use bfe_presentation::cameras::math::{Math, V8};

#[derive(Default)]
pub(super) struct Display {
    owner: soldier_view::Soldier,
    life: Option<[f64; 3]>,
}
impl Display {
    pub fn sample(
        &mut self,
        body: &Soldier,
        before: Option<&Soldier>,
        look_yaw: f64,
        life: [f64; 3],
        alpha: f64,
        dt: f64,
    ) -> Soldier {
        let x = &mut self.owner.input;
        x[f::POSITION..f::POSITION + 3].copy_from_slice(&body.position);
        x[f::VELOCITY..f::VELOCITY + 3].copy_from_slice(&body.velocity);
        x[f::YAW] = body.aim_yaw;
        x[f::PITCH] = body.aim_pitch;
        x[f::GROUND_STEP] = body.ground_adjustment;
        if self.life != Some(life) || before.is_none() {
            // A spawn or an authority pose without a replay interval cannot
            // interpolate from another life or from stale pending commands.
            x[f::COMMAND] = 1.;
            self.owner.frame();
        } else if let Some(before) = before {
            x[f::PREVIOUS_POSITION..f::PREVIOUS_POSITION + 3].copy_from_slice(&before.position);
            x[f::PREVIOUS_VELOCITY..f::PREVIOUS_VELOCITY + 3].copy_from_slice(&before.velocity);
            x[f::PREVIOUS_STEP] = before.ground_adjustment;
            // replayPrediction applied mouse counts before movement. Remove
            // only the fixed-step turn, leaving raw mouse look immediate.
            let turn = body.aim_yaw - before.aim_yaw - look_yaw;
            x[f::TICK_YAW] = V8::atan2(V8::sin(turn), V8::cos(turn));
            if body
                .position
                .iter()
                .zip(before.position)
                .map(|(a, b)| (a - b) * (a - b))
                .sum::<f64>()
                > 100.
            {
                x[f::COMMAND] = 1.;
                self.owner.frame();
            }
        }
        self.life = Some(life);
        let x = &mut self.owner.input;
        x[f::COMMAND] = 0.;
        x[f::ALPHA] = alpha;
        x[f::DT] = dt;
        self.owner.frame();
        let x = &self.owner.input;
        let mut shown = body.clone();
        shown
            .position
            .copy_from_slice(&x[f::SHOWN_POSITION..f::SHOWN_POSITION + 3]);
        shown
            .velocity
            .copy_from_slice(&x[f::SHOWN_VELOCITY..f::SHOWN_VELOCITY + 3]);
        shown.aim_yaw = x[f::SHOWN_YAW];
        shown.aim_pitch = x[f::SHOWN_PITCH];
        shown
    }
}

/// The player's own droideka as a frame draws it: where it is, how fast it
/// moves, and its pose (roll, lean, legs, clip time), with the mode whose
/// clip that time is in and whether it plays backward.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShownWalker {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub pose: [f64; 4],
    pub mode: u8,
    pub reverse: bool,
}
/// The player's own droideka between the server's corrections. Its body is
/// not replayed on this client, so the corrections are all that is known of
/// it: drawn as each arrived, it moved in steps (60 a second in Instant
/// Action) however many frames were drawn. A clock of its own runs a
/// correction and a quarter behind the newest (21 ms in Instant Action), at
/// the speed of the display, and the body is drawn between the two
/// corrections around it. His view angles do not wait: they follow the mouse.
///
/// As in the 2004 game, a droideka that unfolds against a wall it rolled up
/// to is pushed out of it in the unfold's first tick (up to a metre). The
/// part of that step its speed does not explain is drawn over a quarter of a
/// second instead, the body and so the camera that follows it; nothing is
/// hit or fired from the drawn body while it unfolds.
#[derive(Default)]
pub(super) struct Walker {
    life: Option<[f64; 3]>,
    ticks: [f64; Walker::SAMPLES],
    rows: [ShownWalker; Walker::SAMPLES],
    // Corrections held, oldest first.
    len: usize,
    clock: f64,
    // An unfold's push still being drawn: the ticks its step began and ended
    // at, and the push (the step less what the speed explains).
    push: Option<(f64, f64, [f64; 3])>,
}
impl Walker {
    const SAMPLES: usize = 4;
    /// Ticks over which an unfold's push out of a wall is drawn.
    const PUSH_TICKS: f64 = 15.;
    /// Nothing is drawn from what was heard before (the player left the droideka).
    pub fn forget(&mut self) {
        self.len = 0;
    }
    pub fn sample(&mut self, own: &super::receiver::Own, life: [f64; 3], dt: f64) -> ShownWalker {
        let heard = ShownWalker {
            position: own.position,
            velocity: own.velocity,
            pose: [own.walker_roll, own.walker_lean, own.walker_legs, own.walker_time],
            mode: own.walker_mode,
            reverse: own.walker_reverse,
        };
        // Another life, a clock that went back, or more than ten metres in
        // one correction (a spawn or a teleport): it is drawn where it is,
        // never slid there from the old place.
        let cut = self.life != Some(life)
            || self.len.checked_sub(1).is_some_and(|last| {
                let from = self.rows[last].position;
                own.tick < self.ticks[last] || (0..3).map(|k| (own.position[k] - from[k]).powi(2)).sum::<f64>() > 100.
            });
        if cut {
            self.len = 0;
            self.push = None;
        }
        self.life = Some(life);
        if self.len == 0 || own.tick > self.ticks[self.len - 1] {
            // Out of the ball into the unfold: its push out of a wall.
            if let Some(last) = self.len.checked_sub(1) {
                let (a, from) = (&self.rows[last], self.ticks[last]);
                if a.mode == bfe_pose::rig::droideka::BALL as u8 && heard.mode == bfe_pose::rig::droideka::UNFOLD as u8 {
                    let seconds = (own.tick - from) / 60.;
                    let push: [f64; 3] = std::array::from_fn(|k| heard.position[k] - a.position[k] - (a.velocity[k] + heard.velocity[k]) * 0.5 * seconds);
                    let length = push.iter().map(|v| v * v).sum::<f64>().sqrt();
                    self.push = (length > 0.02 && length < 3.).then_some((from, own.tick, push));
                }
            }
            if self.len == Self::SAMPLES {
                self.ticks.copy_within(1.., 0);
                self.rows.copy_within(1.., 0);
                self.len -= 1;
            }
            self.ticks[self.len] = own.tick;
            self.rows[self.len] = heard;
            self.len += 1;
        }
        let (oldest, newest) = (self.ticks[0], self.ticks[self.len - 1]);
        // The usual time between corrections: the middle one of the gaps held (the shorter of
        // two), so one long gap after a stall is not taken for the rhythm. Taken for it, the
        // clock was let fall a whole stall behind, and then snapped when the rhythm came back.
        let gap = |n: usize| self.ticks[n + 1] - self.ticks[n];
        let interval = match self.len {
            0 | 1 => 1.,
            2 => gap(0),
            3 => gap(0).min(gap(1)),
            _ => {
                let (a, b, c) = (gap(0), gap(1), gap(2));
                a.max(b).min(a.min(b).max(c))
            }
        };
        // A correction and a quarter behind the newest on average: the clock
        // then ends each interval three quarters of one short of the newest,
        // so a correction that comes most of an interval late does not stop
        // the body.
        let target = (newest - 1.25 * interval).max(oldest);
        let dt = dt.max(0.);
        self.clock += dt * 60.;
        if cut || !(target - 2. * interval..=newest).contains(&self.clock) {
            self.clock = target;
        } else {
            self.clock += (target - self.clock) * (1. - (-3. * dt).exp());
        }
        self.clock = self.clock.clamp(oldest, newest);
        let after = self.ticks[..self.len].iter().position(|t| *t > self.clock).unwrap_or(self.len);
        let mut shown = if after == 0 || after == self.len {
            self.rows[after.min(self.len - 1)]
        } else {
            let (a, b) = (&self.rows[after - 1], &self.rows[after]);
            let f = (self.clock - self.ticks[after - 1]) / (self.ticks[after] - self.ticks[after - 1]);
            let mix = |a: f64, b: f64| if a.is_finite() && b.is_finite() { a + (b - a) * f } else { b };
            // The clip runs on across a change of mode, as for any droideka drawn
            // between two samples (roll is an angle that is never wrapped).
            let (mode, time, reverse) = bfe_pose::rig::droideka::between(
                (a.mode as i32, a.pose[3], a.reverse),
                (b.mode as i32, b.pose[3], b.reverse),
                (self.clock - self.ticks[after - 1]) / 60.,
                (self.ticks[after] - self.clock) / 60.,
            );
            ShownWalker {
                position: std::array::from_fn(|k| mix(a.position[k], b.position[k])),
                velocity: std::array::from_fn(|k| mix(a.velocity[k], b.velocity[k])),
                pose: [mix(a.pose[0], b.pose[0]), mix(a.pose[1], b.pose[1]), mix(a.pose[2], b.pose[2]), time],
                mode: mode as u8,
                reverse,
            }
        };
        // Of the push, the part its step has drawn already is held back but
        // for what a quarter second's ease shows of it by now.
        if let Some((from, to, push)) = self.push {
            let since = self.clock - from;
            if since >= Self::PUSH_TICKS {
                self.push = None;
            } else if since > 0. {
                let x = since / Self::PUSH_TICKS;
                let (ease, done) = (x * x * (3. - 2. * x), (since / (to - from)).min(1.));
                for k in 0..3 {
                    shown.position[k] += push[k] * (ease - done);
                }
            }
        }
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_interpolates_movement_and_keyboard_turn_without_delaying_mouse() {
        let before = Soldier {
            position: [0.; 3],
            aim_yaw: 0.,
            ..Default::default()
        };
        let body = Soldier {
            position: [1., 0., 0.],
            aim_yaw: 0.6,
            ..Default::default()
        };
        let mut display = Display::default();
        display.sample(&before, None, 0., [1., 2., 3.], 1., 0.);
        let shown = display.sample(&body, Some(&before), 0.4, [1., 2., 3.], 0.5, 1. / 120.);
        assert_eq!(shown.position, [0.5, 0., 0.]);
        assert!((shown.aim_yaw - 0.5).abs() < 1e-12);
        assert_eq!(body.position, [1., 0., 0.]);
        let spawned = display.sample(&body, Some(&before), 0.4, [1., 2., 4.], 0., 0.);
        assert_eq!(spawned.position, body.position);
        assert_eq!(spawned.aim_yaw, body.aim_yaw);
    }

    // A keyboard turn across the +-pi seam is drawn along the short arc, and
    // a body that moved more than ten metres in a tick (a teleport) is drawn
    // where it is, never smeared across the gap.
    #[test]
    fn display_turns_the_short_way_and_cuts_on_a_teleport() {
        let life = [1., 2., 3.];
        let before = Soldier { aim_yaw: 3.1, ..Default::default() };
        let body = Soldier { position: [0.2, 0., 0.], aim_yaw: -3.1, ..Default::default() };
        let mut display = Display::default();
        display.sample(&before, None, 0., life, 1., 0.);
        // The tick turned by 2 pi - 6.2 (about 0.083) through the seam.
        let shown = display.sample(&body, Some(&before), 0., life, 0.5, 1. / 240.);
        let turn = std::f64::consts::TAU - 6.2;
        assert!((shown.aim_yaw - (-3.1 - turn / 2.)).abs() < 1e-12, "{}", shown.aim_yaw);
        assert!((shown.position[0] - 0.1).abs() < 1e-12);
        let far = Soldier { position: [30., 0., 0.], aim_yaw: -3.1, ..Default::default() };
        let shown = display.sample(&far, Some(&body), 0., life, 0.25, 1. / 240.);
        assert_eq!(shown.position, far.position);
        assert_eq!(shown.aim_yaw, far.aim_yaw);
    }

    // The player's droideka rolls at 6 m/s; a correction arrives every tick
    // and four frames are drawn per tick. Every frame must show it further
    // on, never more than two and a half ticks behind the newest correction, and a
    // respawn elsewhere is shown at once.
    #[test]
    fn the_own_droideka_moves_on_every_frame_between_corrections() {
        use crate::online::receiver::Own;
        let life = [1., 9., 1.];
        let mut walker = Walker::default();
        let mut own = Own { walker: true, alive: true, walker_mode: 12, ..Default::default() };
        let mut last = f64::NEG_INFINITY;
        for frame in 0..960 {
            if frame % 4 == 0 {
                own.tick += 1.;
                own.position[0] = own.tick * 0.1;
                own.walker_roll = own.tick * 0.2;
            }
            let shown = walker.sample(&own, life, 1. / 240.);
            if frame > 240 {
                assert!(shown.position[0] > last, "frame {frame}: {} after {last}", shown.position[0]);
                assert!(own.position[0] - shown.position[0] < 0.25, "frame {frame}: {} m behind", own.position[0] - shown.position[0]);
                assert!((shown.pose[0] - 2. * shown.position[0]).abs() < 1e-9);
            }
            last = shown.position[0];
        }
        // A clip that starts again, and the next mode's clip, show from where
        // they began (the newer correction's time counted back): with a
        // correction every tick the clip time falls only twice, each time to
        // the new run's, and the mode changes once, never back.
        let mut frames = |own: &Own, count: usize| -> Vec<ShownWalker> { (0..count).map(|_| walker.sample(own, life, 1. / 240.)).collect() };
        let tick = 1. / 60.;
        let mut shown = Vec::new();
        // The second change happens three quarters of a tick after the sample before it.
        for (mode, time) in [(7, 0.2), (7, 0.2 + tick), (7, 0.25), (7, 0.25 + tick), (7, tick), (7, 2. * tick), (8, 0.25 * tick), (8, 1.25 * tick), (8, 2.25 * tick)] {
            own.tick += 1.;
            own.walker_mode = mode;
            own.walker_time = time;
            shown.extend(frames(&own, 4));
        }
        let shown = &shown[8..];
        let falls: Vec<&[ShownWalker]> = shown.windows(2).filter(|pair| pair[1].pose[3] < pair[0].pose[3]).collect();
        let changes = shown.windows(2).filter(|pair| pair[1].mode != pair[0].mode).count();
        assert!(falls.len() == 2 && falls.iter().all(|pair| pair[1].pose[3] <= tick + 1e-12), "{shown:?}");
        assert!(changes == 1 && shown.last().is_some_and(|last| last.mode == 8), "{shown:?}");
        // Until then the old step is still drawn, its clip running on past its last sample.
        assert!(shown.iter().any(|walker| walker.mode == 7 && walker.pose[3] > 2. * tick + 1e-9), "{shown:?}");
        // The corrections stall for 15 ticks, then come again every tick: the body is where the
        // newest correction puts it, less the usual delay, as soon as that correction is drawn
        // from; it is not left a whole stall behind for a tick first.
        let mut frames = |own: &Own, count: usize| -> Vec<ShownWalker> { (0..count).map(|_| walker.sample(own, life, 1. / 240.)).collect() };
        own.walker_mode = 12;
        for _ in 0..8 {
            own.tick += 1.;
            own.position[0] += 0.1;
            frames(&own, 4);
        }
        own.tick += 15.;
        own.position[0] += 1.5;
        let after_stall = frames(&own, 4)[3].position[0];
        assert!(own.position[0] - after_stall < 0.25, "{} m behind after the stall", own.position[0] - after_stall);
        own.tick += 1.;
        own.position[0] += 0.1;
        let next = frames(&own, 4);
        assert!(next.windows(2).all(|pair| pair[1].position[0] >= pair[0].position[0]) && next[0].position[0] >= after_stall);
        assert!(own.position[0] - next[3].position[0] < 0.25);
        // Spawned again 50 m away: there at once.
        own.tick += 1.;
        own.position = [50., 0., 0.];
        assert_eq!(walker.sample(&own, [1., 9., 2.], 1. / 240.).position, [50., 0., 0.]);
        // The same life moved 50 m in a tick (a teleport): also at once.
        own.tick += 1.;
        own.position = [0., 0., 50.];
        assert_eq!(walker.sample(&own, [1., 9., 2.], 1. / 240.).position, [0., 0., 50.]);
    }

    // Unfolding against a wall, the world pushes the droideka 0.9 m out of it
    // in one tick. Drawn four frames a tick, the body takes a quarter second
    // over it (never more than 0.09 m a tick: smoothstep's steepest part),
    // where the step drawn as it came took 0.23 m a frame, and ends where the
    // world has it.
    #[test]
    fn an_unfold_out_of_a_wall_is_drawn_over_a_quarter_second() {
        use crate::online::receiver::Own;
        let life = [1., 9., 1.];
        let mut walker = Walker::default();
        let mut own = Own { walker: true, alive: true, walker_mode: 12, ..Default::default() };
        let mut drawn = Vec::new();
        for frame in 0..400 {
            if frame % 4 == 0 {
                own.tick += 1.;
                if own.tick == 30. {
                    own.walker_mode = 13;
                    own.position[2] = -0.9;
                }
            }
            drawn.push(walker.sample(&own, life, 1. / 240.).position[2]);
        }
        let steps: Vec<f64> = drawn.windows(2).map(|pair| (pair[1] - pair[0]).abs()).collect();
        let most = steps.iter().fold(0., |most: f64, step| most.max(*step));
        assert!(most < 1.5 * 0.9 / 15. * 0.3, "{most} m in a frame");
        assert!(steps.iter().filter(|step| **step > 1e-9).count() >= 4 * 14);
        assert_eq!(drawn.last(), Some(&-0.9));
    }
}
