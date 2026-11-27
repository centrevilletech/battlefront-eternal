//! Equipped input for every bot body in an event battle.

use bfe_wide::Wide;
use super::*;

/// Per-life 2005 melee guard timer mirroring the alternating ON/OFF combat guard window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct GuardSchedule {
    slot: u32,
    generation: u32,
    life: Wide,
    in_combat: bool,
    guard_on: bool,
    active: bool,
    next_flip: f64,
    last_think: u32,
}

impl GuardSchedule {
    fn sync_life(&mut self, key: &F3UnitKey) {
        let life = Wide::from_f64_bits(key.life);
        if self.slot != key.slot || self.generation != key.generation || self.life != life {
            *self = Self { slot: key.slot, generation: key.generation, life, ..Self::default() };
        }
    }

    fn observe_think(&mut self, think_count: u32) -> bool {
        let changed = self.last_think != think_count;
        self.last_think = think_count;
        changed
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn step(
        &mut self,
        now: f64,
        in_combat: bool,
        think: bool,
        melee_selected: bool,
        melee_throw_selected: bool,
        melee_fire_raw: bool,
        fire: bool,
        mut draw: impl FnMut(f64, f64) -> f64,
    ) -> bool {
        if !in_combat {
            self.in_combat = false;
            self.guard_on = false;
            self.active = false;
            return false;
        }
        if !self.in_combat {
            self.in_combat = true;
            self.guard_on = true;
            self.next_flip = now + draw(3.0, 6.0);
            if melee_selected || melee_throw_selected {
                self.active = true;
            }
        }
        if melee_selected && think {
            if self.next_flip < now {
                self.guard_on = !self.guard_on;
                let span = if self.guard_on { draw(3.0, 7.0) } else { draw(1.5, 4.0) };
                self.next_flip = now + span;
            }
            self.active = self.guard_on;
        }
        if melee_fire_raw || fire {
            self.active = false;
        }
        self.active
    }
}

/// Frames indexed by the character storage slot, independent of its controllable or goal.
#[derive(Default)]
pub struct SoldierInputs {
    pub(crate) fighters: Vec<crate::ai::combat::agent::saber::approach::Fighter>,
    pub(super) samples: Vec<Option<MeleeInput>>,
    guards: Vec<GuardSchedule>,
    pub(crate) epoch: u32,
    pub(super) block: BlockMode,
}

impl SoldierInputs {
    pub(crate) fn reserve(&mut self, characters: usize) -> Option<()> {
        if characters > self.samples.len() {
            self.samples.try_reserve_exact(characters - self.samples.len()).ok()?;
            self.samples.resize(characters, None);
            self.fighters.try_reserve_exact(characters - self.fighters.len()).ok()?;
            self.fighters.resize_with(characters, Default::default);
            self.guards.try_reserve_exact(characters - self.guards.len()).ok()?;
            self.guards.resize(characters, GuardSchedule::default());
        }
        Some(())
    }

    pub(crate) fn reserve_targets(&mut self, targets: usize) -> Option<()> {
        for fighter in &mut self.fighters { fighter.reserve(targets)?; }
        Some(())
    }

    pub(crate) fn pass<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, dt: f64,
    ) {
        let tick = crate::units::f3::tick_in(&world.rows.w.units);
        for character in 0..core.characters() {
            if let Some(view) = core.bot(world, character) {
                self.compose(&core, world, character, view, tick, dt);
            }
        }
    }

    pub(crate) fn take_soldier(&mut self, slot: usize, key: &F3UnitKey, tick: f64, binding: Option<Binding>) -> Option<MeleeInput> {
        let record = self.samples.get_mut(slot)?;
        if record.as_ref().is_none_or(|sample| sample.key != *key) {
            *record = Some(MeleeInput::new(self.epoch, key.clone(), UKey::MAX));
        }
        let sample = record.as_mut()?;
        if sample.tick > tick || sample.tick == tick && sample.consumed { return None; }
        if sample.tick != tick || sample.binding != binding {
            // A birth, mount or equipment change after the tail has no armed intent.
            sample.tick = f64::NAN;
            sample.step(tick, 0.0, Wanted { binding, ..Wanted::default() }, self.block);
        }
        sample.consumed = true;
        Some(sample.clone())
    }

