//! Apply the existing ground/full/compact flight wire formats to a private row.
//! restoreGroundCorrection and restoreCompactFlight deliberately reset local
//! interpolation history while leaving inactive transition/trick state alone.

use crate::gen::{ClientHullSeat, Vehicle, ClientGroundNumbers, ClientFlightNumbers, ClientCompactFlightNumbers,
    ClientTrickNumbers, ClientCorrectionSeatField as Seat, ClientFlightEnumField as Enum, ClientFlightBooleanField as Bool,
    ClientFlightVector as Vector, ClientFlightQuaternion as Quaternion,
    ClientCompactFlightVector as CompactVector, ClientCompactFlightQuaternion as CompactQuaternion};
use crate::flight::state::Trick;
use crate::gen::FlightState as Phase;
use crate::prediction::corrections::{FlightState, GroundState, SeatField};
use crate::flight::store::FlightTextIds;

// Only pilot seat zero receives pending controls. Passenger aim remains a
// once-per-frame authority fact, even while the pilot replays many ticks.
const PILOT: usize = 0;

pub(super) fn seat(s: &[SeatField; 6]) -> ClientHullSeat {
    // Valid gameplay seats use null for an absent look override or last yaw.
    // The wire decoder still distinguishes null/undefined while validating.
    let optional = |field: SeatField| (field.kind == 0).then_some(field.value);
    ClientHullSeat { aim_yaw: s[Seat::AimYaw as usize].value, pitch: s[Seat::Pitch as usize].value, look_yaw: optional(s[Seat::LookYaw as usize]),
        look_pitch: optional(s[Seat::LookPitch as usize]), yaw_rate: s[Seat::YawRate as usize].value, last_yaw: optional(s[Seat::LastYaw as usize]) }
}
fn aim(v: &mut Vehicle, s: &[SeatField; 6]) {
    let s = seat(s); let pilot = &mut v.seats[PILOT];
    pilot.aim_yaw = s.aim_yaw; pilot.pitch = s.pitch;
    pilot.look_yaw = s.look_yaw; pilot.look_pitch = s.look_pitch;
}
// This is the existing ground schema's numeric order. Suspension scratch and
// wheel/walk phases are prerequisites, deliberately absent from its 224 bytes.
pub(super) fn ground(v: &mut Vehicle, c: &GroundState) {
    let n = ClientGroundNumbers::take(&c.numbers, &[]);
    v.health = n.health; v.team = Some(n.team); v.yaw = n.yaw; v.tilt = n.tilt; v.roll = n.roll;
    v.yaw_rate = n.yaw_rate; v.forward_input = n.forward_input; v.right_input = n.right_input;
    v.tilt_rate = n.tilt_rate; v.roll_rate = n.roll_rate; v.support = n.support;
    v.position = c.position; v.velocity = c.velocity; v.quaternion = c.quaternion;
    v.alive = c.alive; v.sleeping = Some(c.sleeping); aim(v, &c.seat);
    v.previous = v.position; v.tick_from = Some(v.position);
    v.previous_yaw = v.yaw; v.tick_from_yaw = Some(v.yaw);
    v.previous_tilt = v.tilt; v.tick_from_tilt = Some(v.tilt);
    v.previous_roll = v.roll; v.tick_from_roll = Some(v.roll);
}

