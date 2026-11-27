//! Input delivered by the mission's bot controller to the existing soldier step.
use super::{contact::LifeId, Controls};
use crate::{
    combat::{defs::WeaponDefs, weapons::Arsenal},
    gen::Combat,
    store::DefId,
    world::owner::World,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub slot: usize,
    pub weapon: DefId,
    pub channel: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub installation: u32,
    pub tick: u32,
    pub life: LifeId,
    pub binding: Option<Binding>,
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub stamp: Stamp,
    pub fire: bool,
    pub fire_pressed: bool,
    pub guard: bool,
    pub guard_pressed: bool,
    pub hold_to_block: bool,
    pub forward: f64,
    pub right: f64,
}

use crate::ai::goals::melee::{BlockMode, MeleeInput, MeleeReport};

impl Stamp {
    fn from_goal(sample: &MeleeInput) -> Self {
        Self {
            installation: sample.epoch,
            tick: sample.tick as u32,
            life: LifeId {
                slot: sample.key.slot,
                slab: sample.key.generation,
                generation: sample.key.life as u32,
            },
            binding: sample.binding.map(|selected| Binding {
                slot: selected.slot,
                weapon: selected.weapon,
                channel: selected.channel,
            }),
        }
    }
}

/// The goal phase supplies intent. Movement, energy and damage each advance once in the unit phase.
pub(super) fn stage_goal_frame(
    world: &mut World, sample: &MeleeInput,
) -> Result<Option<MeleeReport>, crate::battle::rules::objects::RuleError> {
    let Some(infantry) = world.units.infantry.as_mut() else { return Ok(None) };
    infantry.bot_inputs.install(sample.epoch);
    let frame = Frame {
        stamp: Stamp::from_goal(sample),
        fire: sample.fire.held,
        fire_pressed: sample.fire.pressed,
        guard: sample.block.held,
        guard_pressed: sample.block.pressed,
        hold_to_block: sample.mode == BlockMode::Hold,
        forward: sample.forward,
        right: sample.right,
    };
    if world.stage_bot_combat_input(frame) {
        Ok(None)
    } else {
        Err(crate::battle::rules::objects::RuleError::StaleHandle)
    }
}

pub(in crate::units) fn finish_goal_frames(world: &mut World, tick: f64) {
    let Some(profile) = world.units.infantry.as_mut() else { return };
    let Some(soldiers) = crate::battle::f4_tail::soldier_inputs(&mut world.battle) else { return };
    soldiers.report_committed(tick, |sample| {
        let actual = profile.bot_inputs.feedback(Stamp::from_goal(sample))?;
        Some(MeleeReport { guarding: actual.guarding, energy: actual.energy, exhausted: actual.exhausted })
    });
}

