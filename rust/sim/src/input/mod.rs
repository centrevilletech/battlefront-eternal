//! Authoritative command queues. Packet values enter through typed
//! records; ordering, limits, life changes and consumption are native.
pub mod actions;
#[cfg(target_arch = "wasm32")]
pub mod module;
