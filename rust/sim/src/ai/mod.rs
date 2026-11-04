//! The Classic AI's Rust: its random generators (`random`, src/ai/random.js),
//! its navigation (`nav`, src/ai/classic/nav/*), vehicle AI (`vehicles`,
//! src/ai/classic/vehicles/*), and control core (`control`, scheduler,
//! controllers, agents, goals), with perception, combat and aim (`combat`).

pub mod combat;
pub mod control;
pub mod module;
pub mod nav;
pub mod random;
pub mod vehicles;
