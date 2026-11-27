//! Leaving the battlefield in a 2005 battle: the 2004 rule (`battlefield.rs`)
//! with the 2005 game's time to come back. Each Character's update, while he
//! has a body, tests what he controls; with none outside his countdown is -1;
//! when one goes out it starts at 6.5 s; once it is below 0 each thing still
//! outside is killed outright, nobody credited. (The game shortens it to
//! 2.17 s while one of them is a steered missile, which the port's missiles,
//! being rounds, never are.)

use super::battlefield::{Battlefield, BattlefieldWorld};
use super::world::Body;
use crate::gen::Store;

/// Seconds a Character may keep what he controls outside the battlefield in a
/// 2005 battle.
pub const LEAVING_TIME_SWBF2: f64 = 6.5;

impl Battlefield {
    /// The 2005 game's leaving rule, over the battle's Characters.
    pub fn update_swbf2<W: BattlefieldWorld + ?Sized>(&self, dt: f64, s: &mut Store, world: &mut W) {
        if self.boundary.empty() {
            return;
        }
        let mut i = 0;
        while let Some(r) = world.character_at(s, i) {
            i += 1;
            let Some(unit) = s.character.get(r).expect("a Character of the battle").unit else { continue };
            let body: Body = world.body(s, unit);
            if body.removed || !body.alive || body.combat_dead {
                continue;
            }
            let body_out = self.outside(&body.place);
            let droid_out = body.droid.is_some_and(|d| self.outside(&d));
            let character = s.character.get_mut(r).unwrap();
            if !body_out && !droid_out {
                character.out_of_bounds = -1.0;
                continue;
            }
            if !(character.out_of_bounds >= 0.0) {
                character.out_of_bounds = LEAVING_TIME_SWBF2;
            }
            character.out_of_bounds -= dt;
            if !(character.out_of_bounds < 0.0) {
                continue;
            }
            if droid_out {
                world.kill_droid(s, unit);
            }
            if !body_out {
                continue;
            }
            if let Some(vehicle) = world.vehicle(s, unit) {
                world.wreck(s, vehicle);
            }
            if !world.combat_dead(s, unit) {
                world.kill(s, unit);
            }
        }
    }
}
