//! Prepared assets enter through one native admission boundary. Files, JSON,
//! binary geometry and encoded images are cold work; drawing receives stable
//! numeric resources and borrowed presentation/collision content.
pub mod effects;
pub mod geometry;
pub mod loader;
pub mod textures;
pub mod units;
pub use loader::{BattleContent, Loader};

pub mod placed;
pub mod foliage;
pub mod water;

mod pad;

pub mod fingerprint;

pub mod setup;

