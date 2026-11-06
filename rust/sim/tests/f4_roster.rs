//! Network roster lifecycle over canonical Characters, without game content.
use bfe_sim::battle::{commands::{Commands, Result as CommandResult}, roster::{self, RosterWorld}, spawning};
use bfe_sim::gen::*;
use bfe_sim::store::{Ref, StrId};
#[derive(Default)]
struct World { records: Option<bfe_sim::battle::stat_records::Records>, events: Vec<&'static str>, commands: Option<Commands>, lives: Vec<F4Handle>, reads: usize }
impl RosterWorld for World {
    fn recycle_record(&mut self, s: &mut Store, c: Ref<Character>) { if let Some(records) = &mut self.records { records.reset(s, c); } }
    fn unit_alive(&mut self, s: &Store, u: Ref<Unit>) -> bool { self.reads += 1; s.unit.get(u).unwrap().alive }
    fn reassigned(&mut self, s: &Store, c: Ref<Character>) {
        self.lives.push(self.commands.as_mut().unwrap().adopt(s, c)); self.events.push("life");
    }
    fn ai_detach(&mut self, _: &mut Store, _: Ref<Character>) { self.events.push("detach"); }
    fn ai_attach(&mut self, _: &mut Store, _: Ref<Character>) { self.events.push("attach"); }
    fn retire_unit(&mut self, s: &mut Store, c: Ref<Character>, u: Ref<Unit>) {
        // A real native unit service completes unseat/removal synchronously;
        // the kernel must still expose the retiring Character here.
        assert_eq!(s.character.get(c).unwrap().unit, Some(u));
        s.unit.get_mut(u).unwrap().removed = true; self.events.push("remove");
    }
    fn bot_name(&mut self, _: &mut Store, _: usize) -> StrId { StrId(1) }
    fn player_name(&mut self, name: &[u16]) -> StrId { assert!(name.len() <= 32); StrId(2) }
    fn insert(&mut self, _: &mut Store, _: f64, _: StrId) -> Option<Ref<Character>> { panic!("unexpected construction"); }
    fn activate(&mut self, s: &mut Store, c: Ref<Character>) {
        assert!(!s.character.get(c).unwrap().human); assert_eq!(self.lives.last().unwrap().slot, c.slot);
        s.character.get_mut(c).unwrap().spawn.as_mut().unwrap().ready = true; self.events.push("activate");
    }
    fn pad_quotas(&mut self, _: &mut Store, _: usize) { self.events.push("quota"); }
}
fn character(s: &mut Store, team: usize, alive: Option<bool>) -> Ref<Character> {
    let unit = alive.map(|alive| s.unit.insert(Unit { alive, ..Unit::default() }));
    let c = s.character.insert(Character { team: team as f64, unit, spawn: Some(Spawn { timer: 0.0, ordinal: Some(-1.0), ..Spawn::default() }),
        kills: 4.0, deaths: 3.0, captures: 2.0, ..Character::default() });
    s.spawning.teams[team].characters.push(c); s.spawning.characters.push(c); s.battle.characters.push(c); c
}
fn setup() -> (Store, World) {
    let mut s = Store::default(); s.spawning.teams = spawning::make_teams(32.0);
    s.battle.reinforcements.set(1.0, 10.0); s.battle.reinforcements.set(2.0, 10.0);
    (s, World { commands: Some(Commands::new(8)), ..World::default() })
}
#[test]
fn network_reserve_prefers_first_dead_bot_and_releases_same_scoreboard_slot() {
    let (mut s, mut w) = setup(); let live = character(&mut s, 1, Some(true)); let dead = character(&mut s, 1, Some(false));
    let absent = character(&mut s, 1, None); let order = s.battle.characters.clone();
    s.spawning.teams[1].living = 1.0;
    let old = w.commands.as_mut().unwrap().adopt(&s, dead);
    let c = roster::reserve_with(&mut s, 1.0, PlayerId::F64(9.0), &[65], &mut w).unwrap();
    assert_eq!(c, dead); assert_eq!(w.reads, 3); // Both body facts, then retirement's fresh read.
    assert_eq!(w.events, ["detach", "remove", "life"]);
    let ch = s.character.get(c).unwrap(); assert!(ch.human); assert_eq!(ch.kills + ch.deaths + ch.captures, 0.0);
    assert_eq!(ch.player_id, PlayerId::F64(9.0)); assert_eq!(ch.spawn.as_ref().unwrap().timer, f64::INFINITY);
    assert_eq!(w.commands.as_mut().unwrap().apply(&mut s, &F4Command { target: old, op: F4CommandOp::Point, ..F4Command::default() }), CommandResult::Stale);
    w.events.clear(); roster::release_with(&mut s, c, &mut w);
    assert_eq!(w.events, ["detach", "life", "activate", "attach"]);
    assert_eq!(s.battle.characters, order); assert_eq!(s.spawning.teams[1].characters, [live, dead, absent]);
    assert_eq!(s.spawning.teams[1].living, 1.0); assert_eq!(s.battle.reinforcements.at(1.0), Some(10.0));
    let c2 = roster::reserve_with(&mut s, 1.0, PlayerId::F64(10.0), &[66], &mut w).unwrap();
    assert_eq!(c2, c); assert_eq!(s.battle.characters.len(), 3);
}
#[test]
fn switching_teams_trades_roster_positions_and_keeps_wire_character_order() {
    let (mut s, mut w) = setup(); let a = character(&mut s, 1, None); let b = character(&mut s, 2, Some(true));
    s.character.get_mut(a).unwrap().human = true; s.spawning.teams[2].living = 1.0;
    let old = w.commands.as_mut().unwrap().adopt(&s, a);
    assert!(roster::switch_team_with(&mut s, a, 2.0, &mut w));
    assert_eq!(w.events, ["detach", "remove", "life", "life", "activate", "attach"]);
    assert_eq!(s.battle.characters, [a, b]); assert_eq!(s.spawning.teams[1].characters, [b]); assert_eq!(s.spawning.teams[2].characters, [a]);
    assert_eq!(s.character.get(a).unwrap().team, 2.0); assert_eq!(s.character.get(b).unwrap().team, 1.0);
    assert_eq!(s.spawning.teams[2].living, 0.0); assert_eq!(roster::counts(&s), [(1.0, 0.0, 1.0), (2.0, 1.0, 1.0)]);
    assert_eq!(w.commands.as_mut().unwrap().apply(&mut s, &F4Command { target: old, op: F4CommandOp::Point, ..F4Command::default() }), CommandResult::Stale);
    w.events.clear(); assert!(roster::switch_team_with(&mut s, a, 2.0, &mut w)); assert!(w.events.is_empty());
    assert!(!roster::switch_team_with(&mut s, b, 2.0, &mut w)); assert!(w.events.is_empty());
}
#[test]
fn selection_reads_later_bots_even_after_finding_a_dead_slot() {
    let (mut s, mut w) = setup(); let first = character(&mut s, 1, None); character(&mut s, 1, Some(true)); character(&mut s, 1, Some(false));
    assert_eq!(roster::reserve_with(&mut s, 1.0, PlayerId::F64(1.0), &[], &mut w), Ok(first));
    assert_eq!(w.reads, 2); assert_eq!(w.events, ["detach", "life"]);
}

impl roster::PlayersWorld for World {
    fn recycle_record(&mut self, s: &mut Store, c: Ref<Character>) { if let Some(records) = &mut self.records { records.reset(s, c); } }
    fn reassigned(&mut self, s: &Store, c: Ref<Character>) {
        self.lives.push(self.commands.as_mut().unwrap().adopt(s, c)); self.events.push("life");
    }
    fn player_name(&mut self, name: &[u16]) -> StrId { assert!(name.len() <= 32); StrId(2) }
    fn new_human(&mut self, _: &mut roster::Players, s: &mut Store, team: f64) -> Ref<Character> {
        let c = s.character.insert(Character { human: true, team, ..Character::default() });
        s.battle.characters.push(c); self.events.push("birth"); c
    }
    fn remove_body(&mut self, _: &mut roster::Players, s: &mut Store, c: Ref<Character>, u: Ref<Unit>) {
        assert_eq!(s.character.get(c).unwrap().unit, Some(u));
        s.unit.get_mut(u).unwrap().removed = true; self.events.push("remove");
    }
}
#[test]
fn player_only_disconnect_reuses_wire_slot_and_invalidates_delayed_commands() {
    let (mut s, mut w) = setup();
    let mut players = roster::Players::new(&mut s, Some(1.0), false);
    let a = players.reserve(&mut s, 1.0, PlayerId::F64(10.0), &[65], &mut w).unwrap();
    let old = w.lives.last().unwrap().clone();
    assert!(players.reserve(&mut s, 1.0, PlayerId::F64(11.0), &[66], &mut w).is_err());
    let b = players.reserve(&mut s, 2.0, PlayerId::F64(12.0), &[67], &mut w).unwrap();
    assert!(!players.switch_team(&mut s, a, 2.0));
    let unit = s.unit.insert(Unit::default()); s.character.get_mut(a).unwrap().unit = Some(unit);
    w.events.clear(); players.release(&mut s, a, &mut w);
    assert_eq!(w.events, ["remove", "life"]); assert!(s.unit.get(unit).unwrap().removed);
    assert_eq!(s.character.get(a).unwrap().team, 0.0); assert_eq!(players.reserved, [b]);
    assert_eq!(w.commands.as_mut().unwrap().apply(&mut s, &F4Command { target: old, op: F4CommandOp::Point, ..F4Command::default() }), CommandResult::Stale);
    let disconnected = w.lives.last().unwrap().clone();
    let reused = players.reserve(&mut s, 1.0, PlayerId::F64(13.0), &[68], &mut w).unwrap();
    assert_eq!(reused, a); assert_eq!(s.battle.characters, [a, b]); assert_eq!(players.reserved, [b, a]);
    assert_eq!(s.character.get(a).unwrap().player_id, PlayerId::F64(13.0));
    assert_eq!(s.character.get(a).unwrap().spawn.as_ref().unwrap().timer, f64::INFINITY);
    assert_eq!(w.commands.as_mut().unwrap().apply(&mut s, &F4Command { target: disconnected, op: F4CommandOp::Point, ..F4Command::default() }), CommandResult::Stale);
}

#[test]
fn roster_reset_recycles_prepared_records_before_clearing_public_tallies() {
    use bfe_sim::battle::stat_records::Records;
    let (mut s, mut w) = setup(); let c = character(&mut s, 1, None);
    let mut records = Records::new(8); records.prepare(&mut s, c, 8); records.ensure(&mut s, c);
    let Nullable::Value(record) = &mut s.character.get_mut(c).unwrap().record else { unreachable!() };
    record.slots[0].headshots = 7.0; w.records = Some(records);
    assert_eq!(roster::reserve_with(&mut s, 1.0, PlayerId::F64(8.0), &[], &mut w), Ok(c));
    assert!(matches!(s.character.get(c).unwrap().record, Nullable::Null));
    w.records.as_mut().unwrap().ensure(&mut s, c);
    let Nullable::Value(record) = &s.character.get(c).unwrap().record else { unreachable!() };
    assert_eq!(record.slots[0].headshots, 0.0);
    roster::release_with(&mut s, c, &mut w);
    assert!(matches!(s.character.get(c).unwrap().record, Nullable::Null));
    w.records.as_mut().unwrap().ensure(&mut s, c);
    let Nullable::Value(record) = &s.character.get(c).unwrap().record else { unreachable!() };
    assert_eq!(record.health, 255.0);
}
