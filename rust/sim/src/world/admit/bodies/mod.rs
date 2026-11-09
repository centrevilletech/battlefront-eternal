//! UnitFactory.build's immutable construction inputs (src/units.js). A
//! body holds the generated Unit row, adoption and rig facts before they
//! move into F3. There is no drawn actor or second movement/combat owner.
mod build;
mod count;
mod context;
mod riders;
mod walker;
mod seats;
pub(crate) mod prepare;
pub use context::{from_context, health_scale};
pub use build::{build, from_parts, Bindings, Body, CombatInit};
pub use riders::{hull_holder, rider_birth, Holder, Rider};
pub use walker::Muzzles;
pub use seats::{hull_facts, turret_geometry, turret_birth, TurretBinding, TurretGeometry};

use super::{AdmitError, Stage};
fn error(message: impl Into<String>) -> AdmitError {
    AdmitError::Content { stage: Stage::Player, message: message.into() }
}
