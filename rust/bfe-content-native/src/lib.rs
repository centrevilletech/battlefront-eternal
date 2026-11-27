//! Prepared assets enter through one native admission boundary. Files, JSON,
//! binary geometry and encoded images are cold work; drawing receives stable
//! numeric resources and borrowed presentation/collision content.
pub mod effects;
pub mod geometry;
mod inspection;
pub mod loader;
pub mod map;
pub mod textures;
pub mod units;
pub use bfe_content::GameFamily;
pub use loader::{BattleContent, Loader};
pub use map::AdmittedMap;

pub mod placed;
pub mod foliage;
pub mod water;

pub mod pad;

pub mod fingerprint;

pub mod setup;


pub mod animation2;
