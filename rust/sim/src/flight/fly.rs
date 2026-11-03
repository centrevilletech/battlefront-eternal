//! A flyer's tick (src/vehicles.js `Vehicle.tick`, its flyer path, and
//! `Vehicle.fly`): the last pose moved on, then, for a craft that is alive
//! and awake or manned, its crew's aim and its flight: pilotless, or the
//! flight model on its pilot's input (a network player's flight keys, a
//! bot's frozen stick, or nobody's), then its targeting.

use super::model::{or_zero, truthy, FlightInput};
use super::{FlightState, FlightWorld};
use crate::vehicles::Hull;

impl Hull {
    /// captureTick's flyer part (vehicles.js captureTick): the frame at the
    /// tick's start, which a page draws the craft from between ticks
    /// (`flightTickQ`). The hulls' captureTick takes the rest.
    pub fn capture_flight_tick(&mut self) {
        if self.flying {
            self.flight.tick_q = self.quaternion;
        }
    }

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

/// What a flyer's shot carries of its lock (vehicles.js emitShot): the
/// locked target's id once the lock is Locked (-1 before), and the lock's
/// generation (whatever its state).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotTarget {
    pub target_id: f64,
    pub target_generation: f64,
}

/// What a flyer's missile is given after it is fired (vehicles.js emitShot,
/// through the rounds' `Cx::flight_missile`): it steers by its class's
/// seeker, lives the class's missile lifetime, and flies at its missile
/// speed along its velocity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyerMissile {
    pub class: super::FlightClass,
    pub life: f64,
    pub speed: f64,
}

impl Hull {
    /// emitShot's `shot.targetId` and `shot.targetGeneration` for a flyer
    /// (None for a ground hull, whose shots keep their own).
    pub fn flyer_shot_target(&self) -> Option<ShotTarget> {
        if !self.flying {
            return None;
        }
        let f = &self.flight;
        Some(ShotTarget {
            target_id: if f.lock_state == super::LockState::Locked { f.lock_id } else { -1.0 },
            target_generation: f.lock_generation,
        })
    }

    /// emitShot's tweaks to a missile a flyer fired (None for a ground
    /// hull): its class (`round.flightClass`), and its class tuning's
    /// missile lifetime and speed (`flightConfig.missileLifetime`,
    /// `missileSpeed`, the live tuning's).
    pub fn flyer_missile<W: FlightWorld>(&self, w: &mut W) -> Option<FlyerMissile> {
        if !self.flying {
            return None;
        }
        let t = w.tuning(self.flight.class);
        Some(FlyerMissile { class: self.flight.class, life: t.missile_lifetime, speed: t.missile_speed })
    }
}

impl Hull {
    /// initFlight (src/flight/model.js) for a craft named `name` (its class
    /// by flightClass) whose definition's keys are `config`: the hull's turn
    /// rate and top speed from the configuration, then its flight as a
    /// craft landed with full power (state.rs Flight::init).
    pub fn init_flight(&mut self, name: &str, config: super::Config) {
        self.turn_rate = config.turn_rate;
        self.speed = config.max_speed;
        let quaternion = self.quaternion;
        self.flight.init(super::config::flight_class(name), config, &quaternion);
    }

    /// Vehicle.spawn's flight part, in its order (vehicles.js spawn): every
    /// hull grounded as a craft, landed or with no flight state, its flight
    /// numbers 0; a craft turned to its pad's heading and given its flight
    /// (`init_flight`); the floor under it unknown (NaN). The caller sets
    /// the yaw first and the rest of the spawn around these.
    pub fn spawn_flight(&mut self, name: &str, config: super::Config) {
        self.flight.grounded = self.flying;
        self.flight.state = if self.flying { Some(FlightState::Landed) } else { None };
        self.flight.altitude = 0.0;
        self.flight.throttle = 0.0;
        self.flight.airspeed = 0.0;
        self.tilt_rate = 0.0;
        self.flight.state_time = 0.0;
        self.flight.bank = 0.0;
        if self.flying {
            let mut q = three::Quaternion::default();
            q.set_from_axis_angle(&three::Vector3::new(0.0, 1.0, 0.0), self.yaw);
            self.quaternion = q;
            self.init_flight(name, config);
        }
        self.flight.floor_y = f64::NAN;
    }
}
