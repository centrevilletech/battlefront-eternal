//! A flyer's tick (src/vehicles.js `Vehicle.tick`, its flyer path, and
//! `Vehicle.fly`): the last pose moved on, then, for a craft that is alive
//! and awake or manned, its crew's aim and its flight: pilotless, or the
//! flight model on its pilot's input (a network player's flight keys, a
//! bot's frozen stick, or nobody's), then its targeting.

use super::model::{or_zero, truthy, FlightInput};
use super::{FlightState, FlightWorld};
use crate::vehicles::Hull;

impl Hull {
    /// Vehicle.tick for a flyer (Vehicles.update steps flyers at 1/60 s).
    /// `input`: what the tick is handed (the pilot bot's SeatInput, or an
    /// idle stick); `network`: a network pilot's flight keys
    /// (`lease.intent.flight`, where his lease names a network player and
    /// that is an object).
    pub fn flyer_tick<W: FlightWorld>(&mut self, w: &mut W, dt: f64, input: &FlightInput, network: Option<&FlightInput>) {
        self.previous = self.position;
        self.previous_yaw = self.yaw;
        self.previous_tilt = self.tilt;
        self.previous_roll = self.roll;
        for wheel in &mut self.wheels {
            wheel.previous = wheel.phase;
        }
        if !self.alive || self.sleeping == Some(true) && !self.occupied() {
            return;
        }
        self.aim_crew(dt);
        self.fly(w, dt, input, network);
    }

    /// Vehicle.fly.
    pub fn fly<W: FlightWorld>(&mut self, w: &mut W, dt: f64, input: &FlightInput, network: Option<&FlightInput>) {
        if self.driver && self.seat_index != 0 {
            self.aim_player_seat();
        }
        // Nobody aboard a craft in flight: it comes down.
        if self.flight.state == Some(FlightState::Flying) && !self.occupied() {
            self.step_pilotless(w, dt);
            return;
        }
        if !self.occupied() {
            self.abandon_lift();
        }
        let step = match network {
            Some(keys) => *keys,
            None if self.crewed(0) => {
                // { ...input, stickX: input.turn || 0, stickY: input.pitch
                // || 0, takeoff: input.up || input.down, boost, bot: true }
                let mut bot = *input;
                bot.stick_x = Some(or_zero(input.turn));
                bot.stick_y = Some(or_zero(input.pitch));
                bot.takeoff = if truthy(input.up) { input.up } else { input.down };
                bot.bot = 1.0;
                bot
            }
            None => *input,
        };
        self.step_flight(w, dt, &step);
        // A bot presses the lock whenever he holds none; a person by his key.
        // (Read after the step: a crash lets the crew go.)
        let request = if self.crewed(0) && network.is_none() { self.flight.lock_id < 0.0 } else { truthy(step.target_lock) };
        self.update_flight_target(w, dt, request);
    }
}
