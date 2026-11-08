//! Rows of validateStartupClass (spawn-screen/startup.js): the F4StartupClass row (f4_startup_class).
//! A pure function of the loaded battle and the options; checked word for
//! word against the recorded admission calls.
use super::{Calls, Cx};
use crate::world::admit::{AdmitError, Stage};

/// The owner's calls in the host's order, built through the shared context
/// (the battle, its files, the options, the interner, the ids and returns
/// of the owners before it). Unavailable until its piece lands.
pub fn build(cx: &mut Cx) -> Result<Calls, AdmitError> {
    let _ = cx;
    Err(AdmitError::Unavailable(Stage::Startup))
}