    pub(crate) fn report_committed(
        &mut self, tick: f64, mut committed: impl FnMut(&MeleeInput) -> Option<super::super::melee::MeleeReport>,
    ) {
        for sample in self.samples.iter_mut().flatten() {
            if sample.consumed && sample.tick == tick {
                if let Some(value) = committed(sample) {
                    sample.report(value.guarding, value.energy, value.exhausted);
                }
            }
        }
    }

    pub(crate) fn retry_frame(&mut self, slot: usize, frame: &MeleeInput) {
        if let Some(sample) = self.samples.get_mut(slot).and_then(Option::as_mut) {
            if sample.epoch == frame.epoch && sample.is_for(&frame.key, frame.tick) && sample.binding == frame.binding {
                sample.consumed = false;
            }
        }
    }

    pub(crate) fn feedback(&mut self, slot: usize, frame: &MeleeInput, guarding: bool, energy: f64, exhausted: bool) -> bool {
        let Some(sample) = self.samples.get_mut(slot).and_then(Option::as_mut) else { return false };
        sample.epoch == frame.epoch && sample.is_for(&frame.key, frame.tick) && sample.binding == frame.binding &&
            sample.report(guarding, energy, exhausted)
    }

    /// Make the bot's melee sample for the next fixed tick: the trigger of
    /// the channel holding its selected melee weapon as logical Fire, and the
    /// dedicated block while its combat target holds a melee weapon within
    /// reach and the bot is not attacking. A seated bot holds nothing.
    pub(super) fn compose<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(
        &mut self, core: &GoalCore<'_>, world: &mut World<'_, '_, C, M, S>, character: usize, view: BotView, tick: f64, dt: f64,
    ) -> Option<usize> {
        let now = core.now(world);
        let swbf2 = world.rows.state.navigation.nav.swbf2.is_some();
        let (slot, key, seated, forward, right, tap_pressed, tap_was) = {
            let rows = &*world.rows;
            let owner = rows.state.roster.reference(row_context!(rows), &*rows.characters, character)?;
            let body = rows.w.sim.store.character.get(owner)?.unit?;
            let unit = rows.w.sim.store.unit.get(body)?;
            let slot = owner.slot as usize;
            let key = F3UnitKey::of(body, unit);
            let tap_pressed = [
                unit.controls.pressed.first() == Some(&true),
                unit.controls.pressed.get(1) == Some(&true),
            ];
            let tap_was = [unit.controls.was[0], unit.controls.was[1]];
            (slot, key, view.mounted || unit.seat.is_some(), unit.intent.forward, unit.intent.right, tap_pressed, tap_was)
        };
        let epoch = self.epoch;
        let record = self.samples.get_mut(slot)?;
        let input = record.get_or_insert_with(|| MeleeInput::new(epoch, key.clone(), view.unit));
        if input.key != key || input.epoch != epoch {
            *input = MeleeInput::new(epoch, key.clone(), view.unit);
        }
        let guard_slot = self.guards.get_mut(slot)?;
        guard_slot.sync_life(&key);
        let think = guard_slot.observe_think(core.think_count(view.ci));
        let wanted = if seated {
            guard_slot.step(now, false, think, false, false, false, false, |lo, _| lo);
            Wanted { binding: None, fire: false, block: false, forward: 0.0, right: 0.0 }
        } else if swbf2 {
            let (binding, fire) = melee_binding(world, view.unit);
            let in_combat = matches!(core.running(view.ci), Running::Fight | Running::GoalAttack);
            let (selected_channel, melee_fire_raw) = world
                .rows
                .state
                .controllers
                .read_core(view.ci)
                .and_then(|c| c.perception.as_ref())
                .map_or((0.0, false), |p| (p.fire.channel, p.fire.melee_fire_raw));
            let melee_selected = binding.is_some_and(|b| selected_channel == f64::from(b.channel));
            let melee_throw_selected = !melee_selected
                && world
                    .rows
                    .current_weapon(view.unit, selected_channel)
                    .and_then(|slot| world.rows.weapon_definition(view.unit, slot))
                    .is_some_and(|def| def.get("base").is_str("meleethrow"));
            let guard_active = guard_slot.step(
                now,
                in_combat,
                think,
                melee_selected,
                melee_throw_selected,
                melee_fire_raw,
                fire,
                |lo, hi| lo + (hi - lo) * Draws::a_uniform(world.random),
            );
            let block = binding.is_some() && guard_active && !fire;
            Wanted { binding, fire: !block && fire, block, forward, right }
        } else {
            let rows = &*world.rows;
            let owner = rows.state.roster.reference(row_context!(rows), &*rows.characters, character)?;
            let body = rows.w.sim.store.character.get(owner)?.unit?;
            let unit = rows.w.sim.store.unit.get(body)?;
            let (binding, fire) = melee_binding(world, view.unit);
            let position = world.unit_scalars(view.unit).position;
            let target = core.combat_target(view.ci).filter(|t| {
                let s = world.unit_scalars(t.unit);
                s.alive && s.generation.to_bits() == t.generation.to_bits()
            });
            let reach = crate::ai::combat::agent::saber::SWING_REACH;
            let (in_reach, facing) = target.map_or((false, false), |t| {
                let p = world.unit_scalars(t.unit).position;
                let d = Vec3::new(p.x - position.x, p.y - position.y, p.z - position.z);
                let f = unit.forward_on_foot();
                (d.x*d.x + d.y*d.y + d.z*d.z <= reach*reach, d.x*f.x + d.z*f.z >= 0.0)
            });
            let dueling = in_reach && core.running(view.ci) == Running::Fight;
            let opponent = target.filter(|_| dueling && facing).zip(binding);
            let pulse = input.swings.pulse(tick as u32, opponent);
            let swing = if dueling { pulse } else { fire };
            let attacker = match unit.combat.last_attacker {
                crate::gen::Attacker::Unit(r) => Some(r),
                crate::gen::Attacker::OwnerSnapshot(r) => crate::combat::owners::resolve(&rows.w.sim.store, r),
                _ => None,
            }.and_then(|r| rows.state.entities.find(Entity::Unit(r)));
            let incoming = |u| {
                let Some(Entity::Unit(body)) = rows.state.entities.get(u) else { return false };
                let Some(other) = rows.w.sim.store.unit.get(body) else { return false };
                if !other.alive || other.removed || core.relation(unit.team, other.team) >= 0.0 { return false; }
                let p = other.position();
                let d = Vec3::new(p.x - position.x, p.y - position.y, p.z - position.z);
                let threshold = 0.65 * v8math::sqrt(d.x*d.x + d.y*d.y + d.z*d.z);
                let faces = |actor: &crate::gen::Unit, sign: f64| {
                    let f = actor.forward_on_foot();
                    sign * (d.x*f.x + d.y*f.y + d.z*f.z) > threshold
                };
                faces(unit, 1.) && faces(other, -1.) && (0..2).any(|channel| other.controls.was[channel] > 0.0 &&
                    rows.current_weapon(u, channel as f64).and_then(|slot| rows.weapon_definition(u, slot))
                        .is_some_and(|d| !d.get("base").is_str("melee")))
            };
            let guard_window = !dueling || !swing && (4..30).contains(&(tick as u32 % 60)) && input.guard_reserve(binding);
            let block = binding.is_some() && guard_window && (attacker.is_some_and(incoming) || target.is_some_and(|t| incoming(t.unit)) ||
                !swing && target.is_some_and(|t| horizontal(position, world.unit_scalars(t.unit).position) < BLOCK_REACH
                    && melee_binding(world, t.unit).0.is_some()));
            let fire = !block && swing;
            Wanted { binding, fire, block, forward, right }
        };
        // Made after this tick's soldier phase, for the next one.
        let tap = !wanted.block && wanted.binding.is_some_and(|b| {
            let channel = usize::from(b.channel);
            tap_pressed[channel] && tap_was[channel] <= 0.0
        });
        let mode = if swbf2 { BlockMode::Hold } else { self.block };
        input.step_with_tap(tick + 1.0, dt, wanted, mode, tap);
        Some(slot)
    }
}

