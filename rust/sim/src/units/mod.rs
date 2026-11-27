//! The battle's units (src/registry.js,
//! src/droideka.js and src/game.js's
//! separateSoldiers): every soldier on the field, his tick, the registry
//! that answers who is near a point and what a segment passes through, and
//! the crowd's push.

pub mod body;
pub mod class;
pub mod infantry;
pub mod construction;
pub mod damage;
pub mod droideka;
pub mod factory;
pub mod f3;
pub(crate) mod f3_construction;
pub(crate) mod f3_items;
pub(crate) mod f3_props;
pub(crate) mod f3_hulls;
pub(crate) mod f3_fleet;
pub(crate) mod f3_sensors;
pub mod ownership;
pub mod poses;
pub mod module;
pub mod muzzles;
pub(crate) mod live_pose;
pub(crate) mod native;
pub mod registry;
pub mod separate;
pub mod shapes;
pub mod soldier;
pub mod unit;
pub mod water;
pub mod world;

pub mod page_body;
pub(crate) mod owner;

#[cfg(test)]
mod ownership_tests;
