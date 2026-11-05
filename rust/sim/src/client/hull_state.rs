//! Flat page hull state between the frame ABI and its private solver row.
//! predictVehicle copied these values with a shallow clone. The schema now
//! separates immutable class geometry from the mutable correction checkpoint.

use crate::gen::{ClientHullState, Vehicle};

// A projection may copy only Copy fields. Adding a heap-owned field to this
// list fails to compile instead of silently cloning a table on every frame.
// The exhaustive initializer also rejects a schema field omitted from here.
// Each assignment is generated from this single field list. A new schema
// field fails compilation until both directions explicitly account for it.
macro_rules! hull_state {
    ($($field:ident),* $(,)?) => {
        impl ClientHullState {
            // Import into the private row, never the authoritative slab.
            pub fn read(v: &Vehicle) -> Self { Self { $($field: v.$field,)* } }
            pub(super) fn write(&self, v: &mut Vehicle) { $(v.$field = self.$field;)* }
        }
    };
}

hull_state!(
    // Motion and its previous substep are distinct interpolation endpoints.
    // Restoring one must never overwrite the other with a smoothed pose.
    position,
    previous,
    velocity,
    yaw,
    previous_yaw,
    tilt,
    roll,
    previous_tilt,
    previous_roll,
    // Solver rates remain full precision; network draw records are rounded.
    yaw_rate,
    tilt_rate,
    roll_rate,
    support,
    forward_input,
    right_input,
    // Sleeping is optional before spawn; an awake driver still advances.
    alive,
    sleeping,
    health,
    dying,
    // Deep-water clocks advance locally even though prediction suppresses damage.
    water_clock,
    // Hull.syncBody derives these after each tick; authority supplies them on restore.
    quaternion,
    matrix,
    item_center,
    support_normal,
    // captureTick stores world-tick endpoints, not the final 120 Hz substep.
    tick_from,
    tick_from_yaw,
    tick_from_tilt,
    tick_from_roll,
    // Ground hulls retain null flight state and undefined craft-only fields.
    flight_state,
    grounded,
    altitude,
    throttle,
    airspeed,
    state_time,
    floor_y,
    bank,
    // Target relations use the corrected team, including its optional presence.
    team,
    power,
    // Virtual stick and filtered flight rate are separate persistent state.
    stick_x,
    stick_y,
    flight_roll_rate,
    roll_lock,
    boosting,
    // Trick and prompt strings are existing interned handles, never new texts.
    trick,
    trick_time,
    trick_cooldown,
    trick_pending,
    trick_direction,
    gear,
    landing_message,
    landing_message_time,
    flight_trick_right,
    flight_clock,
    boost_exhausted,
    landing_reason,
    flight_warning,
    terrain_warning,
    // Flight targeting checkpoints include lock identity and elapsed LOS state.
    flight_assist_id,
    flight_lock_id,
    flight_lock_generation,
    flight_lock_timer,
    flight_lost_los,
    flight_lock_state,
    // Takeoff/landing and trick paths retain their original frame endpoints.
    flight_start,
    flight_end,
    flight_start_q,
    flight_trick_loop_q,
    flight_trick_end_q,
    flight_previous_q,
    flight_tick_q,
    // Optional action latches distinguish a new life from a released key.
    flight_action_held,
    trick_held,
    immelmann_held,
);
