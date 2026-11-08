//! SoldierPresentation's retained interval and own-player displayed frame.
//! Tick observations never modify authoritative bodies. Stairs have a separate
//! visual offset, and accumulated rolling angles must never wrap.
//!
//! Rendering samples the last simulation interval without changing collision,
//! weapon aim, or input. Mouse look remains immediate; only turns produced by
//! the fixed physics tick are interpolated. Small stair corrections get a
//! separate visual offset shared by the body, held weapon, and camera.
//!
//! The own player's displayed pose has a separate lifetime from collision and
//! prediction. beforeStep observes the beginning of a fixed interval; afterStep
//! observes its final keyboard turn. Neither observation smooths the live body.
//! update samples that interval for the current display alpha and elapsed time.
//! The shared body phase continues to consume the published interval mirrors.
//!
//! Yaw interpolation removes only the fixed tick's turn from immediate look.
//! A mouse turn arriving between observations therefore remains immediate.
//! The tick delta uses atan2(sin, cos), preserving the short path at the wrap.
//! A correction may adjust previous yaw before the interval observation; that
//! input is retained explicitly rather than reconstructed from the last display.
//!
//! Walker roll is an accumulated angle and must not be wrapped. Lean, legs and
//! clip time interpolate only within the same mode with finite endpoints. A
//! restarted clip time takes its current value immediately. Reverse playback
//! uses the opposite restart comparison so a fold/unfold transition cannot
//! interpolate from an unrelated clip clock.
//!
//! Ground adjustment has a separate bounded offset. Each displayed frame adds
//! the interpolated correction delta, then decays that offset exponentially.
//! The final y subtraction changes only the shown feet, camera and held weapon.
//! Zero-time draws do not decay the offset or introduce additional stair motion.
//! A displacement over ten metres resets the complete interval and clears visual
//! lag, instead of interpolating a spawn or teleport across the old position.

use crate::{
    unit_effects::{lerp, put, vector},
    visual_effects_layout::soldier_frame as f,
};
pub struct Soldier {
    pub input: Vec<f64>,
}
impl Default for Soldier {
    fn default() -> Self {
        Self {
            input: vec![0.; f::STRIDE],
        }
    }
}
impl Soldier {
    fn capture(&mut self) {
        let x = &mut self.input;
        for k in 0..3 {
            x[f::PREVIOUS_POSITION + k] = x[f::POSITION + k];
            x[f::PREVIOUS_VELOCITY + k] = x[f::VELOCITY + k];
        }
        x[f::PREVIOUS_YAW] = x[f::YAW];
        x[f::PREVIOUS_STEP] = x[f::GROUND_STEP];
        x[f::PREVIOUS_MODE] = x[f::MODE];
        for k in 0..4 {
            x[f::PREVIOUS_POSE + k] = x[f::POSE + k];
        }
    }
    fn reset(&mut self) {
        self.capture();
        let x = &mut self.input;
        x[f::TICK_YAW] = 0.;
        x[f::LAST_STEP] = x[f::GROUND_STEP];
        x[f::STEP_OFFSET] = 0.;
        for k in 0..3 {
            x[f::SHOWN_POSITION + k] = x[f::POSITION + k];
            x[f::SHOWN_VELOCITY + k] = x[f::VELOCITY + k];
        }
        x[f::SHOWN_YAW] = x[f::YAW];
        x[f::SHOWN_PITCH] = x[f::PITCH];
        for k in 0..4 {
            x[f::SHOWN_POSE + k] = x[f::POSE + k];
        }
    }
    pub fn frame(&mut self) {
        let command = self.input[f::COMMAND];
        self.input[f::TELEPORTED] = 0.;
        if command == 1. {
            self.reset();
            return;
        }
        if command == 2. {
            self.capture();
            return;
        }
        if command == 3. {
            let x = &mut self.input;
            let delta = x[f::YAW] - x[f::PREVIOUS_YAW];
            x[f::TICK_YAW] = v8math::atan2(v8math::sin(delta), v8math::cos(delta));
            if vector(x, f::POSITION).distance_to_squared(&vector(x, f::PREVIOUS_POSITION)) > 100. {
                self.reset();
                self.input[f::TELEPORTED] = 1.;
            }
            return;
        }
        let x = &mut self.input;
        let alpha = x[f::ALPHA].clamp(0., 1.);
        let mut p = vector(x, f::POSITION);
        p.lerp_vectors(
            &vector(x, f::PREVIOUS_POSITION),
            &vector(x, f::POSITION),
            alpha,
        );
        let mut velocity = vector(x, f::VELOCITY);
        velocity.lerp_vectors(
            &vector(x, f::PREVIOUS_VELOCITY),
            &vector(x, f::VELOCITY),
            alpha,
        );
        put(x, f::SHOWN_VELOCITY, velocity);
        x[f::SHOWN_YAW] = x[f::YAW] - x[f::TICK_YAW] * (1. - alpha);
        x[f::SHOWN_PITCH] = x[f::PITCH];
        for k in 0..4 {
            let previous = x[f::PREVIOUS_POSE + k];
            let current = x[f::POSE + k];
            let same =
                x[f::PREVIOUS_MODE] == x[f::MODE] && previous.is_finite() && current.is_finite();
            let restart = k == 3
                && if x[f::REVERSE] != 0. {
                    current > previous
                } else {
                    current < previous
                };
            x[f::SHOWN_POSE + k] = if same && !restart {
                lerp(previous, current, alpha)
            } else {
                current
            };
        }
        let step = lerp(x[f::PREVIOUS_STEP], x[f::GROUND_STEP], alpha);
        x[f::STEP_OFFSET] = (x[f::STEP_OFFSET] + step - x[f::LAST_STEP]).clamp(-0.45, 0.45);
        x[f::LAST_STEP] = step;
        x[f::STEP_OFFSET] *= v8math::exp(-x[f::DT].max(0.) / 0.075);
        p.y -= x[f::STEP_OFFSET];
        put(x, f::SHOWN_POSITION, p);
    }
}
