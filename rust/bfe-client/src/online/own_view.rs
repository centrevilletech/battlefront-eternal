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
}
