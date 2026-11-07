//! Snapshot capture, baseline ownership and byte encoding. The semantic
//! capture boundary also serves native hosts while tick ownership moves.
pub mod codec;
pub mod owner;
pub mod buffered;
mod module;
pub mod records;
pub mod ring;
pub mod presentation;
pub mod host;
pub mod stagger;
pub mod json;
pub mod cosmetics;
pub mod semantic;
pub mod edges;
pub mod input;
pub use module::Runtime;
pub mod registry;
pub mod shots;
pub mod combat;
mod census;
pub mod store_capture;
pub mod store_world;
pub mod store_summaries;
pub mod store_own;
pub mod retained;
pub mod own_ring;
pub mod delivery;
pub mod journal;
pub mod store_projectiles;
pub mod publisher;
#[cfg(target_arch = "wasm32")]
mod publish_module;
#[cfg(target_arch = "wasm32")]
pub(crate) mod store_source;
