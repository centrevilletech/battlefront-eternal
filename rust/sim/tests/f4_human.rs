//! The actual native human phase with synchronous canonical spawn/team/death
//! services. No facade snapshot or game-content loader participates.
use bfe_sim::battle::{human::{Command, Event}, human_owned::{self, Human, Services}};
use bfe_sim::gen::{Character, HumanPhase, HumanPick, HumanSpawn, Spawn, Store, Unit};
use bfe_sim::store::{Ref, StrId};
struct World { character: Ref<Character>, unit: Ref<Unit>, roster: [StrId; 2], classic: bool }
impl Services for World {
    fn team(&mut self, s: &Store) -> f64 { s.character.get(self.character).unwrap().team }
    fn dead(&mut self, s: &Store) -> bool { !s.unit.get(self.unit).unwrap().alive }
    fn over(&mut self, s: &Store) -> bool { s.battle.over }
    fn roster(&mut self, _: &Store, _: f64) -> &[StrId] { &self.roster }
    fn classic(&mut self, _: &Store) -> bool { self.classic }
    fn character_spawn(&mut self, s: &Store) -> Option<(bool, f64)> {
        let c = s.character.get(self.character)?; Some((c.unit.is_some(), c.spawn.as_ref()?.timer))
    }
    fn can_spawn(&mut self, _: &Store, name: StrId) -> bool { self.roster.contains(&name) }
    fn set_team(&mut self, s: &mut Store, events: &mut Vec<Event>, team: f64) {
        let c = s.character.get_mut(self.character).unwrap(); let previous = c.team; c.team = team;
        human_owned::team_changed(s, events, team, previous, self);
    }
    fn spawn(&mut self, s: &mut Store, events: &mut Vec<Event>, choice: &HumanPick) {
        assert!(s.human_spawn.pending.is_some(), "spawn sees current canonical choice");
        s.unit.get_mut(self.unit).unwrap().alive = true;
        s.character.get_mut(self.character).unwrap().unit = Some(self.unit);
        human_owned::spawned(s, events, choice.class_name, choice.post, self);
    }
    fn ready(&mut self, s: &mut Store, _: &mut Vec<Event>, _: &HumanPick) -> bool {
        s.character.get_mut(self.character).unwrap().spawn.as_mut().unwrap().ready = true; true
    }
    fn kill(&mut self, s: &mut Store, events: &mut Vec<Event>) {
        s.unit.get_mut(self.unit).unwrap().alive = false;
        s.character.get_mut(self.character).unwrap().unit = None;
        human_owned::died(s, events, self);
    }
    fn cancel(&mut self, s: &mut Store, _: &mut Vec<Event>) { s.human_spawn.pending = None; }
    fn rejected(&mut self, _: &mut Store, _: &mut Vec<Event>, _: StrId, _: f64) {}
}
fn setup(classic: bool) -> (Store, Human, World) {
    let mut s = Store::default();
    let unit = s.unit.insert(Unit::default());
    let character = s.character.insert(Character { team: 1.0, human: true,
        spawn: Some(Spawn { timer: 0.0, ..Spawn::default() }), ..Character::default() });
    s.battle.characters.push(character);
    s.human_spawn = HumanSpawn { class_name: StrId(1), can_switch_teams: true, ..HumanSpawn::default() };
    (s, Human::new(4), World { character, unit, roster: [StrId(1), StrId(2)], classic })
}
#[test]
fn spawn_service_sees_the_current_choice_and_its_lifecycle_updates_resume_in_place() {
    let (mut s, mut h, mut w) = setup(false);
    h.step(&mut s, 1.0 / 60.0, &[Command::Spawn(HumanPick { class_name: StrId(2), post: None })], &mut w);
    assert_eq!(s.human_spawn.phase, HumanPhase::Alive); assert_eq!(s.human_spawn.class_name, StrId(2));
    assert!(s.human_spawn.pending.is_none()); assert!(s.human_spawn.spawned_once);
    assert!(matches!(h.events(), [Event::Spawned { team: 1.0, class_name: StrId(2), .. }]));
}
#[test]
fn team_choice_precedes_spawn_and_suicide_keeps_its_synchronous_death_notice() {
    let (mut s, mut h, mut w) = setup(false);
    h.step(&mut s, 0.0, &[Command::Team(2.0), Command::Spawn(HumanPick { class_name: StrId(1), post: None })], &mut w);
    assert!(matches!(h.events(), [Event::TeamChanged { team: 2.0, previous: 1.0 }, Event::Spawned { team: 2.0, .. }]));
    h.published(); h.step(&mut s, 0.0, &[Command::Suicide], &mut w);
    assert_eq!(s.human_spawn.phase, HumanPhase::Dying);
    assert!(matches!(h.events(), [Event::PlayerDied { team: 2.0, .. }]));
    assert_eq!(s.human_spawn.dead_time, 0.0);
}
#[test]
fn classic_ready_changes_only_the_native_waiting_character() {
    let (mut s, mut h, mut w) = setup(true);
    h.step(&mut s, 0.0, &[Command::Spawn(HumanPick { class_name: StrId(1), post: None })], &mut w);
    assert!(s.character.get(w.character).unwrap().spawn.as_ref().unwrap().ready);
    assert!(s.character.get(w.character).unwrap().unit.is_none());
    assert!(s.human_spawn.pending.is_none());
    assert!(!h.events().iter().any(|e| matches!(e, Event::Spawned { .. })));
}
#[test]
fn game_over_cancels_a_pending_choice_before_it_can_spawn() {
    let (mut s, mut h, mut w) = setup(false); s.battle.over = true;
    s.human_spawn.pending = Some(HumanPick { class_name: StrId(1), post: None });
    h.step(&mut s, 1.0, &[], &mut w);
    assert!(s.human_spawn.ended); assert!(s.human_spawn.pending.is_none());
    assert!(!s.unit.get(w.unit).unwrap().alive);
}

