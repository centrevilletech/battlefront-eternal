//! Spawn-time construction and the per-tick facts a native host cannot
//! compute from outside: UnitFactory.build's cold stage rows for a class
//! (f3_factory_reserve/stage/resident/player_bind/player_spawn/finish,
//! units/f3_construction.rs), the crews' seat and rider rows, the F3
//! foreign-row count preparePhase returned (WorldTickInput.units) and the
//! discrete actions entry (players_actions) for a live local player.
use super::{AdmitError, Stage};
use crate::world::owner::World;

/// preparePhase's count of foreign input rows for the next tick: the units
/// whose intent or network facts the host wrote. Unavailable until the
/// spawn piece lands; the recorded word stands in for it meanwhile.
pub fn units(world: &World) -> Result<u32, AdmitError> {
    let _ = world;
    Err(AdmitError::Unavailable(Stage::Player))
}
