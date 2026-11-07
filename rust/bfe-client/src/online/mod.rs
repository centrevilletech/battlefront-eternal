//! Direct online battle host. The server supplies snapshots and accepts input;
//! this owner runs no authoritative simulation and never creates a world.
pub mod info;
pub mod receiver;
pub mod replicas;
pub mod feedback;
pub mod input;
pub mod clock;
pub mod client;
pub mod index;
