//! The goal bots' entries outside the rules tick.

use super::{census, GoalController, GoalRegions, MeleeInput};
use crate::gen::F3UnitKey;
use crate::world::owner::World;

/// The battle's character capacity and the map's regions, for goal bots
/// being installed.
pub(crate) fn admission_in(w: &World) -> Option<(usize, GoalRegions)> {
    let battle = w.ai.battle()?;
    let regions = match &w.battle.map_goal_regions {
        Some(index) => index.clone(),
        None => {
            let records = &battle.state.navigation.nav.regions;
            records.as_array()?;
            GoalRegions::read(records)
        }
    };
    Some((battle.character_capacity().max(w.sim.store.character.slots() as usize), regions))
}

/// Hand every bot of `bots` back to the released controller before their
/// rules are dropped. False while the AI cannot be reached; nothing was
/// changed then.
pub(crate) fn retire_in(w: &mut World, bots: &mut GoalController) -> bool {
    census::note();
    // Installing even an empty table changed the core's objective publisher.
    if w.ai.battle_on_loan() { return false; }
    bots.steered().is_none() || crate::ai::owned::goal_loan::retire_in(w, bots)
}

pub(crate) fn step_soldiers(w: &mut World, tick: f64) -> bool {
    let Some(mut driver) = w.battle.melee_driver.take() else { return true };
    let mut ok = true;
    for index in 0..w.sim.store.battle.characters.len() {
        let Some(owner) = w.sim.store.battle.characters.get(index) else { break };
        let Some(body) = w.sim.store.character.get(*owner).and_then(|c| c.unit) else { continue };
        let Some(unit) = w.sim.store.unit.get(body) else { continue };
        let Some(frame) = w.ai_goal_input(F3UnitKey::of(body, unit), tick) else { continue };
        match driver(w, &frame) {
            Ok(Some(report)) => { w.ai_goal_feedback(&frame, report.guarding, report.energy, report.exhausted); }
            Ok(None) => {}
            Err(_) => {
                if let Some((slot, _)) = w.goal_soldier(&frame.key) {
                    if let Some(bots) = crate::battle::f4_tail::soldier_inputs(&mut w.battle) {
                        bots.retry_frame(slot, &frame);
                    }
                }
                ok = false;
                break;
            }
        }
    }
    w.battle.melee_driver = Some(driver);
    ok
}

impl World {
    /// Install the combat consumer of bot frames inside the authoritative soldier phase.
    pub fn register_bot_melee_driver(&mut self, driver: Box<super::melee::SoldierDriver>) {
        self.battle.melee_driver = Some(driver);
        if crate::battle::f4_tail::soldier_inputs(&mut self.battle).is_some() {
            self.battle.soldier_phase = crate::units::f3::goal_step_in;
        }
    }

    /// The melee sample of the bot life `key` for the fixed tick `tick` the
    /// combo machine is stepping, given once. `None` outside event battles,
    /// for a life other than a living bot’s current body, for any
    /// other tick, and for a sample already given, so a delivered edge is
    /// never read again.
    pub fn ai_goal_input(&mut self, key: F3UnitKey, tick: f64) -> Option<MeleeInput> {
        let (slot, binding) = self.goal_soldier(&key)?;
        let now = crate::units::f3::tick_in(&self.units);
        if tick != now && tick != now + 1.0 { return None; }
        crate::battle::f4_tail::soldier_inputs(&mut self.battle)?.take_soldier(slot, &key, tick, binding)
    }

    /// Report the guard and mover state after successfully stepping this delivered frame.
    pub fn ai_goal_feedback(&mut self, frame: &MeleeInput, guarding: bool, energy: f64, exhausted: bool) -> bool {
        let Some((slot, binding)) = self.goal_soldier(&frame.key) else { return false };
        binding.is_some() && binding == frame.binding && crate::battle::f4_tail::soldier_inputs(&mut self.battle)
            .is_some_and(|bots| bots.feedback(slot, frame, guarding, energy, exhausted))
    }

    /// Use the canonical body and arsenal, also while the AI is servicing a soldier callback.
    fn goal_soldier(&mut self, key: &F3UnitKey) -> Option<(usize, Option<super::melee::Binding>)> {
        crate::battle::f4_tail::soldier_inputs(&mut self.battle)?;
        let unit = key.validate(&self.sim.store).ok()?;
        let owner = unit.character?;
        let character = self.sim.store.character.get(owner)?;
        if !unit.alive || unit.dead || unit.combat.dead || unit.removed || character.human || character.unit != Some(key.reference()) {
            return None;
        }
        let slot = owner.slot as usize;
        if unit.seat.is_some() { return Some((slot, None)); }
        let defs = crate::combat::module::defs_in(&mut self.combat);
        let weapons = &defs.weapons;
        for channel in 0..2u8 {
            let selected = crate::ai::combat::world::index_of(unit.combat.selected[channel as usize]);
            let Some((slot, weapon)) = selected.and_then(|index| unit.combat.weapons.iter().enumerate()
                .filter(|(_, w)| weapons.has(w.def) && w.in_channel(weapons, f64::from(channel))).nth(index)) else { continue };
            if weapons.class(weapon.def).levels.get(weapon.level as usize).is_some_and(|level| level.obj.get("base").is_str("melee")) {
                return Some((owner.slot as usize, Some(super::melee::Binding { slot, channel, weapon: weapon.def })));
            }
        }
        Some((slot, None))
    }
}
