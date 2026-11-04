//! The Classic AI's Rust: its random generators (`random`, src/ai/random.js),
//! its navigation (`nav`, src/ai/classic/nav/*), vehicle AI (`vehicles`,
//! src/ai/classic/vehicles/*), and control core (`control`, scheduler,
//! controllers, agents, goals), with perception, combat and aim (`combat`).

pub mod combat;
mod aim_module;
#[cfg(target_arch = "wasm32")]
mod awareness_module;
#[cfg(target_arch = "wasm32")]
mod controller_driver_module;
#[cfg(target_arch = "wasm32")]
mod dispatch_module;
#[cfg(target_arch = "wasm32")]
mod cover_module;
mod fire_module;
mod fsm_module;
#[cfg(target_arch = "wasm32")]
mod fire_service_module;
#[cfg(target_arch = "wasm32")]
mod weapon_module;
pub mod control;
pub mod module;
mod navigation_module;
mod perception_module;
#[cfg(target_arch = "wasm32")]
mod scheduler_module;
#[cfg(target_arch = "wasm32")]
mod sight_module;
#[cfg(target_arch = "wasm32")]
mod tracker_module;
pub mod nav;
pub mod random;
pub mod vehicles;
