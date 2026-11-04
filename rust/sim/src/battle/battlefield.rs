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
use super::world::{BattleWorld, Body};
use crate::gen::Store;
use crate::gen::{Character, Unit, Vehicle};
use crate::store::Ref;

pub const LEAVING_TIME: f64 = 10.0;

pub trait BattlefieldWorld {
    fn character_at(&mut self, s: &Store, index: usize) -> Option<Ref<Character>> { s.battle.characters.get(index).copied() }
    fn body(&mut self, unit: Ref<Unit>) -> Body;
    fn vehicle(&mut self, unit: Ref<Unit>) -> Option<Ref<Vehicle>>;
    fn combat_dead(&mut self, unit: Ref<Unit>) -> bool;
    fn kill(&mut self, s: &mut Store, unit: Ref<Unit>);
    fn kill_droid(&mut self, s: &mut Store, unit: Ref<Unit>);
    fn wreck(&mut self, s: &mut Store, vehicle: Ref<Vehicle>);
}

impl<T: BattleWorld + ?Sized> BattlefieldWorld for T {
    fn body(&mut self, unit: Ref<Unit>) -> Body { BattleWorld::body(self, unit) }
    fn vehicle(&mut self, unit: Ref<Unit>) -> Option<Ref<Vehicle>> { BattleWorld::body(self, unit).vehicle }
    fn combat_dead(&mut self, unit: Ref<Unit>) -> bool { BattleWorld::combat_dead(self, unit) }
    fn kill(&mut self, s: &mut Store, unit: Ref<Unit>) { BattleWorld::kill(self, s, unit); }
    fn kill_droid(&mut self, s: &mut Store, unit: Ref<Unit>) { BattleWorld::kill_droid(self, s, unit); }
    fn wreck(&mut self, s: &mut Store, vehicle: Ref<Vehicle>) { BattleWorld::wreck(self, s, vehicle); }
}

#[derive(Clone, Debug, Default)]
pub struct Battlefield {
    pub boundary: Boundary,
}

impl Battlefield {
    /// outside(position).
    pub fn outside(&self, p: &[f64; 3]) -> bool {
        self.boundary.outside(Vec3 { x: p[0], y: p[1], z: p[2] })
    }

    /// update(dt, characters) (battlefield.js:76), over the battle's
    /// Characters. A kill runs the death path, which the world applies to
    /// the store.
    pub fn update<W: BattlefieldWorld + ?Sized>(&self, dt: f64, s: &mut Store, world: &mut W) {
        if self.boundary.empty() {
            return;
        }
        let mut i = 0;
        while let Some(r) = world.character_at(s, i) {
            i += 1;
            let Some(unit) = s.character.get(r).expect("a Character of the battle").unit else { continue };
            let body: Body = world.body(unit);
            // Only while he has a body: the original lets go of it at his
            // death.
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
                character.out_of_bounds = LEAVING_TIME;
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
            if let Some(vehicle) = world.vehicle(unit) {
                // The hull goes, and everyone aboard with it.
                world.wreck(s, vehicle);
            }
            if !world.combat_dead(unit) {
                world.kill(s, unit);
            }
        }
    }

    pub fn controls_outside(&self, present: bool, place: &[f64; 3], droid: Option<&[f64; 3]>) -> bool {
        present && !self.boundary.empty() && (self.outside(place) || droid.is_some_and(|p| self.outside(p)))
    }
}

/// The page's tick-driven leaving warning (battlefield.js LeavingWarning).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LeavingWarning {
    pub shown: bool,
    pub value: f64,
    pub rate: f64,
    pub tick: f64,
}

impl LeavingWarning {
    pub fn alpha(&self) -> f64 {
        if !self.shown { return 0.0; }
        jsrt::js_min(jsrt::js_max(self.value.trunc(), 2.0), 255.0) / 255.0
    }

    pub fn update(&mut self, tick: f64, outside: bool) -> bool {
        if !outside {
            self.shown = false;
            return false;
        }
        let started = !self.shown;
        let mut steps = jsrt::js_min(jsrt::js_max(tick - self.tick, 0.0), 60.0);
        if started {
            self.shown = true;
            self.value = 254.0;
            self.rate = 0.0;
            steps = 1.0;
        }
        self.tick = tick;
        let mut i = 0.0;
        while i < steps {
            self.value += self.rate / 60.0;
            self.rate += (128.0 - self.value) * 500.0 / 60.0;
            i += 1.0;
        }
        started
    }
}
