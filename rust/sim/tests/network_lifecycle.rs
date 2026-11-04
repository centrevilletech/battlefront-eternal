use bfe_sim::gen::{NetworkLifecycleCall as Call, NetworkLifecycleReply as Reply};
use bfe_sim::world::network_lifecycle::{self as lifecycle, Host};
use std::panic::{catch_unwind, AssertUnwindSafe};

struct World {
    calls: Vec<(Call, f64)>,
    battle_over: bool,
    roster: bool,
    current: bool,
    alive: bool,
    dead_time: f64,
    post: bool,
    prepared: bool,
    previous: bool,
    same_class: bool,
    acquired_previous: bool,
    previous_alive: bool,
    team: bool,
    generations: [f64; 2],
    arsenal: usize,
    visited: usize,
    grow: bool,
    fail: Option<Call>,
    player: bool,
    vehicle: bool,
    turret: bool,
    stop_mounts_vehicle: bool,
    nested: bool,
    nested_calls: Vec<(Call, f64)>,
}
impl Default for World {
    fn default() -> Self {
        Self { calls: vec![], battle_over: false, roster: true, current: true, alive: false,
            dead_time: 4.0, post: true, prepared: true, previous: true, same_class: false,
            acquired_previous: false, previous_alive: false, team: true, generations: [2.0, 5.0],
            arsenal: 2, visited: 0, grow: false, fail: None, player: true, vehicle: false,
            turret: false, stop_mounts_vehicle: false, nested: false, nested_calls: vec![] }
    }
}
impl Host for World {
    fn call(&mut self, op: Call, value: f64) -> Reply {
        self.calls.push((op, value));
        assert_ne!(self.fail, Some(op), "injected callback failure");
        let mut reply = Reply::default();
        match op {
            Call::BattleOver => reply.flag = self.battle_over,
            Call::Roster => reply.flag = self.roster,
            Call::Current => reply.flag = self.current,
            Call::Alive => reply.flag = self.alive,
            Call::DeadTime => reply.value = self.dead_time,
            Call::Post => reply.flag = self.post,
            Call::Prepared => reply.flag = self.prepared,
            Call::Previous => { reply.flag = self.previous; reply.other = self.same_class; }
            Call::Acquire => reply.flag = self.acquired_previous,
            Call::PreviousAlive => reply.flag = self.previous_alive,
            Call::TeamExists => reply.flag = self.team,
            Call::Generations => {
                reply.value = self.generations[0]; reply.second = self.generations[1];
                if self.nested {
                    let mut inner = World { same_class: true, arsenal: 4, ..World::default() };
                    assert!(lifecycle::spawn(&mut inner, 4.0, false, true));
                    lifecycle::unspawn(&mut inner, 7.5);
                    self.nested_calls = inner.calls;
                    self.generations = [100.0, 200.0];
                }
            }
            Call::NextWeapon => reply.flag = self.visited < self.arsenal,
            Call::Weapon => {
                self.visited += 1;
                if self.grow && self.visited == 1 { self.arsenal += 2; }
            }
            Call::Player => reply.flag = self.player,
            Call::Stop if self.stop_mounts_vehicle => self.vehicle = true,
            Call::Vehicle => reply.flag = self.vehicle,
            Call::Turret => reply.flag = self.turret,
            _ => {}
        }
        reply
    }
}
impl World {
    fn has(&self, op: Call) -> bool { self.calls.iter().any(|(v, _)| *v == op) }
    fn index(&self, op: Call) -> usize { self.calls.iter().position(|(v, _)| *v == op).unwrap() }
    fn count(&self, op: Call) -> usize { self.calls.iter().filter(|(v, _)| *v == op).count() }
    fn value(&self, op: Call) -> f64 { self.calls.iter().find(|(v, _)| *v == op).unwrap().1 }
}