// The full checkpoint and compact authority body share fields but not offsets.
// Named assignments keep the two established wire orders explicit; neither
// representation changes when a page prediction mode changes.
pub(super) fn flight(v: &mut Vehicle, c: &FlightState, texts: &FlightTextIds) {
    // restoreCompactFlight retains inactive vectors. Clearing them would
    // change a later full checkpoint even before the next maneuver starts.
    if c.compact {
        let n = ClientCompactFlightNumbers::take(&c.numbers, &[]);
        v.health = n.health; v.team = Some(n.team); v.yaw = n.yaw; v.tilt = n.tilt; v.roll = n.roll;
        v.yaw_rate = n.yaw_rate; v.forward_input = n.forward_input; v.right_input = n.right_input; v.altitude = n.altitude;
        v.throttle = n.throttle; v.airspeed = n.airspeed; v.tilt_rate = n.tilt_rate; v.state_time = n.state_time; v.floor_y = n.floor_y;
        v.bank = n.bank; v.power = Some(n.power); v.stick_x = Some(n.stick_x); v.stick_y = Some(n.stick_y); v.flight_roll_rate = Some(n.flight_roll_rate);
        v.trick_cooldown = Some(n.trick_cooldown); v.trick_pending = Some(n.trick_pending); v.gear = Some(n.gear);
        v.landing_message_time = Some(n.landing_message_time); v.flight_clock = Some(n.flight_clock); v.flight_assist_id = Some(n.flight_assist_id);
        v.flight_lock_id = Some(n.flight_lock_id); v.flight_lock_generation = Some(n.flight_lock_generation); v.flight_lock_timer = Some(n.flight_lock_timer); v.flight_lost_los = Some(n.flight_lost_los);
        let trick = c.enums[Enum::Trick as usize]; let transition = c.enums[Enum::State as usize] == Phase::TakingOff as u8 || c.enums[Enum::State as usize] == Phase::Landing as u8;
        v.trick_time = Some(if trick != 0 { ClientTrickNumbers::take(&c.numbers[ClientCompactFlightNumbers::WORDS..], &[]).trick_time } else { 0.0 }); v.trick_direction = Some(if trick != 0 { ClientTrickNumbers::take(&c.numbers[ClientCompactFlightNumbers::WORDS..], &[]).trick_direction } else { 1.0 });
        v.position = c.vectors[CompactVector::Position as usize]; v.velocity = c.vectors[CompactVector::Velocity as usize]; v.quaternion = c.quaternions[CompactQuaternion::Quaternion as usize];
        // Optional spans follow their fixed base, in wire order. Transition
        // and trick presence were validated before this private row changes.
        let mut at = CompactVector::Optional as usize;
        if transition { v.flight_start = Some(c.vectors[at]); v.flight_end = Some(c.vectors[at + 1]); at += 2; }
        if trick == Trick::Barrel as u8 { v.flight_trick_right = Some(c.vectors[at]); }
        let mut at = CompactQuaternion::Optional as usize;
        if transition { v.flight_start_q = Some(c.quaternions[at]); at += 1; }
        if trick == Trick::Immelmann as u8 { v.flight_trick_loop_q = Some(c.quaternions[at]); v.flight_trick_end_q = Some(c.quaternions[at + 1]); }
        // Rounded authority packets derive these endpoints; they are not the
        // prior continuation's endpoints and cannot be carried across a rewind.
        v.previous = v.position; v.tick_from = Some(v.position);
        v.previous_yaw = v.yaw; v.tick_from_yaw = Some(v.yaw);
        v.previous_tilt = v.tilt; v.tick_from_tilt = Some(v.tilt);
        v.previous_roll = v.roll; v.tick_from_roll = Some(v.roll);
        v.flight_previous_q = Some(v.quaternion); v.flight_tick_q = Some(v.quaternion);
    } else {
        // restoreFlightCorrection is the full continuation format. Preserve
        // old endpoints rather than deriving them from the current pose.
        let n = ClientFlightNumbers::take(&c.numbers, &[]);
        v.health = n.health; v.team = Some(n.team); v.yaw = n.yaw; v.previous_yaw = n.previous_yaw; v.tilt = n.tilt; v.previous_tilt = n.previous_tilt;
        v.roll = n.roll; v.previous_roll = n.previous_roll; v.yaw_rate = n.yaw_rate; v.forward_input = n.forward_input; v.right_input = n.right_input;
        v.altitude = n.altitude; v.throttle = n.throttle; v.airspeed = n.airspeed; v.tilt_rate = n.tilt_rate; v.state_time = n.state_time; v.floor_y = n.floor_y;
        v.bank = n.bank; v.power = Some(n.power); v.stick_x = Some(n.stick_x); v.stick_y = Some(n.stick_y); v.flight_roll_rate = Some(n.flight_roll_rate);
        v.trick_time = Some(n.trick_time); v.trick_cooldown = Some(n.trick_cooldown); v.trick_pending = Some(n.trick_pending); v.trick_direction = Some(n.trick_direction);
        v.gear = Some(n.gear); v.landing_message_time = Some(n.landing_message_time); v.flight_clock = Some(n.flight_clock); v.flight_assist_id = Some(n.flight_assist_id);
        v.flight_lock_id = Some(n.flight_lock_id); v.flight_lock_generation = Some(n.flight_lock_generation); v.flight_lock_timer = Some(n.flight_lock_timer); v.flight_lost_los = Some(n.flight_lost_los);
        v.tick_from_yaw = Some(n.tick_from_yaw); v.tick_from_tilt = Some(n.tick_from_tilt); v.tick_from_roll = Some(n.tick_from_roll);
        v.position = c.vectors[Vector::Position as usize]; v.previous = c.vectors[Vector::Previous as usize]; v.velocity = c.vectors[Vector::Velocity as usize]; v.tick_from = Some(c.vectors[Vector::TickFrom as usize]);
        v.flight_start = Some(c.vectors[Vector::Start as usize]); v.flight_end = Some(c.vectors[Vector::End as usize]); v.flight_trick_right = Some(c.vectors[Vector::TrickRight as usize]);
        v.quaternion = c.quaternions[Quaternion::Quaternion as usize]; v.flight_previous_q = Some(c.quaternions[Quaternion::Previous as usize]); v.flight_tick_q = Some(c.quaternions[Quaternion::Tick as usize]);
        v.flight_start_q = Some(c.quaternions[Quaternion::Start as usize]); v.flight_trick_loop_q = Some(c.quaternions[Quaternion::TrickLoop as usize]); v.flight_trick_end_q = Some(c.quaternions[Quaternion::TrickEnd as usize]);
    }
    // Boolean tag 2 is undefined, distinct from an ordinary released key.
    let b = c.booleans; let optional = |tag: u8| (tag != 2).then_some(tag != 0);
    v.alive = b[Bool::Alive as usize] == 1; v.sleeping = optional(b[Bool::Sleeping as usize]); v.grounded = b[Bool::Grounded as usize] == 1;
    v.roll_lock = optional(b[Bool::RollLock as usize]); v.boosting = optional(b[Bool::Boosting as usize]); v.boost_exhausted = optional(b[Bool::BoostExhausted as usize]); v.terrain_warning = optional(b[Bool::TerrainWarning as usize]);
    v.flight_action_held = optional(b[Bool::ActionHeld as usize]); v.trick_held = optional(b[Bool::TrickHeld as usize]); v.immelmann_held = optional(b[Bool::ImmelmannHeld as usize]);
    texts.correction_enums(v, c.enums); aim(v, &c.seat);
}
