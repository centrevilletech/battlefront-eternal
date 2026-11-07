//! Authoritative command queues. Packet values enter through typed
//! records; ordering, limits, life changes and consumption are native.
pub mod owner;
pub mod actions;
pub mod fire;
pub mod mailbox;
pub mod controls;
pub mod module;
