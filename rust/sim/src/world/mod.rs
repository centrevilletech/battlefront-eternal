//! World-step state and the remaining phase boundaries.
pub mod separation;
pub mod clock;
#[cfg(target_arch = "wasm32")]
mod module;
