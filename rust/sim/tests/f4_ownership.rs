//! Owned phase rows reject stale commands and publish only changed words.
use bfe_sim::battle::ownership::{Publish, Rows};
use bfe_sim::gen::{F4BuildingView, F4Delta, F4ObjectKind};
#[derive(Default)]
struct Building { view: F4BuildingView }
impl Publish for Building {
    const WORDS: usize = F4BuildingView::WORDS;
    fn write(&self, row: &mut [f64], heap: &mut Vec<f64>) { self.view.put(row, heap); }
}
fn rows() -> Rows<Building> { Rows::new(F4ObjectKind::Building, 4) }
fn delta(rows: &Rows<Building>, index: usize) -> F4Delta {
    let (row, heap) = rows.publication.read(index); F4Delta::take(row, heap)
}
#[test]
fn commands_publish_only_the_changed_building() {
    let mut rows = rows(); let a = rows.adopt(1.0, Building::default()); let b = rows.adopt(1.0, Building::default());
    assert_eq!(rows.flush(), 2);
    assert!(rows.command(&a, |b| b.view.state.building.health = 25.0));
    assert_eq!(rows.flush(), 1); assert_eq!(delta(&rows, 0).target, a);
    assert_eq!(delta(&rows, 0).values, [25.0]);
    assert!(rows.command(&b, |_| {})); assert_eq!(rows.flush(), 0);
}
#[test]
fn rebuild_rejects_old_life_even_when_the_view_does_not_change() {
    let mut rows = rows(); let a = rows.adopt(1.0, Building::default()); rows.flush();
    let b = rows.rebirth(&a, 2.0, |_| {}).unwrap();
    assert!(!rows.command(&a, |_| panic!("stale life mutated replacement")));
    assert_eq!(rows.flush(), 1); assert_eq!(delta(&rows, 0).target, b);
    assert_eq!(rows.flush(), 0);
}
#[test]
fn retire_then_slot_reuse_preserves_old_final_state_and_creation_order() {
    let mut rows = rows(); let a = rows.adopt(1.0, Building::default()); let b = rows.adopt(1.0, Building::default()); rows.flush();
    rows.command(&a, |b| b.view.state.building.health = 7.0);
    assert!(rows.retire(&a)); let replacement = rows.adopt(1.0, Building::default());
    assert_eq!(replacement.slot, a.slot); assert_ne!(replacement.generation, a.generation);
    assert!(!rows.command(&a, |_| panic!("stale slot")));
    assert_eq!(rows.handle(0), b); assert_eq!(rows.handle(1), replacement);
    assert_eq!(rows.flush(), 2);
    let old = delta(&rows, 0); assert!(old.retired); assert_eq!(old.target, a); assert_eq!(old.values, [7.0]);
    assert_eq!(delta(&rows, 1).target, replacement);
}
#[test]
fn nested_command_flushes_do_not_republish_unrelated_rows() {
    let mut rows = rows(); let a = rows.adopt(1.0, Building::default()); let b = rows.adopt(1.0, Building::default()); rows.flush();
    rows.command(&a, |b| b.view.state.pulse = 0.25); assert_eq!(rows.flush(), 1);
    rows.command(&b, |b| b.view.state.pulse = 0.5); assert_eq!(rows.flush(), 1);
    assert_eq!(delta(&rows, 0).target, b); assert_eq!(rows.flush(), 0);
}
#[test]
fn publication_preserves_signed_zero_and_nan_bits() {
    let mut rows = rows(); let a = rows.adopt(1.0, Building::default()); rows.flush();
    rows.command(&a, |b| b.view.state.pulse = -0.0); assert_eq!(rows.flush(), 1);
    assert_eq!(delta(&rows, 0).values[0].to_bits(), (-0.0f64).to_bits());
    let payload = f64::from_bits(0x7ff8_0000_0000_0001);
    rows.command(&a, |b| b.view.state.pulse = payload); assert_eq!(rows.flush(), 1);
    assert_eq!(delta(&rows, 0).values[0].to_bits(), payload.to_bits());
    rows.command(&a, |b| b.view.state.pulse = payload); assert_eq!(rows.flush(), 0);
}
#[test]
fn a_handle_for_another_object_kind_is_rejected() {
    let mut rows = rows(); let mut a = rows.adopt(1.0, Building::default()); a.kind = F4ObjectKind::Trap;
    assert!(rows.get(&a).is_none()); assert!(!rows.retire(&a));
}