impl Frame {
    pub(in crate::units) fn controls(self) -> Controls {
        let fire = crate::combat::combo::ComboButton::Fire.bit();
        Controls {
            melee_held: if self.fire { fire } else { 0 },
            melee_pressed: if self.fire_pressed { fire } else { 0 },
            block: self.guard,
            block_pressed: self.guard_pressed,
            hold_to_block: self.hold_to_block,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feedback {
    pub guarding: bool,
    pub energy: f64,
    pub exhausted: bool,
}

#[derive(Clone, Copy)]
pub(in crate::units) struct Delivery {
    pub frame: Frame,
    pub reset_binding: bool,
}

#[derive(Clone, Copy)]
struct Row {
    frame: Frame,
    reset_binding: bool,
    consumed: bool,
    feedback: Option<Feedback>,
    committed: bool,
}

pub(in crate::units) struct BotInputs {
    pub(in crate::units) installation: Option<u32>,
    rows: Vec<Option<Row>>,
}

impl BotInputs {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            installation: None,
            rows: vec![None; capacity],
        }
    }

    fn install(&mut self, installation: u32) {
        if self.installation != Some(installation) {
            self.rows.fill(None);
            self.installation = Some(installation);
        }
    }

    fn stage(&mut self, now: u32, frame: Frame) -> bool {
        if self.installation != Some(frame.stamp.installation)
            || frame.stamp.tick != now
            || !frame.forward.is_finite()
            || !frame.right.is_finite()
        {
            return false;
        }
        let Some(row) = self.rows.get_mut(frame.stamp.life.slot as usize) else {
            return false;
        };
        if row.is_some_and(|r| r.frame.stamp.tick == now && r.frame.stamp.life == frame.stamp.life)
        {
            return false;
        }
        let reset_binding = row.is_none_or(|previous| {
            previous.reset_binding
                || previous.frame.stamp.life != frame.stamp.life
                || previous.frame.stamp.binding != frame.stamp.binding
        });
        *row = Some(Row {
            frame,
            reset_binding,
            consumed: false,
            feedback: None,
            committed: false,
        });
        true
    }

    pub(in crate::units) fn take(
        &mut self,
        tick: u32,
        life: LifeId,
        binding: Option<Binding>,
    ) -> Option<Delivery> {
        let r = self.rows.get_mut(life.slot as usize)?.as_mut()?;
        let stamp = r.frame.stamp;
        if r.consumed || stamp.tick != tick || stamp.life != life || stamp.binding != binding {
            return None;
        }
        r.consumed = true;
        binding?;
        Some(Delivery {
            frame: r.frame,
            reset_binding: std::mem::take(&mut r.reset_binding),
        })
    }

    pub(super) fn commit(&mut self, stamp: Stamp, value: Feedback) {
        if let Some(r) = self
            .rows
            .get_mut(stamp.life.slot as usize)
            .and_then(Option::as_mut)
        {
            if r.consumed && r.frame.stamp == stamp && !r.committed {
                r.committed = true;
                r.feedback = Some(value);
            }
        }
    }

    fn feedback(&mut self, stamp: Stamp) -> Option<Feedback> {
        let row = self.rows.get_mut(stamp.life.slot as usize)?.as_mut()?;
        if row.frame.stamp != stamp {
            return None;
        }
        row.feedback.take()
    }
}

pub(crate) fn equipped(combat: &mut Combat, defs: &WeaponDefs) -> Option<Binding> {
    let arsenal = Arsenal {
        weapons: &mut combat.weapons,
        selected: &mut combat.selected,
    };
    for channel in 0..2 {
        let Some(slot) = arsenal.current(defs, channel) else {
            continue;
        };
        let w = &arsenal.weapons[slot];
        if defs
            .class(w.def)
            .levels
            .get(w.level as usize)
            .is_some_and(|level| level.obj.get("base").is_str("melee"))
        {
            return Some(Binding {
                slot,
                weapon: w.def,
                channel: channel as u8,
            });
        }
    }
    None
}

impl World {
    /// Select the registered input producer for this rules installation.
    pub fn install_bot_combat_input(&mut self, installation: u32) {
        if let Some(infantry) = self.units.infantry.as_mut() {
            infantry.bot_inputs.install(installation);
            self.battle.melee_driver = None;
            self.battle.soldier_phase = crate::units::f3::released_step_in;
        }
    }

    fn bot_owns_combat_stamp(&mut self, stamp: Stamp) -> bool {
        if self
            .units
            .infantry
            .as_ref()
            .and_then(|i| i.bot_inputs.installation)
            != Some(stamp.installation)
        {
            return false;
        }
        let body = crate::store::Ref::new(stamp.life.slot, stamp.life.slab);
        let Some(unit) = self.sim.store.unit.get_mut(body) else {
            return false;
        };
        if unit.human
            || !unit.alive
            || unit.dead
            || unit.removed
            || unit.combat.dead
            || super::contact::identity(body, unit) != stamp.life
        {
            return false;
        }
        let Some(character) = unit.character.and_then(|r| self.sim.store.character.get(r)) else {
            return false;
        };
        if character.human || character.unit != Some(body) {
            return false;
        }
        let binding = if unit.seat.is_some() {
            None
        } else {
            let defs = crate::combat::module::defs_in(&mut self.combat);
            equipped(&mut unit.combat, &defs.weapons)
        };
        stamp.binding == binding
    }

