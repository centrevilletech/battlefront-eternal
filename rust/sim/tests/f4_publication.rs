//! The battle Store remains the sole owner while its generated row/heap
//! encoding is retained for changed-word publications.
use bfe_sim::battle::{publication::{Publication, Source}, rules};
use bfe_sim::gen::{Character, F4Delta, Store, TimeEntry};
use bfe_sim::store::{Nullable, StrId};
use bfe_sim::store::block::HEADER;
fn delta(p: &Publication, i: usize) -> F4Delta { let (row, heap) = p.block.read(i); F4Delta::take(row, heap) }
fn apply(p: &Publication, d: &F4Delta, into: &mut Vec<f64>) {
    let source = p.encoded(&d.target).expect("retained qualified cache");
    assert_eq!(d.source, 0); assert_eq!(d.borrowed_words, 0);
    into.resize(d.length as usize, 0.0); let mut values = d.values.iter();
    for (i, &mask) in d.mask.iter().enumerate() {
        let mut bits = mask;
        while bits != 0 { let bit = bits.trailing_zeros() as usize;
            let value = *values.next().expect("changed native value");
            assert_eq!(value.to_bits(), source[i * 32 + bit].to_bits());
            into[i * 32 + bit] = value; bits &= bits - 1;
        }
    }
    assert!(values.next().is_none());
}
fn character(image: &[f64]) -> Character {
    let mut out = Character::default(); out.take_export(&image[HEADER..], &image[image[2] as usize..]); out
}
#[test]
fn variable_stat_records_publish_growth_shrink_and_only_changed_words() {
    let mut s = Store::default(); let c = s.character.insert(Character::default());
    let mut p = Publication::new(1); let h = p.register(Source::Character(c), 1.0, 1024);
    assert_eq!(p.flush(&s), 1); let mut image = vec![]; apply(&p, &delta(&p, 0), &mut image);
    assert!(matches!(character(&image).record, Nullable::Undefined));
    let mut record = rules::new_stats(); record.time = 1.0; s.battle.stats = record;
    let mut record = bfe_sim::battle::stats::new_record();
    record.slots[0].classes.push(TimeEntry { key: StrId(1), label: None, time: 2.5 });
    s.character.get_mut(c).unwrap().record = Nullable::Value(record);
    p.mark(&h); assert_eq!(p.flush(&s), 1); apply(&p, &delta(&p, 0), &mut image);
    let Nullable::Value(record) = character(&image).record else { panic!("record missing"); };
    assert_eq!(record.slots[0].classes[0].time, 2.5);
    s.character.get_mut(c).unwrap().captures = 3.0; p.mark(&h); assert_eq!(p.flush(&s), 1);
    assert_eq!(delta(&p, 0).values.len(), 1); apply(&p, &delta(&p, 0), &mut image);
    assert_eq!(character(&image).captures, 3.0);
    s.character.get_mut(c).unwrap().record = Nullable::Null; p.mark(&h); assert_eq!(p.flush(&s), 1);
    apply(&p, &delta(&p, 0), &mut image); assert!(matches!(character(&image).record, Nullable::Null));
    p.mark(&h); assert_eq!(p.flush(&s), 0);
}
#[test]
fn commands_and_nested_flushes_do_not_publish_other_characters() {
    let mut s = Store::default(); let a = s.character.insert(Character::default()); let b = s.character.insert(Character::default());
    let mut p = Publication::new(2); let a = p.register(Source::Character(a), 1.0, 1024); let b = p.register(Source::Character(b), 1.0, 1024);
    p.flush(&s); s.character.get_mut(bfe_sim::store::Ref::new(a.slot, a.generation)).unwrap().captures = 1.0;
    p.mark(&a); assert_eq!(p.flush(&s), 1); assert_eq!(delta(&p, 0).target, a);
    s.character.get_mut(bfe_sim::store::Ref::new(b.slot, b.generation)).unwrap().captures = 2.0;
    p.mark(&b); assert_eq!(p.flush(&s), 1); assert_eq!(delta(&p, 0).target, b);
    assert_eq!(p.flush(&s), 0);
}
#[test]
fn retired_store_rows_survive_slot_reuse_and_old_lives_cannot_mark_replacements() {
    let mut s = Store::default(); let old = s.character.insert(Character::default());
    let mut p = Publication::new(2); let a = p.register(Source::Character(old), 1.0, 1024); p.flush(&s);
    s.character.get_mut(old).unwrap().captures = 7.0; assert!(p.retire(&s, &a)); s.character.remove(old);
    let next = s.character.insert(Character::default()); let b = p.register(Source::Character(next), 1.0, 1024);
    assert_eq!(a.slot, b.slot); assert_ne!(a.generation, b.generation); assert!(!p.mark(&a));
    assert_eq!(p.flush(&s), 2); let retired = delta(&p, 0); assert!(retired.retired); assert_eq!(retired.target, a);
    assert_eq!(retired.values, [7.0]); assert_eq!(delta(&p, 1).target, b);
    assert_eq!(character(p.encoded(&a).unwrap()).captures, 7.0);
    assert_eq!(character(p.encoded(&b).unwrap()).captures, 0.0);
    p.flush(&s); assert!(p.encoded(&a).is_none());
}
#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn changing_character_statistics_reuses_encoding_masks_and_publication_storage() {
    let mut s = Store::default(); let mut record = bfe_sim::battle::stats::new_record();
    record.slots[0].classes.push(TimeEntry { key: StrId(1), label: None, time: 0.0 });
    let c = s.character.insert(Character { record: Nullable::Value(record), ..Character::default() });
    let mut p = Publication::new(1); let h = p.register(Source::Character(c), 1.0, 1024); p.flush(&s);
    allocation::start();
    for tick in 1..=1200 {
        let c = s.character.get_mut(c).unwrap(); c.captures = tick as f64;
        let Nullable::Value(record) = &mut c.record else { unreachable!(); };
        record.slots[0].classes[0].time = tick as f64 / 60.0;
        p.mark(&h); assert_eq!(p.flush(&s), 1);
    }
    let requests = allocation::stop(); assert_eq!(requests, 0);
    assert_eq!(delta(&p, 0).values.len(), 2);
}