#[path = "support/allocations.rs"]
mod allocation;
#[global_allocator]
static ALLOCATOR: allocation::Counted = allocation::Counted;
#[test]
fn cold_adoption_reserves_the_first_birth_publication() {
    let mut rows = Rows::new(F4ObjectKind::Building, 0);
    for _ in 0..12 { rows.adopt(1.0, Building::default()); }
    allocation::start(); let births = rows.flush(); let requests = allocation::stop();
    assert_eq!(births, 12); assert_eq!(requests, 0);
}

#[test]
fn checked_out_working_rows_publish_before_nested_commands_without_aliasing() {
    let mut rows = rows(); let a = rows.adopt(1.0, Building::default()); let b = rows.adopt(1.0, Building::default()); rows.flush();
    let mut working = rows.checkout(&a).unwrap(); working.view.state.pulse = 0.25;
    assert!(rows.stage(&a, &working));
    assert!(!rows.command(&a, |_| panic!("working row alias")));
    assert!(rows.command(&b, |v| v.view.state.pulse = 0.5)); assert_eq!(rows.flush(), 2);
    assert_eq!(delta(&rows, 0).target, a); assert_eq!(delta(&rows, 0).values, [0.25]);
    assert_eq!(delta(&rows, 1).target, b);
    working.view.state.pulse = 0.75; assert!(rows.restore(&a, working).is_ok());
    assert_eq!(rows.flush(), 1); assert_eq!(delta(&rows, 0).values, [0.75]);
}
#[test]
fn a_retired_working_rig_cannot_overwrite_a_reused_slot() {
    let mut rows = rows(); let a = rows.adopt(1.0, Building::default()); rows.flush();
    let mut working = rows.checkout(&a).unwrap(); working.view.state.pulse = 0.5; rows.stage(&a, &working);
    assert!(rows.retire(&a)); let b = rows.adopt(1.0, Building::default());
    assert!(rows.restore(&a, working).is_err()); assert_eq!(rows.flush(), 2);
    assert!(delta(&rows, 0).retired); assert_eq!(delta(&rows, 0).target, a); assert_eq!(delta(&rows, 0).values, [0.5]);
    assert_eq!(delta(&rows, 1).target, b); assert_eq!(rows.get(&b).unwrap().view.state.pulse, 0.0);
}
#[test]
fn working_frame_publications_and_restoration_reuse_all_storage() {
    let mut rows = rows(); let a = rows.adopt(1.0, Building::default()); rows.flush();
    allocation::start();
    for i in 0..1200 {
        let mut value = rows.checkout(&a).unwrap(); value.view.state.pulse = i as f64;
        assert!(rows.stage(&a, &value)); rows.flush(); assert!(rows.restore(&a, value).is_ok()); rows.flush();
    }
    assert_eq!(allocation::stop(), 0);
}

#[test]
fn a_large_load_and_rebirth_burst_keep_each_generation_paired_with_its_row() {
    let mut rows = Rows::new(F4ObjectKind::Building, 1);
    let handles: Vec<_> = (0..4096).map(|i| {
        let mut b=Building::default();b.view.state.pulse=i as f64;rows.adopt(1.0,b)
    }).collect();
    assert_eq!(rows.flush(),4096);
    for h in &handles {assert!(rows.retire(h));}
    let next: Vec<_> = (0..4096).map(|_|rows.adopt(1.0,Building::default())).collect();
    allocation::start();let count=rows.flush();let requests=allocation::stop();
    assert_eq!(requests,0);assert_eq!(count,8192);
    for (i,h) in handles.iter().enumerate(){assert_eq!(delta(&rows,i).target,*h);assert!(delta(&rows,i).retired);assert!(!rows.command(h, |_|panic!("old life")));}
    for (i,h) in next.iter().enumerate(){assert_eq!(delta(&rows,4096+i).target,*h);assert!(!delta(&rows,4096+i).retired);}
}