    /// Deliver inside the soldier phase, or queue the next tick between complete steps.
    pub fn stage_bot_combat_input(&mut self, frame: Frame) -> bool {
        if !self.bot_owns_combat_stamp(frame.stamp) {
            return false;
        }
        let Some(infantry) = self.units.infantry.as_mut() else {
            return false;
        };
        let tick = if self.tick.running {
            infantry.tick
        } else {
            infantry.tick.saturating_add(1)
        };
        infantry.bot_inputs.stage(tick, frame)
    }

    /// Read once after the soldier loop, while the rules still own this input frame.
    pub fn take_bot_combat_feedback(&mut self, stamp: Stamp) -> Option<Feedback> {
        if !self.bot_owns_combat_stamp(stamp) {
            return None;
        }
        let infantry = self.units.infantry.as_mut()?;
        if infantry.tick != stamp.tick {
            return None;
        }
        infantry.bot_inputs.feedback(stamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_delivered_equipped_life_can_commit_and_drain_a_soldier_result() {
        let mut inputs = BotInputs::new(2);
        inputs.install(3);
        let weapon = Binding {
            slot: 0,
            weapon: DefId(5),
            channel: 1,
        };
        let life = LifeId {
            slot: 1,
            slab: 2,
            generation: 9,
        };
        let stamp = Stamp {
            installation: 3,
            tick: 20,
            life,
            binding: Some(weapon),
        };
        let frame = Frame {
            stamp,
            fire: false,
            fire_pressed: true,
            guard: false,
            guard_pressed: false,
            hold_to_block: false,
            forward: 0.5,
            right: -0.25,
        };
        let report = Feedback {
            guarding: true,
            energy: 7.,
            exhausted: false,
        };
        assert!(inputs.stage(20, frame));
        assert!(!inputs.stage(20, frame));
        for other in [
            LifeId { slab: 3, ..life },
            LifeId {
                generation: 10,
                ..life
            },
        ] {
            assert!(inputs.take(20, other, Some(weapon)).is_none());
        }
        assert!(inputs
            .take(
                20,
                life,
                Some(Binding {
                    channel: 0,
                    ..weapon
                })
            )
            .is_none());
        assert!(inputs
            .take(
                20,
                life,
                Some(Binding {
                    weapon: DefId(9),
                    ..weapon
                })
            )
            .is_none());
        assert!(inputs
            .take(20, life, Some(Binding { slot: 1, ..weapon }))
            .is_none());
        assert!(inputs.take(21, life, Some(weapon)).is_none());
        inputs.commit(stamp, report);
        assert!(inputs.feedback(stamp).is_none());
        let first = inputs.take(20, life, Some(weapon)).unwrap();
        assert!(first.reset_binding);
        let controls = first.frame.controls();
        assert_ne!(controls.melee_pressed, 0);
        assert_eq!(controls.melee_held, 0);
        assert!(inputs.take(20, life, Some(weapon)).is_none());
        inputs.commit(stamp, report);
        assert_eq!(inputs.feedback(stamp), Some(report));
        inputs.commit(stamp, report);
        assert_eq!(inputs.feedback(stamp), None);
        let reborn = Frame {
            stamp: Stamp {
                life: LifeId {
                    generation: 10,
                    ..life
                },
                ..stamp
            },
            ..frame
        };
        assert!(inputs.stage(20, reborn));
        assert!(inputs.take(20, life, Some(weapon)).is_none());
        assert!(inputs.take(20, reborn.stamp.life, Some(weapon)).is_some());
        inputs.install(4);
        assert!(!inputs.stage(20, frame));
        let seated = Frame {
            stamp: Stamp {
                binding: None,
                installation: 4,
                ..stamp
            },
            ..frame
        };
        assert!(inputs.stage(20, seated));
        assert!(inputs.take(20, life, None).is_none());
        assert_eq!(inputs.feedback(seated.stamp), None);
        let equipped = Frame {stamp: Stamp {installation: 4, tick: 21, ..stamp}, ..frame};
        assert!(inputs.stage(21, equipped));
        assert!(inputs.take(21, life, Some(weapon)).unwrap().reset_binding);
        let unchanged = Frame {stamp: Stamp {tick: 22, ..equipped.stamp}, ..equipped};
        assert!(inputs.stage(22, unchanged));
        assert!(!inputs.take(22, life, Some(weapon)).unwrap().reset_binding);
    }
}
