use bfe_sim::battle::{commands::{Commands, Result}, rules};
use bfe_sim::gen::{Unit, Character, F4Command, F4CommandOp, F4Handle, F4ObjectKind, Store};
#[test]
fn reassignment_rejects_delayed_commands_without_resetting_respawn_credit() {
    let mut s = Store::default(); let mut commands = Commands::new(2);
    let unit = s.unit.insert(Unit { generation: 1.0, ..Unit::default() });
    let c = s.character.insert(Character { unit: Some(unit), ..Character::default() }); let h = commands.adopt(&s, c);
    let mut input = F4Command { target: h.clone(), op: F4CommandOp::Point, ..F4Command::default() };
    assert_eq!(commands.apply(&mut s, &input), Result::Applied);
    assert_eq!(s.character.get(c).unwrap().captures, 1.0);
    // Unit lifecycle changes do not reassign this Character's scoreboard life.
    s.character.get_mut(c).unwrap().unit = None;
    s.unit.get_mut(unit).unwrap().generation = 2.0;
    s.character.get_mut(c).unwrap().unit = Some(unit);
    assert_eq!(commands.apply(&mut s, &input), Result::Applied);
    let next = commands.adopt(&s, c); commands.published();
    assert_eq!(commands.apply(&mut s, &input), Result::Stale); assert!(commands.dirty.is_empty());
    input.target = next; assert_eq!(commands.apply(&mut s, &input), Result::Applied);
    assert_eq!(commands.dirty, [c]);
}
#[test]
fn stale_killer_rejects_credit_before_mutating_the_victim() {
    let mut s = Store::default(); s.battle.stats = rules::new_stats(); let mut commands = Commands::new(2);
    let victim = s.character.insert(Character::default()); let killer = s.character.insert(Character::default());
    let v = commands.adopt(&s, victim); let k = commands.adopt(&s, killer);
    commands.adopt(&s, killer);
    let input = F4Command { target: v, other: Some(k), op: F4CommandOp::Credit, value: -1.0, ..F4Command::default() };
    assert_eq!(commands.apply(&mut s, &input), Result::Stale);
    assert_eq!(s.character.get(victim).unwrap().deaths, 0.0); assert_eq!(s.character.get(killer).unwrap().kills, 0.0);
    assert!(!commands.battle_dirty);
}
#[test]
fn pooled_character_slots_cannot_inherit_old_score_commands() {
    let mut s = Store::default(); let mut commands = Commands::new(1);
    let old = s.character.insert(Character::default()); let h = commands.adopt(&s, old);
    s.character.remove(old); let next = s.character.insert(Character::default()); commands.adopt(&s, next);
    assert_eq!(old.slot, next.slot);
    assert_eq!(commands.apply(&mut s, &F4Command { target: h, op: F4CommandOp::Point, ..F4Command::default() }), Result::Stale);
    assert_eq!(s.character.get(next).unwrap().captures, 0.0);
}
#[test]
fn the_battle_handle_qualifies_ticket_commands() {
    let mut s = Store::default(); s.battle.reinforcements.set(1.0, 10.0);
    let mut commands = Commands::new(0);
    let mut input = F4Command { target: F4Handle { kind: F4ObjectKind::Battle, slot: 0, generation: 1, life: 1.0 },
        op: F4CommandOp::Spend, value: 2.0, ..F4Command::default() };
    input.team = 1.0;
    assert_eq!(commands.apply(&mut s, &input), Result::Applied);
    assert_eq!(s.battle.reinforcements.at(1.0), Some(8.0));
    input.target.life = 0.0;
    assert_eq!(commands.apply(&mut s, &input), Result::Stale);
    assert_eq!(s.battle.reinforcements.at(1.0), Some(8.0));
}
