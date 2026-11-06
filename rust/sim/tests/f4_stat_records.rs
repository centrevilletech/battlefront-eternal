//! First-use stats and roster reuse retain prepared native record storage.
use bfe_sim::battle::{rules::{self, RulesWorld}, stat_records::Records, publication::{Publication, Source}, stats::{self, Seen}};
use bfe_sim::gen::*;
use bfe_sim::store::{Nullable, Ref, StrId, Strings};
struct World { records: Records, seen: Seen }
impl RulesWorld for World {
    fn prepare_record(&mut self, s: &mut Store, c: Ref<Character>) { self.records.ensure(s, c); }
    fn relation(&mut self, a: f64, b: f64) -> f64 { if a == b { 1.0 } else { -1.0 } }
    fn unit_alive(&mut self, s: &Store, u: Ref<Unit>) -> bool { s.unit.get(u).unwrap().alive }
    fn observe(&mut self, _: &Store, _: Ref<Character>, _: Option<Ref<Unit>>) -> Option<Seen> { Some(self.seen.clone()) }
    fn living(&mut self, _: &Store, _: f64) -> Option<f64> { None }
    fn owned_values(&mut self, _: &mut Store, _: f64) -> Vec<Option<f64>> { panic!("no posts"); }
    fn battle_over(&mut self, _: &mut Store, _: f64) { panic!("no game over"); }
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn first_stat_keys_and_repeated_roster_resets_allocate_nothing_in_the_rule_phase() {
    let mut s = Store::default(); s.battle.stats = rules::new_stats();
    let c = s.character.insert(Character { team: 1.0, class_name: Some(StrId(0)), ..Character::default() }); s.battle.characters.push(c);
    let victims: Vec<_> = (0..8).map(|_| s.character.insert(Character::default())).collect();
    let mut strings = Strings::default(); let names: Vec<_> = (0..12).map(|i| strings.intern_str(&format!("label{i}"))).collect();
    let mut records = Records::new(9); records.prepare(&mut s, c, 9);
    for &v in &victims { records.prepare(&mut s, v, 9); }
    assert!(matches!(s.character.get(c).unwrap().record, Nullable::Undefined));
    let mut w = World { records, seen: Seen::default() };
    let mut publication = Publication::new(1); let handle = publication.register(Source::Character(c), 1.0, 1024);
    publication.flush(&s);
    allocation::start();
    for cycle in 0..120 {
        w.records.reset(&mut s, c);
        for &v in &victims { w.records.reset(&mut s, v); }
        for (i, &name) in names.iter().enumerate() {
            s.character.get_mut(c).unwrap().class_name = Some(name);
            w.seen = Seen { class_label: Some(name), weapon: Some((Some(name), Some(name))),
                vehicle: Some((Some(name), Some(name))), position: Some([cycle as f64, 0.0, 0.0]), health: Some(1.0) };
            rules::update(&mut s, 1.0, &strings, |_, _| true, &mut w);
            let v = victims[i % victims.len()]; w.records.ensure(&mut s, v);
            rules::credit(&mut s, Some(v), Some(c), -1.0);
            publication.mark(&handle); assert_eq!(publication.flush(&s), 1);
        }
    }
    assert_eq!(allocation::stop(), 0);
    let Nullable::Value(record) = &s.character.get(c).unwrap().record else { panic!("missing active record"); };
    assert_eq!(record.slots[0].classes.len(), 5); assert_eq!(record.slots[0].weapons.len(), 10); assert_eq!(record.slots[0].vehicles.len(), 5);
    assert_eq!(record.slots[0].victims.size(), 8);
    w.records.reset(&mut s, c); assert!(matches!(s.character.get(c).unwrap().record, Nullable::Null));
    w.records.ensure(&mut s, c);
    let Nullable::Value(record) = &s.character.get(c).unwrap().record else { unreachable!() };
    assert_eq!(record.health, 255.0); assert!(record.slots[0].classes.is_empty()); assert_eq!(record.slots[0].victims.size(), 0);
    assert_eq!(record.slots[0].life_kills, 0.0);
}
#[test]
fn replacing_a_slab_character_requires_new_prepared_record_storage() {
    let mut s = Store::default(); let old = s.character.insert(Character::default());
    let mut records = Records::new(1); records.prepare(&mut s, old, 1); records.ensure(&mut s, old);
    s.character.remove(old); let next = s.character.insert(Character::default());
    assert_eq!(old.slot, next.slot); assert_ne!(old.gen, next.gen);
    records.prepare(&mut s, next, 1); records.ensure(&mut s, next);
    let Nullable::Value(record) = &s.character.get(next).unwrap().record else { unreachable!() };
    assert_eq!(record.health, 255.0); assert!(record.slots[0].weapons.is_empty());
    // The ordinary Rust record constructor remains the capture host's path.
    assert_eq!(stats::new_record().health, record.health);
}
