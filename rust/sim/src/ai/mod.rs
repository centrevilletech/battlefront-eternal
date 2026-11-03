//! The Classic AI's Rust: its random generators (`random`, src/ai/random.js),
//! its navigation (`nav`, src/ai/classic/nav/*), vehicle AI (`vehicles`,
//! src/ai/classic/vehicles/*), and control core (`control`, scheduler,
//! controllers, agents, goals). Perception and combat follow beside them.

pub mod control;
pub mod nav;
pub mod random;
pub mod vehicles;
