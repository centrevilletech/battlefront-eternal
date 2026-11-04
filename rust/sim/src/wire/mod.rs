//! Snapshot capture, baseline ownership and byte encoding. The semantic
//! capture boundary also serves native hosts while tick ownership moves.
pub mod codec;
mod module;
pub mod records;
pub mod ring;
pub mod history;
pub mod presentation;