#[test]
fn variable_statistics_can_exceed_the_cold_hint_without_losing_their_identity() {
    let mut s=Store::default();let c=s.character.insert(Character::default());
    let mut p=Publication::new(0);let h=p.register(Source::Character(c),1.0,0);p.flush(&s);
    let mut image=vec![];apply(&p, &delta(&p,0),&mut image);
    let mut record=bfe_sim::battle::stats::new_record();
    record.slots[0].classes=(0..4096).map(|i|TimeEntry{key:StrId(i),label:None,time:i as f64}).collect();
    s.character.get_mut(c).unwrap().record=Nullable::Value(record);p.mark(&h);
    assert_eq!(p.flush(&s),1);let d=delta(&p,0);assert_eq!(d.target,h);
    // The baseline words remain paired with this exact Character identity.
    apply(&p, &d,&mut image);
    let Nullable::Value(record)=character(&image).record else{panic!("missing record")};
    assert_eq!(record.slots[0].classes.len(),4096);assert_eq!(record.slots[0].classes[4095].time,4095.0);
    p.mark(&h);assert_eq!(p.flush(&s),0);
}

#[test]
fn retirements_keep_each_generation_through_a_capacity_burst() {
    let mut s = Store::default(); let mut r = s.character.insert(Character::default());
    let mut p = Publication::new(0); let mut h = p.register(Source::Character(r), 1.0, 0);
    p.flush(&s); let mut retired = Vec::new();
    // More generations than a battle can admit at once. The hint is not a
    // rejection limit and a replacement cannot overwrite an unread old row.
    for i in 0..1024 {
        s.character.get_mut(r).unwrap().captures = i as f64;
        assert!(p.retire(&s, &h)); retired.push(h); s.character.remove(r);
        r = s.character.insert(Character::default()); h = p.register(Source::Character(r), 1.0, 0);
    }
    assert_eq!(p.flush(&s), 1025);
    for (i, h) in retired.iter().enumerate() {
        assert_eq!(character(p.encoded(h).unwrap()).captures, i as f64);
    }
    assert_eq!(character(p.encoded(&h).unwrap()).captures, 0.0);
    assert_eq!(p.flush(&s), 0);
    for h in retired { assert!(p.encoded(&h).is_none()); }
}
