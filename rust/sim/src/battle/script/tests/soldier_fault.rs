//! A soldier whose movement state a step refuses is that soldier's fault, not the battle's.
use super::*;
use crate::gen::UnitBody;

thread_local! {
    /// Set by a test that corrupts a soldier on purpose, so `flat_step` allows the refusals.
    pub(super) static EXPECTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[test]
#[ignore = "requires a prepared 2005 mission"]
fn a_refused_soldier_is_counted_and_the_battle_goes_on() {
    let (mut world, admitted, _) = prepared_world(hero_assault(), 31, 8);
    let mut clock = WorldClockState { hz: 60., max_steps: 4., max_debt: 8., budget: f64::INFINITY, ..Default::default() };
    for tick in 1..=300 {
        flat_step(&mut world, admitted.units, &mut clock, tick);
    }
    assert_eq!(world.soldier_movement_faults(), 0, "a healthy start");
    // A soldier whose lock is not a number cannot be stepped.
    let victim = world
        .sim
        .store
        .unit
        .iter()
        .find(|(_, u)| u.alive && !u.dead && u.seat.is_none() && matches!(u.soldier, UnitBody::Soldier(_)))
        .map(|(r, _)| r)
        .expect("a living soldier");
    if let Some(UnitBody::Soldier(s)) = world.sim.store.unit.get_mut(victim).map(|u| &mut u.soldier) {
        s.lock = f64::INFINITY;
    }
    EXPECTED.with(|e| e.set(true));
    for tick in 301..=600 {
        flat_step(&mut world, admitted.units, &mut clock, tick);
    }
    EXPECTED.with(|e| e.set(false));
    assert!(world.soldier_movement_faults() > 0, "the refusal is counted");
}
