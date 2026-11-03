//! Fall damage as the 2004 game deals it (src/collision-damage.js): not a
//! fall rule of its own but collision damage, from each contact's speed
//! into the surface past a threshold; and the world's bounds, past which a
//! soldier dies.

use super::world::UnitsWorld;
use crate::gen::{CollisionRule, Unit, UnitBody};

/// m/s down: falling faster than this kills.
pub const FALL_KILL_SPEED: f64 = 60.0;
/// m, x and z.
const WORLD_HALF: f64 = 1000.0;
/// m, up and down.
const WORLD_HEIGHT: f64 = 500.0;

/// contactDamage(rule, into, up, forward): the damage of one contact.
/// `into`: the velocity along the normal (negative moving into it); `up`
/// and `forward`: the normal's parts along the body's up and facing.
pub fn contact_damage(rule: &CollisionRule, into: f64, up: f64, forward: f64) -> f64 {
    crate::movement::soldier::contact_damage(&rule.threshold, &rule.scale, into, up, forward)
}

/// outOfWorld(position, velocity): past the world's bounds, or falling too
/// fast to live.
pub fn out_of_world(position: [f64; 3], velocity: [f64; 3]) -> bool {
    velocity[1] < -FALL_KILL_SPEED
        || position[1] < -WORLD_HEIGHT
        || position[1] > WORLD_HEIGHT
        || position[0].abs() > WORLD_HALF
        || position[2].abs() > WORLD_HALF
}

/// takeImpacts(unit): after a body's step, what its contacts did to it, and
/// the world's bounds for a soldier. On a seat the body does not move by
/// itself.
pub fn take_impacts(id: u32, unit: &mut Unit, world: &mut dyn UnitsWorld) {
    if unit.combat_dead || unit.removed || unit.seat.is_some() {
        return;
    }
    let damage = match &unit.soldier {
        UnitBody::Soldier(s) => s.collision_damage,
        UnitBody::Droideka(d) => d.collision_damage,
    };
    if damage > 0.0 {
        world.damage(id, unit, damage);
    }
    if !unit.combat_dead && !unit.walker {
        if let UnitBody::Soldier(s) = &unit.soldier {
            if out_of_world(s.position, s.velocity) {
                world.damage(id, unit, f64::INFINITY);
            }
        }
    }
}
