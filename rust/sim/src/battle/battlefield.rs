//! Leaving the battlefield (src/battlefield.js), as the 2004 game has it: the
//! battlefield is the map's bnd_ boundary (outside: in no polygon, in x and
//! z only; a map without one has no outside). Each Character's update, while
//! he has a body, tests what he controls (the hull or turret he sits in,
//! else his body, and the recon droid he flies): with none outside his
//! countdown is -1; when one goes out it starts at 10 s; once it is below 0
//! each thing still outside is killed outright, nobody credited: his body,
//! a hull with everyone aboard, a droid.

use crate::ai::nav::boundary::Boundary;
use crate::ai::nav::math::Vec3;
use super::rules::Character;
use super::world::{BattleWorld, Body};

pub const LEAVING_TIME: f64 = 10.0;

#[derive(Clone, Debug, Default)]
pub struct Battlefield {
    pub boundary: Boundary,
}

impl Battlefield {
    /// outside(position).
    pub fn outside(&self, p: &[f64; 3]) -> bool {
        self.boundary.outside(Vec3 { x: p[0], y: p[1], z: p[2] })
    }

    /// update(dt, characters) (battlefield.js:76).
    pub fn update(&self, dt: f64, characters: &mut [Character], world: &mut dyn BattleWorld) {
        if self.boundary.empty() {
            return;
        }
        for character in characters.iter_mut() {
            let Some(unit) = character.unit else { continue };
            let body: Body = world.body(unit);
            // Only while he has a body: the original lets go of it at his
            // death.
            if body.removed || !body.alive || body.combat_dead {
                continue;
            }
            let body_out = self.outside(&body.place);
            let droid_out = body.droid.is_some_and(|d| self.outside(&d));
            if !body_out && !droid_out {
                character.out_of_bounds = -1.0;
                continue;
            }
            if !(character.out_of_bounds >= 0.0) {
                character.out_of_bounds = LEAVING_TIME;
            }
            character.out_of_bounds -= dt;
            if !(character.out_of_bounds < 0.0) {
                continue;
            }
            if droid_out {
                world.kill_droid(unit);
            }
            if !body_out {
                continue;
            }
            if let Some(vehicle) = body.vehicle {
                // The hull goes, and everyone aboard with it.
                world.wreck(vehicle);
            }
            if !world.combat_dead(unit) {
                world.kill(unit);
            }
        }
    }
}
