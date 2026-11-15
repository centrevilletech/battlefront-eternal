//! A dedicated match's connected players (src/runtime/multiplayer-world.js):
//! their command queues, the players' phase of the world tick, their
//! controls on their bodies and the facts the AI and unit phases read of
//! them. The host admits connections and input through `module`; nothing
//! of a tick is decided outside Rust.
pub mod controls;
pub mod owner;
pub mod views;
pub mod commands;
pub mod step;
pub mod rows;
pub mod module;
