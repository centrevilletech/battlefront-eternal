//! A battle's cold setup, built from the player's content the way the page
//! and the headless Node host build it in JavaScript before the first tick:
//! the query a mission is picked by (the battle URL, tools/world-hash.mjs),
//! the files the battle fetches (game.js start()), the water clock the world
//! adopts (WorldOperation's WorldTickSetup, src/sim/wasm/world.js) and the
//! tick rows the host stamps every step (WorldTick.run, src/runtime/world-tick.js).
//!
//! Every row here is a plain function of content and options, so a native
//! host can hand the world the same f64 words the JavaScript hosts hand the
//! WebAssembly module. The whole-world input corpus records those words for
//! its battles (docs/world-replay.md); tools/native-setup-compare.mjs checks
//! this module against them bit for bit through the setup-rows example.
pub mod admission;
pub mod assets;
pub mod clock;
pub mod options;
pub mod water;
pub use admission::Admission;
pub use assets::{manifest, Asset};
pub use clock::{battle_clock, TickRows};
pub use options::{Ai, BattleOptions};
pub use water::tick_setup;
