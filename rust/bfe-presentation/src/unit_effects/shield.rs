//! ShieldBubble place/hit/update/hide and its camera-inside publication.
//! Absorption is an immediate observation, so a long pause cannot overflow
//! an event queue or lose the latest six directional impacts.
//!

//!
//! Gameplay owns charge and absorption. This owner observes successful hits;
//! a decrease in charge alone must never fabricate a localized impact.
//! A hit is applied immediately, including while no displayed update runs.
//! There is no paused-frame queue to grow, overflow or replay at activation.
//! Only the latest six directional hits are retained in circular order.
//!
//! Hit power depends on absorbed damage relative to maximum charge. A missing
//! or non-finite source contributes an undirected flash instead of inventing a
//! side. Directional sources are expressed in the field's ellipsoid coordinates,
//! then normalized; a source at the center uses the original upward direction.
//! Explosion provenance is a separate flag, retained with the same hit slot.
//!
//! The field's center follows the drawn body plus the yaw-rotated ShieldOffset.
//! It uses the body's displayed position, not authoritative collision position,
//! so foot smoothing and network interpolation do not detach the bubble.
//! Scale combines authored radius/height with the current activation expansion.
//! The small minimum scale preserves the shell during the last hidden frame.
//!
//! Classic activation grows past its final size before settling. Enhanced mode
//! retains the fade and expansion from each interrupted transition, so reversing
//! activation at zero elapsed time cannot jump the visible shell. Fade and pop
//! are distinct values: the former controls opacity, the latter dimensions.
//! The activation birth clock restarts on an off-to-on transition only.
//!
//! Hide is a body-lifetime observation. It immediately removes the field rather
//! than advancing an off animation for an actor no longer on the battlefield.
//! The next update can observe a new activation normally. Camera preparation
//! only computes inside/outside from the already published center and scale;
//! it cannot advance fade, flash decay, charge or any hit history.

//!
//! ShieldFrame carries both observations and publications in one fixed row.
//! Hit and hide commands do not consume dt. Only an ordinary displayed update
//! advances the field clock, birth clock and exponential flash decay.
//!
//! Impact positions remain double precision because they feed the field's
//! uniform history. Power and explosion flags retain the original float32
//! boundary. Wrapping the hit ordinal changes only which of six rows is replaced.
//!
//! Inside/outside uses the camera relative to the final drawn ellipsoid. It
//! never depends on the current gameplay charge or collision shield radius.

