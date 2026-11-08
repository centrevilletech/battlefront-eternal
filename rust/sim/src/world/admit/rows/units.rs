//! Rows of attachF3 (f3-sim.js, f3-registry.js, f3-hulls.js, f3-items.js): f3_init, f3_missile_classes, f3_body_names, f3_body_skeleton, f3_weapon_services, hit policy, blast binding, impact rules, droideka class, registry prepare/add, hull and item rows.
//! A pure function of the loaded battle and the options; checked word for
//! word against the recorded admission calls.
use super::{Calls, Cx};
use crate::world::admit::{AdmitError, Stage};

/// The owner's calls in the host's order, built through the shared context
/// (the battle, its files, the options, the interner, the ids and returns
/// of the owners before it). Unavailable until its piece lands.
pub fn build(cx: &mut Cx) -> Result<Calls, AdmitError> {
    let _ = cx;
    Err(AdmitError::Unavailable(Stage::Units))
}
