#![deny(warnings, clippy::all)]

//! Front-end shell (`MAIN MENU`, `SINGLEPLAYER`, `INSTANT ACTION`, `OPTIONS`,
//! `PROFILE MANAGEMENT`, `MULTIPLAYER`, `EXIT TO WINDOWS`), click gate,
//! screen media transitions, shell audio/music, loading timeline, and
//! battle/playlist launch state for Battlefront Eternal.
//!
//! Ports `src/shell/*.js`, `src/launch.js`, and `src/mission.js` into native
//! Rust with zero browser/URL dependencies.

pub mod audio;
pub mod data;
pub mod gate;
pub mod launch;
pub mod loading;
pub mod media;
pub mod mission;
pub mod music;
pub mod profile;
pub mod render;
pub mod screens;
pub mod version;
