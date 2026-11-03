//! A hull's flight between the store (`gen::Vehicle`'s flight fields,
//! rust/sim/schema/flight.mjs) and the tick's working copy (`Flight`): the
//! texts as enums, a craft's class definition's keys and takeoff clip from
//! the content.

use super::config::Config;
use super::state::{Flight, GearClip, LockState, Message, Trick};
use crate::gen::Vehicle;
use crate::store::{StrId, Strings};
use three::{Quaternion, Vector3};

fn v3(a: &[f64; 3]) -> Vector3 {
    Vector3::new(a[0], a[1], a[2])
}

fn a3(v: &Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn q4(a: &[f64; 4]) -> Quaternion {
    Quaternion::new(a[0], a[1], a[2], a[3])
}

fn a4(q: &Quaternion) -> [f64; 4] {
    [q.x, q.y, q.z, q.w]
}

fn text(strings: &Strings, id: Option<StrId>) -> String {
    id.map(|id| String::from_utf16_lossy(strings.units(id))).unwrap_or_default()
}

impl Flight {
    /// A hull's flight as the store holds it; a craft's class definition's
    /// keys (`config`) and takeoff clip from its class. Err for a text no
    /// craft shows.
    pub fn from_store(v: &Vehicle, strings: &Strings, config: Config, gear_clip: Option<GearClip>) -> Result<Flight, String> {
        let message = |id: Option<StrId>, what: &str| {
            let t = text(strings, id);
            Message::from_text(&t).ok_or_else(|| format!("{what}: no message {t:?}"))
        };
        let nan = f64::NAN;
        Ok(Flight {
            state: v.flight_state,
            grounded: v.grounded,
            altitude: v.altitude,
            throttle: v.throttle,
            airspeed: v.airspeed,
            state_time: v.state_time,
            floor_y: v.floor_y,
            bank: v.bank,
            gear_foot: v.gear_foot,
            radius: v.radius,
            team: v.team,
            class: v.flight_class.unwrap_or_default(),
            config,
            min_speed: v.min_speed.unwrap_or(nan),
            mid_speed: v.mid_speed.unwrap_or(nan),
            boost_speed: v.boost_speed.unwrap_or(nan),
            power: v.power.unwrap_or(nan),
            stick_x: v.stick_x.unwrap_or(nan),
            stick_y: v.stick_y.unwrap_or(nan),
            roll_rate: v.flight_roll_rate.unwrap_or(nan),
            roll_lock: v.roll_lock.unwrap_or(false),
            boosting: v.boosting.unwrap_or(false),
            trick: {
                let t = text(strings, v.trick);
                Trick::from_text(&t).ok_or_else(|| format!("trick: no trick {t:?}"))?
            },
            trick_time: v.trick_time.unwrap_or(nan),
            trick_cooldown: v.trick_cooldown.unwrap_or(nan),
            trick_pending: v.trick_pending.unwrap_or(nan),
            trick_direction: v.trick_direction.unwrap_or(nan),
            gear: v.gear.unwrap_or(nan),
            landing_message: message(v.landing_message, "landingMessage")?,
            landing_message_time: v.landing_message_time.unwrap_or(nan),
            trick_right: v.flight_trick_right.as_ref().map(v3).unwrap_or_default(),
            clock: v.flight_clock.unwrap_or(nan),
            boost_exhausted: v.boost_exhausted.unwrap_or(false),
            landing_reason: message(v.landing_reason, "landingReason")?,
            warning: message(v.flight_warning, "flightWarning")?,
            terrain_warning: v.terrain_warning.unwrap_or(false),
            assist_id: v.flight_assist_id.unwrap_or(nan),
            lock_id: v.flight_lock_id.unwrap_or(nan),
            lock_generation: v.flight_lock_generation.unwrap_or(nan),
            lock_timer: v.flight_lock_timer.unwrap_or(nan),
            lost_los: v.flight_lost_los.unwrap_or(nan),
            lock_state: {
                let t = text(strings, v.flight_lock_state);
                LockState::from_text(&t).ok_or_else(|| format!("flightLockState: no state {t:?}"))?
            },
            start: v.flight_start.as_ref().map(v3).unwrap_or_default(),
            end: v.flight_end.as_ref().map(v3).unwrap_or_default(),
            start_q: v.flight_start_q.as_ref().map(q4).unwrap_or_default(),
            trick_loop_q: v.flight_trick_loop_q.as_ref().map(q4).unwrap_or_default(),
            trick_end_q: v.flight_trick_end_q.as_ref().map(q4).unwrap_or_default(),
            previous_q: v.flight_previous_q.as_ref().map(q4).unwrap_or_default(),
            tick_q: v.flight_tick_q.as_ref().map(q4).unwrap_or_default(),
            action_held: v.flight_action_held,
            trick_held: v.trick_held,
            immelmann_held: v.immelmann_held,
            frame_q: v.flight_frame_q.as_ref().map(q4),
            gear_clip,
            lock: None,
        })
    }

    /// What the tick changed, back into the store's hull: every hull's
    /// fields, and a craft's own (a ground hull has none of them).
    pub fn to_store(&self, v: &mut Vehicle, strings: &mut Strings) {
        v.flight_state = self.state;
        v.grounded = self.grounded;
        v.altitude = self.altitude;
        v.throttle = self.throttle;
        v.airspeed = self.airspeed;
        v.state_time = self.state_time;
        v.floor_y = self.floor_y;
        v.bank = self.bank;
        if !v.flying || v.flight_class.is_none() {
            return;
        }
        let mut intern = |s: &str| Some(strings.intern_str(s));
        v.power = Some(self.power);
        v.stick_x = Some(self.stick_x);
        v.stick_y = Some(self.stick_y);
        v.flight_roll_rate = Some(self.roll_rate);
        v.roll_lock = Some(self.roll_lock);
        v.boosting = Some(self.boosting);
        v.trick = intern(self.trick.text());
        v.trick_time = Some(self.trick_time);
        v.trick_cooldown = Some(self.trick_cooldown);
        v.trick_pending = Some(self.trick_pending);
        v.trick_direction = Some(self.trick_direction);
        v.gear = Some(self.gear);
        v.landing_message = intern(self.landing_message.text());
        v.landing_message_time = Some(self.landing_message_time);
        v.flight_trick_right = Some(a3(&self.trick_right));
        v.flight_clock = Some(self.clock);
        v.boost_exhausted = Some(self.boost_exhausted);
        v.landing_reason = intern(self.landing_reason.text());
        v.flight_warning = intern(self.warning.text());
        v.terrain_warning = Some(self.terrain_warning);
        v.flight_assist_id = Some(self.assist_id);
        v.flight_lock_id = Some(self.lock_id);
        v.flight_lock_generation = Some(self.lock_generation);
        v.flight_lock_timer = Some(self.lock_timer);
        v.flight_lost_los = Some(self.lost_los);
        v.flight_lock_state = intern(self.lock_state.text());
        v.flight_start = Some(a3(&self.start));
        v.flight_end = Some(a3(&self.end));
        v.flight_start_q = Some(a4(&self.start_q));
        v.flight_trick_loop_q = Some(a4(&self.trick_loop_q));
        v.flight_trick_end_q = Some(a4(&self.trick_end_q));
        v.flight_previous_q = Some(a4(&self.previous_q));
        v.flight_tick_q = Some(a4(&self.tick_q));
        v.flight_action_held = self.action_held;
        v.trick_held = self.trick_held;
        v.immelmann_held = self.immelmann_held;
        v.flight_frame_q = self.frame_q.as_ref().map(a4);
    }
}
