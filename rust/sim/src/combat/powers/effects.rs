//! What powers leave on a soldier: timed damage and defence modifiers,
//! health regeneration, and being held by a choke.

use super::def::Buff;
use super::key::LifeKey;
use crate::ordnance::math::V3;
use crate::store::StrId;

/// A choked soldier stays held this long after the last tick that held it.
pub const HOLD_RELEASE_TICKS: u32 = 30;

/// One timed modifier. A new one replaces it only when it lasts longer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timed {
    pub seconds: f64,
    pub value: f64,
}

impl Timed {
    pub fn active(&self) -> bool {
        self.seconds > 0.0
    }

    fn give(&mut self, buff: Buff) -> bool {
        if buff.seconds > self.seconds {
            *self = Timed { seconds: buff.seconds, value: buff.value };
            return true;
        }
        false
    }

    fn tick(&mut self, dt: f64) {
        if self.seconds > 0.0 {
            self.seconds -= dt;
            if self.seconds < 0.0 {
                self.seconds = 0.0;
            }
        }
    }

    /// The multiplier while active, otherwise one.
    pub fn scale(&self) -> f64 {
        if self.active() {
            self.value
        } else {
            1.0
        }
    }
}

/// A soldier held by another's choke.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Held {
    pub by: LifeKey,
    /// Ticks left before the hold lapses without another refresh.
    pub left: u32,
    /// Direction from the soldier toward the one holding it.
    pub toward: V3,
    /// CRC of the victim animation named by the holding power (0 when unset).
    pub anim: u32,
}

/// Periodic fire/smolder burn left by an emitter weapon (flamethrower).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Burn {
    pub seconds: f64,
    pub dps: f64,
    pub attacker: LifeKey,
    pub effect: Option<StrId>,
    pub fx_cooldown: f64,
}

/// Everything powers have done to one life.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Received {
    pub life: LifeKey,
    pub offense: Timed,
    pub defense: Timed,
    pub regen: Timed,
    pub held: Option<Held>,
    pub burn: Option<Burn>,
}

impl Received {
    pub fn new(life: LifeKey) -> Self {
        Self { life, offense: Timed::default(), defense: Timed::default(), regen: Timed::default(), held: None, burn: None }
    }

    pub fn is_clear(&self) -> bool {
        !self.offense.active() && !self.defense.active() && !self.regen.active() && self.held.is_none() && self.burn.is_none()
    }
}

/// Received effects for every unit slot of a battle, allocated when the
/// battle starts and grown only when its unit slots grow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectTable {
    rows: Vec<Option<Received>>,
}

impl EffectTable {
    /// An empty table, as every battle without powers keeps.
    pub const fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn with_slots(slots: usize) -> Self {
        Self { rows: vec![None; slots] }
    }

    /// Room for `slots` unit slots, keeping every row already there.
    pub fn reserve(&mut self, slots: usize) {
        if self.rows.len() < slots {
            self.rows.resize(slots, None);
        }
    }

    pub fn slots(&self) -> usize {
        self.rows.len()
    }

    /// The row of `life`, if it has one. A row of an earlier life is ignored.
    pub fn get(&self, life: LifeKey) -> Option<&Received> {
        self.rows.get(life.slot as usize)?.as_ref().filter(|r| r.life == life)
    }

    fn row(&mut self, life: LifeKey) -> Option<&mut Received> {
        let slot = self.rows.get_mut(life.slot as usize)?;
        if slot.is_none_or(|r| r.life != life) {
            *slot = Some(Received::new(life));
        }
        slot.as_mut()
    }

    /// Give an aura's modifiers. Returns whether any of them took hold.
    pub fn give(&mut self, life: LifeKey, offense: Option<Buff>, defense: Option<Buff>, regen: Option<Buff>) -> bool {
        let Some(row) = self.row(life) else { return false };
        let mut took = false;
        if let Some(b) = offense {
            took |= row.offense.give(b);
        }
        if let Some(b) = defense {
            took |= row.defense.give(b);
        }
        if let Some(b) = regen {
            took |= row.regen.give(b);
        }
        took
    }

    /// Hold `life` for another [`HOLD_RELEASE_TICKS`].
    pub fn hold(&mut self, life: LifeKey, by: LifeKey, toward: V3, anim: u32) {
        if let Some(row) = self.row(life) {
            row.held = Some(Held { by, left: HOLD_RELEASE_TICKS, toward, anim });
        }
    }

    /// Clear the row of a slot whose life ended or changed.
    pub fn clear_slot(&mut self, slot: u32) {
        if let Some(row) = self.rows.get_mut(slot as usize) {
            *row = None;
        }
    }

