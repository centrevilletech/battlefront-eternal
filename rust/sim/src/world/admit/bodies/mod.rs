//! The bodies a battle can spawn, prepared from content as UnitFactory.build
//! prepares them (units.js buildBody: the soldier's constants, the animator's
//! clip set, the combat facade), one record list per class in roster order.
//! Pure functions of the loaded content; the spawn stage stages them.
use crate::world::admit::{AdmitError, Options, Stage};
use bfe_content::battle::Battle;

/// Every class's prepared body rows. Unavailable until the spawn piece lands.
pub fn prepare(battle: &Battle, options: &Options) -> Result<Vec<super::rows::Call>, AdmitError> {
    let _ = (battle, options);
    Err(AdmitError::Unavailable(Stage::Player))
}