#[test]
fn eligibility_short_circuits_before_unreached_reads_and_writes() {
    let mut ended = World { battle_over: true, ..World::default() };
    assert!(!lifecycle::spawn(&mut ended, 4.0, true, true));
    assert_eq!(ended.calls, [(Call::BattleOver, 0.0)]);
    let mut missing = World { roster: false, ..World::default() };
    assert!(!lifecycle::spawn(&mut missing, 4.0, true, true));
    assert!(!missing.has(Call::Current));
    let mut delay = World { dead_time: 3.999, ..World::default() };
    assert!(!lifecycle::spawn(&mut delay, 4.0, true, true));
    assert!(!delay.has(Call::Post));
    for (post, prepared, last) in [(false, true, Call::Post), (true, false, Call::Prepared)] {
        let mut world = World { post, prepared, ..World::default() };
        assert!(!lifecycle::spawn(&mut world, 4.0, true, true));
        assert_eq!(world.calls.last().unwrap().0, last);
        assert!(!world.has(Call::Previous));
    }
    for (current, alive) in [(false, false), (true, true)] {
        let mut world = World { current, alive, dead_time: 0.0, ..World::default() };
        assert!(lifecycle::spawn(&mut world, 4.0, false, false));
        assert!(!world.has(Call::DeadTime));
    }
    for dead_time in [4.0, f64::NAN, f64::INFINITY] {
        let mut world = World { dead_time, ..World::default() };
        assert!(lifecycle::spawn(&mut world, 4.0, false, false));
    }
}

#[test]
fn selection_retires_only_a_distinct_preceding_body_before_publication() {
    for (previous, same_class, acquired_previous, retires) in [
        (true, true, false, false), (true, false, true, false),
        (true, false, false, true), (false, false, false, false),
    ] {
        let mut world = World { previous, same_class, acquired_previous, ..World::default() };
        assert!(lifecycle::spawn(&mut world, 4.0, true, false));
        assert_eq!(world.has(Call::Reuse), same_class);
        assert_eq!(world.has(Call::Acquire), !same_class);
        for op in [Call::RetireFlags, Call::RetireHandler, Call::Retire, Call::RemoteClear, Call::Generations] {
            assert_eq!(world.has(op), retires, "{op:?}");
        }
        if retires {
            assert!(world.index(Call::RetireFlags) < world.index(Call::RetireHandler));
            assert!(world.index(Call::RetireHandler) < world.index(Call::Retire));
            assert!(world.index(Call::Retire) < world.index(Call::UnitFlags));
            assert!(world.index(Call::Publish) < world.index(Call::Generation));
            assert!(world.index(Call::Generation) < world.index(Call::Spawn));
        }
        assert!(world.index(Call::CharacterUnit) < world.index(Call::PreviousAlive));
        assert!(world.index(Call::RemoteBind) < world.index(Call::Publish));
        assert!(world.index(Call::Spawn) < world.index(Call::SpawnStatus));
        assert_eq!(world.calls.last().unwrap().0, Call::Emit);
    }
}

#[test]
fn generation_max_preserves_signed_zero_infinities_and_nan() {
    let cases = [
        ([2.0, 5.0], 5.0), ([9.0, 4.0], 9.0),
        ([-0.0, 0.0], 0.0), ([0.0, -0.0], 0.0), ([-0.0, -0.0], -0.0),
        ([f64::NEG_INFINITY, f64::INFINITY], f64::INFINITY),
        ([f64::NAN, 4.0], f64::NAN), ([3.0, f64::NAN], f64::NAN),
    ];
    for (generations, expected) in cases {
        let mut world = World { generations, ..World::default() };
        assert!(lifecycle::spawn(&mut world, 4.0, false, false));
        let actual = world.value(Call::Generation);
        if expected.is_nan() { assert!(actual.is_nan()); }
        else { assert_eq!(actual.to_bits(), expected.to_bits()); }
    }
}

