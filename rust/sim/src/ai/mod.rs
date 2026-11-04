//! The Classic AI's Rust: its random generators (`random`, src/ai/random.js),
//! its navigation (`nav`, src/ai/classic/nav/*), vehicle AI (`vehicles`,
//! src/ai/classic/vehicles/*), and control core (`control`, scheduler,
//! controllers, agents, goals), with perception, combat and aim (`combat`).

pub mod combat;
mod aim_module;
pub mod control;
pub mod module;
mod navigation_module;
mod perception_module;
mod scheduler_module;
pub mod nav;
pub mod random;
pub mod vehicles;
