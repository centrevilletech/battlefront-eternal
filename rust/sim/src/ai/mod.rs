//! The Classic AI's Rust: its random generators (`random`),
//! its navigation (`nav`, src/ai/classic/nav/*), vehicle AI (`vehicles`,
//! src/ai/classic/vehicles/*), and control core (`control`, scheduler,
//! controllers, agents, goals), with perception, combat and aim (`combat`).
//! The goal bots of the 2005 game's modes (`goals`) are a separate controller
//! that drives the Classic one through its own orders.

pub mod combat;
pub mod control;
pub mod nav;
pub mod random;
pub mod phase;
pub mod owned;
pub mod vehicles;
pub mod goals;
