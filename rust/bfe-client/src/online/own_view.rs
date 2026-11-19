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
/// moves, and its pose (roll, lean, legs, clip time).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShownWalker {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub pose: [f64; 4],
}
#[derive(Clone, Copy, Default)]
struct WalkerSample {
    shown: ShownWalker,
    mode: u8,
    reverse: bool,
}
/// The player's own droideka between the server's corrections. Its body is
/// not replayed on this client, so the corrections are all that is known of
/// it: drawn as each arrived, it moved in steps (60 a second in Instant
/// Action) however many frames were drawn. A clock of its own runs a
/// correction and a quarter behind the newest (21 ms in Instant Action), at
/// the speed of the display, and the body is drawn between the two
/// corrections around it. His view angles do not wait: they follow the mouse.
#[derive(Default)]
pub(super) struct Walker {
    life: Option<[f64; 3]>,
    ticks: [f64; Walker::SAMPLES],
    rows: [WalkerSample; Walker::SAMPLES],
    // Corrections held, oldest first.
    len: usize,
    clock: f64,
}
impl Walker {
    const SAMPLES: usize = 4;
    /// Nothing is drawn from what was heard before (the player left the droideka).
    pub fn forget(&mut self) {
        self.len = 0;
    }
    pub fn sample(&mut self, own: &super::receiver::Own, life: [f64; 3], dt: f64) -> ShownWalker {
        let heard = WalkerSample {
            shown: ShownWalker {
                position: own.position,
                velocity: own.velocity,
                pose: [own.walker_roll, own.walker_lean, own.walker_legs, own.walker_time],
            },
            mode: own.walker_mode,
            reverse: own.walker_reverse,
        };
        // Another life, a clock that went back, or more than ten metres in
        // one correction (a spawn or a teleport): it is drawn where it is,
        // never slid there from the old place.
        let cut = self.life != Some(life)
            || self.len.checked_sub(1).is_some_and(|last| {
                let from = self.rows[last].shown.position;
                own.tick < self.ticks[last] || (0..3).map(|k| (own.position[k] - from[k]).powi(2)).sum::<f64>() > 100.
            });
        if cut {
            self.len = 0;
        }
        self.life = Some(life);
        if self.len == 0 || own.tick > self.ticks[self.len - 1] {
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
        let interval = if self.len > 1 { newest - self.ticks[self.len - 2] } else { 1. };
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
        if after == 0 || after == self.len {
            return self.rows[after.min(self.len - 1)].shown;
        }
        let (a, b) = (&self.rows[after - 1], &self.rows[after]);
        let f = (self.clock - self.ticks[after - 1]) / (self.ticks[after] - self.ticks[after - 1]);
        let mix = |a: f64, b: f64| a + (b - a) * f;
        // The pose blends only within one mode, and a clip that started
        // again takes its new time at once (roll is an angle that is never
        // wrapped), as for any droideka drawn between two samples.
        let restarted = if b.reverse { b.shown.pose[3] > a.shown.pose[3] } else { b.shown.pose[3] < a.shown.pose[3] };
        ShownWalker {
            position: std::array::from_fn(|k| mix(a.shown.position[k], b.shown.position[k])),
            velocity: std::array::from_fn(|k| mix(a.shown.velocity[k], b.shown.velocity[k])),
            pose: std::array::from_fn(|k| {
                let (from, to) = (a.shown.pose[k], b.shown.pose[k]);
                if a.mode == b.mode && from.is_finite() && to.is_finite() && !(k == 3 && restarted) { mix(from, to) } else { to }
            }),
        }
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
        // A clip that starts again is not run backwards to its start: its
        // time only ever drops straight to the new clip's. (Four frames a
        // tick as before, then the clock runs up to the newest correction.)
        let mut frames = |own: &Own, count: usize| -> Vec<ShownWalker> { (0..count).map(|_| walker.sample(own, life, 1. / 240.)).collect() };
        own.tick += 1.;
        own.walker_time = 0.5;
        frames(&own, 4);
        own.tick += 1.;
        own.walker_time = 0.1;
        let times: Vec<f64> = frames(&own, 16).iter().map(|shown| shown.pose[3]).collect();
        assert!(times.windows(2).all(|pair| pair[1] >= pair[0] || pair[1] == 0.1), "{times:?}");
        assert_eq!(times[15], 0.1);
        // Another mode takes its pose whole, with no roll in between.
        own.tick += 1.;
        own.walker_mode = 3;
        own.walker_roll = 0.;
        let rolls: Vec<f64> = frames(&own, 16).iter().map(|shown| shown.pose[0]).collect();
        assert!(rolls.windows(2).all(|pair| pair[1] >= pair[0] || pair[1] == 0.), "{rolls:?}");
        assert_eq!(rolls[15], 0.);
        // Spawned again 50 m away: there at once.
        own.tick += 1.;
        own.position = [50., 0., 0.];
        assert_eq!(walker.sample(&own, [1., 9., 2.], 1. / 240.).position, [50., 0., 0.]);
        // The same life moved 50 m in a tick (a teleport): also at once.
        own.tick += 1.;
        own.position = [0., 0., 50.];
        assert_eq!(walker.sample(&own, [1., 9., 2.], 1. / 240.).position, [0., 0., 50.]);
    }
}