#[test]
fn arsenal_growth_is_visited_and_a_failed_body_does_not_publish_status() {
    let mut world = World { grow: true, ..World::default() };
    assert!(lifecycle::spawn(&mut world, 4.0, false, true));
    assert_eq!(world.visited, 4);
    assert_eq!(world.count(Call::NextWeapon), 5);
    assert!(world.index(Call::Weapons) > world.index(Call::Spawn));
    for failed in [Call::Weapons, Call::NextWeapon, Call::Weapon] {
        let mut world = World { fail: Some(failed), ..World::default() };
        assert!(catch_unwind(AssertUnwindSafe(|| lifecycle::spawn(&mut world, 4.0, true, true))).is_err());
        assert!(world.has(Call::Publish));
        assert!(world.has(Call::Spawn));
        assert!(!world.has(Call::SpawnStatus));
        assert!(!world.has(Call::Emit));
    }
}

#[test]
fn living_count_is_checked_after_binding_and_unspawn_keeps_the_delay_value() {
    for (previous_alive, team, adds) in [(true, true, false), (false, false, false), (false, true, true)] {
        let mut world = World { previous_alive, team, ..World::default() };
        assert!(lifecycle::spawn(&mut world, 4.0, false, false));
        assert_eq!(world.has(Call::LivingAdd), adds);
        assert_eq!(world.has(Call::TeamExists), !previous_alive);
    }
    for team in [false, true] {
        let mut world = World { team, ..World::default() };
        lifecycle::unspawn(&mut world, 3.25);
        assert_eq!(world.calls.first().unwrap().0, Call::UnspawnUnit);
        assert!(world.index(Call::UnspawnFlags) < world.index(Call::Invisible));
        assert!(world.index(Call::Invisible) < world.index(Call::UnspawnTime));
        assert_eq!(world.value(Call::UnspawnTime), 3.25);
        assert_eq!(world.has(Call::LivingRemove), team);
        assert_eq!(world.calls.last().unwrap().0, Call::UnspawnStatus);
    }
}

#[test]
fn disconnect_reads_the_body_after_stop_and_exit_and_vehicle_wins() {
    let mut absent = World { player: false, ..World::default() };
    lifecycle::remove(&mut absent);
    assert_eq!(absent.calls, [(Call::Player, 0.0)]);
    let mut mounted = World { stop_mounts_vehicle: true, turret: true, ..World::default() };
    lifecycle::remove(&mut mounted);
    assert!(mounted.index(Call::Disconnect) < mounted.index(Call::Stop));
    assert!(mounted.index(Call::Stop) < mounted.index(Call::RemoteExit));
    assert!(mounted.index(Call::RemoteExit) < mounted.index(Call::RemoveUnit));
    assert!(mounted.has(Call::Unboard));
    assert!(!mounted.has(Call::Turret));
    assert!(!mounted.has(Call::Leave));
    for turret in [false, true] {
        let mut world = World { turret, ..World::default() };
        lifecycle::remove(&mut world);
        assert!(world.has(Call::Turret));
        assert_eq!(world.has(Call::Leave), turret);
        assert!(!world.has(Call::Unboard));
    }
}

#[test]
fn nested_callbacks_keep_the_outstanding_owned_reply_and_outer_sequence() {
    let mut world = World { nested: true, ..World::default() };
    assert!(lifecycle::spawn(&mut world, 4.0, true, true));
    assert_eq!(world.value(Call::Generation), 5.0);
    assert_eq!(world.generations, [100.0, 200.0]);
    assert_eq!(world.visited, 2);
    assert_eq!(world.count(Call::BattleOver), 1);
    assert_eq!(world.nested_calls.iter().filter(|(op, _)| *op == Call::Weapon).count(), 4);
    assert_eq!(world.nested_calls.last().unwrap().0, Call::UnspawnStatus);
    assert_eq!(world.calls.last().unwrap().0, Call::Emit);
}