    /// Count down every timer, then report the regeneration owed to each row
    /// whose timer is still running (life and health per second).
    pub fn tick(&mut self, dt: f64, mut regen: impl FnMut(LifeKey, f64)) {
        for slot in self.rows.iter_mut() {
            let Some(row) = slot else { continue };
            row.offense.tick(dt);
            row.defense.tick(dt);
            row.regen.tick(dt);
            if row.regen.active() {
                regen(row.life, row.regen.value);
            }
            if let Some(h) = &mut row.held {
                h.left = h.left.saturating_sub(1);
                if h.left == 0 {
                    row.held = None;
                }
            }
            if row.is_clear() {
                *slot = None;
            }
        }
    }

    /// Multiplier for damage `attacker` deals to `victim`.
    pub fn damage_scale(&self, attacker: Option<LifeKey>, victim: Option<LifeKey>) -> f64 {
        let offense = attacker.and_then(|a| self.get(a)).map_or(1.0, |r| r.offense.scale());
        let defense = victim.and_then(|v| self.get(v)).map_or(1.0, |r| r.defense.scale());
        offense * defense
    }

    /// Apply or refresh a smolder burn over time.
    pub fn ignite(&mut self, life: LifeKey, attacker: LifeKey, dps: f64, seconds: f64, effect: Option<StrId>) {
        if !(dps > 0.0 && seconds > 0.0) {
            return;
        }
        let Some(row) = self.row(life) else { return };
        match &mut row.burn {
            Some(existing) => {
                existing.seconds = jsrt::js_max(existing.seconds, seconds);
                existing.dps = jsrt::js_max(existing.dps, dps);
                existing.attacker = attacker;
                if effect.is_some() {
                    existing.effect = effect;
                }
            }
            None => {
                row.burn = Some(Burn { seconds, dps, attacker, effect, fx_cooldown: 0.0 });
            }
        }
    }

    /// Step active smolder burns, reporting each burning life and whether its
    /// fire presentation effect should refresh this tick.
    pub fn tick_burns(&mut self, dt: f64, mut on_burn: impl FnMut(LifeKey, Burn, bool)) {
        for slot in self.rows.iter_mut() {
            let Some(row) = slot else { continue };
            let Some(b) = &mut row.burn else { continue };
            b.seconds -= dt;
            b.fx_cooldown -= dt;
            let emit_fx = b.fx_cooldown <= 0.0;
            if emit_fx {
                b.fx_cooldown = 0.25;
            }
            let snapshot = *b;
            let expired = b.seconds <= 0.0;
            if expired {
                row.burn = None;
            }
            let life = row.life;
            if row.is_clear() {
                *slot = None;
            }
            on_burn(life, snapshot, emit_fx);
        }
    }

    pub fn held(&self, life: LifeKey) -> Option<Held> {
        self.get(life).and_then(|r| r.held)
    }

    pub fn held_entries(&self) -> impl Iterator<Item = (LifeKey, Held)> + '_ {
        self.rows
            .iter()
            .flatten()
            .filter_map(|r| r.held.map(|h| (r.life, h)))
    }

    pub fn burning(&self, life: LifeKey) -> Option<Burn> {
        self.get(life).and_then(|r| r.burn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burn_ticks_refreshes_and_expires_cleanly() {
        let mut table = EffectTable::with_slots(4);
        let victim = LifeKey { slot: 1, generation: 1, life: 1.0 };
        let attacker = LifeKey { slot: 2, generation: 1, life: 1.0 };
        table.ignite(victim, attacker, 40.0, 0.5, Some(StrId(7)));
        assert!(table.burning(victim).is_some());

        let mut total_dmg = 0.0;
        let mut fx_pulses = 0u32;
        for _ in 0..15 {
            table.tick_burns(1.0 / 60.0, |life, burn, fx| {
                assert_eq!(life, victim);
                total_dmg += burn.dps * (1.0 / 60.0);
                if fx { fx_pulses += 1; }
            });
        }
        assert!((total_dmg - 10.0).abs() < 1e-6);
        assert_eq!(fx_pulses, 1);

        // Refresh timer back to 0.5 s mid-burn.
        table.ignite(victim, attacker, 40.0, 0.5, Some(StrId(7)));
        assert!((table.burning(victim).unwrap().seconds - 0.5).abs() < 1e-9);

        // Run past 0.5 s until clear.
        for _ in 0..40 {
            table.tick_burns(1.0 / 60.0, |_, burn, fx| {
                total_dmg += burn.dps * (1.0 / 60.0);
                if fx { fx_pulses += 1; }
            });
        }
        assert!(table.burning(victim).is_none());
        assert!(table.get(victim).is_none());
        assert!((total_dmg - 30.0).abs() < 0.7);
        assert!(fx_pulses >= 3);
    }
}
