//! Lifetime and kill recovery for a human in an awarded hero slot.
use crate::battle::props;
use crate::gen::{Attacker, Character, Store, Unit};
use crate::store::Ref;
use crate::units::f3;
use crate::world::owner::World;

fn awarded(s: &Store, character: Ref<Character>) -> Option<Ref<Unit>> {
    let c = s.character.get(character)?;
    let r = c.unit?;
    let body = s.unit.get(r)?;
    (c.human
        && c.hero == Some(true)
        && body.alive
        && !body.combat.dead
        && body.character == Some(character))
    .then_some(r)
}

pub(crate) fn advance(w: &mut World, dt: f64) {
    if dt <= 0.0 || w.sim.store.battle.over {
        return;
    }
    for i in 0..w.sim.store.spawning.characters.len() {
        let character = w.sim.store.spawning.characters[i];
        let Some(r) = awarded(&w.sim.store, character) else {
            continue;
        };
        let combat = &mut w.sim.store.unit.get_mut(r).unwrap().combat;
        let debit = 10.0 * dt;
        if combat.health > debit {
            combat.health -= debit;
            f3::changed_in(w, r);
        } else {
            f3::with_services_in(w, |w, services| {
                crate::combat::live::damage_in(
                    w,
                    r,
                    f64::INFINITY,
                    Attacker::Null,
                    services.expect("admitted character services"),
                )
            });
        }
    }
}

pub(crate) fn credited_kill(w: &mut World, credit: super::awards::Credit, victim_team: i32) {
    if !credit.human || credit.team == victim_team || w.sim.store.battle.over {
        return;
    }
    let Some(character) = credit.character else {
        return;
    };
    let Some(r) = awarded(&w.sim.store, character) else {
        return;
    };
    let kills = w.sim.store.character.get(character).unwrap().kills;
    let amount = if kills < 15.0 {
        150.0
    } else if kills < 30.0 {
        100.0
    } else {
        50.0
    };
    let combat = &mut w.sim.store.unit.get_mut(r).unwrap().combat;
    if let Some(next) = props::supply_health(combat.health, combat.max_health, amount) {
        combat.health = next;
        f3::changed_in(w, r);
    }
}
