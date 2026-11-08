#![deny(warnings, clippy::all)]

//! Front-end shell menus (`MAIN MENU`, `SINGLEPLAYER`, `INSTANT ACTION`,
//! `OPTIONS`, `PROFILE MANAGEMENT`, `MULTIPLAYER`, `EXIT TO WINDOWS`), click gate,
//! screen media transitions, shell audio/music, loading timeline, and launch state.
//! Ports `src/shell/*.js`, `src/launch.js`, and `src/mission.js` to native Rust.

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
