//! Native device hosts. Simulation and presentation own their state behind
//! World; this crate supplies clocks and generated command records.
extern crate self as bfe_client;

pub mod app;
pub mod audio;
pub mod canvas;
pub mod content_loader;
pub mod generated_input;
pub mod host;
pub mod hud_sync;
pub mod input;
pub mod local;
pub mod online;
pub mod presentation_bridge;
pub mod render_bridge;
pub mod resources;
pub mod scene;
pub mod session;
pub mod session_assets;
pub mod session_format;
pub mod session_window;
pub mod sound;
mod static_scene;

mod shadows;