use super::{put, vector};
use crate::visual_effects_layout::shield_frame as f;
use three::Vector3;
pub struct Shield {
    pub input: Vec<f64>,
    pub impacts: Vec<f64>,
    pub powers: Vec<f32>,
    pub blasts: Vec<f32>,
    on: bool,
    from_pop: f64,
    from_fade: f64,
}
impl Default for Shield {
    fn default() -> Self {
        let mut input = vec![0.; f::STRIDE];
        input[f::BIRTH] = 10.;
        input[f::SCALE..f::SCALE + 3].fill(1.);
        let mut impacts = vec![0.; 24];
        for i in 0..6 {
            impacts[i * 4 + 2] = 1.;
            impacts[i * 4 + 3] = -1000.;
        }
        Self {
            input,
            impacts,
            powers: vec![0.; 6],
            blasts: vec![0.; 6],
            on: false,
            from_pop: 0.,
            from_fade: 0.,
        }
    }
}
impl Shield {
    fn place(&mut self) {
        let p = vector(&self.input, f::POSITION);
        let offset = vector(&self.input, f::OFFSET);
        let yaw = self.input[f::YAW];
        put(
            &mut self.input,
            f::CENTER,
            Vector3::new(
                p.x + (v8math::cos(yaw) * offset.x + v8math::sin(yaw) * offset.z),
                p.y + offset.y,
                p.z + (-v8math::sin(yaw) * offset.x + v8math::cos(yaw) * offset.z),
            ),
        );
    }
    pub fn frame(&mut self) {
        // Command 1 is an observed hit; 2 hides a removed body. Neither advances
        // the clock or invents a shield transition between displayed frames.
        let command = self.input[f::COMMAND];
        self.input[f::COMMAND] = 0.;
        if command == 2. {
            self.on = false;
            self.input[f::FADE] = 0.;
            self.input[f::POP] = 0.;
            self.input[f::VISIBLE] = 0.;
            return;
        }
        if command == 3. {
            let mut local = vector(&self.input, f::CAMERA);
            local
                .sub(&vector(&self.input, f::CENTER))
                .divide(&vector(&self.input, f::SCALE));
            self.input[f::INSIDE] = (local.length_sq() < 1.) as u8 as f64;
            return;
        }
        if command == 1. {
            let amount = self.input[f::AMOUNT];
            if !(amount > 0.) {
                return;
            }
            self.place();
            let power = ((amount / self.input[f::MAX].max(1.)).sqrt() * 3.).clamp(0.25, 1.5);
            let source = vector(&self.input, f::SOURCE);
            if self.input[f::HAS_SOURCE] == 0.
                || !source.x.is_finite()
                || !source.y.is_finite()
                || !source.z.is_finite()
            {
                self.input[f::FLASH] = self.input[f::FLASH].max(power);
                return;
            }
            let mut local = source;
            local.sub(&vector(&self.input, f::CENTER));
            local.x /= self.input[f::RADIUS];
            local.y /= self.input[f::HEIGHT];
            local.z /= self.input[f::RADIUS];
            if local.length_sq() < 1e-8 {
                local.set(0., 1., 0.);
            }
            local.normalize();
            let i = (self.input[f::HIT_INDEX] as usize) % 6;
            self.input[f::HIT_INDEX] += 1.;
            put(&mut self.impacts, i * 4, local);
            self.impacts[i * 4 + 3] = self.input[f::TIME];
            self.powers[i] = power as f32;
            self.blasts[i] = self.input[f::EXPLOSION] as f32;
            return;
        }
        let dt = self.input[f::DT].max(0.);
        self.input[f::TIME] += dt;
        let on =
            self.input[f::ON] != 0. && self.input[f::DEAD] == 0. && self.input[f::ROLLED] == 0.;
        let enhanced = self.input[f::ENHANCED] != 0.;
        if on != self.on {
            self.from_pop = self.input[f::POP];
            self.from_fade = self.input[f::FADE];
            if on {
                self.input[f::BIRTH] = 0.;
            }
        }
        self.on = on;
        self.input[f::BIRTH] += dt;
        let duration = if enhanced {
            if on {
                0.24
            } else {
                0.26
            }
        } else if on {
            self.input[f::FADE_IN]
        } else {
            self.input[f::FADE_OUT]
        };
        let duration = if duration == 0. || duration.is_nan() {
            0.15
        } else {
            duration
        };
        let fade =
            (self.input[f::FADE] + (if on { dt } else { -dt }) / duration.max(1e-3)).clamp(0., 1.);
        self.input[f::FADE] = fade;
        self.input[f::VISIBLE] = (fade > 0.) as u8 as f64;
        self.input[f::AURA] = (enhanced && fade > 0.) as u8 as f64;
        self.place();
        let classic = if on {
            if fade < 0.94 {
                fade * 1.06 / 0.94
            } else {
                1. + (1. - fade)
            }
        } else {
            fade
        };
        let progress = if on {
            (fade - self.from_fade) / (1. - self.from_fade).max(1e-4)
        } else {
            fade / self.from_fade.max(1e-4)
        };
        let expansion = 1.
            + 2.2 * v8math::pow_legacy(progress - 1., 3.)
            + 1.2 * v8math::pow_legacy(progress - 1., 2.);
        let pop = if enhanced {
            if on {
                self.from_pop + (1. - self.from_pop) * expansion
            } else {
                self.from_pop * (0.7 + 0.3 * progress)
            }
        } else {
            classic
        };
        self.input[f::POP] = if fade > 0. { pop } else { 0. };
        // Preserve the original minimum shell scale even on the last hidden frame.
        let radius = self.input[f::RADIUS];
        let height = self.input[f::HEIGHT];
        put(
            &mut self.input,
            f::SCALE,
            Vector3::new(
                radius * pop.max(0.001),
                height * pop.max(0.001),
                radius * pop.max(0.001),
            ),
        );
        self.input[f::FLASH] *= v8math::exp(-10. * dt);
        self.input[f::CHARGE] = if self.input[f::MAX] > 0. {
            (self.input[f::VALUE] / self.input[f::MAX]).clamp(0., 1.)
        } else {
            0.
        };
    }
}