#[test]
fn journal_preserves_notice_order_and_values_after_canonical_state_changes() {
    use bfe_sim::gen::{HsEvent, HsEventKind, EVENT_F4_HUMAN};
    let (mut s, mut h, mut w) = setup(false);
    h.step(&mut s, 0.0, &[Command::Team(2.0), Command::Spawn(HumanPick { class_name: StrId(2), post: None })], &mut w);
    let mut journal = bfe_sim::store::calls::Events::default(); journal.reserve(256, HsEvent::WORDS, 0);
    h.journal(&mut journal); assert!(h.events().is_empty());
    s.human_spawn.class_name = StrId(99); s.character.get_mut(w.character).unwrap().team = 1.0;
    let words = journal.snapshot(); let stride = 2 + HsEvent::WORDS;
    assert_eq!(words[1], EVENT_F4_HUMAN as f64);
    let team = HsEvent::take(&words[3..3 + HsEvent::WORDS], &[]);
    let spawned = HsEvent::take(&words[3 + stride..3 + stride + HsEvent::WORDS], &[]);
    assert_eq!(team.kind, HsEventKind::TeamChanged); assert_eq!(team.team, 2.0);
    assert_eq!(spawned.kind, HsEventKind::Spawned); assert_eq!(spawned.team, 2.0);
    assert_eq!(spawned.class_name, Some(StrId(2)));
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn native_spawn_death_and_journal_cycles_reuse_all_phase_storage() {
    let (mut s, mut h, mut w) = setup(false);
    let mut journal = bfe_sim::store::calls::Events::default(); journal.reserve(1024, bfe_sim::gen::HsEvent::WORDS, 0);
    let spawn = [Command::Spawn(HumanPick { class_name: StrId(2), post: None })];
    let suicide = [Command::Suicide];
    allocation::start();
    for _ in 0..1200 {
        h.step(&mut s, 4.0, &spawn, &mut w); h.journal(&mut journal);
        h.step(&mut s, 0.0, &suicide, &mut w); h.journal(&mut journal); journal.clear();
    }
    let requests = allocation::stop(); assert_eq!(requests, 0);
    assert_eq!(s.human_spawn.phase, HumanPhase::Dying); assert!(h.events().is_empty());
}

#[test]
fn a_long_pause_retains_all_notices_and_clamps_an_oversized_command_batch() {
    let (mut s, mut h, mut w) = setup(false);
    for _ in 0..16384 { s.human_spawn.phase=HumanPhase::Unspawned;h.spawned(&mut s, StrId(1), None, &mut w); }
    s.unit.get_mut(w.unit).unwrap().alive=true;
    assert_eq!(h.events().len(), 16384);
    let mut commands = vec![Command::Cancel; 16384]; commands[4] = Command::Team(2.0);
    h.step(&mut s, 0.0, &commands, &mut w);
    assert_eq!(s.character.get(w.character).unwrap().team, 1.0);
    assert_eq!(h.events().len(), 16384);
    let mut journal = bfe_sim::store::calls::Events::default(); h.journal(&mut journal);
    assert_eq!(journal.len(), 16384 * (2 + bfe_sim::gen::HsEvent::WORDS));
    assert!(h.events().is_empty());
}
