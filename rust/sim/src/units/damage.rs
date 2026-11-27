//! Fall damage as the 2004 game deals it: not a
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

/// Provisional per-face safety margin added around 2005 authored map bounds
/// (`wireBounds` / `bounds`).
pub const PROVISIONAL_WORLD_BOUNDS_MARGIN: f64 = 64.0;

/// Per-map authoritative world bounds for 2005 (`GameFamily::Swbf2`) battles,
/// where wide battlefields extend beyond the 2004 fixed ±1000 m × ±500 m box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapWorldBounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl MapWorldBounds {
    /// Derives world bounds for a 2005 map from `wireBounds` (falling back to
    /// `bounds`), rounded outward to whole metres plus the provisional margin.
    pub fn from_swbf2_map(world: &bfe_content::Value) -> Self {
        let read = |key: &str| -> Option<([f64; 3], [f64; 3])> {
            let obj = world.get(key)?;
            let lo = obj.get("min")?.as_array()?;
            let hi = obj.get("max")?.as_array()?;
            if lo.len() < 3 || hi.len() < 3 {
                return None;
            }
            let min = [lo[0].as_f64()?, lo[1].as_f64()?, lo[2].as_f64()?];
            let max = [hi[0].as_f64()?, hi[1].as_f64()?, hi[2].as_f64()?];
            if min.iter().chain(max.iter()).all(|v| v.is_finite())
                && min[0] <= max[0]
                && min[1] <= max[1]
                && min[2] <= max[2]
            {
                Some((min, max))
            } else {
                None
            }
        };
        let (raw_min, raw_max) = read("wireBounds")
            .or_else(|| read("bounds"))
            .unwrap_or(([-WORLD_HALF, -WORLD_HEIGHT, -WORLD_HALF], [WORLD_HALF, WORLD_HEIGHT, WORLD_HALF]));
        let margin = PROVISIONAL_WORLD_BOUNDS_MARGIN;
        Self {
            min: [
                raw_min[0].floor() - margin,
                raw_min[1].floor() - margin,
                raw_min[2].floor() - margin,
            ],
            max: [
                raw_max[0].ceil() + margin,
                raw_max[1].ceil() + margin,
                raw_max[2].ceil() + margin,
            ],
        }
    }
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

/// 2005 sibling of `out_of_world` using the map's authored bounds while
/// leaving the released 2004 fixed box and its arithmetic untouched.
pub fn out_of_world_in_bounds(position: [f64; 3], velocity: [f64; 3], bounds: &MapWorldBounds) -> bool {
    velocity[1] < -FALL_KILL_SPEED
        || position[1] < bounds.min[1]
        || position[1] > bounds.max[1]
        || position[0] < bounds.min[0]
        || position[0] > bounds.max[0]
        || position[2] < bounds.min[2]
        || position[2] > bounds.max[2]
}

/// takeImpacts(unit): after a body's step, what its contacts did to it, and
/// the world's bounds for a soldier. On a seat the body does not move by
/// itself.
pub fn take_impacts(id: u32, unit: &mut Unit, world: &mut dyn UnitsWorld) {
    if unit.combat.dead || unit.removed || unit.seat.is_some() {
        return;
    }
    let damage = match &unit.soldier {
        UnitBody::Soldier(s) => s.collision_damage,
        UnitBody::Droideka(d) => d.collision_damage,
    };
    if damage > 0.0 {
        world.damage(id, unit, damage);
    }
    if !unit.combat.dead && !unit.walker {
        if let UnitBody::Soldier(s) = &unit.soldier {
            let outside = match world.map_world_bounds() {
                Some(bounds) => out_of_world_in_bounds(s.position, s.velocity, &bounds),
                None => out_of_world(s.position, s.velocity),
            };
            if outside {
                world.damage(id, unit, f64::INFINITY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swbf1_fixed_box_and_swbf2_map_bounds_stay_distinct() {
        // Synthetic far-flung hangar positions outside the 2004 ±1000 m box.
        let north_hangar = [-420.0, 20.0, -1450.0];
        let south_hangar = [-1320.0, -180.0, 260.0];
        assert!(out_of_world(north_hangar, [0.0; 3]));
        assert!(out_of_world(south_hangar, [0.0; 3]));

        let map = bfe_content::json::parse(
            r#"{"wireBounds":{"min":[-2400.0,-1600.0,-2600.0],"max":[2200.0,1600.0,2600.0]}}"#,
        )
        .unwrap();
        let bounds = MapWorldBounds::from_swbf2_map(&map);
        assert!(!out_of_world_in_bounds(north_hangar, [0.0; 3], &bounds));
        assert!(!out_of_world_in_bounds(south_hangar, [0.0; 3], &bounds));
        assert!(out_of_world_in_bounds([0.0, -2000.0, 0.0], [0.0; 3], &bounds));
        assert!(out_of_world_in_bounds(north_hangar, [0.0, -61.0, 0.0], &bounds));
    }
}
