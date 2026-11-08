//! Rows of attachF2 (f2-sim.js, f2-seats.js, f2-firing.js): veh_flight_env, f2_init, veh_class, veh_map, f2_register, seats and firing supply rows, the Vehicle and Vehicles store rows.
//! A pure function of the loaded battle and the options; checked word for
//! word against the recorded admission calls.
use super::{Calls, Cx};
use crate::world::admit::{AdmitError, Stage};

/// The owner's calls in the host's order, built through the shared context
/// (the battle, its files, the options, the interner, the ids and returns
/// of the owners before it). Unavailable until its piece lands.
pub fn build(cx: &mut Cx) -> Result<Calls, AdmitError> {
    let _ = cx;
    Err(AdmitError::Unavailable(Stage::Fleet))
}