#[cfg(test)]
mod guard_schedule_tests {
    use super::*;

    #[test]
    fn guard_schedule_starts_raised_alternates_windows_and_rearms_only_on_think_ticks() {
        let mut sched = GuardSchedule::default();
        let mut mid = |lo: f64, hi: f64| 0.5 * (lo + hi);
        // Entering combat starts guard raised for 3..6 s (midpoint 4.5 s).
        assert!(sched.step(10.0, true, true, true, false, false, false, &mut mid));
        assert!(sched.step(14.4, true, false, true, false, false, false, &mut mid));
        // A melee fire request clears guard immediately; non-think ticks leave it lowered until the next think.
        assert!(!sched.step(14.45, true, false, true, false, true, false, &mut mid));
        assert!(!sched.step(14.46, true, false, true, false, false, false, &mut mid));
        assert!(!sched.step(14.47, true, false, true, false, false, false, &mut mid));
        assert!(sched.step(14.48, true, true, true, false, false, false, &mut mid));
        // Past 14.5 s guard flips off on the next think for 1.5..4.0 s (midpoint 2.75 s -> until 17.35 s).
        assert!(!sched.step(14.6, true, true, true, false, false, false, &mut mid));
        assert!(!sched.step(17.3, true, true, true, false, false, false, &mut mid));
        // Past 17.35 s guard flips back on for 3.0..7.0 s (midpoint 5.0 s -> until 22.4 s).
        assert!(sched.step(17.4, true, true, true, false, false, false, &mut mid));
        assert!(sched.step(22.3, true, true, true, false, false, false, &mut mid));
        assert!(!sched.step(22.5, true, true, true, false, false, false, &mut mid));
        // Verify both endpoints of the subsequent ON window (3.0 s minimum, 7.0 s maximum).
        let mut lo_sched = GuardSchedule::default();
        let mut lo = |a: f64, _: f64| a;
        assert!(lo_sched.step(0.0, true, true, true, false, false, false, &mut lo));
        assert!(!lo_sched.step(3.1, true, true, true, false, false, false, &mut lo));
        assert!(lo_sched.step(4.7, true, true, true, false, false, false, &mut lo));
        assert!(lo_sched.step(7.6, true, true, true, false, false, false, &mut lo));
        assert!(!lo_sched.step(7.8, true, true, true, false, false, false, &mut lo));
        let mut hi_sched = GuardSchedule::default();
        let mut hi = |_: f64, b: f64| b;
        assert!(hi_sched.step(0.0, true, true, true, false, false, false, &mut hi));
        assert!(!hi_sched.step(6.1, true, true, true, false, false, false, &mut hi));
        assert!(hi_sched.step(10.2, true, true, true, false, false, false, &mut hi));
        assert!(hi_sched.step(17.1, true, true, true, false, false, false, &mut hi));
        assert!(!hi_sched.step(17.3, true, true, true, false, false, false, &mut hi));
        // Leaving combat lowers guard immediately.
        assert!(!sched.step(22.6, false, false, true, false, false, false, &mut mid));
    }
}

/// The selected melee weapon (the primary channel first) as the native
/// selectors give it, and whether its channel's trigger is down.
fn melee_binding<C: Changes + ?Sized, M: Mounted, S: Services<C, M>>(world: &World<'_, '_, C, M, S>, unit: UKey) -> (Option<Binding>, bool) {
    let rows = &*world.rows;
    let Some(Entity::Unit(body)) = rows.state.entities.get(unit) else { return (None, false) };
    let Some(soldier) = rows.w.sim.store.unit.get(body) else { return (None, false) };
    let swbf2 = rows.state.navigation.nav.swbf2.is_some();
    for channel in 0..2u8 {
        let Some(slot) = rows.current_weapon(unit, f64::from(channel)) else { continue };
        if !rows.weapon_definition(unit, slot).is_some_and(|def| def.get("base").is_str("melee")) {
            continue;
        }
        let Some(weapon) = soldier.combat.weapons.get(slot).map(|w| w.def) else { continue };
        let fire = if swbf2 {
            soldier.intent.buttons[usize::from(channel)] > 0.0
        } else {
            soldier.controls.was[usize::from(channel)] > 0.0
        };
        return (Some(Binding { slot, weapon, channel }), fire);
    }
    (None, false)
}
