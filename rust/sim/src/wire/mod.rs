//! Snapshot capture, baseline ownership and byte encoding. The semantic
//! capture boundary also serves native hosts while tick ownership moves.
pub mod codec;
mod module;
pub mod records;
pub mod ring;
pub mod history;
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
pub mod capture;
pub mod shots;
pub mod combat;
