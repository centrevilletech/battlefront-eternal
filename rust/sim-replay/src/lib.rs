//! The simulation's per-phase replays (plan, verification level V2): what a
//! Rust port of a phase is proven with before it takes the battle over.
//!
//! tools/sim-capture.mjs runs real battles (as tools/world-hash.mjs does) and
//! records, for the chosen phases of sampled ticks, the state each frame
//! starts and ends with, every call across its boundary with its arguments
//! and results, and every random draw in order. This crate reads those
//! captures exactly ([`format`], [`value`]), assembles their frames
//! ([`Capture::frames`]), serves a frame's calls and draws to the port in
//! order while checking what it asks ([`Log`]), and compares the state the
//! port ends with ([`diff`]). A package's replay test, behind its crate's
//! `replay` feature, looks like:
//!
//! ```no_run
//! use sim_replay::{replay_corpus, Corpus, Log, Outcome};
//! let Some(corpus) = Corpus::from_env().unwrap() else { return }; // BFE_REPLAY_CORPUS
//! let report = replay_corpus(&corpus, "ordnance.stepBolt", |capture, frame| {
//!     let mut log = Log::new(frame);
//!     // load frame.before() into the port, run it with its boundary served
//!     // from `log`, compare its state with frame.after() ...
//!     match log.finish() { Ok(()) => Outcome::Same, Err(e) => Outcome::Failed(e.0) }
//! })
//! .unwrap();
//! report.assert_clean();
//! ```
//!
//! Captures are game data: they live with the game's files (BFE_REPLAY_CORPUS
//! names a directory of them), never in the repository; without one, replay
//! tests pass without replaying. Captures from another V8 than the
//! production server's are refused ([`REFERENCE_V8`]).

pub mod diff;
pub mod format;
pub mod frame;
pub mod log;
pub mod phase;
pub mod report;
pub mod value;

pub use diff::{diff, same, Difference};
pub use format::{Kind, Reader, Record};
pub use frame::{Call, Capture, Corpus, Draw, Error, Event, Frame, REFERENCE_V8};
pub use log::{check_bool, check_nullish, check_num, check_value, check_vec3, Log, Mismatch};
pub use phase::{Phase, PHASES};
pub use report::{replay_corpus, Outcome, Report};
pub use value::{same_number, Arena, Node, Typed, Value, V};
